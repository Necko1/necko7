use std::env;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tracing::warn;
use crate::AppResult;
use crate::db::app_settings::{KEY_APP_TOKEN, KEY_BOT_AUTH};
use crate::db::Db;
use crate::db::error::DbResult;
use crate::helix::error::HelixError;
use crate::helix::{api, HelixClient};
use crate::steam::market::MarketClient;

#[derive(Serialize, Deserialize)]
pub struct BotInfo {
    pub user_login: String,
    pub user_id: String,
    pub access_token: String,
    pub refresh_token: String,
}

pub struct AppState {
    pub helix_client: HelixClient,
    pub market_client: MarketClient,

    // env
    pub webhook_secret: String,
    pub client_id: String,
    pub client_secret: String,
    pub app_url: String,

    pub bot_info: RwLock<Option<BotInfo>>,

    pub db: Db,

    pub app_initialized: AtomicBool,
}

impl AppState {
    pub async fn from_env(db: Db) -> DbResult<Arc<Self>> {
        let bot_info: Option<BotInfo> = db
            .get_setting(KEY_BOT_AUTH)
            .await?
            .and_then(|json| serde_json::from_str(&json).ok());

        let app_initialized = bot_info.is_some();

        let client_id = env::var("TWITCH_CLIENT_ID")
            .expect("TWITCH_CLIENT_ID not found in the environment");
        let client_secret = env::var("TWITCH_CLIENT_SECRET")
            .expect("TWITCH_CLIENT_SECRET not found in the environment");

        Ok(Arc::new(Self {
            helix_client: HelixClient::new(client_id.clone(), client_secret.clone()),
            market_client: MarketClient::new(),
            webhook_secret: env::var("TWITCH_EVENTSUB_SECRET")
                .expect("TWITCH_EVENTSUB_SECRET not found in the environment"),
            client_id,
            client_secret,
            app_url: env::var("APP_URL")
                .expect("APP_URL not found in the environment"),
            bot_info: RwLock::new(bot_info),
            db,
            app_initialized: AtomicBool::new(app_initialized),
        }))
    }
}

impl AppState {
    pub async fn with_app_token<F, Fut, T>(&self, mut action: F) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let mut token = self.get_or_refresh_app_token().await?;

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                // Если 401 и это первая попытка — обновляем токен и пробуем снова
                Err(HelixError::Unauthorized(_)) if attempt == 0 => {
                    token = self.update_app_access_token().await?;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn with_broadcaster_token<F, Fut, T>(
        &self,
        broadcaster_id: &str,
        mut action: F,
    ) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let broadcaster = match self.db.get_broadcaster_by_id(broadcaster_id).await? {
            Some(b) => b,
            None => {
                return Err(format!("Couldn't find a broadcaster with ID {}", broadcaster_id).into())
            }
        };

        let mut token = broadcaster.user_access_token;

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                Err(HelixError::Unauthorized(_)) if attempt == 0 => {
                    let token_res = self.helix_client
                        .refresh_user_token(&broadcaster.refresh_token).await?;

                    self.db.update_broadcaster_tokens(
                        broadcaster_id,
                        &token_res.access_token, &token_res.refresh_token
                    ).await?;

                    token = token_res.access_token;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn with_bot_user_token<F, Fut, T>(
        &self,
        mut action: F,
    ) -> AppResult<T>
    where
        F: FnMut(String) -> Fut,
        Fut: Future<Output = Result<T, HelixError>>,
    {
        let (mut token, refresh_token) = {
            let guard = self.bot_info.read();
            let info = guard.as_ref().ok_or("The bot is not initialized")?;
            (info.access_token.clone(), info.refresh_token.clone())
        };

        for attempt in 0..2 {
            match action(token.clone()).await {
                Ok(val) => return Ok(val),
                Err(HelixError::Unauthorized(_)) if attempt == 0 => {
                    let token_res = self.helix_client
                        .refresh_user_token(&refresh_token).await?;

                    let user_info = self.helix_client
                        .get_user_info_by_token(&token_res.access_token).await?;

                    let bot_info = BotInfo {
                        user_login: user_info.login,
                        user_id: user_info.id,
                        access_token: token_res.access_token,
                        refresh_token: token_res.refresh_token,
                    };

                    match serde_json::to_string(&bot_info) {
                        Ok(info_str) => {
                            self.db.set_setting(KEY_BOT_AUTH, &info_str).await?;
                        }
                        Err(e) => {
                            warn!(error = %e, "Failed to serialize bot info as string")
                        }
                    }

                    token = bot_info.access_token;
                    continue;
                }
                Err(err) => return Err(err.into()),
            }
        }

        unreachable!()
    }

    pub async fn update_app_access_token(
        &self
    ) -> AppResult<String> {
        let app_token = self.helix_client.request_app_token().await?;

        self.db.set_setting(KEY_APP_TOKEN, &app_token.access_token).await?;

        Ok(app_token.access_token)
    }

    pub async fn get_or_refresh_app_token(&self) -> AppResult<String> {
        if let Some(token) = self.db.get_setting(KEY_APP_TOKEN).await? {
            Ok(token)
        } else {
            self.update_app_access_token().await
        }
    }

    pub async fn create_eventsub_subscription(
        &self,
        broadcaster_user_id: &str,
    ) -> AppResult<()> {
        let callback_url = format!("{}/eventsub", self.app_url);

        let body = api::eventsub::format_subscription(
            &callback_url,
            &self.webhook_secret,
            broadcaster_user_id,
        );

        self.with_app_token(|token| {
            let body = body.clone();
            async move {
                self.helix_client.create_subscription(body, &token).await
            }
        }).await
    }
}
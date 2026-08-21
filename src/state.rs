use std::env;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use crate::AppResult;
use crate::db::app_settings::{KEY_APP_INITIALIZED, KEY_APP_TOKEN};
use crate::db::Db;
use crate::db::error::DbResult;
use crate::helix::error::HelixError;
use crate::helix::{api, HelixClient};

pub struct AppState {
    pub helix_client: HelixClient,
    // env
    pub webhook_secret: String,
    pub client_id: String,
    pub client_secret: String,
    pub app_url: String,

    pub db: Db,

    pub app_initialized: AtomicBool,
}

impl AppState {
    pub async fn from_env(db: Db) -> DbResult<Arc<Self>> {
        let app_initialized = db.get_setting(KEY_APP_INITIALIZED).await?
            .unwrap_or("false".to_string())
            .to_lowercase()
            == "true";

        let client_id = env::var("TWITCH_CLIENT_ID")
            .expect("TWITCH_CLIENT_ID not found in the environment");
        let client_secret = env::var("TWITCH_CLIENT_SECRET")
            .expect("TWITCH_CLIENT_SECRET not found in the environment");

        Ok(Arc::new(Self {
            helix_client: HelixClient::new(client_id.clone(), client_secret.clone()),
            webhook_secret: env::var("TWITCH_EVENTSUB_SECRET")
                .expect("TWITCH_EVENTSUB_SECRET not found in the environment"),
            client_id,
            client_secret,
            app_url: env::var("APP_URL")
                .expect("APP_URL not found in the environment"),
            db,
            app_initialized: AtomicBool::new(app_initialized),
        }))
    }
}

impl AppState {
    pub async fn update_app_access_token(
        &self
    ) -> AppResult<String> {
        let app_token = self.helix_client.request_app_token().await?;

        self.db.set_setting(KEY_APP_TOKEN, &app_token).await?;

        Ok(app_token)
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

        let mut app_token = self.get_or_refresh_app_token().await?;

        for attempt in 0..2 {
            let body = api::eventsub::format_subscription(
                &callback_url,
                &self.webhook_secret,
                broadcaster_user_id,
            );

            let result = self
                .helix_client
                .create_subscription(body, &app_token)
                .await;

            match result {
                Ok(()) => return Ok(()),

                Err(HelixError::Unauthorized(_)) if attempt == 0 => {
                    app_token = self.update_app_access_token().await?;
                    continue;
                }

                Err(HelixError::Unauthorized(msg)) => return Err(HelixError::Unauthorized(msg).into()),
                Err(HelixError::Reqwest(e)) => return Err(e.into()),
                Err(HelixError::Other(e)) => return Err(e.into()),
            }
        }

        Ok(())
    }
}
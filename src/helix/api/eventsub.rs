use crate::helix::error::HelixResult;
use crate::helix::response::parse_helix_error;
use crate::helix::HelixClient;
use serde_json::Value;

impl HelixClient {
    pub async fn create_subscription(
        &self,
        body: Value,
        app_token: &str,
    ) -> HelixResult<()> {
        let res = self
            .http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .header("Authorization", format!("Bearer {}", app_token))
            .header("Client-Id", &self.client_id)
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() || res.status() == reqwest::StatusCode::CONFLICT {
            return Ok(())
        }

        Err(parse_helix_error(res).await)
    }

    pub async fn create_chat_message_websocket_subscription(
        &self,
        broadcaster_user_id: &str,
        bot_user_id: &str,
        session_id: &str,
        bot_user_token: &str,
    ) -> HelixResult<()> {
        let body = serde_json::json!({
            "type": "channel.chat.message",
            "version": "1",
            "condition": {
                "broadcaster_user_id": broadcaster_user_id,
                "user_id": bot_user_id
            },
            "transport": {
                "method": "websocket",
                "session_id": session_id
            }
        });

        let res = self
            .http_client
            .post("https://api.twitch.tv/helix/eventsub/subscriptions")
            .header("Authorization", format!("Bearer {}", bot_user_token))
            .header("Client-Id", &self.client_id)
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() || res.status() == reqwest::StatusCode::CONFLICT {
            return Ok(());
        }

        Err(parse_helix_error(res).await)
    }
}

pub fn format_subscription(
    callback_url: &str,
    webhook_secret: &str,
    broadcaster_user_id: &str,
) -> Value {
    serde_json::json!({
        "type": "channel.channel_points_custom_reward_redemption.add",
        "version": "1",
        "condition": {
            "broadcaster_user_id": broadcaster_user_id
        },
        "transport": {
            "method": "webhook",
            "callback": callback_url,
            "secret": webhook_secret
        }
    })
}
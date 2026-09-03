use crate::helix::error::HelixResult;
use crate::helix::response::ErrorResponse;
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

        let error_res = res.json::<ErrorResponse>().await?;

        Err(error_res.into())
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
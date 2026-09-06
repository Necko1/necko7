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

}

pub fn format_chat_message_subscription(
    callback_url: &str,
    webhook_secret: &str,
    broadcaster_user_id: &str,
    bot_user_id: &str,
) -> Value {
    serde_json::json!({
        "type": "channel.chat.message",
        "version": "1",
        "condition": {
            "broadcaster_user_id": broadcaster_user_id,
            "user_id": bot_user_id
        },
        "transport": {
            "method": "webhook",
            "callback": callback_url,
            "secret": webhook_secret
        }
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_chat_message_subscription() {
        let val = format_chat_message_subscription(
            "https://example.com/api/v1/eventsub",
            "secret123",
            "1337",
            "9001",
        );

        assert_eq!(val["type"], "channel.chat.message");
        assert_eq!(val["version"], "1");
        assert_eq!(val["condition"]["broadcaster_user_id"], "1337");
        assert_eq!(val["condition"]["user_id"], "9001");
        assert_eq!(val["transport"]["method"], "webhook");
        assert_eq!(val["transport"]["callback"], "https://example.com/api/v1/eventsub");
        assert_eq!(val["transport"]["secret"], "secret123");
    }
}
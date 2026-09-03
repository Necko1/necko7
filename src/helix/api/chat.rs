use serde::Deserialize;
use serde_json::json;
use crate::helix::error::{HelixError, HelixResult};
use crate::helix::HelixClient;
use crate::helix::response::{parse_helix_error, ObjectResponse};

#[derive(Deserialize)]
pub struct SentMessageResponse {
    pub message_id: String,
    pub is_sent: bool,
    pub drop_reason: Option<MessageDropReason>,
}

#[derive(Deserialize)]
pub struct MessageDropReason {
    pub code: String,
    pub message: String,
}

impl HelixClient {
    pub async fn send_chat_message(
        &self,
        broadcaster_id: &str,
        sender_id: &str,
        message: &str,
        reply_parent_message_id: Option<&str>,
        pin: Option<bool>,
        user_token: &str,
    ) -> HelixResult<SentMessageResponse> {
        let body = json!({
            "broadcaster_id": broadcaster_id,
            "sender_id": sender_id,
            "message": message,
            "reply_parent_message_id": reply_parent_message_id,
            "pin": pin,
        });

        let res = self
            .http_client
            .post("https://api.twitch.tv/helix/chat/messages")
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .json(&body)
            .send()
            .await?;

        if res.status().is_success() {
            let res_list = res.json::<ObjectResponse<SentMessageResponse>>().await?;

            let sent_msg = res_list.data.into_iter().next()
                .ok_or_else(|| HelixError::Other(
                    "Got empty data list while sending chat message".to_string()
                ))?;

            if !sent_msg.is_sent {
                if let Some(ref drop_reason) = sent_msg.drop_reason {
                    tracing::warn!(
                        drop_code = %drop_reason.code,
                        drop_message = %drop_reason.message,
                        broadcaster_id = %broadcaster_id,
                        "Twitch chat message was dropped by moderation"
                    );
                }
            }

            return Ok(sent_msg);
        }

        Err(parse_helix_error(res).await)
    }
}
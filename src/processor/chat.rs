use std::sync::Arc;
use chrono::{DateTime, Utc};
use tracing::{debug, error};

use crate::db::chat_messages::NewChatMessage;
use crate::processor::model::EventSubChatMessageNotification;
use crate::state::AppState;

/// Process an incoming channel.chat.message EventSub notification from webhook.
/// Counts all messages without filtering out commands or bot messages.
/// Duplicate delivery is handled safely via database unique constraint (ON CONFLICT DO NOTHING).
pub async fn process_chat_message(
    state: Arc<AppState>,
    notification: EventSubChatMessageNotification,
    message_timestamp: &str,
) {
    let event = notification.event;
    let raw_text = event.message.text;

    // Calculate character count (Unicode characters)
    let char_count = raw_text.chars().count() as i32;

    // Parse sent_at timestamp from webhook header or default to now
    let sent_at = DateTime::parse_from_rfc3339(message_timestamp)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_else(|_| Utc::now());

    let new_msg = NewChatMessage {
        message_id: event.message_id,
        broadcaster_id: event.broadcaster_user_id,
        chatter_user_id: event.chatter_user_id,
        chatter_user_login: event.chatter_user_login,
        message_text: raw_text,
        char_count,
        sent_at,
    };

    if let Err(e) = state.db.insert_chat_message(&new_msg).await {
        error!(error = %e, message_id = %new_msg.message_id, "Failed to insert chat message into database");
    } else {
        debug!(
            broadcaster = %new_msg.broadcaster_id,
            user = %new_msg.chatter_user_login,
            chars = char_count,
            message_id = %new_msg.message_id,
            "Recorded chat message from webhook"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_chat_message_notification() {
        let json_data = r#"{
            "subscription": {
                "id": "sub-12345",
                "type": "channel.chat.message"
            },
            "event": {
                "broadcaster_user_id": "1337",
                "broadcaster_user_login": "streamer_login",
                "chatter_user_id": "9001",
                "chatter_user_login": "viewer_login",
                "message_id": "msg-abc-123",
                "message": {
                    "text": "!drop hello world"
                }
            }
        }"#;

        let notification: EventSubChatMessageNotification = serde_json::from_str(json_data).unwrap();
        assert_eq!(notification.subscription.id, "sub-12345");
        assert_eq!(notification.event.broadcaster_user_id, "1337");
        assert_eq!(notification.event.broadcaster_user_login, "streamer_login");
        assert_eq!(notification.event.chatter_user_id, "9001");
        assert_eq!(notification.event.chatter_user_login, "viewer_login");
        assert_eq!(notification.event.message_id, "msg-abc-123");
        assert_eq!(notification.event.message.text, "!drop hello world");
    }
}

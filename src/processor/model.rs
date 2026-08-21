use serde::Deserialize;
use uuid::Uuid;

#[derive(Debug, Deserialize)]
pub struct EventSubNotification {
    pub subscription: EventSubSubscription,
    pub event: RedemptionEvent,
}

#[derive(Debug, Deserialize)]
pub struct EventSubSubscription {
    pub id: String,
    pub r#type: String,
}

#[derive(Debug, Deserialize)]
pub struct RedemptionEvent {
    pub id: Uuid,
    pub broadcaster_user_id: String,
    pub broadcaster_user_login: String,
    pub user_id: String,
    pub user_login: String,
    pub user_name: String,
    pub user_input: String,
    pub status: String,
    pub reward: RedemptionReward,
    pub redeemed_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Deserialize)]
pub struct RedemptionReward {
    pub id: Uuid,
    pub title: String,
    pub cost: i64,
    pub prompt: Option<String>,
}
use serde::{Deserialize, Serialize};

#[derive(Clone)]
pub struct CreateCustomReward {
    pub title: String,
    pub cost: u32,
    pub description: Option<String>,
    pub background_color: Option<String>,
    pub max_per_stream: Option<u32>,
    pub max_per_user_per_stream: Option<u32>,
    pub global_cooldown_seconds: Option<u32>,
}

#[derive(Clone)]
pub struct UpdateCustomReward {
    pub title: Option<String>,
    pub cost: Option<u32>,
    pub description: Option<String>,
    pub background_color: Option<String>,
    pub max_per_stream: Option<u32>,
    pub max_per_user_per_stream: Option<u32>,
    pub global_cooldown_seconds: Option<u32>,
    pub is_paused: Option<bool>,
}

#[derive(Serialize, Default)]
pub struct TwitchUpdateRewardPayload {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cost: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background_color: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_max_per_stream_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_stream: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_max_per_user_per_stream_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_per_user_per_stream: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_global_cooldown_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub global_cooldown_seconds: Option<u32>,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub is_paused: Option<bool>,
}

#[derive(Deserialize)]
pub struct CustomRewardInfo {
    pub broadcaster_id: String,
    pub broadcaster_login: String,
    pub broadcaster_name: String,
    pub id: String,
    pub title: String,
    pub prompt: String,
    pub cost: u32,
    pub image: Option<RewardImageInfo>,
    pub default_image: RewardImageInfo,
    pub background_color: String,
    pub is_enabled: bool,
    pub is_user_input_required: bool,
    pub max_per_stream_setting: MaxPerStreamSetting,
    pub max_per_user_per_stream_setting: MaxPerUserPerStreamSetting,
    pub global_cooldown_setting: GlobalCooldownSetting,
    pub is_paused: bool,
    pub is_in_stock: bool,
    pub should_redemptions_skip_request_queue: bool,
    pub redemptions_redeemed_current_stream: Option<i32>,
    pub cooldown_expires_at: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct RewardImageInfo {
    pub url_1x: String,
    pub url_2x: String,
    pub url_4x: String,
}

#[derive(Serialize, Deserialize)]
pub struct GlobalCooldownSetting {
    pub is_enabled: bool,
    pub global_cooldown_seconds: u64,
}

#[derive(Serialize, Deserialize)]
pub struct MaxPerUserPerStreamSetting {
    pub is_enabled: bool,
    pub max_per_user_per_stream: u64,
}

#[derive(Serialize, Deserialize)]
pub struct MaxPerStreamSetting {
    pub is_enabled: bool,
    pub max_per_stream: u64,
}

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum CustomRewardRedemptionStatus {
    Unfulfilled,
    Canceled,
    Fulfilled,
}

#[derive(Serialize, Deserialize)]
pub struct ShortRewardInfo {
    pub id: String,
    pub title: String,
    #[serde(rename = "prompt")]
    pub description: String,
    pub cost: i64,
}

#[derive(Serialize, Deserialize)]
pub struct CustomRewardRedemptionInfo {
    pub broadcaster_name: String,
    pub broadcaster_login: String,
    pub broadcaster_id: String,
    pub id: String,
    pub user_id: String,
    pub user_name: String,
    pub user_login: String,
    pub user_input: String,
    pub status: CustomRewardRedemptionStatus,
    pub redeemed_at: String,
    pub reward: ShortRewardInfo,
}
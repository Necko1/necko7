pub mod model;
pub mod redemptions;

use serde_json::json;
use crate::helix::api::custom_rewards::model::{CreateCustomReward, CustomRewardInfo, TwitchUpdateRewardPayload, UpdateCustomReward};
use crate::helix::error::{HelixError, HelixResult};
use crate::helix::HelixClient;
use crate::helix::response::{parse_helix_error, ObjectResponse};

impl HelixClient {
    pub async fn create_custom_reward(
        &self,
        broadcaster_id: &str,
        reward_info: CreateCustomReward,
        user_token: &str,
    ) -> HelixResult<CustomRewardInfo> {
        let is_max_per_stream_enabled = reward_info.max_per_stream.is_some();
        let is_max_per_user_per_stream_enabled = reward_info.max_per_user_per_stream.is_some();
        let is_global_cooldown_enabled = reward_info.global_cooldown_seconds.is_some();
        let body = json!({
            "title": reward_info.title,
            "cost": reward_info.cost,
            "prompt": reward_info.description,
            "background_color": reward_info.background_color,
            "is_user_input_required": true,
            "is_max_per_stream_enabled": is_max_per_stream_enabled,
            "max_per_stream": reward_info.max_per_stream,
            "is_max_per_user_per_stream_enabled": is_max_per_user_per_stream_enabled,
            "max_per_user_per_stream": reward_info.max_per_user_per_stream,
            "is_global_cooldown_enabled": is_global_cooldown_enabled,
            "global_cooldown_seconds": reward_info.global_cooldown_seconds,
        });

        let res = self
            .http_client
            .post("https://api.twitch.tv/helix/channel_points/custom_rewards")
            .query(&[("broadcaster_id", broadcaster_id)])
            .json(&body)
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .send()
            .await?;

        if res.status().is_success() {
            let res_list = res.json::<ObjectResponse<CustomRewardInfo>>().await?;

            return res_list.data.into_iter().next()
                .ok_or_else(|| HelixError::Other(
                    "Got empty data list while creating custom reward".to_string()
                ));
        }

        Err(parse_helix_error(res).await)
    }

    pub async fn delete_custom_reward(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        user_token: &str,
    ) -> HelixResult<()> {
        let params = [
            ("broadcaster_id", broadcaster_id),
            ("id", reward_id)
        ];

        let res = self
            .http_client
            .delete("https://api.twitch.tv/helix/channel_points/custom_rewards")
            .query(&params)
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .send()
            .await?;

        if res.status().is_success() {
            return Ok(())
        }

        Err(parse_helix_error(res).await)
    }

    pub async fn update_custom_reward(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        reward_info: UpdateCustomReward,
        user_token: &str,
    ) -> HelixResult<CustomRewardInfo> {
        let (is_max_per_stream_enabled, max_per_stream) = match reward_info.max_per_stream {
            Some(0) => (Some(false), None),
            Some(val) => (Some(true), Some(val)),
            None => (None, None),
        };

        let (is_max_per_user_enabled, max_per_user) = match reward_info.max_per_user_per_stream {
            Some(0) => (Some(false), None),
            Some(val) => (Some(true), Some(val)),
            None => (None, None),
        };

        let (is_cooldown_enabled, cooldown_seconds) = match reward_info.global_cooldown_seconds {
            Some(0) => (Some(false), None),
            Some(val) => (Some(true), Some(val)),
            None => (None, None),
        };

        let body = TwitchUpdateRewardPayload {
            title: reward_info.title,
            cost: reward_info.cost,
            prompt: reward_info.description,
            background_color: reward_info.background_color,
            is_max_per_stream_enabled,
            max_per_stream,
            is_max_per_user_per_stream_enabled: is_max_per_user_enabled,
            max_per_user_per_stream: max_per_user,
            is_global_cooldown_enabled: is_cooldown_enabled,
            global_cooldown_seconds: cooldown_seconds,
            is_paused: reward_info.is_paused,
        };

        let params = [
            ("broadcaster_id", broadcaster_id),
            ("id", reward_id)
        ];

        let res = self
            .http_client
            .patch("https://api.twitch.tv/helix/channel_points/custom_rewards")
            .query(&params)
            .json(&body)
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .send()
            .await?;

        if res.status().is_success() {
            let res_list = res.json::<ObjectResponse<CustomRewardInfo>>().await?;

            return res_list.data.into_iter().next()
                .ok_or_else(|| HelixError::Other(
                    "Got empty data list while updating custom reward".to_string()
                ));
        }

        Err(parse_helix_error(res).await)
    }
}
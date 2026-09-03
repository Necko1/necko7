use serde_json::json;
use crate::helix::api::custom_rewards::model::CustomRewardRedemptionInfo;
use crate::helix::error::{HelixError, HelixResult};
use crate::helix::HelixClient;
use crate::helix::response::{parse_helix_error, ObjectResponse};

impl HelixClient {
    pub async fn update_redemption_status(
        &self,
        broadcaster_id: &str,
        reward_id: &str,
        redemption_id: &str,
        return_channel_points: bool,
        user_token: &str,
    ) -> HelixResult<CustomRewardRedemptionInfo> {
        let params = [
            ("broadcaster_id", broadcaster_id),
            ("id", redemption_id),
            ("reward_id", reward_id),
        ];

        let status = if return_channel_points { "CANCELED" } else { "FULFILLED" };

        let res = self
            .http_client
            .patch("https://api.twitch.tv/helix/channel_points/custom_rewards/redemptions")
            .query(&params)
            .json(&json!({
                "status": status,
            }))
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .send()
            .await?;

        if res.status().is_success() {
            let res_list = res.json::<ObjectResponse<CustomRewardRedemptionInfo>>().await?;

            return res_list.data.into_iter().next()
                .ok_or_else(|| HelixError::Other(
                    "Got empty data list while updating redemption status".to_string()
                ));
        }

        Err(parse_helix_error(res).await)
    }
}
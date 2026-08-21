use serde::{Deserialize, Serialize};
use crate::helix::error::{HelixError, HelixResult};
use crate::helix::HelixClient;
use crate::helix::response::{ErrorResponse, ObjectResponse};

#[derive(Deserialize)]
pub struct UserInfo {
    pub id: String,
    pub login: String,
    pub display_name: String,
    #[serde(rename = "type")]
    pub user_type: String,
    pub broadcaster_type: String,
    pub description: String,
    pub profile_image_url: String,
    pub offline_image_url: String,
    pub email: Option<String>,
    pub created_at: String,
}

impl HelixClient {
    pub async fn get_user_info_by_token(
        &self,
        user_token: &str,
    ) -> HelixResult<UserInfo> {
        let res = self
            .http_client
            .get("https://api.twitch.tv/helix/users")
            .header("Authorization", format!("Bearer {}", user_token))
            .header("Client-Id", &self.client_id)
            .send()
            .await?;

        if res.status().is_success() {
            let res_list = res.json::<ObjectResponse<UserInfo>>().await?;

            return res_list.data.into_iter().next()
                .ok_or(HelixError::Other(
                    "Could not find the user based on the provided token".to_string()
                ));
        }

        let error_res = res.json::<ErrorResponse>().await?;

        Err(error_res.into())
    }
}
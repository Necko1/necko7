use crate::helix::error::HelixResult;
use crate::helix::HelixClient;
use serde::Deserialize;
use crate::helix::response::ErrorResponse;

#[derive(Deserialize)]
pub struct AppTokenResponse {
    pub access_token: String,
    pub expires_in: u64,
    pub token_type: String,
}

#[derive(Deserialize)]
pub struct CodeAuthResponse {
    pub access_token: String,
    pub expires_in: u32,
    pub refresh_token: String,
    pub scope: Vec<String>,
    pub token_type: String,
}

impl HelixClient {
    pub async fn request_app_token(
        &self,
    ) -> HelixResult<String> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("grant_type", "client_credentials"),
        ];

        let res = self
            .http_client
            .post("https://id.twitch.tv/oauth2/token")
            .form(&params)
            .send()
            .await?;

        if res.status().is_success() {
            let token = res.json::<AppTokenResponse>().await?;

            return Ok(token.access_token)
        }

        let error_res = res.json::<ErrorResponse>().await?;

        Err(error_res.into())
    }

    pub async fn exchange_code_for_user_token(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> HelixResult<CodeAuthResponse> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ];

        let res = self
            .http_client
            .post("https://id.twitch.tv/oauth2/token")
            .form(&params)
            .send()
            .await?;

        if res.status().is_success() {
            let car = res.json::<CodeAuthResponse>().await?;

            return Ok(car)
        }

        let error_res = res.json::<ErrorResponse>().await?;

        Err(error_res.into())
    }
}
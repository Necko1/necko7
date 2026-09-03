use crate::helix::error::{HelixError, HelixResult};
use crate::helix::HelixClient;
use serde::Deserialize;
use crate::helix::response::parse_helix_error;

fn deserialize_null_as_default<'de, D, T>(deserializer: D) -> Result<T, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de> + Default,
{
    let opt = Option::<T>::deserialize(deserializer)?;
    Ok(opt.unwrap_or_default())
}

#[derive(Debug, Deserialize, Clone)]
pub struct AppTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default)]
    pub token_type: String,
}

#[derive(Debug, Deserialize, Clone)]
pub struct UserTokenResponse {
    pub access_token: String,
    #[serde(default)]
    pub expires_in: u64,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub refresh_token: String,
    #[serde(default, deserialize_with = "deserialize_null_as_default")]
    pub scope: Vec<String>,
    #[serde(default)]
    pub token_type: String,
}

impl HelixClient {
    pub async fn request_app_token(
        &self,
    ) -> HelixResult<AppTokenResponse> {
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
            let text = res.text().await?;
            match serde_json::from_str::<AppTokenResponse>(&text) {
                Ok(token) => return Ok(token),
                Err(err) => {
                    tracing::error!(error = %err, "Failed to parse AppTokenResponse from Twitch OAuth");
                    return Err(HelixError::Other(format!("Failed to parse app token response: {}", err)));
                }
            }
        }

        Err(parse_helix_error(res).await)
    }

    pub async fn exchange_code_for_user_token(
        &self,
        code: &str,
        redirect_uri: &str,
    ) -> HelixResult<UserTokenResponse> {
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
            let text = res.text().await?;
            match serde_json::from_str::<UserTokenResponse>(&text) {
                Ok(token) => return Ok(token),
                Err(err) => {
                    tracing::error!(error = %err, "Failed to parse UserTokenResponse from Twitch OAuth");
                    return Err(HelixError::Other(format!("Failed to parse user token response: {}", err)));
                }
            }
        }

        Err(parse_helix_error(res).await)
    }

    pub async fn refresh_user_token(
        &self,
        refresh_token: &str,
    ) -> HelixResult<UserTokenResponse> {
        let params = [
            ("client_id", self.client_id.as_str()),
            ("client_secret", self.client_secret.as_str()),
            ("grant_type", "refresh_token"),
            ("refresh_token", refresh_token),
        ];

        let res = self
            .http_client
            .post("https://id.twitch.tv/oauth2/token")
            .form(&params)
            .send()
            .await?;

        if res.status().is_success() {
            let text = res.text().await?;
            match serde_json::from_str::<UserTokenResponse>(&text) {
                Ok(token) => return Ok(token),
                Err(err) => {
                    tracing::error!(error = %err, "Failed to parse UserTokenResponse from Twitch OAuth");
                    return Err(HelixError::Other(format!("Failed to parse user token response: {}", err)));
                }
            }
        }

        Err(parse_helix_error(res).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_user_token_with_null_scope() {
        let json = r#"{
            "access_token": "mock_access_token",
            "expires_in": 14144,
            "refresh_token": "mock_refresh_token",
            "scope": null,
            "token_type": "bearer"
        }"#;

        let res = serde_json::from_str::<UserTokenResponse>(json);
        assert!(res.is_ok(), "Failed to deserialize UserTokenResponse with scope: null: {:?}", res.err());
        let token = res.unwrap();
        assert_eq!(token.access_token, "mock_access_token");
        assert_eq!(token.expires_in, 14144);
        assert_eq!(token.refresh_token, "mock_refresh_token");
        assert!(token.scope.is_empty());
        assert_eq!(token.token_type, "bearer");
    }

    #[test]
    fn test_deserialize_user_token_with_scopes() {
        let json = r#"{
            "access_token": "mock_access_token",
            "expires_in": 14144,
            "refresh_token": "mock_refresh_token",
            "scope": ["channel:read:redemptions", "channel:bot"],
            "token_type": "bearer"
        }"#;

        let res = serde_json::from_str::<UserTokenResponse>(json);
        assert!(res.is_ok());
        let token = res.unwrap();
        assert_eq!(token.scope.len(), 2);
        assert_eq!(token.scope[0], "channel:read:redemptions");
        assert_eq!(token.scope[1], "channel:bot");
    }

    #[test]
    fn test_deserialize_user_token_with_missing_scope() {
        let json = r#"{
            "access_token": "mock_access_token",
            "expires_in": 14144,
            "refresh_token": "mock_refresh_token",
            "token_type": "bearer"
        }"#;

        let res = serde_json::from_str::<UserTokenResponse>(json);
        assert!(res.is_ok());
        let token = res.unwrap();
        assert!(token.scope.is_empty());
    }
}
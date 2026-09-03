use serde::Deserialize;
use crate::helix::error::HelixError;

#[derive(Deserialize)]
pub struct ObjectResponse<T> {
    pub data: Vec<T>
}

#[derive(Deserialize)]
pub struct ErrorResponse {
    error: Option<String>,
    status: u16,
    message: String,
}

impl From<ErrorResponse> for HelixError {
    fn from(value: ErrorResponse) -> Self {
        if value.status == 401 {
            return HelixError::Unauthorized(value.message);
        }

        HelixError::Other(format!(
            "{} {}: {}",
            value.status,
            value.error.unwrap_or_default(),
            value.message
        ))
    }
}

pub async fn parse_helix_error(res: reqwest::Response) -> HelixError {
    let status = res.status();
    let status_u16 = status.as_u16();
    let text = match res.text().await {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(error = %e, status = status_u16, "Failed to read Helix error response body");
            return HelixError::Other(format!("HTTP {} (failed to read response body)", status_u16));
        }
    };

    if let Ok(err_res) = serde_json::from_str::<ErrorResponse>(&text) {
        err_res.into()
    } else {
        tracing::warn!(
            status = status_u16,
            body = %text,
            "Helix API returned non-JSON error response"
        );
        if status_u16 == 401 {
            HelixError::Unauthorized(text)
        } else {
            HelixError::Other(format!("HTTP {}: {}", status_u16, text))
        }
    }
}
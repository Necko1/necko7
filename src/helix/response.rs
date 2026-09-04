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
        let message = if value.message.is_empty() {
            value.error.clone().unwrap_or_else(|| format!("HTTP {}", value.status))
        } else {
            value.message
        };

        match value.status {
            400 => HelixError::BadRequest(message),
            401 => HelixError::Unauthorized(message),
            403 => HelixError::Forbidden(message),
            404 => HelixError::NotFound(message),
            409 => HelixError::Conflict(message),
            status if status < 500 => HelixError::Api {
                status,
                message,
            },
            status => HelixError::Other(format!(
                "{} {}: {}",
                status,
                value.error.unwrap_or_default(),
                message
            )),
        }
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
        match status_u16 {
            400 => HelixError::BadRequest(text),
            401 => HelixError::Unauthorized(text),
            403 => HelixError::Forbidden(text),
            404 => HelixError::NotFound(text),
            409 => HelixError::Conflict(text),
            s if s < 500 => HelixError::Api { status: s, message: text },
            _ => HelixError::Other(format!("HTTP {}: {}", status_u16, text)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_error_response_to_helix_error() {
        let err_res = ErrorResponse {
            error: Some("Bad Request".to_string()),
            status: 400,
            message: "The parameter \"title\" was malformed: the value must be less than or equal to 45".to_string(),
        };
        let helix_err = HelixError::from(err_res);
        match helix_err {
            HelixError::BadRequest(msg) => {
                assert_eq!(msg, "The parameter \"title\" was malformed: the value must be less than or equal to 45");
            }
            _ => panic!("Expected HelixError::BadRequest"),
        }
    }

    #[test]
    fn test_error_response_unauthorized() {
        let err_res = ErrorResponse {
            error: Some("Unauthorized".to_string()),
            status: 401,
            message: "Invalid OAuth token".to_string(),
        };
        let helix_err = HelixError::from(err_res);
        match helix_err {
            HelixError::Unauthorized(msg) => assert_eq!(msg, "Invalid OAuth token"),
            _ => panic!("Expected HelixError::Unauthorized"),
        }
    }

    #[test]
    fn test_error_response_forbidden_and_not_found() {
        let forbidden = ErrorResponse {
            error: Some("Forbidden".to_string()),
            status: 403,
            message: "User not authorized".to_string(),
        };
        match HelixError::from(forbidden) {
            HelixError::Forbidden(msg) => assert_eq!(msg, "User not authorized"),
            _ => panic!("Expected HelixError::Forbidden"),
        }

        let not_found = ErrorResponse {
            error: Some("Not Found".to_string()),
            status: 404,
            message: "Reward not found".to_string(),
        };
        match HelixError::from(not_found) {
            HelixError::NotFound(msg) => assert_eq!(msg, "Reward not found"),
            _ => panic!("Expected HelixError::NotFound"),
        }
    }
}
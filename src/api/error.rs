use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use utoipa::ToSchema;
use crate::db::error::DbError;
use crate::helix::error::HelixError;

#[derive(Debug, utoipa::IntoResponses)]
pub enum ApiError {
    #[response(status = BAD_REQUEST, description = "The request is invalid (bad parameter, missing field, etc.)")]
    BadRequest {
        message: String,
        param: String,
    },
    #[response(status = UNAUTHORIZED, description = "Authentication required. Provide a valid session cookie.")]
    Unauthorized {
        message: String,
    },
    #[response(status = FORBIDDEN, description = "You do not have permission to access this resource.")]
    Forbidden {
        message: String,
    },
    #[response(status = NOT_FOUND, description = "The requested resource was not found.")]
    NotFound {
        message: String,
    },
    #[response(status = UNPROCESSABLE_ENTITY, description = "Validation failed for the provided data.")]
    UnprocessableEntity {
        message: String,
        param: String,
    },
    #[response(status = INTERNAL_SERVER_ERROR, description = "An internal server error occurred. This is not your fault.")]
    Internal {
        message: String,
    },
}

#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    pub error: ErrorDetail,
}

#[derive(Serialize, ToSchema)]
pub struct ErrorDetail {
    /// Error code (e.g. `invalid_request_error`, `authentication_error`)
    pub code: &'static str,
    /// Error type category
    #[serde(rename = "type")]
    pub error_type: &'static str,
    /// Human-readable error message
    pub message: String,
    /// Parameter name that caused the error (if applicable)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub param: Option<String>,
}

impl ApiError {
    fn status_code(&self) -> StatusCode {
        match self {
            ApiError::BadRequest { .. } => StatusCode::BAD_REQUEST,
            ApiError::Unauthorized { .. } => StatusCode::UNAUTHORIZED,
            ApiError::Forbidden { .. } => StatusCode::FORBIDDEN,
            ApiError::NotFound { .. } => StatusCode::NOT_FOUND,
            ApiError::UnprocessableEntity { .. } => StatusCode::UNPROCESSABLE_ENTITY,
            ApiError::Internal { .. } => StatusCode::INTERNAL_SERVER_ERROR,
        }
    }

    fn code_and_type(&self) -> (&'static str, &'static str) {
        match self {
            ApiError::BadRequest { .. } => ("invalid_request_error", "invalid_request_error"),
            ApiError::Unauthorized { .. } => ("authentication_error", "authentication_error"),
            ApiError::Forbidden { .. } => ("authorization_error", "authorization_error"),
            ApiError::NotFound { .. } => ("not_found", "not_found"),
            ApiError::UnprocessableEntity { .. } => ("validation_error", "validation_error"),
            ApiError::Internal { .. } => ("api_error", "api_error"),
        }
    }

    fn message(&self) -> &str {
        match self {
            ApiError::BadRequest { message, .. } => message,
            ApiError::Unauthorized { message } => message,
            ApiError::Forbidden { message } => message,
            ApiError::NotFound { message } => message,
            ApiError::UnprocessableEntity { message, .. } => message,
            ApiError::Internal { message } => message,
        }
    }

    fn param(&self) -> Option<&str> {
        match self {
            ApiError::BadRequest { param, .. } => {
                if param.is_empty() {
                    None
                } else {
                    Some(param.as_str())
                }
            }
            ApiError::UnprocessableEntity { param, .. } => {
                if param.is_empty() {
                    None
                } else {
                    Some(param.as_str())
                }
            }
            _ => None,
        }
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message())
    }
}

impl std::error::Error for ApiError {}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status_code();
        let (code, error_type) = self.code_and_type();
        let message = self.message();
        let param = self.param();

        if status.is_server_error() {
            tracing::error!(
                status = status.as_u16(),
                error_code = code,
                error_type = error_type,
                error_message = message,
                param = ?param,
                "API request failed with internal server error"
            );
        } else if status == StatusCode::UNAUTHORIZED || status == StatusCode::FORBIDDEN {
            tracing::warn!(
                status = status.as_u16(),
                error_code = code,
                error_message = message,
                "API request rejected by authentication/authorization"
            );
        } else {
            tracing::debug!(
                status = status.as_u16(),
                error_code = code,
                error_message = message,
                param = ?param,
                "API request client error"
            );
        }

        let body = ErrorBody {
            error: ErrorDetail {
                code,
                error_type,
                message: message.to_string(),
                param: param.map(String::from),
            },
        };

        (status, Json(body)).into_response()
    }
}

fn extract_param_from_twitch_msg(msg: &str) -> Option<String> {
    if let Some(start) = msg.find("parameter \"") {
        let remainder = &msg[start + 11..];
        if let Some(end) = remainder.find('"') {
            let param_name = &remainder[..end];
            let mapped = match param_name {
                "title" => "twitch_title",
                "prompt" => "twitch_description",
                "cost" => "cost",
                other => other,
            };
            return Some(mapped.to_string());
        }
    }
    None
}

impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        tracing::error!(error = %err, "Database error mapped to ApiError::Internal");
        ApiError::Internal {
            message: "Internal database error".to_string(),
        }
    }
}

impl From<reqwest::Error> for ApiError {
    fn from(err: reqwest::Error) -> Self {
        tracing::error!(
            error = %err,
            url = ?err.url().map(|u| u.as_str()),
            status = ?err.status().map(|s| s.as_u16()),
            "Reqwest HTTP error mapped to ApiError::Internal"
        );
        ApiError::Internal {
            message: "Internal reqwest error".to_string(),
        }
    }
}

impl From<HelixError> for ApiError {
    fn from(err: HelixError) -> Self {
        match err {
            HelixError::BadRequest(msg) => {
                tracing::warn!(reason = %msg, "HelixError::BadRequest mapped to ApiError::BadRequest");
                let param = extract_param_from_twitch_msg(&msg).unwrap_or_default();
                ApiError::BadRequest {
                    message: msg,
                    param,
                }
            }
            HelixError::Unauthorized(msg) => {
                tracing::warn!(reason = %msg, "HelixError::Unauthorized mapped to ApiError::Unauthorized");
                ApiError::Unauthorized { message: msg }
            }
            HelixError::Forbidden(msg) => {
                tracing::warn!(reason = %msg, "HelixError::Forbidden mapped to ApiError::Forbidden");
                ApiError::Forbidden { message: msg }
            }
            HelixError::NotFound(msg) => {
                tracing::warn!(reason = %msg, "HelixError::NotFound mapped to ApiError::NotFound");
                ApiError::NotFound { message: msg }
            }
            HelixError::Conflict(msg) => {
                tracing::warn!(reason = %msg, "HelixError::Conflict mapped to ApiError::BadRequest");
                ApiError::BadRequest {
                    message: msg,
                    param: String::new(),
                }
            }
            HelixError::Api { status, message } => {
                if status == 422 {
                    tracing::warn!(status = status, reason = %message, "HelixError::Api mapped to ApiError::UnprocessableEntity");
                    ApiError::UnprocessableEntity {
                        message,
                        param: String::new(),
                    }
                } else if status < 500 {
                    tracing::warn!(status = status, reason = %message, "HelixError::Api client error mapped to ApiError::BadRequest");
                    ApiError::BadRequest {
                        message,
                        param: String::new(),
                    }
                } else {
                    tracing::error!(status = status, reason = %message, "Helix upstream error mapped to ApiError::Internal");
                    ApiError::Internal { message }
                }
            }
            HelixError::Other(msg) => {
                tracing::error!(reason = %msg, "HelixError::Other mapped to ApiError::Internal");
                ApiError::Internal { message: msg }
            }
            HelixError::Reqwest(err) => {
                tracing::error!(
                    error = %err,
                    url = ?err.url().map(|u| u.as_str()),
                    status = ?err.status().map(|s| s.as_u16()),
                    "HelixError::Reqwest mapped to ApiError::Internal"
                );
                ApiError::Internal {
                    message: "Upstream HTTP request failed".to_string(),
                }
            }
        }
    }
}

impl From<Box<dyn std::error::Error>> for ApiError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        match err.downcast::<HelixError>() {
            Ok(helix_err) => ApiError::from(*helix_err),
            Err(err) => match err.downcast::<ApiError>() {
                Ok(api_err) => *api_err,
                Err(err) => match err.downcast::<DbError>() {
                    Ok(db_err) => ApiError::from(*db_err),
                    Err(err) => {
                        tracing::error!(error = %err, "Boxed dyn Error mapped to ApiError::Internal");
                        ApiError::Internal {
                            message: err.to_string(),
                        }
                    }
                },
            },
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ApiError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        match err.downcast::<HelixError>() {
            Ok(helix_err) => ApiError::from(*helix_err),
            Err(err) => match err.downcast::<ApiError>() {
                Ok(api_err) => *api_err,
                Err(err) => match err.downcast::<DbError>() {
                    Ok(db_err) => ApiError::from(*db_err),
                    Err(err) => {
                        tracing::error!(error = %err, "Boxed dyn Error + Send + Sync mapped to ApiError::Internal");
                        ApiError::Internal {
                            message: err.to_string(),
                        }
                    }
                },
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::StatusCode;

    #[test]
    fn test_extract_param_from_twitch_msg() {
        let msg = "The parameter \"title\" was malformed: the value must be less than or equal to 45";
        assert_eq!(extract_param_from_twitch_msg(msg), Some("twitch_title".to_string()));

        let msg2 = "The parameter \"prompt\" was malformed: the value must be less than or equal to 500";
        assert_eq!(extract_param_from_twitch_msg(msg2), Some("twitch_description".to_string()));

        let msg3 = "The parameter \"cost\" was malformed";
        assert_eq!(extract_param_from_twitch_msg(msg3), Some("cost".to_string()));

        let msg4 = "Something else went wrong";
        assert_eq!(extract_param_from_twitch_msg(msg4), None);
    }

    #[test]
    fn test_helix_bad_request_to_api_error() {
        let helix_err = HelixError::BadRequest("The parameter \"title\" was malformed: the value must be less than or equal to 45".to_string());
        let api_err = ApiError::from(helix_err);

        assert_eq!(api_err.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(api_err.param(), Some("twitch_title"));
        assert_eq!(api_err.message(), "The parameter \"title\" was malformed: the value must be less than or equal to 45");
    }

    #[test]
    fn test_boxed_helix_error_downcast() {
        let helix_err = HelixError::BadRequest("The parameter \"title\" was malformed: the value must be less than or equal to 45".to_string());
        let boxed: Box<dyn std::error::Error + Send + Sync> = Box::new(helix_err);

        let api_err = ApiError::from(boxed);
        assert_eq!(api_err.status_code(), StatusCode::BAD_REQUEST);
        assert_eq!(api_err.param(), Some("twitch_title"));
    }
}

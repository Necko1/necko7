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
            ApiError::BadRequest { param, .. } => Some(param.as_str()),
            ApiError::UnprocessableEntity { param, .. } => Some(param.as_str()),
            _ => None,
        }
    }
}

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
            HelixError::Unauthorized(msg) => {
                tracing::warn!(reason = %msg, "HelixError::Unauthorized mapped to ApiError::Unauthorized");
                ApiError::Unauthorized { message: msg }
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
        tracing::error!(error = %err, "Boxed dyn Error mapped to ApiError::Internal");
        ApiError::Internal {
            message: err.to_string(),
        }
    }
}

impl From<Box<dyn std::error::Error + Send + Sync>> for ApiError {
    fn from(err: Box<dyn std::error::Error + Send + Sync>) -> Self {
        tracing::error!(error = %err, "Boxed dyn Error + Send + Sync mapped to ApiError::Internal");
        ApiError::Internal {
            message: err.to_string(),
        }
    }
}

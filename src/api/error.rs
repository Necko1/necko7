use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::Serialize;
use crate::db::error::DbError;
use crate::helix::error::HelixError;

#[derive(Debug)]
pub enum ApiError {
    BadRequest {
        message: String,
        param: String,
    },
    Unauthorized {
        message: String,
    },
    Forbidden {
        message: String,
    },
    NotFound {
        message: String,
    },
    UnprocessableEntity {
        message: String,
        param: String,
    },
    Internal {
        message: String,
    },
}

#[derive(Serialize)]
struct ErrorBody {
    error: ErrorDetail,
}

#[derive(Serialize)]
struct ErrorDetail {
    code: &'static str,
    #[serde(rename = "type")]
    error_type: &'static str,
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    param: Option<String>,
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
        let (code, error_type) = self.code_and_type();
        let body = ErrorBody {
            error: ErrorDetail {
                code,
                error_type,
                message: self.message().to_string(),
                param: self.param().map(String::from),
            },
        };

        (self.status_code(), Json(body)).into_response()
    }
}

impl From<DbError> for ApiError {
    fn from(err: DbError) -> Self {
        tracing::error!("Database error: {:?}", err);
        ApiError::Internal {
            message: "Internal database error".to_string(),
        }
    }
}

impl From<HelixError> for ApiError {
    fn from(err: HelixError) -> Self {
        match err {
            HelixError::Unauthorized(msg) => ApiError::Unauthorized { message: msg },
            HelixError::Other(msg) => ApiError::Internal { message: msg },
            HelixError::Reqwest(err) => {
                tracing::error!("HTTP client error: {:?}", err);
                ApiError::Internal {
                    message: "Upstream HTTP request failed".to_string(),
                }
            }
        }
    }
}

impl From<Box<dyn std::error::Error>> for ApiError {
    fn from(err: Box<dyn std::error::Error>) -> Self {
        tracing::error!("Boxed error: {:?}", err);
        ApiError::Internal {
            message: err.to_string(),
        }
    }
}

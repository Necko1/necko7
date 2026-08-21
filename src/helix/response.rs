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
            value.error.unwrap_or("".to_string()),
            value.message
        ))
    }
}
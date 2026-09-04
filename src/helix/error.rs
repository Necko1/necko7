use thiserror::Error;

#[derive(Debug, Error)]
pub enum HelixError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error("Bad request: {0}")]
    BadRequest(String),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Forbidden: {0}")]
    Forbidden(String),

    #[error("Not found: {0}")]
    NotFound(String),

    #[error("Conflict: {0}")]
    Conflict(String),

    #[error("Helix API error ({status}): {message}")]
    Api {
        status: u16,
        message: String,
    },

    #[error("Helix error: {0}")]
    Other(String),
}

pub type HelixResult<T> = Result<T, HelixError>;
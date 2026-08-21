use serde::Deserialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum HelixError {
    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error("Unauthorized: {0}")]
    Unauthorized(String),

    #[error("Helix error: {0}")]
    Other(String)
}

pub type HelixResult<T> = Result<T, HelixError>;
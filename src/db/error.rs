use thiserror::Error;

#[derive(Debug, Error)]
pub enum DbError {
    #[error("Database driver error: {0}")]
    Sqlx(#[from] sqlx::Error),

    #[error("Failed to run migrations: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),
}

pub type DbResult<T> = Result<T, DbError>;
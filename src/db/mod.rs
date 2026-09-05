pub mod broadcasters;
pub mod app_settings;
pub mod rewards;
pub mod redemptions;
pub mod broadcaster_settings;
pub mod error;
pub mod users;
pub mod sessions;
pub mod channel_permissions;
pub mod chat_messages;

use sqlx::postgres::{PgPool, PgPoolOptions};
use tracing::info;
use crate::db::error::DbResult;

#[derive(Clone)]
pub struct Db {
    pool: PgPool,
}

impl Db {
    pub async fn connect(database_url: &str) -> DbResult<Self> {
        let pool = PgPoolOptions::new()
            .max_connections(5)
            .connect(database_url)
            .await?;

        info!("Running database migrations...");
        sqlx::migrate!()
            .run(&pool)
            .await?;
        info!("Database migrations completed successfully");

        Ok(Self { pool })
    }

    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    pub async fn close(&self) {
        self.pool.close().await;
    }
}

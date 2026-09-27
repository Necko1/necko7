use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Serialize, FromRow, utoipa::ToSchema)]
pub struct Device {
    pub id: Uuid,
    pub app_version: String,
    pub created_at: DateTime<Utc>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

impl super::Db {
    pub async fn cs2_device(&self, channel: &str) -> super::error::DbResult<Option<Device>> {
        Ok(sqlx::query_as("SELECT id, app_version, created_at, last_seen_at FROM cs2_devices WHERE channel_id=$1 AND revoked_at IS NULL")
            .bind(channel).fetch_optional(&self.pool).await?)
    }
}

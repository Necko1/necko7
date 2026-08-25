use sqlx::FromRow;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, sqlx::Type, PartialEq, serde::Serialize, utoipa::ToSchema)]
#[sqlx(type_name = "VARCHAR", rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelRole {
    Owner,
    Editor,
}

#[derive(Debug, Clone, FromRow)]
pub struct ChannelPermission {
    pub channel_id: String,
    pub user_id: String,
    pub role: ChannelRole,
    pub granted_by: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewChannelPermission {
    pub channel_id: String,
    pub user_id: String,
    pub role: ChannelRole,
    pub granted_by: String,
}

impl Db {
    pub async fn get_permissions_by_channel(&self, channel_id: &str) -> DbResult<Vec<ChannelPermission>> {
        let permissions = sqlx::query_as::<_, ChannelPermission>(
            "SELECT channel_id, user_id, role, granted_by, created_at FROM channel_permissions WHERE channel_id = $1"
        )
        .bind(channel_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(permissions)
    }

    pub async fn get_permissions_by_user(&self, user_id: &str) -> DbResult<Vec<ChannelPermission>> {
        let permissions = sqlx::query_as::<_, ChannelPermission>(
            "SELECT channel_id, user_id, role, granted_by, created_at FROM channel_permissions WHERE user_id = $1"
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(permissions)
    }

    pub async fn get_permission(&self, channel_id: &str, user_id: &str) -> DbResult<Option<ChannelPermission>> {
        let permission = sqlx::query_as::<_, ChannelPermission>(
            "SELECT channel_id, user_id, role, granted_by, created_at FROM channel_permissions WHERE channel_id = $1 AND user_id = $2"
        )
        .bind(channel_id)
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(permission)
    }

    pub async fn create_permission(&self, new: &NewChannelPermission) -> DbResult<ChannelPermission> {
        let permission = sqlx::query_as::<_, ChannelPermission>(
            "INSERT INTO channel_permissions (channel_id, user_id, role, granted_by, created_at) VALUES ($1, $2, $3, $4, NOW()) RETURNING channel_id, user_id, role, granted_by, created_at"
        )
        .bind(&new.channel_id)
        .bind(&new.user_id)
        .bind(&new.role)
        .bind(&new.granted_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(permission)
    }

    pub async fn upsert_permission(&self, new: &NewChannelPermission) -> DbResult<ChannelPermission> {
        let permission = sqlx::query_as::<_, ChannelPermission>(
            "INSERT INTO channel_permissions (channel_id, user_id, role, granted_by, created_at) VALUES ($1, $2, $3, $4, NOW()) ON CONFLICT (channel_id, user_id) DO UPDATE SET role = EXCLUDED.role, granted_by = EXCLUDED.granted_by RETURNING channel_id, user_id, role, granted_by, created_at"
        )
        .bind(&new.channel_id)
        .bind(&new.user_id)
        .bind(&new.role)
        .bind(&new.granted_by)
        .fetch_one(&self.pool)
        .await?;
        Ok(permission)
    }

    pub async fn delete_permission(&self, channel_id: &str, user_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM channel_permissions WHERE channel_id = $1 AND user_id = $2")
            .bind(channel_id)
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

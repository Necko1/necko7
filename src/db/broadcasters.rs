use sqlx::FromRow;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct Broadcaster {
    pub channel_id: String,
    pub channel_login: String,
    pub user_access_token: String,
    pub refresh_token: String,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewBroadcaster {
    pub channel_id: String,
    pub channel_login: String,
    pub user_access_token: String,
    pub refresh_token: String,
}

impl Db {
    pub async fn get_broadcaster_by_id(&self, channel_id: &str) -> DbResult<Option<Broadcaster>> {
        let broadcaster = sqlx::query_as::<_, Broadcaster>(
            "SELECT channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at FROM broadcasters WHERE channel_id = $1"
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(broadcaster)
    }

    pub async fn get_broadcaster_by_login(&self, channel_login: &str) -> DbResult<Option<Broadcaster>> {
        let broadcaster = sqlx::query_as::<_, Broadcaster>(
            "SELECT channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at FROM broadcasters WHERE channel_login = $1"
        )
        .bind(channel_login)
        .fetch_optional(&self.pool)
        .await?;
        Ok(broadcaster)
    }

    pub async fn get_all_broadcasters(&self) -> DbResult<Vec<Broadcaster>> {
        let broadcasters = sqlx::query_as::<_, Broadcaster>(
            "SELECT channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at FROM broadcasters"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(broadcasters)
    }

    pub async fn create_broadcaster(&self, new: &NewBroadcaster) -> DbResult<Broadcaster> {
        let broadcaster = sqlx::query_as::<_, Broadcaster>(
            "INSERT INTO broadcasters (channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW(), NOW()) RETURNING channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at"
        )
        .bind(&new.channel_id)
        .bind(&new.channel_login)
        .bind(&new.user_access_token)
        .bind(&new.refresh_token)
        .fetch_one(&self.pool)
        .await?;
        Ok(broadcaster)
    }

    pub async fn upsert_broadcaster(&self, new: &NewBroadcaster) -> DbResult<Broadcaster> {
        let broadcaster = sqlx::query_as::<_, Broadcaster>(
            "INSERT INTO broadcasters (channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at) VALUES ($1, $2, $3, $4, NOW(), NOW()) ON CONFLICT (channel_id) DO UPDATE SET channel_login = EXCLUDED.channel_login, user_access_token = EXCLUDED.user_access_token, refresh_token = EXCLUDED.refresh_token, updated_at = NOW() RETURNING channel_id, channel_login, user_access_token, refresh_token, created_at, updated_at"
        )
        .bind(&new.channel_id)
        .bind(&new.channel_login)
        .bind(&new.user_access_token)
        .bind(&new.refresh_token)
        .fetch_one(&self.pool)
        .await?;
        Ok(broadcaster)
    }

    pub async fn update_broadcaster_tokens(&self, channel_id: &str, access_token: &str, refresh_token: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE broadcasters SET user_access_token = $1, refresh_token = $2, updated_at = NOW() WHERE channel_id = $3"
        )
        .bind(access_token)
        .bind(refresh_token)
        .bind(channel_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_broadcaster(&self, channel_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM broadcasters WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

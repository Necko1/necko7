use sqlx::FromRow;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct User {
    pub twitch_id: String,
    pub login: String,
    pub avatar_url: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewUser {
    pub twitch_id: String,
    pub login: String,
    pub avatar_url: Option<String>,
}

impl Db {
    pub async fn get_user_by_twitch_id(&self, twitch_id: &str) -> DbResult<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT twitch_id, login, avatar_url, created_at, updated_at FROM users WHERE twitch_id = $1"
        )
        .bind(twitch_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn get_user_by_login(&self, login: &str) -> DbResult<Option<User>> {
        let user = sqlx::query_as::<_, User>(
            "SELECT twitch_id, login, avatar_url, created_at, updated_at FROM users WHERE login = $1"
        )
        .bind(login)
        .fetch_optional(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn create_user(&self, new: &NewUser) -> DbResult<User> {
        let user = sqlx::query_as::<_, User>(
            "INSERT INTO users (twitch_id, login, avatar_url, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW()) RETURNING twitch_id, login, avatar_url, created_at, updated_at"
        )
        .bind(&new.twitch_id)
        .bind(&new.login)
        .bind(&new.avatar_url)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn upsert_user(&self, new: &NewUser) -> DbResult<User> {
        let user = sqlx::query_as::<_, User>(
            "INSERT INTO users (twitch_id, login, avatar_url, created_at, updated_at) VALUES ($1, $2, $3, NOW(), NOW()) ON CONFLICT (twitch_id) DO UPDATE SET login = EXCLUDED.login, avatar_url = EXCLUDED.avatar_url, updated_at = NOW() RETURNING twitch_id, login, avatar_url, created_at, updated_at"
        )
        .bind(&new.twitch_id)
        .bind(&new.login)
        .bind(&new.avatar_url)
        .fetch_one(&self.pool)
        .await?;
        Ok(user)
    }

    pub async fn delete_user(&self, twitch_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM users WHERE twitch_id = $1")
            .bind(twitch_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

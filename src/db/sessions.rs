use sqlx::FromRow;
use uuid::Uuid;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct Session {
    pub session_id: Uuid,
    pub user_id: String,
    pub expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewSession {
    pub session_id: Uuid,
    pub user_id: String,
    pub expires_at: DateTime<Utc>,
}

impl Db {
    pub async fn get_session_by_id(&self, session_id: Uuid) -> DbResult<Option<Session>> {
        let session = sqlx::query_as::<_, Session>(
            "SELECT session_id, user_id, expires_at FROM sessions WHERE session_id = $1"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(session)
    }

    pub async fn get_valid_session(&self, session_id: Uuid) -> DbResult<Option<Session>> {
        let session = sqlx::query_as::<_, Session>(
            "SELECT session_id, user_id, expires_at FROM sessions WHERE session_id = $1 AND expires_at > NOW()"
        )
        .bind(session_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(session)
    }

    pub async fn create_session(&self, new: &NewSession) -> DbResult<Session> {
        let session = sqlx::query_as::<_, Session>(
            "INSERT INTO sessions (session_id, user_id, expires_at) VALUES ($1, $2, $3) RETURNING session_id, user_id, expires_at"
        )
        .bind(new.session_id)
        .bind(&new.user_id)
        .bind(new.expires_at)
        .fetch_one(&self.pool)
        .await?;
        Ok(session)
    }

    pub async fn extend_session(&self, session_id: Uuid, new_expires_at: DateTime<Utc>) -> DbResult<()> {
        sqlx::query("UPDATE sessions SET expires_at = $1 WHERE session_id = $2")
            .bind(new_expires_at)
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_session(&self, session_id: Uuid) -> DbResult<()> {
        sqlx::query("DELETE FROM sessions WHERE session_id = $1")
            .bind(session_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_all_sessions_for_user(&self, user_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM sessions WHERE user_id = $1")
            .bind(user_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn delete_expired_sessions(&self) -> DbResult<()> {
        sqlx::query("DELETE FROM sessions WHERE expires_at < NOW()")
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use utoipa::ToSchema;
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct ChatMessage {
    pub id: i64,
    pub message_id: String,
    pub broadcaster_id: String,
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    pub message_text: String,
    pub char_count: i32,
    pub sent_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewChatMessage {
    pub message_id: String,
    pub broadcaster_id: String,
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    pub message_text: String,
    pub char_count: i32,
    pub sent_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct LeaderboardUserItem {
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    pub message_count: i64,
    pub char_count: i64,
    pub first_seen_at: DateTime<Utc>,
    pub last_seen_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct UserChatSummary {
    pub chatter_user_id: String,
    pub chatter_user_login: String,
    pub total_messages: i64,
    pub total_chars: i64,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
}

impl Db {
    /// Inserts a new chat message into the database.
    pub async fn insert_chat_message(&self, msg: &NewChatMessage) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO chat_messages (message_id, broadcaster_id, chatter_user_id, chatter_user_login, message_text, char_count, sent_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())"
        )
        .bind(&msg.message_id)
        .bind(&msg.broadcaster_id)
        .bind(&msg.chatter_user_id)
        .bind(&msg.chatter_user_login)
        .bind(&msg.message_text)
        .bind(msg.char_count)
        .bind(msg.sent_at)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Fast lookup of (message_count, char_count) for a specific user in a channel since a given timestamp.
    pub async fn get_user_chat_stats(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        since: Option<DateTime<Utc>>,
    ) -> DbResult<(i64, i64)> {
        let row = sqlx::query_as::<_, (i64, i64)>(
            "SELECT COUNT(*)::BIGINT AS msg_count, COALESCE(SUM(char_count), 0)::BIGINT AS char_count
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND chatter_user_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR sent_at >= $3)"
        )
        .bind(broadcaster_id)
        .bind(user_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    /// Get leaderboard of top chatters in a channel with sorting, pagination, and optional login search.
    pub async fn get_leaderboard(
        &self,
        broadcaster_id: &str,
        since: Option<DateTime<Utc>>,
        sort_by: &str,
        order: &str,
        limit: i64,
        offset: i64,
        search: Option<&str>,
    ) -> DbResult<(Vec<LeaderboardUserItem>, i64)> {
        const QUERY_MSGS_DESC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY message_count DESC, char_count DESC
             OFFSET $4 LIMIT $5";

        const QUERY_MSGS_ASC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY message_count ASC, char_count ASC
             OFFSET $4 LIMIT $5";

        const QUERY_CHARS_DESC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY char_count DESC, message_count DESC
             OFFSET $4 LIMIT $5";

        const QUERY_CHARS_ASC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY char_count ASC, message_count ASC
             OFFSET $4 LIMIT $5";

        const QUERY_ACTIVE_DESC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY last_seen_at DESC, message_count DESC
             OFFSET $4 LIMIT $5";

        const QUERY_ACTIVE_ASC: &str =
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS message_count,
                    COALESCE(SUM(char_count), 0)::BIGINT AS char_count,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)
             GROUP BY chatter_user_id, chatter_user_login
             ORDER BY last_seen_at ASC, message_count ASC
             OFFSET $4 LIMIT $5";

        let asc = order.eq_ignore_ascii_case("asc");
        let search_pattern = search.map(|s| format!("%{}%", s.to_lowercase()));

        let query_sql = match (sort_by, asc) {
            ("characters", false) => QUERY_CHARS_DESC,
            ("characters", true) => QUERY_CHARS_ASC,
            ("last_active", false) => QUERY_ACTIVE_DESC,
            ("last_active", true) => QUERY_ACTIVE_ASC,
            (_, true) => QUERY_MSGS_ASC,
            _ => QUERY_MSGS_DESC,
        };

        let items = sqlx::query_as::<_, LeaderboardUserItem>(query_sql)
            .bind(broadcaster_id)
            .bind(since)
            .bind(&search_pattern)
            .bind(offset)
            .bind(limit)
            .fetch_all(&self.pool)
            .await?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT chatter_user_id)::BIGINT
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND ($2::TIMESTAMPTZ IS NULL OR sent_at >= $2)
               AND ($3::VARCHAR IS NULL OR LOWER(chatter_user_login) LIKE $3)"
        )
        .bind(broadcaster_id)
        .bind(since)
        .bind(&search_pattern)
        .fetch_one(&self.pool)
        .await?;

        Ok((items, total))
    }

    /// Retrieve chat messages for a specific user in a channel with pagination.
    pub async fn get_user_messages(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        since: Option<DateTime<Utc>>,
        limit: i64,
        offset: i64,
    ) -> DbResult<(Vec<ChatMessage>, i64)> {
        let messages = sqlx::query_as::<_, ChatMessage>(
            "SELECT id, message_id, broadcaster_id, chatter_user_id, chatter_user_login, message_text, char_count, sent_at, created_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND chatter_user_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR sent_at >= $3)
             ORDER BY sent_at DESC
             OFFSET $4 LIMIT $5"
        )
        .bind(broadcaster_id)
        .bind(user_id)
        .bind(since)
        .bind(offset)
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        let total: i64 = sqlx::query_scalar(
            "SELECT COUNT(*)::BIGINT
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND chatter_user_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR sent_at >= $3)"
        )
        .bind(broadcaster_id)
        .bind(user_id)
        .bind(since)
        .fetch_one(&self.pool)
        .await?;

        Ok((messages, total))
    }

    /// Retrieve summary profile for a specific user in a channel.
    pub async fn get_user_summary(
        &self,
        broadcaster_id: &str,
        user_id: &str,
        since: Option<DateTime<Utc>>,
    ) -> DbResult<Option<UserChatSummary>> {
        let summary = sqlx::query_as::<_, UserChatSummary>(
            "SELECT chatter_user_id,
                    chatter_user_login,
                    COUNT(*)::BIGINT AS total_messages,
                    COALESCE(SUM(char_count), 0)::BIGINT AS total_chars,
                    MIN(sent_at) AS first_seen_at,
                    MAX(sent_at) AS last_seen_at
             FROM chat_messages
             WHERE broadcaster_id = $1
               AND chatter_user_id = $2
               AND ($3::TIMESTAMPTZ IS NULL OR sent_at >= $3)
             GROUP BY chatter_user_id, chatter_user_login"
        )
        .bind(broadcaster_id)
        .bind(user_id)
        .bind(since)
        .fetch_optional(&self.pool)
        .await?;

        Ok(summary)
    }
}

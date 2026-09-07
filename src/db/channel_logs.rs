use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, QueryBuilder, Postgres};
use utoipa::ToSchema;
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelLogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl ChannelLogLevel {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Debug => "DEBUG",
            Self::Info => "INFO",
            Self::Warn => "WARN",
            Self::Error => "ERROR",
        }
    }

    pub fn from_str_case_insensitive(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" | "WARNING" => Some(Self::Warn),
            "ERROR" => Some(Self::Error),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, ToSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ChannelLogCategory {
    Redemption,
    Reward,
    Market,
    Bot,
    Auth,
    System,
}

impl ChannelLogCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Redemption => "REDEMPTION",
            Self::Reward => "REWARD",
            Self::Market => "MARKET",
            Self::Bot => "BOT",
            Self::Auth => "AUTH",
            Self::System => "SYSTEM",
        }
    }

    pub fn from_str_case_insensitive(s: &str) -> Option<Self> {
        match s.trim().to_uppercase().as_str() {
            "REDEMPTION" => Some(Self::Redemption),
            "REWARD" => Some(Self::Reward),
            "MARKET" => Some(Self::Market),
            "BOT" => Some(Self::Bot),
            "AUTH" => Some(Self::Auth),
            "SYSTEM" => Some(Self::System),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct ChannelLog {
    pub id: i64,
    pub broadcaster_id: String,
    pub level: String,
    pub category: String,
    pub event_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub solution_hint: Option<String>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewChannelLog {
    pub broadcaster_id: String,
    pub level: ChannelLogLevel,
    pub category: ChannelLogCategory,
    pub event_type: String,
    pub message: String,
    pub details: Option<serde_json::Value>,
    pub solution_hint: Option<String>,
}

#[derive(Debug, Clone, FromRow, Serialize, Deserialize, ToSchema)]
pub struct ChannelLogsSummary {
    pub errors_last_24h: i64,
    pub warnings_last_24h: i64,
    pub info_last_24h: i64,
    pub total_last_24h: i64,
}

#[derive(Debug, Clone, Default, Deserialize, ToSchema, utoipa::IntoParams)]
pub struct ListChannelLogsQuery {
    /// Filter by log level (DEBUG, INFO, WARN, ERROR)
    pub level: Option<String>,
    /// Filter by category (REDEMPTION, REWARD, MARKET, BOT, AUTH, SYSTEM)
    pub category: Option<String>,
    /// Search substring within message or event_type
    pub search: Option<String>,
    /// Filter logs starting from timestamp (inclusive)
    pub from: Option<DateTime<Utc>>,
    /// Filter logs until timestamp (inclusive)
    pub to: Option<DateTime<Utc>>,
    /// Pagination offset (default: 0)
    pub offset: Option<i64>,
    /// Pagination limit (default: 50, max: 200)
    pub limit: Option<i64>,
}

impl Db {
    /// Inserts a single channel log record into the database.
    pub async fn insert_channel_log(&self, log: &NewChannelLog) -> DbResult<i64> {
        let row = sqlx::query_scalar::<_, i64>(
            r#"INSERT INTO channel_logs (
                broadcaster_id, level, category, event_type, message, details, solution_hint, created_at
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
            RETURNING id"#
        )
        .bind(&log.broadcaster_id)
        .bind(log.level.as_str())
        .bind(log.category.as_str())
        .bind(&log.event_type)
        .bind(&log.message)
        .bind(&log.details)
        .bind(&log.solution_hint)
        .fetch_one(&self.pool)
        .await?;

        Ok(row)
    }

    /// Inserts multiple channel log records in a single batch.
    pub async fn insert_channel_logs_batch(&self, logs: &[NewChannelLog]) -> DbResult<()> {
        if logs.is_empty() {
            return Ok(());
        }

        let mut query_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "INSERT INTO channel_logs (broadcaster_id, level, category, event_type, message, details, solution_hint, created_at) "
        );

        query_builder.push_values(logs, |mut b, log| {
            b.push_bind(&log.broadcaster_id)
                .push_bind(log.level.as_str())
                .push_bind(log.category.as_str())
                .push_bind(&log.event_type)
                .push_bind(&log.message)
                .push_bind(&log.details)
                .push_bind(&log.solution_hint)
                .push("NOW()");
        });

        let query = query_builder.build();
        query.execute(&self.pool).await?;

        Ok(())
    }

    /// Lists logs for a broadcaster with pagination and filters.
    pub async fn list_channel_logs(
        &self,
        broadcaster_id: &str,
        query: &ListChannelLogsQuery,
    ) -> DbResult<(Vec<ChannelLog>, i64)> {
        let limit = query.limit.unwrap_or(50).clamp(1, 200);
        let offset = query.offset.unwrap_or(0).max(0);

        // First, count total matching rows
        let mut count_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            "SELECT COUNT(*) FROM channel_logs WHERE broadcaster_id = "
        );
        count_builder.push_bind(broadcaster_id);

        if let Some(level) = &query.level {
            if let Some(lvl) = ChannelLogLevel::from_str_case_insensitive(level) {
                count_builder.push(" AND level = ");
                count_builder.push_bind(lvl.as_str());
            }
        }

        if let Some(cat_str) = &query.category {
            if let Some(cat) = ChannelLogCategory::from_str_case_insensitive(cat_str) {
                count_builder.push(" AND category = ");
                count_builder.push_bind(cat.as_str());
            }
        }

        if let Some(search) = &query.search {
            let pattern = format!("%{}%", search.trim());
            count_builder.push(" AND (message ILIKE ");
            count_builder.push_bind(pattern.clone());
            count_builder.push(" OR event_type ILIKE ");
            count_builder.push_bind(pattern);
            count_builder.push(")");
        }

        if let Some(from) = query.from {
            count_builder.push(" AND created_at >= ");
            count_builder.push_bind(from);
        }

        if let Some(to) = query.to {
            count_builder.push(" AND created_at <= ");
            count_builder.push_bind(to);
        }

        let total: i64 = count_builder.build_query_scalar().fetch_one(&self.pool).await?;

        if total == 0 {
            return Ok((Vec::new(), 0));
        }

        // Now select paginated rows
        let mut select_builder: QueryBuilder<Postgres> = QueryBuilder::new(
            r#"SELECT id, broadcaster_id, level, category, event_type, message, details, solution_hint, created_at
               FROM channel_logs
               WHERE broadcaster_id = "#
        );
        select_builder.push_bind(broadcaster_id);

        if let Some(level) = &query.level {
            if let Some(lvl) = ChannelLogLevel::from_str_case_insensitive(level) {
                select_builder.push(" AND level = ");
                select_builder.push_bind(lvl.as_str());
            }
        }

        if let Some(cat_str) = &query.category {
            if let Some(cat) = ChannelLogCategory::from_str_case_insensitive(cat_str) {
                select_builder.push(" AND category = ");
                select_builder.push_bind(cat.as_str());
            }
        }

        if let Some(search) = &query.search {
            let pattern = format!("%{}%", search.trim());
            select_builder.push(" AND (message ILIKE ");
            select_builder.push_bind(pattern.clone());
            select_builder.push(" OR event_type ILIKE ");
            select_builder.push_bind(pattern);
            select_builder.push(")");
        }

        if let Some(from) = query.from {
            select_builder.push(" AND created_at >= ");
            select_builder.push_bind(from);
        }

        if let Some(to) = query.to {
            select_builder.push(" AND created_at <= ");
            select_builder.push_bind(to);
        }

        select_builder.push(" ORDER BY created_at DESC, id DESC LIMIT ");
        select_builder.push_bind(limit);
        select_builder.push(" OFFSET ");
        select_builder.push_bind(offset);

        let logs: Vec<ChannelLog> = select_builder.build_query_as().fetch_all(&self.pool).await?;

        Ok((logs, total))
    }

    /// Fetches aggregated error and warning counts for the last 24 hours.
    pub async fn get_channel_logs_summary(&self, broadcaster_id: &str) -> DbResult<ChannelLogsSummary> {
        let summary = sqlx::query_as::<_, ChannelLogsSummary>(
            r#"
            SELECT
                COALESCE(COUNT(*) FILTER (WHERE level = 'ERROR'), 0)::BIGINT AS errors_last_24h,
                COALESCE(COUNT(*) FILTER (WHERE level = 'WARN'), 0)::BIGINT AS warnings_last_24h,
                COALESCE(COUNT(*) FILTER (WHERE level = 'INFO'), 0)::BIGINT AS info_last_24h,
                COUNT(*)::BIGINT AS total_last_24h
            FROM channel_logs
            WHERE broadcaster_id = $1
              AND created_at >= NOW() - INTERVAL '24 hours'
            "#
        )
        .bind(broadcaster_id)
        .fetch_one(&self.pool)
        .await?;

        Ok(summary)
    }

    /// Deletes old logs according to the retention policy:
    /// - DEBUG and INFO older than `debug_info_days`
    /// - WARN and ERROR older than `warn_error_days`
    pub async fn cleanup_old_channel_logs(
        &self,
        debug_info_days: i32,
        warn_error_days: i32,
    ) -> DbResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM channel_logs
            WHERE (level IN ('DEBUG', 'INFO') AND created_at < NOW() - ($1 || ' days')::INTERVAL)
               OR (level IN ('WARN', 'ERROR') AND created_at < NOW() - ($2 || ' days')::INTERVAL)
            "#
        )
        .bind(debug_info_days.to_string())
        .bind(warn_error_days.to_string())
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_channel_log_level_parsing_and_str() {
        assert_eq!(ChannelLogLevel::Debug.as_str(), "DEBUG");
        assert_eq!(ChannelLogLevel::Info.as_str(), "INFO");
        assert_eq!(ChannelLogLevel::Warn.as_str(), "WARN");
        assert_eq!(ChannelLogLevel::Error.as_str(), "ERROR");

        assert_eq!(ChannelLogLevel::from_str_case_insensitive("debug"), Some(ChannelLogLevel::Debug));
        assert_eq!(ChannelLogLevel::from_str_case_insensitive("INFO"), Some(ChannelLogLevel::Info));
        assert_eq!(ChannelLogLevel::from_str_case_insensitive("warn"), Some(ChannelLogLevel::Warn));
        assert_eq!(ChannelLogLevel::from_str_case_insensitive("warning"), Some(ChannelLogLevel::Warn));
        assert_eq!(ChannelLogLevel::from_str_case_insensitive("error"), Some(ChannelLogLevel::Error));
        assert_eq!(ChannelLogLevel::from_str_case_insensitive("unknown"), None);
    }

    #[test]
    fn test_channel_log_category_parsing_and_str() {
        assert_eq!(ChannelLogCategory::Redemption.as_str(), "REDEMPTION");
        assert_eq!(ChannelLogCategory::Reward.as_str(), "REWARD");
        assert_eq!(ChannelLogCategory::Market.as_str(), "MARKET");
        assert_eq!(ChannelLogCategory::Bot.as_str(), "BOT");
        assert_eq!(ChannelLogCategory::Auth.as_str(), "AUTH");
        assert_eq!(ChannelLogCategory::System.as_str(), "SYSTEM");

        assert_eq!(ChannelLogCategory::from_str_case_insensitive("redemption"), Some(ChannelLogCategory::Redemption));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("reward"), Some(ChannelLogCategory::Reward));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("MARKET"), Some(ChannelLogCategory::Market));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("bot"), Some(ChannelLogCategory::Bot));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("auth"), Some(ChannelLogCategory::Auth));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("system"), Some(ChannelLogCategory::System));
        assert_eq!(ChannelLogCategory::from_str_case_insensitive("invalid"), None);
    }

    #[test]
    fn test_channel_log_serialization() {
        let log = NewChannelLog {
            broadcaster_id: "12345".to_string(),
            level: ChannelLogLevel::Error,
            category: ChannelLogCategory::Market,
            event_type: "MARKET_INSUFFICIENT_BALANCE".to_string(),
            message: "Insufficient market balance".to_string(),
            details: Some(serde_json::json!({ "current_balance": 0 })),
            solution_hint: Some("Deposit funds into your CSGO Market account".to_string()),
        };

        let json = serde_json::to_string(&log).unwrap();
        assert!(json.contains("MARKET_INSUFFICIENT_BALANCE"));
        assert!(json.contains("ERROR"));
        assert!(json.contains("MARKET"));
    }
}

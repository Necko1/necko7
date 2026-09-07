use chrono::{DateTime, Utc};
use sqlx::FromRow;
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct ViewerChannel {
    pub user_id: String,
    pub channel_id: String,
    pub is_pinned: bool,
    pub is_hidden: bool,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, FromRow)]
pub struct ViewerActiveChannelSummaryRow {
    pub channel_id: String,
    pub channel_login: String,
    pub redemptions_count: i64,
    pub messages_count: i64,
}

impl Db {
    /// Pin / add a channel to viewer's list. If it was hidden, unhide it.
    pub async fn pin_viewer_channel(&self, user_id: &str, channel_id: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO viewer_channels (user_id, channel_id, is_pinned, is_hidden, created_at, updated_at)
             VALUES ($1, $2, TRUE, FALSE, NOW(), NOW())
             ON CONFLICT (user_id, channel_id) DO UPDATE SET is_pinned = TRUE, is_hidden = FALSE, updated_at = NOW()"
        )
        .bind(user_id)
        .bind(channel_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Unpin / hide a channel from viewer's list.
    pub async fn unpin_viewer_channel(&self, user_id: &str, channel_id: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO viewer_channels (user_id, channel_id, is_pinned, is_hidden, created_at, updated_at)
             VALUES ($1, $2, FALSE, TRUE, NOW(), NOW())
             ON CONFLICT (user_id, channel_id) DO UPDATE SET is_pinned = FALSE, is_hidden = TRUE, updated_at = NOW()"
        )
        .bind(user_id)
        .bind(channel_id)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    /// Check if viewer has pinned or hidden this channel
    pub async fn get_viewer_channel_preference(&self, user_id: &str, channel_id: &str) -> DbResult<Option<ViewerChannel>> {
        let pref = sqlx::query_as::<_, ViewerChannel>(
            "SELECT user_id, channel_id, is_pinned, is_hidden, created_at, updated_at FROM viewer_channels WHERE user_id = $1 AND channel_id = $2"
        )
        .bind(user_id)
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(pref)
    }

    /// Get all channel IDs accessible to the viewer where they had activity (chat or redemptions) or pinned,
    /// excluding channels they explicitly hid, AND where the broadcaster has enabled public rewards.
    pub async fn get_viewer_accessible_channels(&self, user_id: &str) -> DbResult<Vec<String>> {
        let channel_ids = sqlx::query_scalar::<_, String>(
            r#"
            WITH candidate_channels AS (
                -- Explicitly pinned channels
                SELECT channel_id FROM viewer_channels WHERE user_id = $1 AND is_pinned = TRUE AND is_hidden = FALSE
                UNION
                -- Channels with chat messages from this user
                SELECT DISTINCT broadcaster_id AS channel_id FROM chat_messages WHERE chatter_user_id = $1
                UNION
                -- Channels with redemptions from this user
                SELECT DISTINCT r.streamer_id AS channel_id
                FROM redemptions red
                JOIN rewards r ON r.twitch_id = red.twitch_reward_id
                WHERE red.user_id = $1
            )
            SELECT c.channel_id
            FROM candidate_channels c
            JOIN broadcaster_settings bs ON bs.channel_id = c.channel_id
            LEFT JOIN viewer_channels vc ON vc.user_id = $1 AND vc.channel_id = c.channel_id
            WHERE (vc.is_hidden IS NULL OR vc.is_hidden = FALSE)
              AND (bs.public_rewards_config->>'enabled')::boolean = TRUE
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;
        Ok(channel_ids)
    }

    /// Retrieve active channels summary (message & redemption counts) for a viewer across all channels.
    pub async fn get_viewer_active_channel_summaries(
        &self,
        user_id: &str,
    ) -> DbResult<Vec<ViewerActiveChannelSummaryRow>> {
        let summaries = sqlx::query_as::<_, ViewerActiveChannelSummaryRow>(
            r#"
            WITH user_channels AS (
                SELECT broadcaster_id AS channel_id FROM chat_messages WHERE chatter_user_id = $1
                UNION
                SELECT rew.streamer_id AS channel_id FROM redemptions r JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id WHERE r.user_id = $1
            )
            SELECT
                uc.channel_id,
                COALESCE(b.channel_login, uc.channel_id) AS channel_login,
                (SELECT COUNT(*)::BIGINT FROM redemptions r JOIN rewards rew ON rew.twitch_id = r.twitch_reward_id WHERE rew.streamer_id = uc.channel_id AND r.user_id = $1) AS redemptions_count,
                (SELECT COUNT(*)::BIGINT FROM chat_messages cm WHERE cm.broadcaster_id = uc.channel_id AND cm.chatter_user_id = $1) AS messages_count
            FROM user_channels uc
            LEFT JOIN broadcasters b ON b.channel_id = uc.channel_id
            "#
        )
        .bind(user_id)
        .fetch_all(&self.pool)
        .await?;

        Ok(summaries)
    }
}

use std::sync::Arc;
use axum::extract::{Path, Query, State};
use axum::Json;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use utoipa::{IntoParams, ToSchema};
use uuid::Uuid;

use crate::api::error::ApiError;
use crate::api::extractor::caller_user::CallerUser;
use crate::db::redemptions::{ViewerChannelRedemption, ViewerGlobalRedemption, ViewerRedemptionStats};
use crate::state::AppState;

#[derive(Deserialize, IntoParams)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct ViewerRewardLimitStatus {
    pub twitch_reward_id: Uuid,
    pub reward_title: String,
    pub window_hours: Option<i32>,
    pub max_redemptions: i32,
    pub used_redemptions: i64,
    pub remaining_redemptions: i64,
    pub is_limit_reached: bool,
}

#[derive(Serialize, ToSchema)]
pub struct ViewerChannelChatStats {
    pub total_messages: i64,
    pub total_characters: i64,
    pub first_seen_at: Option<DateTime<Utc>>,
    pub last_seen_at: Option<DateTime<Utc>>,
    pub leaderboard_rank: Option<i64>,
}

#[derive(Serialize, ToSchema)]
pub struct ViewerChannelProfileResponse {
    pub channel_id: String,
    pub channel_login: String,
    pub display_name: Option<String>,
    pub profile_image_url: Option<String>,
    pub redemption_stats: ViewerRedemptionStats,
    pub chat_stats: ViewerChannelChatStats,
    pub limits: Vec<ViewerRewardLimitStatus>,
}

#[derive(Serialize, ToSchema)]
pub struct ViewerGlobalChannelSummary {
    pub channel_id: String,
    pub channel_login: String,
    pub display_name: Option<String>,
    pub profile_image_url: Option<String>,
    pub redemptions_count: i64,
    pub messages_count: i64,
}

#[derive(Serialize, ToSchema)]
pub struct ViewerGlobalProfileResponse {
    pub user_id: String,
    pub redemption_stats: ViewerRedemptionStats,
    pub total_chat_messages: i64,
    pub total_chat_characters: i64,
    pub channels: Vec<ViewerGlobalChannelSummary>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/me/profile",
    tag = "Viewer Profile",
    summary = "Get viewer profile on a specific channel",
    description = "Returns the authenticated viewer's personal statistics, chat stats, and reward limits on a channel.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
    ),
    responses(
        (status = 200, description = "Viewer channel profile", body = ViewerChannelProfileResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Channel not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn get_viewer_channel_profile(
    CallerUser { user_id }: CallerUser,
    Path(channel_id): Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ViewerChannelProfileResponse>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&channel_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Channel '{}' not found", channel_id),
        })?;

    let channel_id = broadcaster.channel_id.clone();
    let twitch_user = state.get_twitch_user_cached(&channel_id).await;
    let (display_name, profile_image_url) = if let Some(ref u) = twitch_user {
        (Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
    } else {
        (None, None)
    };

    let redemption_stats = state.db.get_viewer_redemption_stats_on_channel(&channel_id, &user_id).await?;

    let chat_summary = state.db.get_user_summary(&channel_id, &user_id, None).await?;
    let (total_messages, total_characters, first_seen, last_seen) = if let Some(cs) = chat_summary {
        (cs.total_messages, cs.total_chars, cs.first_seen_at, cs.last_seen_at)
    } else {
        (0, 0, None, None)
    };

    let leaderboard_rank: Option<i64> = if total_messages > 0 {
        state
            .db
            .get_user_chat_leaderboard_rank(&channel_id, &user_id)
            .await
            .unwrap_or(None)
    } else {
        None
    };

    let chat_stats = ViewerChannelChatStats {
        total_messages,
        total_characters,
        first_seen_at: first_seen,
        last_seen_at: last_seen,
        leaderboard_rank,
    };

    // Calculate user limits for rewards on this channel
    let rewards = state.db.get_rewards_by_streamer_id(&channel_id).await?;
    let mut limits = Vec::new();

    for r in rewards {
        if r.is_deleted {
            continue;
        }
        if let Some(ref pl) = r.purchase_limits {
            for rule in &pl.0.user {
                let used = state.db.count_reward_redemptions(
                    r.twitch_id,
                    Some(&user_id),
                    rule.window_hours,
                    None,
                ).await.unwrap_or(0);

                let max = rule.max_redemptions;
                let remaining = (max as i64 - used).max(0);
                limits.push(ViewerRewardLimitStatus {
                    twitch_reward_id: r.twitch_id,
                    reward_title: r.twitch_title.clone(),
                    window_hours: rule.window_hours,
                    max_redemptions: max,
                    used_redemptions: used,
                    remaining_redemptions: remaining,
                    is_limit_reached: used >= max as i64,
                });
            }
        }
    }

    Ok(Json(ViewerChannelProfileResponse {
        channel_id: broadcaster.channel_id,
        channel_login: broadcaster.channel_login,
        display_name,
        profile_image_url,
        redemption_stats,
        chat_stats,
        limits,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/me/redemptions",
    tag = "Viewer Profile",
    summary = "Get viewer's redemption history on a channel",
    description = "Returns a paginated list of redemptions redeemed by the authenticated user on a specific channel.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID"),
        PaginationQuery,
    ),
    responses(
        (status = 200, description = "List of viewer's redemptions on this channel", body = Vec<ViewerChannelRedemption>),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn get_viewer_channel_redemptions(
    CallerUser { user_id }: CallerUser,
    Path(channel_id): Path<String>,
    Query(pagination): Query<PaginationQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ViewerChannelRedemption>>, ApiError> {
    let limit = pagination.limit.unwrap_or(20).clamp(1, 100);
    let offset = pagination.offset.unwrap_or(0).max(0);

    let broadcaster = state.db.resolve_broadcaster(&channel_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Channel '{}' not found", channel_id),
        })?;

    let redemptions = state.db.get_viewer_redemptions_on_channel(&broadcaster.channel_id, &user_id, limit, offset).await?;
    Ok(Json(redemptions))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/profile",
    tag = "Viewer Profile",
    summary = "Get global viewer profile",
    description = "Returns aggregated statistics for the authenticated viewer across all channels where the bot is active.",
    responses(
        (status = 200, description = "Viewer global profile", body = ViewerGlobalProfileResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn get_viewer_global_profile(
    CallerUser { user_id }: CallerUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ViewerGlobalProfileResponse>, ApiError> {
    let redemption_stats = state.db.get_viewer_redemption_stats_global(&user_id).await?;

    let (total_chat_messages, total_chat_characters) = state
        .db
        .get_user_global_chat_stats(&user_id)
        .await
        .unwrap_or((0, 0));

    // Channel summaries for active channels
    let channel_summaries_raw = state
        .db
        .get_viewer_active_channel_summaries(&user_id)
        .await
        .unwrap_or_default();

    let mut channels = Vec::new();
    for summary in channel_summaries_raw {
        let twitch_user = state.get_twitch_user_cached(&summary.channel_id).await;
        let (display_name, profile_image_url) = if let Some(ref u) = twitch_user {
            (Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
        } else {
            (None, None)
        };

        channels.push(ViewerGlobalChannelSummary {
            channel_id: summary.channel_id,
            channel_login: summary.channel_login,
            display_name,
            profile_image_url,
            redemptions_count: summary.redemptions_count,
            messages_count: summary.messages_count,
        });
    }

    channels.sort_by(|a, b| b.redemptions_count.cmp(&a.redemptions_count).then_with(|| b.messages_count.cmp(&a.messages_count)));

    Ok(Json(ViewerGlobalProfileResponse {
        user_id,
        redemption_stats,
        total_chat_messages,
        total_chat_characters,
        channels,
    }))
}

#[utoipa::path(
    get,
    path = "/api/v1/me/redemptions",
    tag = "Viewer Profile",
    summary = "Get global redemption history across all channels",
    description = "Returns a paginated list of all redemptions redeemed by the authenticated user across all channels, including streamer channel ID and login.",
    params(
        PaginationQuery,
    ),
    responses(
        (status = 200, description = "List of all user redemptions with streamer details", body = Vec<ViewerGlobalRedemption>),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn get_viewer_global_redemptions(
    CallerUser { user_id }: CallerUser,
    Query(pagination): Query<PaginationQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<ViewerGlobalRedemption>>, ApiError> {
    let limit = pagination.limit.unwrap_or(20).clamp(1, 100);
    let offset = pagination.offset.unwrap_or(0).max(0);

    let redemptions = state.db.get_viewer_redemptions_global(&user_id, limit, offset).await?;
    Ok(Json(redemptions))
}

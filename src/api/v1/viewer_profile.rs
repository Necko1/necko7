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
use crate::db::inventory::InventoryItem;
use crate::state::AppState;

#[derive(Deserialize, IntoParams)]
pub struct PaginationQuery {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub status: Option<String>,
    pub search: Option<String>,
    pub channel: Option<String>,
}

#[derive(Serialize, ToSchema)]
pub struct InventoryActionResult { pub status: String }

#[derive(Deserialize, IntoParams)]
pub struct AttemptOptions { pub use_saved_link: Option<bool> }

#[utoipa::path(get, path = "/api/v1/me/inventory", tag = "Viewer Profile", responses((status = 200, body = Vec<InventoryItem>)), security(("session_id" = [])))]
pub async fn get_viewer_inventory(
    CallerUser { user_id }: CallerUser,
    Query(pagination): Query<PaginationQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<InventoryItem>>, ApiError> {
    let channel_id = if let Some(ref identifier) = pagination.channel {
        Some(state.db.resolve_broadcaster(identifier).await?.ok_or_else(|| ApiError::NotFound { message: "Channel not found".into() })?.channel_id)
    } else { None };
    Ok(Json(state.db.get_viewer_inventory(&user_id, channel_id.as_deref(), pagination.status.as_deref(), pagination.search.as_deref(), pagination.limit.unwrap_or(50).clamp(1, 100), pagination.offset.unwrap_or(0).max(0)).await?))
}

#[utoipa::path(get, path = "/api/v1/broadcasters/{channel_id}/me/inventory", tag = "Viewer Profile", params(("channel_id" = String, Path)), responses((status = 200, body = Vec<InventoryItem>)), security(("session_id" = [])))]
pub async fn get_viewer_channel_inventory(
    CallerUser { user_id }: CallerUser,
    Path(channel_id): Path<String>,
    Query(pagination): Query<PaginationQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<InventoryItem>>, ApiError> {
    let channel = state.db.resolve_broadcaster(&channel_id).await?.ok_or_else(|| ApiError::NotFound { message: "Channel not found".into() })?;
    Ok(Json(state.db.get_viewer_inventory(&user_id, Some(&channel.channel_id), pagination.status.as_deref(), pagination.search.as_deref(), pagination.limit.unwrap_or(50).clamp(1, 100), pagination.offset.unwrap_or(0).max(0)).await?))
}

#[utoipa::path(get, path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/inventory", tag = "Viewer Profile", params(("channel_id" = String, Path), ("user_id" = String, Path)), responses((status = 200, body = Vec<InventoryItem>)), security(("session_id" = [])))]
pub async fn get_operator_viewer_inventory(
    auth: crate::api::extractor::authorized_channel::AuthorizedChannel,
    Path((channel_id, user_id)): Path<(String, String)>,
    Query(pagination): Query<PaginationQuery>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<InventoryItem>>, ApiError> {
    debug_assert_eq!(channel_id, auth.channel_id);
    Ok(Json(state.db.get_viewer_inventory(&user_id, Some(&auth.channel_id), pagination.status.as_deref(), pagination.search.as_deref(), pagination.limit.unwrap_or(50).clamp(1, 100), pagination.offset.unwrap_or(0).max(0)).await?))
}

fn action_result(outcome: &str) -> Result<Json<InventoryActionResult>, ApiError> {
    if matches!(outcome, "BLOCKED" | "RECONCILIATION_REQUIRED") {
        return Err(ApiError::UnprocessableEntity { message: format!("Inventory action is not safe: {outcome}"), param: "inventory_id".into() });
    }
    Ok(Json(InventoryActionResult { status: outcome.to_string() }))
}

#[utoipa::path(post, path = "/api/v1/me/inventory/{inventory_id}/attempt", tag = "Viewer Profile", params(("inventory_id" = Uuid, Path)), responses((status = 200, body = InventoryActionResult)), security(("session_id" = [])))]
pub async fn request_viewer_inventory_attempt(
    CallerUser { user_id }: CallerUser, Path(inventory_id): Path<Uuid>, Query(options): Query<AttemptOptions>, State(state): State<Arc<AppState>>,
) -> Result<Json<InventoryActionResult>, ApiError> {
    let redemption_id = state.db.viewer_inventory_redemption(inventory_id, &user_id).await?
        .ok_or_else(|| ApiError::NotFound { message: "Inventory item not found".into() })?;
    let core = state.db.get_inventory_core(redemption_id).await?.ok_or_else(|| ApiError::NotFound { message: "Inventory item not found".into() })?;
    if core.4 == "OPERATOR" || core.4 == "LEGACY_REVIEW" {
        return Err(ApiError::Forbidden { message: "This item requires operator review".into() });
    }
    let result = crate::processor::inventory_fulfillment::purchase(&state, redemption_id, true, options.use_saved_link.unwrap_or(false), Some(&user_id)).await
        .map_err(|message| ApiError::Internal { message })?;
    action_result(result)
}

#[utoipa::path(post, path = "/api/v1/me/inventory/{inventory_id}/refund", tag = "Viewer Profile", params(("inventory_id" = Uuid, Path)), responses((status = 200, body = InventoryActionResult)), security(("session_id" = [])))]
pub async fn request_viewer_inventory_refund(
    CallerUser { user_id }: CallerUser, Path(inventory_id): Path<Uuid>, State(state): State<Arc<AppState>>,
) -> Result<Json<InventoryActionResult>, ApiError> {
    let redemption_id = state.db.viewer_inventory_redemption(inventory_id, &user_id).await?
        .ok_or_else(|| ApiError::NotFound { message: "Inventory item not found".into() })?;
    let result = crate::processor::inventory_fulfillment::refund(&state, redemption_id, true, Some(&user_id)).await
        .map_err(|message| ApiError::Internal { message })?;
    action_result(result)
}

#[utoipa::path(post, path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/inventory/{inventory_id}/attempt", tag = "Viewer Profile", params(("channel_id" = String, Path), ("user_id" = String, Path), ("inventory_id" = Uuid, Path)), responses((status = 200, body = InventoryActionResult)), security(("session_id" = [])))]
pub async fn request_operator_inventory_attempt(
    auth: crate::api::extractor::authorized_channel::AuthorizedChannel,
    Path((channel_id, user_id, inventory_id)): Path<(String, String, Uuid)>, Query(options): Query<AttemptOptions>, State(state): State<Arc<AppState>>,
) -> Result<Json<InventoryActionResult>, ApiError> {
    debug_assert_eq!(channel_id, auth.channel_id);
    let redemption_id = state.db.operator_inventory_redemption(inventory_id, &user_id, &auth.channel_id).await?
        .ok_or_else(|| ApiError::NotFound { message: "Inventory item not found".into() })?;
    let result = crate::processor::inventory_fulfillment::purchase(&state, redemption_id, false, options.use_saved_link.unwrap_or(false), Some(&auth.user_id)).await
        .map_err(|message| ApiError::Internal { message })?;
    action_result(result)
}

#[utoipa::path(post, path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/inventory/{inventory_id}/refund", tag = "Viewer Profile", params(("channel_id" = String, Path), ("user_id" = String, Path), ("inventory_id" = Uuid, Path)), responses((status = 200, body = InventoryActionResult)), security(("session_id" = [])))]
pub async fn request_operator_inventory_refund(
    auth: crate::api::extractor::authorized_channel::AuthorizedChannel,
    Path((channel_id, user_id, inventory_id)): Path<(String, String, Uuid)>, State(state): State<Arc<AppState>>,
) -> Result<Json<InventoryActionResult>, ApiError> {
    debug_assert_eq!(channel_id, auth.channel_id);
    let redemption_id = state.db.operator_inventory_redemption(inventory_id, &user_id, &auth.channel_id).await?
        .ok_or_else(|| ApiError::NotFound { message: "Inventory item not found".into() })?;
    let result = crate::processor::inventory_fulfillment::refund(&state, redemption_id, false, Some(&auth.user_id)).await
        .map_err(|message| ApiError::Internal { message })?;
    action_result(result)
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
    build_channel_profile(&channel_id, &user_id, &state).await
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/chat/users/{user_id}/profile",
    tag = "Viewer Profile",
    summary = "Get channel viewer context for an owner or editor",
    params(("channel_id" = String, Path), ("user_id" = String, Path)),
    responses((status = 200, body = ViewerChannelProfileResponse), (status = 403, description = "Owner or editor required")),
    security(("session_id" = []))
)]
pub async fn get_operator_viewer_profile(
    auth: crate::api::extractor::authorized_channel::AuthorizedChannel,
    Path((channel_id, user_id)): Path<(String, String)>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ViewerChannelProfileResponse>, ApiError> {
    debug_assert_eq!(channel_id, auth.channel_id);
    build_channel_profile(&auth.channel_id, &user_id, &state).await
}

async fn build_channel_profile(
    channel_id: &str,
    user_id: &str,
    state: &Arc<AppState>,
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
                ).await?;

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

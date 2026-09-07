pub mod broadcasters;
pub mod permissions;
pub mod proxy;
pub mod rewards;
pub mod redemptions;
pub mod stats;
pub mod users;
pub mod chat_stats;
pub mod channel_logs;

use axum::Router;
use axum::routing::{get, post, delete, put};
use std::sync::Arc;
use utoipa::OpenApi;
use crate::state::AppState;
use crate::api::error::{ErrorBody, ErrorDetail};

#[derive(OpenApi)]
#[openapi(
    paths(
        crate::api::auth::bot_login_redirect,
        crate::api::auth::streamer_login_redirect,
        crate::api::auth::user_login_redirect,
        crate::api::auth::logout,
        users::get_current_user,
        broadcasters::list_broadcasters,
        broadcasters::get_broadcaster_settings,
        broadcasters::update_broadcaster_settings,
        broadcasters::get_broadcaster_chat_messages,
        broadcasters::update_broadcaster_chat_messages,
        broadcasters::get_broadcaster_balance,
        permissions::list_permissions,
        permissions::grant_permission,
        permissions::revoke_permission,
        rewards::list_rewards,
        rewards::create_reward,
        rewards::update_reward,
        rewards::delete_reward,
        rewards::update_reward_price,
        rewards::batch_rewards,
        rewards::preview_filter,
        redemptions::list_redemptions,
        redemptions::retry_redemption,
        redemptions::refund_redemption,
        redemptions::penalty_redemption,
        stats::get_stats,
        proxy::image_proxy,
        chat_stats::get_chat_leaderboard,
        chat_stats::get_channel_chat_dashboard,
        chat_stats::get_channel_chat_messages,
        chat_stats::get_user_chat_stats,
        chat_stats::get_user_chat_summary,
        chat_stats::get_user_chat_messages,
        chat_stats::get_user_redemptions,
        channel_logs::list_channel_logs,
        channel_logs::get_channel_logs_summary,
    ),
    components(schemas(
        crate::api::auth::LogoutResponse,
        users::UserResponse,
        broadcasters::BroadcasterListItem,
        broadcasters::BroadcasterSettingsResponse,
        broadcasters::UpdateBroadcasterSettingsBody,
        broadcasters::ChatMessagesResponse,
        broadcasters::UpdateChatMessagesBody,
        broadcasters::MarketBalanceResponse,
        permissions::PermissionResponse,
        permissions::GrantPermissionBody,
        rewards::RewardResponse,
        rewards::CreateRewardBody,
        rewards::UpdateRewardBody,
        rewards::BatchRewardBody,
        rewards::ListRewardsQuery,
        rewards::PreviewFilterBody,
        rewards::PreviewFilterResponse,
        redemptions::RedemptionResponse,
        redemptions::PaginatedRedemptionsResponse,
        redemptions::ListRedemptionsQuery,
        stats::StatsResponse,
        stats::StatsQuery,
        proxy::ImageProxyParams,
        chat_stats::PaginatedLeaderboardResponse,
        chat_stats::LeaderboardQuery,
        chat_stats::UserChatStatsResponse,
        chat_stats::UserStatsQuery,
        chat_stats::PaginatedUserMessagesResponse,
        chat_stats::UserMessagesQuery,
        chat_stats::PaginatedChannelMessagesResponse,
        chat_stats::ChannelMessagesQuery,
        chat_stats::ChatDashboardQuery,
        crate::db::chat_messages::ChatDashboardData,
        crate::db::chat_messages::ChatDashboardSummary,
        crate::db::chat_messages::ChatTimelinePoint,
        crate::db::chat_messages::ChatTopUserItem,
        chat_stats::UserRedemptionsQuery,
        crate::db::chat_messages::LeaderboardUserItem,
        crate::db::chat_messages::UserChatSummary,
        crate::db::chat_messages::ChatMessage,
        crate::db::rewards::ChatLogicalOperator,
        ErrorBody,
        ErrorDetail,
        crate::db::channel_permissions::ChannelRole,
        crate::db::redemptions::RedemptionStatus,
        crate::db::rewards::PauseReason,
        crate::db::rewards::RewardType,
        crate::db::rewards::PricingMode,
        crate::db::rewards::PriceStrategy,
        crate::db::rewards::FilterConfig,
        crate::db::rewards::PoolItemConfig,
        crate::db::rewards::RewardPurchaseLimitsConfig,
        crate::db::rewards::PurchaseLimitRule,
        crate::steam::market::prices::MarketPriceItem,
        channel_logs::ChannelLogResponse,
        channel_logs::PaginatedChannelLogsResponse,
        channel_logs::ChannelLogsSummaryResponse,
        crate::db::channel_logs::ListChannelLogsQuery,
        crate::db::channel_logs::ChannelLogLevel,
        crate::db::channel_logs::ChannelLogCategory,
    )),
    tags(
        (name = "Auth", description = "Twitch OAuth 2.0 authentication flows and session management"),
        (name = "Users", description = "User profile and session information"),
        (name = "Broadcasters", description = "Broadcaster settings and channel management"),
        (name = "Permissions", description = "Channel access control (owner/editor roles)"),
        (name = "Rewards", description = "Channel point rewards CRUD and batch operations"),
        (name = "Redemptions", description = "Redemption tracking and actions (retry, refund, penalty)"),
        (name = "Stats", description = "Redemption statistics and analytics"),
        (name = "Chat Analytics", description = "Channel chat message analytics, leaderboards, and user activity"),
        (name = "Channel Logs", description = "Broadcaster channel event and error logs with solution recommendations"),
        (name = "Proxy", description = "Image proxy and caching to bypass CORS restrictions"),
    ),
    modifiers(&SecurityAddon),
    security(
        ("session_id" = [])
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        use utoipa::openapi::security::{ApiKey, ApiKeyValue, SecurityScheme};

        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "session_id",
                SecurityScheme::ApiKey(ApiKey::Cookie(ApiKeyValue::new("session_id"))),
            );
        }
    }
}

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/users/me", get(users::get_current_user))
        .route("/broadcasters", get(broadcasters::list_broadcasters))
        .route("/broadcasters/{channel_id}", get(broadcasters::get_broadcaster_settings))
        .route("/broadcasters/{channel_id}/settings", put(broadcasters::update_broadcaster_settings))
        .route("/broadcasters/{channel_id}/messages", get(broadcasters::get_broadcaster_chat_messages).put(broadcasters::update_broadcaster_chat_messages))
        .route("/broadcasters/{channel_id}/market/balance", get(broadcasters::get_broadcaster_balance))
        .route("/broadcasters/{channel_id}/permissions", get(permissions::list_permissions).post(permissions::grant_permission))
        .route("/broadcasters/{channel_id}/permissions/{user_id}", delete(permissions::revoke_permission))
        .route("/broadcasters/{channel_id}/rewards", get(rewards::list_rewards).post(rewards::create_reward))
        .route("/broadcasters/{channel_id}/rewards/preview-filter", post(rewards::preview_filter))
        .route("/broadcasters/{channel_id}/rewards/batch", post(rewards::batch_rewards))
        .route("/broadcasters/{channel_id}/rewards/{reward_id}", put(rewards::update_reward).delete(rewards::delete_reward))
        .route("/broadcasters/{channel_id}/rewards/{reward_id}/update-price", post(rewards::update_reward_price))
        .route("/broadcasters/{channel_id}/redemptions", get(redemptions::list_redemptions))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/retry", post(redemptions::retry_redemption))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/refund", post(redemptions::refund_redemption))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/penalty", post(redemptions::penalty_redemption))
        .route("/broadcasters/{channel_id}/stats", get(stats::get_stats))
        .route("/broadcasters/{channel_id}/chat/leaderboard", get(chat_stats::get_chat_leaderboard))
        .route("/broadcasters/{channel_id}/chat/dashboard", get(chat_stats::get_channel_chat_dashboard))
        .route("/broadcasters/{channel_id}/chat/messages", get(chat_stats::get_channel_chat_messages))
        .route("/broadcasters/{channel_id}/chat/users/{user_id}/stats", get(chat_stats::get_user_chat_stats))
        .route("/broadcasters/{channel_id}/chat/users/{user_id}/summary", get(chat_stats::get_user_chat_summary))
        .route("/broadcasters/{channel_id}/chat/users/{user_id}/messages", get(chat_stats::get_user_chat_messages))
        .route("/broadcasters/{channel_id}/chat/users/{user_id}/redemptions", get(chat_stats::get_user_redemptions))
        .route("/broadcasters/{channel_id}/logs", get(channel_logs::list_channel_logs))
        .route("/broadcasters/{channel_id}/logs/summary", get(channel_logs::get_channel_logs_summary))
        .route("/proxy/image", get(proxy::image_proxy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_openapi_schema_contains_pause_reason() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("PauseReason"), "OpenAPI schema must contain PauseReason");
        assert!(json.contains("MANUAL"), "OpenAPI schema must contain MANUAL");
        assert!(json.contains("NO_MONEY"), "OpenAPI schema must contain NO_MONEY");
        assert!(json.contains("PRICE_LIMIT"), "OpenAPI schema must contain PRICE_LIMIT");
    }

    #[test]
    fn test_openapi_schema_contains_proxy_endpoint() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("/api/v1/proxy/image"), "OpenAPI schema must contain /api/v1/proxy/image");
        assert!(json.contains("ImageProxyParams"), "OpenAPI schema must contain ImageProxyParams");
    }

    #[test]
    fn test_openapi_schema_contains_reward_types_and_filter() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("RewardType"), "OpenAPI schema must contain RewardType");
        assert!(json.contains("PricingMode"), "OpenAPI schema must contain PricingMode");
        assert!(json.contains("PriceStrategy"), "OpenAPI schema must contain PriceStrategy");
        assert!(json.contains("FilterConfig"), "OpenAPI schema must contain FilterConfig");
        assert!(json.contains("PoolItemConfig"), "OpenAPI schema must contain PoolItemConfig");
        assert!(json.contains("PreviewFilterResponse"), "OpenAPI schema must contain PreviewFilterResponse");
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/rewards/preview-filter"), "OpenAPI schema must contain preview-filter endpoint");
    }

    #[test]
    fn test_openapi_schema_contains_chat_analytics() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("ChatLogicalOperator"), "OpenAPI schema must contain ChatLogicalOperator");
        assert!(json.contains("LeaderboardUserItem"), "OpenAPI schema must contain LeaderboardUserItem");
        assert!(json.contains("PaginatedLeaderboardResponse"), "OpenAPI schema must contain PaginatedLeaderboardResponse");
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/chat/leaderboard"), "OpenAPI schema must contain leaderboard route");
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/chat/dashboard"), "OpenAPI schema must contain dashboard route");
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/chat/messages"), "OpenAPI schema must contain messages route");
        assert!(json.contains("ChatDashboardData"), "OpenAPI schema must contain ChatDashboardData");
        assert!(json.contains("PaginatedChannelMessagesResponse"), "OpenAPI schema must contain PaginatedChannelMessagesResponse");
    }

    #[test]
    fn test_openapi_schema_contains_channel_logs() {
        let doc = ApiDoc::openapi();
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/logs"), "OpenAPI schema must contain logs route");
        assert!(json.contains("/api/v1/broadcasters/{channel_id}/logs/summary"), "OpenAPI schema must contain logs summary route");
        assert!(json.contains("ChannelLogResponse"), "OpenAPI schema must contain ChannelLogResponse");
        assert!(json.contains("PaginatedChannelLogsResponse"), "OpenAPI schema must contain PaginatedChannelLogsResponse");
        assert!(json.contains("ChannelLogsSummaryResponse"), "OpenAPI schema must contain ChannelLogsSummaryResponse");
        assert!(json.contains("ChannelLogLevel"), "OpenAPI schema must contain ChannelLogLevel");
        assert!(json.contains("ChannelLogCategory"), "OpenAPI schema must contain ChannelLogCategory");
    }
}

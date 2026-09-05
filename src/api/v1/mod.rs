pub mod broadcasters;
pub mod permissions;
pub mod proxy;
pub mod rewards;
pub mod redemptions;
pub mod stats;
pub mod users;

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
        crate::steam::market::prices::MarketPriceItem,
    )),
    tags(
        (name = "Auth", description = "Twitch OAuth 2.0 authentication flows and session management"),
        (name = "Users", description = "User profile and session information"),
        (name = "Broadcasters", description = "Broadcaster settings and channel management"),
        (name = "Permissions", description = "Channel access control (owner/editor roles)"),
        (name = "Rewards", description = "Channel point rewards CRUD and batch operations"),
        (name = "Redemptions", description = "Redemption tracking and actions (retry, refund, penalty)"),
        (name = "Stats", description = "Redemption statistics and analytics"),
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
        use utoipa::openapi::security::{SecurityScheme, ApiKey};
        let components = openapi.components.get_or_insert_with(Default::default);
        components.add_security_scheme(
            "session_id",
            SecurityScheme::ApiKey(ApiKey::Cookie(utoipa::openapi::security::ApiKeyValue::new("session_id"))),
        );
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
}

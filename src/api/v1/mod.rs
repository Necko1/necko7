pub mod broadcasters;
pub mod permissions;
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
        users::get_current_user,
        broadcasters::list_broadcasters,
        broadcasters::get_broadcaster_settings,
        broadcasters::update_broadcaster_settings,
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
        redemptions::list_redemptions,
        redemptions::retry_redemption,
        redemptions::refund_redemption,
        redemptions::penalty_redemption,
        stats::get_stats,
    ),
    components(schemas(
        users::UserResponse,
        broadcasters::BroadcasterListItem,
        broadcasters::BroadcasterSettingsResponse,
        broadcasters::UpdateBroadcasterSettingsBody,
        broadcasters::MarketBalanceResponse,
        permissions::PermissionResponse,
        permissions::GrantPermissionBody,
        rewards::RewardResponse,
        rewards::CreateRewardBody,
        rewards::UpdateRewardBody,
        rewards::BatchRewardBody,
        rewards::ListRewardsQuery,
        redemptions::RedemptionResponse,
        redemptions::PaginatedRedemptionsResponse,
        redemptions::ListRedemptionsQuery,
        stats::StatsResponse,
        stats::StatsQuery,
        ErrorBody,
        ErrorDetail,
        crate::db::channel_permissions::ChannelRole,
        crate::db::redemptions::RedemptionStatus,
    )),
    tags(
        (name = "Users", description = "User profile and session information"),
        (name = "Broadcasters", description = "Broadcaster settings and channel management"),
        (name = "Permissions", description = "Channel access control (owner/editor roles)"),
        (name = "Rewards", description = "Channel point rewards CRUD and batch operations"),
        (name = "Redemptions", description = "Redemption tracking and actions (retry, refund, penalty)"),
        (name = "Stats", description = "Redemption statistics and analytics"),
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
        .route("/broadcasters/{channel_id}/market/balance", get(broadcasters::get_broadcaster_balance))
        .route("/broadcasters/{channel_id}/permissions", get(permissions::list_permissions).post(permissions::grant_permission))
        .route("/broadcasters/{channel_id}/permissions/{user_id}", delete(permissions::revoke_permission))
        .route("/broadcasters/{channel_id}/rewards", get(rewards::list_rewards).post(rewards::create_reward))
        .route("/broadcasters/{channel_id}/rewards/batch", post(rewards::batch_rewards))
        .route("/broadcasters/{channel_id}/rewards/{reward_id}", put(rewards::update_reward).delete(rewards::delete_reward))
        .route("/broadcasters/{channel_id}/rewards/{reward_id}/update-price", post(rewards::update_reward_price))
        .route("/broadcasters/{channel_id}/redemptions", get(redemptions::list_redemptions))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/retry", post(redemptions::retry_redemption))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/refund", post(redemptions::refund_redemption))
        .route("/broadcasters/{channel_id}/redemptions/{redemption_id}/penalty", post(redemptions::penalty_redemption))
        .route("/broadcasters/{channel_id}/stats", get(stats::get_stats))
}

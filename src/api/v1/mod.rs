pub mod broadcasters;
pub mod permissions;
pub mod rewards;
pub mod redemptions;
pub mod stats;

use axum::Router;
use axum::routing::{get, post, delete, put};
use std::sync::Arc;
use crate::state::AppState;

pub fn router() -> Router<Arc<AppState>> {
    Router::new()
        .route("/broadcasters", get(broadcasters::list_broadcasters))
        .route("/broadcasters/{channel_id}", get(broadcasters::get_broadcaster_settings))
        .route("/broadcasters/{channel_id}/settings", put(broadcasters::update_broadcaster_settings))
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

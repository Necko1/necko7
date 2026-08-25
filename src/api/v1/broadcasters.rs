use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::caller_user::CallerUser;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::channel_permissions::ChannelRole;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct BroadcasterListItem {
    /// Twitch channel ID (numeric string)
    pub channel_id: String,
    /// Twitch channel login name
    pub channel_login: String,
    /// User's role on this channel
    pub role: ChannelRole,
}

#[utoipa::path(
    get,
    path = "/broadcasters",
    tag = "Broadcasters",
    summary = "List user's accessible broadcasters",
    description = "Returns a list of broadcasters that the authenticated user has access to (either as Owner or Editor).",
    responses(
        (status = 200, description = "List of broadcasters the user has access to", body = Vec<BroadcasterListItem>,
            example = json!([
                {
                    "channel_id": "123456789",
                    "channel_login": "some_streamer",
                    "role": "OWNER"
                }
            ])
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 404, description = "App not initialized (bot OAuth not completed)"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn list_broadcasters(
    CallerUser { user_id }: CallerUser,
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<BroadcasterListItem>>, ApiError> {
    let permissions = state.db.get_permissions_by_user(&user_id).await?;

    let mut result = Vec::new();
    for perm in permissions {
        result.push(BroadcasterListItem {
            channel_id: perm.channel_id,
            channel_login: String::new(),
            role: perm.role,
        });
    }

    for item in &mut result {
        if let Ok(Some(b)) = state.db.get_broadcaster_by_id(&item.channel_id).await {
            item.channel_login = b.channel_login;
        }
    }

    Ok(Json(result))
}

#[derive(Serialize, ToSchema)]
pub struct BroadcasterSettingsResponse {
    /// Twitch channel ID
    pub channel_id: String,
    /// Twitch channel login name
    pub channel_login: String,
    /// Whether this broadcaster is actively using the bot
    pub is_active: bool,
    /// Whether a market API key is configured
    pub market_api_key_set: bool,
    /// Market currency used for item purchases (e.g. "USD")
    pub market_currency: String,
    /// Base price multiplier for reward pricing (percentage, e.g. 150 = 150%)
    pub base_price_multiplier: i16,
    /// Period (in seconds) between automatic price updates
    pub update_prices_period: i32,
    /// Automatically refund if buyer fails delivery
    pub refund_on_buyer_fail: bool,
    /// Automatically refund if there's not enough money
    pub refund_if_no_money: bool,
    /// Pause the reward if there's not enough money
    pub pause_reward_if_no_money: bool,
    /// Market chance percentage to transfer item
    pub market_chance_to_transfer: i16,
}

#[utoipa::path(
    get,
    path = "/broadcasters/{channel_id}",
    tag = "Broadcasters",
    summary = "Get broadcaster settings",
    description = "Retrieves the market and bot settings for a specific broadcaster channel.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster"),
    ),
    responses(
        (status = 200, description = "Broadcaster settings retrieved successfully", body = BroadcasterSettingsResponse,
            example = json!({
                "channel_id": "123456789",
                "channel_login": "some_streamer",
                "is_active": true,
                "market_api_key_set": true,
                "market_currency": "USD",
                "base_price_multiplier": 150,
                "update_prices_period": 300,
                "refund_on_buyer_fail": true,
                "refund_if_no_money": false,
                "pause_reward_if_no_money": true,
                "market_chance_to_transfer": 80
            })
        ),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_broadcaster_settings(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
) -> Result<Json<BroadcasterSettingsResponse>, ApiError> {
    let setting = state.db.get_broadcaster_setting(&auth.channel_id).await?;
    let broadcaster = state.db.get_broadcaster_by_id(&auth.channel_id).await?;

    let setting = match setting {
        Some(s) => s,
        None => return Err(ApiError::NotFound {
            message: "Broadcaster settings not found".to_string(),
        }),
    };

    let channel_login = broadcaster.map(|b| b.channel_login).unwrap_or_default();

    Ok(Json(BroadcasterSettingsResponse {
        channel_id: setting.channel_id,
        channel_login,
        is_active: setting.is_active,
        market_api_key_set: !setting.market_api_key.is_empty(),
        market_currency: setting.market_currency,
        base_price_multiplier: setting.base_price_multiplier,
        update_prices_period: setting.update_prices_period,
        refund_on_buyer_fail: setting.refund_on_buyer_fail,
        refund_if_no_money: setting.refund_if_no_money,
        pause_reward_if_no_money: setting.pause_reward_if_no_money,
        market_chance_to_transfer: setting.market_chance_to_transfer,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateBroadcasterSettingsBody {
    /// Whether this broadcaster is actively using the bot
    pub is_active: Option<bool>,
    /// Market API key for item purchases
    pub market_api_key: Option<String>,
    /// Market currency (e.g. "USD", "EUR")
    pub market_currency: Option<String>,
    /// Base price multiplier for reward pricing (percentage)
    pub base_price_multiplier: Option<i16>,
    /// Period (in seconds) between automatic price updates
    pub update_prices_period: Option<i32>,
    /// Automatically refund if buyer fails delivery
    pub refund_on_buyer_fail: Option<bool>,
    /// Automatically refund if there's not enough money
    pub refund_if_no_money: Option<bool>,
    /// Pause the reward if there's not enough money
    pub pause_reward_if_no_money: Option<bool>,
    /// Market chance percentage to transfer item
    pub market_chance_to_transfer: Option<i16>,
}

#[utoipa::path(
    put,
    path = "/broadcasters/{channel_id}/settings",
    tag = "Broadcasters",
    summary = "Update broadcaster settings",
    description = "Updates the market and bot settings for a specific broadcaster channel. Only provided fields are updated.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster"),
    ),
    request_body = UpdateBroadcasterSettingsBody,
    responses(
        (status = 200, description = "Settings updated successfully", body = BroadcasterSettingsResponse,
            example = json!({
                "channel_id": "123456789",
                "channel_login": "some_streamer",
                "is_active": true,
                "market_api_key_set": true,
                "market_currency": "USD",
                "base_price_multiplier": 150,
                "update_prices_period": 300,
                "refund_on_buyer_fail": true,
                "refund_if_no_money": false,
                "pause_reward_if_no_money": true,
                "market_chance_to_transfer": 80
            })
        ),
        (status = 400, description = "Invalid request body (bad parameter name or value)"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 422, description = "Validation error (field type mismatch)"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn update_broadcaster_settings(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    Json(body): Json<UpdateBroadcasterSettingsBody>,
) -> Result<Json<BroadcasterSettingsResponse>, ApiError> {
    let _setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    let patch = crate::db::broadcaster_settings::UpdateBroadcasterSetting {
        is_active: body.is_active,
        market_api_key: body.market_api_key,
        market_currency: body.market_currency,
        base_price_multiplier: body.base_price_multiplier,
        update_prices_period: body.update_prices_period,
        refund_on_buyer_fail: body.refund_on_buyer_fail,
        refund_if_no_money: body.refund_if_no_money,
        pause_reward_if_no_money: body.pause_reward_if_no_money,
        market_chance_to_transfer: body.market_chance_to_transfer,
    };

    state.db.update_broadcaster_setting(&auth.channel_id, &patch).await?;

    let setting = state.db.get_broadcaster_setting(&auth.channel_id).await?.unwrap();
    let broadcaster = state.db.get_broadcaster_by_id(&auth.channel_id).await?;
    let channel_login = broadcaster.map(|b| b.channel_login).unwrap_or_default();

    Ok(Json(BroadcasterSettingsResponse {
        channel_id: setting.channel_id,
        channel_login,
        is_active: setting.is_active,
        market_api_key_set: !setting.market_api_key.is_empty(),
        market_currency: setting.market_currency,
        base_price_multiplier: setting.base_price_multiplier,
        update_prices_period: setting.update_prices_period,
        refund_on_buyer_fail: setting.refund_on_buyer_fail,
        refund_if_no_money: setting.refund_if_no_money,
        pause_reward_if_no_money: setting.pause_reward_if_no_money,
        market_chance_to_transfer: setting.market_chance_to_transfer,
    }))
}

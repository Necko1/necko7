use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use crate::api::error::ApiError;
use crate::api::extractor::caller_user::CallerUser;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::db::channel_permissions::ChannelRole;
use crate::state::AppState;

#[derive(Serialize)]
pub struct BroadcasterListItem {
    pub channel_id: String,
    pub channel_login: String,
    pub role: ChannelRole,
}

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

#[derive(Serialize)]
pub struct BroadcasterSettingsResponse {
    pub channel_id: String,
    pub channel_login: String,
    pub is_active: bool,
    pub market_api_key_set: bool,
    pub market_currency: String,
    pub base_price_multiplier: i16,
    pub update_prices_period: i32,
    pub refund_on_buyer_fail: bool,
    pub refund_if_no_money: bool,
    pub pause_reward_if_no_money: bool,
    pub market_chance_to_transfer: i16,
}

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

#[derive(Deserialize)]
pub struct UpdateBroadcasterSettingsBody {
    pub is_active: Option<bool>,
    pub market_api_key: Option<String>,
    pub market_currency: Option<String>,
    pub base_price_multiplier: Option<i16>,
    pub update_prices_period: Option<i32>,
    pub refund_on_buyer_fail: Option<bool>,
    pub refund_if_no_money: Option<bool>,
    pub pause_reward_if_no_money: Option<bool>,
    pub market_chance_to_transfer: Option<i16>,
}

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

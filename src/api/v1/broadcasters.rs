use std::collections::HashMap;
use std::sync::Arc;
use axum::extract::State;
use axum::Json;
use serde::{Deserialize, Serialize};
use utoipa::ToSchema;
use crate::api::error::ApiError;
use crate::api::extractor::caller_user::CallerUser;
use crate::api::extractor::authorized_channel::AuthorizedChannel;
use crate::api::extractor::json::JsonArg;
use crate::db::channel_permissions::ChannelRole;
use crate::state::AppState;

#[derive(Serialize, ToSchema)]
pub struct BroadcasterListItem {
    /// Twitch channel ID (numeric string)
    pub channel_id: String,
    /// Twitch channel login name
    pub channel_login: String,
    /// Twitch channel display name
    pub display_name: Option<String>,
    /// Twitch channel avatar image URL
    pub profile_image_url: Option<String>,
    /// User's role on this channel
    pub role: ChannelRole,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters",
    tag = "Broadcasters",
    summary = "List user's accessible broadcasters",
    description = "Returns a list of broadcasters that the authenticated user has access to (either as Owner or Editor).",
    responses(
        (status = 200, description = "List of broadcasters the user has access to", body = Vec<BroadcasterListItem>,
            example = json!([
                {
                    "channel_id": "123456789",
                    "channel_login": "some_streamer",
                    "display_name": "Some_Streamer",
                    "profile_image_url": "https://static-cdn.jtvnw.net/jtv_user_pictures/avatar.png",
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
    let mut channel_map = HashMap::new();

    for perm in permissions {
        channel_map.insert(perm.channel_id, perm.role);
    }

    if let Ok(viewer_channels) = state.db.get_viewer_accessible_channels(&user_id).await {
        for ch_id in viewer_channels {
            channel_map.entry(ch_id).or_insert(ChannelRole::Viewer);
        }
    }

    let mut result = Vec::new();
    for (ch_id, role) in channel_map {
        result.push(BroadcasterListItem {
            channel_id: ch_id,
            channel_login: String::new(),
            display_name: None,
            profile_image_url: None,
            role,
        });
    }

    for item in &mut result {
        if let Ok(Some(b)) = state.db.get_broadcaster_by_id(&item.channel_id).await {
            item.channel_login = b.channel_login;
        }
        if let Some(user_info) = state.get_twitch_user_cached(&item.channel_id).await {
            if item.channel_login.is_empty() {
                item.channel_login = user_info.login.clone();
            }
            item.display_name = Some(user_info.display_name.clone());
            item.profile_image_url = Some(user_info.profile_image_url.clone());
        }
    }

    result.sort_by(|a, b| {
        let role_order = |r: &ChannelRole| match r {
            ChannelRole::Owner => 0,
            ChannelRole::Editor => 1,
            ChannelRole::Viewer => 2,
        };
        role_order(&a.role)
            .cmp(&role_order(&b.role))
            .then_with(|| a.channel_login.to_lowercase().cmp(&b.channel_login.to_lowercase()))
    });

    Ok(Json(result))
}

#[derive(Serialize, ToSchema)]
pub struct BroadcasterSettingsResponse {
    /// Twitch channel ID
    pub channel_id: String,
    /// Twitch channel login name
    pub channel_login: String,
    /// Twitch channel display name
    pub display_name: Option<String>,
    /// Twitch channel avatar image URL
    pub profile_image_url: Option<String>,
    /// Whether this broadcaster is actively using the bot
    pub is_active: bool,
    /// Whether a market API key is configured
    pub market_api_key_set: bool,
    /// Base price multiplier for reward pricing (Twitch channel points per 1 major currency unit, e.g. 200 means 200 points per 1 RUB/USD)
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
    /// Whether to add the Twitch chat bot badge to messages (true: send via App Access Token with bot badge, false: send via Bot User Access Token keeping normal user badge)
    pub add_bot_badge: bool,
    /// Public rewards visibility settings for viewers
    pub public_rewards_config: crate::db::broadcaster_settings::PublicRewardsConfig,
    /// Effective Twitch chat message templates for this broadcaster
    pub chat_messages: crate::messages::CategorizedChatMessages,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}",
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
                "display_name": "Some_Streamer",
                "profile_image_url": "https://static-cdn.jtvnw.net/jtv_user_pictures/avatar.png",
                "is_active": true,
                "market_api_key_set": true,
                "base_price_multiplier": 150,
                "update_prices_period": 300,
                "refund_on_buyer_fail": true,
                "refund_if_no_money": false,
                "pause_reward_if_no_money": true,
                "market_chance_to_transfer": 80,
                "add_bot_badge": false,
                "chat_messages": {
                    "trade_created": "@{buyer}, трейд был создан, у тебя есть {remaining} чтобы его принять - {tradeoffer}"
                }
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
    let chat_messages = state.get_channel_chat_messages_merged(&auth.channel_id);

    let twitch_user = state.get_twitch_user_cached(&auth.channel_id).await;
    let (display_name, profile_image_url) = if let Some(ref u) = twitch_user {
        (Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
    } else {
        (None, None)
    };

    let public_rewards_config = setting.public_rewards_config();
    Ok(Json(BroadcasterSettingsResponse {
        channel_id: setting.channel_id,
        channel_login,
        display_name,
        profile_image_url,
        is_active: setting.is_active,
        market_api_key_set: !setting.market_api_key.is_empty(),
        base_price_multiplier: setting.base_price_multiplier,
        update_prices_period: setting.update_prices_period,
        refund_on_buyer_fail: setting.refund_on_buyer_fail,
        refund_if_no_money: setting.refund_if_no_money,
        pause_reward_if_no_money: setting.pause_reward_if_no_money,
        market_chance_to_transfer: setting.market_chance_to_transfer,
        add_bot_badge: setting.add_bot_badge,
        public_rewards_config,
        chat_messages,
    }))
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateBroadcasterSettingsBody {
    /// Whether this broadcaster is actively using the bot
    pub is_active: Option<bool>,
    /// Market API key for item purchases
    pub market_api_key: Option<String>,
    /// Base price multiplier for reward pricing (Twitch channel points per 1 major currency unit, e.g. 200 means 200 points per 1 RUB/USD)
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
    /// Whether to add the Twitch chat bot badge to messages (true: send via App Access Token with bot badge, false: send via Bot User Access Token keeping normal user badge)
    pub add_bot_badge: Option<bool>,
    /// Public rewards visibility configuration for viewers
    pub public_rewards_config: Option<crate::db::broadcaster_settings::PublicRewardsConfig>,
    /// Twitch chat message templates to customize (category -> message_key -> template_text)
    pub chat_messages: Option<HashMap<String, HashMap<String, String>>>,
}

#[utoipa::path(
    put,
    path = "/api/v1/broadcasters/{channel_id}/settings",
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
                "base_price_multiplier": 150,
                "update_prices_period": 300,
                "refund_on_buyer_fail": true,
                "refund_if_no_money": false,
                "pause_reward_if_no_money": true,
                "market_chance_to_transfer": 80,
                "add_bot_badge": false,
                "chat_messages": {
                    "trade_created": "@{buyer}, трейд был создан, у тебя есть {remaining} чтобы его принять - {tradeoffer}"
                }
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
    JsonArg(body): JsonArg<UpdateBroadcasterSettingsBody>,
) -> Result<Json<BroadcasterSettingsResponse>, ApiError> {
    let existing_setting = state.db.get_or_create_broadcaster_setting(&auth.channel_id).await?;

    if let Some(ref msgs) = body.chat_messages {
        state.update_chat_messages_cache(&auth.channel_id, msgs.clone());
    }

    let patch = crate::db::broadcaster_settings::UpdateBroadcasterSetting {
        is_active: body.is_active,
        market_api_key: body.market_api_key,
        base_price_multiplier: body.base_price_multiplier,
        update_prices_period: body.update_prices_period,
        refund_on_buyer_fail: body.refund_on_buyer_fail,
        refund_if_no_money: body.refund_if_no_money,
        pause_reward_if_no_money: body.pause_reward_if_no_money,
        market_chance_to_transfer: body.market_chance_to_transfer,
        chat_messages: body.chat_messages,
        add_bot_badge: body.add_bot_badge,
        public_rewards_config: body.public_rewards_config,
    };

    state.db.update_broadcaster_setting(&auth.channel_id, &patch).await?;

    if let Some(is_active) = patch.is_active {
        if is_active {
            crate::processor::start_broadcaster_tasks(state.clone(), auth.channel_id.clone());
            let state_clone = state.clone();
            let cid_clone = auth.channel_id.clone();
            state.spawn_task(async move {
                if let Err(e) = state_clone.create_eventsub_subscription(&cid_clone).await {
                    tracing::warn!(error = %e, channel_id = %cid_clone, "Failed to re-subscribe EventSub redemption");
                }
                if state_clone.bot_info.read().is_some() {
                    if let Err(e) = state_clone.create_chat_eventsub_subscription(&cid_clone).await {
                        tracing::warn!(error = %e, channel_id = %cid_clone, "Failed to re-subscribe EventSub chat");
                    }
                }
            });
        } else {
            crate::processor::stop_broadcaster_tasks(&state, &auth.channel_id);
        }
    }

    let mut setting_changes: Vec<crate::channel_log::FieldChange> = Vec::new();

    if let Some(is_active) = patch.is_active {
        if is_active != existing_setting.is_active {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "is_active".to_string(),
                old_value: serde_json::json!(existing_setting.is_active),
                new_value: serde_json::json!(is_active),
                summary: format!("is_active: {} -> {}", existing_setting.is_active, is_active),
            });
        }
    }

    if let Some(ref api_key) = patch.market_api_key {
        if api_key != &existing_setting.market_api_key {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "market_api_key".to_string(),
                old_value: serde_json::json!("***"),
                new_value: serde_json::json!("***"),
                summary: "market_api_key updated".to_string(),
            });
        }
    }

    if let Some(mult) = patch.base_price_multiplier {
        if mult != existing_setting.base_price_multiplier {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "base_price_multiplier".to_string(),
                old_value: serde_json::json!(existing_setting.base_price_multiplier),
                new_value: serde_json::json!(mult),
                summary: format!("base_price_multiplier: {} -> {}", existing_setting.base_price_multiplier, mult),
            });
        }
    }

    if let Some(period) = patch.update_prices_period {
        if period != existing_setting.update_prices_period {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "update_prices_period".to_string(),
                old_value: serde_json::json!(existing_setting.update_prices_period),
                new_value: serde_json::json!(period),
                summary: format!("update_prices_period: {}s -> {}s", existing_setting.update_prices_period, period),
            });
        }
    }

    if let Some(ref_buyer) = patch.refund_on_buyer_fail {
        if ref_buyer != existing_setting.refund_on_buyer_fail {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "refund_on_buyer_fail".to_string(),
                old_value: serde_json::json!(existing_setting.refund_on_buyer_fail),
                new_value: serde_json::json!(ref_buyer),
                summary: format!("refund_on_buyer_fail: {} -> {}", existing_setting.refund_on_buyer_fail, ref_buyer),
            });
        }
    }

    if let Some(ref_money) = patch.refund_if_no_money {
        if ref_money != existing_setting.refund_if_no_money {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "refund_if_no_money".to_string(),
                old_value: serde_json::json!(existing_setting.refund_if_no_money),
                new_value: serde_json::json!(ref_money),
                summary: format!("refund_if_no_money: {} -> {}", existing_setting.refund_if_no_money, ref_money),
            });
        }
    }

    if let Some(pause_money) = patch.pause_reward_if_no_money {
        if pause_money != existing_setting.pause_reward_if_no_money {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "pause_reward_if_no_money".to_string(),
                old_value: serde_json::json!(existing_setting.pause_reward_if_no_money),
                new_value: serde_json::json!(pause_money),
                summary: format!("pause_reward_if_no_money: {} -> {}", existing_setting.pause_reward_if_no_money, pause_money),
            });
        }
    }

    if let Some(chance) = patch.market_chance_to_transfer {
        if chance != existing_setting.market_chance_to_transfer {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "market_chance_to_transfer".to_string(),
                old_value: serde_json::json!(existing_setting.market_chance_to_transfer),
                new_value: serde_json::json!(chance),
                summary: format!("market_chance_to_transfer: {}% -> {}%", existing_setting.market_chance_to_transfer, chance),
            });
        }
    }

    if let Some(badge) = patch.add_bot_badge {
        if badge != existing_setting.add_bot_badge {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "add_bot_badge".to_string(),
                old_value: serde_json::json!(existing_setting.add_bot_badge),
                new_value: serde_json::json!(badge),
                summary: format!("add_bot_badge: {} -> {}", existing_setting.add_bot_badge, badge),
            });
        }
    }

    if let Some(ref prc) = patch.public_rewards_config {
        if prc != &existing_setting.public_rewards_config() {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "public_rewards_config".to_string(),
                old_value: serde_json::to_value(&existing_setting.public_rewards_config()).unwrap_or_default(),
                new_value: serde_json::to_value(prc).unwrap_or_default(),
                summary: format!("public_rewards_config: enabled={}", prc.enabled),
            });
        }
    }

    if let Some(ref msgs) = patch.chat_messages {
        if msgs != &existing_setting.parsed_chat_messages() {
            setting_changes.push(crate::channel_log::FieldChange {
                field: "chat_messages".to_string(),
                old_value: serde_json::json!("(previous templates)"),
                new_value: serde_json::json!("(updated templates)"),
                summary: "chat_messages updated".to_string(),
            });
        }
    }

    if !setting_changes.is_empty() {
        state.channel_logger.log_settings_manually_updated(
            &auth.channel_id,
            &auth.user_id,
            &auth.user_login,
            setting_changes,
        );
    }

    tracing::info!(
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        user_login = %auth.user_login,
        is_active = ?patch.is_active,
        base_multiplier = ?patch.base_price_multiplier,
        "Broadcaster settings updated by authorized user"
    );

    let setting = state.db.get_broadcaster_setting(&auth.channel_id).await?.unwrap();
    let broadcaster = state.db.get_broadcaster_by_id(&auth.channel_id).await?;
    let channel_login = broadcaster.map(|b| b.channel_login).unwrap_or_default();
    let chat_messages = state.get_channel_chat_messages_merged(&auth.channel_id);
    let twitch_user = state.get_twitch_user_cached(&auth.channel_id).await;
    let (display_name, profile_image_url) = if let Some(ref u) = twitch_user {
        (Some(u.display_name.clone()), Some(u.profile_image_url.clone()))
    } else {
        (None, None)
    };

    let public_rewards_config = setting.public_rewards_config();
    Ok(Json(BroadcasterSettingsResponse {
        channel_id: setting.channel_id,
        channel_login,
        display_name,
        profile_image_url,
        is_active: setting.is_active,
        market_api_key_set: !setting.market_api_key.is_empty(),
        base_price_multiplier: setting.base_price_multiplier,
        update_prices_period: setting.update_prices_period,
        refund_on_buyer_fail: setting.refund_on_buyer_fail,
        refund_if_no_money: setting.refund_if_no_money,
        pause_reward_if_no_money: setting.pause_reward_if_no_money,
        market_chance_to_transfer: setting.market_chance_to_transfer,
        add_bot_badge: setting.add_bot_badge,
        public_rewards_config,
        chat_messages,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct ChatMessagesResponse {
    /// Twitch channel ID
    pub channel_id: String,
    /// Effective templates currently in use (custom overrides + defaults for unset)
    pub messages: crate::messages::CategorizedChatMessages,
    /// Custom overrides saved for this channel
    pub custom_messages: HashMap<String, HashMap<String, String>>,
    /// Global default templates
    pub default_messages: crate::messages::CategorizedChatMessages,
    /// Supported placeholders for each message ID grouped by category
    pub placeholders: crate::messages::CategorizedPlaceholders,
}

#[derive(Deserialize, ToSchema)]
pub struct UpdateChatMessagesBody {
    /// Map of category -> { message_key: template_text }
    pub messages: HashMap<String, HashMap<String, String>>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/messages",
    tag = "Broadcasters",
    summary = "Get broadcaster chat message templates",
    description = "Returns the broadcaster's Twitch chat message templates grouped into 5 categories, including effective templates, custom overrides, defaults, and supported placeholders.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster"),
    ),
    responses(
        (status = 200, description = "Chat message templates retrieved successfully", body = ChatMessagesResponse),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_broadcaster_chat_messages(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
) -> Result<Json<ChatMessagesResponse>, ApiError> {
    let effective = state.get_channel_chat_messages_merged(&auth.channel_id);
    let custom = state.get_channel_custom_chat_messages(&auth.channel_id);
    let defaults = crate::messages::CategorizedChatMessages::default();
    let placeholders = crate::messages::CategorizedChatMessages::all_placeholders();

    Ok(Json(ChatMessagesResponse {
        channel_id: auth.channel_id,
        messages: effective,
        custom_messages: custom,
        default_messages: defaults,
        placeholders,
    }))
}

#[utoipa::path(
    put,
    path = "/api/v1/broadcasters/{channel_id}/messages",
    tag = "Broadcasters",
    summary = "Update broadcaster chat message templates",
    description = "Updates the broadcaster's custom Twitch chat message templates in DB and in-memory cache.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster"),
    ),
    request_body = UpdateChatMessagesBody,
    responses(
        (status = 200, description = "Chat message templates updated successfully", body = ChatMessagesResponse),
        (status = 400, description = "Invalid request body"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn update_broadcaster_chat_messages(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
    JsonArg(body): JsonArg<UpdateChatMessagesBody>,
) -> Result<Json<ChatMessagesResponse>, ApiError> {
    state.db.update_broadcaster_chat_messages(&auth.channel_id, &body.messages).await?;
    state.update_chat_messages_cache(&auth.channel_id, body.messages);

    state.channel_logger.log_chat_messages_manually_updated(
        &auth.channel_id,
        &auth.user_id,
        &auth.user_login,
    );

    tracing::info!(
        channel_id = %auth.channel_id,
        user_id = %auth.user_id,
        user_login = %auth.user_login,
        "Broadcaster chat messages updated by authorized user"
    );

    let effective = state.get_channel_chat_messages_merged(&auth.channel_id);
    let custom = state.get_channel_custom_chat_messages(&auth.channel_id);
    let defaults = crate::messages::CategorizedChatMessages::default();
    let placeholders = crate::messages::CategorizedChatMessages::all_placeholders();

    Ok(Json(ChatMessagesResponse {
        channel_id: auth.channel_id,
        messages: effective,
        custom_messages: custom,
        default_messages: defaults,
        placeholders,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct MarketBalanceResponse {
    /// Available market balance
    pub money: f64,
    /// Balance in settlement (hold)
    pub money_settlement: f64,
    /// Currency code (e.g. "RUB", "USD")
    pub currency: String,
    /// Last updated timestamp
    pub updated_at: chrono::DateTime<chrono::Utc>,
}

#[utoipa::path(
    get,
    path = "/api/v1/broadcasters/{channel_id}/market/balance",
    tag = "Broadcasters",
    summary = "Get broadcaster market balance",
    description = "Returns the broadcaster's CS:GO market balance (cached, refreshed automatically or when stale).",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster"),
    ),
    responses(
        (status = 200, description = "Market balance retrieved successfully", body = MarketBalanceResponse,
            example = json!({
                "money": 1520.50,
                "money_settlement": 0.0,
                "currency": "RUB",
                "updated_at": "2026-01-15T12:00:00Z"
            })
        ),
        (status = 400, description = "Market API key is not configured or market API error"),
        (status = 401, description = "Unauthorized — missing or invalid session cookie"),
        (status = 403, description = "Forbidden — no access to this channel"),
        (status = 404, description = "Broadcaster settings not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(
        ("session_id" = [])
    )
)]
pub async fn get_broadcaster_balance(
    auth: AuthorizedChannel,
    State(state): State<Arc<AppState>>,
) -> Result<Json<MarketBalanceResponse>, ApiError> {
    let balance = state.get_cached_or_fetch_balance(&auth.channel_id).await
        .map_err(|e| {
            tracing::warn!(error = %e, channel_id = %auth.channel_id, "Failed to retrieve market balance");
            ApiError::BadRequest {
                message: format!("Failed to retrieve market balance: {}", e),
                param: "market_api_key".to_string(),
            }
        })?;

    Ok(Json(MarketBalanceResponse {
        money: balance.money,
        money_settlement: balance.money_settlement,
        currency: balance.currency,
        updated_at: balance.updated_at,
    }))
}

#[derive(Serialize, ToSchema)]
pub struct PinBroadcasterResponse {
    pub success: bool,
    pub message: String,
}

#[utoipa::path(
    post,
    path = "/api/v1/broadcasters/{channel_id}/pin",
    tag = "Broadcasters",
    summary = "Pin broadcaster to user list",
    description = "Pins/saves a broadcaster to the authenticated viewer's channel list.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster to pin"),
    ),
    responses(
        (status = 200, description = "Broadcaster pinned successfully", body = PinBroadcasterResponse),
        (status = 401, description = "Unauthorized"),
        (status = 404, description = "Broadcaster not found"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn pin_broadcaster(
    CallerUser { user_id }: CallerUser,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PinBroadcasterResponse>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&channel_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Broadcaster '{}' not found", channel_id),
        })?;
    state.db.pin_viewer_channel(&user_id, &broadcaster.channel_id).await?;
    Ok(Json(PinBroadcasterResponse {
        success: true,
        message: "Channel pinned successfully".to_string(),
    }))
}

#[utoipa::path(
    delete,
    path = "/api/v1/broadcasters/{channel_id}/pin",
    tag = "Broadcasters",
    summary = "Unpin/hide broadcaster from user list",
    description = "Removes/hides a broadcaster from the authenticated viewer's channel list.",
    params(
        ("channel_id" = String, Path, description = "Twitch channel ID of the broadcaster to unpin"),
    ),
    responses(
        (status = 200, description = "Broadcaster unpinned successfully", body = PinBroadcasterResponse),
        (status = 401, description = "Unauthorized"),
        (status = 500, description = "Internal server error"),
    ),
    security(("session_id" = []))
)]
pub async fn unpin_broadcaster(
    CallerUser { user_id }: CallerUser,
    axum::extract::Path(channel_id): axum::extract::Path<String>,
    State(state): State<Arc<AppState>>,
) -> Result<Json<PinBroadcasterResponse>, ApiError> {
    let broadcaster = state.db.resolve_broadcaster(&channel_id).await?
        .ok_or_else(|| ApiError::NotFound {
            message: format!("Broadcaster '{}' not found", channel_id),
        })?;
    state.db.unpin_viewer_channel(&user_id, &broadcaster.channel_id).await?;
    Ok(Json(PinBroadcasterResponse {
        success: true,
        message: "Channel unpinned/hidden successfully".to_string(),
    }))
}

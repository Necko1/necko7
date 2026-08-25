use sqlx::FromRow;
use chrono::{DateTime, Utc};
use crate::db::error::DbResult;
use super::Db;

#[derive(Debug, Clone, FromRow)]
pub struct BroadcasterSetting {
    pub channel_id: String,
    pub is_active: bool,
    pub market_api_key: String,
    pub market_currency: String,
    pub base_price_multiplier: i16,
    pub update_prices_period: i32,
    pub refund_on_buyer_fail: bool,
    pub refund_if_no_money: bool,
    pub pause_reward_if_no_money: bool,
    pub market_chance_to_transfer: i16,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
pub struct NewBroadcasterSetting {
    pub channel_id: String,
    pub is_active: bool,
    pub market_api_key: String,
    pub market_currency: String,
    pub base_price_multiplier: i16,
    pub update_prices_period: i32,
    pub refund_on_buyer_fail: bool,
    pub refund_if_no_money: bool,
    pub pause_reward_if_no_money: bool,
    pub market_chance_to_transfer: i16,
}

#[derive(Debug, Clone, Default)]
pub struct UpdateBroadcasterSetting {
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

impl Db {
    pub async fn get_broadcaster_setting(&self, channel_id: &str) -> DbResult<Option<BroadcasterSetting>> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "SELECT channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, refund_if_no_money, pause_reward_if_no_money, market_chance_to_transfer, updated_at FROM broadcaster_settings WHERE channel_id = $1"
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn create_broadcaster_setting(&self, new: &NewBroadcasterSetting) -> DbResult<BroadcasterSetting> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "INSERT INTO broadcaster_settings (channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, refund_if_no_money, pause_reward_if_no_money, market_chance_to_transfer, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) RETURNING channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, refund_if_no_money, pause_reward_if_no_money, market_chance_to_transfer, updated_at"
        )
        .bind(&new.channel_id)
        .bind(new.is_active)
        .bind(&new.market_api_key)
        .bind(&new.market_currency)
        .bind(new.base_price_multiplier)
        .bind(new.update_prices_period)
        .bind(new.refund_on_buyer_fail)
        .bind(new.refund_if_no_money)
        .bind(new.pause_reward_if_no_money)
        .bind(new.market_chance_to_transfer)
        .fetch_one(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn upsert_broadcaster_setting(&self, new: &NewBroadcasterSetting) -> DbResult<BroadcasterSetting> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "INSERT INTO broadcaster_settings (channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, refund_if_no_money, pause_reward_if_no_money, market_chance_to_transfer, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) ON CONFLICT (channel_id) DO UPDATE SET is_active = EXCLUDED.is_active, market_api_key = EXCLUDED.market_api_key, market_currency = EXCLUDED.market_currency, base_price_multiplier = EXCLUDED.base_price_multiplier, update_prices_period = EXCLUDED.update_prices_period, refund_on_buyer_fail = EXCLUDED.refund_on_buyer_fail, refund_if_no_money = EXCLUDED.refund_if_no_money, pause_reward_if_no_money = EXCLUDED.pause_reward_if_no_money, market_chance_to_transfer = EXCLUDED.market_chance_to_transfer, updated_at = NOW() RETURNING channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, refund_if_no_money, pause_reward_if_no_money, market_chance_to_transfer, updated_at"
        )
        .bind(&new.channel_id)
        .bind(new.is_active)
        .bind(&new.market_api_key)
        .bind(&new.market_currency)
        .bind(new.base_price_multiplier)
        .bind(new.update_prices_period)
        .bind(new.refund_on_buyer_fail)
        .bind(new.refund_if_no_money)
        .bind(new.pause_reward_if_no_money)
        .bind(new.market_chance_to_transfer)
        .fetch_one(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn update_broadcaster_setting(&self, channel_id: &str, patch: &UpdateBroadcasterSetting) -> DbResult<()> {
        sqlx::query(
            "UPDATE broadcaster_settings SET is_active = COALESCE($2, is_active), market_api_key = COALESCE($3, market_api_key), market_currency = COALESCE($4, market_currency), base_price_multiplier = COALESCE($5, base_price_multiplier), update_prices_period = COALESCE($6, update_prices_period), refund_on_buyer_fail = COALESCE($7, refund_on_buyer_fail), refund_if_no_money = COALESCE($8, refund_if_no_money), pause_reward_if_no_money = COALESCE($9, pause_reward_if_no_money), market_chance_to_transfer = COALESCE($10, market_chance_to_transfer), updated_at = NOW() WHERE channel_id = $1"
        )
        .bind(channel_id)
        .bind(patch.is_active)
        .bind(&patch.market_api_key)
        .bind(&patch.market_currency)
        .bind(patch.base_price_multiplier)
        .bind(patch.update_prices_period)
        .bind(patch.refund_on_buyer_fail)
        .bind(patch.refund_if_no_money)
        .bind(patch.pause_reward_if_no_money)
        .bind(patch.market_chance_to_transfer)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_broadcaster_setting(&self, channel_id: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM broadcaster_settings WHERE channel_id = $1")
            .bind(channel_id)
            .execute(&self.pool)
            .await?;
        Ok(())
    }

    pub async fn get_or_create_broadcaster_setting(&self, channel_id: &str) -> DbResult<BroadcasterSetting> {
        if let Some(setting) = self.get_broadcaster_setting(channel_id).await? {
            return Ok(setting);
        }

        let new = NewBroadcasterSetting {
            channel_id: channel_id.to_string(),
            is_active: true,
            market_api_key: String::new(),
            market_currency: "RUB".to_string(),
            base_price_multiplier: 200,
            update_prices_period: 3600,
            refund_on_buyer_fail: false,
            refund_if_no_money: false,
            pause_reward_if_no_money: false,
            market_chance_to_transfer: 0,
        };

        self.create_broadcaster_setting(&new).await
    }
}

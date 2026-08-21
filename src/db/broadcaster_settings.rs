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
}

#[derive(Debug, Clone, Default)]
pub struct UpdateBroadcasterSetting {
    pub is_active: Option<bool>,
    pub market_api_key: Option<String>,
    pub market_currency: Option<String>,
    pub base_price_multiplier: Option<i16>,
    pub update_prices_period: Option<i32>,
    pub refund_on_buyer_fail: Option<bool>,
}

impl Db {
    pub async fn get_broadcaster_setting(&self, channel_id: &str) -> DbResult<Option<BroadcasterSetting>> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "SELECT channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, updated_at FROM broadcaster_settings WHERE channel_id = $1"
        )
        .bind(channel_id)
        .fetch_optional(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn create_broadcaster_setting(&self, new: &NewBroadcasterSetting) -> DbResult<BroadcasterSetting> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "INSERT INTO broadcaster_settings (channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) RETURNING channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, updated_at"
        )
        .bind(&new.channel_id)
        .bind(new.is_active)
        .bind(&new.market_api_key)
        .bind(&new.market_currency)
        .bind(new.base_price_multiplier)
        .bind(new.update_prices_period)
        .bind(new.refund_on_buyer_fail)
        .fetch_one(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn upsert_broadcaster_setting(&self, new: &NewBroadcasterSetting) -> DbResult<BroadcasterSetting> {
        let setting = sqlx::query_as::<_, BroadcasterSetting>(
            "INSERT INTO broadcaster_settings (channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, updated_at) VALUES ($1, $2, $3, $4, $5, $6, $7, NOW()) ON CONFLICT (channel_id) DO UPDATE SET is_active = EXCLUDED.is_active, market_api_key = EXCLUDED.market_api_key, market_currency = EXCLUDED.market_currency, base_price_multiplier = EXCLUDED.base_price_multiplier, update_prices_period = EXCLUDED.update_prices_period, refund_on_buyer_fail = EXCLUDED.refund_on_buyer_fail, updated_at = NOW() RETURNING channel_id, is_active, market_api_key, market_currency, base_price_multiplier, update_prices_period, refund_on_buyer_fail, updated_at"
        )
        .bind(&new.channel_id)
        .bind(new.is_active)
        .bind(&new.market_api_key)
        .bind(&new.market_currency)
        .bind(new.base_price_multiplier)
        .bind(new.update_prices_period)
        .bind(new.refund_on_buyer_fail)
        .fetch_one(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn update_broadcaster_setting(&self, channel_id: &str, patch: &UpdateBroadcasterSetting) -> DbResult<()> {
        sqlx::query(
            "UPDATE broadcaster_settings SET is_active = COALESCE($2, is_active), market_api_key = COALESCE($3, market_api_key), market_currency = COALESCE($4, market_currency), base_price_multiplier = COALESCE($5, base_price_multiplier), update_prices_period = COALESCE($6, update_prices_period), refund_on_buyer_fail = COALESCE($7, refund_on_buyer_fail), updated_at = NOW() WHERE channel_id = $1"
        )
        .bind(channel_id)
        .bind(patch.is_active)
        .bind(&patch.market_api_key)
        .bind(&patch.market_currency)
        .bind(patch.base_price_multiplier)
        .bind(patch.update_prices_period)
        .bind(patch.refund_on_buyer_fail)
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
}

use sqlx::FromRow;
use crate::db::error::DbResult;
use super::Db;

pub const KEY_APP_TOKEN: &str = "app_access_token";
pub const KEY_BOT_AUTH: &str = "bot_auth";

#[derive(Debug, Clone, FromRow)]
pub struct AppSetting {
    pub key: String,
    pub value: String,
}

impl Db {
    pub async fn get_setting(&self, key: &str) -> DbResult<Option<String>> {
        let setting = sqlx::query_scalar::<_, String>(
            "SELECT value FROM app_settings WHERE key = $1"
        )
        .bind(key)
        .fetch_optional(&self.pool)
        .await?;
        Ok(setting)
    }

    pub async fn get_all_settings(&self) -> DbResult<Vec<AppSetting>> {
        let settings = sqlx::query_as::<_, AppSetting>(
            "SELECT key, value FROM app_settings"
        )
        .fetch_all(&self.pool)
        .await?;
        Ok(settings)
    }

    pub async fn set_setting(&self, key: &str, value: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO app_settings (key, value) VALUES ($1, $2) ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value"
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await?;
        Ok(())
    }

    pub async fn delete_setting(&self, key: &str) -> DbResult<()> {
        sqlx::query("DELETE FROM app_settings WHERE key = $1")
            .bind(key)
            .execute(&self.pool)
            .await?;
        Ok(())
    }
}

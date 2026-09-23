use crate::messages::{
    MSG_MARKET_ERR_BOT_BANNED, MSG_MARKET_ERR_INVENTORY_FULL, MSG_MARKET_ERR_INVENTORY_HIDDEN,
    MSG_MARKET_ERR_NO_MOBILE_AUTH, MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED,
    MSG_MARKET_ERR_STEAM_BANNED, MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED,
    MSG_MARKET_ERR_TRADE_LINK_INVALID, MSG_MARKET_ERR_UNKNOWN,
};

/// Classification of errors returned by Market.csgo.com `buy-for` API.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketBuyForErrorKind {
    /// Code 2: Unknown market error
    Unknown,
    /// Code 3: Failed to check trade link
    TradeLinkCheckFailed,
    /// Code 5: Recipient inventory is hidden / private
    InventoryHidden,
    /// Code 6: User is banned in Steam
    SteamBanned,
    /// Code 7: Mobile authenticator not bound
    NoMobileAuth,
    /// Code 8: Offline trades disabled
    OfflineTradesDisabled,
    /// Code 12: Invalid trade link
    InvalidTradeLink,
    /// Code 20: Market bot for checking trade links is banned
    CheckBotBanned,
    /// Code 21: Recipient Steam inventory is full
    InventoryFull,
    /// Code 0 / text: Bot account has insufficient funds
    NotEnoughFunds,
    /// Code 0 / text: No item found at the specified chance/price
    PriceOrChanceDeviation,
    /// Unrecognized market error
    Other,
}

impl MarketBuyForErrorKind {
    /// Returns true if this error is caused by the buyer's account or trade link,
    /// meaning retrying further items in a filter pool will also fail.
    pub fn is_buyer_terminal_error(&self) -> bool {
        matches!(
            self,
            MarketBuyForErrorKind::InventoryHidden
                | MarketBuyForErrorKind::InventoryFull
                | MarketBuyForErrorKind::SteamBanned
                | MarketBuyForErrorKind::NoMobileAuth
                | MarketBuyForErrorKind::OfflineTradesDisabled
                | MarketBuyForErrorKind::InvalidTradeLink
                | MarketBuyForErrorKind::TradeLinkCheckFailed
                | MarketBuyForErrorKind::CheckBotBanned
        )
    }

    /// Returns the corresponding message template key, or None if it's funds/price deviation/generic.
    pub fn to_market_error_message_key(&self) -> Option<&'static str> {
        match self {
            MarketBuyForErrorKind::Unknown => Some(MSG_MARKET_ERR_UNKNOWN),
            MarketBuyForErrorKind::TradeLinkCheckFailed => Some(MSG_MARKET_ERR_TRADE_LINK_CHECK_FAILED),
            MarketBuyForErrorKind::InventoryHidden => Some(MSG_MARKET_ERR_INVENTORY_HIDDEN),
            MarketBuyForErrorKind::SteamBanned => Some(MSG_MARKET_ERR_STEAM_BANNED),
            MarketBuyForErrorKind::NoMobileAuth => Some(MSG_MARKET_ERR_NO_MOBILE_AUTH),
            MarketBuyForErrorKind::OfflineTradesDisabled => Some(MSG_MARKET_ERR_OFFLINE_TRADES_DISABLED),
            MarketBuyForErrorKind::InvalidTradeLink => Some(MSG_MARKET_ERR_TRADE_LINK_INVALID),
            MarketBuyForErrorKind::CheckBotBanned => Some(MSG_MARKET_ERR_BOT_BANNED),
            MarketBuyForErrorKind::InventoryFull => Some(MSG_MARKET_ERR_INVENTORY_FULL),
            _ => None,
        }
    }
}

/// Classifies Market.csgo.com `buy-for` error using both the numeric error code
/// and error message strings in both Russian and English.
pub fn classify_market_buy_for_error(code: u32, raw_error: &str) -> MarketBuyForErrorKind {
    let lower = raw_error.to_lowercase();

    // Check balance first (can be code 0 or other)
    if lower.contains("not enough funds") || lower.contains("недостаточно средств") {
        return MarketBuyForErrorKind::NotEnoughFunds;
    }

    // Check price or transfer chance deviation (typically code 0)
    if lower.contains("chance to transfer")
        || lower.contains("price or below")
        || lower.contains("шансом на передачу")
        || lower.contains("по указанной цене или ниже")
    {
        return MarketBuyForErrorKind::PriceOrChanceDeviation;
    }

    // Code 5: Inventory hidden (both docs phrasing and actual live API phrasing)
    if code == 5
        || lower.contains("inventory hidden")
        || lower.contains("inventory is hidden")
        || lower.contains("инвентарь получателя предмета скрыт")
        || lower.contains("инвентарь скрыт")
        || lower.contains("открыть инвентарь")
    {
        return MarketBuyForErrorKind::InventoryHidden;
    }

    // Code 21: Inventory full
    if code == 21
        || lower.contains("inventory is full")
        || lower.contains("inventory full")
        || lower.contains("инвентарь получателя предмета переполнен")
        || lower.contains("инвентарь переполнен")
    {
        return MarketBuyForErrorKind::InventoryFull;
    }

    // Code 6: Steam banned
    if code == 6
        || lower.contains("banned in steam")
        || lower.contains("user is banned")
        || lower.contains("забанен в стиме")
        || lower.contains("забанен в steam")
    {
        return MarketBuyForErrorKind::SteamBanned;
    }

    // Code 7: No mobile authenticator
    if code == 7
        || lower.contains("mobile authenticator")
        || lower.contains("мобильный аутентификатор")
        || lower.contains("мобильного аутентификатора")
    {
        return MarketBuyForErrorKind::NoMobileAuth;
    }

    // Code 8: Offline trades disabled
    if code == 8
        || lower.contains("offline trade")
        || lower.contains("offline trades")
        || lower.contains("оффлайн трейд")
        || lower.contains("оффлайн-трейд")
    {
        return MarketBuyForErrorKind::OfflineTradesDisabled;
    }

    // Code 12: Invalid trade link
    if code == 12
        || lower.contains("invalid trade link")
        || lower.contains("неверная трейд ссылка")
        || lower.contains("неверная трейд-ссылка")
    {
        return MarketBuyForErrorKind::InvalidTradeLink;
    }

    // Code 3: Failed to check trade link
    if code == 3
        || lower.contains("failed to check the trade link")
        || lower.contains("failed to check trade link")
        || lower.contains("не удалось проверить трейд ссылку")
        || lower.contains("не удалось проверить трейд-ссылку")
    {
        return MarketBuyForErrorKind::TradeLinkCheckFailed;
    }

    // Code 20: Check bot banned
    if code == 20
        || lower.contains("bot for checking trade links is banned")
        || lower.contains("проверки трейд ссылки забанен")
        || lower.contains("проверки трейд-ссылки забанен")
    {
        return MarketBuyForErrorKind::CheckBotBanned;
    }

    // Code 2: Unknown error
    if code == 2
        || lower.contains("unknown error has occurred")
        || lower.contains("произошла неизвестная ошибка")
    {
        return MarketBuyForErrorKind::Unknown;
    }

    MarketBuyForErrorKind::Other
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_classify_russian_and_english_errors() {
        // Code 2
        assert_eq!(
            classify_market_buy_for_error(2, "An unknown error has occurred. Try again."),
            MarketBuyForErrorKind::Unknown
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Произошла неизвестная ошибка. Попробуйте еще раз."),
            MarketBuyForErrorKind::Unknown
        );

        // Code 3
        assert_eq!(
            classify_market_buy_for_error(3, "Failed to check the trade link"),
            MarketBuyForErrorKind::TradeLinkCheckFailed
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Не удалось проверить трейд ссылку"),
            MarketBuyForErrorKind::TradeLinkCheckFailed
        );

        // Code 5
        assert_eq!(
            classify_market_buy_for_error(5, "The recipient of the item has their inventory hidden"),
            MarketBuyForErrorKind::InventoryHidden
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Инвентарь получателя предмета скрыт"),
            MarketBuyForErrorKind::InventoryHidden
        );
        assert_eq!(
            classify_market_buy_for_error(5, "Вам нужно сначала открыть инвентарь в настройках стим профиля."),
            MarketBuyForErrorKind::InventoryHidden
        );

        // Code 6
        assert_eq!(
            classify_market_buy_for_error(6, "The user is banned in Steam"),
            MarketBuyForErrorKind::SteamBanned
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Пользователь забанен в стиме"),
            MarketBuyForErrorKind::SteamBanned
        );

        // Code 7
        assert_eq!(
            classify_market_buy_for_error(7, "The recipient of the item has not bound a mobile authenticator"),
            MarketBuyForErrorKind::NoMobileAuth
        );
        assert_eq!(
            classify_market_buy_for_error(0, "У получателя предмета не привязан мобильный аутентификатор"),
            MarketBuyForErrorKind::NoMobileAuth
        );

        // Code 8
        assert_eq!(
            classify_market_buy_for_error(8, "Error checking the link. Check the possibility of offline trades on your account"),
            MarketBuyForErrorKind::OfflineTradesDisabled
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Ошибка проверки ссылки. Проверьте возможность оффлайн трейдов на вашем аккаунте"),
            MarketBuyForErrorKind::OfflineTradesDisabled
        );

        // Code 12
        assert_eq!(
            classify_market_buy_for_error(12, "Invalid trade link"),
            MarketBuyForErrorKind::InvalidTradeLink
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Неверная трейд ссылка"),
            MarketBuyForErrorKind::InvalidTradeLink
        );

        // Code 20
        assert_eq!(
            classify_market_buy_for_error(20, "Our bot for checking trade links is banned"),
            MarketBuyForErrorKind::CheckBotBanned
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Наш бот для проверки трейд ссылки забанен"),
            MarketBuyForErrorKind::CheckBotBanned
        );

        // Code 21
        assert_eq!(
            classify_market_buy_for_error(21, "The recipient of the item inventory is full"),
            MarketBuyForErrorKind::InventoryFull
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Инвентарь получателя предмета переполнен"),
            MarketBuyForErrorKind::InventoryFull
        );

        // Funds (code 0)
        assert_eq!(
            classify_market_buy_for_error(0, "Not enough funds on account"),
            MarketBuyForErrorKind::NotEnoughFunds
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Недостаточно средств на счету"),
            MarketBuyForErrorKind::NotEnoughFunds
        );

        // Price deviation (code 0)
        assert_eq!(
            classify_market_buy_for_error(0, "No item found at the specified chance to transfer at the specified price or below"),
            MarketBuyForErrorKind::PriceOrChanceDeviation
        );
        assert_eq!(
            classify_market_buy_for_error(0, "Не найден предмет с указанным шансом на передачу по указанной цене или ниже"),
            MarketBuyForErrorKind::PriceOrChanceDeviation
        );
    }
}

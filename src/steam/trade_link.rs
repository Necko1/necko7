use std::str::FromStr;
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradeLink {
    pub partner: String,
    pub token: String,
}

impl TradeLink {
    pub fn parse(input: &str) -> Option<Self> {
        for word in input.split_whitespace() {
            if word.contains("steamcommunity.com") {
                if let Some(trade_link) = Self::parse_single_url(word) {
                    return Some(trade_link);
                }
            }
        }

        None
    }

    pub fn parse_single_url(input: &str) -> Option<Self> {
        let input = input.trim();

        let url_str = if !input.starts_with("http://") && !input.starts_with("https://") {
            format!("https://{}", input)
        } else {
            input.to_string()
        };

        let parsed = Url::parse(&url_str).ok()?;

        if parsed.host_str()? != "steamcommunity.com" {
            return None;
        }

        let path = parsed.path().trim_end_matches('/');
        if path != "/tradeoffer/new" {
            return None;
        }

        let mut partner = None;
        let mut token = None;

        for (k, v) in parsed.query_pairs() {
            match k.as_ref() {
                "partner" if !v.is_empty() && v.chars().all(|c| c.is_ascii_digit()) => {
                    partner = Some(v.into_owned());
                }
                "token" if !v.is_empty() && v.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') => {
                    token = Some(v.into_owned());
                }
                _ => {}
            }
        }

        Some(Self {
            partner: partner?,
            token: token?,
        })
    }
}

impl FromStr for TradeLink {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s).ok_or(())
    }
}
use std::sync::Arc;
use crate::state::AppState;

pub fn build_cookie_string(name: &str, value: &str, max_age: i64, app_url: &str) -> String {
    let mut cookie = format!(
        "{}={}; HttpOnly; Path=/; Max-Age={}",
        name, value, max_age
    );

    let is_https = app_url.starts_with("https://");
    let is_localhost = app_url.contains("localhost") || app_url.contains("127.0.0.1");

    if is_https {
        cookie.push_str("; SameSite=None; Secure");
    } else if is_localhost {
        cookie.push_str("; SameSite=Lax");
    } else {
        cookie.push_str("; SameSite=Lax");
    }

    cookie
}

pub(super) fn build_csrf_cookie(state: &Arc<AppState>, value: &str) -> String {
    let mut cookie = format!(
        "oauth_state={}; HttpOnly; Path=/api/v1/auth; Max-Age=300",
        value
    );

    let is_https = state.app_url.starts_with("https://");

    if is_https {
        cookie.push_str("; SameSite=None; Secure");
    } else {
        cookie.push_str("; SameSite=Lax");
    }

    cookie
}
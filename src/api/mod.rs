pub mod auth;
pub mod eventsub;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use axum::Router;
use axum::routing::{get, post};
use axum::extract::State;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::http::{Request, StatusCode};
use crate::state::AppState;

async fn app_init_guard(
    State(state): State<Arc<AppState>>,
    req: Request<axum::body::Body>,
    next: Next,
) -> Response {
    if !state.app_initialized.load(Ordering::Relaxed) {
        return StatusCode::NOT_FOUND.into_response();
    }
    next.run(req).await
}

pub fn build_router(state: Arc<AppState>) -> Router {
    let bot = Router::new()
        .route("/auth/login/bot", get(auth::bot_login_redirect));

    let guarded = Router::new()
        .route("/auth/login", get(auth::streamer_login_redirect))
        .route("/auth/callback", get(auth::auth_callback))
        .route("/eventsub", post(eventsub::handle_eventsub))
        .layer(middleware::from_fn_with_state(state.clone(), app_init_guard));

    Router::new()
        .merge(bot)
        .merge(guarded)
        .with_state(state)
}
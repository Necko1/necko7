pub mod auth;
pub mod eventsub;
pub mod v1;
pub mod extractor;
pub mod error;
pub mod cookie;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use axum::Router;
use axum::http::{Method, header};
use axum::routing::{get, post};
use axum::extract::State;
use axum::middleware::{self, Next};
use axum::response::{IntoResponse, Response};
use axum::http::{Request, StatusCode};
use tower_http::cors::CorsLayer;
use utoipa_swagger_ui::SwaggerUi;
use utoipa::OpenApi;
use crate::state::AppState;
use v1::ApiDoc;

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
    let init = Router::new()
        .route("/auth/init/bot", get(auth::bot_login_redirect));

    let guarded = Router::new()
        .route("/auth/connect", get(auth::streamer_login_redirect))
        .route("/auth/login", get(auth::user_login_redirect))
        .route("/auth/logout", post(auth::logout).get(auth::logout))
        .route("/eventsub", post(eventsub::handle_eventsub))
        .merge(v1::router())
        .layer(middleware::from_fn_with_state(state.clone(), app_init_guard));

    let cors = CorsLayer::new()
        .allow_origin(state.frontend_url.parse::<axum::http::HeaderValue>().unwrap())
        .allow_credentials(true)
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS])
        .allow_headers([header::AUTHORIZATION, header::ACCEPT, header::CONTENT_TYPE]);

    let swagger = SwaggerUi::new("/swagger-ui")
        .url("/api-docs/openapi.json", ApiDoc::openapi());

    Router::new()
        .route("/auth/callback", get(auth::auth_callback))
        .merge(init)
        .merge(guarded)
        .merge(swagger)
        .layer(cors)
        .with_state(state)
}

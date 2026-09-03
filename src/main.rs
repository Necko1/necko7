pub mod state;
pub mod db;
pub mod helix;
pub mod api;
pub mod steam;
pub mod processor;
pub mod datetime;

use std::env;

use dotenvy::dotenv;
use tokio::net::TcpListener;
use tracing::info;
use tracing_subscriber::EnvFilter;

use state::AppState;
use crate::db::Db;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

#[tokio::main]
async fn main() -> AppResult<()> {
    dotenv().ok();

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("necko7=debug,info")),
        )
        .with_target(true)
        .with_line_number(true)
        .init();

    info!("Starting necko7 bot service...");

    let addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let database_url = env::var("DATABASE_URL")
        .map_err(|e| {
            tracing::error!("DATABASE_URL environment variable is not set!");
            e
        })?;

    info!("Connecting to database...");
    let db = Db::connect(&database_url).await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to connect to PostgreSQL database");
            e
        })?;
    info!("Database connection established");

    info!("Initializing AppState from environment...");
    let state = AppState::from_env(db).await
        .map_err(|e| {
            tracing::error!(error = %e, "Failed to initialize AppState");
            e
        })?;
    info!("AppState initialized successfully");

    info!("Starting background processor tasks...");
    processor::start_background_tasks(state.clone()).await;

    let app = api::build_router(state);

    info!("Binding TCP listener on {}", addr);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| {
            tracing::error!(error = %e, addr = %addr, "Failed to bind TCP listener");
            e
        })?;

    info!("HTTP server listening on http://{}", addr);
    axum::serve(listener, app).await
        .map_err(|e| {
            tracing::error!(error = %e, "HTTP server error");
            e
        })?;

    Ok(())
}

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
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());

    let db = Db::connect(&env::var("DATABASE_URL")
        .expect("DATABASE_URL not found in the environment")).await?;

    let state = AppState::from_env(db).await?;

    processor::start_background_tasks(state.clone()).await;

    let app = api::build_router(state);

    info!("Starting HTTP server on {addr}");
    let listener = TcpListener::bind(&addr).await?;

    axum::serve(listener, app).await?;

    Ok(())
}

pub mod state;
pub mod db;
pub mod helix;
pub mod api;
pub mod steam;
pub mod processor;
pub mod datetime;
pub mod messages;

use std::env;
use std::time::Duration;

use dotenvy::dotenv;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use tracing::info;
use tracing_subscriber::EnvFilter;

use state::AppState;
use crate::db::Db;

pub type AppResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

async fn shutdown_signal(shutdown_token: CancellationToken) {
    let ctrl_c = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C signal handler");
    };

    #[cfg(unix)]
    let terminate = async {
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("Failed to install SIGTERM signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received SIGINT (Ctrl+C), initiating graceful shutdown...");
        }
        _ = terminate => {
            info!("Received SIGTERM, initiating graceful shutdown...");
        }
        _ = shutdown_token.cancelled() => {
            info!("Shutdown triggered via token, initiating graceful shutdown...");
        }
    }

    shutdown_token.cancel();

    // If user presses Ctrl+C again during graceful shutdown, exit forcefully immediately
    tokio::spawn(async {
        if tokio::signal::ctrl_c().await.is_ok() {
            tracing::warn!("Second Ctrl+C received; terminating immediately!");
            std::process::exit(130);
        }
    });
}

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

    let app = api::build_router(state.clone());

    info!("Binding TCP listener on {}", addr);
    let listener = TcpListener::bind(&addr).await
        .map_err(|e| {
            tracing::error!(error = %e, addr = %addr, "Failed to bind TCP listener");
            e
        })?;

    info!("HTTP server listening on http://{}", addr);

    let shutdown_token = state.shutdown_token.clone();
    let server_token = shutdown_token.clone();

    let server = axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            server_token.cancelled().await;
        });

    tokio::select! {
        res = server => {
            if let Err(e) = res {
                tracing::error!(error = %e, "HTTP server encountered an error");
            }
        }
        _ = shutdown_signal(shutdown_token.clone()) => {}
    }

    info!("HTTP server has stopped accepting new connections. Draining and shutting down background tasks...");

    // Signal all background loops and tasks to stop
    state.shutdown_token.cancel();

    // Close the task tracker so no new tasks are accepted
    state.tasks.close();

    // Wait for in-flight tasks and exiting loops with a timeout
    let shutdown_timeout = Duration::from_secs(30);
    info!("Waiting up to {} seconds for in-flight tasks to finish...", shutdown_timeout.as_secs());

    match tokio::time::timeout(shutdown_timeout, state.tasks.wait()).await {
        Ok(_) => {
            info!("All background tasks have shut down successfully.");
        }
        Err(_) => {
            tracing::warn!("Graceful shutdown timed out; some tasks did not finish within the grace period.");
        }
    }

    info!("Closing PostgreSQL database connection pool...");
    state.db.close().await;
    info!("Database connections closed successfully.");

    info!("Necko7 bot service stopped gracefully.");
    Ok(())
}

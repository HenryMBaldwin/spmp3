mod api;
mod config;
mod error;

use std::process::ExitCode;

use axum::{
    Router,
    routing::{get, post},
};
use tower_http::trace::TraceLayer;

use crate::config::Config;

const DEFAULT_FILTER: &str = "warn,web=info,mp3sync=info";

#[tokio::main]
async fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .json()
        .flatten_event(true)
        .with_current_span(false)
        .with_span_list(false)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| DEFAULT_FILTER.into()),
        )
        .init();

    match run().await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "web exiting");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    tracing::info!(
        library_dir = %config.library_dir.display(),
        bind = %config.bind,
        "starting"
    );

    let app = Router::new()
        .route("/", get(api::index_handler))
        .route("/api/status", get(api::status_handler))
        .route("/api/plan", post(api::plan_handler))
        .route("/api/track/{id}", get(api::track_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(config.clone());

    let listener = tokio::net::TcpListener::bind(config.bind).await?;
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown())
        .await?;

    tracing::info!("web stopped");

    Ok(())
}

#[cfg(unix)]
async fn shutdown() {
    use tokio::signal::unix::{SignalKind, signal};

    let Ok(mut terminate) = signal(SignalKind::terminate()) else {
        let _ = tokio::signal::ctrl_c().await;
        return;
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

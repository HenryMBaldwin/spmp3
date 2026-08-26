mod config;
mod device;
mod library;
mod mount;

use std::{process::ExitCode, sync::Arc};

use tokio::sync::watch;

use crate::config::Config;

const DEFAULT_FILTER: &str = "warn,daemon=info,spsync=info,mp3sync=info";

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
            tracing::error!(error = %e, "daemon exiting");
            ExitCode::FAILURE
        }
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let config = Config::from_env()?;
    tracing::info!(
        library_dir = %config.spotify.library_dir.display(),
        mount_dir = %config.device.mount_dir.display(),
        library_interval_secs = config.library_interval.as_secs(),
        realtime = config.spotify.realtime,
        "starting"
    );

    let client = spsync::Client::new(config.spotify.clone())?;
    let syncer = Arc::new(mp3sync::Syncer::new(config.device.clone()));

    if !client.is_authenticated() {
        tracing::warn!(
            cache_dir = %config.spotify.cache_dir.display(),
            "no cached spotify credentials; run the login binary to authorize. library sync is paused until then"
        );
    }

    let (tx, rx) = watch::channel(false);

    let library = tokio::spawn(library::run(client, config.library_interval, rx.clone()));
    let device = tokio::spawn(device::run(syncer, config.device_poll, rx));

    wait_for_shutdown().await;
    tracing::info!("shutdown signal received");
    let _ = tx.send(true);

    let _ = library.await;
    let _ = device.await;

    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{SignalKind, signal};

    let mut terminate = match signal(SignalKind::terminate()) {
        Ok(signal) => signal,
        Err(e) => {
            tracing::error!(error = %e, "could not listen for SIGTERM");
            let _ = tokio::signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = tokio::signal::ctrl_c() => {}
        _ = terminate.recv() => {}
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
}

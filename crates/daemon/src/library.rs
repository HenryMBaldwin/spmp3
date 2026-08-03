use std::time::Duration;

use spsync::Client;
use tokio::sync::watch;

pub(crate) async fn run(client: Client, interval: Duration, mut shutdown: watch::Receiver<bool>) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {}
        }

        if !client.is_authenticated() {
            tracing::warn!("still not authenticated; skipping library sync");
            continue;
        }

        tokio::select! {
            _ = shutdown.changed() => break,
            result = client.sync_library() => match result {
                Ok(report) if report.added == 0
                    && report.restored == 0
                    && report.removed == 0
                    && report.failed.is_empty() =>
                {
                    tracing::debug!("library already up to date");
                }
                Ok(report) => tracing::info!(
                    added = report.added,
                    restored = report.restored,
                    removed = report.removed,
                    failed = report.failed.len(),
                    "library sync finished"
                ),
                Err(e) => tracing::error!(error = %e, "library sync failed"),
            }
        }
    }

    tracing::info!("library loop stopped");
}

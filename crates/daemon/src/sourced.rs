use std::{
    fs,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant, SystemTime},
};

use common::status::{Failure, Sourced};
use spsync::Client;
use tokio::sync::watch;

use crate::status::StatusFile;

const RETRY_INTERVAL: Duration = Duration::from_mins(5);

fn modified(path: &PathBuf) -> Option<SystemTime> {
    fs::metadata(path)
        .ok()
        .and_then(|meta| meta.modified().ok())
}

pub(crate) async fn run(
    client: Client,
    poll: Duration,
    status: Arc<StatusFile>,
    mut shutdown: watch::Receiver<bool>,
) {
    let path = client.overrides_path();
    let mut ticker = tokio::time::interval(poll);
    let mut seen = modified(&path);
    let mut last = Instant::now()
        .checked_sub(RETRY_INTERVAL)
        .unwrap_or_else(Instant::now);
    let mut first = true;

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {}
        }

        let now = modified(&path);
        let changed = now != seen;

        if !first && !changed && last.elapsed() < RETRY_INTERVAL {
            continue;
        }

        first = false;
        seen = now;
        last = Instant::now();

        tokio::select! {
            _ = shutdown.changed() => break,
            result = client.sync_sourced() => match result {
                Ok(report) => {
                    if report.downloaded > 0 || !report.failures.is_empty() {
                        tracing::info!(
                            tracks = report.tracks,
                            downloaded = report.downloaded,
                            failed = report.failures.len(),
                            "sourced sync finished"
                        );
                    }

                    let failures = report
                        .failures
                        .iter()
                        .map(|f| Failure {
                            uri: f.uri.clone(),
                            artist: f.artist.clone(),
                            title: f.title.clone(),
                            error: f.error.clone(),
                        })
                        .collect();

                    status
                        .update(|s| {
                            s.sourced = Some(Sourced {
                                checked_at: common::status::now(),
                                tracks: report.tracks,
                                downloaded: report.downloaded,
                                failures,
                            });
                        })
                        .await;
                }
                Err(e) => tracing::error!(error = %e, "sourced sync failed"),
            }
        }
    }

    tracing::info!("sourced loop stopped");
}

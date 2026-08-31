use std::{sync::Arc, time::Duration};

use common::status::{Failure, Library, Run};
use spsync::Client;
use tokio::sync::watch;

use crate::status::{StatusFile, library_bytes};

fn snapshot(client: &Client) -> Library {
    let manifest = client.manifest().unwrap_or_default();

    Library {
        tracks: manifest.entries.len(),
        liked: manifest.entries.values().filter(|e| e.liked).count(),
        bytes: library_bytes(&client.config().library_dir),
    }
}

pub(crate) async fn run(
    client: Client,
    interval: Duration,
    status: Arc<StatusFile>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut ticker = tokio::time::interval(interval);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {}
        }

        if !client.is_authenticated() {
            tracing::warn!("still not authenticated; skipping library sync");
            status.update(|s| s.authenticated = false).await;
            continue;
        }

        let started = std::time::Instant::now();

        tokio::select! {
            _ = shutdown.changed() => break,
            result = client.sync_library() => match result {
                Ok(report) => {
                    if report.added == 0
                        && report.restored == 0
                        && report.removed == 0
                        && report.failed.is_empty()
                    {
                        tracing::debug!("library already up to date");
                    } else {
                        tracing::info!(
                            added = report.added,
                            restored = report.restored,
                            removed = report.removed,
                            missing_covers = report.missing_covers,
                            failed = report.failed.len(),
                            "library sync finished"
                        );
                    }

                    let library = snapshot(&client);
                    let failures = report
                        .failed
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
                            s.authenticated = true;
                            s.library = library;
                            s.failures = failures;
                            s.last_run = Some(Run {
                                finished_at: common::status::now(),
                                added: report.added,
                                restored: report.restored,
                                removed: report.removed,
                                missing_covers: report.missing_covers,
                                took_secs: started.elapsed().as_secs(),
                            });
                        })
                        .await;
                }
                Err(e) => tracing::error!(error = %e, "library sync failed"),
            }
        }
    }

    tracing::info!("library loop stopped");
}

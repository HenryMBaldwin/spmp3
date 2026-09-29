use std::{sync::Arc, time::Duration};

use common::status::Device;
use mp3sync::Syncer;
use tokio::sync::watch;

use crate::{mount::is_mounted, status::StatusFile};

pub(crate) async fn run(
    syncer: Arc<Syncer>,
    poll: Duration,
    status: Arc<StatusFile>,
    mut shutdown: watch::Receiver<bool>,
) {
    let mount_dir = syncer.config().mount_dir.clone();
    let mut ticker = tokio::time::interval(poll);
    let mut was_mounted = is_mounted(&mount_dir);

    if was_mounted {
        tracing::info!(path = %mount_dir.display(), "device already present at startup");
        on_mounted(&syncer, &status).await;
    }

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {}
        }

        let mounted = is_mounted(&mount_dir);

        if mounted && !was_mounted {
            tracing::info!(path = %mount_dir.display(), "device mounted");
            on_mounted(&syncer, &status).await;
        } else if !mounted && was_mounted {
            tracing::info!(path = %mount_dir.display(), "device removed");
        }

        was_mounted = mounted;
    }

    tracing::info!("device loop stopped");
}

async fn on_mounted(syncer: &Arc<Syncer>, status: &Arc<StatusFile>) {
    reconcile(syncer).await;
    let failed = sync(syncer).await;
    record(syncer, status, failed).await;
}

async fn record(syncer: &Arc<Syncer>, status: &Arc<StatusFile>, failed: usize) {
    let pending = syncer.pending().map_or(0, |plan| {
        plan.copy.len() + plan.rename.len() + plan.delete.len()
    });
    let files = syncer.state().map_or(0, |state| state.entries.len());

    status
        .update(|s| {
            s.device = Some(Device {
                synced_at: common::status::now(),
                files,
                pending,
                failed,
                source: "daemon".to_owned(),
            });
        })
        .await;
}

async fn reconcile(syncer: &Arc<Syncer>) {
    let syncer = Arc::clone(syncer);

    match tokio::task::spawn_blocking(move || syncer.reconcile()).await {
        Ok(Ok(report)) if report.previously_recorded == 0 => {
            tracing::info!(
                found = report.found,
                missing = report.missing,
                "no recorded device state; rebuilt from the device"
            );
        }
        Ok(Ok(report)) if report.missing == 0 && report.orphans.is_empty() => {
            tracing::debug!(found = report.found, "device contents match recorded state");
        }
        Ok(Ok(report)) => tracing::warn!(
            found = report.found,
            missing = report.missing,
            orphans = report.orphans.len(),
            "device contents did not match recorded state; rebuilt from the device"
        ),
        Ok(Err(e)) => tracing::error!(error = %e, "could not reconcile device state"),
        Err(e) => tracing::error!(error = %e, "reconcile task panicked"),
    }
}

async fn sync(syncer: &Arc<Syncer>) -> usize {
    match syncer.needs_sync() {
        Ok(false) => {
            tracing::info!("device already up to date");
            return 0;
        }
        Ok(true) => {}
        Err(e) => {
            tracing::error!(error = %e, "could not determine device sync state");
            return 0;
        }
    }

    let syncer = Arc::clone(syncer);
    let result = tokio::task::spawn_blocking(move || syncer.sync()).await;

    match result {
        Ok(Ok(report)) => {
            tracing::info!(
                copied = report.copied,
                renamed = report.renamed,
                deleted = report.deleted,
                failed = report.failed.len(),
                "device sync finished"
            );
            report.failed.len()
        }
        Ok(Err(e)) => {
            tracing::error!(error = %e, "device sync failed");
            0
        }
        Err(e) => {
            tracing::error!(error = %e, "device sync task panicked");
            0
        }
    }
}

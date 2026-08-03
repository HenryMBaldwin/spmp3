use std::{sync::Arc, time::Duration};

use mp3sync::Syncer;
use tokio::sync::watch;

use crate::mount::is_mounted;

pub(crate) async fn run(syncer: Arc<Syncer>, poll: Duration, mut shutdown: watch::Receiver<bool>) {
    let mount_dir = syncer.config().mount_dir.clone();
    let mut ticker = tokio::time::interval(poll);
    let mut was_mounted = is_mounted(&mount_dir);

    if was_mounted {
        tracing::info!(path = %mount_dir.display(), "device already present at startup");
        sync(&syncer).await;
    }

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = ticker.tick() => {}
        }

        let mounted = is_mounted(&mount_dir);

        if mounted && !was_mounted {
            tracing::info!(path = %mount_dir.display(), "device mounted");
            sync(&syncer).await;
        } else if !mounted && was_mounted {
            tracing::info!(path = %mount_dir.display(), "device removed");
        }

        was_mounted = mounted;
    }

    tracing::info!("device loop stopped");
}

async fn sync(syncer: &Arc<Syncer>) {
    match syncer.needs_sync() {
        Ok(false) => {
            tracing::info!("device already up to date");
            return;
        }
        Ok(true) => {}
        Err(e) => {
            tracing::error!(error = %e, "could not determine device sync state");
            return;
        }
    }

    let syncer = Arc::clone(syncer);
    let result = tokio::task::spawn_blocking(move || syncer.sync()).await;

    match result {
        Ok(Ok(report)) => tracing::info!(
            copied = report.copied,
            renamed = report.renamed,
            deleted = report.deleted,
            failed = report.failed.len(),
            "device sync finished"
        ),
        Ok(Err(e)) => tracing::error!(error = %e, "device sync failed"),
        Err(e) => tracing::error!(error = %e, "device sync task panicked"),
    }
}

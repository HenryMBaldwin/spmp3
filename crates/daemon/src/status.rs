use std::{fs, path::PathBuf};

use common::status::Status;
use tokio::sync::Mutex;

pub(crate) struct StatusFile {
    path: PathBuf,
    state: Mutex<Status>,
}

impl StatusFile {
    pub(crate) fn new(path: PathBuf) -> Self {
        let state = Status::load(&path).unwrap_or_else(|e| {
            tracing::warn!(error = %e, "could not read status; starting fresh");
            Status::default()
        });

        Self {
            path,
            state: Mutex::new(state),
        }
    }

    /// Reloads first: the web writes the same file, and its device reading must survive.
    pub(crate) async fn update(&self, edit: impl FnOnce(&mut Status)) {
        let mut state = self.state.lock().await;

        if let Ok(disk) = Status::load(&self.path) {
            *state = disk;
        }

        edit(&mut state);
        state.updated_at = common::status::now();

        if let Err(e) = state.save(&self.path) {
            tracing::warn!(error = %e, path = %self.path.display(), "could not write status");
        }
    }
}

/// Total bytes of the `.mp3` files directly inside `dir`.
pub(crate) fn library_bytes(dir: &std::path::Path) -> u64 {
    let Ok(entries) = fs::read_dir(dir) else {
        return 0;
    };

    entries
        .flatten()
        .filter(|entry| entry.path().extension().is_some_and(|e| e == "mp3"))
        .filter_map(|entry| entry.metadata().ok())
        .map(|meta| meta.len())
        .sum()
}

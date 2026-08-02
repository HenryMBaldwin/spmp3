mod config;
mod error;
mod layout;
mod plan;
mod state;

use std::fs;

use common::manifest::{MANIFEST_FILE, Manifest};

pub use crate::{
    config::Config,
    error::Mp3syncError,
    plan::{Copy, Plan, Rename},
    state::{DeviceEntry, DeviceState},
};

#[derive(Debug, Default)]
pub struct SyncReport {
    pub copied: usize,
    pub renamed: usize,
    pub deleted: usize,
    pub failed: Vec<Failure>,
}

#[derive(Debug)]
pub struct Failure {
    pub path: std::path::PathBuf,
    pub error: String,
}

#[derive(Debug)]
pub struct Syncer {
    config: Config,
}

impl Syncer {
    pub const fn new(config: Config) -> Self {
        Self { config }
    }

    pub const fn config(&self) -> &Config {
        &self.config
    }

    fn manifest(&self) -> Result<Manifest, Mp3syncError> {
        Ok(Manifest::load(
            &self.config.library_dir.join(MANIFEST_FILE),
        )?)
    }

    fn state(&self) -> Result<DeviceState, Mp3syncError> {
        DeviceState::load(&self.config.device_state)
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError`] if the manifest or device state cannot be read.
    pub fn needs_sync(&self) -> Result<bool, Mp3syncError> {
        let state = self.state()?;
        let hash = plan::source_hash(&self.manifest()?);

        Ok(state.source_hash.as_deref() != Some(hash.as_str()))
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError`] if the manifest or device state cannot be read.
    pub fn pending(&self) -> Result<Plan, Mp3syncError> {
        Ok(plan::plan(&self.manifest()?, &self.state()?))
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError::DeviceNotMounted`] if the mount point is missing.
    /// Per-file failures are collected into the report rather than aborting.
    pub fn sync(&self) -> Result<SyncReport, Mp3syncError> {
        if !self.config.mount_dir.is_dir() {
            return Err(Mp3syncError::DeviceNotMounted {
                path: self.config.mount_dir.clone(),
            });
        }

        let manifest = self.manifest()?;
        let mut state = self.state()?;
        let steps = plan::plan(&manifest, &state);
        let mut report = SyncReport::default();

        for step in &steps.copy {
            let to = self.config.mount_dir.join(&step.to);
            match Self::copy_file(&self.config.library_dir.join(&step.from), &to) {
                Ok(()) => {
                    state.entries.insert(
                        step.id.clone(),
                        DeviceEntry {
                            path: step.to.clone(),
                            library_path: step.from.clone(),
                        },
                    );
                    state.save(&self.config.device_state)?;
                    report.copied += 1;
                    tracing::info!(to = %step.to.display(), "copied");
                }
                Err(e) => report.failed.push(Failure {
                    path: step.to.clone(),
                    error: e.to_string(),
                }),
            }
        }

        for step in &steps.rename {
            let from = self.config.mount_dir.join(&step.from);
            let to = self.config.mount_dir.join(&step.to);

            match Self::move_file(&from, &to) {
                Ok(()) => {
                    if let Some(existing) = state.entries.get_mut(&step.id) {
                        existing.path.clone_from(&step.to);
                    }
                    state.save(&self.config.device_state)?;
                    report.renamed += 1;
                    tracing::info!(to = %step.to.display(), "renamed");
                }
                Err(e) => report.failed.push(Failure {
                    path: step.to.clone(),
                    error: e.to_string(),
                }),
            }
        }

        for path in &steps.delete {
            let absolute = self.config.mount_dir.join(path);
            if let Err(e) = fs::remove_file(&absolute)
                && e.kind() != std::io::ErrorKind::NotFound
            {
                tracing::warn!(path = %absolute.display(), error = %e, "could not delete");
            }

            state.entries.retain(|_, entry| entry.path != *path);
            state.save(&self.config.device_state)?;
            report.deleted += 1;
            tracing::info!(path = %path.display(), "deleted");
        }

        if report.failed.is_empty() {
            state.source_hash = Some(plan::source_hash(&manifest));
            state.save(&self.config.device_state)?;
        } else {
            tracing::warn!(
                failed = report.failed.len(),
                "leaving source hash unstamped so the next run retries"
            );
        }

        Ok(report)
    }

    fn copy_file(from: &std::path::Path, to: &std::path::Path) -> Result<(), Mp3syncError> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp = to.with_extension("mp3.part");
        fs::copy(from, &tmp)?;
        fs::rename(&tmp, to)?;

        Ok(())
    }

    fn move_file(from: &std::path::Path, to: &std::path::Path) -> Result<(), Mp3syncError> {
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }

        fs::rename(from, to)?;

        Ok(())
    }
}

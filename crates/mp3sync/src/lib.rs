mod config;
mod error;
mod layout;
mod plan;
mod state;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

use common::manifest::{MANIFEST_FILE, Manifest};

pub use crate::{
    config::Config,
    error::Mp3syncError,
    plan::{Copy, Plan, Rename},
    state::{DeviceEntry, DeviceState},
};
use crate::{layout::MUSIC_DIR, layout::device_path};

#[derive(Debug, Default)]
pub struct ReconcileReport {
    pub found: usize,
    pub missing: usize,
    pub orphans: Vec<PathBuf>,
}

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
        let music_root = self.config.mount_dir.join(MUSIC_DIR);

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
                    if let Some(parent) = from.parent() {
                        Self::prune_empty_dirs(&music_root, parent);
                    }
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

            if let Some(parent) = absolute.parent() {
                Self::prune_empty_dirs(&music_root, parent);
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

    fn prune_empty_dirs(stop_at: &Path, start: &Path) {
        let mut current = start.to_path_buf();

        while current.starts_with(stop_at) && current != stop_at {
            let empty = fs::read_dir(&current).is_ok_and(|mut entries| entries.next().is_none());
            if !empty {
                break;
            }

            if fs::remove_dir(&current).is_err() {
                break;
            }

            tracing::debug!(dir = %current.display(), "pruned empty directory");
            match current.parent() {
                Some(parent) => current = parent.to_path_buf(),
                None => break,
            }
        }
    }

    fn device_files(root: &Path) -> Result<Vec<PathBuf>, Mp3syncError> {
        let mut found = Vec::new();
        let mut stack = vec![root.to_path_buf()];

        while let Some(dir) = stack.pop() {
            let entries = match fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => return Err(e.into()),
            };

            for entry in entries {
                let path = entry?.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "mp3") {
                    found.push(path);
                }
            }
        }

        Ok(found)
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError::DeviceNotMounted`] if the mount point is missing.
    pub fn reconcile(&self) -> Result<ReconcileReport, Mp3syncError> {
        if !self.config.mount_dir.is_dir() {
            return Err(Mp3syncError::DeviceNotMounted {
                path: self.config.mount_dir.clone(),
            });
        }

        let manifest = self.manifest()?;
        let expected: HashMap<PathBuf, (&String, PathBuf)> = manifest
            .entries
            .iter()
            .map(|(id, entry)| (device_path(entry), (id, entry.path.clone())))
            .collect();

        let mut state = DeviceState::default();
        let mut report = ReconcileReport::default();

        for absolute in Self::device_files(&self.config.mount_dir.join(MUSIC_DIR))? {
            let Ok(relative) = absolute.strip_prefix(&self.config.mount_dir) else {
                continue;
            };

            if let Some((id, library_path)) = expected.get(relative) {
                state.entries.insert(
                    (*id).clone(),
                    DeviceEntry {
                        path: relative.to_path_buf(),
                        library_path: library_path.clone(),
                    },
                );
                report.found += 1;
            } else {
                report.orphans.push(relative.to_path_buf());
            }
        }

        let steps = plan::plan(&manifest, &state);
        report.missing = steps.copy.len() + steps.rename.len();

        if steps.is_empty() && report.orphans.is_empty() {
            state.source_hash = Some(plan::source_hash(&manifest));
        }

        state.save(&self.config.device_state)?;

        tracing::info!(
            found = report.found,
            missing = report.missing,
            orphans = report.orphans.len(),
            "reconciled device state"
        );

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

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::fs;

    use super::Syncer;

    #[test]
    fn prunes_nested_empty_dirs_up_to_stop() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("Music");
        let leaf = root.join("Artist").join("Album");
        fs::create_dir_all(&leaf).expect("create");

        Syncer::prune_empty_dirs(&root, &leaf);

        assert!(!leaf.exists());
        assert!(!root.join("Artist").exists());
        assert!(root.exists());
    }

    #[test]
    fn stops_pruning_at_first_non_empty_dir() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path().join("Music");
        let album = root.join("Artist").join("Album");
        fs::create_dir_all(&album).expect("create");
        fs::write(root.join("Artist").join("keep.mp3"), b"x").expect("write");

        Syncer::prune_empty_dirs(&root, &album);

        assert!(!album.exists());
        assert!(root.join("Artist").exists());
    }

    #[test]
    fn finds_mp3s_recursively_and_ignores_others() {
        let dir = tempfile::tempdir().expect("tempdir");
        let nested = dir.path().join("Artist").join("Album");
        fs::create_dir_all(&nested).expect("create");
        fs::write(nested.join("a.mp3"), b"x").expect("write");
        fs::write(nested.join("cover.jpg"), b"x").expect("write");

        let found = Syncer::device_files(dir.path()).expect("walk");

        assert_eq!(found.len(), 1);
        assert!(found[0].ends_with("a.mp3"));
    }

    #[test]
    fn device_files_tolerates_missing_root() {
        let dir = tempfile::tempdir().expect("tempdir");
        let found = Syncer::device_files(&dir.path().join("nope")).expect("walk");

        assert!(found.is_empty());
    }
}

use std::path::PathBuf;

use common::manifest::Manifest;
use sha2::{Digest, Sha256};

use crate::{layout::device_path, state::DeviceState};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Copy {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rename {
    pub id: String,
    pub from: PathBuf,
    pub to: PathBuf,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Plan {
    pub copy: Vec<Copy>,
    pub rename: Vec<Rename>,
    pub delete: Vec<PathBuf>,
}

impl Plan {
    pub fn is_empty(&self) -> bool {
        self.copy.is_empty() && self.rename.is_empty() && self.delete.is_empty()
    }

    pub fn len(&self) -> usize {
        self.copy.len() + self.rename.len() + self.delete.len()
    }
}

pub fn source_hash(manifest: &Manifest) -> String {
    let mut hasher = Sha256::new();

    for (id, entry) in manifest.liked() {
        hasher.update(id.as_bytes());
        hasher.update([0]);
        hasher.update(device_path(entry).to_string_lossy().as_bytes());
        hasher.update([0]);
    }

    format!("{:x}", hasher.finalize())
}

pub fn plan(manifest: &Manifest, state: &DeviceState) -> Plan {
    let mut plan = Plan::default();

    for (id, entry) in manifest.liked() {
        let target = device_path(entry);

        match state.entries.get(id) {
            Some(existing) if existing.path == target => {}
            Some(existing) => plan.rename.push(Rename {
                id: id.clone(),
                from: existing.path.clone(),
                to: target,
            }),
            None => plan.copy.push(Copy {
                id: id.clone(),
                from: entry.path.clone(),
                to: target,
            }),
        }
    }

    for (id, existing) in &state.entries {
        let still_wanted = manifest.entries.get(id).is_some_and(|entry| entry.liked);

        if !still_wanted {
            plan.delete.push(existing.path.clone());
        }
    }

    plan
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use std::path::PathBuf;

    use common::manifest::{Entry, Manifest};

    use super::{plan, source_hash};
    use crate::state::{DeviceEntry, DeviceState};

    fn manifest(tracks: &[(&str, &str, &str, bool)]) -> Manifest {
        let mut manifest = Manifest::default();

        for (id, artist, album, liked) in tracks {
            manifest.entries.insert(
                (*id).to_owned(),
                Entry {
                    uri: format!("spotify:track:{id}"),
                    path: PathBuf::from(format!("{artist} - {id}.mp3")),
                    added_at: None,
                    liked: *liked,
                    artist: (*artist).to_owned(),
                    album: (*album).to_owned(),
                    source_format: String::new(),
                    encoder: String::new(),
                },
            );
        }

        manifest
    }

    fn state(entries: &[(&str, &str)]) -> DeviceState {
        let mut state = DeviceState::default();

        for (id, path) in entries {
            state.entries.insert(
                (*id).to_owned(),
                DeviceEntry {
                    path: PathBuf::from(path),
                    library_path: PathBuf::from("ignored.mp3"),
                },
            );
        }

        state
    }

    #[test]
    fn empty_device_copies_all_liked() {
        let manifest = manifest(&[("a", "A", "Al", true), ("b", "B", "Bl", true)]);
        let result = plan(&manifest, &DeviceState::default());

        assert_eq!(result.copy.len(), 2);
        assert!(result.rename.is_empty());
        assert!(result.delete.is_empty());
    }

    #[test]
    fn unliked_tracks_are_not_copied() {
        let manifest = manifest(&[("a", "A", "Al", false)]);
        let result = plan(&manifest, &DeviceState::default());

        assert!(result.is_empty());
    }

    #[test]
    fn unliked_track_already_on_device_is_deleted() {
        let manifest = manifest(&[("a", "A", "Al", false)]);
        let state = state(&[("a", "Music/A/Al/A - a.mp3")]);
        let result = plan(&manifest, &state);

        assert_eq!(result.delete, vec![PathBuf::from("Music/A/Al/A - a.mp3")]);
        assert!(result.copy.is_empty());
    }

    #[test]
    fn in_sync_is_empty() {
        let manifest = manifest(&[("a", "A", "Al", true)]);
        let state = state(&[("a", "Music/A/Al/A - a.mp3")]);

        assert!(plan(&manifest, &state).is_empty());
    }

    #[test]
    fn retagged_track_renames_instead_of_recopying() {
        let manifest = manifest(&[("a", "A", "NewAlbum", true)]);
        let state = state(&[("a", "Music/A/OldAlbum/A - a.mp3")]);
        let result = plan(&manifest, &state);

        assert!(result.copy.is_empty());
        assert_eq!(result.rename.len(), 1);
        assert_eq!(
            result.rename[0].to,
            PathBuf::from("Music/A/NewAlbum/A - a.mp3")
        );
    }

    #[test]
    fn hash_ignores_unliked_entries() {
        let with_unliked = manifest(&[("a", "A", "Al", true), ("b", "B", "Bl", false)]);
        let without = manifest(&[("a", "A", "Al", true)]);

        assert_eq!(source_hash(&with_unliked), source_hash(&without));
    }

    #[test]
    fn hash_changes_when_layout_changes() {
        let before = manifest(&[("a", "A", "Al", true)]);
        let after = manifest(&[("a", "A", "Different", true)]);

        assert_ne!(source_hash(&before), source_hash(&after));
    }

    #[test]
    fn hash_is_stable_across_calls() {
        let manifest = manifest(&[("a", "A", "Al", true), ("b", "B", "Bl", true)]);

        assert_eq!(source_hash(&manifest), source_hash(&manifest));
    }
}

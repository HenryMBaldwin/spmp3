use std::{
    collections::{BTreeMap, HashMap},
    ffi::OsStr,
    fs, io,
    path::{Path, PathBuf},
};

use common::manifest::Manifest;
use serde::{Deserialize, Serialize};

use crate::{error::Mp3syncError, layout::device_path};

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub version: u32,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub entries: BTreeMap<String, DeviceEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct DeviceFile {
    pub path: PathBuf,
    #[serde(default)]
    pub id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DeviceEntry {
    pub path: PathBuf,
    pub library_path: PathBuf,
}

impl Default for DeviceState {
    fn default() -> Self {
        Self {
            version: STATE_VERSION,
            source_hash: None,
            entries: BTreeMap::new(),
        }
    }
}

impl DeviceState {
    pub fn from_contents(manifest: &Manifest, contents: &[DeviceFile]) -> (Self, Vec<PathBuf>) {
        let by_path: HashMap<PathBuf, &String> = manifest
            .entries
            .iter()
            .map(|(id, entry)| (device_path(entry), id))
            .collect();

        let by_file_name: HashMap<&OsStr, &String> = manifest
            .entries
            .iter()
            .filter_map(|(id, entry)| entry.path.file_name().map(|name| (name, id)))
            .collect();

        let mut state = Self::default();
        let mut orphans = Vec::new();

        for file in contents {
            let matched = file
                .id
                .as_ref()
                .filter(|id| manifest.entries.contains_key(id.as_str()))
                .map(String::as_str)
                .or_else(|| by_path.get(&file.path).map(|id| id.as_str()))
                .or_else(|| {
                    file.path
                        .file_name()
                        .and_then(|name| by_file_name.get(name))
                        .map(|id| id.as_str())
                });

            match matched.and_then(|id| manifest.entries.get(id).map(|entry| (id, entry))) {
                Some((id, entry)) => {
                    let candidate = DeviceEntry {
                        path: file.path.clone(),
                        library_path: entry.path.clone(),
                    };

                    match state.entries.get(id) {
                        Some(existing) if existing.path == device_path(entry) => {
                            orphans.push(candidate.path);
                        }
                        Some(existing) => {
                            orphans.push(existing.path.clone());
                            state.entries.insert(id.to_owned(), candidate);
                        }
                        None => {
                            state.entries.insert(id.to_owned(), candidate);
                        }
                    }
                }
                None => orphans.push(file.path.clone()),
            }
        }

        (state, orphans)
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError`] if the file cannot be read or parsed, or was written by
    /// a newer version of this crate.
    pub fn load(path: &Path) -> Result<Self, Mp3syncError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };

        let state: Self = serde_json::from_slice(&bytes)?;
        if state.version > STATE_VERSION {
            return Err(Mp3syncError::StateVersion {
                found: state.version,
                supported: STATE_VERSION,
            });
        }

        Ok(state)
    }

    /// # Errors
    ///
    /// Returns [`Mp3syncError::Io`] if the state cannot be written or renamed.
    pub fn save(&self, path: &Path) -> Result<(), Mp3syncError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use common::manifest::{Entry, Manifest};

    use super::{DeviceFile, DeviceState, device_path};

    fn at(path: &str) -> DeviceFile {
        DeviceFile {
            path: PathBuf::from(path),
            id: None,
        }
    }

    fn manifest(tracks: &[(&str, &str, &str)]) -> Manifest {
        let mut manifest = Manifest::default();

        for (id, artist, album) in tracks {
            manifest.entries.insert(
                (*id).to_owned(),
                Entry {
                    uri: format!("spotify:track:{id}"),
                    path: PathBuf::from(format!("{artist} - {id}.mp3")),
                    added_at: None,
                    liked: true,
                    artist: (*artist).to_owned(),
                    album: (*album).to_owned(),
                    source_format: String::new(),
                    encoder: String::new(),
                    source_url: String::new(),
                },
            );
        }

        manifest
    }

    fn with_id(path: &str, id: &str) -> DeviceFile {
        DeviceFile {
            path: PathBuf::from(path),
            id: Some(id.to_owned()),
        }
    }

    #[test]
    fn duplicate_files_for_one_track_are_orphaned() {
        let m = manifest(&[("a", "Artist", "Album")]);
        let wanted = device_path(&m.entries["a"]);
        let keep = wanted.to_string_lossy().into_owned();
        let stale = "Music/Artist/Album/stale.mp3";

        for order in [
            vec![with_id(stale, "a"), with_id(&keep, "a")],
            vec![with_id(&keep, "a"), with_id(stale, "a")],
        ] {
            let (result, orphans) = DeviceState::from_contents(&m, &order);

            assert_eq!(result.entries["a"].path, wanted);
            assert_eq!(orphans, vec![PathBuf::from(stale)]);
        }
    }

    #[test]
    fn matches_reported_paths_to_track_ids() {
        let manifest = manifest(&[("a", "A", "Al")]);
        let (state, orphans) = DeviceState::from_contents(&manifest, &[at("Music/A/Al/A - a.mp3")]);

        assert_eq!(state.entries.len(), 1);
        assert!(state.entries.contains_key("a"));
        assert!(orphans.is_empty());
    }

    #[test]
    fn unknown_paths_become_orphans() {
        let manifest = manifest(&[("a", "A", "Al")]);
        let (state, orphans) = DeviceState::from_contents(&manifest, &[at("Music/Other/x.mp3")]);

        assert!(state.entries.is_empty());
        assert_eq!(orphans, vec![PathBuf::from("Music/Other/x.mp3")]);
    }

    #[test]
    fn empty_contents_yields_empty_state() {
        let manifest = manifest(&[("a", "A", "Al")]);
        let (state, orphans) = DeviceState::from_contents(&manifest, &[]);

        assert!(state.entries.is_empty());
        assert!(orphans.is_empty());
    }

    fn identified(path: &str, id: &str) -> DeviceFile {
        DeviceFile {
            path: PathBuf::from(path),
            id: Some(id.to_owned()),
        }
    }

    #[test]
    fn embedded_id_wins_over_a_stale_path() {
        let manifest = manifest(&[("a", "A", "NewAlbum")]);
        let (state, orphans) =
            DeviceState::from_contents(&manifest, &[identified("Music/A/OldAlbum/A - a.mp3", "a")]);

        assert_eq!(
            state.entries["a"].path,
            PathBuf::from("Music/A/OldAlbum/A - a.mp3")
        );
        assert!(orphans.is_empty());
    }

    #[test]
    fn file_name_matches_when_only_the_album_changed() {
        let manifest = manifest(&[("a", "A", "NewAlbum")]);
        let (state, orphans) =
            DeviceState::from_contents(&manifest, &[at("Music/A/OldAlbum/A - a.mp3")]);

        assert!(state.entries.contains_key("a"));
        assert!(orphans.is_empty());
    }

    #[test]
    fn unknown_embedded_id_falls_back_to_path() {
        let manifest = manifest(&[("a", "A", "Al")]);
        let (state, orphans) = DeviceState::from_contents(
            &manifest,
            &[identified("Music/A/Al/A - a.mp3", "deleted-from-spotify")],
        );

        assert!(state.entries.contains_key("a"));
        assert!(orphans.is_empty());
    }

    #[test]
    fn carries_library_path_for_copying() {
        let manifest = manifest(&[("a", "A", "Al")]);
        let (state, _) = DeviceState::from_contents(&manifest, &[at("Music/A/Al/A - a.mp3")]);

        assert_eq!(state.entries["a"].library_path, PathBuf::from("A - a.mp3"));
    }
}

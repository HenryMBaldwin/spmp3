use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum ManifestError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("manifest json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("manifest version {found} is newer than the supported version {supported}")]
    Version { found: u32, supported: u32 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Manifest {
    pub version: u32,
    #[serde(default)]
    pub entries: BTreeMap<String, Entry>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            version: MANIFEST_VERSION,
            entries: BTreeMap::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub uri: String,
    pub path: PathBuf,
    pub added_at: Option<i64>,
    #[serde(default = "liked_default")]
    pub liked: bool,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub album: String,
    #[serde(default)]
    pub source_format: String,
    #[serde(default)]
    pub encoder: String,
}

const fn liked_default() -> bool {
    true
}

impl Manifest {
    /// # Errors
    ///
    /// Returns [`ManifestError`] if the file exists but cannot be read or parsed, or if
    /// it was written by a newer version of this crate.
    pub fn load(path: &Path) -> Result<Self, ManifestError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };

        let manifest: Self = serde_json::from_slice(&bytes)?;
        if manifest.version > MANIFEST_VERSION {
            return Err(ManifestError::Version {
                found: manifest.version,
                supported: MANIFEST_VERSION,
            });
        }

        Ok(manifest)
    }

    /// # Errors
    ///
    /// Returns [`ManifestError::Io`] if the manifest cannot be written or renamed.
    pub fn save(&self, path: &Path) -> Result<(), ManifestError> {
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }

    pub fn contains(&self, id: &str) -> bool {
        self.entries.contains_key(id)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn liked(&self) -> impl Iterator<Item = (&String, &Entry)> {
        self.entries.iter().filter(|(_, entry)| entry.liked)
    }
}

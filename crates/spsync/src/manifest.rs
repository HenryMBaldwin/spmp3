use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::SpsyncError;

pub(crate) const MANIFEST_FILE: &str = "manifest.json";
pub const MANIFEST_VERSION: u32 = 1;

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
    /// Returns [`SpsyncError::Io`] if the file exists but cannot be read, or
    /// [`SpsyncError::Json`] if it is not valid manifest json.
    pub fn load(path: &Path) -> Result<Self, SpsyncError> {
        let bytes = match fs::read(path) {
            Ok(bytes) => bytes,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e.into()),
        };

        let manifest: Self = serde_json::from_slice(&bytes)?;
        if manifest.version > MANIFEST_VERSION {
            return Err(SpsyncError::ManifestVersion {
                found: manifest.version,
                supported: MANIFEST_VERSION,
            });
        }

        Ok(manifest)
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::Io`] if the manifest cannot be written or renamed.
    pub fn save(&self, path: &Path) -> Result<(), SpsyncError> {
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
}

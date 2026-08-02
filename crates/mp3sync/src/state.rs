use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::Mp3syncError;

pub const STATE_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceState {
    pub version: u32,
    #[serde(default)]
    pub source_hash: Option<String>,
    #[serde(default)]
    pub entries: BTreeMap<String, DeviceEntry>,
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

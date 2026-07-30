use std::{
    collections::BTreeMap,
    fs, io,
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};

use crate::error::SpsyncError;

pub(crate) const MANIFEST_FILE: &str = "manifest.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Manifest {
    pub entries: BTreeMap<String, Entry>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    pub uri: String,
    pub path: PathBuf,
    pub added_at: Option<i64>,
}

impl Manifest {
    /// # Errors
    ///
    /// Returns [`SpsyncError::Io`] if the file exists but cannot be read, or
    /// [`SpsyncError::Json`] if it is not valid manifest json.
    pub fn load(path: &Path) -> Result<Self, SpsyncError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
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

use std::{
    fs, io,
    path::Path,
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const STATUS_FILE: &str = "status.json";

#[derive(Debug, Error)]
pub enum StatusError {
    #[error("io error: {0}")]
    Io(#[from] io::Error),

    #[error("status json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// Seconds since the unix epoch, saturating to 0 before 1970.
#[must_use]
pub fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Library {
    pub tracks: usize,
    pub liked: usize,
    pub bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Run {
    pub finished_at: i64,
    pub added: usize,
    pub restored: usize,
    pub removed: usize,
    pub missing_covers: usize,
    pub took_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Failure {
    pub uri: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub title: String,
    pub error: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Device {
    pub synced_at: i64,
    pub files: usize,
    pub pending: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Status {
    pub updated_at: i64,
    #[serde(default)]
    pub authenticated: bool,
    #[serde(default)]
    pub library: Library,
    #[serde(default)]
    pub last_run: Option<Run>,
    #[serde(default)]
    pub failures: Vec<Failure>,
    #[serde(default)]
    pub device: Option<Device>,
}

impl Status {
    /// # Errors
    ///
    /// Returns [`StatusError`] if the file exists but cannot be read or parsed.
    pub fn load(path: &Path) -> Result<Self, StatusError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    /// # Errors
    ///
    /// Returns [`StatusError::Io`] if the status cannot be written or renamed.
    pub fn save(&self, path: &Path) -> Result<(), StatusError> {
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, path)?;

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::expect_used)]

    use super::{Failure, Library, Run, Status};

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("status.json");

        let status = Status {
            updated_at: 42,
            authenticated: true,
            library: Library {
                tracks: 1701,
                liked: 1688,
                bytes: 12_562_138_921,
            },
            last_run: Some(Run {
                finished_at: 41,
                added: 3,
                ..Run::default()
            }),
            failures: vec![Failure {
                uri: "spotify:track:abc".to_owned(),
                artist: "Sonny Rollins".to_owned(),
                title: "Valse Hot".to_owned(),
                error: "unavailable".to_owned(),
            }],
            device: None,
        };

        status.save(&path).expect("save");
        let loaded = Status::load(&path).expect("load");

        assert_eq!(loaded.library.liked, 1688);
        assert_eq!(loaded.failures, status.failures);
        assert_eq!(loaded.last_run.expect("run").added, 3);
    }

    #[test]
    fn missing_file_is_default() {
        let dir = tempfile::tempdir().expect("tempdir");
        let status = Status::load(&dir.path().join("nope.json")).expect("load");

        assert_eq!(status.updated_at, 0);
        assert!(status.failures.is_empty());
    }
}

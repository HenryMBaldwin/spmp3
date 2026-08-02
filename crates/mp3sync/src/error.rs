use std::path::PathBuf;

use common::{config::ConfigError, manifest::ManifestError};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum Mp3syncError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("manifest error: {0}")]
    Manifest(#[from] ManifestError),

    #[error("device state json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("device is not mounted at {path}")]
    DeviceNotMounted { path: PathBuf },

    #[error("device state version {found} is newer than the supported version {supported}")]
    StateVersion { found: u32, supported: u32 },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

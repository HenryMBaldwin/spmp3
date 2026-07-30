use std::path::PathBuf;

use common::config::ConfigError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SpsyncError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("no cached spotify credentials at {path}; run an interactive login to authorize")]
    NotAuthenticated { path: PathBuf },

    #[error("spotify error: {0}")]
    Spotify(#[from] librespot_core::Error),

    #[error("spotify oauth error: {0}")]
    OAuth(#[from] librespot_oauth::OAuthError),

    #[error("interactive login did not complete")]
    LoginAborted,

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("manifest json error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("manifest version {found} is newer than the supported version {supported}")]
    ManifestVersion { found: u32, supported: u32 },

    #[error("{uri} is not a track uri")]
    UnsupportedUri { uri: String },

    #[error("{uri} is unavailable and has no playable alternative")]
    TrackUnavailable { uri: String },

    #[error("{uri} has no supported audio format")]
    NoSupportedFormat { uri: String },

    #[error("download did not complete")]
    DownloadAborted,

    #[error("audio decode error: {0}")]
    Symphonia(#[from] symphonia::core::errors::Error),

    #[error("transcode error: {0}")]
    Transcode(String),
}

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
}

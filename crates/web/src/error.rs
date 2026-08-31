use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
};
use common::{
    config::ConfigError, manifest::ManifestError, overrides::OverrideError, status::StatusError,
};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum WebError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),

    #[error("manifest error: {0}")]
    Manifest(#[from] ManifestError),

    #[error("status error: {0}")]
    Status(#[from] StatusError),

    #[error("overrides error: {0}")]
    Overrides(#[from] OverrideError),

    #[error("unknown track {id}")]
    UnknownTrack { id: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

impl IntoResponse for WebError {
    fn into_response(self) -> Response {
        let status = match self {
            Self::UnknownTrack { .. } => StatusCode::NOT_FOUND,
            Self::Overrides(OverrideError::BadUrl(_)) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };

        tracing::warn!(error = %self, "request failed");

        (status, self.to_string()).into_response()
    }
}

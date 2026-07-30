mod auth;
mod config;
mod diff;
mod download;
mod error;
mod library;
mod manifest;
mod session;
mod track;

use std::{fmt, fs, path::PathBuf, sync::Arc};

pub use librespot_core::Session;
use librespot_core::cache::Cache;

pub use crate::{
    config::Config,
    diff::{Diff, Removed},
    download::{TrackAudio, TrackMeta},
    error::SpsyncError,
    manifest::{Entry, Manifest},
    track::TrackRef,
};
use crate::{manifest::MANIFEST_FILE, session::SessionManager};

#[derive(Clone)]
pub struct Client {
    inner: Arc<Inner>,
}

struct Inner {
    config: Config,
    sessions: SessionManager,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("config", &self.inner.config)
            .finish_non_exhaustive()
    }
}

impl Client {
    /// # Errors
    ///
    /// Returns [`SpsyncError::Io`] if the cache or library directories cannot be created.
    pub fn new(config: Config) -> Result<Self, SpsyncError> {
        fs::create_dir_all(&config.library_dir)?;

        let cache = Cache::new(Some(config.cache_dir.as_path()), None, None, None)?;
        let sessions = SessionManager::new(cache, config.cache_dir.clone());

        Ok(Self {
            inner: Arc::new(Inner { config, sessions }),
        })
    }

    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    pub fn is_authenticated(&self) -> bool {
        self.inner.sessions.has_credentials()
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::LoginAborted`] if the browser step never completes, or
    /// [`SpsyncError::Spotify`] if Spotify rejects the resulting token.
    pub async fn login(&self, open_browser: bool) -> Result<String, SpsyncError> {
        let client_id = self.inner.sessions.client_id();
        let credentials =
            auth::interactive_login(client_id, auth::DEFAULT_OAUTH_PORT, open_browser).await?;
        let session = self.inner.sessions.replace(credentials).await?;

        Ok(session.username())
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached.
    pub async fn whoami(&self) -> Result<String, SpsyncError> {
        Ok(self.inner.sessions.get().await?.username())
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached.
    pub async fn session(&self) -> Result<Session, SpsyncError> {
        self.inner.sessions.get().await
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.inner.config.library_dir.join(MANIFEST_FILE)
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::Json`] if the manifest on disk is malformed.
    pub fn manifest(&self) -> Result<Manifest, SpsyncError> {
        Manifest::load(&self.manifest_path())
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached, or
    /// [`SpsyncError::Spotify`] if the collection cannot be fetched.
    pub async fn list_liked(&self) -> Result<Vec<TrackRef>, SpsyncError> {
        let session = self.inner.sessions.get().await?;
        library::list_liked(&session).await
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::TrackUnavailable`] if the track and all its alternatives
    /// are unplayable, or [`SpsyncError::NoSupportedFormat`] if none is ogg vorbis.
    pub async fn download(&self, track: &TrackRef) -> Result<TrackAudio, SpsyncError> {
        let session = self.inner.sessions.get().await?;
        let uri = librespot_core::SpotifyUri::from_uri(&track.uri)?;

        download::download(&session, &uri).await
    }

    /// # Errors
    ///
    /// Returns [`SpsyncError::NotAuthenticated`] if no credentials are cached, or
    /// [`SpsyncError::Json`] if the manifest on disk is malformed.
    pub async fn sync_diff(&self) -> Result<Diff, SpsyncError> {
        let remote = self.list_liked().await?;
        let manifest = self.manifest()?;

        Ok(diff::diff(&remote, &manifest))
    }
}

mod auth;
mod config;
mod error;
mod session;

use std::{fmt, fs, sync::Arc};

use librespot_core::cache::Cache;

use crate::session::SessionManager;
pub use crate::{config::Config, error::SpsyncError};

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
}

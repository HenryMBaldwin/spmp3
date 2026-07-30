use std::{fmt, path::PathBuf};

use librespot_core::{Session, SessionConfig, authentication::Credentials, cache::Cache};
use tokio::sync::Mutex;

use crate::error::SpsyncError;

const CREDENTIALS_FILE: &str = "credentials.json";

pub(crate) struct SessionManager {
    config: SessionConfig,
    cache: Cache,
    cache_dir: PathBuf,
    current: Mutex<Option<Session>>,
}

impl fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionManager")
            .field("cache_dir", &self.cache_dir)
            .finish_non_exhaustive()
    }
}

impl SessionManager {
    pub(crate) fn new(cache: Cache, cache_dir: PathBuf) -> Self {
        Self {
            config: SessionConfig::default(),
            cache,
            cache_dir,
            current: Mutex::new(None),
        }
    }

    pub(crate) fn client_id(&self) -> String {
        self.config.client_id.clone()
    }

    pub(crate) fn has_credentials(&self) -> bool {
        self.cache.credentials().is_some()
    }

    pub(crate) async fn get(&self) -> Result<Session, SpsyncError> {
        let mut current = self.current.lock().await;

        if let Some(session) = current.as_ref() {
            if !session.is_invalid() {
                return Ok(session.clone());
            }
            tracing::warn!("spotify session invalidated, reconnecting");
            *current = None;
        }

        let credentials =
            self.cache
                .credentials()
                .ok_or_else(|| SpsyncError::NotAuthenticated {
                    path: self.cache_dir.join(CREDENTIALS_FILE),
                })?;

        let session = self.connect(credentials).await?;
        *current = Some(session.clone());

        Ok(session)
    }

    pub(crate) async fn replace(&self, credentials: Credentials) -> Result<Session, SpsyncError> {
        let mut current = self.current.lock().await;
        let session = self.connect(credentials).await?;
        *current = Some(session.clone());

        Ok(session)
    }

    async fn connect(&self, credentials: Credentials) -> Result<Session, SpsyncError> {
        let session = Session::new(self.config.clone(), Some(self.cache.clone()));
        session.connect(credentials, true).await?;
        tracing::info!(username = %session.username(), "connected to spotify");

        Ok(session)
    }
}

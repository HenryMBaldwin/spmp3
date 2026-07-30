use std::{env, path::PathBuf, str::FromStr};

use common::config::{ConfigError, try_get_env_parsed};

const CACHE_DIR: &str = "CACHE_DIR";
const LIBRARY_DIR: &str = "LIBRARY_DIR";
const PRESERVE: &str = "PRESERVE";
const REALTIME: &str = "REALTIME";

fn optional<T: FromStr>(key: &str, fallback: T) -> T {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[derive(Debug, Clone)]
pub struct Config {
    pub cache_dir: PathBuf,
    pub library_dir: PathBuf,
    pub preserve: bool,
    pub realtime: bool,
}

impl Config {
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a required variable is unset or cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            cache_dir: try_get_env_parsed(CACHE_DIR)?,
            library_dir: try_get_env_parsed(LIBRARY_DIR)?,
            preserve: try_get_env_parsed(PRESERVE)?,
            realtime: optional(REALTIME, true),
        })
    }
}

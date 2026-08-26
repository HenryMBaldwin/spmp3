use std::{env, net::SocketAddr, path::PathBuf, str::FromStr};

use common::config::{ConfigError, try_get_env_parsed};

const LIBRARY_DIR: &str = "LIBRARY_DIR";
const WEB_BIND: &str = "WEB_BIND";

pub const DEFAULT_WEB_BIND: &str = "0.0.0.0:8080";

fn optional<T: FromStr>(key: &str, fallback: T) -> T {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[derive(Debug, Clone)]
pub struct Config {
    pub library_dir: PathBuf,
    pub bind: SocketAddr,
}

impl Config {
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a required variable is unset or cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            library_dir: try_get_env_parsed(LIBRARY_DIR)?,
            bind: optional(
                WEB_BIND,
                DEFAULT_WEB_BIND
                    .parse()
                    .unwrap_or(SocketAddr::from(([0, 0, 0, 0], 8080))),
            ),
        })
    }
}

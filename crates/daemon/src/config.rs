use std::{env, str::FromStr, time::Duration};

use common::config::ConfigError;

const LIBRARY_INTERVAL_SECS: &str = "LIBRARY_INTERVAL_SECS";
const DEVICE_POLL_SECS: &str = "DEVICE_POLL_SECS";
const OVERRIDE_POLL_SECS: &str = "OVERRIDE_POLL_SECS";

pub const DEFAULT_LIBRARY_INTERVAL_SECS: u64 = 1800;
pub const DEFAULT_DEVICE_POLL_SECS: u64 = 5;
pub const DEFAULT_OVERRIDE_POLL_SECS: u64 = 15;

fn optional<T: FromStr>(key: &str, fallback: T) -> T {
    env::var(key)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(fallback)
}

#[derive(Debug, Clone)]
pub struct Config {
    pub spotify: spsync::Config,
    pub device: mp3sync::Config,
    pub library_interval: Duration,
    pub device_poll: Duration,
    pub override_poll: Duration,
}

impl Config {
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a required variable is unset or cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            spotify: spsync::Config::from_env()?,
            device: mp3sync::Config::from_env()?,
            library_interval: Duration::from_secs(optional(
                LIBRARY_INTERVAL_SECS,
                DEFAULT_LIBRARY_INTERVAL_SECS,
            )),
            device_poll: Duration::from_secs(optional(DEVICE_POLL_SECS, DEFAULT_DEVICE_POLL_SECS)),
            override_poll: Duration::from_secs(optional(
                OVERRIDE_POLL_SECS,
                DEFAULT_OVERRIDE_POLL_SECS,
            )),
        })
    }
}

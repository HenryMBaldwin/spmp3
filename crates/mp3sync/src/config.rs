use std::path::PathBuf;

use common::config::{ConfigError, try_get_env_parsed};

const LIBRARY_DIR: &str = "LIBRARY_DIR";
const DEVICE_STATE: &str = "DEVICE_STATE";
const MOUNT_DIR: &str = "MOUNT_DIR";

#[derive(Debug, Clone)]
pub struct Config {
    pub library_dir: PathBuf,
    pub device_state: PathBuf,
    pub mount_dir: PathBuf,
}

impl Config {
    /// # Errors
    ///
    /// Returns [`ConfigError`] if a required variable is unset or cannot be parsed.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            library_dir: try_get_env_parsed(LIBRARY_DIR)?,
            device_state: try_get_env_parsed(DEVICE_STATE)?,
            mount_dir: try_get_env_parsed(MOUNT_DIR)?,
        })
    }
}

use std::{any::type_name, env};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("environment variable {0} is not set")]
    NotSet(String),
    #[error("environment variable {value} cannot be parsed to {type_name}: {error}")]
    Invalid {
        value: String,
        type_name: &'static str,
        error: String,
    },
}

/// Attempts to read an environment variable.
///
/// # Errors
///
/// Returns [`ConfigError::NotSet`] if the environment variable is not set.
pub fn try_get_env(key: &str) -> Result<String, ConfigError> {
    env::var(key).map_err(|_| ConfigError::NotSet(key.to_string()))
}

/// Attempts to read an environment variable and parsei it to T
///
/// # Errors
///
/// Returns [`ConfigError::NotSet`] if the environment variable is not set.
/// Returns [`ConfigError::Invalid`] if the environment variable is not set.
pub fn try_get_env_parsed<T>(key: &str) -> Result<T, ConfigError>
where
    T: std::str::FromStr,
    T::Err: std::fmt::Display,
{
    let value = try_get_env(key)?;
    value.parse::<T>().map_err(|e| ConfigError::Invalid {
        value: key.to_string(),
        type_name: type_name::<T>(),
        error: e.to_string(),
    })
}

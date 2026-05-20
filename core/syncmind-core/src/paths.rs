use anyhow::{Context, Result};
use std::path::PathBuf;

/// Override for the local data directory. Honored when set; falls back to
/// the platform default. Useful for running multiple isolated instances on
/// the same machine and for integration tests.
const DATA_DIR_ENV: &str = "SYNCMIND_DATA_DIR";

pub fn local_data_dir() -> Result<PathBuf> {
    if let Ok(custom) = std::env::var(DATA_DIR_ENV) {
        if !custom.is_empty() {
            return Ok(PathBuf::from(custom));
        }
    }
    let data_dir = dirs::data_local_dir()
        .context("Failed to determine local data directory")?;
    Ok(data_dir.join("syncmind"))
}

pub fn db_path() -> Result<PathBuf> {
    Ok(local_data_dir()?.join("syncmind.db"))
}

pub fn log_dir() -> Result<PathBuf> {
    Ok(local_data_dir()?.join("logs"))
}

pub fn model_cache_dir() -> Result<PathBuf> {
    Ok(local_data_dir()?.join("models"))
}

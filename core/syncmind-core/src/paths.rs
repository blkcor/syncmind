use anyhow::{Context, Result};
use std::path::PathBuf;

pub fn local_data_dir() -> Result<PathBuf> {
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

//! File system operations

use anyhow::Result;
use std::path::PathBuf;

/// Get the data directory path (~/.meeting-agent)
pub fn data_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().ok_or_else(|| anyhow::anyhow!("Could not find home directory"))?;
    Ok(home.join(".meeting-agent"))
}

/// Get the meetings directory path
pub fn meetings_dir() -> Result<PathBuf> {
    Ok(data_dir()?.join("meetings"))
}

/// Get the config file path
pub fn config_path() -> Result<PathBuf> {
    Ok(data_dir()?.join("config.json"))
}

/// Ensure data directory structure exists
pub fn ensure_data_dir() -> Result<()> {
    let data = data_dir()?;
    let meetings = meetings_dir()?;
    
    std::fs::create_dir_all(&data)?;
    std::fs::create_dir_all(&meetings)?;
    
    Ok(())
}

/// Get meeting directory path by ID
pub fn meeting_dir(meeting_id: &str) -> Result<PathBuf> {
    Ok(meetings_dir()?.join(meeting_id))
}

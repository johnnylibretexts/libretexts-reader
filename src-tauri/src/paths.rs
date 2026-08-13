use std::path::PathBuf;

use crate::error::{AppError, AppResult};

const APP_DIR_NAME: &str = "dev.johnnylibretexts.reader";

pub fn app_data_dir() -> AppResult<PathBuf> {
    let dir = match std::env::var_os("LIBRETEXTS_READER_APP_DATA_DIR") {
        // An empty override would resolve to the process working directory, so
        // treat it as unset and fall back to the platform location.
        Some(path) if !path.is_empty() => PathBuf::from(path),
        _ => platform_app_data_dir()?,
    };

    create_dir(&dir)?;
    Ok(dir)
}

pub fn database_path() -> AppResult<PathBuf> {
    Ok(app_data_dir()?.join("library.sqlite"))
}

pub fn models_dir() -> AppResult<PathBuf> {
    app_subdir("models")
}

pub fn voices_dir() -> AppResult<PathBuf> {
    app_subdir("voices")
}

pub fn covers_dir() -> AppResult<PathBuf> {
    app_subdir("covers")
}

pub fn images_dir() -> AppResult<PathBuf> {
    app_subdir("images")
}

pub fn cache_dir() -> AppResult<PathBuf> {
    app_subdir("cache")
}

pub fn temp_dir() -> AppResult<PathBuf> {
    app_subdir("temp")
}

fn app_subdir(name: &str) -> AppResult<PathBuf> {
    let dir = app_data_dir()?.join(name);
    create_dir(&dir)?;
    Ok(dir)
}

fn create_dir(path: &PathBuf) -> AppResult<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn platform_app_data_dir() -> AppResult<PathBuf> {
    let home = std::env::var_os("HOME")
        .ok_or_else(|| AppError::InvalidInput("HOME is not set".to_string()))?;

    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join(APP_DIR_NAME))
}

#[cfg(target_os = "windows")]
fn platform_app_data_dir() -> AppResult<PathBuf> {
    let appdata = std::env::var_os("APPDATA")
        .ok_or_else(|| AppError::InvalidInput("APPDATA is not set".to_string()))?;

    Ok(PathBuf::from(appdata).join(APP_DIR_NAME))
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn platform_app_data_dir() -> AppResult<PathBuf> {
    if let Some(data_home) = std::env::var_os("XDG_DATA_HOME") {
        return Ok(PathBuf::from(data_home).join(APP_DIR_NAME));
    }

    let home = std::env::var_os("HOME")
        .ok_or_else(|| AppError::InvalidInput("HOME is not set".to_string()))?;

    Ok(PathBuf::from(home)
        .join(".local")
        .join("share")
        .join(APP_DIR_NAME))
}

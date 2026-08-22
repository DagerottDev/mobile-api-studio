use super::AppState;
use core_model::{AppError, AppPreference};
use serde::Serialize;
use tauri::State;

pub const CAPTURE_EXECUTABLE_PREFERENCE: &str = "capture_executable";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureExecutableSetting {
    pub configured: Option<String>,
    pub effective_after_restart: String,
    pub uses_auto_discovery: bool,
}

#[tauri::command]
pub fn capture_executable_setting(
    state: State<'_, AppState>,
) -> Result<CaptureExecutableSetting, AppError> {
    let configured = state
        .database
        .get_preference(CAPTURE_EXECUTABLE_PREFERENCE)
        .map_err(storage_error)?
        .map(|preference| preference.value)
        .filter(|value| !value.trim().is_empty());

    Ok(CaptureExecutableSetting {
        effective_after_restart: configured
            .clone()
            .unwrap_or_else(|| "mitmdump (PATH auto-discovery)".into()),
        uses_auto_discovery: configured.is_none(),
        configured,
    })
}

#[tauri::command]
pub fn set_capture_executable(
    executable: Option<String>,
    state: State<'_, AppState>,
) -> Result<CaptureExecutableSetting, AppError> {
    let normalized = executable
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty());

    state
        .database
        .set_preference(&AppPreference {
            key: CAPTURE_EXECUTABLE_PREFERENCE.into(),
            value: normalized.clone().unwrap_or_default(),
        })
        .map_err(storage_error)?;

    Ok(CaptureExecutableSetting {
        effective_after_restart: normalized
            .clone()
            .unwrap_or_else(|| "mitmdump (PATH auto-discovery)".into()),
        uses_auto_discovery: normalized.is_none(),
        configured: normalized,
    })
}

pub fn configured_capture_executable(database: &storage::Database) -> Result<String, String> {
    database
        .get_preference(CAPTURE_EXECUTABLE_PREFERENCE)
        .map_err(|error| error.to_string())
        .map(|preference| {
            preference
                .map(|value| value.value)
                .filter(|value| !value.trim().is_empty())
                .unwrap_or_else(|| "mitmdump".into())
        })
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

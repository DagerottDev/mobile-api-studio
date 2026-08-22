use super::AppState;
use core_model::AppError;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tauri::State;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointStage {
    Request,
    Response,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointHeader {
    pub name: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointBody {
    pub data_base64: String,
    pub content_type: Option<String>,
    pub is_binary: bool,
    pub is_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingBreakpoint {
    pub schema_version: u16,
    pub id: String,
    pub flow_id: String,
    pub rule_id: String,
    pub rule_name: String,
    pub stage: BreakpointStage,
    pub created_at: String,
    pub deadline_at: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<BreakpointHeader>,
    pub body: Option<BreakpointBody>,
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BreakpointDecisionAction {
    Continue,
    Cancel,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BreakpointDecisionInput {
    pub id: String,
    pub action: BreakpointDecisionAction,
    pub method: Option<String>,
    pub url: Option<String>,
    pub headers: Option<Vec<BreakpointHeader>>,
    pub body: Option<BreakpointBody>,
    pub clear_body: bool,
    pub status_code: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct BreakpointDecisionDocument {
    schema_version: u16,
    id: String,
    action: BreakpointDecisionAction,
    method: Option<String>,
    url: Option<String>,
    headers: Option<Vec<BreakpointHeader>>,
    body: Option<BreakpointBody>,
    clear_body: bool,
    status_code: Option<u16>,
}

#[tauri::command]
pub fn list_pending_breakpoints(
    state: State<'_, AppState>,
) -> Result<Vec<PendingBreakpoint>, AppError> {
    let pending_dir = pending_directory(&state);
    if !pending_dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut pending = Vec::new();
    for entry in fs::read_dir(&pending_dir).map_err(io_error)? {
        let entry = entry.map_err(io_error)?;
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("json") {
            continue;
        }
        let bytes = fs::read(&path).map_err(io_error)?;
        match serde_json::from_slice::<PendingBreakpoint>(&bytes) {
            Ok(value) => pending.push(value),
            Err(_) => continue,
        }
    }

    pending.sort_by(|left, right| {
        left.created_at
            .cmp(&right.created_at)
            .then_with(|| left.id.cmp(&right.id))
    });
    Ok(pending)
}

#[tauri::command]
pub fn resolve_breakpoint(
    input: BreakpointDecisionInput,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    if input.id.trim().is_empty() {
        return Err(AppError::new(
            "breakpoint_id_required",
            "Breakpoint id is required.",
            true,
        ));
    }
    if let Some(status) = input.status_code {
        if !(100..=599).contains(&status) {
            return Err(AppError::new(
                "breakpoint_status_invalid",
                "Breakpoint response status must be between 100 and 599.",
                true,
            ));
        }
    }

    let decision_dir = decision_directory(&state);
    fs::create_dir_all(&decision_dir).map_err(io_error)?;
    let document = BreakpointDecisionDocument {
        schema_version: 1,
        id: input.id.clone(),
        action: input.action,
        method: input.method,
        url: input.url,
        headers: input.headers,
        body: input.body,
        clear_body: input.clear_body,
        status_code: input.status_code,
    };
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| AppError::new("breakpoint_serialize_failed", error.to_string(), true))?;
    atomic_write(&decision_dir.join(format!("{}.json", safe_id(&input.id))), &bytes)?;

    let pending_path = pending_directory(&state).join(format!("{}.json", safe_id(&input.id)));
    if pending_path.exists() {
        let _ = fs::remove_file(pending_path);
    }
    Ok(())
}

#[tauri::command]
pub fn clear_stale_breakpoints(state: State<'_, AppState>) -> Result<usize, AppError> {
    let mut removed = 0usize;
    let now = epoch_millis();
    let pending_dir = pending_directory(&state);
    if pending_dir.is_dir() {
        for entry in fs::read_dir(&pending_dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let expired = fs::read(&path)
                .ok()
                .and_then(|bytes| serde_json::from_slice::<PendingBreakpoint>(&bytes).ok())
                .and_then(|pending| pending.deadline_at.parse::<u128>().ok())
                .is_some_and(|deadline| deadline <= now);
            if expired && fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
    }

    let decision_dir = decision_directory(&state);
    if decision_dir.is_dir() {
        for entry in fs::read_dir(&decision_dir).map_err(io_error)? {
            let entry = entry.map_err(io_error)?;
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let stale = entry
                .metadata()
                .ok()
                .and_then(|metadata| metadata.modified().ok())
                .and_then(|modified| SystemTime::now().duration_since(modified).ok())
                .is_some_and(|age| age > Duration::from_secs(120));
            if stale && fs::remove_file(path).is_ok() {
                removed += 1;
            }
        }
    }
    Ok(removed)
}

fn pending_directory(state: &State<'_, AppState>) -> PathBuf {
    state.capture_engine.conf_dir().join("breakpoints").join("pending")
}

fn decision_directory(state: &State<'_, AppState>) -> PathBuf {
    state.capture_engine.conf_dir().join("breakpoints").join("decisions")
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes).map_err(io_error)?;
    fs::rename(&temporary, path).map_err(io_error)
}

fn safe_id(value: &str) -> String {
    value
        .chars()
        .filter(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        .take(160)
        .collect()
}

fn epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn io_error(error: std::io::Error) -> AppError {
    AppError::new("breakpoint_io_failed", error.to_string(), true)
}

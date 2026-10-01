use super::{replay_commands::redact_detail_for_ui, AppState};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use capture_core::CaptureEvent;
use core_model::{AppError, FlowDetail, WebSocketMessage};
use sdk_protocol::SDK_CORRELATION_HEADER;
use serde::Serialize;
use storage::{BodyStore, Database};
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyPayload {
    sha256: String,
    text: Option<String>,
    base64: Option<String>,
}

#[tauri::command]
pub fn get_flow_detail(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<Option<FlowDetail>, AppError> {
    state
        .database
        .get_flow_detail(&flow_id)
        .map(|detail| detail.map(redact_detail_for_ui))
        .map_err(|error| AppError::storage(error.to_string()))
}

#[tauri::command]
pub fn read_body(sha256: String, state: State<'_, AppState>) -> Result<BodyPayload, AppError> {
    let bytes = state
        .body_store
        .read_bounded(&sha256, 2 * 1024 * 1024)
        .map_err(|error| AppError::storage(error.to_string()))?;

    Ok(BodyPayload {
        sha256,
        text: std::str::from_utf8(&bytes).ok().map(str::to_owned),
        base64: Some(BASE64.encode(bytes)),
    })
}

#[tauri::command]
pub fn export_curl(flow_id: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let detail = state
        .database
        .get_flow_detail(&flow_id)
        .map_err(|error| AppError::storage(error.to_string()))?
        .ok_or_else(|| AppError::new("flow_detail_missing", "Flow details were not found.", true))?;

    let mut parts = vec![
        "curl".to_string(),
        "--request".to_string(),
        shell_quote(&detail.request.as_ref().map(|request| request.method.as_str()).unwrap_or("GET")),
    ];

    let request = detail.request.ok_or_else(|| {
        AppError::new("request_detail_missing", "Request details were not captured.", true)
    })?;
    parts.push(shell_quote(&request.url));

    for header in &request.headers {
        if header.name.eq_ignore_ascii_case(SDK_CORRELATION_HEADER) {
            continue;
        }
        let value = if header.sensitive { "<redacted>" } else { &header.value };
        parts.push("--header".to_string());
        parts.push(shell_quote(&format!("{}: {}", header.name, value)));
    }

    if let Some(body) = request.body {
        if !body.is_binary {
            let bytes = state
                .body_store
                .read(&body.sha256)
                .map_err(|error| AppError::storage(error.to_string()))?;
            if let Ok(text) = String::from_utf8(bytes) {
                parts.push("--data-raw".to_string());
                parts.push(shell_quote(&text));
            }
        }
    }

    Ok(parts.join(" "))
}

pub(super) fn ingest_capture_event(database: &Database, body_store: &BodyStore, event: CaptureEvent) -> Result<(), AppError> {
    app_core::ingest_capture_event_for_storage(database, body_store, event)
}

#[tauri::command]
pub fn list_websocket_messages(flow_id: Option<String>, session_id: Option<String>, text: Option<String>, limit: Option<usize>, offset: Option<usize>, state: State<'_, AppState>) -> Result<Vec<WebSocketMessage>, AppError> {
    state.database.list_websocket_messages(flow_id.as_deref(), session_id.as_deref(), text.as_deref(), limit.unwrap_or(200), offset.unwrap_or(0)).map_err(|error| AppError::storage(error.to_string()))
}

#[tauri::command]
pub fn decode_protocol_body(sha256: String, content_type: String, descriptor_base64: Option<String>, message_type: Option<String>, state: State<'_, AppState>) -> Result<app_core::ProtocolInspection, AppError> {
    let bytes = state.body_store.read_bounded(&sha256, 2 * 1024 * 1024).map_err(|error| AppError::storage(error.to_string()))?;
    app_core::inspect_protocol_bytes(&bytes, content_type, descriptor_base64.as_deref(), message_type.as_deref())
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    format!("'{}'", value.replace("'", "'\"'\"'"))
}

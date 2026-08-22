use super::{replay_commands::redact_detail_for_ui, AppState};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use capture_core::{CapturedBody, CapturedFlow, CaptureEvent};
use core_model::{AppError, BodyRef, FlowDetail, RequestDetail, ResponseDetail};
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
        .read(&sha256)
        .map_err(|error| AppError::storage(error.to_string()))?;

    match String::from_utf8(bytes.clone()) {
        Ok(text) => Ok(BodyPayload {
            sha256,
            text: Some(text),
            base64: None,
        }),
        Err(_) => Ok(BodyPayload {
            sha256,
            text: None,
            base64: Some(BASE64.encode(bytes)),
        }),
    }
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

pub(super) fn ingest_capture_event(
    database: &Database,
    body_store: &BodyStore,
    event: CaptureEvent,
) -> Result<(), AppError> {
    match event {
        CaptureEvent::FlowDetailCompleted(flow) => persist_captured_flow(database, body_store, flow),
        CaptureEvent::FlowStarted(flow)
        | CaptureEvent::FlowUpdated(flow)
        | CaptureEvent::FlowCompleted(flow) => database
            .upsert_flow(&flow)
            .map_err(|error| AppError::storage(error.to_string())),
        CaptureEvent::FlowFailed { .. }
        | CaptureEvent::LifecycleChanged(_)
        | CaptureEvent::EngineReady(_)
        | CaptureEvent::EngineFailed { .. }
        | CaptureEvent::EngineStopped => Ok(()),
    }
}

fn persist_captured_flow(
    database: &Database,
    body_store: &BodyStore,
    flow: CapturedFlow,
) -> Result<(), AppError> {
    let request_body = store_body(body_store, flow.request.body)?;
    let response = match flow.response {
        Some(response) => Some(ResponseDetail {
            status_code: response.status_code,
            reason: response.reason,
            headers: response.headers,
            body: store_body(body_store, response.body)?,
        }),
        None => None,
    };

    let detail = FlowDetail {
        summary: flow.summary,
        request: Some(RequestDetail {
            method: flow.request.method,
            url: flow.request.url,
            scheme: flow.request.scheme,
            host: flow.request.host,
            port: flow.request.port,
            path: flow.request.path,
            query: flow.request.query,
            headers: flow.request.headers,
            body: request_body,
        }),
        response,
        timing: flow.timing,
        error_code: flow.error_code,
        error_message: flow.error_message,
    };

    database
        .upsert_flow_detail(&detail)
        .map_err(|error| AppError::storage(error.to_string()))
}

fn store_body(body_store: &BodyStore, body: Option<CapturedBody>) -> Result<Option<BodyRef>, AppError> {
    let Some(body) = body else {
        return Ok(None);
    };
    let stored = body_store
        .put(&body.bytes)
        .map_err(|error| AppError::storage(error.to_string()))?;

    Ok(Some(BodyRef {
        sha256: stored.sha256,
        byte_size: stored.byte_size,
        content_type: body.content_type,
        encoding: body.encoding,
        is_binary: body.is_binary,
        is_truncated: body.is_truncated,
    }))
}

fn shell_quote(value: &str) -> String {
    if value.is_empty() {
        return "''".into();
    }
    format!("'{}'", value.replace("'", "'\"'\"'"))
}

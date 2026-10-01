use super::{replay_commands::redact_detail_for_ui, AppState};
use crate::State;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use capture_core::{CaptureEvent, CapturedBody, CapturedFlow, CapturedWebSocketMessage};
use core_model::{AppError, BodyRef, FlowDetail, RequestDetail, ResponseDetail, WebSocketMessage};
use sdk_protocol::SDK_CORRELATION_HEADER;
use serde::Serialize;
use storage::{BodyStore, Database};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BodyPayload {
    sha256: String,
    text: Option<String>,
    base64: Option<String>,
}

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

pub fn list_websocket_messages(
    flow_id: Option<String>, session_id: Option<String>, text: Option<String>,
    limit: Option<usize>, offset: Option<usize>, state: State<'_, AppState>,
) -> Result<Vec<WebSocketMessage>, AppError> {
    state.database.list_websocket_messages(flow_id.as_deref(), session_id.as_deref(), text.as_deref(), limit.unwrap_or(200), offset.unwrap_or(0))
        .map_err(|error| AppError::storage(error.to_string()))
}

pub fn export_curl(flow_id: String, state: State<'_, AppState>) -> Result<String, AppError> {
    let detail = state
        .database
        .get_flow_detail(&flow_id)
        .map_err(|error| AppError::storage(error.to_string()))?
        .ok_or_else(|| {
            AppError::new("flow_detail_missing", "Flow details were not found.", true)
        })?;

    let mut parts = vec![
        "curl".to_string(),
        "--request".to_string(),
        shell_quote(
            &detail
                .request
                .as_ref()
                .map(|request| request.method.as_str())
                .unwrap_or("GET"),
        ),
    ];

    let request = detail.request.ok_or_else(|| {
        AppError::new(
            "request_detail_missing",
            "Request details were not captured.",
            true,
        )
    })?;
    parts.push(shell_quote(&request.url));

    for header in &request.headers {
        if header.name.eq_ignore_ascii_case(SDK_CORRELATION_HEADER) {
            continue;
        }
        let value = if header.sensitive {
            "<redacted>"
        } else {
            &header.value
        };
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

pub fn ingest_capture_event(
    database: &Database,
    body_store: &BodyStore,
    event: CaptureEvent,
) -> Result<(), AppError> {
    match event {
        CaptureEvent::FlowDetailCompleted(flow) => {
            persist_captured_flow(database, body_store, flow)
        }
        CaptureEvent::WebSocketMessage(message) => persist_websocket_message(database, body_store, message),
        CaptureEvent::WebSocketClosed { flow_id, close_code, close_reason, closed_by_client } => database
            .update_websocket_close(&flow_id, close_code, close_reason, closed_by_client)
            .map(|_| ()).map_err(|error| AppError::storage(error.to_string())),
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
        proxy_rule_ids: flow.proxy_rule_ids,
        proxy_rule_changes: flow.proxy_rule_changes,
        protocol: flow.protocol,
    };

    database
        .upsert_flow_detail(&detail)
        .map_err(|error| AppError::storage(error.to_string()))?;
    crate::search_index::index_flow(database, body_store, &detail)
}

fn persist_websocket_message(database: &Database, body_store: &BodyStore, message: CapturedWebSocketMessage) -> Result<(), AppError> {
    let search_text = message.body.as_ref().filter(|body| !body.is_binary)
        .map(|body| crate::search_index::redacted_body_text(&body.bytes, body.content_type.as_deref()))
        .transpose()?.unwrap_or_default();
    let record = WebSocketMessage {
        id: message.id, flow_id: message.flow_id, session_id: message.session_id,
        sequence: message.sequence, from_client: message.from_client, opcode: message.opcode,
        timestamp: message.timestamp, dropped: message.dropped, injected: message.injected,
        body: store_body(body_store, message.body)?,
    };
    database.upsert_websocket_message(&record, &search_text).map_err(|error| AppError::storage(error.to_string()))
}

fn store_body(
    body_store: &BodyStore,
    body: Option<CapturedBody>,
) -> Result<Option<BodyRef>, AppError> {
    let Some(body) = body else {
        return Ok(None);
    };
    if body.bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::new("capture_body_too_large", "Captured payload exceeds 2 MiB.", true));
    }
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

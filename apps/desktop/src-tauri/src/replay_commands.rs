use super::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::{
    AppError, BodyRef, FlowDetail, FlowSource, FlowSummary, HeaderValue, RequestDetail,
    ResponseDetail, Timing, SCHEMA_VERSION,
};
use replay::{
    ReplayBodyDraft, ReplayDraft, ReplayEngine, ReplayError, ReplayHeaderDraft, ReplayRequest,
};
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::State;

#[tauri::command]
pub fn create_replay_draft(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<ReplayDraft, AppError> {
    let detail = load_detail(&state, &flow_id)?;
    let request = detail.request.ok_or_else(|| {
        AppError::new(
            "replay_request_missing",
            "The selected flow does not contain captured request details.",
            true,
        )
    })?;

    let headers = request
        .headers
        .iter()
        .enumerate()
        .map(|(index, header)| ReplayHeaderDraft {
            name: header.name.clone(),
            value: (!header.sensitive).then(|| header.value.clone()),
            sensitive: header.sensitive,
            use_original: header.sensitive,
            enabled: true,
            source_index: Some(index),
        })
        .collect();

    let body = request
        .body
        .as_ref()
        .map(|body_ref| build_body_draft(&state, body_ref))
        .transpose()?;

    Ok(ReplayDraft {
        source_flow_id: flow_id,
        method: request.method,
        url: request.url,
        headers,
        body,
    })
}

#[tauri::command]
pub async fn send_replay(
    draft: ReplayDraft,
    state: State<'_, AppState>,
) -> Result<FlowDetail, AppError> {
    let source_detail = load_detail(&state, &draft.source_flow_id)?;
    let source_request = source_detail.request.as_ref().ok_or_else(|| {
        AppError::new(
            "replay_request_missing",
            "The source flow no longer contains request details.",
            true,
        )
    })?;

    let request = resolve_request(&state, &draft, source_request)?;
    let engine = ReplayEngine::new().map_err(replay_error_to_app_error)?;
    let execution = engine
        .execute(request)
        .await
        .map_err(replay_error_to_app_error)?;

    let flow_id = format!("replay-{}", epoch_nanos());
    let request_body = execution
        .request
        .body
        .as_ref()
        .map(|bytes| {
            store_body_ref(
                &state,
                bytes,
                execution.request.content_type.clone(),
                execution.request.body_is_binary,
                false,
            )
        })
        .transpose()?;
    let response_body = if execution.response_body.is_empty() {
        None
    } else {
        Some(store_body_ref(
            &state,
            &execution.response_body,
            execution.response_content_type.clone(),
            is_binary_content_type(execution.response_content_type.as_deref()),
            execution.response_body_truncated,
        )?)
    };

    let summary = FlowSummary {
        schema_version: SCHEMA_VERSION,
        id: flow_id,
        session_id: source_detail.summary.session_id.clone(),
        source: FlowSource::Replay,
        method: execution.request.method.clone(),
        host: execution.host.clone(),
        path: execution.path.clone(),
        status_code: Some(execution.status_code),
        duration_ms: Some(execution.total_ms),
        response_size_bytes: Some(execution.response_body.len() as u64),
        started_at: execution.started_at.clone(),
    };

    let detail = FlowDetail {
        summary,
        request: Some(RequestDetail {
            method: execution.request.method,
            url: execution.request.url,
            scheme: execution.scheme,
            host: execution.host,
            port: execution.port,
            path: execution.path,
            query: execution.query,
            headers: execution.request.headers,
            body: request_body,
        }),
        response: Some(ResponseDetail {
            status_code: execution.status_code,
            reason: execution.reason,
            headers: execution.response_headers,
            body: response_body,
        }),
        timing: Timing {
            total_ms: Some(execution.total_ms),
            ..Timing::default()
        },
        error_code: None,
        error_message: None,
    };

    state
        .database
        .upsert_flow_detail(&detail)
        .map_err(|error| AppError::storage(error.to_string()))?;

    Ok(redact_detail_for_ui(detail))
}

fn load_detail(state: &State<'_, AppState>, flow_id: &str) -> Result<FlowDetail, AppError> {
    state
        .database
        .get_flow_detail(flow_id)
        .map_err(|error| AppError::storage(error.to_string()))?
        .ok_or_else(|| {
            AppError::new(
                "flow_detail_missing",
                "Full request details are required before this flow can be replayed.",
                true,
            )
        })
}

fn build_body_draft(state: &State<'_, AppState>, body_ref: &BodyRef) -> Result<ReplayBodyDraft, AppError> {
    let bytes = state
        .body_store
        .read(&body_ref.sha256)
        .map_err(|error| AppError::storage(error.to_string()))?;

    let (text, base64, is_binary) = if body_ref.is_binary {
        (None, Some(BASE64.encode(&bytes)), true)
    } else {
        match String::from_utf8(bytes.clone()) {
            Ok(text) => (Some(text), None, false),
            Err(_) => (None, Some(BASE64.encode(bytes)), true),
        }
    };

    Ok(ReplayBodyDraft {
        text,
        base64,
        is_binary,
        content_type: body_ref.content_type.clone(),
        use_original: !body_ref.is_truncated,
        source_truncated: body_ref.is_truncated,
    })
}

fn resolve_request(
    state: &State<'_, AppState>,
    draft: &ReplayDraft,
    source_request: &RequestDetail,
) -> Result<ReplayRequest, AppError> {
    let mut headers = Vec::new();
    for header in draft.headers.iter().filter(|header| header.enabled) {
        let (value, sensitive) = if header.use_original {
            let index = header.source_index.ok_or_else(|| {
                AppError::new(
                    "replay_header_source_missing",
                    format!("Header '{}' has no captured source value.", header.name),
                    true,
                )
            })?;
            let original = source_request.headers.get(index).ok_or_else(|| {
                AppError::new(
                    "replay_header_source_missing",
                    format!("Captured source header '{}' is no longer available.", header.name),
                    true,
                )
            })?;
            (original.value.clone(), original.sensitive)
        } else {
            (
                header.value.clone().unwrap_or_default(),
                header.sensitive || replay::is_sensitive_header(&header.name),
            )
        };

        headers.push(HeaderValue {
            name: header.name.clone(),
            value,
            sensitive,
        });
    }

    let (body, body_is_binary, body_content_type) = match &draft.body {
        None => (None, false, None),
        Some(body) if body.use_original => {
            if body.source_truncated {
                return Err(AppError::new(
                    "replay_body_truncated",
                    "The captured request body was truncated. Replace or edit the body before replaying it.",
                    true,
                ));
            }
            let body_ref = source_request.body.as_ref().ok_or_else(|| {
                AppError::new(
                    "replay_body_source_missing",
                    "The original request body is no longer available.",
                    true,
                )
            })?;
            let bytes = state
                .body_store
                .read(&body_ref.sha256)
                .map_err(|error| AppError::storage(error.to_string()))?;
            (Some(bytes), body_ref.is_binary, body_ref.content_type.clone())
        }
        Some(body) if body.is_binary => {
            let encoded = body.base64.as_deref().unwrap_or_default();
            let bytes = BASE64.decode(encoded.as_bytes()).map_err(|error| {
                AppError::new(
                    "replay_body_invalid_base64",
                    format!("Binary replay body is not valid base64: {error}"),
                    true,
                )
            })?;
            (Some(bytes), true, body.content_type.clone())
        }
        Some(body) => (
            Some(body.text.clone().unwrap_or_default().into_bytes()),
            false,
            body.content_type.clone(),
        ),
    };

    let content_type = headers
        .iter()
        .find(|header| header.name.eq_ignore_ascii_case("content-type"))
        .map(|header| header.value.clone())
        .or(body_content_type);

    Ok(ReplayRequest {
        source_flow_id: draft.source_flow_id.clone(),
        method: draft.method.trim().to_uppercase(),
        url: draft.url.trim().to_string(),
        headers,
        body,
        content_type,
        body_is_binary,
    })
}

fn store_body_ref(
    state: &State<'_, AppState>,
    bytes: &[u8],
    content_type: Option<String>,
    is_binary: bool,
    is_truncated: bool,
) -> Result<BodyRef, AppError> {
    let stored = state
        .body_store
        .put(bytes)
        .map_err(|error| AppError::storage(error.to_string()))?;
    Ok(BodyRef {
        sha256: stored.sha256,
        byte_size: stored.byte_size,
        content_type,
        encoding: None,
        is_binary,
        is_truncated,
    })
}

pub(super) fn redact_detail_for_ui(mut detail: FlowDetail) -> FlowDetail {
    if let Some(request) = detail.request.as_mut() {
        for header in &mut request.headers {
            if header.sensitive {
                header.value = "<redacted>".into();
            }
        }
    }
    if let Some(response) = detail.response.as_mut() {
        for header in &mut response.headers {
            if header.sensitive {
                header.value = "<redacted>".into();
            }
        }
    }
    detail
}

fn replay_error_to_app_error(error: ReplayError) -> AppError {
    AppError::new(error.code, error.message, error.recoverable)
}

fn is_binary_content_type(content_type: Option<&str>) -> bool {
    let Some(content_type) = content_type else {
        return true;
    };
    let normalized = content_type.to_ascii_lowercase();
    !(normalized.starts_with("text/")
        || normalized.contains("json")
        || normalized.contains("xml")
        || normalized.contains("javascript")
        || normalized.contains("x-www-form-urlencoded")
        || normalized.contains("graphql"))
}

fn epoch_nanos() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

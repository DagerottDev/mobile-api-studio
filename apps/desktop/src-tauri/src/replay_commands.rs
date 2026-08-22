use super::AppState;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::{
    AppError, BodyRef, FlowDetail, FlowSource, FlowSummary, HeaderValue, RequestDetail,
    ResponseDetail, SavedRequest, SavedRequestBody, Timing, SCHEMA_VERSION,
};
use replay::{
    ReplayBodyDraft, ReplayDraft, ReplayEngine, ReplayError, ReplayHeaderDraft, ReplayRequest,
};
use secret_store::SecretStore;
use std::{
    collections::{HashMap, HashSet},
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use workspace_core::interpolate;

const SAVED_SOURCE_PREFIX: &str = "saved:";

#[tauri::command]
pub fn create_replay_draft(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<ReplayDraft, AppError> {
    if let Some(request_id) = flow_id.strip_prefix(SAVED_SOURCE_PREFIX) {
        return create_saved_request_draft(&state, request_id);
    }

    let detail = load_detail(&state, &flow_id)?;
    let request = detail.request.ok_or_else(|| {
        AppError::new(
            "replay_request_missing",
            "The selected flow does not contain captured request details.",
            true,
        )
    })?;

    let headers = build_header_drafts(&request.headers);
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
    let (request_template, session_id) = if let Some(request_id) = draft.source_flow_id.strip_prefix(SAVED_SOURCE_PREFIX) {
        let saved = load_saved_request(&state, request_id)?;
        let session_id = saved
            .source_flow_id
            .as_deref()
            .and_then(|flow_id| state.database.get_flow_detail(flow_id).ok().flatten())
            .and_then(|detail| detail.summary.session_id);
        (resolve_saved_request(&state, &draft, &saved)?, session_id)
    } else {
        let source_detail = load_detail(&state, &draft.source_flow_id)?;
        let source_request = source_detail.request.as_ref().ok_or_else(|| {
            AppError::new(
                "replay_request_missing",
                "The source flow no longer contains request details.",
                true,
            )
        })?;
        (
            resolve_flow_request(&state, &draft, source_request)?,
            source_detail.summary.session_id,
        )
    };

    let (execution_request, persisted_request, secret_url) =
        interpolate_replay_request(&state, request_template)?;
    let engine = ReplayEngine::new().map_err(replay_error_to_app_error)?;
    let execution = engine
        .execute(execution_request)
        .await
        .map_err(replay_error_to_app_error)?;

    let flow_id = format!("replay-{}", epoch_nanos());
    let request_body = persisted_request
        .body
        .as_ref()
        .map(|bytes| {
            store_body_ref(
                &state,
                bytes,
                persisted_request.content_type.clone(),
                persisted_request.body_is_binary,
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

    let persisted_host = if secret_url {
        "<templated-host>".to_string()
    } else {
        execution.host.clone()
    };
    let persisted_path = if secret_url {
        "<templated-path>".to_string()
    } else {
        execution.path.clone()
    };

    let summary = FlowSummary {
        schema_version: SCHEMA_VERSION,
        id: flow_id,
        session_id,
        source: FlowSource::Replay,
        method: persisted_request.method.clone(),
        host: persisted_host.clone(),
        path: persisted_path.clone(),
        status_code: Some(execution.status_code),
        duration_ms: Some(execution.total_ms),
        response_size_bytes: Some(execution.response_body.len() as u64),
        started_at: execution.started_at.clone(),
    };

    let detail = FlowDetail {
        summary,
        request: Some(RequestDetail {
            method: persisted_request.method,
            url: persisted_request.url,
            scheme: execution.scheme,
            host: persisted_host,
            port: if secret_url { None } else { execution.port },
            path: persisted_path,
            query: if secret_url { None } else { execution.query },
            headers: persisted_request.headers,
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

fn create_saved_request_draft(
    state: &State<'_, AppState>,
    request_id: &str,
) -> Result<ReplayDraft, AppError> {
    let saved = load_saved_request(state, request_id)?;
    let body = saved.body.as_ref().map(|body| ReplayBodyDraft {
        text: body.text.clone(),
        base64: body.base64.clone(),
        is_binary: body.is_binary,
        content_type: body.content_type.clone(),
        use_original: true,
        source_truncated: false,
    });

    Ok(ReplayDraft {
        source_flow_id: format!("{SAVED_SOURCE_PREFIX}{}", saved.id),
        method: saved.method,
        url: saved.url,
        headers: build_header_drafts(&saved.headers),
        body,
    })
}

fn build_header_drafts(headers: &[HeaderValue]) -> Vec<ReplayHeaderDraft> {
    headers
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
        .collect()
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

fn load_saved_request(state: &State<'_, AppState>, request_id: &str) -> Result<SavedRequest, AppError> {
    state
        .database
        .list_saved_requests(None)
        .map_err(|error| AppError::storage(error.to_string()))?
        .into_iter()
        .find(|request| request.id == request_id)
        .ok_or_else(|| AppError::new("saved_request_missing", "Saved request was not found.", true))
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

fn resolve_flow_request(
    state: &State<'_, AppState>,
    draft: &ReplayDraft,
    source_request: &RequestDetail,
) -> Result<ReplayRequest, AppError> {
    let headers = resolve_headers(draft, &source_request.headers)?;
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
        Some(body) => body_from_draft(body)?,
    };

    build_replay_request(draft, headers, body, body_is_binary, body_content_type)
}

fn resolve_saved_request(
    _state: &State<'_, AppState>,
    draft: &ReplayDraft,
    source_request: &SavedRequest,
) -> Result<ReplayRequest, AppError> {
    let headers = resolve_headers(draft, &source_request.headers)?;
    let (body, body_is_binary, body_content_type) = match &draft.body {
        None => (None, false, None),
        Some(body) if body.use_original => source_request
            .body
            .as_ref()
            .map(saved_body_bytes)
            .transpose()?
            .unwrap_or((None, false, None)),
        Some(body) => body_from_draft(body)?,
    };

    build_replay_request(draft, headers, body, body_is_binary, body_content_type)
}

fn resolve_headers(draft: &ReplayDraft, source_headers: &[HeaderValue]) -> Result<Vec<HeaderValue>, AppError> {
    let mut headers = Vec::new();
    for header in draft.headers.iter().filter(|header| header.enabled) {
        let (value, sensitive) = if header.use_original {
            let index = header.source_index.ok_or_else(|| {
                AppError::new(
                    "replay_header_source_missing",
                    format!("Header '{}' has no stored source value.", header.name),
                    true,
                )
            })?;
            let original = source_headers.get(index).ok_or_else(|| {
                AppError::new(
                    "replay_header_source_missing",
                    format!("Stored source header '{}' is no longer available.", header.name),
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
    Ok(headers)
}

fn build_replay_request(
    draft: &ReplayDraft,
    headers: Vec<HeaderValue>,
    body: Option<Vec<u8>>,
    body_is_binary: bool,
    body_content_type: Option<String>,
) -> Result<ReplayRequest, AppError> {
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

fn body_from_draft(body: &ReplayBodyDraft) -> Result<(Option<Vec<u8>>, bool, Option<String>), AppError> {
    if body.is_binary {
        let encoded = body.base64.as_deref().unwrap_or_default();
        let bytes = BASE64.decode(encoded.as_bytes()).map_err(|error| {
            AppError::new(
                "replay_body_invalid_base64",
                format!("Binary replay body is not valid base64: {error}"),
                true,
            )
        })?;
        Ok((Some(bytes), true, body.content_type.clone()))
    } else {
        Ok((
            Some(body.text.clone().unwrap_or_default().into_bytes()),
            false,
            body.content_type.clone(),
        ))
    }
}

fn saved_body_bytes(body: &SavedRequestBody) -> Result<(Option<Vec<u8>>, bool, Option<String>), AppError> {
    if body.is_binary {
        let bytes = BASE64
            .decode(body.base64.as_deref().unwrap_or_default().as_bytes())
            .map_err(|error| {
                AppError::new(
                    "saved_request_body_invalid",
                    format!("Saved binary body is not valid base64: {error}"),
                    true,
                )
            })?;
        Ok((Some(bytes), true, body.content_type.clone()))
    } else {
        Ok((
            Some(body.text.clone().unwrap_or_default().into_bytes()),
            false,
            body.content_type.clone(),
        ))
    }
}

fn interpolate_replay_request(
    state: &State<'_, AppState>,
    template: ReplayRequest,
) -> Result<(ReplayRequest, ReplayRequest, bool), AppError> {
    let requested_keys = collect_template_keys(&template);
    if requested_keys.is_empty() {
        return Ok((template.clone(), template, false));
    }

    let values = active_environment_values(state, &requested_keys)?;
    let mut execution = template.clone();
    let mut persisted = template;

    let url_result = interpolate(&execution.url, &values);
    ensure_variables_resolved(&url_result.missing_variables)?;
    execution.url = url_result.value.clone();
    if !url_result.used_secret {
        persisted.url = url_result.value;
    }

    for (execution_header, persisted_header) in execution
        .headers
        .iter_mut()
        .zip(persisted.headers.iter_mut())
    {
        let result = interpolate(&execution_header.value, &values);
        ensure_variables_resolved(&result.missing_variables)?;
        execution_header.value = result.value.clone();
        if result.used_secret {
            persisted_header.sensitive = true;
        } else {
            persisted_header.value = result.value;
        }
    }

    if !execution.body_is_binary {
        if let Some(body) = execution.body.as_ref() {
            if let Ok(text) = String::from_utf8(body.clone()) {
                let result = interpolate(&text, &values);
                ensure_variables_resolved(&result.missing_variables)?;
                execution.body = Some(result.value.clone().into_bytes());
                if !result.used_secret {
                    persisted.body = Some(result.value.into_bytes());
                }
            }
        }
    }

    Ok((execution, persisted, url_result.used_secret))
}

fn collect_template_keys(request: &ReplayRequest) -> HashSet<String> {
    let mut keys = HashSet::new();
    collect_keys_from_text(&request.url, &mut keys);
    for header in &request.headers {
        collect_keys_from_text(&header.value, &mut keys);
    }
    if !request.body_is_binary {
        if let Some(body) = request.body.as_ref() {
            if let Ok(text) = String::from_utf8(body.clone()) {
                collect_keys_from_text(&text, &mut keys);
            }
        }
    }
    keys
}

fn collect_keys_from_text(text: &str, keys: &mut HashSet<String>) {
    let mut rest = text;
    while let Some(start) = rest.find("{{") {
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            break;
        };
        let key = after_start[..end].trim();
        if !key.is_empty() {
            keys.insert(key.to_string());
        }
        rest = &after_start[end + 2..];
    }
}

fn active_environment_values(
    state: &State<'_, AppState>,
    requested_keys: &HashSet<String>,
) -> Result<HashMap<String, (String, bool)>, AppError> {
    let Some(environment) = state
        .database
        .list_environments()
        .map_err(|error| AppError::storage(error.to_string()))?
        .into_iter()
        .find(|environment| environment.is_active)
    else {
        return Ok(HashMap::new());
    };

    let secret_store = SecretStore;
    let mut values = HashMap::new();
    for variable in state
        .database
        .list_environment_variables(&environment.id)
        .map_err(|error| AppError::storage(error.to_string()))?
        .into_iter()
        .filter(|variable| variable.enabled && requested_keys.contains(&variable.key))
    {
        if variable.is_secret {
            let Some(reference) = variable.secret_ref.as_deref() else {
                continue;
            };
            let secret = secret_store
                .get(reference)
                .map_err(|error| AppError::new(error.code, error.message, true))?;
            if let Some(secret) = secret {
                values.insert(variable.key, (secret, true));
            }
        } else {
            values.insert(variable.key, (variable.value.unwrap_or_default(), false));
        }
    }
    Ok(values)
}

fn ensure_variables_resolved(missing: &[String]) -> Result<(), AppError> {
    if missing.is_empty() {
        return Ok(());
    }
    Err(AppError::new(
        "environment_variables_missing",
        format!("Missing active-environment variables: {}", missing.join(", ")),
        true,
    ))
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

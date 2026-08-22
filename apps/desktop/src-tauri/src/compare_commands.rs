use super::AppState;
use compare_core::{
    compare_sessions as compare_snapshots, diagnose_session, AppContextEvidence, ComparableBody,
    ComparableFlow, SessionComparison, SessionDiagnostics, SessionSnapshot,
};
use core_model::{AppError, BodyRef, FlowDetail, TrafficSearchQuery};
use sdk_protocol::{SdkEvent, SdkPlatform, SDK_CORRELATION_HEADER};
use tauri::State;

#[tauri::command]
pub fn compare_sessions(
    baseline_session_id: String,
    candidate_session_id: String,
    state: State<'_, AppState>,
) -> Result<SessionComparison, AppError> {
    if baseline_session_id == candidate_session_id {
        return Err(AppError::new(
            "compare_same_session",
            "Choose two different capture sessions to compare.",
            true,
        ));
    }
    let baseline = build_snapshot(&baseline_session_id, &state)?;
    let candidate = build_snapshot(&candidate_session_id, &state)?;
    Ok(compare_snapshots(baseline, candidate))
}

#[tauri::command]
pub fn session_diagnostics(
    session_id: String,
    state: State<'_, AppState>,
) -> Result<SessionDiagnostics, AppError> {
    let snapshot = build_snapshot(&session_id, &state)?;
    Ok(diagnose_session(&snapshot))
}

fn build_snapshot(session_id: &str, state: &State<'_, AppState>) -> Result<SessionSnapshot, AppError> {
    let session = state
        .database
        .list_sessions(10_000)
        .map_err(storage_error)?
        .into_iter()
        .find(|session| session.id == session_id)
        .ok_or_else(|| AppError::new("compare_session_missing", "Capture session was not found.", true))?;

    let results = state
        .database
        .search_flows(&TrafficSearchQuery {
            session_id: Some(session_id.to_string()),
            limit: Some(5_000),
            ..TrafficSearchQuery::default()
        })
        .map_err(storage_error)?;

    let mut flows = Vec::with_capacity(results.len());
    for result in results {
        let Some(detail) = state.database.get_flow_detail(&result.flow.id).map_err(storage_error)? else {
            continue;
        };
        flows.push(ComparableFlow {
            request_body: body_for_compare(detail.request.as_ref().and_then(|request| request.body.as_ref()), state)?,
            response_body: body_for_compare(detail.response.as_ref().and_then(|response| response.body.as_ref()), state)?,
            app_context: sdk_context_for_detail(&detail, state)?,
            detail,
        });
    }

    flows.sort_by_key(|flow| flow.detail.summary.started_at.parse::<u128>().unwrap_or_default());
    Ok(SessionSnapshot { session, flows })
}

fn body_for_compare(
    reference: Option<&BodyRef>,
    state: &State<'_, AppState>,
) -> Result<Option<ComparableBody>, AppError> {
    let Some(reference) = reference else { return Ok(None) };
    let text = if reference.is_binary {
        None
    } else {
        let bytes = state.body_store.read(&reference.sha256).map_err(storage_error)?;
        String::from_utf8(bytes).ok()
    };
    Ok(Some(ComparableBody {
        content_type: reference.content_type.clone(),
        text,
        byte_size: reference.byte_size,
        truncated: reference.is_truncated,
    }))
}

fn sdk_context_for_detail(
    detail: &FlowDetail,
    state: &State<'_, AppState>,
) -> Result<Option<AppContextEvidence>, AppError> {
    let request_id = detail.request.as_ref().and_then(|request| {
        request
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(SDK_CORRELATION_HEADER))
            .map(|header| header.value.as_str())
    });
    let Some(request_id) = request_id else { return Ok(None) };
    let events = state
        .sdk_database
        .events_for_request(request_id)
        .map_err(sdk_storage_error)?;
    let network = events.iter().rev().find_map(|envelope| match &envelope.event {
        SdkEvent::Network(network) => Some(network),
        _ => None,
    });
    let Some(network) = network else { return Ok(None) };
    let client = state
        .sdk_database
        .get_client(&network.client_id)
        .map_err(sdk_storage_error)?;
    let source = network.context.source.as_ref();
    Ok(Some(AppContextEvidence {
        app_id: client.as_ref().map(|client| client.app_id.clone()),
        app_name: client.as_ref().map(|client| client.app_name.clone()),
        platform: client.as_ref().map(|client| platform_name(&client.platform).to_string()),
        screen: network.context.screen.clone(),
        feature: network.context.feature.clone(),
        source_file: source.and_then(|source| source.file.clone()),
        source_function: source.and_then(|source| source.function.clone()),
        source_line: source.and_then(|source| source.line),
    }))
}

fn platform_name(platform: &SdkPlatform) -> &'static str {
    match platform {
        SdkPlatform::Ios => "ios",
        SdkPlatform::Android => "android",
        SdkPlatform::Other => "other",
    }
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn sdk_storage_error(error: sdk_storage::SdkStorageError) -> AppError {
    AppError::storage(error.to_string())
}

use super::AppState;
use core_model::AppError;
use sdk_protocol::{SdkEnvelope, SDK_CORRELATION_HEADER, SDK_EVENT_PATH, SDK_HEALTH_PATH, SDK_INGESTION_PORT};
use sdk_storage::SdkClientRecord;
use serde::Serialize;
use tauri::State;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SdkSetupInfo {
    pub port: u16,
    pub ios_base_url: String,
    pub android_base_url: String,
    pub event_path: String,
    pub health_path: String,
    pub correlation_header: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FlowSdkEnrichment {
    pub request_id: Option<String>,
    pub client: Option<SdkClientRecord>,
    pub request_events: Vec<SdkEnvelope>,
    pub nearby_events: Vec<SdkEnvelope>,
}

#[tauri::command]
pub fn sdk_setup_info() -> SdkSetupInfo {
    SdkSetupInfo {
        port: SDK_INGESTION_PORT,
        ios_base_url: format!("http://127.0.0.1:{SDK_INGESTION_PORT}"),
        android_base_url: format!("http://10.0.2.2:{SDK_INGESTION_PORT}"),
        event_path: SDK_EVENT_PATH.into(),
        health_path: SDK_HEALTH_PATH.into(),
        correlation_header: SDK_CORRELATION_HEADER.into(),
    }
}

#[tauri::command]
pub fn list_sdk_clients(state: State<'_, AppState>) -> Result<Vec<SdkClientRecord>, AppError> {
    state.sdk_database.list_clients().map_err(sdk_storage_error)
}

#[tauri::command]
pub fn list_sdk_events(
    client_id: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SdkEnvelope>, AppError> {
    state
        .sdk_database
        .events_for_client(&client_id, limit.unwrap_or(250))
        .map_err(sdk_storage_error)
}

#[tauri::command]
pub fn search_sdk_events(
    text: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<SdkEnvelope>, AppError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }
    state
        .sdk_database
        .search_events(&text, limit.unwrap_or(500))
        .map_err(sdk_storage_error)
}

#[tauri::command]
pub fn sdk_enrichment_for_flow(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<FlowSdkEnrichment, AppError> {
    let detail = state
        .database
        .get_flow_detail(&flow_id)
        .map_err(storage_error)?
        .ok_or_else(|| AppError::new("sdk_flow_missing", "Flow details were not found.", true))?;

    let request_id = detail.request.as_ref().and_then(|request| {
        request
            .headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(SDK_CORRELATION_HEADER))
            .map(|header| header.value.clone())
    });

    let Some(request_id_value) = request_id.as_deref() else {
        return Ok(FlowSdkEnrichment {
            request_id: None,
            client: None,
            request_events: Vec::new(),
            nearby_events: Vec::new(),
        });
    };

    let request_events = state
        .sdk_database
        .events_for_request(request_id_value)
        .map_err(sdk_storage_error)?;
    let client_id = request_events
        .first()
        .map(|envelope| envelope.event.client_id().to_string());
    let client = client_id
        .as_deref()
        .map(|id| state.sdk_database.get_client(id))
        .transpose()
        .map_err(sdk_storage_error)?
        .flatten();

    let around_ms = detail.summary.started_at.parse::<u128>().unwrap_or_default();
    let nearby_events = match client_id.as_deref() {
        Some(id) if around_ms > 0 => state
            .sdk_database
            .recent_context_events(id, around_ms, 5_000, 200)
            .map_err(sdk_storage_error)?,
        _ => Vec::new(),
    };

    Ok(FlowSdkEnrichment {
        request_id,
        client,
        request_events,
        nearby_events,
    })
}

pub(super) fn spawn_sdk_ingestion(
    database: sdk_storage::SdkDatabase,
    server: std::sync::Arc<sdk_transport::SdkIngestionServer>,
) {
    let mut receiver = server.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(envelope) => {
                    let _ = database.record(&envelope);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });

    tauri::async_runtime::spawn(async move {
        let _ = server.run().await;
    });
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn sdk_storage_error(error: sdk_storage::SdkStorageError) -> AppError {
    AppError::storage(error.to_string())
}

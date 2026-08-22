use super::{now_epoch_millis, AppState};
use core_model::AppError;
use sdk_protocol::{SdkEnvelope, SDK_CORRELATION_HEADER, SDK_EVENT_PATH, SDK_HEALTH_PATH, SDK_INGESTION_PORT};
use sdk_storage::SdkClientRecord;
use serde::Serialize;
use std::{collections::HashSet, net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream}, time::Duration};
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
    pub ingestion_reachable: bool,
    pub active_client_count: usize,
    pub known_client_count: usize,
    pub latest_seen_at: Option<String>,
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
pub fn sdk_setup_info(state: State<'_, AppState>) -> Result<SdkSetupInfo, AppError> {
    let clients = state.sdk_database.list_clients().map_err(sdk_storage_error)?;
    let now = now_epoch_millis()?.parse::<u128>().unwrap_or_default();
    let active_client_count = clients
        .iter()
        .filter(|client| {
            client
                .last_seen_at
                .parse::<u128>()
                .map(|seen| now.saturating_sub(seen) <= 15_000)
                .unwrap_or(false)
        })
        .count();
    let latest_seen_at = clients
        .iter()
        .max_by_key(|client| client.last_seen_at.parse::<u128>().unwrap_or_default())
        .map(|client| client.last_seen_at.clone());
    let address = SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), SDK_INGESTION_PORT);
    let ingestion_reachable = TcpStream::connect_timeout(&address, Duration::from_millis(120)).is_ok();

    Ok(SdkSetupInfo {
        port: SDK_INGESTION_PORT,
        ios_base_url: format!("http://127.0.0.1:{SDK_INGESTION_PORT}"),
        android_base_url: format!("http://10.0.2.2:{SDK_INGESTION_PORT}"),
        event_path: SDK_EVENT_PATH.into(),
        health_path: SDK_HEALTH_PATH.into(),
        correlation_header: SDK_CORRELATION_HEADER.into(),
        ingestion_reachable,
        active_client_count,
        known_client_count: clients.len(),
        latest_seen_at,
    })
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
pub fn sdk_flow_ids_matching(
    text: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<String>, AppError> {
    if text.trim().is_empty() {
        return Ok(Vec::new());
    }

    let events = state
        .sdk_database
        .search_events(&text, limit.unwrap_or(1_000))
        .map_err(sdk_storage_error)?;
    let request_ids: HashSet<String> = events
        .iter()
        .filter_map(|event| event.event.request_id().map(str::to_string))
        .collect();
    if request_ids.is_empty() {
        return Ok(Vec::new());
    }

    let flows = state
        .database
        .list_flows(10_000)
        .map_err(storage_error)?;
    let mut matches = Vec::new();
    for flow in flows {
        let detail = state
            .database
            .get_flow_detail(&flow.id)
            .map_err(storage_error)?;
        let request_id = detail
            .as_ref()
            .and_then(|detail| detail.request.as_ref())
            .and_then(|request| {
                request
                    .headers
                    .iter()
                    .find(|header| header.name.eq_ignore_ascii_case(SDK_CORRELATION_HEADER))
                    .map(|header| header.value.as_str())
            });
        if request_id.is_some_and(|value| request_ids.contains(value)) {
            matches.push(flow.id);
        }
        if matches.len() >= limit.unwrap_or(1_000).min(10_000) {
            break;
        }
    }
    Ok(matches)
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

    if detail.summary.session_id.is_some() {
        if let Some(client) = client.as_ref() {
            let _ = state.database.attribute_session_from_request_header(
                SDK_CORRELATION_HEADER,
                request_id_value,
                &client.app_id,
            );
        }
    }

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
    let session_database = storage::Database::open(database.path()).ok();
    let mut receiver = server.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(envelope) => {
                    if database.record(&envelope).is_ok() {
                        if let (Some(request_id), Some(session_database)) = (
                            envelope.event.request_id(),
                            session_database.as_ref(),
                        ) {
                            if let Ok(Some(client)) = database.get_client(envelope.event.client_id()) {
                                let _ = session_database.attribute_session_from_request_header(
                                    SDK_CORRELATION_HEADER,
                                    request_id,
                                    &client.app_id,
                                );
                            }
                        }
                    }
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

use super::{now_epoch_millis, AppState};
use crate::State;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::AppError;
use mock_core::{
    normalized_path_for_flow, MockBodyEncoding, MockBodyOverride, MockFailureMode, MockPathMatch,
    MockRule, MockRulesDocument, MOCK_SCHEMA_VERSION,
};
use mock_storage::MockDatabase;
use std::{fs, path::Path};

pub fn list_mock_rules(state: State<'_, AppState>) -> Result<Vec<MockRule>, AppError> {
    let database = mock_database(&state)?;
    let rules = database.list_rules().map_err(mock_storage_error)?;
    publish_rules(&state, rules.clone())?;
    Ok(rules)
}

pub fn upsert_mock_rule(rule: MockRule, state: State<'_, AppState>) -> Result<MockRule, AppError> {
    validate_rule(&rule)?;
    let database = mock_database(&state)?;
    database.upsert_rule(&rule).map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)?;
    Ok(rule)
}

pub fn create_mock_from_flow(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<MockRule, AppError> {
    let detail = state
        .database
        .get_flow_detail(&flow_id)
        .map_err(storage_error)?
        .ok_or_else(|| {
            AppError::new(
                "mock_flow_detail_missing",
                "Full flow details are required to create a mock.",
                true,
            )
        })?;
    let response = detail.response.as_ref().ok_or_else(|| {
        AppError::new(
            "mock_response_missing",
            "The selected flow does not contain a response to use as a mock.",
            true,
        )
    })?;

    let database = mock_database(&state)?;
    let priority = database.list_rules().map_err(mock_storage_error)?.len() as i64;
    let timestamp = now_epoch_millis()?;
    let response_body = response
        .body
        .as_ref()
        .map(|reference| {
            if reference.is_truncated {
                return Err(AppError::new(
                    "mock_body_truncated",
                    "The captured response body is truncated. Create a manual mock body or recapture with a larger body limit before creating the mock.",
                    true,
                ));
            }
            let bytes = state.body_store.read(&reference.sha256).map_err(storage_error)?;
            let (encoding, data) = if reference.is_binary {
                (MockBodyEncoding::Base64, BASE64.encode(bytes))
            } else {
                match String::from_utf8(bytes.clone()) {
                    Ok(text) => (MockBodyEncoding::Text, text),
                    Err(_) => (MockBodyEncoding::Base64, BASE64.encode(bytes)),
                }
            };
            Ok::<_, AppError>(MockBodyOverride {
                content_type: reference.content_type.clone(),
                encoding,
                data,
            })
        })
        .transpose()?;

    let rule = MockRule {
        schema_version: MOCK_SCHEMA_VERSION,
        id: format!("mock-{}", epoch_nanos()),
        name: format!(
            "{} {}{}",
            detail.summary.method,
            detail.summary.host,
            normalized_path_for_flow(&detail.summary)
        ),
        enabled: true,
        priority,
        method: Some(detail.summary.method.clone()),
        host: Some(detail.summary.host.clone()),
        path_pattern: normalized_path_for_flow(&detail.summary),
        path_match: MockPathMatch::Normalized,
        status_code: Some(response.status_code),
        response_headers: Vec::new(),
        response_body,
        json_mutations: Vec::new(),
        latency_ms: None,
        failure_mode: MockFailureMode::None,
        request_breakpoint: false,
        response_breakpoint: false,
        source_flow_id: Some(flow_id),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    database.upsert_rule(&rule).map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)?;
    Ok(rule)
}

pub fn set_mock_rule_enabled(
    id: String,
    enabled: bool,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let database = mock_database(&state)?;
    database
        .set_enabled(&id, enabled)
        .map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)
}

pub fn set_mock_rule_priority(
    id: String,
    priority: i64,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let database = mock_database(&state)?;
    database
        .set_priority(&id, priority)
        .map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)
}

pub fn delete_mock_rule(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let database = mock_database(&state)?;
    database.delete_rule(&id).map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)
}

pub fn disable_all_mocks(state: State<'_, AppState>) -> Result<usize, AppError> {
    let database = mock_database(&state)?;
    let changed = database.disable_all().map_err(mock_storage_error)?;
    publish_current_rules(&state, &database)?;
    Ok(changed)
}

fn mock_database(state: &State<'_, AppState>) -> Result<MockDatabase, AppError> {
    MockDatabase::open(state.database.path()).map_err(mock_storage_error)
}

fn publish_current_rules(
    state: &State<'_, AppState>,
    database: &MockDatabase,
) -> Result<(), AppError> {
    let rules = database.list_rules().map_err(mock_storage_error)?;
    publish_rules(state, rules)
}

fn publish_rules(state: &State<'_, AppState>, rules: Vec<MockRule>) -> Result<(), AppError> {
    let document = MockRulesDocument::active(rules, true);
    let bytes = serde_json::to_vec_pretty(&document)
        .map_err(|error| AppError::new("mock_rules_serialize_failed", error.to_string(), true))?;
    let directory = state.capture_engine.conf_dir();
    fs::create_dir_all(directory)
        .map_err(|error| AppError::new("mock_rules_directory_failed", error.to_string(), true))?;
    atomic_write(&directory.join("mock-rules.json"), &bytes)
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<(), AppError> {
    let temporary = path.with_extension("json.tmp");
    fs::write(&temporary, bytes)
        .map_err(|error| AppError::new("mock_rules_write_failed", error.to_string(), true))?;
    fs::rename(&temporary, path)
        .map_err(|error| AppError::new("mock_rules_publish_failed", error.to_string(), true))
}

fn validate_rule(rule: &MockRule) -> Result<(), AppError> {
    if rule.name.trim().is_empty() {
        return Err(AppError::new(
            "mock_rule_name_required",
            "Mock rule name is required.",
            true,
        ));
    }
    if rule.path_pattern.trim().is_empty() {
        return Err(AppError::new(
            "mock_rule_path_required",
            "Mock path pattern is required.",
            true,
        ));
    }
    if let Some(status) = rule.status_code {
        if !(100..=599).contains(&status) {
            return Err(AppError::new(
                "mock_rule_status_invalid",
                "Mock status code must be between 100 and 599.",
                true,
            ));
        }
    }
    if let Some(body) = rule.response_body.as_ref() {
        if matches!(body.encoding, MockBodyEncoding::Base64) {
            BASE64.decode(body.data.as_bytes()).map_err(|error| {
                AppError::new(
                    "mock_rule_body_invalid_base64",
                    format!("Mock response body is not valid base64: {error}"),
                    true,
                )
            })?;
        }
    }
    Ok(())
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn mock_storage_error(error: mock_storage::MockStorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn epoch_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

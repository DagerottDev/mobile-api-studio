use super::{now_epoch_millis, AppState};
use crate::State;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::AppError;
use mock_core::{
    MockBodyEncoding, MockBodyOverride, MockHeaderMutation, MockRule, MockRulesDocument,
};
use mock_fixtures::{
    MockFixture, MockFixtureDatabase, MockFixtureStorageError, MOCK_FIXTURE_SCHEMA_VERSION,
};
use mock_storage::{MockDatabase, MockStorageError};
use std::{fs, path::Path};

pub fn list_mock_fixtures(state: State<'_, AppState>) -> Result<Vec<MockFixture>, AppError> {
    fixture_database(&state)?.list().map_err(fixture_error)
}

pub fn upsert_mock_fixture(
    fixture: MockFixture,
    state: State<'_, AppState>,
) -> Result<MockFixture, AppError> {
    validate_fixture(&fixture)?;
    fixture_database(&state)?
        .upsert(&fixture)
        .map_err(fixture_error)?;
    Ok(fixture)
}

pub fn delete_mock_fixture(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    fixture_database(&state)?.delete(&id).map_err(fixture_error)
}

pub fn create_fixture_from_flow(
    flow_id: String,
    name: Option<String>,
    state: State<'_, AppState>,
) -> Result<MockFixture, AppError> {
    let detail = state
        .database
        .get_flow_detail(&flow_id)
        .map_err(storage_error)?
        .ok_or_else(|| {
            AppError::new(
                "fixture_flow_detail_missing",
                "Full flow details are required to create a response fixture.",
                true,
            )
        })?;
    let response = detail.response.as_ref().ok_or_else(|| {
        AppError::new(
            "fixture_response_missing",
            "The selected flow does not contain a response to save as a fixture.",
            true,
        )
    })?;

    let response_body = response
        .body
        .as_ref()
        .map(|reference| {
            if reference.is_truncated {
                return Err(AppError::new(
                    "fixture_body_truncated",
                    "The captured response body is truncated. Create a manual fixture or recapture with a larger body limit before saving it.",
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

    let response_headers = response
        .headers
        .iter()
        .filter(|header| is_fixture_header_safe(header))
        .map(|header| MockHeaderMutation {
            name: header.name.clone(),
            value: Some(header.value.clone()),
            remove: false,
        })
        .collect();

    let timestamp = now_epoch_millis()?;
    let fixture = MockFixture {
        schema_version: MOCK_FIXTURE_SCHEMA_VERSION,
        id: format!("fixture-{}", epoch_nanos()),
        name: sanitize_name(
            name.as_deref(),
            &format!(
                "{} {}{}",
                detail.summary.method, detail.summary.host, detail.summary.path
            ),
        ),
        status_code: response.status_code,
        response_headers,
        response_body,
        source_flow_id: Some(flow_id),
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    fixture_database(&state)?
        .upsert(&fixture)
        .map_err(fixture_error)?;
    Ok(fixture)
}

pub fn apply_fixture_to_mock(
    fixture_id: String,
    rule_id: String,
    state: State<'_, AppState>,
) -> Result<MockRule, AppError> {
    let fixture = fixture_database(&state)?
        .get(&fixture_id)
        .map_err(fixture_error)?
        .ok_or_else(|| {
            AppError::new("mock_fixture_missing", "Response fixture not found.", true)
        })?;
    let mock_database = MockDatabase::open(state.database.path()).map_err(mock_error)?;
    let mut rule = mock_database
        .get_rule(&rule_id)
        .map_err(mock_error)?
        .ok_or_else(|| AppError::new("mock_rule_missing", "Mock rule not found.", true))?;

    rule.status_code = Some(fixture.status_code);
    rule.response_headers = fixture.response_headers.clone();
    rule.response_body = fixture.response_body.clone();
    rule.updated_at = now_epoch_millis()?;
    mock_database.upsert_rule(&rule).map_err(mock_error)?;
    publish_rules(&state, mock_database.list_rules().map_err(mock_error)?)?;
    Ok(rule)
}

fn fixture_database(state: &State<'_, AppState>) -> Result<MockFixtureDatabase, AppError> {
    MockFixtureDatabase::open(state.database.path()).map_err(fixture_error)
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

fn validate_fixture(fixture: &MockFixture) -> Result<(), AppError> {
    if fixture.name.trim().is_empty() {
        return Err(AppError::new(
            "mock_fixture_name_required",
            "Fixture name is required.",
            true,
        ));
    }
    if !(100..=599).contains(&fixture.status_code) {
        return Err(AppError::new(
            "mock_fixture_status_invalid",
            "Fixture status code must be between 100 and 599.",
            true,
        ));
    }
    if let Some(body) = fixture.response_body.as_ref() {
        if matches!(body.encoding, MockBodyEncoding::Base64) {
            BASE64.decode(body.data.as_bytes()).map_err(|error| {
                AppError::new(
                    "mock_fixture_body_invalid_base64",
                    format!("Fixture body is not valid base64: {error}"),
                    true,
                )
            })?;
        }
    }
    Ok(())
}

fn is_fixture_header_safe(header: &&core_model::HeaderValue) -> bool {
    if header.sensitive {
        return false;
    }
    !matches!(
        header.name.to_ascii_lowercase().as_str(),
        "content-length" | "content-encoding" | "transfer-encoding" | "connection"
    )
}

fn sanitize_name(name: Option<&str>, fallback: &str) -> String {
    let trimmed = name.unwrap_or_default().trim();
    if trimmed.is_empty() {
        fallback.chars().take(120).collect()
    } else {
        trimmed.chars().take(120).collect()
    }
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn fixture_error(error: MockFixtureStorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn mock_error(error: MockStorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn epoch_nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default()
}

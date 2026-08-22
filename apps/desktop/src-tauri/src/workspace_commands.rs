use super::{now_epoch_millis, AppState};
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use core_model::{
    AppError, Environment, EnvironmentVariable, SavedCollection, SavedRequest, SavedRequestBody,
    TrafficSearchQuery, TrafficSearchResult, SCHEMA_VERSION,
};
use secret_store::SecretStore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::State;
use workspace_core::{interpolate, InterpolationResult};

const SECRET_PREFIX: &str = "env";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionMetadataInput {
    pub id: String,
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionInput {
    pub id: Option<String>,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SaveFlowInput {
    pub collection_id: String,
    pub flow_id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentInput {
    pub id: Option<String>,
    pub name: String,
    pub is_active: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableInput {
    pub environment_id: String,
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub enabled: bool,
    pub sort_order: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentSnapshot {
    pub environment: Environment,
    pub variables: Vec<EnvironmentVariable>,
}

#[tauri::command]
pub fn update_session_metadata(
    input: SessionMetadataInput,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let name = sanitize_name(&input.name, "Untitled Session", 120);
    state
        .database
        .update_session_metadata(&input.id, &name, input.notes.as_deref())
        .map_err(storage_error)
}

#[tauri::command]
pub fn archive_session(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.database.archive_session(&id).map_err(storage_error)
}

#[tauri::command]
pub fn delete_session(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.database.delete_session(&id).map_err(storage_error)
}

#[tauri::command]
pub fn search_traffic(
    query: TrafficSearchQuery,
    state: State<'_, AppState>,
) -> Result<Vec<TrafficSearchResult>, AppError> {
    state.database.search_flows(&query).map_err(storage_error)
}

#[tauri::command]
pub fn list_collections(state: State<'_, AppState>) -> Result<Vec<SavedCollection>, AppError> {
    state.database.list_collections().map_err(storage_error)
}

#[tauri::command]
pub fn upsert_collection(
    input: CollectionInput,
    state: State<'_, AppState>,
) -> Result<SavedCollection, AppError> {
    let timestamp = now_epoch_millis()?;
    let id = input
        .id
        .unwrap_or_else(|| format!("collection-{timestamp}"));
    let existing = state
        .database
        .list_collections()
        .map_err(storage_error)?
        .into_iter()
        .find(|collection| collection.id == id);

    let collection = SavedCollection {
        schema_version: SCHEMA_VERSION,
        id,
        name: sanitize_name(&input.name, "Untitled Collection", 120),
        description: input.description.and_then(trim_optional),
        sort_order: input
            .sort_order
            .or_else(|| existing.as_ref().map(|value| value.sort_order))
            .unwrap_or(0),
        created_at: existing
            .as_ref()
            .map(|value| value.created_at.clone())
            .unwrap_or_else(|| timestamp.clone()),
        updated_at: timestamp,
    };

    state
        .database
        .upsert_collection(&collection)
        .map_err(storage_error)?;
    Ok(collection)
}

#[tauri::command]
pub fn delete_collection(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state.database.delete_collection(&id).map_err(storage_error)
}

#[tauri::command]
pub fn list_saved_requests(
    collection_id: Option<String>,
    state: State<'_, AppState>,
) -> Result<Vec<SavedRequest>, AppError> {
    state
        .database
        .list_saved_requests(collection_id.as_deref())
        .map_err(storage_error)
        .map(redact_saved_requests)
}

#[tauri::command]
pub fn save_flow_to_collection(
    input: SaveFlowInput,
    state: State<'_, AppState>,
) -> Result<SavedRequest, AppError> {
    let detail = state
        .database
        .get_flow_detail(&input.flow_id)
        .map_err(storage_error)?
        .ok_or_else(|| {
            AppError::new(
                "flow_detail_missing",
                "Full request details are required before a request can be saved.",
                true,
            )
        })?;
    let request = detail.request.ok_or_else(|| {
        AppError::new(
            "request_detail_missing",
            "The selected flow does not contain request details.",
            true,
        )
    })?;

    let body = request
        .body
        .as_ref()
        .map(|reference| saved_body(&state, reference))
        .transpose()?;
    let timestamp = now_epoch_millis()?;
    let saved = SavedRequest {
        schema_version: SCHEMA_VERSION,
        id: format!("request-{timestamp}"),
        collection_id: input.collection_id,
        name: input
            .name
            .map(|value| sanitize_name(&value, "Saved Request", 120))
            .unwrap_or_else(|| format!("{} {}{}", request.method, request.host, request.path)),
        method: request.method,
        url: request.url,
        headers: request.headers,
        body,
        source_flow_id: Some(input.flow_id),
        sort_order: 0,
        created_at: timestamp.clone(),
        updated_at: timestamp,
    };

    state
        .database
        .upsert_saved_request(&saved)
        .map_err(storage_error)?;
    Ok(redact_saved_request(saved))
}

#[tauri::command]
pub fn delete_saved_request(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    state
        .database
        .delete_saved_request(&id)
        .map_err(storage_error)
}

#[tauri::command]
pub fn list_environments(state: State<'_, AppState>) -> Result<Vec<Environment>, AppError> {
    state.database.list_environments().map_err(storage_error)
}

#[tauri::command]
pub fn upsert_environment(
    input: EnvironmentInput,
    state: State<'_, AppState>,
) -> Result<Environment, AppError> {
    let timestamp = now_epoch_millis()?;
    let id = input.id.unwrap_or_else(|| format!("environment-{timestamp}"));
    let existing = state
        .database
        .list_environments()
        .map_err(storage_error)?
        .into_iter()
        .find(|environment| environment.id == id);

    let environment = Environment {
        schema_version: SCHEMA_VERSION,
        id,
        name: sanitize_name(&input.name, "Environment", 120),
        is_active: input
            .is_active
            .or_else(|| existing.as_ref().map(|value| value.is_active))
            .unwrap_or(false),
        created_at: existing
            .as_ref()
            .map(|value| value.created_at.clone())
            .unwrap_or_else(|| timestamp.clone()),
        updated_at: timestamp,
    };

    state
        .database
        .upsert_environment(&environment)
        .map_err(storage_error)?;
    Ok(environment)
}

#[tauri::command]
pub fn set_active_environment(
    id: Option<String>,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    state
        .database
        .set_active_environment(id.as_deref())
        .map_err(storage_error)
}

#[tauri::command]
pub fn delete_environment(id: String, state: State<'_, AppState>) -> Result<(), AppError> {
    let secret_store = SecretStore;
    for variable in state
        .database
        .list_environment_variables(&id)
        .map_err(storage_error)?
    {
        if let Some(reference) = variable.secret_ref.as_deref() {
            let _ = secret_store.delete(reference);
        }
    }
    state.database.delete_environment(&id).map_err(storage_error)
}

#[tauri::command]
pub fn environment_snapshot(
    environment_id: String,
    state: State<'_, AppState>,
) -> Result<EnvironmentSnapshot, AppError> {
    let environment = state
        .database
        .list_environments()
        .map_err(storage_error)?
        .into_iter()
        .find(|environment| environment.id == environment_id)
        .ok_or_else(|| AppError::new("environment_missing", "Environment not found.", true))?;
    let variables = state
        .database
        .list_environment_variables(&environment.id)
        .map_err(storage_error)?;
    Ok(EnvironmentSnapshot {
        environment,
        variables,
    })
}

#[tauri::command]
pub fn upsert_environment_variable(
    input: EnvironmentVariableInput,
    state: State<'_, AppState>,
) -> Result<EnvironmentVariable, AppError> {
    let key = input.key.trim();
    if key.is_empty() {
        return Err(AppError::new(
            "environment_key_empty",
            "Environment variable key cannot be empty.",
            true,
        ));
    }

    let id = environment_variable_id(&input.environment_id, key);
    let secret_ref = input
        .is_secret
        .then(|| secret_reference(&input.environment_id, key));
    let secret_store = SecretStore;

    if input.is_secret {
        let value = input.value.as_deref().unwrap_or_default();
        if !value.is_empty() {
            if !secret_store.is_available() {
                return Err(AppError::new(
                    "secure_store_unavailable",
                    "Secure secret storage is not available in this desktop build.",
                    true,
                ));
            }
            secret_store
                .set(secret_ref.as_deref().expect("secret ref exists"), value)
                .map_err(secret_error)?;
        }
    } else if let Some(previous) = state
        .database
        .list_environment_variables(&input.environment_id)
        .map_err(storage_error)?
        .into_iter()
        .find(|variable| variable.id == id)
    {
        if let Some(reference) = previous.secret_ref.as_deref() {
            let _ = secret_store.delete(reference);
        }
    }

    let variable = EnvironmentVariable {
        schema_version: SCHEMA_VERSION,
        id,
        environment_id: input.environment_id,
        key: key.to_string(),
        value: if input.is_secret { None } else { input.value },
        is_secret: input.is_secret,
        secret_ref,
        enabled: input.enabled,
        sort_order: input.sort_order.unwrap_or(0),
    };
    state
        .database
        .upsert_environment_variable(&variable)
        .map_err(storage_error)?;
    Ok(variable)
}

#[tauri::command]
pub fn delete_environment_variable(
    id: String,
    environment_id: String,
    state: State<'_, AppState>,
) -> Result<(), AppError> {
    let variables = state
        .database
        .list_environment_variables(&environment_id)
        .map_err(storage_error)?;
    if let Some(variable) = variables.iter().find(|variable| variable.id == id) {
        if let Some(reference) = variable.secret_ref.as_deref() {
            let _ = SecretStore.delete(reference);
        }
    }
    state
        .database
        .delete_environment_variable(&id)
        .map_err(storage_error)
}

#[tauri::command]
pub fn interpolate_with_active_environment(
    template: String,
    state: State<'_, AppState>,
) -> Result<InterpolationResult, AppError> {
    let Some(environment) = state
        .database
        .list_environments()
        .map_err(storage_error)?
        .into_iter()
        .find(|environment| environment.is_active)
    else {
        return Ok(InterpolationResult {
            value: template,
            used_secret: false,
            missing_variables: Vec::new(),
        });
    };

    let secret_store = SecretStore;
    let mut values = HashMap::new();
    for variable in state
        .database
        .list_environment_variables(&environment.id)
        .map_err(storage_error)?
        .into_iter()
        .filter(|variable| variable.enabled)
    {
        let value = if variable.is_secret {
            let reference = variable.secret_ref.as_deref().ok_or_else(|| {
                AppError::new(
                    "secret_reference_missing",
                    format!("Secret variable '{}' is missing its secure-store reference.", variable.key),
                    true,
                )
            })?;
            secret_store
                .get(reference)
                .map_err(secret_error)?
                .ok_or_else(|| {
                    AppError::new(
                        "secret_value_missing",
                        format!("Secret variable '{}' has no value in secure storage.", variable.key),
                        true,
                    )
                })?
        } else {
            variable.value.clone().unwrap_or_default()
        };
        values.insert(variable.key, (value, variable.is_secret));
    }

    Ok(interpolate(&template, &values))
}

fn saved_body(
    state: &State<'_, AppState>,
    reference: &core_model::BodyRef,
) -> Result<SavedRequestBody, AppError> {
    let bytes = state
        .body_store
        .read(&reference.sha256)
        .map_err(storage_error)?;
    if reference.is_binary {
        Ok(SavedRequestBody {
            text: None,
            base64: Some(BASE64.encode(bytes)),
            content_type: reference.content_type.clone(),
            is_binary: true,
        })
    } else {
        match String::from_utf8(bytes.clone()) {
            Ok(text) => Ok(SavedRequestBody {
                text: Some(text),
                base64: None,
                content_type: reference.content_type.clone(),
                is_binary: false,
            }),
            Err(_) => Ok(SavedRequestBody {
                text: None,
                base64: Some(BASE64.encode(bytes)),
                content_type: reference.content_type.clone(),
                is_binary: true,
            }),
        }
    }
}

fn redact_saved_requests(requests: Vec<SavedRequest>) -> Vec<SavedRequest> {
    requests.into_iter().map(redact_saved_request).collect()
}

fn redact_saved_request(mut request: SavedRequest) -> SavedRequest {
    for header in &mut request.headers {
        if header.sensitive {
            header.value = "<redacted>".into();
        }
    }
    request
}

fn environment_variable_id(environment_id: &str, key: &str) -> String {
    format!("envvar:{environment_id}:{}", key.to_lowercase())
}

fn secret_reference(environment_id: &str, key: &str) -> String {
    format!("{SECRET_PREFIX}:{environment_id}:{}", key.to_lowercase())
}

fn sanitize_name(value: &str, fallback: &str, limit: usize) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.chars().take(limit).collect()
    }
}

fn trim_optional(value: String) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

fn secret_error(error: secret_store::SecretStoreError) -> AppError {
    AppError::new(error.code, error.message, true)
}

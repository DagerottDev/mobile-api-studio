use super::{compare_commands, now_epoch_millis, AppState};
use crate::State;
use ai_core::{
    build_context_preview, default_secret_json_keys, AiContextPolicy, AiContextPreview, AiProvider,
    AiProviderRequest, AiTaskKind, OpenAiProvider, DEFAULT_OPENAI_MODEL, OPENAI_PROVIDER_ID,
};
use ai_storage::AiResultRecord;
use core_model::{AppError, AppPreference};
use secret_store::SecretStore;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

const PREF_AI_PROVIDER: &str = "ai.provider";
const PREF_AI_MODEL: &str = "ai.model";
const PREF_AI_SECRET_KEYS: &str = "ai.secret_json_keys";
const OPENAI_KEY_REF: &str = "ai:openai:api-key";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsSnapshot {
    pub provider: String,
    pub model: String,
    pub api_key_configured: bool,
    pub secure_store_available: bool,
    pub secret_json_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiSettingsInput {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub secret_json_keys: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerateInput {
    pub expected_context_fingerprint: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AiGenerationResult {
    pub record: AiResultRecord,
    pub context: AiContextPreview,
}

pub fn ai_settings(state: State<'_, AppState>) -> Result<AiSettingsSnapshot, AppError> {
    load_settings(&state)
}

pub fn set_ai_settings(
    input: AiSettingsInput,
    state: State<'_, AppState>,
) -> Result<AiSettingsSnapshot, AppError> {
    if input.provider.trim() != OPENAI_PROVIDER_ID {
        return Err(AppError::new(
            "ai_provider_unsupported",
            "Only the OpenAI provider is implemented in this release.",
            true,
        ));
    }
    let model = input.model.trim();
    if model.is_empty() {
        return Err(AppError::new(
            "ai_model_required",
            "Choose an OpenAI model.",
            true,
        ));
    }
    let secret_keys = normalize_secret_keys(input.secret_json_keys);
    state
        .database
        .set_preference(&AppPreference {
            key: PREF_AI_PROVIDER.into(),
            value: OPENAI_PROVIDER_ID.into(),
        })
        .map_err(storage_error)?;
    state
        .database
        .set_preference(&AppPreference {
            key: PREF_AI_MODEL.into(),
            value: model.into(),
        })
        .map_err(storage_error)?;
    state
        .database
        .set_preference(&AppPreference {
            key: PREF_AI_SECRET_KEYS.into(),
            value: serde_json::to_string(&secret_keys).map_err(json_error)?,
        })
        .map_err(storage_error)?;

    if let Some(api_key) = input.api_key {
        let trimmed = api_key.trim();
        if !trimmed.is_empty() {
            SecretStore::default()
                .set(OPENAI_KEY_REF, trimmed)
                .map_err(secret_error)?;
        }
    }
    load_settings(&state)
}

pub fn clear_ai_api_key(state: State<'_, AppState>) -> Result<AiSettingsSnapshot, AppError> {
    let store = SecretStore::default();
    if store.is_available() {
        store.delete(OPENAI_KEY_REF).map_err(secret_error)?;
    }
    load_settings(&state)
}

pub fn preview_session_ai_context(
    baseline_session_id: String,
    candidate_session_id: String,
    state: State<'_, AppState>,
) -> Result<AiContextPreview, AppError> {
    let value = session_context_value(&baseline_session_id, &candidate_session_id, &state)?;
    preview_value(&value, &state)
}

pub async fn explain_session_comparison(
    baseline_session_id: String,
    candidate_session_id: String,
    input: AiGenerateInput,
    state: State<'_, AppState>,
) -> Result<AiGenerationResult, AppError> {
    let value = session_context_value(&baseline_session_id, &candidate_session_id, &state)?;
    let preview = preview_value(&value, &state)?;
    verify_preview(&preview, &input)?;
    run_ai(
        AiTaskKind::SessionDiff,
        format!("session-diff:{baseline_session_id}:{candidate_session_id}"),
        preview,
        &state,
    )
    .await
}

pub fn preview_flow_ai_context(
    flow_id: String,
    state: State<'_, AppState>,
) -> Result<AiContextPreview, AppError> {
    let value = flow_context_value(&flow_id, &state)?;
    preview_value(&value, &state)
}

pub async fn diagnose_flow_with_ai(
    flow_id: String,
    input: AiGenerateInput,
    state: State<'_, AppState>,
) -> Result<AiGenerationResult, AppError> {
    let value = flow_context_value(&flow_id, &state)?;
    let preview = preview_value(&value, &state)?;
    verify_preview(&preview, &input)?;
    run_ai(
        AiTaskKind::FlowDiagnosis,
        format!("flow:{flow_id}"),
        preview,
        &state,
    )
    .await
}

pub fn list_ai_results(
    source_ref: String,
    limit: Option<usize>,
    state: State<'_, AppState>,
) -> Result<Vec<AiResultRecord>, AppError> {
    state
        .ai_database
        .list_for_source(&source_ref, limit.unwrap_or(20))
        .map_err(ai_storage_error)
}

fn load_settings(state: &State<'_, AppState>) -> Result<AiSettingsSnapshot, AppError> {
    let provider = state
        .database
        .get_preference(PREF_AI_PROVIDER)
        .map_err(storage_error)?
        .map(|value| value.value)
        .unwrap_or_else(|| OPENAI_PROVIDER_ID.into());
    let model = state
        .database
        .get_preference(PREF_AI_MODEL)
        .map_err(storage_error)?
        .map(|value| value.value)
        .unwrap_or_else(|| DEFAULT_OPENAI_MODEL.into());
    let secret_json_keys = state
        .database
        .get_preference(PREF_AI_SECRET_KEYS)
        .map_err(storage_error)?
        .and_then(|value| serde_json::from_str::<Vec<String>>(&value.value).ok())
        .map(normalize_secret_keys)
        .unwrap_or_else(default_secret_json_keys);
    let store = SecretStore::default();
    let api_key_configured = if store.is_available() {
        store.get(OPENAI_KEY_REF).map_err(secret_error)?.is_some()
    } else {
        false
    };
    Ok(AiSettingsSnapshot {
        provider,
        model,
        api_key_configured,
        secure_store_available: store.is_available(),
        secret_json_keys,
    })
}

fn context_policy(state: &State<'_, AppState>) -> Result<AiContextPolicy, AppError> {
    let settings = load_settings(state)?;
    Ok(AiContextPolicy {
        secret_json_keys: settings.secret_json_keys,
        ..AiContextPolicy::default()
    })
}

fn session_context_value(
    baseline_session_id: &str,
    candidate_session_id: &str,
    state: &State<'_, AppState>,
) -> Result<Value, AppError> {
    if baseline_session_id == candidate_session_id {
        return Err(AppError::new(
            "compare_same_session",
            "Choose two different sessions before preparing AI context.",
            true,
        ));
    }
    let baseline = compare_commands::build_snapshot(baseline_session_id, state)?;
    let candidate = compare_commands::build_snapshot(candidate_session_id, state)?;
    let comparison = compare_core::compare_sessions(baseline, candidate);
    Ok(json!({
        "task": "session_diff",
        "evidencePolicy": "deterministic comparison is source of truth; AI explains evidence only",
        "comparison": comparison,
    }))
}

fn flow_context_value(flow_id: &str, state: &State<'_, AppState>) -> Result<Value, AppError> {
    let detail = state
        .database
        .get_flow_detail(flow_id)
        .map_err(storage_error)?
        .ok_or_else(|| AppError::new("ai_flow_missing", "Captured flow was not found.", true))?;
    let request_body = compare_commands::body_for_compare(
        detail
            .request
            .as_ref()
            .and_then(|request| request.body.as_ref()),
        state,
    )?;
    let response_body = compare_commands::body_for_compare(
        detail
            .response
            .as_ref()
            .and_then(|response| response.body.as_ref()),
        state,
    )?;
    let app_context = compare_commands::sdk_context_for_detail(&detail, state)?;
    Ok(json!({
        "task": "flow_diagnosis",
        "evidencePolicy": "captured network and SDK context are source of truth; AI explains evidence only",
        "flow": detail,
        "requestBody": request_body,
        "responseBody": response_body,
        "appContext": app_context,
    }))
}

fn preview_value(value: &Value, state: &State<'_, AppState>) -> Result<AiContextPreview, AppError> {
    build_context_preview(value, &context_policy(state)?).map_err(ai_error)
}

fn verify_preview(preview: &AiContextPreview, input: &AiGenerateInput) -> Result<(), AppError> {
    if preview.context_fingerprint != input.expected_context_fingerprint {
        return Err(AppError::new(
            "ai_context_changed",
            "The redacted AI context changed after preview. Review the new preview before sending it.",
            true,
        ));
    }
    Ok(())
}

async fn run_ai(
    task: AiTaskKind,
    source_ref: String,
    preview: AiContextPreview,
    state: &State<'_, AppState>,
) -> Result<AiGenerationResult, AppError> {
    let settings = load_settings(state)?;
    if settings.provider != OPENAI_PROVIDER_ID {
        return Err(AppError::new(
            "ai_provider_unsupported",
            "Configured AI provider is not available.",
            true,
        ));
    }
    let key = SecretStore::default()
        .get(OPENAI_KEY_REF)
        .map_err(secret_error)?
        .ok_or_else(|| {
            AppError::new(
                "ai_api_key_missing",
                "Add an OpenAI API key in Settings before using AI.",
                true,
            )
        })?;
    let provider = OpenAiProvider::new(key, settings.model.clone()).map_err(ai_error)?;
    let response = provider
        .generate(AiProviderRequest {
            task: task.clone(),
            context_json: preview.json.clone(),
        })
        .await
        .map_err(ai_error)?;
    let created_at = now_epoch_millis()?;
    let record = AiResultRecord {
        id: format!("ai-{}-{created_at}", task.as_str()),
        task_kind: task.as_str().into(),
        source_ref,
        provider: response.provider,
        model: response.model,
        context_fingerprint: preview.context_fingerprint.clone(),
        remote_response_id: response.remote_response_id,
        output_text: response.output_text,
        created_at,
    };
    state
        .ai_database
        .insert(&record)
        .map_err(ai_storage_error)?;
    Ok(AiGenerationResult {
        record,
        context: preview,
    })
}

fn normalize_secret_keys(values: Vec<String>) -> Vec<String> {
    let mut output = values
        .into_iter()
        .map(|value| value.trim().to_ascii_lowercase())
        .filter(|value| !value.is_empty())
        .collect::<Vec<_>>();
    output.sort();
    output.dedup();
    if output.is_empty() {
        default_secret_json_keys()
    } else {
        output
    }
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}
fn ai_storage_error(error: ai_storage::AiStorageError) -> AppError {
    AppError::storage(error.to_string())
}
fn secret_error(error: secret_store::SecretStoreError) -> AppError {
    AppError::new(error.code, error.message, true)
}
fn ai_error(error: ai_core::AiError) -> AppError {
    AppError::new(error.code, error.message, error.recoverable)
}
fn json_error(error: serde_json::Error) -> AppError {
    AppError::new("ai_settings_invalid", error.to_string(), true)
}

use async_trait::async_trait;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::time::Duration;

pub const OPENAI_PROVIDER_ID: &str = "openai";
pub const DEFAULT_OPENAI_MODEL: &str = "gpt-5.6-luna";
pub const OPENAI_RESPONSES_URL: &str = "https://api.openai.com/v1/responses";
pub const INTERNAL_CORRELATION_HEADER: &str = "x-mobile-api-studio-request-id";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AiTaskKind {
    SessionDiff,
    FlowDiagnosis,
}

impl AiTaskKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::SessionDiff => "session_diff",
            Self::FlowDiagnosis => "flow_diagnosis",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiContextPolicy {
    pub max_context_bytes: usize,
    pub max_string_chars: usize,
    pub secret_json_keys: Vec<String>,
}

impl Default for AiContextPolicy {
    fn default() -> Self {
        Self {
            max_context_bytes: 120_000,
            max_string_chars: 12_000,
            secret_json_keys: default_secret_json_keys(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiContextPreview {
    pub json: String,
    pub context_fingerprint: String,
    pub byte_count: usize,
    pub redaction_count: usize,
    pub truncated: bool,
}

#[derive(Debug, Clone)]
pub struct AiProviderRequest {
    pub task: AiTaskKind,
    pub context_json: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiProviderResponse {
    pub provider: String,
    pub model: String,
    pub remote_response_id: Option<String>,
    pub output_text: String,
}

#[async_trait]
pub trait AiProvider: Send + Sync {
    fn provider_id(&self) -> &'static str;
    fn model(&self) -> &str;
    async fn generate(&self, request: AiProviderRequest) -> Result<AiProviderResponse, AiError>;
}

#[derive(Debug, Clone)]
pub struct OpenAiProvider {
    client: Client,
    api_key: String,
    model: String,
}

impl OpenAiProvider {
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Result<Self, AiError> {
        let api_key = api_key.into();
        if api_key.trim().is_empty() {
            return Err(AiError::new("ai_api_key_missing", "OpenAI API key is required.", true));
        }
        let model = model.into();
        if model.trim().is_empty() {
            return Err(AiError::new("ai_model_missing", "OpenAI model is required.", true));
        }
        let client = Client::builder()
            .connect_timeout(Duration::from_secs(15))
            .timeout(Duration::from_secs(120))
            .build()
            .map_err(|error| AiError::new("ai_client_failed", error.to_string(), true))?;
        Ok(Self { client, api_key, model })
    }

    fn developer_instruction(task: &AiTaskKind) -> &'static str {
        match task {
            AiTaskKind::SessionDiff => {
                "You are analyzing deterministic network-session comparison evidence from a mobile API debugger. Explain only what is supported by the supplied evidence. Prioritize missing/extra calls, status or payload/schema differences, timing regressions, retry/error patterns, and app screen/feature/source context. Separate observed facts from plausible hypotheses. Give concise next debugging actions. Do not invent requests, code locations, or backend behavior."
            }
            AiTaskKind::FlowDiagnosis => {
                "You are diagnosing one captured mobile API flow. Explain only what is supported by the supplied redacted request/response/timing/app-context evidence. Identify likely failure or latency causes, distinguish facts from hypotheses, and give concise next debugging actions. Do not invent headers, payload fields, code locations, or backend behavior."
            }
        }
    }
}

#[async_trait]
impl AiProvider for OpenAiProvider {
    fn provider_id(&self) -> &'static str { OPENAI_PROVIDER_ID }
    fn model(&self) -> &str { &self.model }

    async fn generate(&self, request: AiProviderRequest) -> Result<AiProviderResponse, AiError> {
        let payload = json!({
            "model": self.model,
            "store": false,
            "input": [
                {
                    "role": "developer",
                    "content": [{"type": "input_text", "text": Self::developer_instruction(&request.task)}]
                },
                {
                    "role": "user",
                    "content": [{"type": "input_text", "text": format!("Analyze this redacted Mobile API Studio evidence:\n{}", request.context_json)}]
                }
            ]
        });

        let response = self.client
            .post(OPENAI_RESPONSES_URL)
            .bearer_auth(&self.api_key)
            .json(&payload)
            .send()
            .await
            .map_err(|error| AiError::new("ai_request_failed", error.to_string(), true))?;
        let status = response.status();
        let body: Value = response
            .json()
            .await
            .map_err(|error| AiError::new("ai_response_invalid", error.to_string(), true))?;

        if !status.is_success() {
            let message = body
                .pointer("/error/message")
                .and_then(Value::as_str)
                .unwrap_or("OpenAI request failed.");
            return Err(AiError::new(
                "ai_provider_error",
                format!("OpenAI returned HTTP {}: {}", status.as_u16(), message),
                true,
            ));
        }

        let output_text = extract_response_text(&body).ok_or_else(|| {
            AiError::new("ai_output_missing", "OpenAI response did not contain output text.", true)
        })?;

        Ok(AiProviderResponse {
            provider: OPENAI_PROVIDER_ID.into(),
            model: body.get("model").and_then(Value::as_str).unwrap_or(&self.model).to_string(),
            remote_response_id: body.get("id").and_then(Value::as_str).map(str::to_string),
            output_text,
        })
    }
}

pub fn build_context_preview(value: &Value, policy: &AiContextPolicy) -> Result<AiContextPreview, AiError> {
    let secret_keys = policy.secret_json_keys
        .iter()
        .map(|key| normalize_secret_key(key))
        .collect::<HashSet<_>>();
    let mut redactions = 0usize;
    let mut sanitized = redact_value(value, policy, &secret_keys, &mut redactions, None);
    let mut json = serde_json::to_string_pretty(&sanitized)
        .map_err(|error| AiError::new("ai_context_serialize_failed", error.to_string(), true))?;
    let mut truncated = false;

    if json.len() > policy.max_context_bytes {
        truncated = true;
        let preview = safe_prefix(&json, policy.max_context_bytes.saturating_sub(256));
        sanitized = json!({
            "contextTruncated": true,
            "originalSanitizedBytes": json.len(),
            "sanitizedPreview": preview,
        });
        json = serde_json::to_string_pretty(&sanitized)
            .map_err(|error| AiError::new("ai_context_serialize_failed", error.to_string(), true))?;
    }

    let context_fingerprint = format!("{:x}", Sha256::digest(json.as_bytes()));
    Ok(AiContextPreview {
        byte_count: json.len(),
        json,
        context_fingerprint,
        redaction_count: redactions,
        truncated,
    })
}

fn redact_value(
    value: &Value,
    policy: &AiContextPolicy,
    secret_keys: &HashSet<String>,
    redactions: &mut usize,
    parent_key: Option<&str>,
) -> Value {
    match value {
        Value::Object(object) => {
            if looks_like_header_object(object) {
                return redact_header_object(object, policy, secret_keys, redactions);
            }
            let mut next = Map::new();
            for (key, child) in object {
                let normalized_key = normalize_secret_key(key);
                if secret_keys.contains(&normalized_key) {
                    *redactions += 1;
                    next.insert(key.clone(), Value::String("<redacted>".into()));
                    continue;
                }
                next.insert(
                    key.clone(),
                    redact_value(child, policy, secret_keys, redactions, Some(key)),
                );
            }
            Value::Object(next)
        }
        Value::Array(array) => {
            let is_headers = parent_key.is_some_and(|key| key.to_ascii_lowercase().contains("header"));
            Value::Array(array.iter().filter_map(|child| {
                if is_headers && child.as_object().and_then(header_name).is_some_and(is_internal_header) {
                    *redactions += 1;
                    return None;
                }
                Some(redact_value(child, policy, secret_keys, redactions, parent_key))
            }).collect())
        }
        Value::String(text) => {
            if let Ok(parsed) = serde_json::from_str::<Value>(text) {
                if matches!(parsed, Value::Object(_) | Value::Array(_)) {
                    let nested = redact_value(&parsed, policy, secret_keys, redactions, parent_key);
                    let encoded = serde_json::to_string_pretty(&nested).unwrap_or_else(|_| "<invalid-json>".into());
                    return Value::String(truncate_string(&encoded, policy.max_string_chars));
                }
            }
            Value::String(truncate_string(text, policy.max_string_chars))
        }
        _ => value.clone(),
    }
}

fn redact_header_object(
    object: &Map<String, Value>,
    policy: &AiContextPolicy,
    secret_keys: &HashSet<String>,
    redactions: &mut usize,
) -> Value {
    let name = header_name(object).unwrap_or_default();
    if is_internal_header(name) {
        *redactions += 1;
        return json!({"name": "<internal-header-omitted>", "value": "<omitted>"});
    }
    let sensitive_flag = object.get("sensitive").and_then(Value::as_bool).unwrap_or(false);
    let sensitive_name = is_sensitive_header(name);
    let mut next = Map::new();
    for (key, child) in object {
        if key.eq_ignore_ascii_case("value") && (sensitive_flag || sensitive_name) {
            *redactions += 1;
            next.insert(key.clone(), Value::String("<redacted>".into()));
        } else {
            next.insert(key.clone(), redact_value(child, policy, secret_keys, redactions, Some(key)));
        }
    }
    Value::Object(next)
}

fn looks_like_header_object(object: &Map<String, Value>) -> bool {
    object.get("name").and_then(Value::as_str).is_some()
        && object.contains_key("value")
}

fn header_name(object: &Map<String, Value>) -> Option<&str> {
    object.get("name").and_then(Value::as_str)
}

fn is_internal_header(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(INTERNAL_CORRELATION_HEADER)
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.trim().to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
            | "x-access-token"
    )
}

fn normalize_secret_key(key: &str) -> String {
    key.trim()
        .to_ascii_lowercase()
        .replace(['-', ' ', '.'], "_")
}

fn truncate_string(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let prefix: String = value.chars().take(max_chars).collect();
    format!("{}\n… <truncated by Mobile API Studio> …", prefix)
}

fn safe_prefix(value: &str, max_bytes: usize) -> String {
    if value.len() <= max_bytes {
        return value.to_string();
    }
    let mut boundary = max_bytes.min(value.len());
    while boundary > 0 && !value.is_char_boundary(boundary) {
        boundary -= 1;
    }
    value[..boundary].to_string()
}

fn extract_response_text(response: &Value) -> Option<String> {
    let output = response.get("output")?.as_array()?;
    let mut parts = Vec::new();
    for item in output {
        if item.get("type").and_then(Value::as_str) != Some("message") {
            continue;
        }
        let Some(content) = item.get("content").and_then(Value::as_array) else { continue };
        for part in content {
            if part.get("type").and_then(Value::as_str) == Some("output_text") {
                if let Some(text) = part.get("text").and_then(Value::as_str) {
                    parts.push(text);
                }
            }
        }
    }
    if parts.is_empty() { None } else { Some(parts.join("\n")) }
}

pub fn default_secret_json_keys() -> Vec<String> {
    [
        "password", "passwd", "token", "access_token", "refresh_token", "id_token",
        "secret", "client_secret", "api_key", "apikey", "authorization", "cookie",
        "session", "session_id", "jwt", "bearer", "private_key", "credential",
    ]
    .into_iter()
    .map(str::to_string)
    .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl AiError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self { code: code.into(), message: message.into(), recoverable }
    }
}

impl std::fmt::Display for AiError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for AiError {}

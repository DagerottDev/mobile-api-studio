use core_model::{FlowSource, FlowSummary};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

pub const WORKSPACE_SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SessionUpdate {
    pub name: String,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlowSearchQuery {
    pub text: Option<String>,
    pub session_id: Option<String>,
    pub method: Option<String>,
    pub source: Option<FlowSource>,
    pub status_class: Option<u16>,
    pub min_duration_ms: Option<u64>,
    pub max_duration_ms: Option<u64>,
    pub limit: usize,
    pub offset: usize,
}

impl Default for FlowSearchQuery {
    fn default() -> Self {
        Self {
            text: None,
            session_id: None,
            method: None,
            source: None,
            status_class: None,
            min_duration_ms: None,
            max_duration_ms: None,
            limit: 500,
            offset: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct EndpointGroup {
    pub key: String,
    pub method: String,
    pub host: String,
    pub normalized_path: String,
    pub count: u64,
    pub error_count: u64,
    pub average_duration_ms: Option<f64>,
    pub latest_started_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Collection {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequestHeader {
    pub name: String,
    pub value: String,
    pub enabled: bool,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequestBody {
    pub text: Option<String>,
    pub base64: Option<String>,
    pub is_binary: bool,
    pub content_type: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequest {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<SavedRequestHeader>,
    pub body: Option<SavedRequestBody>,
    pub source_flow_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub environment_id: String,
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub secret_present: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariableInput {
    pub key: String,
    pub value: String,
    pub is_secret: bool,
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct InterpolationResult {
    pub value: String,
    pub used_secret: bool,
    pub missing_variables: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExport {
    pub schema_version: u16,
    pub exported_at: String,
    pub sessions: Vec<ExportedSession>,
    pub collections: Vec<CollectionExport>,
    pub environments: Vec<EnvironmentExport>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExportedSession {
    pub id: String,
    pub name: String,
    pub notes: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub device_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionExport {
    pub collection: Collection,
    pub requests: Vec<SavedRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentExport {
    pub environment: Environment,
    pub variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DoctorStatus {
    Pass,
    Warning,
    Fail,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DoctorCheck {
    pub id: String,
    pub title: String,
    pub status: DoctorStatus,
    pub detail: String,
    pub action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDoctorReport {
    pub checks: Vec<DoctorCheck>,
    pub capture_executable: Option<String>,
    pub booted_ios_count: usize,
    pub android_emulator_count: usize,
    pub pending_rollback: bool,
}

pub fn normalize_endpoint_path(path: &str) -> String {
    let path_only = path.split('?').next().unwrap_or(path);
    let mut result = String::new();

    for segment in path_only.split('/') {
        if segment.is_empty() {
            if result.is_empty() {
                result.push('/');
            }
            continue;
        }

        if !result.ends_with('/') {
            result.push('/');
        }
        if looks_dynamic_segment(segment) {
            result.push_str(":id");
        } else {
            result.push_str(segment);
        }
    }

    if result.is_empty() {
        "/".into()
    } else {
        result
    }
}

pub fn group_endpoints(flows: &[FlowSummary]) -> Vec<EndpointGroup> {
    #[derive(Default)]
    struct Aggregate {
        method: String,
        host: String,
        path: String,
        count: u64,
        errors: u64,
        duration_total: u128,
        duration_count: u64,
        latest: String,
    }

    let mut groups: HashMap<String, Aggregate> = HashMap::new();
    for flow in flows {
        let normalized = normalize_endpoint_path(&flow.path);
        let key = format!("{} {}{}", flow.method, flow.host, normalized);
        let entry = groups.entry(key).or_default();
        entry.method = flow.method.clone();
        entry.host = flow.host.clone();
        entry.path = normalized;
        entry.count += 1;
        if flow.status_code.unwrap_or_default() >= 400 {
            entry.errors += 1;
        }
        if let Some(duration) = flow.duration_ms {
            entry.duration_total += u128::from(duration);
            entry.duration_count += 1;
        }
        if flow.started_at > entry.latest {
            entry.latest = flow.started_at.clone();
        }
    }

    let mut result: Vec<_> = groups
        .into_iter()
        .map(|(key, aggregate)| EndpointGroup {
            key,
            method: aggregate.method,
            host: aggregate.host,
            normalized_path: aggregate.path,
            count: aggregate.count,
            error_count: aggregate.errors,
            average_duration_ms: (aggregate.duration_count > 0).then(|| {
                aggregate.duration_total as f64 / aggregate.duration_count as f64
            }),
            latest_started_at: aggregate.latest,
        })
        .collect();

    result.sort_by(|left, right| {
        right
            .count
            .cmp(&left.count)
            .then_with(|| left.key.cmp(&right.key))
    });
    result
}

pub fn interpolate(template: &str, values: &HashMap<String, (String, bool)>) -> InterpolationResult {
    let mut output = String::with_capacity(template.len());
    let mut missing = Vec::new();
    let mut used_secret = false;
    let bytes = template.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if index + 1 < bytes.len() && bytes[index] == b'{' && bytes[index + 1] == b'{' {
            if let Some(relative_end) = template[index + 2..].find("}}") {
                let end = index + 2 + relative_end;
                let key = template[index + 2..end].trim();
                if let Some((value, secret)) = values.get(key) {
                    output.push_str(value);
                    used_secret |= *secret;
                } else {
                    missing.push(key.to_string());
                    output.push_str(&template[index..end + 2]);
                }
                index = end + 2;
                continue;
            }
        }

        let character = template[index..].chars().next().expect("valid UTF-8 boundary");
        output.push(character);
        index += character.len_utf8();
    }

    missing.sort();
    missing.dedup();
    InterpolationResult {
        value: output,
        used_secret,
        missing_variables: missing,
    }
}

fn looks_dynamic_segment(segment: &str) -> bool {
    if !segment.is_empty() && segment.chars().all(|character| character.is_ascii_digit()) {
        return true;
    }

    let lower = segment.to_ascii_lowercase();
    if lower.len() == 36 {
        let expected_hyphens = [8, 13, 18, 23];
        if lower.chars().enumerate().all(|(index, character)| {
            if expected_hyphens.contains(&index) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        }) {
            return true;
        }
    }

    lower.len() >= 20
        && lower
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
}

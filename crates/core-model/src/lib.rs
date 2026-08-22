use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlowSource {
    Proxy,
    Replay,
    Mock,
    Sdk,
    Fixture,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SessionStatus {
    Active,
    Completed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct DeviceCapabilities {
    pub can_install_ca: bool,
    pub can_auto_route_proxy: bool,
    pub can_target_process: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Device {
    pub schema_version: u16,
    pub id: String,
    pub platform: DevicePlatform,
    pub name: String,
    pub os_version: Option<String>,
    pub state: String,
    pub capabilities: DeviceCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSession {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub status: SessionStatus,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub device_id: Option<String>,
    pub app_id: Option<String>,
    pub connection_strategy: Option<String>,
    pub capture_engine: Option<String>,
    pub notes: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct HeaderValue {
    pub name: String,
    pub value: String,
    pub sensitive: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct BodyRef {
    pub sha256: String,
    pub byte_size: u64,
    pub content_type: Option<String>,
    pub encoding: Option<String>,
    pub is_binary: bool,
    pub is_truncated: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Timing {
    pub dns_ms: Option<u64>,
    pub connect_ms: Option<u64>,
    pub tls_ms: Option<u64>,
    pub request_ms: Option<u64>,
    pub server_ms: Option<u64>,
    pub download_ms: Option<u64>,
    pub total_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RequestDetail {
    pub method: String,
    pub url: String,
    pub scheme: String,
    pub host: String,
    pub port: Option<u16>,
    pub path: String,
    pub query: Option<String>,
    pub headers: Vec<HeaderValue>,
    pub body: Option<BodyRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ResponseDetail {
    pub status_code: u16,
    pub reason: Option<String>,
    pub headers: Vec<HeaderValue>,
    pub body: Option<BodyRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlowSummary {
    pub schema_version: u16,
    pub id: String,
    pub session_id: Option<String>,
    pub source: FlowSource,
    pub method: String,
    pub host: String,
    pub path: String,
    pub status_code: Option<u16>,
    pub duration_ms: Option<u64>,
    pub response_size_bytes: Option<u64>,
    pub started_at: String,
}

impl FlowSummary {
    #[allow(clippy::too_many_arguments)]
    pub fn fixture(
        id: impl Into<String>,
        method: impl Into<String>,
        host: impl Into<String>,
        path: impl Into<String>,
        status_code: u16,
        duration_ms: u64,
        response_size_bytes: u64,
        started_at: impl Into<String>,
    ) -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            id: id.into(),
            session_id: None,
            source: FlowSource::Fixture,
            method: method.into(),
            host: host.into(),
            path: path.into(),
            status_code: Some(status_code),
            duration_ms: Some(duration_ms),
            response_size_bytes: Some(response_size_bytes),
            started_at: started_at.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct FlowDetail {
    pub summary: FlowSummary,
    pub request: Option<RequestDetail>,
    pub response: Option<ResponseDetail>,
    pub timing: Timing,
    pub error_code: Option<String>,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDiagnostic {
    pub code: String,
    pub title: String,
    pub message: String,
    pub recoverable: bool,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl AppError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }

    pub fn storage(message: impl Into<String>) -> Self {
        Self::new("storage_failure", message, true)
    }

    pub fn initialization(message: impl Into<String>) -> Self {
        Self::new("initialization_failed", message, false)
    }
}

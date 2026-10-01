use serde::{Deserialize, Serialize};

pub mod proxy_rules;
pub mod network_profiles;

pub const SCHEMA_VERSION: u16 = 1;
pub const PROJECT_BUNDLE_VERSION: u16 = 1;

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
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum DevicePlatform {
    Ios,
    Android,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureTarget {
    pub schema_version: u16,
    #[serde(flatten)]
    pub kind: CaptureTargetKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(
    tag = "type",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum CaptureTargetKind {
    IosSimulator { device_id: String },
    AndroidEmulator { device_id: String },
    MacAll,
    MacProcess { pid: u32, name: String },
    PhysicalIos { address: String, interface: String },
    PhysicalAndroid { address: String, interface: String },
    ProxyListener { mode: CaptureModeKind, listen_port: u16 },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureMode {
    pub schema_version: u16,
    #[serde(flatten)]
    pub kind: CaptureModeKind,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum CaptureModeKind {
    RegularProxy,
    LocalAll,
    LocalProcess { pid: u32 },
    ReverseProxy { url: String },
    UpstreamProxy { url: String },
    Socks5,
    DnsProxy,
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
    #[serde(default)]
    pub capture_target: Option<CaptureTarget>,
    #[serde(default)]
    pub capture_mode: Option<CaptureMode>,
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
    #[serde(default)]
    pub proxy_rule_ids: Vec<String>,
    #[serde(default)]
    pub proxy_rule_changes: Vec<ProxyRuleChange>,
    #[serde(default)]
    pub protocol: Option<ProtocolDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CertificateDetails {
    pub subject: String,
    pub issuer: String,
    pub not_before: String,
    pub not_after: String,
    pub sha256: String,
    pub subject_alternative_names: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionDetails {
    pub id: String,
    pub transport: String,
    pub peer_address: Option<String>,
    pub local_address: Option<String>,
    pub server_address: Option<String>,
    pub tls_version: Option<String>,
    pub cipher: Option<String>,
    pub alpn: Option<String>,
    pub sni: Option<String>,
    pub tls_established: bool,
    pub started_at: Option<String>,
    pub tls_established_at: Option<String>,
    pub ended_at: Option<String>,
    pub peer_certificates: Vec<CertificateDetails>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct ProtocolDetails {
    pub request_http_version: Option<String>,
    pub response_http_version: Option<String>,
    pub request_trailers: Vec<HeaderValue>,
    pub response_trailers: Vec<HeaderValue>,
    pub client_connection: Option<ConnectionDetails>,
    pub server_connection: Option<ConnectionDetails>,
    pub websocket: bool,
    #[serde(default)]
    pub websocket_close_code: Option<u16>,
    #[serde(default)]
    pub websocket_close_reason: Option<String>,
    #[serde(default)]
    pub websocket_closed_by_client: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct WebSocketMessage {
    pub id: String,
    pub flow_id: String,
    pub session_id: Option<String>,
    pub sequence: u64,
    pub from_client: bool,
    pub opcode: u8,
    pub timestamp: String,
    pub dropped: bool,
    pub injected: bool,
    pub body: Option<BodyRef>,
}

pub fn validate_protocol_details(protocol: &ProtocolDetails) -> Result<(), String> {
    let text_ok = |value: &str| value.len() <= 1_024;
    if protocol.request_trailers.len() > 64 || protocol.response_trailers.len() > 64
        || protocol.request_http_version.as_deref().is_some_and(|value| !text_ok(value))
        || protocol.response_http_version.as_deref().is_some_and(|value| !text_ok(value))
        || protocol.websocket_close_reason.as_deref().is_some_and(|value| !text_ok(value))
        || protocol.request_trailers.iter().chain(&protocol.response_trailers).any(|header| !text_ok(&header.name) || !text_ok(&header.value)) {
        return Err("Protocol metadata exceeds trailer or text limits.".into());
    }
    for connection in [protocol.client_connection.as_ref(), protocol.server_connection.as_ref()].into_iter().flatten() {
        if connection.id.is_empty() || connection.id.len() > 512 || connection.peer_certificates.len() > 16
            || !text_ok(&connection.transport)
            || [connection.peer_address.as_deref(), connection.local_address.as_deref(), connection.server_address.as_deref(),
                connection.tls_version.as_deref(), connection.cipher.as_deref(), connection.alpn.as_deref(), connection.sni.as_deref(),
                connection.started_at.as_deref(), connection.tls_established_at.as_deref(), connection.ended_at.as_deref()]
                .into_iter().flatten().any(|value| !text_ok(value)) {
            return Err("Connection metadata exceeds supported limits.".into());
        }
        for cert in &connection.peer_certificates {
            if cert.subject_alternative_names.len() > 64
                || [&cert.subject, &cert.issuer, &cert.not_before, &cert.not_after, &cert.sha256]
                    .into_iter().any(|value| !text_ok(value))
                || cert.sha256.len() != 64 || !cert.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                || cert.subject_alternative_names.iter().any(|value| !text_ok(value)) {
                return Err("Certificate metadata exceeds supported limits.".into());
            }
        }
    }
    if serde_json::to_vec(protocol).map_err(|error| error.to_string())?.len() > 64 * 1024 {
        return Err("Protocol metadata exceeds 64 KiB.".into());
    }
    Ok(())
}

pub fn validate_websocket_message(message: &WebSocketMessage) -> Result<(), String> {
    if message.id.is_empty() || message.id.len() > 512 || message.flow_id.is_empty() || message.flow_id.len() > 512
        || message.session_id.as_deref().is_some_and(|value| value.len() > 512)
        || message.sequence == 0 || message.opcode > 15
        || message.timestamp.is_empty() || message.timestamp.len() > 32 || !message.timestamp.bytes().all(|byte| byte.is_ascii_digit())
        || message.body.as_ref().is_some_and(|body| {
            body.sha256.len() != 64 || !body.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
                || body.byte_size > 2 * 1024 * 1024
                || body.content_type.as_deref().is_some_and(|value| value.len() > 1_024)
                || body.encoding.as_deref().is_some_and(|value| value.len() > 1_024)
        }) {
        return Err("WebSocket message metadata exceeds supported limits.".into());
    }
    if serde_json::to_vec(message).map_err(|error| error.to_string())?.len() > 64 * 1024 {
        return Err("WebSocket message metadata exceeds 64 KiB.".into());
    }
    Ok(())
}

#[cfg(test)]
mod protocol_model_tests {
    use super::*;

    #[test]
    fn old_flow_details_default_protocol_and_new_fields_use_camel_case() {
        let detail = FlowDetail {
            summary: FlowSummary::fixture("flow", "GET", "example.test", "/", 200, 1, 0, "1"),
            request: None, response: None, timing: Timing::default(), error_code: None,
            error_message: None, proxy_rule_ids: vec![], proxy_rule_changes: vec![], protocol: None,
        };
        let mut json = serde_json::to_value(detail).unwrap();
        json.as_object_mut().unwrap().remove("protocol");
        let restored: FlowDetail = serde_json::from_value(json).unwrap();
        assert!(restored.protocol.is_none());
        let protocol = ProtocolDetails { request_http_version: Some("HTTP/2".into()), websocket: true,
            websocket_close_code: Some(1000), ..Default::default() };
        let value = serde_json::to_value(protocol).unwrap();
        assert_eq!(value["requestHttpVersion"], "HTTP/2");
        assert_eq!(value["websocketCloseCode"], 1000);
        assert!(validate_protocol_details(&ProtocolDetails::default()).is_ok());
        let oversized = ProtocolDetails { request_http_version: Some("x".repeat(1_025)), ..Default::default() };
        assert!(validate_protocol_details(&oversized).is_err());
        let message = WebSocketMessage { id: "m".into(), flow_id: "f".into(), session_id: None, sequence: 1,
            from_client: true, opcode: 1, timestamp: "1".into(), dropped: false, injected: false, body: None };
        assert!(validate_websocket_message(&message).is_ok());
        assert!(validate_websocket_message(&WebSocketMessage { opcode: 255, ..message }).is_err());
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProxyRuleChange {
    pub rule_id: String,
    pub field: String,
    pub before: String,
    pub after: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedEndpoint {
    pub key: String,
    pub method: String,
    pub host: String,
    pub path_template: String,
}

pub fn normalize_endpoint(method: &str, host: &str, path: &str) -> NormalizedEndpoint {
    let clean_path = path.split('?').next().unwrap_or(path);
    let path_template = clean_path
        .split('/')
        .map(normalize_path_segment)
        .collect::<Vec<_>>()
        .join("/");
    let method = method.trim().to_uppercase();
    let host = host.trim().to_lowercase();
    NormalizedEndpoint {
        key: format!("{} {}{}", method, host, path_template),
        method,
        host,
        path_template,
    }
}

fn normalize_path_segment(segment: &str) -> &str {
    if segment.is_empty() {
        return segment;
    }
    if segment.bytes().all(|byte| byte.is_ascii_digit()) {
        return ":id";
    }
    if looks_like_uuid(segment) {
        return ":uuid";
    }
    if segment.len() >= 16 && segment.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return ":hex";
    }
    segment
}

fn looks_like_uuid(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    value.bytes().enumerate().all(|(index, byte)| match index {
        8 | 13 | 18 | 23 => byte == b'-',
        _ => byte.is_ascii_hexdigit(),
    })
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedCollection {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequestBody {
    pub text: Option<String>,
    pub base64: Option<String>,
    pub content_type: Option<String>,
    pub is_binary: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SavedRequest {
    pub schema_version: u16,
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub method: String,
    pub url: String,
    pub headers: Vec<HeaderValue>,
    pub body: Option<SavedRequestBody>,
    pub source_flow_id: Option<String>,
    pub sort_order: i64,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Environment {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub is_active: bool,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct EnvironmentVariable {
    pub schema_version: u16,
    pub id: String,
    pub environment_id: String,
    pub key: String,
    pub value: Option<String>,
    pub is_secret: bool,
    pub secret_ref: Option<String>,
    pub enabled: bool,
    pub sort_order: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSearchQuery {
    pub text: Option<String>,
    pub session_id: Option<String>,
    pub source: Option<FlowSource>,
    pub method: Option<String>,
    pub status_class: Option<u16>,
    pub endpoint_key: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct TrafficSearchResult {
    pub flow: FlowSummary,
    pub endpoint: NormalizedEndpoint,
    pub session_name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AppPreference {
    pub key: String,
    pub value: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct OnboardingStep {
    pub key: String,
    pub completed: bool,
    pub completed_at: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ProjectBundle {
    pub bundle_version: u16,
    pub exported_at: String,
    pub sessions: Vec<CaptureSession>,
    pub collections: Vec<SavedCollection>,
    pub saved_requests: Vec<SavedRequest>,
    pub environments: Vec<Environment>,
    pub environment_variables: Vec<EnvironmentVariable>,
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

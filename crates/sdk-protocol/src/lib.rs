use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

pub const SDK_PROTOCOL_VERSION: u16 = 1;
pub const SDK_INGESTION_PORT: u16 = 8182;
pub const SDK_EVENT_PATH: &str = "/v1/events";
pub const SDK_HEALTH_PATH: &str = "/health";
pub const SDK_CORRELATION_HEADER: &str = "X-Mobile-API-Studio-Request-Id";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SdkPlatform {
    Ios,
    Android,
    Other,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SdkLogLevel {
    Debug,
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SdkNetworkPhase {
    Started,
    Completed,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkSourceLocation {
    pub file: Option<String>,
    pub function: Option<String>,
    pub line: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub struct SdkContextSnapshot {
    pub screen: Option<String>,
    pub feature: Option<String>,
    pub attributes: BTreeMap<String, String>,
    pub source: Option<SdkSourceLocation>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkHandshake {
    pub client_id: String,
    pub app_id: String,
    pub app_name: String,
    pub app_version: Option<String>,
    pub app_build: Option<String>,
    pub platform: SdkPlatform,
    pub device_name: Option<String>,
    pub os_version: Option<String>,
    pub sdk_version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkContextEvent {
    pub client_id: String,
    pub context: SdkContextSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkLogEvent {
    pub client_id: String,
    pub level: SdkLogLevel,
    pub message: String,
    pub context: SdkContextSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SdkNetworkEvent {
    pub client_id: String,
    pub request_id: String,
    pub phase: SdkNetworkPhase,
    pub method: String,
    pub url: String,
    pub status_code: Option<u16>,
    pub duration_ms: Option<u64>,
    pub error: Option<String>,
    pub context: SdkContextSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum SdkEvent {
    Handshake(SdkHandshake),
    Context(SdkContextEvent),
    Log(SdkLogEvent),
    Network(SdkNetworkEvent),
}

impl SdkEvent {
    pub fn client_id(&self) -> &str {
        match self {
            Self::Handshake(value) => &value.client_id,
            Self::Context(value) => &value.client_id,
            Self::Log(value) => &value.client_id,
            Self::Network(value) => &value.client_id,
        }
    }

    pub fn request_id(&self) -> Option<&str> {
        match self {
            Self::Network(value) => Some(&value.request_id),
            _ => None,
        }
    }

    pub fn kind(&self) -> &'static str {
        match self {
            Self::Handshake(_) => "handshake",
            Self::Context(_) => "context",
            Self::Log(_) => "log",
            Self::Network(_) => "network",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct SdkEnvelope {
    pub schema_version: u16,
    pub event_id: String,
    pub occurred_at: String,
    pub event: SdkEvent,
}

impl SdkEnvelope {
    pub fn validate(&self) -> Result<(), SdkProtocolError> {
        if self.schema_version != SDK_PROTOCOL_VERSION {
            return Err(SdkProtocolError::new(
                "sdk_schema_unsupported",
                format!(
                    "Unsupported SDK schema version {}. Desktop supports {}.",
                    self.schema_version, SDK_PROTOCOL_VERSION
                ),
            ));
        }
        if self.event_id.trim().is_empty() {
            return Err(SdkProtocolError::new("sdk_event_id_required", "eventId is required"));
        }
        if self.occurred_at.trim().is_empty() {
            return Err(SdkProtocolError::new(
                "sdk_occurred_at_required",
                "occurredAt is required",
            ));
        }
        if self.event.client_id().trim().is_empty() {
            return Err(SdkProtocolError::new("sdk_client_id_required", "clientId is required"));
        }
        if let SdkEvent::Network(network) = &self.event {
            if network.request_id.trim().is_empty() {
                return Err(SdkProtocolError::new(
                    "sdk_request_id_required",
                    "requestId is required for network events",
                ));
            }
            if network.method.trim().is_empty() || network.url.trim().is_empty() {
                return Err(SdkProtocolError::new(
                    "sdk_network_request_invalid",
                    "Network events require method and URL",
                ));
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkProtocolError {
    pub code: String,
    pub message: String,
}

impl SdkProtocolError {
    pub fn new(code: impl Into<String>, message: impl Into<String>) -> Self {
        Self { code: code.into(), message: message.into() }
    }
}

impl std::fmt::Display for SdkProtocolError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SdkProtocolError {}

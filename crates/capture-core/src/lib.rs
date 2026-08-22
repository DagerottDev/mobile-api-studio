use async_trait::async_trait;
use core_model::FlowSummary;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureConfig {
    pub session_id: String,
    pub listen_host: String,
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureCapabilities {
    pub engine_name: String,
    pub engine_version: Option<String>,
    pub supports_https: bool,
    pub supports_http2: bool,
    pub supports_websocket: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureHandle {
    pub id: String,
    pub session_id: String,
    pub listen_host: String,
    pub listen_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CaptureLifecycleState {
    Idle,
    Preparing,
    Starting,
    Ready,
    Stopping,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "type", content = "payload", rename_all = "snake_case")]
pub enum CaptureEvent {
    LifecycleChanged(CaptureLifecycleState),
    EngineReady(CaptureCapabilities),
    FlowStarted(FlowSummary),
    FlowUpdated(FlowSummary),
    FlowCompleted(FlowSummary),
    FlowFailed { flow_id: String, code: String, message: String },
    EngineFailed {
        code: String,
        message: String,
        recoverable: bool,
    },
    EngineStopped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CaptureError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
}

impl CaptureError {
    pub fn new(code: impl Into<String>, message: impl Into<String>, recoverable: bool) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
            recoverable,
        }
    }
}

#[async_trait]
pub trait CaptureEngine: Send + Sync {
    async fn prepare(&self) -> Result<CaptureCapabilities, CaptureError>;

    async fn start(&self, config: CaptureConfig) -> Result<CaptureHandle, CaptureError>;

    async fn stop(&self, handle: CaptureHandle) -> Result<(), CaptureError>;

    fn subscribe(&self) -> broadcast::Receiver<CaptureEvent>;
}

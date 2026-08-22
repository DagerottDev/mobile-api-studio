use async_trait::async_trait;
use capture_core::{
    CaptureCapabilities, CaptureConfig, CaptureEngine, CaptureError, CaptureEvent, CaptureHandle,
    CaptureLifecycleState,
};
use core_model::{FlowSource, FlowSummary, SCHEMA_VERSION};
use serde::Deserialize;
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
    sync::{broadcast, Mutex},
};

const EVENT_PREFIX: &str = "MAS_EVENT ";

#[derive(Debug, Clone)]
pub struct MitmDumpEngine {
    executable: PathBuf,
    addon_path: PathBuf,
    conf_dir: PathBuf,
    sender: broadcast::Sender<CaptureEvent>,
    children: Arc<Mutex<HashMap<String, Child>>>,
}

impl MitmDumpEngine {
    pub fn new(addon_path: impl Into<PathBuf>, conf_dir: impl Into<PathBuf>) -> Self {
        let (sender, _) = broadcast::channel(4_096);
        Self {
            executable: PathBuf::from("mitmdump"),
            addon_path: addon_path.into(),
            conf_dir: conf_dir.into(),
            sender,
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = executable.into();
        self
    }

    pub fn certificate_path(&self) -> PathBuf {
        self.conf_dir.join("mitmproxy-ca-cert.pem")
    }

    pub fn conf_dir(&self) -> &Path {
        &self.conf_dir
    }

    async fn spawn_stdout_reader(
        &self,
        stdout: tokio::process::ChildStdout,
        session_id: String,
    ) {
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(payload) = line.strip_prefix(EVENT_PREFIX) else {
                    continue;
                };

                match serde_json::from_str::<BridgeEvent>(payload) {
                    Ok(event) => publish_bridge_event(&sender, &session_id, event),
                    Err(error) => {
                        let _ = sender.send(CaptureEvent::EngineFailed {
                            code: "mitm_bridge_parse_failed".into(),
                            message: error.to_string(),
                            recoverable: true,
                        });
                    }
                }
            }
        });
    }

    async fn spawn_stderr_drain(&self, stderr: tokio::process::ChildStderr) {
        tokio::spawn(async move {
            let mut lines = BufReader::new(stderr).lines();
            while let Ok(Some(_line)) = lines.next_line().await {
                // Keep stderr drained so the child cannot block on a full pipe.
                // Structured runtime diagnostics are emitted by the bridge itself.
            }
        });
    }
}

#[async_trait]
impl CaptureEngine for MitmDumpEngine {
    async fn prepare(&self) -> Result<CaptureCapabilities, CaptureError> {
        let _ = self
            .sender
            .send(CaptureEvent::LifecycleChanged(CaptureLifecycleState::Preparing));

        if !self.addon_path.is_file() {
            return Err(CaptureError::new(
                "mitm_addon_missing",
                format!("Capture addon not found at {}", self.addon_path.display()),
                true,
            ));
        }

        fs::create_dir_all(&self.conf_dir).map_err(|error| {
            CaptureError::new("mitm_confdir_failed", error.to_string(), true)
        })?;

        let output = Command::new(&self.executable)
            .arg("--version")
            .output()
            .await
            .map_err(|error| {
                CaptureError::new(
                    "mitmdump_unavailable",
                    format!("Unable to launch mitmdump: {error}"),
                    true,
                )
            })?;

        if !output.status.success() {
            return Err(CaptureError::new(
                "mitmdump_version_failed",
                String::from_utf8_lossy(&output.stderr).trim().to_string(),
                true,
            ));
        }

        let version = String::from_utf8_lossy(&output.stdout)
            .lines()
            .next()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string);

        Ok(CaptureCapabilities {
            engine_name: "mitmdump".into(),
            engine_version: version,
            supports_https: true,
            supports_http2: true,
            supports_websocket: true,
        })
    }

    async fn start(&self, config: CaptureConfig) -> Result<CaptureHandle, CaptureError> {
        let capabilities = self.prepare().await?;
        let _ = self
            .sender
            .send(CaptureEvent::LifecycleChanged(CaptureLifecycleState::Starting));

        let handle_id = format!("mitm-{}", now_epoch_millis());
        let mut command = Command::new(&self.executable);
        command
            .arg("--quiet")
            .arg("--listen-host")
            .arg(&config.listen_host)
            .arg("--listen-port")
            .arg(config.listen_port.to_string())
            .arg("--set")
            .arg(format!("confdir={}", self.conf_dir.display()))
            .arg("--set")
            .arg("block_global=false")
            .arg("-s")
            .arg(&self.addon_path)
            .env("MAS_SESSION_ID", &config.session_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        let mut child = command.spawn().map_err(|error| {
            CaptureError::new(
                "mitmdump_start_failed",
                format!("Unable to start mitmdump: {error}"),
                true,
            )
        })?;

        let stdout = child.stdout.take().ok_or_else(|| {
            CaptureError::new(
                "mitmdump_stdout_unavailable",
                "mitmdump stdout pipe was not available",
                true,
            )
        })?;
        let stderr = child.stderr.take().ok_or_else(|| {
            CaptureError::new(
                "mitmdump_stderr_unavailable",
                "mitmdump stderr pipe was not available",
                true,
            )
        })?;

        self.spawn_stdout_reader(stdout, config.session_id.clone()).await;
        self.spawn_stderr_drain(stderr).await;
        self.children.lock().await.insert(handle_id.clone(), child);

        let _ = self.sender.send(CaptureEvent::EngineReady(capabilities));
        let _ = self
            .sender
            .send(CaptureEvent::LifecycleChanged(CaptureLifecycleState::Ready));

        Ok(CaptureHandle {
            id: handle_id,
            session_id: config.session_id,
            listen_host: config.listen_host,
            listen_port: config.listen_port,
        })
    }

    async fn stop(&self, handle: CaptureHandle) -> Result<(), CaptureError> {
        let _ = self
            .sender
            .send(CaptureEvent::LifecycleChanged(CaptureLifecycleState::Stopping));

        let child = self.children.lock().await.remove(&handle.id);
        if let Some(mut child) = child {
            child.kill().await.map_err(|error| {
                CaptureError::new(
                    "mitmdump_stop_failed",
                    format!("Unable to stop mitmdump: {error}"),
                    true,
                )
            })?;
        }

        let _ = self.sender.send(CaptureEvent::EngineStopped);
        let _ = self
            .sender
            .send(CaptureEvent::LifecycleChanged(CaptureLifecycleState::Idle));
        Ok(())
    }

    fn subscribe(&self) -> broadcast::Receiver<CaptureEvent> {
        self.sender.subscribe()
    }
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum BridgeEvent {
    FlowCompleted {
        id: String,
        method: String,
        host: String,
        path: String,
        status_code: u16,
        duration_ms: Option<u64>,
        response_size_bytes: Option<u64>,
        started_at: String,
    },
    FlowFailed {
        id: String,
        code: String,
        message: String,
    },
}

fn publish_bridge_event(
    sender: &broadcast::Sender<CaptureEvent>,
    session_id: &str,
    event: BridgeEvent,
) {
    match event {
        BridgeEvent::FlowCompleted {
            id,
            method,
            host,
            path,
            status_code,
            duration_ms,
            response_size_bytes,
            started_at,
        } => {
            let flow = FlowSummary {
                schema_version: SCHEMA_VERSION,
                id,
                session_id: Some(session_id.to_string()),
                source: FlowSource::Proxy,
                method,
                host,
                path,
                status_code: Some(status_code),
                duration_ms,
                response_size_bytes,
                started_at,
            };
            let _ = sender.send(CaptureEvent::FlowCompleted(flow));
        }
        BridgeEvent::FlowFailed { id, code, message } => {
            let _ = sender.send(CaptureEvent::FlowFailed {
                flow_id: id,
                code,
                message,
            });
        }
    }
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

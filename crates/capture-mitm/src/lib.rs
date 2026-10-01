use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use capture_core::{
    CaptureCapabilities, CaptureConfig, CaptureEngine, CaptureError, CaptureEvent, CaptureHandle,
    CaptureLifecycleState, CapturedBody, CapturedFlow, CapturedRequest, CapturedResponse,
    CapturedWebSocketMessage,
};
use core_model::{
    CaptureModeKind, FlowSource, FlowSummary, HeaderValue, ProtocolDetails, SCHEMA_VERSION, Timing,
};
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
    sync::{Mutex, broadcast, oneshot},
    time::{Duration, timeout},
};

const EVENT_PREFIX: &str = "MAS_EVENT ";
const MAX_BODY_BYTES: usize = 2 * 1024 * 1024;
const MAX_PROTOCOL_TEXT: usize = 1024;

fn mode_spec(mode: &CaptureModeKind) -> String {
    match mode {
        CaptureModeKind::RegularProxy => "regular".into(),
        CaptureModeKind::ReverseProxy { url } => format!("reverse:{url}"),
        CaptureModeKind::UpstreamProxy { url } => format!("upstream:{url}"),
        CaptureModeKind::Socks5 => "socks5".into(),
        CaptureModeKind::DnsProxy => "dns".into(),
        CaptureModeKind::LocalAll => "local".into(),
        CaptureModeKind::LocalProcess { pid } => format!("local:{pid}"),
    }
}

#[derive(Debug, Clone)]
pub struct MitmDumpEngine {
    executable: PathBuf,
    addon_path: PathBuf,
    conf_dir: PathBuf,
    rule_socket_path: Option<PathBuf>,
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
            rule_socket_path: None,
            sender,
            children: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn with_executable(mut self, executable: impl Into<PathBuf>) -> Self {
        self.executable = executable.into();
        self
    }

    pub fn with_rule_socket(mut self, path: impl Into<PathBuf>) -> Self {
        self.rule_socket_path = Some(path.into());
        self
    }

    pub fn certificate_path(&self) -> PathBuf {
        self.conf_dir.join("mitmproxy-ca-cert.pem")
    }

    pub fn conf_dir(&self) -> &Path {
        &self.conf_dir
    }

    pub async fn is_running(&self, handle: &CaptureHandle) -> bool {
        self.children
            .lock()
            .await
            .get_mut(&handle.id)
            .is_some_and(|child| matches!(child.try_wait(), Ok(None)))
    }

    async fn spawn_stdout_reader(
        &self,
        stdout: tokio::process::ChildStdout,
        session_id: String,
        ready: oneshot::Sender<()>,
    ) {
        let sender = self.sender.clone();
        tokio::spawn(async move {
            let mut lines = BufReader::new(stdout).lines();
            let mut ready = Some(ready);
            while let Ok(Some(line)) = lines.next_line().await {
                let Some(payload) = line.strip_prefix(EVENT_PREFIX) else {
                    continue;
                };

                match serde_json::from_str::<BridgeEvent>(payload) {
                    Ok(BridgeEvent::EngineStarted) => {
                        if let Some(ready) = ready.take() {
                            let _ = ready.send(());
                        }
                    }
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
                // Structured flow diagnostics are emitted by the bridge itself.
            }
        });
    }
}

#[async_trait]
impl CaptureEngine for MitmDumpEngine {
    async fn prepare(&self) -> Result<CaptureCapabilities, CaptureError> {
        let _ = self.sender.send(CaptureEvent::LifecycleChanged(
            CaptureLifecycleState::Preparing,
        ));

        if !self.addon_path.is_file() {
            return Err(CaptureError::new(
                "mitm_addon_missing",
                format!("Capture addon not found at {}", self.addon_path.display()),
                true,
            ));
        }

        fs::create_dir_all(&self.conf_dir)
            .map_err(|error| CaptureError::new("mitm_confdir_failed", error.to_string(), true))?;

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
        let _ = self.sender.send(CaptureEvent::LifecycleChanged(
            CaptureLifecycleState::Starting,
        ));

        let handle_id = format!("mitm-{}", now_epoch_millis());
        let mut command = Command::new(&self.executable);
        command
            .arg("--quiet")
            .arg("--set")
            .arg(format!("confdir={}", self.conf_dir.display()))
            .arg("--set")
            .arg("block_global=false")
            .arg("--set")
            .arg("connection_strategy=lazy")
            .arg("-s")
            .arg(&self.addon_path)
            .env("MAS_SESSION_ID", &config.session_id)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        if let Some(path) = &self.rule_socket_path {
            command.env("MAS_RULE_SOCKET", path);
        }

        let mode = mode_spec(&config.mode.kind);
        command.arg("--mode").arg(&mode);
        if !matches!(
            config.mode.kind,
            CaptureModeKind::LocalAll | CaptureModeKind::LocalProcess { .. }
        ) {
            command.arg("--listen-host").arg(&config.listen_host);
            command
                .arg("--listen-port")
                .arg(config.listen_port.to_string());
        }

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

        let (ready_sender, ready_receiver) = oneshot::channel();
        self.spawn_stdout_reader(stdout, config.session_id.clone(), ready_sender)
            .await;
        self.spawn_stderr_drain(stderr).await;
        let startup = tokio::select! {
            result = timeout(Duration::from_secs(20), ready_receiver) => match result {
                Ok(Ok(())) => Ok(()),
                Ok(Err(_)) => Err("Capture bridge closed before the listener was ready."),
                Err(_) => Err("Capture listener did not become ready within 20 seconds."),
            },
            _ = child.wait() => Err("Capture engine exited before the listener was ready. Check the mode, port, and capture permissions."),
        };
        if let Err(message) = startup {
            let _ = child.kill().await;
            let _ = self.sender.send(CaptureEvent::LifecycleChanged(
                CaptureLifecycleState::Failed,
            ));
            return Err(CaptureError::new(
                "mitmdump_listener_start_failed",
                message,
                true,
            ));
        }
        if child
            .try_wait()
            .map_err(|error| CaptureError::new("mitmdump_status_failed", error.to_string(), true))?
            .is_some()
        {
            return Err(CaptureError::new(
                "mitmdump_listener_start_failed",
                "Capture engine exited during startup.",
                true,
            ));
        }
        self.children.lock().await.insert(handle_id.clone(), child);

        let children = self.children.clone();
        let sender = self.sender.clone();
        let monitored_id = handle_id.clone();
        tokio::spawn(async move {
            loop {
                tokio::time::sleep(Duration::from_millis(500)).await;
                let mut children = children.lock().await;
                let Some(child) = children.get_mut(&monitored_id) else {
                    break;
                };
                if !matches!(child.try_wait(), Ok(None)) {
                    children.remove(&monitored_id);
                    let _ = sender.send(CaptureEvent::EngineFailed {
                        code: "mitmdump_unexpected_exit".into(),
                        message: "Capture engine stopped unexpectedly. Disconnect to restore the capture settings before reconnecting.".into(),
                        recoverable: true,
                    });
                    let _ = sender.send(CaptureEvent::LifecycleChanged(
                        CaptureLifecycleState::Failed,
                    ));
                    break;
                }
            }
        });

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
        let _ = self.sender.send(CaptureEvent::LifecycleChanged(
            CaptureLifecycleState::Stopping,
        ));

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
    EngineStarted,
    FlowCompleted {
        id: String,
        started_at: String,
        response_size_bytes: Option<u64>,
        request: BridgeRequest,
        response: BridgeResponse,
        timing: BridgeTiming,
        mock_rule_id: Option<String>,
        mock_rule_name: Option<String>,
        #[serde(default)]
        proxy_rule_ids: Vec<String>,
        #[serde(default)]
        proxy_rule_changes: Vec<core_model::ProxyRuleChange>,
        protocol: Option<ProtocolDetails>,
    },
    WebSocketMessage {
        id: String,
        flow_id: String,
        session_id: Option<String>,
        sequence: u64,
        from_client: bool,
        opcode: u8,
        timestamp: String,
        dropped: bool,
        injected: bool,
        body: Option<BridgeBody>,
    },
    WebSocketClosed {
        flow_id: String,
        close_code: Option<u16>,
        close_reason: Option<String>,
        closed_by_client: Option<bool>,
    },
    FlowFailed {
        id: String,
        code: String,
        message: String,
        mock_rule_id: Option<String>,
        mock_rule_name: Option<String>,
    },
    MockRulesFailed {
        code: String,
        message: String,
        rule_id: Option<String>,
    },
    ProxyRulesFailed {
        code: String,
        message: String,
        rule_id: Option<String>,
    },
}

#[derive(Debug, Deserialize)]
struct BridgeHeader {
    name: String,
    value: String,
}

#[derive(Debug, Deserialize)]
struct BridgeBody {
    data_base64: String,
    content_type: Option<String>,
    encoding: Option<String>,
    is_binary: bool,
    is_truncated: bool,
}

#[derive(Debug, Deserialize)]
struct BridgeRequest {
    method: String,
    url: String,
    scheme: String,
    host: String,
    port: Option<u16>,
    path: String,
    query: Option<String>,
    headers: Vec<BridgeHeader>,
    body: Option<BridgeBody>,
}

#[derive(Debug, Deserialize)]
struct BridgeResponse {
    status_code: u16,
    reason: Option<String>,
    headers: Vec<BridgeHeader>,
    body: Option<BridgeBody>,
}

#[derive(Debug, Deserialize)]
struct BridgeTiming {
    request_ms: Option<u64>,
    server_ms: Option<u64>,
    download_ms: Option<u64>,
    total_ms: Option<u64>,
}

fn publish_bridge_event(
    sender: &broadcast::Sender<CaptureEvent>,
    session_id: &str,
    event: BridgeEvent,
) {
    match event {
        BridgeEvent::EngineStarted => {}
        BridgeEvent::FlowCompleted {
            id,
            started_at,
            response_size_bytes,
            request,
            response,
            timing,
            mock_rule_id,
            mock_rule_name: _,
            proxy_rule_ids,
            proxy_rule_changes,
            protocol,
        } => match normalize_captured_flow(
            session_id,
            id,
            started_at,
            response_size_bytes,
            request,
            response,
            timing,
            mock_rule_id.is_some(),
            proxy_rule_ids,
            proxy_rule_changes,
            protocol,
        ) {
            Ok(flow) => {
                let _ = sender.send(CaptureEvent::FlowDetailCompleted(flow));
            }
            Err(message) => {
                let _ = sender.send(CaptureEvent::EngineFailed {
                    code: "mitm_flow_normalize_failed".into(),
                    message,
                    recoverable: true,
                });
            }
        },
        BridgeEvent::WebSocketMessage {
            id,
            flow_id,
            session_id: _,
            sequence,
            from_client,
            opcode,
            timestamp,
            dropped,
            injected,
            body,
        } => {
            let normalized = (|| {
                if id.chars().count() > MAX_PROTOCOL_TEXT
                    || flow_id.chars().count() > MAX_PROTOCOL_TEXT
                    || !matches!(opcode, 1 | 2)
                    || sequence == 0
                    || timestamp.len() > 32
                    || !timestamp.bytes().all(|byte| byte.is_ascii_digit())
                {
                    return Err("invalid WebSocket message metadata".to_string());
                }
                let mut body = decode_body(body)?.ok_or("WebSocket message missing body")?;
                let text = opcode == 1 && std::str::from_utf8(&body.bytes).is_ok();
                body.is_binary = !text;
                body.content_type = Some(
                    if text {
                        "text/plain"
                    } else {
                        "application/octet-stream"
                    }
                    .into(),
                );
                body.encoding = None;
                Ok(CapturedWebSocketMessage {
                    id,
                    flow_id,
                    session_id: Some(session_id.to_string()),
                    sequence,
                    from_client,
                    opcode,
                    timestamp,
                    dropped,
                    injected,
                    body: Some(body),
                })
            })();
            match normalized {
                Ok(message) => {
                    let _ = sender.send(CaptureEvent::WebSocketMessage(message));
                }
                Err(message) => {
                    let _ = sender.send(CaptureEvent::EngineFailed {
                        code: "mitm_websocket_normalize_failed".into(),
                        message,
                        recoverable: true,
                    });
                }
            }
        }
        BridgeEvent::WebSocketClosed {
            flow_id,
            close_code,
            close_reason,
            closed_by_client,
        } => {
            if flow_id.chars().count() <= MAX_PROTOCOL_TEXT {
                let _ = sender.send(CaptureEvent::WebSocketClosed {
                    flow_id,
                    close_code,
                    close_reason: close_reason.map(|value| bounded_text(value)),
                    closed_by_client,
                });
            }
        }
        BridgeEvent::FlowFailed {
            id,
            code,
            message,
            mock_rule_id: _,
            mock_rule_name: _,
        } => {
            let _ = sender.send(CaptureEvent::FlowFailed {
                flow_id: id,
                code,
                message,
            });
        }
        BridgeEvent::MockRulesFailed {
            code,
            message,
            rule_id,
        } => {
            let message = rule_id
                .map(|id| format!("Mock rule {id}: {message}"))
                .unwrap_or(message);
            let _ = sender.send(CaptureEvent::EngineFailed {
                code,
                message,
                recoverable: true,
            });
        }
        BridgeEvent::ProxyRulesFailed {
            code,
            message,
            rule_id,
        } => {
            let message = rule_id
                .map(|id| format!("Proxy rule {id}: {message}"))
                .unwrap_or(message);
            let _ = sender.send(CaptureEvent::EngineFailed {
                code,
                message,
                recoverable: true,
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn normalize_captured_flow(
    session_id: &str,
    id: String,
    started_at: String,
    response_size_bytes: Option<u64>,
    request: BridgeRequest,
    response: BridgeResponse,
    timing: BridgeTiming,
    mocked: bool,
    proxy_rule_ids: Vec<String>,
    proxy_rule_changes: Vec<core_model::ProxyRuleChange>,
    protocol: Option<ProtocolDetails>,
) -> Result<CapturedFlow, String> {
    let request_body = decode_body(request.body)?;
    let response_body = decode_body(response.body)?;
    let total_ms = timing.total_ms;

    let summary = FlowSummary {
        schema_version: SCHEMA_VERSION,
        id,
        session_id: Some(session_id.to_string()),
        source: if mocked {
            FlowSource::Mock
        } else {
            FlowSource::Proxy
        },
        method: request.method.clone(),
        host: request.host.clone(),
        path: request.path.clone(),
        status_code: Some(response.status_code),
        duration_ms: total_ms,
        response_size_bytes,
        started_at,
    };

    Ok(CapturedFlow {
        summary,
        request: CapturedRequest {
            method: request.method,
            url: request.url,
            scheme: request.scheme,
            host: request.host,
            port: request.port,
            path: request.path,
            query: request.query,
            headers: normalize_headers(request.headers),
            body: request_body,
        },
        response: Some(CapturedResponse {
            status_code: response.status_code,
            reason: response.reason,
            headers: normalize_headers(response.headers),
            body: response_body,
        }),
        timing: Timing {
            request_ms: timing.request_ms,
            server_ms: timing.server_ms,
            download_ms: timing.download_ms,
            total_ms,
            ..Timing::default()
        },
        error_code: None,
        error_message: None,
        proxy_rule_ids,
        proxy_rule_changes,
        protocol: protocol.map(normalize_protocol),
    })
}

fn bounded_text(value: String) -> String {
    value.chars().take(MAX_PROTOCOL_TEXT).collect()
}

fn normalize_protocol(mut protocol: ProtocolDetails) -> ProtocolDetails {
    protocol.request_http_version = protocol.request_http_version.map(bounded_text);
    protocol.response_http_version = protocol.response_http_version.map(bounded_text);
    for trailers in [
        &mut protocol.request_trailers,
        &mut protocol.response_trailers,
    ] {
        trailers.truncate(64);
        for header in trailers.iter_mut() {
            header.name = bounded_text(std::mem::take(&mut header.name));
            header.value = bounded_text(std::mem::take(&mut header.value));
            header.sensitive = is_sensitive_header(&header.name);
        }
    }
    for connection in [
        &mut protocol.client_connection,
        &mut protocol.server_connection,
    ]
    .into_iter()
    .flatten()
    {
        connection.id = bounded_text(std::mem::take(&mut connection.id));
        connection.transport = bounded_text(std::mem::take(&mut connection.transport));
        for field in [
            &mut connection.peer_address,
            &mut connection.local_address,
            &mut connection.server_address,
            &mut connection.tls_version,
            &mut connection.cipher,
            &mut connection.alpn,
            &mut connection.sni,
            &mut connection.started_at,
            &mut connection.tls_established_at,
            &mut connection.ended_at,
        ] {
            *field = field.take().map(bounded_text);
        }
        connection.peer_certificates.truncate(16);
        for cert in &mut connection.peer_certificates {
            for field in [
                &mut cert.subject,
                &mut cert.issuer,
                &mut cert.not_before,
                &mut cert.not_after,
                &mut cert.sha256,
            ] {
                *field = bounded_text(std::mem::take(field));
            }
            cert.subject_alternative_names.truncate(64);
            for name in &mut cert.subject_alternative_names {
                *name = bounded_text(std::mem::take(name));
            }
        }
    }
    protocol.websocket_close_reason = protocol.websocket_close_reason.map(bounded_text);
    protocol
}

fn normalize_headers(headers: Vec<BridgeHeader>) -> Vec<HeaderValue> {
    headers
        .into_iter()
        .map(|header| HeaderValue {
            sensitive: is_sensitive_header(&header.name),
            name: header.name,
            value: header.value,
        })
        .collect()
}

fn is_sensitive_header(name: &str) -> bool {
    matches!(
        name.to_ascii_lowercase().as_str(),
        "authorization"
            | "proxy-authorization"
            | "cookie"
            | "set-cookie"
            | "x-api-key"
            | "api-key"
            | "x-auth-token"
    )
}

fn decode_body(body: Option<BridgeBody>) -> Result<Option<CapturedBody>, String> {
    let Some(body) = body else {
        return Ok(None);
    };
    if body.data_base64.len() > (MAX_BODY_BYTES + 2) / 3 * 4 + 4 {
        return Err("mitm bridge body exceeds capture limit".into());
    }
    let bytes = BASE64
        .decode(body.data_base64.as_bytes())
        .map_err(|error| format!("invalid base64 body from mitm bridge: {error}"))?;
    if bytes.len() > MAX_BODY_BYTES {
        return Err("mitm bridge body exceeds capture limit".into());
    }

    Ok(Some(CapturedBody {
        bytes,
        content_type: body.content_type,
        encoding: body.encoding,
        is_binary: body.is_binary,
        is_truncated: body.is_truncated,
    }))
}

fn now_epoch_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(unix)]
    fn startup_requires_bridge_readiness_and_live_child() {
        use std::os::unix::fs::PermissionsExt;
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-start-check-{}-{}", std::process::id(), now_epoch_millis()));
            fs::create_dir_all(&root).unwrap();
            let addon = root.join("addon.py");
            fs::write(&addon, "").unwrap();
            let executable = root.join("capture-stub");
            let config = CaptureConfig { session_id: "check".into(), listen_host: "127.0.0.1".into(), listen_port: 8185,
                mode: core_model::CaptureMode { schema_version: SCHEMA_VERSION, kind: CaptureModeKind::Socks5 } };
            fs::write(&executable, "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo stub; exit 0; fi\nexit 7\n").unwrap();
            fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
            let engine = MitmDumpEngine::new(&addon, root.join("conf")).with_executable(&executable);
            assert_eq!(engine.start(config.clone()).await.unwrap_err().code, "mitmdump_listener_start_failed");
            fs::write(&executable, "#!/bin/sh\nif [ \"$1\" = '--version' ]; then echo stub; exit 0; fi\nprintf 'MAS_EVENT {\"type\":\"engine_started\"}\\n'\nexec sleep 30\n").unwrap();
            let handle = engine.start(config).await.unwrap();
            assert!(engine.is_running(&handle).await);
            engine.stop(handle.clone()).await.unwrap();
            assert!(!engine.is_running(&handle).await);
            fs::remove_dir_all(root).unwrap();
        });
    }

    #[test]
    fn mitmdump_mode_specs() {
        assert_eq!(
            mode_spec(&CaptureModeKind::ReverseProxy {
                url: "https://example.com".into()
            }),
            "reverse:https://example.com"
        );
        assert_eq!(
            mode_spec(&CaptureModeKind::UpstreamProxy {
                url: "http://127.0.0.1:8080".into()
            }),
            "upstream:http://127.0.0.1:8080"
        );
        assert_eq!(mode_spec(&CaptureModeKind::Socks5), "socks5");
        assert_eq!(mode_spec(&CaptureModeKind::DnsProxy), "dns");
        assert_eq!(mode_spec(&CaptureModeKind::RegularProxy), "regular");
        assert_eq!(mode_spec(&CaptureModeKind::LocalAll), "local");
        assert_eq!(
            mode_spec(&CaptureModeKind::LocalProcess { pid: 42 }),
            "local:42"
        );
    }
}

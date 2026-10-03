use app_core::CoreService;
use core_model::AppError;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{future::Future, io, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    time::timeout,
};

pub(crate) const MAX_REQUEST: usize = 128 * 1024;
pub(crate) const MAX_RESPONSE: usize = 16 * 1024 * 1024;
pub(crate) const MAX_CONNECTIONS: usize = 16;
const IO_TIMEOUT: Duration = Duration::from_secs(10);
const INVOKE_TIMEOUT: Duration = Duration::from_secs(5 * 60);

pub(crate) const COMMANDS: &[&str] = &[
    "capture_platform_info",
    "connect_capture_target",
    "current_connection",
    "delete_network_profile",
    "delete_proxy_rule",
    "disconnect_device",
    "export_interchange",
    "export_workspace",
    "get_flow_detail",
    "health",
    "list_desktop_processes",
    "list_devices",
    "list_lan_interfaces",
    "list_flows",
    "list_network_profiles",
    "list_proxy_rules",
    "list_sessions",
    "preview_proxy_rule",
    "search_traffic",
    "upsert_network_profile",
    "upsert_proxy_rule",
];

type DispatchFuture = Pin<Box<dyn Future<Output = Result<Value, AppError>> + Send>>;
pub(crate) type Dispatch = Arc<dyn Fn(String, Value) -> DispatchFuture + Send + Sync>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    command: String,
    args: Value,
}

pub(crate) fn make_dispatch(core: CoreService) -> Dispatch {
    Arc::new(move |command, args| {
        let core = core.clone();
        Box::pin(async move { core.invoke(&command, args).await })
    })
}

pub(crate) async fn handle_connection<S>(mut stream: S, dispatch: Dispatch)
where
    S: AsyncRead + AsyncWrite + Unpin + Send,
{
    let read = timeout(IO_TIMEOUT, async {
        let mut bytes = Vec::new();
        BufReader::new(&mut stream)
            .take((MAX_REQUEST + 1) as u64)
            .read_until(b'\n', &mut bytes)
            .await?;
        Ok::<_, io::Error>(bytes)
    })
    .await;
    let bytes = match read {
        Ok(Ok(bytes)) if bytes.len() <= MAX_REQUEST => bytes,
        Ok(Ok(_)) => {
            write_json(
                stream,
                &error(
                    "control_request_too_large",
                    "Control request exceeds 128 KiB.",
                ),
            )
            .await;
            return;
        }
        _ => return,
    };
    let request = match serde_json::from_slice::<Request>(&bytes) {
        Ok(request) if request.args.is_object() => request,
        _ => {
            write_json(
                stream,
                &error(
                    "control_request_invalid",
                    "Control request must contain a command and object args.",
                ),
            )
            .await;
            return;
        }
    };
    if !COMMANDS.contains(&request.command.as_str()) {
        write_json(
            stream,
            &error(
                "control_command_forbidden",
                "Command is unavailable through the local control service.",
            ),
        )
        .await;
        return;
    }
    match timeout(INVOKE_TIMEOUT, dispatch(request.command, request.args)).await {
        Ok(Ok(value)) => {
            let bytes = serde_json::to_vec(&value).unwrap_or_else(|_| b"null".to_vec());
            if bytes.len() > MAX_RESPONSE {
                write_json(
                    stream,
                    &error(
                        "control_response_too_large",
                        "Control response exceeds 16 MiB.",
                    ),
                )
                .await;
            } else {
                let _ = write_bytes(stream, &bytes).await;
            }
        }
        Ok(Err(app_error)) => write_json(stream, &json!({"error": app_error})).await,
        Err(_) => {
            write_json(
                stream,
                &error(
                    "control_timeout",
                    "Control command exceeded its time limit.",
                ),
            )
            .await;
        }
    }
}

fn error(code: &str, message: &str) -> Value {
    json!({"error":{"code":code,"message":message,"recoverable":false}})
}

async fn write_json<S>(stream: S, value: &Value)
where
    S: AsyncWrite + Unpin,
{
    let bytes = serde_json::to_vec(value).unwrap_or_else(|_| b"{}".to_vec());
    let bytes = if bytes.len() <= MAX_RESPONSE {
        bytes
    } else {
        serde_json::to_vec(&error(
            "control_response_too_large",
            "Control response exceeds 16 MiB.",
        ))
        .unwrap()
    };
    let _ = write_bytes(stream, &bytes).await;
}

async fn write_bytes<S>(mut stream: S, bytes: &[u8]) -> io::Result<()>
where
    S: AsyncWrite + Unpin,
{
    timeout(IO_TIMEOUT, async {
        stream.write_all(bytes).await?;
        stream.write_all(b"\n").await?;
        stream.shutdown().await
    })
    .await
    .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "control response write timed out"))?
}

use std::{
    env,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::{Ipv4Addr, SocketAddrV4},
    path::PathBuf,
    process::Command,
    sync::Arc,
};

use app_core::CoreService;
use axum::{
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{any, get, post},
    Json, Router,
};
use core_model::AppError;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tower_http::services::{ServeDir, ServeFile};

#[derive(Clone)]
struct ServerState {
    core: CoreService,
    token: Arc<str>,
    authority: Arc<str>,
}

#[derive(Deserialize)]
struct InvokeRequest {
    command: String,
    #[serde(default)]
    args: Value,
}

#[derive(Serialize)]
struct ErrorBody {
    error: AppError,
}

fn error(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(ErrorBody {
            error: AppError::new(code, message, false),
        }),
    )
        .into_response()
}

fn secure_headers(response: &mut Response) {
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(header::X_FRAME_OPTIONS, HeaderValue::from_static("DENY"));
    headers.insert(header::CONTENT_SECURITY_POLICY, HeaderValue::from_static("default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; base-uri 'none'; frame-ancestors 'none'"));
}

fn validate_host_origin(headers: &HeaderMap, authority: &str) -> Result<(), Response> {
    let host = headers.get(header::HOST).and_then(|v| v.to_str().ok());
    if host != Some(authority) {
        return Err(error(
            StatusCode::FORBIDDEN,
            "invalid_host",
            "This service accepts only its loopback address.",
        ));
    }
    if let Some(origin) = headers.get(header::ORIGIN) {
        let expected = format!("http://{authority}");
        if origin.to_str().ok() != Some(expected.as_str()) {
            return Err(error(
                StatusCode::FORBIDDEN,
                "invalid_origin",
                "Cross-origin requests are not allowed.",
            ));
        }
    }
    Ok(())
}

async fn guard(State(state): State<ServerState>, request: Request<Body>, next: Next) -> Response {
    if let Err(mut response) = validate_host_origin(request.headers(), &state.authority) {
        secure_headers(&mut response);
        return response;
    }
    let mut response = next.run(request).await;
    secure_headers(&mut response);
    response
}

async fn session(State(state): State<ServerState>) -> Json<Value> {
    Json(json!({ "token": state.token.as_ref() }))
}

async fn invoke(
    State(state): State<ServerState>,
    headers: HeaderMap,
    Json(input): Json<InvokeRequest>,
) -> Response {
    if !valid_token(&headers, &state.token) {
        return error(
            StatusCode::UNAUTHORIZED,
            "invalid_token",
            "Reload the page to establish a new session.",
        );
    }
    match state.core.invoke(&input.command, input.args).await {
        Ok(value) => Json(value).into_response(),
        Err(error_value) => {
            let status = match error_value.code.as_str() {
                "unknown_command" => StatusCode::NOT_FOUND,
                "invalid_arguments" => StatusCode::BAD_REQUEST,
                _ => StatusCode::UNPROCESSABLE_ENTITY,
            };
            (status, Json(ErrorBody { error: error_value })).into_response()
        }
    }
}

fn valid_token(headers: &HeaderMap, token: &str) -> bool {
    headers.get("x-mas-token").and_then(|v| v.to_str().ok()) == Some(token)
}

fn random_token() -> Result<String, String> {
    let mut bytes = [0_u8; 32];
    File::open("/dev/urandom")
        .and_then(|mut file| file.read_exact(&mut bytes))
        .map_err(|error| format!("Cannot create a local session token: {error}"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

fn arguments() -> Result<(u16, bool, PathBuf), String> {
    let mut port = 8180;
    let mut open_browser = true;
    let mut data_dir = env::var_os("HOME")
        .map(PathBuf::from)
        .ok_or("HOME is required")?
        .join("Library/Application Support/dev.mobileapistudio.desktop");
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--port" => {
                port = args
                    .next()
                    .ok_or("--port requires a value")?
                    .parse()
                    .map_err(|_| "Invalid port")?
            }
            "--no-open" => open_browser = false,
            "--data-dir" => {
                data_dir = PathBuf::from(args.next().ok_or("--data-dir requires a value")?)
            }
            "--help" | "-h" => {
                println!(
                    "Usage: mobile-api-studio-server [--port PORT] [--no-open] [--data-dir PATH]"
                );
                std::process::exit(0);
            }
            _ => return Err(format!("Unknown argument: {arg}")),
        }
    }
    if port == 0 {
        return Err("Port must be between 1 and 65535".into());
    }
    Ok((port, open_browser, data_dir))
}

struct DataLock {
    path: PathBuf,
}

impl Drop for DataLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

fn lock_data_dir(data_dir: &PathBuf) -> Result<DataLock, String> {
    fs::create_dir_all(data_dir).map_err(|error| error.to_string())?;
    for process_name in ["mobile-api-studio", "Mobile API Studio"] {
        match Command::new("pgrep").arg("-x").arg(process_name).output() {
            Ok(result) if result.status.success() => {
                return Err(
                    "Close the historical Mobile API Studio app before starting the local service."
                        .into(),
                );
            }
            Ok(result) if result.status.code() == Some(1) => {}
            Ok(result) => {
                return Err(format!(
                    "Cannot check whether the old app is running (pgrep status {}).",
                    result.status
                ));
            }
            Err(error) => {
                return Err(format!(
                    "Cannot check whether the old app is running: {error}"
                ));
            }
        }
    }
    let database = data_dir.join("app.db");
    if database.exists() {
        let output = Command::new("lsof")
            .arg("-t")
            .arg(&database)
            .output()
            .map_err(|error| {
                format!(
                    "Cannot check whether {} is in use: {error}",
                    database.display()
                )
            })?;
        if output.status.success() && !output.stdout.is_empty() {
            return Err(format!(
                "{} is in use. Close the other process before starting the local service.",
                database.display()
            ));
        }
        if !output.status.success() && output.status.code() != Some(1) {
            return Err(format!(
                "Cannot check whether {} is in use (lsof status {}).",
                database.display(),
                output.status
            ));
        }
    }
    let path = data_dir.join("local-server.lock");
    if let Ok(pid) = fs::read_to_string(&path) {
        if let Ok(pid) = pid.trim().parse::<u32>() {
            if Command::new("kill")
                .arg("-0")
                .arg(pid.to_string())
                .output()
                .is_ok_and(|result| result.status.success())
            {
                return Err(format!(
                    "Another local service is using {} (process {pid}).",
                    data_dir.display()
                ));
            }
        }
        fs::remove_file(&path)
            .map_err(|error| format!("Cannot clear stale lock {}: {error}", path.display()))?;
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|error| format!("Cannot lock {}: {error}", data_dir.display()))?;
    write!(file, "{}", std::process::id()).map_err(|error| error.to_string())?;
    Ok(DataLock { path })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (port, open_browser, data_dir) = arguments()?;
    let listener =
        tokio::net::TcpListener::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)).await?;
    let _data_lock = lock_data_dir(&data_dir)?;
    let core = CoreService::start(data_dir)?;
    let state = ServerState {
        core: core.clone(),
        token: random_token()?.into(),
        authority: format!("127.0.0.1:{port}").into(),
    };
    let dist = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../desktop/dist");
    if !dist.join("index.html").is_file() {
        return Err(format!(
            "UI missing at {}. Run npm run build in apps/desktop first.",
            dist.display()
        )
        .into());
    }
    let app = Router::new()
        .route("/api/session", get(session))
        .route("/api/invoke", post(invoke))
        .route(
            "/api/{*path}",
            any(|| async {
                error(
                    StatusCode::NOT_FOUND,
                    "unknown_api_route",
                    "Unknown local API route.",
                )
            }),
        )
        .route("/healthz", get(|| async { "ok" }))
        .fallback_service(ServeDir::new(&dist));
    let app = [
        "/",
        "/connect",
        "/traffic",
        "/replay",
        "/mocks",
        "/compare",
        "/ai",
        "/sdk",
        "/workspace",
        "/settings",
    ]
    .into_iter()
    .fold(app, |router, path| {
        router.route_service(path, ServeFile::new(dist.join("index.html")))
    })
    .layer(DefaultBodyLimit::max(256 * 1024 * 1024))
    .layer(middleware::from_fn_with_state(state.clone(), guard))
    .with_state(state);
    let url = format!("http://127.0.0.1:{port}");
    println!("Mobile API Studio: {url}");
    if open_browser {
        let _ = Command::new("open").arg(&url).spawn();
    }
    axum::serve(listener, app)
        .with_graceful_shutdown(async {
            #[cfg(unix)]
            {
                if let Ok(mut terminate) =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                {
                    tokio::select! { _ = tokio::signal::ctrl_c() => {}, _ = terminate.recv() => {} }
                } else {
                    let _ = tokio::signal::ctrl_c().await;
                }
            }
            #[cfg(not(unix))]
            {
                let _ = tokio::signal::ctrl_c().await;
            }
        })
        .await?;
    if let Err(error) = core.shutdown().await {
        return Err(format!("Shutdown recovery failed: {}", error.message).into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_expected_loopback_authority_and_origin_are_accepted() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("127.0.0.1:8180"));
        assert!(validate_host_origin(&headers, "127.0.0.1:8180").is_ok());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("http://127.0.0.1:8180"),
        );
        assert!(validate_host_origin(&headers, "127.0.0.1:8180").is_ok());
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(validate_host_origin(&headers, "127.0.0.1:8180").is_err());
        headers.remove(header::ORIGIN);
        headers.insert(header::HOST, HeaderValue::from_static("evil.example"));
        assert!(validate_host_origin(&headers, "127.0.0.1:8180").is_err());
    }

    #[test]
    fn invoke_requires_header_token() {
        let mut headers = HeaderMap::new();
        assert!(!valid_token(&headers, "secret"));
        headers.insert("x-mas-token", HeaderValue::from_static("wrong"));
        assert!(!valid_token(&headers, "secret"));
        headers.insert("x-mas-token", HeaderValue::from_static("secret"));
        assert!(valid_token(&headers, "secret"));
    }

    #[test]
    fn tokens_are_random_and_not_url_material() {
        let first = random_token().unwrap();
        let second = random_token().unwrap();
        assert_eq!(first.len(), 64);
        assert_ne!(first, second);
        assert!(first.chars().all(|character| character.is_ascii_hexdigit()));
    }

    #[test]
    fn security_headers_apply_to_rejected_requests() {
        let mut response = error(StatusCode::FORBIDDEN, "invalid_host", "Wrong host");
        secure_headers(&mut response);
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
        assert_eq!(response.headers()[header::X_FRAME_OPTIONS], "DENY");
        assert!(response.headers()[header::CONTENT_SECURITY_POLICY]
            .to_str()
            .unwrap()
            .contains("frame-ancestors 'none'"));
    }
}

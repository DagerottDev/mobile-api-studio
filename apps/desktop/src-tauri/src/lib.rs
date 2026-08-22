mod inspect;
mod replay_commands;
mod settings_commands;
mod workspace_commands;

use capture_core::{CaptureConfig, CaptureEngine, CaptureHandle};
use capture_mitm::MitmDumpEngine;
use core_model::{
    AppError, CaptureSession, ConnectionDiagnostic, Device, DevicePlatform, FlowSummary,
    SessionStatus, SCHEMA_VERSION,
};
use device_android::AndroidDeviceProvider;
use device_ios::IosDeviceProvider;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use storage::{BodyStore, Database};
use tauri::{Manager, State};
use tokio::{
    sync::Mutex,
    time::{sleep, Duration},
};

const DEFAULT_CAPTURE_PORT: u16 = 8181;
const ANDROID_HOST_ALIAS: &str = "10.0.2.2";

struct AppState {
    database: Database,
    body_store: BodyStore,
    capture_engine: Arc<MitmDumpEngine>,
    active_connection: Mutex<Option<ActiveConnection>>,
    rollback_path: PathBuf,
}

#[derive(Debug, Clone)]
struct ActiveConnection {
    handle: CaptureHandle,
    device_id: String,
    strategy: String,
    previous_android_proxy: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceDiscoveryPayload {
    devices: Vec<Device>,
    diagnostics: Vec<ConnectionDiagnostic>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectionSnapshot {
    connected: bool,
    session_id: Option<String>,
    device_id: Option<String>,
    strategy: Option<String>,
    proxy_host: Option<String>,
    proxy_port: Option<u16>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConnectDeviceResult {
    connection: ConnectionSnapshot,
    diagnostics: Vec<ConnectionDiagnostic>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct RollbackJournal {
    schema_version: u16,
    device_id: String,
    platform: DevicePlatform,
    session_id: String,
    previous_android_proxy: Option<String>,
    ios_ca_installed: bool,
}

#[tauri::command]
fn health(state: State<'_, AppState>) -> String {
    format!("Rust core ready · {}", state.database.path().display())
}

#[tauri::command]
fn list_devices() -> DeviceDiscoveryPayload {
    let mut devices = Vec::new();
    let mut diagnostics = Vec::new();

    let ios_provider = IosDeviceProvider;
    if ios_provider.is_available() {
        match ios_provider.list_devices() {
            Ok(mut discovered) => devices.append(&mut discovered),
            Err(error) => diagnostics.push(ConnectionDiagnostic {
                code: error.code,
                title: "iOS Simulator discovery failed".into(),
                message: error.message,
                recoverable: error.recoverable,
                suggested_action: Some(
                    "Open Xcode and ensure Command Line Tools and Simulator runtimes are installed."
                        .into(),
                ),
            }),
        }
    } else {
        diagnostics.push(ConnectionDiagnostic {
            code: "ios_tool_unavailable".into(),
            title: "iOS tools unavailable".into(),
            message: "xcrun/simctl could not be found on this machine.".into(),
            recoverable: true,
            suggested_action: Some(
                "Install Xcode and select its Command Line Tools before connecting an iOS Simulator."
                    .into(),
            ),
        });
    }

    let android_provider = AndroidDeviceProvider;
    if android_provider.is_available() {
        match android_provider.list_devices() {
            Ok(mut discovered) => devices.append(&mut discovered),
            Err(error) => diagnostics.push(ConnectionDiagnostic {
                code: error.code,
                title: "Android Emulator discovery failed".into(),
                message: error.message,
                recoverable: error.recoverable,
                suggested_action: Some(
                    "Start ADB from Android Platform Tools and ensure the emulator is visible in `adb devices`."
                        .into(),
                ),
            }),
        }
    } else {
        diagnostics.push(ConnectionDiagnostic {
            code: "android_tool_unavailable".into(),
            title: "Android tools unavailable".into(),
            message: "adb could not be found on this machine.".into(),
            recoverable: true,
            suggested_action: Some(
                "Install Android Platform Tools or make the Android SDK platform-tools directory available on PATH."
                    .into(),
            ),
        });
    }

    DeviceDiscoveryPayload {
        devices,
        diagnostics,
    }
}

#[tauri::command]
fn list_flows(state: State<'_, AppState>) -> Result<Vec<FlowSummary>, AppError> {
    state
        .database
        .list_flows(5_000)
        .map_err(|error| AppError::storage(error.to_string()))
}

#[tauri::command]
fn list_sessions(state: State<'_, AppState>) -> Result<Vec<CaptureSession>, AppError> {
    state
        .database
        .list_sessions(100)
        .map_err(|error| AppError::storage(error.to_string()))
}

#[tauri::command]
async fn current_connection(state: State<'_, AppState>) -> Result<ConnectionSnapshot, AppError> {
    let active = state.active_connection.lock().await;
    Ok(active
        .as_ref()
        .map(connection_snapshot)
        .unwrap_or_else(disconnected_snapshot))
}

#[tauri::command]
async fn connect_device(
    device_id: String,
    session_name: Option<String>,
    state: State<'_, AppState>,
) -> Result<ConnectDeviceResult, AppError> {
    if state.active_connection.lock().await.is_some() {
        return Err(AppError::new(
            "connection_already_active",
            "Disconnect the active capture session before connecting another runtime.",
            true,
        ));
    }

    if state.rollback_path.exists() {
        return Err(AppError::new(
            "pending_connection_rollback",
            "A previous connection left device changes pending. Recover them before starting another capture.",
            true,
        ));
    }

    let timestamp = now_epoch_millis()?;
    let session_id = format!("session-{timestamp}");
    let is_android = device_id.starts_with("android:");
    let is_ios = device_id.starts_with("ios:");

    if !is_android && !is_ios {
        return Err(AppError::new(
            "unsupported_device",
            "Only discovered iOS Simulators and Android Emulators can be connected.",
            true,
        ));
    }

    let listen_host = if is_android { "0.0.0.0" } else { "127.0.0.1" };
    let strategy = if is_android {
        "android_adb_global_proxy"
    } else {
        "ios_manual_proxy"
    };

    let handle = state
        .capture_engine
        .start(CaptureConfig {
            session_id: session_id.clone(),
            listen_host: listen_host.into(),
            listen_port: DEFAULT_CAPTURE_PORT,
        })
        .await
        .map_err(capture_error_to_app_error)?;

    let mut diagnostics = Vec::new();
    let mut previous_android_proxy = None;

    if is_android {
        let provider = AndroidDeviceProvider;
        previous_android_proxy = provider
            .get_http_proxy(&device_id)
            .map_err(device_error_to_app_error)?;

        let journal = RollbackJournal {
            schema_version: SCHEMA_VERSION,
            device_id: device_id.clone(),
            platform: DevicePlatform::Android,
            session_id: session_id.clone(),
            previous_android_proxy: previous_android_proxy.clone(),
            ios_ca_installed: false,
        };
        save_rollback_journal(&state.rollback_path, &journal)?;

        if let Err(error) = provider.set_http_proxy(&device_id, ANDROID_HOST_ALIAS, DEFAULT_CAPTURE_PORT) {
            let _ = state.capture_engine.stop(handle.clone()).await;
            let _ = restore_android_proxy(&provider, &device_id, previous_android_proxy.as_deref());
            let _ = clear_rollback_journal(&state.rollback_path);
            return Err(device_error_to_app_error(error));
        }

        diagnostics.push(ConnectionDiagnostic {
            code: "android_ca_trust_guided".into(),
            title: "HTTPS trust may require app configuration".into(),
            message: "The emulator is routed through Mobile API Studio. HTTPS interception also requires the app to trust the mitmproxy CA.".into(),
            recoverable: true,
            suggested_action: Some(
                "For development builds, trust user-added CAs with Android network security configuration, or install the CA manually from mitm.it. Certificate-pinned apps require an app-side debug path rather than proxy bypassing."
                    .into(),
            ),
        });
    } else {
        let certificate = wait_for_certificate(&state.capture_engine.certificate_path()).await?;
        let journal = RollbackJournal {
            schema_version: SCHEMA_VERSION,
            device_id: device_id.clone(),
            platform: DevicePlatform::Ios,
            session_id: session_id.clone(),
            previous_android_proxy: None,
            ios_ca_installed: true,
        };
        save_rollback_journal(&state.rollback_path, &journal)?;

        if let Err(error) = IosDeviceProvider.install_root_ca(&device_id, &certificate) {
            let _ = state.capture_engine.stop(handle.clone()).await;
            let _ = clear_rollback_journal(&state.rollback_path);
            return Err(device_error_to_app_error(error));
        }

        diagnostics.push(ConnectionDiagnostic {
            code: "ios_proxy_manual_configuration".into(),
            title: "Configure the Simulator proxy manually".into(),
            message: format!(
                "The capture engine is listening on 127.0.0.1:{DEFAULT_CAPTURE_PORT}, but Mobile API Studio does not change macOS/iOS proxy settings automatically."
            ),
            recoverable: true,
            suggested_action: Some(
                "Route the Simulator through the local proxy for this session. The app intentionally avoids changing system-wide macOS proxy settings automatically."
                    .into(),
            ),
        });
        diagnostics.push(ConnectionDiagnostic {
            code: "ios_ca_full_trust_required".into(),
            title: "Enable full trust for the capture CA".into(),
            message: "The CA was added to the Simulator root store; recent iOS versions can still require enabling full trust in Certificate Trust Settings.".into(),
            recoverable: true,
            suggested_action: Some(
                "In the Simulator open Settings → General → About → Certificate Trust Settings and enable full trust for the mitmproxy certificate."
                    .into(),
            ),
        });
    }

    let session = CaptureSession {
        schema_version: SCHEMA_VERSION,
        id: session_id.clone(),
        name: sanitize_session_name(session_name.unwrap_or_else(|| format!("Capture {timestamp}"))),
        status: SessionStatus::Active,
        started_at: timestamp,
        ended_at: None,
        device_id: Some(device_id.clone()),
        app_id: None,
        connection_strategy: Some(strategy.into()),
        capture_engine: Some("mitmdump".into()),
        notes: None,
    };
    state
        .database
        .create_session(&session)
        .map_err(|error| AppError::storage(error.to_string()))?;

    let active = ActiveConnection {
        handle,
        device_id,
        strategy: strategy.into(),
        previous_android_proxy,
    };
    let snapshot = connection_snapshot(&active);
    *state.active_connection.lock().await = Some(active);

    Ok(ConnectDeviceResult {
        connection: snapshot,
        diagnostics,
    })
}

#[tauri::command]
async fn disconnect_device(state: State<'_, AppState>) -> Result<ConnectionSnapshot, AppError> {
    let active = state.active_connection.lock().await.take();
    let Some(active) = active else {
        return Ok(disconnected_snapshot());
    };

    if active.device_id.starts_with("android:") {
        restore_android_proxy(
            &AndroidDeviceProvider,
            &active.device_id,
            active.previous_android_proxy.as_deref(),
        )?;
    }

    state
        .capture_engine
        .stop(active.handle.clone())
        .await
        .map_err(capture_error_to_app_error)?;

    let ended_at = now_epoch_millis()?;
    state
        .database
        .complete_session(&active.handle.session_id, &ended_at)
        .map_err(|error| AppError::storage(error.to_string()))?;
    clear_rollback_journal(&state.rollback_path)?;

    Ok(disconnected_snapshot())
}

#[tauri::command]
fn pending_rollback(state: State<'_, AppState>) -> Result<Option<RollbackJournal>, AppError> {
    load_rollback_journal(&state.rollback_path)
}

#[tauri::command]
fn recover_pending_rollback(state: State<'_, AppState>) -> Result<Vec<ConnectionDiagnostic>, AppError> {
    let Some(journal) = load_rollback_journal(&state.rollback_path)? else {
        return Ok(Vec::new());
    };

    let mut diagnostics = Vec::new();
    match journal.platform {
        DevicePlatform::Android => {
            restore_android_proxy(
                &AndroidDeviceProvider,
                &journal.device_id,
                journal.previous_android_proxy.as_deref(),
            )?;
            diagnostics.push(ConnectionDiagnostic {
                code: "android_proxy_restored".into(),
                title: "Android proxy restored".into(),
                message: "The emulator proxy setting from the interrupted capture session was restored.".into(),
                recoverable: true,
                suggested_action: None,
            });
        }
        DevicePlatform::Ios => {
            diagnostics.push(ConnectionDiagnostic {
                code: "ios_ca_left_installed".into(),
                title: "Simulator CA remains installed".into(),
                message: "Mobile API Studio does not reset the Simulator keychain automatically because that could delete unrelated developer credentials.".into(),
                recoverable: true,
                suggested_action: Some(
                    "You may leave the locally generated CA installed, disable its full-trust toggle, or remove it manually if desired."
                        .into(),
                ),
            });
        }
    }

    clear_rollback_journal(&state.rollback_path)?;
    Ok(diagnostics)
}

fn connection_snapshot(active: &ActiveConnection) -> ConnectionSnapshot {
    ConnectionSnapshot {
        connected: true,
        session_id: Some(active.handle.session_id.clone()),
        device_id: Some(active.device_id.clone()),
        strategy: Some(active.strategy.clone()),
        proxy_host: Some(if active.device_id.starts_with("android:") {
            ANDROID_HOST_ALIAS.into()
        } else {
            "127.0.0.1".into()
        }),
        proxy_port: Some(active.handle.listen_port),
    }
}

fn disconnected_snapshot() -> ConnectionSnapshot {
    ConnectionSnapshot {
        connected: false,
        session_id: None,
        device_id: None,
        strategy: None,
        proxy_host: None,
        proxy_port: None,
    }
}

fn restore_android_proxy(
    provider: &AndroidDeviceProvider,
    device_id: &str,
    previous: Option<&str>,
) -> Result<(), AppError> {
    match previous {
        Some(proxy) => {
            let (host, port) = proxy.rsplit_once(':').ok_or_else(|| {
                AppError::new(
                    "android_proxy_restore_invalid",
                    format!("Unable to parse previous Android proxy value: {proxy}"),
                    true,
                )
            })?;
            let port = port.parse::<u16>().map_err(|error| {
                AppError::new("android_proxy_restore_invalid", error.to_string(), true)
            })?;
            provider
                .set_http_proxy(device_id, host, port)
                .map_err(device_error_to_app_error)
        }
        None => provider
            .clear_http_proxy(device_id)
            .map_err(device_error_to_app_error),
    }
}

async fn wait_for_certificate(path: &Path) -> Result<PathBuf, AppError> {
    for _ in 0..40 {
        if path.is_file() {
            return Ok(path.to_path_buf());
        }
        sleep(Duration::from_millis(50)).await;
    }

    Err(AppError::new(
        "capture_ca_not_ready",
        format!("Capture CA was not created at {}", path.display()),
        true,
    ))
}

fn save_rollback_journal(path: &Path, journal: &RollbackJournal) -> Result<(), AppError> {
    let bytes = serde_json::to_vec_pretty(journal)
        .map_err(|error| AppError::new("rollback_serialize_failed", error.to_string(), true))?;
    fs::write(path, bytes)
        .map_err(|error| AppError::new("rollback_write_failed", error.to_string(), true))
}

fn load_rollback_journal(path: &Path) -> Result<Option<RollbackJournal>, AppError> {
    if !path.is_file() {
        return Ok(None);
    }
    let bytes = fs::read(path)
        .map_err(|error| AppError::new("rollback_read_failed", error.to_string(), true))?;
    let journal = serde_json::from_slice(&bytes)
        .map_err(|error| AppError::new("rollback_parse_failed", error.to_string(), true))?;
    Ok(Some(journal))
}

fn clear_rollback_journal(path: &Path) -> Result<(), AppError> {
    if path.exists() {
        fs::remove_file(path)
            .map_err(|error| AppError::new("rollback_clear_failed", error.to_string(), true))?;
    }
    Ok(())
}

fn spawn_capture_ingestion(database: Database, body_store: BodyStore, engine: Arc<MitmDumpEngine>) {
    let mut receiver = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match receiver.recv().await {
                Ok(event) => {
                    let _ = inspect::ingest_capture_event(&database, &body_store, event);
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
            }
        }
    });
}

fn initialize_state(app_data_dir: PathBuf, addon_path: PathBuf) -> Result<AppState, String> {
    fs::create_dir_all(&app_data_dir).map_err(|error| error.to_string())?;
    let database = Database::open(app_data_dir.join("app.db")).map_err(|error| error.to_string())?;
    let body_store = BodyStore::new(app_data_dir.join("bodies")).map_err(|error| error.to_string())?;
    let capture_engine = Arc::new(MitmDumpEngine::new(
        addon_path,
        app_data_dir.join("mitmproxy"),
    ));

    Ok(AppState {
        database,
        body_store,
        capture_engine,
        active_connection: Mutex::new(None),
        rollback_path: app_data_dir.join("connection-rollback.json"),
    })
}

fn resolve_addon_path(app: &tauri::App) -> Result<PathBuf, String> {
    let development_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../sidecars/mitm-addon/mas_bridge.py");
    if development_path.is_file() {
        return Ok(development_path);
    }

    let resource_dir = app.path().resource_dir().map_err(|error| error.to_string())?;
    Ok(resource_dir.join("sidecars/mitm-addon/mas_bridge.py"))
}

fn capture_error_to_app_error(error: capture_core::CaptureError) -> AppError {
    AppError::new(error.code, error.message, error.recoverable)
}

fn device_error_to_app_error(error: impl IntoDeviceError) -> AppError {
    let error = error.into_parts();
    AppError::new(error.0, error.1, error.2)
}

trait IntoDeviceError {
    fn into_parts(self) -> (String, String, bool);
}

impl IntoDeviceError for device_android::DeviceError {
    fn into_parts(self) -> (String, String, bool) {
        (self.code, self.message, self.recoverable)
    }
}

impl IntoDeviceError for device_ios::DeviceError {
    fn into_parts(self) -> (String, String, bool) {
        (self.code, self.message, self.recoverable)
    }
}

fn now_epoch_millis() -> Result<String, AppError> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| AppError::initialization(error.to_string()))?;
    Ok(duration.as_millis().to_string())
}

fn sanitize_session_name(name: String) -> String {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        "Untitled Session".to_string()
    } else {
        trimmed.chars().take(120).collect()
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let addon_path = resolve_addon_path(app).map_err(std::io::Error::other)?;
            let state = initialize_state(app_data_dir, addon_path).map_err(std::io::Error::other)?;
            spawn_capture_ingestion(
                state.database.clone(),
                state.body_store.clone(),
                state.capture_engine.clone(),
            );
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            list_devices,
            list_flows,
            list_sessions,
            current_connection,
            connect_device,
            disconnect_device,
            pending_rollback,
            recover_pending_rollback,
            inspect::get_flow_detail,
            inspect::read_body,
            inspect::export_curl,
            replay_commands::create_replay_draft,
            replay_commands::send_replay,
            workspace_commands::update_session_metadata,
            workspace_commands::archive_session,
            workspace_commands::delete_session,
            workspace_commands::search_traffic,
            workspace_commands::list_collections,
            workspace_commands::upsert_collection,
            workspace_commands::delete_collection,
            workspace_commands::list_saved_requests,
            workspace_commands::save_flow_to_collection,
            workspace_commands::delete_saved_request,
            workspace_commands::list_environments,
            workspace_commands::upsert_environment,
            workspace_commands::set_active_environment,
            workspace_commands::delete_environment,
            workspace_commands::environment_snapshot,
            workspace_commands::upsert_environment_variable,
            workspace_commands::delete_environment_variable,
            workspace_commands::interpolate_with_active_environment,
            settings_commands::connection_doctor,
            settings_commands::list_onboarding_steps,
            settings_commands::set_onboarding_step,
            settings_commands::export_workspace,
            settings_commands::import_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Mobile API Studio");
}

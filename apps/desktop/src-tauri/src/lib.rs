use capture_core::CaptureEvent;
use core_model::{
    AppError, CaptureSession, ConnectionDiagnostic, Device, FlowSummary, SessionStatus,
    SCHEMA_VERSION,
};
use device_android::AndroidDeviceProvider;
use device_ios::IosDeviceProvider;
use serde::Serialize;
use std::{
    path::PathBuf,
    time::{SystemTime, UNIX_EPOCH},
};
use storage::{BodyStore, Database};
use tauri::{Manager, State};

struct AppState {
    database: Database,
    _body_store: BodyStore,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DeviceDiscoveryPayload {
    devices: Vec<Device>,
    diagnostics: Vec<ConnectionDiagnostic>,
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
fn create_session(name: String, state: State<'_, AppState>) -> Result<CaptureSession, AppError> {
    let timestamp = now_epoch_millis()?;
    let session = CaptureSession {
        schema_version: SCHEMA_VERSION,
        id: format!("session-{timestamp}"),
        name: sanitize_session_name(name),
        status: SessionStatus::Active,
        started_at: timestamp,
        ended_at: None,
        device_id: None,
        app_id: None,
        connection_strategy: None,
        capture_engine: None,
        notes: None,
    };

    state
        .database
        .create_session(&session)
        .map_err(|error| AppError::storage(error.to_string()))?;

    Ok(session)
}

#[tauri::command]
fn ingest_demo_flow(state: State<'_, AppState>) -> Result<FlowSummary, AppError> {
    let timestamp = now_epoch_millis()?;
    let flow = FlowSummary {
        schema_version: SCHEMA_VERSION,
        id: format!("demo-flow-{timestamp}"),
        session_id: None,
        source: core_model::FlowSource::Fixture,
        method: "GET".into(),
        host: "api.example.dev".into(),
        path: format!("/debug/flow/{timestamp}"),
        status_code: Some(200),
        duration_ms: Some(126),
        response_size_bytes: Some(864),
        started_at: timestamp,
    };

    ingest_capture_event(&state.database, CaptureEvent::FlowCompleted(flow.clone()))?;
    Ok(flow)
}

fn ingest_capture_event(database: &Database, event: CaptureEvent) -> Result<(), AppError> {
    match event {
        CaptureEvent::FlowStarted(flow)
        | CaptureEvent::FlowUpdated(flow)
        | CaptureEvent::FlowCompleted(flow) => database
            .upsert_flow(&flow)
            .map_err(|error| AppError::storage(error.to_string())),
        CaptureEvent::FlowFailed { .. }
        | CaptureEvent::EngineReady(_)
        | CaptureEvent::EngineStopped => Ok(()),
    }
}

fn initial_fixture_flows() -> Vec<FlowSummary> {
    vec![
        FlowSummary::fixture(
            "fixture-1",
            "GET",
            "api.example.dev",
            "/products/123",
            200,
            184,
            4_282,
            "2026-08-22T06:40:00Z",
        ),
        FlowSummary::fixture(
            "fixture-2",
            "POST",
            "api.example.dev",
            "/cart",
            201,
            311,
            1_104,
            "2026-08-22T06:40:01Z",
        ),
        FlowSummary::fixture(
            "fixture-3",
            "GET",
            "recommendations.example.dev",
            "/v2/recommendations",
            503,
            1_842,
            312,
            "2026-08-22T06:40:02Z",
        ),
    ]
}

fn seed_initial_flows(database: &Database) -> Result<(), String> {
    if !database.is_empty().map_err(|error| error.to_string())? {
        return Ok(());
    }

    for flow in initial_fixture_flows() {
        database
            .upsert_flow(&flow)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

fn initialize_state(app_data_dir: PathBuf) -> Result<AppState, String> {
    let database = Database::open(app_data_dir.join("app.db")).map_err(|error| error.to_string())?;
    let body_store = BodyStore::new(app_data_dir.join("bodies")).map_err(|error| error.to_string())?;

    seed_initial_flows(&database)?;

    Ok(AppState {
        database,
        _body_store: body_store,
    })
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
            let state = initialize_state(app_data_dir).map_err(std::io::Error::other)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            health,
            list_devices,
            list_flows,
            list_sessions,
            create_session,
            ingest_demo_flow,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Mobile API Studio");
}

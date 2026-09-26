use super::{now_epoch_millis, AppState};
use crate::State;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use capture_core::CaptureEngine;
use core_model::{
    AppError, BodyRef, CaptureSession, Environment, EnvironmentVariable, FlowDetail, FlowSummary,
    OnboardingStep, SavedCollection, SavedRequest, SessionStatus,
};
use device_android::AndroidDeviceProvider;
use device_ios::IosDeviceProvider;
use secret_store::SecretStore;
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, fs};
use workspace_core::{ConnectionDoctorReport, DoctorCheck, DoctorStatus};

const PORTABLE_BUNDLE_VERSION: u16 = 2;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableWorkspaceBundle {
    pub bundle_version: u16,
    pub exported_at: String,
    pub sessions: Vec<PortableSession>,
    pub collections: Vec<SavedCollection>,
    pub saved_requests: Vec<SavedRequest>,
    pub environments: Vec<Environment>,
    pub environment_variables: Vec<EnvironmentVariable>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableSession {
    pub session: CaptureSession,
    pub flows: Vec<PortableFlow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableFlow {
    pub summary: FlowSummary,
    pub detail: Option<FlowDetail>,
    pub request_body_base64: Option<String>,
    pub response_body_base64: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ImportMode {
    Merge,
    Replace,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub sessions: usize,
    pub flows: usize,
    pub collections: usize,
    pub saved_requests: usize,
    pub environments: usize,
    pub variables: usize,
    pub secret_values_omitted: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WorkspaceExportResult {
    pub bundle: PortableWorkspaceBundle,
    pub path: String,
}

pub async fn connection_doctor(
    state: State<'_, AppState>,
) -> Result<ConnectionDoctorReport, AppError> {
    let mut checks = Vec::new();

    let ios_provider = IosDeviceProvider;
    let ios_devices = if ios_provider.is_available() {
        match ios_provider.list_devices() {
            Ok(devices) => {
                checks.push(DoctorCheck {
                    id: "ios-tools".into(),
                    title: "Xcode / simctl".into(),
                    status: DoctorStatus::Pass,
                    detail: format!(
                        "simctl is available; {} Simulator runtimes discovered.",
                        devices.len()
                    ),
                    action: None,
                });
                devices
            }
            Err(error) => {
                checks.push(DoctorCheck {
                    id: "ios-tools".into(),
                    title: "Xcode / simctl".into(),
                    status: DoctorStatus::Warning,
                    detail: error.message,
                    action: Some("Open Xcode and confirm Command Line Tools and Simulator runtimes are installed.".into()),
                });
                Vec::new()
            }
        }
    } else {
        checks.push(DoctorCheck {
            id: "ios-tools".into(),
            title: "Xcode / simctl".into(),
            status: DoctorStatus::Warning,
            detail: "xcrun/simctl is not available on PATH.".into(),
            action: Some("Install Xcode and select Xcode Command Line Tools.".into()),
        });
        Vec::new()
    };

    let android_provider = AndroidDeviceProvider;
    let android_devices = if android_provider.is_available() {
        match android_provider.list_devices() {
            Ok(devices) => {
                checks.push(DoctorCheck {
                    id: "android-tools".into(),
                    title: "Android Platform Tools / ADB".into(),
                    status: DoctorStatus::Pass,
                    detail: format!(
                        "ADB is available; {} emulator runtimes discovered.",
                        devices.len()
                    ),
                    action: None,
                });
                devices
            }
            Err(error) => {
                checks.push(DoctorCheck {
                    id: "android-tools".into(),
                    title: "Android Platform Tools / ADB".into(),
                    status: DoctorStatus::Warning,
                    detail: error.message,
                    action: Some(
                        "Start ADB and confirm the emulator is listed by `adb devices`.".into(),
                    ),
                });
                Vec::new()
            }
        }
    } else {
        checks.push(DoctorCheck {
            id: "android-tools".into(),
            title: "Android Platform Tools / ADB".into(),
            status: DoctorStatus::Warning,
            detail: "adb is not available on PATH.".into(),
            action: Some(
                "Install Android SDK Platform Tools or expose platform-tools on PATH.".into(),
            ),
        });
        Vec::new()
    };

    let capture_result = state.capture_engine.prepare().await;
    let capture_executable = match capture_result {
        Ok(capabilities) => {
            checks.push(DoctorCheck {
                id: "capture-engine".into(),
                title: "Capture engine".into(),
                status: DoctorStatus::Pass,
                detail: capabilities
                    .engine_version
                    .map(|version| {
                        format!("{} is available ({version}).", capabilities.engine_name)
                    })
                    .unwrap_or_else(|| format!("{} is available.", capabilities.engine_name)),
                action: None,
            });
            Some("mitmdump".into())
        }
        Err(error) => {
            checks.push(DoctorCheck {
                id: "capture-engine".into(),
                title: "Capture engine".into(),
                status: DoctorStatus::Fail,
                detail: error.message,
                action: Some("Install mitmproxy/mitmdump or configure a managed capture sidecar before capturing traffic.".into()),
            });
            None
        }
    };

    let certificate_path = state.capture_engine.certificate_path();
    checks.push(DoctorCheck {
        id: "capture-ca".into(),
        title: "Capture certificate".into(),
        status: if certificate_path.is_file() { DoctorStatus::Pass } else { DoctorStatus::Warning },
        detail: if certificate_path.is_file() {
            format!("Local capture CA exists at {}.", certificate_path.display())
        } else {
            "The isolated capture CA has not been generated yet; mitmdump creates it during capture startup.".into()
        },
        action: (!certificate_path.is_file()).then(|| "Start a capture once to generate the isolated local CA.".into()),
    });

    let pending_rollback = {
        let _operation = state.connection_operation.lock().await;
        state.active_connection.lock().await.is_none() && state.rollback_path.exists()
    };
    checks.push(DoctorCheck {
        id: "rollback".into(),
        title: "Connection rollback state".into(),
        status: if pending_rollback {
            DoctorStatus::Warning
        } else {
            DoctorStatus::Pass
        },
        detail: if pending_rollback {
            "A previous connection left recovery state on disk.".into()
        } else {
            "No interrupted connection changes are pending.".into()
        },
        action: pending_rollback
            .then(|| "Use Recover on the Connect screen before starting another capture.".into()),
    });

    let secret_store = SecretStore;
    checks.push(DoctorCheck {
        id: "secure-store".into(),
        title: "Secure environment secrets".into(),
        status: if secret_store.is_available() { DoctorStatus::Pass } else { DoctorStatus::Warning },
        detail: if secret_store.is_available() {
            "Native secure storage is available; secret environment values are stored outside SQLite.".into()
        } else {
            "Native secure secret storage is not available in this desktop build.".into()
        },
        action: None,
    });

    Ok(ConnectionDoctorReport {
        checks,
        capture_executable,
        booted_ios_count: ios_devices
            .iter()
            .filter(|device| device.state.eq_ignore_ascii_case("booted"))
            .count(),
        android_emulator_count: android_devices
            .iter()
            .filter(|device| device.state.eq_ignore_ascii_case("device"))
            .count(),
        pending_rollback,
    })
}

pub fn list_onboarding_steps(state: State<'_, AppState>) -> Result<Vec<OnboardingStep>, AppError> {
    state
        .database
        .list_onboarding_steps()
        .map_err(storage_error)
}

pub fn set_onboarding_step(
    key: String,
    completed: bool,
    state: State<'_, AppState>,
) -> Result<OnboardingStep, AppError> {
    let step = OnboardingStep {
        key,
        completed,
        completed_at: if completed {
            Some(now_epoch_millis()?)
        } else {
            None
        },
    };
    state
        .database
        .upsert_onboarding_step(&step)
        .map_err(storage_error)?;
    Ok(step)
}

pub fn export_workspace(state: State<'_, AppState>) -> Result<PortableWorkspaceBundle, AppError> {
    let sessions = state
        .database
        .list_sessions(100_000)
        .map_err(storage_error)?;
    let all_flows = state.database.list_flows(100_000).map_err(storage_error)?;
    let mut portable_sessions = Vec::with_capacity(sessions.len());

    for session in sessions {
        let mut flows = Vec::new();
        for summary in all_flows
            .iter()
            .filter(|flow| flow.session_id.as_deref() == Some(session.id.as_str()))
        {
            let detail = state
                .database
                .get_flow_detail(&summary.id)
                .map_err(storage_error)?;
            let (request_body_base64, response_body_base64, detail) = match detail {
                Some(mut detail) => {
                    let request_body = read_body_for_export(
                        &state,
                        detail
                            .request
                            .as_ref()
                            .and_then(|request| request.body.as_ref()),
                    )?;
                    let response_body = read_body_for_export(
                        &state,
                        detail
                            .response
                            .as_ref()
                            .and_then(|response| response.body.as_ref()),
                    )?;
                    redact_flow_detail(&mut detail);
                    (request_body, response_body, Some(detail))
                }
                None => (None, None, None),
            };
            flows.push(PortableFlow {
                summary: summary.clone(),
                detail,
                request_body_base64,
                response_body_base64,
            });
        }
        portable_sessions.push(PortableSession { session, flows });
    }

    let collections = state.database.list_collections().map_err(storage_error)?;
    let saved_requests = state
        .database
        .list_saved_requests(None)
        .map_err(storage_error)?
        .into_iter()
        .map(redact_saved_request)
        .collect();
    let environments = state.database.list_environments().map_err(storage_error)?;
    let mut environment_variables = Vec::new();
    for environment in &environments {
        for mut variable in state
            .database
            .list_environment_variables(&environment.id)
            .map_err(storage_error)?
        {
            if variable.is_secret {
                variable.value = None;
                variable.secret_ref = None;
            }
            environment_variables.push(variable);
        }
    }

    Ok(PortableWorkspaceBundle {
        bundle_version: PORTABLE_BUNDLE_VERSION,
        exported_at: now_epoch_millis()?,
        sessions: portable_sessions,
        collections,
        saved_requests,
        environments,
        environment_variables,
    })
}

pub fn export_workspace_to_download(
    state: State<'_, AppState>,
) -> Result<WorkspaceExportResult, AppError> {
    let bundle = export_workspace(state)?;
    let bytes = serde_json::to_vec_pretty(&bundle).map_err(|error| {
        AppError::new("workspace_export_serialize_failed", error.to_string(), true)
    })?;
    let download_dir = std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .map(|home| home.join("Downloads"))
        .ok_or_else(|| {
            AppError::new(
                "workspace_export_path_failed",
                "Home directory is unavailable.",
                true,
            )
        })?;
    fs::create_dir_all(&download_dir).map_err(|error| {
        AppError::new("workspace_export_directory_failed", error.to_string(), true)
    })?;
    let path = download_dir.join(format!(
        "mobile-api-studio-workspace-{}.mas.json",
        bundle.exported_at
    ));
    fs::write(&path, bytes)
        .map_err(|error| AppError::new("workspace_export_write_failed", error.to_string(), true))?;

    Ok(WorkspaceExportResult {
        bundle,
        path: path.to_string_lossy().into_owned(),
    })
}

pub async fn import_workspace(
    bundle: PortableWorkspaceBundle,
    mode: ImportMode,
    state: State<'_, AppState>,
) -> Result<ImportSummary, AppError> {
    let _connection_operation = if matches!(mode, ImportMode::Replace) {
        Some(state.connection_operation.lock().await)
    } else {
        None
    };
    if bundle.bundle_version != PORTABLE_BUNDLE_VERSION {
        return Err(AppError::new(
            "unsupported_bundle_version",
            format!(
                "This build supports workspace bundle version {PORTABLE_BUNDLE_VERSION}, received {}.",
                bundle.bundle_version
            ),
            true,
        ));
    }
    validate_bundle_bodies(&bundle)?;

    if matches!(mode, ImportMode::Replace) {
        validate_replace_references(&bundle)?;
        if state.active_connection.lock().await.is_some() {
            return Err(AppError::new(
                "replace_import_capture_active",
                "Disconnect the active capture before replacing workspace data.",
                true,
            ));
        }
        clear_workspace(&state)?;
    }

    let imported_at = now_epoch_millis()?;
    let mut flow_count = 0usize;
    for portable_session in &bundle.sessions {
        let mut session = portable_session.session.clone();
        if session.status == SessionStatus::Active {
            session.status = SessionStatus::Interrupted;
            if session.ended_at.is_none() {
                session.ended_at = Some(imported_at.clone());
            }
        }
        state
            .database
            .create_session(&session)
            .map_err(storage_error)?;

        for portable_flow in &portable_session.flows {
            if let Some(mut detail) = portable_flow.detail.clone() {
                restore_body(
                    &state,
                    detail
                        .request
                        .as_mut()
                        .and_then(|request| request.body.as_mut()),
                    portable_flow.request_body_base64.as_deref(),
                )?;
                restore_body(
                    &state,
                    detail
                        .response
                        .as_mut()
                        .and_then(|response| response.body.as_mut()),
                    portable_flow.response_body_base64.as_deref(),
                )?;
                detail.summary.session_id = Some(session.id.clone());
                state
                    .database
                    .upsert_flow_detail(&detail)
                    .map_err(storage_error)?;
            } else {
                let mut summary = portable_flow.summary.clone();
                summary.session_id = Some(session.id.clone());
                state
                    .database
                    .upsert_flow(&summary)
                    .map_err(storage_error)?;
            }
            flow_count += 1;
        }
    }

    for collection in &bundle.collections {
        state
            .database
            .upsert_collection(collection)
            .map_err(storage_error)?;
    }
    for request in &bundle.saved_requests {
        state
            .database
            .upsert_saved_request(request)
            .map_err(storage_error)?;
    }
    for environment in &bundle.environments {
        state
            .database
            .upsert_environment(environment)
            .map_err(storage_error)?;
    }

    let mut omitted_secrets = 0usize;
    for variable in &bundle.environment_variables {
        let mut variable = variable.clone();
        if variable.is_secret {
            omitted_secrets += 1;
            variable.value = None;
            variable.secret_ref = None;
        }
        state
            .database
            .upsert_environment_variable(&variable)
            .map_err(storage_error)?;
    }

    Ok(ImportSummary {
        sessions: bundle.sessions.len(),
        flows: flow_count,
        collections: bundle.collections.len(),
        saved_requests: bundle.saved_requests.len(),
        environments: bundle.environments.len(),
        variables: bundle.environment_variables.len(),
        secret_values_omitted: omitted_secrets,
    })
}

fn validate_bundle_bodies(bundle: &PortableWorkspaceBundle) -> Result<(), AppError> {
    for session in &bundle.sessions {
        for flow in &session.flows {
            if let Some(detail) = flow.detail.as_ref() {
                if let Some(reference) = detail
                    .request
                    .as_ref()
                    .and_then(|request| request.body.as_ref())
                {
                    decode_bundle_body(reference, flow.request_body_base64.as_deref())?;
                }
                if let Some(reference) = detail
                    .response
                    .as_ref()
                    .and_then(|response| response.body.as_ref())
                {
                    decode_bundle_body(reference, flow.response_body_base64.as_deref())?;
                }
            }
        }
    }
    Ok(())
}

fn validate_replace_references(bundle: &PortableWorkspaceBundle) -> Result<(), AppError> {
    let collection_ids: HashSet<&str> = bundle
        .collections
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    for request in &bundle.saved_requests {
        if !collection_ids.contains(request.collection_id.as_str()) {
            return Err(AppError::new(
                "bundle_reference_missing",
                format!(
                    "Saved request {} refers to a collection missing from the bundle.",
                    request.id
                ),
                true,
            ));
        }
    }

    let environment_ids: HashSet<&str> = bundle
        .environments
        .iter()
        .map(|item| item.id.as_str())
        .collect();
    for variable in &bundle.environment_variables {
        if !environment_ids.contains(variable.environment_id.as_str()) {
            return Err(AppError::new(
                "bundle_reference_missing",
                format!(
                    "Environment variable {} refers to an environment missing from the bundle.",
                    variable.id
                ),
                true,
            ));
        }
    }
    Ok(())
}

fn clear_workspace(state: &State<'_, AppState>) -> Result<(), AppError> {
    let secret_store = SecretStore;
    for environment in state.database.list_environments().map_err(storage_error)? {
        for variable in state
            .database
            .list_environment_variables(&environment.id)
            .map_err(storage_error)?
        {
            if let Some(reference) = variable.secret_ref.as_deref() {
                let _ = secret_store.delete(reference);
            }
        }
        state
            .database
            .delete_environment(&environment.id)
            .map_err(storage_error)?;
    }
    for collection in state.database.list_collections().map_err(storage_error)? {
        state
            .database
            .delete_collection(&collection.id)
            .map_err(storage_error)?;
    }
    for session in state
        .database
        .list_sessions(100_000)
        .map_err(storage_error)?
    {
        state
            .database
            .delete_session(&session.id)
            .map_err(storage_error)?;
    }
    Ok(())
}

fn read_body_for_export(
    state: &State<'_, AppState>,
    reference: Option<&BodyRef>,
) -> Result<Option<String>, AppError> {
    reference
        .map(|reference| {
            state
                .body_store
                .read(&reference.sha256)
                .map(|bytes| BASE64.encode(bytes))
                .map_err(storage_error)
        })
        .transpose()
}

fn restore_body(
    state: &State<'_, AppState>,
    reference: Option<&mut BodyRef>,
    encoded: Option<&str>,
) -> Result<(), AppError> {
    let Some(reference) = reference else {
        return Ok(());
    };
    let bytes = decode_bundle_body(reference, encoded)?;
    let stored = state.body_store.put(&bytes).map_err(storage_error)?;
    reference.sha256 = stored.sha256;
    reference.byte_size = stored.byte_size;
    Ok(())
}

fn decode_bundle_body(reference: &BodyRef, encoded: Option<&str>) -> Result<Vec<u8>, AppError> {
    let Some(encoded) = encoded else {
        return Err(AppError::new(
            "bundle_body_missing",
            format!("Bundle is missing body bytes for {}.", reference.sha256),
            true,
        ));
    };
    BASE64.decode(encoded.as_bytes()).map_err(|error| {
        AppError::new(
            "bundle_body_invalid",
            format!("Invalid base64 body: {error}"),
            true,
        )
    })
}

fn redact_flow_detail(detail: &mut FlowDetail) {
    if let Some(request) = detail.request.as_mut() {
        for header in &mut request.headers {
            if header.sensitive || replay::is_sensitive_header(&header.name) {
                header.value = "<redacted>".into();
                header.sensitive = true;
            }
        }
    }
    if let Some(response) = detail.response.as_mut() {
        for header in &mut response.headers {
            if header.sensitive || replay::is_sensitive_header(&header.name) {
                header.value = "<redacted>".into();
                header.sensitive = true;
            }
        }
    }
}

fn redact_saved_request(mut request: SavedRequest) -> SavedRequest {
    for header in &mut request.headers {
        if header.sensitive || replay::is_sensitive_header(&header.name) {
            header.value = "<redacted>".into();
            header.sensitive = true;
        }
    }
    request
}

fn storage_error(error: storage::StorageError) -> AppError {
    AppError::storage(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use capture_core::CaptureHandle;
    use core_model::{HeaderValue, RequestDetail, ResponseDetail, Timing, SCHEMA_VERSION};

    #[test]
    fn export_redacts_known_secret_headers_even_when_unmarked() {
        let mut detail = FlowDetail {
            summary: FlowSummary::fixture("flow", "GET", "example.test", "/", 200, 1, 0, "1"),
            request: Some(RequestDetail {
                method: "GET".into(),
                url: "https://example.test/".into(),
                scheme: "https".into(),
                host: "example.test".into(),
                port: None,
                path: "/".into(),
                query: None,
                headers: vec![HeaderValue {
                    name: "Authorization".into(),
                    value: "Bearer private-token".into(),
                    sensitive: false,
                }],
                body: None,
            }),
            response: Some(ResponseDetail {
                status_code: 200,
                reason: None,
                headers: vec![HeaderValue {
                    name: "Set-Cookie".into(),
                    value: "session=private-cookie".into(),
                    sensitive: false,
                }],
                body: None,
            }),
            timing: Timing::default(),
            error_code: None,
            error_message: None,
        };
        redact_flow_detail(&mut detail);
        let request_header = &detail.request.unwrap().headers[0];
        let response_header = &detail.response.unwrap().headers[0];
        assert_eq!(request_header.value, "<redacted>");
        assert!(request_header.sensitive);
        assert_eq!(response_header.value, "<redacted>");
        assert!(response_header.sensitive);

        let saved = SavedRequest {
            schema_version: SCHEMA_VERSION,
            id: "request".into(),
            collection_id: "collection".into(),
            name: "API".into(),
            method: "GET".into(),
            url: "https://example.test/".into(),
            headers: vec![HeaderValue {
                name: "X-API-Key".into(),
                value: "private-key".into(),
                sensitive: false,
            }],
            body: None,
            source_flow_id: None,
            sort_order: 0,
            created_at: "1".into(),
            updated_at: "1".into(),
        };
        let saved = redact_saved_request(saved);
        assert_eq!(saved.headers[0].value, "<redacted>");
        assert!(saved.headers[0].sensitive);
    }

    #[test]
    fn doctor_reports_only_interrupted_journals_as_pending() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let data_dir = std::env::temp_dir().join(format!(
                    "mas-doctor-journal-{}-{}",
                    std::process::id(),
                    now_epoch_millis().unwrap()
                ));
                let state =
                    crate::initialize_state(data_dir.clone(), crate::resolve_addon_path().unwrap())
                        .unwrap();
                fs::write(&state.rollback_path, b"test journal").unwrap();
                *state.active_connection.lock().await = Some(crate::ActiveConnection {
                    handle: CaptureHandle {
                        id: "test-capture".into(),
                        session_id: "test-session".into(),
                        listen_host: "127.0.0.1".into(),
                        listen_port: 8181,
                    },
                    device_id: "ios:test-device".into(),
                    strategy: "ios_manual_proxy".into(),
                    previous_android_proxy: None,
                });
                let active_report = connection_doctor(State(&state)).await.unwrap();
                assert!(!active_report.pending_rollback);

                *state.active_connection.lock().await = None;
                let interrupted_report = connection_doctor(State(&state)).await.unwrap();
                assert!(interrupted_report.pending_rollback);

                drop(state);
                fs::remove_dir_all(data_dir).unwrap();
            });
    }

    #[test]
    fn corrupt_replace_bundle_preserves_existing_workspace() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let data_dir = std::env::temp_dir().join(format!(
                    "mas-invalid-replace-{}-{}",
                    std::process::id(),
                    now_epoch_millis().unwrap()
                ));
                let state =
                    crate::initialize_state(data_dir.clone(), crate::resolve_addon_path().unwrap())
                        .unwrap();
                let original = CaptureSession {
                    schema_version: SCHEMA_VERSION,
                    id: "original-session".into(),
                    name: "Keep this session".into(),
                    status: SessionStatus::Completed,
                    started_at: "1".into(),
                    ended_at: Some("2".into()),
                    device_id: None,
                    app_id: None,
                    connection_strategy: None,
                    capture_engine: None,
                    notes: None,
                };
                state.database.create_session(&original).unwrap();

                let mut summary =
                    FlowSummary::fixture("new-flow", "POST", "example.test", "/", 200, 1, 0, "3");
                summary.session_id = Some("new-session".into());
                let bundle = PortableWorkspaceBundle {
                    bundle_version: PORTABLE_BUNDLE_VERSION,
                    exported_at: "3".into(),
                    sessions: vec![PortableSession {
                        session: CaptureSession {
                            id: "new-session".into(),
                            ..original.clone()
                        },
                        flows: vec![PortableFlow {
                            summary: summary.clone(),
                            detail: Some(FlowDetail {
                                summary,
                                request: Some(RequestDetail {
                                    method: "POST".into(),
                                    url: "http://example.test/".into(),
                                    scheme: "http".into(),
                                    host: "example.test".into(),
                                    port: Some(80),
                                    path: "/".into(),
                                    query: None,
                                    headers: vec![],
                                    body: Some(BodyRef {
                                        sha256: "invalid-body".into(),
                                        byte_size: 1,
                                        content_type: None,
                                        encoding: None,
                                        is_binary: false,
                                        is_truncated: false,
                                    }),
                                }),
                                response: None,
                                timing: Timing::default(),
                                error_code: None,
                                error_message: None,
                            }),
                            request_body_base64: Some("not base64!".into()),
                            response_body_base64: None,
                        }],
                    }],
                    collections: vec![],
                    saved_requests: vec![],
                    environments: vec![],
                    environment_variables: vec![],
                };

                let error = import_workspace(bundle, ImportMode::Replace, State(&state))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "bundle_body_invalid");
                let sessions = state.database.list_sessions(10).unwrap();
                assert_eq!(sessions.len(), 1);
                assert_eq!(sessions[0].id, original.id);

                let orphan_bundle = PortableWorkspaceBundle {
                    bundle_version: PORTABLE_BUNDLE_VERSION,
                    exported_at: "4".into(),
                    sessions: vec![],
                    collections: vec![],
                    saved_requests: vec![],
                    environments: vec![],
                    environment_variables: vec![EnvironmentVariable {
                        schema_version: SCHEMA_VERSION,
                        id: "orphan-variable".into(),
                        environment_id: "missing-environment".into(),
                        key: "KEY".into(),
                        value: Some("value".into()),
                        is_secret: false,
                        secret_ref: None,
                        enabled: true,
                        sort_order: 0,
                    }],
                };
                let error = import_workspace(orphan_bundle, ImportMode::Replace, State(&state))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "bundle_reference_missing");
                assert_eq!(state.database.list_sessions(10).unwrap()[0].id, original.id);

                drop(state);
                fs::remove_dir_all(data_dir).unwrap();
            });
    }
}

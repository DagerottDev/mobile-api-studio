use super::{AppState, now_epoch_millis};
use crate::State;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use capture_core::CaptureEngine;
use core_model::{
    proxy_rules::{ProxyRule, ProxyRuleAction, RulePattern, RulePatternKind},
    network_profiles::{NetworkProfile, validate_network_profiles},
    AppError, BodyRef, CaptureSession, Environment, EnvironmentVariable, FlowDetail, FlowSummary,
    OnboardingStep, SavedCollection, SavedRequest, SessionStatus, WebSocketMessage,
};
use device_android::AndroidDeviceProvider;
use device_ios::IosDeviceProvider;
use secret_store::SecretStore;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{collections::HashSet, fs};
use storage::{ImportedFlow, ImportedSession, WorkspaceReplacement};
use workspace_core::{ConnectionDoctorReport, DoctorCheck, DoctorStatus};

const PORTABLE_BUNDLE_VERSION: u16 = 7;

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
    #[serde(default)]
    pub proxy_rules: Vec<PortableProxyRule>,
    #[serde(default)]
    pub network_profiles: Vec<NetworkProfile>,
    #[serde(default)]
    pub websocket_messages: Vec<PortableWebSocketMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableWebSocketMessage {
    pub message: WebSocketMessage,
    pub body_base64: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PortableProxyRule {
    pub rule: ProxyRule,
    #[serde(default)]
    pub map_local_file_omitted: bool,
    #[serde(default)]
    pub action_omitted: bool,
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
    pub proxy_rules: usize,
    pub map_local_files_omitted: usize,
    pub rule_actions_omitted: usize,
    pub proxy_rules_disabled: usize,
    pub network_profiles: usize,
    pub network_profiles_disabled: usize,
    pub websocket_messages: usize,
    pub search_index_warning: Option<String>,
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

    let stored_rules = state.database.list_proxy_rules().map_err(storage_error)?;
    let mut used_ids: HashSet<String> = stored_rules.iter().map(|rule| rule.id.clone()).collect();
    let proxy_rules = stored_rules.into_iter().enumerate().map(|(index, mut rule)| {
        let map_local_file_omitted = matches!(rule.action, ProxyRuleAction::MapLocal { .. });
        let action_omitted = !matches!(rule.action, ProxyRuleAction::Allow | ProxyRuleAction::Block { .. } | ProxyRuleAction::Breakpoint { .. } | ProxyRuleAction::NoCache | ProxyRuleAction::BlockCookies);
        if action_omitted {
            let mut candidate = format!("omitted-rule-{index}");
            while !used_ids.insert(candidate.clone()) { candidate.push('_'); }
            rule.id = candidate;
            rule.name = "Omitted rule action".into();
            rule.matcher.method = None;
            rule.matcher.host = RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() };
            rule.matcher.path = RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() };
            rule.action = ProxyRuleAction::Allow;
            rule.enabled = false;
        }
        PortableProxyRule { rule, map_local_file_omitted, action_omitted }
    }).collect();

    let included_flows: HashSet<&str> = portable_sessions.iter().flat_map(|session| session.flows.iter().map(|flow| flow.summary.id.as_str())).collect();
    let mut websocket_messages = Vec::new();
    let mut offset = 0;
    loop {
        let messages = state.database.list_websocket_messages(None, None, None, 1_000, offset).map_err(storage_error)?;
        for message in &messages {
            if included_flows.contains(message.flow_id.as_str()) {
                websocket_messages.push(PortableWebSocketMessage { message: message.clone(), body_base64: read_body_for_export(&state, message.body.as_ref())? });
            }
        }
        offset += messages.len();
        if messages.len() < 1_000 { break; }
    }
    Ok(PortableWorkspaceBundle {
        bundle_version: PORTABLE_BUNDLE_VERSION,
        exported_at: now_epoch_millis()?,
        sessions: portable_sessions,
        collections,
        saved_requests,
        environments,
        environment_variables,
        proxy_rules,
        network_profiles: state.database.list_network_profiles().map_err(storage_error)?,
        websocket_messages,
    })
}

pub fn export_selected_script_rules(ids: Vec<String>, state: State<'_, AppState>) -> Result<PortableWorkspaceBundle, AppError> {
    if ids.is_empty() || ids.len() > 100 || ids.iter().collect::<HashSet<_>>().len() != ids.len() {
        return Err(AppError::new("script_export_selection_invalid", "Select 1–100 distinct stored script rules.", true));
    }
    let stored = state.database.list_proxy_rules().map_err(storage_error)?;
    let mut rules = Vec::new();
    for id in ids {
        let mut rule = stored.iter().find(|rule| rule.id == id).cloned().ok_or_else(|| AppError::new("script_export_rule_missing", "Selected script rule is unavailable.", true))?;
        if !matches!(rule.action, ProxyRuleAction::ScriptHook { .. }) {
            return Err(AppError::new("script_export_rule_invalid", "Select only JavaScript hook rules.", true));
        }
        crate::proxy_rule_commands::validate_proxy_rule(&rule, &state, false)?;
        rule.enabled = false;
        rules.push(PortableProxyRule { rule, map_local_file_omitted: false, action_omitted: false });
    }
    Ok(PortableWorkspaceBundle { bundle_version: PORTABLE_BUNDLE_VERSION, exported_at: now_epoch_millis()?,
        sessions: vec![], collections: vec![], saved_requests: vec![], environments: vec![], environment_variables: vec![],
        proxy_rules: rules, network_profiles: vec![], websocket_messages: vec![] })
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
    if !matches!(bundle.bundle_version, 2 | 3 | 4 | 5 | 6 | PORTABLE_BUNDLE_VERSION) {
        return Err(AppError::new(
            "unsupported_bundle_version",
            format!(
                "This build supports workspace bundle versions 2 through {PORTABLE_BUNDLE_VERSION}, received {}.",
                bundle.bundle_version
            ),
            true,
        ));
    }
    for imported in &bundle.sessions {
        if imported
            .session
            .capture_target
            .as_ref()
            .is_some_and(|target| target.schema_version != 1)
            || imported
                .session
                .capture_mode
                .as_ref()
                .is_some_and(|mode| mode.schema_version != 1)
        {
            return Err(AppError::new(
                "unsupported_capture_metadata_version",
                "This workspace contains unsupported capture metadata.",
                true,
            ));
        }
    }
    validate_bundle_bodies(&bundle)?;
    validate_bundle_messages(&bundle)?;
    let proxy_rules = validate_bundle_rules(&bundle, &mode, &state)?;
    let network_profiles = validate_bundle_network_profiles(&bundle, &mode, &state)?;
    let imported_at = now_epoch_millis()?;

    if matches!(mode, ImportMode::Replace) {
        validate_replace_references(&bundle)?;
        if state.active_connection.lock().await.is_some() {
            return Err(AppError::new(
                "replace_import_capture_active",
                "Disconnect the active capture before replacing workspace data.",
                true,
            ));
        }
        let replacement = prepare_replacement(&bundle, &state, &imported_at, proxy_rules, network_profiles)?;
        let old_secret_refs = state
            .database
            .replace_workspace(&replacement)
            .map_err(storage_error)?;
        let secret_store = SecretStore;
        for reference in old_secret_refs {
            let _ = secret_store.delete(&reference);
        }
        let mut summary = import_summary(&bundle);
        summary.search_index_warning = rebuild_imported_indexes(&bundle, &state).err().map(|_| "Workspace imported; search indexing failed. Use Rebuild search index in Settings.".into());
        return Ok(summary);
    }

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

    for portable in &bundle.websocket_messages {
        let mut message = portable.message.clone();
        restore_body(&state, message.body.as_mut(), portable.body_base64.as_deref())?;
        state.database.upsert_websocket_message(&message, "").map_err(storage_error)?;
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
    for rule in &proxy_rules {
        state.database.upsert_proxy_rule(rule).map_err(storage_error)?;
    }

    for profile in &network_profiles {
        state.database.upsert_network_profile(profile).map_err(storage_error)?;
    }

    Ok(ImportSummary {
        sessions: bundle.sessions.len(),
        flows: flow_count,
        collections: bundle.collections.len(),
        saved_requests: bundle.saved_requests.len(),
        environments: bundle.environments.len(),
        variables: bundle.environment_variables.len(),
        secret_values_omitted: omitted_secrets,
        proxy_rules: proxy_rules.len(),
        map_local_files_omitted: bundle.proxy_rules.iter().filter(|item| item.map_local_file_omitted).count(),
        rule_actions_omitted: bundle.proxy_rules.iter().filter(|item| item.action_omitted).count(),
        proxy_rules_disabled: proxy_rules.len(),
        network_profiles: network_profiles.len(),
        network_profiles_disabled: network_profiles.len(),
        websocket_messages: bundle.websocket_messages.len(),
        search_index_warning: rebuild_imported_indexes(&bundle, &state).err().map(|_| "Workspace imported; search indexing failed. Use Rebuild search index in Settings.".into()),
    })
}

fn prepare_replacement(
    bundle: &PortableWorkspaceBundle,
    state: &State<'_, AppState>,
    imported_at: &str,
    proxy_rules: Vec<ProxyRule>,
    network_profiles: Vec<NetworkProfile>,
) -> Result<WorkspaceReplacement, AppError> {
    let mut sessions = Vec::with_capacity(bundle.sessions.len());
    for portable_session in &bundle.sessions {
        let mut session = portable_session.session.clone();
        if session.status == SessionStatus::Active {
            session.status = SessionStatus::Interrupted;
            if session.ended_at.is_none() {
                session.ended_at = Some(imported_at.to_owned());
            }
        }
        let mut flows = Vec::with_capacity(portable_session.flows.len());
        for portable_flow in &portable_session.flows {
            let mut detail = portable_flow.detail.clone();
            if let Some(detail) = detail.as_mut() {
                restore_body(
                    state,
                    detail
                        .request
                        .as_mut()
                        .and_then(|request| request.body.as_mut()),
                    portable_flow.request_body_base64.as_deref(),
                )?;
                restore_body(
                    state,
                    detail
                        .response
                        .as_mut()
                        .and_then(|response| response.body.as_mut()),
                    portable_flow.response_body_base64.as_deref(),
                )?;
                detail.summary.session_id = Some(session.id.clone());
            }
            let mut summary = detail
                .as_ref()
                .map(|detail| detail.summary.clone())
                .unwrap_or_else(|| portable_flow.summary.clone());
            summary.session_id = Some(session.id.clone());
            flows.push(ImportedFlow { summary, detail });
        }
        sessions.push(ImportedSession { session, flows });
    }
    let environment_variables = bundle
        .environment_variables
        .iter()
        .cloned()
        .map(|mut variable| {
            if variable.is_secret {
                variable.value = None;
                variable.secret_ref = None;
            }
            variable
        })
        .collect();
    let mut websocket_messages = Vec::with_capacity(bundle.websocket_messages.len());
    for portable in &bundle.websocket_messages {
        let mut message = portable.message.clone();
        restore_body(state, message.body.as_mut(), portable.body_base64.as_deref())?;
        websocket_messages.push(message);
    }
    Ok(WorkspaceReplacement {
        sessions,
        collections: bundle.collections.clone(),
        saved_requests: bundle.saved_requests.clone(),
        environments: bundle.environments.clone(),
        environment_variables,
        proxy_rules,
        network_profiles,
        websocket_messages,
    })
}

fn import_summary(bundle: &PortableWorkspaceBundle) -> ImportSummary {
    ImportSummary {
        sessions: bundle.sessions.len(),
        flows: bundle
            .sessions
            .iter()
            .map(|session| session.flows.len())
            .sum(),
        collections: bundle.collections.len(),
        saved_requests: bundle.saved_requests.len(),
        environments: bundle.environments.len(),
        variables: bundle.environment_variables.len(),
        secret_values_omitted: bundle
            .environment_variables
            .iter()
            .filter(|variable| variable.is_secret)
            .count(),
        proxy_rules: bundle.proxy_rules.len(),
        map_local_files_omitted: bundle.proxy_rules.iter().filter(|item| item.map_local_file_omitted).count(),
        rule_actions_omitted: bundle.proxy_rules.iter().filter(|item| item.action_omitted).count(),
        proxy_rules_disabled: bundle.proxy_rules.len(),
        network_profiles: bundle.network_profiles.len(),
        network_profiles_disabled: bundle.network_profiles.len(),
        websocket_messages: bundle.websocket_messages.len(),
        search_index_warning: None,
    }
}

fn validate_bundle_network_profiles(bundle: &PortableWorkspaceBundle, mode: &ImportMode, state: &State<'_, AppState>) -> Result<Vec<NetworkProfile>, AppError> {
    if bundle.bundle_version < 6 && !bundle.network_profiles.is_empty() {
        return Err(AppError::new("bundle_network_profiles_invalid", "Network profiles require bundle version 6.", true));
    }
    validate_network_profiles(&bundle.network_profiles).map_err(|error| AppError::new("bundle_network_profiles_invalid", error, true))?;
    let profiles = bundle.network_profiles.iter().cloned().map(|mut profile| { profile.enabled = false; profile }).collect::<Vec<_>>();
    if matches!(mode, ImportMode::Merge) {
        let mut merged = state.database.list_network_profiles().map_err(storage_error)?;
        merged.retain(|current| !profiles.iter().any(|profile| profile.id == current.id));
        merged.extend(profiles.clone());
        validate_network_profiles(&merged).map_err(|error| AppError::new("bundle_network_profiles_invalid", error, true))?;
    }
    Ok(profiles)
}

fn validate_bundle_rules(bundle: &PortableWorkspaceBundle, mode: &ImportMode, state: &State<'_, AppState>) -> Result<Vec<ProxyRule>, AppError> {
    if bundle.bundle_version < 7 && bundle.proxy_rules.iter().any(|rule| matches!(rule.rule.action, core_model::proxy_rules::ProxyRuleAction::ScriptHook { .. })) {
        return Err(AppError::new("bundle_scripts_invalid", "Script hooks require bundle version 7.", true));
    }

    if bundle.proxy_rules.len() > 1_000 || (bundle.bundle_version < 4 && !bundle.proxy_rules.is_empty()) {
        return Err(AppError::new("bundle_proxy_rules_invalid", "Bundle contains unsupported or too many proxy rules.", true));
    }
    let mut ids = HashSet::new();
    let mut rules = Vec::with_capacity(bundle.proxy_rules.len());
    let mut storage_bytes = 0_usize;
    for portable in &bundle.proxy_rules {
        let mut rule = portable.rule.clone();
        if !ids.insert(rule.id.clone()) || (portable.map_local_file_omitted && !portable.action_omitted)
            || (portable.action_omitted && (rule.enabled || !matches!(rule.action, ProxyRuleAction::Allow)))
            || (!portable.action_omitted && matches!(rule.action, ProxyRuleAction::MapLocal { .. })) {
            return Err(AppError::new("bundle_proxy_rules_invalid", "Bundle contains duplicate rules or invalid omitted action metadata.", true));
        }
        rule.enabled = false;
        super::proxy_rule_commands::validate_proxy_rule(&rule, state, false)?;
        storage_bytes = storage_bytes.saturating_add(serde_json::to_vec(&rule).map_err(|error| AppError::new("bundle_proxy_rules_invalid", error.to_string(), true))?.len());
        rules.push(rule);
    }
    if matches!(mode, ImportMode::Merge) {
        let existing = state.database.list_proxy_rules().map_err(storage_error)?;
        for rule in existing.iter().filter(|rule| !ids.contains(&rule.id)) {
            storage_bytes = storage_bytes.saturating_add(serde_json::to_vec(rule).map_err(|error| AppError::new("bundle_proxy_rules_invalid", error.to_string(), true))?.len());
        }
        let total = existing.iter().filter(|rule| !ids.contains(&rule.id)).count() + rules.len();
        if total > 1_000 {
            return Err(AppError::new("proxy_rule_limit", "The workspace supports at most 1,000 proxy rules.", true));
        }
    }
    if storage_bytes > super::proxy_rule_commands::MAX_RULE_STORAGE_BYTES {
        return Err(AppError::new("proxy_rule_storage_limit", "Proxy rule definitions exceed the workspace limit of 16 MiB.", true));
    }
    Ok(rules)
}

fn validate_bundle_bodies(bundle: &PortableWorkspaceBundle) -> Result<(), AppError> {
    for session in &bundle.sessions {
        for flow in &session.flows {
            if let Some(detail) = flow.detail.as_ref() {
                if let Some(protocol) = detail.protocol.as_ref() {
                    core_model::validate_protocol_details(protocol).map_err(|error| AppError::new("bundle_protocol_invalid", error, true))?;
                }
                if detail.proxy_rule_ids.len() > 100 || detail.proxy_rule_changes.len() > 100
                    || detail.proxy_rule_ids.iter().any(|id| id.len() > 120)
                    || detail.proxy_rule_changes.iter().any(|change| {
                        change.rule_id.len() > 120 || change.field.len() > 120
                            || change.before.len() > 256 || change.after.len() > 256
                    }) {
                    return Err(AppError::new("bundle_rule_metadata_invalid", "Proxy rule flow metadata exceeds supported limits.", true));
                }
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

fn validate_bundle_messages(bundle: &PortableWorkspaceBundle) -> Result<(), AppError> {
    if bundle.websocket_messages.len() > 100_000 || (bundle.bundle_version < 5 && !bundle.websocket_messages.is_empty()) {
        return Err(AppError::new("bundle_messages_invalid", "WebSocket messages require bundle v5 and at most 100,000 records.", true));
    }
    let flows: std::collections::HashMap<&str, &str> = bundle.sessions.iter().flat_map(|session| session.flows.iter().map(move |flow| (flow.summary.id.as_str(), session.session.id.as_str()))).collect();
    let mut ids = HashSet::new();
    for portable in &bundle.websocket_messages {
        let message = &portable.message;
        core_model::validate_websocket_message(message).map_err(|error| AppError::new("bundle_messages_invalid", error, true))?;
        if !ids.insert(&message.id) || !flows.contains_key(message.flow_id.as_str()) || flows.get(message.flow_id.as_str()).copied() != message.session_id.as_deref() {
            return Err(AppError::new("bundle_messages_invalid", "Duplicate message or mismatched flow/session reference.", true));
        }
        if let Some(body) = message.body.as_ref() {
            let bytes = decode_bundle_body(body, portable.body_base64.as_deref())?;
            if bytes.len() as u64 != body.byte_size || format!("{:x}", Sha256::digest(&bytes)) != body.sha256.to_ascii_lowercase() {
                return Err(AppError::new("bundle_messages_invalid", "WebSocket payload size or SHA-256 does not match its metadata.", true));
            }
        } else if portable.body_base64.is_some() {
            return Err(AppError::new("bundle_messages_invalid", "WebSocket payload is missing its body metadata.", true));
        }
    }
    Ok(())
}

fn rebuild_imported_indexes(bundle: &PortableWorkspaceBundle, state: &State<'_, AppState>) -> Result<(), AppError> {
    for session in &bundle.sessions {
        for flow in &session.flows {
            if let Some(detail) = state.database.get_flow_detail(&flow.summary.id).map_err(storage_error)? {
                super::search_index::index_flow(&state.database, &state.body_store, &detail)?;
            }
        }
    }
    for portable in &bundle.websocket_messages { index_imported_message(state, &portable.message)?; }
    Ok(())
}

fn index_imported_message(state: &State<'_, AppState>, message: &WebSocketMessage) -> Result<(), AppError> {
    let text = match message.body.as_ref().filter(|body| !body.is_binary) {
        Some(body) => {
            let bytes = state.body_store.read_bounded(&body.sha256, 2 * 1024 * 1024).map_err(storage_error)?;
            super::search_index::redacted_body_text(&bytes, body.content_type.as_deref())?
        }
        None => String::new(),
    };
    state.database.set_websocket_search_text(&message.id, &text).map_err(storage_error)
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
    if encoded.len() > 4 * (2 * 1024 * 1024_usize).div_ceil(3) {
        return Err(AppError::new("bundle_body_too_large", "Portable payloads must be 2 MiB or smaller.", true));
    }
    let bytes = BASE64.decode(encoded.as_bytes()).map_err(|error| {
        AppError::new(
            "bundle_body_invalid",
            format!("Invalid base64 body: {error}"),
            true,
        )
    })?;
    if bytes.len() > 2 * 1024 * 1024 {
        return Err(AppError::new("bundle_body_too_large", "Portable payloads must be 2 MiB or smaller.", true));
    }
    Ok(bytes)
}

fn redact_flow_detail(detail: &mut FlowDetail) {
    if let Some(protocol) = detail.protocol.as_mut() {
        for header in protocol.request_trailers.iter_mut().chain(&mut protocol.response_trailers) {
            if header.sensitive || replay::is_sensitive_header(&header.name) {
                header.value = "<redacted>".into();
                header.sensitive = true;
            }
        }
    }
    detail.proxy_rule_ids.clear();
    detail.proxy_rule_changes.clear();
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
    use core_model::proxy_rules::{ProxyRuleMatcher, PROXY_RULE_SCHEMA_VERSION};
    use core_model::{HeaderValue, RequestDetail, ResponseDetail, SCHEMA_VERSION, Timing};

    #[test]
    fn network_profiles_bundle_v6_round_trip_disabled_and_replace_validation_preserves_data() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-network-bundle-{}-{}", std::process::id(), now_epoch_millis().unwrap()));
            let state = crate::initialize_state(root.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            let profile = NetworkProfile { schema_version: 1, id: "slow".into(), name: "Slow".into(), enabled: true,
                priority: 0, scope: core_model::network_profiles::NetworkScope::Global {},
                latency_ms: 100, jitter_ms: 10, upload_bytes_per_second: None, download_bytes_per_second: Some(1024),
                offline: false, failure_percent: 1.0, created_at: "1".into(), updated_at: "2".into() };
            state.database.upsert_network_profile(&profile).unwrap();
            let bundle = export_workspace(State(&state)).unwrap();
            assert_eq!(bundle.bundle_version, 7);
            assert_eq!(bundle.network_profiles, vec![profile.clone()]);
            let mut invalid = bundle.clone(); invalid.network_profiles[0].latency_ms = 10_001;
            assert_eq!(import_workspace(invalid, ImportMode::Replace, State(&state)).await.unwrap_err().code, "bundle_network_profiles_invalid");
            assert_eq!(state.database.list_network_profiles().unwrap(), vec![profile.clone()]);
            let mut duplicate = bundle.clone(); duplicate.network_profiles.push(profile.clone());
            assert!(import_workspace(duplicate, ImportMode::Merge, State(&state)).await.is_err());
            assert_eq!(state.database.list_network_profiles().unwrap(), vec![profile.clone()]);
            for mode in [ImportMode::Merge, ImportMode::Replace] {
                let summary = import_workspace(bundle.clone(), mode, State(&state)).await.unwrap();
                assert_eq!(summary.network_profiles, 1); assert_eq!(summary.network_profiles_disabled, 1);
                assert!(!state.database.list_network_profiles().unwrap()[0].enabled);
            }
            let mut old = serde_json::to_value(bundle).unwrap();
            old.as_object_mut().unwrap().remove("networkProfiles");
            for version in 2..=5 {
                old["bundleVersion"] = version.into();
                let old_bundle: PortableWorkspaceBundle = serde_json::from_value(old.clone()).unwrap();
                assert!(old_bundle.network_profiles.is_empty());
                import_workspace(old_bundle, ImportMode::Replace, State(&state)).await.unwrap();
                assert!(state.database.list_network_profiles().unwrap().is_empty());
            }
            drop(state); std::fs::remove_dir_all(root).unwrap();
        });
    }

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
            proxy_rule_ids: Vec::new(),
            proxy_rule_changes: Vec::new(),
            protocol: None,
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
                    device_id: Some("ios:test-device".into()),
                    target: core_model::CaptureTarget {
                        schema_version: core_model::SCHEMA_VERSION,
                        kind: core_model::CaptureTargetKind::IosSimulator {
                            device_id: "ios:test-device".into(),
                        },
                    },
                    strategy: "ios_manual_proxy".into(),
                    previous_android_proxy: None,
                    lan_guard: None,
                    sdk_guard: None,
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
    fn protocol_bundle_roundtrip_rebuilds_search_and_rejects_orphans() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let root = std::env::temp_dir().join(format!("mas-protocol-bundle-{}-{}", std::process::id(), now_epoch_millis().unwrap()));
            let state = crate::initialize_state(root.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            let bytes = serde_json::to_vec(&serde_json::json!({"a_message": "needle", "large": "界".repeat(12_000), "password": "private-secret"})).unwrap();
            let body = BodyRef { sha256: format!("{:x}", Sha256::digest(&bytes)), byte_size: bytes.len() as u64,
                content_type: Some("application/json".into()), encoding: None, is_binary: false, is_truncated: false };
            let mut summary = FlowSummary::fixture("ws-flow", "GET", "example.test", "/ws", 101, 1, 0, "1");
            summary.session_id = Some("session".into());
            let detail = FlowDetail { summary: summary.clone(), request: None, response: None, timing: Timing::default(),
                error_code: None, error_message: None, proxy_rule_ids: vec![], proxy_rule_changes: vec![],
                protocol: Some(core_model::ProtocolDetails { request_http_version: Some("HTTP/2.0".into()), websocket: true,
                    response_trailers: vec![HeaderValue { name: "grpc-status".into(), value: "0".into(), sensitive: false }], ..Default::default() }) };
            let message = WebSocketMessage { id: "ws-msg".into(), flow_id: summary.id.clone(), session_id: summary.session_id.clone(),
                sequence: 1, from_client: true, opcode: 1, timestamp: "2".into(), dropped: false, injected: false, body: Some(body) };
            let bundle = PortableWorkspaceBundle { bundle_version: 5, exported_at: "3".into(),
                sessions: vec![PortableSession { session: CaptureSession { schema_version: SCHEMA_VERSION, id: "session".into(),
                    name: "Protocol".into(), status: SessionStatus::Completed, started_at: "1".into(), ended_at: Some("3".into()),
                    device_id: None, app_id: None, connection_strategy: None, capture_engine: None, notes: None,
                    capture_target: None, capture_mode: None }, flows: vec![PortableFlow { summary, detail: Some(detail),
                        request_body_base64: None, response_body_base64: None }] }],
                collections: vec![], saved_requests: vec![], environments: vec![], environment_variables: vec![], proxy_rules: vec![], network_profiles: vec![],
                websocket_messages: vec![PortableWebSocketMessage { message: message.clone(), body_base64: Some(BASE64.encode(&bytes)) }] };
            import_workspace(bundle, ImportMode::Merge, State(&state)).await.unwrap();
            assert_eq!(state.database.list_websocket_messages(None, None, Some("needle"), 10, 0).unwrap().len(), 1);
            assert!(state.database.list_websocket_messages(None, None, Some("private-secret"), 10, 0).unwrap().is_empty());
            let exported = export_workspace(State(&state)).unwrap();
            assert!(!serde_json::to_string(&exported).unwrap().contains("search_text"));
            drop(state);
            let state = crate::initialize_state(root.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            assert_eq!(state.database.list_websocket_messages(None, None, None, 10, 0).unwrap(), vec![message.clone()]);
            import_workspace(exported.clone(), ImportMode::Replace, State(&state)).await.unwrap();
            assert_eq!(state.database.list_websocket_messages(None, None, Some("needle"), 10, 0).unwrap().len(), 1);
            let protocol = state.database.get_flow_detail("ws-flow").unwrap().unwrap().protocol.unwrap();
            assert_eq!(protocol.request_http_version.as_deref(), Some("HTTP/2.0"));
            assert_eq!(protocol.response_trailers[0].value, "0");
            let mut orphan = exported.clone();
            orphan.websocket_messages[0].message.flow_id = "missing".into();
            orphan.websocket_messages[0].message.session_id = None;
            assert_eq!(import_workspace(orphan, ImportMode::Merge, State(&state)).await.unwrap_err().code, "bundle_messages_invalid");
            let oversized = BASE64.encode(vec![0; 2 * 1024 * 1024 + 1]);
            assert_eq!(decode_bundle_body(message.body.as_ref().unwrap(), Some(&oversized)).unwrap_err().code, "bundle_body_too_large");
            let mut corrupt = exported;
            corrupt.websocket_messages[0].body_base64 = Some(BASE64.encode(b"wrong"));
            assert_eq!(import_workspace(corrupt, ImportMode::Replace, State(&state)).await.unwrap_err().code, "bundle_messages_invalid");
            assert_eq!(state.database.list_websocket_messages(None, None, None, 10, 0).unwrap(), vec![message]);
            drop(state);
            fs::remove_dir_all(root).unwrap();
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
                    capture_target: None,
                    capture_mode: None,
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
                                proxy_rule_ids: Vec::new(),
                                proxy_rule_changes: Vec::new(),
            protocol: None,
                            }),
                            request_body_base64: Some("not base64!".into()),
                            response_body_base64: None,
                        }],
                    }],
                    collections: vec![],
                    saved_requests: vec![],
                    environments: vec![],
                    environment_variables: vec![],
                    proxy_rules: vec![], network_profiles: vec![],
                    websocket_messages: Vec::new(),
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
                    proxy_rules: vec![], network_profiles: vec![],
                    websocket_messages: Vec::new(),
                };
                let error = import_workspace(orphan_bundle, ImportMode::Replace, State(&state))
                    .await
                    .unwrap_err();
                assert_eq!(error.code, "bundle_reference_missing");
                assert_eq!(state.database.list_sessions(10).unwrap()[0].id, original.id);

                let duplicate_name_bundle = PortableWorkspaceBundle {
                    bundle_version: PORTABLE_BUNDLE_VERSION,
                    exported_at: "5".into(),
                    sessions: vec![],
                    collections: vec![],
                    saved_requests: vec![],
                    environments: vec![
                        Environment {
                            schema_version: SCHEMA_VERSION,
                            id: "first-environment".into(),
                            name: "Duplicate".into(),
                            is_active: false,
                            created_at: "5".into(),
                            updated_at: "5".into(),
                        },
                        Environment {
                            schema_version: SCHEMA_VERSION,
                            id: "second-environment".into(),
                            name: "duplicate".into(),
                            is_active: false,
                            created_at: "5".into(),
                            updated_at: "5".into(),
                        },
                    ],
                    environment_variables: vec![],
                    proxy_rules: vec![], network_profiles: vec![],
                    websocket_messages: Vec::new(),
                };
                assert!(
                    import_workspace(duplicate_name_bundle, ImportMode::Replace, State(&state))
                        .await
                        .is_err()
                );
                assert_eq!(state.database.list_sessions(10).unwrap()[0].id, original.id);
                assert!(state.database.list_environments().unwrap().is_empty());

                let replacement = CaptureSession {
                    id: "replacement-session".into(),
                    name: "Imported session".into(),
                    ..original.clone()
                };
                let valid_bundle = PortableWorkspaceBundle {
                    bundle_version: PORTABLE_BUNDLE_VERSION,
                    exported_at: "6".into(),
                    sessions: vec![PortableSession {
                        session: replacement.clone(),
                        flows: vec![PortableFlow {
                            summary: FlowSummary::fixture(
                                "replacement-flow",
                                "GET",
                                "example.test",
                                "/new",
                                200,
                                1,
                                0,
                                "6",
                            ),
                            detail: None,
                            request_body_base64: None,
                            response_body_base64: None,
                        }],
                    }],
                    collections: vec![SavedCollection {
                        schema_version: SCHEMA_VERSION,
                        id: "replacement-collection".into(),
                        name: "Imported collection".into(),
                        description: None,
                        sort_order: 0,
                        created_at: "6".into(),
                        updated_at: "6".into(),
                    }],
                    saved_requests: vec![SavedRequest {
                        schema_version: SCHEMA_VERSION,
                        id: "replacement-request".into(),
                        collection_id: "replacement-collection".into(),
                        name: "Imported request".into(),
                        method: "GET".into(),
                        url: "https://example.test/new".into(),
                        headers: vec![],
                        body: None,
                        source_flow_id: None,
                        sort_order: 0,
                        created_at: "6".into(),
                        updated_at: "6".into(),
                    }],
                    environments: vec![Environment {
                        schema_version: SCHEMA_VERSION,
                        id: "replacement-environment".into(),
                        name: "Imported environment".into(),
                        is_active: true,
                        created_at: "6".into(),
                        updated_at: "6".into(),
                    }],
                    environment_variables: vec![EnvironmentVariable {
                        schema_version: SCHEMA_VERSION,
                        id: "replacement-variable".into(),
                        environment_id: "replacement-environment".into(),
                        key: "TOKEN".into(),
                        value: Some("must-not-import".into()),
                        is_secret: true,
                        secret_ref: Some("must-not-import".into()),
                        enabled: true,
                        sort_order: 0,
                    }],
                    proxy_rules: vec![], network_profiles: vec![],
                    websocket_messages: Vec::new(),
                };
                let result = import_workspace(valid_bundle, ImportMode::Replace, State(&state))
                    .await
                    .unwrap();
                assert_eq!(result.sessions, 1);
                assert_eq!(result.flows, 1);
                assert_eq!(result.secret_values_omitted, 1);
                assert_eq!(
                    state.database.list_sessions(10).unwrap()[0].id,
                    replacement.id
                );
                assert_eq!(
                    state.database.list_flows(10).unwrap()[0]
                        .session_id
                        .as_deref(),
                    Some(replacement.id.as_str())
                );
                assert_eq!(state.database.list_collections().unwrap().len(), 1);
                assert_eq!(state.database.list_saved_requests(None).unwrap().len(), 1);
                let variables = state
                    .database
                    .list_environment_variables("replacement-environment")
                    .unwrap();
                assert_eq!(variables.len(), 1);
                assert_eq!(variables[0].value, None);
                assert_eq!(variables[0].secret_ref, None);

                drop(state);
                fs::remove_dir_all(data_dir).unwrap();
            });
    }

    #[test]
    fn bundle_omits_sensitive_rule_actions_and_replaces_rules_atomically() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let data_dir = std::env::temp_dir().join(format!("mas-rule-bundle-{}-{}", std::process::id(), now_epoch_millis().unwrap()));
            let state = crate::initialize_state(data_dir.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            let rule = ProxyRule {
                schema_version: PROXY_RULE_SCHEMA_VERSION,
                id: "private-id".into(), name: "private-name".into(), enabled: true, priority: 1,
                matcher: ProxyRuleMatcher { method: None,
                    host: RulePattern { kind: RulePatternKind::Exact, value: "private.example".into() },
                    path: RulePattern { kind: RulePatternKind::Exact, value: "/private".into() } },
                action: ProxyRuleAction::MapRemote { url: "https://example.test/?token=secret".into() },
                created_at: "1".into(), updated_at: "1".into(),
            };
            state.database.upsert_proxy_rule(&rule).unwrap();
            let safe = ProxyRule { id: "omitted-rule-0".into(), name: "Block".into(), priority: 2,
                matcher: ProxyRuleMatcher { method: None,
                    host: RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() },
                    path: RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() } },
                action: ProxyRuleAction::Block { status_code: 403 }, ..rule.clone() };
            state.database.upsert_proxy_rule(&safe).unwrap();
            let local = ProxyRule { id: "local-id".into(), name: "Local".into(), priority: 3,
                action: ProxyRuleAction::MapLocal { path: "private-map-file".into() }, ..safe.clone() };
            state.database.upsert_proxy_rule(&local).unwrap();
            let bundle = export_workspace(State(&state)).unwrap();
            let json = serde_json::to_string(&bundle).unwrap();
            assert!(!json.contains("secret"));
            assert!(!json.contains("private"));
            assert!(bundle.proxy_rules[0].action_omitted);
            assert!(!bundle.proxy_rules[0].rule.enabled);
            assert_ne!(bundle.proxy_rules[0].rule.id, safe.id);
            assert!(bundle.proxy_rules[1].rule.enabled);
            assert!(bundle.proxy_rules[2].map_local_file_omitted);
            assert!(!json.contains("private-map-file"));

            let mut invalid = bundle.clone();
            invalid.proxy_rules[0].rule.enabled = true;
            assert_eq!(import_workspace(invalid, ImportMode::Replace, State(&state)).await.unwrap_err().code, "bundle_proxy_rules_invalid");
            assert_eq!(state.database.list_proxy_rules().unwrap().len(), 3);

            let summary = import_workspace(bundle, ImportMode::Replace, State(&state)).await.unwrap();
            assert_eq!(summary.proxy_rules, 3);
            assert_eq!(summary.rule_actions_omitted, 2);
            assert_eq!(summary.map_local_files_omitted, 1);
            assert_eq!(summary.proxy_rules_disabled, 3);
            assert_eq!(state.database.list_proxy_rules().unwrap()[0].action, ProxyRuleAction::Allow);
            assert!(state.database.list_proxy_rules().unwrap().iter().all(|rule| !rule.enabled));

            let mut old_json = serde_json::to_value(export_workspace(State(&state)).unwrap()).unwrap();
            old_json.as_object_mut().unwrap().remove("proxyRules");
            old_json["bundleVersion"] = 3.into();
            let old_bundle: PortableWorkspaceBundle = serde_json::from_value(old_json).unwrap();
            assert!(old_bundle.proxy_rules.is_empty());
            import_workspace(old_bundle, ImportMode::Replace, State(&state)).await.unwrap();
            assert!(state.database.list_proxy_rules().unwrap().is_empty());

            drop(state);
            fs::remove_dir_all(data_dir).unwrap();
        });
    }
    #[test]
    fn selected_script_roundtrip_requires_review_and_old_bundles_cannot_smuggle_hooks() {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(async {
            let data_dir = std::env::temp_dir().join(format!("mas-script-bundle-{}-{}", std::process::id(), now_epoch_millis().unwrap()));
            let state = crate::initialize_state(data_dir.clone(), crate::resolve_addon_path().unwrap()).unwrap();
            let rule = ProxyRule { schema_version: PROXY_RULE_SCHEMA_VERSION, id: "script-a".into(), name: "Local hook".into(), enabled: true, priority: 0,
                matcher: ProxyRuleMatcher { method: None, host: RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() }, path: RulePattern { kind: RulePatternKind::Wildcard, value: "*".into() } },
                action: ProxyRuleAction::ScriptHook { stage: "request".into(), script: "function transform(e) { e.headers.push({name:'X-Test',value:'literal'}); return e; }".into() }, created_at: "1".into(), updated_at: "1".into() };
            crate::proxy_rule_commands::upsert_proxy_rule(rule.clone(), State(&state)).unwrap();
            let ordinary = export_workspace(State(&state)).unwrap();
            assert!(ordinary.proxy_rules[0].action_omitted);
            assert!(!serde_json::to_string(&ordinary).unwrap().contains("function transform"));
            let selected = export_selected_script_rules(vec![rule.id.clone()], State(&state)).unwrap();
            assert_eq!(selected.proxy_rules[0].rule.action, rule.action);
            assert!(!selected.proxy_rules[0].rule.enabled);
            let mut old = selected.clone(); old.bundle_version = 6;
            assert_eq!(import_workspace(old, ImportMode::Merge, State(&state)).await.unwrap_err().code, "bundle_scripts_invalid");
            assert!(state.database.list_proxy_rules().unwrap()[0].enabled);
            import_workspace(selected, ImportMode::Merge, State(&state)).await.unwrap();
            assert!(!state.database.list_proxy_rules().unwrap()[0].enabled);
            drop(state); fs::remove_dir_all(data_dir).unwrap();
        });
    }

}

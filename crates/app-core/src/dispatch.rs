use crate::{AppState, State};
use core_model::AppError;
use serde_json::Value;

fn input<T: serde::de::DeserializeOwned>(args: &Value, key: &str) -> Result<T, AppError> {
    serde_json::from_value(args.get(key).cloned().unwrap_or(Value::Null)).map_err(|error| {
        AppError::new("invalid_arguments", format!("Invalid {key}: {error}"), true)
    })
}

fn output<T: serde::Serialize>(value: T) -> Result<Value, AppError> {
    serde_json::to_value(value)
        .map_err(|error| AppError::new("response_serialize_failed", error.to_string(), true))
}

pub async fn invoke(command: &str, args: Value, state: &AppState) -> Result<Value, AppError> {
    match command {
        "ai_settings" => output(crate::ai_commands::ai_settings(State(state))?),
        "set_ai_settings" => {
            let input = input(&args, "input")?;
            output(crate::ai_commands::set_ai_settings(input, State(state))?)
        }
        "clear_ai_api_key" => output(crate::ai_commands::clear_ai_api_key(State(state))?),
        "preview_session_ai_context" => {
            let baseline_session_id = input(&args, "baselineSessionId")?;
            let candidate_session_id = input(&args, "candidateSessionId")?;
            output(crate::ai_commands::preview_session_ai_context(
                baseline_session_id,
                candidate_session_id,
                State(state),
            )?)
        }
        "explain_session_comparison" => {
            let baseline_session_id = input(&args, "baselineSessionId")?;
            let candidate_session_id = input(&args, "candidateSessionId")?;
            let input = input(&args, "input")?;
            output(
                crate::ai_commands::explain_session_comparison(
                    baseline_session_id,
                    candidate_session_id,
                    input,
                    State(state),
                )
                .await?,
            )
        }
        "preview_flow_ai_context" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::ai_commands::preview_flow_ai_context(
                flow_id,
                State(state),
            )?)
        }
        "diagnose_flow_with_ai" => {
            let flow_id = input(&args, "flowId")?;
            let input = input(&args, "input")?;
            output(crate::ai_commands::diagnose_flow_with_ai(flow_id, input, State(state)).await?)
        }
        "list_ai_results" => {
            let source_ref = input(&args, "sourceRef")?;
            let limit = input(&args, "limit")?;
            output(crate::ai_commands::list_ai_results(
                source_ref,
                limit,
                State(state),
            )?)
        }
        "list_pending_breakpoints" => output(crate::breakpoint_commands::list_pending_breakpoints(
            State(state),
        )?),
        "resolve_breakpoint" => {
            let input = input(&args, "input")?;
            output(crate::breakpoint_commands::resolve_breakpoint(
                input,
                State(state),
            )?)
        }
        "clear_stale_breakpoints" => output(crate::breakpoint_commands::clear_stale_breakpoints(
            State(state),
        )?),
        "compare_sessions" => {
            let baseline_session_id = input(&args, "baselineSessionId")?;
            let candidate_session_id = input(&args, "candidateSessionId")?;
            output(crate::compare_commands::compare_sessions(
                baseline_session_id,
                candidate_session_id,
                State(state),
            )?)
        }
        "session_diagnostics" => {
            let session_id = input(&args, "sessionId")?;
            output(crate::compare_commands::session_diagnostics(
                session_id,
                State(state),
            )?)
        }
        "list_mock_fixtures" => output(crate::fixture_commands::list_mock_fixtures(State(state))?),
        "upsert_mock_fixture" => {
            let fixture = input(&args, "fixture")?;
            output(crate::fixture_commands::upsert_mock_fixture(
                fixture,
                State(state),
            )?)
        }
        "delete_mock_fixture" => {
            let id = input(&args, "id")?;
            output(crate::fixture_commands::delete_mock_fixture(
                id,
                State(state),
            )?)
        }
        "create_fixture_from_flow" => {
            let flow_id = input(&args, "flowId")?;
            let name = input(&args, "name")?;
            output(crate::fixture_commands::create_fixture_from_flow(
                flow_id,
                name,
                State(state),
            )?)
        }
        "apply_fixture_to_mock" => {
            let fixture_id = input(&args, "fixtureId")?;
            let rule_id = input(&args, "ruleId")?;
            output(crate::fixture_commands::apply_fixture_to_mock(
                fixture_id,
                rule_id,
                State(state),
            )?)
        }
        "get_flow_detail" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::inspect::get_flow_detail(flow_id, State(state))?)
        }
        "read_body" => {
            let sha256 = input(&args, "sha256")?;
            output(crate::inspect::read_body(sha256, State(state))?)
        }
        "export_curl" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::inspect::export_curl(flow_id, State(state))?)
        }
        "health" => output(crate::health(State(state))),
        "list_devices" => output(crate::list_devices()),
        "list_flows" => output(crate::list_flows(State(state))?),
        "list_sessions" => output(crate::list_sessions(State(state))?),
        "current_connection" => output(crate::current_connection(State(state)).await?),
        "connect_device" => {
            let device_id = input(&args, "deviceId")?;
            let session_name = input(&args, "sessionName")?;
            output(crate::connect_device(device_id, session_name, State(state)).await?)
        }
        "disconnect_device" => output(crate::disconnect_device(State(state)).await?),
        "pending_rollback" => output(crate::pending_rollback(State(state)).await?),
        "recover_pending_rollback" => output(crate::recover_pending_rollback(State(state)).await?),
        "list_mock_rules" => output(crate::mock_commands::list_mock_rules(State(state))?),
        "upsert_mock_rule" => {
            let rule = input(&args, "rule")?;
            output(crate::mock_commands::upsert_mock_rule(rule, State(state))?)
        }
        "create_mock_from_flow" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::mock_commands::create_mock_from_flow(
                flow_id,
                State(state),
            )?)
        }
        "set_mock_rule_enabled" => {
            let id = input(&args, "id")?;
            let enabled = input(&args, "enabled")?;
            output(crate::mock_commands::set_mock_rule_enabled(
                id,
                enabled,
                State(state),
            )?)
        }
        "set_mock_rule_priority" => {
            let id = input(&args, "id")?;
            let priority = input(&args, "priority")?;
            output(crate::mock_commands::set_mock_rule_priority(
                id,
                priority,
                State(state),
            )?)
        }
        "delete_mock_rule" => {
            let id = input(&args, "id")?;
            output(crate::mock_commands::delete_mock_rule(id, State(state))?)
        }
        "disable_all_mocks" => output(crate::mock_commands::disable_all_mocks(State(state))?),
        "create_replay_draft" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::replay_commands::create_replay_draft(
                flow_id,
                State(state),
            )?)
        }
        "send_replay" => {
            let draft = input(&args, "draft")?;
            output(crate::replay_commands::send_replay(draft, State(state)).await?)
        }
        "sdk_setup_info" => output(crate::sdk_commands::sdk_setup_info(State(state))?),
        "list_sdk_clients" => output(crate::sdk_commands::list_sdk_clients(State(state))?),
        "list_sdk_events" => {
            let client_id = input(&args, "clientId")?;
            let limit = input(&args, "limit")?;
            output(crate::sdk_commands::list_sdk_events(
                client_id,
                limit,
                State(state),
            )?)
        }
        "search_sdk_events" => {
            let text = input(&args, "text")?;
            let limit = input(&args, "limit")?;
            output(crate::sdk_commands::search_sdk_events(
                text,
                limit,
                State(state),
            )?)
        }
        "sdk_flow_ids_matching" => {
            let text = input(&args, "text")?;
            let limit = input(&args, "limit")?;
            output(crate::sdk_commands::sdk_flow_ids_matching(
                text,
                limit,
                State(state),
            )?)
        }
        "sdk_enrichment_for_flow" => {
            let flow_id = input(&args, "flowId")?;
            output(crate::sdk_commands::sdk_enrichment_for_flow(
                flow_id,
                State(state),
            )?)
        }
        "connection_doctor" => {
            output(crate::settings_commands::connection_doctor(State(state)).await?)
        }
        "list_onboarding_steps" => output(crate::settings_commands::list_onboarding_steps(State(
            state,
        ))?),
        "set_onboarding_step" => {
            let key = input(&args, "key")?;
            let completed = input(&args, "completed")?;
            output(crate::settings_commands::set_onboarding_step(
                key,
                completed,
                State(state),
            )?)
        }
        "export_workspace" => output(crate::settings_commands::export_workspace(State(state))?),
        "export_workspace_to_download" => output(
            crate::settings_commands::export_workspace_to_download(State(state))?,
        ),
        "import_workspace" => {
            let bundle = input(&args, "bundle")?;
            let mode = input(&args, "mode")?;
            output(crate::settings_commands::import_workspace(bundle, mode, State(state)).await?)
        }
        "capture_executable_setting" => output(
            crate::sidecar_commands::capture_executable_setting(State(state))?,
        ),
        "set_capture_executable" => {
            let executable = input(&args, "executable")?;
            output(crate::sidecar_commands::set_capture_executable(
                executable,
                State(state),
            )?)
        }
        "update_session_metadata" => {
            let input = input(&args, "input")?;
            output(crate::workspace_commands::update_session_metadata(
                input,
                State(state),
            )?)
        }
        "archive_session" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::archive_session(
                id,
                State(state),
            )?)
        }
        "delete_session" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::delete_session(id, State(state))?)
        }
        "search_traffic" => {
            let query = input(&args, "query")?;
            output(crate::workspace_commands::search_traffic(
                query,
                State(state),
            )?)
        }
        "list_collections" => output(crate::workspace_commands::list_collections(State(state))?),
        "upsert_collection" => {
            let input = input(&args, "input")?;
            output(crate::workspace_commands::upsert_collection(
                input,
                State(state),
            )?)
        }
        "delete_collection" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::delete_collection(
                id,
                State(state),
            )?)
        }
        "list_saved_requests" => {
            let collection_id = input(&args, "collectionId")?;
            output(crate::workspace_commands::list_saved_requests(
                collection_id,
                State(state),
            )?)
        }
        "save_flow_to_collection" => {
            let input = input(&args, "input")?;
            output(crate::workspace_commands::save_flow_to_collection(
                input,
                State(state),
            )?)
        }
        "delete_saved_request" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::delete_saved_request(
                id,
                State(state),
            )?)
        }
        "list_environments" => output(crate::workspace_commands::list_environments(State(state))?),
        "upsert_environment" => {
            let input = input(&args, "input")?;
            output(crate::workspace_commands::upsert_environment(
                input,
                State(state),
            )?)
        }
        "set_active_environment" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::set_active_environment(
                id,
                State(state),
            )?)
        }
        "delete_environment" => {
            let id = input(&args, "id")?;
            output(crate::workspace_commands::delete_environment(
                id,
                State(state),
            )?)
        }
        "environment_snapshot" => {
            let environment_id = input(&args, "environmentId")?;
            output(crate::workspace_commands::environment_snapshot(
                environment_id,
                State(state),
            )?)
        }
        "upsert_environment_variable" => {
            let input = input(&args, "input")?;
            output(crate::workspace_commands::upsert_environment_variable(
                input,
                State(state),
            )?)
        }
        "delete_environment_variable" => {
            let id = input(&args, "id")?;
            let environment_id = input(&args, "environmentId")?;
            output(crate::workspace_commands::delete_environment_variable(
                id,
                environment_id,
                State(state),
            )?)
        }
        "interpolate_with_active_environment" => {
            let template = input(&args, "template")?;
            output(
                crate::workspace_commands::interpolate_with_active_environment(
                    template,
                    State(state),
                )?,
            )
        }
        _ => Err(AppError::new(
            "unknown_command",
            format!("Unknown command: {command}"),
            true,
        )),
    }
}

use super::{inspect, AppState};
use core_model::{AppError, FlowDetail};
use replay::{ReplayDraft, ReplayEngine};
use tauri::State;

#[tauri::command]
pub async fn execute_replay(draft: ReplayDraft, state: State<'_, AppState>) -> Result<FlowDetail, AppError> {
    let engine = ReplayEngine::new().map_err(replay_error_to_app_error)?;
    let captured = engine.execute(draft).await.map_err(replay_error_to_app_error)?;
    let flow_id = captured.summary.id.clone();
    inspect::persist_captured_flow(&state.database, &state.body_store, captured)?;
    state.database.get_flow_detail(&flow_id)
        .map_err(|error| AppError::storage(error.to_string()))?
        .ok_or_else(|| AppError::new("replay_persist_failed", "Replay completed but its persisted flow could not be loaded.", true))
}

fn replay_error_to_app_error(error: replay::ReplayError) -> AppError { AppError::new(error.code, error.message, error.recoverable) }

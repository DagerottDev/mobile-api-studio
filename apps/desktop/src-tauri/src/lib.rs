use core_model::FlowSummary;
use std::path::PathBuf;
use storage::{BodyStore, Database};
use tauri::{Manager, State};

struct AppState {
    database: Database,
    _body_store: BodyStore,
}

#[tauri::command]
fn health(state: State<'_, AppState>) -> String {
    format!("Rust core ready · {}", state.database.path().display())
}

#[tauri::command]
fn list_flows(state: State<'_, AppState>) -> Result<Vec<FlowSummary>, String> {
    state
        .database
        .list_flows(5_000)
        .map_err(|error| error.to_string())
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

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .setup(|app| {
            let app_data_dir = app.path().app_data_dir()?;
            let state = initialize_state(app_data_dir).map_err(std::io::Error::other)?;
            app.manage(state);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![health, list_flows])
        .run(tauri::generate_context!())
        .expect("error while running Mobile API Studio");
}

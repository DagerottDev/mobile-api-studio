use core_model::{FlowSource, FlowSummary};

#[tauri::command]
fn health() -> String {
    "Rust core ready".to_string()
}

#[tauri::command]
fn list_fake_flows() -> Vec<FlowSummary> {
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
        FlowSummary {
            schema_version: 1,
            id: "fixture-3".into(),
            source: FlowSource::Fixture,
            method: "GET".into(),
            host: "recommendations.example.dev".into(),
            path: "/v2/recommendations".into(),
            status_code: Some(503),
            duration_ms: Some(1_842),
            response_size_bytes: Some(312),
            started_at: "2026-08-22T06:40:02Z".into(),
        },
    ]
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![health, list_fake_flows])
        .run(tauri::generate_context!())
        .expect("error while running Mobile API Studio");
}

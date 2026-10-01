use ai_core::{AiContextPolicy, build_context_preview};
use core_model::{AppError, BodyRef, FlowDetail};
use serde::Serialize;
use serde_json::{Value, json};
use storage::{BodyStore, Database};

const MAX_INDEX: usize = 8 * 1024;

pub fn redacted_body_text(bytes: &[u8], content_type: Option<&str>) -> Result<String, AppError> {
    let Ok(text) = std::str::from_utf8(bytes) else { return Ok(String::new()); };
    // Index structured bodies with the same policy used by the AI preview. Arbitrary
    // unstructured text has no reliable secret-field boundary, so stays in the raw view.
    let mime = content_type.unwrap_or("").split(';').next().unwrap_or("").trim();
    let value = if mime.eq_ignore_ascii_case("application/x-www-form-urlencoded") {
        json!({"contentType": mime, "text": text})
    } else {
        match serde_json::from_str::<Value>(text) {
            Ok(value @ (Value::Object(_) | Value::Array(_))) => value,
            _ => return Ok(String::new()),
        }
    };
    redact(&value)
}

fn redact(value: &Value) -> Result<String, AppError> {
    let policy = AiContextPolicy { max_context_bytes: MAX_INDEX, max_string_chars: 12_000, ..Default::default() };
    let mut text = build_context_preview(value, &policy)
        .map_err(|error| AppError::new("search_redaction_failed", error.message, true))?.json;
    let mut end = text.len().min(MAX_INDEX);
    while !text.is_char_boundary(end) { end -= 1; }
    text.truncate(end);
    Ok(text)
}

fn body_text(body_store: &BodyStore, body: Option<&BodyRef>) -> Result<String, AppError> {
    let Some(body) = body.filter(|body| !body.is_binary) else { return Ok(String::new()); };
    let bytes = body_store.read_bounded(&body.sha256, 2 * 1024 * 1024)
        .map_err(|error| AppError::storage(error.to_string()))?;
    redacted_body_text(&bytes, body.content_type.as_deref())
}

pub fn index_flow(database: &Database, body_store: &BodyStore, detail: &FlowDetail) -> Result<(), AppError> {
    let request = detail.request.as_ref();
    let response = detail.response.as_ref();
    let index = redact(&json!({
        "requestHeaders": request.map(|request| &request.headers),
        "responseHeaders": response.map(|response| &response.headers),
        "requestTrailers": detail.protocol.as_ref().map(|protocol| &protocol.request_trailers),
        "responseTrailers": detail.protocol.as_ref().map(|protocol| &protocol.response_trailers),
        "requestBody": body_text(body_store, request.and_then(|request| request.body.as_ref()))?,
        "responseBody": body_text(body_store, response.and_then(|response| response.body.as_ref()))?,
    }))?;
    database.upsert_flow_search_text(&detail.summary.id, &index).map_err(|error| AppError::storage(error.to_string()))
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexProgress { pub processed: usize, pub next_offset: usize, pub done: bool }

pub fn rebuild(database: &Database, body_store: &BodyStore, phase: &str, offset: usize) -> Result<IndexProgress, AppError> {
    if phase == "websocket" {
        let messages = database.list_websocket_messages(None, None, None, 100, offset).map_err(|error| AppError::storage(error.to_string()))?;
        for message in &messages {
            let text = body_text(body_store, message.body.as_ref())?;
            database.set_websocket_search_text(&message.id, &text).map_err(|error| AppError::storage(error.to_string()))?;
        }
        return Ok(IndexProgress { processed: messages.len(), next_offset: offset + messages.len(), done: messages.len() < 100 });
    }
    if phase != "flows" { return Err(AppError::new("invalid_search_phase", "Choose flows or websocket.", true)); }
    let ids = database.list_flow_ids_for_search(100, offset).map_err(|error| AppError::storage(error.to_string()))?;
    for id in &ids {
        if let Some(detail) = database.get_flow_detail(id).map_err(|error| AppError::storage(error.to_string()))? {
            index_flow(database, body_store, &detail)?;
        }
    }
    Ok(IndexProgress { processed: ids.len(), next_offset: offset + ids.len(), done: ids.len() < 100 })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexes_supported_text_without_secret_values_or_binary() {
        let json = redacted_body_text(br#"{"message":"needle","password":"never-index-this","headers":[{"name":"authorization","value":"Bearer forbidden"}]}"#, Some("application/json")).unwrap();
        assert!(json.contains("needle"));
        assert!(!json.contains("never-index-this"));
        assert!(!json.contains("Bearer forbidden"));
        for shape in [json!({"name": "ordinary", "value": "safe", "password": "never-index-this"}),
                      json!({"name": "ordinary", "baseline": "safe", "candidate": "safe", "token": "never-index-this"})] {
            let sanitized = redacted_body_text(&serde_json::to_vec(&shape).unwrap(), Some("application/json")).unwrap();
            assert!(!sanitized.contains("never-index-this"));
            assert!(sanitized.contains("safe"));
        }
        let form = redacted_body_text(b"name=needle&token=never-index-this", Some("application/x-www-form-urlencoded")).unwrap();
        assert!(form.contains("needle"));
        assert!(!form.contains("never-index-this"));
        assert!(redacted_body_text(&[0xff], None).unwrap().is_empty());
        assert!(redacted_body_text(b"Bearer unknown-secret", Some("text/plain")).unwrap().is_empty());
        let large = serde_json::to_vec(&json!({"message": "界".repeat(12_000), "password": "never-index-this"})).unwrap();
        let bounded = redacted_body_text(&large, Some("application/json")).unwrap();
        assert!(bounded.len() <= 8 * 1024);
        assert!(!bounded.contains("never-index-this"));
    }
}

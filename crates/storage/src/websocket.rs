use super::{Database, StorageError};
use core_model::{WebSocketMessage, validate_websocket_message};
use rusqlite::{params, Connection};

const MIGRATION_006: &str = r#"
CREATE TABLE websocket_messages (
    id TEXT PRIMARY KEY,
    flow_id TEXT NOT NULL,
    session_id TEXT,
    sequence INTEGER NOT NULL,
    timestamp TEXT NOT NULL,
    message_json TEXT NOT NULL,
    search_text TEXT NOT NULL DEFAULT '',
    FOREIGN KEY(flow_id) REFERENCES flows(id) ON DELETE CASCADE
);
CREATE INDEX idx_websocket_flow_sequence ON websocket_messages(flow_id, sequence, id);
CREATE INDEX idx_websocket_session_time ON websocket_messages(session_id, timestamp, id);
CREATE TABLE flow_search_text (
    flow_id TEXT PRIMARY KEY,
    redacted_text TEXT NOT NULL,
    FOREIGN KEY(flow_id) REFERENCES flows(id) ON DELETE CASCADE
);
INSERT INTO schema_migrations(version) VALUES (6);
"#;

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let mut connection = database.connection()?;
    let migrated: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 6)", [], |row| row.get(0),
    )?;
    if !migrated {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_006)?;
        transaction.commit()?;
    }
    Ok(())
}

pub(super) fn insert_message(connection: &Connection, record: &WebSocketMessage, search_text: &str) -> Result<(), StorageError> {
    validate(record, search_text)?;
    let json = serde_json::to_string(record)?;
    connection.execute(
        "INSERT INTO websocket_messages (id, flow_id, session_id, sequence, timestamp, message_json, search_text) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7) ON CONFLICT(id) DO UPDATE SET flow_id=excluded.flow_id, session_id=excluded.session_id, sequence=excluded.sequence, timestamp=excluded.timestamp, message_json=excluded.message_json, search_text=excluded.search_text",
        params![&record.id, &record.flow_id, &record.session_id, i64::try_from(record.sequence).map_err(|_| StorageError::InvalidInput("WebSocket sequence exceeds SQLite range".into()))?, &record.timestamp, json, search_text],
    )?;
    Ok(())
}

fn validate(record: &WebSocketMessage, search_text: &str) -> Result<(), StorageError> {
    validate_websocket_message(record).map_err(StorageError::InvalidInput)?;
    if search_text.len() > 8_192 { return Err(StorageError::InvalidInput("WebSocket search text exceeds 8 KiB".into())); }
    Ok(())
}

impl Database {
    pub fn upsert_flow_search_text(&self, flow_id: &str, redacted_text: &str) -> Result<(), StorageError> {
        if flow_id.is_empty() || flow_id.len() > 512 || redacted_text.len() > 64 * 1024 {
            return Err(StorageError::InvalidInput("Flow search text exceeds supported bounds".into()));
        }
        self.connection()?.execute(
            "INSERT INTO flow_search_text (flow_id, redacted_text) VALUES (?1, ?2) ON CONFLICT(flow_id) DO UPDATE SET redacted_text=excluded.redacted_text",
            params![flow_id, redacted_text],
        )?;
        Ok(())
    }

    pub fn list_flow_ids_for_search(&self, limit: usize, offset: usize) -> Result<Vec<String>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare("SELECT id FROM flows ORDER BY started_at DESC, id LIMIT ?1 OFFSET ?2")?;
        statement.query_map(params![i64::try_from(limit.min(1_000)).unwrap(), i64::try_from(offset).map_err(|_| StorageError::InvalidInput("Flow search offset exceeds SQLite range".into()))?], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn set_websocket_search_text(&self, message_id: &str, redacted_text: &str) -> Result<(), StorageError> {
        if message_id.is_empty() || message_id.len() > 512 || redacted_text.len() > 8_192 {
            return Err(StorageError::InvalidInput("WebSocket search text exceeds supported bounds".into()));
        }
        self.connection()?.execute("UPDATE websocket_messages SET search_text = ?2 WHERE id = ?1", params![message_id, redacted_text])?;
        Ok(())
    }

    pub fn upsert_websocket_message(&self, record: &WebSocketMessage, search_text: &str) -> Result<(), StorageError> {
        insert_message(&self.connection()?, record, search_text)
    }

    pub fn list_websocket_messages(&self, flow_id: Option<&str>, session_id: Option<&str>, text: Option<&str>, limit: usize, offset: usize) -> Result<Vec<WebSocketMessage>, StorageError> {
        if flow_id.is_some_and(|value| value.len() > 512) || session_id.is_some_and(|value| value.len() > 512)
            || text.is_some_and(|value| value.len() > 256) {
            return Err(StorageError::InvalidInput("WebSocket query exceeds supported bounds".into()));
        }
        let connection = self.connection()?;
        // ponytail: substring search scans matching rows; add FTS5 if large captures make this slow.
        let mut statement = connection.prepare(
            "SELECT message_json FROM websocket_messages WHERE (?1 IS NULL OR flow_id = ?1) AND (?2 IS NULL OR session_id = ?2) AND (?3 IS NULL OR instr(lower(search_text), lower(?3)) > 0) ORDER BY CAST(timestamp AS INTEGER), flow_id, sequence, id LIMIT ?4 OFFSET ?5",
        )?;
        let rows = statement.query_map(params![flow_id, session_id, text.filter(|value| !value.is_empty()), i64::try_from(limit.min(1_000)).unwrap(), i64::try_from(offset).map_err(|_| StorageError::InvalidInput("WebSocket query offset exceeds SQLite range".into()))?], |row| row.get::<_, String>(0))?;
        rows.map(|row| serde_json::from_str(&row?).map_err(StorageError::from)).collect()
    }

    pub fn update_websocket_close(&self, flow_id: &str, close_code: Option<u16>, close_reason: Option<String>, closed_by_client: Option<bool>) -> Result<bool, StorageError> {
        if flow_id.is_empty() || flow_id.len() > 512 || close_reason.as_ref().is_some_and(|value| value.len() > 1_024) {
            return Err(StorageError::InvalidInput("WebSocket close metadata exceeds supported bounds".into()));
        }
        let Some(mut detail) = self.get_flow_detail(flow_id)? else { return Ok(false); };
        let protocol = detail.protocol.get_or_insert_with(Default::default);
        protocol.websocket = true;
        protocol.websocket_close_code = close_code;
        protocol.websocket_close_reason = close_reason;
        protocol.websocket_closed_by_client = closed_by_client;
        self.upsert_flow_detail(&detail)?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{ImportedFlow, ImportedSession, WorkspaceReplacement};
    use core_model::{CaptureSession, FlowSummary, SessionStatus, TrafficSearchQuery, SCHEMA_VERSION};

    #[test]
    fn messages_search_and_replace_rollback() {
        let root = std::env::temp_dir().join(format!("mas-websocket-{}-{}", std::process::id(), std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir_all(&root).unwrap();
        let database = Database::open(root.join("app.db")).unwrap();
        let session = CaptureSession { schema_version: SCHEMA_VERSION, id: "old".into(), name: "Old".into(), status: SessionStatus::Completed,
            started_at: "1".into(), ended_at: Some("2".into()), device_id: None, app_id: None, connection_strategy: None,
            capture_engine: None, notes: None, capture_target: None, capture_mode: None };
        database.create_session(&session).unwrap();
        let mut flow = FlowSummary::fixture("old-flow", "GET", "example.test", "/safe?token=secret", 101, 1, 0, "1");
        flow.session_id = Some(session.id.clone());
        database.upsert_flow(&flow).unwrap();
        let old_message = WebSocketMessage { id: "old-msg".into(), flow_id: flow.id.clone(), session_id: flow.session_id.clone(),
            sequence: 1, from_client: true, opcode: 1, timestamp: "2".into(), dropped: false, injected: false, body: None };
        database.upsert_websocket_message(&old_message, "hello world").unwrap();
        database.upsert_flow_search_text(&flow.id, "safe response").unwrap();
        assert_eq!(database.list_websocket_messages(Some(&flow.id), None, Some("hello"), 20, 0).unwrap(), vec![old_message.clone()]);
        assert!(database.search_flows(&TrafficSearchQuery { text: Some("secret".into()), ..Default::default() }).unwrap().is_empty());
        assert_eq!(database.search_flows(&TrafficSearchQuery { text: Some("hello".into()), ..Default::default() }).unwrap().len(), 1);

        let replacement_session = CaptureSession { id: "new".into(), ..session };
        let mut replacement_flow = FlowSummary::fixture("new-flow", "GET", "example.test", "/new", 101, 1, 0, "3");
        replacement_flow.session_id = Some("new".into());
        let mut new_message = WebSocketMessage { id: "new-msg".into(), flow_id: "missing-flow".into(), session_id: Some("new".into()), ..old_message.clone() };
        let make_replacement = |message: WebSocketMessage| WorkspaceReplacement {
            sessions: vec![ImportedSession { session: replacement_session.clone(), flows: vec![ImportedFlow { summary: replacement_flow.clone(), detail: None }] }],
            collections: vec![], saved_requests: vec![], environments: vec![], environment_variables: vec![], proxy_rules: vec![], websocket_messages: vec![message],
        };
        assert!(database.replace_workspace(&make_replacement(new_message.clone())).is_err());
        assert_eq!(database.list_sessions(10).unwrap()[0].id, "old");
        assert_eq!(database.list_websocket_messages(None, None, None, 10, 0).unwrap(), vec![old_message]);
        new_message.flow_id = replacement_flow.id.clone();
        database.replace_workspace(&make_replacement(new_message.clone())).unwrap();
        assert_eq!(database.list_sessions(10).unwrap()[0].id, "new");
        assert_eq!(database.list_websocket_messages(None, None, None, 10, 0).unwrap(), vec![new_message]);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}

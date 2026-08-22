use rusqlite::{params, Connection, OptionalExtension};
use sdk_protocol::{SdkEnvelope, SdkEvent, SdkHandshake, SdkPlatform};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

const MIGRATION_001: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS sdk_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sdk_clients (
    client_id TEXT PRIMARY KEY,
    app_id TEXT NOT NULL,
    app_name TEXT NOT NULL,
    app_version TEXT,
    app_build TEXT,
    platform TEXT NOT NULL,
    device_name TEXT,
    os_version TEXT,
    sdk_version TEXT NOT NULL,
    first_seen_at TEXT NOT NULL,
    last_seen_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS sdk_events (
    event_id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    client_id TEXT NOT NULL,
    event_kind TEXT NOT NULL,
    request_id TEXT,
    occurred_at TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_sdk_events_request_id
    ON sdk_events(request_id, occurred_at, event_id);
CREATE INDEX IF NOT EXISTS idx_sdk_events_client_time
    ON sdk_events(client_id, occurred_at, event_id);
CREATE INDEX IF NOT EXISTS idx_sdk_events_kind
    ON sdk_events(event_kind, occurred_at);
CREATE INDEX IF NOT EXISTS idx_sdk_clients_last_seen
    ON sdk_clients(last_seen_at DESC);

INSERT OR IGNORE INTO sdk_schema_migrations(version) VALUES (1);
"#;

#[derive(Debug, Clone)]
pub struct SdkDatabase {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SdkClientRecord {
    pub client_id: String,
    pub app_id: String,
    pub app_name: String,
    pub app_version: Option<String>,
    pub app_build: Option<String>,
    pub platform: SdkPlatform,
    pub device_name: Option<String>,
    pub os_version: Option<String>,
    pub sdk_version: String,
    pub first_seen_at: String,
    pub last_seen_at: String,
}

impl SdkDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, SdkStorageError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let database = Self { path };
        database.initialize()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<(), SdkStorageError> {
        self.connection()?.execute_batch(MIGRATION_001)?;
        Ok(())
    }

    pub fn record(&self, envelope: &SdkEnvelope) -> Result<(), SdkStorageError> {
        envelope.validate().map_err(SdkStorageError::Protocol)?;
        let json = serde_json::to_string(envelope)?;
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT OR IGNORE INTO sdk_events (
                event_id, schema_version, client_id, event_kind, request_id, occurred_at, payload_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            "#,
            params![
                &envelope.event_id,
                i64::from(envelope.schema_version),
                envelope.event.client_id(),
                envelope.event.kind(),
                envelope.event.request_id(),
                &envelope.occurred_at,
                json,
            ],
        )?;

        if let SdkEvent::Handshake(handshake) = &envelope.event {
            self.upsert_client_with_connection(&connection, handshake, &envelope.occurred_at)?;
        } else {
            connection.execute(
                "UPDATE sdk_clients SET last_seen_at = ?2 WHERE client_id = ?1",
                params![envelope.event.client_id(), &envelope.occurred_at],
            )?;
        }
        Ok(())
    }

    pub fn list_clients(&self) -> Result<Vec<SdkClientRecord>, SdkStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT client_id, app_id, app_name, app_version, app_build, platform,
                   device_name, os_version, sdk_version, first_seen_at, last_seen_at
            FROM sdk_clients
            ORDER BY last_seen_at DESC, app_name COLLATE NOCASE ASC, client_id ASC
            "#,
        )?;
        let rows = statement.query_map([], map_client)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(SdkStorageError::from)
    }

    pub fn get_client(&self, client_id: &str) -> Result<Option<SdkClientRecord>, SdkStorageError> {
        self.connection()?
            .query_row(
                r#"
                SELECT client_id, app_id, app_name, app_version, app_build, platform,
                       device_name, os_version, sdk_version, first_seen_at, last_seen_at
                FROM sdk_clients WHERE client_id = ?1
                "#,
                [client_id],
                map_client,
            )
            .optional()
            .map_err(SdkStorageError::from)
    }

    pub fn events_for_request(&self, request_id: &str) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
        self.query_events(
            "SELECT payload_json FROM sdk_events WHERE request_id = ?1 ORDER BY occurred_at ASC, event_id ASC",
            [request_id],
        )
    }

    pub fn events_for_client(
        &self,
        client_id: &str,
        limit: usize,
    ) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT payload_json FROM sdk_events
            WHERE client_id = ?1
            ORDER BY occurred_at DESC, event_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![client_id, limit.min(2_000) as i64], |row| row.get::<_, String>(0))?;
        decode_events(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn recent_context_events(
        &self,
        client_id: &str,
        around_ms: u128,
        window_ms: u128,
        limit: usize,
    ) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
        let lower = around_ms.saturating_sub(window_ms).to_string();
        let upper = around_ms.saturating_add(window_ms).to_string();
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT payload_json FROM sdk_events
            WHERE client_id = ?1
              AND occurred_at >= ?2
              AND occurred_at <= ?3
              AND event_kind IN ('context', 'log', 'network')
            ORDER BY occurred_at ASC, event_id ASC
            LIMIT ?4
            "#,
        )?;
        let rows = statement.query_map(
            params![client_id, lower, upper, limit.min(2_000) as i64],
            |row| row.get::<_, String>(0),
        )?;
        decode_events(rows.collect::<Result<Vec<_>, _>>()?)
    }

    pub fn search_events(&self, text: &str, limit: usize) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
        let needle = format!("%{}%", text.trim());
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT payload_json FROM sdk_events
            WHERE payload_json LIKE ?1
            ORDER BY occurred_at DESC, event_id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![needle, limit.min(2_000) as i64], |row| row.get::<_, String>(0))?;
        decode_events(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn query_events<const N: usize>(
        &self,
        sql: &str,
        params_array: [&str; N],
    ) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(sql)?;
        let rows = statement.query_map(rusqlite::params_from_iter(params_array), |row| row.get::<_, String>(0))?;
        decode_events(rows.collect::<Result<Vec<_>, _>>()?)
    }

    fn upsert_client_with_connection(
        &self,
        connection: &Connection,
        handshake: &SdkHandshake,
        occurred_at: &str,
    ) -> Result<(), SdkStorageError> {
        connection.execute(
            r#"
            INSERT INTO sdk_clients (
                client_id, app_id, app_name, app_version, app_build, platform,
                device_name, os_version, sdk_version, first_seen_at, last_seen_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10)
            ON CONFLICT(client_id) DO UPDATE SET
                app_id = excluded.app_id,
                app_name = excluded.app_name,
                app_version = excluded.app_version,
                app_build = excluded.app_build,
                platform = excluded.platform,
                device_name = excluded.device_name,
                os_version = excluded.os_version,
                sdk_version = excluded.sdk_version,
                last_seen_at = excluded.last_seen_at
            "#,
            params![
                &handshake.client_id,
                &handshake.app_id,
                &handshake.app_name,
                &handshake.app_version,
                &handshake.app_build,
                platform_string(&handshake.platform),
                &handshake.device_name,
                &handshake.os_version,
                &handshake.sdk_version,
                occurred_at,
            ],
        )?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, SdkStorageError> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}

fn map_client(row: &rusqlite::Row<'_>) -> rusqlite::Result<SdkClientRecord> {
    let platform: String = row.get(5)?;
    Ok(SdkClientRecord {
        client_id: row.get(0)?,
        app_id: row.get(1)?,
        app_name: row.get(2)?,
        app_version: row.get(3)?,
        app_build: row.get(4)?,
        platform: parse_platform(&platform),
        device_name: row.get(6)?,
        os_version: row.get(7)?,
        sdk_version: row.get(8)?,
        first_seen_at: row.get(9)?,
        last_seen_at: row.get(10)?,
    })
}

fn decode_events(values: Vec<String>) -> Result<Vec<SdkEnvelope>, SdkStorageError> {
    values
        .into_iter()
        .map(|value| serde_json::from_str(&value).map_err(SdkStorageError::from))
        .collect()
}

fn platform_string(platform: &SdkPlatform) -> &'static str {
    match platform {
        SdkPlatform::Ios => "ios",
        SdkPlatform::Android => "android",
        SdkPlatform::Other => "other",
    }
}

fn parse_platform(value: &str) -> SdkPlatform {
    match value {
        "ios" => SdkPlatform::Ios,
        "android" => SdkPlatform::Android,
        _ => SdkPlatform::Other,
    }
}

#[derive(Debug)]
pub enum SdkStorageError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
    Protocol(sdk_protocol::SdkProtocolError),
}

impl std::fmt::Display for SdkStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::Protocol(error) => write!(formatter, "SDK protocol error: {error}"),
        }
    }
}

impl std::error::Error for SdkStorageError {}
impl From<rusqlite::Error> for SdkStorageError { fn from(value: rusqlite::Error) -> Self { Self::Sqlite(value) } }
impl From<std::io::Error> for SdkStorageError { fn from(value: std::io::Error) -> Self { Self::Io(value) } }
impl From<serde_json::Error> for SdkStorageError { fn from(value: serde_json::Error) -> Self { Self::Json(value) } }

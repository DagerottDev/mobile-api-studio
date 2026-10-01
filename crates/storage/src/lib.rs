mod detail;
mod import;
mod proxy_rules;
mod workflow;

pub use import::{ImportedFlow, ImportedSession, WorkspaceReplacement};

use core_model::{CaptureSession, FlowSource, FlowSummary, SessionStatus};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};
use std::{
    fs, io,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MIGRATION_001: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL DEFAULT 1,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'active',
    started_at TEXT NOT NULL,
    ended_at TEXT,
    device_id TEXT,
    app_id TEXT,
    connection_strategy TEXT,
    capture_engine TEXT,
    notes TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS flows (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    session_id TEXT,
    source TEXT NOT NULL,
    method TEXT NOT NULL,
    host TEXT NOT NULL,
    path TEXT NOT NULL,
    status_code INTEGER,
    duration_ms INTEGER,
    response_size_bytes INTEGER,
    started_at TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(session_id) REFERENCES sessions(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS bodies (
    sha256 TEXT PRIMARY KEY,
    byte_size INTEGER NOT NULL,
    content_type TEXT,
    encoding TEXT,
    is_binary INTEGER NOT NULL DEFAULT 0,
    is_truncated INTEGER NOT NULL DEFAULT 0,
    stored_path TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS headers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    flow_id TEXT NOT NULL,
    side TEXT NOT NULL,
    name TEXT NOT NULL,
    value TEXT NOT NULL,
    is_sensitive INTEGER NOT NULL DEFAULT 0,
    ordinal INTEGER NOT NULL,
    FOREIGN KEY(flow_id) REFERENCES flows(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_sessions_started_at
    ON sessions(started_at DESC);
CREATE INDEX IF NOT EXISTS idx_flows_session_started_at
    ON flows(session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_flows_host
    ON flows(host);
CREATE INDEX IF NOT EXISTS idx_flows_status_code
    ON flows(status_code);
CREATE INDEX IF NOT EXISTS idx_flows_method
    ON flows(method);
CREATE INDEX IF NOT EXISTS idx_headers_flow_side
    ON headers(flow_id, side, ordinal);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
"#;

const MIGRATION_004: &str = r#"
ALTER TABLE sessions ADD COLUMN capture_target_json TEXT;
ALTER TABLE sessions ADD COLUMN capture_mode_json TEXT;
INSERT INTO schema_migrations(version) VALUES (4);
"#;

#[derive(Debug, Clone)]
pub struct Database {
    path: PathBuf,
}

impl Database {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        if path.is_file() {
            let connection = Connection::open(&path)?;
            let has_migrations: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'schema_migrations')",
                [],
                |row| row.get(0),
            )?;
            let migrated: bool = if has_migrations {
                connection.query_row(
                    "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 5)",
                    [],
                    |row| row.get(0),
                )?
            } else {
                false
            };
            if !migrated {
                let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_nanos();
                let backup = format!("{}.pre-proxy-rules-{stamp}.bak", path.display());
                #[cfg(unix)]
                {
                    use std::os::unix::fs::OpenOptionsExt;
                    fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&backup)?;
                }
                connection.execute("VACUUM INTO ?1", [&backup])?;
            }
        }

        let database = Self { path };
        database.initialize()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn initialize(&self) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute_batch(MIGRATION_001)?;
        drop(connection);
        detail::initialize(self)?;
        workflow::initialize(self)?;
        let mut connection = self.connection()?;
        let migrated: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 4)",
            [],
            |row| row.get(0),
        )?;
        if !migrated {
            let migration = connection.transaction()?;
            migration.execute_batch(MIGRATION_004)?;
            migration.commit()?;
        }
        drop(connection);
        proxy_rules::initialize(self)?;
        Ok(())
    }

    pub fn create_session(&self, session: &CaptureSession) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let capture_target_json = session
            .capture_target
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        let capture_mode_json = session
            .capture_mode
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        connection.execute(
            r#"
            INSERT INTO sessions (
                id,
                schema_version,
                name,
                status,
                started_at,
                ended_at,
                device_id,
                app_id,
                connection_strategy,
                capture_engine,
                notes,
                capture_target_json,
                capture_mode_json
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                status = excluded.status,
                ended_at = excluded.ended_at,
                device_id = excluded.device_id,
                app_id = excluded.app_id,
                connection_strategy = excluded.connection_strategy,
                capture_engine = excluded.capture_engine,
                notes = excluded.notes,
                capture_target_json = excluded.capture_target_json,
                capture_mode_json = excluded.capture_mode_json
            "#,
            params![
                &session.id,
                i64::from(session.schema_version),
                &session.name,
                session_status_to_str(&session.status),
                &session.started_at,
                &session.ended_at,
                &session.device_id,
                &session.app_id,
                &session.connection_strategy,
                &session.capture_engine,
                &session.notes,
                capture_target_json,
                capture_mode_json,
            ],
        )?;
        Ok(())
    }

    pub fn complete_session(&self, id: &str, ended_at: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE sessions SET status = 'completed', ended_at = ?2 WHERE id = ?1",
            params![id, ended_at],
        )?;
        Ok(())
    }

    pub fn attribute_session_from_request_header(
        &self,
        header_name: &str,
        header_value: &str,
        app_id: &str,
    ) -> Result<usize, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            r#"
            UPDATE sessions
            SET app_id = ?3
            WHERE id IN (
                SELECT f.session_id
                FROM flows f
                JOIN headers h ON h.flow_id = f.id
                WHERE h.side = 'request'
                  AND h.name = ?1 COLLATE NOCASE
                  AND h.value = ?2
                  AND f.session_id IS NOT NULL
                ORDER BY f.started_at DESC
                LIMIT 1
            )
              AND (app_id IS NULL OR app_id = '' OR app_id = ?3)
            "#,
            params![header_name, header_value, app_id],
        )?;
        Ok(changed)
    }

    pub fn list_sessions(&self, limit: usize) -> Result<Vec<CaptureSession>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                schema_version,
                id,
                name,
                status,
                started_at,
                ended_at,
                device_id,
                app_id,
                connection_strategy,
                capture_engine,
                notes,
                capture_target_json,
                capture_mode_json
            FROM sessions
            ORDER BY started_at DESC
            LIMIT ?1
            "#,
        )?;

        let rows = statement.query_map([limit as i64], |row| {
            let schema_version: i64 = row.get(0)?;
            let status: String = row.get(3)?;
            let capture_target_json: Option<String> = row.get(11)?;
            let capture_mode_json: Option<String> = row.get(12)?;
            let capture_target = capture_target_json
                .map(|json| {
                    serde_json::from_str(&json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            11,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .transpose()?;
            let capture_mode = capture_mode_json
                .map(|json| {
                    serde_json::from_str(&json).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            12,
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })
                })
                .transpose()?;
            Ok(CaptureSession {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                name: row.get(2)?,
                status: session_status_from_str(&status),
                started_at: row.get(4)?,
                ended_at: row.get(5)?,
                device_id: row.get(6)?,
                app_id: row.get(7)?,
                connection_strategy: row.get(8)?,
                capture_engine: row.get(9)?,
                notes: row.get(10)?,
                capture_target,
                capture_mode,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn upsert_flow(&self, flow: &FlowSummary) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO flows (
                id,
                schema_version,
                session_id,
                source,
                method,
                host,
                path,
                status_code,
                duration_ms,
                response_size_bytes,
                started_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
            ON CONFLICT(id) DO UPDATE SET
                schema_version = excluded.schema_version,
                session_id = excluded.session_id,
                source = excluded.source,
                method = excluded.method,
                host = excluded.host,
                path = excluded.path,
                status_code = excluded.status_code,
                duration_ms = excluded.duration_ms,
                response_size_bytes = excluded.response_size_bytes,
                started_at = excluded.started_at
            "#,
            params![
                &flow.id,
                i64::from(flow.schema_version),
                &flow.session_id,
                flow_source_to_str(&flow.source),
                &flow.method,
                &flow.host,
                &flow.path,
                flow.status_code.map(i64::from),
                flow.duration_ms.map(|value| value as i64),
                flow.response_size_bytes.map(|value| value as i64),
                &flow.started_at,
            ],
        )?;
        workflow::upsert_endpoint_index(&connection, flow)?;
        Ok(())
    }

    pub fn list_flows(&self, limit: usize) -> Result<Vec<FlowSummary>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                schema_version,
                id,
                session_id,
                source,
                method,
                host,
                path,
                status_code,
                duration_ms,
                response_size_bytes,
                started_at
            FROM flows
            ORDER BY started_at DESC
            LIMIT ?1
            "#,
        )?;

        let rows = statement.query_map([limit as i64], |row| {
            let source: String = row.get(3)?;
            let schema_version: i64 = row.get(0)?;
            let status_code: Option<i64> = row.get(7)?;
            let duration_ms: Option<i64> = row.get(8)?;
            let response_size_bytes: Option<i64> = row.get(9)?;

            Ok(FlowSummary {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                session_id: row.get(2)?,
                source: flow_source_from_str(&source),
                method: row.get(4)?,
                host: row.get(5)?,
                path: row.get(6)?,
                status_code: status_code.map(|value| value as u16),
                duration_ms: duration_ms.map(|value| value as u64),
                response_size_bytes: response_size_bytes.map(|value| value as u64),
                started_at: row.get(10)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>()
            .map_err(StorageError::from)
    }

    pub fn is_empty(&self) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let count: i64 =
            connection.query_row("SELECT COUNT(*) FROM flows", [], |row| row.get(0))?;
        Ok(count == 0)
    }

    pub(crate) fn connection(&self) -> Result<Connection, StorageError> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}

#[derive(Debug, Clone)]
pub struct BodyStore {
    root: PathBuf,
}

impl BodyStore {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, StorageError> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn put(&self, bytes: &[u8]) -> Result<StoredBody, StorageError> {
        let sha256 = format!("{:x}", Sha256::digest(bytes));
        let directory = self.root.join(&sha256[0..2]).join(&sha256[2..4]);
        fs::create_dir_all(&directory)?;

        let path = directory.join(format!("{sha256}.body"));
        if !path.exists() {
            fs::write(&path, bytes)?;
        }

        Ok(StoredBody {
            sha256,
            byte_size: bytes.len() as u64,
            path,
        })
    }

    pub fn read(&self, sha256: &str) -> Result<Vec<u8>, StorageError> {
        if sha256.len() != 64 || !sha256.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err(StorageError::InvalidBodyHash);
        }

        let path = self
            .root
            .join(&sha256[0..2])
            .join(&sha256[2..4])
            .join(format!("{sha256}.body"));

        Ok(fs::read(path)?)
    }
}

#[cfg(test)]
mod body_hash_tests {
    use super::*;

    #[test]
    fn body_reads_require_a_hex_sha256_digest() {
        let root = std::env::temp_dir().join(format!(
            "mas-body-hash-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let store = BodyStore::new(&root).unwrap();
        let non_hex = "g".repeat(64);
        for invalid in ["../../outside", "🔥🔥", non_hex.as_str()] {
            assert!(matches!(
                store.read(invalid),
                Err(StorageError::InvalidBodyHash)
            ));
        }
        let stored = store.put(b"sample body").unwrap();
        assert_eq!(store.read(&stored.sha256).unwrap(), b"sample body");
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod capture_metadata_migration_tests {
    use super::*;
    use core_model::{
        CaptureMode, CaptureModeKind, CaptureTarget, CaptureTargetKind, SCHEMA_VERSION,
    };

    #[test]
    fn old_sessions_survive_capture_metadata_migration() {
        let root = std::env::temp_dir().join(format!(
            "mas-capture-migration-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("app.db");
        let old = Connection::open(&path).unwrap();
        old.execute_batch(MIGRATION_001).unwrap();
        old.execute(
            "INSERT INTO sessions (id, name, started_at) VALUES ('old', 'Old session', '1')",
            [],
        )
        .unwrap();
        drop(old);

        let database = Database::open(&path).unwrap();
        let old_session = database.list_sessions(10).unwrap().remove(0);
        assert_eq!(old_session.id, "old");
        assert!(old_session.capture_target.is_none());
        assert!(fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("pre-proxy-rules")
        }));

        let mut current = old_session;
        current.id = "new".into();
        current.schema_version = SCHEMA_VERSION;
        current.capture_target = Some(CaptureTarget {
            schema_version: 1,
            kind: CaptureTargetKind::MacAll,
        });
        current.capture_mode = Some(CaptureMode {
            schema_version: 1,
            kind: CaptureModeKind::LocalAll,
        });
        database.create_session(&current).unwrap();
        assert_eq!(
            database
                .list_sessions(10)
                .unwrap()
                .into_iter()
                .find(|session| session.id == "new")
                .unwrap(),
            current
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[derive(Debug, Clone)]
pub struct StoredBody {
    pub sha256: String,
    pub byte_size: u64,
    pub path: PathBuf,
}

#[derive(Debug)]
pub enum StorageError {
    Sqlite(rusqlite::Error),
    Io(io::Error),
    Json(serde_json::Error),
    InvalidBodyHash,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
            Self::InvalidBodyHash => write!(formatter, "invalid body hash"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<rusqlite::Error> for StorageError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<io::Error> for StorageError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(value: serde_json::Error) -> Self {
        Self::Json(value)
    }
}

pub(crate) fn flow_source_to_str(source: &FlowSource) -> &'static str {
    match source {
        FlowSource::Proxy => "proxy",
        FlowSource::Replay => "replay",
        FlowSource::Mock => "mock",
        FlowSource::Sdk => "sdk",
        FlowSource::Fixture => "fixture",
    }
}

pub(crate) fn flow_source_from_str(value: &str) -> FlowSource {
    match value {
        "proxy" => FlowSource::Proxy,
        "replay" => FlowSource::Replay,
        "mock" => FlowSource::Mock,
        "sdk" => FlowSource::Sdk,
        _ => FlowSource::Fixture,
    }
}

pub(crate) fn session_status_to_str(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::Completed => "completed",
        SessionStatus::Interrupted => "interrupted",
        SessionStatus::Archived => "archived",
    }
}

pub(crate) fn session_status_from_str(value: &str) -> SessionStatus {
    match value {
        "completed" => SessionStatus::Completed,
        "interrupted" => SessionStatus::Interrupted,
        "archived" => SessionStatus::Archived,
        _ => SessionStatus::Active,
    }
}

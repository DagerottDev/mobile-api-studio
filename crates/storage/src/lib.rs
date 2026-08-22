use core_model::{FlowSource, FlowSummary};
use rusqlite::{params, Connection};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io,
    path::{Path, PathBuf},
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
    name TEXT NOT NULL,
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

CREATE INDEX IF NOT EXISTS idx_flows_session_started_at
    ON flows(session_id, started_at DESC);
CREATE INDEX IF NOT EXISTS idx_flows_host
    ON flows(host);
CREATE INDEX IF NOT EXISTS idx_flows_status_code
    ON flows(status_code);
CREATE INDEX IF NOT EXISTS idx_flows_method
    ON flows(method);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (1);
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

        let database = Self { path };
        database.initialize()?;
        Ok(database)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn initialize(&self) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute_batch(MIGRATION_001)?;
        Ok(())
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
            ) VALUES (?1, ?2, NULL, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                schema_version = excluded.schema_version,
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
        Ok(())
    }

    pub fn list_flows(&self, limit: usize) -> Result<Vec<FlowSummary>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT
                schema_version,
                id,
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
            let source: String = row.get(2)?;
            let schema_version: i64 = row.get(0)?;
            let status_code: Option<i64> = row.get(6)?;
            let duration_ms: Option<i64> = row.get(7)?;
            let response_size_bytes: Option<i64> = row.get(8)?;

            Ok(FlowSummary {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                source: flow_source_from_str(&source),
                method: row.get(3)?,
                host: row.get(4)?,
                path: row.get(5)?,
                status_code: status_code.map(|value| value as u16),
                duration_ms: duration_ms.map(|value| value as u64),
                response_size_bytes: response_size_bytes.map(|value| value as u64),
                started_at: row.get(9)?,
            })
        })?;

        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn is_empty(&self) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM flows", [], |row| row.get(0))?;
        Ok(count == 0)
    }

    fn connection(&self) -> Result<Connection, StorageError> {
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
        if sha256.len() < 4 {
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
    InvalidBodyHash,
}

impl std::fmt::Display for StorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
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

fn flow_source_to_str(source: &FlowSource) -> &'static str {
    match source {
        FlowSource::Proxy => "proxy",
        FlowSource::Replay => "replay",
        FlowSource::Mock => "mock",
        FlowSource::Sdk => "sdk",
        FlowSource::Fixture => "fixture",
    }
}

fn flow_source_from_str(value: &str) -> FlowSource {
    match value {
        "proxy" => FlowSource::Proxy,
        "replay" => FlowSource::Replay,
        "mock" => FlowSource::Mock,
        "sdk" => FlowSource::Sdk,
        _ => FlowSource::Fixture,
    }
}

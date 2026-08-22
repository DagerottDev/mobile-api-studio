use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

const MIGRATION_001: &str = r#"
CREATE TABLE IF NOT EXISTS ai_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS ai_results (
    id TEXT PRIMARY KEY,
    task_kind TEXT NOT NULL,
    source_ref TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    context_fingerprint TEXT NOT NULL,
    remote_response_id TEXT,
    output_text TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_ai_results_source_created
    ON ai_results(source_ref, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_ai_results_fingerprint
    ON ai_results(context_fingerprint, created_at DESC);

INSERT OR IGNORE INTO ai_schema_migrations(version) VALUES (1);
"#;

#[derive(Debug, Clone)]
pub struct AiDatabase {
    path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct AiResultRecord {
    pub id: String,
    pub task_kind: String,
    pub source_ref: String,
    pub provider: String,
    pub model: String,
    pub context_fingerprint: String,
    pub remote_response_id: Option<String>,
    pub output_text: String,
    pub created_at: String,
}

impl AiDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, AiStorageError> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let database = Self { path };
        database.connection()?.execute_batch(MIGRATION_001)?;
        Ok(database)
    }

    pub fn path(&self) -> &Path { &self.path }

    pub fn insert(&self, record: &AiResultRecord) -> Result<(), AiStorageError> {
        self.connection()?.execute(
            r#"
            INSERT INTO ai_results (
                id, task_kind, source_ref, provider, model, context_fingerprint,
                remote_response_id, output_text, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            "#,
            params![
                &record.id,
                &record.task_kind,
                &record.source_ref,
                &record.provider,
                &record.model,
                &record.context_fingerprint,
                &record.remote_response_id,
                &record.output_text,
                &record.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_for_source(&self, source_ref: &str, limit: usize) -> Result<Vec<AiResultRecord>, AiStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT id, task_kind, source_ref, provider, model, context_fingerprint,
                   remote_response_id, output_text, created_at
            FROM ai_results
            WHERE source_ref = ?1
            ORDER BY created_at DESC, id DESC
            LIMIT ?2
            "#,
        )?;
        let rows = statement.query_map(params![source_ref, limit.clamp(1, 100) as i64], map_record)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(AiStorageError::from)
    }

    fn connection(&self) -> Result<Connection, AiStorageError> {
        Ok(Connection::open(&self.path)?)
    }
}

fn map_record(row: &rusqlite::Row<'_>) -> rusqlite::Result<AiResultRecord> {
    Ok(AiResultRecord {
        id: row.get(0)?,
        task_kind: row.get(1)?,
        source_ref: row.get(2)?,
        provider: row.get(3)?,
        model: row.get(4)?,
        context_fingerprint: row.get(5)?,
        remote_response_id: row.get(6)?,
        output_text: row.get(7)?,
        created_at: row.get(8)?,
    })
}

#[derive(Debug)]
pub enum AiStorageError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
}

impl std::fmt::Display for AiStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
        }
    }
}

impl std::error::Error for AiStorageError {}

impl From<rusqlite::Error> for AiStorageError {
    fn from(value: rusqlite::Error) -> Self { Self::Sqlite(value) }
}

impl From<std::io::Error> for AiStorageError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}

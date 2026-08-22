use mock_core::MockRule;
use rusqlite::{params, Connection, OptionalExtension};
use std::{fs, path::{Path, PathBuf}};

const MIGRATION_001: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS mock_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS mock_rules (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    rule_json TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1,
    priority INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mock_rules_enabled_priority
    ON mock_rules(enabled, priority, created_at, id);

INSERT OR IGNORE INTO mock_schema_migrations(version) VALUES (1);
"#;

#[derive(Debug, Clone)]
pub struct MockDatabase {
    path: PathBuf,
}

impl MockDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MockStorageError> {
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

    pub fn initialize(&self) -> Result<(), MockStorageError> {
        self.connection()?.execute_batch(MIGRATION_001)?;
        Ok(())
    }

    pub fn upsert_rule(&self, rule: &MockRule) -> Result<(), MockStorageError> {
        let json = serde_json::to_string(rule)?;
        self.connection()?.execute(
            r#"
            INSERT INTO mock_rules (
                id, schema_version, rule_json, enabled, priority, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                schema_version = excluded.schema_version,
                rule_json = excluded.rule_json,
                enabled = excluded.enabled,
                priority = excluded.priority,
                updated_at = excluded.updated_at
            "#,
            params![
                &rule.id,
                i64::from(rule.schema_version),
                json,
                rule.enabled,
                rule.priority,
                &rule.created_at,
                &rule.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_rules(&self) -> Result<Vec<MockRule>, MockStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT rule_json, enabled, priority
            FROM mock_rules
            ORDER BY priority ASC, created_at ASC, id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, bool>(1)?,
                row.get::<_, i64>(2)?,
            ))
        })?;
        let raw = rows.collect::<Result<Vec<_>, _>>()?;
        raw.into_iter()
            .map(|(json, enabled, priority)| {
                let mut rule: MockRule = serde_json::from_str(&json)?;
                rule.enabled = enabled;
                rule.priority = priority;
                Ok(rule)
            })
            .collect()
    }

    pub fn get_rule(&self, id: &str) -> Result<Option<MockRule>, MockStorageError> {
        let raw: Option<(String, bool, i64)> = self
            .connection()?
            .query_row(
                "SELECT rule_json, enabled, priority FROM mock_rules WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()?;
        raw.map(|(json, enabled, priority)| {
            let mut rule: MockRule = serde_json::from_str(&json)?;
            rule.enabled = enabled;
            rule.priority = priority;
            Ok(rule)
        })
        .transpose()
    }

    pub fn set_enabled(&self, id: &str, enabled: bool) -> Result<(), MockStorageError> {
        self.connection()?.execute(
            "UPDATE mock_rules SET enabled = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    pub fn set_priority(&self, id: &str, priority: i64) -> Result<(), MockStorageError> {
        self.connection()?.execute(
            "UPDATE mock_rules SET priority = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, priority],
        )?;
        Ok(())
    }

    pub fn delete_rule(&self, id: &str) -> Result<(), MockStorageError> {
        self.connection()?.execute("DELETE FROM mock_rules WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn disable_all(&self) -> Result<usize, MockStorageError> {
        Ok(self.connection()?.execute(
            "UPDATE mock_rules SET enabled = 0, updated_at = CURRENT_TIMESTAMP WHERE enabled != 0",
            [],
        )?)
    }

    fn connection(&self) -> Result<Connection, MockStorageError> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}

#[derive(Debug)]
pub enum MockStorageError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for MockStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
        }
    }
}

impl std::error::Error for MockStorageError {}

impl From<rusqlite::Error> for MockStorageError {
    fn from(value: rusqlite::Error) -> Self { Self::Sqlite(value) }
}

impl From<std::io::Error> for MockStorageError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}

impl From<serde_json::Error> for MockStorageError {
    fn from(value: serde_json::Error) -> Self { Self::Json(value) }
}

use mock_core::{MockBodyOverride, MockHeaderMutation};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::{fs, path::{Path, PathBuf}};

pub const MOCK_FIXTURE_SCHEMA_VERSION: u16 = 1;

const MIGRATION_001: &str = r#"
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS mock_fixture_schema_migrations (
    version INTEGER PRIMARY KEY,
    applied_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS mock_fixtures (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    name TEXT NOT NULL,
    fixture_json TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_mock_fixtures_name
    ON mock_fixtures(name COLLATE NOCASE, created_at, id);

INSERT OR IGNORE INTO mock_fixture_schema_migrations(version) VALUES (1);
"#;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MockFixture {
    pub schema_version: u16,
    pub id: String,
    pub name: String,
    pub status_code: u16,
    pub response_headers: Vec<MockHeaderMutation>,
    pub response_body: Option<MockBodyOverride>,
    pub source_flow_id: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone)]
pub struct MockFixtureDatabase {
    path: PathBuf,
}

impl MockFixtureDatabase {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, MockFixtureStorageError> {
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

    pub fn initialize(&self) -> Result<(), MockFixtureStorageError> {
        self.connection()?.execute_batch(MIGRATION_001)?;
        Ok(())
    }

    pub fn upsert(&self, fixture: &MockFixture) -> Result<(), MockFixtureStorageError> {
        let fixture_json = serde_json::to_string(fixture)?;
        self.connection()?.execute(
            r#"
            INSERT INTO mock_fixtures (
                id, schema_version, name, fixture_json, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                schema_version = excluded.schema_version,
                name = excluded.name,
                fixture_json = excluded.fixture_json,
                updated_at = excluded.updated_at
            "#,
            params![
                &fixture.id,
                i64::from(fixture.schema_version),
                &fixture.name,
                fixture_json,
                &fixture.created_at,
                &fixture.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list(&self) -> Result<Vec<MockFixture>, MockFixtureStorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT fixture_json FROM mock_fixtures ORDER BY name COLLATE NOCASE ASC, created_at ASC, id ASC",
        )?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|json| serde_json::from_str(&json).map_err(MockFixtureStorageError::from))
            .collect()
    }

    pub fn get(&self, id: &str) -> Result<Option<MockFixture>, MockFixtureStorageError> {
        let json: Option<String> = self
            .connection()?
            .query_row(
                "SELECT fixture_json FROM mock_fixtures WHERE id = ?1",
                [id],
                |row| row.get(0),
            )
            .optional()?;
        json.map(|value| serde_json::from_str(&value).map_err(MockFixtureStorageError::from))
            .transpose()
    }

    pub fn delete(&self, id: &str) -> Result<(), MockFixtureStorageError> {
        self.connection()?.execute("DELETE FROM mock_fixtures WHERE id = ?1", [id])?;
        Ok(())
    }

    fn connection(&self) -> Result<Connection, MockFixtureStorageError> {
        let connection = Connection::open(&self.path)?;
        connection.pragma_update(None, "foreign_keys", "ON")?;
        Ok(connection)
    }
}

#[derive(Debug)]
pub enum MockFixtureStorageError {
    Sqlite(rusqlite::Error),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for MockFixtureStorageError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Sqlite(error) => write!(formatter, "SQLite error: {error}"),
            Self::Io(error) => write!(formatter, "I/O error: {error}"),
            Self::Json(error) => write!(formatter, "JSON error: {error}"),
        }
    }
}

impl std::error::Error for MockFixtureStorageError {}

impl From<rusqlite::Error> for MockFixtureStorageError {
    fn from(value: rusqlite::Error) -> Self { Self::Sqlite(value) }
}

impl From<std::io::Error> for MockFixtureStorageError {
    fn from(value: std::io::Error) -> Self { Self::Io(value) }
}

impl From<serde_json::Error> for MockFixtureStorageError {
    fn from(value: serde_json::Error) -> Self { Self::Json(value) }
}

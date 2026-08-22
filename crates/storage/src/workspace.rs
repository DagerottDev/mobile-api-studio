use super::{flow_source_from_str, flow_source_to_str, Database, StorageError};
use core_model::{FlowSummary, SessionStatus};
use rusqlite::{params, OptionalExtension};
use workspace_core::{
    Collection, Environment, EnvironmentVariable, FlowSearchQuery, SavedRequest,
};

const MIGRATION_003: &str = r#"
CREATE TABLE IF NOT EXISTS collections (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    description TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS saved_requests (
    id TEXT PRIMARY KEY,
    collection_id TEXT NOT NULL,
    name TEXT NOT NULL,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json TEXT NOT NULL,
    body_json TEXT,
    source_flow_id TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(collection_id) REFERENCES collections(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS idx_saved_requests_collection
    ON saved_requests(collection_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS environment_variables (
    environment_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT,
    is_secret INTEGER NOT NULL DEFAULT 0,
    secret_ref TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY(environment_id, key),
    FOREIGN KEY(environment_id) REFERENCES environments(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS app_settings (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
"#;

#[derive(Debug, Clone)]
pub struct StoredEnvironmentVariable {
    pub variable: EnvironmentVariable,
    pub secret_ref: Option<String>,
}

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let connection = database.connection()?;
    connection.execute_batch(MIGRATION_003)?;
    Ok(())
}

impl Database {
    pub fn update_session_metadata(
        &self,
        id: &str,
        name: &str,
        notes: Option<&str>,
    ) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE sessions SET name = ?2, notes = ?3 WHERE id = ?1",
            params![id, name, notes],
        )?;
        Ok(changed > 0)
    }

    pub fn delete_session(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(changed > 0)
    }

    pub fn search_flows(&self, query: &FlowSearchQuery) -> Result<Vec<FlowSummary>, StorageError> {
        let connection = self.connection()?;
        let text = query
            .text
            .as_ref()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let session_id = query.session_id.clone().filter(|value| !value.is_empty());
        let method = query
            .method
            .as_ref()
            .map(|value| value.trim().to_ascii_uppercase())
            .filter(|value| !value.is_empty());
        let source = query.source.as_ref().map(flow_source_to_str);
        let status_min = query.status_class.map(|class| i64::from(class) * 100);
        let status_max = status_min.map(|minimum| minimum + 99);
        let min_duration = query.min_duration_ms.map(|value| value as i64);
        let max_duration = query.max_duration_ms.map(|value| value as i64);
        let limit = query.limit.clamp(1, 5_000) as i64;
        let offset = query.offset as i64;

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
            WHERE
                (?1 IS NULL OR lower(method || ' ' || host || path) LIKE '%' || lower(?1) || '%')
                AND (?2 IS NULL OR session_id = ?2)
                AND (?3 IS NULL OR method = ?3)
                AND (?4 IS NULL OR source = ?4)
                AND (?5 IS NULL OR status_code >= ?5)
                AND (?6 IS NULL OR status_code <= ?6)
                AND (?7 IS NULL OR duration_ms >= ?7)
                AND (?8 IS NULL OR duration_ms <= ?8)
            ORDER BY started_at DESC
            LIMIT ?9 OFFSET ?10
            "#,
        )?;

        let rows = statement.query_map(
            params![
                text,
                session_id,
                method,
                source,
                status_min,
                status_max,
                min_duration,
                max_duration,
                limit,
                offset,
            ],
            |row| {
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
            },
        )?;

        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn create_collection(
        &self,
        id: &str,
        name: &str,
        description: Option<&str>,
        timestamp: &str,
    ) -> Result<Collection, StorageError> {
        let collection = Collection {
            id: id.to_string(),
            name: name.to_string(),
            description: description.map(str::to_string),
            created_at: timestamp.to_string(),
            updated_at: timestamp.to_string(),
        };
        self.upsert_collection(&collection)?;
        Ok(collection)
    }

    pub fn upsert_collection(&self, collection: &Collection) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO collections (id, name, description, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                updated_at = excluded.updated_at
            "#,
            params![
                &collection.id,
                &collection.name,
                &collection.description,
                &collection.created_at,
                &collection.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_collections(&self) -> Result<Vec<Collection>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, description, created_at, updated_at FROM collections ORDER BY updated_at DESC, name ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Collection {
                id: row.get(0)?,
                name: row.get(1)?,
                description: row.get(2)?,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_collection(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM collections WHERE id = ?1", [id])? > 0)
    }

    pub fn upsert_saved_request(&self, request: &SavedRequest) -> Result<(), StorageError> {
        let headers_json = serde_json::to_string(&request.headers).map_err(StorageError::Json)?;
        let body_json = request
            .body
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(StorageError::Json)?;
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO saved_requests (
                id, collection_id, name, method, url, headers_json, body_json,
                source_flow_id, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            ON CONFLICT(id) DO UPDATE SET
                collection_id = excluded.collection_id,
                name = excluded.name,
                method = excluded.method,
                url = excluded.url,
                headers_json = excluded.headers_json,
                body_json = excluded.body_json,
                source_flow_id = excluded.source_flow_id,
                updated_at = excluded.updated_at
            "#,
            params![
                &request.id,
                &request.collection_id,
                &request.name,
                &request.method,
                &request.url,
                headers_json,
                body_json,
                &request.source_flow_id,
                &request.created_at,
                &request.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_saved_requests(&self, collection_id: &str) -> Result<Vec<SavedRequest>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT id, collection_id, name, method, url, headers_json, body_json,
                   source_flow_id, created_at, updated_at
            FROM saved_requests
            WHERE collection_id = ?1
            ORDER BY updated_at DESC, name ASC
            "#,
        )?;
        let rows = statement.query_map([collection_id], saved_request_from_row)?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn get_saved_request(&self, id: &str) -> Result<Option<SavedRequest>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                r#"
                SELECT id, collection_id, name, method, url, headers_json, body_json,
                       source_flow_id, created_at, updated_at
                FROM saved_requests WHERE id = ?1
                "#,
                [id],
                saved_request_from_row,
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn delete_saved_request(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM saved_requests WHERE id = ?1", [id])? > 0)
    }

    pub fn upsert_environment(&self, environment: &Environment) -> Result<(), StorageError> {
        let connection = self.connection()?;
        if environment.is_active {
            connection.execute("UPDATE environments SET is_active = 0", [])?;
        }
        connection.execute(
            r#"
            INSERT INTO environments (id, name, is_active, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                is_active = excluded.is_active,
                updated_at = excluded.updated_at
            "#,
            params![
                &environment.id,
                &environment.name,
                environment.is_active as i64,
                &environment.created_at,
                &environment.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, is_active, created_at, updated_at FROM environments ORDER BY is_active DESC, name ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(Environment {
                id: row.get(0)?,
                name: row.get(1)?,
                is_active: row.get::<_, i64>(2)? != 0,
                created_at: row.get(3)?,
                updated_at: row.get(4)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn get_active_environment(&self) -> Result<Option<Environment>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT id, name, is_active, created_at, updated_at FROM environments WHERE is_active = 1 LIMIT 1",
                [],
                |row| {
                    Ok(Environment {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        is_active: true,
                        created_at: row.get(3)?,
                        updated_at: row.get(4)?,
                    })
                },
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn delete_environment(&self, id: &str) -> Result<bool, StorageError> {
        let connection = self.connection()?;
        Ok(connection.execute("DELETE FROM environments WHERE id = ?1", [id])? > 0)
    }

    pub fn upsert_environment_variable(
        &self,
        variable: &EnvironmentVariable,
        secret_ref: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO environment_variables (
                environment_id, key, value, is_secret, secret_ref, enabled
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(environment_id, key) DO UPDATE SET
                value = excluded.value,
                is_secret = excluded.is_secret,
                secret_ref = excluded.secret_ref,
                enabled = excluded.enabled
            "#,
            params![
                &variable.environment_id,
                &variable.key,
                &variable.value,
                variable.is_secret as i64,
                secret_ref,
                variable.enabled as i64,
            ],
        )?;
        Ok(())
    }

    pub fn list_environment_variables(
        &self,
        environment_id: &str,
    ) -> Result<Vec<StoredEnvironmentVariable>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT environment_id, key, value, is_secret, secret_ref, enabled
            FROM environment_variables
            WHERE environment_id = ?1
            ORDER BY key COLLATE NOCASE ASC
            "#,
        )?;
        let rows = statement.query_map([environment_id], |row| {
            let is_secret = row.get::<_, i64>(3)? != 0;
            let secret_ref: Option<String> = row.get(4)?;
            Ok(StoredEnvironmentVariable {
                variable: EnvironmentVariable {
                    environment_id: row.get(0)?,
                    key: row.get(1)?,
                    value: if is_secret { None } else { row.get(2)? },
                    is_secret,
                    secret_present: is_secret && secret_ref.is_some(),
                    enabled: row.get::<_, i64>(5)? != 0,
                },
                secret_ref,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_environment_variable(
        &self,
        environment_id: &str,
        key: &str,
    ) -> Result<Option<String>, StorageError> {
        let connection = self.connection()?;
        let secret_ref: Option<String> = connection
            .query_row(
                "SELECT secret_ref FROM environment_variables WHERE environment_id = ?1 AND key = ?2",
                params![environment_id, key],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        connection.execute(
            "DELETE FROM environment_variables WHERE environment_id = ?1 AND key = ?2",
            params![environment_id, key],
        )?;
        Ok(secret_ref)
    }

    pub fn set_setting(&self, key: &str, value: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO app_settings (key, value) VALUES (?1, ?2)
            ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP
            "#,
            params![key, value],
        )?;
        Ok(())
    }

    pub fn get_setting(&self, key: &str) -> Result<Option<String>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row("SELECT value FROM app_settings WHERE key = ?1", [key], |row| row.get(0))
            .optional()
            .map_err(StorageError::from)
    }
}

fn saved_request_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<SavedRequest> {
    let headers_json: String = row.get(5)?;
    let body_json: Option<String> = row.get(6)?;
    let headers = serde_json::from_str(&headers_json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            headers_json.len(),
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })?;
    let body = body_json
        .map(|json| {
            serde_json::from_str(&json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })
        })
        .transpose()?;

    Ok(SavedRequest {
        id: row.get(0)?,
        collection_id: row.get(1)?,
        name: row.get(2)?,
        method: row.get(3)?,
        url: row.get(4)?,
        headers,
        body,
        source_flow_id: row.get(7)?,
        created_at: row.get(8)?,
        updated_at: row.get(9)?,
    })
}

pub fn session_status_label(status: &SessionStatus) -> &'static str {
    match status {
        SessionStatus::Active => "active",
        SessionStatus::Completed => "completed",
        SessionStatus::Interrupted => "interrupted",
    }
}

use super::{flow_source_from_str, flow_source_to_str, session_status_from_str, Database, StorageError};
use core_model::{
    normalize_endpoint, AppPreference, CaptureSession, Environment, EnvironmentVariable, FlowSource,
    FlowSummary, OnboardingStep, SavedCollection, SavedRequest, SessionStatus, TrafficSearchQuery,
    TrafficSearchResult,
};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::{HashMap, HashSet};

const MIGRATION_003: &str = r#"
CREATE TABLE IF NOT EXISTS flow_endpoint_index (
    flow_id TEXT PRIMARY KEY,
    endpoint_key TEXT NOT NULL,
    method TEXT NOT NULL,
    host TEXT NOT NULL,
    path_template TEXT NOT NULL,
    FOREIGN KEY(flow_id) REFERENCES flows(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_flow_endpoint_key ON flow_endpoint_index(endpoint_key);

CREATE TABLE IF NOT EXISTS saved_collections (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS saved_requests (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    collection_id TEXT NOT NULL,
    name TEXT NOT NULL,
    method TEXT NOT NULL,
    url TEXT NOT NULL,
    headers_json TEXT NOT NULL,
    body_json TEXT,
    source_flow_id TEXT,
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(collection_id) REFERENCES saved_collections(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_saved_requests_collection ON saved_requests(collection_id, sort_order, created_at);

CREATE TABLE IF NOT EXISTS environments (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    name TEXT NOT NULL,
    is_active INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_environment_name ON environments(name COLLATE NOCASE);

CREATE TABLE IF NOT EXISTS environment_variables (
    id TEXT PRIMARY KEY,
    schema_version INTEGER NOT NULL,
    environment_id TEXT NOT NULL,
    key TEXT NOT NULL,
    value TEXT,
    is_secret INTEGER NOT NULL DEFAULT 0,
    secret_ref TEXT,
    enabled INTEGER NOT NULL DEFAULT 1,
    sort_order INTEGER NOT NULL DEFAULT 0,
    FOREIGN KEY(environment_id) REFERENCES environments(id) ON DELETE CASCADE,
    UNIQUE(environment_id, key)
);
CREATE INDEX IF NOT EXISTS idx_environment_variables_environment
    ON environment_variables(environment_id, sort_order, key);

CREATE TABLE IF NOT EXISTS app_preferences (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS onboarding_steps (
    key TEXT PRIMARY KEY,
    completed INTEGER NOT NULL DEFAULT 0,
    completed_at TEXT
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (3);
"#;

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let connection = database.connection()?;
    connection.execute_batch(MIGRATION_003)?;
    backfill_endpoint_index(&connection)?;
    Ok(())
}

pub(super) fn upsert_endpoint_index(
    connection: &Connection,
    flow: &FlowSummary,
) -> Result<(), StorageError> {
    let endpoint = normalize_endpoint(&flow.method, &flow.host, &flow.path);
    connection.execute(
        r#"
        INSERT INTO flow_endpoint_index (flow_id, endpoint_key, method, host, path_template)
        VALUES (?1, ?2, ?3, ?4, ?5)
        ON CONFLICT(flow_id) DO UPDATE SET
            endpoint_key = excluded.endpoint_key,
            method = excluded.method,
            host = excluded.host,
            path_template = excluded.path_template
        "#,
        params![
            &flow.id,
            endpoint.key,
            endpoint.method,
            endpoint.host,
            endpoint.path_template,
        ],
    )?;
    Ok(())
}

fn backfill_endpoint_index(connection: &Connection) -> Result<(), StorageError> {
    let mut statement = connection.prepare(
        r#"
        SELECT f.id, f.schema_version, f.session_id, f.source, f.method, f.host, f.path,
               f.status_code, f.duration_ms, f.response_size_bytes, f.started_at
        FROM flows f
        LEFT JOIN flow_endpoint_index e ON e.flow_id = f.id
        WHERE e.flow_id IS NULL
        "#,
    )?;
    let rows = statement.query_map([], |row| {
        let source: String = row.get(3)?;
        let schema_version: i64 = row.get(1)?;
        let status_code: Option<i64> = row.get(7)?;
        let duration_ms: Option<i64> = row.get(8)?;
        let response_size_bytes: Option<i64> = row.get(9)?;
        Ok(FlowSummary {
            id: row.get(0)?,
            schema_version: schema_version as u16,
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
    let flows = rows.collect::<Result<Vec<_>, _>>()?;
    drop(statement);
    for flow in &flows {
        upsert_endpoint_index(connection, flow)?;
    }
    Ok(())
}

impl Database {
    pub fn update_session_metadata(
        &self,
        id: &str,
        name: &str,
        notes: Option<&str>,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE sessions SET name = ?2, notes = ?3 WHERE id = ?1",
            params![id, name, notes],
        )?;
        Ok(())
    }

    pub fn archive_session(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("UPDATE sessions SET status = 'archived' WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn delete_session(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM sessions WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn search_flows(
        &self,
        query: &TrafficSearchQuery,
    ) -> Result<Vec<TrafficSearchResult>, StorageError> {
        self.search_flows_with_limit(query, query.limit.unwrap_or(500).clamp(1, 5_000))
    }

    pub fn sdk_flow_ids_matching(
        &self,
        text: &str,
        limit: usize,
    ) -> Result<Vec<String>, StorageError> {
        if text.trim().is_empty() {
            return Ok(Vec::new());
        }
        let query = TrafficSearchQuery {
            sdk_text: Some(text.into()),
            ..Default::default()
        };
        Ok(self
            .search_flows_with_limit(&query, limit.clamp(1, 10_000))?
            .into_iter()
            .map(|result| result.flow.id)
            .collect())
    }

    fn search_flows_with_limit(
        &self,
        query: &TrafficSearchQuery,
        limit: usize,
    ) -> Result<Vec<TrafficSearchResult>, StorageError> {
        let sdk_needle = query
            .sdk_text
            .as_deref()
            .map(str::trim)
            .filter(|text| !text.is_empty());
        let connection = self.connection()?;
        if sdk_needle.is_some() && !connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='sdk_events')", [], |row| row.get::<_, bool>(0))? {
            return Ok(Vec::new());
        }
        let sessions = self
            .list_sessions(10_000)?
            .into_iter()
            .map(|session| (session.id, session.name))
            .collect::<HashMap<_, _>>();
        let needle = query
            .text
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_lowercase);
        if needle.as_ref().is_some_and(|value| value.len() > 256) {
            return Err(StorageError::InvalidInput(
                "Traffic search text exceeds 256 bytes".into(),
            ));
        }
        let derived_matches: HashSet<String> = if let Some(needle) = needle.as_deref() {
            let connection = self.connection()?;
            let mut statement = connection.prepare(
                "SELECT f.id FROM flows f WHERE EXISTS (SELECT 1 FROM flow_search_text s WHERE s.flow_id=f.id AND instr(lower(s.redacted_text), ?1)>0) OR EXISTS (SELECT 1 FROM websocket_messages w WHERE w.flow_id=f.id AND instr(lower(w.search_text), ?1)>0)",
            )?;
            statement
                .query_map([needle], |row| row.get(0))?
                .collect::<Result<_, _>>()?
        } else {
            HashSet::new()
        };
        let method = query
            .method
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(str::to_uppercase);
        let endpoint_key = query
            .endpoint_key
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());

        let mut results = Vec::new();
        // Filter SDK metadata before streaming summaries; output limits never hide older matches.
        // The first case-insensitive correlation header has the same semantics as Inspector enrichment.
        let sdk_filter = if sdk_needle.is_some() {
            "WHERE EXISTS (SELECT 1 FROM flow_details d WHERE d.flow_id=f.id AND EXISTS (
                SELECT 1 FROM sdk_events e WHERE e.request_id=(
                    SELECT json_extract(h.value, '$.value') FROM json_each(d.detail_json, '$.request.headers') h
                    WHERE lower(json_extract(h.value, '$.name'))='x-mobile-api-studio-request-id'
                    ORDER BY CAST(h.key AS INTEGER) LIMIT 1
                ) AND instr(lower(e.payload_json), lower(?1))>0))"
        } else {
            "WHERE ?1 IS NULL"
        };
        let mut statement = connection.prepare(&format!(
            "SELECT f.id, f.schema_version, f.session_id, f.source, f.method, f.host, f.path,
             f.status_code, f.duration_ms, f.response_size_bytes, f.started_at
             FROM flows f {sdk_filter} ORDER BY f.started_at DESC"
        ))?;
        let rows = statement.query_map([sdk_needle], |row| {
            let source: String = row.get(3)?;
            Ok(FlowSummary {
                id: row.get(0)?,
                schema_version: row.get::<_, i64>(1)? as u16,
                session_id: row.get(2)?,
                source: flow_source_from_str(&source),
                method: row.get(4)?,
                host: row.get(5)?,
                path: row.get(6)?,
                status_code: row.get::<_, Option<i64>>(7)?.map(|value| value as u16),
                duration_ms: row.get::<_, Option<i64>>(8)?.map(|value| value as u64),
                response_size_bytes: row.get::<_, Option<i64>>(9)?.map(|value| value as u64),
                started_at: row.get(10)?,
            })
        })?;
        for flow in rows {
            let flow = flow?;
            if let Some(session_id) = query.session_id.as_deref() {
                if flow.session_id.as_deref() != Some(session_id) {
                    continue;
                }
            }
            if let Some(source) = query.source.as_ref() {
                if &flow.source != source {
                    continue;
                }
            }
            if let Some(method) = method.as_deref() {
                if flow.method.to_uppercase() != method {
                    continue;
                }
            }
            if let Some(status_class) = query.status_class {
                if flow.status_code.map(|status| status / 100) != Some(status_class) {
                    continue;
                }
            }

            let endpoint = normalize_endpoint(&flow.method, &flow.host, &flow.path);
            if let Some(expected) = endpoint_key {
                if endpoint.key != expected {
                    continue;
                }
            }

            let session_name = flow
                .session_id
                .as_ref()
                .and_then(|id| sessions.get(id))
                .cloned();
            if let Some(needle) = needle.as_deref() {
                let haystack = format!(
                    "{} {} {} {} {}",
                    flow.method,
                    flow.host,
                    flow.path.split('?').next().unwrap_or(&flow.path),
                    endpoint.path_template,
                    session_name.as_deref().unwrap_or_default(),
                )
                .to_lowercase();
                if !haystack.contains(needle) && !derived_matches.contains(&flow.id) {
                    continue;
                }
            }

            results.push(TrafficSearchResult {
                flow,
                endpoint,
                session_name,
            });
            if results.len() >= limit {
                break;
            }
        }
        Ok(results)
    }

    pub fn upsert_collection(&self, collection: &SavedCollection) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO saved_collections
                (id, schema_version, name, description, sort_order, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                description = excluded.description,
                sort_order = excluded.sort_order,
                updated_at = excluded.updated_at
            "#,
            params![
                &collection.id,
                i64::from(collection.schema_version),
                &collection.name,
                &collection.description,
                collection.sort_order,
                &collection.created_at,
                &collection.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_collections(&self) -> Result<Vec<SavedCollection>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT schema_version, id, name, description, sort_order, created_at, updated_at
            FROM saved_collections ORDER BY sort_order ASC, created_at ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            let schema_version: i64 = row.get(0)?;
            Ok(SavedCollection {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                name: row.get(2)?,
                description: row.get(3)?,
                sort_order: row.get(4)?,
                created_at: row.get(5)?,
                updated_at: row.get(6)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_collection(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM saved_collections WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn upsert_saved_request(&self, request: &SavedRequest) -> Result<(), StorageError> {
        let headers_json = serde_json::to_string(&request.headers)?;
        let body_json = request.body.as_ref().map(serde_json::to_string).transpose()?;
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO saved_requests (
                id, schema_version, collection_id, name, method, url, headers_json, body_json,
                source_flow_id, sort_order, created_at, updated_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
            ON CONFLICT(id) DO UPDATE SET
                collection_id = excluded.collection_id,
                name = excluded.name,
                method = excluded.method,
                url = excluded.url,
                headers_json = excluded.headers_json,
                body_json = excluded.body_json,
                source_flow_id = excluded.source_flow_id,
                sort_order = excluded.sort_order,
                updated_at = excluded.updated_at
            "#,
            params![
                &request.id,
                i64::from(request.schema_version),
                &request.collection_id,
                &request.name,
                &request.method,
                &request.url,
                headers_json,
                body_json,
                &request.source_flow_id,
                request.sort_order,
                &request.created_at,
                &request.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_saved_requests(
        &self,
        collection_id: Option<&str>,
    ) -> Result<Vec<SavedRequest>, StorageError> {
        let connection = self.connection()?;
        let sql = if collection_id.is_some() {
            r#"SELECT schema_version, id, collection_id, name, method, url, headers_json,
                      body_json, source_flow_id, sort_order, created_at, updated_at
               FROM saved_requests WHERE collection_id = ?1
               ORDER BY sort_order ASC, created_at ASC"#
        } else {
            r#"SELECT schema_version, id, collection_id, name, method, url, headers_json,
                      body_json, source_flow_id, sort_order, created_at, updated_at
               FROM saved_requests ORDER BY collection_id, sort_order ASC, created_at ASC"#
        };
        let mut statement = connection.prepare(sql)?;
        let map_row = |row: &rusqlite::Row<'_>| -> rusqlite::Result<(i64, String, String, String, String, String, String, Option<String>, Option<String>, i64, String, String)> {
            Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?, row.get(6)?, row.get(7)?, row.get(8)?, row.get(9)?, row.get(10)?, row.get(11)?))
        };
        let raw = if let Some(collection_id) = collection_id {
            statement.query_map([collection_id], map_row)?.collect::<Result<Vec<_>, _>>()?
        } else {
            statement.query_map([], map_row)?.collect::<Result<Vec<_>, _>>()?
        };
        raw.into_iter()
            .map(|(schema_version, id, collection_id, name, method, url, headers_json, body_json, source_flow_id, sort_order, created_at, updated_at)| {
                Ok(SavedRequest {
                    schema_version: schema_version as u16,
                    id,
                    collection_id,
                    name,
                    method,
                    url,
                    headers: serde_json::from_str(&headers_json)?,
                    body: body_json.map(|json| serde_json::from_str(&json)).transpose()?,
                    source_flow_id,
                    sort_order,
                    created_at,
                    updated_at,
                })
            })
            .collect()
    }

    pub fn delete_saved_request(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM saved_requests WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn upsert_environment(&self, environment: &Environment) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        if environment.is_active {
            transaction.execute("UPDATE environments SET is_active = 0", [])?;
        }
        transaction.execute(
            r#"
            INSERT INTO environments (id, schema_version, name, is_active, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6)
            ON CONFLICT(id) DO UPDATE SET
                name = excluded.name,
                is_active = excluded.is_active,
                updated_at = excluded.updated_at
            "#,
            params![
                &environment.id,
                i64::from(environment.schema_version),
                &environment.name,
                environment.is_active,
                &environment.created_at,
                &environment.updated_at,
            ],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn set_active_environment(&self, id: Option<&str>) -> Result<(), StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("UPDATE environments SET is_active = 0", [])?;
        if let Some(id) = id {
            transaction.execute("UPDATE environments SET is_active = 1 WHERE id = ?1", [id])?;
        }
        transaction.commit()?;
        Ok(())
    }

    pub fn list_environments(&self) -> Result<Vec<Environment>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT schema_version, id, name, is_active, created_at, updated_at FROM environments ORDER BY is_active DESC, name COLLATE NOCASE ASC",
        )?;
        let rows = statement.query_map([], |row| {
            let schema_version: i64 = row.get(0)?;
            Ok(Environment {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                name: row.get(2)?,
                is_active: row.get(3)?,
                created_at: row.get(4)?,
                updated_at: row.get(5)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_environment(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM environments WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn upsert_environment_variable(
        &self,
        variable: &EnvironmentVariable,
    ) -> Result<(), StorageError> {
        let connection = self.connection()?;
        let stored_value = if variable.is_secret { None } else { variable.value.as_deref() };
        connection.execute(
            r#"
            INSERT INTO environment_variables (
                id, schema_version, environment_id, key, value, is_secret, secret_ref, enabled, sort_order
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
            ON CONFLICT(id) DO UPDATE SET
                environment_id = excluded.environment_id,
                key = excluded.key,
                value = excluded.value,
                is_secret = excluded.is_secret,
                secret_ref = excluded.secret_ref,
                enabled = excluded.enabled,
                sort_order = excluded.sort_order
            "#,
            params![
                &variable.id,
                i64::from(variable.schema_version),
                &variable.environment_id,
                &variable.key,
                stored_value,
                variable.is_secret,
                &variable.secret_ref,
                variable.enabled,
                variable.sort_order,
            ],
        )?;
        Ok(())
    }

    pub fn list_environment_variables(
        &self,
        environment_id: &str,
    ) -> Result<Vec<EnvironmentVariable>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT schema_version, id, environment_id, key, value, is_secret, secret_ref, enabled, sort_order
            FROM environment_variables WHERE environment_id = ?1
            ORDER BY sort_order ASC, key COLLATE NOCASE ASC
            "#,
        )?;
        let rows = statement.query_map([environment_id], |row| {
            let schema_version: i64 = row.get(0)?;
            Ok(EnvironmentVariable {
                schema_version: schema_version as u16,
                id: row.get(1)?,
                environment_id: row.get(2)?,
                key: row.get(3)?,
                value: row.get(4)?,
                is_secret: row.get(5)?,
                secret_ref: row.get(6)?,
                enabled: row.get(7)?,
                sort_order: row.get(8)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }

    pub fn delete_environment_variable(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM environment_variables WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn set_preference(&self, preference: &AppPreference) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"INSERT INTO app_preferences (key, value) VALUES (?1, ?2)
               ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = CURRENT_TIMESTAMP"#,
            params![&preference.key, &preference.value],
        )?;
        Ok(())
    }

    pub fn get_preference(&self, key: &str) -> Result<Option<AppPreference>, StorageError> {
        let connection = self.connection()?;
        connection
            .query_row(
                "SELECT key, value FROM app_preferences WHERE key = ?1",
                [key],
                |row| Ok(AppPreference { key: row.get(0)?, value: row.get(1)? }),
            )
            .optional()
            .map_err(StorageError::from)
    }

    pub fn upsert_onboarding_step(&self, step: &OnboardingStep) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            r#"INSERT INTO onboarding_steps (key, completed, completed_at) VALUES (?1, ?2, ?3)
               ON CONFLICT(key) DO UPDATE SET completed = excluded.completed, completed_at = excluded.completed_at"#,
            params![&step.key, step.completed, &step.completed_at],
        )?;
        Ok(())
    }

    pub fn list_onboarding_steps(&self) -> Result<Vec<OnboardingStep>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT key, completed, completed_at FROM onboarding_steps ORDER BY key ASC",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(OnboardingStep {
                key: row.get(0)?,
                completed: row.get(1)?,
                completed_at: row.get(2)?,
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>().map_err(StorageError::from)
    }
}

#[cfg(test)]
mod traffic_search_tests {
    use super::*;

    #[test]
    fn sdk_and_ordinary_filters_find_history_before_output_limits() {
        let root = std::env::temp_dir().join(format!(
            "mas-sdk-search-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let database = Database::open(root.join("app.db")).unwrap();
        // SDK storage is optional in imported workspaces; an SDK filter must simply match nothing.
        assert!(database
            .sdk_flow_ids_matching("needle", 10)
            .unwrap()
            .is_empty());
        assert!(database
            .search_flows(&TrafficSearchQuery::default())
            .unwrap()
            .is_empty());
        assert!(database
            .search_flows(&TrafficSearchQuery {
                sdk_text: Some("  ".into()),
                ..Default::default()
            })
            .unwrap()
            .is_empty());
        let mut connection = database.connection().unwrap();
        connection
            .execute_batch(
                "CREATE TABLE sdk_events(request_id TEXT, payload_json TEXT);
            CREATE INDEX idx_sdk_events_request_id ON sdk_events(request_id);",
            )
            .unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute("INSERT INTO sessions(id,name,started_at) VALUES ('old-session','Historical fixture','0')", []).unwrap();
        for index in 0..10_020 {
            transaction.execute("INSERT INTO flows(id,schema_version,source,method,host,path,status_code,started_at)
                VALUES (?1,1,'proxy',?2,'example.test','/items/42',?3,?4)",
                params![format!("flow-{index}"), if index == 0 { "GET" } else { "POST" },
                    if index == 0 { 200 } else { 500 }, format!("{index:08}")]).unwrap();
        }
        transaction
            .execute(
                "UPDATE flows SET session_id='old-session' WHERE id='flow-0'",
                [],
            )
            .unwrap();
        for (index, headers) in [
            (
                0,
                serde_json::json!([{"name":"X-MOBILE-api-STUDIO-request-ID","value":"exact"}]),
            ),
            (
                1,
                serde_json::json!([{"name":"x-mobile-api-studio-request-id","value":"other"}]),
            ),
            // Request ID values remain exact, and only the first matching header counts.
            (
                2,
                serde_json::json!([{"name":"X-Mobile-API-Studio-Request-Id","value":"EXACT"}]),
            ),
            (
                3,
                serde_json::json!([{"name":"X-Mobile-API-Studio-Request-Id","value":"wrong"},
                {"name":"x-mobile-api-studio-request-id","value":"exact"}]),
            ),
        ] {
            transaction
                .execute(
                    "INSERT INTO flow_details(flow_id,detail_json) VALUES (?1,?2)",
                    params![
                        format!("flow-{index}"),
                        serde_json::json!({"request":{"headers":headers}}).to_string()
                    ],
                )
                .unwrap();
        }
        transaction
            .execute(
                "INSERT INTO sdk_events VALUES ('exact',?1),('other',?1)",
                [serde_json::json!({"context":{"feature":"literal%_metadata"}}).to_string()],
            )
            .unwrap();
        // Matching newer telemetry without captured request IDs must not consume the output limit.
        for index in 0..2_100 {
            transaction
                .execute(
                    "INSERT INTO sdk_events VALUES (?1,?2)",
                    params![format!("uncaptured-{index}"), "literal%_metadata"],
                )
                .unwrap();
        }
        transaction
            .execute(
                "INSERT INTO flow_search_text VALUES ('flow-0','safe body token')",
                [],
            )
            .unwrap();
        transaction.commit().unwrap();
        drop(connection);
        assert_eq!(
            database
                .sdk_flow_ids_matching("LITERAL%_METADATA", 10)
                .unwrap(),
            ["flow-1", "flow-0"]
        );
        assert_eq!(
            database
                .sdk_flow_ids_matching("literal%_metadata", 1)
                .unwrap(),
            ["flow-1"]
        );
        assert!(database
            .sdk_flow_ids_matching("literal__metadata", 10)
            .unwrap()
            .is_empty());
        assert!(database.sdk_flow_ids_matching("  ", 10).unwrap().is_empty());
        let query = TrafficSearchQuery {
            sdk_text: Some("literal%_metadata".into()),
            session_id: Some("old-session".into()),
            method: Some("get".into()),
            status_class: Some(2),
            source: Some(FlowSource::Proxy),
            text: Some("safe body token".into()),
            endpoint_key: Some(normalize_endpoint("GET", "example.test", "/items/42").key),
            limit: Some(1),
            ..Default::default()
        };
        assert_eq!(database.search_flows(&query).unwrap()[0].flow.id, "flow-0");
        assert!(database
            .search_flows(&TrafficSearchQuery {
                text: Some("private secret".into()),
                ..query.clone()
            })
            .unwrap()
            .is_empty());
        assert!(database
            .search_flows(&TrafficSearchQuery {
                session_id: Some("missing".into()),
                ..query.clone()
            })
            .unwrap()
            .is_empty());
        assert!(database
            .search_flows(&TrafficSearchQuery {
                status_class: Some(4),
                ..query
            })
            .unwrap()
            .is_empty());
        assert_eq!(
            database
                .search_flows(&TrafficSearchQuery {
                    sdk_text: Some("  ".into()),
                    limit: Some(1),
                    ..Default::default()
                })
                .unwrap()[0]
                .flow
                .id,
            "flow-10019"
        );
        let connection = database.connection().unwrap();
        connection.execute("DROP TABLE sdk_events", []).unwrap();
        drop(connection);
        assert_eq!(
            database
                .search_flows(&TrafficSearchQuery {
                    limit: Some(1),
                    ..Default::default()
                })
                .unwrap()[0]
                .flow
                .id,
            "flow-10019"
        );
        assert!(database
            .sdk_flow_ids_matching("literal%_metadata", 10)
            .unwrap()
            .is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}

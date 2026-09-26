use super::{Database, StorageError, flow_source_to_str, session_status_to_str, workflow};
use core_model::{
    CaptureSession, Environment, EnvironmentVariable, FlowDetail, FlowSummary, SavedCollection,
    SavedRequest,
};
use rusqlite::params;

pub struct ImportedFlow {
    pub summary: FlowSummary,
    pub detail: Option<FlowDetail>,
}

pub struct ImportedSession {
    pub session: CaptureSession,
    pub flows: Vec<ImportedFlow>,
}

pub struct WorkspaceReplacement {
    pub sessions: Vec<ImportedSession>,
    pub collections: Vec<SavedCollection>,
    pub saved_requests: Vec<SavedRequest>,
    pub environments: Vec<Environment>,
    pub environment_variables: Vec<EnvironmentVariable>,
}

impl Database {
    /// SQLite commits the replacement only after every imported row succeeds.
    /// Body blobs must be written before this call; unused blobs are harmless if it rolls back.
    pub fn replace_workspace(
        &self,
        replacement: &WorkspaceReplacement,
    ) -> Result<Vec<String>, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let old_secret_refs = {
            let mut statement = transaction.prepare(
                "SELECT secret_ref FROM environment_variables WHERE secret_ref IS NOT NULL",
            )?;
            statement
                .query_map([], |row| row.get(0))?
                .collect::<Result<Vec<String>, _>>()?
        };
        transaction.execute("DELETE FROM environments", [])?;
        transaction.execute("DELETE FROM saved_collections", [])?;
        transaction.execute("DELETE FROM sessions", [])?;

        for imported in &replacement.sessions {
            let session = &imported.session;
            transaction.execute(
                "INSERT INTO sessions (id, schema_version, name, status, started_at, ended_at, device_id, app_id, connection_strategy, capture_engine, notes) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                ],
            )?;
            for imported_flow in &imported.flows {
                let flow = &imported_flow.summary;
                transaction.execute(
                    "INSERT INTO flows (id, schema_version, session_id, source, method, host, path, status_code, duration_ms, response_size_bytes, started_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
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
                workflow::upsert_endpoint_index(&transaction, flow)?;
                if let Some(detail) = &imported_flow.detail {
                    let json = serde_json::to_string(detail)?;
                    transaction.execute(
                        "INSERT INTO flow_details (flow_id, detail_json) VALUES (?1, ?2)",
                        params![&flow.id, json],
                    )?;
                }
            }
        }

        for collection in &replacement.collections {
            transaction.execute(
                "INSERT INTO saved_collections (id, schema_version, name, description, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![&collection.id, i64::from(collection.schema_version), &collection.name, &collection.description, collection.sort_order, &collection.created_at, &collection.updated_at],
            )?;
        }
        for request in &replacement.saved_requests {
            let headers_json = serde_json::to_string(&request.headers)?;
            let body_json = request
                .body
                .as_ref()
                .map(serde_json::to_string)
                .transpose()?;
            transaction.execute(
                "INSERT INTO saved_requests (id, schema_version, collection_id, name, method, url, headers_json, body_json, source_flow_id, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![&request.id, i64::from(request.schema_version), &request.collection_id, &request.name, &request.method, &request.url, headers_json, body_json, &request.source_flow_id, request.sort_order, &request.created_at, &request.updated_at],
            )?;
        }
        for environment in &replacement.environments {
            transaction.execute(
                "INSERT INTO environments (id, schema_version, name, is_active, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![&environment.id, i64::from(environment.schema_version), &environment.name, environment.is_active, &environment.created_at, &environment.updated_at],
            )?;
        }
        for variable in &replacement.environment_variables {
            let stored_value = if variable.is_secret {
                None
            } else {
                variable.value.as_deref()
            };
            transaction.execute(
                "INSERT INTO environment_variables (id, schema_version, environment_id, key, value, is_secret, secret_ref, enabled, sort_order) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![&variable.id, i64::from(variable.schema_version), &variable.environment_id, &variable.key, stored_value, variable.is_secret, &variable.secret_ref, variable.enabled, variable.sort_order],
            )?;
        }
        transaction.commit()?;
        Ok(old_secret_refs)
    }
}

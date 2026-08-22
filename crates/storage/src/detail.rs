use super::{Database, StorageError};
use core_model::FlowDetail;
use rusqlite::{params, OptionalExtension};

const MIGRATION_002: &str = r#"
CREATE TABLE IF NOT EXISTS flow_details (
    flow_id TEXT PRIMARY KEY,
    detail_json TEXT NOT NULL,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    FOREIGN KEY(flow_id) REFERENCES flows(id) ON DELETE CASCADE
);

INSERT OR IGNORE INTO schema_migrations(version) VALUES (2);
"#;

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let connection = database.connection()?;
    connection.execute_batch(MIGRATION_002)?;
    Ok(())
}

impl Database {
    pub fn upsert_flow_detail(&self, detail: &FlowDetail) -> Result<(), StorageError> {
        self.upsert_flow(&detail.summary)?;
        let detail_json = serde_json::to_string(detail).map_err(StorageError::Json)?;
        let connection = self.connection()?;
        connection.execute(
            r#"
            INSERT INTO flow_details (flow_id, detail_json)
            VALUES (?1, ?2)
            ON CONFLICT(flow_id) DO UPDATE SET
                detail_json = excluded.detail_json,
                updated_at = CURRENT_TIMESTAMP
            "#,
            params![&detail.summary.id, detail_json],
        )?;
        Ok(())
    }

    pub fn get_flow_detail(&self, flow_id: &str) -> Result<Option<FlowDetail>, StorageError> {
        let connection = self.connection()?;
        let detail_json: Option<String> = connection
            .query_row(
                "SELECT detail_json FROM flow_details WHERE flow_id = ?1",
                [flow_id],
                |row| row.get(0),
            )
            .optional()?;

        detail_json
            .map(|json| serde_json::from_str(&json).map_err(StorageError::Json))
            .transpose()
    }
}

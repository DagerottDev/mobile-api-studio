use super::{Database, StorageError};
use mock_core::MockRule;
use rusqlite::{params, OptionalExtension};

const MIGRATION_004: &str = r#"
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

INSERT OR IGNORE INTO schema_migrations(version) VALUES (4);
"#;

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let connection = database.connection()?;
    connection.execute_batch(MIGRATION_004)?;
    Ok(())
}

impl Database {
    pub fn upsert_mock_rule(&self, rule: &MockRule) -> Result<(), StorageError> {
        let rule_json = serde_json::to_string(rule)?;
        let connection = self.connection()?;
        connection.execute(
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
                rule_json,
                rule.enabled,
                rule.priority,
                &rule.created_at,
                &rule.updated_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_mock_rules(&self) -> Result<Vec<MockRule>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            r#"
            SELECT rule_json, enabled, priority
            FROM mock_rules
            ORDER BY priority ASC, created_at ASC, id ASC
            "#,
        )?;
        let rows = statement.query_map([], |row| {
            let json: String = row.get(0)?;
            let enabled: bool = row.get(1)?;
            let priority: i64 = row.get(2)?;
            Ok((json, enabled, priority))
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

    pub fn get_mock_rule(&self, id: &str) -> Result<Option<MockRule>, StorageError> {
        let connection = self.connection()?;
        let raw: Option<(String, bool, i64)> = connection
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

    pub fn set_mock_rule_enabled(&self, id: &str, enabled: bool) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE mock_rules SET enabled = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, enabled],
        )?;
        Ok(())
    }

    pub fn set_mock_rule_priority(&self, id: &str, priority: i64) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute(
            "UPDATE mock_rules SET priority = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?1",
            params![id, priority],
        )?;
        Ok(())
    }

    pub fn delete_mock_rule(&self, id: &str) -> Result<(), StorageError> {
        let connection = self.connection()?;
        connection.execute("DELETE FROM mock_rules WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn disable_all_mock_rules(&self) -> Result<usize, StorageError> {
        let connection = self.connection()?;
        let changed = connection.execute(
            "UPDATE mock_rules SET enabled = 0, updated_at = CURRENT_TIMESTAMP WHERE enabled != 0",
            [],
        )?;
        Ok(changed)
    }
}

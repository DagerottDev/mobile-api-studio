use super::{Database, StorageError};
use core_model::network_profiles::{NetworkProfile, validate_network_profile};
use rusqlite::params;

const MIGRATION_007: &str = r#"
CREATE TABLE network_profiles (
    id TEXT PRIMARY KEY,
    enabled INTEGER NOT NULL,
    priority INTEGER NOT NULL,
    created_at TEXT NOT NULL,
    profile_json TEXT NOT NULL
);
CREATE INDEX idx_network_profiles_order ON network_profiles(enabled, priority, created_at, id);
INSERT INTO schema_migrations(version) VALUES (7);
"#;

pub(super) fn initialize(database: &Database) -> Result<(), StorageError> {
    let mut connection = database.connection()?;
    let migrated: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 7)",
        [],
        |row| row.get(0),
    )?;
    if !migrated {
        let transaction = connection.transaction()?;
        transaction.execute_batch(MIGRATION_007)?;
        transaction.commit()?;
    }
    Ok(())
}

impl Database {
    pub fn list_network_profiles(&self) -> Result<Vec<NetworkProfile>, StorageError> {
        let connection = self.connection()?;
        let mut statement = connection.prepare(
            "SELECT profile_json FROM network_profiles ORDER BY priority, created_at, id",
        )?;
        let json = statement
            .query_map([], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        json.iter()
            .map(|value| serde_json::from_str(value).map_err(StorageError::from))
            .collect()
    }

    pub fn upsert_network_profile(&self, profile: &NetworkProfile) -> Result<(), StorageError> {
        validate_network_profile(profile).map_err(StorageError::InvalidInput)?;
        let mut connection = self.connection()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let count: i64 = transaction.query_row(
            "SELECT count(*) FROM network_profiles WHERE id != ?1",
            [&profile.id],
            |row| row.get(0),
        )?;
        if count >= 100 {
            return Err(StorageError::InvalidInput(
                "The workspace supports at most 100 network profiles.".into(),
            ));
        }
        let json = serde_json::to_string(profile)?;
        transaction.execute(
            "INSERT INTO network_profiles (id, enabled, priority, created_at, profile_json) VALUES (?1, ?2, ?3, ?4, ?5) ON CONFLICT(id) DO UPDATE SET enabled = excluded.enabled, priority = excluded.priority, created_at = excluded.created_at, profile_json = excluded.profile_json",
            params![profile.id, profile.enabled, profile.priority, profile.created_at, json],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn delete_network_profile(&self, id: &str) -> Result<(), StorageError> {
        self.connection()?
            .execute("DELETE FROM network_profiles WHERE id = ?1", [id])?;
        Ok(())
    }

    pub fn disable_all_network_profiles(&self) -> Result<usize, StorageError> {
        let mut connection = self.connection()?;
        let transaction = connection.transaction()?;
        let profiles = {
            let mut statement = transaction
                .prepare("SELECT profile_json FROM network_profiles WHERE enabled != 0")?;
            statement
                .query_map([], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()?
        };
        for json in &profiles {
            let mut profile: NetworkProfile = serde_json::from_str(json)?;
            profile.enabled = false;
            transaction.execute(
                "UPDATE network_profiles SET enabled = 0, profile_json = ?2 WHERE id = ?1",
                params![profile.id, serde_json::to_string(&profile)?],
            )?;
        }
        transaction.commit()?;
        Ok(profiles.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_model::network_profiles::NetworkScope;

    #[test]
    fn network_profiles_migrate_back_up_persist_disable_and_enforce_limits() {
        let root = std::env::temp_dir().join(format!(
            "mas-network-profile-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let path = root.join("workspace.db");
        let database = Database::open(&path).unwrap();
        database
            .connection()
            .unwrap()
            .execute_batch(
                "DROP TABLE network_profiles; DELETE FROM schema_migrations WHERE version = 7;",
            )
            .unwrap();
        drop(database);
        let database = Database::open(&path).unwrap();
        assert!(std::fs::read_dir(&root).unwrap().any(|entry| {
            entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .contains("pre-network-profiles")
        }));
        assert!(database.list_network_profiles().unwrap().is_empty());
        let profile = NetworkProfile {
            schema_version: 1,
            id: "first".into(),
            name: "Slow".into(),
            enabled: true,
            priority: 0,
            scope: NetworkScope::Global {},
            latency_ms: 100,
            jitter_ms: 5,
            upload_bytes_per_second: Some(1024),
            download_bytes_per_second: None,
            offline: false,
            failure_percent: 2.5,
            created_at: "1".into(),
            updated_at: "2".into(),
        };
        database.upsert_network_profile(&profile).unwrap();
        drop(database);
        let database = Database::open(&path).unwrap();
        assert_eq!(
            database.list_network_profiles().unwrap(),
            vec![profile.clone()]
        );
        assert_eq!(database.disable_all_network_profiles().unwrap(), 1);
        assert_eq!(database.disable_all_network_profiles().unwrap(), 0);
        assert!(!database.list_network_profiles().unwrap()[0].enabled);
        for n in 1..100 {
            database
                .upsert_network_profile(&NetworkProfile {
                    id: format!("p-{n}"),
                    ..profile.clone()
                })
                .unwrap();
        }
        assert!(
            database
                .upsert_network_profile(&NetworkProfile {
                    id: "overflow".into(),
                    ..profile.clone()
                })
                .is_err()
        );
        database.upsert_network_profile(&profile).unwrap();
        assert!(
            database
                .upsert_network_profile(&NetworkProfile {
                    latency_ms: 10_001,
                    ..profile.clone()
                })
                .is_err()
        );
        database.delete_network_profile(&profile.id).unwrap();
        assert_eq!(database.list_network_profiles().unwrap().len(), 99);
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }
}

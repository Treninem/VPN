use amri_core::{
    DestinationKey, NetworkProfile, NodeId, ProbeSample, RouteProof, RouteProofChain,
    RouteProofError, TrafficClass,
};
use amri_secrets::{load_or_create_key_32, SecretStore, SecretStoreError};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("route proof JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("route proof integrity error: {0}")]
    Proof(#[from] RouteProofError),
    #[error("secret storage error: {0}")]
    Secret(#[from] SecretStoreError),
}

pub struct LocalStore {
    conn: Connection,
}

impl LocalStore {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StorageError> {
        let conn = Connection::open(path)?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    pub fn in_memory() -> Result<Self, StorageError> {
        let conn = Connection::open_in_memory()?;
        let store = Self { conn };
        store.migrate()?;
        Ok(store)
    }

    fn migrate(&self) -> Result<(), StorageError> {
        self.conn.execute_batch(
            r#"
            PRAGMA journal_mode=WAL;
            PRAGMA foreign_keys=ON;

            CREATE TABLE IF NOT EXISTS probe_samples (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                node_id TEXT NOT NULL,
                destination_host TEXT NOT NULL,
                process_name TEXT,
                network_fingerprint TEXT NOT NULL,
                network_label TEXT,
                traffic_class TEXT NOT NULL,
                measured_at TEXT NOT NULL,
                latency_ms REAL NOT NULL,
                jitter_ms REAL NOT NULL,
                packet_loss_ratio REAL NOT NULL,
                tcp_connect_ms REAL,
                tls_handshake_ms REAL,
                dns_ms REAL,
                download_mbps REAL,
                upload_mbps REAL,
                success INTEGER NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_probe_lookup
            ON probe_samples(node_id, destination_host, network_fingerprint, traffic_class, measured_at);

            CREATE TABLE IF NOT EXISTS route_decisions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                destination_host TEXT NOT NULL,
                process_name TEXT,
                network_fingerprint TEXT NOT NULL,
                traffic_class TEXT NOT NULL,
                selected_node_id TEXT NOT NULL,
                score REAL NOT NULL,
                confidence REAL NOT NULL,
                reason TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE TABLE IF NOT EXISTS route_proofs (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                destination_token TEXT NOT NULL CHECK(length(destination_token) = 64),
                proof_hash TEXT NOT NULL UNIQUE CHECK(length(proof_hash) = 64),
                previous_proof_hash TEXT,
                proof_json TEXT NOT NULL,
                created_at TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_route_proof_destination
            ON route_proofs(destination_token, id);
            "#,
        )?;
        Ok(())
    }

    pub fn insert_probe(
        &self,
        node_id: &NodeId,
        destination: &DestinationKey,
        network: &NetworkProfile,
        traffic: TrafficClass,
        sample: &ProbeSample,
    ) -> Result<(), StorageError> {
        self.conn.execute(
            r#"
            INSERT INTO probe_samples (
                node_id, destination_host, process_name,
                network_fingerprint, network_label, traffic_class,
                measured_at, latency_ms, jitter_ms, packet_loss_ratio,
                tcp_connect_ms, tls_handshake_ms, dns_ms,
                download_mbps, upload_mbps, success
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)
            "#,
            params![
                &node_id.0,
                &destination.host,
                destination.process.as_deref(),
                &network.fingerprint,
                network.label.as_deref(),
                format!("{traffic:?}"),
                sample.measured_at.to_rfc3339(),
                sample.latency_ms,
                sample.jitter_ms,
                sample.packet_loss_ratio,
                sample.tcp_connect_ms,
                sample.tls_handshake_ms,
                sample.dns_ms,
                sample.download_mbps,
                sample.upload_mbps,
                sample.success as i64,
            ],
        )?;
        Ok(())
    }

    /// Opens the persistent Route Proof chain with an OS-protected installation key.
    ///
    /// Existing receipts are verified before their tails are adopted. Corrupt or reordered
    /// history therefore fails closed instead of silently becoming training input.
    pub fn open_route_proof_chain(
        &self,
        secrets: &dyn SecretStore,
        secret_key: &str,
    ) -> Result<RouteProofChain, StorageError> {
        let secret = load_or_create_key_32(secrets, secret_key)?;
        let mut chain = RouteProofChain::try_from_secret(secret.expose_secret())?;
        let proofs = self.load_route_proofs()?;
        chain.restore_verified(&proofs)?;
        Ok(chain)
    }

    pub fn insert_verified_route_proof(
        &self,
        chain: &RouteProofChain,
        proof: &RouteProof,
    ) -> Result<(), StorageError> {
        chain.verify(proof)?;

        let expected_previous: Option<String> = self
            .conn
            .query_row(
                "SELECT proof_hash FROM route_proofs WHERE destination_token = ?1 ORDER BY id DESC LIMIT 1",
                [&proof.destination_token],
                |row| row.get(0),
            )
            .optional()?;
        if proof.previous_proof_hash != expected_previous {
            return Err(StorageError::Proof(RouteProofError::BrokenChain));
        }

        let json = serde_json::to_string(proof)?;
        self.conn.execute(
            r#"
            INSERT INTO route_proofs (
                destination_token, proof_hash, previous_proof_hash, proof_json, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5)
            "#,
            params![
                &proof.destination_token,
                &proof.proof_hash,
                proof.previous_proof_hash.as_deref(),
                json,
                proof.created_at.to_rfc3339(),
            ],
        )?;
        Ok(())
    }

    pub fn load_route_proofs(&self) -> Result<Vec<RouteProof>, StorageError> {
        let mut statement = self
            .conn
            .prepare("SELECT proof_json FROM route_proofs ORDER BY id ASC")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        let mut proofs = Vec::new();
        for row in rows {
            proofs.push(serde_json::from_str(&row?)?);
        }
        Ok(proofs)
    }

    pub fn load_verified_route_proofs(
        &self,
        chain: &RouteProofChain,
    ) -> Result<Vec<RouteProof>, StorageError> {
        let proofs = self.load_route_proofs()?;
        chain.verify_chain(&proofs)?;
        Ok(proofs)
    }

    pub fn route_proof_count(&self) -> Result<u64, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM route_proofs", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    pub fn clear_route_proofs_for_token(
        &self,
        destination_token: &str,
    ) -> Result<u64, StorageError> {
        let deleted = self.conn.execute(
            "DELETE FROM route_proofs WHERE destination_token = ?1",
            [destination_token],
        )?;
        Ok(deleted as u64)
    }

    pub fn clear_all_route_proofs(&self) -> Result<u64, StorageError> {
        let deleted = self.conn.execute("DELETE FROM route_proofs", [])?;
        Ok(deleted as u64)
    }

    pub fn probe_count(&self) -> Result<u64, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM probe_samples", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    pub fn latest_probe_time(&self) -> Result<Option<DateTime<Utc>>, StorageError> {
        let raw: Option<String> =
            self.conn
                .query_row("SELECT MAX(measured_at) FROM probe_samples", [], |row| {
                    row.get(0)
                })?;
        Ok(raw.and_then(|s| {
            DateTime::parse_from_rfc3339(&s)
                .ok()
                .map(|v| v.with_timezone(&Utc))
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MemorySecrets(std::sync::Mutex<std::collections::HashMap<String, Vec<u8>>>);

    impl amri_secrets::SecretStore for MemorySecrets {
        fn put(
            &self,
            key: &str,
            value: &amri_secrets::SecretValue,
        ) -> Result<(), amri_secrets::SecretStoreError> {
            self.0
                .lock()
                .unwrap()
                .insert(key.to_owned(), value.expose_secret().to_vec());
            Ok(())
        }

        fn get(
            &self,
            key: &str,
        ) -> Result<Option<amri_secrets::SecretValue>, amri_secrets::SecretStoreError> {
            self.0
                .lock()
                .unwrap()
                .get(key)
                .cloned()
                .map(amri_secrets::SecretValue::from_bytes)
                .transpose()
        }

        fn delete(&self, key: &str) -> Result<bool, amri_secrets::SecretStoreError> {
            Ok(self.0.lock().unwrap().remove(key).is_some())
        }
    }

    #[test]
    fn persistent_chain_reopens_with_secret_store_and_continues_history() {
        let store = LocalStore::in_memory().unwrap();
        let secrets = MemorySecrets(std::sync::Mutex::new(std::collections::HashMap::new()));
        let destination = proof_destination("private.example");

        let mut first_session = store
            .open_route_proof_chain(&secrets, "route-proof:key")
            .unwrap();
        let first = first_session.append(&destination, "node-a", "first", vec![], Utc::now());
        store
            .insert_verified_route_proof(&first_session, &first)
            .unwrap();
        drop(first_session);

        let mut second_session = store
            .open_route_proof_chain(&secrets, "route-proof:key")
            .unwrap();
        let second = second_session.append(&destination, "node-b", "second", vec![], Utc::now());

        assert_eq!(
            second.previous_proof_hash.as_deref(),
            Some(first.proof_hash.as_str())
        );
        store
            .insert_verified_route_proof(&second_session, &second)
            .unwrap();
        assert_eq!(store.route_proof_count().unwrap(), 2);
    }

    fn proof_destination(host: &str) -> DestinationKey {
        DestinationKey {
            host: host.into(),
            process: Some("browser.exe".into()),
        }
    }

    #[test]
    fn stores_and_verifies_route_proof_without_raw_destination() {
        let store = LocalStore::in_memory().unwrap();
        let mut chain = RouteProofChain::new([9; 32]);
        let destination = proof_destination("private.example");
        let proof = chain.append(&destination, "node-a", "best score", vec![], Utc::now());

        store.insert_verified_route_proof(&chain, &proof).unwrap();
        let loaded = store.load_verified_route_proofs(&chain).unwrap();

        assert_eq!(loaded, vec![proof]);
        assert_eq!(store.route_proof_count().unwrap(), 1);
        let json: String = store
            .conn
            .query_row("SELECT proof_json FROM route_proofs", [], |row| row.get(0))
            .unwrap();
        assert!(!json.contains("private.example"));
        assert!(!json.contains("browser.exe"));
    }

    #[test]
    fn rejects_proof_that_does_not_continue_persisted_destination_chain() {
        let store = LocalStore::in_memory().unwrap();
        let destination = proof_destination("private.example");
        let mut first_chain = RouteProofChain::new([7; 32]);
        let first = first_chain.append(&destination, "node-a", "first", vec![], Utc::now());
        store
            .insert_verified_route_proof(&first_chain, &first)
            .unwrap();

        let mut restarted_without_restore = RouteProofChain::new([7; 32]);
        let disconnected =
            restarted_without_restore.append(&destination, "node-b", "second", vec![], Utc::now());

        assert!(matches!(
            store.insert_verified_route_proof(&restarted_without_restore, &disconnected),
            Err(StorageError::Proof(RouteProofError::BrokenChain))
        ));
    }

    #[test]
    fn detects_tampered_persisted_proof() {
        let store = LocalStore::in_memory().unwrap();
        let mut chain = RouteProofChain::new([5; 32]);
        let proof = chain.append(
            &proof_destination("private.example"),
            "node-a",
            "best",
            vec![],
            Utc::now(),
        );
        store.insert_verified_route_proof(&chain, &proof).unwrap();

        let mut tampered = proof;
        tampered.selected_node_id = "attacker-node".into();
        let json = serde_json::to_string(&tampered).unwrap();
        store
            .conn
            .execute("UPDATE route_proofs SET proof_json = ?1", [json])
            .unwrap();

        assert!(matches!(
            store.load_verified_route_proofs(&chain),
            Err(StorageError::Proof(RouteProofError::InvalidSignature))
        ));
    }

    #[test]
    fn clearing_one_destination_keeps_other_chain_valid() {
        let store = LocalStore::in_memory().unwrap();
        let mut chain = RouteProofChain::new([3; 32]);
        let a = proof_destination("a.example");
        let b = proof_destination("b.example");
        let proof_a = chain.append(&a, "node-a", "a", vec![], Utc::now());
        let proof_b = chain.append(&b, "node-b", "b", vec![], Utc::now());
        store.insert_verified_route_proof(&chain, &proof_a).unwrap();
        store.insert_verified_route_proof(&chain, &proof_b).unwrap();

        let token_a = chain.token_for(&a);
        assert_eq!(store.clear_route_proofs_for_token(&token_a).unwrap(), 1);
        let remaining = store.load_verified_route_proofs(&chain).unwrap();

        assert_eq!(remaining, vec![proof_b]);
    }

    #[test]
    fn stores_probe_locally() {
        let store = LocalStore::in_memory().unwrap();
        let node = NodeId("node-1".into());
        let destination = DestinationKey {
            host: "example.com".into(),
            process: Some("browser.exe".into()),
        };
        let network = NetworkProfile {
            fingerprint: "ethernet-home".into(),
            label: Some("Home".into()),
        };
        let sample = ProbeSample::basic(30.0, 2.0, 0.0);

        store
            .insert_probe(&node, &destination, &network, TrafficClass::Web, &sample)
            .unwrap();

        assert_eq!(store.probe_count().unwrap(), 1);
    }
}

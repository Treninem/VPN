use amri_core::{DestinationKey, NetworkProfile, NodeId, ProbeSample, TrafficClass};
use chrono::{DateTime, Utc};
use rusqlite::{params, Connection};
use std::path::Path;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StorageError {
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
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
                node_id.0,
                destination.host,
                destination.process,
                network.fingerprint,
                network.label,
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

    pub fn probe_count(&self) -> Result<u64, StorageError> {
        let count: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM probe_samples", [], |row| row.get(0))?;
        Ok(count as u64)
    }

    pub fn latest_probe_time(&self) -> Result<Option<DateTime<Utc>>, StorageError> {
        let raw: Option<String> = self.conn.query_row(
            "SELECT MAX(measured_at) FROM probe_samples",
            [],
            |row| row.get(0),
        )?;
        Ok(raw.and_then(|s| DateTime::parse_from_rfc3339(&s).ok().map(|v| v.with_timezone(&Utc))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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

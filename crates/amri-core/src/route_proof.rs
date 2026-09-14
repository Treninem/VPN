use crate::{DestinationKey, ScoreBreakdown};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;
const PROOF_VERSION: &str = "amri-route-proof-v1";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CandidateEvidence {
    pub node_id: String,
    pub score: f64,
    pub confidence: f64,
    pub latency_score: f64,
    pub jitter_score: f64,
    pub loss_score: f64,
    pub throughput_score: f64,
    pub stability_score: f64,
}

impl CandidateEvidence {
    pub fn from_breakdown(node_id: impl Into<String>, score: &ScoreBreakdown) -> Self {
        Self {
            node_id: node_id.into(),
            score: score.score,
            confidence: score.confidence,
            latency_score: score.latency_score,
            jitter_score: score.jitter_score,
            loss_score: score.loss_score,
            throughput_score: score.throughput_score,
            stability_score: score.stability_score,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteProof {
    pub version: String,
    pub created_at: DateTime<Utc>,
    /// Stable only for this local secret; never contains a host or process name.
    pub destination_token: String,
    pub selected_node_id: String,
    pub reason: String,
    pub evidence: Vec<CandidateEvidence>,
    pub previous_proof_hash: Option<String>,
    pub proof_hash: String,
}

pub struct RouteProofChain {
    secret: [u8; 32],
    previous_hash: Option<String>,
}

impl RouteProofChain {
    pub fn new(secret: [u8; 32]) -> Self {
        Self {
            secret,
            previous_hash: None,
        }
    }

    pub fn append(
        &mut self,
        destination: &DestinationKey,
        selected_node_id: impl Into<String>,
        reason: impl Into<String>,
        evidence: Vec<CandidateEvidence>,
        created_at: DateTime<Utc>,
    ) -> RouteProof {
        let mut proof = RouteProof {
            version: PROOF_VERSION.into(),
            created_at,
            destination_token: self.destination_token(destination),
            selected_node_id: selected_node_id.into(),
            reason: reason.into(),
            evidence,
            previous_proof_hash: self.previous_hash.clone(),
            proof_hash: String::new(),
        };
        proof.proof_hash = self.sign_proof(&proof);
        self.previous_hash = Some(proof.proof_hash.clone());
        proof
    }

    pub fn verify(&self, proof: &RouteProof) -> Result<(), RouteProofError> {
        if proof.version != PROOF_VERSION {
            return Err(RouteProofError::UnsupportedVersion);
        }
        let expected = self.sign_proof(proof);
        if constant_time_eq(expected.as_bytes(), proof.proof_hash.as_bytes()) {
            Ok(())
        } else {
            Err(RouteProofError::InvalidSignature)
        }
    }

    pub fn verify_chain(&self, proofs: &[RouteProof]) -> Result<(), RouteProofError> {
        let mut previous: Option<&str> = None;
        for proof in proofs {
            if proof.previous_proof_hash.as_deref() != previous {
                return Err(RouteProofError::BrokenChain);
            }
            self.verify(proof)?;
            previous = Some(proof.proof_hash.as_str());
        }
        Ok(())
    }

    /// Starts a new local chain, for example after the user clears learning history.
    pub fn reset(&mut self) {
        self.previous_hash = None;
    }

    fn destination_token(&self, destination: &DestinationKey) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts a 32-byte key");
        write_field(&mut mac, b"destination-v1");
        write_field(&mut mac, destination.host.as_bytes());
        write_field(
            &mut mac,
            destination
                .process
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        hex(&mac.finalize().into_bytes())
    }

    fn sign_proof(&self, proof: &RouteProof) -> String {
        let mut mac = HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts a 32-byte key");
        write_field(&mut mac, PROOF_VERSION.as_bytes());
        write_field(&mut mac, proof.created_at.to_rfc3339().as_bytes());
        write_field(&mut mac, proof.destination_token.as_bytes());
        write_field(&mut mac, proof.selected_node_id.as_bytes());
        write_field(&mut mac, proof.reason.as_bytes());
        write_field(
            &mut mac,
            proof
                .previous_proof_hash
                .as_deref()
                .unwrap_or_default()
                .as_bytes(),
        );
        for item in &proof.evidence {
            write_field(&mut mac, item.node_id.as_bytes());
            for value in [
                item.score,
                item.confidence,
                item.latency_score,
                item.jitter_score,
                item.loss_score,
                item.throughput_score,
                item.stability_score,
            ] {
                write_field(&mut mac, &value.to_bits().to_be_bytes());
            }
        }
        hex(&mac.finalize().into_bytes())
    }
}

impl Drop for RouteProofChain {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum RouteProofError {
    #[error("unsupported AMRI Route Proof version")]
    UnsupportedVersion,
    #[error("route proof signature is invalid")]
    InvalidSignature,
    #[error("route proof chain is broken")]
    BrokenChain,
}

fn write_field(mac: &mut HmacSha256, bytes: &[u8]) {
    mac.update(&(bytes.len() as u64).to_be_bytes());
    mac.update(bytes);
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut result = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        result.push(DIGITS[(byte >> 4) as usize] as char);
        result.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    result
}

fn constant_time_eq(left: &[u8], right: &[u8]) -> bool {
    if left.len() != right.len() {
        return false;
    }
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn destination() -> DestinationKey {
        DestinationKey {
            host: "private.example".into(),
            process: Some("private-app".into()),
        }
    }

    fn evidence(node: &str, score: f64) -> CandidateEvidence {
        CandidateEvidence {
            node_id: node.into(),
            score,
            confidence: 88.0,
            latency_score: 0.8,
            jitter_score: 0.9,
            loss_score: 1.0,
            throughput_score: 0.7,
            stability_score: 0.95,
        }
    }

    #[test]
    fn proof_contains_no_raw_destination_identity() {
        let mut chain = RouteProofChain::new([7; 32]);
        let proof = chain.append(
            &destination(),
            "node-a",
            "best score",
            vec![evidence("node-a", 91.0)],
            Utc.with_ymd_and_hms(2026, 9, 14, 12, 0, 0).unwrap(),
        );

        let encoded = serde_json::to_string(&proof).unwrap();
        assert!(!encoded.contains("private.example"));
        assert!(!encoded.contains("private-app"));
        assert_eq!(proof.destination_token.len(), 64);
    }

    #[test]
    fn detects_any_changed_decision_evidence() {
        let mut chain = RouteProofChain::new([9; 32]);
        let mut proof = chain.append(
            &destination(),
            "node-a",
            "best score",
            vec![evidence("node-a", 91.0)],
            Utc::now(),
        );
        proof.evidence[0].score = 12.0;

        assert_eq!(chain.verify(&proof), Err(RouteProofError::InvalidSignature));
    }

    #[test]
    fn verifies_ordered_chain_and_rejects_removed_middle_record() {
        let mut chain = RouteProofChain::new([11; 32]);
        let first = chain.append(
            &destination(),
            "node-a",
            "first",
            vec![evidence("node-a", 90.0)],
            Utc::now(),
        );
        let second = chain.append(
            &destination(),
            "node-b",
            "second",
            vec![evidence("node-b", 92.0)],
            Utc::now(),
        );
        let third = chain.append(
            &destination(),
            "node-c",
            "third",
            vec![evidence("node-c", 94.0)],
            Utc::now(),
        );

        assert!(chain
            .verify_chain(&[first.clone(), second, third.clone()])
            .is_ok());
        assert_eq!(
            chain.verify_chain(&[first, third]),
            Err(RouteProofError::BrokenChain)
        );
    }

    #[test]
    fn different_installation_secrets_create_unlinkable_tokens() {
        let mut first = RouteProofChain::new([1; 32]);
        let mut second = RouteProofChain::new([2; 32]);
        let at = Utc::now();

        let a = first.append(&destination(), "node", "reason", vec![], at);
        let b = second.append(&destination(), "node", "reason", vec![], at);

        assert_ne!(a.destination_token, b.destination_token);
    }
}

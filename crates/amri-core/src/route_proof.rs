use crate::{DestinationKey, ScoreBreakdown};
use chrono::{DateTime, Utc};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use std::collections::HashMap;
use thiserror::Error;

type HmacSha256 = Hmac<Sha256>;
const PROOF_VERSION: &str = "amri-route-proof-v1";
const KEY_LENGTH: usize = 32;

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
    /// Stable only for this installation secret; never contains host/process plaintext.
    pub destination_token: String,
    pub selected_node_id: String,
    pub reason: String,
    pub evidence: Vec<CandidateEvidence>,
    /// Per-destination predecessor, allowing explicit deletion of one destination history.
    pub previous_proof_hash: Option<String>,
    pub proof_hash: String,
}

pub struct RouteProofChain {
    secret: [u8; KEY_LENGTH],
    tails: HashMap<String, String>,
}

impl RouteProofChain {
    pub fn new(secret: [u8; KEY_LENGTH]) -> Self {
        Self {
            secret,
            tails: HashMap::new(),
        }
    }

    pub fn try_from_secret(secret: &[u8]) -> Result<Self, RouteProofError> {
        let secret: [u8; KEY_LENGTH] = secret
            .try_into()
            .map_err(|_| RouteProofError::InvalidKeyLength)?;
        Ok(Self::new(secret))
    }

    pub fn append(
        &mut self,
        destination: &DestinationKey,
        selected_node_id: impl Into<String>,
        reason: impl Into<String>,
        evidence: Vec<CandidateEvidence>,
        created_at: DateTime<Utc>,
    ) -> RouteProof {
        let destination_token = self.token_for(destination);
        let mut proof = RouteProof {
            version: PROOF_VERSION.into(),
            created_at,
            previous_proof_hash: self.tails.get(&destination_token).cloned(),
            destination_token,
            selected_node_id: selected_node_id.into(),
            reason: reason.into(),
            evidence,
            proof_hash: String::new(),
        };
        proof.proof_hash = self.sign_proof(&proof);
        self.tails
            .insert(proof.destination_token.clone(), proof.proof_hash.clone());
        proof
    }

    pub fn token_for(&self, destination: &DestinationKey) -> String {
        let mut mac = self.mac();
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

    /// Verifies interleaved per-destination chains without linking destinations together.
    pub fn verify_chain(&self, proofs: &[RouteProof]) -> Result<(), RouteProofError> {
        let mut tails: HashMap<&str, &str> = HashMap::new();
        for proof in proofs {
            let expected_previous = tails.get(proof.destination_token.as_str()).copied();
            if proof.previous_proof_hash.as_deref() != expected_previous {
                return Err(RouteProofError::BrokenChain);
            }
            self.verify(proof)?;
            tails.insert(&proof.destination_token, &proof.proof_hash);
        }
        Ok(())
    }

    /// Verifies persisted receipts before adopting their per-destination tails.
    pub fn restore_verified(&mut self, proofs: &[RouteProof]) -> Result<(), RouteProofError> {
        self.verify_chain(proofs)?;
        self.tails.clear();
        for proof in proofs {
            self.tails
                .insert(proof.destination_token.clone(), proof.proof_hash.clone());
        }
        Ok(())
    }

    pub fn reset_destination(&mut self, destination: &DestinationKey) {
        self.tails.remove(&self.token_for(destination));
    }

    pub fn reset_all(&mut self) {
        self.tails.clear();
    }

    fn mac(&self) -> HmacSha256 {
        HmacSha256::new_from_slice(&self.secret).expect("HMAC accepts a 32-byte key")
    }

    fn sign_proof(&self, proof: &RouteProof) -> String {
        let mut mac = self.mac();
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
    #[error("route proof key must contain exactly 32 bytes")]
    InvalidKeyLength,
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

    fn destination(host: &str) -> DestinationKey {
        DestinationKey {
            host: host.into(),
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
    fn rejects_wrong_key_length() {
        assert!(matches!(
            RouteProofChain::try_from_secret(&[1; 16]),
            Err(RouteProofError::InvalidKeyLength)
        ));
    }

    #[test]
    fn proof_contains_no_raw_destination_identity() {
        let mut chain = RouteProofChain::new([7; 32]);
        let proof = chain.append(
            &destination("private.example"),
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
            &destination("private.example"),
            "node-a",
            "best score",
            vec![evidence("node-a", 91.0)],
            Utc::now(),
        );
        proof.evidence[0].score = 12.0;

        assert_eq!(chain.verify(&proof), Err(RouteProofError::InvalidSignature));
    }

    #[test]
    fn verifies_interleaved_destination_chains() {
        let mut chain = RouteProofChain::new([11; 32]);
        let first_a = chain.append(
            &destination("a.example"),
            "node-a",
            "a1",
            vec![],
            Utc::now(),
        );
        let first_b = chain.append(
            &destination("b.example"),
            "node-b",
            "b1",
            vec![],
            Utc::now(),
        );
        let second_a = chain.append(
            &destination("a.example"),
            "node-c",
            "a2",
            vec![],
            Utc::now(),
        );

        assert!(chain
            .verify_chain(&[first_a.clone(), first_b, second_a.clone()])
            .is_ok());
        assert_eq!(
            chain.verify_chain(&[second_a]),
            Err(RouteProofError::BrokenChain)
        );
    }

    #[test]
    fn clearing_one_destination_does_not_break_another() {
        let mut chain = RouteProofChain::new([13; 32]);
        let a = destination("a.example");
        let b = destination("b.example");
        let first_a = chain.append(&a, "node-a", "a1", vec![], Utc::now());
        let first_b = chain.append(&b, "node-b", "b1", vec![], Utc::now());

        chain.reset_destination(&a);
        let new_a = chain.append(&a, "node-c", "a-new", vec![], Utc::now());
        let second_b = chain.append(&b, "node-d", "b2", vec![], Utc::now());

        assert!(new_a.previous_proof_hash.is_none());
        assert_eq!(
            second_b.previous_proof_hash.as_deref(),
            Some(first_b.proof_hash.as_str())
        );
        assert!(chain.verify_chain(&[first_a, first_b, second_b]).is_ok());
    }

    #[test]
    fn different_installation_secrets_create_unlinkable_tokens() {
        let mut first = RouteProofChain::new([1; 32]);
        let mut second = RouteProofChain::new([2; 32]);
        let at = Utc::now();

        let a = first.append(&destination("same.example"), "node", "reason", vec![], at);
        let b = second.append(&destination("same.example"), "node", "reason", vec![], at);

        assert_ne!(a.destination_token, b.destination_token);
    }
}

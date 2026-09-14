use amri_core::ProbeSample;
use chrono::Utc;
use std::net::{TcpStream, ToSocketAddrs};
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProbeError {
    #[error("DNS resolution failed")]
    Dns,
    #[error("TCP connection failed")]
    Tcp,
}

#[derive(Debug, Clone)]
pub struct TcpProbeConfig {
    pub timeout: Duration,
}

impl Default for TcpProbeConfig {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(3),
        }
    }
}

pub fn probe_tcp(host: &str, port: u16, config: &TcpProbeConfig) -> Result<ProbeSample, ProbeError> {
    let dns_started = Instant::now();
    let mut addrs = (host, port).to_socket_addrs().map_err(|_| ProbeError::Dns)?;
    let dns_ms = dns_started.elapsed().as_secs_f64() * 1000.0;
    let addr = addrs.next().ok_or(ProbeError::Dns)?;

    let connect_started = Instant::now();
    let result = TcpStream::connect_timeout(&addr, config.timeout);
    let connect_ms = connect_started.elapsed().as_secs_f64() * 1000.0;

    let success = result.is_ok();
    if !success {
        return Ok(ProbeSample {
            measured_at: Utc::now(),
            latency_ms: connect_ms,
            jitter_ms: 0.0,
            packet_loss_ratio: 1.0,
            tcp_connect_ms: Some(connect_ms),
            tls_handshake_ms: None,
            dns_ms: Some(dns_ms),
            download_mbps: None,
            upload_mbps: None,
            success: false,
        });
    }

    Ok(ProbeSample {
        measured_at: Utc::now(),
        latency_ms: connect_ms,
        jitter_ms: 0.0,
        packet_loss_ratio: 0.0,
        tcp_connect_ms: Some(connect_ms),
        tls_handshake_ms: None,
        dns_ms: Some(dns_ms),
        download_mbps: None,
        upload_mbps: None,
        success: true,
    })
}

pub fn summarize_latency(samples: &[ProbeSample]) -> Option<(f64, f64, f64)> {
    let mut values: Vec<f64> = samples
        .iter()
        .filter(|sample| sample.success)
        .map(|sample| sample.latency_ms)
        .collect();

    if values.is_empty() {
        return None;
    }

    values.sort_by(|a, b| a.total_cmp(b));
    let mean = values.iter().sum::<f64>() / values.len() as f64;
    let min = *values.first().unwrap();
    let max = *values.last().unwrap();
    Some((mean, min, max))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn summarizes_latency() {
        let samples = vec![
            ProbeSample::basic(20.0, 0.0, 0.0),
            ProbeSample::basic(30.0, 0.0, 0.0),
            ProbeSample::basic(40.0, 0.0, 0.0),
        ];
        let (mean, min, max) = summarize_latency(&samples).unwrap();
        assert_eq!(mean, 30.0);
        assert_eq!(min, 20.0);
        assert_eq!(max, 40.0);
    }
}

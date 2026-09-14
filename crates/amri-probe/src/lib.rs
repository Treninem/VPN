use amri_core::ProbeSample;
use chrono::Utc;
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::{mpsc, Arc};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error, Clone, Copy, PartialEq, Eq)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProbeTarget {
    pub id: String,
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone)]
pub struct ProbeRaceConfig {
    /// Per-candidate TCP timeout. Hot-pool races are intentionally much shorter than deep probes.
    pub per_target_timeout: Duration,
    /// Hard budget visible to the route-selection caller.
    pub overall_timeout: Duration,
    /// Small window after the first success to allow an almost-simultaneous better result to win.
    pub settle_window: Duration,
    /// Hard cap to avoid spawning a thread for a large subscription pool.
    pub max_parallel: usize,
}

impl Default for ProbeRaceConfig {
    fn default() -> Self {
        Self {
            per_target_timeout: Duration::from_millis(650),
            overall_timeout: Duration::from_millis(750),
            settle_window: Duration::from_millis(35),
            max_parallel: 4,
        }
    }
}

#[derive(Debug, Clone)]
pub enum ProbeAttemptOutcome {
    Sample(ProbeSample),
    Error(ProbeError),
}

#[derive(Debug, Clone)]
pub struct ProbeAttempt {
    pub target: ProbeTarget,
    pub completed_in: Duration,
    pub outcome: ProbeAttemptOutcome,
}

impl ProbeAttempt {
    pub fn is_success(&self) -> bool {
        matches!(&self.outcome, ProbeAttemptOutcome::Sample(sample) if sample.success)
    }

    pub fn quality_latency_ms(&self) -> Option<f64> {
        match &self.outcome {
            ProbeAttemptOutcome::Sample(sample) if sample.success => {
                Some(sample.tcp_connect_ms.unwrap_or(sample.latency_ms))
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProbeRaceOutcome {
    pub winner: Option<ProbeAttempt>,
    pub attempts: Vec<ProbeAttempt>,
    /// Number of targets actually started. Targets beyond `max_parallel` are intentionally ignored.
    pub started: usize,
}

pub fn probe_tcp(
    host: &str,
    port: u16,
    config: &TcpProbeConfig,
) -> Result<ProbeSample, ProbeError> {
    let dns_started = Instant::now();
    let mut addrs = (host, port)
        .to_socket_addrs()
        .map_err(|_| ProbeError::Dns)?;
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

/// Runs a short parallel TCP race over an already-small hot pool.
///
/// This is intentionally not a scanner for every subscription node. The caller should pass only
/// candidates previously admitted to the hot pool. At most `max_parallel` attempts are started.
/// Once the first successful attempt arrives, AMRI waits only `settle_window` for another
/// near-simultaneous result, then returns the best successful sample seen in that window.
pub fn race_tcp_hot_pool(targets: &[ProbeTarget], config: &ProbeRaceConfig) -> ProbeRaceOutcome {
    race_with_probe(targets, config, |target, timeout| {
        probe_tcp(&target.host, target.port, &TcpProbeConfig { timeout })
    })
}

fn race_with_probe<F>(
    targets: &[ProbeTarget],
    config: &ProbeRaceConfig,
    probe: F,
) -> ProbeRaceOutcome
where
    F: Fn(&ProbeTarget, Duration) -> Result<ProbeSample, ProbeError> + Send + Sync + 'static,
{
    let started = targets.len().min(config.max_parallel);
    if started == 0 || config.overall_timeout.is_zero() {
        return ProbeRaceOutcome {
            winner: None,
            attempts: Vec::new(),
            started: 0,
        };
    }

    let started_at = Instant::now();
    let overall_deadline = started_at + config.overall_timeout;
    let probe = Arc::new(probe);
    let (sender, receiver) = mpsc::channel();

    for target in targets.iter().take(started).cloned() {
        let sender = sender.clone();
        let probe = Arc::clone(&probe);
        let timeout = config.per_target_timeout;
        thread::spawn(move || {
            let outcome = match probe(&target, timeout) {
                Ok(sample) => ProbeAttemptOutcome::Sample(sample),
                Err(error) => ProbeAttemptOutcome::Error(error),
            };
            let _ = sender.send(ProbeAttempt {
                target,
                completed_in: started_at.elapsed(),
                outcome,
            });
        });
    }
    drop(sender);

    let mut attempts = Vec::with_capacity(started);
    let mut settle_deadline: Option<Instant> = None;

    while attempts.len() < started {
        let now = Instant::now();
        let deadline = settle_deadline
            .map(|settle| std::cmp::min(settle, overall_deadline))
            .unwrap_or(overall_deadline);
        if now >= deadline {
            break;
        }

        match receiver.recv_timeout(deadline.saturating_duration_since(now)) {
            Ok(attempt) => {
                let first_success = attempt.is_success() && settle_deadline.is_none();
                attempts.push(attempt);
                if first_success {
                    settle_deadline = Some(Instant::now() + config.settle_window);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => break,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        }
    }

    attempts.sort_by(|a, b| a.completed_in.cmp(&b.completed_in));
    let winner = attempts
        .iter()
        .filter_map(|attempt| {
            attempt
                .quality_latency_ms()
                .map(|quality| (attempt, quality))
        })
        .min_by(|(a, a_quality), (b, b_quality)| {
            a_quality
                .total_cmp(b_quality)
                .then_with(|| a.completed_in.cmp(&b.completed_in))
        })
        .map(|(attempt, _)| attempt.clone());

    ProbeRaceOutcome {
        winner,
        attempts,
        started,
    }
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
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn target(id: &str) -> ProbeTarget {
        ProbeTarget {
            id: id.into(),
            host: "example.invalid".into(),
            port: 443,
        }
    }

    fn sample(latency_ms: f64, success: bool) -> ProbeSample {
        ProbeSample {
            measured_at: Utc::now(),
            latency_ms,
            jitter_ms: 0.0,
            packet_loss_ratio: if success { 0.0 } else { 1.0 },
            tcp_connect_ms: Some(latency_ms),
            tls_handshake_ms: None,
            dns_ms: None,
            download_mbps: None,
            upload_mbps: None,
            success,
        }
    }

    fn race_config() -> ProbeRaceConfig {
        ProbeRaceConfig {
            per_target_timeout: Duration::from_millis(100),
            overall_timeout: Duration::from_millis(120),
            settle_window: Duration::from_millis(30),
            max_parallel: 4,
        }
    }

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

    #[test]
    fn settle_window_can_choose_better_near_simultaneous_candidate() {
        let targets = vec![target("first"), target("better")];
        let outcome = race_with_probe(&targets, &race_config(), |target, _| {
            match target.id.as_str() {
                "first" => {
                    thread::sleep(Duration::from_millis(5));
                    Ok(sample(80.0, true))
                }
                "better" => {
                    thread::sleep(Duration::from_millis(15));
                    Ok(sample(20.0, true))
                }
                _ => unreachable!(),
            }
        });

        assert_eq!(outcome.winner.unwrap().target.id, "better");
        assert_eq!(outcome.attempts.len(), 2);
    }

    #[test]
    fn first_success_ends_race_after_small_settle_window() {
        let targets = vec![target("fast"), target("slow")];
        let mut config = race_config();
        config.settle_window = Duration::from_millis(10);
        config.overall_timeout = Duration::from_millis(200);

        let outcome = race_with_probe(&targets, &config, |target, _| {
            if target.id == "fast" {
                thread::sleep(Duration::from_millis(3));
                Ok(sample(25.0, true))
            } else {
                thread::sleep(Duration::from_millis(80));
                Ok(sample(15.0, true))
            }
        });

        assert_eq!(outcome.winner.unwrap().target.id, "fast");
        assert_eq!(outcome.attempts.len(), 1);
    }

    #[test]
    fn failed_attempt_does_not_stop_later_success() {
        let targets = vec![target("failed"), target("good")];
        let outcome = race_with_probe(&targets, &race_config(), |target, _| {
            if target.id == "failed" {
                thread::sleep(Duration::from_millis(3));
                Ok(sample(3.0, false))
            } else {
                thread::sleep(Duration::from_millis(12));
                Ok(sample(30.0, true))
            }
        });

        assert_eq!(outcome.winner.unwrap().target.id, "good");
        assert_eq!(outcome.attempts.len(), 2);
    }

    #[test]
    fn race_never_starts_more_than_parallel_cap() {
        let targets = vec![
            target("a"),
            target("b"),
            target("c"),
            target("d"),
            target("e"),
        ];
        let calls = Arc::new(AtomicUsize::new(0));
        let probe_calls = Arc::clone(&calls);
        let mut config = race_config();
        config.max_parallel = 2;

        let outcome = race_with_probe(&targets, &config, move |_, _| {
            probe_calls.fetch_add(1, Ordering::SeqCst);
            Ok(sample(10.0, true))
        });

        assert_eq!(outcome.started, 2);
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }
}

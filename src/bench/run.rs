use crate::bench::TopologyKind;
use crate::commit::{CommitQueue, Committer};
use crate::record::Claim;
use crate::store::{Store, StoreError};
use serde::Serialize;
use std::path::Path;
use std::time::Instant;

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("could not open the store for the run: {0}")]
    Store(#[from] StoreError),
    #[error("an agent thread panicked, so this run measured nothing")]
    AgentPanicked,
}

#[derive(Clone, Copy, Debug)]
pub struct RunConfig {
    pub topology: TopologyKind,
    pub agents: usize,
    pub claims_per_agent: usize,
}

/// Latency in microseconds, submit to `Ack`.
///
/// Percentiles rather than a mean: the mean hides exactly the tail a gateway
/// waits on, and the tail is the number that decides whether fail-closed
/// logging is deployable.
#[derive(Clone, Debug, Serialize)]
pub struct Latency {
    pub p50_us: f64,
    pub p95_us: f64,
    pub p99_us: f64,
    pub max_us: f64,
}

/// Batch statistics from `QueueStats`. This population is every claim the
/// committer processed, including claims in a batch that ultimately failed
/// to ack — i.e. it is sized by `submitted`, not by `claims`.
#[derive(Clone, Debug, Serialize)]
pub struct Batches {
    pub count: u64,
    pub mean_size: f64,
    pub max_size: usize,
}

/// `RunReport` mixes two different populations, and each field below says
/// which one it belongs to:
///
/// - `submitted` and `batches` cover every claim the harness attempted to
///   submit, whether or not it was ever acked.
/// - `claims`, `claims_per_sec`, and `latency` cover only claims whose
///   `submit` call returned `Ok`.
///
/// The distinction is not cosmetic: `Committer` latches `unavailable` after
/// any `Write`/`Sync` error and then refuses every later batch for the rest
/// of the run, so a single transient fsync failure partway through a run can
/// silently fail the back half of it. Without `submitted`/`failed` next to
/// `claims`, a report from such a run reads as a smaller, but otherwise
/// healthy, run — exactly the wrong failure mode to hide for a benchmark of
/// a fail-closed evidence log.
#[derive(Clone, Debug, Serialize)]
pub struct RunReport {
    pub topology: String,
    pub agents: usize,
    /// Claims attempted: `agents * claims_per_agent`, regardless of outcome.
    pub submitted: usize,
    /// Claims whose `submit` call returned `Ok`. Everything below this field
    /// except `batches` and `fsyncs` is computed only over this population.
    pub claims: usize,
    /// `submitted - claims`: claims whose `submit` call returned `Err`.
    pub failed: usize,
    pub wall_secs: f64,
    /// `claims / wall_secs` — successfully acked claims only.
    pub claims_per_sec: f64,
    pub latency: Latency,
    pub batches: Batches,
    /// The physical quantity the throughput figure follows from.
    pub fsyncs: u64,
}

/// Drive the queue with `agents` threads and measure what comes back.
///
/// Every latency here is measured around a real submit-to-`Ack` round trip.
/// Nothing is modelled, extrapolated, or inferred from batch timings.
pub fn run(cfg: &RunConfig, dir: &Path) -> Result<RunReport, RunError> {
    let queue = CommitQueue::new(Committer::new(Store::open(dir)?));

    let started = Instant::now();
    let mut threads = Vec::with_capacity(cfg.agents);
    for a in 0..cfg.agents {
        let h = queue.handle();
        let per = cfg.claims_per_agent;
        threads.push(std::thread::spawn(move || {
            let agent = format!("agent-{a:05}");
            let mut samples = Vec::with_capacity(per);
            for i in 0..per {
                let c = claim(&agent, i);
                let t = Instant::now();
                let ack = h.submit(c, 1_700_000_000);
                let us = t.elapsed().as_secs_f64() * 1e6;
                if ack.is_ok() {
                    samples.push(us);
                }
            }
            samples
        }));
    }

    let mut samples = Vec::new();
    for t in threads {
        samples.extend(t.join().map_err(|_| RunError::AgentPanicked)?);
    }
    let wall = started.elapsed().as_secs_f64();
    let stats = queue.shutdown();

    let mean_size = if stats.batches == 0 {
        0.0
    } else {
        stats.claims as f64 / stats.batches as f64
    };

    let submitted = cfg.agents * cfg.claims_per_agent;
    let claims = samples.len();

    Ok(RunReport {
        topology: cfg.topology.name().to_string(),
        agents: cfg.agents,
        submitted,
        claims,
        failed: submitted.saturating_sub(claims),
        wall_secs: wall,
        claims_per_sec: if wall > 0.0 {
            claims as f64 / wall
        } else {
            0.0
        },
        latency: percentiles(&mut samples),
        batches: Batches {
            count: stats.batches,
            mean_size,
            max_size: stats.batch_sizes.iter().copied().max().unwrap_or(0),
        },
        fsyncs: stats.fsyncs,
    })
}

/// The claim an agent submits. Every field is fixed except the identity
/// ones, and those are zero-padded to a fixed width (`{a:05}`, `{i:05}`) so
/// every claim in the matrix serializes to the same byte length regardless
/// of how far into a run it was submitted. Without the padding, `action_id`
/// and `nonce` grow with the digit count of `a` and `i`, which would make
/// later claims in a run — and runs with more agents — systematically
/// larger, contaminating a throughput comparison across the agent sweep
/// with a comparison of serialization size instead of concurrency.
///
/// `agent` is expected to already be in the padded `agent-NNNNN` form (see
/// `run`, which builds it once per thread with `format!("agent-{a:05}")`).
/// Five digits give headroom to 99,999 agents/claims, far beyond the
/// matrix's 1000-agent, 20-claims-per-agent range.
pub fn claim(agent: &str, i: usize) -> Claim {
    Claim {
        action_id: format!("{agent}-{i:05}"),
        agent_id: agent.to_string(),
        initiating_user: "user:alice".into(),
        interception_point: "PRE_CALL_TOOL_INVOCATION".into(),
        target_resource: "/v1/charges".into(),
        canonical_snapshot_hash: "54c323d3".into(),
        path_summary_hash: None,
        policy_bundle_hash: "a7713be5".into(),
        verdict: "ALLOW".into(),
        nonce: format!("n-{agent}-{i:05}"),
    }
}

/// Nearest-rank percentiles over the sorted samples.
fn percentiles(samples: &mut [f64]) -> Latency {
    if samples.is_empty() {
        return Latency {
            p50_us: f64::NAN,
            p95_us: f64::NAN,
            p99_us: f64::NAN,
            max_us: f64::NAN,
        };
    }
    samples.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let at = |q: f64| -> f64 {
        let rank = (q * samples.len() as f64).ceil() as usize;
        let idx = rank.saturating_sub(1).min(samples.len() - 1);
        samples[idx]
    };
    Latency {
        p50_us: at(0.50),
        p95_us: at(0.95),
        p99_us: at(0.99),
        max_us: at(1.0),
    }
}

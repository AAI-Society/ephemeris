//! Runs the measurement matrix and writes `results/<topology>.json`.
//!
//!     cargo run --release --features bench --bin ephemeris-bench
//!
//! `--release` matters: a debug build measures rustc's bounds checks as much
//! as the log.

use anyhow::{Context, Result};
use clap::Parser;
use ephemeris::bench::host::Host;
use ephemeris::bench::linkability::measure;
use ephemeris::bench::run::{run, RunConfig, RunReport};
use ephemeris::bench::TopologyKind;
use serde::Serialize;
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "ephemeris-bench",
    about = "Measure both axes of the topology trade-off"
)]
struct Cli {
    /// Where to write results.
    #[arg(long, default_value = "results")]
    out: PathBuf,
    /// Agent counts to sweep.
    #[arg(long, value_delimiter = ',', default_value = "1,10,100,1000")]
    agents: Vec<usize>,
    /// Claims each agent submits.
    #[arg(long, default_value_t = 20)]
    claims_per_agent: usize,
}

#[derive(Serialize)]
struct TopologyResults {
    topology: String,
    host: Host,
    /// Recorded before any number was measured, so a contradicting run reads
    /// as a contradiction rather than quietly becoming the new expectation.
    prediction: &'static str,
    throughput: Vec<RunReport>,
    linkability: Vec<serde_json::Value>,
}

const PREDICTION: &str = "All three topologies share one store and one fsync per batch, \
so throughput is expected to be mean_batch_size * (1e6 / sync_us) claims/second for every \
topology, and the topology axis is expected to barely move it. Linkability is expected to \
rank global best, per-agent worst, and joint exactly equal to per-agent.";

fn main() -> Result<()> {
    let cli = Cli::parse();
    std::fs::create_dir_all(&cli.out)
        .with_context(|| format!("could not create {}", cli.out.display()))?;

    let scratch = tempfile::tempdir().context("could not create a scratch directory")?;
    let host = Host::detect(scratch.path());
    println!(
        "host: {} {} · {} · {} cores · {} = {:.0} us",
        host.os, host.arch, host.cpu_model, host.logical_cores, host.sync_call, host.sync_us
    );

    for kind in TopologyKind::ALL {
        let mut throughput = Vec::new();
        let mut links = Vec::new();

        for &agents in &cli.agents {
            let dir = tempfile::tempdir().context("could not create a run directory")?;
            let cfg = RunConfig {
                topology: kind,
                agents,
                claims_per_agent: cli.claims_per_agent,
            };
            let report = run(&cfg, dir.path()).context("the run failed")?;
            println!(
                "  {:9} agents={:<5} {:>9.1} claims/s  p99={:>9.1}us  batch={:.1}  fsyncs={}",
                kind.name(),
                agents,
                report.claims_per_sec,
                report.latency.p99_us,
                report.batches.mean_size,
                report.fsyncs
            );
            throughput.push(report);

            // NOTE: deviation from the brief. The brief used
            // `format!("agent-{a}")` here, unpadded. `src/bench/run.rs`
            // builds the agent ids the throughput run actually used with
            // `format!("agent-{a:05}")` (zero-padded, per Task 3's review,
            // so identity length does not contaminate the throughput
            // measurement). The two halves of one results file — the
            // `throughput` array and the `linkability` array — must
            // describe the same set of agents, so this must build the same
            // padded ids or `TopologyKind::tree_for` would be given inputs
            // the throughput run never used.
            let per_agent: Vec<(String, usize)> = (0..agents)
                .map(|a| (format!("agent-{a:05}"), cli.claims_per_agent))
                .collect();
            let l = measure(kind, &per_agent).context("linkability measurement failed")?;
            links.push(serde_json::to_value(l)?);
        }

        let out = TopologyResults {
            topology: kind.name().to_string(),
            host: host.clone(),
            prediction: PREDICTION,
            throughput,
            linkability: links,
        };
        let path = cli.out.join(format!("{}.json", kind.name()));
        let text = serde_json::to_string_pretty(&out)?;
        std::fs::write(&path, text)
            .with_context(|| format!("could not write {}", path.display()))?;
        println!("  wrote {}", path.display());
    }

    Ok(())
}

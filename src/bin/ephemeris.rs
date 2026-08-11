use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use ephemeris::commit::Committer;
use ephemeris::record::Claim;
use ephemeris::sign::{Signer, SoftwareSigner};
use ephemeris::store::Store;
use ephemeris::verify::{against_root, PriorRoot};
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "ephemeris",
    version,
    about = "An append-only evidence log for Proof-of-Control records"
)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Append one claim, read from a JSON file.
    Append {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long)]
        claim: PathBuf,
        /// Injected, never read from the system clock.
        #[arg(long)]
        now: u64,
    },
    /// Print the signed tree head.
    Root {
        #[arg(long)]
        dir: PathBuf,
    },
    /// Verify the log, optionally against a root published earlier.
    Verify {
        #[arg(long)]
        dir: PathBuf,
        #[arg(long, requires = "prior_root")]
        prior_size: Option<u64>,
        #[arg(long, requires = "prior_size")]
        prior_root: Option<String>,
    },
}

/// A fixed seed so Phase 1 output is reproducible. A persistent software key
/// would imply a custody story this build does not have.
///
/// This is safe here only because `SoftwareSigner` claims nothing: its roots
/// render `SOFTWARE`, and that label already concedes "no defence against the
/// operator" — a predictable key betrays nothing a reader wasn't already
/// told. `TeeSigner` had a comparable seed parameter once; it does not
/// anymore. `TeeSigner::generate` and `generate_at` take no seed at all and
/// draw the key from the OS CSPRNG unconditionally, because their roots
/// render `custody TEE` — a claim a predictable key would falsify — so the
/// constant that is harmless on this line has no counterpart to be copied
/// into. If a `TeeSigner` constructor is ever added that accepts key
/// material from outside the TEE, this constant is not a template for it.
const DEV_SEED: [u8; 32] = [7u8; 32];

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn run() -> Result<ExitCode> {
    let cli = Cli::parse();
    match cli.cmd {
        Cmd::Append { dir, claim, now } => {
            let text = std::fs::read_to_string(&claim)
                .with_context(|| format!("could not read {}", claim.display()))?;
            let c: Claim = serde_json::from_str(&text)
                .with_context(|| format!("could not parse {}", claim.display()))?;
            let store = Store::open(&dir)?;
            let mut committer = Committer::new(store);
            let acks = committer.submit(vec![c], now);
            match acks.into_iter().next() {
                Some(Ok(a)) => {
                    println!("acked step {} head {}", a.step_index(), a.chain_head());
                    Ok(ExitCode::SUCCESS)
                }
                Some(Err(e)) => {
                    eprintln!("{e}");
                    Ok(ExitCode::from(1))
                }
                None => {
                    eprintln!("no acknowledgement was produced");
                    Ok(ExitCode::from(1))
                }
            }
        }
        Cmd::Root { dir } => {
            let store = Store::open(&dir)?;
            let accepted = against_root(store.records(), None)?;
            let signer = SoftwareSigner::generate(DEV_SEED);
            println!(
                "{}",
                signer.sign_root(&accepted.root, accepted.size).render()
            );
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Verify {
            dir,
            prior_size,
            prior_root,
        } => {
            let store = Store::open(&dir)?;
            let prior = match (prior_size, prior_root) {
                (Some(size), Some(hexed)) => {
                    let raw = hex::decode(&hexed).context("--prior-root is not hex")?;
                    let root: [u8; 32] = raw
                        .try_into()
                        .map_err(|_| anyhow::anyhow!("--prior-root must be 32 bytes"))?;
                    Some(PriorRoot { size, root })
                }
                _ => None,
            };
            match against_root(store.records(), prior) {
                Ok(a) => {
                    println!("accepted: size {} root {}", a.size, hex::encode(a.root));
                    Ok(ExitCode::SUCCESS)
                }
                Err(e) => {
                    eprintln!("{e}");
                    Ok(ExitCode::from(1))
                }
            }
        }
    }
}

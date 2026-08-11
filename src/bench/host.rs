use serde::Serialize;
use std::io::Write;
use std::path::Path;
use std::time::Instant;

/// The machine a measurement came from. Committed alongside every result,
/// because a laptop number is not a production number and the file should say
/// so rather than the README saying it once.
#[derive(Clone, Debug, Serialize)]
pub struct Host {
    pub os: String,
    pub arch: String,
    pub cpu_model: String,
    pub logical_cores: usize,
    /// The call `store::append_batch` makes.
    pub sync_call: String,
    /// **Measured**, not assumed. On macOS `File::sync_data` is
    /// `F_FULLFSYNC` and costs milliseconds; on Linux it is `fdatasync` and
    /// typically does not force a drive cache flush at all. A throughput
    /// number is meaningless without this figure next to it, and the two
    /// platforms are not comparable.
    pub sync_us: f64,
    pub crate_commit: String,
}

impl Host {
    pub fn detect(dir: &Path) -> Host {
        Host {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            cpu_model: cpu_model(),
            logical_cores: std::thread::available_parallelism()
                .map(|n| n.get())
                .unwrap_or(0),
            sync_call: "std::fs::File::sync_data".to_string(),
            sync_us: measure_sync_us(dir),
            crate_commit: crate_commit(),
        }
    }
}

/// Time one `write` + `sync_data` pair, the same sequence `append_batch`
/// performs. Returns the mean over `N` iterations in microseconds, or `f64::NAN`
/// if the probe could not run — a NaN in the results file is a visible absence
/// rather than a plausible zero.
fn measure_sync_us(dir: &Path) -> f64 {
    const N: u32 = 50;
    let path = dir.join(".sync-probe");
    let mut f = match std::fs::OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(true)
        .open(&path)
    {
        Ok(f) => f,
        Err(_) => return f64::NAN,
    };
    let start = Instant::now();
    for _ in 0..N {
        if f.write_all(b"sync-probe\n").is_err() || f.sync_data().is_err() {
            let _ = std::fs::remove_file(&path);
            return f64::NAN;
        }
    }
    let per = start.elapsed().as_secs_f64() / f64::from(N) * 1e6;
    let _ = std::fs::remove_file(&path);
    per
}

fn cpu_model() -> String {
    #[cfg(target_os = "macos")]
    let probe = std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output();
    #[cfg(not(target_os = "macos"))]
    let probe = std::process::Command::new("sh")
        .args([
            "-c",
            "grep -m1 'model name' /proc/cpuinfo | cut -d: -f2- | sed 's/^ *//'",
        ])
        .output();

    match probe {
        Ok(o) if o.status.success() => {
            let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if s.is_empty() {
                "unknown".to_string()
            } else {
                s
            }
        }
        _ => "unknown".to_string(),
    }
}

fn crate_commit() -> String {
    match std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
    {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout).trim().to_string(),
        _ => "unknown".to_string(),
    }
}

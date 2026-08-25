//! Reading-path memory bench: process memory samples for the frontend runner
//! in `src/lib/bench.ts`.
//!
//! The runner walks every message through the reading pane and records how the
//! web process's memory moves as it goes. It needs two things only this side
//! can provide — whether a bench was requested, and the memory of this process
//! and the WebKit children — so those are the only commands here. The runner
//! owns the walk, the pacing and the log format.
//!
//! Inert unless both hold: a debug build, and `AGENTIC_MAIL_BENCH=1` in the
//! environment. Release builds register the commands but `bench_config` always
//! answers `None`, so the frontend never starts. Sampling reads `/proc` and is
//! Linux-only; `scripts/bench-reading.sh` is the intended entry point.

use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BenchConfig {
    /// Full passes over every message. The first pass pays one-time costs
    /// (fonts, decoders, JIT); growth that persists into later rounds is the
    /// signal.
    pub rounds: u32,
    /// Pause after a message reports rendered, before sampling — lets image
    /// decode and layout finish.
    pub settle_ms: u64,
    /// Idle pause at the end of each round, sampled again afterwards, so a GC
    /// that only runs when nothing is happening gets its chance.
    pub round_idle_ms: u64,
    /// Give up on a message whose body or render never arrives.
    pub timeout_ms: u64,
    /// Cap per account per round; 0 is every message.
    pub limit: usize,
    /// Where the CSV goes.
    pub log_path: String,
}

#[derive(Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BenchSample {
    pub ts_ms: u128,
    pub main_anon_kb: u64,
    pub main_rss_kb: u64,
    pub web_anon_kb: u64,
    pub web_rss_kb: u64,
    pub net_anon_kb: u64,
    pub web_process_count: u32,
}

fn env_u64(name: &str, default: u64) -> u64 {
    std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
}

/// `Some` only when a bench was asked for on a debug build.
#[tauri::command]
pub fn bench_config() -> Option<BenchConfig> {
    if !cfg!(debug_assertions) || std::env::var("AGENTIC_MAIL_BENCH").ok().as_deref() != Some("1") {
        return None;
    }
    let default_log = dirs_home().join(".cortexist").join("mail").join("bench-reading.csv");
    Some(BenchConfig {
        rounds: env_u64("AGENTIC_MAIL_BENCH_ROUNDS", 3) as u32,
        settle_ms: env_u64("AGENTIC_MAIL_BENCH_SETTLE_MS", 1500),
        round_idle_ms: env_u64("AGENTIC_MAIL_BENCH_ROUND_IDLE_MS", 15_000),
        timeout_ms: env_u64("AGENTIC_MAIL_BENCH_TIMEOUT_MS", 10_000),
        limit: env_u64("AGENTIC_MAIL_BENCH_LIMIT", 0) as usize,
        log_path: std::env::var("AGENTIC_MAIL_BENCH_LOG")
            .unwrap_or_else(|_| default_log.to_string_lossy().into_owned()),
    })
}

fn dirs_home() -> std::path::PathBuf {
    std::env::var_os("HOME").map(Into::into).unwrap_or_else(|| std::path::PathBuf::from("."))
}

/// Memory of this process and its WebKit children, from `/proc`.
#[tauri::command]
pub fn bench_sample() -> Result<BenchSample, String> {
    #[cfg(not(target_os = "linux"))]
    {
        Err("bench sampling reads /proc and is Linux-only".to_string())
    }
    #[cfg(target_os = "linux")]
    {
        let me = std::process::id();
        let (main_anon_kb, main_rss_kb) = smaps_rollup(me);
        let mut sample = BenchSample {
            ts_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis())
                .unwrap_or(0),
            main_anon_kb,
            main_rss_kb,
            ..Default::default()
        };
        for (pid, comm) in children_of(me) {
            let (anon, rss) = smaps_rollup(pid);
            if comm.starts_with("WebKitWebProc") {
                sample.web_anon_kb += anon;
                sample.web_rss_kb += rss;
                sample.web_process_count += 1;
            } else if comm.starts_with("WebKitNetworkP") {
                sample.net_anon_kb += anon;
            }
        }
        Ok(sample)
    }
}

/// Append one line to the bench log. The runner writes the header itself.
#[tauri::command]
pub fn bench_append(path: String, line: String) -> Result<(), String> {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{path}: {e}"))?;
    writeln!(file, "{line}").map_err(|e| e.to_string())
}

/// (Anonymous, Rss) in kB for `pid`, zero if unreadable.
#[cfg(target_os = "linux")]
fn smaps_rollup(pid: u32) -> (u64, u64) {
    let Ok(text) = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")) else {
        return (0, 0);
    };
    let field = |name: &str| {
        text.lines()
            .find(|l| l.starts_with(name))
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0)
    };
    (field("Anonymous:"), field("Rss:"))
}

/// Direct children of `parent` as (pid, comm).
#[cfg(target_os = "linux")]
fn children_of(parent: u32) -> Vec<(u32, String)> {
    let Ok(entries) = std::fs::read_dir("/proc") else { return Vec::new() };
    entries
        .filter_map(|e| e.ok())
        .filter_map(|e| e.file_name().to_str()?.parse::<u32>().ok())
        .filter_map(|pid| {
            let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
            // comm is parenthesised and may contain spaces; ppid is the field after it.
            let open = stat.find('(')?;
            let close = stat.rfind(')')?;
            let comm = stat[open + 1..close].to_string();
            let ppid: u32 = stat[close + 1..].split_whitespace().nth(1)?.parse().ok()?;
            (ppid == parent).then_some((pid, comm))
        })
        .collect()
}

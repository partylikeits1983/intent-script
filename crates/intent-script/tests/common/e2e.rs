//! End-to-end test harness for the `advisor` binary.
//!
//! Spawns a seeded local Anvil (via `scripts/start-anvil.sh`) on a free port,
//! runs the `advisor` binary against it with `--simulate --rpc <local>`, and
//! returns its captured output for assertion.
//!
//! Skip-not-fail when prerequisites are missing: `OPENAI_API_KEY`, `anvil`,
//! `cast`, or `forge`. The whole stack is gated behind the `advisor` feature
//! at the Cargo level, so this module only compiles when the advisor deps
//! pull in.

use std::net::TcpListener;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

/// The intent-script repo root (one above `crates/`).
pub fn repo_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR has no grandparent")
        .to_path_buf()
}

/// Reasons we'd skip an e2e test instead of failing it. Returns `None` when
/// everything is ready, `Some(reason)` when the test should be skipped.
pub fn skip_reason() -> Option<String> {
    if std::env::var("OPENAI_API_KEY").is_err() {
        let dotenv = repo_root().join(".env");
        if !dotenv.is_file() {
            return Some("OPENAI_API_KEY not set and intent-script/.env not found".into());
        }
    }
    for bin in ["anvil", "cast", "forge", "jq", "bash"] {
        if Command::new(bin)
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_err()
        {
            return Some(format!("missing prerequisite on PATH: {bin}"));
        }
    }
    None
}

/// A running anvil + seeded router, torn down in `Drop`.
///
/// `start-anvil.sh` spawns `anvil` as a child, then traps SIGINT/SIGTERM/EXIT
/// to kill it on shutdown. Plain `Child::kill()` sends SIGKILL, which can't
/// be trapped — the bash dies but anvil orphans. To avoid that, we put the
/// bash script in its own process group at spawn time, then SIGTERM the
/// entire group on Drop so bash's trap fires and reaps anvil cleanly.
pub struct AnvilGuard {
    child: Child,
    pgid: i32,
    pub port: u16,
    pub rpc: String,
}

impl AnvilGuard {
    /// Spawn anvil via `scripts/start-anvil.sh` on a free port and wait until
    /// the IntentRouter is deployed and ready.
    pub fn spawn() -> Result<Self, String> {
        let port = pick_free_port()?;
        let rpc = format!("http://127.0.0.1:{port}");
        let script = repo_root().join("scripts/start-anvil.sh");
        if !script.is_file() {
            return Err(format!("missing {}", script.display()));
        }

        let mut cmd = Command::new("bash");
        cmd.arg(&script)
            .env("ANVIL_PORT", port.to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .process_group(0);
        let child = cmd
            .spawn()
            .map_err(|e| format!("failed to spawn start-anvil.sh: {e}"))?;
        let pgid = child.id() as i32;

        wait_for_router(&rpc, Duration::from_secs(180))?;
        Ok(Self {
            child,
            pgid,
            port,
            rpc,
        })
    }
}

impl Drop for AnvilGuard {
    fn drop(&mut self) {
        // SIGTERM the whole group so bash's `trap` fires and kills anvil.
        // Falls back to SIGKILL after a short grace period in case the
        // graceful shutdown stalls.
        let target = format!("-{}", self.pgid);
        let _ = Command::new("kill")
            .args(["-TERM", &target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        for _ in 0..20 {
            match self.child.try_wait() {
                Ok(Some(_)) => return,
                _ => sleep(Duration::from_millis(100)),
            }
        }
        let _ = Command::new("kill")
            .args(["-KILL", &target])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
        let _ = self.child.wait();
    }
}

/// Pick a port the OS is willing to hand out. There's a TOCTOU race between
/// us dropping the listener and anvil binding the port, but it's fine for a
/// single-machine e2e suite.
fn pick_free_port() -> Result<u16, String> {
    let listener = TcpListener::bind("127.0.0.1:0")
        .map_err(|e| format!("could not bind 127.0.0.1:0 to find a free port: {e}"))?;
    let port = listener
        .local_addr()
        .map_err(|e| format!("local_addr: {e}"))?
        .port();
    drop(listener);
    Ok(port)
}

/// Block until `eth_getCode` on the IntentRouter address returns non-empty
/// bytecode. That's the last thing `start-anvil.sh` does, so by the time it
/// returns the seeding + allowlist are also done.
fn wait_for_router(rpc: &str, timeout: Duration) -> Result<(), String> {
    // Deterministic CREATE address from account #1 (nonce 0) — see
    // start-anvil.sh.
    const ROUTER: &str = "0x8464135c8F25Da09e49BC8782676a84730C318bC";
    let started = Instant::now();
    loop {
        let out = Command::new("cast")
            .args(["code", ROUTER, "--rpc-url", rpc])
            .output();
        if let Ok(o) = out {
            if o.status.success() {
                let code = String::from_utf8_lossy(&o.stdout);
                let trimmed = code.trim();
                if trimmed.len() > 2 && trimmed != "0x" {
                    return Ok(());
                }
            }
        }
        if started.elapsed() > timeout {
            return Err(format!(
                "router did not come up at {ROUTER} on {rpc} within {:?}",
                timeout
            ));
        }
        sleep(Duration::from_millis(500));
    }
}

/// Captured output of a single `advisor` invocation.
pub struct AdvisorOutput {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl AdvisorOutput {
    /// True if the simulation block reports "all transactions succeeded".
    pub fn simulation_succeeded(&self) -> bool {
        self.stdout
            .contains("Result: all transactions succeeded")
    }

    /// True iff the advisor failed because OpenAI returned a 429 / rate
    /// limit. These are upstream issues, not pipeline regressions, and tests
    /// should skip-not-fail on them.
    pub fn was_rate_limited(&self) -> bool {
        let s = &self.stderr;
        s.contains("429") || s.contains("Too Many Requests") || s.contains("rate_limit")
    }

    /// Extract the signed asset deltas the advisor printed in the
    /// "Asset deltas for the signer:" block. Returns `(symbol, change_str)`
    /// pairs in the order they were printed.
    pub fn asset_deltas(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        let mut in_block = false;
        for line in self.stdout.lines() {
            let trimmed = line.trim();
            if trimmed == "Asset deltas for the signer:" {
                in_block = true;
                continue;
            }
            if in_block {
                if trimmed.starts_with("Result:") || trimmed.is_empty() {
                    break;
                }
                // Format: `<SYMBOL>   <before> → <after>  (<change>)`
                if let (Some(sym), Some(open), Some(close)) =
                    (trimmed.split_whitespace().next(), trimmed.rfind('('), trimmed.rfind(')'))
                {
                    if close > open + 1 {
                        let change = &trimmed[open + 1..close];
                        out.push((sym.to_string(), change.to_string()));
                    }
                }
            }
        }
        out
    }
}

/// Run the compiled `advisor` binary with `--simulate --rpc <rpc>` against
/// the given prompt + context. Captures stdout/stderr for assertion.
///
/// Pins `--model gpt-4o` so the test is deterministic regardless of any
/// `ADVISOR_MODEL` the user may have set in `intent-script/.env`. gpt-4o
/// follows the tool-use system prompt reliably; some newer models
/// occasionally emit non-JSON variants of the DSL block that the advisor's
/// parser rejects, which would show up as test flake rather than a real
/// pipeline regression.
pub fn run_advisor(prompt: &str, context: &Path, rpc: &str) -> Result<AdvisorOutput, String> {
    let bin = env!("CARGO_BIN_EXE_advisor");
    let config_dir = repo_root().join("config");
    let output = Command::new(bin)
        .arg(prompt)
        .arg("--context")
        .arg(context)
        .arg("--config-dir")
        .arg(&config_dir)
        .arg("--network")
        .arg("anvil")
        .arg("--model")
        .arg("gpt-4o")
        .arg("--simulate")
        .arg("--rpc")
        .arg(rpc)
        .current_dir(repo_root())
        .output()
        .map_err(|e| format!("failed to run advisor binary at {bin}: {e}"))?;
    Ok(AdvisorOutput {
        exit_code: output.status.code(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

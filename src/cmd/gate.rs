//! `layers gate` — The Perfect Code Gate.
//!
//! Runs a sequential pipeline of checks that must all pass before the workspace
//! is considered shippable. If any check fails, the gate closes and the process
//! exits with a non-zero status.
//!
//! ## Pipeline
//!
//! 1. **Format** — `cargo fmt --all -- --check`
//! 2. **Compile** — `cargo check --workspace`
//! 3. **Clippy** — `cargo clippy --workspace -- -D warnings`
//! 4. **Test** — `cargo test --workspace`
//! 5. **Audit** — `cargo audit` (with advisory DB cache awareness)
//! 6. **MCP Ping** — Ping the gitnexus-rs MCP server (optional, if configured)
//!
//! The gate is strict by design. "Perfect code or it doesn't move."

use std::io::{Read, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{Context, Result};

// ─── Public API ─────────────────────────────────────────────────────────────

/// Arguments for the `layers gate` command.
#[derive(Debug, Clone, clap::Parser)]
pub struct GateArgs {
    /// Run without requiring MCP tool connectivity (skip the gitnexus-rs ping).
    #[arg(long, default_value = "true")]
    pub mcp: bool,

    /// Override the timeout for `cargo audit` in seconds.
    /// Default is 120s. First run may need more time to fetch the advisory DB.
    #[arg(long, default_value = "120")]
    pub audit_timeout: u64,

    /// Path to the workspace to gate. Defaults to the current directory.
    #[arg(long)]
    pub workspace: Option<PathBuf>,
}

/// Handler for `layers gate`.
pub fn handle_gate(args: &GateArgs) -> Result<()> {
    let workspace = args
        .workspace
        .clone()
        .unwrap_or_else(|| std::env::current_dir().expect("can't determine current dir"));

    eprintln!();
    eprintln!("═══ Perfect Code Gate ═══");
    eprintln!("  Workspace: {}", workspace.display());
    eprintln!("  MCP check: {}", if args.mcp { "on" } else { "off" });
    eprintln!();

    run_gate(&workspace, args.mcp, args.audit_timeout)?;

    eprintln!();
    eprintln!("Workspace is Perfect. Gate Open.");
    eprintln!();

    Ok(())
}

/// Run the full Perfect Code Gate pipeline.
pub fn run_gate(workspace: &Path, mcp_check: bool, audit_timeout_secs: u64) -> Result<()> {
    run_fmt_check(workspace)?;
    run_compile_check(workspace)?;
    run_clippy_check(workspace)?;
    run_test_check(workspace)?;
    run_audit_check(workspace, audit_timeout_secs)?;

    if mcp_check {
        run_mcp_ping(30)?;
    }

    Ok(())
}

// ─── Shared helpers ─────────────────────────────────────────────────────────

/// Spawn a process and wait for it, enforcing a deadline.
/// Captures stdout and stderr into buffers as they arrive.
fn run_command_with_deadline<F>(
    spawn: F,
    deadline: Instant,
) -> std::io::Result<std::process::Output>
where
    F: FnOnce() -> std::io::Result<Child>,
{
    let mut child = spawn()?;
    let mut stdout_buf = Vec::new();
    let mut stderr_buf = Vec::new();

    loop {
        // Drain stdout
        if let Some(ref mut out) = child.stdout {
            let mut chunk = [0u8; 4096];
            loop {
                match out.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => stdout_buf.extend_from_slice(&chunk[..n]),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                    Err(e) => return Err(e),
                }
            }
        }

        // Drain stderr
        if let Some(ref mut err) = child.stderr {
            let mut chunk = [0u8; 4096];
            loop {
                match err.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(n) => stderr_buf.extend_from_slice(&chunk[..n]),
                    Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                    Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => break,
                    Err(e) => return Err(e),
                }
            }
        }

        // Check if process has exited
        if let Some(status) = child.try_wait()? {
            // Collect any remaining output after process exits
            if let Some(ref mut out) = child.stdout {
                let mut chunk = [0u8; 4096];
                loop {
                    match out.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => stdout_buf.extend_from_slice(&chunk[..n]),
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            if let Some(ref mut err) = child.stderr {
                let mut chunk = [0u8; 4096];
                loop {
                    match err.read(&mut chunk) {
                        Ok(0) => break,
                        Ok(n) => stderr_buf.extend_from_slice(&chunk[..n]),
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut =>
                        {
                            break;
                        }
                        Err(e) => return Err(e),
                    }
                }
            }
            return Ok(std::process::Output {
                status,
                stdout: stdout_buf,
                stderr: stderr_buf,
            });
        }

        // Not done yet — check deadline
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "process did not complete within deadline",
            ));
        }

        // Poll every 100ms
        std::thread::sleep(Duration::from_millis(100));
    }
}

// ─── Individual checks ─────────────────────────────────────────────────────

fn run_fmt_check(workspace: &Path) -> Result<()> {
    eprintln!("Running Format check...");
    let deadline = Instant::now() + Duration::from_secs(30);

    let output = run_command_with_deadline(
        || {
            Command::new("cargo")
                .args(["fmt", "--all", "--", "--check"])
                .current_dir(workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    )
    .context("cargo fmt timed out or failed to spawn")?;

    if output.status.success() {
        eprintln!("  + Format passed");
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Gate Failure: Format check failed.\n\
         Code is not formatted. Run `cargo fmt --all` to fix.\n{}",
        fmt_output(&stdout, &stderr)
    );
}

fn run_compile_check(workspace: &Path) -> Result<()> {
    eprintln!("Running Compile check...");
    let deadline = Instant::now() + Duration::from_secs(120);

    let output = run_command_with_deadline(
        || {
            Command::new("cargo")
                .args(["check", "--workspace"])
                .current_dir(workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    )
    .context("cargo check timed out or failed to spawn")?;

    if output.status.success() {
        eprintln!("  + Compile passed");
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Gate Failure: Compile check failed.\n\
         The workspace does not compile.\n{}",
        fmt_output(&stdout, &stderr)
    );
}

fn run_clippy_check(workspace: &Path) -> Result<()> {
    eprintln!("Running Clippy check (perfection standard: -D warnings)...");
    let deadline = Instant::now() + Duration::from_secs(180);

    let output = run_command_with_deadline(
        || {
            Command::new("cargo")
                .args(["clippy", "--workspace", "--", "-D", "warnings"])
                .current_dir(workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    )
    .context("cargo clippy timed out or failed to spawn")?;

    if output.status.success() {
        eprintln!("  + Clippy passed — zero warnings");
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Gate Failure: Clippy found warnings or errors.\n\
         Perfection mandate violated.\n{}",
        fmt_output(&stdout, &stderr)
    );
}

fn run_test_check(workspace: &Path) -> Result<()> {
    eprintln!("Running Test check...");
    let deadline = Instant::now() + Duration::from_secs(300);

    let output = run_command_with_deadline(
        || {
            Command::new("cargo")
                .args(["test", "--workspace"])
                .current_dir(workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    )
    .context("cargo test timed out or failed to spawn")?;

    if output.status.success() {
        eprintln!("  + Tests passed");
        return Ok(());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    anyhow::bail!(
        "Gate Failure: Test check failed.\n\
         Behavioral contract violated.\n{}",
        fmt_output(&stdout, &stderr)
    );
}

fn fmt_output(stdout: &str, stderr: &str) -> String {
    if stdout.is_empty() && stderr.is_empty() {
        String::new()
    } else {
        format!("Output:\n{stdout}\n{stderr}")
    }
}

// ─── Audit ────────────────────────────────────────────────────────────────

/// Returns true if the `RustSec` advisory DB appears to be cached locally.
fn advisory_db_cached() -> bool {
    let home = std::env::var("HOME").unwrap_or_default();
    Path::new(&home).join(".cargo").join("advisory-db").is_dir()
}

fn run_audit_check(workspace: &Path, timeout_secs: u64) -> Result<()> {
    eprintln!(
        "Running Audit check (cache-aware, timeout={timeout_secs}s)..."
    );

    let advisory_was_cached = advisory_db_cached();
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    let raw_result = run_command_with_deadline(
        || {
            Command::new("cargo")
                .args(["audit"])
                .current_dir(workspace)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
        },
        deadline,
    );

    let output = match raw_result {
        Ok(o) => o,
        Err(ref e) if e.kind() == std::io::ErrorKind::TimedOut => {
            anyhow::bail!(
                "Gate Failure: Audit check timed out after {timeout_secs}s.\n\
                 Advisory DB may be cold (first-run fetch). Retry — cache will be warm.\n\
                 Otherwise check network to RustSec advisory DB."
            );
        }
        Err(e) => {
            anyhow::bail!("Gate Failure: Audit check could not run: {e}");
        }
    };

    if output.status.success() {
        eprintln!("  + Audit passed — no known vulnerabilities");
        return Ok(());
    }

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    // If advisory DB was cold, retry once with warm cache
    if !advisory_was_cached && code == 101 {
        eprintln!("  advisory DB was cold on first run, re-running with warm cache...");

        let deadline2 = Instant::now() + Duration::from_secs(timeout_secs);
        let retry_result = run_command_with_deadline(
            || {
                Command::new("cargo")
                    .args(["audit"])
                    .current_dir(workspace)
                    .stdin(Stdio::null())
                    .stdout(Stdio::piped())
                    .stderr(Stdio::piped())
                    .spawn()
            },
            deadline2,
        );

        match retry_result {
            Ok(r) if r.status.success() => {
                eprintln!("  + Audit passed on retry — advisory DB now warm");
                return Ok(());
            }
            Ok(r) => {
                let stdout = String::from_utf8_lossy(&r.stdout);
                let stderr = String::from_utf8_lossy(&r.stderr);
                anyhow::bail!(
                    "Gate Failure: Audit check failed after cache warm-up retry.\n{}",
                    fmt_output(&stdout, &stderr)
                );
            }
            Err(e) => {
                anyhow::bail!(
                    "Gate Failure: Audit retry timed out after {timeout_secs}s.\n\
                     Advisory DB fetch may be slow. Error: {e}"
                );
            }
        }
    }

    anyhow::bail!(
        "Gate Failure: Audit check failed (exit {}).\n{}",
        code,
        fmt_output(&stdout, &stderr)
    );
}

// ─── MCP Ping ─────────────────────────────────────────────────────────────

/// Ping the gitnexus-rs MCP server via JSON-RPC 2.0 initialize handshake.
/// Sends an initialize request, waits for a valid JSON-RPC response.
fn run_mcp_ping(timeout_secs: u64) -> Result<()> {
    eprintln!(
        "Running MCP:gitnexus-rs ping check (timeout={timeout_secs}s)..."
    );

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": {
                "name": "layers-gate",
                "version": "0.1.0"
            }
        }
    });

    let request_str =
        serde_json::to_string(&request).context("failed to serialize MCP request")? + "\n";

    let deadline = Instant::now() + Duration::from_secs(timeout_secs);

    let mut child = Command::new("gitnexus-rs")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("failed to spawn gitnexus-rs. Is it on PATH?")?;

    // Write the initialize request then close stdin to signal end of input
    {
        let mut stdin = child.stdin.take().expect("stdin captured");
        stdin
            .write_all(request_str.as_bytes())
            .context("failed to write MCP initialize request")?;
    }

    let mut stdout = child.stdout.take().expect("stdout captured");
    let mut buf = Vec::new();

    loop {
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            let _ = child.kill();
            let _ = child.wait();
            anyhow::bail!(
                "Gate Failure: MCP:gitnexus-rs ping timed out after {timeout_secs}s.\n\
                 The MCP server is unreachable or hung.\n\
                 Routing to gitnexus would fail."
            );
        }

        // Try a non-blocking read
        let mut chunk = [0u8; 4096];
        match stdout.read(&mut chunk) {
            Ok(0) => {
                let status = child.wait().context("MCP server wait failed after EOF")?;
                if status.success() || status.code() == Some(0) {
                    eprintln!("  + MCP:gitnexus-rs exited cleanly — server is healthy");
                    return Ok(());
                }
                anyhow::bail!(
                    "Gate Failure: MCP:gitnexus-rs exited unexpectedly (status {status:?})."
                );
            }
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);

                // Try to parse as JSON-RPC response
                if let Ok(s) = std::str::from_utf8(buf.as_slice()) {
                    if let Ok(resp) = serde_json::from_str::<serde_json::Value>(s.trim_end()) {
                        if resp.get("jsonrpc") == Some(&serde_json::Value::String("2.0".into())) {
                            let _ = child.kill();
                            let _ = child.wait();
                            eprintln!("  + MCP:gitnexus-rs responded — server is healthy");
                            return Ok(());
                        }
                    }
                }
            }
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("Gate Failure: MCP:gitnexus-rs stdout read error: {e}");
            }
        }
    }
}

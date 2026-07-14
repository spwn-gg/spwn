//! Driving a Claude chat session through the Node Agent SDK sidecar.
//!
//! Each Claude terminal owns one sidecar process. We write JSON-line commands to
//! its stdin and forward its JSON-line events to the frontend as
//! `claude://event/<terminal_id>` (and `claude://exit/<terminal_id>` on close).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter};

pub struct ClaudeAgent {
    child: Child,
    stdin: ChildStdin,
}

impl ClaudeAgent {
    /// Write one JSON command line to the sidecar's stdin.
    pub fn send_json(&mut self, line: &str) -> std::io::Result<()> {
        self.stdin.write_all(line.as_bytes())?;
        self.stdin.write_all(b"\n")?;
        self.stdin.flush()
    }

    pub fn kill(&mut self) {
        // Killing the child closes its stdout, ending the reader thread.
        let _ = self.child.kill();
    }
}

/// Control events a headless (scheduled) run needs to observe from the sidecar's
/// stdout, in addition to the raw lines still forwarded to the frontend.
pub enum HeadlessEvent {
    /// The session id arrived — bind it to the terminal so the run is resumable.
    Init { session_id: String },
    /// The turn finished (`ok` false on an error subtype).
    Result { ok: bool },
    /// The run failed / the sidecar died before finishing.
    Error { message: String },
}

/// How the stdout reader thread behaves. `Forward` is the interactive default
/// (raw lines → frontend, unchanged). `Observed` additionally parses lines and
/// fires `HeadlessEvent`s so a windowless run can drive itself from Rust.
enum ReaderMode {
    Forward,
    Observed(Box<dyn Fn(HeadlessEvent) + Send + 'static>),
}

/// Spawn a sidecar for a Claude session (optionally resuming/forking one). Drives
/// the Agent SDK; the chat UI sends user turns / permission answers over stdin and
/// renders the streamed events forwarded from stdout.
#[allow(clippy::too_many_arguments)]
pub fn spawn_claude_agent(
    app: AppHandle,
    terminal_id: &str,
    cwd: &Path,
    resume: Option<&str>,
    resume_at: Option<&str>,
    fork: bool,
    claude_path: &Path,
    permission_mode: Option<&str>,
) -> anyhow::Result<ClaudeAgent> {
    spawn_inner(
        app,
        terminal_id,
        cwd,
        resume,
        resume_at,
        fork,
        claude_path,
        permission_mode,
        false,
        ReaderMode::Forward,
    )
}

/// Spawn a headless (read-only, no-UI) sidecar for a scheduled run. Still forwards
/// events to `claude://event/<id>` (so an open window renders the run live) and
/// additionally invokes `on_event` for init/result/error so the scheduler can bind
/// the session id, flag completion, and tear the agent down.
pub fn spawn_claude_agent_headless(
    app: AppHandle,
    terminal_id: &str,
    cwd: &Path,
    claude_path: &Path,
    on_event: impl Fn(HeadlessEvent) + Send + 'static,
) -> anyhow::Result<ClaudeAgent> {
    spawn_inner(
        app,
        terminal_id,
        cwd,
        None,
        None,
        false,
        claude_path,
        Some("plan"),
        true,
        ReaderMode::Observed(Box::new(on_event)),
    )
}

#[allow(clippy::too_many_arguments)]
fn spawn_inner(
    app: AppHandle,
    terminal_id: &str,
    cwd: &Path,
    resume: Option<&str>,
    resume_at: Option<&str>,
    fork: bool,
    claude_path: &Path,
    permission_mode: Option<&str>,
    headless: bool,
    mode: ReaderMode,
) -> anyhow::Result<ClaudeAgent> {
    let node = find_node_bin().ok_or_else(|| anyhow::anyhow!("node binary not found"))?;
    let script = sidecar_script().ok_or_else(|| anyhow::anyhow!("sidecar script not found"))?;

    let mut cmd = Command::new(&node);
    cmd.arg(&script)
        .arg("--cwd")
        .arg(cwd)
        .arg("--claude-path")
        .arg(claude_path);
    if let Some(r) = resume {
        cmd.arg("--resume").arg(r);
    }
    if let Some(at) = resume_at {
        cmd.arg("--resume-at").arg(at);
    }
    if fork {
        cmd.arg("--fork");
    }
    if let Some(pm) = permission_mode {
        cmd.arg("--permission-mode").arg(pm);
    }
    if headless {
        cmd.arg("--headless");
    }
    // Capture stderr (was /dev/null). A sidecar that crashes at startup — e.g. the
    // bundled node aborting under the hardened runtime — writes its reason here and
    // emits nothing on stdout, which used to leave the chat spinning with no signal.
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");
    let stderr = child.stderr.take().expect("piped stderr");

    // Drain stderr on its own thread, logging each line and keeping the tail so we
    // can attach it to an error if the sidecar dies without responding.
    let err_tail: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));
    let err_tail_reader = Arc::clone(&err_tail);
    let err_thread = std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stderr);
        for line in reader.lines().map_while(Result::ok) {
            eprintln!("[claude-sidecar] {line}");
            let mut tail = err_tail_reader.lock().unwrap();
            tail.push(line);
            let overflow = tail.len().saturating_sub(40);
            if overflow > 0 {
                tail.drain(0..overflow);
            }
        }
    });

    let event = format!("claude://event/{terminal_id}");
    let exit_event = format!("claude://exit/{terminal_id}");
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        let mut emitted = 0usize;
        let mut saw_result = false;
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => {
                    emitted += 1;
                    // Observed runs also inspect the line for control events.
                    if let ReaderMode::Observed(cb) = &mode {
                        if let Some(ev) = parse_headless(&l) {
                            if matches!(ev, HeadlessEvent::Result { .. }) {
                                saw_result = true;
                            }
                            cb(ev);
                        }
                    }
                    if app.emit(&event, l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        // stdout closed → the sidecar is done. If it never produced a single event,
        // it died before responding; surface stderr as an error the chat can render
        // instead of hanging on the typing indicator forever.
        let _ = err_thread.join();
        if emitted == 0 {
            let tail = err_tail.lock().unwrap().join("\n");
            let message = if tail.trim().is_empty() {
                "The Claude sidecar exited before responding.".to_string()
            } else {
                format!("The Claude sidecar exited before responding:\n{tail}")
            };
            let payload = serde_json::json!({ "t": "error", "message": message }).to_string();
            let _ = app.emit(&event, payload);
        }
        // An observed run that closed without a result never finished — tell the
        // scheduler so it can finalize (clear "running", flag the session).
        if let ReaderMode::Observed(cb) = &mode {
            if !saw_result {
                let tail = err_tail.lock().unwrap().join("\n");
                let message = if tail.trim().is_empty() {
                    "The scheduled run ended before completing.".to_string()
                } else {
                    format!("The scheduled run ended before completing:\n{tail}")
                };
                cb(HeadlessEvent::Error { message });
            }
        }
        let _ = app.emit(&exit_event, ());
    });

    Ok(ClaudeAgent { child, stdin })
}

/// Parse one sidecar stdout JSON line into a control event, if it is one.
fn parse_headless(line: &str) -> Option<HeadlessEvent> {
    let v: serde_json::Value = serde_json::from_str(line).ok()?;
    match v.get("t").and_then(|t| t.as_str())? {
        "init" => Some(HeadlessEvent::Init {
            session_id: v.get("sessionId")?.as_str()?.to_string(),
        }),
        "result" => Some(HeadlessEvent::Result {
            ok: v.get("subtype").and_then(|s| s.as_str()) == Some("success"),
        }),
        "error" => Some(HeadlessEvent::Error {
            message: v
                .get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("unknown error")
                .to_string(),
        }),
        _ => None,
    }
}

/// The directory containing the running executable (Contents/MacOS in a bundle).
fn exe_dir() -> Option<PathBuf> {
    std::env::current_exe().ok()?.parent().map(|p| p.to_path_buf())
}

/// Locate a `node` binary. Prefer the bundled sidecar binary (Contents/MacOS/node),
/// then `CM_NODE`, `$PATH`, common install dirs, and nvm.
pub fn find_node_bin() -> Option<PathBuf> {
    if let Some(bundled) = exe_dir().map(|d| d.join("node")).filter(|p| p.exists()) {
        return Some(bundled);
    }
    if let Ok(p) = std::env::var("CM_NODE") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(p) = which("node") {
        return Some(p);
    }
    for c in ["/opt/homebrew/bin/node", "/usr/local/bin/node", "/usr/bin/node"] {
        let pb = PathBuf::from(c);
        if pb.exists() {
            return Some(pb);
        }
    }
    // nvm: ~/.nvm/versions/node/<ver>/bin/node — pick the latest.
    let home = directories::BaseDirs::new().map(|b| b.home_dir().to_path_buf())?;
    let nvm = home.join(".nvm/versions/node");
    let mut versions: Vec<PathBuf> = std::fs::read_dir(&nvm)
        .ok()?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.join("bin/node").exists())
        .collect();
    versions.sort();
    versions.last().map(|p| p.join("bin/node"))
}

/// Locate the sidecar entry script. `CM_SIDECAR` overrides; otherwise the
/// compiled-in repo path (works for a locally built app — bundling is a follow-up).
pub fn sidecar_script() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CM_SIDECAR") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    // Bundled: Contents/Resources/resources/sidecar.mjs (next to Contents/MacOS).
    if let Some(bundled) = exe_dir()
        .and_then(|d| d.parent().map(|c| c.join("Resources/resources/sidecar.mjs")))
        .filter(|p| p.exists())
    {
        return Some(bundled);
    }
    // Dev: the unbundled source script in the repo.
    let dev = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../sidecar/index.mjs"));
    dev.exists().then_some(dev)
}

fn which(name: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

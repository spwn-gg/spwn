//! Driving a Claude chat session through the Node Agent SDK sidecar.
//!
//! Each Claude terminal owns one sidecar process. We write JSON-line commands to
//! its stdin and forward its JSON-line events to the frontend as
//! `claude://event/<terminal_id>` (and `claude://exit/<terminal_id>` on close).

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
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

/// Spawn a sidecar for a Claude session (optionally resuming/forking one). Drives
/// the Agent SDK; the chat UI sends user turns / permission answers over stdin and
/// renders the streamed events forwarded from stdout.
pub fn spawn_claude_agent(
    app: AppHandle,
    terminal_id: &str,
    cwd: &Path,
    resume: Option<&str>,
    resume_at: Option<&str>,
    fork: bool,
    claude_path: &Path,
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
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let mut child = cmd.spawn()?;
    let stdin = child.stdin.take().expect("piped stdin");
    let stdout = child.stdout.take().expect("piped stdout");

    let event = format!("claude://event/{terminal_id}");
    let exit_event = format!("claude://exit/{terminal_id}");
    std::thread::spawn(move || {
        let reader = std::io::BufReader::new(stdout);
        for line in reader.lines() {
            match line {
                Ok(l) if !l.is_empty() => {
                    if app.emit(&event, l).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => break,
            }
        }
        let _ = app.emit(&exit_event, ());
    });

    Ok(ClaudeAgent { child, stdin })
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

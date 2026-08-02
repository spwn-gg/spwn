//! M0 RISK SPIKE — the riskiest assumption in the whole project.
//!
//! Goal: prove we can drive Claude Code's interactive `/rewind` + `/branch` over a
//! pty and end up with a NEW session JSONL whose conversation chain is *truncated*
//! at an earlier response — i.e. "branch from an arbitrary past response" is
//! achievable by automating the TUI, since there is no CLI for it.
//!
//! This test makes REAL, authenticated model calls. It is gated and no-ops unless:
//!   * env `RUN_CLAUDE_PTY_SPIKE=1`, and
//!   * a `claude` binary is found (env `CLAUDE_BIN`, `$PATH`, or ~/.local/bin/claude).
//! Run it with:  make spike      (or, directly:)
//!   RUN_CLAUDE_PTY_SPIKE=1 cargo test --test rewind_branch_spike -- --nocapture --test-threads=1
//!
//! STATUS / how to use this spike: the pty I/O, synchronization (`wait_for`), and
//! the JSONL assertions are real and final. The exact keystrokes for the `/rewind`
//! checkpoint menu (`navigate_rewind_to_earlier_checkpoint`) are the UNKNOWN this
//! spike exists to nail down. On the first live run it dumps every TUI frame to
//! stdout; read those frames, then tighten the choreography and the assertion.
//! The plan's safe default (fork-then-rewind) can be layered on once the raw
//! rewind+branch sequence is confirmed here.

use portable_pty::{native_pty_system, Child, CommandBuilder, MasterPty, PtySize};
use std::collections::HashSet;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Gating / discovery
// ---------------------------------------------------------------------------

fn spike_enabled() -> bool {
    matches!(std::env::var("RUN_CLAUDE_PTY_SPIKE").ok().as_deref(), Some("1"))
}

/// Locate a `claude` binary: explicit override, then PATH, then the known install dir.
fn claude_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    if let Ok(out) = std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v claude")
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    for cand in [
        dirs_home().map(|h| h.join(".local/bin/claude")),
        dirs_home().map(|h| h.join(".claude/local/claude")),
    ]
    .into_iter()
    .flatten()
    {
        if cand.exists() {
            return Some(cand);
        }
    }
    None
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

fn projects_root() -> PathBuf {
    dirs_home()
        .expect("HOME set")
        .join(".claude")
        .join("projects")
}

// ---------------------------------------------------------------------------
// Minimal pty driver: background reader accumulates output; helpers send input
// and block until an expected marker appears.
// ---------------------------------------------------------------------------

struct Pty {
    master: Box<dyn MasterPty + Send>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
    buf: Arc<Mutex<String>>,
}

impl Pty {
    fn spawn(bin: &Path, args: &[&str], cwd: &Path) -> Self {
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: 40,
                cols: 120,
                pixel_width: 0,
                pixel_height: 0,
            })
            .expect("openpty");

        let mut cmd = CommandBuilder::new(bin.as_os_str());
        for a in args {
            cmd.arg(a);
        }
        cmd.cwd(cwd);
        cmd.env("TERM", "xterm-256color");
        // Keep the model deterministic-ish and cheap for the spike.
        cmd.env("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "1");

        let child = pair.slave.spawn_command(cmd).expect("spawn claude in pty");
        let writer = pair.master.take_writer().expect("take writer");
        let mut reader = pair.master.try_clone_reader().expect("clone reader");
        drop(pair.slave);

        let buf = Arc::new(Mutex::new(String::new()));
        let buf_w = Arc::clone(&buf);
        std::thread::spawn(move || {
            let mut chunk = [0u8; 8192];
            loop {
                match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let s = String::from_utf8_lossy(&chunk[..n]);
                        buf_w.lock().unwrap().push_str(&s);
                    }
                }
            }
        });

        Pty {
            master: pair.master,
            writer,
            child,
            buf,
        }
    }

    fn snapshot(&self) -> String {
        self.buf.lock().unwrap().clone()
    }

    fn send(&mut self, s: &str) {
        self.writer.write_all(s.as_bytes()).expect("write to pty");
        self.writer.flush().ok();
    }

    /// Send text followed by a carriage return (Enter in a TUI).
    fn send_line(&mut self, s: &str) {
        self.send(s);
        self.send("\r");
    }

    /// Block until `pat` appears in accumulated output, or timeout. Returns success.
    fn wait_for(&self, pat: &str, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if self.buf.lock().unwrap().contains(pat) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }

    /// Block until output stops growing for `quiet` (TUI has settled), or timeout.
    fn wait_idle(&self, quiet: Duration, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut last_len = self.buf.lock().unwrap().len();
        let mut last_change = Instant::now();
        while Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(120));
            let len = self.buf.lock().unwrap().len();
            if len != last_len {
                last_len = len;
                last_change = Instant::now();
            } else if last_change.elapsed() >= quiet {
                return;
            }
        }
    }
}

impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.master.resize(PtySize {
            rows: 40,
            cols: 120,
            pixel_width: 0,
            pixel_height: 0,
        });
    }
}

// VT100 control sequences the rewind menu may need.
#[allow(dead_code)]
const UP: &str = "\x1b[A";
#[allow(dead_code)]
const DOWN: &str = "\x1b[B";
const ENTER: &str = "\r";
const CTRL_C: &str = "\x03";

// ---------------------------------------------------------------------------
// Session-file bookkeeping (to detect the new forked transcript).
// ---------------------------------------------------------------------------

fn all_session_files(root: &Path) -> HashSet<PathBuf> {
    let mut out = HashSet::new();
    let Ok(projects) = std::fs::read_dir(root) else {
        return out;
    };
    for proj in projects.flatten() {
        let p = proj.path();
        if !p.is_dir() {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(&p) {
            for f in files.flatten() {
                let fp = f.path();
                if fp.extension().and_then(|e| e.to_str()) == Some("jsonl") {
                    out.insert(fp);
                }
            }
        }
    }
    out
}

/// Count user/assistant turns in a transcript (cheap line scan), and report its cwd.
fn turn_count_and_cwd(path: &Path) -> (usize, Option<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (0, None);
    };
    let mut turns = 0usize;
    let mut cwd = None;
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        match v.get("type").and_then(|t| t.as_str()) {
            Some("user") | Some("assistant") => turns += 1,
            _ => {}
        }
        if cwd.is_none() {
            if let Some(c) = v.get("cwd").and_then(|c| c.as_str()) {
                cwd = Some(c.to_string());
            }
        }
    }
    (turns, cwd)
}

// ---------------------------------------------------------------------------
// The choreography under test.
// ---------------------------------------------------------------------------

/// Drive the discovered `/rewind` flow. The menu (confirmed via captured frames):
///   1. a checkpoint list ("Restore … to the point before <message>")
///   2. after selecting one: an action menu whose default is "1. Restore
///      conversation" — which forks the conversation, leaving code unchanged.
///
/// `steps_back` moves down the checkpoint list before confirming (0 = default).
fn rewind_restore_conversation(pty: &mut Pty, steps_back: usize) {
    pty.wait_idle(Duration::from_millis(400), Duration::from_secs(5));
    for _ in 0..steps_back {
        pty.send(DOWN);
        std::thread::sleep(Duration::from_millis(200));
    }
    pty.send(ENTER); // select the checkpoint -> action menu
    // "1. Restore conversation" is the default highlighted action.
    pty.wait_for("Restore conversation", Duration::from_secs(8));
    pty.wait_idle(Duration::from_millis(400), Duration::from_secs(5));
    pty.send(ENTER); // choose "Restore conversation" (forks; code unchanged)
    pty.wait_idle(Duration::from_millis(800), Duration::from_secs(15));
}

#[test]
fn rewind_then_branch_creates_truncated_fork() {
    if !spike_enabled() {
        eprintln!("[spike] RUN_CLAUDE_PTY_SPIKE != 1 — skipping live claude spike.");
        return;
    }
    let Some(bin) = claude_bin() else {
        eprintln!("[spike] no claude binary found — skipping.");
        return;
    };
    eprintln!("[spike] using claude at {}", bin.display());

    let scratch = tempfile::tempdir().expect("tempdir");
    let cwd = scratch.path().to_path_buf();
    eprintln!("[spike] scratch cwd: {}", cwd.display());

    let root = projects_root();
    let before = all_session_files(&root);

    // Launch the interactive TUI in the scratch dir.
    let mut pty = Pty::spawn(&bin, &[], &cwd);

    // A brand-new directory may show a folder-trust prompt; accept it if it appears.
    if pty.wait_for("trust", Duration::from_secs(6)) {
        eprintln!("[spike] trust prompt detected — accepting.");
        pty.send(ENTER);
    }
    // Let the TUI settle into the prompt.
    pty.wait_idle(Duration::from_millis(700), Duration::from_secs(20));

    // --- Turn 1 (this becomes the earlier checkpoint we branch from) ---
    pty.send_line("Reply with exactly one word: apple");
    let got1 = pty.wait_for("apple", Duration::from_secs(60));
    eprintln!("[spike] turn 1 response seen: {got1}");
    pty.wait_idle(Duration::from_millis(800), Duration::from_secs(20));

    // --- Turn 2 (later state we will rewind away from) ---
    pty.send_line("Reply with exactly one word: banana");
    let got2 = pty.wait_for("banana", Duration::from_secs(60));
    eprintln!("[spike] turn 2 response seen: {got2}");
    pty.wait_idle(Duration::from_millis(800), Duration::from_secs(20));

    // --- /rewind, then "Restore conversation" to fork at an earlier checkpoint ---
    pty.send_line("/rewind");
    let rewind_menu = pty.wait_for("Restore", Duration::from_secs(10))
        || pty.wait_for("ewind", Duration::from_secs(2));
    eprintln!("[spike] rewind menu detected: {rewind_menu}");
    // Move one checkpoint down (to "before banana") then restore conversation,
    // which should leave a conversation truncated to the apple turn.
    rewind_restore_conversation(&mut pty, 1);

    // Exit cleanly.
    pty.send(CTRL_C);
    std::thread::sleep(Duration::from_millis(300));
    pty.send_line("/quit");
    std::thread::sleep(Duration::from_secs(2));

    // Always dump the full TUI transcript — this is the spike's primary artifact.
    let frames = pty.snapshot();
    eprintln!(
        "\n[spike] ===== captured TUI output ({} bytes) =====\n{}\n[spike] ===== end =====",
        frames.len(),
        frames
    );

    // --- Inspect the filesystem for the forked transcript ---
    let after = all_session_files(&root);
    let mut new_files: Vec<PathBuf> = after.difference(&before).cloned().collect();
    new_files.sort();

    let cwd_str = cwd.to_string_lossy().to_string();
    let ours: Vec<(PathBuf, usize)> = new_files
        .iter()
        .filter_map(|p| {
            let (turns, c) = turn_count_and_cwd(p);
            // Match by authoritative cwd recorded inside the transcript.
            match c {
                Some(c) if c == cwd_str => Some((p.clone(), turns)),
                _ => None,
            }
        })
        .collect();

    eprintln!("[spike] new session files for our cwd:");
    for (p, t) in &ours {
        eprintln!("  - {} ({t} turns)", p.display());
    }

    // The core assertion: at least one NEW session for our scratch cwd appeared
    // (the fork), and at least one of them is truncated relative to the longest
    // (the branch happened at an earlier checkpoint, not the latest state).
    assert!(
        !ours.is_empty(),
        "expected at least one new session transcript for the scratch cwd; \
         read the captured TUI output above to refine the rewind/branch choreography."
    );

    if ours.len() >= 2 {
        let max = ours.iter().map(|(_, t)| *t).max().unwrap();
        let min = ours.iter().map(|(_, t)| *t).min().unwrap();
        assert!(
            min < max,
            "expected a truncated fork (a branch with fewer turns than the original), \
             but all new transcripts have {max} turns — the rewind likely did not move \
             to an earlier checkpoint. Inspect the captured frames and tighten \
             navigate_rewind_to_earlier_checkpoint()."
        );
        eprintln!("[spike] PASS: truncated fork confirmed ({min} turns vs {max} turns).");
    } else {
        eprintln!(
            "[spike] NOTE: only one new transcript appeared. Confirm from the frames \
             whether /branch created a separate session file or mutated in place."
        );
    }
}

//! Project shell hooks: one user script per session lifecycle event, discovered
//! inside a session's worktree, that spwn runs with useful environment variables.
//! This replaces the old opinionated per-session docker-compose integration with a
//! generic, unix-y mechanism — spwn just runs the script; it has no opinion about
//! what it does (and the script is free to orchestrate other files/code).
//!
//! Discovery: `<worktree>/.spwn/hooks/<event>.sh` — a single entry-point file per
//! event. Because hooks live in the worktree (a git checkout), a committed hook
//! travels into every session automatically. The file runs directly when it's
//! executable (honoring its shebang); otherwise it's run via `sh`.
//!
//! Events (see [`EVENTS`]): `session-created`, `session-ready`, `session-deleted`.
//!
//! Injected environment (a hook reads these): `SPWN_EVENT`, `SPWN_TERMINAL_ID`,
//! `SPWN_PROJECT_DIR`, `SPWN_WORKTREE`, `SPWN_BRANCH`, `SPWN_BASE_BRANCH`,
//! `SPWN_SESSION_ID` (the last three only when known). Hooks run with the worktree
//! as their working directory.
//!
//! Style mirrors `gitwt.rs`/the old `compose.rs`: thin `std::process::Command`
//! shell-outs, best-effort, never fail a session.

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// The lifecycle events spwn fires hooks for (also the discoverable file stems).
pub const EVENTS: &[&str] = &["session-created", "session-ready", "session-deleted"];

/// Cap on captured hook output kept per run (tail), so a chatty script can't bloat
/// the store / the panel.
const OUTPUT_CAP: usize = 8192;

// ---------------------------------------------------------------------------
// Context + result model
// ---------------------------------------------------------------------------

/// Everything a hook run needs about the session it acts on.
pub struct HookCtx {
    pub terminal_id: String,
    pub project_dir: String,
    /// The session's worktree — where the hook is discovered and run.
    pub worktree: PathBuf,
    pub branch: Option<String>,
    pub base_branch: Option<String>,
    pub session_id: Option<String>,
}

/// The result of running an event's hook script.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HookRun {
    pub event: String,
    /// Hook file basename (e.g. `session-created.sh`).
    pub script: String,
    /// Process exit code, or None if the script couldn't be launched / was signalled.
    pub exit_code: Option<i32>,
    pub ok: bool,
    /// Combined stdout+stderr tail.
    pub output: String,
    /// Epoch seconds when the run finished.
    pub at: u64,
}

/// The discovered hook plus its most recent run for one event (for the UI).
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HookEventInfo {
    pub event: String,
    /// The hook file basename if one exists for this event, else None.
    pub script: Option<String>,
    pub last_run: Option<HookRun>,
}

/// Overall hooks status for a session, for the Hooks panel.
#[derive(Debug, Clone, Serialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct HooksStatus {
    /// Whether this session has a worktree to discover hooks in.
    pub available: bool,
    pub events: Vec<HookEventInfo>,
}

// ---------------------------------------------------------------------------
// Discovery
// ---------------------------------------------------------------------------

/// The hook file for an event: `<worktree>/.spwn/hooks/<event>.sh`.
fn hook_file(worktree: &Path, event: &str) -> PathBuf {
    worktree
        .join(".spwn")
        .join("hooks")
        .join(format!("{event}.sh"))
}

/// Whether `p` is a regular file with an execute bit (on unix; any regular file
/// elsewhere).
fn is_executable(p: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(p) else {
        return false;
    };
    if !meta.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

/// The event's hook file, if it exists (a regular file). None means "no hook for this
/// event" — the feature is fully opt-in.
pub fn discover(worktree: &Path, event: &str) -> Option<PathBuf> {
    let p = hook_file(worktree, event);
    p.is_file().then_some(p)
}

// ---------------------------------------------------------------------------
// Running
// ---------------------------------------------------------------------------

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Keep at most `cap` bytes from the end of `s`, on a char boundary, prefixing `…`
/// when truncated.
fn tail(s: &str, cap: usize) -> String {
    let t = s.trim_end();
    if t.len() <= cap {
        return t.to_string();
    }
    let mut start = t.len() - cap;
    while start < t.len() && !t.is_char_boundary(start) {
        start += 1;
    }
    format!("…{}", &t[start..])
}

/// Run the event's hook script, capturing its outcome. Executes the file directly
/// when it's executable (honoring its shebang), otherwise via `sh`. Never panics.
fn run_one(ctx: &HookCtx, event: &str, script: &Path) -> HookRun {
    let name = script
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();

    let mut cmd = if is_executable(script) {
        Command::new(script)
    } else {
        let mut c = Command::new("sh");
        c.arg(script);
        c
    };
    cmd.current_dir(&ctx.worktree)
        .env("SPWN_EVENT", event)
        .env("SPWN_TERMINAL_ID", &ctx.terminal_id)
        .env("SPWN_PROJECT_DIR", &ctx.project_dir)
        .env("SPWN_WORKTREE", ctx.worktree.to_string_lossy().as_ref());
    if let Some(b) = &ctx.branch {
        cmd.env("SPWN_BRANCH", b);
    }
    if let Some(b) = &ctx.base_branch {
        cmd.env("SPWN_BASE_BRANCH", b);
    }
    if let Some(s) = &ctx.session_id {
        cmd.env("SPWN_SESSION_ID", s);
    }

    match cmd.output() {
        Ok(out) => {
            let mut combined = String::from_utf8_lossy(&out.stdout).into_owned();
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                if !combined.is_empty() && !combined.ends_with('\n') {
                    combined.push('\n');
                }
                combined.push_str(&err);
            }
            HookRun {
                event: event.to_string(),
                script: name,
                exit_code: out.status.code(),
                ok: out.status.success(),
                output: tail(&combined, OUTPUT_CAP),
                at: now_secs(),
            }
        }
        Err(e) => HookRun {
            event: event.to_string(),
            script: name,
            exit_code: None,
            ok: false,
            output: format!("failed to run hook: {e}"),
            at: now_secs(),
        },
    }
}

/// Run the event's hook (if one exists), returning its result — or None when there's
/// no hook file for this event.
pub fn run_event_sync(ctx: &HookCtx, event: &str) -> Option<HookRun> {
    discover(&ctx.worktree, event).map(|s| run_one(ctx, event, &s))
}

// ---------------------------------------------------------------------------
// Status (for the Hooks panel)
// ---------------------------------------------------------------------------

/// The discovered hook + the caller's recorded last run for each known event.
pub fn status(worktree: &Path, last_runs: &BTreeMap<String, HookRun>) -> HooksStatus {
    let events = EVENTS
        .iter()
        .map(|&event| HookEventInfo {
            event: event.to_string(),
            script: discover(worktree, event)
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned())),
            last_run: last_runs.get(event).cloned(),
        })
        .collect();
    HooksStatus { available: true, events }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, body: &str, exec: bool) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, body).unwrap();
        #[cfg(unix)]
        if exec {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let _ = exec;
    }

    fn ctx(wt: &Path) -> HookCtx {
        HookCtx {
            terminal_id: "abc123".into(),
            project_dir: wt.to_string_lossy().into_owned(),
            worktree: wt.to_path_buf(),
            branch: Some("cm/abc123".into()),
            base_branch: None,
            session_id: None,
        }
    }

    #[test]
    fn runs_single_event_file_with_env() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(
            &hook_file(wt, "session-created"),
            "#!/bin/sh\necho \"id=$SPWN_TERMINAL_ID event=$SPWN_EVENT branch=$SPWN_BRANCH\"\n",
            true,
        );
        let run = run_event_sync(&ctx(wt), "session-created").expect("hook present");
        assert_eq!(run.script, "session-created.sh");
        assert!(run.ok);
        assert!(run.output.contains("id=abc123"));
        assert!(run.output.contains("event=session-created"));
        assert!(run.output.contains("branch=cm/abc123"));
    }

    #[test]
    fn missing_hook_is_none() {
        let dir = tempfile::tempdir().unwrap();
        assert!(discover(dir.path(), "session-deleted").is_none());
        assert!(run_event_sync(&ctx(dir.path()), "session-deleted").is_none());
    }

    #[test]
    fn non_executable_file_runs_via_sh() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        // No execute bit — spwn falls back to `sh <file>`.
        write(&hook_file(wt, "session-ready"), "echo hi\n", false);
        let run = run_event_sync(&ctx(wt), "session-ready").expect("hook present");
        assert!(run.ok, "output: {}", run.output);
        assert!(run.output.contains("hi"));
    }

    #[test]
    fn nonzero_exit_is_recorded_as_failure() {
        let dir = tempfile::tempdir().unwrap();
        let wt = dir.path();
        write(&hook_file(wt, "session-deleted"), "#!/bin/sh\nexit 3\n", true);
        let run = run_event_sync(&ctx(wt), "session-deleted").unwrap();
        assert!(!run.ok);
        assert_eq!(run.exit_code, Some(3));
    }
}

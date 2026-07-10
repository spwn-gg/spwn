//! Code-undo via APFS copy-on-write checkpoints.
//!
//! A checkpoint is an instant `cp -c` (clonefile) snapshot of the whole project
//! directory — repo-agnostic, so it captures every nested git repo plus any
//! non-git folders uniformly. Restore rewrites the working files to a checkpoint
//! with `rsync --delete` while EXCLUDING every `.git` (so commit history is never
//! touched) and heavy/ephemeral dirs. Stored under `<app_data>/checkpoints/`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// Newest `turn` checkpoints kept per session; older ones are pruned.
pub const TURN_CAP: usize = 20;
/// Synthetic (pre-restore / pre-switch) checkpoints kept per session.
pub const SYNTH_CAP: usize = 6;

/// Dirs removed from each clone (top-level) to keep snapshots small.
pub const PRUNE_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    ".venv",
    "venv",
    "target",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    ".turbo",
];

/// rsync excludes on restore. Unanchored, so they match at any depth — every
/// `.git/` (dir) and `.git` (submodule/worktree gitlink file) is protected from
/// both transfer and `--delete`, and heavy dirs are left as-is.
const RSYNC_EXCLUDES: &[&str] = &[
    ".git/",
    ".git",
    "node_modules/",
    ".venv/",
    "venv/",
    "target/",
    "dist/",
    "build/",
    ".next/",
    ".svelte-kit/",
    ".turbo/",
    ".DS_Store",
];

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct CheckpointMeta {
    /// Directory name: `<turn_uuid>__<unix_millis>`.
    pub id: String,
    pub session_id: String,
    /// The assistant turn this snapshots after; "baseline"/"pre-restore"/"pre-switch" for synthetic.
    pub turn_uuid: String,
    /// Absolute project directory captured.
    pub project_dir: String,
    pub created_ms: u128,
    /// "turn" | "baseline" | "pre-restore" | "pre-switch".
    pub kind: String,
}

fn now_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis())
        .unwrap_or(0)
}

fn session_dir(app_data: &Path, session_id: &str) -> PathBuf {
    app_data.join("checkpoints").join(session_id)
}

fn index_path(app_data: &Path, session_id: &str) -> PathBuf {
    session_dir(app_data, session_id).join("index.json")
}

fn load_index(app_data: &Path, session_id: &str) -> Vec<CheckpointMeta> {
    std::fs::read_to_string(index_path(app_data, session_id))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_index(app_data: &Path, session_id: &str, idx: &[CheckpointMeta]) {
    let path = index_path(app_data, session_id);
    if let Some(p) = path.parent() {
        let _ = std::fs::create_dir_all(p);
    }
    let json = serde_json::to_string_pretty(idx).unwrap_or_else(|_| "[]".into());
    let _ = std::fs::write(path, json);
}

/// Clone `src` into a fresh `dst` (which must not exist) using clonefile when
/// possible, then prune heavy dirs from the clone.
fn clone_dir(src: &Path, dst: &Path) -> Result<(), String> {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir checkpoints: {e}"))?;
    }
    // Instant COW clone on APFS; fall back to a plain recursive copy otherwise.
    let cloned = Command::new("/bin/cp")
        .arg("-cR")
        .arg(src)
        .arg(dst)
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    if !cloned {
        let _ = std::fs::remove_dir_all(dst);
        let ok = Command::new("/bin/cp")
            .arg("-R")
            .arg(src)
            .arg(dst)
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        if !ok {
            return Err("failed to snapshot the project directory".into());
        }
    }
    // Prune heavy/ephemeral dirs at ANY depth (e.g. src-tauri/target, a/node_modules)
    // so a checkpoint's COW delta stays tiny across rebuilds.
    for d in PRUNE_DIRS {
        let _ = Command::new("/usr/bin/find")
            .arg(dst)
            .args(["-type", "d", "-name", d, "-prune", "-exec", "/bin/rm", "-rf", "{}", "+"])
            .status();
    }
    Ok(())
}

/// Capture a checkpoint of `project_dir`; appends to the session index and prunes.
pub fn capture(
    app_data: &Path,
    project_dir: &Path,
    session_id: &str,
    turn_uuid: &str,
    kind: &str,
) -> Result<CheckpointMeta, String> {
    let created_ms = now_millis();
    let id = format!("{turn_uuid}__{created_ms}");
    let dst = session_dir(app_data, session_id).join(&id);
    clone_dir(project_dir, &dst)?;

    let meta = CheckpointMeta {
        id,
        session_id: session_id.to_string(),
        turn_uuid: turn_uuid.to_string(),
        project_dir: project_dir.to_string_lossy().into_owned(),
        created_ms,
        kind: kind.to_string(),
    };
    let mut idx = load_index(app_data, session_id);
    idx.push(meta.clone());
    prune(app_data, session_id, &mut idx);
    save_index(app_data, session_id, &idx);
    Ok(meta)
}

/// Restore `project_dir`'s working files to a checkpoint (history-safe).
pub fn restore(
    app_data: &Path,
    session_id: &str,
    checkpoint_id: &str,
    project_dir: &Path,
) -> Result<(), String> {
    let cp = session_dir(app_data, session_id).join(checkpoint_id);
    if !cp.is_dir() {
        return Err("that checkpoint no longer exists".into());
    }
    let mut cmd = Command::new("/usr/bin/rsync");
    // --checksum: compare by content, not size+mtime — rsync's quick-check would
    // skip a same-size edit made within the same second as the checkpoint.
    cmd.arg("-a").arg("--checksum").arg("--delete");
    for ex in RSYNC_EXCLUDES {
        cmd.arg(format!("--exclude={ex}"));
    }
    // Trailing slashes: copy the checkpoint's *contents* into the project dir.
    cmd.arg(format!("{}/", cp.display()))
        .arg(format!("{}/", project_dir.display()));
    let out = cmd.output().map_err(|e| format!("failed to run rsync: {e}"))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(format!(
            "restore failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ))
    }
}

pub fn list(app_data: &Path, session_id: &str) -> Vec<CheckpointMeta> {
    let mut idx = load_index(app_data, session_id);
    idx.reverse(); // newest first for display
    idx
}

/// Absolute path to a checkpoint's snapshot directory (may not exist on disk).
pub fn checkpoint_dir(app_data: &Path, session_id: &str, checkpoint_id: &str) -> PathBuf {
    session_dir(app_data, session_id).join(checkpoint_id)
}

/// The most recent checkpoint captured for a session, if any.
pub fn latest(app_data: &Path, session_id: &str) -> Option<CheckpointMeta> {
    load_index(app_data, session_id).into_iter().last()
}

/// The newest checkpoint captured for a given assistant turn, if any.
pub fn find_for_turn(app_data: &Path, session_id: &str, turn_uuid: &str) -> Option<CheckpointMeta> {
    load_index(app_data, session_id)
        .into_iter()
        .rev()
        .find(|m| m.turn_uuid == turn_uuid)
}

/// Drop all checkpoints for a session (e.g. when its terminal is deleted).
pub fn remove_session(app_data: &Path, session_id: &str) {
    let _ = std::fs::remove_dir_all(session_dir(app_data, session_id));
}

/// Enforce per-kind caps: keep the newest TURN_CAP `turn` and SYNTH_CAP synthetic
/// checkpoints (baseline is always kept), deleting the rest from disk + index.
fn prune(app_data: &Path, session_id: &str, idx: &mut Vec<CheckpointMeta>) {
    let mut turn_seen = 0usize;
    let mut synth_seen = 0usize;
    let mut remove: Vec<String> = Vec::new();
    // Walk newest -> oldest, counting per kind.
    for m in idx.iter().rev() {
        match m.kind.as_str() {
            "turn" => {
                turn_seen += 1;
                if turn_seen > TURN_CAP {
                    remove.push(m.id.clone());
                }
            }
            "baseline" => {}
            _ => {
                synth_seen += 1;
                if synth_seen > SYNTH_CAP {
                    remove.push(m.id.clone());
                }
            }
        }
    }
    if remove.is_empty() {
        return;
    }
    for id in &remove {
        let _ = std::fs::remove_dir_all(session_dir(app_data, session_id).join(id));
    }
    idx.retain(|m| !remove.contains(&m.id));
}

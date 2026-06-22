//! Per-session git worktrees: each Claude session works on its own branch in an
//! isolated worktree (managed under the app data dir), so sessions don't clobber
//! each other's files. Branches live in the user's real repo, so a session's work
//! can be merged back.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Run `git -C <dir> <args>`, returning trimmed stdout on success or stderr on error.
fn git(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// The repository root containing `dir`, or None if it isn't inside a git repo.
pub fn repo_root(dir: &Path) -> Option<PathBuf> {
    git(dir, &["rev-parse", "--show-toplevel"])
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
}

/// The currently checked-out branch name in `dir` (None if detached HEAD).
pub fn current_branch(dir: &Path) -> Option<String> {
    let b = git(dir, &["rev-parse", "--abbrev-ref", "HEAD"]).ok()?;
    (b != "HEAD" && !b.is_empty()).then_some(b)
}

/// Create a new worktree at `path` on a new `branch` forked from `base`.
pub fn add_worktree(repo: &Path, path: &Path, branch: &str, base: &str) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("mkdir worktrees dir: {e}"))?;
    }
    git(
        repo,
        &["worktree", "add", "-b", branch, &path.to_string_lossy(), base],
    )
    .map(|_| ())
}

/// Remove a worktree (force, so uncommitted changes don't block it). The branch
/// itself is kept (so its commits aren't lost).
pub fn remove_worktree(repo: &Path, path: &Path) -> Result<(), String> {
    git(repo, &["worktree", "remove", "--force", &path.to_string_lossy()]).map(|_| ())
}

/// The worktree path that currently has `branch` checked out, if any.
pub fn worktree_for_branch(repo: &Path, branch: &str) -> Option<PathBuf> {
    let out = git(repo, &["worktree", "list", "--porcelain"]).ok()?;
    let mut cur: Option<PathBuf> = None;
    for line in out.lines() {
        if let Some(p) = line.strip_prefix("worktree ") {
            cur = Some(PathBuf::from(p));
        } else if let Some(b) = line.strip_prefix("branch ") {
            let name = b.strip_prefix("refs/heads/").unwrap_or(b);
            if name == branch {
                return cur;
            }
        }
    }
    None
}

/// Whether `dir`'s working tree is clean (no staged/unstaged changes).
pub fn is_clean(dir: &Path) -> bool {
    git(dir, &["status", "--porcelain"])
        .map(|s| s.is_empty())
        .unwrap_or(false)
}

/// Merge `branch` into `base`. Operates in whichever worktree has `base` checked
/// out (commonly the project's main folder). Aborts on conflict so nothing is left
/// half-merged. Returns a human-readable summary on success.
pub fn merge_into_base(repo: &Path, base: &str, branch: &str) -> Result<String, String> {
    let base_wt = worktree_for_branch(repo, base).ok_or_else(|| {
        format!("Branch '{base}' isn't checked out anywhere — check it out (e.g. in your project folder) and try again.")
    })?;
    if !is_clean(&base_wt) {
        return Err(format!(
            "The checkout of '{base}' has uncommitted changes — commit or stash them first."
        ));
    }
    match git(&base_wt, &["merge", "--no-edit", branch]) {
        Ok(msg) => {
            let head = msg.lines().next().unwrap_or("").trim();
            Ok(if head.is_empty() {
                format!("Merged '{branch}' into '{base}'.")
            } else {
                format!("Merged '{branch}' into '{base}' — {head}")
            })
        }
        Err(e) => {
            let _ = git(&base_wt, &["merge", "--abort"]);
            Err(format!(
                "Couldn't merge '{branch}' into '{base}' (conflicts?). Left '{base}' untouched; resolve manually. {e}"
            ))
        }
    }
}

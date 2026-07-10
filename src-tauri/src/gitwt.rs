//! Per-session git worktrees: each Claude session works on its own branch in an
//! isolated worktree (managed under the app data dir), so sessions can run
//! concurrently/autonomously without clobbering each other's files. Branches live
//! in the user's real repo, so a session's work can be merged back with normal git.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Heavy, gitignored dirs COW-cloned into a fresh worktree so an autonomous agent
/// can build/run immediately instead of paying a cold `npm install` / `cargo build`.
/// (A worktree only checks out *tracked* files, so these are otherwise absent.)
/// `.git` is intentionally excluded — the worktree already has its own gitlink.
const HEAVY_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".venv",
    "venv",
    "dist",
    "build",
    ".next",
    ".svelte-kit",
    ".turbo",
];

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

/// COW-clone (APFS clonefile) the heavy gitignored build dirs from `project_dir`
/// into a freshly created `worktree`, so a session can build/run without a cold
/// install. Block-level copy-on-write: near-instant, shares disk until written, and
/// each worktree stays isolated on mutation. Best-effort — a failure just means the
/// agent reinstalls. Skips dirs absent in the source or already present in the tree.
pub fn seed_heavy_dirs(project_dir: &Path, worktree: &Path) {
    for d in HEAVY_DIRS {
        let src = project_dir.join(d);
        let dst = worktree.join(d);
        if !src.is_dir() || dst.exists() {
            continue;
        }
        // `cp -cR`: clonefile on APFS, plain recursive copy elsewhere.
        let _ = Command::new("/bin/cp")
            .arg("-cR")
            .arg(&src)
            .arg(&dst)
            .status();
    }
}

/// Stage everything and commit on `dir`'s current branch, so the session branch
/// carries real, mergeable history (and forks inherit committed work). Returns
/// Ok(true) if a commit was made, Ok(false) if the tree was already clean. Uses a
/// fixed identity via env so it works in repos with no configured user.name/email,
/// and skips hooks (an autonomous run shouldn't trip pre-commit hooks). `git add -A`
/// respects `.gitignore`, so heavy build dirs stay out of the commit.
pub fn commit_all(dir: &Path, message: &str) -> Result<bool, String> {
    git(dir, &["add", "-A"])?;
    // `diff --cached --quiet` exits 0 (Ok) when nothing is staged — nothing to commit.
    if git(dir, &["diff", "--cached", "--quiet"]).is_ok() {
        return Ok(false);
    }
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["commit", "--no-verify", "-m", message])
        .env("GIT_AUTHOR_NAME", "spwn session")
        .env("GIT_AUTHOR_EMAIL", "spwn@localhost")
        .env("GIT_COMMITTER_NAME", "spwn session")
        .env("GIT_COMMITTER_EMAIL", "spwn@localhost")
        .output()
        .map_err(|e| format!("failed to run git commit: {e}"))?;
    if out.status.success() {
        Ok(true)
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

/// Remove a worktree (force, so uncommitted changes don't block it). The branch
/// itself is kept (so its commits aren't lost — the user merges manually).
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

/// Number of commits in `range` (e.g. "base..branch" = commits on branch not in base).
pub fn count_commits(dir: &Path, range: &str) -> u32 {
    git(dir, &["rev-list", "--count", range])
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// Files a `branch` introduces relative to `base` (three-dot: changes since they
/// diverged), for a merge preview.
pub fn changed_files(dir: &Path, base: &str, branch: &str) -> Vec<String> {
    git(dir, &["diff", "--name-only", &format!("{base}...{branch}")])
        .map(|s| s.lines().filter(|l| !l.is_empty()).map(String::from).collect())
        .unwrap_or_default()
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

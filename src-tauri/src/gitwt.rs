//! Git helpers for per-session worktrees: branch/commit/merge operations plus the
//! worktree-location layout used to pick where a session's worktree goes. Each Claude
//! session works on its own branch in an isolated worktree so sessions can run
//! concurrently without clobbering each other's files; branches live in the user's real
//! repo, so a session's work merges back with normal git.
//!
//! Note: creating/removing the worktree itself lives in the shared global hook scripts
//! (`~/.spwn/hooks/session-created.sh` / `session-deleted.sh`), not here — this module
//! only computes the target path (see `sibling_worktrees_dir` etc.) and handles the
//! branch/commit/merge side that the app drives directly.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Prefix for the git branch each Claude session works on (e.g. `spwn/<short>`).
/// Single source of truth — both interactive and scheduled session creation use it.
/// (Historically this was `cm/`; existing branches keep their stored name.)
pub const SESSION_BRANCH_PREFIX: &str = "spwn/";

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

/// Like [`git`], but for network operations (fetch/pull/push). Sets
/// `GIT_TERMINAL_PROMPT=0` so a missing credential or an SSH passphrase prompt
/// fails fast with a readable error instead of hanging forever — the spawned
/// process has no TTY to answer an interactive prompt on. Auth still works when
/// it's non-interactive (ssh-agent, keychain-cached HTTPS credential).
fn git_net(dir: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .env("GIT_TERMINAL_PROMPT", "0")
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        // Network commands put progress on stderr; include both so the UI can
        // show a useful summary line.
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        let combined = format!("{}\n{}", stdout.trim(), stderr.trim());
        Ok(combined.trim().to_string())
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

/// The dot-prefixed sibling directory that holds all of `repo`'s worktrees:
/// `<repo-parent>/.<repo-name>-worktrees/`. It lives *outside* the working tree, so no
/// build tool, file watcher, or IDE indexer ever recurses into it. Falls back to the
/// in-repo layout if `repo` has no parent (a filesystem root).
pub fn sibling_worktrees_dir(repo: &Path) -> PathBuf {
    let name = repo
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "repo".to_string());
    match repo.parent() {
        Some(parent) => parent.join(format!(".{name}-worktrees")),
        None => internal_worktrees_dir(repo),
    }
}

/// The in-repo directory that holds all of `repo`'s worktrees: `<repo>/.spwn/worktrees/`.
/// The `.spwn/` dot-prefix keeps most tooling from scanning it; pair with
/// [`ensure_git_excludes`] so git treats it as ignored too.
pub fn internal_worktrees_dir(repo: &Path) -> PathBuf {
    repo.join(".spwn").join("worktrees")
}

/// Ensure `pattern` (gitignore syntax) is present in `repo`'s local `.git/info/exclude`,
/// so an in-repo worktree dir reads as ignored without touching the tracked
/// `.gitignore`. Best-effort: a failure just means `git status` shows the dir.
pub fn ensure_git_excludes(repo: &Path, pattern: &str) {
    // `--git-common-dir` points at the shared `.git` even from inside a worktree.
    let Ok(common) = git(repo, &["rev-parse", "--git-common-dir"]) else {
        return;
    };
    let mut git_dir = PathBuf::from(&common);
    if git_dir.is_relative() {
        git_dir = repo.join(git_dir);
    }
    let exclude = git_dir.join("info").join("exclude");
    let existing = std::fs::read_to_string(&exclude).unwrap_or_default();
    if existing.lines().any(|l| l.trim() == pattern) {
        return; // already excluded
    }
    if let Some(parent) = exclude.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut content = existing;
    if !content.is_empty() && !content.ends_with('\n') {
        content.push('\n');
    }
    content.push_str("# spwn session worktrees\n");
    content.push_str(pattern);
    content.push('\n');
    let _ = std::fs::write(&exclude, content);
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

// ---------------------------------------------------------------------------
// Source Control: managing the project's *main* checkout (branch switch + sync).
// These operate on `dir` directly (the project directory), not a session worktree.
// ---------------------------------------------------------------------------

/// The upstream (remote-tracking) branch of `dir`'s current branch, if one is
/// configured — e.g. `origin/main`. None when the branch has no upstream.
pub fn upstream_branch(dir: &Path) -> Option<String> {
    git(dir, &["rev-parse", "--abbrev-ref", "--symbolic-full-name", "@{upstream}"])
        .ok()
        .filter(|s| !s.is_empty())
}

/// How many commits the current branch is ahead of / behind its upstream,
/// as `(ahead, behind)`. Returns `(0, 0)` when there's no upstream or on error.
pub fn ahead_behind(dir: &Path) -> (u32, u32) {
    // `--left-right --count HEAD...@{upstream}` prints "<ahead>\t<behind>".
    let Ok(out) = git(
        dir,
        &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
    ) else {
        return (0, 0);
    };
    let mut parts = out.split_whitespace();
    let ahead = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let behind = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (ahead, behind)
}

/// Local branch names (`refs/heads`), sorted by most-recent commit first.
pub fn local_branches(dir: &Path) -> Result<Vec<String>, String> {
    git(
        dir,
        &[
            "for-each-ref",
            "--sort=-committerdate",
            "--format=%(refname:short)",
            "refs/heads",
        ],
    )
    .map(|s| s.lines().filter(|l| !l.is_empty()).map(String::from).collect())
}

/// Remote-tracking branch names (`refs/remotes`), excluding the `*/HEAD` symrefs.
pub fn remote_branches(dir: &Path) -> Result<Vec<String>, String> {
    git(
        dir,
        &["for-each-ref", "--format=%(refname:short)", "refs/remotes"],
    )
    .map(|s| {
        s.lines()
            .filter(|l| !l.is_empty() && !l.ends_with("/HEAD"))
            .map(String::from)
            .collect()
    })
}

/// Check out an existing branch in `dir`. Git's own stderr is returned on failure
/// (e.g. "would be overwritten by checkout", "already checked out at <path>"),
/// so the UI can show exactly why it didn't switch.
pub fn checkout_branch(dir: &Path, branch: &str) -> Result<(), String> {
    git(dir, &["checkout", branch]).map(|_| ())
}

/// Create a new branch off the current HEAD and switch to it.
pub fn create_branch(dir: &Path, name: &str) -> Result<(), String> {
    git(dir, &["checkout", "-b", name]).map(|_| ())
}

/// Fetch all remotes and prune deleted remote-tracking refs.
pub fn fetch(dir: &Path) -> Result<String, String> {
    git_net(dir, &["fetch", "--all", "--prune"])
}

/// Fast-forward-only pull. Fails (rather than creating a merge commit) if the
/// branch has diverged — the UI surfaces git's message so the user can resolve
/// manually.
pub fn pull(dir: &Path) -> Result<String, String> {
    git_net(dir, &["pull", "--ff-only"])
}

/// Push the current branch. When `set_upstream` is true (no upstream configured
/// yet), push to `origin` and set it as the tracking branch.
pub fn push(dir: &Path, set_upstream: bool) -> Result<String, String> {
    if set_upstream {
        git_net(dir, &["push", "-u", "origin", "HEAD"])
    } else {
        git_net(dir, &["push"])
    }
}

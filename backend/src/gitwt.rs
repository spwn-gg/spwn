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

/// The identity spwn commits under when the repo has none of its own.
const FALLBACK_IDENTITY: [(&str, &str); 4] = [
    ("GIT_AUTHOR_NAME", "spwn session"),
    ("GIT_AUTHOR_EMAIL", "spwn@localhost"),
    ("GIT_COMMITTER_NAME", "spwn session"),
    ("GIT_COMMITTER_EMAIL", "spwn@localhost"),
];

/// Whether `dir` has both `user.name` and `user.email` resolvable (local, global or
/// system config). A fresh home -- a container's mounted volume, say -- has no
/// `~/.gitconfig`, and git then refuses to write any commit at all.
fn has_git_identity(dir: &Path) -> bool {
    ["user.name", "user.email"].iter().all(|key| {
        Command::new("git")
            .arg("-C")
            .arg(dir)
            .args(["config", "--get", key])
            .output()
            .map(|o| o.status.success() && !o.stdout.trim_ascii().is_empty())
            .unwrap_or(false)
    })
}

/// Like [`git`], for a command that can write a commit. Falls back to spwn's own
/// identity only when the repo has none -- the user's own, when they have one, is
/// what should land on a merge commit they asked for.
fn git_committing(dir: &Path, args: &[&str]) -> Result<String, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(dir).args(args);
    if !has_git_identity(dir) {
        cmd.envs(FALLBACK_IDENTITY);
    }
    let out = cmd
        .output()
        .map_err(|e| format!("failed to run git: {e}"))?;
    if out.status.success() {
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

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
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(match crate::gitauth::auth_hint(&err) {
            Some(hint) => format!("{err}\n\n{hint}"),
            None => err,
        })
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
        .envs(FALLBACK_IDENTITY)
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

/// The outcome of a trial merge, computed without touching a worktree or moving a ref.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MergePreview {
    /// The merge applies cleanly.
    Clean,
    /// The merge would conflict, in these paths.
    Conflicts(Vec<String>),
    /// The trial couldn't be run at all -- unrelated histories, an unknown ref, or a
    /// git older than 2.38 (which has no `merge-tree --write-tree`). Carries git's own
    /// message. Deliberately distinct from `Clean`: "nothing conflicts" and "we
    /// couldn't tell" must never render the same, or the panel quietly promises a
    /// clean merge it never actually checked.
    Unavailable(String),
}

/// Would merging `branch` into `base` conflict? Runs the entire merge in memory via
/// `git merge-tree`: no worktree is touched, no ref moves, and there is nothing to
/// abort afterwards -- unlike [`merge_into_base`], which can only answer the question
/// by doing it for real and rolling back.
///
/// `--write-tree` does write the merged blobs and trees into the object database.
/// They're unreferenced and `git gc` collects them; this is the same trick forges use
/// to put a mergeability badge on a pull request.
///
/// `-z` is load-bearing, not a style choice: without it git C-quotes any path that
/// isn't plain ASCII (`uni-café.txt` comes back as `"uni-caf\303\251.txt"`), so a
/// preview in a repo with accented filenames would name paths that match nothing.
pub fn merge_preview(dir: &Path, base: &str, branch: &str) -> MergePreview {
    let out = match Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["merge-tree", "--write-tree", "--name-only", "-z", base, branch])
        .output()
    {
        Ok(out) => out,
        Err(e) => return MergePreview::Unavailable(format!("failed to run git: {e}")),
    };
    let stdout = String::from_utf8_lossy(&out.stdout);
    let mut fields = stdout.split('\0');
    // Exit 1 means BOTH "conflicts found" and "that isn't a ref" -- git tells them
    // apart only by stdout, which leads with the merged tree's oid whenever the merge
    // actually ran. Checking the status alone would report a typo'd branch as clean.
    let ran = matches!(out.status.code(), Some(0 | 1));
    let Some(_merged_tree) = fields.next().filter(|oid| ran && !oid.is_empty()) else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        return MergePreview::Unavailable(if err.is_empty() {
            format!("git merge-tree failed ({})", out.status)
        } else {
            err
        });
    };
    // Conflicted paths run until the empty field closing the section; what follows is
    // git's own prose about each conflict, which we don't surface.
    let conflicts: Vec<String> = fields
        .take_while(|f| !f.is_empty())
        .map(String::from)
        .collect();
    if conflicts.is_empty() {
        MergePreview::Clean
    } else {
        MergePreview::Conflicts(conflicts)
    }
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
    match git_committing(&base_wt, &["merge", "--no-edit", branch]) {
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

/// Expand a repo reference into a clonable URL. Accepts full URLs (`https://…`,
/// `git@host:…`, `ssh://…`), `github.com/owner/repo`, and the `owner/repo` shorthand
/// (resolved against GitHub).
pub fn normalize_repo_url(input: &str) -> Result<String, String> {
    let s = input.trim().trim_end_matches('/');
    if s.is_empty() {
        return Err("enter a repository URL or owner/repo".to_string());
    }
    if s.contains("://") || s.starts_with("git@") {
        return Ok(s.to_string());
    }
    let path = s.strip_prefix("github.com/").unwrap_or(s);
    let parts: Vec<&str> = path.split('/').collect();
    let valid = |p: &str| !p.is_empty() && p.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c));
    match parts.as_slice() {
        [owner, repo] if valid(owner) && valid(repo) => {
            Ok(format!("https://github.com/{owner}/{}.git", repo.trim_end_matches(".git")))
        }
        _ => Err(format!("not a repository URL or owner/repo: {s}")),
    }
}

/// The directory name `git clone` would pick for `url` (last path segment, sans `.git`).
pub fn repo_name_from_url(url: &str) -> Option<String> {
    let last = url
        .trim_end_matches('/')
        .rsplit(|c| c == '/' || c == ':')
        .next()?;
    let name = last.trim_end_matches(".git");
    (!name.is_empty() && name != "." && name != "..").then(|| name.to_string())
}

/// `git clone <url> <dest>`, run from `dest`'s parent. Non-interactive (see [`git_net`]).
pub fn clone(url: &str, dest: &Path) -> Result<String, String> {
    let parent = dest.parent().ok_or("clone destination has no parent")?;
    let dest = dest.to_string_lossy();
    git_net(parent, &["clone", "--", url, &dest])
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

#[cfg(test)]
mod repo_url_tests {
    use super::*;

    #[test]
    fn normalizes_repo_references() {
        let gh = "https://github.com/owner/repo.git";
        assert_eq!(normalize_repo_url("owner/repo").unwrap(), gh);
        assert_eq!(normalize_repo_url(" owner/repo.git/ ").unwrap(), gh);
        assert_eq!(normalize_repo_url("github.com/owner/repo").unwrap(), gh);
        assert_eq!(normalize_repo_url("https://github.com/o/r").unwrap(), "https://github.com/o/r");
        assert_eq!(normalize_repo_url("git@github.com:o/r.git").unwrap(), "git@github.com:o/r.git");
        assert!(normalize_repo_url("").is_err());
        assert!(normalize_repo_url("just-a-name").is_err());
        assert!(normalize_repo_url("a/b/c").is_err());
        assert!(normalize_repo_url("o/--upload-pack=x;").is_err());
    }

    #[test]
    fn derives_clone_folder_name() {
        assert_eq!(repo_name_from_url("https://github.com/o/spwn.git").as_deref(), Some("spwn"));
        assert_eq!(repo_name_from_url("git@github.com:o/spwn.git").as_deref(), Some("spwn"));
        assert_eq!(repo_name_from_url("https://github.com/o/spwn/").as_deref(), Some("spwn"));
        assert_eq!(repo_name_from_url("https://github.com/o/..").as_deref(), None);
    }
}

#[cfg(test)]
mod merge_preview_tests {
    use super::*;

    /// A repo on `main` with one committed file, ready to diverge.
    fn repo() -> tempfile::TempDir {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        git(dir, &["init", "-q", "-b", "main", "."]).unwrap();
        std::fs::write(dir.join("f.txt"), "one\ntwo\nthree\n").unwrap();
        commit_all(dir, "base").unwrap();
        tmp
    }

    /// Branch off `main`, write `file`, commit, and return to `main`.
    fn branch_with(dir: &Path, name: &str, file: &str, body: &str) {
        git(dir, &["checkout", "-q", "-b", name]).unwrap();
        std::fs::write(dir.join(file), body).unwrap();
        commit_all(dir, name).unwrap();
        git(dir, &["checkout", "-q", "main"]).unwrap();
    }

    #[test]
    fn divergent_files_merge_clean() {
        let tmp = repo();
        let dir = tmp.path();
        branch_with(dir, "feat", "g.txt", "from the session\n");
        std::fs::write(dir.join("h.txt"), "from main\n").unwrap();
        commit_all(dir, "main moves too").unwrap();
        assert_eq!(merge_preview(dir, "main", "feat"), MergePreview::Clean);
    }

    #[test]
    fn overlapping_edits_name_the_conflicting_path() {
        let tmp = repo();
        let dir = tmp.path();
        branch_with(dir, "feat", "f.txt", "FEAT\ntwo\nthree\n");
        std::fs::write(dir.join("f.txt"), "MAIN\ntwo\nthree\n").unwrap();
        commit_all(dir, "main edits the same line").unwrap();
        assert_eq!(
            merge_preview(dir, "main", "feat"),
            MergePreview::Conflicts(vec!["f.txt".to_string()])
        );
    }

    /// The trap this function exists to avoid: `merge-tree` exits 1 for conflicts AND
    /// for an unknown ref, so keying off the exit code alone would call a typo'd
    /// branch a clean merge.
    #[test]
    fn unknown_ref_is_unavailable_not_clean() {
        let tmp = repo();
        match merge_preview(tmp.path(), "main", "no-such-branch") {
            MergePreview::Unavailable(msg) => assert!(!msg.is_empty(), "want git's message"),
            other => panic!("expected Unavailable, got {other:?}"),
        }
    }

    #[test]
    fn unrelated_histories_are_unavailable() {
        let tmp = repo();
        let dir = tmp.path();
        git(dir, &["checkout", "-q", "--orphan", "solo"]).unwrap();
        std::fs::write(dir.join("only.txt"), "x\n").unwrap();
        commit_all(dir, "orphan root").unwrap();
        git(dir, &["checkout", "-q", "main"]).unwrap();
        assert!(matches!(
            merge_preview(dir, "main", "solo"),
            MergePreview::Unavailable(_)
        ));
    }

    /// Without `-z` this path comes back C-quoted and matches nothing on disk.
    #[test]
    fn non_ascii_conflict_paths_survive_verbatim() {
        let tmp = repo();
        let dir = tmp.path();
        let name = "uni-café.txt";
        std::fs::write(dir.join(name), "a\n").unwrap();
        commit_all(dir, "add an accented filename").unwrap();
        branch_with(dir, "feat", name, "FEAT\n");
        std::fs::write(dir.join(name), "MAIN\n").unwrap();
        commit_all(dir, "main edits it too").unwrap();
        assert_eq!(
            merge_preview(dir, "main", "feat"),
            MergePreview::Conflicts(vec![name.to_string()])
        );
    }
}

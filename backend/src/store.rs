//! spwn's own project model: a project is a named working directory
//! that groups terminals you've opened. Persisted to `app_data_dir/projects.json`
//! so projects + their terminals survive restarts (terminals reattach to their
//! still-alive rmux sessions by stable id).

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct TerminalRec {
    pub id: String,
    pub title: String,
    /// `"shell"` | `"agent"`.
    ///
    /// Records written before the sidecar was retired carry `"claude"`; they are
    /// rewritten to `"agent"` on load (see [`ProjectStore::migrate`]).
    pub kind: String,
    /// Which agent definition drives this session (`TerminalRec.kind == "agent"`).
    /// `None` for shells and for legacy sidecar sessions.
    #[serde(default)]
    pub agent: Option<String>,
    pub cwd: String,
    /// Claude session id once discovered (enables the transcript/rewind panel).
    #[serde(default)]
    pub session_id: Option<String>,
    /// Groups forks/branches together. A fresh session has None (its own group,
    /// keyed by its id); a fork/branch inherits its source's group key.
    #[serde(default)]
    pub group_id: Option<String>,
    /// The terminal this one was forked from (its direct parent in the branch
    /// tree). None for a root session. Lets the nav render true fork lineage.
    #[serde(default)]
    pub parent_id: Option<String>,
    /// Git branch this session works on in its own worktree (None if the project
    /// isn't a git repo, or the worktree couldn't be created). `cwd` points at the
    /// worktree when set.
    #[serde(default)]
    pub branch: Option<String>,
    /// The branch this session's branch should merge back into.
    #[serde(default)]
    pub base_branch: Option<String>,
    /// A persisted attention flag: the session finished a turn / hit a prompt / failed
    /// while the user wasn't looking. Set by the sidecar reader (any session) and by
    /// windowless scheduled runs; cleared when the session is viewed. Survives restart
    /// (where no live sidecar exists) so the nav still reflects it on next launch.
    #[serde(default)]
    pub needs_attention: bool,
    /// Why attention is needed — `"blocked"` | `"done"` | `"error"`. Lets the sidebar
    /// render the right treatment after restart, before a sidecar re-attaches. None
    /// when `needs_attention` is false.
    #[serde(default)]
    pub attention_reason: Option<String>,
    /// The environment this session's panes run in, reported by a `session-created`
    /// hook via `::spwn:set:: exec=…`. None → panes run directly on the host, which
    /// is every session that has no such hook.
    #[serde(default)]
    pub exec: Option<ExecSpec>,
}

/// How to run a session's processes somewhere other than the host — a container, a
/// VM, a remote shell. spwn never creates or inspects that environment; a hook does,
/// and reports back how to reach it.
///
/// Deliberately opaque: the prefix is whatever the hook says, so spwn carries no
/// opinion about Docker (the per-session docker-compose integration that preceded
/// project hooks was removed for exactly that reason — see `hooks.rs`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ExecSpec {
    /// Command prefix every interactive pane's argv is prepended with, e.g.
    /// `docker exec -it -w /path/to/worktree spwn-<id>`.
    ///
    /// For a TUI agent this MUST allocate a tty (`-t` under docker): without one the
    /// agent renders nothing, every `detect` rule in its definition misses, and
    /// `C-c` kills the wrapper instead of interrupting the turn.
    pub prefix: String,
    /// Prefix for headless (scheduled) runs. Separate from `prefix` because headless
    /// output is parsed as line-delimited JSON — a tty interleaves spinner bytes with
    /// it — so this one must NOT allocate a tty. None → scheduled runs stay on the
    /// host rather than silently failing to parse.
    #[serde(default)]
    pub headless_prefix: Option<String>,
    /// Agent binary inside the environment. None → the agent definition's bare
    /// `binary.name`, resolved by the environment's own PATH. The host-resolved
    /// absolute path is never right here: a macOS binary can't exec in a Linux
    /// container.
    #[serde(default)]
    pub bin: Option<String>,
    /// Shell for shell panes inside the environment. None → `/bin/sh`, which is the
    /// only shell a minimal image is guaranteed to have.
    #[serde(default)]
    pub shell: Option<String>,
}

fn default_true() -> bool {
    true
}

/// A per-project scheduled task: a prompt run headlessly on a daily/weekly
/// cadence, optionally reusing the project's assembled context.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ScheduledTask {
    pub id: String,
    pub name: String,
    /// The task instruction sent as the session's first turn.
    pub prompt: String,
    /// Local time of day, "HH:MM" (24h).
    pub time: String,
    /// Weekdays it may fire on: 0=Sun..6=Sat. Empty = every day.
    #[serde(default)]
    pub weekdays: Vec<u8>,
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Prepend the project's assembled context to the prompt.
    #[serde(default = "default_true")]
    pub use_context: bool,
    /// Epoch ms of the scheduled instant last fired (not wall-clock of the run) —
    /// gates no-double-fire and single-shot catch-up.
    #[serde(default)]
    pub last_run: Option<i64>,
}

/// A block in a project's context space: a manual note, a file's contents, or a
/// turn picked from a session. Blocks are assembled into a first message on inject.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ContextBlock {
    pub id: String,
    /// "note" | "file" | "session"
    pub kind: String,
    /// Short label (filename, role, or "note").
    pub label: String,
    pub text: String,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ProjectRec {
    pub id: String,
    pub name: String,
    pub directory: String,
    #[serde(default)]
    pub terminals: Vec<TerminalRec>,
    /// The project's context space (composed, then injected into a new session).
    #[serde(default)]
    pub context: Vec<ContextBlock>,
    /// Scheduled tasks that fire headless Claude runs on a cadence.
    #[serde(default)]
    pub scheduled_tasks: Vec<ScheduledTask>,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
pub struct ProjectStore {
    pub projects: Vec<ProjectRec>,
}

impl ProjectStore {
    pub fn load(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Rewrite legacy sidecar records as TUI agent sessions.
    ///
    /// The Agent-SDK sidecar is gone, so a `kind: "claude"` record has nothing left
    /// to render it. Its conversation is not lost: the session id is unchanged, so
    /// the session reopens as the agent's own TUI resuming exactly where it was, and
    /// its worktree, branch and checkpoints carry over untouched.
    ///
    /// Idempotent, and it never overwrites an agent that was already chosen.
    pub fn migrate(&mut self) {
        for p in &mut self.projects {
            for t in &mut p.terminals {
                if t.kind == "claude" {
                    t.kind = "agent".to_string();
                    t.agent.get_or_insert_with(|| "claude".to_string());
                }
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_else(|_| "{}".into());
        std::fs::write(path, json)
    }

    pub fn project(&self, id: &str) -> Option<&ProjectRec> {
        self.projects.iter().find(|p| p.id == id)
    }

    pub fn project_mut(&mut self, id: &str) -> Option<&mut ProjectRec> {
        self.projects.iter_mut().find(|p| p.id == id)
    }

    /// Find a terminal record (and its project's directory) by terminal id.
    pub fn terminal(&self, terminal_id: &str) -> Option<&TerminalRec> {
        self.projects
            .iter()
            .flat_map(|p| p.terminals.iter())
            .find(|t| t.id == terminal_id)
    }

    /// Mutable lookup of a terminal record by id, across all projects.
    pub fn terminal_mut(&mut self, terminal_id: &str) -> Option<&mut TerminalRec> {
        self.projects
            .iter_mut()
            .flat_map(|p| p.terminals.iter_mut())
            .find(|t| t.id == terminal_id)
    }
}

#[allow(dead_code)]
impl TerminalRec {
    /// Does this session run a coding agent (as opposed to a plain shell)?
    pub fn is_agent(&self) -> bool {
        self.kind == "agent"
    }
}

/// The rmux session name for a terminal — stable across restarts so we can
/// reattach to the same daemon-side session.
pub fn rmux_session_name(terminal_id: &str) -> String {
    format!("cm-{}", terminal_id.replace('-', ""))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(kind: &str, agent: Option<&str>) -> TerminalRec {
        TerminalRec {
            id: "t1".into(),
            title: "t".into(),
            kind: kind.into(),
            agent: agent.map(str::to_string),
            cwd: "/tmp".into(),
            session_id: None,
            group_id: None,
            parent_id: None,
            branch: None,
            base_branch: None,
            needs_attention: false,
            attention_reason: None,
            exec: None,
        }
    }

    #[test]
    fn distinguishes_sessions_from_shells() {
        assert!(!rec("shell", None).is_agent());
        assert!(rec("agent", Some("claude")).is_agent());
    }

    fn migrated(json: &str) -> ProjectStore {
        let mut s: ProjectStore = serde_json::from_str(json).unwrap();
        s.migrate();
        s
    }

    #[test]
    fn a_legacy_sidecar_record_becomes_a_claude_tui_session() {
        // Sessions created before the sidecar was retired must keep working. The
        // session id is untouched, so the conversation resumes rather than restarts —
        // losing it would mean losing real work and its worktree.
        let store = migrated(
            r#"{"projects":[{"id":"p","name":"P","directory":"/tmp","terminals":[
                {"id":"t","title":"my session","kind":"claude","cwd":"/tmp",
                 "sessionId":"abc-123","branch":"spwn/t"}]}]}"#,
        );
        let t = &store.projects[0].terminals[0];
        assert_eq!(t.kind, "agent");
        assert_eq!(t.agent.as_deref(), Some("claude"));
        assert_eq!(t.session_id.as_deref(), Some("abc-123"));
        assert_eq!(t.branch.as_deref(), Some("spwn/t"));
    }

    #[test]
    fn a_record_written_before_environments_existed_still_loads() {
        // `ProjectStore::load` turns ANY deserialization error into an empty store —
        // i.e. every project silently gone. So a store written by an older spwn, with
        // no `exec` key, must parse; that's what makes `exec` safe to add.
        let store = migrated(
            r#"{"projects":[{"id":"p","name":"P","directory":"/tmp","terminals":[
                {"id":"t","title":"s","kind":"agent","agent":"claude","cwd":"/tmp"}]}]}"#,
        );
        assert_eq!(store.projects[0].terminals[0].exec, None);
    }

    #[test]
    fn an_environment_round_trips_through_the_store() {
        let json = r#"{"projects":[{"id":"p","name":"P","directory":"/tmp","terminals":[
            {"id":"t","title":"s","kind":"agent","agent":"claude","cwd":"/tmp",
             "exec":{"prefix":"docker exec -it box","execShell":"/bin/bash"}}]}]}"#;
        let store = migrated(json);
        let e = store.projects[0].terminals[0].exec.as_ref().unwrap();
        assert_eq!(e.prefix, "docker exec -it box");
        // Absent optional keys stay None rather than failing the whole parse.
        assert_eq!(e.bin, None);
        assert_eq!(e.headless_prefix, None);
    }

    #[test]
    fn migration_is_idempotent_and_leaves_shells_alone() {
        let mut store = migrated(
            r#"{"projects":[{"id":"p","name":"P","directory":"/tmp","terminals":[
                {"id":"a","title":"s","kind":"shell","cwd":"/tmp"},
                {"id":"b","title":"x","kind":"claude","cwd":"/tmp"}]}]}"#,
        );
        store.migrate();
        store.migrate();
        assert_eq!(store.projects[0].terminals[0].kind, "shell");
        assert_eq!(store.projects[0].terminals[0].agent, None);
        assert_eq!(store.projects[0].terminals[1].kind, "agent");
    }

    #[test]
    fn migration_never_overrides_an_agent_already_chosen() {
        let store = migrated(
            r#"{"projects":[{"id":"p","name":"P","directory":"/tmp","terminals":[
                {"id":"t","title":"x","kind":"claude","agent":"codex","cwd":"/tmp"}]}]}"#,
        );
        assert_eq!(store.projects[0].terminals[0].agent.as_deref(), Some("codex"));
    }

    #[test]
    fn session_name_is_stable_and_dash_free() {
        // rmux session names can't contain dashes; the mapping has to stay stable
        // or a restart would fail to reattach to still-running panes.
        let n = rmux_session_name("0bffa637-c355-43ff-9c59-7d24d78b12b0");
        assert_eq!(n, "cm-0bffa637c35543ff9c597d24d78b12b0");
        assert_eq!(n, rmux_session_name("0bffa637-c355-43ff-9c59-7d24d78b12b0"));
    }
}

/// Resolve the on-disk path for the project store under the app data dir.
pub fn store_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("projects.json")
}

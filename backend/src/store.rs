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
    /// `"shell"` | `"agent"` | `"claude"`.
    ///
    /// `"claude"` is the legacy Agent-SDK-sidecar session; `"agent"` is a coding
    /// agent driven as a TUI in an rmux pane. Both exist while the two paths run
    /// side by side — existing sessions keep their sidecar until the UI flips over,
    /// so this milestone can't strand anyone mid-conversation.
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
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default()
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

// Consumed by the status/turn watchers and the UI flip in later milestones; the
// classification lives here so there's one definition of "is this an agent".
#[allow(dead_code)]
impl TerminalRec {
    /// Does this session run a coding agent (either transport)?
    pub fn is_agent(&self) -> bool {
        self.kind == "agent" || self.kind == "claude"
    }

    /// Is this a legacy Agent-SDK-sidecar session rather than an rmux TUI?
    pub fn is_sidecar(&self) -> bool {
        self.kind == "claude"
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
        }
    }

    #[test]
    fn classifies_the_three_kinds() {
        assert!(!rec("shell", None).is_agent());
        assert!(rec("agent", Some("claude")).is_agent());
        assert!(!rec("agent", Some("claude")).is_sidecar());
        // Legacy records are agents, but on the sidecar transport.
        assert!(rec("claude", None).is_agent());
        assert!(rec("claude", None).is_sidecar());
    }

    #[test]
    fn an_old_store_without_the_agent_field_still_loads() {
        // Sessions predating multi-agent support must survive an upgrade — losing
        // them would mean losing running conversations and their worktrees.
        let json = r#"{"projects":[{"id":"p","name":"P","directory":"/tmp",
            "terminals":[{"id":"t","title":"claude","kind":"claude","cwd":"/tmp"}]}]}"#;
        let store: ProjectStore = serde_json::from_str(json).unwrap();
        let t = &store.projects[0].terminals[0];
        assert_eq!(t.kind, "claude");
        assert_eq!(t.agent, None);
        assert!(t.is_sidecar());
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

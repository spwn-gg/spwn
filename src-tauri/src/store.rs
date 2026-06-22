//! Context Manager's own project model: a project is a named working directory
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
    /// "shell" | "claude"
    pub kind: String,
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

/// The rmux session name for a terminal — stable across restarts so we can
/// reattach to the same daemon-side session.
pub fn rmux_session_name(terminal_id: &str) -> String {
    format!("cm-{}", terminal_id.replace('-', ""))
}

/// Resolve the on-disk path for the project store under the app data dir.
pub fn store_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("projects.json")
}

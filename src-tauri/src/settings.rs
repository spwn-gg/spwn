//! User settings, persisted to `app_data_dir/settings.json`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Where per-session git worktrees are placed. See `gitwt` for how each maps to a
/// concrete directory.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum WorktreeLocation {
    /// A dot-prefixed sibling next to the repo: `<repo-parent>/.<repo-name>-worktrees/`.
    /// Outside the working tree, so no build tool or file watcher ever recurses in.
    #[default]
    Sibling,
    /// Inside the repo at `.spwn/worktrees/`, registered in `.git/info/exclude`. The
    /// dot-prefix keeps most tooling from scanning it; kept out of git via the exclude.
    Internal,
    /// Under the app's data dir (`…/com.markbarta.spwn/worktrees/`), away from repos.
    AppData,
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// Override for the `claude` CLI path. Empty/None falls back to auto-detect.
    #[serde(default)]
    pub claude_path: Option<String>,
    /// Where new session worktrees are created. Only affects sessions started after
    /// it's changed; existing worktrees stay where they were made.
    #[serde(default)]
    pub worktree_location: WorktreeLocation,
}

impl Settings {
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
}

pub fn settings_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("settings.json")
}

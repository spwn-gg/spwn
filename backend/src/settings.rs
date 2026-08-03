//! User settings, persisted to `app_data_dir/settings.json`.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
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

fn default_true() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// LEGACY: the pre-multi-agent override for the `claude` CLI path. Still read so
    /// an existing settings.json keeps working; [`Settings::migrate`] folds it into
    /// `agent_paths` on load. New writes go to `agent_paths` only.
    #[serde(default)]
    pub claude_path: Option<String>,
    /// Per-agent binary overrides, keyed by agent id. Empty/absent ⇒ auto-detect.
    #[serde(default)]
    pub agent_paths: BTreeMap<String, String>,
    /// Agent id used for new sessions when the caller doesn't pick one. `None` ⇒
    /// the first installed agent.
    #[serde(default)]
    pub default_agent: Option<String>,
    /// Where new session worktrees are created. Only affects sessions started after
    /// it's changed; existing worktrees stay where they were made.
    #[serde(default)]
    pub worktree_location: WorktreeLocation,
    /// Whether the shared global hooks in `~/.spwn/hooks` run. When off, spwn falls
    /// back to its native built-in behavior (worktree create/remove) and only per-repo
    /// `.spwn/hooks` still run. Defaults to on.
    #[serde(default = "default_true")]
    pub global_hooks_enabled: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            claude_path: None,
            agent_paths: BTreeMap::new(),
            default_agent: None,
            worktree_location: WorktreeLocation::default(),
            global_hooks_enabled: true,
        }
    }
}

impl Settings {
    pub fn load(path: &Path) -> Self {
        let mut s: Self = std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        s.migrate();
        s
    }

    /// Fold the legacy single-agent `claude_path` into `agent_paths["claude"]`.
    /// Idempotent, and never overwrites an explicit per-agent override.
    ///
    /// Called on **save** as well as load: the Settings UI still round-trips
    /// `claudePath`, so without normalizing on the way in, saving any setting would
    /// move the configured path back into the legacy field where nothing reads it —
    /// and the user's custom `claude` binary would be silently ignored until restart.
    pub(crate) fn migrate(&mut self) {
        if let Some(p) = self.claude_path.take().filter(|p| !p.is_empty()) {
            self.agent_paths.entry("claude".to_string()).or_insert(p);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn load_from(json: &str) -> Settings {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("settings.json");
        std::fs::write(&p, json).unwrap();
        Settings::load(&p)
    }

    #[test]
    fn legacy_claude_path_moves_into_agent_paths() {
        let s = load_from(r#"{"claudePath":"/opt/claude"}"#);
        assert_eq!(s.agent_paths.get("claude").map(String::as_str), Some("/opt/claude"));
        assert_eq!(s.claude_path, None, "legacy field should be consumed");
    }

    #[test]
    fn an_explicit_agent_path_wins_over_the_legacy_field() {
        let s = load_from(r#"{"claudePath":"/old","agentPaths":{"claude":"/new"}}"#);
        assert_eq!(s.agent_paths.get("claude").map(String::as_str), Some("/new"));
    }

    #[test]
    fn an_empty_legacy_path_is_not_migrated_as_a_real_override() {
        // "" meant "auto-detect"; carrying it over as an override would make
        // binary resolution fail for everyone who ever opened Settings.
        let s = load_from(r#"{"claudePath":""}"#);
        assert!(s.agent_paths.is_empty());
    }

    #[test]
    fn defaults_apply_to_an_empty_or_missing_file() {
        let s = load_from("{}");
        assert!(s.global_hooks_enabled);
        assert_eq!(s.worktree_location, WorktreeLocation::Sibling);
        assert!(s.default_agent.is_none());
    }
}

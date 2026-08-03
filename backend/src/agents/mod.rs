//! Pluggable agent definitions: which coding-agent CLIs spwn can drive, and how.
//!
//! An agent is data, not code (see [`def::AgentDef`]). This module loads those
//! definitions from disk, resolves each one's binary, and answers "what can this
//! agent actually do" so the UI can disable affordances instead of offering ones
//! that will fail.

pub mod def;
pub mod defaults;

pub use def::{AgentDef, Capabilities, SessionStatus};
pub use defaults::{global_agents_dir, install_default_agents};

use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Where a definition came from. Later scopes override earlier ones by `id`.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum AgentScope {
    /// Compiled into the binary, so spwn works with an empty `~/.spwn`.
    BuiltIn,
    /// `~/.spwn/agents/*.toml` — the user's own, applies everywhere.
    Global,
    /// `<project>/.spwn/agents/*.toml` — committed with a repo.
    Repo,
}

/// A loaded definition plus where it came from.
#[derive(Clone, Debug)]
pub struct LoadedAgent {
    pub def: AgentDef,
    pub scope: AgentScope,
}

/// Everything the UI needs to render one agent in a picker.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct AgentSummary {
    pub id: String,
    pub name: String,
    pub icon: Option<String>,
    pub untested: bool,
    pub scope: AgentScope,
    /// Resolved executable, or `None` if it isn't installed. The UI shows
    /// "not found" rather than letting the user start a session that can't run.
    pub binary: Option<String>,
    pub capabilities: Capabilities,
}

/// The set of agents spwn currently knows about.
#[derive(Default, Clone, Debug)]
pub struct AgentRegistry {
    by_id: BTreeMap<String, LoadedAgent>,
    /// Parse failures, surfaced to the UI so a typo in a hand-edited TOML is
    /// visible instead of the agent just silently vanishing from the picker.
    errors: Vec<String>,
}

impl AgentRegistry {
    /// Load built-ins, then `~/.spwn/agents`, then the project's `.spwn/agents`.
    pub fn load(project_dir: Option<&Path>) -> Self {
        let mut reg = Self::default();
        reg.load_builtin();
        if let Some(dir) = global_agents_dir() {
            reg.load_dir(&dir, AgentScope::Global);
        }
        if let Some(p) = project_dir {
            reg.load_dir(&p.join(".spwn").join("agents"), AgentScope::Repo);
        }
        reg
    }

    fn load_builtin(&mut self) {
        for (name, body) in defaults::bundled() {
            match toml::from_str::<AgentDef>(body) {
                Ok(def) => {
                    self.by_id.insert(
                        def.id.clone(),
                        LoadedAgent { def, scope: AgentScope::BuiltIn },
                    );
                }
                // A bundled definition failing to parse is a build-time bug, not a
                // user problem; the unit tests in `def` catch it before release.
                Err(e) => self.errors.push(format!("built-in {name}: {e}")),
            }
        }
    }

    fn load_dir(&mut self, dir: &Path, scope: AgentScope) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("toml"))
            .collect();
        paths.sort(); // deterministic order regardless of readdir
        for path in paths {
            let Ok(body) = std::fs::read_to_string(&path) else {
                continue;
            };
            match toml::from_str::<AgentDef>(&body) {
                Ok(def) => {
                    self.by_id
                        .insert(def.id.clone(), LoadedAgent { def, scope });
                }
                Err(e) => self
                    .errors
                    .push(format!("{}: {e}", path.file_name().unwrap_or_default().to_string_lossy())),
            }
        }
    }

    pub fn get(&self, id: &str) -> Option<&AgentDef> {
        self.by_id.get(id).map(|l| &l.def)
    }

    pub fn errors(&self) -> &[String] {
        &self.errors
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Summaries for the UI, sorted so installed agents come first and untested
    /// ones last — the picker should lead with what will actually work.
    pub fn summaries(&self, overrides: &BTreeMap<String, String>) -> Vec<AgentSummary> {
        let mut out: Vec<AgentSummary> = self
            .by_id
            .values()
            .map(|l| AgentSummary {
                id: l.def.id.clone(),
                name: l.def.name.clone(),
                icon: l.def.icon.clone(),
                untested: l.def.untested,
                scope: l.scope,
                binary: resolve_binary(&l.def, overrides)
                    .map(|p| p.to_string_lossy().into_owned()),
                capabilities: l.def.capabilities(),
            })
            .collect();
        out.sort_by(|a, b| {
            let key = |s: &AgentSummary| (s.binary.is_none(), s.untested, s.id.clone());
            key(a).cmp(&key(b))
        });
        out
    }

    /// The agent to use when none is specified: the caller's preference if it
    /// resolves, else the first installed one.
    pub fn default_id(&self, preferred: Option<&str>, overrides: &BTreeMap<String, String>) -> Option<String> {
        if let Some(p) = preferred {
            if self.by_id.contains_key(p) {
                return Some(p.to_string());
            }
        }
        self.summaries(overrides)
            .into_iter()
            .find(|s| s.binary.is_some())
            .map(|s| s.id)
    }
}

/// Locate an agent's executable: explicit user override → the def's env var →
/// `$PATH` → home-relative candidates.
///
/// The candidate probing is not redundant with `$PATH`: a daemon-launched process
/// doesn't inherit the user's login shell environment, which is exactly how spwn's
/// panes start.
pub fn resolve_binary(def: &AgentDef, overrides: &BTreeMap<String, String>) -> Option<PathBuf> {
    if let Some(p) = overrides.get(&def.id).filter(|s| !s.is_empty()) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(var) = &def.binary.env {
        if let Ok(p) = std::env::var(var) {
            let pb = PathBuf::from(p);
            if pb.exists() {
                return Some(pb);
            }
        }
    }
    if let Some(p) = which(&def.binary.name) {
        return Some(p);
    }
    let home = directories::BaseDirs::new()?.home_dir().to_path_buf();
    def.binary
        .candidates
        .iter()
        .map(|rel| home.join(rel))
        .find(|p| p.exists())
}

fn which(name: &str) -> Option<PathBuf> {
    let out = std::process::Command::new("sh")
        .arg("-c")
        .arg(format!("command -v {name}"))
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!s.is_empty()).then(|| PathBuf::from(s))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn registry_with(dir: &Path, scope: AgentScope) -> AgentRegistry {
        let mut r = AgentRegistry::default();
        r.load_builtin();
        r.load_dir(dir, scope);
        r
    }

    #[test]
    fn builtins_load_and_are_addressable() {
        let mut r = AgentRegistry::default();
        r.load_builtin();
        assert!(r.errors().is_empty(), "bundled defs must parse: {:?}", r.errors());
        assert!(r.get("claude").is_some());
        assert!(r.get("codex").is_some());
        assert!(r.get("nope").is_none());
    }

    #[test]
    fn a_user_definition_overrides_the_builtin_of_the_same_id() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("claude.toml"),
            r#"
                id = "claude"
                name = "My Claude"
                [binary]
                name = "claude"
                [argv]
                new = ["{bin}"]
                resume = ["{bin}"]
            "#,
        )
        .unwrap();
        let r = registry_with(tmp.path(), AgentScope::Global);
        assert_eq!(r.get("claude").unwrap().name, "My Claude");
        // and it didn't disturb the others
        assert_eq!(r.get("codex").unwrap().name, "Codex CLI");
    }

    #[test]
    fn a_malformed_definition_is_reported_not_silently_dropped() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("broken.toml"), "id = \"broken\"\nname =").unwrap();
        let r = registry_with(tmp.path(), AgentScope::Global);
        assert_eq!(r.errors().len(), 1);
        assert!(r.errors()[0].contains("broken.toml"));
        // The built-ins still work — one bad file doesn't take the picker down.
        assert!(r.get("claude").is_some());
    }

    #[test]
    fn a_broken_user_override_falls_back_to_the_builtin() {
        // Built-ins load first and a failed override simply doesn't replace them, so
        // a typo while hand-editing degrades to the bundled definition instead of
        // making the agent vanish from the picker. Worth pinning: the alternative
        // (agent disappears, no explanation) is a genuinely confusing failure.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("gemini.toml"), "id = \"gemini\"\nname =").unwrap();
        let r = registry_with(tmp.path(), AgentScope::Global);
        assert_eq!(r.get("gemini").map(|d| d.name.as_str()), Some("Gemini CLI"));
        assert_eq!(r.errors().len(), 1);
    }

    #[test]
    fn an_explicit_override_path_wins_over_path_lookup() {
        let tmp = tempfile::tempdir().unwrap();
        let fake = tmp.path().join("my-claude");
        std::fs::write(&fake, "#!/bin/sh\n").unwrap();
        let mut r = AgentRegistry::default();
        r.load_builtin();
        let def = r.get("claude").unwrap();
        let overrides: BTreeMap<String, String> =
            [("claude".to_string(), fake.to_string_lossy().into_owned())].into();
        assert_eq!(resolve_binary(def, &overrides).unwrap(), fake);
    }

    #[test]
    fn a_nonexistent_override_falls_through_rather_than_breaking_the_agent() {
        let mut r = AgentRegistry::default();
        r.load_builtin();
        let def = r.get("claude").unwrap();
        let overrides: BTreeMap<String, String> =
            [("claude".to_string(), "/nope/does/not/exist".to_string())].into();
        // Falls back to PATH discovery. On a machine without claude this is None,
        // which is still correct behavior — the point is it doesn't return the
        // bogus path.
        assert_ne!(
            resolve_binary(def, &overrides).map(|p| p.to_string_lossy().into_owned()),
            Some("/nope/does/not/exist".to_string())
        );
    }

    #[test]
    fn summaries_put_uninstalled_and_untested_agents_last() {
        let mut r = AgentRegistry::default();
        r.load_builtin();
        let s = r.summaries(&BTreeMap::new());
        let installed_untested: Vec<(bool, bool)> =
            s.iter().map(|x| (x.binary.is_none(), x.untested)).collect();
        let mut sorted = installed_untested.clone();
        sorted.sort();
        assert_eq!(installed_untested, sorted, "summaries must be ordered");
    }

    #[test]
    fn default_id_prefers_the_requested_agent_then_an_installed_one() {
        let mut r = AgentRegistry::default();
        r.load_builtin();
        let o = BTreeMap::new();
        assert_eq!(r.default_id(Some("codex"), &o).as_deref(), Some("codex"));
        // An unknown preference must not be echoed back — it would produce an
        // unlaunchable session.
        assert_ne!(r.default_id(Some("ghost"), &o).as_deref(), Some("ghost"));
    }
}

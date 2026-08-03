//! Installing spwn's bundled agent definitions into `~/.spwn/agents/`.
//!
//! Deliberately the same contract as the default hooks (`hooks::install_defaults_into`):
//! spwn owns the files it ships and updates them on version bumps, but it never
//! clobbers a user's own files and never resurrects one they deleted on purpose.
//! Agent definitions are meant to be edited — a TUI change upstream should be fixable
//! by the user in a text editor, without waiting for a spwn release.

use std::path::{Path, PathBuf};

/// `(filename, body)` for each bundled definition.
const DEFAULT_AGENTS: &[(&str, &str)] = &[
    ("claude.toml", include_str!("../../assets/agents/claude.toml")),
    ("codex.toml", include_str!("../../assets/agents/codex.toml")),
    ("gemini.toml", include_str!("../../assets/agents/gemini.toml")),
];

/// The bundled definitions, for the registry to load as the built-in scope. Kept
/// alongside the installer so there is exactly one list.
pub fn bundled() -> &'static [(&'static str, &'static str)] {
    DEFAULT_AGENTS
}

/// `~/.spwn/agents`.
pub fn global_agents_dir() -> Option<PathBuf> {
    directories::BaseDirs::new().map(|b| b.home_dir().join(".spwn").join("agents"))
}

/// Install the bundled definitions. Called once on startup; best-effort.
pub fn install_default_agents() {
    if let Some(dir) = global_agents_dir() {
        install_defaults_into(&dir, env!("CARGO_PKG_VERSION"));
    }
}

/// Core of [`install_default_agents`], parameterized so it's testable without
/// touching the real `~/.spwn/agents`.
///
///   - Fresh install (no marker): write every default.
///   - Version change: refresh only the defaults still present on disk, leaving
///     user-deleted ones deleted.
///   - Same version: nothing to do.
///
/// A user's own `*.toml` in this directory is never touched.
fn install_defaults_into(dir: &Path, current: &str) {
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let marker = dir.join(".spwn-version");
    let prev = std::fs::read_to_string(&marker).ok().map(|s| s.trim().to_string());
    let refresh_existing = match prev.as_deref() {
        Some(v) if v == current => return, // already installed for this version
        Some(_) => true,                   // version change
        None => false,                     // fresh install
    };
    for (name, body) in DEFAULT_AGENTS {
        let path = dir.join(name);
        let exists = path.exists();
        if (refresh_existing && exists) || (!refresh_existing && !exists) {
            let _ = std::fs::write(&path, body);
        }
    }
    let _ = std::fs::write(&marker, current);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".toml"))
            .collect();
        v.sort();
        v
    }

    #[test]
    fn fresh_install_writes_every_default() {
        let tmp = tempfile::tempdir().unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        assert_eq!(names(tmp.path()), vec!["claude.toml", "codex.toml", "gemini.toml"]);
        assert_eq!(
            std::fs::read_to_string(tmp.path().join(".spwn-version")).unwrap(),
            "1.0.0"
        );
    }

    #[test]
    fn same_version_leaves_user_edits_alone() {
        let tmp = tempfile::tempdir().unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        std::fs::write(tmp.path().join("claude.toml"), "# my edit").unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        // Re-running at the same version must not stomp the edit — otherwise every
        // restart would silently discard the user's customizations.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("claude.toml")).unwrap(),
            "# my edit"
        );
    }

    #[test]
    fn version_bump_refreshes_files_that_still_exist() {
        let tmp = tempfile::tempdir().unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        std::fs::write(tmp.path().join("claude.toml"), "# stale").unwrap();
        install_defaults_into(tmp.path(), "1.1.0");
        let body = std::fs::read_to_string(tmp.path().join("claude.toml")).unwrap();
        assert!(body.contains("id     = \"claude\""), "expected the bundled body back");
    }

    #[test]
    fn version_bump_does_not_resurrect_a_deleted_default() {
        let tmp = tempfile::tempdir().unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        std::fs::remove_file(tmp.path().join("gemini.toml")).unwrap();
        install_defaults_into(tmp.path(), "1.1.0");
        // Deleting a bundled agent is a deliberate act ("I don't want this in my
        // picker"). An upgrade that brings it back would be infuriating.
        assert!(!tmp.path().join("gemini.toml").exists());
        assert_eq!(names(tmp.path()), vec!["claude.toml", "codex.toml"]);
    }

    #[test]
    fn user_added_definitions_are_never_touched() {
        let tmp = tempfile::tempdir().unwrap();
        install_defaults_into(tmp.path(), "1.0.0");
        std::fs::write(tmp.path().join("mine.toml"), "# custom agent").unwrap();
        install_defaults_into(tmp.path(), "2.0.0");
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("mine.toml")).unwrap(),
            "# custom agent"
        );
    }
}

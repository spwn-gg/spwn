//! GitHub HTTPS auth for private repos: a personal access token the user pastes in
//! Settings, handed to git through a credential helper.
//!
//! The token lives in its own file (`<app data>/github-token`, mode 0600) rather than
//! in settings.json, so `get_settings` never sends it back to the browser. The helper
//! is injected as environment config (`GIT_CONFIG_COUNT` / `GIT_CONFIG_KEY_n` /
//! `GIT_CONFIG_VALUE_n`) instead of being written to `~/.gitconfig`: nothing of the
//! user's is touched, yet every git spwn starts — its own clone/fetch/pull/push, hooks,
//! and the shells and agents in its panes — picks it up. The helper reads the file on
//! each use, so saving or removing the token applies to already-running panes too.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static TOKEN_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Scoped to github.com so the token is never offered to any other host.
const HELPER_KEY: &str = "credential.https://github.com.helper";

fn token_path(app_data_dir: &Path) -> PathBuf {
    app_data_dir.join("github-token")
}

/// Remember where the token lives and export the helper config into this process's
/// environment, so every git child spwn spawns inherits it. Call once at startup.
pub fn install(app_data_dir: &Path) {
    let path = token_path(app_data_dir);
    for (k, v) in config_env(|k| std::env::var(k).ok(), &path) {
        std::env::set_var(k, v);
    }
    let _ = TOKEN_PATH.set(path);
}

/// `KEY=VALUE` entries carrying this process's `GIT_CONFIG_*` into a pane. Panes are
/// spawned by the long-lived rmux daemon, which may predate this process, so what it
/// inherited can't be relied on.
pub fn pane_env() -> Vec<String> {
    let Some(n) = std::env::var("GIT_CONFIG_COUNT").ok().and_then(|c| c.trim().parse::<usize>().ok())
    else {
        return Vec::new();
    };
    let mut env = vec![format!("GIT_CONFIG_COUNT={n}")];
    for i in 0..n {
        for k in [format!("GIT_CONFIG_KEY_{i}"), format!("GIT_CONFIG_VALUE_{i}")] {
            if let Ok(v) = std::env::var(&k) {
                env.push(format!("{k}={v}"));
            }
        }
    }
    env
}

/// Whether a token is saved.
pub fn has_token() -> bool {
    TOKEN_PATH
        .get()
        .and_then(|p| std::fs::read_to_string(p).ok())
        .is_some_and(|s| !s.trim().is_empty())
}

/// Save `token`, or remove the saved one when it's blank.
pub fn set_token(token: &str) -> Result<(), String> {
    let path = TOKEN_PATH.get().ok_or("could not resolve the app data dir")?;
    let token = token.trim();
    if token.is_empty() {
        return match std::fs::remove_file(path) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e.to_string()),
            _ => Ok(()),
        };
    }
    if token.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return Err("that doesn't look like a GitHub token (it contains spaces)".to_string());
    }
    write_private(path, token).map_err(|e| format!("couldn't save the token: {e}"))
}

fn write_private(path: &Path, contents: &str) -> std::io::Result<()> {
    use std::io::Write;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        opts.mode(0o600);
        let f = opts.open(path)?;
        // `mode` only applies on create; tighten a pre-existing file before writing.
        f.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        return (&f).write_all(contents.as_bytes());
    }
    #[allow(unreachable_code)]
    opts.open(path)?.write_all(contents.as_bytes())
}

/// The `GIT_CONFIG_*` variables that append the helper after any config already
/// passed that way. Empty when the helper is already there (spwn started from inside
/// one of its own panes).
fn config_env(get: impl Fn(&str) -> Option<String>, token_path: &Path) -> Vec<(String, String)> {
    let n: usize = get("GIT_CONFIG_COUNT").and_then(|c| c.trim().parse().ok()).unwrap_or(0);
    let value = helper(token_path);
    let present = (0..n).any(|i| {
        get(&format!("GIT_CONFIG_KEY_{i}")).as_deref() == Some(HELPER_KEY)
            && get(&format!("GIT_CONFIG_VALUE_{i}")).as_deref() == Some(value.as_str())
    });
    if present {
        return Vec::new();
    }
    vec![
        (format!("GIT_CONFIG_KEY_{n}"), HELPER_KEY.to_string()),
        (format!("GIT_CONFIG_VALUE_{n}"), value),
        ("GIT_CONFIG_COUNT".to_string(), (n + 1).to_string()),
    ]
}

/// A credential helper (git runs a `!` helper through the shell) that answers `get`
/// with the saved token and stays silent without one, so git falls through to the
/// user's own helpers or fails exactly as it would have.
fn helper(token_path: &Path) -> String {
    format!(
        "!f() {{ test \"$1\" = get || return 0; t=$(cat {} 2>/dev/null); \
         test -n \"$t\" && printf 'username=x-access-token\\npassword=%s\\n' \"$t\"; return 0; }}; f",
        shell_words::quote(&token_path.to_string_lossy())
    )
}

/// A next step to append to git's error when it failed for want of GitHub
/// credentials, or None when the failure is something else.
pub fn auth_hint(stderr: &str) -> Option<&'static str> {
    auth_hint_for(stderr, has_token())
}

fn auth_hint_for(stderr: &str, token_saved: bool) -> Option<&'static str> {
    const MARKERS: [&str; 4] = [
        "could not read Username",
        "Authentication failed",
        "Invalid username or",
        "Repository not found",
    ];
    if !stderr.contains("github.com") || !MARKERS.iter().any(|m| stderr.contains(m)) {
        return None;
    }
    Some(if token_saved {
        "GitHub rejected the saved token, or it can't see this repository. Check in \
         Settings → GitHub that it hasn't expired and has access to the repo."
    } else {
        "If this is a private repository, add a GitHub token in Settings → GitHub."
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Write;
    use std::process::{Command, Stdio};

    fn lookup(vars: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let map: HashMap<String, String> =
            vars.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| map.get(k).cloned()
    }

    #[test]
    fn appends_after_inherited_config_and_only_once() {
        let p = Path::new("/data/github-token");
        let env = config_env(lookup(&[("GIT_CONFIG_COUNT", "2")]), p);
        assert_eq!(env[0], ("GIT_CONFIG_KEY_2".into(), HELPER_KEY.into()));
        assert_eq!(env[2], ("GIT_CONFIG_COUNT".into(), "3".into()));

        let fresh = config_env(lookup(&[]), p);
        let vars: Vec<(&str, &str)> = fresh.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        assert!(config_env(lookup(&vars), p).is_empty());
    }

    #[test]
    fn hints_only_at_github_auth_failures() {
        let no_user = "fatal: could not read Username for 'https://github.com': terminal prompts disabled";
        assert!(auth_hint_for(no_user, false).unwrap().contains("add a GitHub token"));
        assert!(auth_hint_for(no_user, true).unwrap().contains("rejected"));
        let not_found = "remote: Repository not found.\nfatal: repository 'https://github.com/o/r.git/' not found";
        assert!(auth_hint_for(not_found, false).is_some());
        assert!(auth_hint_for("fatal: could not read Username for 'https://gitlab.com'", false).is_none());
        assert!(auth_hint_for("error: failed to push some refs to 'https://github.com/o/r.git'", false).is_none());
    }

    /// Ask real git for github.com credentials with only the helper configured.
    fn credential_fill(token_path: &Path) -> (bool, String) {
        let home = tempfile::tempdir().unwrap();
        let mut cmd = Command::new("git");
        // Outside any repo, so neither repo config nor a broken `.git` link can interfere.
        cmd.args(["credential", "fill"])
            .current_dir(home.path())
            .env("HOME", home.path())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_ASKPASS")
            .env_remove("SSH_ASKPASS")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        for i in 0..8 {
            cmd.env_remove(format!("GIT_CONFIG_KEY_{i}")).env_remove(format!("GIT_CONFIG_VALUE_{i}"));
        }
        cmd.envs(config_env(lookup(&[]), token_path));
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"protocol=https\nhost=github.com\n\n").unwrap();
        let out = child.wait_with_output().unwrap();
        (out.status.success(), String::from_utf8_lossy(&out.stdout).into_owned())
    }

    #[test]
    fn git_gets_the_token_from_the_helper() {
        let tmp = tempfile::tempdir().unwrap();
        // A space in the path, like macOS's "Application Support".
        let path = tmp.path().join("app data").join("github-token");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();

        let (ok, _) = credential_fill(&path);
        assert!(!ok, "no token saved: git should find no credentials");

        write_private(&path, "ghp_example123").unwrap();
        let (ok, out) = credential_fill(&path);
        assert!(ok);
        assert!(out.contains("username=x-access-token"), "{out}");
        assert!(out.contains("password=ghp_example123"), "{out}");
    }
}

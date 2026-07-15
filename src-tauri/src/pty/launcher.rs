//! Locating the binaries terminals run: the user's shell, `claude`, and the rmux
//! daemon the SDK launches.

use std::path::PathBuf;

/// The user's login shell, falling back to zsh (macOS default).
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
}

/// Locate a `claude` binary: explicit `CLAUDE_BIN`, then `$PATH`, then known
/// install locations. GUI processes don't always inherit the shell `$PATH`, so we
/// probe the well-known install dirs as a fallback.
pub fn find_claude_bin() -> Option<PathBuf> {
    probe("CLAUDE_BIN", "claude", &[".local/bin/claude", ".claude/local/claude", ".npm-global/bin/claude"])
}

/// Locate the `rmux` daemon binary for the SDK to launch. Prefers an explicit
/// `RMUX_SDK_DAEMON_BINARY`, then `$PATH`, then known install locations. (When we
/// bundle rmux, startup sets `RMUX_SDK_DAEMON_BINARY` to the resource path.)
pub fn find_rmux_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RMUX_SDK_DAEMON_BINARY") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    for cand in ["/opt/homebrew/bin/rmux", "/usr/local/bin/rmux", "/usr/bin/rmux"] {
        let pb = PathBuf::from(cand);
        if pb.exists() {
            return Some(pb);
        }
    }
    which("rmux").or_else(|| {
        directories::BaseDirs::new()
            .map(|b| b.home_dir().join(".cargo/bin/rmux"))
            .filter(|p| p.exists())
    })
}

/// Directory holding VS Code's `code` CLI shim, if VS Code is installed. GUI
/// processes launch with a minimal `$PATH` that usually omits it, so we probe the
/// well-known bundle locations to make a bare `code …` diff template resolve.
pub fn find_code_dir() -> Option<PathBuf> {
    const CANDIDATES: &[&str] = &[
        "/Applications/Visual Studio Code.app/Contents/Resources/app/bin",
        "/Applications/Visual Studio Code - Insiders.app/Contents/Resources/app/bin",
    ];
    for dir in CANDIDATES {
        let pb = PathBuf::from(dir);
        if pb.join("code").exists() || pb.join("code-insiders").exists() {
            return Some(pb);
        }
    }
    None
}

fn probe(env_var: &str, name: &str, home_rel: &[&str]) -> Option<PathBuf> {
    if let Ok(p) = std::env::var(env_var) {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Some(p) = which(name) {
        return Some(p);
    }
    directories::BaseDirs::new().and_then(|b| {
        home_rel
            .iter()
            .map(|rel| b.home_dir().join(rel))
            .find(|p| p.exists())
    })
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

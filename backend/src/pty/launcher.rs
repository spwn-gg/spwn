//! Locating the binaries panes run: the user's shell, and the rmux daemon the SDK
//! launches. Agent binaries are resolved from their definitions (`agents::resolve_binary`).

use std::path::PathBuf;

/// The user's login shell, falling back to zsh (macOS default).
pub fn default_shell() -> String {
    std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string())
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

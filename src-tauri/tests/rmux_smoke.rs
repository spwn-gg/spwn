//! Headless verification of the rmux integration: connect to/start the daemon,
//! run a command in a session, and confirm its output streams back. Independent
//! of `claude`. Skips (passes) if the rmux daemon binary can't be located.

use rmux_sdk::{
    EnsureSession, EnsureSessionPolicy, PaneOutputChunk, PaneOutputStart, ProcessSpec, RmuxBuilder,
    SessionName, TerminalSizeSpec,
};
use std::path::PathBuf;
use std::time::{Duration, Instant};

fn locate_rmux() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RMUX_SDK_DAEMON_BINARY") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    if let Ok(out) = std::process::Command::new("sh")
        .arg("-c")
        .arg("command -v rmux")
        .output()
    {
        if out.status.success() {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if !s.is_empty() {
                return Some(PathBuf::from(s));
            }
        }
    }
    ["/opt/homebrew/bin/rmux", "/usr/local/bin/rmux", "/usr/bin/rmux"]
        .into_iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
}

#[tokio::test]
async fn rmux_streams_command_output() {
    let Some(rmux_bin) = locate_rmux() else {
        eprintln!("[rmux-smoke] rmux not found — skipping.");
        return;
    };
    std::env::set_var("RMUX_SDK_DAEMON_BINARY", &rmux_bin);
    eprintln!("[rmux-smoke] using rmux at {}", rmux_bin.display());

    let rmux = RmuxBuilder::new()
        .default_timeout(Duration::from_secs(20))
        .connect_or_start()
        .await
        .expect("connect to rmux daemon");

    let name = SessionName::new("cm-smoke").expect("valid name");
    let session = rmux
        .ensure_session(
            EnsureSession::named(name)
                .policy(EnsureSessionPolicy::CreateOrReuse)
                .detached(true)
                .size(TerminalSizeSpec::new(80, 24))
                .process(ProcessSpec::argv(vec![
                    "bash".to_string(),
                    "-lc".to_string(),
                    "printf RMUX_OK; sleep 1".to_string(),
                ])),
        )
        .await
        .expect("ensure session");

    let pane = session.pane(0, 0);
    // Oldest: replay from the start so we don't miss output that printed before
    // we subscribed.
    let mut stream = pane
        .output_stream_starting_at(PaneOutputStart::Oldest)
        .await
        .expect("output stream");

    let mut acc = String::new();
    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline && !acc.contains("RMUX_OK") {
        match tokio::time::timeout(Duration::from_secs(2), stream.next()).await {
            Ok(Ok(Some(PaneOutputChunk::Bytes { bytes, .. }))) => {
                acc.push_str(&String::from_utf8_lossy(&bytes));
            }
            Ok(Ok(Some(_))) => {}
            Ok(Ok(None)) | Ok(Err(_)) => break,
            Err(_) => {} // per-read timeout; keep waiting until deadline
        }
    }

    let _ = session.kill().await;
    assert!(acc.contains("RMUX_OK"), "expected RMUX_OK in output, got: {acc:?}");
}

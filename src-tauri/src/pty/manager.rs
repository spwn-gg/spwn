//! Run `claude` inside an rmux session and stream its output to the frontend.
//!
//! Each session runs under the rmux daemon (persistent — survives app restart),
//! exposes a live byte stream via `output_stream`, and structured snapshots for
//! the rewind/branch automation. Output is emitted on `pty://output/<id>`
//! (base64-encoded raw bytes); exit on `pty://exit/<id>`.

use base64::Engine;
use rmux_sdk::{
    EnsureSession, EnsureSessionPolicy, Pane, PaneOutputChunk, PaneOutputStart, ProcessSpec, Rmux,
    SessionName, TerminalSizeSpec,
};
use std::path::Path;
use tauri::{async_runtime, AppHandle, Emitter};

/// A live rmux-backed session: the pane (input, resize) and the output-forwarding
/// task (aborted on detach). The session is killed by name when deleted, so we
/// don't retain the Session handle here.
pub struct RmuxSession {
    pub pane: Pane,
    pub output_task: async_runtime::JoinHandle<()>,
}

/// Create (or reattach to) an rmux session named `session_name` running `argv` in
/// `cwd`, and start forwarding its output to `pty://output/<id>`.
///
/// `CreateOrReuse` means a still-alive session (e.g. after an app restart) is
/// reattached with its process intact; a missing one is created fresh from `argv`.
pub async fn spawn_rmux_session(
    rmux: &Rmux,
    app: AppHandle,
    id: &str,
    session_name: &str,
    argv: Vec<String>,
    cwd: &Path,
    cols: u16,
    rows: u16,
) -> anyhow::Result<RmuxSession> {
    let name = SessionName::new(session_name.to_string())
        .map_err(|e| anyhow::anyhow!("invalid session name: {e}"))?;

    let session = rmux
        .ensure_session(
            EnsureSession::named(name)
                .policy(EnsureSessionPolicy::CreateOrReuse)
                .detached(true)
                .size(TerminalSizeSpec::new(cols.max(1), rows.max(1)))
                .working_directory(cwd.to_string_lossy().into_owned())
                .process(ProcessSpec::argv(argv))
                .environment(vec!["TERM=xterm-256color".to_string()]),
        )
        .await?;

    let pane = session.pane(0, 0);

    let out_event = format!("pty://output/{id}");
    let exit_event = format!("pty://exit/{id}");
    let pane_out = pane.clone();
    let output_task = async_runtime::spawn(async move {
        let engine = base64::engine::general_purpose::STANDARD;
        match pane_out.output_stream_starting_at(PaneOutputStart::Now).await {
            Ok(mut stream) => loop {
                match stream.next().await {
                    Ok(Some(PaneOutputChunk::Bytes { bytes, .. })) => {
                        if app.emit(&out_event, engine.encode(&bytes)).is_err() {
                            break;
                        }
                    }
                    Ok(Some(PaneOutputChunk::Lag(notice))) => {
                        // After a lag, replay the recent buffer so the terminal
                        // re-syncs rather than dropping content silently.
                        let _ = app.emit(&out_event, engine.encode(&notice.recent.bytes));
                    }
                    // PaneOutputChunk is non-exhaustive; ignore future variants.
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            },
            Err(e) => {
                let _ = app.emit(&out_event, engine.encode(format!("\r\n[rmux output error: {e}]").as_bytes()));
            }
        }
        let _ = app.emit(&exit_event, ());
    });

    Ok(RmuxSession { pane, output_task })
}

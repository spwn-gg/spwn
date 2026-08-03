//! Run a process inside an rmux pane and stream its output to the frontend.
//!
//! One code path backs both plain shells and agent TUIs — they differ only in argv
//! and in whether an [`AgentRuntime`] rides along. Each pane lives in the rmux daemon
//! (persistent — survives an app restart), exposes a live byte stream, and is
//! addressed by a stable name derived from the terminal id.
//!
//! Output is emitted on `pty://output/<id>` (base64-encoded raw bytes); exit on
//! `pty://exit/<id>`.

use crate::server::hub::EventHub;
use base64::Engine;
use parking_lot::Mutex;
use rmux_sdk::{
    EnsureSession, EnsureSessionPolicy, Pane, PaneOutputChunk, PaneOutputStart, ProcessSpec, Rmux,
    SessionName, TerminalSizeSpec,
};
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Wall-clock ms since the epoch. Used only for elapsed-time comparisons.
fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Tracks when a pane last produced output.
///
/// The status watcher needs to know "is this pane busy right now", and the obvious
/// implementation — a second `output_stream` subscription — would double every
/// pane's daemon traffic for information the forwarding task already has. Instead
/// the forwarder stamps this on every chunk and the watcher reads it.
// The clock is written here but only *read* by the status watcher, which lands in
// the next milestone. Recording activity from the start means the watcher can be
// added without touching this hot path again.
#[allow(dead_code)]
#[derive(Debug)]
pub struct PaneActivity {
    last_ms: AtomicU64,
    /// Bumped on every chunk, so a watcher can cheaply detect "something happened
    /// since I last looked" without polling the clock.
    seq: AtomicU64,
}

impl Default for PaneActivity {
    fn default() -> Self {
        Self {
            last_ms: AtomicU64::new(now_ms()),
            seq: AtomicU64::new(0),
        }
    }
}

#[allow(dead_code)] // read by the status watcher next milestone
impl PaneActivity {
    fn touch(&self) {
        self.last_ms.store(now_ms(), Ordering::Relaxed);
        self.seq.fetch_add(1, Ordering::Relaxed);
    }

    /// Milliseconds since the last byte arrived.
    pub fn quiet_ms(&self) -> u64 {
        now_ms().saturating_sub(self.last_ms.load(Ordering::Relaxed))
    }

    pub fn seq(&self) -> u64 {
        self.seq.load(Ordering::Relaxed)
    }
}

/// Agent-specific state attached to a pane. `None` for a plain shell.
// `session_id` and `activity` are consumed by the status/turn watchers next
// milestone; they're recorded now so the pane layer doesn't need revisiting.
#[allow(dead_code)]
#[derive(Debug)]
pub struct AgentRuntime {
    /// Which [`crate::agents::AgentDef`] drives this pane.
    pub agent_id: String,
    /// The agent's session id. Known up front for `idStrategy = "assign"`.
    pub session_id: Option<String>,
    /// Last permission mode spwn set or observed.
    pub mode: Mutex<Option<String>>,
}

/// A live rmux-backed pane: the handle (input, resize), the output-forwarding task
/// (aborted on detach), and the activity clock.
///
/// The rmux `Session` handle is deliberately not retained — panes are addressed by
/// name, so teardown re-resolves rather than holding state that can go stale.
#[allow(dead_code)]
pub struct PaneSession {
    pub pane: Pane,
    pub output_task: tokio::task::JoinHandle<()>,
    pub activity: Arc<PaneActivity>,
    pub agent: Option<AgentRuntime>,
}

impl PaneSession {
    pub fn agent_id(&self) -> Option<&str> {
        self.agent.as_ref().map(|a| a.agent_id.as_str())
    }
}

/// How to start a pane.
pub struct SpawnSpec<'a> {
    pub id: &'a str,
    pub session_name: &'a str,
    pub argv: Vec<String>,
    pub cwd: &'a Path,
    pub cols: u16,
    pub rows: u16,
    /// Extra `KEY=VALUE` environment, merged after the defaults.
    pub env: Vec<String>,
    pub agent: Option<AgentRuntime>,
}

/// Create (or reattach to) an rmux session named `session_name` running `argv` in
/// `cwd`, and start forwarding its output to `pty://output/<id>`.
///
/// `CreateOrReuse` means a still-alive session (e.g. after an app restart) is
/// reattached with its process intact; a missing one is created fresh from `argv`.
pub async fn spawn_pane(rmux: &Rmux, hub: EventHub, spec: SpawnSpec<'_>) -> anyhow::Result<PaneSession> {
    let name = SessionName::new(spec.session_name.to_string())
        .map_err(|e| anyhow::anyhow!("invalid session name: {e}"))?;

    let mut env = vec!["TERM=xterm-256color".to_string()];
    env.extend(spec.env);

    let session = rmux
        .ensure_session(
            EnsureSession::named(name)
                .policy(EnsureSessionPolicy::CreateOrReuse)
                .detached(true)
                .size(TerminalSizeSpec::new(spec.cols.max(1), spec.rows.max(1)))
                .working_directory(spec.cwd.to_string_lossy().into_owned())
                .process(ProcessSpec::argv(spec.argv))
                .environment(env),
        )
        .await?;

    let pane = session.pane(0, 0);
    let activity = Arc::new(PaneActivity::default());

    let out_event = format!("pty://output/{}", spec.id);
    let exit_event = format!("pty://exit/{}", spec.id);
    let engine = base64::engine::general_purpose::STANDARD;

    // Prime the client with the pane's CURRENT screen before streaming.
    //
    // The output stream starts at `Now`, so without this a reattached pane is blank
    // until the process next writes. For a shell at a prompt that's merely odd; for
    // a full-screen TUI — which is the primary surface now — it looks broken on
    // every browser reload and every tab switch, and an idle agent may not write
    // anything for minutes.
    //
    // `escape_ansi(true)` keeps SGR as real escape sequences, so the bytes can go
    // straight into xterm. (`escape_sequences(true)` octal-escapes them into
    // literal text, which renders as garbage — measured in the M0 spike.)
    match pane.capture_pane().escape_ansi(true).await {
        Ok(cap) if !cap.stdout.is_empty() => {
            hub.emit(&out_event, engine.encode(&cap.stdout));
        }
        // Capture unsupported or empty: ask the TUI to repaint instead. Harmless at
        // a shell prompt (redraws the line), and the alternative is a blank pane.
        _ => {
            let _ = pane.send_key("C-l".to_string()).await;
        }
    }

    let pane_out = pane.clone();
    let activity_out = Arc::clone(&activity);
    let output_task = tokio::spawn(async move {
        match pane_out.output_stream_starting_at(PaneOutputStart::Now).await {
            Ok(mut stream) => loop {
                match stream.next().await {
                    Ok(Some(PaneOutputChunk::Bytes { bytes, .. })) => {
                        activity_out.touch();
                        hub.emit(&out_event, engine.encode(&bytes));
                    }
                    Ok(Some(PaneOutputChunk::Lag(notice))) => {
                        // After a lag, replay the recent buffer so the terminal
                        // re-syncs rather than dropping content silently.
                        activity_out.touch();
                        hub.emit(&out_event, engine.encode(&notice.recent.bytes));
                    }
                    // PaneOutputChunk is non-exhaustive; ignore future variants.
                    Ok(Some(_)) => {}
                    Ok(None) | Err(_) => break,
                }
            },
            Err(e) => {
                hub.emit(
                    &out_event,
                    engine.encode(format!("\r\n[rmux output error: {e}]").as_bytes()),
                );
            }
        }
        hub.emit(&exit_event, ());
    });

    Ok(PaneSession {
        pane,
        output_task,
        activity,
        agent: spec.agent,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use rmux_sdk::RmuxBuilder;
    use std::time::Duration;

    /// Locate the rmux daemon, or `None` to skip (same precedence as `launcher`).
    fn rmux_available() -> bool {
        if std::env::var("RMUX_SDK_DAEMON_BINARY").is_ok() {
            return true;
        }
        if let Some(p) = crate::pty::find_rmux_bin() {
            std::env::set_var("RMUX_SDK_DAEMON_BINARY", p);
            return true;
        }
        false
    }

    /// Reattaching to a live pane must immediately paint its current screen.
    ///
    /// The output stream starts at `Now`, so without the capture-and-prime step a
    /// reattached pane stays blank until the process next writes — which for an idle
    /// full-screen TUI can be forever. This is the regression that would make
    /// terminal-first mode look broken on every browser reload, and it is invisible
    /// in any test that only checks *live* output.
    #[tokio::test(flavor = "multi_thread")]
    async fn reattaching_primes_the_client_with_the_current_screen() {
        if !rmux_available() {
            eprintln!("[pane] rmux not found — skipping.");
            return;
        }
        let rmux = match RmuxBuilder::new()
            .default_timeout(Duration::from_secs(20))
            .connect_or_start()
            .await
        {
            Ok(r) => r,
            Err(e) => {
                eprintln!("[pane] could not reach the rmux daemon ({e}) — skipping.");
                return;
            }
        };

        let name = "cmtest-prime";
        let hub = EventHub::default();
        let cwd = std::env::temp_dir();

        // First attach: print a marker, then idle so the pane stays alive but silent.
        let first = spawn_pane(
            &rmux,
            hub.clone(),
            SpawnSpec {
                id: "prime1",
                session_name: name,
                argv: vec![
                    "bash".into(),
                    "-lc".into(),
                    "printf 'PANE_MARKER\\n'; sleep 30".into(),
                ],
                cwd: &cwd,
                cols: 80,
                rows: 24,
                env: Vec::new(),
                agent: None,
            },
        )
        .await
        .expect("spawn");

        // Let the marker render and the pane fall silent.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        first.output_task.abort();

        // Second attach = what a browser reload does. Subscribe BEFORE reattaching,
        // then assert the very first frames already contain the marker — i.e. it came
        // from the capture, not from new output (the process is sleeping and writes
        // nothing more).
        let mut rx = hub.subscribe();
        let second = spawn_pane(
            &rmux,
            hub.clone(),
            SpawnSpec {
                id: "prime2",
                session_name: name,
                argv: vec!["bash".into(), "-lc".into(), "sleep 30".into()],
                cwd: &cwd,
                cols: 80,
                rows: 24,
                env: Vec::new(),
                agent: None,
            },
        )
        .await
        .expect("reattach");

        let mut seen = String::new();
        let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
        while tokio::time::Instant::now() < deadline && !seen.contains("PANE_MARKER") {
            match tokio::time::timeout(Duration::from_secs(1), rx.recv()).await {
                Ok(Ok(frame)) => {
                    if let Ok(v) = serde_json::from_str::<serde_json::Value>(&frame) {
                        if v.get("topic").and_then(|t| t.as_str()) == Some("pty://output/prime2") {
                            if let Some(b64) = v.get("payload").and_then(|p| p.as_str()) {
                                if let Ok(bytes) = base64::engine::general_purpose::STANDARD
                                    .decode(b64)
                                {
                                    seen.push_str(&String::from_utf8_lossy(&bytes));
                                }
                            }
                        }
                    }
                }
                _ => break,
            }
        }

        second.output_task.abort();
        let _ = EnsureSession::named(SessionName::new(name.to_string()).unwrap())
            .policy(EnsureSessionPolicy::ReuseOnly)
            .ensure(&rmux)
            .await
            .map(|s| async move { s.kill().await });

        assert!(
            seen.contains("PANE_MARKER"),
            "reattach did not replay the current screen; a reattached TUI would be \
             blank until its next write. Got {} bytes: {seen:?}",
            seen.len()
        );
    }
}

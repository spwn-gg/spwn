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

/// Rewrite bare `\n` as `\r\n`.
///
/// `capture-pane` separates rows with a plain newline, which is fine for a file but
/// wrong for a terminal: with the pty in raw mode, LF moves the cursor DOWN without
/// returning it to column 0. Writing a capture verbatim therefore renders a
/// staircase — every row starting where the previous row ended, text broken across
/// the screen at increasing offsets. It reads convincingly like a width mismatch,
/// which is what makes it worth naming here.
fn crlf(bytes: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(bytes.len() + bytes.len() / 40);
    let mut prev = 0u8;
    for &b in bytes {
        if b == b'\n' && prev != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        prev = b;
    }
    out
}

/// Emit the pane's CURRENT screen on `pty://output/<id>` so a freshly-attached
/// client paints immediately.
///
/// The output stream starts at `Now`, so without this a reattached pane stays blank
/// until the process next writes — which for an idle full-screen TUI can be forever.
///
/// This is a SEPARATE call rather than part of `spawn_pane` on purpose. Priming
/// inside the spawn broadcasts before `open_terminal` has even returned, so the
/// client — which can only subscribe once it knows the terminal id — is guaranteed
/// to miss it. The frame goes out to a topic with no listeners and the pane renders
/// blank anyway. The client must subscribe first, then ask for the repaint.
///
/// `escape_ansi(true)` keeps SGR as real escape sequences, so the bytes can be
/// written straight into xterm. (`escape_sequences(true)` octal-escapes them into
/// literal text, which renders as visible garbage — measured in the M0 spike.)
pub async fn prime_pane(pane: &Pane, hub: &EventHub, id: &str, cols: u16, rows: u16) {
    let engine = base64::engine::general_purpose::STANDARD;
    let out_event = format!("pty://output/{id}");

    // Match the pane to the client's geometry FIRST.
    //
    // A capture is a grid of the pane's current width. Written into an xterm of a
    // different width it re-wraps, and the result is visibly scrambled — text broken
    // mid-word across columns, the composer offset. `EnsureSession`'s size applies
    // when a session is created, not when an existing one is reused, so a reattach
    // from a differently-sized window lands mismatched unless we resize here.
    //
    // The resize is also worth more than it looks: if the size really changed, the
    // TUI redraws itself at the new geometry, which is a fresher and more correct
    // repaint than any capture.
    let _ = pane
        .resize(TerminalSizeSpec::new(cols.max(1), rows.max(1)))
        .await;
    tokio::time::sleep(std::time::Duration::from_millis(120)).await;

    match pane.capture_pane().escape_ansi(true).await {
        Ok(cap) if !cap.stdout.is_empty() => {
            // Clear and home before painting: the client's xterm may hold stale
            // content (or a stale cursor position) from a previous attach, and the
            // capture is a full-screen image, not a delta.
            let mut out = b"\x1b[2J\x1b[H".to_vec();
            out.extend_from_slice(&crlf(&cap.stdout));
            hub.emit(&out_event, engine.encode(&out));
        }
        // Capture unsupported or empty: ask the program to repaint instead.
        // Harmless at a shell prompt (redraws the line), and the alternative is a
        // blank pane.
        _ => {
            let _ = pane.send_key("C-l".to_string()).await;
        }
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
    /// Run the pane's command through this prefix instead of directly — the argv of
    /// a hook-provided environment (e.g. `docker exec -it … <container>`), already
    /// split. None → run on the host, unchanged. See [`wrap_argv`].
    pub exec_prefix: Option<Vec<String>>,
}

/// Build the argv actually handed to rmux: the environment's prefix, then the
/// environment as an explicit `env` invocation, then the original command.
///
/// The `env` indirection is load-bearing. rmux applies `EnsureSession::environment`
/// to the process it spawns — which, once a prefix is in play, is the *wrapper*
/// (the `docker` client), not the agent inside. Passing the variables as arguments
/// to `env(1)` carries them across the boundary without spwn needing to know what
/// the boundary is or which flag that particular wrapper spells it with.
///
/// `env K= cmd` sets an empty value rather than unsetting, which is exactly what
/// `claude.toml`'s `CLAUDE_CODE_CHILD_SESSION = ""` requires to keep transcript
/// saving on.
/// Split a hook-reported exec prefix into argv, the way a shell would.
///
/// Returns None for an empty or unparseable prefix (an unbalanced quote, say). The
/// caller treats that as "no environment" and runs on the host: a session that
/// quietly runs uncontained is recoverable, whereas one spawned with shredded argv
/// is a pane that dies with no diagnosis.
pub fn split_exec_prefix(prefix: &str) -> Option<Vec<String>> {
    let parts = shell_words::split(prefix).ok()?;
    (!parts.is_empty()).then_some(parts)
}

fn wrap_argv(prefix: Option<Vec<String>>, env: &[String], argv: Vec<String>) -> Vec<String> {
    match prefix {
        None => argv,
        Some(mut v) => {
            if !env.is_empty() {
                v.push("env".to_string());
                v.extend(env.iter().cloned());
            }
            v.extend(argv);
            v
        }
    }
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

    // `working_directory` stays the HOST path either way: it's the cwd of whatever
    // rmux spawns, which under a prefix is the wrapper process. The cwd *inside* the
    // environment is the prefix's business (a `-w` flag, say).
    let argv = wrap_argv(spec.exec_prefix, &env, spec.argv);

    let session = rmux
        .ensure_session(
            EnsureSession::named(name)
                .policy(EnsureSessionPolicy::CreateOrReuse)
                .detached(true)
                .size(TerminalSizeSpec::new(spec.cols.max(1), spec.rows.max(1)))
                .working_directory(spec.cwd.to_string_lossy().into_owned())
                .process(ProcessSpec::argv(argv))
                .environment(env),
        )
        .await?;

    let pane = session.pane(0, 0);
    let activity = Arc::new(PaneActivity::default());

    let out_event = format!("pty://output/{}", spec.id);
    let exit_event = format!("pty://exit/{}", spec.id);
    let engine = base64::engine::general_purpose::STANDARD;

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

    #[test]
    fn no_prefix_leaves_argv_untouched() {
        // The uncontained path is the overwhelming majority of sessions; it must be
        // byte-identical to what spwn ran before environments existed.
        let argv = vec!["claude".to_string(), "--session-id".to_string()];
        assert_eq!(
            wrap_argv(None, &["TERM=xterm-256color".to_string()], argv.clone()),
            argv
        );
    }

    #[test]
    fn prefix_wraps_command_and_carries_env_across_the_boundary() {
        let out = wrap_argv(
            Some(vec!["docker".into(), "exec".into(), "-it".into(), "box".into()]),
            &["TERM=xterm-256color".to_string(), "K=v".to_string()],
            vec!["claude".into(), "--session-id".into(), "abc".into()],
        );
        assert_eq!(
            out,
            vec![
                "docker",
                "exec",
                "-it",
                "box",
                "env",
                "TERM=xterm-256color",
                "K=v",
                "claude",
                "--session-id",
                "abc"
            ]
        );
    }

    #[test]
    fn empty_env_omits_the_env_word() {
        // A shell pane carries no [env] block. Emitting a bare `env` would still work,
        // but only on images that have it — don't require one for nothing.
        let out = wrap_argv(Some(vec!["ssh".into(), "host".into()]), &[], vec!["/bin/sh".into()]);
        assert_eq!(out, vec!["ssh", "host", "/bin/sh"]);
    }

    #[test]
    fn an_empty_env_value_stays_set_rather_than_dropped() {
        // claude.toml's CLAUDE_CODE_CHILD_SESSION = "" must arrive SET AND EMPTY:
        // unsetting it turns transcript saving off, which silently kills session
        // binding, the Timeline and rewind.
        let out = wrap_argv(
            Some(vec!["docker".into(), "exec".into(), "box".into()]),
            &["CLAUDE_CODE_CHILD_SESSION=".to_string()],
            vec!["claude".into()],
        );
        assert!(out.contains(&"CLAUDE_CODE_CHILD_SESSION=".to_string()));
    }

    #[test]
    fn a_prefix_path_with_spaces_survives_as_one_argument() {
        let split = split_exec_prefix("docker exec -w '/Users/me/My Projects/wt' box").unwrap();
        assert_eq!(
            split,
            vec!["docker", "exec", "-w", "/Users/me/My Projects/wt", "box"]
        );
    }

    #[test]
    fn an_unusable_prefix_is_rejected_rather_than_shredded() {
        assert!(split_exec_prefix("").is_none());
        assert!(split_exec_prefix("   ").is_none());
        // Unbalanced quote: splitting this would hand rmux nonsense argv.
        assert!(split_exec_prefix("docker exec -w 'unterminated").is_none());
    }

    #[test]
    fn crlf_fixes_bare_newlines_without_doubling_existing_ones() {
        assert_eq!(crlf(b"a\nb"), b"a\r\nb".to_vec());
        // Already-correct line endings must not become \r\r\n, which renders as a
        // blank line between every row.
        assert_eq!(crlf(b"a\r\nb"), b"a\r\nb".to_vec());
        assert_eq!(crlf(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n".to_vec());
        assert_eq!(crlf(b"no newlines"), b"no newlines".to_vec());
    }

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
                exec_prefix: None,
            },
        )
        .await
        .expect("spawn");

        // Let the marker render and the pane fall silent.
        tokio::time::sleep(Duration::from_millis(1500)).await;
        first.output_task.abort();

        // Second attach = what reopening the session does.
        //
        // The ordering here mirrors the real client EXACTLY, and that matters: the
        // browser cannot subscribe until `open_terminal` has returned it a terminal
        // id, so anything emitted during the spawn is missed. An earlier version of
        // this test subscribed before spawning and passed while the UI stayed blank.
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
                exec_prefix: None,
            },
        )
        .await
        .expect("reattach");

        // ...only now can a client subscribe, and only then ask for the repaint.
        let mut rx = hub.subscribe();
        prime_pane(&second.pane, &hub, "prime2", 80, 24).await;

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

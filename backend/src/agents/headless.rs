//! Non-interactive agent runs, for scheduled tasks.
//!
//! A headless run is an ordinary rmux pane running the agent's `argv.headless`
//! command line. That is a deliberate choice over piping a child process:
//!
//!   * the run is **attachable** — you can open the session and watch it work,
//!     instead of waiting for a black box to report back;
//!   * it **survives a backend restart**, like every other pane;
//!   * it feeds the same status and turn machinery as an interactive session, so
//!     the sidebar, per-turn commits and checkpoints all work with no special case;
//!   * `ProcessSpec::argv` never goes through a shell, so a 40 KB assembled context
//!     prompt is passed as one argument with no quoting.

use crate::agents::AgentDef;
use crate::pty::{spawn_pane, SpawnSpec};
use crate::state::AppState;
use crate::store::rmux_session_name;
use rmux_sdk::Rmux;
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

/// How a headless run ended.
pub enum Outcome {
    /// The agent reported a successful result.
    Ok,
    /// The agent reported a failure, or exited without reporting one.
    Failed(String),
}

/// Launch a headless run and observe it to completion.
///
/// `on_done` fires exactly once. The caller owns finalization (flagging the
/// session, clearing the run guard, notifying the UI).
#[allow(clippy::too_many_arguments)]
pub async fn run(
    state: Arc<AppState>,
    rmux: &Rmux,
    def: &AgentDef,
    terminal_id: String,
    session_id: String,
    cwd: &Path,
    prompt: String,
    on_done: impl FnOnce(Outcome) + Send + 'static,
) -> Result<(), String> {
    let template = def
        .argv
        .headless
        .as_ref()
        .ok_or_else(|| format!("{} cannot run non-interactively", def.name))?;

    // A scheduled run uses the session's environment only if its hook reported a
    // headless-specific prefix. This is not an oversight: the interactive prefix
    // allocates a tty (a TUI needs one), but this path parses `--output-format
    // stream-json` line by line in `observe`, and a tty lets the agent interleave
    // spinner/progress bytes with that JSON — every line then fails to parse and the
    // run reports failure having actually succeeded. Running on the host is the
    // status quo and fails loudly if anything is missing, which is the better default.
    let exec = state
        .store
        .lock()
        .terminal(&terminal_id)
        .and_then(|t| t.exec.clone())
        .filter(|e| e.headless_prefix.is_some());

    let overrides = state.settings.lock().agent_paths.clone();
    let bin = match &exec {
        Some(e) => e.bin.clone().unwrap_or_else(|| def.binary.name.clone()),
        None => crate::agents::resolve_binary(def, &overrides)
            .ok_or_else(|| format!("{} binary not found", def.name))?
            .to_string_lossy()
            .into_owned(),
    };
    let exec_prefix = exec
        .as_ref()
        .and_then(|e| e.headless_prefix.as_deref())
        .and_then(crate::pty::split_exec_prefix);

    let ctx: BTreeMap<&str, String> = [
        ("bin", bin),
        ("sessionId", session_id.clone()),
        ("prompt", prompt),
        ("cwd", cwd.to_string_lossy().into_owned()),
    ]
    .into_iter()
    .collect();
    let argv = crate::agents::def::render_argv(template, &ctx);
    let env: Vec<String> = def.env.iter().map(|(k, v)| format!("{k}={v}")).collect();

    let session = spawn_pane(
        rmux,
        state.hub.clone(),
        SpawnSpec {
            id: &terminal_id,
            session_name: &rmux_session_name(&terminal_id),
            argv,
            cwd,
            cols: 120,
            rows: 40,
            env,
            agent: Some(crate::pty::AgentRuntime {
                agent_id: def.id.clone(),
                session_id: Some(session_id),
                mode: parking_lot::Mutex::new(None),
            }),
            exec_prefix,
        },
    )
    .await
    .map_err(|e| e.to_string())?;

    let pane = session.pane.clone();
    if let Some(prev) = state.sessions.lock().insert(terminal_id.clone(), session) {
        prev.output_task.abort();
    }

    // Observe the run on its own task so the caller isn't blocked for minutes.
    tokio::spawn(async move {
        let outcome = observe(&pane).await;
        on_done(outcome);
    });
    Ok(())
}

/// Read the run's output until it reports a result or the process exits.
///
/// `wait_exit` is the backstop, not the primary signal: an agent that dies without
/// printing a result would otherwise leave the task marked "running" forever, and
/// the scheduler would refuse to fire it again.
async fn observe(pane: &rmux_sdk::Pane) -> Outcome {
    let mut lines = match pane.line_stream().await {
        Ok(s) => s,
        Err(e) => return Outcome::Failed(format!("could not read the run's output: {e}")),
    };

    let mut last_error: Option<String> = None;
    loop {
        tokio::select! {
            next = lines.next() => match next {
                Ok(Some(rmux_sdk::PaneLineItem::Line { text })) => {
                    if let Some(o) = parse_result(&text) {
                        return o;
                    }
                    if text.contains("\"type\":\"error\"") || text.contains("Error:") {
                        last_error = Some(text.trim().to_string());
                    }
                }
                // A gap in the stream: the result line may have been in it, so keep
                // reading rather than declaring failure — wait_exit is the backstop.
                Ok(Some(_)) => {}
                Ok(None) | Err(_) => break,
            },
            exit = pane.wait_exit() => {
                let _ = exit;
                break;
            }
        }
    }
    Outcome::Failed(
        last_error.unwrap_or_else(|| "the run ended before reporting a result".to_string()),
    )
}

/// Recognise the agent's terminal "result" line in a stream-json output.
fn parse_result(line: &str) -> Option<Outcome> {
    let v: serde_json::Value = serde_json::from_str(line.trim()).ok()?;
    if v.get("type").and_then(|t| t.as_str()) != Some("result") {
        return None;
    }
    match v.get("subtype").and_then(|s| s.as_str()) {
        Some("success") => Some(Outcome::Ok),
        other => Some(Outcome::Failed(format!(
            "the run reported {}",
            other.unwrap_or("an error")
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ok(o: Option<Outcome>) -> Option<bool> {
        o.map(|x| matches!(x, Outcome::Ok))
    }

    #[test]
    fn recognises_a_successful_result() {
        assert_eq!(
            ok(parse_result(r#"{"type":"result","subtype":"success","session_id":"x"}"#)),
            Some(true)
        );
    }

    #[test]
    fn a_non_success_subtype_is_a_failure_not_a_success() {
        assert_eq!(
            ok(parse_result(r#"{"type":"result","subtype":"error_max_turns"}"#)),
            Some(false)
        );
        // A result with no subtype at all must not be optimistically treated as ok.
        assert_eq!(ok(parse_result(r#"{"type":"result"}"#)), Some(false));
    }

    #[test]
    fn ordinary_stream_lines_are_not_results() {
        for line in [
            r#"{"type":"assistant","message":{"content":[]}}"#,
            r#"{"type":"system","subtype":"init"}"#,
            "not json at all",
            "",
        ] {
            assert!(parse_result(line).is_none(), "{line:?} should not be a result");
        }
    }

    #[test]
    fn tolerates_surrounding_whitespace() {
        assert_eq!(
            ok(parse_result("  {\"type\":\"result\",\"subtype\":\"success\"}\r\n")),
            Some(true)
        );
    }
}

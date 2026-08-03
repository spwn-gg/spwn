//! Detecting turn boundaries and firing the `session-turn` hooks.
//!
//! Previously the FRONTEND drove this: `ClaudePane` saw the sidecar's `result`
//! event and called `hooks_run_turn`. That means a session with no open tab never
//! auto-commits and never checkpoints — so background work, scheduled runs, and any
//! session whose tab you closed silently accumulate uncommitted changes and have no
//! Timeline. Moving it to the backend fixes that as a side effect.
//!
//! The trigger is the transcript watcher that already exists (debounced, and already
//! emitting `projects://changed`), so this costs one extra file read per changed
//! session rather than a new polling loop.

use crate::state::AppState;
use crate::transcript;
use std::collections::HashMap;
use std::sync::Arc;

/// Remembers which turn was last fired per terminal, so a transcript that changes
/// several times while resting on the same turn fires once.
#[derive(Default)]
pub struct TurnTracker {
    last_fired: HashMap<String, String>,
}

impl TurnTracker {
    /// Returns true if this (terminal, turn) pair has not been fired yet, and
    /// records it.
    fn claim(&mut self, terminal_id: &str, turn_uuid: &str) -> bool {
        match self.last_fired.get(terminal_id) {
            Some(prev) if prev == turn_uuid => false,
            _ => {
                self.last_fired
                    .insert(terminal_id.to_string(), turn_uuid.to_string());
                true
            }
        }
    }

    pub fn forget(&mut self, terminal_id: &str) {
        self.last_fired.remove(terminal_id);
    }

    /// Seed the tracker without firing.
    ///
    /// Called when a session attaches, so re-opening a tab on an old conversation
    /// doesn't re-fire hooks for a turn that finished days ago — which would produce
    /// a spurious commit and checkpoint on every reattach.
    pub fn prime(&mut self, terminal_id: &str, turn_uuid: &str) {
        self.last_fired
            .insert(terminal_id.to_string(), turn_uuid.to_string());
    }
}

/// Handle a batch of changed session ids from the transcript watcher.
pub fn on_transcript_changed(state: &Arc<AppState>, session_ids: &[String]) {
    for session_id in session_ids {
        // Map the changed transcript back to the terminal that owns it.
        let terminal_id = {
            let store = state.store.lock();
            store
                .projects
                .iter()
                .flat_map(|p| p.terminals.iter())
                .find(|t| t.session_id.as_deref() == Some(session_id.as_str()))
                .map(|t| t.id.clone())
        };
        if let Some(terminal_id) = terminal_id {
            check(state, &terminal_id, session_id);
        }
    }
}

/// Re-check a terminal after its status settles.
///
/// TWO signals converge on the same check, and both are needed:
///
///   * The transcript watcher fires as soon as records land — which is usually
///     while the agent is still mid-response, so the turn isn't finished yet.
///   * The status watcher fires when the pane goes quiet — but by then the
///     transcript has long since stopped changing.
///
/// Relying on the transcript alone drops the turn permanently: the check runs too
/// early, declines, and nothing ever re-runs it because the file never changes
/// again. Measured live — status went `thinking` → `done` and no turn ever fired,
/// so the commit and checkpoint silently never happened.
pub fn on_status_settled(state: &Arc<AppState>, terminal_id: &str) {
    let session_id = {
        let store = state.store.lock();
        store.terminal(terminal_id).and_then(|t| t.session_id.clone())
    };
    if let Some(sid) = session_id {
        check(state, terminal_id, &sid);
    }
}

/// The shared turn check. Idempotent: `claim` makes double-firing impossible no
/// matter how many signals arrive.
fn check(state: &Arc<AppState>, terminal_id: &str, session_id: &str) {
    let Some(path) = crate::projects::locate_session(session_id) else {
        return;
    };
    let tail = transcript::tail_summary(&path);
    if !tail.turn_complete() {
        return;
    }
    let Some(uuid) = tail.last_uuid.clone() else {
        return;
    };

    // A turn complete on disk while the pane is still streaming is not finished —
    // more records are coming. Decline now; `on_status_settled` will re-check.
    if matches!(
        state.agent_status.lock().get(terminal_id),
        Some(crate::agents::SessionStatus::Thinking)
    ) {
        return;
    }

    if !state.turns.lock().claim(terminal_id, &uuid) {
        return;
    }

    // Off the caller's thread: `fire_hooks` is synchronous and shells out to git,
    // which can take seconds, and this runs from the fs-watcher callback.
    let state = Arc::clone(state);
    let tid = terminal_id.to_string();
    let sid = session_id.to_string();
    std::thread::spawn(move || {
        crate::commands::fire_turn_hooks(&state, &tid, &uuid);
        state.hub.emit(
            "agent://turn",
            serde_json::json!({
                "terminalId": tid,
                "sessionId": sid,
                "turnUuid": uuid,
            }),
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_turn_fires_once_even_if_the_transcript_changes_again() {
        let mut t = TurnTracker::default();
        assert!(t.claim("term", "turn-1"));
        // The transcript is touched repeatedly while resting on the same turn
        // (metadata records, title updates); each must not re-fire the commit hook.
        assert!(!t.claim("term", "turn-1"));
        assert!(!t.claim("term", "turn-1"));
        assert!(t.claim("term", "turn-2"));
    }

    #[test]
    fn priming_suppresses_the_turn_that_already_existed_on_attach() {
        // Reopening a tab on an old conversation must not commit and checkpoint a
        // turn that finished long ago.
        let mut t = TurnTracker::default();
        t.prime("term", "old-turn");
        assert!(!t.claim("term", "old-turn"));
        assert!(t.claim("term", "new-turn"));
    }

    #[test]
    fn terminals_are_tracked_independently() {
        let mut t = TurnTracker::default();
        assert!(t.claim("a", "turn-1"));
        assert!(t.claim("b", "turn-1"));
        assert!(!t.claim("a", "turn-1"));
    }

    #[test]
    fn forgetting_a_terminal_lets_its_turn_fire_again() {
        let mut t = TurnTracker::default();
        assert!(t.claim("a", "turn-1"));
        t.forget("a");
        assert!(t.claim("a", "turn-1"));
    }
}

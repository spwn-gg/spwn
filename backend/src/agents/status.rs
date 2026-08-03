//! Deriving a session's live status by reading the agent's screen.
//!
//! Two sources feed one status, because neither is sufficient alone:
//!
//!   * The **screen** is the only place a pending permission prompt exists. Nothing
//!     is written to the transcript until the user answers, so a transcript-only
//!     watcher is structurally blind to "blocked, waiting for you" — the single most
//!     valuable status spwn shows.
//!   * The **transcript** is authoritative for turn boundaries (see `turns.rs`) and
//!     never has to guess at rendering.
//!
//! So: screen scraping owns the "needs you" states, the transcript owns turns.
//!
//! Every pattern lives in the agent's TOML rather than here, because a TUI's wording
//! changes on its own schedule. Fixing spwn after such a change should be an edit to
//! a config file, not a release.

use crate::agents::def::{AgentDef, DetectRule, SessionStatus};
use crate::pty::PaneActivity;
use crate::state::AppState;
use regex::Regex;
use std::sync::Arc;
use std::time::Duration;

/// A rule's patterns, pre-compiled once per watcher rather than per snapshot.
struct CompiledRule {
    status: SessionStatus,
    any: Vec<String>,
    all: Vec<String>,
    regex: Option<Regex>,
    rows: Option<usize>,
}

fn compile(rules: &[DetectRule]) -> Vec<CompiledRule> {
    rules
        .iter()
        .map(|r| CompiledRule {
            status: r.status,
            any: r.any.clone(),
            all: r.all.clone(),
            // A bad pattern disables that arm rather than the whole rule — a typo in
            // a hand-edited regex shouldn't blind the watcher entirely.
            regex: r.regex.as_deref().and_then(|p| Regex::new(p).ok()),
            rows: r.rows.map(|w| w.last),
        })
        .collect()
}

/// Whitespace-normalize each rendered row, then join.
///
/// Matching happens on rendered text, so box-drawing characters occupy their own
/// cells and never interleave into words. Normalizing collapses the variable
/// padding a TUI uses to align columns, which would otherwise break a substring
/// that happens to span it.
fn window(lines: &[String], last: Option<usize>) -> String {
    let start = match last {
        Some(n) => lines.len().saturating_sub(n),
        None => 0,
    };
    lines[start..]
        .iter()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n")
}

fn matches(rule: &CompiledRule, lines: &[String]) -> bool {
    let text = window(lines, rule.rows);
    // `all` are required regardless of how the rule otherwise matched — that is what
    // lets a rule be specific without one long brittle pattern.
    if !rule.all.iter().all(|p| text.contains(p)) {
        return false;
    }
    let any_hit = rule.any.iter().any(|p| text.contains(p));
    let re_hit = rule.regex.as_ref().is_some_and(|r| r.is_match(&text));
    if rule.any.is_empty() && rule.regex.is_none() {
        // Only `all` was specified — treat it as the whole condition.
        return !rule.all.is_empty();
    }
    any_hit || re_hit
}

/// Evaluate the rules against a rendered screen. First match wins, so a definition
/// orders blocking states before "thinking".
fn classify(rules: &[CompiledRule], lines: &[String]) -> Option<SessionStatus> {
    rules.iter().find(|r| matches(r, lines)).map(|r| r.status)
}

/// Tracks the debounce state for one session.
struct Debounce {
    published: Option<SessionStatus>,
    candidate: Option<SessionStatus>,
    streak: u8,
}

impl Debounce {
    fn new() -> Self {
        Self {
            published: None,
            candidate: None,
            streak: 0,
        }
    }

    /// Feed an observation; returns a status to publish, if it changed.
    ///
    /// A `blocked*` must survive `confirm_frames` consecutive observations before
    /// being published. Half-drawn frames are common while a TUI repaints, and a
    /// spurious "waiting for you" dot is worse than a late one — it trains the user
    /// to ignore the signal.
    fn feed(&mut self, observed: SessionStatus, confirm_frames: u8) -> Option<SessionStatus> {
        let needs_confirm = matches!(
            observed,
            SessionStatus::BlockedPermission | SessionStatus::BlockedQuestion
        );
        if Some(observed) == self.candidate {
            self.streak = self.streak.saturating_add(1);
        } else {
            self.candidate = Some(observed);
            self.streak = 1;
        }
        let confirmed = !needs_confirm || self.streak >= confirm_frames.max(1);
        if confirmed && self.published != Some(observed) {
            self.published = Some(observed);
            return Some(observed);
        }
        None
    }
}

/// Watch one agent pane and publish its status.
///
/// Sampling is driven by the pane's activity clock rather than a fixed timer: a
/// quiet session costs nothing, and a streaming one is sampled a few times a second.
/// `render_stream`'s 16ms debounce would be far too chatty across a dozen sessions.
pub fn spawn_watcher(
    state: Arc<AppState>,
    terminal_id: String,
    def: AgentDef,
    pane: rmux_sdk::Pane,
    activity: Arc<PaneActivity>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let rules = compile(&def.detect.rules);
        let generic = rules.is_empty();
        let idle_after = def.detect.idle_after_ms.max(200);
        let confirm = def.detect.confirm_frames;
        let mut deb = Debounce::new();
        let mut last_seq = activity.seq();

        loop {
            // Cheap poll of the activity counter. The alternative — a second
            // output_stream subscription per pane — would double daemon traffic for
            // information the forwarding task already has.
            tokio::time::sleep(Duration::from_millis(250)).await;
            if !state.sessions.lock().contains_key(&terminal_id) {
                return; // pane closed
            }

            let seq = activity.seq();
            let moved = seq != last_seq;
            last_seq = seq;
            let quiet = activity.quiet_ms();

            // Nothing happened and we already settled — don't spend a snapshot.
            if !moved && matches!(deb.published, Some(SessionStatus::Done) | Some(SessionStatus::Idle))
            {
                continue;
            }

            let observed = if generic {
                // No rules: activity alone. Works for any binary with zero
                // configuration, but can never report "blocked" — which is honest,
                // since without patterns there is no way to know.
                if moved || quiet < idle_after {
                    SessionStatus::Thinking
                } else {
                    SessionStatus::Done
                }
            } else {
                let Ok(snap) = pane.snapshot().await else {
                    continue;
                };
                let lines = snap.visible_lines();
                match classify(&rules, &lines) {
                    Some(s) => s,
                    None if quiet >= idle_after => SessionStatus::Done,
                    None => continue, // in-between frame; wait rather than guess
                }
            };

            if let Some(publish) = deb.feed(observed, confirm) {
                crate::commands::emit_agent_status(&state, &terminal_id, publish);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::def::RowWindow;

    fn lines(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    fn rule(status: SessionStatus, any: &[&str], all: &[&str], re: Option<&str>, rows: Option<usize>) -> DetectRule {
        DetectRule {
            status,
            any: any.iter().map(|s| s.to_string()).collect(),
            all: all.iter().map(|s| s.to_string()).collect(),
            regex: re.map(str::to_string),
            rows: rows.map(|last| RowWindow { last }),
        }
    }

    /// The real claude.toml rules, so the tests exercise what actually ships.
    fn claude_rules() -> Vec<CompiledRule> {
        let def: AgentDef =
            toml::from_str(include_str!("../../assets/agents/claude.toml")).unwrap();
        compile(&def.detect.rules)
    }

    #[test]
    fn detects_a_running_turn_from_either_signal() {
        let r = claude_rules();
        // The footer marker.
        assert_eq!(
            classify(&r, &lines(&["⏸ manual mode on · esc to interrupt · ← for agents"])),
            Some(SessionStatus::Thinking)
        );
        // The spinner, whose verb is randomized — only its shape is stable.
        for spinner in ["✽ Ruminating…", "✢ Gitifying…", "· Bootstrapping… (1s · ↓ 22 tokens)"] {
            assert_eq!(
                classify(&r, &lines(&[spinner])),
                Some(SessionStatus::Thinking),
                "spinner {spinner:?} should read as thinking"
            );
        }
    }

    #[test]
    fn a_finished_turn_does_not_read_as_thinking() {
        let r = claude_rules();
        // These all sit near the spinner slot and must NOT match: the completion
        // row, the effort chip, the idle footer, and the truncated cost line (which
        // ends in an ellipsis and would fool a naive "ends with …" rule).
        for row in [
            "✻ Baked for 2s",
            "● high · /effort",
            "⏸ manual mode on · ? for shortcuts · ← for agents",
            "🤖 Opus 5 (1M context) (high) | 💰 $0.00 session | 🧠 21,973…",
        ] {
            assert_eq!(classify(&r, &lines(&[row])), None, "row {row:?} must not match");
        }
    }

    #[test]
    fn a_permission_prompt_outranks_thinking() {
        let r = claude_rules();
        let screen = lines(&[
            "✽ Ruminating…",
            "Do you want to create probe.txt?",
            "❯ 1. Yes",
            "2. Yes, allow all edits during this session (shift+tab)",
            "3. No",
            "Esc to cancel · Tab to amend",
        ]);
        // Rules are ordered, and blocking must win — otherwise a stale spinner row
        // left on screen would mask the prompt the user needs to answer.
        assert_eq!(classify(&r, &screen), Some(SessionStatus::BlockedPermission));
    }

    #[test]
    fn the_rewind_menu_is_not_mistaken_for_a_permission_prompt() {
        // "Esc to cancel" appears on the rewind menu AND the trust screen too. The
        // permission rule pins itself with "Tab to amend" precisely so an ordinary
        // rewind doesn't light up the "needs you" dot.
        let r = claude_rules();
        let screen = lines(&[
            "Rewind",
            "Restore the code and/or conversation to the point before…",
            "❯ (current)",
            "Enter to continue · Esc to cancel",
        ]);
        assert_ne!(classify(&r, &screen), Some(SessionStatus::BlockedPermission));
    }

    #[test]
    fn the_trust_gate_reads_as_blocked_not_idle() {
        // spwn deliberately does not auto-answer this. If it didn't surface as
        // "needs you", a fresh session would sit silently forever looking idle.
        let r = claude_rules();
        let screen = lines(&[
            "Quick safety check: Is this a project you created or one you trust?",
            "❯ 1. Yes, I trust this folder",
        ]);
        assert_eq!(classify(&r, &screen), Some(SessionStatus::BlockedQuestion));
    }

    #[test]
    fn a_row_window_ignores_matches_that_scrolled_away() {
        let rules = compile(&[rule(SessionStatus::Thinking, &["working"], &[], None, Some(2))]);
        let scrolled = lines(&["working", "a", "b"]); // outside the last 2 rows
        let visible = lines(&["a", "b", "working"]);
        assert_eq!(classify(&rules, &scrolled), None);
        assert_eq!(classify(&rules, &visible), Some(SessionStatus::Thinking));
    }

    #[test]
    fn all_narrows_a_rule_without_a_longer_pattern() {
        let rules = compile(&[rule(
            SessionStatus::BlockedPermission,
            &["Do you want to"],
            &["Tab to amend"],
            None,
            None,
        )]);
        assert_eq!(classify(&rules, &lines(&["Do you want to X?", "Tab to amend"])),
                   Some(SessionStatus::BlockedPermission));
        // Same `any` hit, but the required marker is absent.
        assert_eq!(classify(&rules, &lines(&["Do you want to X?", "Esc to cancel"])), None);
    }

    #[test]
    fn normalizes_whitespace_so_column_padding_cannot_break_a_match() {
        let rules = compile(&[rule(SessionStatus::Thinking, &["esc to interrupt"], &[], None, None)]);
        // A TUI pads with variable runs of spaces to align columns, and where those
        // runs fall shifts with the pane width. Every one of these is the same
        // footer at a different width, and all must match — that's the whole point
        // of normalizing before comparing.
        for row in [
            "⏸ manual mode on · esc to interrupt · ←",
            "⏸  manual   mode on ·  esc to interrupt  · ←",
            "⏸ manual mode on  ·   esc to  interrupt   ·  ←",
        ] {
            assert_eq!(
                classify(&rules, &lines(&[row])),
                Some(SessionStatus::Thinking),
                "padding variant {row:?} should still match"
            );
        }
        // Normalization collapses runs; it does not delete separators. A word that
        // genuinely isn't there still doesn't match.
        assert_eq!(classify(&rules, &lines(&["⏸ manual mode on · ? for shortcuts"])), None);
    }

    #[test]
    fn a_malformed_regex_disables_only_that_arm() {
        let rules = compile(&[rule(SessionStatus::Thinking, &["ok"], &[], Some("([unclosed"), None)]);
        // The `any` arm still works — one bad hand-edited pattern must not blind the
        // whole watcher.
        assert_eq!(classify(&rules, &lines(&["ok"])), Some(SessionStatus::Thinking));
        assert_eq!(classify(&rules, &lines(&["nope"])), None);
    }

    #[test]
    fn blocked_is_held_back_until_confirmed_but_others_publish_at_once() {
        let mut d = Debounce::new();
        // A transient blocked frame on its own must not publish.
        assert_eq!(d.feed(SessionStatus::BlockedPermission, 2), None);
        // Confirmed on the second consecutive observation.
        assert_eq!(
            d.feed(SessionStatus::BlockedPermission, 2),
            Some(SessionStatus::BlockedPermission)
        );
        // Non-blocking states are immediate — a spinner is self-correcting.
        assert_eq!(d.feed(SessionStatus::Thinking, 2), Some(SessionStatus::Thinking));
        // And repeats don't re-publish.
        assert_eq!(d.feed(SessionStatus::Thinking, 2), None);
    }

    #[test]
    fn a_flapping_frame_never_reaches_a_blocked_publish() {
        let mut d = Debounce::new();
        let mut published = Vec::new();
        // A half-drawn frame that alternates with the real state can never
        // accumulate a streak, so "waiting for you" is never announced. This is the
        // failure that would matter most: a spurious blocked dot teaches the user to
        // ignore the one signal that always deserves attention.
        for _ in 0..5 {
            if let Some(s) = d.feed(SessionStatus::BlockedPermission, 3) {
                published.push(s);
            }
            if let Some(s) = d.feed(SessionStatus::Thinking, 3) {
                published.push(s);
            }
        }
        assert!(
            !published.contains(&SessionStatus::BlockedPermission),
            "a flapping frame must never publish blocked, got {published:?}"
        );
        // Thinking publishes once, on the first transition, and isn't re-announced.
        assert_eq!(published, vec![SessionStatus::Thinking]);
    }
}

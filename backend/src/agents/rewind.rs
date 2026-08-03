//! Rewinding a conversation by driving the agent's own rewind UI.
//!
//! spwn never writes an agent's private on-disk history. It types the agent's
//! rewind command, navigates the menu, and — critically — **reads back the
//! highlighted row and refuses to proceed unless it matches the requested turn**.
//!
//! That guard is the whole safety story. A rewind can be paired with a file
//! restore, so landing on the wrong row doesn't just show the wrong conversation,
//! it reverts the working tree to the wrong point. Given the choice between
//! "rewound somewhere near where you asked" and "told you it couldn't", the second
//! is the only acceptable failure.
//!
//! Everything about the choreography is measured, not assumed (see the M0 spike):
//! the list runs oldest→newest with `(current)` last and the selection starting
//! there, so older entries are **Up**; rows show the user's prompt truncated with an
//! ellipsis, so matching is prefix-based; and the `❯` marker also decorates the
//! composer and every echoed turn, so the search must be scoped below the menu
//! header.

use crate::agents::def::{AgentDef, StepDirection, TuiRewind};

/// Collapse whitespace so a row's column padding can't break a comparison.
fn norm(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The menu rows currently carrying the selection marker, scoped below `header`.
///
/// Scoping is not optional: `❯` is also the composer prompt and the echo of every
/// user turn in the scrollback, so an unscoped search returns conversation lines and
/// the driver would believe it was on a menu row it never selected.
pub fn highlighted(lines: &[String], header: &str, marker: &str) -> Option<String> {
    let start = lines.iter().position(|l| l.contains(header))?;
    lines[start..]
        .iter()
        .find(|l| l.contains(marker))
        .map(|l| norm(l))
}

/// Strip a row down to its message text: drop the selection marker, the leading
/// indentation, and the trailing ellipsis the menu adds when it truncates.
fn row_text(row: &str, marker: &str) -> String {
    let n = norm(row);
    let n = n.strip_prefix(marker).unwrap_or(&n).trim();
    n.trim_end_matches('…').trim().to_string()
}

/// Does this menu row refer to the message `anchor_text`?
///
/// The row is the message **truncated**, so the correct relationship is
/// `anchor.starts_with(row)` — the row is a prefix of the full prompt, not the other
/// way round.
///
/// Getting this backwards is not academic. An earlier version took a fixed 24-char
/// prefix of the anchor and asked whether the row *contained* it. With prompts that
/// share an opening — "Reply with exactly one word: bravo" and "…: charlie" — every
/// row matched every anchor, so the walk stopped on the first row it saw and rewound
/// to the wrong point. The unit tests missed it because they compared prompts that
/// differed in their first word.
pub fn row_matches(row: &str, anchor_text: &str, marker: &str) -> bool {
    let row = row_text(row, marker);
    let anchor = norm(anchor_text);
    if anchor.is_empty() || row.is_empty() {
        return false;
    }
    // A row that is not a prefix of the anchor is a different message, full stop.
    anchor.starts_with(&row)
}

/// Where a navigation attempt ended up.
#[derive(Debug, PartialEq)]
pub enum Landing {
    /// The highlighted row matches the anchor; safe to accept.
    OnAnchor,
    /// The list ran out (clamped, or `max_steps`) without ever matching.
    NotFound,
}

/// Decide the next move given the rows seen so far.
///
/// Split out from the async driver so the stopping rules — the part that must never
/// be wrong — are testable without a live agent.
pub fn evaluate(seen: &[String], anchor_text: &str, marker: &str, max_steps: usize) -> Option<Landing> {
    let Some(current) = seen.last() else {
        return None;
    };
    if row_matches(current, anchor_text, marker) {
        return Some(Landing::OnAnchor);
    }
    if seen.len() > max_steps {
        return Some(Landing::NotFound);
    }
    // The list clamps at the top: once pressing Up stops changing the highlight,
    // there is nothing older to reach. Without this the driver would keep pressing
    // a key that does nothing until it hit `max_steps`.
    if seen.len() >= 2 && seen[seen.len() - 1] == seen[seen.len() - 2] {
        return Some(Landing::NotFound);
    }
    None
}

/// The key token that steps toward older entries.
pub fn step_key(def: &AgentDef, tui: &TuiRewind) -> String {
    match tui.step_back {
        StepDirection::Up => def.keys.menu_up.first().cloned(),
        StepDirection::Down => def.keys.menu_down.first().cloned(),
    }
    .unwrap_or_else(|| "Up".to_string())
}

// ---------------------------------------------------------------------------
// The live driver
// ---------------------------------------------------------------------------

use rmux_sdk::Pane;
use std::time::Duration;

async fn snapshot_lines(pane: &Pane) -> Vec<String> {
    pane.snapshot()
        .await
        .map(|s| s.visible_lines())
        .unwrap_or_default()
}

async fn press(pane: &Pane, keys: &[String]) -> Result<(), String> {
    for k in keys {
        pane.send_key(k.clone()).await.map_err(|e| e.to_string())?;
    }
    Ok(())
}

/// Wait until the rendered screen stops changing.
async fn settle(pane: &Pane, quiet: Duration, timeout: Duration) {
    let deadline = tokio::time::Instant::now() + timeout;
    let mut last = snapshot_lines(pane).await;
    let mut stable_since = tokio::time::Instant::now();
    while tokio::time::Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(120)).await;
        let now = snapshot_lines(pane).await;
        if now != last {
            last = now;
            stable_since = tokio::time::Instant::now();
        } else if stable_since.elapsed() >= quiet {
            return;
        }
    }
}

/// Open the agent's rewind menu, walk to `anchor_text`, and restore.
///
/// Returns `Err` — leaving the menu closed and the conversation untouched — if the
/// requested turn cannot be positively identified. The caller must treat that as a
/// hard failure and must NOT go on to restore files.
pub async fn drive(
    pane: &Pane,
    def: &AgentDef,
    anchor_text: &str,
    restore_files: bool,
) -> Result<(), String> {
    let tui = def
        .rewind
        .tui
        .as_ref()
        .ok_or_else(|| "this agent has no rewind choreography".to_string())?;

    if norm(anchor_text).is_empty() {
        return Err("could not resolve which turn to return to".to_string());
    }

    // EMPTY the composer, don't just interrupt.
    //
    // Measured live: one Escape stops a turn but leaves the input intact — Claude
    // Code even says "Esc again to clear". Typing the command into a non-empty
    // composer appends to whatever was there and submits the result as an ordinary
    // prompt. That is how a `/rewind` silently became the message
    // "Reply with exactly one word: charlie/rewind" — no error, no menu, and the
    // conversation gained a junk turn instead of losing three.
    press(pane, &def.keys.interrupt).await?;
    tokio::time::sleep(Duration::from_millis(250)).await;
    press(pane, &def.keys.clear).await?;
    settle(pane, Duration::from_millis(400), Duration::from_secs(8)).await;

    pane.send_text(&tui.command).await.map_err(|e| e.to_string())?;
    tokio::time::sleep(Duration::from_millis(def.input.submit_delay_ms.max(200))).await;

    // Verify what is actually staged before pressing Enter. If the composer holds
    // anything other than our command, submitting would send it to the model.
    let staged = snapshot_lines(pane).await;
    let composer = staged
        .iter()
        .rev()
        .find(|l| l.contains(&tui.marker) && norm(l).len() > tui.marker.len())
        .map(|l| norm(l))
        .unwrap_or_default();
    let cleaned = composer
        .trim_start_matches(&tui.marker)
        .trim()
        .to_string();
    if cleaned != tui.command {
        let _ = press(pane, &def.keys.clear).await;
        return Err(format!(
            "could not stage '{}' cleanly (composer held {cleaned:?}); nothing was changed",
            tui.command
        ));
    }

    press(pane, &def.keys.submit).await?;
    settle(pane, Duration::from_millis(500), Duration::from_secs(12)).await;

    let lines = snapshot_lines(pane).await;
    // Require the header AND the "no rewind" row: the header word alone could
    // plausibly appear in ordinary conversation text scrolled up the pane, and
    // mistaking that for an open menu means pressing navigation keys into a live
    // composer.
    let open = lines.iter().any(|l| l.contains(&tui.header))
        && lines.iter().any(|l| l.contains(&tui.current_row));
    if !open {
        let _ = press(pane, &def.keys.clear).await;
        return Err(format!(
            "the {} menu did not open — nothing was changed",
            tui.command
        ));
    }

    // Walk toward older entries, reading the highlight back after every press.
    let key = step_key(def, tui);
    let mut seen: Vec<String> = Vec::new();
    if let Some(row) = highlighted(&lines, &tui.header, &tui.marker) {
        seen.push(row);
    }
    let landing = loop {
        match evaluate(&seen, anchor_text, &tui.marker, tui.max_steps) {
            Some(l) => break l,
            None => {
                pane.send_key(key.clone()).await.map_err(|e| e.to_string())?;
                tokio::time::sleep(Duration::from_millis(180)).await;
                let lines = snapshot_lines(pane).await;
                match highlighted(&lines, &tui.header, &tui.marker) {
                    Some(row) => seen.push(row),
                    // The menu vanished mid-walk; bail rather than keep pressing
                    // keys into whatever is now on screen.
                    None => break Landing::NotFound,
                }
            }
        }
    };

    if landing == Landing::NotFound {
        // Close the menu so the session is left exactly as we found it.
        let _ = press(pane, &def.keys.menu_cancel).await;
        return Err(
            "that point is no longer in the agent's rewind list, so spwn could not \
             return to it safely. Nothing was changed."
                .to_string(),
        );
    }

    // On the right row: accept it, which opens a confirmation.
    press(pane, &def.keys.menu_accept).await?;
    settle(pane, Duration::from_millis(500), Duration::from_secs(12)).await;

    let want = if restore_files {
        tui.restore_both.as_deref().unwrap_or(&tui.restore_conversation)
    } else {
        &tui.restore_conversation
    };
    let action = snapshot_lines(pane).await;
    if !action.iter().any(|l| l.contains(want)) {
        let _ = press(pane, &def.keys.menu_cancel).await;
        return Err(format!("the rewind action menu did not offer '{want}'"));
    }

    // SECOND independent check, immediately before the irreversible step.
    //
    // The confirmation echoes the message it is about to restore to. Verifying that
    // echo means a mis-navigation has to fool BOTH the row read and this one to do
    // damage — and unlike the row read, this text is the agent's own statement of
    // what it is about to do.
    let echoed = action.iter().any(|l| row_matches(l, anchor_text, &tui.marker));
    if !echoed {
        let _ = press(pane, &def.keys.menu_cancel).await;
        return Err(
            "the confirmation did not name the turn spwn selected, so it stopped \
             rather than restore the wrong point. Nothing was changed."
                .to_string(),
        );
    }

    // Walk the ACTION menu with the menu's own down key, never the step-back key:
    // the wanted action is first, and stepping backwards from it wraps onto the
    // destructive tail of the list.
    let down = def.keys.menu_down.first().cloned().unwrap_or_else(|| "Down".into());
    for _ in 0..8 {
        let lines = snapshot_lines(pane).await;
        match highlighted(&lines, &tui.header, &tui.marker) {
            Some(row) if row.contains(want) => break,
            Some(_) => {
                pane.send_key(down.clone()).await.map_err(|e| e.to_string())?;
                tokio::time::sleep(Duration::from_millis(150)).await;
            }
            None => break,
        }
    }
    // Refuse to press Enter unless the highlight really is on the wanted action.
    let final_row = highlighted(&snapshot_lines(pane).await, &tui.header, &tui.marker);
    if !final_row.as_deref().is_some_and(|r| r.contains(want)) {
        let _ = press(pane, &def.keys.menu_cancel).await;
        return Err(format!(
            "could not select '{want}' (highlight was {final_row:?}); nothing was changed"
        ));
    }

    press(pane, &def.keys.menu_accept).await?;
    settle(pane, Duration::from_millis(800), Duration::from_secs(20)).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn highlight_is_scoped_below_the_menu_header() {
        // The conversation above the menu is full of `❯` — the composer, and every
        // echoed user turn. Unscoped, the driver reads one of those and believes it
        // is on a menu row it never selected. This is the single most dangerous
        // misread available, because the next step is pressing Enter.
        let screen = lines(&[
            "❯ Write the numbers 1 through 30",
            "⏺ done",
            "Rewind",
            "Restore the code and/or conversation to the point before…",
            "  Write the numbers 1 through 30",
            "❯ (current)",
        ]);
        assert_eq!(
            highlighted(&screen, "Rewind", "❯").as_deref(),
            Some("❯ (current)")
        );
    }

    #[test]
    fn no_menu_header_means_no_highlight() {
        let screen = lines(&["❯ some prompt", "⏺ output"]);
        assert_eq!(highlighted(&screen, "Rewind", "❯"), None);
    }

    #[test]
    fn a_truncated_row_still_matches_its_turn() {
        // Menu rows are the prompt cut off with an ellipsis.
        let row = "❯ Write the numbers 1 through 30, each on its own line, as plain…";
        let anchor = "Write the numbers 1 through 30, each on its own line, as plain text. \
                      Do not use any tools. End with the word: apple";
        assert!(row_matches(row, anchor, "❯"));
    }

    #[test]
    fn padding_differences_do_not_break_a_match() {
        let row = "❯   Write   the numbers 1 through 30, each on its own line, as plain…";
        // The anchor is the FULL prompt; the row is always a truncation of it.
        let anchor = "Write the numbers 1 through 30, each on its own line, as plain text. \
                      Do not use any tools.";
        assert!(row_matches(row, anchor, "❯"));
    }

    #[test]
    fn prompts_sharing_an_opening_are_told_apart() {
        // THE regression. These differ only in their last word, which is exactly the
        // shape of a repeated instruction ("Reply with exactly one word: X").
        // Matching on a fixed-length prefix of the anchor made every row match every
        // anchor, so the walk stopped on the first row and rewound to the wrong
        // point — discarding a different amount of work than the user asked for,
        // with both guards reporting success.
        let bravo = "Reply with exactly one word: bravo";
        let charlie = "Reply with exactly one word: charlie";
        assert!(row_matches("❯ Reply with exactly one word: bravo", bravo, "❯"));
        assert!(!row_matches("❯ Reply with exactly one word: charlie", bravo, "❯"));
        assert!(!row_matches("❯ Reply with exactly one word: bravo", charlie, "❯"));
        assert!(row_matches("❯ Reply with exactly one word: charlie", charlie, "❯"));
    }

    #[test]
    fn a_truncation_that_cuts_before_the_distinguishing_word_is_not_a_match() {
        // If the menu truncates so early that two prompts are indistinguishable, the
        // row is a prefix of BOTH. Matching the wrong one is silent damage, so this
        // documents the accepted behaviour: it matches whichever anchor it prefixes,
        // and the confirmation screen is the second line of defence.
        let row = "❯ Reply with exactly one…";
        assert!(row_matches(row, "Reply with exactly one word: bravo", "❯"));
        assert!(row_matches(row, "Reply with exactly one word: charlie", "❯"));
    }

    #[test]
    fn a_different_turn_does_not_match() {
        let row = "❯ Count slowly from 1 to 40, one number per line…";
        let anchor = "Write the numbers 1 through 30, each on its own line, as plain text";
        assert!(!row_matches(row, anchor, "❯"));
    }

    #[test]
    fn a_short_prompt_will_not_match_a_longer_one_that_starts_the_same() {
        // "ok" must not select "okay, now delete everything". Prefix matching on a
        // very short anchor is exactly where a silent mis-target would come from.
        assert!(!row_matches("❯ okay, now delete everything", "ok", "❯"));
        assert!(row_matches("❯ ok", "ok", "❯"));
    }

    #[test]
    fn the_current_row_never_matches_a_real_turn() {
        assert!(!row_matches("❯ (current)", "Write the numbers 1 through 30", "❯"));
    }

    #[test]
    fn stops_as_soon_as_the_anchor_is_highlighted() {
        let seen = vec![
            "❯ (current)".to_string(),
            "❯ Count slowly from 1 to 40…".to_string(),
            "❯ Write the numbers 1 through 30, each on…".to_string(),
        ];
        assert_eq!(
            evaluate(&seen, "Write the numbers 1 through 30, each on its own line", "❯", 60),
            Some(Landing::OnAnchor)
        );
    }

    #[test]
    fn a_clamped_list_reports_not_found_rather_than_pressing_forever() {
        // Once Up stops changing the highlight, the oldest entry is reached. The
        // requested turn is simply not in the agent's list (it prunes checkpoints),
        // and the only safe answer is to refuse.
        let seen = vec![
            "❯ (current)".to_string(),
            "❯ oldest entry".to_string(),
            "❯ oldest entry".to_string(),
        ];
        assert_eq!(evaluate(&seen, "a turn that is long gone", "❯", 60), Some(Landing::NotFound));
    }

    #[test]
    fn max_steps_bounds_the_walk() {
        let seen: Vec<String> = (0..8).map(|i| format!("❯ row {i}")).collect();
        assert_eq!(evaluate(&seen, "nowhere", "❯", 5), Some(Landing::NotFound));
    }

    #[test]
    fn keeps_walking_while_rows_still_change_and_do_not_match() {
        let seen = vec!["❯ (current)".to_string(), "❯ another turn".to_string()];
        assert_eq!(evaluate(&seen, "the one I want", "❯", 60), None);
    }

    #[test]
    fn never_reports_onanchor_for_an_empty_anchor() {
        // An anchor whose prompt text couldn't be resolved must abort, not match the
        // first row it sees.
        let seen = vec!["❯ (current)".to_string()];
        assert_ne!(evaluate(&seen, "", "❯", 60), Some(Landing::OnAnchor));
    }
}

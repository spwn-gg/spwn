//! The agent definition schema — everything spwn needs to drive one coding-agent
//! CLI as a TUI inside an rmux pane, expressed as data.
//!
//! The point of this file is that adding support for a new agent should be a TOML
//! file, not a Rust change. Every value here was chosen because the M0 spike
//! (`backend/tests/agent_tui_spike.rs`) had to measure it against a real `claude`.
//! Where a field looks over-general, it is usually because the spike proved the
//! obvious simpler shape was wrong — those cases are called out inline.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

fn one() -> u32 {
    1
}

// ---------------------------------------------------------------------------
// Top level
// ---------------------------------------------------------------------------

/// One agent CLI spwn knows how to drive.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AgentDef {
    /// Stable key, e.g. `"claude"`. Persisted on `TerminalRec.agent`, so renaming one
    /// orphans existing sessions.
    pub id: String,
    /// Display name for the UI.
    pub name: String,
    /// Single-glyph icon for tabs and the project tree.
    #[serde(default)]
    pub icon: Option<String>,
    /// Schema version, for future migrations of this file format.
    #[serde(default = "one")]
    pub schema: u32,
    /// Ships with spwn but has never been driven against the real binary. The UI
    /// marks these "experimental"; we'd rather say so than imply parity.
    #[serde(default)]
    pub untested: bool,
    pub binary: BinarySpec,
    /// Extra environment for the pane, as `KEY=VALUE`.
    #[serde(default)]
    pub env: BTreeMap<String, String>,
    pub argv: ArgvSpec,
    #[serde(default)]
    pub session: SessionSpec,
    #[serde(default)]
    pub transcript: TranscriptSpec,
    #[serde(default)]
    pub keys: KeySpec,
    #[serde(default)]
    pub input: InputSpec,
    #[serde(default)]
    pub modes: ModeSpec,
    #[serde(default)]
    pub detect: DetectSpec,
    #[serde(default)]
    pub rewind: RewindSpec,
}

impl AgentDef {
    /// What this agent can actually do, derived from its definition. The UI hides
    /// affordances rather than offering ones that will fail — an agent with no
    /// transcript adapter genuinely cannot be rewound.
    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            transcript: self.transcript.format != TranscriptFormat::None,
            rewind: self.rewind.strategy != RewindStrategy::None
                && self.transcript.format != TranscriptFormat::None,
            headless: self.argv.headless.is_some(),
            status: !self.detect.rules.is_empty(),
        }
    }
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Capabilities {
    /// Can spwn read this agent's conversation history?
    pub transcript: bool,
    /// Can spwn rewind the conversation to an earlier turn?
    pub rewind: bool,
    /// Can spwn run this agent headlessly (scheduled tasks)?
    pub headless: bool,
    /// Does this agent have real status rules, or only generic activity detection?
    pub status: bool,
}

// ---------------------------------------------------------------------------
// Binary discovery
// ---------------------------------------------------------------------------

/// How to locate the agent's executable. Generalizes `pty::launcher::find_claude_bin`:
/// GUI and daemon processes don't inherit the shell `$PATH`, so probing well-known
/// install dirs is a necessity, not a nicety.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BinarySpec {
    /// Environment variable checked first, e.g. `CLAUDE_BIN`.
    #[serde(default)]
    pub env: Option<String>,
    /// Name to resolve on `$PATH`.
    pub name: String,
    /// Home-relative fallbacks, tried in order.
    #[serde(default)]
    pub candidates: Vec<String>,
}

// ---------------------------------------------------------------------------
// argv
// ---------------------------------------------------------------------------

/// Command lines for each way of starting the agent. See [`render_argv`] for the
/// placeholder rules.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ArgvSpec {
    /// A brand-new session.
    pub new: Vec<String>,
    /// Resume an existing session by id.
    pub resume: Vec<String>,
    /// Fork an existing session at its tip into a new one.
    #[serde(default)]
    pub fork: Option<Vec<String>>,
    /// Non-interactive run for scheduled tasks. `None` ⇒ can't be scheduled.
    #[serde(default)]
    pub headless: Option<Vec<String>>,
}

// ---------------------------------------------------------------------------
// Session identity
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum IdStrategy {
    /// spwn generates the id and passes it in (`claude --session-id <uuid>`).
    /// Strongly preferred: binding is synchronous and there is no discovery race.
    /// CONFIRMED working for claude by the M0 spike.
    #[default]
    Assign,
    /// The agent picks its own id; spwn discovers it afterwards by watching the
    /// transcript root for a new file.
    Discover,
    /// No session identity at all — no transcript, no resume, no rewind.
    None,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SessionSpec {
    #[serde(default)]
    pub id_strategy: IdStrategy,
}

// ---------------------------------------------------------------------------
// Transcript
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum TranscriptFormat {
    /// Claude Code's JSONL (`transcript::parser`).
    ClaudeJsonl,
    /// No adapter. The agent still gets worktrees, commits, checkpoints and
    /// activity-based status — but no history view, no rewind, no fork-at-a-point.
    #[default]
    None,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum LocateMode {
    /// Scan the transcript root's subdirectories for `<session_id>.<ext>`.
    ///
    /// This is the default *deliberately*. Claude Code's per-project directory name
    /// is a slug of the cwd that mangles `/`, `.` and `_` alike
    /// (`/Users/m/.foo_bar` → `-Users-m--foo-bar`), and reimplementing it means
    /// silently breaking whenever it changes. Searching is O(projects) once per
    /// lookup and always correct.
    #[default]
    Search,
    /// Build the path directly from `root` + `file`.
    Template,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TranscriptSpec {
    #[serde(default)]
    pub format: TranscriptFormat,
    /// Root directory, supports `{claudeConfigDir}` and `{home}`.
    #[serde(default)]
    pub root: Option<String>,
    #[serde(default)]
    pub locate: LocateMode,
    /// Filename within the root, supports `{sessionId}`.
    #[serde(default)]
    pub file: Option<String>,
}

// ---------------------------------------------------------------------------
// Keys
// ---------------------------------------------------------------------------

/// tmux-style key tokens passed to `Pane::send_key`. Known names (`Enter`, `Escape`,
/// `Up`, `BTab`, `C-c`) are encoded as keys; anything else is sent as literal bytes.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct KeySpec {
    pub submit: Vec<String>,
    /// Graceful interrupt. CONFIRMED for claude: `Escape` stops a running turn.
    pub interrupt: Vec<String>,
    /// Empty the composer.
    ///
    /// Separate from `interrupt`, because one Escape does NOT clear Claude Code's
    /// input — it prompts "Esc again to clear". Typing into a non-empty composer
    /// APPENDS, which is how a `/rewind` silently became the message
    /// "…charlie/rewind" and a follow-up prompt arrived mangled.
    ///
    /// Default `C-u` (readline kill-line), NOT a double Escape: measured live,
    /// Esc-Esc on an EMPTY composer is Claude Code's "edit previous message"
    /// shortcut, so using it as a clear silently drops the TUI into history-editing
    /// mode and every later keystroke goes somewhere unintended. `C-u` clears when
    /// there is text and is a harmless no-op when there isn't.
    pub clear: Vec<String>,
    /// Escalation when `interrupt` doesn't take.
    pub interrupt_hard: Vec<String>,
    /// Cycle permission modes (claude: shift-tab).
    pub mode_cycle: Vec<String>,
    /// Force a redraw — the fallback for priming a reattached pane when
    /// `capture_pane` is unavailable.
    pub redraw: Vec<String>,
    pub menu_up: Vec<String>,
    pub menu_down: Vec<String>,
    pub menu_accept: Vec<String>,
    pub menu_cancel: Vec<String>,
}

impl Default for KeySpec {
    fn default() -> Self {
        Self {
            submit: vec!["Enter".into()],
            interrupt: vec!["Escape".into()],
            clear: vec!["C-u".into()],
            interrupt_hard: vec!["C-c".into(), "C-c".into()],
            mode_cycle: vec!["BTab".into()],
            redraw: vec!["C-l".into()],
            menu_up: vec!["Up".into()],
            menu_down: vec!["Down".into()],
            menu_accept: vec!["Enter".into()],
            menu_cancel: vec!["Escape".into()],
        }
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum PasteMode {
    /// Wrap in bracketed-paste markers so the TUI treats a multi-line blob as one
    /// paste instead of N submissions. CONFIRMED working for claude.
    #[default]
    Bracketed,
    /// Send the raw text.
    Literal,
    /// Send in fixed-size chunks with a delay — for TUIs that drop large writes.
    Chunked,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct InputSpec {
    pub paste: PasteMode,
    pub paste_prefix: String,
    pub paste_suffix: String,
    /// Delay between pasting and pressing submit. The paste has to land in the
    /// composer before Enter, or Enter submits an empty prompt.
    pub submit_delay_ms: u64,
    pub chunk_bytes: usize,
    pub chunk_delay_ms: u64,
}

impl Default for InputSpec {
    fn default() -> Self {
        Self {
            paste: PasteMode::Bracketed,
            paste_prefix: "\u{1b}[200~".into(),
            paste_suffix: "\u{1b}[201~".into(),
            submit_delay_ms: 120,
            chunk_bytes: 4096,
            chunk_delay_ms: 10,
        }
    }
}

// ---------------------------------------------------------------------------
// Permission modes
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ModeSpec {
    /// Modes the UI offers, in cycle order.
    #[serde(default)]
    pub cycle: Vec<String>,
    /// mode name → value for the launch flag. An empty value means "omit the flag".
    #[serde(default)]
    pub launch: BTreeMap<String, String>,
    /// Regex that reads the CURRENT mode off the screen, capturing it in group 1.
    /// Needed because `mode_cycle` is a blind keypress: without reading back, spwn
    /// can't know how many times to press to reach a target.
    #[serde(default)]
    pub line: Option<String>,
}

// ---------------------------------------------------------------------------
// Status detection
// ---------------------------------------------------------------------------

/// Live status of an agent session. Wire-compatible with the frontend's
/// `SessionStatus` union (camelCase).
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum SessionStatus {
    Thinking,
    BlockedPermission,
    BlockedQuestion,
    Done,
    Error,
    Idle,
}

/// Restrict a rule to part of the screen.
#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RowWindow {
    /// Only the last N rendered rows.
    pub last: usize,
}

/// One status rule. `any` (substring) and `regex` are OR'd; a rule with neither
/// never matches.
///
/// NOTE the M0 spike's finding: a row window is the *wrong* default for a "is a turn
/// running" rule. Measured live, the bottom 12 rows were entirely blank 600ms and
/// 1500ms into a turn — the spinner is not pinned to the bottom. Use `rows` only for
/// rules whose text really is anchored (menus, footers).
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DetectRule {
    pub status: SessionStatus,
    #[serde(default)]
    pub any: Vec<String>,
    #[serde(default)]
    pub regex: Option<String>,
    #[serde(default)]
    pub rows: Option<RowWindow>,
    /// Every listed substring must ALSO be present. Lets a rule be specific without
    /// one long brittle pattern — e.g. claude's permission prompt is
    /// `any = ["Do you want to"]` + `all = ["Tab to amend"]`, because "Esc to cancel"
    /// alone also appears on the trust and rewind menus.
    #[serde(default)]
    pub all: Vec<String>,
}

#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct DetectSpec {
    /// Ordered; first match wins.
    #[serde(default)]
    pub rules: Vec<DetectRule>,
    /// Substrings that mean "the TUI is up and accepting input" — gates the initial
    /// prompt injection.
    #[serde(default)]
    pub ready: Vec<String>,
    /// Quiet time after which a session with no matching rule is considered `Done`.
    pub idle_after_ms: u64,
    /// How many consecutive snapshots a `blocked*` must survive before it is
    /// published. Guards against a half-drawn frame being read as a prompt.
    pub confirm_frames: u8,
}

impl Default for DetectSpec {
    fn default() -> Self {
        Self {
            rules: Vec::new(),
            ready: Vec::new(),
            idle_after_ms: 1500,
            confirm_frames: 2,
        }
    }
}

// ---------------------------------------------------------------------------
// Rewind
// ---------------------------------------------------------------------------

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum RewindStrategy {
    /// Drive the agent's own rewind UI. Keeps spwn entirely inside supported
    /// surfaces — it never writes the agent's private on-disk history.
    TuiCommand,
    /// No conversation rewind available.
    #[default]
    None,
}

/// Choreography for [`RewindStrategy::TuiCommand`].
///
/// All of this is measured, not guessed. From the M0 spike against claude v2.1.220:
/// the list runs oldest→newest with `(current)` **last** and the selection starting
/// there, so reaching an earlier turn means pressing **Up**; rows show the user
/// prompt truncated with `…`, so matching is prefix-based; and the `❯` marker is also
/// the composer and every echoed turn, so the search must be scoped below `header`.
#[derive(Deserialize, Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TuiRewind {
    /// Slash command that opens the rewind UI.
    pub command: String,
    /// Text identifying the menu; also the scope anchor for the highlight search.
    pub header: String,
    /// Marker on the highlighted row.
    pub marker: String,
    /// Direction to step toward older entries.
    #[serde(default)]
    pub step_back: StepDirection,
    /// Max presses before giving up (the list is bounded and clamps at the top).
    pub max_steps: usize,
    /// Row text for "no rewind" — where the selection starts.
    pub current_row: String,
    /// Action-menu entry that restores only the conversation.
    pub restore_conversation: String,
    /// Action-menu entry that restores conversation *and* files, when the agent
    /// offers it. `None` ⇒ spwn restores files itself from its own checkpoint.
    #[serde(default)]
    pub restore_both: Option<String>,
}

#[derive(Deserialize, Serialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase")]
pub enum StepDirection {
    /// CONFIRMED for claude: older entries are UP from `(current)`.
    #[default]
    Up,
    Down,
}

#[derive(Deserialize, Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RewindSpec {
    #[serde(default)]
    pub strategy: RewindStrategy,
    #[serde(default)]
    pub tui: Option<TuiRewind>,
}

// ---------------------------------------------------------------------------
// argv rendering
// ---------------------------------------------------------------------------

/// Substitute `{placeholder}`s in an argv template.
///
/// Two rules, deliberately dumb — no conditionals, no expressions, no shell:
///   1. `{name}` anywhere in an element is replaced by `ctx["name"]` (empty if unset).
///   2. An element wrapped in `[[ ]]` is an **optional group**: its body is split on
///      whitespace into one-or-more argv elements, and the WHOLE group is dropped if
///      any placeholder inside resolves empty.
///
/// Rule 2 is a group rather than a per-element flag because a flag and its value have
/// to disappear *together*. Marking them separately —
/// `["[[--permission-mode]]", "[[{permissionMode}]]"]` — leaves the flag behind when
/// the value is empty, because the flag element contains no placeholder to be empty.
/// The agent then gets a dangling `--permission-mode` and refuses to start. So the
/// correct spelling is one group: `["[[--permission-mode {permissionMode}]]"]`.
///
/// The whitespace split happens on the TEMPLATE, before substitution, so a
/// substituted value containing spaces is never split. Combined with
/// `ProcessSpec::argv` never invoking a shell, a 40 KB assembled context prompt
/// passes through as exactly one argument with no quoting.
pub fn render_argv(template: &[String], ctx: &BTreeMap<&str, String>) -> Vec<String> {
    let mut out = Vec::with_capacity(template.len());
    for raw in template {
        let trimmed = raw.trim();
        if trimmed.starts_with("[[") && trimmed.ends_with("]]") && trimmed.len() >= 4 {
            let body = &trimmed[2..trimmed.len() - 2];
            // Split the TEMPLATE, then substitute each token.
            let tokens: Vec<&str> = body.split_whitespace().collect();
            let mut rendered = Vec::with_capacity(tokens.len());
            let mut drop_group = false;
            for t in tokens {
                let (v, had_empty) = substitute(t, ctx);
                if had_empty {
                    drop_group = true;
                    break;
                }
                rendered.push(v);
            }
            if !drop_group {
                out.extend(rendered);
            }
            continue;
        }
        let (rendered, _) = substitute(raw, ctx);
        out.push(rendered);
    }
    out
}

/// Replace every `{key}`; report whether any resolved empty.
fn substitute(s: &str, ctx: &BTreeMap<&str, String>) -> (String, bool) {
    let mut out = String::with_capacity(s.len());
    let mut had_empty = false;
    let mut rest = s;
    while let Some(open) = rest.find('{') {
        let Some(close_rel) = rest[open..].find('}') else {
            break; // unterminated — treat the remainder as literal
        };
        let close = open + close_rel;
        out.push_str(&rest[..open]);
        let key = &rest[open + 1..close];
        match ctx.get(key) {
            Some(v) if !v.is_empty() => out.push_str(v),
            _ => had_empty = true,
        }
        rest = &rest[close + 1..];
    }
    out.push_str(rest);
    (out, had_empty)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(pairs: &[(&'static str, &str)]) -> BTreeMap<&'static str, String> {
        pairs.iter().map(|(k, v)| (*k, v.to_string())).collect()
    }

    #[test]
    fn substitutes_placeholders_in_place() {
        let t: Vec<String> = ["{bin}", "--session-id", "{sessionId}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let c = ctx(&[("bin", "/usr/bin/claude"), ("sessionId", "abc-123")]);
        assert_eq!(render_argv(&t, &c), vec!["/usr/bin/claude", "--session-id", "abc-123"]);
    }

    #[test]
    fn drops_an_optional_group_as_a_unit() {
        let t: Vec<String> = ["{bin}", "[[--permission-mode {permissionMode}]]"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        // Set: the group expands to two argv elements.
        let with = ctx(&[("bin", "claude"), ("permissionMode", "plan")]);
        assert_eq!(render_argv(&t, &with), vec!["claude", "--permission-mode", "plan"]);
        // Empty: the flag AND its value go together. A flag left behind with no value
        // makes the agent exit immediately with a usage error, which surfaces as a
        // pane that dies on open — very hard to diagnose from the UI.
        let without = ctx(&[("bin", "claude"), ("permissionMode", "")]);
        assert_eq!(render_argv(&t, &without), vec!["claude"]);
        // Missing entirely behaves the same as empty.
        let absent = ctx(&[("bin", "claude")]);
        assert_eq!(render_argv(&t, &absent), vec!["claude"]);
    }

    #[test]
    fn an_optional_group_splits_on_the_template_not_on_values() {
        // The group is split into argv elements before substitution, so a value
        // containing spaces stays ONE argument.
        let t: Vec<String> = ["{bin}", "[[--name {title}]]"].iter().map(|s| s.to_string()).collect();
        let c = ctx(&[("bin", "claude"), ("title", "two words here")]);
        assert_eq!(render_argv(&t, &c), vec!["claude", "--name", "two words here"]);
    }

    #[test]
    fn a_literal_element_with_an_empty_placeholder_is_kept_as_empty() {
        // Only `[[ ]]` opts into dropping; a bare element must not silently vanish,
        // or a required argument would disappear and the argv would be malformed in
        // a way that's very hard to debug from a TUI that just exits.
        let t: Vec<String> = ["{bin}", "--resume", "{sessionId}"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let c = ctx(&[("bin", "claude")]);
        assert_eq!(render_argv(&t, &c), vec!["claude", "--resume", ""]);
    }

    #[test]
    fn passes_arbitrary_text_through_without_quoting() {
        // No shell is involved, so a prompt containing spaces, quotes, newlines and
        // braces must survive as ONE argument.
        let t: Vec<String> = ["{bin}", "-p", "{prompt}"].iter().map(|s| s.to_string()).collect();
        let nasty = "a \"quoted\" $VAR\nsecond line; rm -rf /";
        let c = ctx(&[("bin", "claude"), ("prompt", nasty)]);
        let out = render_argv(&t, &c);
        assert_eq!(out.len(), 3);
        assert_eq!(out[2], nasty);
    }

    #[test]
    fn unknown_braces_are_left_alone() {
        let t: Vec<String> = ["echo", "{not_a_key}", "{"].iter().map(|s| s.to_string()).collect();
        let c = ctx(&[]);
        // Unterminated brace is literal; unknown key renders empty but keeps the element.
        assert_eq!(render_argv(&t, &c), vec!["echo", "", "{"]);
    }

    #[test]
    fn claude_default_parses_and_reports_full_capabilities() {
        let def: AgentDef = toml::from_str(include_str!("../../assets/agents/claude.toml"))
            .expect("bundled claude.toml must parse");
        assert_eq!(def.id, "claude");
        assert_eq!(def.session.id_strategy, IdStrategy::Assign);
        assert_eq!(def.transcript.format, TranscriptFormat::ClaudeJsonl);
        assert_eq!(def.rewind.strategy, RewindStrategy::TuiCommand);
        let caps = def.capabilities();
        assert!(caps.transcript && caps.rewind && caps.headless && caps.status);
        // The env fix the M0 spike found is load-bearing: without it the agent runs
        // with transcript saving off and every downstream feature silently dies.
        assert!(def.env.contains_key("CLAUDE_CODE_CHILD_SESSION"));
        // Rewind steps UP toward older entries (measured, not guessed).
        let tui = def.rewind.tui.as_ref().expect("claude has tui rewind choreography");
        assert_eq!(tui.step_back, StepDirection::Up);
    }

    #[test]
    fn untested_defaults_parse_and_report_reduced_capabilities() {
        for body in [
            include_str!("../../assets/agents/codex.toml"),
            include_str!("../../assets/agents/gemini.toml"),
        ] {
            let def: AgentDef = toml::from_str(body).expect("bundled agent toml must parse");
            assert!(def.untested, "{} should be marked untested", def.id);
            let caps = def.capabilities();
            assert!(!caps.rewind, "{} must not advertise rewind", def.id);
            assert!(!caps.transcript, "{} must not advertise a transcript", def.id);
        }
    }

    #[test]
    fn rewind_requires_a_transcript_to_map_anchors_onto_menu_rows() {
        // Rewind targets a turn by its prompt text, which only the transcript can
        // supply. A def claiming tuiCommand without a transcript adapter must not
        // advertise the capability.
        let mut def: AgentDef =
            toml::from_str(include_str!("../../assets/agents/claude.toml")).unwrap();
        def.transcript.format = TranscriptFormat::None;
        assert!(!def.capabilities().rewind);
    }

    #[test]
    fn unknown_fields_are_rejected() {
        // A typo'd key should be a loud parse error, not a silently-ignored setting
        // that leaves the user wondering why their pattern never fires.
        let bad = r#"
            id = "x"
            name = "X"
            [binary]
            name = "x"
            [argv]
            new = ["{bin}"]
            resume = ["{bin}"]
            [detect]
            idleAfterMs = 1
            confirmFrames = 1
            typoField = 3
        "#;
        assert!(toml::from_str::<AgentDef>(bad).is_err());
    }
}

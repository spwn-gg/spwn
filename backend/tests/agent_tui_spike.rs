//! M0 RISK SPIKE — drive a real agent TUI through **rmux** and prove every
//! primitive the pluggable-agent design depends on.
//!
//! The older `rewind_branch_spike.rs` drove `claude` over `portable-pty`. This one
//! drives it the way spwn actually will: an rmux pane, `send_text`/`send_key` in,
//! `snapshot().visible_lines()` out. Everything asserted here becomes a line of
//! `assets/agents/claude.toml`.
//!
//! What it validates, in order:
//!   A. `claude --session-id <uuid>` launches in an rmux pane and the transcript
//!      lands at the uuid we assigned  (→ `[session] idStrategy = "assign"`)
//!   B. bracketed paste + `send_key("Enter")` submits a prompt (→ `[input]`)
//!   C. `esc to interrupt` is on screen mid-turn                (→ detect: thinking)
//!   D. the screen goes quiet and the marker clears             (→ detect: done)
//!   E. `capture_pane` reproduces the screen for reattach priming
//!   F. `send_key("Escape")` interrupts a running turn          (→ `[keys] interrupt`)
//!   G. a tool-permission prompt's on-screen shape              (→ detect: blocked*)
//!   H. the `/rewind` menu choreography                         (→ `[rewind.tui]`)
//!
//! REAL, authenticated model calls. Gated — no-ops unless `RUN_CLAUDE_PTY_SPIKE=1`
//! and both `claude` and `rmux` are found. Run it with:
//!   make agent-spike
//! or directly:
//!   RUN_CLAUDE_PTY_SPIKE=1 cargo test --test agent_tui_spike -- --nocapture --test-threads=1
//!
//! Phases G and H are DISCOVERY: they never fail the test, they dump annotated
//! frames. Read those frames, then write the patterns into the agent TOML.

use rmux_sdk::{
    EnsureSession, EnsureSessionPolicy, Pane, ProcessSpec, Rmux, RmuxBuilder, SessionName,
    TerminalSizeSpec,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Gating / discovery
// ---------------------------------------------------------------------------

fn spike_enabled() -> bool {
    matches!(std::env::var("RUN_CLAUDE_PTY_SPIKE").ok().as_deref(), Some("1"))
}

fn home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
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

/// Mirrors `pty::launcher::find_claude_bin` so the spike resolves the same binary
/// the product will.
fn claude_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CLAUDE_BIN") {
        let p = PathBuf::from(p);
        if p.exists() {
            return Some(p);
        }
    }
    which("claude").or_else(|| {
        [".local/bin/claude", ".claude/local/claude", ".npm-global/bin/claude"]
            .iter()
            .filter_map(|rel| home().map(|h| h.join(rel)))
            .find(|p| p.exists())
    })
}

/// Mirrors `pty::launcher::find_rmux_bin`.
fn rmux_bin() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("RMUX_SDK_DAEMON_BINARY") {
        let pb = PathBuf::from(p);
        if pb.exists() {
            return Some(pb);
        }
    }
    ["/opt/homebrew/bin/rmux", "/usr/local/bin/rmux", "/usr/bin/rmux"]
        .iter()
        .map(PathBuf::from)
        .find(|p| p.exists())
        .or_else(|| which("rmux"))
}

/// `$CLAUDE_CONFIG_DIR/projects` or `~/.claude/projects` — same rule as
/// `projects::scanner::projects_root`.
fn projects_root() -> PathBuf {
    std::env::var("CLAUDE_CONFIG_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| home().expect("HOME set").join(".claude"))
        .join("projects")
}

/// Find `<session_id>.jsonl` anywhere under the projects root. Deliberately a
/// SEARCH, not a slug computation — the directory name mangles `/`, `.` and `_`
/// and we never want to reimplement that. Same approach as `locate_session`.
fn locate_session(session_id: &str) -> Option<PathBuf> {
    let want = format!("{session_id}.jsonl");
    for proj in std::fs::read_dir(projects_root()).ok()?.flatten() {
        let p = proj.path();
        if !p.is_dir() {
            continue;
        }
        if let Ok(files) = std::fs::read_dir(&p) {
            for f in files.flatten() {
                if f.file_name().to_string_lossy() == want {
                    return Some(f.path());
                }
            }
        }
    }
    None
}

/// Count main-chain user/assistant records, and collect their uuids in file order.
fn transcript_turns(path: &Path) -> (usize, Vec<String>) {
    let Ok(text) = std::fs::read_to_string(path) else {
        return (0, Vec::new());
    };
    let mut uuids = Vec::new();
    for line in text.lines() {
        let Ok(v) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        let is_side = v.get("isSidechain").and_then(|s| s.as_bool()).unwrap_or(false);
        let kind = v.get("type").and_then(|t| t.as_str()).unwrap_or("");
        if !is_side && (kind == "user" || kind == "assistant") {
            if let Some(u) = v.get("uuid").and_then(|u| u.as_str()) {
                uuids.push(u.to_string());
            }
        }
    }
    (uuids.len(), uuids)
}

// ---------------------------------------------------------------------------
// The rmux-backed driver — the exact primitives `agents/session.rs` will use.
// ---------------------------------------------------------------------------

const BRACKET_START: &str = "\u{1b}[200~";
const BRACKET_END: &str = "\u{1b}[201~";

/// Is this rendered row the "a turn is running" spinner?
///
/// CONFIRMED live (Claude Code v2.1.220): mid-turn the TUI renders
/// `✽ Transmuting…` / `· Nesting…` — a rotating spinner glyph plus a RANDOMIZED
/// gerund — and on completion the same slot becomes `✻ Baked for 2s`. The plan's
/// assumed `esc to interrupt` marker **does not exist in this version at all**.
///
/// So the stable signal is structural, not lexical: a single non-alphanumeric
/// glyph followed by a word ending in `…`. Deliberately checked against the
/// neighbours that must NOT match:
///   `✻ Baked for 2s`                      → tok[1] has no ellipsis
///   `● high · /effort`                    → tok[1] has no ellipsis
///   `⏸ manual mode on · ← for agents`     → tok[1] has no ellipsis
///   `🤖 Opus 5 … | 🧠 21,973…`            → tok[1] = "Opus", no ellipsis
///
/// This is why `DetectSpec` needs a `regex` arm and not just substrings.
fn is_thinking_line(l: &str) -> bool {
    let toks: Vec<&str> = l.trim().split_whitespace().collect();
    if toks.len() < 2 {
        return false;
    }
    let first_is_glyph = toks[0].chars().count() == 1
        && toks[0].chars().next().is_some_and(|c| !c.is_alphanumeric());
    first_is_glyph && toks[1].ends_with('…')
}

/// The footer's turn-running marker.
///
/// CONFIRMED live: the status footer swaps a segment while a turn is in flight —
///   idle:     `⏸ manual mode on · ? for shortcuts · ← for agents`
///   running:  `⏸ manual mode on · esc to interrupt · ← for agents`
/// So `esc to interrupt` IS real (the plan was right); earlier runs missed it only
/// because they sampled the wrong row window at the wrong moments.
///
/// Two independent signals for the same state is a good thing: ship both as
/// `any = [...]` so a change to either one alone doesn't blind the watcher.
const FOOTER_RUNNING: &str = "esc to interrupt";

/// The completed-turn row, e.g. `✻ Baked for 2s`. Used instead of a content
/// sentinel: the prompt is echoed on screen, so any word we ask the model to emit
/// is already visible *before* the turn starts.
fn is_done_line(l: &str) -> bool {
    let toks: Vec<&str> = l.trim().split_whitespace().collect();
    toks.len() >= 4
        && toks[0].chars().count() == 1
        && toks[0].chars().next().is_some_and(|c| !c.is_alphanumeric())
        && toks[2] == "for"
        && toks[3].ends_with('s')
}

struct AgentPane<'a> {
    pane: Pane,
    session_name: String,
    rmux: &'a Rmux,
}

impl<'a> AgentPane<'a> {
    async fn launch(
        rmux: &'a Rmux,
        session_name: &str,
        argv: Vec<String>,
        cwd: &Path,
        cols: u16,
        rows: u16,
    ) -> anyhow::Result<Self> {
        let name = SessionName::new(session_name.to_string())
            .map_err(|e| anyhow::anyhow!("invalid session name: {e}"))?;
        let session = rmux
            .ensure_session(
                EnsureSession::named(name)
                    .policy(EnsureSessionPolicy::CreateOrReuse)
                    .detached(true)
                    .size(TerminalSizeSpec::new(cols, rows))
                    .working_directory(cwd.to_string_lossy().into_owned())
                    .process(ProcessSpec::argv(argv))
                    .environment(vec![
                        "TERM=xterm-256color".to_string(),
                        // Keep the spike cheap / non-chatty.
                        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1".to_string(),
                        // CRITICAL, discovered live: a `claude` launched from inside
                        // another Claude session inherits CLAUDE_CODE_CHILD_SESSION,
                        // which turns TRANSCRIPT SAVING OFF ("⚠ Transcript saving is
                        // off — inherited CLAUDE_CODE_CHILD_SESSION marker"). No
                        // transcript means no session id binding, no turn detection,
                        // no Timeline, no rewind — i.e. most of spwn.
                        //
                        // This is NOT just a test artifact: rmux panes inherit the
                        // long-lived DAEMON's environment, so whatever env the daemon
                        // was first started with leaks into every agent spwn spawns.
                        // These two lines belong in `[env]` of assets/agents/claude.toml.
                        "CLAUDE_CODE_CHILD_SESSION=".to_string(),
                        "CLAUDE_CODE_FORCE_SESSION_PERSISTENCE=1".to_string(),
                    ]),
            )
            .await?;
        Ok(Self {
            pane: session.pane(0, 0),
            session_name: session_name.to_string(),
            rmux,
        })
    }

    /// The rendered screen as lines — what `detect.rules` will match against.
    async fn lines(&self) -> Vec<String> {
        match self.pane.snapshot().await {
            Ok(s) => s.visible_lines(),
            Err(e) => vec![format!("<snapshot error: {e}>")],
        }
    }

    async fn text(&self) -> String {
        self.lines().await.join("\n")
    }

    /// Whitespace-normalized join of the last `n` rows — the `rows = { last = n }`
    /// window from the detect schema.
    async fn tail_text(&self, n: usize) -> String {
        let lines = self.lines().await;
        let start = lines.len().saturating_sub(n);
        lines[start..]
            .iter()
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Type a prompt the way spwn will: one bracketed paste, a settle delay, then
    /// a separate Enter. Pasting and submitting must be distinct so `agent_send`
    /// can offer `submit: false` (the "→ parent" paste-for-review contract).
    async fn paste(&self, text: &str) -> anyhow::Result<()> {
        self.pane
            .send_text(format!("{BRACKET_START}{text}{BRACKET_END}"))
            .await?;
        Ok(())
    }

    async fn key(&self, k: &str) -> anyhow::Result<()> {
        self.pane.send_key(k.to_string()).await?;
        Ok(())
    }

    async fn submit(&self, text: &str) -> anyhow::Result<()> {
        self.paste(text).await?;
        tokio::time::sleep(Duration::from_millis(120)).await;
        self.key("Enter").await
    }

    /// Poll until ANY of `pats` is visible; returns the one that matched. Menu text
    /// varies across TUI versions, so every wait in this spike is multi-pattern —
    /// the product's `detect.rules[].any` has the same shape for the same reason.
    async fn wait_visible_any(&self, pats: &[&str], timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            let t = self.text().await;
            if let Some(p) = pats.iter().find(|p| t.contains(**p)) {
                return Some((*p).to_string());
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
        None
    }

    /// Rows carrying the selection marker. CONFIRMED live: Claude Code marks the
    /// highlighted menu row with a leading `❯`, and that glyph is a real cell, so it
    /// survives `visible_lines()` (which drops SGR). This is what lets `agent_rewind`
    /// verify it is on the right row before committing — and abort when nothing
    /// matches the anchor.
    ///
    /// `after` scopes the search to rows below a header. CONFIRMED NECESSARY: `❯` is
    /// ALSO the composer prompt and the echo of each user turn in the scrollback, so
    /// an unscoped search returns transcript lines and would make the rewind driver
    /// think it was on a menu row it never selected.
    async fn highlighted(&self, after: Option<&str>) -> Vec<String> {
        let lines = self.lines().await;
        let start = match after {
            Some(h) => match lines.iter().position(|l| l.contains(h)) {
                Some(i) => i,
                None => return Vec::new(),
            },
            None => 0,
        };
        lines[start..]
            .iter()
            .filter(|l| l.contains('❯'))
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect()
    }

    /// Poll until the rendered screen stops changing for `quiet`. This is the
    /// spike's stand-in for the product's activity clock (which is fed by the
    /// output stream rather than by polling).
    async fn wait_settled(&self, quiet: Duration, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        let mut last = self.text().await;
        let mut last_change = Instant::now();
        while Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(150)).await;
            let now = self.text().await;
            if now != last {
                last = now;
                last_change = Instant::now();
            } else if last_change.elapsed() >= quiet {
                return;
            }
        }
    }

    /// The spinner row, if a turn is running — searched across the WHOLE screen.
    ///
    /// A fixed "last N rows" window looked natural and is WRONG: measured live, at
    /// +600ms and +1500ms into a turn the bottom 12 rows were entirely blank while
    /// the spinner rendered higher up. The TUI does not pin the spinner to the
    /// bottom, and where it lands depends on how much conversation is on screen.
    ///
    /// Screen-wide is safe here precisely because the rule is structural: the
    /// spinner row only exists while a turn runs, and the completed form
    /// (`✻ Baked for 2s`) does not match. A lexical marker like "esc to interrupt"
    /// could not be scanned screen-wide without false-positiving on quoted history.
    fn thinking_in(lines: &[String]) -> Option<String> {
        lines.iter().find(|l| is_thinking_line(l)).map(|l| l.trim().to_string())
    }

    async fn thinking_row(&self) -> Option<String> {
        Self::thinking_in(&self.lines().await)
    }

    /// Poll until a turn is visibly running.
    async fn wait_thinking(&self, timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if let Some(r) = self.thinking_row().await {
                return Some(r);
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        None
    }

    async fn kill(&self) {
        if let Ok(name) = SessionName::new(self.session_name.clone()) {
            if let Ok(s) = EnsureSession::named(name)
                .policy(EnsureSessionPolicy::ReuseOnly)
                .ensure(&self.rmux)
                .await
            {
                let _ = s.kill().await;
            }
        }
    }
}

/// Dump a labeled frame. These are the spike's primary artifact — the patterns in
/// `claude.toml` get written by reading them.
fn dump(label: &str, body: &str) {
    eprintln!("\n┌── FRAME: {label} {}", "─".repeat(50usize.saturating_sub(label.len())));
    for line in body.lines() {
        eprintln!("│ {line}");
    }
    eprintln!("└{}", "─".repeat(60));
}

fn head(label: &str) {
    eprintln!("\n════════════════════════════════════════════════════════════");
    eprintln!("  {label}");
    eprintln!("════════════════════════════════════════════════════════════");
}

// ---------------------------------------------------------------------------
// The spike
// ---------------------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn agent_tui_is_drivable_through_rmux() {
    if !spike_enabled() {
        eprintln!("[spike] RUN_CLAUDE_PTY_SPIKE != 1 — skipping live agent TUI spike.");
        return;
    }
    let Some(claude) = claude_bin() else {
        eprintln!("[spike] no claude binary — skipping.");
        return;
    };
    let Some(rmux_path) = rmux_bin() else {
        eprintln!("[spike] no rmux binary — skipping.");
        return;
    };
    std::env::set_var("RMUX_SDK_DAEMON_BINARY", &rmux_path);
    eprintln!("[spike] claude: {}", claude.display());
    eprintln!("[spike] rmux:   {}", rmux_path.display());

    let scratch = tempfile::tempdir().expect("tempdir");
    let cwd = scratch.path().to_path_buf();
    // A git repo, so the session looks like a real spwn worktree.
    let _ = std::process::Command::new("git").arg("init").arg("-q").current_dir(&cwd).status();
    std::fs::write(cwd.join("README.md"), "# spike\n").ok();
    eprintln!("[spike] cwd:    {}", cwd.display());

    let rmux = RmuxBuilder::new()
        .default_timeout(Duration::from_secs(30))
        .connect_or_start()
        .await
        .expect("connect to rmux daemon");

    // ---------------------------------------------------------------- Phase A
    head("A · assigned session id + launch");

    let session_id = uuid::Uuid::new_v4().to_string();
    eprintln!("[spike] assigning --session-id {session_id}");
    assert!(
        locate_session(&session_id).is_none(),
        "a transcript for a freshly generated uuid already exists — impossible"
    );

    // `--permission-mode manual` is deliberate: the user's saved default is `auto`,
    // which auto-approves tools and made phase G unobservable on the first run.
    // Passing it here also exercises the `[modes] launch` mapping.
    let argv = vec![
        claude.to_string_lossy().into_owned(),
        "--session-id".to_string(),
        session_id.clone(),
        "--permission-mode".to_string(),
        "manual".to_string(),
        // Ignore the developer's own ~/.claude allowlist. Without this, phase G's
        // tool call was silently auto-approved by a personal permission rule and the
        // "blocked" state was unobservable — a spike that only reproduces on a clean
        // machine is worthless.
        "--setting-sources".to_string(),
        "project".to_string(),
    ];
    eprintln!("[spike] argv: {argv:?}");

    let session_name = format!("spike-{}", session_id.replace('-', ""));
    let agent = AgentPane::launch(&rmux, &session_name, argv, &cwd, 120, 40)
        .await
        .expect("launch claude in an rmux pane");

    // The folder-trust prompt is NOT auto-accepted in the product (user decision),
    // but the spike must get past it to test anything else.
    //
    // CONFIRMED on the first live run — the wording is NOT "trust the files in this
    // folder" (that guess was wrong); it is a "Quick safety check" screen. These
    // three patterns are what a `[detect.trustPrompt]` would have to match if the
    // product ever grows an opt-in auto-accept.
    const TRUST_PATS: &[&str] = &[
        "Quick safety check",
        "Is this a project you created",
        "1. Yes, I trust this folder",
    ];
    if let Some(hit) = agent.wait_visible_any(TRUST_PATS, Duration::from_secs(10)).await {
        dump("A1 · folder-trust prompt", &agent.text().await);
        eprintln!("[spike] trust prompt matched on: {hit:?}");
        eprintln!("[spike] highlighted row(s): {:?}", agent.highlighted(None).await);
        eprintln!(
            "[spike] NOTE: this appears on EVERY fresh spwn worktree. Product decision \
             is that the user answers it; the spike accepts it to continue."
        );
        agent.key("Enter").await.ok();
        tokio::time::sleep(Duration::from_millis(800)).await;
    } else {
        eprintln!("[spike] no trust prompt (cwd already trusted).");
    }

    agent.wait_settled(Duration::from_millis(700), Duration::from_secs(40)).await;
    let ready_frame = agent.text().await;
    dump("A2 · ready / idle screen", &ready_frame);
    // Probes updated from the first live run's actual splash screen.
    for probe in [
        "Welcome back",
        "Claude Code v",
        "Tips for getting started",
        "/effort",
        "shift+tab to cycle",
    ] {
        eprintln!(
            "[spike] detect.ready probe {:26} present: {}",
            probe,
            ready_frame.contains(probe)
        );
    }
    // The permission-mode footer — the target of `[modes] line`.
    eprintln!(
        "[spike] mode footer line(s): {:?}",
        ready_frame
            .lines()
            .filter(|l| l.contains("mode on") || l.contains("shift+tab"))
            .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
            .collect::<Vec<_>>()
    );
    // Transcript persistence must be ON or nothing downstream works.
    let saving_off = ready_frame.contains("Transcript saving is off");
    eprintln!("[spike] ⚠ transcript saving OFF: {saving_off}");
    assert!(
        !saving_off,
        "transcript saving is disabled in this pane — the [env] fix for \
         CLAUDE_CODE_CHILD_SESSION did not take. Nothing downstream (session id, \
         turns, Timeline, rewind) can work without it."
    );
    assert!(
        !TRUST_PATS.iter().any(|p| ready_frame.contains(p)),
        "still on the trust screen after pressing Enter — nothing else can be tested"
    );

    // ---------------------------------------------------------------- Phase B/C
    head("B/C · bracketed paste, submit, thinking marker");

    // Deliberately a SLOW turn. The first run used a one-word reply that finished in
    // ~1s, which no realistic polling interval can catch mid-flight — the thinking
    // marker was missed not because it is absent but because the turn was too short.
    const SLOW_PROMPT: &str =
        "Write the numbers 1 through 30, each on its own line, as plain text. \
         Do not use any tools. End with the word: apple";

    agent.paste(SLOW_PROMPT).await.expect("paste");
    tokio::time::sleep(Duration::from_millis(500)).await;
    let composed = agent.text().await;
    dump("B1 · after bracketed paste, before Enter", &composed);
    assert!(
        composed.contains("numbers 1 through 30"),
        "bracketed paste did not reach the composer — the [input] paste mode needs \
         rethinking. Frame above."
    );
    eprintln!("[spike] ✓ bracketed paste reaches the composer, un-submitted");

    agent.key("Enter").await.expect("submit");

    // ONE loop that both samples and dumps.
    //
    // A previous version dumped mid-turn frames in a loop of sleeps and *then*
    // started sampling — but the dumps consumed 3.5s and the turn only lasted ~3s,
    // so the sampler always started after the turn was over and recorded "no signal
    // ever appeared". The frames it printed showed the spinner plainly. Sampling and
    // observing have to share one clock.
    let t0 = Instant::now();
    let mut dumps: Vec<u64> = vec![600, 1500, 3500];
    let mut spinner_rows: HashSet<String> = HashSet::new();
    let mut saw_footer = false;
    let mut saw_spinner = false;
    let deadline = t0 + Duration::from_secs(120);
    while Instant::now() < deadline {
        // ONE snapshot per iteration — the product's status watcher has the same
        // constraint, so the spike shouldn't model a cheaper world than exists.
        let lines = agent.lines().await;
        let spinner = AgentPane::thinking_in(&lines);
        let footer = lines.iter().any(|l| l.contains(FOOTER_RUNNING));
        saw_footer |= footer;
        if let Some(r) = spinner.clone() {
            saw_spinner = true;
            spinner_rows.insert(r);
        }

        let elapsed = t0.elapsed().as_millis() as u64;
        if dumps.first().is_some_and(|d| elapsed >= *d) {
            let at = dumps.remove(0);
            // FULL screen, not the tail — the tail was blank mid-turn and hid the spinner.
            dump(
                &format!("C0 · +{at}ms after Enter (full screen)"),
                &lines.join("\n"),
            );
        }

        // Terminate on the STATE, never on a content word: the prompt echo already
        // contains every sentinel we could ask the model to print, so a
        // `contains("apple")` check fires before the turn even begins.
        if saw_spinner && spinner.is_none() && !footer && lines.iter().any(|l| is_done_line(l)) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let mut rows: Vec<&String> = spinner_rows.iter().collect();
    rows.sort();
    eprintln!("[spike] distinct spinner rows observed mid-turn ({}):", rows.len());
    for r in &rows {
        eprintln!("[spike]   {r:?}");
    }
    eprintln!("[spike] footer '{FOOTER_RUNNING}' seen mid-turn: {saw_footer}");
    assert!(
        saw_spinner,
        "no thinking spinner detected during a multi-second turn — the structural \
         `is_thinking_line` rule is wrong; read the C0 frames above"
    );
    assert!(
        saw_footer,
        "footer '{FOOTER_RUNNING}' never appeared mid-turn — drop it from the \
         detect rules and rely on the spinner alone"
    );
    eprintln!(
        "[spike] ✓ TWO independent thinking signals confirmed: the structural spinner \
         row (glyph + word ending in '…') and the footer segment '{FOOTER_RUNNING}'. \
         Ship both under `any = [...]`."
    );

    // ---------------------------------------------------------------- Phase D
    head("D · done / idle detection");

    agent.wait_settled(Duration::from_millis(1500), Duration::from_secs(60)).await;
    let done_lines = agent.lines().await;
    let done_frame = done_lines.join("\n");
    dump("D1 · settled after turn (detect: done)", &done_frame);

    let still_thinking = AgentPane::thinking_in(&done_lines);
    let completion: Vec<String> = done_lines
        .iter()
        .filter(|l| is_done_line(l))
        .map(|l| l.trim().to_string())
        .collect();
    eprintln!("[spike] spinner cleared after turn: {}", still_thinking.is_none());
    eprintln!("[spike] footer still running: {}", done_frame.contains(FOOTER_RUNNING));
    eprintln!("[spike] completion row(s): {completion:?}");
    // The model was asked to end with "apple"; it is echoed in the prompt too, so
    // this is a weak check kept only as a smoke signal.
    eprintln!("[spike] 'apple' on screen: {}", done_frame.contains("apple"));
    assert!(
        still_thinking.is_none() && !done_frame.contains(FOOTER_RUNNING),
        "both thinking signals must clear once the turn settles, or `done` never fires"
    );
    assert!(
        !completion.is_empty(),
        "no completion row (`<glyph> <Verb> for <N>s`) after the turn — turn-boundary \
         detection from the screen has no positive signal"
    );

    // ---------------------------------------------------------------- Phase A'
    head("A' · transcript landed at the ASSIGNED uuid");

    let mut located = None;
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline {
        if let Some(p) = locate_session(&session_id) {
            located = Some(p);
            break;
        }
        tokio::time::sleep(Duration::from_millis(400)).await;
    }
    match &located {
        Some(p) => {
            let (n, uuids) = transcript_turns(p);
            eprintln!("[spike] transcript: {}", p.display());
            eprintln!("[spike] main-chain turns: {n}");
            eprintln!("[spike] turn uuids: {uuids:?}");
        }
        None => eprintln!("[spike] !! no transcript found for the assigned uuid"),
    }
    assert!(
        located.is_some(),
        "`--session-id` did not produce ~/.claude/projects/**/{session_id}.jsonl — \
         [session] idStrategy=\"assign\" is INVALID and the design needs a discover \
         strategy instead."
    );
    let transcript = located.unwrap();
    let (turns_before_rewind, uuids_before) = transcript_turns(&transcript);

    // ---------------------------------------------------------------- Phase E
    head("E · capture_pane reattach priming");

    let plain = agent.pane.capture_pane().await;
    let ansi = agent.pane.capture_pane().escape_ansi(true).await;
    let escaped = agent.pane.capture_pane().escape_sequences(true).await;
    for (label, res) in [
        ("plain", &plain),
        ("escape_ansi(true)", &ansi),
        ("escape_sequences(true)", &escaped),
    ] {
        match res {
            Ok(c) => eprintln!(
                "[spike] capture {label:24} -> {} bytes, contains 'apple': {}",
                c.stdout.len(),
                String::from_utf8_lossy(&c.stdout).contains("apple")
            ),
            Err(e) => eprintln!("[spike] capture {label:24} -> ERROR {e}"),
        }
    }
    eprintln!(
        "[spike] → the variant to use for xterm priming is the one whose bytes can be \
         written straight to the terminal: escape_ansi keeps SGR as real escapes, \
         escape_sequences OCTAL-ESCAPES them into literal text (wrong for priming)."
    );
    if let Ok(c) = &ansi {
        assert!(
            !c.stdout.is_empty(),
            "capture_pane returned nothing — reattach priming (M2) has no mechanism"
        );
    }

    // ---------------------------------------------------------------- Phase F
    head("F · Escape interrupts a running turn");

    agent
        .submit(
            "Write a detailed 600-word essay about the history of terminal multiplexers. \
             Do not use any tools.",
        )
        .await
        .expect("submit long task");
    let started = agent.wait_thinking(Duration::from_secs(30)).await;
    eprintln!("[spike] long turn started, spinner: {started:?}");
    assert!(started.is_some(), "long turn never showed a spinner — cannot test interrupt");

    tokio::time::sleep(Duration::from_secs(3)).await;
    agent.key("Escape").await.expect("interrupt");

    // Escape should clear the spinner well before the essay could finish.
    let mut stopped = false;
    let deadline = Instant::now() + Duration::from_secs(15);
    while Instant::now() < deadline {
        if agent.thinking_row().await.is_none() {
            stopped = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    agent.wait_settled(Duration::from_millis(1000), Duration::from_secs(20)).await;
    dump("F1 · after Escape (detect: done/interrupted)", &agent.tail_text(16).await);
    eprintln!("[spike] Escape cleared the spinner: {stopped}");
    assert!(stopped, "Escape did not stop the turn — [keys] interrupt is wrong");
    eprintln!("[spike] ✓ confirms [keys] interrupt = [\"Escape\"]");

    // ---------------------------------------------------------------- Phase G
    head("G · DISCOVERY — tool-permission prompt shape");
    eprintln!("[spike] This phase never fails the test. Read the frame and write the \
               substrings into [[detect.rules]] status=\"blockedPermission\".");

    // A file write is gated in `manual` mode even with no allowlist at all; `echo`
    // was not (the first attempt sailed straight through).
    agent
        .submit("Create a file called probe.txt containing the single word: spike")
        .await
        .expect("submit tool request");
    // A blocking prompt looks like: screen settles, but the spinner is gone AND the
    // turn never produced its result. That combination is the product's `blocked`
    // signal too, so assert the shape rather than just eyeballing it.
    agent.wait_settled(Duration::from_millis(1200), Duration::from_secs(90)).await;
    let perm_full = agent.text().await;
    let perm_tail = agent.tail_text(16).await;
    dump("G1 · full screen at suspected permission prompt", &perm_full);
    dump("G2 · last 16 rows, whitespace-normalized", &perm_tail);
    eprintln!(
        "[spike] menu highlight on this screen: {:?}",
        agent.highlighted(None).await
    );
    for probe in [
        "Do you want to",
        "Do you want me to",
        "1. Yes",
        "2. Yes, and",
        "3. No",
        "no, and tell Claude",
        "Create file",
        "Write",
        "esc to",
    ] {
        eprintln!("[spike] probe {:24} in last-16: {}", probe, perm_tail.contains(probe));
    }
    eprintln!(
        "[spike] spinner present while settled: {:?} (blocked ⇒ settled AND no spinner \
         AND no completion row)",
        agent.thinking_row().await
    );

    // Answer whatever is on screen so the session is usable for phase H.
    agent.key("Escape").await.ok();
    tokio::time::sleep(Duration::from_millis(600)).await;
    agent.wait_settled(Duration::from_millis(1000), Duration::from_secs(30)).await;

    // ---------------------------------------------------------------- Phase H
    head("H · DISCOVERY — /rewind menu choreography");
    eprintln!("[spike] The product plan drives this menu instead of writing JSONL. \
               Every frame below feeds [rewind.tui].");

    agent.paste("/rewind").await.ok();
    tokio::time::sleep(Duration::from_millis(300)).await;
    agent.key("Enter").await.ok();
    agent.wait_settled(Duration::from_millis(800), Duration::from_secs(20)).await;
    dump("H1 · /rewind checkpoint list", &agent.text().await);
    // Scope to the menu — `❯` also marks the composer and every echoed user turn.
    eprintln!("[spike] H1 menu highlight: {:?}", agent.highlighted(Some("Rewind")).await);
    for probe in [
        "Restore the code and/or conversation",
        "more above",
        "(current)",
        "No code changes",
        "Enter to continue",
    ] {
        let t = agent.text().await;
        eprintln!("[spike] rewind-menu probe {:38} present: {}", probe, t.contains(probe));
    }

    // CONFIRMED live: the list runs oldest → newest with `(current)` LAST, and the
    // selection starts on `(current)`. So reaching an earlier checkpoint means
    // pressing **Up**, not Down — the first run pressed Down and never moved, which
    // read as "arrow keys don't work". `↑ N more above` reports how many entries are
    // scrolled out of view, which is the natural bound for the driver's abort check.
    for i in 1..=4 {
        agent.key("Up").await.ok();
        tokio::time::sleep(Duration::from_millis(350)).await;
        eprintln!(
            "[spike] after {i}× Up, menu highlight: {:?}",
            agent.highlighted(Some("Rewind")).await
        );
        dump(&format!("H2.{i} · after {i}× Up"), &agent.tail_text(20).await);
    }

    eprintln!(
        "[spike] → The `❯` marker, SCOPED below the 'Rewind' header, is how \
         `agent_rewind` verifies it is on the right row BEFORE pressing Enter, and \
         how it aborts when no row matches the anchor."
    );

    // Select whatever row we're on, and dump the action menu (conversation vs files).
    agent.key("Enter").await.ok();
    agent.wait_settled(Duration::from_millis(800), Duration::from_secs(20)).await;
    dump("H3 · action menu (conversation only vs + files)", &agent.text().await);
    for probe in [
        "Restore conversation",
        "Restore code",
        "Restore both",
        "conversation only",
    ] {
        let t = agent.text().await;
        eprintln!("[spike] probe {:24} on action menu: {}", probe, t.contains(probe));
    }

    agent.key("Enter").await.ok();
    agent.wait_settled(Duration::from_millis(1200), Duration::from_secs(30)).await;
    dump("H4 · after restore", &agent.text().await);

    // ---------------------------------------------------------------- Wrap-up
    head("SUMMARY");

    let (turns_after, uuids_after) = transcript_turns(&transcript);
    eprintln!("[spike] transcript {}", transcript.display());
    eprintln!("[spike] main-chain turns before /rewind: {turns_before_rewind}");
    eprintln!("[spike] main-chain turns after  /rewind: {turns_after}");
    eprintln!(
        "[spike] session id unchanged by /rewind (same file still present): {}",
        locate_session(&session_id).is_some()
    );
    let before_set: HashSet<_> = uuids_before.iter().collect();
    let dropped = uuids_before.len() as i64
        - uuids_after.iter().filter(|u| before_set.contains(u)).count() as i64;
    eprintln!("[spike] turn uuids from before that survived the rewind chain: dropped={dropped}");
    eprintln!(
        "[spike] → If the file still exists at the SAME uuid, /rewind branches in place \
         and `agent_rewind` needs no bind_session and no checkpoint re-keying (plan §5)."
    );

    // Any NEW transcripts (would mean /rewind forked into a new session file).
    if let Ok(dir) = std::fs::read_dir(transcript.parent().unwrap()) {
        let siblings: Vec<String> = dir
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".jsonl"))
            .collect();
        eprintln!("[spike] transcripts in this project dir: {siblings:?}");
    }

    agent.kill().await;
    eprintln!("[spike] done — rmux session {session_name} killed.");
}

//! Per-project scheduled tasks: fire a headless, read-only Claude run on a
//! daily/weekly cadence, reusing the project's assembled context. Runs only while
//! the process is alive (the tray keeps it alive across window closes); on start
//! a missed occurrence is caught up exactly once.

use crate::claude::HeadlessEvent;
use crate::commands::{bind_session, persist, resolved_claude, worktrees_dir};
use crate::gitwt;
use crate::state::AppState;
use crate::store::{ContextBlock, ScheduledTask, TerminalRec};
use chrono::{DateTime, Datelike, Duration as ChronoDuration, Local, LocalResult, NaiveTime, TimeZone};
use std::path::{Path, PathBuf};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};
use uuid::Uuid;

/// Start the background scheduler: one task that ticks every 30s and fires any
/// due scheduled task. Uses Tauri's managed runtime (there is no tokio reactor on
/// the main thread during setup).
pub fn start_scheduler(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            tick(&app);
        }
    });
}

/// Check every project's tasks and fire the ones that are due.
fn tick(app: &AppHandle) {
    let now = Local::now();
    let state = app.state::<AppState>();

    // Collect due (project, task, scheduled-instant) without holding the lock across firing.
    let due: Vec<(String, String, i64)> = {
        let store = state.store.lock();
        let mut v = Vec::new();
        for p in &store.projects {
            for t in &p.scheduled_tasks {
                if !t.enabled {
                    continue;
                }
                if let Some(occ) = most_recent_occurrence(t, now) {
                    if occ > t.last_run.unwrap_or(i64::MIN) {
                        v.push((p.id.clone(), t.id.clone(), occ));
                    }
                }
            }
        }
        v
    };

    if due.is_empty() {
        return;
    }

    for (project_id, task_id, occ) in &due {
        // Record the scheduled instant as last_run BEFORE firing so a crash mid-run
        // (or the next 30s tick) can't double-fire this occurrence.
        {
            let mut store = state.store.lock();
            if let Some(p) = store.project_mut(project_id) {
                if let Some(t) = p.scheduled_tasks.iter_mut().find(|t| &t.id == task_id) {
                    t.last_run = Some(*occ);
                }
            }
        }
        fire(app, project_id, task_id);
    }
    persist(&state);
}

/// Fire one scheduled task now: create a session in the project, spawn a headless
/// read-only run seeded with (optionally) the project context + the task prompt.
/// Safe to call from the scheduler tick or a Run-now command.
pub fn fire(app: &AppHandle, project_id: &str, task_id: &str) {
    let state = app.state::<AppState>();

    // Guard against a second concurrent run of the same task.
    if !state.running_tasks.lock().insert(task_id.to_string()) {
        return;
    }

    // Under the store lock: load the task + project, and create the session record.
    let prepared: Option<(String, String, Vec<ContextBlock>, ScheduledTask)> = {
        let mut store = state.store.lock();
        store.project_mut(project_id).and_then(|project| {
            // No `enabled` gate here: the tick filters that, and manual Run-now
            // should work on a disabled task too.
            let task = project
                .scheduled_tasks
                .iter()
                .find(|t| t.id == task_id)
                .cloned()?;
            let directory = project.directory.clone();
            let context = project.context.clone();
            let terminal_id = Uuid::new_v4().to_string();
            project.terminals.push(TerminalRec {
                id: terminal_id.clone(),
                title: format!("◷ {}", task.name),
                kind: "claude".to_string(),
                cwd: directory.clone(),
                session_id: None,
                group_id: None,
                parent_id: None,
                branch: None,
                base_branch: None,
                needs_attention: false,
            });
            Some((terminal_id, directory, context, task))
        })
    };

    let Some((terminal_id, directory, context, task)) = prepared else {
        state.running_tasks.lock().remove(task_id);
        return;
    };
    persist(&state);

    // A scheduled run gets its own worktree+branch too, so it's isolated from (and
    // concurrency-safe with) any interactive session. Falls back to the project dir
    // on a non-git project or if the worktree can't be created. The worktree is kept
    // after the run (the flagged session stays viewable) and removed on delete.
    let mut run_dir = directory.clone();
    if let Some(repo) = gitwt::repo_root(Path::new(&directory)) {
        if let (Some(base), Some(wt_root)) = (gitwt::current_branch(&repo), worktrees_dir(&state)) {
            let short = terminal_id.split('-').next().unwrap_or(terminal_id.as_str());
            let branch = format!("cm/{short}");
            let wt_path = wt_root.join(&terminal_id);
            if gitwt::add_worktree(&repo, &wt_path, &branch, &base).is_ok() {
                gitwt::seed_heavy_dirs(Path::new(&directory), &wt_path);
                run_dir = wt_path.to_string_lossy().into_owned();
                {
                    let mut store = state.store.lock();
                    if let Some(t) = store.terminal_mut(&terminal_id) {
                        t.cwd = run_dir.clone();
                        t.branch = Some(branch);
                        t.base_branch = Some(base);
                    }
                }
                persist(&state);
            }
        }
    }

    // Assemble the first turn: project context (optional) then the task prompt.
    let mut first_turn = String::new();
    if task.use_context && !context.is_empty() {
        first_turn.push_str(&assemble_context(&context));
        first_turn.push_str("\n\n---\n\n");
    }
    first_turn.push_str(&task.prompt);

    let Some(claude_bin) = resolved_claude(&state) else {
        finalize(app, project_id, &terminal_id, task_id, false);
        return;
    };
    let cwd_path = std::fs::canonicalize(&run_dir).unwrap_or_else(|_| PathBuf::from(&run_dir));

    // Callback observes the sidecar: bind the session id on init, finalize on end.
    let app_cb = app.clone();
    let project_cb = project_id.to_string();
    let terminal_cb = terminal_id.clone();
    let task_cb = task_id.to_string();
    let agent = crate::claude::spawn_claude_agent_headless(
        app.clone(),
        &terminal_id,
        &cwd_path,
        &claude_bin,
        move |ev| match ev {
            HeadlessEvent::Init { session_id } => {
                let state = app_cb.state::<AppState>();
                bind_session(&state, &terminal_cb, &session_id);
                // Refresh the tree so the bound session (and its title) show up.
                let _ = app_cb.emit("projects://changed", Vec::<String>::new());
            }
            HeadlessEvent::Result { ok } => {
                finalize(&app_cb, &project_cb, &terminal_cb, &task_cb, ok);
            }
            HeadlessEvent::Error { message } => {
                eprintln!("scheduled run failed: {message}");
                finalize(&app_cb, &project_cb, &terminal_cb, &task_cb, false);
            }
        },
    );

    match agent {
        Ok(agent) => {
            // Register the agent BEFORE sending (send-before-insert = "no such session").
            state
                .claude_agents
                .lock()
                .insert(terminal_id.clone(), agent);
            let payload = serde_json::json!({ "t": "user", "text": first_turn }).to_string();
            if let Some(a) = state.claude_agents.lock().get_mut(&terminal_id) {
                let _ = a.send_json(&payload);
            }
        }
        Err(e) => {
            eprintln!("scheduled run spawn failed: {e}");
            finalize(app, project_id, &terminal_id, task_id, false);
        }
    }
}

/// Wind down a finished (or failed) scheduled run: flag the session for attention,
/// clear the running guard, tear down the agent, and notify the UI.
fn finalize(app: &AppHandle, project_id: &str, terminal_id: &str, task_id: &str, ok: bool) {
    let state = app.state::<AppState>();
    {
        let mut store = state.store.lock();
        if let Some(t) = store.terminal_mut(terminal_id) {
            t.needs_attention = true;
        }
    }
    persist(&state);
    state.running_tasks.lock().remove(task_id);
    if let Some(mut agent) = state.claude_agents.lock().remove(terminal_id) {
        agent.kill();
    }
    let _ = app.emit(
        "schedule://fired",
        serde_json::json!({ "projectId": project_id, "terminalId": terminal_id, "ok": ok }),
    );
}

/// Port of the frontend `ContextComposer.assemble()`: join context blocks into one
/// Markdown string (file → "## File: …", session → "## From a session (…)", note → raw).
fn assemble_context(blocks: &[ContextBlock]) -> String {
    blocks
        .iter()
        .map(|b| match b.kind.as_str() {
            "file" => format!("## File: {}\n\n{}", b.label, b.text),
            "session" => format!("## From a session ({})\n\n{}", b.label, b.text),
            _ => b.text.clone(),
        })
        .collect::<Vec<_>>()
        .join("\n\n---\n\n")
}

/// The single most-recent local instant (epoch ms) that matches the task's
/// weekday(s) + time and is at or before `now`, or None if none in the last week.
fn most_recent_occurrence(task: &ScheduledTask, now: DateTime<Local>) -> Option<i64> {
    let time = parse_hhmm(&task.time)?;
    // Walk back day-by-day (today first) to the most recent matching occurrence.
    for back in 0..8i64 {
        let day = (now - ChronoDuration::days(back)).date_naive();
        if !task.weekdays.is_empty() {
            let wd = day.weekday().num_days_from_sunday() as u8; // 0=Sun..6=Sat
            if !task.weekdays.contains(&wd) {
                continue;
            }
        }
        let naive = day.and_time(time);
        let dt = match Local.from_local_datetime(&naive) {
            LocalResult::Single(d) => d,
            LocalResult::Ambiguous(earlier, _) => earlier,
            LocalResult::None => continue, // spring-forward gap — no such local time
        };
        if dt <= now {
            return Some(dt.timestamp_millis());
        }
    }
    None
}

/// Parse "HH:MM" (24h) into a NaiveTime.
fn parse_hhmm(s: &str) -> Option<NaiveTime> {
    let (h, m) = s.split_once(':')?;
    NaiveTime::from_hms_opt(h.trim().parse().ok()?, m.trim().parse().ok()?, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn task(time: &str, weekdays: Vec<u8>, last_run: Option<i64>) -> ScheduledTask {
        ScheduledTask {
            id: "t".into(),
            name: "t".into(),
            prompt: "p".into(),
            time: time.into(),
            weekdays,
            enabled: true,
            use_context: true,
            last_run,
        }
    }

    fn at(y: i32, mo: u32, d: u32, h: u32, mi: u32) -> DateTime<Local> {
        Local
            .with_ymd_and_hms(y, mo, d, h, mi, 0)
            .single()
            .expect("valid local time in test")
    }

    #[test]
    fn daily_fires_after_the_time_passes() {
        // 2024-06-05 is a Wednesday.
        let t = task("09:00", vec![], None);
        // Before 09:00 → the occurrence is yesterday 09:00 (<= now), still returned.
        let before = most_recent_occurrence(&t, at(2024, 6, 5, 8, 0)).unwrap();
        assert_eq!(before, at(2024, 6, 4, 9, 0).timestamp_millis());
        // After 09:00 → today's 09:00.
        let after = most_recent_occurrence(&t, at(2024, 6, 5, 9, 30)).unwrap();
        assert_eq!(after, at(2024, 6, 5, 9, 0).timestamp_millis());
    }

    #[test]
    fn no_double_fire_and_single_catch_up() {
        let now = at(2024, 6, 5, 10, 0);
        let occ = at(2024, 6, 5, 9, 0).timestamp_millis();
        // Already ran this occurrence → not due (occ not > last_run).
        let t_ran = task("09:00", vec![], Some(occ));
        assert!(most_recent_occurrence(&t_ran, now).unwrap() <= occ);
        // A week's absence still yields exactly one occurrence (the most recent).
        let t_stale = task("09:00", vec![], Some(occ - 7 * 86_400_000));
        assert_eq!(most_recent_occurrence(&t_stale, now).unwrap(), occ);
    }

    #[test]
    fn weekly_only_matches_selected_weekdays() {
        // Fire only on Mondays (1). 2024-06-05 is Wed; most recent Monday is 06-03.
        let t = task("09:00", vec![1], None);
        let got = most_recent_occurrence(&t, at(2024, 6, 5, 12, 0)).unwrap();
        assert_eq!(got, at(2024, 6, 3, 9, 0).timestamp_millis());
    }

    #[test]
    fn bad_time_is_ignored() {
        assert!(parse_hhmm("nope").is_none());
        assert!(parse_hhmm("25:00").is_none());
        assert!(parse_hhmm("09:00").is_some());
    }
}

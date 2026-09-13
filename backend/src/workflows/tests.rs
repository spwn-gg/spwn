use super::*;
use crate::hooks::{HookPromptOption, HookPromptRequest, PromptMode};
use crate::store::{ProjectRec, TerminalRec, WorkflowSettings};
use serde_json::json;
use std::fs;

/// A project with workflows enabled, in a temp dir, on a real runtime.
struct Env {
    _tmp: tempfile::TempDir,
    data: PathBuf,
    dir: PathBuf,
    state: Arc<AppState>,
    _rt: tokio::runtime::Runtime,
}

const PROJECT: &str = "p1";

fn env() -> Env {
    let tmp = tempfile::tempdir().unwrap();
    let dir = fs::canonicalize(tmp.path()).unwrap().join("project");
    fs::create_dir_all(workflows_dir(&dir)).unwrap();
    let data = tmp.path().join("data");
    fs::create_dir_all(&data).unwrap();

    let state = Arc::new(AppState::default());
    *state.store_path.lock() = Some(data.join("projects.json"));
    // Never run the developer's own ~/.spwn/hooks from a test.
    state.settings.lock().global_hooks_enabled = false;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    state.workflows.init(rt.handle().clone());
    state.store.lock().projects.push(ProjectRec {
        id: PROJECT.into(),
        name: "Project".into(),
        directory: dir.to_string_lossy().into_owned(),
        terminals: Vec::new(),
        context: Vec::new(),
        scheduled_tasks: Vec::new(),
        workflows: WorkflowSettings {
            enabled: true,
            autostart: Vec::new(),
        },
    });
    Env { _tmp: tmp, data, dir, state, _rt: rt }
}

impl Env {
    fn write(&self, rel: &str, body: &str) {
        let p = workflows_dir(&self.dir).join(rel);
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, body).unwrap();
    }

    fn start(&self, name: &str, inputs: Value) -> RunInfo {
        start(&self.state, PROJECT, name, inputs, "manual").unwrap()
    }

    fn info(&self, run_id: &str) -> RunInfo {
        self.state.workflows.find_run(run_id).unwrap().info()
    }

    fn logs(&self, run_id: &str) -> Vec<String> {
        log(&self.state, run_id)
            .unwrap()
            .into_iter()
            .map(|l| l.msg)
            .collect()
    }

    /// Wait for a run to end, failing with its log if it doesn't.
    fn wait(&self, run_id: &str) -> RunInfo {
        self.wait_until(run_id, |i, _| !i.status.is_live())
    }

    fn wait_for_log(&self, run_id: &str, needle: &str) {
        self.wait_until(run_id, |_, logs| logs.iter().any(|l| l.contains(needle)));
    }

    fn wait_until(&self, run_id: &str, done: impl Fn(&RunInfo, &[String]) -> bool) -> RunInfo {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let info = self.info(run_id);
            let logs = self.logs(run_id);
            if done(&info, &logs) {
                return info;
            }
            assert!(
                Instant::now() < deadline,
                "timed out; run is {:?}, log:\n{}",
                info.status,
                logs.join("\n")
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[test]
fn only_top_level_scripts_are_workflows() {
    let e = env();
    for f in ["board.js", "triage.ts", "_shared.js", "spwn.d.ts", "lib/util.js", "notes.md"] {
        e.write(f, "export default () => {}");
    }
    let names: Vec<String> = discover(&workflows_dir(&e.dir)).into_iter().map(|(n, _)| n).collect();
    assert_eq!(names, vec!["board", "triage"]);
}

#[test]
fn meta_is_read_and_a_default_export_is_required() {
    let e = env();
    e.write(
        "board.js",
        r#"export const meta = { name: "Board", description: "Works tickets", keepAlive: true,
             inputs: { column: { default: "Todo" } } };
           export default async function main() {}"#,
    );
    e.write("broken.js", "export const meta = { name: 'nope' };");
    e.write("syntax.js", "export default (");

    let listing = list(&e.state, PROJECT).unwrap();
    assert!(listing.enabled);
    let by_name = |n: &str| listing.workflows.iter().find(|w| w.name == n).unwrap().clone();

    let board = by_name("board");
    let meta = board.meta.unwrap();
    assert_eq!(meta.name.as_deref(), Some("Board"));
    assert!(meta.keep_alive);
    assert_eq!(meta.inputs["column"]["default"], "Todo");
    assert_eq!(board.file, ".spwn/workflows/board.js");

    assert!(by_name("broken").error.unwrap().contains("export default"));
    assert!(by_name("syntax").error.is_some());
}

#[test]
fn a_run_logs_and_receives_its_inputs_with_defaults() {
    let e = env();
    e.write(
        "hello.js",
        r#"export const meta = { inputs: { who: { default: "world" }, n: {} } };
           export default async function main(spwn, inputs) {
             await new Promise((resolve) => setTimeout(resolve, 10));
             console.log(`hello ${inputs.who} ${inputs.n}`);
             spwn.warn("from", spwn.workflow.name, "in", spwn.project.name);
           }"#,
    );
    let run = e.start("hello", json!({ "n": 3 }));
    let info = e.wait(&run.id);
    assert_eq!(info.status, RunStatus::Finished, "{:?}", e.logs(&run.id));
    let logs = e.logs(&run.id);
    assert!(logs.contains(&"hello world 3".to_string()), "{logs:?}");
    assert!(logs.contains(&"from hello in Project".to_string()), "{logs:?}");
}

#[test]
fn a_typescript_workflow_runs_with_its_types_stripped() {
    let e = env();
    e.write(
        "typed.ts",
        r#"import type { Spwn } from "./spwn";
           import { double } from "./lib/math";
           enum Level { Low = 1, High = 2 }
           interface Inputs { who?: string }
           export const meta = { name: "Typed" };
           export default async function main(spwn: Spwn, inputs: Inputs): Promise<void> {
             const n: number = double(Level.High);
             spwn.log(`ts ${inputs.who ?? "anyone"} ${n}`);
           }"#,
    );
    e.write("lib/math.ts", "export const double = (n: number): number => n * 2;");
    e.write("bad.ts", "export default function (: number) {}");

    let listing = list(&e.state, PROJECT).unwrap();
    let typed = listing.workflows.iter().find(|w| w.name == "typed").unwrap();
    assert_eq!(typed.meta.as_ref().and_then(|m| m.name.as_deref()), Some("Typed"), "{:?}", typed.error);
    assert!(listing.workflows.iter().find(|w| w.name == "bad").unwrap().error.is_some());

    let run = e.start("typed", Value::Null);
    assert_eq!(e.wait(&run.id).status, RunStatus::Finished, "{:?}", e.logs(&run.id));
    assert!(e.logs(&run.id).contains(&"ts anyone 4".to_string()), "{:?}", e.logs(&run.id));
}

#[test]
fn new_workflows_start_from_a_loadable_template_with_types_beside_them() {
    let e = env();
    assert_eq!(scaffold(&e.state, PROJECT, "ask", false).unwrap(), ".spwn/workflows/ask.js");
    assert_eq!(scaffold(&e.state, PROJECT, "ask-ts", true).unwrap(), ".spwn/workflows/ask-ts.ts");
    assert!(workflows_dir(&e.dir).join("spwn.d.ts").is_file());
    assert!(scaffold(&e.state, PROJECT, "ask", true).unwrap_err().contains("already exists"));
    assert!(scaffold(&e.state, PROJECT, "../escape", false).is_err());

    let listing = list(&e.state, PROJECT).unwrap();
    let names: Vec<&str> = listing.workflows.iter().map(|w| w.name.as_str()).collect();
    assert_eq!(names, vec!["ask", "ask-ts"]);
    for w in &listing.workflows {
        assert_eq!(w.error, None, "{} should load", w.name);
    }
}

#[test]
fn the_github_board_example_loads() {
    let e = env();
    e.write(
        "github-board.ts",
        include_str!("../../../examples/workflows/github-board.ts"),
    );
    let listing = list(&e.state, PROJECT).unwrap();
    let w = &listing.workflows[0];
    assert_eq!(w.error, None);
    let meta = w.meta.as_ref().unwrap();
    assert_eq!(meta.name.as_deref(), Some("GitHub board"));
    assert!(meta.keep_alive);
    assert_eq!(meta.inputs["owner"]["required"], true);
}

#[test]
fn a_required_input_must_be_given() {
    let e = env();
    e.write(
        "needs.js",
        "export const meta = { inputs: { board: { required: true } } }; export default () => {}",
    );
    let err = start(&e.state, PROJECT, "needs", Value::Null, "manual").unwrap_err();
    assert!(err.contains("board"), "{err}");
}

#[test]
fn state_persists_across_runs() {
    let e = env();
    e.write(
        "count.js",
        r#"export default (spwn) => {
             const n = spwn.state.get("n", 0) + 1;
             spwn.state.set("n", n);
             console.log(`n=${n}`);
           }"#,
    );
    let first = e.start("count", Value::Null);
    e.wait(&first.id);
    let second = e.start("count", Value::Null);
    e.wait(&second.id);
    assert!(e.logs(&second.id).contains(&"n=2".to_string()), "{:?}", e.logs(&second.id));
    assert!(e.data.join("workflows").join(PROJECT).join("count.state.json").is_file());
}

#[test]
fn stopping_a_waiting_run_ends_it_promptly() {
    let e = env();
    e.write(
        "waits.js",
        r#"export default async (spwn) => {
             console.log("sleeping");
             try { await spwn.sleep(60_000); } catch (e) { console.log(`woke: ${e.code}`); }
           }"#,
    );
    let run = e.start("waits", Value::Null);
    e.wait_for_log(&run.id, "sleeping");
    let asked = Instant::now();
    stop(&e.state, &run.id).unwrap();
    let info = e.wait(&run.id);
    assert_eq!(info.status, RunStatus::Stopped);
    assert!(asked.elapsed() < Duration::from_secs(3));
}

#[test]
fn stopping_a_busy_loop_interrupts_it() {
    let e = env();
    e.write("spins.js", r#"export default () => { console.log("spinning"); for (;;) {} }"#);
    let run = e.start("spins", Value::Null);
    e.wait_for_log(&run.id, "spinning");
    stop(&e.state, &run.id).unwrap();
    assert_eq!(e.wait(&run.id).status, RunStatus::Stopped);
}

#[test]
fn a_throwing_workflow_fails_with_its_error() {
    let e = env();
    e.write("throws.js", r#"export default async () => { throw new Error("boom"); }"#);
    let run = e.start("throws", Value::Null);
    let info = e.wait(&run.id);
    assert_eq!(info.status, RunStatus::Failed);
    assert!(info.error.unwrap().contains("boom"));
}

#[test]
fn a_workflow_runs_once_at_a_time() {
    let e = env();
    e.write("waits.js", "export default (spwn) => spwn.untilStopped()");
    let run = e.start("waits", Value::Null);
    let err = start(&e.state, PROJECT, "waits", Value::Null, "manual").unwrap_err();
    assert!(err.contains("already running"), "{err}");
    stop(&e.state, &run.id).unwrap();
    e.wait(&run.id);
}

#[test]
fn keep_alive_restarts_a_crashing_workflow_until_stopped() {
    let e = env();
    e.write(
        "crashy.js",
        r#"export const meta = { keepAlive: true };
           export default () => { throw new Error("crash"); }"#,
    );
    let run = e.start("crashy", Value::Null);
    e.wait_until(&run.id, |i, _| i.restarts >= 2);
    stop(&e.state, &run.id).unwrap();
    assert_eq!(e.wait(&run.id).status, RunStatus::Stopped);
}

#[test]
fn imports_resolve_only_inside_the_workflows_dir() {
    let e = env();
    e.write("lib/greet.js", "export const greet = (n) => `hi ${n}`;");
    e.write(
        "uses_lib.js",
        r#"import { greet } from "./lib/greet.js";
           export default () => console.log(greet("lib"));"#,
    );
    fs::write(e.dir.join("outside.js"), "export const x = 1;").unwrap();
    e.write(
        "escapes.js",
        r#"import { x } from "../../outside.js";
           export default () => console.log(x);"#,
    );
    e.write("bare.js", r#"import fs from "fs"; export default () => {};"#);

    let ok = e.start("uses_lib", Value::Null);
    assert_eq!(e.wait(&ok.id).status, RunStatus::Finished, "{:?}", e.logs(&ok.id));
    assert!(e.logs(&ok.id).contains(&"hi lib".to_string()));

    let listing = list(&e.state, PROJECT).unwrap();
    let error = |n: &str| {
        listing
            .workflows
            .iter()
            .find(|w| w.name == n)
            .unwrap()
            .error
            .clone()
            .unwrap_or_default()
    };
    assert!(error("escapes").contains("outside"), "{}", error("escapes"));
    assert!(error("bare").contains("relative"), "{}", error("bare"));
}

#[test]
fn exec_runs_in_the_project_dir() {
    let e = env();
    e.write(
        "shell.js",
        r#"export default async (spwn) => {
             const r = await spwn.sh("pwd; echo oops >&2; exit 3");
             console.log(JSON.stringify(r));
           }"#,
    );
    let run = e.start("shell", Value::Null);
    e.wait(&run.id);
    let line = e.logs(&run.id).into_iter().find(|l| l.starts_with('{')).unwrap();
    let r: Value = serde_json::from_str(&line).unwrap();
    assert_eq!(r["code"], 3);
    assert_eq!(r["ok"], false);
    assert_eq!(r["stdout"].as_str().unwrap().trim(), e.dir.to_string_lossy());
    assert_eq!(r["stderr"].as_str().unwrap().trim(), "oops");
}

#[test]
fn fs_stays_inside_the_project() {
    let e = env();
    e.write(
        "files.js",
        r#"export default async (spwn) => {
             await spwn.fs.write("out/note.txt", "hi");
             console.log(await spwn.fs.read("out/note.txt"));
             try { await spwn.fs.read("../elsewhere"); } catch (err) { console.log(err.message); }
           }"#,
    );
    let run = e.start("files", Value::Null);
    e.wait(&run.id);
    let logs = e.logs(&run.id);
    assert_eq!(logs[0], "hi");
    assert!(logs[1].contains("outside the project"), "{logs:?}");
}

#[test]
fn disabled_projects_run_nothing() {
    let e = env();
    e.write("hello.js", "export default () => {}");
    set_enabled(&e.state, PROJECT, false).unwrap();
    let err = start(&e.state, PROJECT, "hello", Value::Null, "manual").unwrap_err();
    assert!(err.contains("not enabled"), "{err}");
}

#[test]
fn workflow_hooks_fire_around_a_run() {
    let e = env();
    e.write("hello.js", "export default () => {}");
    let hooks_dir = e.dir.join(".spwn").join("hooks");
    fs::create_dir_all(&hooks_dir).unwrap();
    fs::write(
        hooks_dir.join("workflow-started.sh"),
        "echo \"started $SPWN_WORKFLOW\"\n",
    )
    .unwrap();
    fs::write(
        hooks_dir.join("workflow-stopped.sh"),
        "echo \"stopped $SPWN_WORKFLOW $SPWN_WORKFLOW_STATUS\"\n",
    )
    .unwrap();
    let run = e.start("hello", Value::Null);
    e.wait(&run.id);
    // The stopped hook runs just after the run is marked finished.
    e.wait_for_log(&run.id, "stopped hello ok");
    assert!(e.logs(&run.id).contains(&"started hello".to_string()));
}

#[test]
fn a_hook_prompt_is_answered_by_the_workflow_that_owns_the_session() {
    let e = env();
    e.state.store.lock().projects[0].terminals.push(TerminalRec {
        id: "t1".into(),
        title: "s".into(),
        kind: "agent".into(),
        agent: Some("claude".into()),
        cwd: e.dir.to_string_lossy().into_owned(),
        session_id: None,
        group_id: None,
        parent_id: None,
        branch: None,
        base_branch: None,
        needs_attention: false,
        attention_reason: None,
        exec: None,
        workflow: None,
    });
    e.write(
        "answers.js",
        r#"export default async (spwn) => {
             const s = await spwn.sessions.get("t1", {
               onHookPrompt: (q) => `${q.event}:${q.options[1].label}`,
             });
             // Record fields must not shadow the methods beside them.
             console.log(`claimed ${s.id} ${await s.status()} ${s.title} ${s.key} ${typeof s.press}`);
             await spwn.untilStopped();
           }"#,
    );
    // Before any workflow claims it, a plain session's prompts go to the UI.
    assert!(matches!(prompt_mode_for(&e.state, "t1"), PromptMode::Ui));

    let run = e.start("answers", Value::Null);
    e.wait_for_log(&run.id, "claimed t1 idle s null function");
    let PromptMode::Workflow(ask) = prompt_mode_for(&e.state, "t1") else {
        panic!("the owning workflow should answer");
    };
    let request = HookPromptRequest {
        question: "Which?".into(),
        header: None,
        multi_select: false,
        options: ["A", "B"]
            .map(|l| HookPromptOption { label: l.into(), description: None })
            .to_vec(),
    };
    // The hook runner blocks on the answer, so ask from another thread, as it would.
    let answer = std::thread::spawn(move || ask("session-created", "t1", request))
        .join()
        .unwrap();
    assert_eq!(answer, "session-created:B");
    assert!(e.info(&run.id).sessions.contains(&"t1".to_string()));

    stop(&e.state, &run.id).unwrap();
    e.wait(&run.id);
    assert!(matches!(prompt_mode_for(&e.state, "t1"), PromptMode::Ui));
}

#[test]
fn spwn_on_hears_its_projects_session_events() {
    let e = env();
    e.write(
        "listens.js",
        r#"export default async (spwn) => {
             spwn.on("session-turn", (p) => console.log(`turn ${p.terminalId}`));
             console.log("listening");
             await spwn.untilStopped();
           }"#,
    );
    let run = e.start("listens", Value::Null);
    e.wait_for_log(&run.id, "listening");
    let fired = |project: &str, tid: &str| {
        e.state.hub.emit(
            "hooks://fired",
            json!({ "event": "session-turn", "projectId": project, "terminalId": tid }),
        );
    };
    fired("another-project", "t0");
    fired(PROJECT, "t9");
    e.wait_for_log(&run.id, "turn t9");
    assert!(!e.logs(&run.id).iter().any(|l| l == "turn t0"));
    stop(&e.state, &run.id).unwrap();
    e.wait(&run.id);
}

//! OfficeState.cs's `state` message: the names and their order are the page's contract
//! (web/office reads them), so they are checked here against the C# anonymous objects,
//! and the rows of the timeline against what Row() wrote.

use hover_agents::agents::AgentReady;
use hover_agents::ask::{AgentAsk, AgentQuestion};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::spaces;
use hover_agents::stream::{KiroEvent, KiroPhase, KiroResult};
use hover_backend::office::{self, Ctx};
use hover_core::json::Json;
use hover_core::model::{AgentTool, KiroState, KiroStep};
use hover_core::settings::Settings;
use std::sync::Arc;
use std::time::{Duration, Instant};

fn keys(j: &Json) -> Vec<&str> { j.props().unwrap().iter().map(|(k, _)| k.as_str()).collect() }

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hover-backend-office-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn step(kind: &str, title: &str, target: Option<&str>, input: Option<&str>) -> KiroStep {
    KiroStep { input: input.map(str::to_owned), ..KiroStep::new(&format!("{kind}-{title}"), kind, title, target.map(str::to_owned), "completed") }
}

/// A run that reports these steps and completes with an answer.
fn runner(steps: Vec<KiroStep>) -> RunTask {
    Arc::new(move |a: RunArgs| {
        (a.progress)(KiroPhase::Reading);
        for s in &steps { (a.events)(KiroEvent { step: Some(s.clone()), ..Default::default() }); }
        (a.events)(KiroEvent { context: Some(12.5), session_id: Some("s-1".into()), credits: Some(0.25), ..Default::default() });
        KiroResult::new(KiroState::Completed, "# Done\n\nIt works.")
    })
}

fn one_session(dir: &std::path::Path, steps: Vec<KiroStep>) -> (KiroSessions, KiroSession) {
    let sessions = KiroSessions::new(move |_| runner(steps.clone()), None);
    let s = sessions.start_as(AgentTool::Codex, &dir.to_string_lossy(), "Fix the login", vec![], Some("risky")).expect("a session");
    let end = Instant::now() + Duration::from_secs(10);
    while sessions.get(s.id).is_some_and(|s| s.busy()) && Instant::now() < end { std::thread::sleep(Duration::from_millis(10)); }
    let done = sessions.get(s.id).unwrap();
    assert_eq!(done.state, KiroState::Completed);
    (sessions, done)
}

fn known(t: AgentTool) -> Option<AgentReady> {
    match t {
        AgentTool::Codex => Some(AgentReady { installed: true, signed_in: true, hint: String::new() }),
        AgentTool::Cursor => Some(AgentReady { installed: true, signed_in: false, hint: "Sign in: run “cursor-agent login” in a terminal.".into() }),
        _ => None,
    }
}

#[test]
fn the_state_has_the_names_of_the_c_sharp_snapshot_in_its_order() {
    let d = temp("names");
    let (sessions, _) = one_session(&d, vec![step("read", "Read File", Some("src/a.rs"), None)]);
    let settings = Settings::load(d.join("settings.json"));
    settings.set_kiro_folder(Some(&d.to_string_lossy()));
    let all = sessions.all();
    let ctx = Ctx { settings: &settings, sessions: &all, history: &[], can_start: true, max_running: 3, checkpoints: false, known: &known };
    let state = office::snapshot(&ctx);
    assert_eq!(keys(&state), ["type", "canStart", "maxRunning", "spaces", "folder", "tool", "tools", "sessions", "history"]);
    assert_eq!(state.get("spaces"), Some(&Json::Bool(false)), "agent desktops are off, here and wherever Cua isn't");
    let tools = state.get("tools").unwrap().items().unwrap();
    assert_eq!(tools.iter().map(|t| t.get("id").unwrap().as_str().unwrap()).collect::<Vec<_>>(), ["kiro", "codex", "cursor", "opencode", "claude"]);
    assert_eq!(keys(&tools[0]), ["id", "name", "ready", "hint", "checkedYet", "installed", "signedIn", "canSetup", "setup", "access", "readOnly", "hideSteps",
        "models", "model", "efforts", "effort", "effortLabel", "questions"]);
    assert_eq!(keys(tools[0].get("setup").unwrap()), ["step", "line", "error", "busy", "needs"]);
    assert_eq!(keys(&tools[0].get("models").unwrap().items().unwrap()[0]), ["id", "name", "levels"]);
    // Unchecked: not ready, "Checking installation…"; checked: as Agents.Known says.
    assert_eq!(tools[0].get("ready"), Some(&Json::Bool(false)));
    assert_eq!(tools[0].get("hint").unwrap().as_str(), Some("Checking installation…"));
    assert_eq!(tools[1].get("ready"), Some(&Json::Bool(true)));
    assert_eq!(tools[2].get("installed"), Some(&Json::Bool(true)));
    assert_eq!(tools[2].get("signedIn"), Some(&Json::Bool(false)));
    assert_eq!(tools[2].get("ready"), Some(&Json::Bool(false)));
    // Claude Code is a tool like the others, with its own effort word.
    assert_eq!(tools[4].get("id").unwrap().as_str(), Some("claude"));
    assert_eq!(tools[4].get("name").unwrap().as_str(), Some("Claude Code"));
    assert_eq!(tools[3].get("effortLabel").unwrap().as_str(), Some("Variant"));
    assert_eq!(tools[1].get("questions"), Some(&Json::Bool(false)));
    // Kiro's own models before it has run start with its Auto, so no Default is added; a tool that listed none has only Default.
    let models = tools[0].get("models").unwrap().items().unwrap();
    assert_eq!(models[0].compact(), r#"{"id":"auto","name":"Auto","levels":null}"#);
    assert_eq!(models.len(), 14);
    assert_eq!(tools[1].get("models").unwrap().compact(), r#"[{"id":"","name":"Default","levels":null}]"#);

    let s = &state.get("sessions").unwrap().items().unwrap()[0];
    assert_eq!(keys(s), ["id", "key", "files", "tool", "bot", "seat", "title", "folder", "ctx", "access", "stage", "stopping", "act", "ask", "pose", "file",
        "testing", "space", "browsing", "apps", "turns"]);
    assert_eq!(s.get("space"), Some(&Json::Null), "no desktop of its own while agent desktops are off");
    assert!(s.get("files").unwrap().as_str().unwrap().starts_with("hover://files/"));
    assert_eq!(s.get("access").unwrap().as_str(), Some("risky"));
    assert_eq!(s.get("stage").unwrap().as_str(), Some("done"));
    assert_eq!(s.get("ctx").unwrap().compact(), "12");
    assert_eq!(s.get("ask"), Some(&Json::Null));
    assert_eq!(s.get("apps"), Some(&Json::Null));
    assert_eq!(s.get("file").unwrap().as_str(), Some("a.rs"));
    let turn = &s.get("turns").unwrap().items().unwrap()[0];
    assert_eq!(keys(turn), ["prompt", "images", "queued", "stage", "steps", "answer", "t0", "woke", "took", "credits", "restore", "again"]);
    assert_eq!(turn.get("answer").unwrap().as_str(), Some("# Done\n\nIt works."));
    assert_eq!(turn.get("credits").unwrap().compact(), "0.25");
    assert_eq!(turn.get("restore"), Some(&Json::Bool(false)), "no checkpoints: nothing to restore");
    // The compact form is a single line of JSON (the protocol is lines).
    assert!(!state.compact().contains('\n'));
}

#[test]
fn a_session_names_the_project_desktop_it_shares() {
    let d = temp("space");
    let (_sessions, s) = one_session(&d, vec![]);
    let mut mate = s.clone();
    mate.id += 1;
    let mut elsewhere = s.clone();
    elsewhere.id += 2;
    elsewhere.folder = d.join("other").to_string_lossy().into_owned();
    let all = vec![s.clone(), mate.clone(), elsewhere];
    let name = spaces::name_for(&s.folder);

    // Nothing asked of it yet: phase "none", and the other agent in the same folder shares it.
    let none = office::space(&s, &all, None);
    assert_eq!(keys(&none), ["name", "project", "phase", "line", "fraction", "error", "with"]);
    assert_eq!(none.compact(), format!(r#"{{"name":"{name}","project":"{}","phase":"none","line":"","fraction":null,"error":null,"with":[{}]}}"#,
        d.file_name().unwrap().to_string_lossy(), mate.id));
    assert!(name.starts_with("hover-hover-backend-office-") && name.len() == "hover-hover-backend-office-".len() + 6, "{name}: 20 letters of the folder's name, then the hash");

    let state = spaces::SpaceState { phase: "creating".into(), line: "Downloading the desktop image…".into(), fraction: Some(0.5), error: None };
    let creating = office::space(&mate, &all, Some(&state));
    assert_eq!((creating.get("phase"), creating.get("line"), creating.get("fraction").map(Json::compact)), (Some(&Json::str("creating")), Some(&Json::str("Downloading the desktop image…")), Some("0.5".to_owned())));
    assert_eq!(creating.get("with").unwrap().compact(), format!("[{}]", s.id), "each knows it shares the desktop with the other");

    let failed = spaces::SpaceState { phase: "failed".into(), line: String::new(), fraction: None, error: Some("No room.".into()) };
    assert_eq!(office::space(&s, &all[..1], Some(&failed)).get("error"), Some(&Json::str("No room.")));
    assert_eq!(office::space(&s, &all[..1], None).get("with").unwrap().compact(), "[]");
}

#[test]
fn restore_and_again_are_offered_where_checkpoints_were_kept() {
    let d = temp("restore");
    let (_sessions, mut s) = one_session(&d, vec![]);
    s.turns[0].before = Some("tree-before".into());
    s.turns[0].after = Some("tree-after".into());
    let mut second = s.turns[0].clone();
    second.after = None;
    s.turns.push(second);
    let settings = Settings::load(d.join("settings.json"));
    let turns_of = |checkpoints: bool| {
        let all = vec![s.clone()];
        let ctx = Ctx { settings: &settings, sessions: &all, history: &[], can_start: true, max_running: 3, checkpoints, known: &known };
        let j = office::snapshot(&ctx);
        j.get("sessions").unwrap().items().unwrap()[0].get("turns").unwrap().items().unwrap().iter()
            .map(|t| (t.get("restore").unwrap().clone(), t.get("again").unwrap().clone())).collect::<Vec<_>>()
    };
    assert_eq!(turns_of(true), [(Json::Bool(true), Json::Bool(true)), (Json::Bool(false), Json::Bool(true))]);
    // Without checkpoints (no git) neither is offered.
    assert_eq!(turns_of(false), [(Json::Bool(false), Json::Bool(false)), (Json::Bool(false), Json::Bool(false))]);
}

#[test]
fn a_session_whose_folder_is_gone_has_no_files_address() {
    let d = temp("gone");
    let (sessions, s) = one_session(&d, vec![]);
    std::fs::remove_dir_all(&d).unwrap();
    let settings = Settings::load(std::env::temp_dir().join(format!("hover-backend-office-gone-{}.json", std::process::id())));
    let all = vec![s];
    let ctx = Ctx { settings: &settings, sessions: &all, history: &[], can_start: false, max_running: 3, checkpoints: true, known: &known };
    let state = office::snapshot(&ctx);
    let s = &state.get("sessions").unwrap().items().unwrap()[0];
    assert_eq!(s.get("files"), Some(&Json::Null));
    assert_eq!(state.get("canStart"), Some(&Json::Bool(false)));
    drop(sessions);
}

#[test]
fn the_timeline_rows_are_what_row_wrote() {
    let f = if cfg!(windows) { r"C:\p" } else { "/p" };
    let inside = if cfg!(windows) { r"c:\P\src\a.ts" } else { "/p/src/a.ts" };
    let row = |x: KiroStep| office::row(&x, f).compact();
    // A file read: its name bright, its folder dim.
    assert_eq!(row(step("read", "Read File", Some(inside), None)),
        r#"{"k":"read","verb":"Read","name":"a.ts","dir":"src","cmd":null,"status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}"#);
    // A command: the command, and the exit code it printed.
    let mut run = step("execute", "Run", Some("npm\ntest"), None);
    run.output = Some("ok".into());
    run.exit = Some(1);
    assert_eq!(row(run), r#"{"k":"run","verb":"Ran","name":null,"dir":null,"cmd":"npm test","status":"completed","add":0,"del":0,"diff":null,"out":"ok","exit":1,"ms":null}"#);
    // What the tool thought folds under a short row.
    let mut thought = step("thought", "Thinking", None, None);
    thought.output = Some("Plan: run the build.".into());
    assert_eq!(row(thought), r#"{"k":"thought","verb":"Thought","status":"completed","out":"Plan: run the build.","ms":null}"#);
    // A subagent: its task and what it came back with (cut at 2000).
    let mut agent = step("other", "Task", None, Some(r#"{"subagent_type":"explore","description":"Find the config","prompt":"Look"}"#));
    agent.log = Some("x".repeat(2500));
    let r = office::row(&agent, f);
    assert_eq!(r.props().unwrap().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["k", "verb", "agent", "cmd", "status", "out", "ms"]);
    assert_eq!(r.get("agent").unwrap().as_str(), Some("explore"));
    assert_eq!(r.get("cmd").unwrap().as_str(), Some("Find the config"));
    assert_eq!(r.get("out").unwrap().as_str().unwrap().chars().count(), 2001);
    // A task tool that arrives as kind "agent" (Claude Code's and OpenCode's) is one too.
    assert_eq!(office::row(&step("agent", "Task", None, None), f).get("k").unwrap().as_str(), Some("agent"));
    // Hover's browser: what it did, and where.
    let web = row(step("other", "hover-browser/browser_open", None, Some(r#"{"url":"http://localhost:5173/"}"#)));
    assert_eq!(web, r#"{"k":"web","verb":"Opened","cmd":"http://localhost:5173/","status":"completed","out":null,"ms":null}"#);
    let click = row(step("other", "mcp__hover-browser__browser_click", None, Some(r#"{"ref":4}"#)));
    assert_eq!(click, r#"{"k":"web","verb":"Clicked","cmd":"[4]","status":"completed","out":null,"ms":null}"#);
    let typed = row(step("other", "browser_type", None, Some(r#"{"text":"hello"}"#)));
    assert!(typed.contains(r#""verb":"Typed","cmd":"hello""#), "{typed}");
    assert!(row(step("other", "browser_snapshot", None, None)).contains(r#""verb":"Read the page","cmd":null"#));
    // Computer use (Arz's 760d71e): the screen panel's activity.
    let mut screen = step("other", "cua-driver: click", None, Some(r#"{"label":"Sign in"}"#));
    screen.output = Some("o".repeat(700));
    let r = office::row(&screen, f);
    assert_eq!(r.props().unwrap().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["k", "verb", "cmd", "status", "out", "ms"]);
    assert_eq!((r.get("k").unwrap().as_str(), r.get("verb").unwrap().as_str(), r.get("cmd").unwrap().as_str()), (Some("screen"), Some("Clicked"), Some("Sign in")));
    assert_eq!(r.get("out").unwrap().as_str().unwrap().chars().count(), 601);
    // An edit carries its change.
    let mut edit = step("edit", "Edit", Some(inside), None);
    edit.added = 2;
    edit.removed = 1;
    edit.diff = Some("+a\n-b".into());
    assert!(row(edit).contains(r#""k":"edit","verb":"Edited","name":"a.ts","dir":"src""#));
    // A fetch has its address; an unknown kind only its title.
    assert!(row(step("fetch", "Fetch", Some("https://example.com/docs"), None)).contains(r#""k":"search","verb":"Fetched","name":null,"dir":null,"cmd":"https://example.com/docs""#));
    assert!(row(step("other", "Working", None, None)).contains(r#""k":"think","verb":"Working""#));
}

#[test]
fn a_question_and_a_tool_call_ask_as_the_page_reads_them() {
    let mut s = KiroSession::new(AgentTool::OpenCode);
    s.folder = std::env::temp_dir().to_string_lossy().into_owned();
    s.state = KiroState::Running;
    s.phase = KiroPhase::Running;
    s.asks.push(AgentAsk { id: "a1".into(), kind: "execute".into(), title: "Run".into(), command: Some("npm install".into()), path: None, preview: None,
        added: 0, removed: 0, reason: "Runs a command".into(), danger: false, questions: None });
    let mut q = KiroSession::new(AgentTool::OpenCode);
    q.folder = s.folder.clone();
    q.state = KiroState::Running;
    q.asks.push(AgentAsk { id: "a2".into(), kind: "question".into(), title: "Pick".into(), command: None, path: None, preview: None, added: 0, removed: 0,
        reason: String::new(), danger: false, questions: Some(vec![AgentQuestion { header: "Style".into(), question: "Tabs or spaces?".into(),
            options: vec![("Tabs".into(), "Wider".into())], multiple: false, custom: true }]) });
    let settings = Settings::load(std::env::temp_dir().join(format!("hover-backend-office-ask-{}.json", std::process::id())));
    let all = vec![s, q];
    let ctx = Ctx { settings: &settings, sessions: &all, history: &[], can_start: true, max_running: 3, checkpoints: false, known: &known };
    let j = office::snapshot(&ctx);
    let both = j.get("sessions").unwrap().items().unwrap();
    let s = &both[0];
    assert_eq!(s.get("stage").unwrap().as_str(), Some("waiting"));
    assert_eq!(s.get("act").unwrap().as_str(), Some("Running"));
    assert_eq!(s.get("pose").unwrap().as_str(), Some("Running"));
    let ask = s.get("ask").unwrap();
    assert_eq!(keys(ask), ["id", "kind", "title", "line", "command", "path", "preview", "added", "removed", "reason", "danger", "allow", "more", "questions"]);
    assert_eq!(ask.get("line").unwrap().as_str(), Some("Wants to run npm install"));
    // The reason that only names the kind of call is left out (AskWhy).
    assert_eq!(ask.get("reason").unwrap().as_str(), Some(""));
    assert_eq!(ask.get("more").unwrap().compact(), "0");
    assert_eq!(ask.get("allow").unwrap().as_str(), Some("Run"));
    assert_eq!(ask.get("questions"), Some(&Json::Null));
    // A question carries its choices for the page's buttons.
    let qa = both[1].get("ask").unwrap();
    assert_eq!(qa.get("kind").unwrap().as_str(), Some("question"));
    assert_eq!(qa.get("allow").unwrap().as_str(), Some("Answer"));
    assert_eq!(qa.get("questions").unwrap().compact(),
        r#"[{"header":"Style","question":"Tabs or spaces?","options":[{"label":"Tabs","description":"Wider"}],"multiple":false,"custom":true}]"#);
    // AskWhy adds the lines a change makes, for anything but an edit (whose card shows them).
    let mut a = AgentAsk { id: "x".into(), kind: "delete".into(), title: String::new(), command: None, path: None, preview: None, added: 1, removed: 3,
        reason: "Deletes outside the folder".into(), danger: true, questions: None };
    assert_eq!(office::ask_why(&a), "Deletes outside the folder · +1 −3");
    a.reason = "Deletes files".into();
    assert_eq!(office::ask_why(&a), "+1 −3");
    a.kind = "edit".into();
    a.reason = "Edits a file".into();
    assert_eq!(office::ask_why(&a), "");
}

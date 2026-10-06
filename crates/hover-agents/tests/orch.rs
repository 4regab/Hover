//! Orchestration: a lead asks other agents for help through Hover. These run the real sessions and the real
//! orchestrator against scripted agents; the one with worktrees runs the real git.

use hover_agents::orch::{self, Delegate, Delivery, Env, Orch, Provider, RunState};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_core::crypto::Crypto;
use hover_core::ext::{OrchLink, SessionExt};
use hover_core::history::AgentHistory;
use hover_core::json::Json;
use hover_core::model::{AgentTool, DelegationLimits, KiroState};
use hover_core::store::Sealed;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hover-orch-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Without Windows' \\?\ prefix, which git can't take in a path.
    PathBuf::from(std::fs::canonicalize(&d).unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
}

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

struct Stub { limits: Mutex<DelegationLimits>, worktrees: PathBuf, access: Mutex<String> }

impl Env for Stub {
    fn providers(&self) -> Vec<Provider> {
        let p = |id: &str, tool, ready: bool, leads: bool| Provider { id: id.into(), name: id.to_uppercase(), tool, instance: None, ready, hint: if ready { String::new() } else { "sign in first".into() }, read_only: true, resume: true, leads };
        vec![p("kiro", AgentTool::Kiro, true, true), p("codex", AgentTool::Codex, true, true), p("claude", AgentTool::Claude, false, true), p("opencode", AgentTool::OpenCode, true, false)]
    }
    fn access_of(&self, s: &KiroSession) -> String { s.access.clone().unwrap_or_else(|| self.access.lock().unwrap().clone()) }
    fn limits(&self) -> DelegationLimits { *self.limits.lock().unwrap() }
    fn worktrees(&self) -> PathBuf { self.worktrees.clone() }
}

/// What a scripted agent does: it gets its run arguments and the orchestrator.
type Script = Arc<dyn Fn(&RunArgs, &Arc<Orch>) -> KiroResult + Send + Sync>;

struct Rig {
    k: KiroSessions,
    orch: Arc<Orch>,
    env: Arc<Stub>,
    root: PathBuf,
    folder: String,
    release: Arc<Mutex<bool>>,
    crypto: Arc<Crypto>,
}

/// Blocks until released or stopped: the "[hold]" helper.
fn hold(a: &RunArgs, release: &Arc<Mutex<bool>>) -> KiroResult {
    while !*release.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(10)); }
    if a.ct.is_cancelled() { KiroResult::new(KiroState::Cancelled, "Stopped.") } else { KiroResult::new(KiroState::Completed, "Held, then done.") }
}

fn rig(name: &str, limits: DelegationLimits, script: impl Fn(&RunArgs, &Arc<Orch>, &Arc<Mutex<bool>>) -> KiroResult + Send + Sync + 'static) -> Rig {
    let root = dir(name);
    let folder = root.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let crypto = Arc::new(Crypto::with_key([9; 32]));
    let release = Arc::new(Mutex::new(false));
    let cell: Arc<OnceLock<Arc<Orch>>> = Arc::new(OnceLock::new());
    let (c2, r2) = (cell.clone(), release.clone());
    let script: Script = Arc::new(move |a, o| script(a, o, &r2));
    let k = KiroSessions::new(move |_| -> RunTask {
        let (cell, script) = (c2.clone(), script.clone());
        Arc::new(move |a: RunArgs| { let o = cell.get().expect("the orchestrator").clone(); script(&a, &o) })
    }, Some(Arc::new(AgentHistory::new(root.join("history"), crypto.clone()))));
    let env = Arc::new(Stub { limits: Mutex::new(limits), worktrees: root.join("worktrees"), access: Mutex::new("full".into()) });
    let orch = Orch::new(k.clone(), env.clone(), Some(Sealed::in_dir(&root.join("orch"), "runs", crypto.clone())));
    cell.set(orch.clone()).ok();
    Rig { k, orch, env, folder: folder.to_string_lossy().into_owned(), root, release, crypto }
}

fn link() -> SessionExt { SessionExt { orch: Some(OrchLink { delegation: true, ..Default::default() }), ..Default::default() } }

impl Rig {
    fn lead(&self, prompt: &str) -> KiroSession { self.k.start_bound(AgentTool::Kiro, &self.folder, prompt, vec![], None, None, link()).expect("a lead") }
    fn ask(&self, lead: &str, provider: &str, brief: &str, request: Option<&str>) -> Result<orch::Info, String> {
        self.orch.delegate(lead, Delegate { provider: provider.into(), brief: brief.into(), request: request.map(str::to_owned), ..Default::default() })
    }
    fn release(&self) { *self.release.lock().unwrap() = true; }
}

#[test]
fn a_lead_delegates_waits_and_gets_the_result_and_the_record_survives_a_restart() {
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let got: Arc<Mutex<Option<String>>> = Default::default();
    let (s2, g2) = (seen.clone(), got.clone());
    let r = rig("basic", DelegationLimits::default(), move |a, o, _| {
        if a.prompt.starts_with("[Hover helper task]") { s2.lock().unwrap().push(a.prompt.clone()); return KiroResult::new(KiroState::Completed, "The answer is 42."); }
        let tag = a.tag.clone().unwrap();
        let run = o.delegate(&tag, Delegate { provider: "codex".into(), brief: "Work out the answer.".into(), role: Some("mathematician".into()), request: Some("req-1".into()), ..Default::default() }).unwrap();
        let again = o.delegate(&tag, Delegate { provider: "codex".into(), brief: "Work out the answer.".into(), request: Some("req-1".into()), ..Default::default() }).unwrap();
        assert_eq!(again.run, run.run, "a retry with the same request id makes no second helper");
        let done = o.wait(&tag, &run.run, Duration::from_secs(10)).unwrap();
        *g2.lock().unwrap() = done.result.clone();
        KiroResult::new(KiroState::Completed, &format!("Helper said: {}", done.result.unwrap_or_default()))
    });
    let lead = r.lead("SECRET-LEAD-CONTEXT: please get the answer");
    wait_for("the lead to finish", || r.k.get(lead.id).is_some_and(|s| !s.busy()));
    assert_eq!(got.lock().unwrap().as_deref(), Some("The answer is 42."));
    assert_eq!(r.k.get(lead.id).unwrap().result().unwrap().text, "Helper said: The answer is 42.");
    // One helper, with the brief and role, and nothing of the lead's own conversation.
    let prompts = seen.lock().unwrap().clone();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("Work out the answer.") && prompts[0].contains("mathematician") && !prompts[0].contains("SECRET-LEAD-CONTEXT"), "{}", prompts[0]);
    assert_eq!(r.k.all().len(), 2);
    let helpers = r.orch.helpers_of(&lead.key);
    assert_eq!((helpers.len(), helpers[0].state, helpers[0].delivery), (1, RunState::Done, Delivery::Taken));
    let hs = r.k.find(helpers[0].session.as_ref().unwrap()).unwrap();
    let l = hs.ext.orch.clone().unwrap();
    assert_eq!((l.run.as_deref(), l.parent.as_deref(), l.depth), (Some(helpers[0].run.as_str()), Some(lead.key.as_str()), 1));
    // The record is on disk, sealed, and a new orchestrator reads it: the same run, result and attempt.
    r.orch.flush();
    let again = Orch::new(r.k.clone(), r.env.clone(), Some(Sealed::in_dir(&r.root.join("orch"), "runs", r.crypto.clone())));
    let back = again.helpers_of(&lead.key);
    assert_eq!((back.len(), back[0].state, back[0].result.as_deref()), (1, RunState::Done, Some("The answer is 42.")));
    let raw: Vec<u8> = std::fs::read_dir(r.root.join("orch")).unwrap().flatten().flat_map(|e| std::fs::read(e.path()).unwrap()).collect();
    assert!(!raw.windows(6).any(|w| w == b"answer"), "sealed, not plain text");
}

#[test]
fn helpers_never_have_more_access_than_the_lead_and_failures_say_why() {
    let r = rig("perm", DelegationLimits { max_helpers: 10, max_parallel: 10, max_depth: 1 }, |a, _, release| hold(a, release));
    *r.env.access.lock().unwrap() = "risky".into();
    let lead = r.lead("hold");
    let ask = |p: &str, access: Option<&str>| r.orch.delegate(&lead.key, Delegate { provider: p.into(), brief: "x".into(), access: access.map(str::to_owned), ..Default::default() });
    let a = ask("codex", Some("full")).unwrap();
    assert_eq!(a.access, "risky");
    assert!(a.note.unwrap().contains("narrowed to risky"));
    assert_eq!(ask("kiro", Some("read")).unwrap().access, "read", "narrower is fine");
    assert_eq!(ask("kiro", None).unwrap().access, "risky", "the lead's own by default");
    assert!(ask("claude", None).unwrap_err().contains("isn’t available: sign in first"));
    let e = ask("gemini", None).unwrap_err();
    assert!(e.contains("no provider called “gemini”") && e.contains("kiro, codex"), "{e}");
    assert!(r.orch.delegate(&lead.key, Delegate { provider: "kiro".into(), brief: "  ".into(), ..Default::default() }).unwrap_err().contains("needs a brief"));
    // Delegation off for a task: a clear refusal.
    let plain = r.k.start(AgentTool::Codex, &r.folder, "hold", vec![]).unwrap();
    assert_eq!(r.orch.delegate(&plain.key, Delegate { provider: "kiro".into(), brief: "x".into(), ..Default::default() }).unwrap_err(), orch::OFF);
    r.release();
    wait_for("all to finish", || r.k.running() == 0);
    // A lead whose turn is over has stale credentials.
    let e = ask("kiro", None).unwrap_err();
    assert!(e.contains("no longer valid"), "{e}");
}

#[test]
fn the_user_set_limits_hold_for_count_parallel_work_and_depth() {
    let r = rig("limits", DelegationLimits { max_helpers: 3, max_parallel: 1, max_depth: 1 }, |a, o, release| {
        if a.prompt.contains("[try-nested]") {
            let e = o.delegate(a.tag.as_ref().unwrap(), Delegate { provider: "kiro".into(), brief: "deeper".into(), ..Default::default() }).unwrap_err();
            return KiroResult::new(KiroState::Completed, &e);
        }
        hold(a, release)
    });
    let lead = r.lead("hold");
    let first = r.ask(&lead.key, "kiro", "[try-nested] one", None).unwrap();
    wait_for("the first to finish", || r.orch.run_of(first.session.as_deref().unwrap_or("")).is_some() || r.orch.helpers_of(&lead.key)[0].state.finished());
    wait_for("done", || r.orch.helpers_of(&lead.key)[0].state == RunState::Done);
    let nested = r.orch.helpers_of(&lead.key)[0].result.clone().unwrap();
    assert_eq!(nested, orch::OFF, "a helper at the depth limit can't delegate");
    let second = r.ask(&lead.key, "kiro", "hold please", None).unwrap();
    let e = r.ask(&lead.key, "kiro", "too many at once", None).unwrap_err();
    assert!(e.contains("already working") && e.contains("limit is 1"), "{e}");
    r.orch.cancel(&lead.key, &second.run).unwrap();
    wait_for("cancelled", || r.orch.helpers_of(&lead.key)[1].state == RunState::Cancelled);
    let third = r.ask(&lead.key, "kiro", "hold again", None).unwrap();
    r.orch.cancel(&lead.key, &third.run).unwrap();
    wait_for("cancelled too", || r.orch.helpers_of(&lead.key)[2].state == RunState::Cancelled);
    let e = r.ask(&lead.key, "kiro", "a fourth", None).unwrap_err();
    assert!(e.contains("used its 3 helpers"), "{e}");
    r.release();
}

#[test]
fn a_wait_that_runs_out_of_time_does_not_cancel_and_a_waiting_lead_gives_up_its_place() {
    let r = rig("wait", DelegationLimits::default(), |a, _, release| hold(a, release));
    r.k.set_max_running(1);
    let lead = r.lead("hold");
    // The only place is the lead's. The helper queues, and starts once the lead waits.
    let run = r.ask(&lead.key, "codex", "slow job", None).unwrap();
    assert_eq!(run.state, RunState::Queued, "no place yet");
    let mid = r.orch.wait(&lead.key, &run.run, Duration::from_millis(600)).unwrap();
    assert!(!mid.state.finished(), "the wait timed out and said the helper is still working");
    wait_for("the helper to be running", || r.orch.helpers_of(&lead.key)[0].state == RunState::Running);
    // Waiting again, the lead holds no place: the helper has the only one.
    let (o2, key, id) = (r.orch.clone(), lead.key.clone(), run.run.clone());
    let waiter = std::thread::spawn(move || o2.wait(&key, &id, Duration::from_secs(20)));
    wait_for("the lead to be parked", || r.k.running() == 1);
    r.release();
    let done = waiter.join().unwrap().unwrap();
    assert_eq!((done.state, done.result.as_deref()), (RunState::Done, Some("Held, then done.")));
}

#[test]
fn stop_reaches_helpers_and_their_helpers_and_late_news_wakes_nobody() {
    let r = rig("stop", DelegationLimits { max_helpers: 6, max_parallel: 4, max_depth: 2 }, |a, o, release| {
        if a.prompt.contains("[delegate-then-hold]") {
            o.delegate(a.tag.as_ref().unwrap(), Delegate { provider: "codex".into(), brief: "grandchild job".into(), ..Default::default() }).unwrap();
        }
        hold(a, release)
    });
    let lead = r.lead("hold");
    let child = r.ask(&lead.key, "kiro", "[delegate-then-hold] child job", None).unwrap();
    let _ = child;
    wait_for("the grandchild to run", || r.k.all().len() == 3 && r.k.running() == 3);
    let child_session = r.orch.helpers_of(&lead.key)[0].session.clone().unwrap();
    r.k.stop(lead.id);
    wait_for("everything to stop", || r.k.running() == 0);
    wait_for("runs settled", || { let c = r.orch.helpers_of(&lead.key); c[0].state == RunState::Cancelled });
    assert_eq!(r.orch.helpers_of(&child_session).len(), 1);
    wait_for("the grandchild stopped", || r.orch.helpers_of(&child_session)[0].state == RunState::Cancelled);
    // Nothing they report afterwards wakes the lead that was stopped, and it may not start new helpers.
    r.release();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(r.k.get(lead.id).unwrap().turns.len(), 1, "the stopped lead was not restarted");
    assert!(r.orch.helpers_of(&lead.key).iter().all(|h| h.delivery != Delivery::Sent));
    assert!(r.orch.delegate(&lead.key, Delegate { provider: "kiro".into(), brief: "x".into(), ..Default::default() }).is_err());
    assert_eq!(r.k.running(), 0);
}

#[test]
fn a_result_that_comes_after_the_lead_finished_is_sent_once_and_never_twice() {
    let r = rig("late", DelegationLimits::default(), |a, o, release| {
        if a.prompt.starts_with("[Hover helper task]") { return hold(a, release); }
        if a.prompt.contains("hover-run:") { return KiroResult::new(KiroState::Completed, "Thanks, noted."); }
        o.delegate(a.tag.as_ref().unwrap(), Delegate { provider: "codex".into(), brief: "go and look".into(), ..Default::default() }).unwrap();
        KiroResult::new(KiroState::Completed, "I asked a helper and I'm finished for now.")
    });
    let lead = r.lead("delegate and don't wait");
    wait_for("the lead to finish", || r.k.get(lead.id).is_some_and(|s| !s.busy()));
    assert_eq!(r.orch.pending_for(&lead.key), 1, "the helper is still at work and the lead's screen can say so");
    r.release();
    wait_for("the news to reach the lead", || r.k.get(lead.id).is_some_and(|s| s.turns.len() == 2 && s.turns[1].result.is_some()));
    let s = r.k.get(lead.id).unwrap();
    assert!(s.turns[1].prompt.contains("hover-run:") && s.turns[1].prompt.contains("Held, then done."), "{}", s.turns[1].prompt);
    assert_eq!(r.orch.helpers_of(&lead.key)[0].delivery, Delivery::Sent);
    // Asking again, or a restart that finds the mark already in the lead's history, sends nothing more.
    r.orch.deliver_to(&lead.key);
    r.orch.deliver_pending();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.k.get(lead.id).unwrap().turns.len(), 2);
}

#[test]
fn a_restart_fails_runs_that_were_cut_off_and_does_not_send_a_result_twice() {
    let r = rig("restart", DelegationLimits::default(), |_, _, _| KiroResult::new(KiroState::Completed, "ok"));
    // A record left by an earlier run of Hover: one helper still "running" with no process behind it, and one
    // that finished and whose result is already in the lead's history but was never marked.
    let lead = r.lead("the lead");
    wait_for("the lead to finish", || r.k.get(lead.id).is_some_and(|s| !s.busy()));
    let mut st = r.root.join("orch2");
    std::fs::create_dir_all(&st).unwrap();
    st.push("x");
    let run = |id: &str, state: &str, session: Option<&str>| Json::obj(vec![("Id", Json::str(id)), ("Parent", Json::str(&lead.key)), ("Root", Json::str(&lead.key)), ("Depth", Json::int(1)),
        ("Provider", Json::str("codex")), ("Brief", Json::str("b")), ("Access", Json::str("full")), ("State", Json::str(state)), ("Result", Json::str("the result")),
        ("Session", Json::opt_str_of(session)), ("Attempts", Json::Arr(vec![])), ("Delivery", Json::str("pending")), ("Created", Json::int(1))]);
    let doc = Sealed::in_dir(&r.root.join("orch2"), "runs", r.crypto.clone());
    doc.write(&Json::obj(vec![("Runs", Json::Arr(vec![run("r-cut", "running", Some("ghost")), run("r-sent", "done", None)]))])).unwrap();
    // The lead already has r-sent's result in its history (the earlier run sent it, then died before it wrote that down).
    assert!(r.k.reply(lead.id, "[Hover] A helper finished (hover-run:r-sent; codex; done).\n\nthe result", vec![]));
    wait_for("that reply", || r.k.get(lead.id).is_some_and(|s| !s.busy() && s.turns.len() == 2));
    let turns_before = r.k.get(lead.id).unwrap().turns.len();
    assert_eq!(turns_before, 2);
    let o = Orch::new(r.k.clone(), r.env.clone(), Some(doc));
    let h = o.helpers_of(&lead.key);
    let cut = h.iter().find(|x| x.run == "r-cut").unwrap();
    assert_eq!(cut.state, RunState::Failed);
    assert!(cut.note.as_deref().unwrap().contains("Hover closed"));
    o.deliver_pending();
    // The lead is told about the run that was cut off, once; r-sent is in its history already, so it is marked and not sent again.
    wait_for("the news of the cut-off run", || r.k.get(lead.id).is_some_and(|s| s.turns.len() == turns_before + 1 && !s.busy()));
    let told = r.k.get(lead.id).unwrap().turns.last().unwrap().prompt.clone();
    assert!(told.contains("hover-run:r-cut") && !told.contains("hover-run:r-sent"), "{told}");
    o.deliver_pending();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(r.k.get(lead.id).unwrap().turns.len(), turns_before + 1, "nothing is sent twice");
    assert_eq!(o.helpers_of(&lead.key).iter().find(|x| x.run == "r-sent").unwrap().delivery, Delivery::Sent);
}

#[test]
fn a_writing_helper_gets_a_worktree_and_a_read_only_helper_shares_the_leads_folder() {
    let r = rig("tree", DelegationLimits::default(), |a, o, _| {
        if a.prompt.starts_with("[Hover helper task]") {
            if a.access.as_deref() != Some("read") { std::fs::write(Path::new(&a.folder).join("by-helper.txt"), "x").unwrap(); }
            return KiroResult::new(KiroState::Completed, &a.folder);
        }
        let tag = a.tag.clone().unwrap();
        let w = o.delegate(&tag, Delegate { provider: "codex".into(), brief: "write".into(), ..Default::default() }).unwrap();
        let ro = o.delegate(&tag, Delegate { provider: "kiro".into(), brief: "look".into(), access: Some("read".into()), ..Default::default() }).unwrap();
        let (w, ro) = (o.wait(&tag, &w.run, Duration::from_secs(20)).unwrap(), o.wait(&tag, &ro.run, Duration::from_secs(20)).unwrap());
        assert_eq!((w.state, ro.state), (RunState::Done, RunState::Done));
        KiroResult::new(KiroState::Completed, &format!("{}|{}", w.result.unwrap(), ro.result.unwrap()))
    });
    let git = |args: &[&str]| { let o = std::process::Command::new("git").current_dir(&r.folder).args(["-c", "user.name=T", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).output().unwrap(); assert!(o.status.success()); };
    git(&["init", "-q"]);
    std::fs::write(Path::new(&r.folder).join("a.txt"), "1").unwrap();
    git(&["add", "."]);
    git(&["commit", "-qm", "first"]);
    let lead = r.lead("ask two helpers");
    wait_for("the lead to finish", || r.k.get(lead.id).is_some_and(|s| !s.busy() && s.result().is_some()));
    let said = r.k.get(lead.id).unwrap().result().unwrap().text.clone();
    let (writer, reader) = said.split_once('|').unwrap_or_else(|| panic!("the lead said: {said}"));
    assert_ne!(writer, r.folder, "the writing helper has a folder of its own");
    assert!(writer.starts_with(r.root.join("worktrees").to_str().unwrap()), "{writer}");
    assert!(Path::new(writer).join("by-helper.txt").exists() && !Path::new(&r.folder).join("by-helper.txt").exists(), "the lead's folder was not written");
    assert_eq!(reader, r.folder, "a read-only helper looks at the lead's folder");
    let ws = r.k.find(r.orch.helpers_of(&lead.key)[0].session.as_ref().unwrap()).unwrap().ext.workspace.unwrap();
    assert!(ws.is_worktree() && ws.branch.is_some());
}

#[test]
fn a_lead_reads_messages_and_stops_only_the_threads_it_launched() {
    let r = rig("threads", DelegationLimits::default(), |a, _, release| if a.prompt.contains("[hold]") { hold(a, release) } else { KiroResult::new(KiroState::Completed, &format!("reply to: {}", a.prompt)) });
    let lead = r.lead("[hold]");
    let other = r.k.start_bound(AgentTool::Codex, &r.folder, "[hold]", vec![], None, None, link()).unwrap();
    let t = r.orch.thread_launch(&lead.key, "codex", "first message", None).unwrap();
    wait_for("the thread to answer", || r.orch.thread_read(&lead.key, &t, 0, 10_000).is_ok_and(|(text, _, _)| text.contains("reply to: first message")));
    r.orch.thread_send(&lead.key, &t, "second [hold]").unwrap();
    wait_for("the second turn to run", || r.orch.thread_read(&lead.key, &t, 1, 10_000).is_ok_and(|(text, _, _)| text.contains("second [hold]")));
    // Reading is bounded, in pages, from a turn.
    let (page, next, more) = r.orch.thread_read(&lead.key, &t, 0, 70).unwrap();
    assert!(page.contains("first message") && next == 1 && more, "{page} {next} {more}");
    // Another task that has the id can neither read nor change it.
    for e in [r.orch.thread_read(&other.key, &t, 0, 100).unwrap_err(), r.orch.thread_send(&other.key, &t, "hi").unwrap_err(), r.orch.thread_interrupt(&other.key, &t).unwrap_err()] {
        assert!(e.contains("belongs to another task"), "{e}");
    }
    r.orch.thread_interrupt(&lead.key, &t).unwrap();
    wait_for("the thread to stop", || r.orch.thread_read(&lead.key, &t, 1, 10_000).is_ok_and(|(text, _, _)| text.contains("Stopped.")));
    r.release();
}

#[test]
fn the_mcp_tools_answer_a_lead_and_refuse_a_stale_one() {
    let r = rig("mcp", DelegationLimits::default(), |a, o, _| {
        if a.prompt.starts_with("[Hover helper task]") { return KiroResult::new(KiroState::Completed, "found it"); }
        let tag = a.tag.clone().unwrap();
        let call = |name: &str, args: Vec<(&str, Json)>| -> (String, bool) {
            let m = Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", Json::int(1)), ("method", Json::str("tools/call")), ("params", Json::obj(vec![("name", Json::str(name)), ("arguments", Json::obj(args))]))]);
            let out = o.answer(&tag, &m).unwrap();
            let res = out.get("result").unwrap();
            (res.get("content").unwrap().items().unwrap()[0].get("text").and_then(Json::as_str).unwrap().to_owned(), res.get("isError").map(|b| b.bool().unwrap()).unwrap_or(false))
        };
        let (listing, bad) = call("list_providers", vec![]);
        assert!(!bad && listing.contains("codex — CODEX: ready") && listing.contains("claude — CLAUDE: not ready (sign in first)"), "{listing}");
        let (text, bad) = call("delegate_task", vec![("provider", Json::str("codex")), ("brief", Json::str("find it")), ("request_id", Json::str("q1"))]);
        assert!(!bad && text.contains("run_id: r-"), "{text}");
        let run_id = text.lines().next().unwrap().trim_start_matches("run_id: ").to_owned();
        let (text, bad) = call("wait_for_task", vec![("run_id", Json::str(&run_id)), ("timeout_secs", Json::int(10))]);
        assert!(!bad && text.contains("state: done") && text.contains("found it"), "{text}");
        let (text, bad) = call("task_result", vec![("run_id", Json::str("r-nope"))]);
        assert!(bad && text.contains("no helper with that id"), "{text}");
        let (text, bad) = call("delegate_task", vec![("provider", Json::str("codex"))]);
        assert!(bad && text.contains("brief is needed"), "{text}");
        KiroResult::new(KiroState::Completed, "ok")
    });
    let lead = r.lead("use the tools");
    wait_for("the lead to finish", || r.k.get(lead.id).is_some_and(|s| !s.busy() && s.result().is_some()));
    assert_eq!(r.k.get(lead.id).unwrap().result().unwrap().state, KiroState::Completed, "{}", r.k.get(lead.id).unwrap().result().unwrap().text);
    // The server lists the tools, and a call from the finished turn is refused as stale.
    let list = r.orch.answer(&lead.key, &Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", Json::int(2)), ("method", Json::str("tools/list"))])).unwrap();
    assert!(list.compact().contains("delegate_task") && list.compact().contains("interrupt_thread"));
    let stale = r.orch.answer(&lead.key, &Json::obj(vec![("jsonrpc", Json::str("2.0")), ("id", Json::int(3)), ("method", Json::str("tools/call")),
        ("params", Json::obj(vec![("name", Json::str("list_providers")), ("arguments", Json::obj(vec![]))]))])).unwrap();
    assert!(stale.compact().contains("no longer valid") && stale.compact().contains("\"isError\":true"));
    assert!(r.orch.answer(&lead.key, &Json::obj(vec![("method", Json::str("notifications/initialized"))])).is_none(), "a notification gets no reply");
}

/// The whole way an agent reaches it: the MCP command Hover hands out (perl's relay) joined to Hover's socket,
/// JSON lines in and out. A task without delegation is handed no server at all.
#[cfg(unix)]
#[test]
fn an_agent_reaches_the_helpers_through_the_relay_and_the_socket() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let short = std::path::PathBuf::from(format!("/tmp/hvo-{}", std::process::id()));
    std::fs::create_dir_all(&short).unwrap();
    std::env::set_var("HOVER_BROWSER_SOCKET", short.join("b.sock"));
    let r = rig("socket", DelegationLimits::default(), |a, _, release| hold(a, release));
    r.orch.install();
    let lead = r.lead("[hold]");
    let plain = r.k.start(AgentTool::Codex, &r.folder, "[hold]", vec![]).unwrap();
    assert!(orch::servers(Some(&plain.key)).is_empty(), "no delegation, no server");
    assert!(orch::servers(None).is_empty());
    let servers = orch::servers(Some(&lead.key));
    assert_eq!(servers.len(), 1, "a Unix host with perl hands the server out");
    let s = &servers[0];
    assert_eq!(s.name, orch::SERVER_NAME);
    assert!(!s.args.iter().any(|a| a.contains("HELLO")), "no token on the command line");
    let mut p = Command::new(&s.command).args(&s.args).envs(s.env.iter().cloned()).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut input = p.stdin.take().unwrap();
    let mut out = BufReader::new(p.stdout.take().unwrap());
    let mut ask = |line: &str| -> Json { writeln!(input, "{line}").unwrap(); let mut l = String::new(); out.read_line(&mut l).unwrap(); hover_core::json::parse(l.trim()).unwrap() };
    let init = ask(r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}"#);
    assert_eq!(init.get("result").unwrap().get("serverInfo").unwrap().get("name").and_then(Json::as_str), Some(orch::SERVER_NAME));
    let list = ask(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
    assert!(list.compact().contains("delegate_task"));
    let call = ask(r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"delegate_task","arguments":{"provider":"codex","brief":"look around","request_id":"x1"}}}"#);
    assert!(call.compact().contains("run_id: r-"), "{}", call.compact());
    // The call was made for the lead whose token this is, and nobody else.
    assert_eq!(r.orch.helpers_of(&lead.key).len(), 1);
    assert_eq!(r.orch.helpers_of(&plain.key).len(), 0);
    drop(input);
    let _ = p.wait();
    r.release();
    wait_for("all done", || r.k.running() == 0);
    hover_agents::browser::stop();
    let _ = std::fs::remove_dir_all(&short);
}

#[test]
fn an_agent_sent_a_conversation_reference_reads_it_in_pages_and_cannot_read_others() {
    use hover_agents::session::Msg;
    let r = rig("readconv", DelegationLimits::default(), |a, o, _| {
        let tag = a.tag.clone().unwrap();
        if !a.prompt.contains("[Attached by Hover]") { return KiroResult::new(KiroState::Completed, &format!("I am {}. The secret of this one is: blue.", if a.prompt.contains("other") { "other" } else { "earlier" })); }
        let key = a.prompt.split("(key ").nth(1).and_then(|r| r.split(')').next()).unwrap().to_owned();
        let (page, next, more) = o.read_conversation(&tag, &key, 0, 10_000).unwrap();
        let denied = o.read_conversation(&tag, "not-given", 0, 100).unwrap_err();
        KiroResult::new(KiroState::Completed, &format!("{page}|{next}|{more}|{denied}"))
    });
    let earlier = r.k.start(AgentTool::Codex, &r.folder, "earlier topic", vec![]).unwrap();
    let other = r.k.start(AgentTool::Codex, &r.folder, "other topic", vec![]).unwrap();
    let me = r.k.start(AgentTool::Kiro, &r.folder, "hello", vec![]).unwrap();
    wait_for("all", || r.k.running() == 0);
    let chip = hover_agents::context::thread(&earlier.key, "Earlier topic");
    assert!(r.k.reply_msg(me.id, Msg { text: "please read it".into(), chips: vec![chip], ..Default::default() }));
    wait_for("the reply", || r.k.get(me.id).is_some_and(|s| !s.busy() && s.turns.len() == 2 && s.turns[1].result.is_some()));
    let said = r.k.get(me.id).unwrap().result().unwrap().text.clone();
    assert!(said.contains("[0] User: earlier topic") && said.contains("secret of this one is: blue") && said.contains("|1|false|"), "{said}");
    assert!(!said.contains("other topic"), "only the referenced conversation");
    assert!(said.contains("You were not given a reference to that conversation"), "{said}");
    assert!(r.orch.answer(&me.key, &Json::obj(vec![("id", Json::int(1)), ("method", Json::str("tools/list"))])).unwrap().compact().contains("read_conversation"));
    let listed = r.orch.answer(&me.key, &Json::obj(vec![("id", Json::int(1)), ("method", Json::str("tools/list"))])).unwrap().compact();
    assert!(!listed.contains("delegate_task"), "without delegation only the reading tool is listed: {listed}");
    let _ = other;
}

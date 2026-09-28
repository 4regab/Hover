//! KiroSessionTests, KiroSessionsTests and AgentHistoryTests (tests/Hover.Tests),
//! ported: the shared run state with the runner stubbed out.

use hover_agents::session::{KiroSessions, RunArgs, RunTask, MAX_KEPT, MAX_RUNNING};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::crypto::Crypto;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-sessions-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(5) { std::thread::sleep(Duration::from_millis(10)); } }

/// A run that waits for its result, or ends as stopped when cancelled.
struct Gate { prompt: String, resume: Option<String>, tool: AgentTool, done: Sender<KiroResult> }

fn gated() -> (impl Fn(AgentTool) -> RunTask + Send + Sync + 'static, Arc<Mutex<Vec<Gate>>>) {
    let runs: Arc<Mutex<Vec<Gate>>> = Default::default();
    let r = runs.clone();
    let make = move |tool: AgentTool| -> RunTask {
        let r = r.clone();
        Arc::new(move |a: RunArgs| {
            let (tx, rx): (Sender<KiroResult>, Receiver<KiroResult>) = channel();
            let tx2 = tx.clone();
            let _reg = a.ct.on_cancel(move || { let _ = tx2.send(KiroResult::new(KiroState::Cancelled, "stopped")); });
            (a.events)(KiroEvent { session_id: Some("sess_9".into()), ..Default::default() });
            r.lock().unwrap().push(Gate { prompt: a.prompt, resume: a.resume, tool, done: tx });
            rx.recv().unwrap()
        })
    };
    (make, runs)
}

fn finish(runs: &Mutex<Vec<Gate>>, i: usize, r: KiroResult) { let _ = runs.lock().unwrap()[i].done.send(r); }

/// KiroSessionTests.A_run_goes_from_idle_to_running_to_completed (a session starts
/// through KiroSessions here: the port has no bare session).
#[test]
fn a_run_goes_from_running_to_completed() {
    let f = folder("run");
    let (make, runs) = gated();
    let k = KiroSessions::new(make, None);
    let ended: Arc<Mutex<Vec<KiroResult>>> = Default::default();
    let e2 = ended.clone();
    k.on_ended(move |_, r| e2.lock().unwrap().push(r.clone()));
    let s = k.start(AgentTool::Kiro, &f, "  Write the changelog  ", vec![]).unwrap();
    assert_eq!(s.state, KiroState::Running);
    wait_for(|| runs.lock().unwrap().len() == 1);
    assert_eq!(runs.lock().unwrap()[0].prompt, "Write the changelog");
    finish(&runs, 0, KiroResult { state: KiroState::Completed, text: "Wrote it.".into(), exit_code: Some(0) });
    wait_for(|| !k.get(s.id).unwrap().busy());
    let s = k.get(s.id).unwrap();
    assert_eq!((s.state, s.result().unwrap().text.as_str(), s.title().as_str()), (KiroState::Completed, "Wrote it.", "Write the changelog"));
    wait_for(|| ended.lock().unwrap().len() == 1);
    assert_eq!(ended.lock().unwrap()[0].state, KiroState::Completed);
}

#[test]
fn it_will_not_start_without_a_usable_folder_or_a_prompt() {
    let f = folder("nostart");
    let k = KiroSessions::new(|_| -> RunTask { Arc::new(|_| panic!("must not run")) }, None);
    assert!(k.start(AgentTool::Kiro, &format!("{f}/missing"), "task", vec![]).is_none());
    assert!(k.start(AgentTool::Kiro, "", "task", vec![]).is_none());
    assert!(k.start(AgentTool::Kiro, &f, "   ", vec![]).is_none());
    assert!(k.all().is_empty());
}

#[test]
fn stop_cancels_the_run_and_a_runner_that_panics_fails() {
    let f = folder("stop");
    let k = KiroSessions::new(|_| -> RunTask {
        Arc::new(|a: RunArgs| {
            if a.prompt == "boom" { panic!("boom"); }
            while !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); }
            KiroResult { state: KiroState::Failed, text: "killed".into(), exit_code: Some(-1) }
        })
    }, None);
    let s = k.start(AgentTool::Kiro, &f, "long", vec![]).unwrap();
    k.stop(s.id);
    wait_for(|| !k.get(s.id).unwrap().busy());
    assert_eq!(k.get(s.id).unwrap().state, KiroState::Cancelled, "a stopped run reads as stopped, however it ended");
    let b = k.start(AgentTool::Kiro, &f, "boom", vec![]).unwrap();
    wait_for(|| !k.get(b.id).unwrap().busy());
    assert_eq!(k.get(b.id).unwrap().result().unwrap(), &KiroResult::new(KiroState::Failed, "boom"));
}

#[test]
fn a_reply_carries_on_with_the_tools_id_and_waits_while_a_turn_runs() {
    let f = folder("reply");
    let (make, runs) = gated();
    let k = KiroSessions::new(make, None);
    let s = k.start(AgentTool::Kiro, &f, "first", vec![]).unwrap();
    wait_for(|| k.get(s.id).unwrap().kiro_id.is_some());
    assert!(k.reply(s.id, "second", vec![]));
    assert!(k.get(s.id).unwrap().turns.last().unwrap().queued, "it waits for the turn that runs");
    finish(&runs, 0, KiroResult::new(KiroState::Completed, "one"));
    wait_for(|| runs.lock().unwrap().len() == 2);
    {
        let r = runs.lock().unwrap();
        assert_eq!((r[1].prompt.as_str(), r[1].resume.as_deref()), ("second", Some("sess_9")));
    }
    let now = k.get(s.id).unwrap();
    assert!(now.busy() && !now.turns.last().unwrap().queued);
    assert_eq!(now.prompt(), "first", "the session keeps its first prompt as its title");
    k.reply(s.id, "third", vec![]);
    k.stop(s.id);
    wait_for(|| !k.get(s.id).unwrap().busy());
    let last = k.get(s.id).unwrap().turns.last().unwrap().clone();
    assert_eq!(last.result.unwrap(), KiroResult::new(KiroState::Cancelled, "Not sent: the run before it was stopped."), "a stop drops the waiting reply");
    assert_eq!(runs.lock().unwrap().len(), 2);
}

#[test]
fn images_go_to_the_agent_as_paths_after_the_prompt() {
    let f = folder("images");
    let (make, runs) = gated();
    let k = KiroSessions::new(make, None);
    k.start(AgentTool::Kiro, &f, "", vec!["/x/a.png".into(), "/x/b.jpg".into()]).unwrap();
    wait_for(|| runs.lock().unwrap().len() == 1);
    assert_eq!(runs.lock().unwrap()[0].prompt, "Look at the attached image.\n\nAttached image (read it from this file): /x/a.png\nAttached image (read it from this file): /x/b.jpg");
    k.stop_all();
}

#[test]
fn tasks_run_side_by_side_up_to_the_cap_across_tools() {
    let f = folder("cap");
    let (make, runs) = gated();
    let k = KiroSessions::new(make, None);
    let a = k.start(AgentTool::Kiro, &f, "one", vec![]).unwrap();
    let b = k.start(AgentTool::Codex, &f, "two", vec![]).unwrap();
    let c = k.start(AgentTool::Cursor, &f, "three", vec![]).unwrap();
    assert_eq!(k.running(), MAX_RUNNING);
    assert!(k.start(AgentTool::Kiro, &f, "four", vec![]).is_none(), "three running in all, whatever the tools");
    assert_eq!(k.selected(), Some(c.id), "a new task is the one shown");
    wait_for(|| runs.lock().unwrap().len() == 3);
    let mut tools: Vec<AgentTool> = runs.lock().unwrap().iter().map(|g| g.tool).collect();
    tools.sort_by_key(|t| *t as u8);
    assert_eq!(tools, [AgentTool::Kiro, AgentTool::Codex, AgentTool::Cursor], "each on its own tool");
    let bi = runs.lock().unwrap().iter().position(|g| g.prompt == "two").unwrap();
    finish(&runs, bi, KiroResult::new(KiroState::Completed, "done"));
    wait_for(|| k.running() == 2);
    assert_eq!(k.get(b.id).unwrap().state, KiroState::Completed);
    assert!(k.get(a.id).unwrap().busy() && k.get(c.id).unwrap().busy(), "the others carry on");
    assert!(k.start(AgentTool::Kiro, &f, "four", vec![]).is_some(), "a free slot takes a new task");
    assert!(!k.reply(b.id, "more", vec![]), "no fourth run by a reply either");
    k.stop_all();
    wait_for(|| k.running() == 0);
    assert_eq!(k.all().iter().filter(|s| s.state == KiroState::Cancelled).count(), 3);
    // Seats and bots: the lowest free, in start order.
    assert_eq!(k.all().iter().map(|s| (s.seat, s.bot)).collect::<Vec<_>>(), [(0, 0), (1, 1), (2, 2), (3, 3)]);
}

#[test]
fn only_the_newest_are_kept_and_dismiss_removes_a_finished_task() {
    let f = folder("kept");
    let (make, runs) = gated();
    let k = KiroSessions::new(make, None);
    for i in 0..MAX_KEPT + 2 {
        k.start(AgentTool::Kiro, &f, &format!("task {i}"), vec![]).unwrap();
        wait_for(|| runs.lock().unwrap().len() == i + 1);
        finish(&runs, i, KiroResult::new(KiroState::Completed, "ok"));
        wait_for(|| k.running() == 0);
    }
    assert_eq!(k.all().len(), MAX_KEPT);
    assert_eq!(k.all()[0].prompt(), "task 2");
    let first = k.all()[0].clone();
    k.select(Some(first.id));
    k.dismiss(first.id);
    assert_eq!(k.all().len(), MAX_KEPT - 1);
    assert_eq!(k.selected(), None, "dismissing the shown task goes back to a new one");
}

fn history(name: &str) -> Arc<AgentHistory> {
    Arc::new(AgentHistory::new(std::path::PathBuf::from(folder(name)).join("agents"), Arc::new(Crypto::with_key([1; 32]))))
}

/// AgentHistoryTests: runs that answer at once, saying which conversation they resumed.
fn answering(resumed: Arc<Mutex<Vec<Option<String>>>>) -> impl Fn(AgentTool) -> RunTask + Send + Sync + 'static {
    move |_| {
        let resumed = resumed.clone();
        Arc::new(move |a: RunArgs| {
            resumed.lock().unwrap().push(a.resume.clone());
            (a.events)(KiroEvent { session_id: Some("acp-1".into()), ..Default::default() });
            KiroResult::new(KiroState::Completed, format!("answer to {}", a.prompt))
        })
    }
}

#[test]
fn a_session_that_left_its_desk_wakes_on_a_reply_and_carries_on_its_conversation() {
    let f = folder("wake");
    let h = history("wake-h");
    let resumed: Arc<Mutex<Vec<Option<String>>>> = Default::default();
    let k = KiroSessions::new(answering(resumed.clone()), Some(h.clone()));
    let first = k.start(AgentTool::Cursor, &f, "first", vec![]).unwrap();
    wait_for(|| !k.get(first.id).unwrap().busy());
    for i in 0..MAX_KEPT {
        let s = k.start(AgentTool::Kiro, &f, &format!("task {i}"), vec![]).unwrap();
        wait_for(|| k.get(s.id).is_some_and(|s| !s.busy()));
    }
    assert!(!k.all().iter().any(|x| x.key == first.key));
    h.flush();
    assert!(h.entries().iter().any(|e| e.key == first.key));
    let woken = k.wake(&first.key).unwrap();
    assert_eq!(woken.turns[0].result.as_ref().unwrap().text, "answer to first");
    assert!(k.reply(woken.id, "and then?", vec![]));
    wait_for(|| !k.get(woken.id).unwrap().busy());
    let w = k.get(woken.id).unwrap();
    assert_eq!(w.tool, AgentTool::Cursor);
    assert_eq!(resumed.lock().unwrap().last().unwrap().as_deref(), Some("acp-1"), "the reply resumed the saved conversation");
    assert_eq!(w.turns.len(), 2);
    assert_eq!(k.all().len(), MAX_KEPT);
    // Sealed on disk and whole again.
    h.flush();
    let again = AgentHistory::new(std::env::temp_dir().join(format!("hover-sessions-wake-h-{}", std::process::id())).join("agents"), Arc::new(Crypto::with_key([1; 32])));
    let saved = again.load(&first.key).unwrap();
    assert_eq!((saved.tool, saved.acp_id.as_deref(), saved.turns.len()), (AgentTool::Cursor, Some("acp-1"), 2));
}

#[test]
fn delete_takes_a_session_out_of_the_office_and_the_history() {
    let f = folder("delete");
    let h = history("delete-h");
    let k = KiroSessions::new(answering(Default::default()), Some(h.clone()));
    let s = k.start(AgentTool::Kiro, &f, "one", vec![]).unwrap();
    wait_for(|| !k.get(s.id).unwrap().busy());
    k.delete(&s.key);
    h.flush();
    assert!(k.all().is_empty());
    assert!(h.entries().is_empty());
    assert!(h.load(&s.key).is_none());
    assert!(k.saved(&s.key).is_none());
}

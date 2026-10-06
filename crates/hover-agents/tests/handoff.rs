//! Moving a conversation between agents, forking it and bringing findings back. The scripted agents note what they are sent.

use hover_agents::session::{KiroSessions, Msg, RunArgs, RunTask, Target};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::crypto::Crypto;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-handoff-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

/// (tool, prompt, resume) for each run.
type Log = Arc<Mutex<Vec<(AgentTool, String, Option<String>)>>>;

/// Agents that answer "<tool> says <n>" and name their own conversation `<tool>-conv`; `hold` makes the first turn wait.
fn rig(name: &str) -> (KiroSessions, Log, String, Arc<Mutex<bool>>) {
    let log: Log = Default::default();
    let hold = Arc::new(Mutex::new(false));
    let (l2, h2) = (log.clone(), hold.clone());
    let f = folder(name);
    let crypto = Arc::new(Crypto::with_key([8; 32]));
    let k = KiroSessions::new(move |tool| -> RunTask {
        let (l, h) = (l2.clone(), h2.clone());
        Arc::new(move |a: RunArgs| {
            let n = { let mut g = l.lock().unwrap(); g.push((tool, a.prompt.clone(), a.resume.clone())); g.len() };
            (a.events)(KiroEvent { session_id: Some(format!("{}-conv", tool.id())), ..Default::default() });
            if a.prompt.contains("[hold]") { while *h.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); } }
            KiroResult::new(KiroState::Completed, &format!("{} says {n}", tool.id()))
        })
    }, Some(Arc::new(AgentHistory::new(std::path::Path::new(&f).join("history"), crypto))));
    (k, log, f, hold)
}

fn done(k: &KiroSessions, id: i32, n: usize) { wait_for("the turn", || k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == n && s.turns[n - 1].result.is_some())); }

#[test]
fn a_conversation_moves_to_another_agent_with_an_account_and_comes_back_to_its_own_memory() {
    let (k, log, f, _) = rig("move");
    let s = k.start(AgentTool::Kiro, &f, "Build the parser. Never allocate in the hot loop.", vec![]).unwrap();
    done(&k, s.id, 1);
    assert!(k.reply(s.id, "Now the lexer", vec![]));
    done(&k, s.id, 2);
    // Kiro -> Codex: a new conversation for Codex, started from an account; the history is untouched.
    let codex = Target::parse("codex").unwrap();
    let sw = k.switch_provider(s.id, &codex).unwrap();
    assert_eq!((sw.mode, sw.carried, sw.omitted), ("portable", 2, 0));
    let now = k.get(s.id).unwrap();
    assert_eq!(now.tool, AgentTool::Codex);
    assert_eq!(now.turns.len(), 2, "no turn was added or changed by the move");
    assert!(now.turns[1].result.as_ref().unwrap().text.starts_with("kiro says"), "old turns keep the agent that gave them");
    let lin = now.ext.lineage.clone().unwrap();
    assert_eq!((lin.handoffs.len(), lin.handoffs[0].turn, lin.handoffs[0].from.as_str(), lin.handoffs[0].to.as_str(), lin.handoffs[0].mode.as_str()), (1, 2, "kiro", "codex", "portable"));
    assert!(lin.pending.is_some() && lin.natives.iter().any(|n| n.provider == "kiro" && n.id == "kiro-conv" && n.seen == 2));
    // The next message: the account comes first, the user's words after the line, whole and untouched.
    assert!(k.reply(s.id, "Add error recovery, please.", vec![]));
    done(&k, s.id, 3);
    let (tool, prompt, resume) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!((tool, resume), (AgentTool::Codex, None), "Codex starts a conversation of its own");
    assert!(prompt.starts_with("[Hover handoff]") && prompt.contains("Never allocate in the hot loop") && prompt.contains("2. Now the lexer"), "{prompt}");
    assert!(prompt.ends_with("---\nAdd error recovery, please."), "{prompt}");
    // Sent once: the next reply to Codex carries no account.
    assert!(k.reply(s.id, "And docs", vec![]));
    done(&k, s.id, 4);
    let (_, prompt, resume) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!((prompt.as_str(), resume.as_deref()), ("And docs", Some("codex-conv")));
    // Back to Kiro: its own conversation resumes, and only the two turns it missed are handed over.
    let sw = k.switch_provider(s.id, &Target::parse("kiro").unwrap()).unwrap();
    assert_eq!((sw.mode, sw.carried), ("native", 2));
    assert!(k.reply(s.id, "Thanks, carry on", vec![]));
    done(&k, s.id, 5);
    let (tool, prompt, resume) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!((tool, resume.as_deref()), (AgentTool::Kiro, Some("kiro-conv")));
    assert!(prompt.contains("went on without you for 2 turns") && prompt.contains("3. Add error recovery") && !prompt.contains("The original request") && prompt.ends_with("Thanks, carry on"), "{prompt}");
    assert_eq!(k.get(s.id).unwrap().ext.lineage.unwrap().handoffs.len(), 2);
    // The same move again is refused, not repeated.
    assert!(k.switch_provider(s.id, &Target::parse("kiro").unwrap()).unwrap_err().contains("already"));
}

#[test]
fn a_switch_asked_for_with_a_queued_message_happens_when_that_message_is_sent() {
    let (k, log, f, hold) = rig("queued");
    *hold.lock().unwrap() = true;
    let s = k.start(AgentTool::Kiro, &f, "first [hold]", vec![]).unwrap();
    wait_for("the run", || log.lock().unwrap().len() == 1);
    assert!(k.switch_provider(s.id, &Target::parse("codex").unwrap()).unwrap_err().contains("A run is going on"));
    assert!(k.reply_msg(s.id, Msg { text: "then this, on Codex".into(), switch_to: Some("codex".into()), ..Default::default() }));
    assert_eq!(k.get(s.id).unwrap().tool, AgentTool::Kiro, "not while the earlier work runs");
    *hold.lock().unwrap() = false;
    done(&k, s.id, 2);
    let (tool, prompt, _) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!(tool, AgentTool::Codex);
    assert!(prompt.starts_with("[Hover handoff]") && prompt.ends_with("then this, on Codex"), "{prompt}");
    assert_eq!(k.get(s.id).unwrap().tool, AgentTool::Codex);
    // One that can't be made is said to the agent that gets the message, and the conversation stays.
    assert!(k.reply_msg(s.id, Msg { text: "and this".into(), switch_to: Some("nonesuch".into()), ..Default::default() }));
    done(&k, s.id, 3);
    let (tool, prompt, _) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!(tool, AgentTool::Codex);
    assert!(prompt.starts_with("[Hover] The switch to “nonesuch” couldn’t be made"), "{prompt}");
}

#[test]
fn a_conversation_too_long_to_carry_stays_where_it_is() {
    let (k, _, f, _) = rig("long");
    let s = k.start(AgentTool::Kiro, &f, &format!("Start. {}", "requirement ".repeat(900)), vec![]).unwrap();
    done(&k, s.id, 1);
    for i in 2..=40 { assert!(k.reply(s.id, &format!("Step {i}: {}", "constraint ".repeat(45)), vec![])); done(&k, s.id, i); }
    let before = k.get(s.id).unwrap();
    let e = k.switch_provider(s.id, &Target::parse("codex").unwrap()).unwrap_err();
    assert!(e.contains("too long to carry over"), "{e}");
    let after = k.get(s.id).unwrap();
    assert_eq!((after.tool, after.ext.lineage.clone(), after.kiro_id.clone()), (AgentTool::Kiro, None, before.kiro_id), "nothing changed");
}

#[test]
fn the_account_waiting_for_the_next_message_survives_a_restart_and_is_sent_once() {
    let (k, log, f, _) = rig("restart");
    let s = k.start(AgentTool::Kiro, &f, "Origin", vec![]).unwrap();
    done(&k, s.id, 1);
    k.switch_provider(s.id, &Target::parse("cursor").unwrap()).unwrap();
    k.history().unwrap().flush();
    let key = k.get(s.id).unwrap().key;
    let crypto = Arc::new(Crypto::with_key([8; 32]));
    let l2 = log.clone();
    let again = KiroSessions::new(move |tool| -> RunTask { let l = l2.clone(); Arc::new(move |a: RunArgs| { l.lock().unwrap().push((tool, a.prompt.clone(), a.resume.clone())); KiroResult::new(KiroState::Completed, "ok") }) },
        Some(Arc::new(AgentHistory::new(std::path::Path::new(&f).join("history"), crypto))));
    let woke = again.wake(&key).unwrap();
    assert_eq!(woke.tool, AgentTool::Cursor);
    assert!(woke.ext.lineage.as_ref().unwrap().pending.is_some(), "the account was saved with the conversation");
    assert!(again.reply(woke.id, "go on", vec![]));
    wait_for("the reply", || again.get(woke.id).is_some_and(|x| !x.busy() && x.turns.len() == 2 && x.turns[1].result.is_some()));
    let (tool, prompt, _) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!(tool, AgentTool::Cursor);
    assert!(prompt.starts_with("[Hover handoff]") && prompt.ends_with("go on"));
    assert!(again.get(woke.id).unwrap().ext.lineage.unwrap().pending.is_none());
}

#[test]
fn a_fork_copies_the_turns_up_to_a_stable_point_keeps_its_lineage_and_leaves_the_source_alone() {
    let (k, log, f, hold) = rig("fork");
    let s = k.start(AgentTool::Kiro, &f, "Design the cache", vec![]).unwrap();
    done(&k, s.id, 1);
    assert!(k.reply(s.id, "Pick a size", vec![]));
    done(&k, s.id, 2);
    let key = k.get(s.id).unwrap().key;
    let other = std::env::temp_dir().join(format!("hover-handoff-fork-dir-{}", std::process::id()));
    std::fs::create_dir_all(&other).unwrap();
    let fk = k.fork(&key, 0, &Target::parse("codex").unwrap(), &other.to_string_lossy(), None).unwrap();
    assert_ne!(fk.key, key);
    assert_eq!((fk.tool, fk.turns.len(), fk.folder.as_str(), fk.turns[0].prompt.as_str()), (AgentTool::Codex, 1, other.to_str().unwrap(), "Design the cache"));
    let lin = fk.ext.lineage.clone().unwrap();
    assert_eq!((lin.fork.clone().map(|f| (f.key, f.turn)), lin.handoffs.len()), (Some((key.clone(), 0)), 1));
    assert!(lin.pending.is_some());
    // The source is as it was: its turns, provider and conversation.
    let src = k.get(s.id).unwrap();
    assert_eq!((src.turns.len(), src.tool, src.kiro_id.as_deref()), (2, AgentTool::Kiro, Some("kiro-conv")));
    // The fork's agent starts from an account; the fork goes on on its own.
    assert!(k.reply(fk.id, "What about 64 MB?", vec![]));
    done(&k, fk.id, 2);
    let (tool, prompt, resume) = log.lock().unwrap().last().cloned().unwrap();
    assert_eq!((tool, resume), (AgentTool::Codex, None));
    assert!(prompt.contains("a fork of another, taken after turn 1") && prompt.ends_with("What about 64 MB?"), "{prompt}");
    assert_eq!(k.get(s.id).unwrap().turns.len(), 2);
    // Only from a turn that ended: not a running one, not one that isn't there.
    *hold.lock().unwrap() = true;
    assert!(k.reply(s.id, "third [hold]", vec![]));
    wait_for("the hold", || k.get(s.id).unwrap().busy());
    assert!(k.fork(&key, 2, &Target::parse("kiro").unwrap(), &f, None).unwrap_err().contains("has ended"));
    assert!(k.fork(&key, 9, &Target::parse("kiro").unwrap(), &f, None).unwrap_err().contains("isn’t there"));
    *hold.lock().unwrap() = false;
    done(&k, s.id, 3);
}

#[test]
fn findings_come_back_as_one_message_once_and_change_no_files() {
    let (k, _, f, _) = rig("back");
    let parent = k.start(AgentTool::Kiro, &f, "Design the cache", vec![]).unwrap();
    done(&k, parent.id, 1);
    let pkey = k.get(parent.id).unwrap().key;
    let fk = k.fork(&pkey, 0, &Target::parse("codex").unwrap(), &f, None).unwrap();
    assert!(k.bring_findings_back(&fk.key, None).unwrap_err().contains("Nothing was asked"), "nothing to bring yet");
    assert!(k.reply(fk.id, "Try the risky approach", vec![]));
    done(&k, fk.id, 2);
    let files_before: Vec<_> = std::fs::read_dir(&f).unwrap().flatten().map(|e| e.file_name()).collect();
    let chars = k.bring_findings_back(&fk.key, None).unwrap();
    assert!(chars > 0);
    done(&k, parent.id, 2);
    let msg = k.get(parent.id).unwrap().turns[1].prompt.clone();
    assert!(msg.contains("hover-return:") && msg.contains("Asked: Try the risky approach") && msg.contains("does not merge any code") && msg.contains("Found: codex says"), "{msg}");
    assert_eq!(k.get(parent.id).unwrap().turns[1].chips[0].kind, "thread", "the fork is referenced, not copied");
    assert_eq!(k.get(parent.id).unwrap().ext.lineage.unwrap().returned.len(), 1);
    assert_eq!(std::fs::read_dir(&f).unwrap().flatten().map(|e| e.file_name()).collect::<Vec<_>>().len(), files_before.len(), "no file moved");
    // A retry finds it done; a fork that went on has new findings.
    assert_eq!(k.bring_findings_back(&fk.key, None).unwrap(), 0);
    assert_eq!(k.get(parent.id).unwrap().turns.len(), 2, "no second message");
    assert!(k.reply(fk.id, "And measure it", vec![]));
    done(&k, fk.id, 3);
    assert!(k.bring_findings_back(&fk.key, None).unwrap() > 0);
    done(&k, parent.id, 3);
}

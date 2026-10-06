//! A conversation with a custom agent belongs to that agent: the record names it, a restart brings it back to the same one, and one
//! whose agent was removed says so without losing the conversation. Sessions saved before custom agents read as they did.

use hover_agents::session::{KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_core::crypto::Crypto;
use hover_core::ext::SessionExt;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } assert!(f(), "timed out"); }

#[test]
fn a_custom_conversation_keeps_its_agent_across_a_restart_and_survives_the_agents_removal() {
    let d = std::env::temp_dir().join(format!("hover-customsess-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    let folder = d.to_string_lossy().into_owned();
    let crypto = Arc::new(Crypto::with_key([4; 32]));
    let which: Arc<Mutex<Vec<String>>> = Default::default();
    let make = |w: Arc<Mutex<Vec<String>>>| move |_: AgentTool| -> RunTask { let w = w.clone(); Arc::new(move |_: RunArgs| { w.lock().unwrap().push("built-in".into()); KiroResult::new(KiroState::Completed, "built-in") }) };
    let hist = || Some(Arc::new(AgentHistory::new(d.join("history"), crypto.clone())));
    let k = KiroSessions::new(make(which.clone()), hist());
    let w2 = which.clone();
    k.set_custom(move |id| { let w = w2.clone(); let id = id.to_owned(); Some(Arc::new(move |_: RunArgs| { w.lock().unwrap().push(format!("custom {id}")); KiroResult::new(KiroState::Completed, "from the custom agent") }) as RunTask) });
    let ext = SessionExt { provider: Some("ca-one".into()), ..Default::default() };
    let s = k.start_bound(AgentTool::Custom, &folder, "hi", vec![], None, None, ext).unwrap();
    wait_for(|| k.get(s.id).is_some_and(|x| !x.busy()));
    assert_eq!(which.lock().unwrap().as_slice(), ["custom ca-one"]);
    // A plain session next to it still uses its own tool.
    let plain = k.start(AgentTool::Codex, &folder, "hi", vec![]).unwrap();
    wait_for(|| k.get(plain.id).is_some_and(|x| !x.busy()));
    assert_eq!(which.lock().unwrap().last().unwrap(), "built-in");
    k.history().unwrap().flush();
    // After a restart, with the agent still there: a reply goes to the same agent.
    let again = KiroSessions::new(make(which.clone()), hist());
    let w3 = which.clone();
    again.set_custom(move |id| { let w = w3.clone(); let id = id.to_owned(); Some(Arc::new(move |_: RunArgs| { w.lock().unwrap().push(format!("custom {id}")); KiroResult::new(KiroState::Completed, "again") }) as RunTask) });
    let woke = again.wake(&s.key).unwrap();
    assert_eq!((woke.tool, woke.ext.provider.as_deref()), (AgentTool::Custom, Some("ca-one")));
    assert!(again.reply(woke.id, "more", vec![]));
    wait_for(|| again.get(woke.id).is_some_and(|x| !x.busy() && x.turns.len() == 2 && x.turns[1].result.is_some()));
    assert_eq!(which.lock().unwrap().last().unwrap(), "custom ca-one");
    // The agent is removed: the conversation stays, and a reply says what to do instead of using some other agent.
    let gone = KiroSessions::new(make(which.clone()), hist());
    gone.set_custom(|_| None);
    let woke = gone.wake(&s.key).unwrap();
    assert_eq!(woke.turns.len(), 2, "the whole conversation is kept");
    let before = which.lock().unwrap().len();
    assert!(gone.reply(woke.id, "still there?", vec![]));
    wait_for(|| gone.get(woke.id).is_some_and(|x| !x.busy() && x.turns.len() == 3 && x.turns[2].result.is_some()));
    let r = gone.get(woke.id).unwrap().turns[2].result.clone().unwrap();
    assert_eq!(r.state, KiroState::Failed);
    assert!(r.text.contains("isn’t set up any more"), "{}", r.text);
    assert_eq!(which.lock().unwrap().len(), before, "no other agent took it");
    // A session saved before any of this (no Ext) is a built-in session, as it was.
    let old = KiroSessions::new(make(which.clone()), hist()).wake(&plain.key).unwrap();
    assert_eq!((old.tool, old.ext.provider), (AgentTool::Codex, None));
}

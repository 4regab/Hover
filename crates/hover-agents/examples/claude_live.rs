//! Claude Code by hand: the real `claude` CLI, through ClaudeHost.
//!   cargo run --release -p hover-agents --example claude_live -- <folder> "<prompt>" [access] ["<reply>"] [model]
//! Access is full (the default), risky, always, read or none. The reply, if any, goes
//! to the same conversation after the first turn. HOVER_LIVE_DENY=1 denies approvals
//! (else they are allowed); a question gets its first choice. Point it at
//! fake-anthropic (tools/hover-measure) with ANTHROPIC_BASE_URL to run it offline.

use hover_agents::ask::AskAnswer;
use hover_agents::cancel::Cancel;
use hover_agents::claude::ClaudeHost;
use hover_core::model::AgentOptions;
use std::sync::{Arc, Mutex};

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let folder = a.first().cloned().expect("a folder");
    let prompt = a.get(1).cloned().unwrap_or_else(|| "Say hello in five words.".into());
    let access = a.get(2).cloned().filter(|x| x != "full");
    let reply = a.get(3).cloned().filter(|x| !x.is_empty());
    let model = a.get(4).cloned().filter(|m| !m.is_empty());
    let host = ClaudeHost::new(move || AgentOptions { model: model.clone(), ..Default::default() });
    host.on_options_seen(|_, o| for x in o { println!("offer {}: {}", x.id, x.choices.iter().map(|c| format!("{} {:?}", c.value, c.levels)).collect::<Vec<_>>().join(", ")); });
    let deny = std::env::var("HOVER_LIVE_DENY").is_ok_and(|v| v == "1");
    host.set_asking(Arc::new(move |sid, ask, _, reply| {
        println!("ASK [{sid}] {} {} {:?} {:?} - {}", ask.kind, ask.title, ask.command, ask.path, ask.reason);
        reply(if deny { AskAnswer::Deny } else { AskAnswer::Allow })
    }));
    host.set_questioning(Arc::new(|_, ask, _, reply| {
        let q = ask.questions.unwrap();
        println!("QUESTION {:?}", q);
        reply(Some(q.iter().map(|x| vec![x.options.first().map(|o| o.0.clone()).unwrap_or_default()]).collect()))
    }));
    let sid: Arc<Mutex<Option<String>>> = Default::default();
    let turn = |prompt: &str, resume: Option<String>| {
        let s2 = sid.clone();
        let t = std::time::Instant::now();
        let r = host.run(&folder, prompt, Some(Box::new(|p| println!("phase {p:?}"))), &Cancel::new(), resume.as_deref(),
            Some(Box::new(move |e| {
                if let Some(s) = e.step { println!("step {} {} {:?} {} {:?}", s.kind, s.title, s.target, s.status, s.diff.or(s.output)); }
                if let Some(c) = e.context { println!("ctx {c:.1}"); }
                if let Some(i) = e.session_id { println!("session {i}"); *s2.lock().unwrap() = Some(i); }
            })),
            access.as_deref());
        println!("RESULT {:?} after {:.1}s:\n{}\n", r.state, t.elapsed().as_secs_f64(), r.text);
    };
    turn(&prompt, None);
    if let Some(r) = reply {
        let resume = sid.lock().unwrap().clone();
        turn(&r, resume);
    }
    println!("live processes: {}", host.live());
    host.shutdown("done");
}

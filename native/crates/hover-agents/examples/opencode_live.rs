//! OpenCodeLiveTests, by hand: a real "opencode serve" and a real model.
//!   cargo run --release -p hover-agents --example opencode_live -- <folder> "<prompt>" [model] [access]
//! HOVER_LIVE_ANSWER picks the answer to a question (default: the first choice);
//! approvals are allowed.

use hover_agents::ask::AskAnswer;
use hover_agents::cancel::Cancel;
use hover_agents::opencode::OpenCodeHost;
use hover_core::model::AgentOptions;
use std::sync::Arc;

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let folder = a.first().cloned().expect("a folder");
    let prompt = a.get(1).cloned().unwrap_or_else(|| "Say hello in five words.".into());
    let model = a.get(2).cloned().filter(|m| !m.is_empty()).or_else(|| Some("opencode/big-pickle".into()));
    let access = a.get(3).cloned();
    let host = OpenCodeHost::new(move || AgentOptions { model: model.clone(), ..Default::default() });
    host.on_options_seen(|_, o| for x in o { println!("offer {} ({} choices)", x.id, x.choices.len()); });
    host.set_asking(Arc::new(|_, ask, _, reply| { println!("ASK {} {:?} {:?} - {}", ask.kind, ask.command, ask.path, ask.reason); reply(AskAnswer::Allow) }));
    host.set_questioning(Arc::new(|_, ask, _, reply| {
        let q = ask.questions.unwrap();
        println!("QUESTION {:?}", q);
        let pick = std::env::var("HOVER_LIVE_ANSWER").unwrap_or_else(|_| q[0].options.first().map(|o| o.0.clone()).unwrap_or_default());
        reply(Some(q.iter().map(|_| vec![pick.clone()]).collect()))
    }));
    let t = std::time::Instant::now();
    let r = host.run(&folder, &prompt, Some(Box::new(|p| println!("phase {p:?}"))), &Cancel::new(), None,
        Some(Box::new(|e| {
            if let Some(s) = e.step { println!("step {} {} {:?} {}", s.kind, s.title, s.target, s.status); }
            if let Some(c) = e.context { println!("ctx {c:.1}"); }
            if let Some(i) = e.session_id { println!("session {i}"); }
        })),
        access.as_deref());
    println!("RESULT {:?} after {:.0}s:\n{}", r.state, t.elapsed().as_secs_f64(), r.text);
    host.shutdown("done");
}

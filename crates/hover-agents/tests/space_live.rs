//! Two agents on a real project desktop (a Cua Space on this Mac), each through a driver
//! session of its own. Run by hand: it starts (and then stops) the VM.
//!
//! HOVER_LIVE_SPACE=<project folder> cargo test -p hover-agents --test space_live -- --ignored --nocapture

use hover_agents::space_driver::Driver;
use hover_agents::spaces;
use hover_core::json::{self, Json};
use std::io::{BufRead, BufReader, Write};
use std::time::Instant;

fn agent(folder: String, tag: &str, calls: &[(&str, &str)]) -> Vec<Json> {
    let (from_agent_r, mut from_agent_w) = std::io::pipe().unwrap();
    let (to_agent_r, to_agent_w) = std::io::pipe().unwrap();
    let d = Driver::new(move || Some(folder.clone()), &format!("hover-{tag}"));
    let server = std::thread::spawn(move || d.run(Box::new(BufReader::new(from_agent_r)), Box::new(to_agent_w)));
    let mut lines = vec![r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"live","version":"1"}}}"#.to_owned(),
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#.to_owned(), r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#.to_owned()];
    for (i, (name, args)) in calls.iter().enumerate() {
        lines.push(format!(r#"{{"jsonrpc":"2.0","id":{},"method":"tools/call","params":{{"name":"{name}","arguments":{args}}}}}"#, i + 3));
    }
    let want = lines.iter().filter(|l| l.contains("\"id\"")).count();
    let mut out = BufReader::new(to_agent_r);
    let mut replies = vec![];
    for l in &lines {
        writeln!(from_agent_w, "{l}").unwrap();
        if !l.contains("\"id\"") { continue; }
        // One at a time, so the replies come back in order.
        let mut s = String::new();
        out.read_line(&mut s).unwrap();
        replies.push(json::parse(s.trim()).unwrap());
    }
    assert_eq!(replies.len(), want);
    drop(from_agent_w);
    server.join().unwrap();
    replies
}

fn summary(r: &Json) -> String { let c = r.compact(); c.chars().take(300).collect() }

#[test]
#[ignore = "starts a real VM"]
fn two_agents_work_on_one_real_desktop_each_in_its_own_session() {
    let Some(folder) = std::env::var("HOVER_LIVE_SPACE").ok() else { eprintln!("set HOVER_LIVE_SPACE"); return };
    spaces::set_source(|| spaces::Switches { on: true, linux: false });
    assert!(spaces::wanted(), "agent desktops can't run here");
    // Only a desktop that is already made: this never makes (downloads) one.
    let (_, listed) = spaces::run(&spaces::exe().expect("cua"), std::time::Duration::from_secs(30), &["sb", "ls", "--local", "--json"]);
    assert!(spaces::parse_list(&listed).iter().any(|s| s.id == spaces::id_for(&folder)), "no desktop {} on this Mac: {listed}", spaces::id_for(&folder));
    let t = Instant::now();
    if let Some(why) = spaces::ensure(&folder, &hover_agents::cancel::Cancel::new()) { panic!("{why}"); }
    eprintln!("desktop {} ready in {:?}", spaces::name_for(&folder), t.elapsed());
    let calls: &[(&str, &str)] = &[("list_apps", "{}"), ("get_cursor_position", "{}")];
    let (a, b) = (folder.clone(), folder.clone());
    let one = std::thread::spawn(move || agent(a, "live-one", calls));
    let two = std::thread::spawn(move || agent(b, "live-two", calls));
    let (one, two) = (one.join().unwrap(), two.join().unwrap());
    for (who, r) in [("one", &one), ("two", &two)] {
        for x in r { eprintln!("{who}: {}", summary(x)); }
        let Some(Json::Arr(tools)) = r[1].get("result").and_then(|x| x.get("tools")) else { panic!("{who}: no tools") };
        assert!(tools.iter().all(|t| t.get("name").and_then(Json::as_str) != Some("kill_app")), "{who}: a denied tool is listed");
        for x in &r[2..] { assert!(x.get("error").is_none() && x.get("result").and_then(|r| r.get("isError")) != Some(&Json::Bool(true)), "{who}: {}", summary(x)); }
    }
    spaces::stop(&folder);
    eprintln!("stopped after {:?}", t.elapsed());
}

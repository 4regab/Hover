//! AcpHost and the sessions against port/tools/FakeAcp, the stand-in agent both
//! builds are benchmarked with. Each scenario runs from a recording in
//! native/golden/acp (what Hover sent, `> `, and what FakeAcp answered, `< `), so it
//! needs no .NET: the replay checks every line Hover sends against the recording and
//! answers with FakeAcp's own bytes. With FAKEACP=<path to the FakeAcp binary> the
//! same scenarios run against the real process instead, and HOVER_RECORD=1 writes the
//! recordings again.
//!
//! What Hover sends is also what the C# AcpHost sends: the recorded `> ` lines are the
//! anonymous objects of AcpHost.cs as System.Text.Json writes them (checked by hand
//! against the source when recorded; see the Phase 3 report).

use hover_agents::acp::AcpHost;
use hover_agents::cancel::Cancel;
use hover_agents::proc::{launch_grouped, Group, Link};
use hover_agents::session::KiroSessions;
use hover_agents::stream::{KiroEvent, KiroPhase};
use hover_core::model::{AgentOptions, AgentTool, KiroState};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn golden(name: &str) -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/acp").join(format!("{name}.txt")) }
fn rich() -> String { std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/fixtures/rich.md")).unwrap() }

fn folder(name: &str) -> String {
    let d = std::env::temp_dir().join(format!("hover-fakeacp-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&d).unwrap();
    d.to_string_lossy().into_owned()
}

fn live() -> Option<PathBuf> { std::env::var_os("FAKEACP").map(PathBuf::from).filter(|p| p.is_file()) }

// MARK: Live, with a recorder between Hover and FakeAcp

#[derive(Default)]
struct Log { lines: Vec<String>, pids: Vec<String> }

struct Tee<T> { inner: T, buf: Vec<u8>, dir: &'static str, log: Arc<Mutex<Log>> }

impl<T> Tee<T> {
    fn take_lines(&mut self) {
        while let Some(i) = self.buf.iter().position(|&b| b == b'\n') {
            let line: Vec<u8> = self.buf.drain(..=i).collect();
            self.log.lock().unwrap().lines.push(format!("{}{}", self.dir, String::from_utf8_lossy(&line[..line.len() - 1])));
        }
    }
}

impl<T: Write> Write for Tee<T> {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> { let n = self.inner.write(b)?; self.buf.extend_from_slice(&b[..n]); self.take_lines(); Ok(n) }
    fn flush(&mut self) -> std::io::Result<()> { self.inner.flush() }
}

impl<T: Read> Read for Tee<T> {
    fn read(&mut self, b: &mut [u8]) -> std::io::Result<usize> {
        let n = self.inner.read(b)?;
        self.buf.extend_from_slice(&b[..n]);
        self.take_lines();
        if n == 0 { self.log.lock().unwrap().lines.push("! eof".into()); }
        Ok(n)
    }
}

struct Live { log: Arc<Mutex<Log>>, groups: Arc<Mutex<Vec<Arc<Group>>>> }

fn live_connect(exe: PathBuf, env: Vec<(String, String)>) -> (Live, impl Fn() -> std::io::Result<Option<Link>> + Send + Sync) {
    let log: Arc<Mutex<Log>> = Default::default();
    let groups: Arc<Mutex<Vec<Arc<Group>>>> = Default::default();
    let (l, g) = (log.clone(), groups.clone());
    let connect = move || {
        let (link, group) = launch_grouped(&exe, &["acp"], &env)?;
        l.lock().unwrap().pids.push(group.pid().unwrap().to_string());
        g.lock().unwrap().push(group);
        let Link { to_agent, from_agent, kill, errors } = link;
        Ok(Some(Link {
            to_agent: Box::new(Tee { inner: to_agent, buf: vec![], dir: "> ", log: l.clone() }),
            from_agent: Box::new(Tee { inner: from_agent, buf: vec![], dir: "< ", log: l.clone() }),
            kill, errors,
        }))
    };
    (Live { log, groups }, connect)
}

impl Live {
    /// The recording, with this run's folder and FakeAcp's process ids made stable.
    fn save(&self, name: &str, folder: &str) {
        if std::env::var_os("HOVER_RECORD").is_none() { return; }
        let log = self.log.lock().unwrap();
        let esc = hover_core::json::Json::str(folder).compact();
        let esc = &esc[1..esc.len() - 1];
        let mut text = String::new();
        for l in &log.lines {
            let mut l = l.replace(esc, "{folder}");
            for (i, pid) in log.pids.iter().enumerate() { l = l.replace(&format!("fake-{pid}-"), &format!("fake-P{}-", i + 1)); }
            text.push_str(&l);
            text.push('\n');
        }
        std::fs::create_dir_all(golden(name).parent().unwrap()).unwrap();
        std::fs::write(golden(name), text).unwrap();
    }
}

// MARK: Replay

/// Plays FakeAcp's side of a recording: each line Hover sends is checked against the
/// next `> ` line, then the `< ` lines that followed it are sent back.
struct Replay { script: Arc<Mutex<VecDeque<String>>>, wrong: Arc<Mutex<Vec<String>>> }

fn replay_connect(name: &str, folder: &str) -> (Replay, impl Fn() -> std::io::Result<Option<Link>> + Send + Sync) {
    let esc = hover_core::json::Json::str(folder).compact();
    let esc = esc[1..esc.len() - 1].to_owned();
    let text = std::fs::read_to_string(golden(name)).unwrap_or_else(|_| panic!("no recording {name}: run with FAKEACP and HOVER_RECORD=1"));
    let script: Arc<Mutex<VecDeque<String>>> = Arc::new(Mutex::new(text.lines().map(|l| l.replace("{folder}", &esc)).collect()));
    let wrong: Arc<Mutex<Vec<String>>> = Default::default();
    let (s, w) = (script.clone(), wrong.clone());
    let connect = move || {
        let (hover_reads, agent_writes) = std::io::pipe()?;
        let (agent_reads, hover_writes) = std::io::pipe()?;
        let out = Arc::new(Mutex::new(Some(agent_writes)));
        let (s, w, o) = (s.clone(), w.clone(), out.clone());
        let emit = move |s: &Mutex<VecDeque<String>>, o: &Mutex<Option<std::io::PipeWriter>>| loop {
            let next = { let mut g = s.lock().unwrap(); match g.front() { Some(l) if l.starts_with("< ") || l == "! eof" => g.pop_front(), _ => None } };
            match next {
                Some(l) if l == "! eof" => { o.lock().unwrap().take(); return; }
                Some(l) => {
                    // Paced, so a turn takes some time (a reply can queue behind it).
                    std::thread::sleep(Duration::from_millis(3));
                    if let Some(wr) = o.lock().unwrap().as_mut() { let _ = writeln!(wr, "{}", &l[2..]); }
                }
                None => return,
            }
        };
        std::thread::spawn(move || {
            emit(&s, &o);
            for line in BufReader::new(agent_reads).lines() {
                let Ok(line) = line else { break };
                let want = s.lock().unwrap().pop_front();
                if want.as_deref() != Some(&format!("> {line}")) { w.lock().unwrap().push(format!("sent  {line}\nwanted {want:?}")); }
                emit(&s, &o);
            }
        });
        let o2 = out.clone();
        Ok(Some(Link { to_agent: Box::new(hover_writes), from_agent: Box::new(hover_reads), kill: Box::new(move || { o2.lock().unwrap().take(); }), errors: Box::new(String::new) }))
    };
    (Replay { script, wrong }, connect)
}

impl Replay {
    fn check(&self) {
        let w = self.wrong.lock().unwrap();
        assert!(w.is_empty(), "Hover sent other lines than recorded:\n{}", w.join("\n"));
        let left: Vec<String> = self.script.lock().unwrap().iter().filter(|l| l.starts_with("> ")).cloned().collect();
        assert!(left.is_empty(), "Hover didn't send:\n{}", left.join("\n"));
    }
}

/// A scenario, live when FAKEACP is set, else replayed.
type Kill = Arc<dyn Fn() + Send + Sync>;

fn scenario(name: &str, env: &[(&str, &str)], body: impl FnOnce(&AcpHost, &str, Kill)) {
    let dir = folder(name);
    let env: Vec<(String, String)> = env.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
    match live() {
        Some(exe) => {
            let (l, connect) = live_connect(exe, env);
            let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, connect);
            let groups = l.groups.clone();
            body(&host, &dir, Arc::new(move || { if let Some(g) = groups.lock().unwrap().last() { g.kill(); } }));
            host.shutdown("test");
            std::thread::sleep(Duration::from_millis(200));
            l.save(name, &dir);
        }
        None => {
            let (r, connect) = replay_connect(name, &dir);
            let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, connect);
            // Replayed, the recording's end of stream stands for the kill.
            body(&host, &dir, Arc::new(|| {}));
            host.shutdown("test");
            r.check();
        }
    }
}

type Seen<T> = Arc<Mutex<Vec<T>>>;
type Recorders = (Seen<KiroPhase>, Seen<KiroEvent>, Box<dyn Fn(KiroPhase) + Send + Sync>, Box<dyn Fn(KiroEvent) + Send + Sync>);

fn recorders() -> Recorders {
    let (p, e): (Seen<KiroPhase>, Seen<KiroEvent>) = Default::default();
    let (p2, e2) = (p.clone(), e.clone());
    (p, e, Box::new(move |x| p2.lock().unwrap().push(x)), Box::new(move |x| e2.lock().unwrap().push(x)))
}

/// One second at 20 updates a second: two steps (read, search), usage twice, thinking
/// between, then the rich answer; a reply goes to the same conversation.
#[test]
fn a_turn_and_a_reply() {
    let answer = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/fixtures/rich.md");
    scenario("turn-and-reply", &[("FAKEACP_SECONDS", "1"), ("FAKEACP_RATE", "20"), ("FAKEACP_ANSWER", answer.to_str().unwrap())], |host, dir, _| {
        let (phases, events, p, e) = recorders();
        let r = host.run(dir, "Fix the \"flaky\" tests & say what changed", Some(p), &Cancel::new(), None, Some(e));
        assert_eq!(r.state, KiroState::Completed);
        assert_eq!(r.text, rich().trim(), "the answer comes back whole, escapes and all");
        let ev = events.lock().unwrap();
        let sid = ev.iter().find_map(|e| e.session_id.clone()).unwrap();
        // The reasoning it sent is kept too, as thoughts between the tool calls.
        let order: Vec<&str> = ev.iter().filter_map(|e| e.step.as_ref()).filter(|s| s.status == "completed").map(|s| s.kind.as_str()).collect();
        assert_eq!(order, ["thought", "read", "thought", "thought", "search", "thought"]);
        assert!(ev.iter().filter_map(|e| e.step.as_ref()).filter(|s| s.kind == "thought" && s.status == "completed").all(|s| s.output.as_deref().is_some_and(|o| o.starts_with("thinking")) && s.ms.is_some()));
        let steps: Vec<(String, String, String, Option<String>)> = ev.iter().filter_map(|e| e.step.as_ref()).filter(|s| s.kind != "thought")
            .map(|s| (s.id.clone(), s.kind.clone(), s.status.clone(), s.target.clone())).collect();
        assert_eq!(steps, [
            ("t0".into(), "read".into(), "in_progress".into(), Some("src/file0.cs".into())),
            ("t0".into(), "read".into(), "completed".into(), Some("src/file0.cs".into())),
            ("t10".into(), "search".into(), "in_progress".into(), Some("src/file1.cs".into())),
            ("t10".into(), "search".into(), "completed".into(), Some("src/file1.cs".into())),
        ]);
        // usage_update 1280 then 1680 of 200000: 0.64 %, then 0.84 % is within half a point.
        assert_eq!(ev.iter().filter_map(|e| e.context).collect::<Vec<_>>(), [0.64]);
        assert_eq!(*phases.lock().unwrap(), [KiroPhase::Starting, KiroPhase::Reading, KiroPhase::Thinking, KiroPhase::Searching, KiroPhase::Thinking, KiroPhase::Writing]);
        drop(ev);
        let again = host.run(dir, "and the docs", None, &Cancel::new(), Some(&sid), None);
        assert_eq!(again.state, KiroState::Completed);
    });
}

/// A long run stopped at its second step: session/cancel, then FakeAcp's cancelled.
#[test]
fn a_run_is_stopped() {
    scenario("stop", &[("FAKEACP_SECONDS", "30"), ("FAKEACP_RATE", "20")], |host, dir, _| {
        let ct = Cancel::new();
        let c2 = ct.clone();
        let e: Box<dyn Fn(KiroEvent) + Send + Sync> = Box::new(move |e| { if e.step.as_ref().is_some_and(|s| s.id == "t10") { c2.cancel(); } });
        let t = Instant::now();
        let r = host.run(dir, "a long task", None, &ct, None, Some(e));
        assert_eq!((r.state, r.text.as_str()), (KiroState::Cancelled, "Stopped before Kiro finished."));
        assert!(t.elapsed() < Duration::from_secs(5), "stopped at once, not after 30 s");
    });
}

/// The tool shut down for being idle: the reply starts it again, loads the
/// conversation (session/load) and carries on in it.
#[test]
fn an_idle_tool_comes_back_with_the_conversation() {
    scenario("load", &[("FAKEACP_SECONDS", "0.2"), ("FAKEACP_RATE", "20")], |host, dir, _| {
        let (_, events, _, e) = recorders();
        host.run(dir, "first", None, &Cancel::new(), None, Some(e));
        let sid = events.lock().unwrap().iter().find_map(|e| e.session_id.clone()).unwrap();
        host.shutdown("idle");
        // The reader thread notices the closed pipe a moment later; under load that
        // can come after this line, so the test waits for it.
        let t = Instant::now();
        while host.alive() && t.elapsed() < Duration::from_secs(2) { std::thread::sleep(Duration::from_millis(10)); }
        assert!(!host.alive());
        let r = host.run(dir, "second", None, &Cancel::new(), Some(&sid), None);
        assert_eq!(r.state, KiroState::Completed);
    });
}

/// FakeAcp killed mid-run: the run fails, saying so.
#[test]
fn a_tool_that_is_killed_fails_its_run() {
    scenario("killed", &[("FAKEACP_SECONDS", "30"), ("FAKEACP_RATE", "20")], |host, dir, kill| {
        let fired = std::sync::atomic::AtomicBool::new(false);
        // Killed from outside at the first step, as a crash or the OOM killer would.
        let e: Box<dyn Fn(KiroEvent) + Send + Sync> = Box::new(move |e| {
            if e.step.is_some() && !fired.swap(true, std::sync::atomic::Ordering::SeqCst) { let k = kill.clone(); std::thread::spawn(move || k()); }
        });
        let r = host.run(dir, "a long task", None, &Cancel::new(), None, Some(e));
        assert_eq!(r.state, KiroState::Failed);
        assert!(r.text.starts_with("Kiro stopped unexpectedly."), "{}", r.text);
    });
}

/// Sessions on FakeAcp: a reply sent while the first turn runs waits, then goes to
/// the same conversation; no config options are offered, so none are set.
#[test]
fn a_queued_reply_follows_in_the_same_conversation() {
    scenario("queue", &[("FAKEACP_SECONDS", "0.5"), ("FAKEACP_RATE", "20")], |host, dir, _| {
        let offered = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let o2 = offered.clone();
        host.on_options_seen(move |_, _| o2.store(true, std::sync::atomic::Ordering::SeqCst));
        let h = host.clone();
        let k = KiroSessions::new(move |_| h.runner(), None);
        let s = k.start(AgentTool::Kiro, dir, "first", vec![]).unwrap();
        let t = Instant::now();
        while k.get(s.id).unwrap().kiro_id.is_none() && t.elapsed() < Duration::from_secs(10) { std::thread::sleep(Duration::from_millis(10)); }
        assert!(k.reply(s.id, "second", vec![]));
        assert!(k.get(s.id).unwrap().turns[1].queued);
        while k.get(s.id).unwrap().turns.iter().any(|t| t.result.is_none()) && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
        let s = k.get(s.id).unwrap();
        assert_eq!(s.turns.iter().map(|t| t.result.as_ref().unwrap().text.as_str()).collect::<Vec<_>>(), ["Done. Nothing needed changing."; 2]);
        assert_eq!(s.turns[0].steps.iter().filter(|x| x.kind != "thought").count(), 1, "one tool call in half a second");
        assert!(s.turns[0].woke_at.is_some());
        assert!(!offered.load(std::sync::atomic::Ordering::SeqCst));
    });
}

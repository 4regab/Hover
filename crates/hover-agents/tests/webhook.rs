//! Webhooks: signed, once, bounded, local by default, and only the chosen fields reach the prompt.

use hover_agents::orch::{Env, Provider};
use hover_agents::sched::{Hook, NewTask, RunState, Schedule, Scheduler, Tz};
use hover_agents::session::{KiroSession, KiroSessions, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::wake::Wake;
use hover_agents::webhook::{hex, hmac_sha256, Hooks, Outcome, Request};
use hover_core::crypto::Crypto;
use hover_core::model::{AgentTool, DelegationLimits, KiroState};
use hover_core::secrets::Secrets;
use hover_core::store::Sealed;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = Instant::now();
    while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); }
    assert!(f(), "timed out waiting for {what}");
}

struct E(PathBuf);
impl Env for E {
    fn providers(&self) -> Vec<Provider> { vec![Provider { id: "kiro".into(), name: "Kiro".into(), tool: AgentTool::Kiro, instance: None, ready: true, hint: String::new(), read_only: true, resume: true, leads: true }] }
    fn access_of(&self, _: &KiroSession) -> String { "full".into() }
    fn limits(&self) -> DelegationLimits { Default::default() }
    fn worktrees(&self) -> PathBuf { self.0.join("wt") }
}

struct Rig { s: Arc<Scheduler>, h: Arc<Hooks>, k: KiroSessions, folder: String, seen: Arc<Mutex<Vec<String>>>, hold: Arc<Mutex<bool>>, root: PathBuf, crypto: Arc<Crypto> }

fn rig(name: &str) -> Rig {
    let root = std::env::temp_dir().join(format!("hover-hook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let folder = root.join("p");
    std::fs::create_dir_all(&folder).unwrap();
    let crypto = Arc::new(Crypto::with_key([5; 32]));
    let (seen, hold): (Arc<Mutex<Vec<String>>>, Arc<Mutex<bool>>) = Default::default();
    let (s2, h2) = (seen.clone(), hold.clone());
    let k = KiroSessions::new(move |_| -> RunTask {
        let (s2, h2) = (s2.clone(), h2.clone());
        Arc::new(move |a: RunArgs| { s2.lock().unwrap().push(a.prompt.clone()); while *h2.lock().unwrap() && !a.ct.is_cancelled() { std::thread::sleep(Duration::from_millis(5)); } KiroResult::new(KiroState::Completed, "ok") })
    }, None);
    let s = Scheduler::new(k.clone(), Arc::new(E(root.clone())), Wake::new(None), Some(Sealed::in_dir(&root.join("st"), "tasks", crypto.clone())));
    let h = Hooks::new(s.clone(), Arc::new(Secrets::new(root.join("secrets.dat"), Some(crypto.clone()))), Some(Sealed::in_dir(&root.join("st"), "hooks", crypto.clone())));
    Rig { s, h, k, folder: folder.to_string_lossy().into_owned(), seen, hold, root, crypto }
}

fn task(r: &Rig, hook: Hook) -> String {
    r.s.add(NewTask { name: "On push".into(), folder: r.folder.clone(), prompt: "Review the push.".into(), provider: "kiro".into(), workspace: "folder".into(), access: "risky".into(), schedule: Schedule::Manual, tz: Tz::Fixed(0), hook: Some(hook) }).unwrap()
}

fn hook(fields: &[&str], events: &[&str]) -> Hook { Hook { enabled: true, events: events.iter().map(|s| s.to_string()).collect(), fields: fields.iter().map(|s| s.to_string()).collect() } }

fn github(task: &str, secret: &str, delivery: &str, event: &str, body: &str) -> Request {
    let sig = format!("sha256={}", hex(&hmac_sha256(secret.as_bytes(), body.as_bytes())));
    Request { path: format!("/hook/{task}"), headers: vec![("x-hub-signature-256".into(), sig), ("x-github-delivery".into(), delivery.into()), ("x-github-event".into(), event.into())], body: body.as_bytes().to_vec() }
}

fn hover_signed(task: &str, secret: &str, delivery: &str, ts: i64, body: &str) -> Request {
    let sig = format!("sha256={}", hex(&hmac_sha256(secret.as_bytes(), format!("{ts}.{body}").as_bytes())));
    Request { path: format!("/hook/{task}"), headers: vec![("x-hover-signature-256".into(), sig), ("x-hover-timestamp".into(), ts.to_string()), ("x-hover-delivery".into(), delivery.into())], body: body.as_bytes().to_vec() }
}

const BODY: &str = r#"{"pull_request":{"title":"Fix login","number":12},"sender":{"token":"must-not-reach-the-agent"}}"#;

#[test]
fn a_signed_delivery_starts_one_run_with_only_the_chosen_fields_and_a_repeat_is_refused() {
    let r = rig("ok");
    let id = task(&r, hook(&["/pull_request/title", "/pull_request/number"], &["pull_request"]));
    let secret = r.h.rotate(&id).unwrap();
    assert!(r.h.has_secret(&id) && secret.len() == 64);
    assert_eq!(r.h.handle(&github(&id, &secret, "d-1", "pull_request", BODY)), Outcome::Accepted);
    wait_for("the run", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    let prompt = r.seen.lock().unwrap()[0].clone();
    assert!(prompt.starts_with("Review the push.") && prompt.contains("- /pull_request/title: Fix login") && prompt.contains("- /pull_request/number: 12"), "{prompt}");
    assert!(!prompt.contains("must-not-reach-the-agent"), "fields that weren't chosen stay out: {prompt}");
    assert_eq!(r.h.handle(&github(&id, &secret, "d-1", "pull_request", BODY)), Outcome::Duplicate);
    std::thread::sleep(Duration::from_millis(100));
    assert_eq!(r.seen.lock().unwrap().len(), 1, "a delivery makes one run");
    // Other events are filtered out, after they have been checked and remembered.
    assert_eq!(r.h.handle(&github(&id, &secret, "d-2", "issues", BODY)), Outcome::Filtered);
    assert_eq!(r.seen.lock().unwrap().len(), 1);
    // The log says what happened to each.
    let log: Vec<_> = r.h.log().iter().map(|e| (e.delivery.clone(), e.outcome)).collect();
    assert_eq!(log, [("d-1".to_owned(), Outcome::Accepted), ("d-1".to_owned(), Outcome::Duplicate), ("d-2".to_owned(), Outcome::Filtered)]);
    // A restart still knows d-1.
    let again = Hooks::new(r.s.clone(), Arc::new(Secrets::new(r.root.join("secrets.dat"), Some(r.crypto.clone()))), Some(Sealed::in_dir(&r.root.join("st"), "hooks", r.crypto.clone())));
    assert_eq!(again.handle(&github(&id, &secret, "d-1", "pull_request", BODY)), Outcome::Duplicate);
}

#[test]
fn a_wrong_signature_or_task_is_refused_and_does_not_use_up_the_delivery_id() {
    let r = rig("bad");
    let id = task(&r, hook(&[], &[]));
    let secret = r.h.rotate(&id).unwrap();
    assert_eq!(r.h.handle(&github(&id, "wrong secret", "d-1", "push", BODY)), Outcome::BadSignature);
    let mut tampered = github(&id, &secret, "d-1", "push", BODY);
    tampered.body.push(b' ');
    assert_eq!(r.h.handle(&tampered), Outcome::BadSignature);
    assert_eq!(r.h.handle(&Request { path: format!("/hook/{id}"), headers: vec![("x-github-delivery".into(), "d-1".into())], body: BODY.into() }), Outcome::BadSignature, "unsigned");
    assert_eq!(r.h.handle(&github(&id, &secret, "d-1", "push", BODY)), Outcome::Accepted, "the id was not used up by the refused ones");
    assert_eq!(r.h.handle(&github("tk-nothing", &secret, "d-2", "push", BODY)), Outcome::Unknown);
    assert_eq!(r.h.handle(&Request { path: "/elsewhere".into(), headers: vec![], body: vec![] }), Outcome::Unknown);
    // Paused: as if it isn't there.
    r.s.set_enabled(&id, false);
    assert_eq!(r.h.handle(&github(&id, &secret, "d-3", "push", BODY)), Outcome::Unknown);
    // Rotating the secret stops the old one at once.
    r.s.set_enabled(&id, true);
    let new = r.h.rotate(&id).unwrap();
    assert_ne!(new, secret);
    assert_eq!(r.h.handle(&github(&id, &secret, "d-4", "push", BODY)), Outcome::BadSignature);
    assert_eq!(r.h.handle(&github(&id, &new, "d-4", "push", BODY)), Outcome::Accepted);
    // Body limits.
    let big = "x".repeat((1 << 20) + 1);
    assert_eq!(r.h.handle(&github(&id, &new, "d-5", "push", &big)), Outcome::TooLarge);
}

#[test]
fn hovers_own_scheme_needs_a_fresh_timestamp() {
    let r = rig("ts");
    let id = task(&r, hook(&[], &[]));
    let secret = r.h.rotate(&id).unwrap();
    let now = hover_agents::wake::now_ms() / 1000;
    assert_eq!(r.h.handle(&hover_signed(&id, &secret, "h-1", now, "{}")), Outcome::Accepted);
    assert_eq!(r.h.handle(&hover_signed(&id, &secret, "h-2", now - 600, "{}")), Outcome::Expired, "a captured call can’t be sent again later");
    assert_eq!(r.h.handle(&hover_signed(&id, &secret, "h-3", now + 600, "{}")), Outcome::Expired);
    assert_eq!(r.h.handle(&hover_signed(&id, "nope", "h-4", now, "{}")), Outcome::BadSignature);
    // A timestamp isn't covered by a signature made without it.
    let mut swapped = hover_signed(&id, &secret, "h-5", now, "{}");
    swapped.headers.retain(|(k, _)| k != "x-hover-timestamp");
    swapped.headers.push(("x-hover-timestamp".into(), (now - 1).to_string()));
    assert_eq!(r.h.handle(&swapped), Outcome::BadSignature);
}

#[test]
fn too_many_waiting_runs_are_refused_not_queued_without_end() {
    let r = rig("busy");
    r.k.set_max_running(1);
    *r.hold.lock().unwrap() = true;
    let mut last = Outcome::Accepted;
    for i in 0..30 {
        let id = task(&r, hook(&[], &[]));
        let secret = r.h.rotate(&id).unwrap();
        last = r.h.handle(&github(&id, &secret, &format!("d-{i}"), "push", "{}"));
        if last == Outcome::Busy { break; }
    }
    assert_eq!(last, Outcome::Busy);
    *r.hold.lock().unwrap() = false;
}

#[test]
fn it_listens_on_this_computer_only_unless_allowed_and_answers_over_a_real_connection() {
    let r = rig("tcp");
    let id = task(&r, hook(&["/pull_request/title"], &[]));
    let secret = r.h.rotate(&id).unwrap();
    assert!(r.h.listen("0.0.0.0:0", false).unwrap_err().contains("this computer only"));
    assert!(r.h.listen("not an address", false).is_err());
    let addr = r.h.listen("127.0.0.1:0", false).unwrap();
    assert_eq!(r.h.addr(), Some(addr));
    let send = |path: &str, headers: &[(&str, String)], body: &str| -> String {
        let mut c = std::net::TcpStream::connect(addr).unwrap();
        let mut req = format!("POST {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\n", body.len());
        for (k, v) in headers { req += &format!("{k}: {v}\r\n"); }
        req += "\r\n";
        c.write_all(req.as_bytes()).unwrap();
        c.write_all(body.as_bytes()).unwrap();
        let mut out = String::new();
        c.read_to_string(&mut out).unwrap();
        out
    };
    let sig = format!("sha256={}", hex(&hmac_sha256(secret.as_bytes(), BODY.as_bytes())));
    let ok = send(&format!("/hook/{id}"), &[("X-Hub-Signature-256", sig.clone()), ("X-GitHub-Delivery", "net-1".into()), ("X-GitHub-Event", "push".into())], BODY);
    assert!(ok.starts_with("HTTP/1.1 202 Accepted") && ok.contains("\"status\":\"accepted\""), "{ok}");
    wait_for("the run", || r.s.get(&id).unwrap().runs.iter().any(|x| x.state == RunState::Done));
    assert!(r.seen.lock().unwrap()[0].contains("- /pull_request/title: Fix login"));
    assert!(send(&format!("/hook/{id}"), &[("X-Hub-Signature-256", sig), ("X-GitHub-Delivery", "net-1".into())], BODY).starts_with("HTTP/1.1 409"));
    assert!(send(&format!("/hook/{id}"), &[("X-Hub-Signature-256", "sha256=00".into()), ("X-GitHub-Delivery", "net-2".into())], BODY).starts_with("HTTP/1.1 401"));
    assert!(send("/hook/none", &[], "{}").starts_with("HTTP/1.1 404"));
    // Not a POST: no.
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.write_all(b"GET /hook/x HTTP/1.1\r\n\r\n").unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 405"));
    // Said too large before the body is even read.
    let mut c = std::net::TcpStream::connect(addr).unwrap();
    c.write_all(format!("POST /hook/{id} HTTP/1.1\r\nContent-Length: 99999999\r\n\r\n").as_bytes()).unwrap();
    let mut out = String::new();
    c.read_to_string(&mut out).unwrap();
    assert!(out.starts_with("HTTP/1.1 413"), "{out}");
    r.h.stop();
    wait_for("the listener to close", || std::net::TcpStream::connect_timeout(&addr, Duration::from_millis(300)).is_err());
    assert!(r.h.addr().is_none());
}

//! The office's `state` message (KiroPage.State in the C#) read into turns, as main.js's
//! renderDrawer turns it into the thread: each turn's timeline (steps as objects, or the
//! demo's [icon, "Verb target", tag] arrays through stepOf), how long it worked, the
//! answer.
use serde_json::Value;

use crate::doc::{Stage, Step, StepIcon, Turn};

/// main.js BOTS: the six bots' names and colours, by `bot` index.
pub const BOTS: [(&str, [u8; 4]); 6] = [("Pip", [0x9b, 0x6b, 0xff, 255]), ("Juno", [0x2f, 0xc9, 0xb0, 255]), ("Moss", [0xff, 0x9a, 0x4a, 255]),
    ("Nova", [0xff, 0x6f, 0xae, 255]), ("Ada", [0x5a, 0xa8, 0xff, 255]), ("Rue", [0xb4, 0xe0, 0x4a, 255])];

/// main.js `took`: seconds under a minute (at least 1), then "3m 07s", then "1h 05m".
pub fn took(ms: f64) -> String {
    if ms < 60e3 { format!("{} s", ((ms / 1000.0).round() as i64).max(1)) }
    else if ms < 3600e3 { format!("{}m {:02}s", (ms / 60e3).floor() as i64, ((ms % 60e3 / 1000.0).round() as i64) % 60) }
    else { format!("{}h {:02}m", (ms / 3600e3).floor() as i64, (ms % 3600e3 / 60e3).floor() as i64) }
}

/// main.js `clockOf`: how long a running turn has gone, "m:ss".
pub fn clock(ms: f64) -> String {
    let n = (ms / 1000.0).floor().max(0.0) as i64;
    format!("{}:{:02}", n / 60, n % 60)
}

/// main.js `credits`: what a turn cost, "0.09 credits", and "<0.01 credits" for less.
pub fn credits(n: f64) -> String {
    let s = format!("{n:.2}");
    if n < 0.005 { "<0.01 credits".into() } else { format!("{s} credit{}", if s == "1.00" { "" } else { "s" }) }
}

fn stage(s: &str) -> Stage {
    match s { "waking" | "queued" => Stage::Waking, "working" | "waiting" => Stage::Working, "done" => Stage::Done, "failed" => Stage::Failed, _ => Stage::Stopped }
}

fn opt_str(v: &Value) -> Option<String> { v.as_str().filter(|s| !s.is_empty()).map(String::from) }

/// main.js stepOf: an object from Hover, or the demo's array.
pub fn step(x: &Value) -> Step {
    if let Some(a) = x.as_array() {
        let (k, text, tag) = (a.first().and_then(Value::as_str).unwrap_or(""), a.get(1).and_then(Value::as_str).unwrap_or(""), a.get(2).and_then(Value::as_str));
        let (verb, rest) = text.split_once(' ').unwrap_or((text, ""));
        let mut s = Step { kind: StepIcon::parse(k), verb: verb.into(), status: if tag == Some("failed") { "failed".into() } else { "completed".into() },
            tag: tag.filter(|t| *t != "failed").map(String::from), ..Step::default() };
        if k == "run" || k == "search" { s.cmd = Some(rest.into()); }
        else if !rest.is_empty() {
            match rest.rfind('/') { Some(i) => { s.name = Some(rest[i + 1..].into()); s.dir = Some(rest[..i].into()); } None => s.name = Some(rest.into()) }
        }
        // "+14 −2": an edit's counts, as the demo writes them.
        if let Some((a, d)) = s.tag.as_deref().and_then(|t| t.strip_prefix('+')).and_then(|t| t.split_once(" −")) {
            if let (Ok(a), Ok(d)) = (a.parse(), d.parse()) { s.add = a; s.del = d; s.tag = None; }
        }
        return s;
    }
    Step {
        kind: StepIcon::parse(x["k"].as_str().unwrap_or("")),
        verb: x["verb"].as_str().unwrap_or("").into(),
        name: opt_str(&x["name"]), dir: opt_str(&x["dir"]), cmd: opt_str(&x["cmd"]),
        status: x["status"].as_str().unwrap_or("completed").into(),
        add: x["add"].as_i64().unwrap_or(0) as i32, del: x["del"].as_i64().unwrap_or(0) as i32,
        diff: opt_str(&x["diff"]), out: opt_str(&x["out"]), exit: x["exit"].as_i64().map(|e| e as i32), ms: x["ms"].as_f64(), tag: None,
    }
}

/// A session of the state message as the thread shows it. `now` is the clock in Unix
/// ms (a running turn shows how long it has gone); `hm` writes a turn's start time.
pub fn turns_at(session: &Value, now: f64, hm: &dyn Fn(f64) -> String) -> Vec<Turn> {
    let raw = session["turns"].as_array().cloned().unwrap_or_default();
    // last(s): the last turn that isn't queued (or the first).
    let last = raw.iter().rposition(|t| !t["queued"].as_bool().unwrap_or(false)).unwrap_or(0);
    raw.iter().enumerate().map(|(i, t)| {
        let st = if i == last { session["stage"].as_str().or(t["stage"].as_str()).unwrap_or("done") } else { t["stage"].as_str().unwrap_or("done") };
        let live = i == last && matches!(st, "working" | "waiting");
        let t0 = t["t0"].as_f64().unwrap_or(0.0);
        Turn {
            prompt: t["prompt"].as_str().unwrap_or("").into(),
            images: t["images"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default(),
            queued: t["queued"].as_bool().unwrap_or(false),
            steps: t["steps"].as_array().map(|a| a.iter().map(step).collect()).unwrap_or_default(),
            took: t["took"].as_f64().map(took),
            took_ms: t["took"].as_f64(),
            credits: t["credits"].as_f64().map(credits),
            stage: stage(st),
            live,
            clock: if live { clock(now - t0) } else { String::new() },
            when: if t0 > 0.0 { hm(t0) } else { String::new() },
            status: None,
            answer: t["answer"].as_str().unwrap_or("").into(),
            waiting: live && st == "waiting",
            stopping: i == last && live && session["stopping"].as_bool().unwrap_or(false),
            restore: false,
            again: false,
        }
    }).collect()
}

/// main.js `hm`: toLocaleTimeString(undefined, { hour: 'numeric', minute: '2-digit' }) as
/// en-US writes it ("1:47 PM"), at the given offset from UTC in minutes.
pub fn hm(ms: f64, offset_min: i64) -> String {
    let m = ((ms / 60e3).floor() as i64 + offset_min).rem_euclid(24 * 60);
    let (h, mm) = (m / 60, m % 60);
    format!("{}:{mm:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" })
}

/// turns_at with no running clock, times in UTC (a finished session, the tests).
pub fn turns(session: &Value) -> Vec<Turn> { turns_at(session, 0.0, &|ms| hm(ms, 0)) }

//! The office's `state` message (KiroPage.State in the C#) read into turns, as main.js's
//! renderDrawer turns it into the thread: the status line of a running turn, the took
//! time, the live step, the answer.
use serde_json::Value;

use crate::doc::{Stage, Step, StepIcon, Turn};

/// main.js BOTS: the six bots' names and colours, by `bot` index.
pub const BOTS: [(&str, [u8; 4]); 6] = [("Pip", [0x9b, 0x6b, 0xff, 255]), ("Juno", [0x2f, 0xc9, 0xb0, 255]), ("Moss", [0xff, 0x9a, 0x4a, 255]),
    ("Nova", [0xff, 0x6f, 0xae, 255]), ("Ada", [0x5a, 0xa8, 0xff, 255]), ("Rue", [0xb4, 0xe0, 0x4a, 255])];

/// main.js `took`: under a minute in seconds (at least 1), else in minutes.
pub fn took(ms: f64) -> String {
    if ms < 60e3 { format!("{} s", ((ms / 1000.0).round() as i64).max(1)) } else { format!("{} min", (ms / 60e3).round() as i64) }
}

fn stage(s: &str) -> Stage {
    match s { "waking" | "queued" => Stage::Waking, "working" => Stage::Working, "done" => Stage::Done, "failed" => Stage::Failed, _ => Stage::Stopped }
}

/// A session of the state message as the thread shows it.
pub fn turns(session: &Value) -> Vec<Turn> {
    let raw = session["turns"].as_array().cloned().unwrap_or_default();
    // last(s): the last turn that isn't queued (or the first).
    let last = raw.iter().rposition(|t| !t["queued"].as_bool().unwrap_or(false)).unwrap_or(0);
    let act = session["act"].as_str().unwrap_or("");
    raw.iter().enumerate().map(|(i, t)| {
        let st = if i == last { session["stage"].as_str().or(t["stage"].as_str()).unwrap_or("done") } else { t["stage"].as_str().unwrap_or("done") };
        let stage = stage(st);
        let status = (i == last && matches!(stage, Stage::Waking | Stage::Working)).then(|| match (stage, act) {
            (Stage::Waking, _) => "Waking up…".to_string(),
            (_, "Writing") => "Writing it up…".into(),
            (_, a) if !a.is_empty() && a != "Thinking" => format!("{a}…"),
            _ => "Thinking…".into(),
        });
        Turn {
            prompt: t["prompt"].as_str().unwrap_or("").into(),
            images: t["images"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(String::from)).collect()).unwrap_or_default(),
            queued: t["queued"].as_bool().unwrap_or(false),
            steps: t["steps"].as_array().map(|a| a.iter().map(|s| Step {
                icon: StepIcon::parse(s[0].as_str().unwrap_or("")),
                text: s[1].as_str().unwrap_or("").into(),
                tag: s[2].as_str().filter(|x| !x.is_empty()).map(String::from),
            }).collect()).unwrap_or_default(),
            took: t["took"].as_f64().map(took),
            stage,
            status,
            answer: t["answer"].as_str().unwrap_or("").into(),
        }
    }).collect()
}

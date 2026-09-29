//! The office's state message (KiroPage.Push/State) against native/golden/fixtures/
//! office-state.json, the state the page goldens were made from. The fixture's
//! sessions are rebuilt as sessions, settings and checks, and the message the port
//! writes must be the fixture's bytes as System.Text.Json writes them (compact,
//! default encoder). Where the hand-made fixture can't be what C# writes (four
//! places), the expected value is corrected, each commented.

use hover_agents::agents::AgentReady;
use hover_agents::session::{KiroSession, KiroTurn};
use hover_agents::state::{push, Office};
use hover_agents::stream::{KiroPhase, KiroResult};
use hover_core::history::HistoryEntry;
use hover_core::json::{self, Json};
use hover_core::model::{AcpChoice, AcpOption, AgentOptions, AgentTool, KiroState, KiroStep};
use hover_core::settings::Settings;
use hover_core::time::{Kind, Stamp};

fn fixture() -> Json {
    let p = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../golden/fixtures/office-state.json");
    json::parse(&std::fs::read_to_string(p).unwrap()).unwrap().get("state").unwrap().clone()
}

fn s<'a>(v: &'a Json, k: &str) -> &'a str { v.get(k).unwrap().as_str().unwrap() }
fn f(v: &Json, k: &str) -> Option<f64> { v.get(k).unwrap().opt_f64().unwrap() }

fn state_of(stage: &str) -> Option<KiroState> {
    match stage { "done" => Some(KiroState::Completed), "failed" => Some(KiroState::Failed), "stopped" => Some(KiroState::Cancelled), _ => None }
}

/// A session whose C#-rule rendering is the fixture's: rows become steps whose title
/// is the row's text (no target), except the last step of a session with a file,
/// whose target is the row's text after its verb.
fn session(v: &Json) -> KiroSession {
    let t = &v.get("turns").unwrap().items().unwrap()[0];
    let t0 = Stamp::from_unix_ms(f(t, "t0").unwrap() as i64, Kind::Local);
    let rows = t.get("steps").unwrap().items().unwrap();
    let file = s(v, "file");
    let steps = rows.iter().enumerate().map(|(i, r)| {
        let r = r.items().unwrap();
        let kind = match r[0].as_str().unwrap() { "run" => "execute", k => k };
        let text = r[1].as_str().unwrap();
        let last = i + 1 == rows.len() && !file.is_empty();
        KiroStep { id: format!("t{i}"), kind: kind.into(), title: if last { "Read File".into() } else { text.into() },
            target: last.then(|| text.split_once(' ').unwrap().1.to_owned()), status: if r[2].is_null() { "completed" } else { "failed" }.into(), ..Default::default() }
    }).collect();
    let stage = s(v, "stage");
    let mut turn = KiroTurn::new(s(t, "prompt"), vec![]);
    turn.steps = steps;
    turn.started_at = t0;
    turn.woke_at = f(t, "woke").map(|w| t0.add_secs(w));
    turn.ended_at = f(t, "took").map(|ms| t0.add_secs(ms / 1000.0));
    turn.result = state_of(stage).map(|st| KiroResult::new(st, s(t, "answer")));
    let mut k = KiroSession::new(AgentTool::parse(Some(s(v, "tool"))).unwrap());
    k.id = v.get("id").unwrap().i32().unwrap();
    k.key = s(v, "key").into();
    k.bot = v.get("bot").unwrap().i32().unwrap() as usize;
    k.seat = v.get("seat").unwrap().i32().unwrap() as usize;
    k.folder = s(v, "folder").into();
    k.context = f(v, "ctx");
    k.state = state_of(stage).unwrap_or(KiroState::Running);
    k.phase = match (stage, s(v, "act")) { ("waking", _) => KiroPhase::Starting, (_, "Reading") => KiroPhase::Reading, _ => KiroPhase::Working };
    k.turns = vec![turn];
    k
}

fn option(id: &str, category: &str, choices: &[(&str, &str)]) -> AcpOption {
    AcpOption { id: id.into(), category: Some(category.into()), current: None,
        choices: choices.iter().map(|(v, n)| AcpChoice { value: v.to_string(), name: n.to_string() }).collect() }
}

#[test]
fn the_state_message_is_the_fixtures_bytes() {
    let fx = fixture();
    let dir = std::env::temp_dir().join(format!("hover-state-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let settings = Settings::load(dir.join("settings.json"));
    settings.set_agent_offers(AgentTool::Kiro, &[option("model", "model", &[("auto", "Auto"), ("claude-opus-5.5", "Claude Opus 5.5")]),
        option("effortLevel", "thought_level", &[("low", "Low"), ("medium", "Medium"), ("high", "High")])]);
    settings.set_agent_offers(AgentTool::Codex, &[option("model", "model", &[("gpt-5.6-sol", "GPT-5.6 Sol")])]);
    settings.set_agent_options(AgentTool::Cursor, AgentOptions { read_only: true, ..Default::default() });
    let sessions: Vec<KiroSession> = fx.get("sessions").unwrap().items().unwrap().iter().map(session).collect();
    let history: Vec<HistoryEntry> = fx.get("history").unwrap().items().unwrap().iter().map(|h| HistoryEntry {
        key: s(h, "key").into(), tool: AgentTool::parse(Some(s(h, "tool"))).unwrap(), title: s(h, "title").into(), folder: s(h, "folder").into(),
        updated: Stamp::from_unix_ms(h.get("at").unwrap().i64().unwrap(), Kind::Local),
        state: state_of(s(h, "stage")).unwrap_or(KiroState::Running), turns: h.get("turns").unwrap().i32().unwrap() }).collect();
    let ready = |t: AgentTool| Some(if t == AgentTool::Cursor {
        AgentReady { installed: true, signed_in: false, hint: hover_agents::agents::sign_in_hint(t).into() }
    } else { AgentReady { installed: true, signed_in: true, hint: String::new() } });
    let office = Office { window: false, open: None, settings: &settings, folder: Some(r"C:\hover".into()), history: Some(history),
        ready: &ready, files: &|_| None };
    let got = push(&office, &sessions).compact();

    // The fixture's session titles are made up; KiroSession.Title is the prompt's
    // first line cut to 60, so that is what C# writes.
    let mut fx = fx;
    if let Json::Obj(props) = &mut fx {
        let (_, Json::Arr(list)) = props.iter_mut().find(|(k, _)| k == "sessions").unwrap() else { panic!() };
        for (v, k) in list.iter_mut().zip(&sessions) {
            let Json::Obj(p) = v else { panic!() };
            p.iter_mut().find(|(n, _)| n == "title").unwrap().1 = Json::str(k.title());
        }
    }
    assert_eq!(sessions[0].title(), "Refresh tokens never expire. Make them expire after 30 days…");
    let mut want = fx.compact();
    // The fixture writes session 1's t0 as a double; Ms() is a long.
    want = want.replace(r#""t0":1789999916000.0"#, r#""t0":1789999916000"#);
    // Cursor's models are [] in the fixture; KiroPage.Models always puts Default first
    // when the list is empty, so C# writes it, and the model is its id.
    want = want.replace(r#""models":[],"model":null"#, r#""models":[{"id":"","name":"Default"}],"model":"""#);
    // A history row's stage is Stage(e.State, Working): a running entry reads
    // "working"; "waking" needs an Idle entry, which AgentHistory.Save never writes.
    want = want.replace(r#""at":1789949600000,"stage":"waking""#, r#""at":1789949600000,"stage":"working""#);
    assert!(want.contains("\\u201C"), "non-ASCII escaped, as the default encoder does");
    if got != want {
        let at = got.bytes().zip(want.bytes()).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
        panic!("differs at {at}:\n got  …{}\n want …{}", &got[at.saturating_sub(80)..(at + 80).min(got.len())], &want[at.saturating_sub(80)..(at + 80).min(want.len())]);
    }
}

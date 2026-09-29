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

/// A session whose C#-rule rendering is the fixture's: each row becomes the step
/// Row() writes it from (its kind from the icon, its title the verb, its target the
/// file or the command), with its change, output, exit code and time.
fn session(v: &Json) -> KiroSession {
    let t = &v.get("turns").unwrap().items().unwrap()[0];
    let t0 = Stamp::from_unix_ms(f(t, "t0").unwrap() as i64, Kind::Local);
    let rows = t.get("steps").unwrap().items().unwrap();
    let o = |r: &Json, k: &str| r.get(k).unwrap().opt_str().unwrap();
    let steps = rows.iter().enumerate().map(|(i, r)| {
        let kind = match s(r, "k") { "run" => "execute", k => k };
        let target = match (o(r, "dir"), o(r, "name"), o(r, "cmd")) {
            (Some(d), Some(n), _) => Some(format!("{d}/{n}")),
            (None, Some(n), _) => Some(n),
            (_, _, c) => c,
        };
        KiroStep { id: format!("t{i}"), kind: kind.into(), title: s(r, "verb").into(), target, status: s(r, "status").into(),
            added: r.get("add").unwrap().i32().unwrap(), removed: r.get("del").unwrap().i32().unwrap(), diff: o(r, "diff"), output: o(r, "out"),
            exit: f(r, "exit").map(|e| e as i32), ms: f(r, "ms") }
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
        choices: choices.iter().map(|(v, n)| AcpChoice { value: v.to_string(), name: n.to_string(), levels: None }).collect() }
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
    // 55111fc: each model carries its levels, each tool its effort label and whether it
    // asks questions, and OpenCode is the fifth tool. The fixture predates them, so
    // they are put in as KiroPage writes them.
    let mut fxj = json::parse(&want).unwrap();
    if let Some((_, Json::Arr(tools))) = match &mut fxj { Json::Obj(p) => p.iter_mut().find(|(k, _)| k == "tools"), _ => None } {
        for t in tools.iter_mut() {
            let Json::Obj(p) = t else { panic!() };
            if let Some((_, Json::Arr(ms))) = p.iter_mut().find(|(k, _)| k == "models") {
                for m in ms { if let Json::Obj(mp) = m { mp.push(("levels".into(), Json::Null)); } }
            }
            // Codex offers Read only where it has a sandbox for it: not on Windows.
            if p.iter().any(|(k, v)| k == "id" && v.as_str() == Some("codex")) {
                if let Some((_, v)) = p.iter_mut().find(|(k, _)| k == "readOnly") { *v = Json::Bool(hover_agents::agents::read_only_works(AgentTool::Codex)); }
            }
            p.push(("effortLabel".into(), Json::str("Effort")));
            p.push(("questions".into(), Json::Bool(false)));
        }
        tools.push(json::parse(r#"{"id":"opencode","name":"OpenCode","ready":true,"hint":"","access":"full","readOnly":true,"hideSteps":false,"models":[{"id":"","name":"Default","levels":null}],"model":"","efforts":[],"effort":null,"effortLabel":"Variant","questions":true}"#).unwrap());
    }
    want = fxj.compact();
    assert!(want.contains("\\u201C"), "non-ASCII escaped, as the default encoder does");
    if got != want {
        let at = got.bytes().zip(want.bytes()).position(|(a, b)| a != b).unwrap_or(got.len().min(want.len()));
        panic!("differs at {at}:\n got  …{}\n want …{}", &got[at.saturating_sub(80)..(at + 80).min(got.len())], &want[at.saturating_sub(80)..(at + 80).min(want.len())]);
    }
}

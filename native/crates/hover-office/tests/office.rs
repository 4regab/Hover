//! The office's model against the page: the fixture's sessions become bots at their
//! desks, clicks on them open them, a new session walks in through the door and a
//! gone one walks out, the camera keeps to its limits, the pacing slows when idle.
//! Expected values are main.js's own constants (DESKS, DOOR, the zoom limits).

use hover_core::json::{parse, Json};
use hover_office::bot::Stage;
use hover_office::office::{Click, Office, Prop};
use hover_office::scene::{seat, DESKS};

fn fixture() -> (Json, f64) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden/fixtures/office-state.json");
    let fx = parse(&std::fs::read_to_string(p).unwrap()).unwrap();
    (fx.get("state").unwrap().clone(), fx.get("now").unwrap().f64().unwrap())
}

fn office() -> Office {
    let (state, now) = fixture();
    let mut o = Office::new(1104.0, 424.0, false);
    o.wall_clock = Box::new(move || (now, 0));
    o.state(&state);
    // 6.4 s, past the capture's 6 s settle (the done bubble has gone by then).
    for i in 0..400 { o.frame(i as f64 * 16.0, 16.0); }
    o
}

#[test]
fn the_fixtures_sessions_sit_at_their_desks() {
    let o = office();
    assert_eq!(o.sessions.len(), 5);
    for s in &o.sessions {
        assert!(s.b.seated, "{}", s.b.name);
        assert!((s.b.x - (seat(s.desk) + 0.05)).abs() < 1e-9 && (s.b.z - DESKS[s.desk].1).abs() < 1e-9);
    }
    let names: Vec<_> = o.sessions.iter().map(|s| s.b.name).collect();
    assert_eq!(names, ["Pip", "Juno", "Moss", "Nova", "Ada"]);
    // bubbleFor, for each stage in the fixture.
    let bubbles: Vec<String> = o.sessions.iter().map(Office::bubble).collect();
    assert_eq!(bubbles, ["Reading refresh.ts", "", "Waking up…", "Couldn’t finish", "z z z"]);
}

#[test]
fn a_click_on_a_bot_opens_its_session_and_props_do_their_thing() {
    let mut o = office();
    for s in o.tags() {
        // Just under the tag: the bot's head.
        o.pointer = Some((s.x, s.y + 12.0));
        o.pick();
        assert_eq!(o.click(), Click::Open(s.id), "{}", s.name);
    }
    // The TV and the window, through their hit boxes.
    let (view, proj) = o.camera();
    let at = |x: f64, y: f64, z: f64| { let p = proj.mul(&view).point(hover_office::m::v3(x, y, z)); ((p.x + 1.0) / 2.0 * 1104.0, (1.0 - p.y) / 2.0 * 424.0) };
    o.pointer = Some(at(5.4, 2.38, -5.3));
    o.pick();
    assert_eq!(o.click(), Click::Panel("tv"));
    o.pointer = Some(at(1.5, 2.3, -5.3));
    o.pick();
    assert_eq!(o.hint(Prop::Window), "Make it day");
    assert!(matches!(o.click(), Click::Time(_)));
    o.pointer = Some((5.0, 5.0));
    o.pick();
    assert_eq!(o.click(), Click::Nothing);
}

#[test]
fn a_new_session_walks_in_and_a_gone_one_walks_out() {
    let (state, _) = fixture();
    let mut o = office();
    // A sixth session, waking: in through the door, then seated.
    let mut s2 = state.clone();
    if let Json::Obj(p) = &mut s2 {
        for (k, v) in p.iter_mut() {
            if k == "sessions" {
                if let Json::Arr(list) = v {
                    let mut n = list[2].clone();
                    if let Json::Obj(q) = &mut n { for (k, v) in q.iter_mut() { match k.as_str() { "id" => *v = Json::int(9), "seat" => *v = Json::int(5), "bot" => *v = Json::int(5), _ => {} } } }
                    list.push(n);
                }
            }
        }
    }
    o.state(&s2);
    let new = o.sessions.iter().find(|s| s.id == 9).unwrap();
    assert!(!new.b.seated && new.b.walking());
    assert_eq!(Office::bubble(new), "On my way…");
    for i in 400..1100 { o.frame(i as f64 * 16.0, 16.0); }
    let new = o.sessions.iter().find(|s| s.id == 9).unwrap();
    assert!(new.b.seated && !new.b.walking());
    assert_eq!(new.last().stage, Stage::Waking);
    // Gone from the state: retired.
    o.state(&state);
    assert!(o.sessions.iter().all(|s| s.id != 9));
}

#[test]
fn the_camera_keeps_to_its_limits_and_the_pace_drops_when_idle() {
    let mut o = office();
    o.zoom_by(10.0, 0.0, 0.0);
    assert_eq!(o.user[2], 2.8);
    o.zoom_by(0.01, 0.0, 0.0);
    assert_eq!(o.user[2], 0.85);
    o.drag(10_000.0, 0.0);
    assert!(o.user[0].abs() <= 6.0 && o.user[1].abs() <= 5.0);
    o.reset_view();
    assert_eq!(o.user, [0.0, 0.0, 1.0]);
    // The empty office goes quiet: after the settle, frames come at 10 fps, not 30.
    let mut e = Office::new(1104.0, 424.0, false);
    let (mut state, _) = fixture();
    if let Json::Obj(p) = &mut state { for (k, v) in p.iter_mut() { if k == "sessions" { *v = Json::Arr(vec![]); } } }
    e.state(&state);
    let mut drawn = 0;
    for i in 0..1000 { if e.frame(i as f64 * 16.0, 16.0) { drawn += 1; } }
    let late: usize = (1000..1625).filter(|i| e.frame(*i as f64 * 16.0, 16.0)).count();
    assert!(drawn > late);
    // 10 s at 10 fps; on a 16 ms clock a 100 ms gap takes 7 ticks, so about 89.
    assert!((80..=101).contains(&late), "{late} frames in 10 s");
}

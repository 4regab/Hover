//! A session's subagents as helpers at its desk (main.js's "Helpers"): how many there are
//! and where they stand, that they face the room, come and go with the subagents, are
//! made again rather than added to the scene, and keep the pacing rules. All without a GPU.

use hover_core::json::{parse, Json};
use hover_office::m::Rgb;
use hover_office::mini::{palette, spot, Duty, MINI, MINI_SPOTS};
use hover_office::office::Office;
use hover_office::scene::DESKS;

fn fixture() -> (Json, f64) {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/fixtures/office-state.json");
    let fx = parse(&std::fs::read_to_string(p).unwrap()).unwrap();
    (fx.get("state").unwrap().clone(), fx.get("now").unwrap().f64().unwrap())
}

/// The fixture's state with the newest turn of session `i` holding one subagent row per
/// status (as state.rs writes them: k "agent", a status).
fn with_agents(state: &Json, i: usize, statuses: &[&str]) -> Json {
    let mut s = state.clone();
    let rows: Vec<Json> = statuses.iter().map(|st| Json::obj(vec![("k", Json::str("agent")), ("verb", Json::str("Subagent")), ("status", Json::str(*st))])).collect();
    let Json::Obj(p) = &mut s else { panic!() };
    let (_, Json::Arr(list)) = p.iter_mut().find(|(k, _)| k == "sessions").unwrap() else { panic!() };
    let Json::Obj(q) = &mut list[i] else { panic!() };
    let (_, Json::Arr(turns)) = q.iter_mut().find(|(k, _)| k == "turns").unwrap() else { panic!() };
    let Json::Obj(t) = turns.last_mut().unwrap() else { panic!() };
    t.iter_mut().find(|(k, _)| k == "steps").unwrap().1 = Json::Arr(rows);
    s
}

struct Clock(f64);
impl Clock {
    fn run(&mut self, o: &mut Office, frames: usize) { for _ in 0..frames { o.frame(self.0, 16.0); self.0 += 16.0; } }
}

/// The office settled with the fixture's sessions at their desks (6.4 s).
fn office() -> (Office, Clock, Json) {
    let (state, now) = fixture();
    let mut o = Office::new(1104.0, 424.0, false);
    o.wall_clock = Box::new(move || (now, 0));
    o.state(&state);
    let mut c = Clock(0.0);
    c.run(&mut o, 400);
    (o, c, state)
}

fn out(o: &Office, i: usize) -> usize { o.crew.minis.iter().filter(|m| m.sid == o.sessions[i].id && !m.leaving).count() }

#[test]
fn a_busy_seated_session_gets_a_helper_per_running_subagent() {
    let (mut o, mut c, state) = office();
    assert!(o.crew.minis.is_empty(), "the fixture has no subagents");
    assert_eq!(o.sessions[0].last().stage, hover_office::bot::Stage::Working);
    // Two out, one back already: two helpers.
    o.state(&with_agents(&state, 0, &["in_progress", "completed", "in_progress"]));
    assert_eq!(o.sessions[0].last().agents, 2);
    c.run(&mut o, 60);
    assert_eq!(out(&o, 0), 2);
    assert_eq!(o.crew.minis.len(), 2);
    let desk = o.sessions[0].desk;
    for m in &o.crew.minis {
        let (sx, sz) = spot(desk, m.slot);
        let p = m.at(&o.g);
        assert!((p.x - sx).abs() < 1e-6 && (p.z - sz).abs() < 1e-6, "slot {} stands at its spot: {p:?} vs {sx},{sz}", m.slot);
        assert!(p.y < 0.02, "on the floor");
        // Around the desk, clear of the chair side: within reach of the desk's centre.
        assert!((p.x - DESKS[desk].0).abs() <= 0.75 && (p.z - DESKS[desk].1).abs() <= 1.1);
        assert_eq!(m.out, 1.0);
        // Small: about MINI the bot's size.
        assert!((o.g.nodes[m.root].s.x - MINI).abs() < 1e-9);
        // Facing the room: the camera looks from +x and +z, so it faces that way, as the
        // page's three-quarter turn does.
        assert!(m.yaw > 0.2 && m.yaw < 1.4 && (m.yaw - std::f64::consts::FRAC_PI_4).abs() > 0.5, "yaw {}", m.yaw);
        assert!(o.g.shown()[m.root]);
    }
    // Two places, both different; the tags carry each helper's colour, which is not the bot's.
    assert_ne!(o.crew.minis[0].slot, o.crew.minis[1].slot);
    let tag = o.tags().into_iter().find(|t| t.id == o.sessions[0].id).unwrap();
    assert_eq!(tag.helpers.len(), 2);
    assert!(tag.helpers.iter().all(|h| *h != tag.color) && tag.helpers[0] != tag.helpers[1]);
    // The other desks have none.
    assert!(o.tags().iter().filter(|t| t.id != tag.id).all(|t| t.helpers.is_empty()));
}

#[test]
fn helpers_come_and_go_with_the_subagents_and_are_at_most_four() {
    let (mut o, mut c, state) = office();
    o.state(&with_agents(&state, 0, &["in_progress"; 6]));
    c.run(&mut o, 60);
    assert_eq!(out(&o, 0), MINI_SPOTS.len(), "four places to stand");
    let slots: Vec<usize> = { let mut s: Vec<_> = o.crew.minis.iter().map(|m| m.slot).collect(); s.sort(); s };
    assert_eq!(slots, [0, 1, 2, 3]);
    // Down to one: the newest hop back into their bot and are gone within the hop's 0.6 s.
    o.state(&with_agents(&state, 0, &["in_progress", "completed", "failed"]));
    c.run(&mut o, 3);
    assert_eq!(out(&o, 0), 1);
    assert_eq!(o.crew.minis.len(), 4, "the three still on their way back");
    assert!(o.crew.moving(), "hopping: the shadows follow every frame");
    c.run(&mut o, 40);
    assert_eq!(o.crew.minis.len(), 1);
    assert_eq!(o.crew.minis[0].slot, 0, "the first one stays");
    // None: the last goes too.
    o.state(&with_agents(&state, 0, &["completed"]));
    c.run(&mut o, 60);
    assert!(o.crew.minis.is_empty());
    assert!(!o.crew.any());
}

#[test]
fn only_a_busy_bot_at_its_desk_has_helpers() {
    let (mut o, mut c, state) = office();
    // Pip's done (or failed): nothing to help with, whatever rows say.
    let failed = o.sessions.iter().position(|s| s.last().stage == hover_office::bot::Stage::Failed).unwrap();
    o.state(&with_agents(&state, failed, &["in_progress", "in_progress"]));
    c.run(&mut o, 60);
    assert!(o.crew.minis.is_empty());
    // A session that walks in is not at its desk until it has sat down: no helpers before.
    let mut s2 = with_agents(&state, 2, &["in_progress"]);
    if let Json::Obj(p) = &mut s2 {
        let (_, Json::Arr(list)) = p.iter_mut().find(|(k, _)| k == "sessions").unwrap() else { panic!() };
        let mut n = list[2].clone();
        if let Json::Obj(q) = &mut n { for (k, v) in q.iter_mut() { match k.as_str() { "id" => *v = Json::int(77), "seat" => *v = Json::int(5), "bot" => *v = Json::int(5), _ => {} } } }
        list.push(n);
    }
    o.state(&s2);
    let new = o.sessions.iter().position(|s| s.id == 77).unwrap();
    assert!(o.sessions[new].b.walking());
    c.run(&mut o, 5);
    assert_eq!(out(&o, new), 0, "walking in");
    c.run(&mut o, 700);
    assert!(o.sessions[new].b.seated);
    assert_eq!(out(&o, new), 1, "seated and waking up, with a subagent out");
    // The session goes: its helpers hop back in and are gone, with its bot on the way out.
    o.state(&state);
    c.run(&mut o, 60);
    assert!(o.crew.minis.iter().all(|m| m.sid != 77));
}

#[test]
fn helpers_are_used_again_so_the_scene_stops_growing() {
    let (mut o, mut c, state) = office();
    let mut sizes = vec![];
    for round in 0..6 {
        o.state(&with_agents(&state, 0, &["in_progress"; 4]));
        c.run(&mut o, 60 + round);
        assert_eq!(o.crew.minis.len(), 4);
        o.state(&with_agents(&state, 0, &["completed"; 4]));
        c.run(&mut o, 60);
        assert!(o.crew.minis.is_empty());
        sizes.push(o.g.nodes.len());
    }
    assert!(sizes.windows(2).all(|w| w[0] == w[1]), "the scene grew: {sizes:?}");
    // Nodes hidden with the helpers: nothing of them is drawn once they are gone.
    let drawn = |o: &Office| { let shown = o.g.shown(); o.g.nodes.iter().enumerate().filter(|(i, n)| shown[*i] && n.draw.is_some()).count() };
    let empty = drawn(&o);
    o.state(&with_agents(&state, 0, &["in_progress"; 2]));
    c.run(&mut o, 60);
    assert!(drawn(&o) > empty + 2 * 20, "two helpers draw tens of boxes: {} vs {empty}", drawn(&o));
    // Even all desks' helpers stay within the renderer's draw buffer (2048) with room to spare.
    assert!(drawn(&o) + 22 * 50 < 2048, "{}", drawn(&o));
}

#[test]
fn a_helper_is_recoloured_from_its_bot_and_does_its_paperwork() {
    let (mut o, mut c, state) = office();
    o.state(&with_agents(&state, 0, &["in_progress", "in_progress"]));
    // Each duty comes round, the file one hands a sheet to the desk's tray.
    let mut seen = std::collections::HashSet::new();
    let (mut air, mut moving_frames, mut frames) = (0, 0, 0);
    for _ in 0..1200 {
        c.run(&mut o, 1);
        for m in &o.crew.minis { seen.insert(format!("{:?}", m.duty_now())); }
        air = air.max(o.crew.sheets_in_air());
        if o.crew.minis.iter().all(|m| m.out == 1.0) { frames += 1; if o.crew.moving() { moving_frames += 1; } }
    }
    for d in [Duty::Write, Duty::Stamp, Duty::Flip, Duty::File] { assert!(seen.contains(&format!("{d:?}")), "{d:?}"); }
    assert!(air >= 1, "a sheet was handed in");
    // The redraw rule: only while a sheet is in the air do the shadows need every frame.
    assert!(moving_frames * 2 < frames, "{moving_frames} of {frames} frames");
    // The palette: the bot's hue turned per slot, lighter, within three.js's limits.
    let bot = Rgb::hex(0x9b6bff);
    let (h0, _, l0) = bot.hsl();
    let mains: Vec<Rgb> = (0..4).map(|k| palette(bot, k)[0]).collect();
    for (k, m) in mains.iter().enumerate() {
        let (h, _, l) = m.hsl();
        let want = (h0 + [0.5, 0.17, -0.17, 0.33][k] + 1.0) % 1.0;
        assert!((h - want).abs() < 1e-6 || (h - want).abs() > 1.0 - 1e-6, "slot {k}: hue {h} vs {want}");
        assert!(l <= 0.7 + 1e-9 && (l - (l0 + 0.06).min(0.7)).abs() < 1e-6);
        let [main, dark, pale] = palette(bot, k);
        assert_eq!(dark, main.mul(0.5));
        assert!(pale.0 >= main.0 && pale.1 >= main.1 && pale.2 >= main.2);
    }
    for a in 0..4 { for b in a + 1..4 { assert_ne!(mains[a], mains[b]); } }
}

#[test]
fn the_colour_round_trip_matches_three() {
    for h in [0x9b6bff, 0x2fc9b0, 0xff9a4a, 0xff6fae, 0x5aa8ff, 0xb4e04a, 0x808080, 0x000000, 0xffffff] {
        let c = Rgb::hex(h);
        let (a, b, l) = c.hsl();
        let d = Rgb::from_hsl(a, b, l);
        assert!((c.0 - d.0).abs() < 1e-9 && (c.1 - d.1).abs() < 1e-9 && (c.2 - d.2).abs() < 1e-9, "{h:x}");
    }
    // A pure red: hue 0, full saturation, lightness ½ (in the linear space).
    assert_eq!(Rgb(1.0, 0.0, 0.0).hsl(), (0.0, 1.0, 0.5));
    // Hue wraps.
    assert_eq!(Rgb::from_hsl(1.25, 1.0, 0.5), Rgb::from_hsl(0.25, 1.0, 0.5));
}

#[test]
fn the_pace_returns_to_idle_once_the_helpers_are_gone() {
    let (mut o, mut c, state) = office();
    // Everyone done or sent away: an empty office, so nothing is lively but the helpers.
    let mut e = state.clone();
    if let Json::Obj(p) = &mut e { for (k, v) in p.iter_mut() { if k == "sessions" { *v = Json::Arr(vec![]); } } }
    o.state(&with_agents(&state, 0, &["in_progress"]));
    c.run(&mut o, 60);
    assert!(o.crew.any() && o.lively, "helpers keep the pace at 30 fps");
    o.state(&e);
    c.run(&mut o, 1500);
    assert!(o.crew.minis.is_empty() && o.sessions.is_empty());
    let late = (0..625).filter(|_| { let n = o.frame(c.0, 16.0); c.0 += 16.0; n }).count();
    assert!((80..=101).contains(&late), "{late} frames in 10 s: idle again");
}

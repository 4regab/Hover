//! The rich chat's behaviour, headless: selection and copy across blocks, links, the
//! section cache while an answer streams, and a long conversation's cost.
use std::path::Path;
use std::time::Instant;

use hover_chat::{state, Hit, Painter, Pos, Shaper, Stage, Step, StepIcon, Thread, Turn};

fn fonts() -> Vec<Vec<u8>> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let mut v: Vec<Vec<u8>> = ([] as [&str; 0]).iter()
        .map(|f| std::fs::read(repo.join(format!("src/Hover/Assets/Fonts/{f}.ttf"))).unwrap()).collect();
    v.push(std::fs::read(repo.join("web/office/fonts/PixelifySans.ttf")).unwrap());
    v
}

fn rich() -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden/fixtures/rich.md")).unwrap()
}

fn turn(prompt: &str, answer: &str) -> Turn {
    let step = |t: &str| Step { icon: StepIcon::Read, text: t.into(), tag: None };
    Turn { steps: vec![step("Read a"), step("Read b"), step("Ran c")], took: Some("3 min".into()), answer: answer.into(), ..Turn::new(prompt) }
}

fn golden(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../golden").join(name)).unwrap()
}

/// The office-state fixture's session k, laid out as the page lays it out.
fn fixture(k: usize) -> (Thread, Vec<Turn>) {
    let fx: serde_json::Value = serde_json::from_str(&golden("fixtures/office-state.json")).unwrap();
    let s = &fx["state"]["sessions"][k];
    let (name, color) = state::BOTS[s["bot"].as_u64().unwrap() as usize];
    let turns = state::turns(s);
    let mut th = Thread::new(Shaper::new(&fonts()), name, color);
    th.set(&turns, 358.0);
    (th, turns)
}

#[test]
fn a_select_all_copies_what_the_page_copies() {
    let want: serde_json::Value = serde_json::from_str(&golden("expected/copy.json")).unwrap();
    for (name, k, open) in [("done-rich", 1, false), ("failed", 3, false), ("stopped", 4, false), ("failed-steps-open", 3, true), ("working-live", 0, false)] {
        let (mut th, turns) = fixture(k);
        if open { th.toggle_steps(&turns, 0); }
        th.select_all();
        assert_eq!(th.selected_text(), want[name]["thread"].as_str().unwrap(), "{name}");
    }
}

#[test]
fn the_step_list_is_open_while_it_runs_and_flips_on_a_click() {
    let (th, _) = fixture(0);
    assert!(th.sections[0].summary.is_some());
    let texts: Vec<&str> = th.sections[0].frag.texts.iter().map(|t| t.text.as_str()).collect();
    assert!(texts.contains(&"Reading src/auth/refresh.ts"), "the live step says what it is doing: {texts:?}");
    assert!(th.sections[0].frag.texts.iter().any(|t| t.shimmer));
    let (mut th, turns) = fixture(3);
    let n = th.sections[0].frag.texts.len();
    let [x, y, _, h] = th.sections[0].summary.unwrap();
    match th.hit(12.0 + x + 20.0, th.sections[0].y + y + h / 2.0) { Hit::Toggle(0) => {} _ => panic!("summary not hit") }
    th.toggle_steps(&turns, 0);
    assert!(th.sections[0].frag.texts.len() > n, "opened");
    th.toggle_steps(&turns, 0);
    assert_eq!(th.sections[0].frag.texts.len(), n, "closed again");
}

fn find(th: &Thread, needle: &str) -> Pos {
    for (si, s) in th.sections.iter().enumerate() {
        for (ti, t) in s.frag.texts.iter().enumerate() {
            if let Some(b) = t.text.find(needle) {
                return Pos { section: si, text: ti, byte: b };
            }
        }
    }
    panic!("{needle} not laid out");
}

#[test]
fn a_selection_crosses_paragraphs_lists_quotes_tables_and_code_and_copies_as_text() {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&[turn("Why?", &rich())], 360.0);
    let a = find(&th, "I moved");
    let mut f = find(&th, "return now");
    f.byte += "return now".len();
    th.select(a, f);
    let text = th.selected_text();
    for want in ["I moved the token check into RefreshService", "See the RFC or https://example.com/docs/auth.", "\n\nFiles\n",
        "src/auth/refresh.ts: the expiry check\n", "so tests can move time\n", "Build is clean\n",
        "The tokens in production were minted before this change,\nso they get the full 30 days from today.",
        "File\tLines\tStatus\nrefresh.ts\t+24 −3\tchanged", "export function expired(t: Token, now = Date.now()): boolean {\n  return now"] {
        assert!(text.contains(want), "copy lacks {want:?}:\n{text}");
    }
    assert!(!text.contains("TS"), "the language tag is not part of the copy");
    // Backwards selections copy the same.
    th.select(f, a);
    assert_eq!(th.selected_text(), text);
    // The selection is drawn in every box it covers.
    let rects: usize = (0..th.sections[0].frag.texts.len()).map(|i| th.selection_rects(0, i).len()).sum();
    assert!(rects > 15, "{rects} selection rects");
}

#[test]
fn a_click_on_a_link_finds_its_address_and_on_text_a_caret() {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&[turn("Links?", "See [the RFC](https://datatracker.ietf.org/doc/html/rfc6749) now.")], 360.0);
    let p = find(&th, "the RFC");
    let t = &th.sections[0].frag.texts[p.text];
    let s = &th.sections[0];
    let c = parley::Cursor::from_byte_index(&t.layout, p.byte + 2, parley::Affinity::Downstream).geometry(&t.layout, 1.0);
    let (x, y) = (12.0 + t.x + c.x0 as f32 + 1.0, s.y + t.y + (c.y0 + c.y1) as f32 / 2.0);
    match th.hit(x, y) {
        Hit::Link(url) => assert_eq!(&*url, "https://datatracker.ietf.org/doc/html/rfc6749"),
        _ => panic!("no link under the pointer"),
    }
    match th.hit(12.0 + t.x + 2.0, y) {
        Hit::Text(pos) => assert_eq!(pos.byte, 0),
        _ => panic!("no caret on the text"),
    }
}

#[test]
fn streaming_relays_out_only_the_turn_that_changed() {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    let mut turns: Vec<Turn> = (0..20).map(|i| turn(&format!("Step {i}"), &rich())).collect();
    turns.push(Turn { status: Some("Thinking…".into()), stage: Stage::Working, ..Turn::new("Now?") });
    th.set(&turns, 360.0);
    assert_eq!(th.relayouts, 21);
    let words: Vec<&str> = "The answer streams in a word at a time, and only its own section is laid out again.".split(' ').collect();
    for n in 1..=words.len() {
        let last = turns.last_mut().unwrap();
        last.answer = words[..n].join(" ");
        th.set(&turns, 360.0);
    }
    assert_eq!(th.relayouts, 21 + words.len(), "one section per chunk");
    // A new width lays everything out again, once.
    th.set(&turns, 340.0);
    assert_eq!(th.relayouts, 21 + words.len() + 21);
}

#[test]
fn a_long_rich_conversation_stays_responsive() {
    let f = fonts();
    let mut th = Thread::new(Shaper::new(&f), "Juno", [47, 201, 176, 255]);
    let turns: Vec<Turn> = (0..200).map(|i| turn(&format!("Step {i}: tighten the check."), &rich())).collect();
    let t = Instant::now();
    th.set(&turns, 360.0);
    let layout = t.elapsed();
    let mut p = Painter::new(&f, Box::new(|_| None));
    let t = Instant::now();
    let _ = p.paint(&th, th.height - 300.0, 360, 300, 1.0, hover_chat::theme::DRAWER_BG);
    let first = t.elapsed();
    let t = Instant::now();
    for k in 0..30 {
        let _ = p.paint(&th, th.height - 300.0 - k as f32 * 40.0, 360, 300, 1.0, hover_chat::theme::DRAWER_BG);
    }
    let scroll = t.elapsed() / 30;
    let mut turns2 = turns.clone();
    turns2.last_mut().unwrap().answer.push_str("\n\nOne more line.");
    let t = Instant::now();
    th.set(&turns2, 360.0);
    let stream = t.elapsed();
    eprintln!("200 turns: height {:.0} px; layout {layout:?}; first paint {first:?}; scroll paint {scroll:?}; streamed chunk relayout {stream:?}", th.height);
    // Loose bounds (debug builds are slow); the release numbers go in the report.
    assert!(stream.as_millis() < 250 && scroll.as_millis() < 250);
}

#[test]
fn step_rows_sit_where_the_page_puts_them() {
    // copy.json's `rows`: each open step row's x and width in the page's #thread.
    let want: serde_json::Value = serde_json::from_str(&golden("expected/copy.json")).unwrap();
    for (name, k, open) in [("working-live", 0, false), ("failed-steps-open", 3, true)] {
        let (mut th, turns) = fixture(k);
        if open { th.toggle_steps(&turns, 0); }
        let xs: Vec<f32> = th.sections[0].frag.shapes.iter().filter_map(|s| match s {
            hover_chat::doc::Shape::Svg { x, w, y, .. } if *w == 13.0 && *y > 60.0 && *y < 140.0 => Some(*x + 12.0),
            _ => None,
        }).collect();
        let rows = want[name]["rows"].as_array().unwrap();
        let got: Vec<f32> = xs.iter().take(rows.len()).copied().collect();
        let exp: Vec<f32> = rows.iter().map(|r| r[0].as_f64().unwrap() as f32).collect();
        eprintln!("{name}: rows at {got:?}, page {exp:?}");
        assert_eq!(got.len(), exp.len(), "{name}");
        for (g, e) in got.iter().zip(&exp) { assert!((g - e).abs() <= 1.5, "{name}: {g} vs {e}"); }
    }
}

#[test]
fn answers_copy_as_the_page_copies_them() {
    // 600 seeded answers mixing every block md.js writes, each copied whole in the page.
    let want: serde_json::Value = serde_json::from_str(&golden("expected/copy.json")).unwrap();
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    let mut bad = vec![];
    let rows = want["answers"].as_array().unwrap();
    for r in rows {
        let (src, copy) = (r[0].as_str().unwrap(), r[1].as_str().unwrap());
        th.set(&[Turn { answer: src.into(), ..Turn::new("Q") }], 358.0);
        let got = th.answer_text(0);
        if got != copy { bad.push((src.to_string(), copy.to_string(), got)); }
    }
    for (s, w, g) in bad.iter().take(4) { eprintln!("---\nsrc  {s:?}\nwant {w:?}\ngot  {g:?}"); }
    assert!(bad.is_empty(), "{} of {} differ", bad.len(), rows.len());
}

#[test]
fn a_selection_ending_before_a_diagram_leaves_its_labels_out() {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&[Turn { answer: "First.\n\n```mermaid\ngraph LR\nA-->B\n```\n\nLast.".into(), ..Turn::new("Q") }], 358.0);
    let a = find(&th, "First.");
    let mut f = a;
    f.byte += "First.".len();
    th.select(a, f);
    assert_eq!(th.selected_text(), "First.");
    th.select_all();
    assert!(th.selected_text().ends_with("First.\n\nA\nB\nLast."), "{:?}", th.selected_text());
}

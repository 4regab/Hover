//! The rich chat's behaviour, headless: selection and copy across blocks, links, the
//! section cache while an answer streams, and a long conversation's cost.
use std::path::Path;
use std::time::Instant;

use hover_chat::{Hit, Painter, Pos, Shaper, Stage, Thread, Turn};

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
    Turn { prompt: prompt.into(), queued: false, steps: 3, took: Some("3 min".into()), stage: Stage::Done, status: None, answer: answer.into() }
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
    for want in ["I moved the token check into RefreshService", "See the RFC or https://example.com/docs/auth.", "Files\n",
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
    turns.push(Turn { answer: String::new(), status: Some("Thinking…".into()), stage: Stage::Working, took: None, ..turn("Now?", "") });
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

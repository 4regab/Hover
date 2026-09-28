//! The rich chat's behaviour, headless: selection and copy across blocks, links, the
//! section cache while an answer streams, and a long conversation's cost.
use std::path::Path;
use std::time::Instant;

use hover_chat::{state, Hit, Painter, Pos, Shaper, Stage, Step, StepIcon, Thread, Turn, Unit};

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
    let mut p = Painter::new(&f, hover_chat::Images::none());
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

/// Golden UTF-16 offsets (the page's text nodes) to byte offsets in a native text box,
/// which also holds a '\n' for each <br>.
fn align(page: &str, native: &str) -> Vec<usize> {
    let mut map = vec![usize::MAX; page.encode_utf16().count() + 1];
    let (mut u, mut it) = (0, page.chars().peekable());
    for (b, c) in native.char_indices() {
        if it.peek() == Some(&c) {
            map[u] = b;
            u += c.len_utf16();
            it.next();
        }
    }
    map[u] = native.len();
    map
}

/// Double and triple clicks as the real page answers them (golden/gen-words.mjs): each
/// character of fixtures/words.md clicked at 1/4 and 3/4 of its width, with the copy of
/// what got selected. Chromium's Linux editing behaviour, so no trailing space. Where the
/// pointer lands is parley's hit test (nearest caret), checked in `hit` tests; this is
/// about what the caret selects.
#[test]
fn double_and_triple_clicks_select_what_the_page_selects() {
    let want: serde_json::Value = serde_json::from_str(&golden("expected/words.json")).unwrap();
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&[Turn { answer: golden("fixtures/words.md"), ..Turn::new("Q") }], 358.0);
    let (mut n, mut bad, mut wrapped, mut emoji_skipped) = (0, vec![], 0, 0);
    for leaf in want.as_array().unwrap() {
        let page = leaf["text"].as_str().unwrap();
        let flat = |s: &str| s.replace('\n', "");
        let sec = &th.sections[0];
        let (ti, t) = sec.frag.texts.iter().enumerate().find(|(_, t)| !t.text.is_empty() && flat(&t.text) == flat(page)).unwrap_or_else(|| panic!("no box for {page:?}"));
        let map = align(page, &t.text);
        // The soft wraps, in page offsets: the page's (recorded) and this layout's. A caret
        // that ends a soft-wrapped line in one and not the other depends on the fonts'
        // metrics, not on the selection rules, and is counted apart.
        let units: Vec<u16> = page.encode_utf16().collect();
        let back = |b: usize| map.iter().position(|&m| m == b);
        let hard = |u: usize| map[u] > 0 && t.text.as_bytes()[map[u] - 1] == b'\n';
        let page_soft: Vec<usize> = leaf["lines"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).filter(|&u| !hard(u)).collect();
        let lines: Vec<_> = t.layout.lines().collect();
        let ours: Vec<usize> = lines.windows(2).filter(|w| matches!(w[0].break_reason(), parley::BreakReason::Regular | parley::BreakReason::Emergency))
            .filter_map(|w| back(w[1].text_range().start)).collect();
        let ends_soft = |starts: &[usize], c: usize| starts.iter().any(|&s| c < s && units[c..s].iter().all(|&u| u == b' ' as u16));
        for p in leaf["probes"].as_array().unwrap() {
            let (i, at) = (p[0].as_u64().unwrap() as usize, p[1].as_f64().unwrap());
            // From the caret the page's own click found there: its hit test works in whole
            // pixels, so a narrow glyph's right quarter can still land before it.
            // A caret after the clicked character sits before any break that follows it.
            let caret = p[2].as_u64().unwrap() as usize;
            let byte = if caret > i { let c = page.encode_utf16().nth(caret - 1).unwrap(); let back = if (0xdc00..0xe000).contains(&c) { 2 } else { 1 };
                let b = map[caret - back]; b + t.text[b..].chars().next().unwrap().len_utf8() } else { map[caret] };
            let pos = Pos { section: 0, text: ti, byte };
            if ends_soft(&page_soft, caret) != ends_soft(&ours, caret) { wrapped += 1; continue; }
            for (k, unit) in [(3, Unit::Word), (4, Unit::Para)] {
                let (a, f, tail) = th.unit_at(pos, unit);
                th.selection = Some((a, f));
                th.tail = tail;
                let got = th.selected_text();
                let exp = p[k][1].as_str().unwrap();
                let k = k - 1;
                // Emoji next to each other or to a space: Chromium's ICU walks its word
                // boundaries forwards and backwards inconsistently there (" 🙂" from one
                // caret, "🙂🎉" from the next). Not reproduced; listed in REPORT.md.
                let emoji = |s: &str| s.chars().any(|c| c as u32 >= 0x1f000);
                if unit == Unit::Word && (emoji(exp) || emoji(&got)) { emoji_skipped += 1; continue; }
                n += 1;
                if got != exp { bad.push(format!("{page:.20?} @{i} {at} x{k}: got {got:?}, page {exp:?}")); }
            }
        }
    }
    assert!(bad.is_empty(), "{} of {n} differ:\n{}", bad.len(), bad.join("\n"));
    // Where this layout wraps a line elsewhere than Chromium did (see above).
    assert!(wrapped <= 8, "{wrapped} probes wrap differently");
    assert!(emoji_skipped <= 8, "{emoji_skipped} emoji probes");
    eprintln!("{n} clicks match the page; skipped: {wrapped} probes where the lines wrap differently, {emoji_skipped} double clicks on emoji");
}

#[test]
fn a_double_click_on_windows_takes_the_spaces_after_the_word() {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.set(&[Turn { answer: "one two\nthree".into(), ..Turn::new("Q") }], 358.0);
    let t = th.sections[0].frag.texts.iter().position(|t| t.text.starts_with("one")).unwrap();
    let at = |byte| Pos { section: 0, text: t, byte };
    let (a, f, _) = th.word_at(at(1));
    th.select(a, th.trailing_space(f));
    assert_eq!(th.selected_text(), "one ");
    // Not across a line break.
    let (a, f, _) = th.word_at(at(5));
    th.select(a, th.trailing_space(f));
    assert_eq!(th.selected_text(), "two");
}

/// With its scrollbars shown (golden/gen-scroll.mjs), the page's thread is 10 px
/// narrower, and a code block wider than the drawer gains a 10 px bar and scrolls:
/// the boxes' heights, their places relative to the first, and their scroll widths.
#[test]
fn scrolling_boxes_are_laid_out_as_the_page_lays_them_out() {
    let want: serde_json::Value = serde_json::from_str(&golden("expected/scroll.json")).unwrap();
    for name in ["rich", "wide"] {
        let w = &want[name];
        let cw = w["thread"]["cw"].as_f64().unwrap() as f32;
        let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
        th.set(&[Turn { answer: golden(&format!("fixtures/{name}.md")), ..Turn::new("Q") }], cw);
        let f = &th.sections[0].frag;
        // The boxes' borders: pre, .table and figure are the 10 px rounded outlines.
        let boxes: Vec<(f32, f32)> = f.shapes.iter().filter_map(|s| match s {
            hover_chat::doc::Shape::Rect { y, h, radius, stroke: Some(_), .. } if radius[0] == 10.0 => Some((*y, *h)),
            _ => None,
        }).collect();
        let page = w["boxes"].as_array().unwrap();
        assert_eq!(boxes.len(), page.len(), "{name}");
        let (y0, py0) = (boxes[0].0, page[0]["y"].as_f64().unwrap() as f32);
        for (k, ((y, h), p)) in boxes.iter().zip(page).enumerate() {
            let (py, ph) = (p["y"].as_f64().unwrap() as f32, p["h"].as_f64().unwrap() as f32);
            assert!((h - ph).abs() < 0.5, "{name} box {k}: height {h}, page {ph}");
            assert!((y - y0 - (py - py0)).abs() < 0.5, "{name} box {k}: at {}, page {}", y - y0, py - py0);
            eprintln!("{name} box {k}: height {h:.2} (page {ph}), at {:.2} (page {:.2})", y - y0, py - py0);
        }
        // Scroll widths: only the long code lines overflow (tables wrap anywhere instead).
        let wide: Vec<f32> = page.iter().filter(|p| p["sw"].as_f64() > p["cw"].as_f64()).map(|p| p["sw"].as_f64().unwrap() as f32).collect();
        assert_eq!(f.scrollers.len(), wide.len(), "{name}");
        for (sc, sw) in f.scrollers.iter().zip(wide) {
            assert!((sc.content - sw).abs() < 6.0, "{name}: scroll width {}, page {sw}", sc.content);
        }
    }
}

/// A step list opened in one chat stays open there, and doesn't open turn i of the next.
#[test]
fn step_list_state_belongs_to_its_session() {
    let (mut th, turns) = fixture(3);
    th.session = 4;
    th.set(&turns, 358.0);
    let closed = th.sections[0].frag.texts.len();
    th.toggle_steps(&turns, 0);
    let open = th.sections[0].frag.texts.len();
    assert!(open > closed);
    // Another session with the same turns: closed, as it was never opened there.
    th.session = 5;
    th.set(&turns, 358.0);
    assert_eq!(th.sections[0].frag.texts.len(), closed);
    // Back: still open.
    th.session = 4;
    th.set(&turns, 358.0);
    assert_eq!(th.sections[0].frag.texts.len(), open);
}

/// The answer's content, laid out at the page's answer width (the thread less its padding).
fn answer(src: &str, width: f32, images: hover_chat::images::Shared) -> Thread {
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.use_images(images);
    th.set(&[Turn { answer: src.into(), ..Turn::new("Q") }], width + 24.0);
    th
}

/// Images that don't load take the room of their alt text, with the broken-image icon
/// before it (golden/gen-broken.mjs): the image boxes and the paragraphs after them sit
/// where the page puts them, relative to the answer's first paragraph.
#[test]
fn broken_images_take_their_alt_texts_room() {
    use hover_chat::doc::Shape;
    let want: serde_json::Value = serde_json::from_str(&golden("expected/broken.json")).unwrap();
    for case in want.as_array().unwrap() {
        let src = case["src"].as_str().unwrap();
        let th = answer(src, case["width"].as_f64().unwrap() as f32, hover_chat::Images::none());
        let f = &th.sections[0].frag;
        let top = f.texts.iter().find(|t| t.text.starts_with("Before") || t.text.starts_with("Text")).unwrap().y;
        // The image boxes: their fill, or (no alt) nothing drawn, so only the others are checked.
        let boxes: Vec<(f32, f32)> = f.shapes.iter().filter_map(|s| match s {
            Shape::Rect { y, h, fill: Some(c), .. } if *c == hover_chat::theme::IMG_BG => Some((y - top, *h)), _ => None }).collect();
        let page: Vec<(f32, f32)> = case["imgs"].as_array().unwrap().iter().map(|r| (r[1].as_f64().unwrap() as f32, r[3].as_f64().unwrap() as f32)).filter(|r| r.1 > 0.0).collect();
        assert_eq!(boxes.len(), page.len(), "{src:?}");
        for (b, p) in boxes.iter().zip(&page) {
            assert!((b.0 - p.0).abs() < 0.5 && (b.1 - p.1).abs() < 0.5, "{src:?}: box {b:?}, page {p:?}");
        }
        let icons = f.shapes.iter().filter(|s| matches!(s, Shape::Broken { .. })).count();
        assert_eq!(icons, page.len(), "{src:?}: one icon per image with alt text");
        let after = f.texts.iter().find(|t| t.text == "After").unwrap().y - top;
        let page_after = case["ps"].as_array().unwrap().last().unwrap()[1].as_f64().unwrap() as f32;
        assert!((after - page_after).abs() < 0.5, "{src:?}: After at {after}, page {page_after}");
    }
}

/// An image loads later: until then it is broken-looking; when it arrives only the
/// sections that show it are laid out again, now at the image's size.
#[test]
fn an_image_that_arrives_lays_out_only_its_section_again() {
    use std::cell::Cell;
    use std::rc::Rc;
    let png = {
        let mut b = std::io::Cursor::new(vec![]);
        image::RgbaImage::from_pixel(200, 100, image::Rgba([200, 50, 50, 255])).write_to(&mut b, image::ImageFormat::Png).unwrap();
        b.into_inner()
    };
    let ready = Rc::new(Cell::new(false));
    let r = ready.clone();
    let images = hover_chat::Images::new(Box::new(move |_| if r.get() { hover_chat::Fetch::Bytes(png.clone()) } else { hover_chat::Fetch::Pending }));
    let mut th = Thread::new(Shaper::new(&fonts()), "Juno", [47, 201, 176, 255]);
    th.use_images(images.clone());
    let turns = vec![Turn { answer: "No image.".into(), ..Turn::new("A") }, Turn { answer: "See\n\n![pic](https://e.x/p.png)".into(), ..Turn::new("B") }];
    th.set(&turns, 358.0);
    let h0 = th.sections[1].h;
    assert!(th.sections[1].frag.shapes.iter().any(|s| matches!(s, hover_chat::doc::Shape::Broken { .. })));
    ready.set(true);
    let n = th.relayouts;
    assert!(th.image_changed("https://e.x/p.png"));
    th.set(&turns, 358.0);
    assert_eq!(th.relayouts, n + 1, "only the section with the image");
    assert!((th.sections[1].h - (h0 - 18.0 + 100.0)).abs() < 1.0, "{} vs {h0}", th.sections[1].h);
    let mut p = Painter::new(&fonts(), images);
    let px = p.paint(&th, 0.0, 358, th.height as u32, 1.0, hover_chat::theme::DRAWER_BG);
    let (x, y) = (12 + 100, (th.sections[1].y + th.sections[1].h - 50.0) as u32);
    let c = px.pixel(x, y).unwrap();
    assert!(c.red() > 150 && c.green() < 100, "the image is painted: {c:?}");
}

/// .ans.fresh: the new answer (only it) fades in over .35 s, rising 4 px; laying the
/// turn out again (the next state) drops the fade, as main.js's re-render does.
#[test]
fn a_fresh_answer_fades_in_and_a_relayout_ends_it() {
    let f = fonts();
    let mut th = Thread::new(Shaper::new(&f), "Juno", [47, 201, 176, 255]);
    let mut turns = vec![Turn { answer: "A fresh answer, painted white.".into(), ..Turn::new("Question?") }];
    th.set(&turns, 358.0);
    let mut p = Painter::new(&f, hover_chat::Images::none());
    let h = th.height as u32;
    let bright = |px: &resvg::tiny_skia::Pixmap, y0: f32, y1: f32| (y0 as u32..y1 as u32).flat_map(|y| (0..358).map(move |x| (x, y)))
        .filter(|&(x, y)| px.pixel(x, y).unwrap().red() > 200).count();
    let s = &th.sections[0];
    let (ti, _, _) = s.answer_at.unwrap();
    let ans = &s.frag.texts[ti];
    let (a0, a1) = (s.y + ans.y, s.y + ans.y + ans.layout.height());
    let full = bright(&p.paint(&th, 0.0, 358, h, 1.0, hover_chat::theme::DRAWER_BG), a0, a1);
    let prompt = bright(&p.paint(&th, 0.0, 358, h, 1.0, hover_chat::theme::DRAWER_BG), 0.0, a0 - 30.0);
    assert!(full > 50);
    p.time = 10.0;
    th.fresh = Some((0, 10.0));
    let px = p.paint(&th, 0.0, 358, h, 1.0, hover_chat::theme::DRAWER_BG);
    assert_eq!(bright(&px, a0, a1), 0, "invisible at the start");
    assert_eq!(bright(&px, 0.0, a0 - 30.0), prompt, "the prompt doesn't fade");
    assert!(p.fading(&th));
    p.time = 10.35;
    assert!(!p.fading(&th));
    assert_eq!(bright(&p.paint(&th, 0.0, 358, h, 1.0, hover_chat::theme::DRAWER_BG), a0, a1), full);
    p.time = 10.1;
    turns[0].answer.push_str(" More.");
    th.set(&turns, 358.0);
    assert_eq!(th.fresh, None);
}

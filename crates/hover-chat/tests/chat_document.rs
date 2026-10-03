//! A Markdown text on its own (the desk's pull request description) is laid out and painted
//! as an answer is: headings, lists, bold, inline code and code blocks, with nothing of the
//! chat around it.
use std::path::Path;

use hover_chat::{Images, Painter, Shaper, Thread};

fn fonts() -> Vec<Vec<u8>> {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    vec![std::fs::read(repo.join("app/assets/PixelifySans.ttf")).unwrap()]
}

const BODY: &str = "## Summary\n\nSkips **clean** views in `refresh()`.\n\n- the panel stops redrawing\n- the tests cover both\n\n```rust\nif !view.dirty { return; }\n```";

fn thread(src: &str) -> Thread {
    let mut th = Thread::new(Shaper::new(&fonts()), "Pip", [155, 107, 255, 255]);
    th.document(src, 400.0);
    th
}

#[test]
fn a_document_is_one_section_of_blocks_without_the_chat_around_it() {
    let th = thread(BODY);
    assert_eq!(th.sections.len(), 1);
    let texts: Vec<&str> = th.sections[0].frag.texts.iter().map(|t| t.text.as_str()).collect();
    // The heading's words without its marks, the list's items as their own boxes, the code whole.
    assert!(texts.contains(&"Summary"), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("Skips clean views in refresh().")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("the panel stops redrawing")), "{texts:?}");
    assert!(texts.iter().any(|t| t.contains("if !view.dirty")), "{texts:?}");
    // No bot name, prompt bubble or Copy/Retry row of a turn.
    assert!(!texts.contains(&"Pip"), "{texts:?}");
    assert!(th.sections[0].frag.hits.iter().all(|(_, a)| matches!(a, hover_chat::doc::Act::Copy(_))), "only the code block's Copy");
    assert!((th.height - th.sections[0].h).abs() < 0.01 && th.height > 60.0, "{}", th.height);
}

#[test]
fn a_document_lays_out_again_at_another_width() {
    let mut th = thread("A paragraph long enough to wrap on a narrow page, and then some more words to make sure of it.");
    let wide = th.height;
    th.document("A paragraph long enough to wrap on a narrow page, and then some more words to make sure of it.", 120.0);
    assert!(th.height > wide + 10.0, "narrower is taller: {} vs {wide}", th.height);
}

#[test]
fn a_document_paints_formatted_text() {
    let th = thread(BODY);
    let mut p = Painter::new(&fonts(), Images::none());
    let px = p.paint(&th, 0.0, 424, th.height.ceil() as u32, 1.0, [0, 0, 0, 0]);
    let lit = px.data().chunks(4).filter(|c| c[3] > 0).count();
    assert!(lit > 500, "text and the code block's box are drawn: {lit}");
}

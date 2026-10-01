//! The port writes what md.js writes, byte for byte, for every fixture; and the image
//! rule answers as main.js's imageFor does.
use std::fs;
use std::path::{Path, PathBuf};

use hover_md::image::{image_for, Session};

fn golden() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden")
}

const SESSION: Session = Session { files: Some("fabc123def456.hover"), folder: "C:\\proj\\app" };

#[test]
fn markdown_matches_md_js() {
    let g = golden();
    let mut n = 0;
    for f in fs::read_dir(g.join("fixtures")).unwrap() {
        let p = f.unwrap().path();
        if p.extension().and_then(|e| e.to_str()) != Some("md") {
            continue;
        }
        let src = fs::read_to_string(&p).unwrap();
        let stem = p.file_stem().unwrap().to_str().unwrap();
        let img = |s: &str| image_for(&SESSION, s);
        let want = fs::read_to_string(g.join(format!("expected/{stem}.html"))).unwrap();
        assert_eq!(hover_md::markdown(&src, Some(&img)), want, "{stem}");
        let want = fs::read_to_string(g.join(format!("expected/{stem}.noimg.html"))).unwrap();
        assert_eq!(hover_md::markdown(&src, None), want, "{stem} (no image rule)");
        n += 1;
    }
    assert!(n >= 5);
}

#[test]
fn image_rule_matches_image_for() {
    let text = fs::read_to_string(golden().join("expected/image-paths.json")).unwrap();
    // [["src", "url" | null], ...] written by gen.mjs; read without a JSON crate.
    let rows: Vec<(String, Option<String>)> = text
        .split("\n [\n")
        .skip(1)
        .map(|chunk| {
            let lines: Vec<&str> = chunk.lines().map(str::trim).collect();
            let unq = |s: &str| s.trim_end_matches(',').trim_matches('"').replace("\\\\", "\\");
            let url = if lines[1].trim_end_matches(',') == "null" { None } else { Some(unq(lines[1])) };
            (unq(lines[0]), url)
        })
        .collect();
    assert!(rows.len() >= 10);
    for (src, want) in rows {
        assert_eq!(image_for(&SESSION, &src), want, "{src}");
    }
}

#[test]
fn blocks_read_back_every_kind() {
    use hover_md::Block;
    let src = fs::read_to_string(golden().join("fixtures/rich.md")).unwrap();
    let img = |s: &str| image_for(&SESSION, s);
    let b = hover_md::parse(&src, Some(&img));
    let kinds: Vec<&str> = b.iter().map(|b| match b {
        Block::Para(_) => "p", Block::Heading(..) => "h", Block::Rule => "hr", Block::Code { .. } => "code",
        Block::Diagram { .. } => "diagram", Block::Quote(_) => "quote", Block::List { .. } => "list", Block::Table { .. } => "table",
    }).collect();
    assert_eq!(kinds, ["h", "p", "p", "h", "list", "list", "quote", "table", "code", "diagram", "hr", "p", "p"]);
    let Block::List { ordered: true, items, .. } = &b[4] else { panic!() };
    assert_eq!(items[1].lists.len(), 1, "nested list stays with its item");
    let Block::List { items, .. } = &b[5] else { panic!() };
    assert_eq!(items.iter().map(|i| i.task).collect::<Vec<_>>(), [Some(true), Some(false), Some(true)]);
    let Block::Code { lang, text } = &b[8] else { panic!() };
    assert_eq!(lang.as_deref(), Some("ts"));
    assert!(text.contains("now - t.issuedAt > 30 * DAY"));
}

#[test]
fn random_corpus_matches_md_js() {
    let text = fs::read_to_string(golden().join("expected/random.json")).unwrap();
    let rows: Vec<(String, String)> = serde_json::from_str(&text).unwrap();
    let img = |s: &str| image_for(&SESSION, s);
    let mut bad = vec![];
    let mut skipped = 0;
    for (src, want) in &rows {
        let got = hover_md::markdown(src, Some(&img));
        // md.js throws or never returns on these (MARKDOWN.md); the port must still answer.
        if want.starts_with('\u{0}') {
            skipped += 1;
            continue;
        }
        if &got != want {
            bad.push((src.clone(), want.clone(), got));
        }
    }
    for (s, w, g) in bad.iter().take(5) {
        eprintln!("---\nsrc  {s:?}\nwant {w:?}\ngot  {g:?}");
    }
    eprintln!("{} cases compared, {skipped} where md.js throws or hangs", rows.len() - skipped);
    assert!(bad.is_empty(), "{} of {} differ", bad.len(), rows.len());
}

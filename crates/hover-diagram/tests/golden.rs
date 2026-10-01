//! Every flowchart case gives the SVG diagram.js gives, byte for byte.
use std::fs;
use std::path::Path;

#[test]
fn flowcharts_match_diagram_js() {
    let g = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden");
    let src = fs::read_to_string(g.join("fixtures/mermaid-cases.txt")).unwrap().replace("\r\n", "\n");
    let cases: Vec<&str> = src.split("\n===\n").collect();
    assert!(cases.len() >= 8);
    for (i, c) in cases.iter().enumerate() {
        let want = fs::read_to_string(g.join(format!("expected/mermaid-{i}.svg"))).unwrap();
        let got = hover_diagram::flowchart(c).unwrap_or_else(|| "null".into());
        assert_eq!(got, want, "case {i}:\n{c}");
    }
}

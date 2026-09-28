//! Renders a fixture session to a PNG, headless:
//! `cargo run -p hover-chat --example render -- out.png [scale] [session index]`.
use hover_chat::{Painter, Shaper, Thread};
use std::path::Path;

fn fonts(repo: &Path) -> Vec<Vec<u8>> {
    let mut v = vec![];
    for f in [] as [&str; 0] {
        v.push(std::fs::read(repo.join(format!("src/Hover/Assets/Fonts/{f}.ttf"))).unwrap());
    }
    v.push(std::fs::read(repo.join("web/office/fonts/PixelifySans.ttf")).unwrap());
    v
}

fn main() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../..");
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or("thread.png".into());
    let scale: f32 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let f = fonts(&repo);
    let fx: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(repo.join("native/golden/fixtures/office-state.json")).unwrap()).unwrap();
    let k: usize = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(1);
    let s = &fx["state"]["sessions"][k];
    let (name, color) = hover_chat::state::BOTS[s["bot"].as_u64().unwrap() as usize];
    let turns = hover_chat::state::turns(s);
    let mut th = Thread::new(Shaper::new(&f), name, color);
    th.set(&turns, 358.0);
    let mut p = Painter::new(&f, Box::new(|_| None));
    let t = std::time::Instant::now();
    let px = p.paint(&th, 0.0, (358.0 * scale) as u32, (th.height * scale).ceil() as u32, scale, hover_chat::theme::DRAWER_BG);
    eprintln!("height {} px, painted in {:?}", th.height, t.elapsed());
    px.save_png(&out).unwrap();
}

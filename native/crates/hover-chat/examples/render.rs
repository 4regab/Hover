//! Renders a thread to a PNG, headless: `cargo run -p hover-chat --example render -- out.png [scale]`.
use hover_chat::{Painter, Shaper, Stage, Thread, Turn};
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
    let rich = std::fs::read_to_string(repo.join("native/golden/fixtures/rich.md")).unwrap();
    let mut th = Thread::new(Shaper::new(&f), "Juno", [0x2f, 0xc9, 0xb0, 255]);
    let turns = vec![Turn { prompt: "The notch blinks when I change the workspace size in Settings. Find out why and fix it.".into(), queued: false, steps: 3,
        took: Some("3 min".into()), stage: Stage::Done, status: None, answer: rich }];
    th.set(&turns, 360.0);
    let mut p = Painter::new(&f, Box::new(|_| None));
    let t = std::time::Instant::now();
    let px = p.paint(&th, 0.0, (360.0 * scale) as u32, (th.height * scale).ceil() as u32, scale, hover_chat::theme::DRAWER_BG);
    eprintln!("height {} px, painted in {:?}", th.height, t.elapsed());
    px.save_png(&out).unwrap();
}

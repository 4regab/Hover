//! The office rendered headless, as port/bench/capture-office.mjs captures the page:
//! the fixture's state at its fixed clock, 1104 × 424, night or day, settled 6 s.
//!   cargo run --release -p hover-office --example shot -- out.png [night|day] [--empty] [--zoom] [--helpers N] [--frames N]

use hover_office::office::{Office, Time};
use hover_office::render::Renderer;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let out = args.get(1).cloned().unwrap_or_else(|| "office.png".into());
    let day = args.iter().any(|a| a == "day");
    let (w, h) = (1104usize, 424usize);
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/golden/fixtures/office-state.json");
    let fx = hover_core::json::parse(&std::fs::read_to_string(root).unwrap()).unwrap();
    let now = fx.get("now").unwrap().f64().unwrap();
    let t0 = std::time::Instant::now();
    let mut o = Office::new(w as f64, h as f64, false);
    // The capture's clock: the fixture's instant, in UTC (the Chromium run's zone here).
    let start = now;
    let clock = std::sync::Arc::new(std::sync::atomic::AtomicU64::new(start.to_bits()));
    let c2 = clock.clone();
    o.wall_clock = Box::new(move || (f64::from_bits(c2.load(std::sync::atomic::Ordering::Relaxed)), 0));
    o.apply_time(if day { Time::Day } else { Time::Night });
    let mut state = fx.get("state").unwrap().clone();
    if args.iter().any(|a| a == "--empty") { if let hover_core::json::Json::Obj(p) = &mut state { for (k, v) in p { if k == "sessions" { *v = hover_core::json::Json::Arr(vec![]); } } } }
    o.state(&state);
    // --helpers N: the working session has N subagents out (its newest turn's steps are N
    // running subagent rows), so its helpers stand at the desk.
    if let Some(n) = args.iter().position(|a| a == "--helpers").and_then(|i| args.get(i + 1)).and_then(|v| v.parse::<usize>().ok()) {
        use hover_core::json::Json;
        let rows = Json::Arr((0..n).map(|_| Json::obj(vec![("k", Json::str("agent")), ("verb", Json::str("Subagent")), ("status", Json::str("in_progress"))])).collect());
        let mut s = state.clone();
        if let Json::Obj(p) = &mut s {
            if let Some((_, Json::Arr(list))) = p.iter_mut().find(|(k, _)| k == "sessions") {
                if let Some(Json::Obj(q)) = list.first_mut() {
                    if let Some((_, Json::Arr(turns))) = q.iter_mut().find(|(k, _)| k == "turns") {
                        if let Some(Json::Obj(t)) = turns.last_mut() { if let Some(x) = t.iter_mut().find(|(k, _)| k == "steps") { x.1 = rows; } }
                    }
                }
            }
        }
        o.state(&s);
    }
    if args.iter().any(|a| a == "--zoom") { o.zoom_by(1.6, 0.0, 0.0); }
    let frames: usize = args.iter().position(|a| a == "--frames").and_then(|i| args.get(i + 1)).and_then(|v| v.parse().ok()).unwrap_or(375);
    // requestAnimationFrame at 60 Hz under Playwright's clock: 6 s.
    for i in 0..frames { clock.store((start + i as f64 * 16.0).to_bits(), std::sync::atomic::Ordering::Relaxed); o.frame(i as f64 * 16.0, 16.0); }
    let mut r = Renderer::new(w as u32, h as u32).expect("a GPU");
    let frame = r.render(&mut o);
    let rgb = hover_office::page::compose(&frame, w, h, day);
    image::RgbImage::from_raw(w as u32, h as u32, rgb).unwrap().save(&out).unwrap();
    eprintln!("{out}: {} in {:?}", r.adapter_name, t0.elapsed());
}

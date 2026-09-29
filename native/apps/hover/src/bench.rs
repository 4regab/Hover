//! `HOVER_BENCH=1`: a line channel on stdin for port/bench/measure-hover.py, the Linux
//! counterpart of Measure-Hover.ps1 (which drives the C# app through UI Automation).
//! Each command takes the path a user's action takes (toggle is the shortcut's,
//! open is the history panel's); answers are `bench <what> <value>` lines on stdout.
// The channel is Linux's (the benchmark there); on Windows Measure-Hover.ps1 drives the app.
#![cfg_attr(windows, allow(dead_code))]

use slint::ComponentHandle;
use std::io::BufRead;
use std::sync::Mutex;
use std::time::Instant;

static FRAMES: Mutex<Vec<Instant>> = Mutex::new(Vec::new());
static WAIT: Mutex<Option<(Instant, &'static str)>> = Mutex::new(None);

pub fn active() -> bool { std::env::var_os("HOVER_BENCH").is_some() }

/// An office frame reached the window: its time, and the end of a timed wait.
pub fn office_frame() {
    if !active() { return; }
    let now = Instant::now();
    let mut f = FRAMES.lock().unwrap();
    f.push(now);
    if f.len() > 4000 { f.drain(..2000); }
    if let Some((t, what)) = WAIT.lock().unwrap().take() { println!("bench {what} {:.1}", (now - t).as_secs_f64() * 1000.0); }
}

/// The notch is on screen (S1's end).
pub fn visible() { if active() { println!("bench visible"); } }

fn pct(v: &mut [f64], p: f64) -> f64 { if v.is_empty() { return 0.0; } v.sort_by(f64::total_cmp); v[((v.len() - 1) as f64 * p).round() as usize] }

pub fn listen() {
    std::thread::spawn(|| {
        for line in std::io::stdin().lock().lines() {
            let Ok(line) = line else { return };
            let parts: Vec<String> = line.split_whitespace().map(str::to_owned).collect();
            let Some(cmd) = parts.first().cloned() else { continue };
            crate::ui_do(move |a| match cmd.as_str() {
                "toggle" => {
                    let opening = a.n.borrow().hover.state == hover_notch::State::Rest;
                    if opening { *WAIT.lock().unwrap() = Some((Instant::now(), "reopen")); }
                    a.toggle();
                    println!("bench toggled {}", if opening { "open" } else { "rest" });
                }
                "history" => { a.open_panel(Some("history")); println!("bench ok"); }
                "open" => {
                    let key = parts.get(1).cloned().unwrap_or_default();
                    *WAIT.lock().unwrap() = Some((Instant::now(), "opened"));
                    match a.hover.sessions.all().into_iter().find(|s| s.key == key) {
                        Some(s) => a.open_session(s.id),
                        None => { let s = a.hover.sessions.saved(&key); println!("bench saved {}", s.is_some()); if let Some(s) = a.hover.sessions.wake(&key) { a.office_changed(); a.open_session(s.id); } }
                    }
                }
                "scroll" => { let dy: f32 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(0.0); a.notch.global::<crate::ui::Office>().invoke_d_wheel(dy); println!("bench ok"); }
                "start" => {
                    let folder = parts.get(1).cloned().unwrap_or_default();
                    let n: usize = parts.get(2).and_then(|v| v.parse().ok()).unwrap_or(1);
                    let mut ids = vec![];
                    for i in 0..n { if let Some(s) = a.hover.sessions.start(hover_core::model::AgentTool::Kiro, &folder, &format!("bench task {i}"), vec![]) { ids.push(s.id); } }
                    a.office_changed();
                    if let Some(id) = ids.first() { a.open_session(*id); }
                    println!("bench started {}", ids.iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","));
                }
                "stopall" => { a.hover.sessions.stop_all(); println!("bench ok"); }
                "running" => println!("bench running {}", a.hover.sessions.running()),
                "state" => {
                    let n = a.n.borrow();
                    println!("bench state {:?} {:.2} view {} kind {} card {}", n.hover.state, n.openness(), a.notch.get_view_visible(), a.notch.get_rest_kind(), a.card.get());
                }
                "frames" => {
                    // Presented-frame intervals over the last N seconds (default 10).
                    let secs: f64 = parts.get(1).and_then(|v| v.parse().ok()).unwrap_or(10.0);
                    let f = FRAMES.lock().unwrap();
                    let now = Instant::now();
                    let recent: Vec<&Instant> = f.iter().filter(|t| (now - **t).as_secs_f64() <= secs).collect();
                    let mut iv: Vec<f64> = recent.windows(2).map(|w| (*w[1] - *w[0]).as_secs_f64() * 1000.0).collect();
                    let n = recent.len();
                    println!("bench frames {n} {:.1} {:.1} {:.1} {}", pct(&mut iv, 0.5), pct(&mut iv, 0.95), pct(&mut iv, 0.99), crate::FRAMES.load(std::sync::atomic::Ordering::Relaxed));
                }
                "quit" => { let _ = slint::quit_event_loop(); }
                _ => println!("bench unknown"),
            });
        }
    });
}

//! Hover's agent backend: the process the Mac app's Swift UI starts, and the same protocol
//! for any host. JSON lines on stdin (commands) and stdout (messages), one event loop,
//! hover-core, hover-agents and hover-quota underneath. src/Hover.Backend in Rust.
//!
//! The first command is `initialize` with the history key; stdin ending means the host
//! died, so every tool is shut down, the history is written and the backend exits.

pub mod backend;
pub mod browser_host;
pub mod office;
pub mod panels;
pub mod prefs;
pub mod quotas;
pub mod screen;
pub mod updates;
pub mod wire;

use backend::Link;
use hover_core::json::Json;
use std::io::{BufRead, Read};
use std::sync::Arc;
use wire::{run_loop, Host, Loop, Out};

/// Program.cs refuses a line longer than this and treats it as the end of the host.
const LINE_LIMIT: u64 = 48 * 1024 * 1024;

/// The host's lines, each posted to the loop as a command; the end of the input shuts
/// everything down.
fn read_commands(mut input: impl BufRead, lp: &Loop, out: &Arc<Out>) {
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match (&mut input).take(LINE_LIMIT + 1).read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) if buf.len() as u64 > LINE_LIMIT => break,
            Ok(_) => {}
        }
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\r', '\n']);
        if line.trim().is_empty() { continue; }
        match hover_core::json::parse(line) {
            Err(_) => out.toast("Invalid host message."),
            Ok(command) => {
                let link = Link::new(lp.clone(), out.clone());
                lp.post(move |h| dispatch(h, command, link));
            }
        }
    }
    // The native host died or closed the pipe.
    lp.post(|h| { if let Some(b) = &h.backend { b.shutdown(); } h.done = true; });
}

/// One command: the first makes the backend, the rest are its to handle. A failure before
/// the backend exists is `backendFailure` (the host stops); after, an `error` the host shows
/// as a popup (it was a toast, gone before it could be read).
fn dispatch(h: &mut Host, command: Json, link: Link) {
    let of = wire::str_of(&command, "type").map(str::to_owned);
    let result = match h.backend.as_mut() {
        // A command that panics costs that command, not the host's backend.
        Some(b) => std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| b.handle(&command))).unwrap_or_else(|p| {
            Err(p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| "That didn’t work.".into()))
        }),
        None => backend::initialize(&command, &link).map(|b| { h.backend = Some(b); }),
    };
    if let Err(e) = result {
        if h.backend.is_none() { link.out.send(&Json::obj(vec![("type", Json::str("backendFailure")), ("text", Json::str(e))])); }
        else {
            hover_core::log::line(&format!("backend: {} failed - {e}", of.as_deref().unwrap_or("a command")));
            link.out.error(&e, of.as_deref());
        }
    }
    if h.backend.as_ref().is_some_and(|b| b.closing()) { h.done = true; }
}

/// Runs the backend on this process's own pipes, until the host says `shutdown` or hangs
/// up. The caller exits afterwards; threads still blocked on the input don't hold it.
pub fn run(input: impl Read + Send + 'static, out: Arc<Out>) {
    let (lp, rx) = Loop::new();
    let out2 = out.clone();
    let lp2 = lp.clone();
    std::thread::Builder::new().name("host-input".into()).spawn(move || read_commands(std::io::BufReader::new(input), &lp2, &out2)).expect("a thread for the host's input");
    run_loop(rx);
}

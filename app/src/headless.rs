//! `hoverai --service`: Hover with no window, for saved tasks, webhooks, pull request watches and quota resumes while the app is
//! closed (hover-agents::service says how it is installed and how it hands over to the app).
//!
//! It takes the timers only if the app doesn't hold them, runs only tasks that never ask, and when the app starts it lets go of the
//! timers, lets the runs it has finish, and exits. It is started again (by the user's unit) when the app quits.

use crate::app::Hover;
use hover_agents::{service, wake};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

static STOP: AtomicBool = AtomicBool::new(false);

#[cfg(unix)]
fn on_signals() {
    extern "C" fn on(_: libc::c_int) { STOP.store(true, Ordering::SeqCst); }
    unsafe {
        libc::signal(libc::SIGTERM, on as extern "C" fn(libc::c_int) as libc::sighandler_t);
        libc::signal(libc::SIGINT, on as extern "C" fn(libc::c_int) as libc::sighandler_t);
    }
}

/// Runs until stopped or handed over. The exit code is for the unit: 0 is a clean end.
pub fn run() -> i32 {
    let dir = Hover::exec_dir();
    if let wake::Held::By(who) = wake::held(&dir) {
        println!("{who} runs the timers; the service has nothing to do now.");
        return 0;
    }
    #[cfg(unix)]
    on_signals();
    let hover = Hover::start_headless();
    hover.sched.set_service_mode(true);
    hover_core::log::line("service: started");
    while !STOP.load(Ordering::SeqCst) {
        if !hover.has_timers() && !hover.take_timers("the service", true) { break; }
        if service::handover_requested(&dir) {
            hover_core::log::line("service: the app is starting; letting go of the timers");
            hover.give_up_timers();
            // The runs it has finish; new ones are the app's.
            while hover.sessions.running() > 0 && !STOP.load(Ordering::SeqCst) { std::thread::sleep(Duration::from_millis(500)); }
            break;
        }
        std::thread::sleep(Duration::from_millis(500));
    }
    hover.shutdown();
    hover_core::log::line("service: stopped");
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The service takes the timers when nothing else has them; when the app asks, it lets go, finishes and exits; and when the app
    /// already has the timers it does nothing. (The real exit code and the lock are the ones the unit relies on.)
    #[test]
    fn the_service_takes_the_timers_and_hands_them_to_the_app_when_asked() {
        let dir = Hover::exec_dir();
        let _ = std::fs::remove_file(service::handover_file(&dir));
        let t = std::thread::spawn(run);
        let t0 = std::time::Instant::now();
        while wake::held(&dir) != wake::Held::By("the service".into()) && t0.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(20)); }
        assert_eq!(wake::held(&dir), wake::Held::By("the service".into()), "the service holds the timers");
        // The app starts: it finds them held, and asks.
        assert!(wake::Executor::acquire(&dir, "the app").is_err());
        service::request_handover(&dir, "the app");
        let t0 = std::time::Instant::now();
        while !t.is_finished() && t0.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(20)); }
        assert!(t.is_finished(), "the service left");
        assert_eq!(t.join().unwrap(), 0);
        // The app can now take them, and clears its request.
        let app = wake::Executor::acquire(&dir, "the app").expect("free now");
        service::clear_handover(&dir);
        assert!(!service::handover_requested(&dir));
        // With the app holding them, a service started now has nothing to do.
        STOP.store(false, Ordering::SeqCst);
        assert_eq!(run(), 0);
        assert_eq!(wake::held(&dir), wake::Held::By("the app".into()));
        drop(app);
    }
}

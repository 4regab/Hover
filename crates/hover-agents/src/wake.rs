//! One durable place for "do this later": a list of timers kept in Hover's data folder and one thread that
//! sleeps until the earliest is due. Scheduled tasks, pull request watches and quota-reset resumes all use it,
//! so they share its rules:
//!
//! - **One owner.** Only the process that holds the profile's executor lock fires timers (`Executor`). The desktop
//!   app and the optional background service can start together; one of them owns the timers, the other waits or
//!   just shows. A second never starts a duplicate.
//! - **Nothing stale fires.** Setting a timer for a key replaces the old one and bumps its generation; cancelling
//!   removes it. A handler is given the generation and asks `current` before acting, so a wake-up that was
//!   already on its way when the user cancelled or changed something does nothing.
//! - **At most once.** A timer is taken off the list and saved before its handler runs. A crash in between loses
//!   that one firing (the handler's own records, with the due time, say whether it happened) and never repeats it.
//! - **Honest about time.** Due times are wall-clock milliseconds. The wait is at most 30 s at a time, so a clock
//!   change, or a computer that slept, is noticed on the next wake-up; a timer found overdue is handed to its handler
//!   with how late it is, and the handler applies its own catch-up rule (never an unlimited backlog).
//! - **No process for waiting.** Waiting creates no provider process: the thread sleeps on a condition.

use hover_core::json::Json;
use hover_core::store::Sealed;
use std::collections::HashMap;
use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::Duration;

pub fn now_ms() -> i64 { hover_core::time::Stamp::now().unix_ms() }

/// The longest the thread sleeps without looking at the clock again.
const SLICE: Duration = Duration::from_secs(30);

#[derive(Clone, Debug, PartialEq)]
pub struct Timer {
    /// What it is for (`task`, `watch`, `resume`, …) and whose it is (a task id, a watch id, a session key).
    pub kind: String,
    pub key: String,
    pub due: i64,
    pub gen: u64,
    /// Whatever the handler wants back (a few words).
    pub note: String,
}

type Handler = Arc<dyn Fn(&Timer, i64) + Send + Sync>;

#[derive(Default)]
struct St { timers: Vec<Timer>, next_gen: u64, stop: bool, handlers: HashMap<String, Handler>, fired: u64, floor: HashMap<(String, String), u64> }

pub struct Wake {
    me: Weak<Wake>,
    st: Mutex<St>,
    cv: Condvar,
    doc: Option<Sealed>,
    running: Mutex<bool>,
}

impl Wake {
    pub fn new(doc: Option<Sealed>) -> Arc<Wake> {
        let mut st = St::default();
        if let Some(Json::Arr(list)) = doc.as_ref().and_then(Sealed::read).and_then(|v| v.get("Timers").cloned()) {
            for t in &list {
                let s = |k: &str| t.get(k).and_then(Json::as_str).unwrap_or("").to_owned();
                let n = |k: &str| t.get(k).and_then(|x| x.i64().ok()).unwrap_or(0);
                st.timers.push(Timer { kind: s("Kind"), key: s("Key"), due: n("Due"), gen: n("Gen") as u64, note: s("Note") });
            }
            st.next_gen = st.timers.iter().map(|t| t.gen).max().unwrap_or(0) + 1;
        }
        Arc::new_cyclic(|me| Wake { me: me.clone(), st: Mutex::new(st), cv: Condvar::new(), doc, running: Mutex::new(false) })
    }

    fn save(&self, st: &St) {
        if let Some(d) = &self.doc {
            let list = st.timers.iter().map(|t| Json::obj(vec![("Kind", Json::str(&t.kind)), ("Key", Json::str(&t.key)), ("Due", Json::int(t.due)), ("Gen", Json::int(t.gen as i64)), ("Note", Json::str(&t.note))])).collect();
            if let Err(e) = d.write(&Json::obj(vec![("Timers", Json::Arr(list))])) { hover_core::log::line(&format!("wake: save failed - {e}")); }
        }
    }

    /// What to do when a timer of this kind is due: given the timer and how many milliseconds late it is.
    pub fn on(&self, kind: &str, f: impl Fn(&Timer, i64) + Send + Sync + 'static) { self.st.lock().unwrap().handlers.insert(kind.into(), Arc::new(f)); }

    /// Sets the timer for (kind, key), replacing any earlier one. Returns its generation.
    pub fn set(&self, kind: &str, key: &str, due: i64, note: &str) -> u64 {
        let mut g = self.st.lock().unwrap();
        g.timers.retain(|t| !(t.kind == kind && t.key == key));
        let gen = g.next_gen;
        g.next_gen += 1;
        g.floor.insert((kind.into(), key.into()), gen);
        g.timers.push(Timer { kind: kind.into(), key: key.into(), due, gen, note: note.into() });
        self.save(&g);
        drop(g);
        self.cv.notify_all();
        gen
    }

    pub fn cancel(&self, kind: &str, key: &str) {
        let mut g = self.st.lock().unwrap();
        let before = g.timers.len();
        g.timers.retain(|t| !(t.kind == kind && t.key == key));
        // Anything already on its way with an older generation is stale from now on.
        let floor = g.next_gen;
        g.next_gen += 1;
        g.floor.insert((kind.into(), key.into()), floor);
        if g.timers.len() != before { self.save(&g); }
    }

    /// Whether the timer the handler was given is still the one set for its key (it was not replaced or cancelled since).
    pub fn current(&self, kind: &str, key: &str, gen: u64) -> bool { self.st.lock().unwrap().floor.get(&(kind.to_owned(), key.to_owned())).is_none_or(|f| gen >= *f) }

    pub fn get(&self, kind: &str, key: &str) -> Option<Timer> { self.st.lock().unwrap().timers.iter().find(|t| t.kind == kind && t.key == key).cloned() }
    pub fn all(&self) -> Vec<Timer> { self.st.lock().unwrap().timers.clone() }
    /// How many timers have fired in this run of Hover.
    pub fn fired(&self) -> u64 { self.st.lock().unwrap().fired }

    /// Starts the thread that fires timers. Only the executor's owner should (see `Executor`). Starting it twice is harmless.
    pub fn start(&self) {
        let mut r = self.running.lock().unwrap();
        if *r { return; }
        *r = true;
        let me = self.me.clone();
        std::thread::Builder::new().name("wake".into()).spawn(move || loop {
            let Some(w) = me.upgrade() else { return };
            if !w.turn() { return; }
        }).expect("a thread for the timers");
    }

    pub fn stop(&self) {
        self.st.lock().unwrap().stop = true;
        self.cv.notify_all();
    }

    /// One wait and, if something is due, its firing. False when stopped.
    fn turn(&self) -> bool {
        let mut g = self.st.lock().unwrap();
        if g.stop { *self.running.lock().unwrap() = false; return false; }
        let now = now_ms();
        if let Some(i) = g.timers.iter().enumerate().filter(|(_, t)| t.due <= now).min_by_key(|(_, t)| t.due).map(|(i, _)| i) {
            let t = g.timers.remove(i);
            g.fired += 1;
            let h = g.handlers.get(&t.kind).cloned();
            self.save(&g);
            drop(g);
            if let Some(h) = h {
                let late = now - t.due;
                // A handler that panics costs that one firing, not the thread.
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| h(&t, late)));
            }
            return true;
        }
        let wait = g.timers.iter().map(|t| t.due - now).min().map_or(SLICE, |ms| Duration::from_millis(ms.max(1) as u64).min(SLICE));
        let _ = self.cv.wait_timeout(g, wait).unwrap();
        true
    }
}

// MARK: One executor per profile

/// Holds the profile's executor lock while it lives. The lock is the OS's advisory lock on a file in the data
/// folder, so it goes when the process does, however it ends.
pub struct Executor { _file: File, path: PathBuf }

/// Who holds the lock.
#[derive(Clone, Debug, PartialEq)]
pub enum Held { Free, By(String) }

impl Executor {
    /// Takes the lock for `who` ("the app", "the service"), or says who has it.
    pub fn acquire(dir: &Path, who: &str) -> Result<Executor, String> {
        let _ = std::fs::create_dir_all(dir);
        let path = dir.join("executor.lock");
        let file = std::fs::OpenOptions::new().create(true).read(true).write(true).truncate(false).open(&path).map_err(|e| format!("the lock file: {e}"))?;
        match file.try_lock() {
            Ok(()) => {
                // The name goes in a file of its own: on Windows the lock keeps everyone else from reading the locked file.
                // A glance (`held`, no name) writes nothing, so it can't blank the holder's name.
                if !who.is_empty() { let _ = std::fs::write(dir.join("executor.who"), who.as_bytes()); }
                Ok(Executor { _file: file, path })
            }
            Err(_) => Err(Self::holder(dir)),
        }
    }

    /// The name the holder wrote (empty if it hasn't yet).
    pub fn holder(dir: &Path) -> String { std::fs::read_to_string(dir.join("executor.who")).unwrap_or_default().trim().to_owned() }

    pub fn path(&self) -> &Path { &self.path }
}

/// Whether some process holds the lock right now (without taking it).
pub fn held(dir: &Path) -> Held {
    match Executor::acquire(dir, "") {
        Ok(_) => Held::Free,
        Err(who) => Held::By(if who.is_empty() { "another process".into() } else { who }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::crypto::Crypto;
    use std::sync::atomic::{AtomicUsize, Ordering};

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-wake-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    fn wait_for(f: impl Fn() -> bool) { let t = std::time::Instant::now(); while !f() && t.elapsed() < Duration::from_secs(10) { std::thread::sleep(Duration::from_millis(5)); } assert!(f(), "timed out"); }

    #[test]
    fn a_timer_fires_once_when_due_and_the_handler_is_told_how_late() {
        let w = Wake::new(None);
        let seen: Arc<Mutex<Vec<(String, i64)>>> = Default::default();
        let s2 = seen.clone();
        w.on("t", move |t, late| s2.lock().unwrap().push((t.key.clone(), late)));
        w.start();
        w.set("t", "a", now_ms() + 50, "");
        w.set("t", "overdue", now_ms() - 5_000, "");
        wait_for(|| seen.lock().unwrap().len() == 2);
        let got = seen.lock().unwrap().clone();
        assert_eq!(got[0].0, "overdue");
        assert!(got[0].1 >= 5_000, "found 5 s late and said so: {got:?}");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(seen.lock().unwrap().len(), 2, "each fired once");
        assert!(w.all().is_empty());
    }

    #[test]
    fn setting_replaces_and_cancelling_removes_so_an_old_wakeup_finds_itself_stale() {
        let w = Wake::new(None);
        let g1 = w.set("t", "k", now_ms() + 60_000, "");
        assert!(w.current("t", "k", g1));
        let g2 = w.set("t", "k", now_ms() + 120_000, "later");
        assert_eq!(w.all().len(), 1, "one timer per key");
        assert!(!w.current("t", "k", g1) && w.current("t", "k", g2));
        w.cancel("t", "k");
        assert!(w.get("t", "k").is_none());
        assert!(!w.current("t", "k", g2), "a wake-up already on its way when the timer was cancelled is stale");
        let g3 = w.set("t", "k", now_ms() + 1, "again");
        assert!(w.current("t", "k", g3) && !w.current("t", "k", g2));
    }

    #[test]
    fn timers_survive_a_restart_sealed_and_an_unowned_one_never_fires() {
        let d = dir("persist");
        let crypto = Arc::new(Crypto::with_key([3; 32]));
        let w = Wake::new(Some(Sealed::in_dir(&d, "wake", crypto.clone())));
        w.set("resume", "sess-1", now_ms() + 3_600_000, "after the limit");
        let raw = std::fs::read(d.join("wake.dat")).unwrap();
        assert!(!raw.windows(5).any(|x| x == b"sess-"), "sealed");
        let again = Wake::new(Some(Sealed::in_dir(&d, "wake", crypto)));
        assert_eq!(again.get("resume", "sess-1").map(|t| t.note), Some("after the limit".to_owned()));
        // A new timer after the restart gets a newer generation than any saved one.
        let g = again.set("resume", "sess-2", now_ms() + 1, "");
        assert!(g > again.get("resume", "sess-1").unwrap().gen);
        // Not started (not the owner): nothing fires, however due.
        let fired = Arc::new(AtomicUsize::new(0));
        let f2 = fired.clone();
        again.on("resume", move |_, _| { f2.fetch_add(1, Ordering::SeqCst); });
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(fired.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_handler_that_panics_costs_one_firing_and_a_stopped_wake_fires_nothing_more() {
        let w = Wake::new(None);
        let n = Arc::new(AtomicUsize::new(0));
        let n2 = n.clone();
        w.on("t", move |t, _| { n2.fetch_add(1, Ordering::SeqCst); if t.key == "boom" { panic!("handler failed"); } });
        w.start();
        w.set("t", "boom", now_ms(), "");
        w.set("t", "fine", now_ms() + 30, "");
        wait_for(|| n.load(Ordering::SeqCst) == 2);
        w.stop();
        std::thread::sleep(Duration::from_millis(100));
        w.set("t", "never", now_ms(), "");
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(n.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn only_one_process_owns_the_timers_at_a_time_and_the_lock_is_let_go_with_it() {
        let d = dir("lock");
        assert_eq!(held(&d), Held::Free);
        let a = Executor::acquire(&d, "the service").unwrap();
        assert_eq!(held(&d), Held::By("the service".into()));
        assert_eq!(Executor::acquire(&d, "the app").err().as_deref(), Some("the service"), "the second one is told who has it");
        drop(a);
        assert_eq!(held(&d), Held::Free);
        let b = Executor::acquire(&d, "the app").unwrap();
        assert!(b.path().ends_with("executor.lock"));
    }
}

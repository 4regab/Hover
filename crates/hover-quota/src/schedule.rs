//! OwlApp's quota polling: every quota that is switched on is read once its last
//! reading is five minutes old (the app ticks every 30 s), or at once when forced
//! (switched on, or Refresh in Settings). Readings of quotas switched off are dropped,
//! so a stale number never comes back with the switch. A read runs on a thread of its
//! own, and one quota is never read twice at once.

use crate::{KiroUsage, Reading};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const EVERY: Duration = Duration::from_secs(5 * 60);
/// OwlApp's DispatcherTimer.
pub const TICK: Duration = Duration::from_secs(30);

/// The readings and which reads are under way. Pure bookkeeping, so it is tested
/// with a clock of its own.
#[derive(Default)]
pub struct Book {
    pub readings: HashMap<String, (Reading, Instant)>,
    busy: HashSet<String>,
}

impl Book {
    /// RefreshQuotas: drops what is off (true when that changed anything) and marks
    /// busy the ids to read now, in the order given.
    pub fn refresh(&mut self, on: &[String], force: bool, now: Instant) -> (bool, Vec<String>) {
        let dropped: Vec<String> = self.readings.keys().filter(|k| !on.contains(k)).cloned().collect();
        for k in &dropped { self.readings.remove(k); }
        let mut start = vec![];
        for id in on {
            if self.busy.contains(id) { continue; }
            if !force && self.readings.get(id).is_some_and(|(_, at)| now.duration_since(*at) < EVERY) { continue; }
            self.busy.insert(id.clone());
            start.push(id.clone());
        }
        (!dropped.is_empty(), start)
    }

    /// A read finished: kept only while the quota is still switched on. True when
    /// the reading was kept (the views redraw).
    pub fn finished(&mut self, id: &str, r: Reading, still_on: bool, now: Instant) -> bool {
        self.busy.remove(id);
        if !still_on { return false; }
        self.readings.insert(id.to_owned(), (r, now));
        true
    }

    pub fn busy(&self, id: &str) -> bool { self.busy.contains(id) }
}

type Reader = Arc<dyn Fn(&str) -> Reading + Send + Sync>;
type IsOn = Arc<dyn Fn(&str) -> bool + Send + Sync>;
/// Told, on the poll's thread, the raw credits of every good Kiro reading.
pub type OnUsage = Arc<dyn Fn(KiroUsage) + Send + Sync>;

/// The book, the reader and a callback for changes: what OwlApp's quota half is.
#[derive(Clone)]
pub struct Poller {
    book: Arc<Mutex<Book>>,
    read: Reader,
    is_on: IsOn,
    changed: Arc<dyn Fn() + Send + Sync>,
}

impl Poller {
    pub fn new(read: Reader, is_on: IsOn, changed: Arc<dyn Fn() + Send + Sync>) -> Poller {
        Poller { book: Default::default(), read, is_on, changed }
    }

    /// The real readers.
    pub fn system(is_on: IsOn, changed: Arc<dyn Fn() + Send + Sync>) -> Poller { Poller::system_with(is_on, changed, Arc::new(|_| {})) }

    /// The real readers, and Kiro's raw credits handed on as well. They don't ride on
    /// `Reading`: a field there changes every struct literal of it (the backend's too),
    /// and the other tools would carry a field they never fill. The Kiro reader sees them
    /// where it parses the report, and the other tools are read as before.
    pub fn system_with(is_on: IsOn, changed: Arc<dyn Fn() + Send + Sync>, on_usage: OnUsage) -> Poller {
        Poller::new(Arc::new(move |id: &str| {
            if id != crate::item::KIRO { return crate::read::by_id(id); }
            let (r, usage) = crate::read::kiro_read();
            if let Some(u) = usage { on_usage(u); }
            r
        }), is_on, changed)
    }

    pub fn reading(&self, id: &str) -> Option<Reading> { self.book.lock().unwrap().readings.get(id).map(|r| r.0.clone()) }

    pub fn refresh(&self, force: bool) {
        let on: Vec<String> = crate::item::QUOTAS.iter().filter(|id| (self.is_on)(id)).map(|s| s.to_string()).collect();
        let (dropped, start) = self.book.lock().unwrap().refresh(&on, force, Instant::now());
        if dropped { (self.changed)(); }
        for id in start {
            let me = self.clone();
            std::thread::Builder::new().name(format!("quota-{id}")).spawn(move || {
                let r = (me.read)(&id);

                if !r.ok() { hover_core::log::line(&format!("quota {id}: {}", r.detail)); }
                let still_on = (me.is_on)(&id);
                let kept = me.book.lock().unwrap().finished(&id, r, still_on, Instant::now());
                if kept { (me.changed)(); }
            }).expect("a thread for the quota read");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> { v.iter().map(|s| s.to_string()).collect() }

    /// OwlApp.RefreshQuotas and ReadQuota, step by step.
    #[test]
    fn reads_what_is_on_once_it_is_five_minutes_old() {
        let mut b = Book::default();
        let t0 = Instant::now();
        assert_eq!(b.refresh(&ids(&["claude", "codex"]), false, t0), (false, ids(&["claude", "codex"])));
        // Under way: not started twice, even when forced.
        assert_eq!(b.refresh(&ids(&["claude", "codex"]), true, t0), (false, vec![]));
        assert!(b.finished("claude", Reading::fail("x"), true, t0));
        assert!(b.finished("codex", Reading::fail("y"), true, t0));
        assert_eq!(b.refresh(&ids(&["claude", "codex"]), false, t0 + Duration::from_secs(299)), (false, vec![]));
        assert_eq!(b.refresh(&ids(&["claude", "codex"]), false, t0 + Duration::from_secs(300)).1, ids(&["claude", "codex"]));
        b.finished("claude", Reading::fail("x"), true, t0 + Duration::from_secs(300));
        // Switched off while its read ran: the new reading isn't kept, and the old one
        // goes with the next refresh (ReadQuota returns early; RefreshQuotas drops it).
        assert!(!b.finished("codex", Reading::fail("new"), false, t0 + Duration::from_secs(300)));
        assert_eq!(b.readings["codex"].0.detail, "y");
        assert_eq!(b.refresh(&ids(&["claude"]), false, t0 + Duration::from_secs(300)), (true, vec![]));
        assert!(!b.readings.contains_key("codex"));

        // Forced: read again at once.
        assert_eq!(b.refresh(&ids(&["claude"]), true, t0 + Duration::from_secs(301)), (false, ids(&["claude"])));
        b.finished("claude", Reading::fail("x"), true, t0 + Duration::from_secs(301));
        // Switched off: dropped, and that is a change.
        assert_eq!(b.refresh(&[], false, t0 + Duration::from_secs(302)), (true, vec![]));
        assert!(b.readings.is_empty());
    }

    #[test]
    fn a_poller_reads_on_threads_and_says_when() {
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let p = Poller::new(Arc::new(|id: &str| Reading { used: Some(42.0), detail: id.to_owned() }), Arc::new(|id: &str| id == "kiro"),
            Arc::new(move || { let _ = tx.lock().unwrap().send(()); }));
        p.refresh(false);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(p.reading("kiro"), Some(Reading { used: Some(42.0), detail: "kiro".into() }));
        assert_eq!(p.reading("claude"), None);
    }
}

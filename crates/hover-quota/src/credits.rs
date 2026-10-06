//! Kiro's credits by day, for Settings → Kiro: "B minus A". B is what the Kiro account
//! spent on a day across every client (`daily`, from the kiro-cli /usage counter), A is
//! what Hover's own Kiro turns spent (`hover_core::ledger`, from the saved history), and
//! what is left is spent outside Hover: the Kiro IDE, kiro-cli on its own and Kiro Web.
//! `combine` is pure. `Credits` runs it on a thread of its own, since the history's
//! sessions are sealed files, and keeps the latest result for Settings to read.

use crate::daily::{self, Day};
use crate::KiroUsage;
use chrono::{Datelike, Local, Months, NaiveDate};
use hover_core::history::AgentHistory;
use hover_core::ledger::{self, DayA, SessionCredits};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Condvar, Mutex, Once};
use std::time::Duration;

/// Days in the view (the longer of Settings' two ranges).
pub const DAYS: i64 = 30;
/// A month's pace means little before this many days of it.
const MIN_CYCLE_DAYS: i64 = 3;
/// Hover's credits may top Kiro's total by this much before it is worth a line in the log:
/// credits come in 0.01 steps.
const SLACK: f64 = 0.05;

/// One day. `total` is None when there is no reading to tell it from (Hover wasn't
/// running, or the quota is off), and then so is `outside`.
#[derive(Clone, Debug, PartialEq)]
pub struct CreditDay { pub date: NaiveDate, pub hover: f64, pub total: Option<f64>, pub outside: Option<f64>, pub partial: bool }

#[derive(Clone, Debug, PartialEq)]
pub struct CreditsView {
    /// The last 30 days, oldest first, those with nothing included.
    pub days: Vec<CreditDay>,
    pub today: CreditDay,
    /// The last 7 days' totals (the days with a total) and Hover's share of the 7.
    pub week_total: Option<f64>,
    pub week_hover: f64,
    /// The week's total over the days it is known for.
    pub per_day_7: Option<f64>,
    /// The reading of today, i.e. this month so far.
    pub month: Option<KiroUsage>,
    pub runs_out: Option<NaiveDate>,
    /// Today's dearest sessions in Hover, three at most.
    pub top_today: Vec<SessionCredits>,
}

pub fn combine(a: &BTreeMap<NaiveDate, DayA>, days: &[Day], today: NaiveDate) -> CreditsView {
    let b = daily::spent(days);
    let list: Vec<CreditDay> = (0..DAYS).rev().map(|back| today - chrono::Duration::days(back)).map(|date| {
        let hover = a.get(&date).map_or(0.0, |x| x.credits);
        let total = b.get(&date).map(|s| s.credits);
        CreditDay { date, hover, total, outside: total.map(|t| (t - hover).max(0.0)), partial: b.get(&date).is_some_and(|s| s.partial) }
    }).collect();
    let week = &list[list.len() - 7..];
    let known: Vec<f64> = week.iter().filter_map(|d| d.total).collect();
    let week_total = (!known.is_empty()).then(|| known.iter().sum::<f64>());
    // Only a reading from today describes this month so far; an older one is a stale number.
    let month = days.iter().max_by_key(|d| d.date).filter(|d| d.date == today)
        .map(|d| KiroUsage { used: d.used, limit: d.limit, plan: d.plan.clone(), reset: d.reset.clone() });
    let mut top_today = a.get(&today).map(|x| x.sessions.clone()).unwrap_or_default();
    top_today.sort_by(|x, y| y.credits.total_cmp(&x.credits));
    top_today.truncate(3);
    CreditsView {
        today: list[list.len() - 1].clone(),
        week_hover: week.iter().map(|d| d.hover).sum(),
        per_day_7: week_total.map(|t| t / known.len() as f64),
        week_total,
        runs_out: month.as_ref().and_then(|m| runs_out(m, today)),
        month,
        top_today,
        days: list,
    }
}

/// The next reset as kiro-cli prints it: "2026-10-01", or "10/01" (this year's, else next
/// year's once it has passed).
pub fn next_reset(text: &str, today: NaiveDate) -> Option<NaiveDate> {
    if let Ok(d) = NaiveDate::parse_from_str(text, "%Y-%m-%d") { return Some(d); }
    let (m, d) = text.split_once('/')?;
    let (m, d) = (m.parse().ok()?, d.parse().ok()?);
    let this = NaiveDate::from_ymd_opt(today.year(), m, d)?;
    if this >= today { Some(this) } else { NaiveDate::from_ymd_opt(today.year() + 1, m, d) }
}

/// The day the month's credits run out at the month's pace so far (used over the days
/// since the last reset, today counted), if that is before the next reset. None with too
/// few days of the month to tell, or no reset date to measure them from.
pub fn runs_out(u: &KiroUsage, today: NaiveDate) -> Option<NaiveDate> {
    if u.used >= u.limit { return Some(today); }
    let reset = next_reset(u.reset.as_deref()?, today)?;
    let elapsed = (today - reset.checked_sub_months(Months::new(1))?).num_days() + 1;
    if elapsed < MIN_CYCLE_DAYS || u.used <= 0.0 { return None; }
    let per_day = u.used / elapsed as f64;
    let out = today + chrono::Duration::days(((u.limit - u.used) / per_day).ceil() as i64);
    (out < reset).then_some(out)
}

// MARK: The thread

/// What the view is made from, and the thread's wake-ups.
struct Shared {
    history: Option<Arc<AgentHistory>>,
    file: PathBuf,
    latest: Mutex<Option<Arc<CreditsView>>>,
    state: Mutex<State>,
    wake: Condvar,
    changed: Arc<dyn Fn() + Send + Sync>,
}

struct State { dirty: bool, closed: bool }

/// A change asks for a new view, and a burst of them (a running task saves its session at
/// every step) makes one.
const GAP: Duration = Duration::from_secs(2);
/// With nothing changing, a new day still needs a new "today".
const IDLE: Duration = Duration::from_secs(5 * 60);

/// The latest view, kept up to date on a thread of its own. The UI thread only reads it.
pub struct Credits(Arc<Shared>, Once);

impl Credits {
    /// `changed` is called on the thread, when a new view differs from the last. The
    /// thread starts at the first `view`: the background service, which nobody looks at
    /// Settings in, never decrypts sessions to count credits.
    pub fn new(history: Option<Arc<AgentHistory>>, file: PathBuf, changed: Arc<dyn Fn() + Send + Sync>) -> Credits {
        Credits(Arc::new(Shared { history, file, latest: Mutex::new(None), state: Mutex::new(State { dirty: true, closed: false }), wake: Condvar::new(), changed }), Once::new())
    }

    /// The latest view; None until the first is made (the first call starts making it).
    pub fn view(&self) -> Option<Arc<CreditsView>> {
        self.1.call_once(|| {
            let me = self.0.clone();
            std::thread::Builder::new().name("credits".into()).spawn(move || run(me)).expect("a thread for the credits");
        });
        self.0.latest.lock().unwrap().clone()
    }

    /// The history changed, or Kiro was read: make the view again.
    pub fn poke(&self) {
        self.0.state.lock().unwrap().dirty = true;
        self.0.wake.notify_all();
    }

    /// A good Kiro reading (on the poll's thread): kept as today's, then counted.
    pub fn on_usage(&self, u: &KiroUsage) {
        daily::record(&self.0.file, u, Local::now());
        self.poke();
    }
}

impl Drop for Credits {
    fn drop(&mut self) {
        self.0.state.lock().unwrap().closed = true;
        self.0.wake.notify_all();
    }
}

fn run(sh: Arc<Shared>) {
    let mut logged = HashSet::new();
    let mut shown = Local::now().date_naive();
    loop {
        {
            let mut g = sh.state.lock().unwrap();
            while !g.dirty && !g.closed {
                let (next, timeout) = sh.wake.wait_timeout(g, IDLE).unwrap();
                g = next;
                if timeout.timed_out() && Local::now().date_naive() != shown { g.dirty = true; }
            }
            if g.closed { return; }
            g.dirty = false;
        }
        let today = Local::now().date_naive();
        shown = today;
        let a = sh.history.as_ref().map(|h| ledger::kiro_daily(h, today - chrono::Duration::days(DAYS - 1), today)).unwrap_or_default();
        let v = combine(&a, &daily::load(&sh.file), today);
        // Usually timing, a turn that ended around a poll: a line in the log, once a day, not an error on the page.
        for d in &v.days {
            if d.total.is_some_and(|t| d.hover > t + SLACK) && logged.insert(d.date) {
                hover_core::log::line(&format!("credits: Hover's Kiro tasks on {} ({:.2}) are over Kiro's own total ({:.2})", d.date, d.hover, d.total.unwrap_or(0.0)));
            }
        }
        let v = Arc::new(v);
        let same = sh.latest.lock().unwrap().as_deref() == Some(&*v);
        if !same {
            *sh.latest.lock().unwrap() = Some(v);
            (sh.changed)();
        }
        let g = sh.state.lock().unwrap();
        let _ = sh.wake.wait_timeout_while(g, GAP, |s| !s.closed).unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::crypto::Crypto;
    use hover_core::history::{SavedSession, SavedTurn};
    use hover_core::model::AgentTool;
    use hover_core::time::Stamp;

    fn date(m: u32, d: u32) -> NaiveDate { NaiveDate::from_ymd_opt(2026, m, d).unwrap() }
    fn day(m: u32, d: u32, first: f64, used: f64) -> Day {
        Day { date: date(m, d), first, used, limit: 50.0, reset: Some("11/01".into()), plan: Some("KIRO PRO".into()), at: String::new() }
    }
    fn a_of(m: u32, d: u32, credits: f64) -> BTreeMap<NaiveDate, DayA> {
        BTreeMap::from([(date(m, d), DayA { credits, turns: 1, sessions: vec![SessionCredits { key: "k".into(), title: "t".into(), folder: "f".into(), credits }] })])
    }
    fn usage(used: f64, reset: Option<&str>) -> KiroUsage { KiroUsage { used, limit: 50.0, plan: None, reset: reset.map(str::to_owned) } }

    #[test]
    fn thirty_days_oldest_first_and_outside_is_total_less_hover_never_below_zero() {
        let mut a = a_of(10, 6, 2.5);
        a.extend(a_of(10, 5, 4.0));
        let v = combine(&a, &[day(10, 4, 10.0, 12.0), day(10, 5, 12.0, 15.0), day(10, 6, 15.0, 17.0)], date(10, 6));
        assert_eq!(v.days.len(), 30);
        assert_eq!((v.days[0].date, v.days[29].date), (date(9, 7), date(10, 6)));
        assert_eq!(v.days[0], CreditDay { date: date(9, 7), hover: 0.0, total: None, outside: None, partial: false }, "a day with nothing is in it, unknown");
        let d5 = v.days.iter().find(|d| d.date == date(10, 5)).unwrap();
        assert_eq!((d5.hover, d5.total, d5.outside), (4.0, Some(3.0), Some(0.0)), "Hover is over Kiro's total: timing, not a negative");
        assert_eq!((v.today.hover, v.today.total, v.today.outside, v.today.partial), (2.5, Some(2.0), Some(0.0), false));
        let d4 = v.days.iter().find(|d| d.date == date(10, 4)).unwrap();
        assert_eq!((d4.total, d4.outside, d4.partial), (Some(2.0), Some(2.0), true), "the first day on file is partial");
    }

    #[test]
    fn with_no_reading_the_total_and_the_outside_are_unknown_but_hovers_credits_stand() {
        let v = combine(&a_of(10, 6, 1.25), &[], date(10, 6));
        assert_eq!((v.today.hover, v.today.total, v.today.outside), (1.25, None, None));
        assert_eq!((v.week_total, v.per_day_7, v.month.clone(), v.runs_out), (None, None, None, None));
        assert_eq!(v.week_hover, 1.25);
        assert_eq!(v.top_today.len(), 1);
    }

    #[test]
    fn the_week_adds_up_the_days_it_knows() {
        // Readings on 10/01, 10/02 and 10/04 (10/03 missing: a gap), read today 10/06 is unknown.
        let days = [day(10, 1, 10.0, 11.0), day(10, 2, 11.0, 13.0), day(10, 4, 20.0, 24.0)];
        let mut a = a_of(10, 2, 0.5);
        a.extend(a_of(10, 4, 1.0));
        let v = combine(&a, &days, date(10, 6));
        // 10/01 1 (first day), 10/02 2, 10/04 4 (after a gap: its own change).
        assert_eq!((v.week_total, v.per_day_7), (Some(7.0), Some(7.0 / 3.0)));
        assert_eq!(v.week_hover, 1.5);
        assert_eq!(v.month, None, "the last reading isn't from today");
    }

    #[test]
    fn the_month_is_todays_reading_and_the_top_sessions_are_three_at_most_dearest_first() {
        let mut a = a_of(10, 6, 6.0);
        a.get_mut(&date(10, 6)).unwrap().sessions = [0.5, 3.0, 1.0, 1.5].iter().enumerate().map(|(i, c)| SessionCredits { key: i.to_string(), title: String::new(), folder: String::new(), credits: *c }).collect();
        let v = combine(&a, &[day(10, 5, 1.0, 2.0), day(10, 6, 2.0, 5.0)], date(10, 6));
        assert_eq!(v.month.as_ref().map(|m| (m.used, m.limit, m.plan.as_deref())), Some((5.0, 50.0, Some("KIRO PRO"))));
        assert_eq!(v.top_today.iter().map(|s| s.credits).collect::<Vec<_>>(), [3.0, 1.5, 1.0]);
    }

    #[test]
    fn it_runs_out_only_when_that_is_before_the_reset() {
        let today = date(10, 10);
        // Reset 11/01: the month began 10/01, so today is day 10. 25 used is 2.5 a day: 10 more days.
        assert_eq!(runs_out(&usage(25.0, Some("11/01")), today), Some(date(10, 20)));
        // 4.5 a day is out in 2 days (1.1 rounds up); the slower 1.5 a day (15 used) in 24: after the reset.
        assert_eq!(runs_out(&usage(45.0, Some("11/01")), today), Some(date(10, 12)));
        assert_eq!(runs_out(&usage(15.0, Some("11/01")), today), None, "day 10 + 24 is past 11/01");
        // The same with the reset printed as a full date.
        assert_eq!(runs_out(&usage(25.0, Some("2026-11-01")), today), Some(date(10, 20)));
        // Too few days of the month, nothing used, or no reset date to count from.
        assert_eq!(runs_out(&usage(9.0, Some("11/01")), date(10, 2)), None);
        assert_eq!(runs_out(&usage(0.0, Some("11/01")), today), None);
        assert_eq!(runs_out(&usage(25.0, None), today), None);
        // Already out.
        assert_eq!(runs_out(&usage(50.0, Some("11/01")), today), Some(today));
        // A reset in January is next year's.
        assert_eq!(next_reset("01/01", date(12, 20)), NaiveDate::from_ymd_opt(2027, 1, 1));
        assert_eq!(runs_out(&usage(100.0, Some("01/01")), date(12, 20)), Some(date(12, 20)));
    }

    /// The thread, end to end: a Kiro task in the history and a reading, counted off the caller's thread.
    #[test]
    fn the_thread_makes_the_view_from_the_history_and_the_readings_and_says_when() {
        let dir = std::env::temp_dir().join(format!("hover-credits-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let h = Arc::new(AgentHistory::new(dir.join("agents"), Arc::new(Crypto::with_key([6; 32]))));
        let now = Stamp::now();
        let turn = SavedTurn { prompt: "p".into(), images: vec![], steps: vec![], state: None, text: None, started_at: now, woke_at: None, ended_at: Some(now), credits: Some(1.5), before: None, after: None, ext: Default::default() };
        h.save(&SavedSession { key: "kk".into(), tool: AgentTool::Kiro, folder: "C:\\x".into(), title: "T".into(), acp_id: None, context: None, turns: vec![turn], updated: now, access: None, cloud: None, ext: Default::default() });
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        let c = Credits::new(Some(h), dir.join("kiro-usage.json"), Arc::new(move || { let _ = tx.lock().unwrap().send(()); }));
        assert!(c.view().is_none(), "the first look starts it");
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let v = c.view().unwrap();
        assert_eq!((v.today.hover, v.today.total, v.top_today.len()), (1.5, None, 1));
        c.on_usage(&usage(4.0, Some("11/01")));
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        let v = c.view().unwrap();
        assert_eq!((v.today.hover, v.today.total, v.today.outside, v.today.partial), (1.5, Some(0.0), Some(0.0), true), "first day on file: its own change, nothing yet");
        assert_eq!(v.month.as_ref().map(|m| m.used), Some(4.0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}

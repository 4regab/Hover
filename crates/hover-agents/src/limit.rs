//! A task that stopped because the provider's usage ran out, and getting it going again when the limit lifts.
//!
//! - **Only real limits.** A turn counts as limited only when its ending text says a usage limit was reached (a quota, a 5-hour or
//!   weekly cap, out of credits). A busy model, too many requests for a moment, a sign-in that lapsed, or an ordinary failure are not
//!   limits and are left to the places that handle them. Nothing is inferred from a nearly full usage ring.
//! - **The provider's own time.** The reset time is read from what the provider said: a date and time, a time of day, "in 2 hours 13
//!   minutes", or a count of seconds since 1970. With none, the task shows Limited with the reason and no time, and offers a manual retry;
//!   Hover never guesses one.
//! - **One continuation, by the user's choice.** The user can arm a resume for the reset (per task, or as the default for new limits),
//!   snooze it, or cancel it. Snoozing alone sends nothing. The resume is one short message, "continue", sent as the next turn of the same
//!   conversation with the same agent, model, folder, access and queue: it goes before any message that was held behind the limit, and
//!   never changes the permissions.
//! - **Nothing stale.** New work from the user, Stop, deleting or putting the task away, or cancelling the resume removes it; a timer that
//!   was already on its way finds itself stale (wake.rs). One limit makes at most one continuation.
//! - **Timers only.** Waiting for a reset creates no provider process. If Hover was closed or asleep at the reset, the resume is made when it
//!   next runs (the reset has passed, which is what it was waiting for). Where the background service is off, this needs Hover open.
//!
//! Which providers really word their limits this way has not been checked against the providers themselves; the patterns below are the
//! commonly seen wordings, covered by tests, and anything they miss is simply not treated as a limit.

use crate::session::{KiroSessions, Msg};
use crate::wake::{now_ms, Timer, Wake};
use hover_core::model::KiroState;
use std::sync::{Arc, Mutex, Weak};

const KIND: &str = "resume";
/// A little after the reset, so the provider has really let go.
const SLACK_MS: i64 = 15_000;

/// What the provider said.
#[derive(Clone, Debug, PartialEq)]
pub struct Limit {
    /// The provider's words, short.
    pub reason: String,
    /// When it lifts (ms since 1970), if it said.
    pub reset_at: Option<i64>,
}

const LIMIT_WORDS: [&str; 10] = ["usage limit", "limit reached", "reached your limit", "reached the limit", "out of credits", "out of usage", "quota", "exceeded your", "weekly limit", "5-hour limit"];
const NOT_LIMIT: [&str; 9] = ["sign in", "log in", "logged out", "unauthorized", "authentication", "overloaded", "high demand", "high traffic", "trouble responding"];

/// A number and a unit, like "2 hours" or "45m": milliseconds.
fn span(words: &[&str]) -> Option<i64> {
    let mut total = 0i64;
    let mut found = false;
    let mut i = 0;
    while i < words.len() {
        let w = words[i].trim_matches(|c: char| !c.is_alphanumeric());
        // "2h", "45m", "30s" glued together, or "2" then "hours".
        let (num, unit) = match w.find(|c: char| c.is_alphabetic()) {
            Some(p) if p > 0 => (w[..p].parse::<i64>().ok(), w[p..].to_owned()),
            _ => (w.parse::<i64>().ok(), words.get(i + 1).map(|u| u.trim_matches(|c: char| !c.is_alphanumeric()).to_owned()).unwrap_or_default()),
        };
        if let Some(n) = num {
            let ms = match unit.as_str() { "d" | "day" | "days" => 86_400_000, "h" | "hr" | "hrs" | "hour" | "hours" => 3_600_000, "m" | "min" | "mins" | "minute" | "minutes" => 60_000, "s" | "sec" | "secs" | "second" | "seconds" => 1_000, _ => 0 };
            if ms > 0 { total += n * ms; found = true; if w.find(|c: char| c.is_alphabetic()).is_none() { i += 1; } }
        }
        i += 1;
    }
    found.then_some(total)
}

/// A time of day in the text ("3:45 PM", "15:45", "3pm"): minutes since midnight.
fn clock(words: &[&str]) -> Option<i64> {
    for (i, raw) in words.iter().enumerate() {
        let w = raw.trim_matches(|c: char| !c.is_alphanumeric() && c != ':').to_lowercase();
        let next = words.get(i + 1).map(|n| n.trim_matches(|c: char| !c.is_alphanumeric()).to_lowercase()).unwrap_or_default();
        let (digits, suffix) = match w.find(|c: char| c.is_alphabetic()) { Some(p) => (w[..p].to_owned(), w[p..].to_owned()), None => (w.clone(), next.clone()) };
        let pm = suffix == "pm" || suffix == "p.m";
        let am = suffix == "am" || suffix == "a.m";
        let (h, m) = match digits.split_once(':') { Some((h, m)) => (h.parse::<i64>().ok()?, m.parse::<i64>().ok()?), None if am || pm => (digits.parse::<i64>().ok()?, 0), None => continue };
        if !(0..=59).contains(&m) { continue; }
        let h = if pm && h < 12 { h + 12 } else if am && h == 12 { 0 } else { h };
        if (0..24).contains(&h) && (digits.contains(':') || am || pm) { return Some(h * 60 + m); }
    }
    None
}

/// Whether `text` says a usage limit was reached, and when it lifts. `offset_min` is the local zone's offset, for a time of day.
pub fn detect(text: &str, now: i64, offset_min: i64) -> Option<Limit> {
    let t = text.to_lowercase();
    if NOT_LIMIT.iter().any(|w| t.contains(w)) && !LIMIT_WORDS.iter().any(|w| t.contains(w)) { return None; }
    if !LIMIT_WORDS.iter().any(|w| t.contains(w)) { return None; }
    let reason = crate::stream::clip_to(text.lines().map(str::trim).find(|l| { let l = l.to_lowercase(); LIMIT_WORDS.iter().any(|w| l.contains(w)) }).unwrap_or(text.trim()), 200);
    let words: Vec<&str> = t.split_whitespace().collect();
    // A count of seconds since 1970 ("resets_at: 1790000000"), in seconds or milliseconds.
    let epoch = words.iter().filter_map(|w| w.trim_matches(|c: char| !c.is_ascii_digit()).parse::<i64>().ok()).find(|n| (1_500_000_000..=4_000_000_000).contains(n) || (1_500_000_000_000..=4_000_000_000_000).contains(n));
    // A date and time ("2026-10-06T18:00:00Z").
    let iso = text.split(|c: char| c.is_whitespace() || c == ',' || c == '(' || c == ')').find_map(|w| { let w = w.trim_matches(|c: char| c == '.' || c == '"'); if w.len() >= 16 && w.as_bytes()[4] == b'-' && w.contains('T') { hover_core::time::Stamp::parse(w).map(|s| s.unix_ms()) } else { None } });
    let reset_at = if let Some(ms) = iso { Some(ms) }
        else if let Some(n) = epoch { Some(if n < 100_000_000_000 { n * 1000 } else { n }) }
        else if let Some(pos) = words.iter().position(|w| matches!(*w, "in" | "after")) {
            span(&words[pos + 1..]).filter(|ms| *ms > 0).map(|ms| now + ms)
        } else if words.iter().any(|w| w.starts_with("reset") || *w == "at" || *w == "until" || *w == "again") {
            clock(&words).map(|mins| {
                // The next time the local clock shows that, after now.
                let local = now + offset_min * 60_000;
                let day = local.div_euclid(86_400_000) * 86_400_000;
                let mut at = day + mins * 60_000 - offset_min * 60_000;
                if at <= now { at += 86_400_000; }
                at
            })
        } else { None };
    Some(Limit { reason, reset_at: reset_at.filter(|r| *r > now - 60_000) })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Mode { /** shown, nothing scheduled */ Off, /** resumes at the reset */ Auto, /** hidden until then; sends nothing */ Snoozed(i64) }

/// A limited task: what happened and what is planned.
#[derive(Clone, Debug, PartialEq)]
pub struct Limited {
    pub session: String,
    /// How many turns the task had when it was limited: more means new work began since.
    pub turns: usize,
    pub limit: Limit,
    pub mode: Mode,
}

pub struct Limits {
    me: Weak<Limits>,
    st: Mutex<Vec<Limited>>,
    wake: Arc<Wake>,
    sessions: KiroSessions,
    auto_default: Box<dyn Fn() -> bool + Send + Sync>,
    stopped: Box<dyn Fn(&str) -> bool + Send + Sync>,
    offset: Box<dyn Fn(i64) -> i64 + Send + Sync>,
    slack: std::sync::atomic::AtomicI64,
}

impl Limits {
    /// `auto_default` says whether a new limit is armed for resuming at once (off unless the user turned it on).
    pub fn new(sessions: KiroSessions, wake: Arc<Wake>, auto_default: impl Fn() -> bool + Send + Sync + 'static, stopped: impl Fn(&str) -> bool + Send + Sync + 'static) -> Arc<Limits> {
        Limits::with_zone(sessions, wake, auto_default, stopped, |ms| crate::sched::Tz::Local.offset_min(ms))
    }

    pub fn with_zone(sessions: KiroSessions, wake: Arc<Wake>, auto_default: impl Fn() -> bool + Send + Sync + 'static, stopped: impl Fn(&str) -> bool + Send + Sync + 'static,
        offset: impl Fn(i64) -> i64 + Send + Sync + 'static) -> Arc<Limits> {
        let l = Arc::new_cyclic(|me| Limits { me: me.clone(), st: Mutex::new(vec![]), wake: wake.clone(), sessions: sessions.clone(), auto_default: Box::new(auto_default), stopped: Box::new(stopped), offset: Box::new(offset), slack: std::sync::atomic::AtomicI64::new(SLACK_MS) });
        let me = l.me.clone();
        wake.on(KIND, move |t, late| { if let Some(l) = me.upgrade() { l.fire(t, late); } });
        let me = l.me.clone();
        sessions.on_ended(move |s, r| { if let Some(l) = me.upgrade() { l.ended(s, r); } });
        let me = l.me.clone();
        sessions.on_stop(move |s| { if let Some(l) = me.upgrade() { l.cancel(&s.key); } });
        l
    }

    /// How long after the reset the resume waits (tests make it short).
    pub fn set_slack(&self, ms: i64) { self.slack.store(ms, std::sync::atomic::Ordering::SeqCst); }
    fn slack(&self) -> i64 { self.slack.load(std::sync::atomic::Ordering::SeqCst) }

    pub fn of(&self, session: &str) -> Option<Limited> { self.st.lock().unwrap().iter().find(|l| l.session == session).cloned() }
    pub fn all(&self) -> Vec<Limited> { self.st.lock().unwrap().clone() }

    fn ended(&self, s: &crate::session::KiroSession, r: &crate::stream::KiroResult) {
        // A task that carries on past a limit (new work) is no longer limited.
        if let Some(cur) = self.of(&s.key) { if s.turns.len() > cur.turns && r.state == KiroState::Completed { self.cancel(&s.key); } }
        if r.state != KiroState::Failed || r.unconfirmed { return; }
        let now = now_ms();
        let Some(limit) = detect(&r.text, now, (self.offset)(now)) else { return };
        let mode = if (self.auto_default)() && limit.reset_at.is_some() { Mode::Auto } else { Mode::Off };
        self.st.lock().unwrap().retain(|l| l.session != s.key);
        self.st.lock().unwrap().push(Limited { session: s.key.clone(), turns: s.turns.len(), limit: limit.clone(), mode: mode.clone() });
        if mode == Mode::Auto { if let Some(at) = limit.reset_at { self.wake.set(KIND, &s.key, at + self.slack(), ""); } }
        hover_core::log::line(&format!("limit: {} is limited{}", s.key, limit.reset_at.map_or(String::new(), |a| format!(" until {}", crate::sched::civil_text(a, crate::sched::Tz::Local)))));
    }

    /// Resume at the reset. Needs a reset time the provider gave.
    pub fn arm(&self, session: &str) -> Result<i64, String> {
        let mut g = self.st.lock().unwrap();
        let l = g.iter_mut().find(|l| l.session == session).ok_or("This task isn’t limited.")?;
        let at = l.limit.reset_at.ok_or("The provider didn’t say when the limit lifts, so Hover can’t schedule a resume. Retry by hand when you like.")?;
        l.mode = Mode::Auto;
        self.wake.set(KIND, session, at.max(now_ms()) + self.slack(), "");
        Ok(at)
    }

    /// Hides it until `until`; nothing is sent.
    pub fn snooze(&self, session: &str, until: i64) {
        let mut g = self.st.lock().unwrap();
        if let Some(l) = g.iter_mut().find(|l| l.session == session) { l.mode = Mode::Snoozed(until); }
        self.wake.cancel(KIND, session);
    }

    /// No resume, no limit shown.
    pub fn cancel(&self, session: &str) {
        self.wake.cancel(KIND, session);
        self.st.lock().unwrap().retain(|l| l.session != session);
    }

    /// The user’s own retry: continue now.
    pub fn retry_now(&self, session: &str) -> Result<(), String> {
        let l = self.of(session).ok_or("This task isn’t limited.")?;
        self.resume(&l)
    }

    fn fire(&self, t: &Timer, late: i64) {
        if !self.wake.current(KIND, &t.key, t.gen) { return; }
        let Some(l) = self.of(&t.key).filter(|l| l.mode == Mode::Auto) else { return };
        let _ = late; // The reset has passed, which is what it was waiting for: an overdue resume is still right.
        if (self.stopped)(&l.session) { self.cancel(&l.session); return; }
        if let Err(e) = self.resume(&l) {
            hover_core::log::line(&format!("limit: resume of {} waits: {e}", l.session));
            // No place or desk free yet: look again in a minute (still one continuation).
            self.wake.set(KIND, &l.session, now_ms() + 60_000, "");
        }
    }

    /// Sends the one continuation. Only if nothing has happened to the task since: not busy, no new turn, not stopped.
    fn resume(&self, l: &Limited) -> Result<(), String> {
        let s = self.sessions.find(&l.session).or_else(|| self.sessions.wake(&l.session)).ok_or("The task isn’t open and no desk is free.")?;
        if s.busy() { return Err("It is working.".into()); }
        if s.turns.iter().filter(|t| !t.queued).count() > l.turns || (self.stopped)(&l.session) { self.cancel(&l.session); return Ok(()); }
        // This continuation is the one: taken off first, so a second timer or a double click can't make another.
        self.wake.cancel(KIND, &l.session);
        self.st.lock().unwrap().retain(|x| x.session != l.session);
        // Ahead of anything held behind the limit.
        if !self.sessions.reply_first(s.id, Msg::text("continue")) {
            self.st.lock().unwrap().push(l.clone());
            return Err("No place is free to start it.".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_790_000_000_000;

    #[test]
    fn real_limits_are_told_from_busy_models_sign_ins_and_ordinary_failures() {
        for (text, is) in [
            ("You've hit your usage limit. Try again in 2 hours 13 minutes.", true),
            ("Claude usage limit reached. Your limit will reset at 3:45 PM.", true),
            ("5-hour limit reached ∙ resets 9pm", true),
            ("Error: quota exceeded for this account", true),
            ("You have reached your limit of free messages.", true),
            ("The model is overloaded. Try again in a few minutes.", false),
            ("High demand right now: too many requests", false),
            ("Please sign in to continue. Authentication failed.", false),
            ("The build failed with 3 errors.", false),
            ("Rate limiting is a technique for controlling traffic.", false),
        ] {
            assert_eq!(detect(text, NOW, 0).is_some(), is, "{text}");
        }
    }

    #[test]
    fn the_providers_own_reset_time_is_read_and_an_unknown_one_stays_unknown() {
        let ms = |mins: i64| NOW + mins * 60_000;
        assert_eq!(detect("usage limit reached. Try again in 2 hours 13 minutes.", NOW, 0).unwrap().reset_at, Some(ms(133)));
        assert_eq!(detect("usage limit reached, try again in 45m", NOW, 0).unwrap().reset_at, Some(ms(45)));
        assert_eq!(detect("usage limit reached, retry after 1 day", NOW, 0).unwrap().reset_at, Some(ms(1440)));
        assert_eq!(detect("usage limit reached (resets_at: 1790003600)", NOW, 0).unwrap().reset_at, Some(NOW + 3_600_000), "seconds since 1970");
        assert_eq!(detect("usage limit reached until 2026-10-01T10:00:00Z", 1_790_000_000_000 - 90 * 86_400_000, 0).unwrap().reset_at, Some(hover_core::time::Stamp::parse("2026-10-01T10:00:00Z").unwrap().unix_ms()));
        // A time of day is the next time the local clock shows it. NOW is 14:13:20 UTC.
        let r = detect("usage limit reached. Resets at 3:45 PM", NOW, 0).unwrap().reset_at.unwrap();
        assert_eq!(r - NOW, (91 * 60 + 40) * 1000, "15:45 UTC is 91 minutes 40 seconds later");
        let east = detect("usage limit reached. Resets at 3:45 PM", NOW, 120).unwrap().reset_at.unwrap();
        assert_eq!(east - NOW, 84_700_000, "two hours east it is already 16:13, so 15:45 local is tomorrow's: 13:45 UTC the next day");
        let tomorrow = detect("5-hour limit reached ∙ resets 9am", NOW, 0).unwrap().reset_at.unwrap();
        assert!(tomorrow > NOW + 18 * 3_600_000, "9am has passed today, so it is tomorrow's: {}", (tomorrow - NOW) / 3_600_000);
        // No time given: none is made up. One already in the past is not taken.
        assert_eq!(detect("You have hit your weekly limit.", NOW, 0).unwrap().reset_at, None);
        assert_eq!(detect("usage limit reached, resets_at: 1700000000", NOW, 0).unwrap().reset_at, None);
        assert!(detect("usage limit reached. Try again in 2 hours", NOW, 0).unwrap().reason.contains("usage limit"));
    }
}

//! One Kiro usage reading per local day, kept in `kiro-usage.json`, so a day's credits
//! (all of Kiro's clients together: the IDE, kiro-cli, Kiro Web and Hover) can be told
//! as the change in the monthly "used" counter between the days' last readings. The
//! file is plain JSON, with nothing secret in it. `kiro-cli /usage` is the only source,
//! read while Hover runs; the maths (`add`, `spent`) is pure, with the clock and the
//! data passed in.

use crate::KiroUsage;
use chrono::{DateTime, Local, NaiveDate, SecondsFormat};
use hover_core::json::{self, Json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// Days kept: more than a year of them is a few tens of KB.
const KEEP: usize = 400;

/// A day's readings: the first (where the day began, as far as Hover saw) and the latest.
/// `reset` is kiro-cli's printed next reset ("10/01" or "2026-10-01").
#[derive(Clone, Debug, PartialEq)]
pub struct Day {
    pub date: NaiveDate,
    pub first: f64,
    pub used: f64,
    pub limit: f64,
    pub reset: Option<String>,
    pub plan: Option<String>,
    /// When the latest reading was taken (ISO 8601, local offset).
    pub at: String,
}

/// What Kiro's account spent on a day, across every client. `partial`: only the part of
/// the day Hover saw (the first day, or the one after a day Hover wasn't running).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spent { pub credits: f64, pub partial: bool }

pub fn path() -> PathBuf { hover_core::paths::support().join("kiro-usage.json") }

/// The counter started again: it went down, or kiro-cli now prints another reset date.
/// A reset text that is missing on either side proves nothing (a format drift).
fn reset_between(before: &Day, used: f64, reset: Option<&str>) -> bool {
    used < before.used || matches!((before.reset.as_deref(), reset), (Some(a), Some(b)) if a != b)
}

/// Adds a reading to today's row (or starts it). The first reading of a day stays its
/// `first`, except after a reset within the day: the counter restarted from nothing, so
/// what it held before doesn't count toward today.
pub fn add(days: &mut Vec<Day>, u: &KiroUsage, now: DateTime<Local>) {
    let date = now.date_naive();
    let at = now.to_rfc3339_opts(SecondsFormat::Secs, false);
    match days.iter_mut().find(|d| d.date == date) {
        Some(d) => {
            if reset_between(d, u.used, u.reset.as_deref()) { d.first = 0.0; }
            (d.used, d.limit, d.reset, d.plan, d.at) = (u.used, u.limit, u.reset.clone(), u.plan.clone(), at);
        }
        None => {
            days.push(Day { date, first: u.used, used: u.used, limit: u.limit, reset: u.reset.clone(), plan: u.plan.clone(), at });
            days.sort_by_key(|d| d.date);
        }
    }
    if days.len() > KEEP { days.drain(..days.len() - KEEP); }
}

/// Each recorded day's credits. Normally the day's last reading less the previous day's.
/// After a monthly reset that is the reading itself. With no previous day, or one that
/// isn't the day before (Hover wasn't running), only the day's own change is known: last
/// less first, and partial. Never negative.
pub fn spent(days: &[Day]) -> BTreeMap<NaiveDate, Spent> {
    let mut sorted: Vec<&Day> = days.iter().collect();
    sorted.sort_by_key(|d| d.date);
    let mut out = BTreeMap::new();
    let mut prev: Option<&Day> = None;
    for d in sorted {
        let (credits, partial) = match prev {
            Some(p) if p.date.succ_opt() == Some(d.date) => (if reset_between(p, d.used, d.reset.as_deref()) { d.used } else { d.used - p.used }, false),
            _ => (d.used - d.first, true),
        };
        out.insert(d.date, Spent { credits: credits.max(0.0), partial });
        prev = Some(d);
    }
    out
}

// MARK: The file

fn to_json(days: &[Day]) -> String {
    let row = |d: &Day| Json::obj(vec![
        ("Date", Json::str(d.date.format("%Y-%m-%d").to_string())), ("First", Json::double(d.first)), ("Used", Json::double(d.used)), ("Limit", Json::double(d.limit)),
        ("Reset", Json::opt_str_of(d.reset.as_deref())), ("Plan", Json::opt_str_of(d.plan.as_deref())), ("At", Json::str(&d.at)),
    ]);
    Json::obj(vec![("Version", Json::int(1)), ("Days", Json::Arr(days.iter().map(row).collect()))]).compact()
}

/// A row that can't be read is left out; a file that isn't ours (not JSON, another
/// Version) is an Err.
fn from_json(text: &str) -> Result<Vec<Day>, String> {
    let v = json::parse(text).map_err(|e| e.to_string())?;
    if v.get("Version").and_then(|x| x.f64().ok()) != Some(1.0) { return Err("not version 1".into()); }
    let rows = v.get("Days").and_then(|d| d.items().ok()).ok_or("no Days")?;
    let s = |r: &Json, k: &str| r.get(k).and_then(Json::as_str).map(str::to_owned);
    let n = |r: &Json, k: &str| r.get(k).and_then(|x| x.f64().ok());
    let mut days: Vec<Day> = rows.iter().filter_map(|r| Some(Day {
        date: NaiveDate::parse_from_str(&s(r, "Date")?, "%Y-%m-%d").ok()?,
        first: n(r, "First")?, used: n(r, "Used")?, limit: n(r, "Limit")?,
        reset: s(r, "Reset"), plan: s(r, "Plan"), at: s(r, "At").unwrap_or_default(),
    })).collect();
    days.sort_by_key(|d| d.date);
    Ok(days)
}

/// The days on file; none when there is no file. One that can't be read is set aside,
/// not written over (the history does the same with its index).
pub fn load(path: &Path) -> Vec<Day> {
    let Ok(bytes) = std::fs::read(path) else { return vec![] };
    from_json(&json::text_of(&bytes)).unwrap_or_else(|e| {
        hover_core::log::line(&format!("kiro usage: {} unreadable - {e}; starting a new one", path.display()));
        let _ = std::fs::rename(path, path.with_file_name(format!("kiro-usage-{}.bad", hover_core::guid_n())));
        vec![]
    })
}

static WRITING: Mutex<()> = Mutex::new(());

/// Saves a good Kiro reading as today's. Blocks on the disk: called on the poll's
/// thread, never the UI's. Written to a temporary file and then renamed over the old one.
pub fn record(path: &Path, u: &KiroUsage, now: DateTime<Local>) {
    let _g = WRITING.lock().unwrap_or_else(|e| e.into_inner());
    let mut days = load(path);
    add(&mut days, u, now);
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let r = std::fs::create_dir_all(path.parent().unwrap_or(Path::new("."))).and_then(|_| std::fs::write(&tmp, to_json(&days))).and_then(|_| std::fs::rename(&tmp, path));
    if let Err(e) = r { hover_core::log::line(&format!("kiro usage: save failed - {e}")); }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn date(m: u32, d: u32) -> NaiveDate { NaiveDate::from_ymd_opt(2026, m, d).unwrap() }
    fn at(m: u32, d: u32, h: u32) -> DateTime<Local> { Local.with_ymd_and_hms(2026, m, d, h, 0, 0).unwrap() }
    fn usage(used: f64, reset: &str) -> KiroUsage { KiroUsage { used, limit: 50.0, plan: Some("KIRO PRO".into()), reset: Some(reset.into()) } }
    fn day(m: u32, d: u32, first: f64, used: f64, reset: &str) -> Day {
        Day { date: date(m, d), first, used, limit: 50.0, reset: Some(reset.into()), plan: Some("KIRO PRO".into()), at: String::new() }
    }
    fn credits(s: &BTreeMap<NaiveDate, Spent>, m: u32, d: u32) -> (f64, bool) { let x = s[&date(m, d)]; (x.credits, x.partial) }

    #[test]
    fn a_normal_day_is_the_change_from_the_days_before() {
        let s = spent(&[day(10, 4, 10.0, 12.0, "11/01"), day(10, 5, 12.0, 15.0, "11/01"), day(10, 6, 15.5, 16.25, "11/01")]);
        assert_eq!(credits(&s, 10, 4), (2.0, true), "the first day ever is only what was seen of it");
        assert_eq!(credits(&s, 10, 5), (3.0, false), "15 less 12, not 15 less its own first 12");
        assert_eq!(credits(&s, 10, 6), (1.25, false), "the credits between the days' last readings count toward the later day");
    }

    #[test]
    fn a_monthly_reset_counts_the_reading_itself() {
        // The counter went down.
        let s = spent(&[day(9, 30, 44.0, 47.0, "10/01"), day(10, 1, 2.0, 3.5, "11/01")]);
        assert_eq!(credits(&s, 10, 1), (3.5, false));
        // Burned through the whole month and more in a day: the counter is higher than before, but the reset date moved.
        let s = spent(&[day(9, 30, 44.0, 47.0, "10/01"), day(10, 1, 50.0, 50.0, "11/01")]);
        assert_eq!(credits(&s, 10, 1), (50.0, false));
        // A reset date that isn't printed proves nothing.
        let mut d = day(10, 2, 50.0, 52.0, "11/01");
        d.reset = None;
        assert_eq!(credits(&spent(&[day(10, 1, 40.0, 50.0, "11/01"), d]), 10, 2), (2.0, false));
    }

    #[test]
    fn a_gap_is_only_what_the_day_itself_shows_and_partial() {
        let s = spent(&[day(10, 1, 10.0, 12.0, "11/01"), day(10, 4, 20.0, 23.0, "11/01")]);
        assert_eq!(credits(&s, 10, 4), (3.0, true));
        // Not negative, whatever the file holds (a reset inside a gap).
        let s = spent(&[day(10, 1, 10.0, 12.0, "11/01"), day(10, 4, 20.0, 5.0, "11/01")]);
        assert_eq!(credits(&s, 10, 4), (0.0, true));
        assert_eq!(credits(&spent(&[day(10, 1, 10.0, 12.0, "11/01"), day(10, 2, 12.0, 11.0, "11/01")]), 10, 2), (11.0, false), "a reset: the new counter's reading");
        assert!(spent(&[]).is_empty());
    }

    #[test]
    fn several_readings_in_a_day_keep_the_first_and_the_latest() {
        let mut days = vec![];
        add(&mut days, &usage(21.0, "10/01"), at(10, 6, 9));
        add(&mut days, &usage(22.5, "10/01"), at(10, 6, 12));
        add(&mut days, &usage(24.25, "10/01"), at(10, 6, 18));
        assert_eq!(days.len(), 1);
        assert_eq!((days[0].first, days[0].used), (21.0, 24.25));
        assert!(days[0].at.starts_with("2026-10-06T18:00:00"), "{}", days[0].at);
        add(&mut days, &usage(26.0, "10/01"), at(10, 7, 8));
        assert_eq!(credits(&spent(&days), 10, 7), (1.75, false), "yesterday's last reading, not its first");
        // Reset within the day: what the counter held before it isn't today's.
        add(&mut days, &usage(1.5, "11/01"), at(10, 7, 20));
        assert_eq!((days[1].first, days[1].used), (0.0, 1.5));
        assert_eq!(credits(&spent(&days), 10, 7), (1.5, false));
    }

    #[test]
    fn at_most_400_days_are_kept() {
        let mut days = vec![];
        for i in 0..410i64 { add(&mut days, &usage(1.0, "10/01"), Local.with_ymd_and_hms(2025, 1, 1, 12, 0, 0).unwrap() + chrono::Duration::days(i)); }
        assert_eq!(days.len(), 400);
        assert_eq!(days[0].date, NaiveDate::from_ymd_opt(2025, 1, 11).unwrap());
    }

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-daily-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    #[test]
    fn the_file_is_plain_json_that_reads_back_and_a_bad_one_is_set_aside() {
        let d = dir("file");
        let f = d.join("kiro-usage.json");
        assert!(load(&f).is_empty(), "no file, no days");
        record(&f, &usage(21.0, "10/01"), at(10, 6, 9));
        record(&f, &usage(23.0, "10/01"), at(10, 6, 12));
        let text = std::fs::read_to_string(&f).unwrap();
        assert!(text.starts_with(r#"{"Version":1,"Days":[{"Date":"2026-10-06","First":21,"Used":23,"Limit":50,"Reset":"10/01","Plan":"KIRO PRO","At":"2026-10-06T12:00:00"#), "{text}");
        assert!(!d.join("kiro-usage.json.tmp").exists());
        let days = load(&f);
        assert_eq!((days.len(), days[0].first, days[0].used), (1, 21.0, 23.0));
        // Fields a later version adds, a row with no date and a reading with no plan are all fine.
        std::fs::write(&f, r#"{"Version":1,"Extra":true,"Days":[{"Date":"2026-10-05","First":1.5,"Used":2.5,"Limit":50,"Reset":null,"Plan":null,"At":"x","More":1},{"First":1}]}"#).unwrap();
        let days = load(&f);
        assert_eq!(days.len(), 1);
        assert_eq!((days[0].date, days[0].reset.clone(), days[0].plan.clone()), (date(10, 5), None, None));
        // Not ours: kept beside it, and a new file is started.
        std::fs::write(&f, "torn").unwrap();
        record(&f, &usage(1.0, "10/01"), at(10, 6, 9));
        assert_eq!(load(&f).len(), 1);
        assert!(std::fs::read_dir(&d).unwrap().flatten().any(|e| e.file_name().to_string_lossy().ends_with(".bad")));
        let _ = std::fs::remove_dir_all(&d);
    }
}

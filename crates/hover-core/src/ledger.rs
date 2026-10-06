//! What Hover's own Kiro turns cost each local day, from the saved history: the "A" of
//! the Settings → Kiro credits ("B minus A", B being everything the Kiro account spent).
//! A turn belongs to the day it ended on, else the day it started on. Only turns that
//! report credits count: Kiro Web turns don't, and show as outside Hover.

use crate::history::{AgentHistory, SavedSession};
use crate::model::AgentTool;
use chrono::NaiveDate;
use std::collections::BTreeMap;

/// A session's credits on one day.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionCredits { pub key: String, pub title: String, pub folder: String, pub credits: f64 }

/// A day's Hover-run Kiro credits: the turns' added up, how many turns, and by session
/// (the dearest first).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DayA { pub credits: f64, pub turns: u32, pub sessions: Vec<SessionCredits> }

/// The days from `from` to `to` (inclusive, local) with Kiro turns in them. The index
/// says which sessions can matter (a Kiro session with credits that was updated on or
/// after `from`), so only those are opened: it decrypts files, so not for the UI thread.
pub fn kiro_daily(h: &AgentHistory, from: NaiveDate, to: NaiveDate) -> BTreeMap<NaiveDate, DayA> {
    let mut out = BTreeMap::new();
    // Newest first, so the first one too old to matter ends it.
    for e in h.entries().into_iter().take_while(|e| e.updated.local_date().is_none_or(|d| d >= from)) {
        if e.tool != AgentTool::Kiro || e.credits.is_none() { continue; }
        if let Some(s) = h.load(&e.key) { add_session(&mut out, &s, from, to); }
    }
    for d in out.values_mut() { d.sessions.sort_by(|a, b| b.credits.total_cmp(&a.credits)); }
    out
}

/// One session's turns into the days, those inside `from..=to`.
pub fn add_session(out: &mut BTreeMap<NaiveDate, DayA>, s: &SavedSession, from: NaiveDate, to: NaiveDate) {
    if s.tool != AgentTool::Kiro { return; }
    for t in &s.turns {
        let Some(c) = t.credits else { continue };
        let Some(d) = t.ended_at.unwrap_or(t.started_at).local_date().filter(|d| (from..=to).contains(d)) else { continue };
        let day = out.entry(d).or_default();
        day.credits += c;
        day.turns += 1;
        match day.sessions.iter_mut().find(|x| x.key == s.key) {
            Some(x) => x.credits += c,
            None => day.sessions.push(SessionCredits { key: s.key.clone(), title: s.title.clone(), folder: s.folder.clone(), credits: c }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::Crypto;
    use crate::history::SavedTurn;
    use crate::time::Stamp;
    use std::sync::Arc;

    fn day(m: u32, d: u32) -> NaiveDate { NaiveDate::from_ymd_opt(2026, m, d).unwrap() }

    /// Stamps with no zone are the wall clock as written, so these dates hold in any time zone.
    fn at(s: &str) -> Stamp { Stamp::parse(s).unwrap() }

    fn turn(start: &str, end: Option<&str>, credits: Option<f64>) -> SavedTurn {
        SavedTurn { prompt: "p".into(), images: vec![], steps: vec![], state: None, text: None, started_at: at(start), woke_at: None, ended_at: end.map(at), credits,
            before: None, after: None, ext: Default::default() }
    }

    fn session(key: &str, tool: AgentTool, turns: Vec<SavedTurn>) -> SavedSession {
        SavedSession { key: key.into(), tool, folder: format!("C:\\work\\{key}"), title: format!("Task {key}"), acp_id: None, context: None, turns, updated: at("2026-10-06T12:00:00"),
            access: None, cloud: None, ext: Default::default() }
    }

    fn of(sessions: &[SavedSession]) -> BTreeMap<NaiveDate, DayA> {
        let mut out = BTreeMap::new();
        for s in sessions { add_session(&mut out, s, day(10, 1), day(10, 31)); }
        out
    }

    #[test]
    fn only_kiro_turns_that_report_credits_count() {
        let out = of(&[
            session("k", AgentTool::Kiro, vec![turn("2026-10-06T09:00:00", Some("2026-10-06T09:05:00"), Some(0.25)), turn("2026-10-06T10:00:00", Some("2026-10-06T10:05:00"), None),
                turn("2026-10-06T11:00:00", Some("2026-10-06T11:05:00"), Some(1.5))]),
            session("c", AgentTool::Codex, vec![turn("2026-10-06T09:00:00", Some("2026-10-06T09:05:00"), Some(9.0))]),
        ]);
        assert_eq!(out.len(), 1);
        let d = &out[&day(10, 6)];
        assert_eq!((d.credits, d.turns), (1.75, 2));
        assert_eq!(d.sessions, [SessionCredits { key: "k".into(), title: "Task k".into(), folder: "C:\\work\\k".into(), credits: 1.75 }]);
    }

    #[test]
    fn a_turn_belongs_to_the_day_it_ended_or_else_the_day_it_started() {
        let out = of(&[session("k", AgentTool::Kiro, vec![
            turn("2026-10-05T23:50:00", Some("2026-10-06T00:10:00"), Some(0.5)),
            turn("2026-10-06T08:00:00", None, Some(0.25)),
        ])]);
        assert_eq!(out.keys().collect::<Vec<_>>(), [&day(10, 6)], "the first ended after midnight; the second never ended");
        assert_eq!(out[&day(10, 6)].credits, 0.75);
    }

    #[test]
    fn a_session_over_several_days_is_in_each_with_its_share() {
        let s = session("k", AgentTool::Kiro, vec![
            turn("2026-10-04T09:00:00", Some("2026-10-04T09:05:00"), Some(1.0)), turn("2026-10-04T10:00:00", Some("2026-10-04T10:05:00"), Some(0.5)),
            turn("2026-10-06T09:00:00", Some("2026-10-06T09:05:00"), Some(2.0)),
        ]);
        let out = of(&[s.clone(), session("j", AgentTool::Kiro, vec![turn("2026-10-06T09:00:00", Some("2026-10-06T09:05:00"), Some(3.0))])]);
        assert_eq!((out[&day(10, 4)].credits, out[&day(10, 4)].turns, out[&day(10, 4)].sessions.len()), (1.5, 2, 1));
        assert_eq!(out[&day(10, 6)].sessions.iter().map(|x| (x.key.as_str(), x.credits)).collect::<Vec<_>>(), [("k", 2.0), ("j", 3.0)]);
        // Days outside the range are left out.
        let mut narrow = BTreeMap::new();
        add_session(&mut narrow, &s, day(10, 5), day(10, 6));
        assert_eq!(narrow.keys().collect::<Vec<_>>(), [&day(10, 6)]);
    }

    /// An instant (a Local or Utc stamp) is on the day the machine's zone puts it.
    #[test]
    fn an_instant_falls_on_the_day_of_the_machines_zone() {
        use chrono::TimeZone;
        for ms in [1_790_000_000_000i64, 1_790_040_000_000, 1_790_080_000_000] {
            let want = chrono::Local.timestamp_millis_opt(ms).unwrap().date_naive();
            assert_eq!(Stamp::from_unix_ms(ms, crate::time::Kind::Utc).local_date(), Some(want));
            assert_eq!(Stamp::from_unix_ms(ms, crate::time::Kind::Local).local_date(), Some(want));
        }
    }

    /// Through a real history: the index decides which sessions are opened, and the
    /// days come out dearest session first.
    #[test]
    fn the_history_gives_the_days() {
        let d = std::env::temp_dir().join(format!("hover-ledger-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let h = AgentHistory::new(d.clone(), Arc::new(Crypto::with_key([5; 32])));
        let t = |c| turn("2026-10-06T09:00:00", Some("2026-10-06T09:05:00"), c);
        h.save(&session("cheap", AgentTool::Kiro, vec![t(Some(0.5))]));
        h.save(&session("dear", AgentTool::Kiro, vec![t(Some(2.5))]));
        h.save(&session("none", AgentTool::Kiro, vec![t(None)]));
        h.save(&session("codex", AgentTool::Codex, vec![t(Some(7.0))]));
        let mut old = session("old", AgentTool::Kiro, vec![t(Some(4.0))]);
        old.updated = at("2026-08-01T12:00:00");
        h.save(&old);
        let out = kiro_daily(&h, day(10, 1), day(10, 31));
        assert_eq!(out.len(), 1);
        assert_eq!(out[&day(10, 6)].sessions.iter().map(|x| x.key.as_str()).collect::<Vec<_>>(), ["dear", "cheap"], "old was not opened, codex and none have nothing");
        assert_eq!(out[&day(10, 6)].credits, 3.0);
        let _ = std::fs::remove_dir_all(&d);
    }
}

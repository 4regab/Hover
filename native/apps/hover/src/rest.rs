//! What the resting notch shows (NotchHost.UpdateRest, QuotaSeg): the island's one
//! agent segment (a question waiting, the agents at work, or an end nobody has seen),
//! then a divider and the quotas switched on; or the question's card.

use hover_agents::ask::AgentAsk;
use hover_agents::session::KiroSession;
use hover_agents::words;
use hover_core::model::{AgentTool, KiroState};
use hover_quota::{item, Reading};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { None, Pill, Card }

/// One quota's segment: its ring (None: an empty track), its number ("38", or "—") and
/// whether a % follows, and whether it is the dim one of a failed read.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaSeg { pub id: String, pub name: &'static str, pub ring: Option<f64>, pub value: String, pub pct: bool, pub dim: bool }

/// The agent segment.
#[derive(Clone, Debug, PartialEq)]
pub enum Seg {
    None,
    /// A question: the session, the ask in front, and how many wait in all.
    Ask { session: i32, tool: AgentTool, ask: AgentAsk, total: usize },
    /// At work: the tools in order, the one speaking, what it is doing, for how long.
    Work { tools: Vec<AgentTool>, active: usize, verb: &'static str, obj: String, secs: f64, name: &'static str, more: usize },
    /// An end nobody has seen: its tool, how it went, the task, how long it took, and
    /// how many ended unseen.
    Done { tool: AgentTool, state: KiroState, title: String, took_secs: f64, count: usize },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Island {
    pub kind: Kind,
    pub seg: Seg,
    pub divider: bool,
    pub quotas: Vec<QuotaSeg>,
    /// The items, joined: when it changes (or the kind does) the content cross-fades.
    pub key: String,
}

/// A quota's segment from its reading: "—" until one arrives, dim when it failed.
pub fn quota_seg(id: &str, reading: Option<&Reading>) -> QuotaSeg {
    let used = reading.and_then(|r| r.used);
    QuotaSeg {
        id: id.to_owned(),
        name: item::short(id),
        ring: used,
        value: used.map_or("—".into(), |u| hover_quota::num::custom(u, 0)),
        pct: used.is_some(),
        dim: reading.is_some_and(|r| !r.ok()),
    }
}

/// What ended unseen, the latest (OwlApp.KiroUnseenLast) and how many.
#[derive(Clone, Debug, PartialEq)]
pub struct Unseen { pub count: usize, pub tool: AgentTool, pub state: KiroState, pub title: String, pub took_secs: f64 }

/// UpdateRest: a question waiting wins, else the agents at work, else an end nobody has
/// seen; then a divider and the quotas. `speaker` picks who speaks among those at work
/// (it moves on every 3 s); `now` is the clock for their timers; `card` the card is open.
pub fn island(on: &[&str], readings: &dyn Fn(&str) -> Option<Reading>, sessions: &[KiroSession], unseen: Option<Unseen>,
    speaker: usize, now: hover_core::time::Stamp, card: bool) -> Island {
    let waiting: Vec<&KiroSession> = sessions.iter().filter(|s| s.waiting()).collect();
    let working: Vec<&KiroSession> = sessions.iter().filter(|s| s.busy() && !s.waiting()).collect();
    let quotas: Vec<QuotaSeg> = item::QUOTAS.iter().filter(|id| on.contains(id)).map(|id| quota_seg(id, readings(id).as_ref())).collect();
    let (seg, name) = if let Some(s) = waiting.first() {
        (Seg::Ask { session: s.id, tool: s.tool, ask: s.asking().unwrap().clone(), total: waiting.iter().map(|s| s.asks.len()).sum() }, "ask")
    } else if !working.is_empty() {
        let sp = working[speaker % working.len()];
        let (verb, obj) = words::activity(sp);
        let secs = sp.current().map_or(0.0, |t| t.ended_at.unwrap_or(now).secs_since(&t.started_at));
        (Seg::Work { tools: working.iter().map(|s| s.tool).collect(), active: speaker % working.len(), verb, obj, secs, name: sp.tool.name(), more: working.len() - 1 }, "work")
    } else if let Some(u) = unseen.filter(|u| u.count > 0) {
        (Seg::Done { tool: u.tool, state: u.state, title: u.title, took_secs: u.took_secs, count: u.count }, "done")
    } else { (Seg::None, "") };
    let mut items: Vec<&str> = vec![];
    if !name.is_empty() { items.push(name); }
    let divider = !items.is_empty() && !quotas.is_empty();
    if divider { items.push("|"); }
    items.extend(quotas.iter().map(|q| q.id.as_str()));
    let kind = if card && matches!(seg, Seg::Ask { .. }) { Kind::Card } else if !items.is_empty() { Kind::Pill } else { Kind::None };
    Island { kind, key: items.join(","), seg, divider, quotas }
}

/// NotchHost.Clock: h:mm:ss past an hour, else m:ss.
pub fn clock(secs: f64) -> String {
    let n = secs.max(0.0).floor() as i64;
    if n >= 3600 { format!("{}:{:02}:{:02}", n / 3600, n / 60 % 60, n % 60) } else { format!("{}:{:02}", n / 60, n % 60) }
}

/// NotchHost.Took: "45 s", "3m 07s", "1h 05m".
pub fn took(secs: f64) -> String {
    if secs < 60.0 { format!("{} s", (secs.round() as i64).max(1)) }
    else if secs < 3600.0 { format!("{}m {:02}s", (secs / 60.0).floor() as i64, (secs.round() as i64) % 60) }
    else { format!("{}h {:02}m", (secs / 3600.0).floor() as i64, (secs / 60.0).floor() as i64 % 60) }
}

/// Ui.Level: a quota's ring goes green, amber, red as it fills. The palette's colour
/// is picked by the caller.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level { Green, Orange, Red }

pub fn level(used: f64) -> Level { if used < 70.0 { Level::Green } else if used < 90.0 { Level::Orange } else { Level::Red } }

/// A menu: items with their tick (None: not a checkbox), None for a separator.
pub type Menu = Vec<Option<(String, Option<bool>)>>;

/// The tray icon's menu (Actions.BuildMainMenu), top to bottom.
pub fn tray_menu(shortcut: &str, launch_at_login: bool) -> Menu {
    vec![
        Some((format!("Open Agent Office  {shortcut}"), None)),
        Some(("Open App Window".into(), None)),
        None,
        Some(("Launch at Login".into(), Some(launch_at_login))),
        None,
        Some(("Settings…".into(), None)),
        Some(("Quit Hover".into(), None)),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::time::Stamp;

    fn ok(v: f64) -> Reading { Reading { used: Some(v), detail: String::new() } }

    fn running(tool: AgentTool) -> KiroSession {
        let mut s = KiroSession::new(tool);
        s.state = KiroState::Running;
        s.phase = hover_agents::stream::KiroPhase::Thinking;
        let mut t = hover_agents::session::KiroTurn::new("Tidy the imports", vec![]);
        t.started_at = Stamp::parse("2026-09-28T10:00:00Z").unwrap();
        s.turns.push(t);
        s
    }

    /// NotchHost.UpdateRest: one agent segment first, a divider, then the quotas.
    #[test]
    fn the_island_follows_what_is_on_and_what_happened() {
        let none = |_: &str| None;
        let now = Stamp::parse("2026-09-28T10:01:05Z").unwrap();
        assert_eq!(island(&[], &none, &[], None, 0, now, false).kind, Kind::None);
        let q = island(&["codex", "claude"], &|id: &str| (id == "claude").then(|| ok(37.5)), &[], None, 0, now, false);
        assert_eq!((q.kind, q.key.as_str(), q.divider), (Kind::Pill, "claude,codex", false));
        assert_eq!(q.quotas[0], QuotaSeg { id: "claude".into(), name: "Claude", ring: Some(37.5), value: "38".into(), pct: true, dim: false });
        assert_eq!((q.quotas[1].value.as_str(), q.quotas[1].pct), ("—", false));
        let s = [running(AgentTool::Kiro), running(AgentTool::Codex)];
        let w = island(&["claude"], &none, &s, None, 1, now, false);
        assert_eq!(w.key, "work,|,claude");
        match &w.seg { Seg::Work { tools, active, verb, secs, more, .. } => assert_eq!((tools.len(), *active, *verb, *secs, *more), (2, 1, "Thinking", 65.0, 1)), s => panic!("{s:?}") }
        // A question takes the segment over, and the card only when asked for.
        let mut a = s.clone();
        a[0].asks.push(AgentAsk { id: "q".into(), kind: "execute".into(), title: "Run".into(), command: Some("npm i".into()), path: None, preview: None,
            added: 0, removed: 0, reason: "r".into(), danger: false });
        assert_eq!(island(&[], &none, &a, None, 0, now, false).key, "ask");
        assert_eq!(island(&[], &none, &a, None, 0, now, true).kind, Kind::Card);
        assert_eq!(island(&[], &none, &s, None, 0, now, true).kind, Kind::Pill, "no question, no card");
        let u = Unseen { count: 2, tool: AgentTool::Cursor, state: KiroState::Failed, title: "x".into(), took_secs: 3.0 };
        assert_eq!(island(&[], &none, &[], Some(u), 0, now, false).key, "done");
        assert!(quota_seg("kiro", Some(&Reading::fail("Sign in first."))).dim);
    }

    #[test]
    fn clocks_took_and_levels() {
        assert_eq!([clock(5.0), clock(65.9), clock(3725.0)], ["0:05", "1:05", "1:02:05"]);
        assert_eq!([took(0.2), took(44.6), took(187.0), took(3900.0)], ["1 s", "45 s", "3m 07s", "1h 05m"]);
        assert_eq!([level(69.9), level(70.0), level(89.9), level(90.0)], [Level::Green, Level::Orange, Level::Orange, Level::Red]);
        let m = tray_menu("Alt+N", true);
        assert_eq!(m[0].as_ref().unwrap().0, "Open Agent Office  Alt+N");
        assert_eq!(m[3], Some(("Launch at Login".into(), Some(true))));
        assert_eq!(m.len(), 7);
    }
}

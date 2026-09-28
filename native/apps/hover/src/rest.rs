//! What the resting notch shows (NotchHost.PillItems, UpdateRest, QuotaSeg): the
//! quotas switched on, the newest task at work, the ends nobody has seen, or an alert.

use hover_quota::{item, Reading};

pub const KIRO_RUN: &str = "kiro-run";
pub const KIRO_DONE: &str = "kiro-done";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind { None, Pill, Alert }

/// One quota's segment: its ring (None: an empty track), its value and whether the
/// value is the dim one of a failed read.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaSeg { pub id: String, pub name: &'static str, pub ring: Option<f64>, pub value: String, pub dim: bool }

#[derive(Clone, Debug, PartialEq)]
pub struct Rest {
    pub kind: Kind,
    pub items: Vec<String>,
    pub quotas: Vec<QuotaSeg>,
    pub working: Option<String>,
    pub done: Option<String>,
    /// The number of unseen ends: when it grows, the done bot hops again (Cheer).
    pub done_count: usize,
    pub alert: Option<(String, String)>,
}

/// The pill's items in order: the quotas switched on (in Settings' order), then Kiro
/// at work, then the ends not yet seen.
pub fn items(on: &[&str], running: usize, unseen: usize) -> Vec<String> {
    let mut v: Vec<String> = on.iter().filter(|id| item::QUOTAS.contains(id)).map(|s| s.to_string()).collect();
    if running > 0 { v.push(KIRO_RUN.into()); }
    if unseen > 0 { v.push(KIRO_DONE.into()); }
    v
}

/// "3 Codex tasks ended", "3 tasks ended" (different tools), "Kiro ended", "A task ended".
pub fn done_text(n: usize, who: Option<&str>) -> String {
    if n > 1 {
        match who { None => format!("{n} tasks ended"), Some(w) => format!("{n} {w} tasks ended") }
    } else {
        format!("{} ended", who.unwrap_or("A task"))
    }
}

/// A quota's segment from its reading: "—" until one arrives, dim when it failed.
pub fn quota_seg(id: &str, reading: Option<&Reading>) -> QuotaSeg {
    QuotaSeg {
        id: id.to_owned(),
        name: item::short(id),
        ring: reading.and_then(|r| r.used),
        value: reading.and_then(|r| r.used).map_or("—".into(), |u| format!("{}%", hover_quota::num::custom(u, 0))),
        dim: reading.is_some_and(|r| !r.ok()),
    }
}

/// UpdateRest: an alert wins; otherwise the pill when it has anything, else nothing.
pub fn rest(on: &[&str], readings: &dyn Fn(&str) -> Option<Reading>, working: Option<String>, unseen: (usize, Option<&str>), alert: Option<(String, String)>) -> Rest {
    let items = if alert.is_some() { vec![] } else { items(on, usize::from(working.is_some()), unseen.0) };
    let kind = if alert.is_some() { Kind::Alert } else if !items.is_empty() { Kind::Pill } else { Kind::None };
    let quotas = items.iter().filter(|i| item::QUOTAS.contains(&i.as_str())).map(|id| quota_seg(id, readings(id).as_ref())).collect();
    let has = |k: &str| items.iter().any(|i| i == k);
    Rest {
        kind,
        quotas,
        working: if has(KIRO_RUN) { working } else { None },
        done: has(KIRO_DONE).then(|| done_text(unseen.0, unseen.1)),
        done_count: unseen.0,
        alert,
        items,
    }
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

    fn ok(v: f64) -> Reading { Reading { used: Some(v), detail: String::new() } }

    /// NotchHost.UpdateRest and PillItems, with its strings.
    #[test]
    fn the_rest_follows_what_is_on_and_what_happened() {
        let none = |_: &str| None;
        assert_eq!(rest(&[], &none, None, (0, None), None).kind, Kind::None);
        let r = rest(&["claude", "codex"], &|id: &str| (id == "claude").then(|| ok(37.5)), Some("Kiro · Reading the code · 2".into()), (3, Some("Codex")), None);
        assert_eq!(r.kind, Kind::Pill);
        assert_eq!(r.items, ["claude", "codex", KIRO_RUN, KIRO_DONE]);
        assert_eq!(r.quotas[0], QuotaSeg { id: "claude".into(), name: "Claude", ring: Some(37.5), value: "38%".into(), dim: false });
        assert_eq!(r.quotas[1].value, "—");
        assert!(!r.quotas[1].dim);
        assert_eq!(r.working.as_deref(), Some("Kiro · Reading the code · 2"));
        assert_eq!(r.done.as_deref(), Some("3 Codex tasks ended"));
        let failed = Reading::fail("Sign in first.");
        assert!(quota_seg("kiro", Some(&failed)).dim);
        assert_eq!(quota_seg("kiro", Some(&failed)).value, "—");
        // An alert takes the notch over.
        let a = rest(&["claude"], &none, None, (1, None), Some(("Kiro is done: x".into(), "y".into())));
        assert_eq!((a.kind, a.items.len()), (Kind::Alert, 0));
    }

    #[test]
    fn done_texts_and_levels() {
        assert_eq!(done_text(1, Some("Kiro")), "Kiro ended");
        assert_eq!(done_text(1, None), "A task ended");
        assert_eq!(done_text(2, None), "2 tasks ended");
        assert_eq!([level(69.9), level(70.0), level(89.9), level(90.0)], [Level::Green, Level::Orange, Level::Orange, Level::Red]);
        let m = tray_menu("Alt+N", true);
        assert_eq!(m[0].as_ref().unwrap().0, "Open Agent Office  Alt+N");
        assert_eq!(m[3], Some(("Launch at Login".into(), Some(true))));
        assert_eq!(m.len(), 7);
    }
}

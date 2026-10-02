//! MenuBar.swift's menu, as data: what the status item's menu holds on a Mac, top to
//! bottom. The usage readings and their details, a "Show in menu bar" submenu to switch
//! each reader on or off, the agents at work, Open Office, Office in a Window, Start a
//! Voice Task, Open on Hover, Launch at Login, Settings, Refresh, Quit. `status.rs` turns
//! it into an NSMenu; main.rs answers the `Act` a click names. Compiled everywhere so the
//! words and the order are tested on every OS.

use super::keycodes::{NS_COMMAND, NS_CONTROL, NS_OPTION};

/// What a click on an item asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Act {
    /// Open or close the office in the notch (Option-N).
    Office,
    /// The dashboard window.
    Window,
    /// A voice task, as the shortcut's tap starts one.
    Voice,
    /// Whether hovering the notch opens the office.
    HoverOpens,
    Login,
    Settings,
    /// Read the tools and their usage again.
    Refresh,
    Quit,
    /// Switch a usage reader on or off in the menu bar (the tool's id).
    ToggleQuota(String),
    /// Open that session's chat in the office.
    Session(i32),
    /// A row of the usage list: read again.
    Reread,
}

/// The picture at the left of an item.
#[derive(Clone, Debug, PartialEq)]
pub enum Icon {
    /// The tool's ring with its mark in it (the share used, when it is known).
    Ring(String, Option<f64>),
    /// The tool's mark on its tile.
    Tile(String),
    /// A system symbol by name (the voice item's waveform).
    Symbol(&'static str),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Item {
    pub title: String,
    /// A second, smaller line (a usage reading's details).
    pub detail: Option<String>,
    /// None is not a checkbox.
    pub check: Option<bool>,
    pub act: Option<Act>,
    pub enabled: bool,
    /// The key equivalent and its modifiers (NSEvent's bits): what the menu shows at the right.
    pub key: Option<(&'static str, usize)>,
    pub icon: Option<Icon>,
    pub tip: Option<String>,
    pub sub: Vec<Entry>,
}

impl Item {
    fn new(title: impl Into<String>, act: Act) -> Item {
        Item { title: title.into(), detail: None, check: None, act: Some(act), enabled: true, key: None, icon: None, tip: None, sub: vec![] }
    }
    /// A line that only reads: no action, greyed.
    fn note(title: impl Into<String>) -> Item {
        Item { title: title.into(), detail: None, check: None, act: None, enabled: false, key: None, icon: None, tip: None, sub: vec![] }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Entry {
    /// A section header ("Usage", "Agents").
    Header(String),
    Sep,
    Item(Item),
}

/// One usage reader that is switched on, as the menu lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct QuotaRow {
    pub id: String,
    pub name: String,
    /// "38%", "—" (failed) or "…" (not read yet).
    pub percent: String,
    pub detail: String,
    pub used: Option<f64>,
}

/// An agent at work (or waiting on the user).
#[derive(Clone, Debug, PartialEq)]
pub struct Working {
    pub id: i32,
    pub tool: String,
    pub title: String,
    /// What it is doing, or "Needs your approval".
    pub what: String,
}

/// Everything the menu is made from.
pub struct Inputs<'a> {
    /// Every reader there is (id, name, whether it is switched on).
    pub readers: &'a [(String, String, bool)],
    pub rows: &'a [QuotaRow],
    pub working: &'a [Working],
    /// The office's shortcut for the item's key hint, e.g. "⌥N".
    pub office_keys: &'a str,
    pub voice_on: bool,
    pub hover_opens: bool,
    pub login: bool,
    /// A read is going on now.
    pub reading: bool,
}

/// The percentage beside a ring: "…" until a reading arrives, "—" when it failed or has no
/// number, else the share used, rounded.
pub fn percent(reading: Option<(bool, Option<f64>)>) -> String {
    match reading {
        None => "…".into(),
        Some((true, Some(used))) => format!("{}%", used.round() as i64),
        Some(_) => "—".into(),
    }
}

/// At most this many agents are listed.
pub const MAX_WORKING: usize = 6;

/// The menu, top to bottom.
pub fn build(i: &Inputs) -> Vec<Entry> {
    let mut m: Vec<Entry> = vec![Entry::Header("Usage".into())];
    if i.rows.is_empty() {
        m.push(Entry::Item(Item::note("Choose tools below to show their usage here")));
    }
    for q in i.rows {
        let mut row = Item::new(format!("{}  {}", q.name, q.percent), Act::Reread);
        row.detail = Some(if q.detail.is_empty() { "Reading…".into() } else { q.detail.clone() });
        row.icon = Some(Icon::Ring(q.id.clone(), q.used));
        row.tip = Some("Read again".into());
        m.push(Entry::Item(row));
    }
    let mut readers = Item::note("Show in menu bar");
    readers.enabled = true;
    for (id, name, on) in i.readers {
        let mut t = Item::new(name.clone(), Act::ToggleQuota(id.clone()));
        t.check = Some(*on);
        t.icon = Some(Icon::Tile(id.clone()));
        readers.sub.push(Entry::Item(t));
    }
    readers.sub.push(Entry::Sep);
    readers.sub.push(Entry::Item(Item::note("Readers use each tool’s own sign-in, read-only")));
    m.push(Entry::Item(readers));

    m.push(Entry::Sep);
    m.push(Entry::Header("Agents".into()));
    if i.working.is_empty() { m.push(Entry::Item(Item::note("The office is quiet"))); }
    for w in i.working.iter().take(MAX_WORKING) {
        let title = if w.title.is_empty() { w.tool.clone() } else { w.title.clone() };
        let mut row = Item::new(format!("{title} — {}", w.what), Act::Session(w.id));
        row.icon = Some(Icon::Tile(w.tool.clone()));
        m.push(Entry::Item(row));
    }
    let mut office = Item::new("Open Office", Act::Office);
    office.tip = Some(format!("Open or close the office ({})", i.office_keys));
    office.key = Some(("n", NS_OPTION));
    m.push(Entry::Item(office));
    m.push(Entry::Item(Item::new("Office in a Window", Act::Window)));
    let mut voice = Item::new("Start a Voice Task", Act::Voice);
    voice.key = Some((" ", NS_CONTROL | NS_OPTION));
    voice.enabled = i.voice_on;
    voice.icon = Some(Icon::Symbol("waveform"));
    voice.tip = Some("Hold ⌃⌥Space and speak; let go to see the task. A tap listens hands-free.".into());
    m.push(Entry::Item(voice));

    m.push(Entry::Sep);
    let mut hover = Item::new("Open on Hover", Act::HoverOpens);
    hover.check = Some(i.hover_opens);
    m.push(Entry::Item(hover));
    let mut login = Item::new("Launch at Login", Act::Login);
    login.check = Some(i.login);
    m.push(Entry::Item(login));
    let mut settings = Item::new("Settings…", Act::Settings);
    settings.key = Some((",", NS_COMMAND));
    m.push(Entry::Item(settings));
    let mut refresh = Item::new(if i.reading { "Reading usage…" } else { "Refresh Tools and Usage" }, Act::Refresh);
    refresh.key = Some(("r", NS_COMMAND));
    refresh.enabled = !i.reading;
    m.push(Entry::Item(refresh));
    m.push(Entry::Sep);
    let mut quit = Item::new("Quit Hover", Act::Quit);
    quit.key = Some(("q", NS_COMMAND));
    m.push(Entry::Item(quit));
    m
}

#[cfg(test)]
mod tests {
    use super::*;

    fn inputs<'a>(readers: &'a [(String, String, bool)], rows: &'a [QuotaRow], working: &'a [Working]) -> Inputs<'a> {
        Inputs { readers, rows, working, office_keys: "⌥N", voice_on: true, hover_opens: true, login: false, reading: false }
    }

    fn titles(m: &[Entry]) -> Vec<String> {
        m.iter().map(|e| match e { Entry::Header(h) => format!("# {h}"), Entry::Sep => "-".into(), Entry::Item(i) => i.title.clone() }).collect()
    }

    #[test]
    fn the_menu_runs_in_the_order_the_mac_app_had() {
        let readers = vec![("codex".to_owned(), "Codex".to_owned(), true), ("kiro".to_owned(), "Kiro".to_owned(), false)];
        let rows = vec![QuotaRow { id: "codex".into(), name: "Codex".into(), percent: "38%".into(), detail: "5h window: 38% used".into(), used: Some(37.6) }];
        let working = vec![Working { id: 4, tool: "kiro".into(), title: "Tidy the imports".into(), what: "Needs your approval".into() }];
        let m = build(&inputs(&readers, &rows, &working));
        assert_eq!(titles(&m), ["# Usage", "Codex  38%", "Show in menu bar", "-", "# Agents", "Tidy the imports — Needs your approval", "Open Office",
            "Office in a Window", "Start a Voice Task", "-", "Open on Hover", "Launch at Login", "Settings…", "Refresh Tools and Usage", "-", "Quit Hover"]);
        let Entry::Item(row) = &m[1] else { panic!() };
        assert_eq!((row.act.clone(), row.detail.as_deref(), row.icon.clone()), (Some(Act::Reread), Some("5h window: 38% used"), Some(Icon::Ring("codex".into(), Some(37.6)))));
        // The reader switches: one per tool, ticked when on, then the note.
        let Entry::Item(sub) = &m[2] else { panic!() };
        assert_eq!(titles(&sub.sub), ["Codex", "Kiro", "-", "Readers use each tool’s own sign-in, read-only"]);
        let Entry::Item(kiro) = &sub.sub[1] else { panic!() };
        assert_eq!((kiro.check, kiro.act.clone()), (Some(false), Some(Act::ToggleQuota("kiro".into()))));
    }

    #[test]
    fn an_empty_office_and_no_readers_say_so() {
        let m = build(&inputs(&[], &[], &[]));
        let t = titles(&m);
        assert!(t.contains(&"Choose tools below to show their usage here".to_owned()));
        assert!(t.contains(&"The office is quiet".to_owned()));
        let Entry::Item(hint) = &m[1] else { panic!() };
        assert!(!hint.enabled && hint.act.is_none());
    }

    #[test]
    fn voice_refresh_and_the_ticks_follow_the_state() {
        let mut i = inputs(&[], &[], &[]);
        i.voice_on = false; i.reading = true; i.login = true; i.hover_opens = false;
        let m = build(&i);
        let item = |title: &str| m.iter().find_map(|e| match e { Entry::Item(i) if i.title == title => Some(i.clone()), _ => None }).unwrap();
        assert!(!item("Start a Voice Task").enabled);
        let r = item("Reading usage…");
        assert!(!r.enabled && r.act == Some(Act::Refresh));
        assert_eq!(item("Launch at Login").check, Some(true));
        assert_eq!(item("Open on Hover").check, Some(false));
        assert_eq!(item("Open Office").key, Some(("n", NS_OPTION)));
        assert_eq!(item("Quit Hover").key, Some(("q", NS_COMMAND)));
    }

    #[test]
    fn only_six_agents_are_listed() {
        let working: Vec<Working> = (0..9).map(|n| Working { id: n, tool: "codex".into(), title: String::new(), what: "Working".into() }).collect();
        let m = build(&inputs(&[], &[], &working));
        let listed = m.iter().filter(|e| matches!(e, Entry::Item(i) if matches!(i.act, Some(Act::Session(_))))).count();
        assert_eq!(listed, MAX_WORKING);
        // An untitled session is named by its tool.
        assert!(titles(&m).contains(&"codex — Working".to_owned()));
    }

    #[test]
    fn the_percentage_reads_like_the_menu_bar_did() {
        assert_eq!(percent(None), "…");
        assert_eq!(percent(Some((false, Some(40.0)))), "—");
        assert_eq!(percent(Some((true, None))), "—");
        assert_eq!(percent(Some((true, Some(37.5)))), "38%");
        assert_eq!(percent(Some((true, Some(0.2)))), "0%");
    }
}

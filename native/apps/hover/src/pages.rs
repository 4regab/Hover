//! Owl/Pages.cs (SettingsPage): five sections, each a few headed groups of rows, a
//! label on the left and its control on the right. Built here as data, row for row
//! and string for string, with the C#'s automation ids; ui/settings.slint draws it.
//! Where Windows is named and Linux differs, the Linux words are the nearest ones.

use hover_agents::agents::{self, AgentReady};
use hover_core::model::{AcpOption, AgentOptions, AgentTool, Appearance, SavedTheme, WorkspaceSize};
use hover_core::palette::{InstalledTheme, Palette};
use hover_core::settings::Settings;
use hover_quota::{item, Reading};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section { General, Integrations, Kiro, Codex, Cursor }

impl Section {
    pub const ALL: [Section; 5] = [Section::General, Section::Integrations, Section::Kiro, Section::Codex, Section::Cursor];
    pub fn title(self) -> &'static str { ["General", "Integrations", "Kiro", "Codex", "Cursor"][self as usize] }
    /// The sidebar's icon and its tile's colour.
    pub fn glyph(self) -> (&'static str, Tint) {
        [("settings", Tint::Gray), ("plug", Tint::Purple), ("ghost", Tint::Bot), ("terminal", Tint::Green), ("sparkles", Tint::Blue)][self as usize]
    }
    pub fn tool(self) -> AgentTool { match self { Section::Codex => AgentTool::Codex, Section::Cursor => AgentTool::Cursor, _ => AgentTool::Kiro } }
}

/// The tile colours Pages.cs uses: the palette's accents, Ui.Gray, and the Kiro bot's purple.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint { Gray, Purple, Bot, Green, Blue, Orange, Teal }

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    None,
    Switch { id: String, name: String, on: bool },
    /// A grey pill (OwlLightButton) with its text.
    Button { id: String, name: String, text: String, enabled: bool },
    /// The shortcut field: shows the shortcut, records the next chord.
    Shortcut { text: String },
    Segments { id: String, labels: Vec<String>, picked: i32 },
    /// A button showing the current choice that opens a menu of them.
    Picker { id: String, name: String, shown: String, options: Vec<(String, bool)> },
    Text(String),
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lead { None, Tile(&'static str, Tint), Ring(Option<f64>) }

#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub label: String, pub sub: Option<String>, pub control: Control, pub lead: Lead, pub enabled: bool, pub sub_id: Option<String> }

fn row(label: impl Into<String>, sub: Option<String>, control: Control, lead: Lead) -> Row {
    Row { label: label.into(), sub, control, lead, enabled: true, sub_id: None }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Tile { pub id: String, pub name: String, pub from: String, pub picked: bool, pub palette: Palette }

#[derive(Clone, Debug, PartialEq)]
pub enum Block {
    Title(String),
    /// A heading; the first sits right under the title.
    Heading(String, bool),
    Group(Vec<Row>),
    Footnote(String),
    Tiles(Vec<Tile>),
    /// An accent link with its icon (Import…, Refresh…), dim or not, and a status line.
    Link { id: String, name: String, icon: &'static str, text: String, dim: bool, status: String },
}

/// What a page is built from, beyond the settings.
pub struct Input<'a> {
    pub settings: &'a Settings,
    pub launch_at_login: bool,
    /// The shortcut field's text: the shortcut, "Press keys…" or the modifier hint.
    pub shortcut: String,
    pub reading: &'a dyn Fn(&str) -> Option<Reading>,
    pub ready: &'a dyn Fn(AgentTool) -> Option<AgentReady>,
    pub installed: &'a [(InstalledTheme, SavedTheme)],
    pub system_dark: bool,
    pub import_status: String,
    pub kiro_agents: Vec<String>,
}

const WIN: bool = cfg!(windows);

pub fn build(section: Section, i: &Input) -> Vec<Block> {
    let mut b = vec![Block::Title(section.title().into())];
    match section {
        Section::General => {
            general(&mut b, i);
            heading(&mut b, "Notch");
            notch(&mut b, i);
            heading(&mut b, "Appearance");
            themes(&mut b, i);
        }
        Section::Integrations => {
            heading(&mut b, "AI quotas");
            quotas(&mut b, i);
        }
        _ => agent(&mut b, section, i),
    }
    b
}

fn heading(b: &mut Vec<Block>, text: &str) {
    // The first heading sits right under the title; later ones start a new group.
    let first = b.len() <= 1;
    b.push(Block::Heading(text.to_uppercase(), first));
}

fn switch(id: &str, name: &str, on: bool) -> Control { Control::Switch { id: id.into(), name: name.into(), on } }

fn segments(id: &str, labels: &[&str], picked: i32) -> Control {
    Control::Segments { id: id.into(), labels: labels.iter().map(|s| s.to_string()).collect(), picked }
}

fn general(b: &mut Vec<Block>, i: &Input) {
    let s = i.settings;
    b.push(Block::Group(vec![
        row("Launch at login", Some(if WIN { "Hover starts with Windows and waits at the top of the screen." } else { "Hover starts when you log in and waits at the top of the screen." }.into()),
            switch("LaunchAtLogin", "Launch at login", i.launch_at_login), Lead::None),
        row("Open on hover", Some("Off, only the shortcut or a click on the notch opens it — handy if browser tabs live up there.".into()),
            switch("HoverOpens", "Open on hover", s.hover_opens_workspace()), Lead::None),
        row("Notch shortcut", Some(if WIN { "Click, then press the keys. Include Ctrl, Alt, Shift or Win." } else { "Click, then press the keys. Include Ctrl, Alt, Shift or Super." }.into()),
            Control::Shortcut { text: i.shortcut.clone() }, Lead::None),
        row("Quit Hover", Some("Stops every agent that is still working.".into()),
            Control::Button { id: "Quit".into(), name: "Quit Hover".into(), text: "Quit".into(), enabled: true }, Lead::None),
    ]));
    b.push(Block::Footnote("The same office opens from the tray icon, and in its own window from a click on its name in the notch.".into()));
}

pub const SIZES: [(WorkspaceSize, &str); 4] = [(WorkspaceSize::Small, "Small"), (WorkspaceSize::Default, "Default"), (WorkspaceSize::Large, "Large"), (WorkspaceSize::ExtraLarge, "Extra large")];
pub const APPEARANCES: [(Appearance, &str); 3] = [(Appearance::System, "System"), (Appearance::Light, "Light"), (Appearance::Dark, "Dark")];

fn notch(b: &mut Vec<Block>, i: &Input) {
    let cur = i.settings.workspace_size();
    b.push(Block::Group(vec![row("Office size", Some("How big the notch opens. It never grows past the screen.".into()),
        segments("WorkspaceSize", &SIZES.map(|s| s.1), SIZES.iter().position(|s| s.0 == cur).map_or(-1, |p| p as i32)), Lead::None)]));
    b.push(Block::Footnote("With nothing to show, the notch hides. Hover the top centre or press the shortcut to open it. \
        The app window keeps its own size: drag its edges.".into()));
}

/// Same(a, b): name, darkness and every colour.
pub fn same(a: &SavedTheme, b: &SavedTheme) -> bool {
    a.name == b.name && a.dark == b.dark && a.colors.len() == b.colors.len() && a.colors.iter().all(|(k, v)| b.colors.iter().any(|(k2, v2)| k2 == k && v2 == v))
}

fn themes(b: &mut Vec<Block>, i: &Input) {
    let current = i.settings.theme();
    let app = if current.is_none() { APPEARANCES.iter().position(|a| a.0 == i.settings.appearance()).map_or(-1, |p| p as i32) } else { -1 };
    b.push(Block::Group(vec![row("Appearance", Some(if WIN { "Hover's own colours: follow Windows, or keep them light or dark." } else { "Hover's own colours: follow the system, or keep them light or dark." }.into()),
        segments("Appearance", &APPEARANCES.map(|a| a.1), app), Lead::None)]));
    heading(b, "Themes");
    let hover_dark = match i.settings.appearance() { Appearance::Light => false, Appearance::Dark => true, Appearance::System => i.system_dark };
    let mut tiles = vec![Tile { id: "ThemeHover".into(), name: "Hover".into(), from: "Built in".into(), picked: current.is_none(), palette: Palette::hover(hover_dark) }];
    // An imported file is not in the list; it still shows while it is the one in use.
    if let Some(c) = &current {
        if !i.installed.iter().any(|(_, t)| same(t, c)) {
            tiles.push(Tile { id: format!("Theme{}", c.name), name: c.name.clone(), from: "Imported".into(), picked: true, palette: Palette::from_theme(c) });
        }
    }
    for (src, t) in i.installed {
        tiles.push(Tile { id: format!("Theme{}", src.label), name: src.label.clone(), from: src.from.clone(), picked: current.as_ref().is_some_and(|c| same(t, c)), palette: Palette::from_theme(t) });
    }
    b.push(Block::Tiles(tiles));
    b.push(Block::Link { id: "ImportTheme".into(), name: "Import a VS Code theme file".into(), icon: "import", text: "Import a VS Code theme file…".into(), dim: false, status: i.import_status.clone() });
    b.push(Block::Footnote(format!("The colour themes of VS Code, Cursor, Kiro and Windsurf on this {} show here, and any VS Code theme file (.json) can be imported. \
        A theme colours Settings, its menus and the app window; the office and the resting notch keep their own look.", if WIN { "PC" } else { "computer" })));
}

pub fn quota_hint(id: &str) -> &'static str {
    match id {
        item::CLAUDE => "Needs Claude Code signed in with a Pro or Max plan.",
        item::KIRO => "Needs kiro-cli installed and signed in.",
        item::CODEX => "Reads the limits Codex records as you use it.",
        _ => "Needs Cursor installed and signed in.",
    }
}

/// RefreshQuotaRows: the ring and the line under a quota's switch.
pub fn quota_status(on: bool, id: &str, reading: Option<&Reading>) -> (Option<f64>, String) {
    if !on { return (None, quota_hint(id).into()); }
    match reading {
        None => (None, "Reading…".into()),
        Some(r) => (r.used, if r.ok() { format!("{}% used · {}", hover_quota::num::custom(r.used.unwrap(), 0), r.detail) } else { r.detail.clone() }),
    }
}

fn quotas(b: &mut Vec<Block>, i: &Input) {
    let mut rows = vec![];
    for id in item::QUOTAS {
        let on = i.settings.has_notch_item(id);
        // Built with "Reading..." (three dots) and filled in at once, as the C# does.
        let (ring, text) = quota_status(on, id, (i.reading)(id).as_ref());
        let mut r = row(item::title(id), Some(text), switch(&format!("NotchItem{id}"), item::title(id), on), Lead::Ring(ring));
        r.sub_id = Some(format!("QuotaStatus{id}"));
        rows.push(r);
    }
    b.push(Block::Group(rows));
    b.push(Block::Link { id: "RefreshQuotas".into(), name: "Refresh quotas now".into(), icon: "refresh", text: "Refresh quotas now".into(), dim: true, status: String::new() });
    b.push(Block::Footnote("Quotas are read every five minutes: Kiro from \"kiro-cli /usage\", Codex from its own session logs, \
        Cursor from cursor.com and Claude Code from api.anthropic.com, each with the sign-in that tool already keeps. Nothing else is sent.".into()));
}

fn offer<'a>(offers: &'a [AcpOption], category: &str, ids: &[&str]) -> Option<&'a AcpOption> {
    offers.iter().find(|x| x.category.as_deref() == Some(category)).or_else(|| offers.iter().find(|x| ids.contains(&x.id.as_str())))
}

/// The models to pick from: what the tool offered in its last run, Kiro's own list
/// before that, with a "Default" first when the list has no auto of its own.
pub fn models(tool: AgentTool, offers: &[AcpOption]) -> Vec<(String, String)> {
    let mut m: Vec<(String, String)> = match offer(offers, "model", &["model"]) {
        Some(o) => o.choices.iter().map(|c| (c.value.clone(), c.name.clone())).collect(),
        None if tool == AgentTool::Kiro => hover_agents::KIRO_MODELS.iter().map(|(i, n)| (i.to_string(), n.to_string())).collect(),
        None => vec![],
    };
    if m.is_empty() || !(m[0].0 == "auto" || m[0].0.starts_with("default")) { m.insert(0, (String::new(), "Default".into())); }
    m
}

pub fn effort_offer(offers: &[AcpOption]) -> Option<&AcpOption> { offer(offers, "thought_level", &["effortLevel", "reasoning_effort", "effort"]) }

/// An effort's label: "xhigh" is X-High, the rest capitalised.
pub fn effort_label(l: &str) -> String {
    if l == "xhigh" { return "X-High".into(); }
    let mut c = l.chars();
    c.next().map_or(String::new(), |f| f.to_uppercase().collect::<String>() + c.as_str())
}

/// The effort shown as picked: the saved one if offered, else the tool's current, else the first.
pub fn effort_picked(o: &AgentOptions, offer: Option<&AcpOption>) -> usize {
    let levels: Vec<&str> = offer.map(|x| x.choices.iter().map(|c| c.value.as_str()).collect()).unwrap_or_default();
    o.effort.as_deref().and_then(|e| levels.iter().position(|l| *l == e))
        .or_else(|| offer.and_then(|x| x.current.as_deref()).and_then(|n| levels.iter().position(|l| *l == n)))
        .unwrap_or(0)
}

fn agent(b: &mut Vec<Block>, section: Section, i: &Input) {
    let tool = section.tool();
    let name = tool.name();
    let id = name;
    let o = i.settings.agent_options(tool);
    let offers = i.settings.agent_offers(tool);

    // Installed and signed in? Greyed out, with what to do, when not.
    let ready = (i.ready)(tool);
    let status = match &ready { None => "Checking…".to_owned(), Some(r) if r.ok() => "Installed and signed in.".into(), Some(r) => r.hint.clone() };
    let bad = ready.as_ref().is_some_and(|r| !r.ok());
    b.push(Block::Group(vec![row(name, Some(status), Control::Button { id: format!("{id}Recheck"), name: format!("Check {name} again"), text: "Check again".into(), enabled: true },
        Lead::Tile(if bad { "bell" } else { "done" }, if bad { Tint::Orange } else { Tint::Green }))]));
    let usable = !bad;

    heading(b, "Model");
    let models = models(tool, &offers);
    let current = o.model.clone().unwrap_or_else(|| models[0].0.clone());
    let shown = models.iter().find(|m| m.0 == current).map_or(current.clone(), |m| m.1.clone());
    let model = Control::Picker { id: format!("{id}Model"), name: "Model".into(), shown, options: models.iter().map(|m| (m.1.clone(), m.0 == current)).collect() };
    let eff = effort_offer(&offers);
    let levels: Vec<String> = eff.map(|x| x.choices.iter().map(|c| c.value.clone()).collect()).unwrap_or_default();
    let effort = if levels.is_empty() {
        Control::Text(if tool == AgentTool::Cursor { "Part of the model" } else { "Set by the model" }.into())
    } else {
        Control::Segments { id: format!("{id}Effort"), labels: levels.iter().map(|l| effort_label(l)).collect(), picked: effort_picked(&o, eff) as i32 }
    };
    let has_models = offer(&offers, "model", &["model"]).is_some();
    b.push(Block::Group(vec![
        row("Model", Some(if has_models { format!("The first is {name}’s own choice for each task.") } else { format!("More models show here once {name} has run a task.") }), model, Lead::Tile("brain", Tint::Purple)),
        row("Effort", Some(if levels.is_empty() {
            if tool == AgentTool::Cursor { "Cursor’s models carry their effort in their name.".into() } else { "Shown once a task has run with a model that takes one.".into() }
        } else { "How long it thinks. Higher is slower and uses more of your plan.".into() }), effort, Lead::Tile("gauge", Tint::Orange)),
    ]));

    heading(b, "Tools and memory");
    let mut rows = vec![];
    if tool == AgentTool::Kiro {
        let modes = kiro_modes(&offers, &i.kiro_agents);
        let shown = match &o.agent { None => "Default".to_owned(), Some(a) => modes.iter().find(|m| &m.0 == a).map_or(a.clone(), |m| m.1.clone()) };
        let mut options = vec![("Default".to_owned(), o.agent.is_none())];
        options.extend(modes.iter().filter(|m| m.0 != "vibe").map(|m| (m.1.clone(), Some(&m.0) == o.agent.as_ref())));
        rows.push(row("Agent", Some("Its MCP servers, skills and steering come with it. Kiro’s own modes (Spec, Plan…) are here too.".into()),
            Control::Picker { id: "KiroAgent".into(), name: "Agent".into(), shown, options }, Lead::Tile("bot", Tint::Blue)));
    }
    // Full access, with or without asking first in the notch, or read only. Read only
    // (where it holds) overrules asking.
    let ro = agents::read_only_works(tool);
    let access = o.access_id(ro);
    let mut labels = vec!["Full", "Ask first", "Ask always"];
    if ro { labels.push("Read only"); }
    let text = match access {
        "read" => format!("{name} can only read and search. It can’t change files or run commands."),
        // Codex decides what to ask about itself in this mode: its sandbox lets commands
        // inside the folder run, and asks to go past it.
        "risky" if tool == AgentTool::Codex => "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands.".into(),
        "risky" => format!("{name} asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead."),
        "always" => format!("{name} asks in the notch before any change or command. Reading and searching go ahead."),
        _ => format!("{name} can edit files and run commands without asking."),
    } + if ro { "" } else { " Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows." };
    let picked = ["full", "risky", "always", "read"].iter().position(|a| *a == access).unwrap_or(0) as i32;
    rows.push(row("Tool access", Some(text), segments(&format!("{id}Tools"), &labels, picked), Lead::Tile("shield", Tint::Green)));
    if tool == AgentTool::Kiro {
        rows.push(row("Require MCP servers", Some("Stop the task when one of the agent’s MCP servers doesn’t start.".into()),
            switch("KiroRequireMcp", "Require MCP servers", o.require_mcp), Lead::Tile("plug", Tint::Teal)));
    }
    rows.push(row("Show the tools it runs", Some(if o.hide_steps { format!("The chat shows only what you asked and {name}’s answers. The steps are still kept.") } else { format!("The chat lists each file {name} reads or edits and each command it runs.") }),
        switch(&format!("{id}ShowSteps"), "Show the tools it runs", !o.hide_steps), Lead::Tile("lines", Tint::Blue)));
    let idle: Vec<String> = AgentOptions::IDLE_CHOICES.iter().map(|m| format!("{m} min")).collect();
    rows.push(row("Keep it running", Some(format!("How long {name} stays open with nothing to do. A reply after that starts it again and picks the conversation back up.")),
        Control::Segments { id: format!("{id}Idle"), labels: idle, picked: AgentOptions::IDLE_CHOICES.iter().position(|m| *m == o.idle_minutes).map_or(-1, |p| p as i32) },
        Lead::Tile("clock", Tint::Gray)));
    for r in &mut rows { r.enabled = usable; }
    b.push(Block::Group(rows));

    let args = agents::arguments(tool).join(" ");
    if tool != AgentTool::Kiro {
        let exe = agents::exe(tool).and_then(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned())).unwrap_or_else(|| tool.id().into());
        b.push(Block::Footnote(format!("{name} runs in the background as an ACP server (\"{}\"), one for all its tasks, with no terminal window. \
            Prompts go to it on its input, never on a command line. Changes apply to the next task.", format!("{exe} {args}").trim())));
        return;
    }

    heading(b, "Project");
    let folder = i.settings.kiro_folder();
    let have = hover_agents::usable_folder(folder.as_deref());
    b.push(Block::Group(vec![
        row("Project folder", Some(match &folder { None => "None yet. The office asks for one before the first task.".into(), Some(f) if have => f.clone(), Some(f) => format!("{f} isn’t there any more; the office will ask for another.") }),
            Control::Button { id: "SettingsKiroFolder".into(), name: "Choose the agents' folder".into(), text: if have { "Change…" } else { "Choose…" }.into(), enabled: true }, Lead::Tile("folder", Tint::Purple)),
        row("Note about tool access", Some("The note the office shows before its first task.".into()),
            Control::Button { id: "KiroNoticeAgain".into(), name: "Show the note about tool access again".into(), text: "Show".into(), enabled: i.settings.kiro_notice_seen() }, Lead::Tile("sparkles", Tint::Gray)),
    ]));
    b.push(Block::Footnote(format!("Kiro runs in the background as an ACP server (\"kiro-cli {args}\"), one for all its \
        tasks, with no terminal window. Prompts go to it on its input, never on a command line. Changes apply to the next task.")));
}

/// Kiro's agents: its own modes when it offered them, else the agents in the folder.
pub fn kiro_modes(offers: &[AcpOption], folder_agents: &[String]) -> Vec<(String, String)> {
    match offer(offers, "mode", &["mode"]) {
        Some(o) => o.choices.iter().map(|c| (c.value.clone(), c.name.clone())).collect(),
        None => folder_agents.iter().map(|a| (a.clone(), a.clone())).collect(),
    }
}

/// A pick in a section: the new options, as Pages.cs's Set(o with { … }) makes them.
pub fn pick_model(tool: AgentTool, o: &AgentOptions, offers: &[AcpOption], index: usize) -> AgentOptions {
    let m = models(tool, offers);
    let v = &m[index.min(m.len() - 1)].0;
    AgentOptions { model: if *v == m[0].0 || v.is_empty() { None } else { Some(v.clone()) }, ..o.clone() }
}

pub fn pick_agent(o: &AgentOptions, offers: &[AcpOption], folder_agents: &[String], index: usize) -> AgentOptions {
    if index == 0 { return AgentOptions { agent: None, ..o.clone() }; }
    let modes: Vec<(String, String)> = kiro_modes(offers, folder_agents).into_iter().filter(|m| m.0 != "vibe").collect();
    AgentOptions { agent: modes.get(index - 1).map(|m| m.0.clone()), ..o.clone() }
}

pub fn pick_effort(o: &AgentOptions, offers: &[AcpOption], index: usize) -> AgentOptions {
    let v = effort_offer(offers).and_then(|x| x.choices.get(index)).map(|c| c.value.clone());
    AgentOptions { effort: v.or(o.effort.clone()), ..o.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::model::AcpChoice;

    fn input<'a>(s: &'a Settings, installed: &'a [(InstalledTheme, SavedTheme)], reading: &'a dyn Fn(&str) -> Option<Reading>, ready: &'a dyn Fn(AgentTool) -> Option<AgentReady>) -> Input<'a> {
        Input { settings: s, launch_at_login: false, shortcut: s.sc_workspace().label(), reading, ready, installed, system_dark: true, import_status: String::new(), kiro_agents: vec!["reviewer".into()] }
    }

    fn settings() -> std::sync::Arc<Settings> {
        let d = std::env::temp_dir().join(format!("hover-pages-{}", hover_core::guid_n()));
        std::fs::create_dir_all(&d).unwrap();
        Settings::load(d.join("settings.json"))
    }

    fn rows(b: &[Block]) -> Vec<&Row> { b.iter().filter_map(|x| if let Block::Group(r) = x { Some(r) } else { None }).flatten().collect() }

    fn ids(b: &[Block]) -> Vec<String> {
        let mut v = vec![];
        for r in rows(b) {
            match &r.control {
                Control::Switch { id, .. } | Control::Button { id, .. } | Control::Picker { id, .. } => v.push(id.clone()),
                Control::Segments { id, labels, .. } => v.extend(labels.iter().map(|l| format!("{id}{l}"))),
                Control::Shortcut { .. } => v.push("WorkspaceShortcut".into()),
                _ => {}
            }
            if let Some(s) = &r.sub_id { v.push(s.clone()); }
        }
        for x in b {
            match x { Block::Tiles(t) => v.extend(t.iter().map(|t| t.id.clone())), Block::Link { id, .. } => v.push(id.clone()), _ => {} }
        }
        v
    }

    /// Every automation id SCREENS.md lists for Settings, section by section.
    #[test]
    fn each_section_has_the_ids_screens_lists() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let i = input(&s, &[], &none, &ready);
        let g = ids(&build(Section::General, &i));
        for id in ["LaunchAtLogin", "HoverOpens", "WorkspaceShortcut", "Quit", "WorkspaceSizeSmall", "WorkspaceSizeDefault", "WorkspaceSizeLarge",
            "WorkspaceSizeExtra large", "AppearanceSystem", "AppearanceLight", "AppearanceDark", "ThemeHover", "ImportTheme"] {
            assert!(g.contains(&id.to_string()), "{id} in {g:?}");
        }
        let q = ids(&build(Section::Integrations, &i));
        for id in ["claude", "kiro", "codex", "cursor"] {
            assert!(q.contains(&format!("NotchItem{id}")) && q.contains(&format!("QuotaStatus{id}")));
        }
        assert!(q.contains(&"RefreshQuotas".to_string()));
        let k = ids(&build(Section::Kiro, &i));
        for id in ["KiroRecheck", "KiroModel", "KiroAgent", "KiroToolsFull", "KiroToolsRead only", "KiroRequireMcp", "KiroShowSteps", "KiroIdle5 min", "KiroIdle15 min", "SettingsKiroFolder", "KiroNoticeAgain"] {
            assert!(k.contains(&id.to_string()), "{id} in {k:?}");
        }
        let c = ids(&build(Section::Codex, &i));
        assert!(c.contains(&"CodexRecheck".to_string()) && c.contains(&"CodexShowSteps".to_string()));
        // Codex's read only: offered on Linux (it has a sandbox there), not on Windows.
        assert_eq!(c.contains(&"CodexToolsRead only".to_string()), !cfg!(windows));
    }

    /// The rows' words, from Pages.cs.
    #[test]
    fn the_words_are_the_csharps() {
        let s = settings();
        let reading = |id: &str| (id == "codex").then(|| Reading { used: Some(37.5), detail: "5h 38% · week 12%".into() });
        let ready = |t| (t == AgentTool::Cursor).then(|| AgentReady { installed: false, signed_in: false, hint: "Install the Cursor CLI.".into() });
        let i = input(&s, &[], &reading, &ready);
        s.set_notch_item("codex", true);
        let q = build(Section::Integrations, &i);
        let r = rows(&q);
        assert_eq!(r[2].sub.as_deref(), Some("38% used · 5h 38% · week 12%"));
        assert_eq!(r[2].lead, Lead::Ring(Some(37.5)));
        assert_eq!(r[0].sub.as_deref(), Some("Needs Claude Code signed in with a Pro or Max plan."));
        assert_eq!(quota_status(true, "kiro", None).1, "Reading…");
        // Not ready: the hint, a bell, and the rest greyed.
        let c = build(Section::Cursor, &i);
        let r = rows(&c);
        assert_eq!((r[0].sub.as_deref(), &r[0].lead), (Some("Install the Cursor CLI."), &Lead::Tile("bell", Tint::Orange)));
        assert!(r[3..].iter().all(|x| !x.enabled));
        assert_eq!(r[2].control, Control::Text("Part of the model".into()));
        // Kiro before any run: its own list with Auto first, "Checking…" until known.
        let k = build(Section::Kiro, &i);
        let r = rows(&k);
        assert_eq!(r[0].sub.as_deref(), Some("Checking…"));
        let Control::Picker { shown, options, .. } = &r[1].control else { panic!() };
        assert_eq!((shown.as_str(), options[0].0.as_str(), options.len()), ("Auto", "Auto", 14));
        assert!(matches!(&k[0], Block::Title(t) if t == "Kiro"));
        assert!(k.contains(&Block::Heading("TOOLS AND MEMORY".into(), false)));
    }

    #[test]
    fn picks_make_the_options_as_pages_does() {
        let o = AgentOptions::default();
        let offers = vec![AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: vec![
            AcpChoice { value: "gpt-5".into(), name: "GPT-5".into() }, AcpChoice { value: "o3".into(), name: "o3".into() }] },
            AcpOption { id: "reasoning_effort".into(), category: None, current: Some("medium".into()), choices: vec![
            AcpChoice { value: "low".into(), name: "Low".into() }, AcpChoice { value: "medium".into(), name: "Medium".into() }, AcpChoice { value: "xhigh".into(), name: "x".into() }] }];
        // No auto of its own: "Default" (none sent) comes first.
        assert_eq!(models(AgentTool::Codex, &offers)[0], (String::new(), "Default".to_string()));
        assert_eq!(pick_model(AgentTool::Codex, &o, &offers, 2).model.as_deref(), Some("o3"));
        assert_eq!(pick_model(AgentTool::Codex, &o, &offers, 0).model, None);
        assert_eq!(effort_picked(&o, effort_offer(&offers)), 1);
        assert_eq!(pick_effort(&o, &offers, 2).effort.as_deref(), Some("xhigh"));
        assert_eq!(effort_label("xhigh"), "X-High");
        assert_eq!(effort_label("low"), "Low");
        let modes = vec!["vibe".to_string(), "reviewer".to_string()];
        assert_eq!(pick_agent(&o, &[], &modes, 1).agent.as_deref(), Some("reviewer"));
    }
}

//! Owl/Pages.cs (SettingsPage): six sections, each a few headed groups of rows, a
//! label on the left and its control on the right. Built here as data, row for row
//! and string for string, with the C#'s automation ids; ui/settings.slint draws it.
//! Where Windows is named and Linux differs, the Linux words are the nearest ones.

use hover_agents::agents::{self, AgentReady};
use hover_core::model::{AcpOption, AgentOptions, AgentTool, Appearance, SavedTheme, WorkspaceSize};
use hover_core::palette::{InstalledTheme, Palette};
use hover_core::projects::{resolve_folder, CleanupProvider, Project, SpeechMode, ACCESS_IDS, GROQ_SECRET, TRANSCRIBE_MODELS};
use hover_core::settings::Settings;
use hover_quota::{item, Reading};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section { General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode }

impl Section {
    pub const ALL: [Section; 8] = [Section::General, Section::Integrations, Section::Projects, Section::Voice, Section::Kiro, Section::Codex, Section::Cursor, Section::OpenCode];
    pub fn title(self) -> &'static str { ["General", "Integrations", "Projects", "Voice", "Kiro", "Codex", "Cursor", "OpenCode"][self as usize] }
    /// The sidebar's icon and its tile's colour.
    pub fn glyph(self) -> (&'static str, Tint) {
        [("settings", Tint::Gray), ("plug", Tint::Purple), ("folder", Tint::Orange), ("mic", Tint::Pink), ("ghost", Tint::Bot), ("terminal", Tint::Green), ("sparkles", Tint::Blue), ("terminal", Tint::Gray)][self as usize]
    }
    /// The section of a tool's own page.
    pub fn of(tool: AgentTool) -> Section {
        match tool { AgentTool::Codex => Section::Codex, AgentTool::Cursor => Section::Cursor, AgentTool::OpenCode => Section::OpenCode, _ => Section::Kiro }
    }
    pub fn tool(self) -> AgentTool {
        match self { Section::Codex => AgentTool::Codex, Section::Cursor => AgentTool::Cursor, Section::OpenCode => AgentTool::OpenCode, _ => AgentTool::Kiro }
    }
}

/// The tile colours Pages.cs uses: the palette's accents, Ui.Gray, and the Kiro bot's
/// purple; Pink is the mockup's Voice tile.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tint { Gray, Purple, Bot, Green, Blue, Orange, Teal, Pink }

#[derive(Clone, Debug, PartialEq)]
pub enum Control {
    None,
    Switch { id: String, name: String, on: bool },
    /// A grey pill (OwlLightButton) with its text.
    Button { id: String, name: String, text: String, enabled: bool },
    /// A shortcut field: shows the shortcut, records the next chord. The notch's is
    /// "WorkspaceShortcut".
    Shortcut { id: String, name: String, text: String },
    Segments { id: String, labels: Vec<String>, picked: i32 },
    /// A button showing the current choice that opens a menu of them.
    Picker { id: String, name: String, shown: String, options: Vec<(String, bool)> },
    Text(String),
    /// A one-line text box, saved on Enter or when it loses focus. A secret one is
    /// masked and always starts empty (the placeholder says whether a key is kept);
    /// `on` is whether one is, which offers Remove key.
    Field { id: String, name: String, value: String, placeholder: String, secret: bool, on: bool },
    /// Badges (warn: amber), then buttons (id, text, red). With `open`, the whole row is
    /// a button to that id, with a chevron at its end.
    Chips { badges: Vec<(String, bool)>, buttons: Vec<(String, String, bool)>, open: Option<String> },
    /// A button that acts while held: "{id}.press" on the press, "{id}.release" on the release.
    Hold { id: String, name: String, text: String },
}

impl Control {
    /// The id the row's own control answers to.
    pub fn id(&self) -> Option<&str> {
        match self {
            Control::Switch { id, .. } | Control::Button { id, .. } | Control::Shortcut { id, .. } | Control::Segments { id, .. }
            | Control::Picker { id, .. } | Control::Field { id, .. } | Control::Hold { id, .. } => Some(id),
            Control::Chips { open, .. } => open.as_deref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Lead { None, Tile(&'static str, Tint), Ring(Option<f64>), Letter(String, Tint) }

#[derive(Clone, Debug, PartialEq)]
pub struct Row { pub label: String, pub sub: Option<String>, pub control: Control, pub lead: Lead, pub enabled: bool, pub sub_id: Option<String>, pub progress: Option<f32> }

fn row(label: impl Into<String>, sub: Option<String>, control: Control, lead: Lead) -> Row {
    Row { label: label.into(), sub, control, lead, enabled: true, sub_id: None, progress: None }
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
    /// The line under a page's title (the mockup's lead).
    Lead(String),
}

/// What only the running app knows about voice, filled in by it (the Phonon card, Try
/// it, the microphones, a check's answer). Kept in view::Pane; Default shows each part
/// as not known yet.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Live {
    pub phonon: Option<PhononCard>,
    pub voice_try: Option<TryCard>,
    /// Input devices by name; the system default is offered on its own.
    pub mics: Vec<String>,
    /// Why the voice shortcut couldn't be taken (another app holds it).
    pub shortcut_error: Option<String>,
    /// Check key's answer: "Checking…", "The key works.", or what Groq said.
    pub groq_check: Option<String>,
}

/// The local model's card.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PhononCard {
    /// "Phonon-2".
    pub model: String,
    /// "Not installed", "Downloading", "Verifying…", "Installing…", "Ready", "Cancelled",
    /// "Failed", "Can’t run on this computer".
    pub state: String,
    /// Bytes so far and the total (None when no truthful total is known), while downloading.
    pub progress: Option<(u64, Option<u64>)>,
    /// The facts below it, label and value: version, download, installed, runtime, folder.
    pub facts: Vec<(String, String)>,
    pub actions: Vec<PhononAction>,
    /// What failed, or why this computer can't run it.
    pub error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PhononAction { Download, Cancel, Retry, Repair, Remove }

impl PhononAction {
    pub fn id(self) -> &'static str { ["phonon.download", "phonon.cancel", "phonon.retry", "phonon.repair", "phonon.remove"][self as usize] }
    pub fn label(self) -> &'static str { ["Download", "Cancel", "Retry", "Repair", "Remove"][self as usize] }
}

/// Try it's result so far.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TryCard {
    /// The stage: "Listening… 2 s", "Transcribing…", "Done".
    pub status: String,
    /// What came of it, label and value: Heard, Cleaned up, Folder, Agent, Access, Task.
    pub lines: Vec<(String, String)>,
    pub error: Option<String>,
}

/// 1.5 MB, 164 MB, 2.1 GB: sizes as the cards show them.
pub fn size(bytes: u64) -> String {
    let mb = bytes as f64 / 1_000_000.0;
    if mb >= 1000.0 { format!("{:.1} GB", mb / 1000.0) } else if mb >= 10.0 { format!("{mb:.0} MB") } else { format!("{mb:.1} MB") }
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
    /// The voice shortcut field's text, as `shortcut` is the notch's.
    pub voice_shortcut: String,
    /// Whether the secret store holds a key by that name.
    pub has_secret: &'a dyn Fn(&str) -> bool,
    /// Keys set now are kept across restarts (Hover's own key is there).
    pub secrets_kept: bool,
    /// The project whose page is open in Projects; None is the list.
    pub project: Option<String>,
    /// What the last action said, by the id of the control it is about (a refused
    /// folder, a key that couldn't be saved): shown under that row.
    pub note: Option<(String, String)>,
    pub live: &'a Live,
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
        Section::Projects => projects(&mut b, i),
        Section::Voice => voice(&mut b, i),
        _ => agent(&mut b, section, i),
    }
    // The last action's message goes under the row (or beside the link) it is about.
    if let Some((id, text)) = &i.note {
        for x in &mut b {
            match x {
                Block::Group(rows) => for r in rows.iter_mut().filter(|r| r.control.id() == Some(id.as_str())) {
                    r.sub = Some(match &r.sub { Some(s) => format!("{s}\n{text}"), None => text.clone() });
                },
                Block::Link { id: l, status, .. } if l == id => *status = text.clone(),
                _ => {}
            }
        }
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
            Control::Shortcut { id: "WorkspaceShortcut".into(), name: "Notch shortcut".into(), text: i.shortcut.clone() }, Lead::None),
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

/// The access a project or the default workspace has: ACCESS_IDS, worded as the tool pages word them.
pub const ACCESS_LABELS: [&str; 4] = ["Full", "Ask first", "Ask always", "Read only"];

pub fn access_label(id: &str) -> &'static str { ACCESS_IDS.iter().position(|a| *a == id).map_or(ACCESS_LABELS[1], |p| ACCESS_LABELS[p]) }

fn access_segments(id: &str, access: &str) -> Control {
    segments(id, &ACCESS_LABELS, ACCESS_IDS.iter().position(|a| *a == access).map_or(-1, |p| p as i32))
}

/// What a target's access means for the agent voice starts there (the default one).
/// What a tool can't do is said, never widened.
fn target_access(access: &str, tool: AgentTool) -> String {
    let name = tool.name();
    match access {
        "read" if !agents::read_only_works(tool) => format!("{name} has no read only mode on this computer. Pick another access, or another agent in Settings → Voice."),
        "read" => format!("{name} can only read and search here."),
        "always" => format!("{name} asks in the notch before any change or command."),
        "risky" if tool == AgentTool::Codex => "Codex asks in the notch before it writes outside the folder or goes online.".into(),
        "risky" => format!("{name} asks in the notch before commands, deletes, the network or anything outside the folder."),
        _ => format!("{name} edits files and runs commands here without asking."),
    }
}

fn field(id: &str, name: &str, value: &str, placeholder: &str) -> Control {
    Control::Field { id: id.into(), name: name.into(), value: value.into(), placeholder: placeholder.into(), secret: false, on: false }
}

/// A key's box: empty, its placeholder saying whether one is kept.
fn secret(id: &str, name: &str, name_in_store: &str, i: &Input) -> Control {
    let has = (i.has_secret)(name_in_store);
    let placeholder = if !has { "Not set" } else if i.secrets_kept { "Saved" } else { "Kept this run only" };
    Control::Field { id: id.into(), name: name.into(), value: String::new(), placeholder: placeholder.into(), secret: true, on: has }
}

fn key_note(i: &Input, text: &str) -> String {
    if i.secrets_kept { text.into() } else { format!("{text} Hover can’t keep keys on this computer right now, so one set now lasts until Hover quits.") }
}

/// A project's tile: its first letter on a colour of its own (by its id, so it keeps it).
pub fn letter(p: &Project) -> Lead {
    const TINTS: [Tint; 6] = [Tint::Bot, Tint::Teal, Tint::Pink, Tint::Blue, Tint::Orange, Tint::Green];
    let k = p.id.bytes().map(usize::from).sum::<usize>() % TINTS.len();
    Lead::Letter(p.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default(), TINTS[k])
}

fn projects(b: &mut Vec<Block>, i: &Input) {
    let s = i.settings;
    let tool = s.voice().agent.unwrap_or_else(|| s.agent_tool());
    if let Some(p) = i.project.as_deref().and_then(|id| s.project(id)) { return project(b, &p, tool); }
    b.push(Block::Lead("Folders voice may start tasks in. When you talk to Hover, it picks one from this list.".into()));
    heading(b, "Projects");
    let rows: Vec<Row> = s.projects().iter().map(|p| {
        let sub = match resolve_folder(&p.folder) {
            Err(e) => e,
            Ok(_) => match p.aliases.first() { Some(a) => format!("{} · say “{a}”", p.folder), None => p.folder.clone() },
        };
        let badges = vec![(if p.voice { "Voice" } else { "Voice off" }.to_owned(), false), (access_label(&p.access).to_owned(), p.access == "full")];
        row(&p.name, Some(sub), Control::Chips { badges, buttons: vec![], open: Some(format!("Project.{}", p.id)) }, letter(p))
    }).collect();
    if !rows.is_empty() { b.push(Block::Group(rows)); }
    b.push(Block::Link { id: "ProjectAdd".into(), name: "Add a project".into(), icon: "add", text: "Add a project…".into(), dim: false, status: String::new() });
    b.push(Block::Footnote("Being on this list doesn’t mean Full access: each project keeps its own. Wherever voice starts a task, it uses the default agent and model (see Voice).".into()));
    heading(b, "Default workspace");
    let w = s.default_workspace();
    let sub = match w.path() {
        None => "No home folder was found. Choose a folder.".to_owned(),
        Some(p) if p.is_dir() => p.to_string_lossy().into_owned(),
        Some(p) => format!("{} · made when first needed", p.display()),
    };
    b.push(Block::Group(vec![
        row("Location", Some(sub), Control::Button { id: "DefaultFolder".into(), name: "Change the default workspace".into(), text: "Change…".into(), enabled: true }, Lead::Tile("home", Tint::Gray)),
        row("Tool access", Some(target_access(&w.access, tool)), access_segments("DefaultAccess", &w.access), Lead::Tile("shield", Tint::Green)),
    ]));
    b.push(Block::Footnote("When what you say names no project here, or isn’t clear, the task starts in the default workspace.".into()));
}

fn project(b: &mut Vec<Block>, p: &Project, tool: AgentTool) {
    b[0] = Block::Link { id: "ProjectBack".into(), name: "Back to Projects".into(), icon: "chevron-left", text: "Projects".into(), dim: false, status: String::new() };
    b.push(Block::Title(p.name.clone()));
    let folder = match resolve_folder(&p.folder) { Ok(_) => p.folder.clone(), Err(e) => e };
    b.push(Block::Group(vec![
        row("Name", None, field("ProjectName", "Name", &p.name, "A name to say"), Lead::None),
        row("Folder", Some(folder), Control::Button { id: "ProjectFolder".into(), name: "Change the project’s folder".into(), text: "Change…".into(), enabled: true }, Lead::None),
    ]));
    heading(b, "Voice");
    b.push(Block::Group(vec![
        row("Also called", Some("Saying any of these finds this project. Separate them with commas.".into()), field("ProjectAliases", "Also called", &p.aliases.join(", "), "the site, website"), Lead::None),
        row("Voice can start tasks here", Some("Off: voice skips this project.".into()), switch("ProjectVoice", "Voice can start tasks here", p.voice), Lead::None),
    ]));
    heading(b, "Access");
    b.push(Block::Group(vec![row("Tool access", Some(target_access(&p.access, tool)), access_segments("ProjectAccess", &p.access), Lead::None)]));
    b.push(Block::Group(vec![row("Remove from projects", None,
        Control::Chips { badges: vec![], buttons: vec![("ProjectRemove".into(), "Remove".into(), true)], open: None }, Lead::None)]));
    b.push(Block::Footnote("Removing it only takes it off this list. Its folder, files, history and any task still running stay as they are.".into()));
}

fn voice(b: &mut Vec<Block>, i: &Input) {
    let s = i.settings;
    let v = s.voice();
    let local = v.speech == SpeechMode::Local;
    b.push(Block::Lead("Talk to Hover from anywhere. The notch listens while you hold the shortcut.".into()));
    let mut mics = vec![("System default".to_owned(), v.microphone.is_none())];
    mics.extend(i.live.mics.iter().map(|m| (m.clone(), v.microphone.as_ref() == Some(m))));
    // A saved device that isn't plugged in now still shows as the one picked.
    if let Some(m) = v.microphone.as_ref().filter(|m| !i.live.mics.contains(m)) { mics.push((m.clone(), true)); }
    b.push(Block::Group(vec![
        row("Voice control", None, switch("VoiceEnabled", "Voice control", v.enabled), Lead::None),
        row("Shortcut", Some(i.live.shortcut_error.clone().unwrap_or_else(|| "Hold it to talk, let go to finish. Up to ten minutes.".into())),
            Control::Shortcut { id: "VoiceShortcut".into(), name: "Voice shortcut".into(), text: i.voice_shortcut.clone() }, Lead::None),
        row("Microphone", None, Control::Picker { id: "VoiceMicrophone".into(), name: "Microphone".into(),
            shown: v.microphone.clone().unwrap_or_else(|| "System default".into()), options: mics }, Lead::None),
    ]));

    heading(b, "Speech recognition");
    let mut language = row("Language", None, Control::Text(if local { "English only" } else { "Detected automatically" }.into()), Lead::None);
    language.enabled = !local;
    let mut rows = vec![
        row("Speech recognition", Some(if local { "Speech recognition stays on this computer. English only. For other languages, choose Cloud (Groq)." }
            else { "Audio is sent to Groq. Language is detected automatically." }.into()),
            segments("VoiceSpeech", &SpeechMode::ALL.map(|m| m.label()), SpeechMode::ALL.iter().position(|m| *m == v.speech).map_or(-1, |p| p as i32)), Lead::None),
        language,
    ];
    if !local {
        rows.push(row("Groq API key", Some(key_note(i, "Your own key, from console.groq.com.")), secret("VoiceGroqKey", "Groq API key", GROQ_SECRET, i), Lead::None));
        rows.push(row("Check the key", Some(i.live.groq_check.clone().unwrap_or_else(|| "Asks Groq whether the key works.".into())),
            Control::Button { id: "groq.check".into(), name: "Check the Groq key".into(), text: "Check key".into(), enabled: (i.has_secret)(GROQ_SECRET) }, Lead::None));
        let shown = TRANSCRIBE_MODELS.iter().find(|m| m.0 == v.model).map_or(v.model.clone(), |m| m.1.into());
        rows.push(row("Model", None, Control::Picker { id: "VoiceModel".into(), name: "Model".into(), shown,
            options: TRANSCRIBE_MODELS.iter().map(|m| (m.1.to_owned(), m.0 == v.model)).collect() }, Lead::None));
    }
    b.push(Block::Group(rows));
    if local {
        let english = vec![("English only".to_owned(), false)];
        let mut rows = vec![];
        match &i.live.phonon {
            None => rows.push(row("Phonon-2", Some("Checking…".into()), Control::Chips { badges: english, buttons: vec![], open: None }, Lead::Tile("cpu", Tint::Teal))),
            Some(c) => {
                let mut sub = c.state.clone();
                match c.progress {
                    Some((d, Some(t))) => sub += &format!(" · {} of {}", size(d), size(t)),
                    Some((d, None)) => sub += &format!(" · {}", size(d)),
                    None => {}
                }
                if let Some(e) = &c.error { sub += &format!("\n{e}"); }
                let buttons = c.actions.iter().map(|a| (a.id().to_owned(), a.label().to_owned(), *a == PhononAction::Remove)).collect();
                let mut r = row(&c.model, Some(sub), Control::Chips { badges: english, buttons, open: None }, Lead::Tile("cpu", Tint::Teal));
                r.progress = c.progress.and_then(|(d, t)| t.filter(|t| *t > 0).map(|t| (d as f64 / t as f64).min(1.0) as f32));
                rows.push(r);
                // A long value (the folder) wraps under its label; a short one sits on the right.
                rows.extend(c.facts.iter().map(|(l, v)| if v.chars().count() > 40 { row(l, Some(v.clone()), Control::None, Lead::None) } else { row(l, None, Control::Text(v.clone()), Lead::None) }));
            }
        }
        b.push(Block::Group(rows));
    }

    heading(b, "Cleanup");
    let p = v.cleanup_provider;
    let custom = p == CleanupProvider::Custom;
    let mut rows = vec![
        row("Clean up the text", Some("Fixes punctuation, grammar and filler words, in the language you spoke. If it fails, the original text is used.".into()),
            switch("VoiceCleanup", "Clean up the text", v.cleanup), Lead::None),
        row("Service", None, segments("VoiceCleanupProvider", &CleanupProvider::ALL.map(|c| c.name()), p as i32), Lead::None),
        row("Model", None, field("VoiceCleanupModel", "Cleanup model", v.cleanup_model.as_deref().unwrap_or(""), "The service’s model id"), Lead::None),
        row("API key", Some(key_note(i, &format!("Your own {} key.", if custom { "service’s" } else { p.name() }))), secret("VoiceCleanupKey", "Cleanup API key", p.secret(), i), Lead::None),
    ];
    if custom {
        rows.push(row("Base URL", Some("An OpenAI-compatible API, ending in /v1.".into()), field("VoiceCleanupBase", "Base URL", v.cleanup_base.as_deref().unwrap_or(""), "https://…/v1"), Lead::None));
    }
    b.push(Block::Group(rows));
    b.push(Block::Footnote(format!("With cleanup on, the transcript (never the audio) is sent to {} with your key.", if custom { "the address above" } else { p.name() })));

    heading(b, "Starting tasks");
    let tool = v.agent.unwrap_or_else(|| s.agent_tool());
    let o = s.agent_options(tool);
    let m = models(tool, &s.agent_offers(tool));
    let current = o.model.clone().unwrap_or_else(|| m[0].0.clone());
    let model = m.iter().find(|x| x.0 == current).map_or(current.clone(), |x| x.1.clone());
    let mut sub = if v.agent.is_some() { "Voice tasks start with this agent. The card lets you pick another for one task." }
        else { "Voice tasks start with the agent the office picked for its last new task. Pick one to keep it." }.to_owned();
    if let Some(r) = (i.ready)(tool).filter(|r| !r.ok()) { sub += &format!(" {} isn’t ready: {} Voice will ask you to pick another.", tool.name(), r.hint); }
    let w = s.default_workspace();
    let place = w.path().map_or_else(|| "No home folder".to_owned(), |p| p.to_string_lossy().into_owned());
    b.push(Block::Group(vec![
        row("Agent", Some(sub), Control::Picker { id: "VoiceAgentTool".into(), name: "Voice agent".into(), shown: tool.name().into(),
            options: AgentTool::ALL.iter().map(|t| (t.name().to_owned(), *t == tool)).collect() }, Lead::None),
        row("Model", Some(format!("The model, effort and access are {}’s own settings.", tool.name())),
            Control::Button { id: "VoiceAgent".into(), name: format!("Open {} settings", tool.name()), text: format!("{} · {model}", tool.name()), enabled: true }, Lead::None),
        row("Default workspace", Some(format!("{place} · {}", access_label(&w.access))),
            Control::Button { id: "VoiceWorkspace".into(), name: "Open Projects".into(), text: "Projects…".into(), enabled: true }, Lead::None),
    ]));

    heading(b, "Try it");
    let t = i.live.voice_try.as_ref();
    let sub = t.and_then(|t| t.error.clone().or_else(|| Some(t.status.clone()).filter(|x| !x.is_empty())))
        .unwrap_or_else(|| "Hold the button and speak. It shows what voice would start; nothing starts and no files are touched.".into());
    let mut rows = vec![row("Try it", Some(sub), Control::Hold { id: "voice.try".into(), name: "Hold to try voice".into(), text: "Hold to talk".into() }, Lead::None)];
    if let Some(t) = t { rows.extend(t.lines.iter().map(|(l, v)| row(l, Some(v.clone()), Control::None, Lead::None))); }
    b.push(Block::Group(rows));
    b.push(Block::Footnote("Hold the shortcut, say what to do (“in Hover, fix the notch blink”) and let go. A card shows the folder, agent, access and task, \
        and starts it after 3 seconds. Enter starts it now, editing the task stops the countdown, and Esc cancels.".into()));
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
    // OpenCode's variants belong to each model: only the picked model's are offered.
    let per_model = tool == AgentTool::OpenCode;
    let levels = effort_levels(tool, &o, &offers);
    let effort = if levels.is_empty() {
        Control::Text(if tool == AgentTool::Cursor { "Part of the model" } else if per_model { "None for this model" } else { "Set by the model" }.into())
    } else {
        let picked = o.effort.as_ref().and_then(|e| levels.iter().position(|l| l == e))
            .or_else(|| eff.and_then(|x| x.current.as_ref()).and_then(|n| levels.iter().position(|l| l == n))).unwrap_or(0);
        Control::Segments { id: format!("{id}Effort"), labels: levels.iter().map(|l| effort_label(l)).collect(), picked: picked as i32 }
    };
    let has_models = offer(&offers, "model", &["model"]).is_some();
    b.push(Block::Group(vec![
        row("Model", Some(if !has_models { format!("More models show here once {name} has run a task.") }
            else if per_model { "Your OpenCode providers’ models: API keys, sign-ins and local models. Default is your opencode config’s.".into() }
            else { format!("The first is {name}’s own choice for each task.") }), model, Lead::Tile("brain", Tint::Purple)),
        row(hover_agents::runtime::caps(tool).effort_label, Some(if per_model {
            if levels.is_empty() { "Pick a model with variants to choose one. Default leaves it to OpenCode.".into() } else { "The picked model’s own variants, from OpenCode.".into() }
        } else if levels.is_empty() {
            if tool == AgentTool::Cursor { "Cursor’s models carry their effort in their name.".into() } else { "Shown once a task has run with a model that takes one.".into() }
        } else { "How long it thinks. Higher is slower and uses more of your plan.".into() }), effort, Lead::Tile("gauge", Tint::Orange)),
    ]));

    heading(b, "Tools and memory");
    let mut rows = vec![];
    if tool == AgentTool::OpenCode {
        let modes = opencode_agents(&offers);
        let shown = match &o.agent { None => "Default".to_owned(), Some(a) => modes.iter().find(|m| &m.0 == a).map_or(a.clone(), |m| m.1.clone()) };
        let mut options = vec![("Default".to_owned(), o.agent.is_none())];
        options.extend(modes.iter().map(|m| (m.1.clone(), Some(&m.0) == o.agent.as_ref())));
        rows.push(row("Agent", Some(if modes.is_empty() { "Build, Plan and your own agents show here once OpenCode has run a task.".into() }
                else { "OpenCode’s agents, yours included. Plan can’t edit files by its own rules; it isn’t a sandbox.".into() }),
            Control::Picker { id: "OpenCodeAgent".into(), name: "Agent".into(), shown, options }, Lead::Tile("bot", Tint::Blue)));
    }
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
        "read" if tool == AgentTool::OpenCode => "OpenCode can only read and search. Its server refuses every edit, command, subagent and anything outside the folder.".into(),
        "read" => format!("{name} can only read and search. It can’t change files or run commands."),
        // Codex decides what to ask about itself in this mode: its sandbox lets commands
        // inside the folder run, and asks to go past it.
        "risky" if tool == AgentTool::Codex => "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands.".into(),
        "risky" => format!("{name} asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead."),
        "always" => format!("{name} asks in the notch before any change or command. Reading and searching go ahead."),
        _ if tool == AgentTool::OpenCode => "OpenCode can edit files and run commands without asking. Deny rules in your OpenCode config still win, and it still asks when it repeats a tool call over and over.".into(),
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
    if tool == AgentTool::OpenCode {
        b.push(Block::Footnote("OpenCode runs in the background as its own server (\"opencode serve\"), one for all its tasks, on this PC only \
            (127.0.0.1, with a password made for each start), with no terminal window. Your OpenCode providers, agents, skills and MCP servers \
            work as they do in OpenCode. It uses about 0.5 to 1 GB while it runs, so it stops when idle. Changes apply to the next task.".into()));
        return;
    }
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

/// The efforts Settings offers: the tool's own list, or for OpenCode the picked
/// model's variants.
pub fn effort_levels(tool: AgentTool, o: &AgentOptions, offers: &[AcpOption]) -> Vec<String> {
    if tool == AgentTool::OpenCode {
        return offer(offers, "model", &["model"]).and_then(|m| m.choices.iter().find(|c| Some(&c.value) == o.model.as_ref()).and_then(|c| c.levels.clone())).unwrap_or_default();
    }
    effort_offer(offers).map(|x| x.choices.iter().map(|c| c.value.clone()).collect()).unwrap_or_default()
}

/// OpenCode's agents (Build, Plan, the user's own), as it offered them.
pub fn opencode_agents(offers: &[AcpOption]) -> Vec<(String, String)> {
    offer(offers, "mode", &["mode"]).map(|o| o.choices.iter().map(|c| (c.value.clone(), c.name.clone())).collect()).unwrap_or_default()
}

pub fn pick_opencode_agent(o: &AgentOptions, offers: &[AcpOption], index: usize) -> AgentOptions {
    if index == 0 { return AgentOptions { agent: None, ..o.clone() }; }
    AgentOptions { agent: opencode_agents(offers).get(index - 1).map(|m| m.0.clone()), ..o.clone() }
}

pub fn pick_effort(tool: AgentTool, o: &AgentOptions, offers: &[AcpOption], index: usize) -> AgentOptions {
    let v = effort_levels(tool, o, offers).get(index).cloned();
    AgentOptions { effort: v.or(o.effort.clone()), ..o.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_core::model::AcpChoice;

    fn input<'a>(s: &'a Settings, installed: &'a [(InstalledTheme, SavedTheme)], reading: &'a dyn Fn(&str) -> Option<Reading>, ready: &'a dyn Fn(AgentTool) -> Option<AgentReady>) -> Input<'a> {
        static LIVE: std::sync::OnceLock<Live> = std::sync::OnceLock::new();
        fn no(_: &str) -> bool { false }
        Input { settings: s, launch_at_login: false, shortcut: s.sc_workspace().label(), reading, ready, installed, system_dark: true, import_status: String::new(), kiro_agents: vec!["reviewer".into()],
            voice_shortcut: s.voice().shortcut.label(), has_secret: &no, secrets_kept: true, project: None, note: None, live: LIVE.get_or_init(Live::default) }
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
                Control::Switch { id, .. } | Control::Button { id, .. } | Control::Picker { id, .. } | Control::Field { id, .. } | Control::Hold { id, .. } => v.push(id.clone()),
                Control::Segments { id, labels, .. } => v.extend(labels.iter().map(|l| format!("{id}{l}"))),
                Control::Shortcut { id, .. } => v.push(id.clone()),
                Control::Chips { buttons, open, .. } => { v.extend(open.clone()); v.extend(buttons.iter().map(|b| b.0.clone())); }
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

    /// Projects (the list, a project's page) and Voice (Cloud, then Local with Phonon
    /// downloading); a key's box never shows the key.
    #[test]
    fn projects_and_voice_pages() {
        use hover_core::projects::VoiceSettings;
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let live = Live { phonon: Some(PhononCard { model: "Phonon-2".into(), state: "Downloading".into(), progress: Some((41_000_000, Some(164_000_000))),
            facts: vec![("Version".into(), "phonon-2".into())], actions: vec![PhononAction::Cancel], error: None }), ..Default::default() };
        let has = |n: &str| n == GROQ_SECRET;
        let mut i = input(&s, &[], &none, &ready);
        let d = std::env::temp_dir().join(format!("hover-pages-proj-{}", hover_core::guid_n()));
        std::fs::create_dir_all(&d).unwrap();
        let p = s.add_project(&d.to_string_lossy()).unwrap();
        assert!(s.add_project(&format!("{}{}", d.display(), std::path::MAIN_SEPARATOR)).is_err(), "one folder is registered once");
        let list = build(Section::Projects, &i);
        let v = ids(&list);
        for id in [format!("Project.{}", p.id), "ProjectAdd".into(), "DefaultFolder".into(), "DefaultAccessAsk first".into()] { assert!(v.contains(&id), "{id} in {v:?}"); }
        assert_eq!(rows(&list)[0].control, Control::Chips { badges: vec![("Voice".into(), false), ("Ask first".into(), false)], buttons: vec![], open: Some(format!("Project.{}", p.id)) });
        i.project = Some(p.id.clone());
        let page = build(Section::Projects, &i);
        assert!(matches!(&page[0], Block::Link { id, .. } if id == "ProjectBack"));
        let v = ids(&page);
        for id in ["ProjectName", "ProjectFolder", "ProjectAliases", "ProjectVoice", "ProjectAccessRead only", "ProjectRemove"] { assert!(v.contains(&id.to_string()), "{id} in {v:?}"); }
        i.project = None;

        i.has_secret = &has;
        let cloud = build(Section::Voice, &i);
        let v = ids(&cloud);
        for id in ["VoiceEnabled", "VoiceShortcut", "VoiceMicrophone", "VoiceSpeechLocal (Phonon)", "VoiceGroqKey", "groq.check", "VoiceModel", "VoiceCleanup", "VoiceCleanupKey", "VoiceAgentTool", "VoiceAgent", "voice.try"] {
            assert!(v.contains(&id.to_string()), "{id} in {v:?}");
        }
        let key = rows(&cloud).into_iter().find(|r| r.control.id() == Some("VoiceGroqKey")).unwrap();
        assert_eq!(key.control, Control::Field { id: "VoiceGroqKey".into(), name: "Groq API key".into(), value: String::new(), placeholder: "Saved".into(), secret: true, on: true });
        s.set_voice(VoiceSettings { speech: SpeechMode::Local, ..s.voice() });
        i.live = &live;
        let local = build(Section::Voice, &i);
        let r = rows(&local);
        assert!(!ids(&local).contains(&"VoiceGroqKey".to_string()), "no Groq key asked for in Local");
        assert!(!r.iter().find(|r| r.label == "Language").unwrap().enabled);
        let card = r.iter().find(|r| r.label == "Phonon-2").unwrap();
        assert_eq!(card.sub.as_deref(), Some("Downloading · 41 MB of 164 MB"));
        assert_eq!(card.progress, Some(0.25));
        assert_eq!(card.control, Control::Chips { badges: vec![("English only".into(), false)], buttons: vec![("phonon.cancel".into(), "Cancel".into(), false)], open: None });
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn picks_make_the_options_as_pages_does() {
        let o = AgentOptions::default();
        let offers = vec![AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: vec![
            AcpChoice { value: "gpt-5".into(), name: "GPT-5".into(), levels: None }, AcpChoice { value: "o3".into(), name: "o3".into(), levels: None }] },
            AcpOption { id: "reasoning_effort".into(), category: None, current: Some("medium".into()), choices: vec![
            AcpChoice { value: "low".into(), name: "Low".into(), levels: None }, AcpChoice { value: "medium".into(), name: "Medium".into(), levels: None }, AcpChoice { value: "xhigh".into(), name: "x".into(), levels: None }] }];
        // No auto of its own: "Default" (none sent) comes first.
        assert_eq!(models(AgentTool::Codex, &offers)[0], (String::new(), "Default".to_string()));
        assert_eq!(pick_model(AgentTool::Codex, &o, &offers, 2).model.as_deref(), Some("o3"));
        assert_eq!(pick_model(AgentTool::Codex, &o, &offers, 0).model, None);
        assert_eq!(effort_picked(&o, effort_offer(&offers)), 1);
        assert_eq!(pick_effort(AgentTool::Codex, &o, &offers, 2).effort.as_deref(), Some("xhigh"));
        assert_eq!(effort_label("xhigh"), "X-High");
        assert_eq!(effort_label("low"), "Low");
        let modes = vec!["vibe".to_string(), "reviewer".to_string()];
        assert_eq!(pick_agent(&o, &[], &modes, 1).agent.as_deref(), Some("reviewer"));
    }

    /// Pages.cs's OpenCode page (55111fc): the model's own variants, its agents, its
    /// access words and footnote.
    #[test]
    fn opencode_has_its_own_page() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let i = input(&s, &[], &none, &ready);
        let b0 = build(Section::OpenCode, &i);
        let r0 = rows(&b0);
        assert_eq!(r0[1].sub.as_deref(), Some("More models show here once OpenCode has run a task."));
        assert_eq!((r0[2].label.as_str(), &r0[2].control), ("Variant", &Control::Text("None for this model".into())));
        assert_eq!(r0[3].sub.as_deref(), Some("Build, Plan and your own agents show here once OpenCode has run a task."));
        let offers = hover_agents::opencode::offers(
            &hover_core::json::parse(r#"{"providers":[{"id":"p","name":"Prov","models":{"a":{"name":"A","variants":{"low":{},"high":{}}},"m":{"name":"M"}}}]}"#).unwrap(),
            &hover_core::json::parse(r#"[{"name":"build","mode":"primary"},{"name":"plan","mode":"primary"}]"#).unwrap());
        s.set_agent_offers(AgentTool::OpenCode, &offers);
        s.set_agent_options(AgentTool::OpenCode, AgentOptions { model: Some("p/a".into()), effort: Some("high".into()), agent: Some("plan".into()), ..Default::default() });
        let b = build(Section::OpenCode, &i);
        let r = rows(&b);
        let Control::Picker { shown, .. } = &r[1].control else { panic!() };
        assert_eq!(shown, "A · Prov");
        assert_eq!(r[2].control, Control::Segments { id: "OpenCodeEffort".into(), labels: vec!["Low".into(), "High".into()], picked: 1 });
        let Control::Picker { id, shown, options, .. } = &r[3].control else { panic!() };
        assert_eq!((id.as_str(), shown.as_str(), options.len()), ("OpenCodeAgent", "Plan", 3));
        assert!(r[4].sub.as_deref().unwrap().starts_with("OpenCode can edit files and run commands without asking. Deny rules"));
        assert!(matches!(b.last(), Some(Block::Footnote(f)) if f.contains("opencode serve") && f.contains("127.0.0.1")));
        let o = s.agent_options(AgentTool::OpenCode);
        assert_eq!(pick_effort(AgentTool::OpenCode, &o, &offers, 0).effort.as_deref(), Some("low"));
        assert_eq!(pick_opencode_agent(&o, &offers, 1).agent.as_deref(), Some("build"));
        assert_eq!(pick_opencode_agent(&o, &offers, 0).agent, None);
    }
}

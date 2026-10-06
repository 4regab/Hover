//! Owl/Pages.cs (SettingsPage): six sections, each a few headed groups of rows, a
//! label on the left and its control on the right. Built here as data, row for row
//! and string for string, with the C#'s automation ids; ui/settings.slint draws it.
//! Where Windows is named and Linux differs, the Linux words are the nearest ones.

use hover_agents::agents::{self, AgentReady};
use hover_core::model::{AcpOption, AgentOptions, AgentTool, Appearance, SavedTheme, WorkspaceSize};
use hover_core::palette::{InstalledTheme, Palette};
use hover_core::projects::{resolve_folder, CleanupProvider, Project, SpeechMode, VoiceSettings, ACCESS_IDS, GROQ_SECRET, TRANSCRIBE_MODELS};
use hover_core::settings::{Settings, COMPACT_MIN};
use hover_quota::credits::{CreditDay, CreditsView};
use hover_quota::{item, Reading};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Section { General, Integrations, Projects, Voice, Kiro, Codex, Cursor, OpenCode, Claude, Automation }

impl Section {
    pub const ALL: [Section; 10] = [Section::General, Section::Integrations, Section::Projects, Section::Voice, Section::Kiro, Section::Codex, Section::Cursor, Section::OpenCode, Section::Claude, Section::Automation];
    pub fn title(self) -> &'static str { ["General", "Integrations", "Projects", "Voice", "Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Automation"][self as usize] }
    /// The sidebar's icon and its tile's colour.
    pub fn glyph(self) -> (&'static str, Tint) {
        [("settings", Tint::Gray), ("plug", Tint::Purple), ("folder", Tint::Orange), ("mic", Tint::Pink), ("ghost", Tint::Bot), ("terminal", Tint::Green), ("sparkles", Tint::Blue), ("terminal", Tint::Gray), ("sparkles", Tint::Orange), ("calendar", Tint::Teal)][self as usize]
    }
    /// A tool's page shows the tool's own mark (the office's, ui/marks.slint) in place of a
    /// glyph: Claude's spark on its clay tile, Cursor's cube, Codex's, OpenCode's, Kiro's ghost.
    pub fn mark(self) -> Option<&'static str> {
        matches!(self, Section::Kiro | Section::Codex | Section::Cursor | Section::OpenCode | Section::Claude).then(|| self.tool().id())
    }
    /// The section of a tool's own page.
    pub fn of(tool: AgentTool) -> Section {
        match tool { AgentTool::Codex => Section::Codex, AgentTool::Cursor => Section::Cursor, AgentTool::OpenCode => Section::OpenCode, AgentTool::Claude => Section::Claude, AgentTool::Kiro | AgentTool::Custom => Section::Kiro }
    }
    pub fn tool(self) -> AgentTool {
        match self { Section::Codex => AgentTool::Codex, Section::Cursor => AgentTool::Cursor, Section::OpenCode => AgentTool::OpenCode, Section::Claude => AgentTool::Claude, _ => AgentTool::Kiro }
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
    /// A slider for a whole number from `min` to `max`; the new value is told on release (through `picked_seg`'s path, as a number).
    Slider { id: String, name: String, value: i32, min: i32, max: i32 },
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
            Control::Switch { id, .. } | Control::Button { id, .. } | Control::Shortcut { id, .. } | Control::Segments { id, .. } | Control::Slider { id, .. }
            | Control::Picker { id, .. } | Control::Field { id, .. } | Control::Hold { id, .. } => Some(id),
            Control::Chips { open, .. } => open.as_deref(),
            _ => None,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
/// Mark: a tool's own mark, by its id.
pub enum Lead { None, Tile(&'static str, Tint), Ring(Option<f64>), Letter(String, Tint), Mark(&'static str) }

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
    /// Settings → Kiro's credits: its heading with the range, the card, and a line under it.
    Credits(Box<CreditsCard>),
}

/// A day's bar: Hover's and the outside share, each 0..1 of the chart's top.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreditBar { pub label: String, pub hover: f32, pub outside: f32, pub partial: bool, pub tip: String }

/// One of today's dearest sessions: its title, short folder and credits.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TopRow { pub title: String, pub folder: String, pub credits: String }

/// The credits card as text and bar heights, made from the credits view, so the page is
/// tested without Slint. `month_progress` is -1 with no month to show; `note` is the dim
/// line under the card (why Kiro's own total is missing).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreditsCard {
    pub range: i32,
    pub today: String, pub today_sub: String,
    pub week: String, pub week_sub: String,
    pub month_title: String, pub month: String, pub month_progress: f32, pub month_pct: String, pub month_sub: String,
    pub bars: Vec<CreditBar>, pub y_top: String, pub y_mid: String, pub empty: String,
    pub top: Vec<TopRow>, pub top_empty: String,
    pub note: String,
    /// The chart's accessible label: the range summed up.
    pub label: String,
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
    /// Computer use, the sandbox and each agent's one-click setup, as they are now.
    pub integ: Integ,
    /// Saved tasks, the service, custom agents and the registry, as they are now (Automation).
    pub auto: AutoView,
    /// The credits chart's range, an index of CREDITS_RANGES: the page's own choice, not a setting.
    pub credits_range: i32,
}

/// A saved task as Automation lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TaskRow { pub id: String, pub name: String, pub when: String, pub state: String, pub enabled: bool, pub last: String }

/// A custom agent as Automation lists it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomRow { pub id: String, pub name: String, pub status: String, pub ready: bool, pub sign_in: bool }

/// A registry entry as Automation offers it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RegRow { pub id: String, pub name: String, pub note: String, pub can: bool }

/// What only the running app knows for Automation: filled in by it, with the form boxes' drafts.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutoView {
    /// Editors found on this computer: id and name.
    pub editors: Vec<(String, String)>,
    pub tasks: Vec<TaskRow>,
    /// The agents a task can use: provider id and name.
    pub agents: Vec<(String, String)>,
    pub service: String,
    pub service_installed: bool,
    pub timers: String,
    pub webhook: String,
    pub customs: Vec<CustomRow>,
    pub registry: Vec<RegRow>,
    pub registry_note: String,
    /// The text of the "new task" and "new agent" boxes, by their names.
    pub draft: std::collections::HashMap<String, String>,
}

/// What this system can run of the agents' extras; what it can't is switched off, with a note.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Caps { pub sandbox: bool, pub browser: bool, pub setup: bool, pub computer_use: bool, pub mac: bool }

impl Caps {
    pub fn here() -> Caps {
        Caps { sandbox: agents_sandbox::supported(), browser: hover_agents::browser::supported(), setup: hover_agents::setup::supported(),
            computer_use: hover_agents::computer_use::supported(), mac: cfg!(target_os = "macos") }
    }
}

impl Default for Caps { fn default() -> Caps { Caps::here() } }

use hover_agents::sandbox as agents_sandbox;

/// Cua Driver, as Settings shows it: installed, its grants, and a setup going.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cua {
    pub installed: bool,
    pub version: String,
    /// "granted", "partial", "denied" or "unknown".
    pub permissions: String,
    pub hint: String,
    /// Installing or granting: the line it says, and what failed.
    pub busy: bool,
    pub line: String,
    pub error: Option<String>,
}

/// A tool's one-click setup now.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SetupCard { pub busy: bool, pub line: String, pub error: Option<String> }

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Integ {
    pub caps: Caps,
    /// None until the first look (off the UI thread).
    pub cua: Option<Cua>,
    pub setup: Vec<(AgentTool, SetupCard)>,
    /// What the sandbox lacks (srt, ripgrep…), when it is on and can't start yet.
    pub sandbox_missing: Option<String>,
}

impl Integ {
    fn setup_of(&self, t: AgentTool) -> SetupCard { self.setup.iter().find(|s| s.0 == t).map(|s| s.1.clone()).unwrap_or_default() }
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
    /// Kiro's credits by day, as last made off the UI thread; None until the first is.
    pub credits: Option<&'a CreditsView>,
}

const WIN: bool = cfg!(windows);
const MAC: bool = cfg!(target_os = "macos");

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
            heading(&mut b, "Agents");
            extras(&mut b, i);
        }
        Section::Projects => projects(&mut b, i),
        Section::Voice => voice(&mut b, i),
        Section::Automation => automation(&mut b, i),
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


const ACCESS: [(&str, &str); 4] = [("risky", "Ask first"), ("always", "Ask always"), ("read", "Read only"), ("full", "Full access")];

fn btn(id: &str, name: &str, text: &str, enabled: bool) -> Control { Control::Button { id: id.into(), name: name.into(), text: text.into(), enabled } }

fn automation(b: &mut Vec<Block>, i: &Input) {
    let (s, a) = (i.settings, &i.live.auto);
    let d = |k: &str| a.draft.get(k).cloned().unwrap_or_default();
    b.push(Block::Lead("Work that runs by itself, helpers for your agents, and agents of your own. Nothing here starts until you set it up.".into()));

    heading(b, "Where tasks work");
    b.push(Block::Group(vec![row("Work in the project folder itself", Some("By default a task that edits files gets its own Git worktree and branch, so two tasks can’t overwrite each other. Switch this on to work in the folder itself. Read-only tasks, Kiro Web, and folders that aren’t Git projects always do.".into()),
        switch("UseFolder", "Work in the project folder itself", s.automation().use_folder), Lead::Tile("folder", Tint::Blue))]));

    heading(b, "Open in editor");
    let ed = s.editor();
    let mut options = vec![("Ask each time".to_owned(), ed.default.is_none())];
    options.extend(a.editors.iter().map(|(id, n)| (n.clone(), ed.default.as_deref() == Some(id.as_str()))));
    options.push(("Custom program".into(), ed.default.as_deref() == Some("custom")));
    let shown = options.iter().find(|o| o.1).map_or("Ask each time", |o| o.0.as_str()).to_owned();
    let args: Vec<&str> = ed.custom_args.as_deref().unwrap_or("").lines().collect();
    let mut rows = vec![
        row("Default editor", Some("The desk card’s Open in editor button opens the task’s own folder here (a task’s worktree, not the project).".into()), Control::Picker { id: "EditorDefault".into(), name: "Default editor".into(), shown, options }, Lead::Tile("code", Tint::Blue)),
        row("Custom program", Some("Its name, or its full path. Started with the arguments below, one each, with no shell.".into()), field("EditorExe", "Custom editor program", ed.custom_exe.as_deref().unwrap_or(""), "code-insiders"), Lead::None),
    ];
    for n in 1..=4 {
        rows.push(row(format!("Argument {n}"), if n == 1 { Some("{folder}, {file}, {line} and {column} are filled in. An argument that needs a file is left out when none is chosen.".into()) } else { None },
            field(&format!("EditorArg{n}"), &format!("Custom editor argument {n}"), args.get(n - 1).copied().unwrap_or(""), if n == 1 { "{folder}" } else { "" }), Lead::None));
    }
    b.push(Block::Group(rows));

    heading(b, "Helpers");
    let l = s.delegation();
    let pick = |v: u32, all: &[u32]| all.iter().position(|x| *x == v).map_or(-1, |p| p as i32);
    b.push(Block::Group(vec![
        row("Helpers for one task", Some("An agent can ask others for help only in a task where you switch this on. These are your limits.".into()), segments("DelegMax", &["2", "4", "6", "10"], pick(l.max_helpers, &[2, 4, 6, 10])), Lead::Tile("sliders", Tint::Purple)),
        row("Working at once", None, segments("DelegParallel", &["1", "2", "3", "4"], pick(l.max_parallel, &[1, 2, 3, 4])), Lead::None),
        row("Helpers of helpers", Some("How far down a helper may ask for help in turn.".into()), segments("DelegDepth", &["Not at all", "One level", "Two levels"], pick(l.max_depth, &[1, 2, 3])), Lead::None),
    ]));

    heading(b, "Usage limits");
    b.push(Block::Group(vec![row("Continue when a limit lifts", Some("A task that stopped on its agent’s usage limit is continued at the reset time the agent gave, once, with the same access. Without a time from the agent, it only offers a retry.".into()),
        switch("AutoResume", "Continue when a usage limit lifts", s.automation().auto_resume), Lead::Tile("stopwatch", Tint::Orange))]));

    heading(b, "Background service");
    let mut buttons = vec![];
    if a.service_installed { buttons.push(("Service.stop".to_owned(), "Stop".to_owned(), false)); buttons.push(("Service.remove".to_owned(), "Remove".to_owned(), true)); } else { buttons.push(("Service.install".to_owned(), "Install".to_owned(), false)); }
    b.push(Block::Group(vec![row("Keep tasks running when Hover is closed", Some(format!("{}\n{}", a.service, a.timers)), Control::Chips { badges: vec![], buttons, open: None }, Lead::Tile("server", Tint::Gray))]));
    b.push(Block::Footnote("Off until you install it. It runs tasks that never ask for permission; a task that asks waits for Hover to be open, because nobody being at the screen never answers for you. Removing it keeps your tasks and their history.".into()));

    heading(b, "Saved tasks");
    let mut rows: Vec<Row> = a.tasks.iter().map(|t| {
        let buttons = vec![(format!("Task.run.{}", t.id), "Run now".to_owned(), false), (format!("Task.toggle.{}", t.id), if t.enabled { "Pause" } else { "Resume" }.to_owned(), false), (format!("Task.remove.{}", t.id), "Remove".to_owned(), true)];
        row(&t.name, Some(format!("{}\n{}", t.when, t.last)), Control::Chips { badges: vec![(t.state.clone(), !t.enabled)], buttons, open: None }, Lead::Tile("calendar", Tint::Teal))
    }).collect();
    if rows.is_empty() { rows.push(row("No tasks yet", Some("Add one below.".into()), Control::None, Lead::None)); }
    b.push(Block::Group(rows));
    let kind: i32 = d("kind").parse().unwrap_or(0);
    let agent = { let want = d("agent"); a.agents.iter().find(|x| x.0 == want).or(a.agents.first()).cloned() };
    let acc = { let want = d("access"); ACCESS.iter().find(|x| x.0 == want).copied().unwrap_or(ACCESS[0]) };
    let folder = d("folder");
    let mut form = vec![
        row("Name", None, field("TaskName", "Task name", &d("name"), "Nightly check"), Lead::None),
        row("What to do", None, field("TaskPrompt", "What the task does", &d("prompt"), "Run the tests and tell me what failed"), Lead::None),
        row("Folder", None, btn("TaskFolder", "Choose the task’s folder", if folder.is_empty() { "Choose…" } else { &folder }, true), Lead::Tile("folder", Tint::Orange)),
        row("Agent", None, Control::Picker { id: "TaskAgent".into(), name: "Agent".into(), shown: agent.as_ref().map_or("None ready".into(), |x| x.1.clone()), options: a.agents.iter().map(|x| (x.1.clone(), agent.as_ref().is_some_and(|y| y.0 == x.0))).collect() }, Lead::None),
        row("Access", Some("What it may do without asking. Default: ask first.".into()), Control::Picker { id: "TaskAccess".into(), name: "Access".into(), shown: acc.1.into(), options: ACCESS.iter().map(|x| (x.1.to_owned(), x.0 == acc.0)).collect() }, Lead::None),
        row("Runs", None, segments("TaskKind", &["By hand", "Once", "Every", "Daily"], kind), Lead::None),
    ];
    if kind > 0 {
        let (ph, sub) = match kind { 1 => ("2026-11-02 09:30", "Your local date and time."), 2 => ("60", "Minutes between runs (at least 5)."), _ => ("09:00", "Your local time; the task runs at this wall-clock time through clock changes.") };
        form.push(row("When", Some(sub.into()), field("TaskWhen", "When the task runs", &d("when"), ph), Lead::None));
    }
    if kind == 3 { form.push(row("On", None, segments("TaskDays", &["Every day", "Weekdays"], d("days").parse().unwrap_or(0)), Lead::None)); }
    form.push(row("Webhook", Some("Also start it when a webhook calls (set the address below). Fields of what the call sent, by path, separated by commas, go into the prompt; nothing else does.".into()), field("TaskHook", "Webhook fields", &d("hook"), "off, or /pull_request/title, /sender/login"), Lead::None));
    form.push(row("", None, btn("TaskAdd", "Add the task", "Add task", true), Lead::None));
    b.push(Block::Group(form));

    heading(b, "Webhooks");
    b.push(Block::Group(vec![
        row("Listen on", Some(format!("{}\nThis computer only, unless you allow more. Reaching it from the internet (a tunnel or a proxy) is for you to set up.", a.webhook)), field("WebhookAddr", "Webhook address", s.automation().webhook_addr.as_deref().unwrap_or(""), "127.0.0.1:47653"), Lead::Tile("plug", Tint::Purple)),
        row("Allow other computers", Some("Lets it listen beyond this computer. Every call still needs its task’s signature.".into()), switch("WebhookPublic", "Allow other computers", s.automation().webhook_public), Lead::None),
    ]));

    heading(b, "Agents of your own");
    let mut rows: Vec<Row> = a.customs.iter().map(|c| {
        let mut buttons = vec![(format!("Custom.check.{}", c.id), "Check".to_owned(), false)];
        if c.sign_in { buttons.push((format!("Custom.signin.{}", c.id), "Sign in".to_owned(), false)); }
        buttons.push((format!("Custom.remove.{}", c.id), "Remove".to_owned(), true));
        row(&c.name, Some(c.status.clone()), Control::Chips { badges: vec![(if c.ready { "Ready" } else { "Not ready" }.to_owned(), !c.ready)], buttons, open: None }, Lead::Letter(c.name.chars().next().unwrap_or('?').to_string(), Tint::Blue))
    }).collect();
    if rows.is_empty() { rows.push(row("None added", Some("Any program that speaks ACP on its stdio can be added, or found in the ACP Registry below.".into()), Control::None, Lead::None)); }
    b.push(Block::Group(rows));
    let mut form = vec![
        row("Name", None, field("CaName", "Agent name", &d("ca_name"), "My agent"), Lead::None),
        row("Program", Some("Its name, or its full path.".into()), field("CaExe", "Agent program", &d("ca_exe"), "my-agent"), Lead::None),
    ];
    for n in 1..=4 { form.push(row(format!("Argument {n}"), if n == 1 { Some("One argument each; nothing is run through a shell.".into()) } else { None }, field(&format!("CaArg{n}"), &format!("Agent argument {n}"), &d(&format!("ca_arg{n}")), ""), Lead::None)); }
    form.push(row("Environment name", None, field("CaEnvName", "Environment variable name", &d("ca_env_name"), "MY_API_KEY"), Lead::None));
    form.push(row("Environment value", None, field("CaEnvValue", "Environment variable value", &d("ca_env_value"), ""), Lead::None));
    form.push(row("Keep the value secret", Some("Sealed on this computer and never shown again or written to a log.".into()), switch("CaEnvSecret", "Keep the value secret", d("ca_env_secret") == "1"), Lead::None));
    form.push(row("", None, btn("CaAdd", "Add the agent", "Add agent", true), Lead::None));
    b.push(Block::Group(form));

    heading(b, "ACP Registry");
    let mut rows = vec![row("Search", Some(if a.registry_note.is_empty() { "Finds agents other people made. Nothing is downloaded until you press Install.".to_owned() } else { a.registry_note.clone() }), field("RegSearch", "Search the ACP Registry", &d("reg_q"), "gemini"), Lead::Tile("globe", Tint::Blue))];
    for r in &a.registry { rows.push(row(&r.name, Some(r.note.clone()), btn(&format!("Reg.install.{}", r.id), &format!("Install {}", r.name), "Install", r.can), Lead::None)); }
    b.push(Block::Group(rows));
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
        row("Notch shortcut", Some(if WIN { "Click, then press the keys. Include Ctrl, Alt, Shift or Win." } else if MAC { "Click, then press the keys. Include ⌃ Control, ⌥ Option, ⇧ Shift or ⌘ Command." } else { "Click, then press the keys. Include Ctrl, Alt, Shift or Super." }.into()),
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

/// The credits chart's range: its Segments id, and each choice's label and days.
pub const CREDITS_RANGE: &str = "KiroCreditsRange";
pub const CREDITS_RANGES: [(&str, usize); 2] = [("14 days", 14), ("30 days", 30)];

/// The chart's top: the busiest day rounded up to a number whose half is a round one too.
fn nice_top(v: f64) -> f64 {
    if v <= 0.0 { return 1.0; }
    let p = 10f64.powf(v.log10().floor());
    // Rounded, so 3 × 0.1 is 0.3 and not a hair over.
    [1.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0, 10.0].into_iter().map(|f| (f * p * 1e6).round() / 1e6).find(|t| *t >= v - 1e-9).unwrap_or(10.0 * p)
}

/// Which bars carry a day-of-month label: every one of 14; every third of 30, counted
/// back from today, and the first of a month always, its neighbours then left blank.
fn labelled(dates: &[chrono::NaiveDate]) -> Vec<bool> {
    use chrono::Datelike;
    let n = dates.len();
    let every = if n > 14 { 3 } else { 1 };
    let mut on: Vec<bool> = (0..n).map(|i| (n - 1 - i).is_multiple_of(every)).collect();
    for i in (0..n).filter(|&i| dates[i].day() == 1) {
        on[i] = true;
        if every > 1 {
            if i > 0 { on[i - 1] = false; }
            if i + 1 < n { on[i + 1] = false; }
        }
    }
    on
}

/// Settings → Kiro's credits, from the view the app keeps. Kiro's own total (and so
/// Outside) shows only while its quota is on and reads: a total from a reading that now
/// fails would be a stale one. Hover's own numbers always show.
pub fn credits_card(v: Option<&CreditsView>, quota_on: bool, reading: Option<&Reading>, range: i32) -> CreditsCard {
    use chrono::Datelike;
    let blank;
    let v = match v { Some(v) => v, None => { blank = hover_quota::credits::combine(&Default::default(), &[], chrono::Local::now().date_naive()); &blank } };
    let failing = reading.filter(|r| !r.ok());
    let live = quota_on && failing.is_none();
    let total = |d: &CreditDay| d.total.filter(|_| live);
    let outside = |d: &CreditDay| d.outside.filter(|_| live);
    let n2 = |x: f64| format!("{x:.2}");
    let or_dash = |x: Option<f64>| x.map_or("—".to_owned(), n2);
    let range = range.clamp(0, CREDITS_RANGES.len() as i32 - 1);
    let (range_label, n) = CREDITS_RANGES[range as usize];
    let days = &v.days[v.days.len().saturating_sub(n)..];
    let top = nice_top(days.iter().map(|d| d.hover + outside(d).unwrap_or(0.0)).fold(0.0, f64::max));
    let marks = labelled(&days.iter().map(|d| d.date).collect::<Vec<_>>());
    let bars = days.iter().zip(marks).enumerate().map(|(k, (d, on))| {
        let label = if !on { String::new() } else if k == 0 || d.date.day() == 1 { d.date.format("%b %-d").to_string() } else { d.date.day().to_string() };
        let mut tip = format!("{} · Hover {}", d.date.format("%a, %b %-d"), n2(d.hover));
        match (outside(d), total(d)) {
            (Some(o), Some(t)) => tip += &format!(" · Outside {} · Total {}", n2(o), n2(t)),
            _ => tip += " · Kiro total —",
        }
        let partial = live && d.partial;
        if partial { tip += " · partial"; }
        CreditBar { label, hover: (d.hover / top) as f32, outside: (outside(d).unwrap_or(0.0) / top) as f32, partial, tip }
    }).collect();
    let nothing = v.days.iter().all(|d| d.hover <= 0.0 && total(d).is_none());
    let sum = |f: &dyn Fn(&CreditDay) -> Option<f64>| { let k: Vec<f64> = days.iter().filter_map(f).collect(); (!k.is_empty()).then(|| k.iter().sum::<f64>()) };
    let range_hover: f64 = days.iter().map(|d| d.hover).sum();
    let range_days = range_label.split(' ').next().unwrap_or("");
    let label = match sum(&|d| total(d)) {
        Some(t) => format!("Last {range_days} days: {} credits, {} in Hover", n2(t), n2(range_hover)),
        None => format!("Last {range_days} days: {} credits in Hover", n2(range_hover)),
    };
    let month = v.month.as_ref().filter(|_| live);
    let custom = hover_quota::num::custom;
    CreditsCard {
        range,
        today: or_dash(total(&v.today)),
        today_sub: format!("{} Hover", n2(v.today.hover)),
        week: or_dash(v.week_total.filter(|_| live)),
        week_sub: match v.per_day_7.filter(|_| live) { Some(p) => format!("{} a day", n2(p)), None => format!("{} Hover", n2(v.week_hover)) },
        month_title: match month.and_then(|m| m.plan.as_deref()) { Some(p) => format!("This month · {p}"), None => "This month".into() },
        month: month.map_or("—".into(), |m| format!("{} of {}", custom(m.used, 2), custom(m.limit, 2))),
        month_progress: month.map_or(-1.0, |m| (m.used / m.limit).clamp(0.0, 1.0) as f32),
        month_pct: month.map_or(String::new(), |m| format!("{} %", custom((m.used / m.limit * 100.0).clamp(0.0, 100.0), 0))),
        month_sub: month.map_or(String::new(), |m| {
            let mut s = m.reset.as_ref().map(|r| format!("resets {r}")).unwrap_or_default();
            if let Some(out) = v.runs_out { if !s.is_empty() { s += " · "; } s += &format!("out by {}/{}", out.month(), out.day()); }
            s
        }),
        bars,
        y_top: custom(top, 2),
        y_mid: custom(top / 2.0, 2),
        empty: if nothing { "Credits show here once Kiro has run a task.".into() } else { String::new() },
        top: v.top_today.iter().map(|s| TopRow { title: s.title.clone(), folder: hover_office::office::short(&s.folder), credits: hover_chat::state::credits(s.credits) }).collect(),
        top_empty: if v.top_today.is_empty() { "No Kiro tasks in Hover today.".into() } else { String::new() },
        note: if !quota_on { "Switch on the Kiro quota in Integrations to see Kiro’s own total.".into() } else { failing.map(|r| r.detail.clone()).unwrap_or_default() },
        label,
    }
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
        let name = if cfg!(target_os = "macos") { format!("Show {} in the menu bar", item::title(id)) } else { item::title(id).to_owned() };
        let mut r = row(item::title(id), Some(text), switch(&format!("NotchItem{id}"), &name, on), Lead::Ring(ring));
        r.sub_id = Some(format!("QuotaStatus{id}"));
        rows.push(r);
    }
    b.push(Block::Group(rows));
    b.push(Block::Link { id: "RefreshQuotas".into(), name: "Refresh quotas now".into(), icon: "refresh", text: "Refresh quotas now".into(), dim: true, status: String::new() });
    if cfg!(target_os = "macos") { b.push(Block::Footnote(crate::mac::notes::QUOTAS_IN_MENU_BAR.into())); }
    b.push(Block::Footnote("Quotas are read every five minutes: Kiro from \"kiro-cli /usage\", Codex from its own session logs, \
        Cursor from cursor.com and Claude Code from api.anthropic.com, each with the sign-in that tool already keeps. Nothing else is sent.".into()));
}

/// What a Cua Driver status says, as the row under its switch.
pub fn cua_line(c: Option<&Cua>) -> String {
    let Some(c) = c else { return "Checking…".into() };
    if c.busy { return if c.line.is_empty() { "Working…".into() } else { c.line.clone() }; }
    if let Some(e) = &c.error { return e.clone(); }
    if !c.installed { return if c.hint.is_empty() { "Not installed. Hover installs it with Cua’s own installer.".into() } else { format!("Not installed. {}", c.hint) }; }
    let v = if c.version.is_empty() { "Cua Driver".to_owned() } else { format!("Cua Driver {}", c.version) };
    match c.permissions.as_str() {
        "granted" => format!("{v} · Accessibility and Screen Recording are granted."),
        "partial" => format!("{v} · {}", if c.hint.is_empty() { "Screen Recording isn’t granted." } else { c.hint.as_str() }),
        "denied" | "unknown" if cfg!(target_os = "macos") => format!("{v} · {}", if c.hint.is_empty() { "Hover can’t see whether it has its permissions yet." } else { c.hint.as_str() }),
        _ => v,
    }
}

/// Computer use, the sandbox and the agent browser: each a switch, off with its note where
/// this system can't run it.
fn extras(b: &mut Vec<Block>, i: &Input) {
    let s = i.settings;
    let n = &i.live.integ;
    let caps = n.caps;
    // Computer use.
    // Off where it can’t run, whatever the setting says (then no Cua Driver row either).
    let on = caps.computer_use && s.computer_use();
    let mut sub = "Each agent gets Cua Driver’s tools, so it can open the app it built, click through it and check what it shows.".to_owned();
    let mut cu = row("Computer use", None, switch("ComputerUse", "Computer use", on), Lead::Tile("sparkles", Tint::Purple));
    if !caps.computer_use { sub = format!("{}\n{sub}", hover_agents::computer_use::UNSUPPORTED); cu.enabled = false; }
    cu.sub = Some(sub);
    let mut rows = vec![cu];
    if on {
        let cua = n.cua.as_ref();
        let busy = cua.is_some_and(|c| c.busy);
        let mut buttons = vec![];
        if busy { buttons.push(("integ.cua.cancel".to_owned(), "Cancel".to_owned(), false)); }
        else if let Some(c) = cua {
            if !c.installed { buttons.push(("integ.cua.install".to_owned(), "Install".to_owned(), false)); }
            else if caps.mac && !matches!(c.permissions.as_str(), "granted") { buttons.push(("integ.cua.grant".to_owned(), "Grant access…".to_owned(), false)); }
        }
        let badges = match cua { Some(c) if c.installed && c.permissions != "unknown" && !busy => vec![(if matches!(c.permissions.as_str(), "granted" | "partial") { "Ready" } else { "Needs access" }.to_owned(), c.permissions != "granted")], _ => vec![] };
        rows.push(row("Cua Driver", Some(cua_line(cua)), Control::Chips { badges, buttons, open: None }, Lead::Tile("cpu", Tint::Teal)));
    }
    // Agent desktops (Cua Spaces): the Mac app's (its Swift Settings switch them on), so
    // here the switch is only shown off, with why.
    let mut sub = "Each project gets its own desktop, a macOS VM its agents work in instead of your screen. Drag an app or files onto the notch to send them there.".to_owned();
    sub = format!("{}\n{sub}", if caps.mac { "Agent desktops are switched on in Hover for Mac." } else { hover_agents::spaces::UNSUPPORTED });
    let mut ad = row("Agent desktops", Some(sub), switch("AgentSpaces", "Agent desktops", false), Lead::Tile("cpu", Tint::Purple));
    ad.enabled = false;
    rows.push(ad);
    // The sandbox.
    let mut sub = "Agents change only the folders they work in, can’t open windows or control your apps, and reach only their own service, package registries and GitHub. Computer use still works in the background.".to_owned();
    let mut sb = row("Sandbox", None, switch("Sandbox", "Run agents in a sandbox", caps.sandbox && s.sandbox()), Lead::Tile("shield", Tint::Green));
    if !caps.sandbox { sub = format!("{}\n{sub}", hover_agents::sandbox::UNSUPPORTED); sb.enabled = false; }
    else if s.sandbox() { if let Some(m) = &n.sandbox_missing { sub += &format!("\n{m}"); } }
    sb.sub = Some(sub);
    rows.push(sb);
    // The agent browser.
    let mut sub = "Lets the agents open pages in Hover’s own browser, which shows in the desk’s Browser panel. It has no cookies or sign-ins of yours and opens web pages only. It runs outside the sandbox, so it can reach any website; each step follows the agent’s tool access like any other tool.".to_owned();
    let mut br = row("Agent browser", None, switch("AgentBrowser", "Agent browser", caps.browser && s.agent_browser()), Lead::Tile("globe", Tint::Blue));
    if !caps.browser { sub = format!("{}\n{sub}", hover_agents::browser::UNSUPPORTED); br.enabled = false; }
    br.sub = Some(sub);
    rows.push(br);
    // Discord: Hover on the status.
    rows.push(row("Show on Discord", Some("Shows Hover on your Discord status, with how many agents are working and which ones. Task names are never shared. The Discord app has to be open on this computer, and “Share my activity” on in Discord’s Activity Privacy.".into()),
        switch("DiscordPresence", "Show Hover on Discord", s.discord_presence()), Lead::Tile("plug", Tint::Purple)));
    b.push(Block::Group(rows));
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
    b.push(Block::Lead(if v.hold { "Talk to Hover from anywhere. The notch listens while you hold the shortcut." }
        else { "Talk to Hover from anywhere. Press the shortcut to start listening, and press it again to finish." }.into()));
    let mut mics = vec![("System default".to_owned(), v.microphone.is_none())];
    mics.extend(i.live.mics.iter().map(|m| (m.clone(), v.microphone.as_ref() == Some(m))));
    // A saved device that isn't plugged in now still shows as the one picked.
    if let Some(m) = v.microphone.as_ref().filter(|m| !i.live.mics.contains(m)) { mics.push((m.clone(), true)); }
    b.push(Block::Group(vec![
        row("Voice control", None, switch("VoiceEnabled", "Voice control", v.enabled), Lead::None),
        row("Shortcut", Some(i.live.shortcut_error.clone().unwrap_or_else(|| if v.hold { "Hold it to talk, let go to finish. Up to ten minutes." }
            else { "Press it to talk, press it again to finish. Up to ten minutes." }.into())),
            Control::Shortcut { id: "VoiceShortcut".into(), name: "Voice shortcut".into(), text: i.voice_shortcut.clone() }, Lead::None),
        row("Voice Recording Mode", None, segments("VoiceMode", &["Toggle (Click on/off)", "Hold to speak"], v.hold as i32), Lead::None),
        row("Microphone", None, Control::Picker { id: "VoiceMicrophone".into(), name: "Microphone".into(),
            shown: v.microphone.clone().unwrap_or_else(|| "System default".into()), options: mics }, Lead::None),
    ]));
    // The aura on the listening card: one of the offered colours, or any typed in.
    let aura = v.aura();
    let preset = VoiceSettings::AURA_COLORS.iter().find(|c| c.1 == aura);
    b.push(Block::Group(vec![
        row("Aura colour", Some("The light that swirls on the notch while it listens and works on what you said.".into()),
            Control::Picker { id: "VoiceAuraColor".into(), name: "Aura colour".into(), shown: preset.map_or_else(|| "Custom".to_owned(), |c| c.0.to_owned()),
                options: VoiceSettings::AURA_COLORS.iter().map(|c| (c.0.to_owned(), Some(c) == preset)).collect() }, Lead::None),
        row("Custom colour", Some("Any colour, as hex: #1FD5F9.".into()), field("VoiceAuraHex", "Aura colour, hex", aura, VoiceSettings::AURA_COLOR), Lead::None),
    ]));

    heading(b, "Speech recognition");
    let mut language = row("Language", None, Control::Text(if local { "English only" } else { "Detected automatically" }.into()), Lead::None);
    language.enabled = !local;
    let mut rows = vec![
        row("Speech recognition", Some(if local { match crate::phonon::local_note() { Some(n) => n.to_owned(), None => "Speech recognition stays on this computer. English only. For other languages, choose Cloud (Groq).".to_owned() } }
            else { "Audio is sent to Groq. Language is detected automatically.".to_owned() }),
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
    let labels = VoiceSettings::COUNTDOWNS.map(|n| if n == 0 { "Off".to_owned() } else { format!("{n} s") });
    let at = VoiceSettings::COUNTDOWNS.iter().position(|n| *n == v.countdown).map_or(-1, |k| k as i32);
    let wait = if v.countdown == 0 { "The card waits for Start (or Enter).".to_owned() }
        else { format!("The card starts the task {} after it shows it, unless you edit it first.", secs_words(v.countdown)) };
    b.push(Block::Group(vec![
        row("Agent", Some(sub), Control::Picker { id: "VoiceAgentTool".into(), name: "Voice agent".into(), shown: tool.name().into(),
            options: AgentTool::ALL.iter().map(|t| (t.name().to_owned(), *t == tool)).collect() }, Lead::None),
        row("Model", Some(format!("The model, effort and access are {}’s own settings.", tool.name())),
            Control::Button { id: "VoiceAgent".into(), name: format!("Open {} settings", tool.name()), text: format!("{} · {model}", tool.name()), enabled: true }, Lead::None),
        row("Start on its own", Some(wait), segments("VoiceCountdown", &labels.each_ref().map(String::as_str), at), Lead::None),
        row("Default workspace", Some(format!("{place} · {}", access_label(&w.access))),
            Control::Button { id: "VoiceWorkspace".into(), name: "Open Projects".into(), text: "Projects…".into(), enabled: true }, Lead::None),
    ]));

    heading(b, "Try it");
    let t = i.live.voice_try.as_ref();
    let sub = t.and_then(|t| t.error.clone().or_else(|| Some(t.status.clone()).filter(|x| !x.is_empty())))
        .unwrap_or_else(|| if v.hold { "Hold the button and speak. It shows what voice would start; nothing starts and no files are touched." }
        else { "Click the button, speak, then click it again. It shows what voice would start; nothing starts and no files are touched." }.into());
    let mut rows = vec![row("Try it", Some(sub), Control::Hold { id: "voice.try".into(), name: (if v.hold { "Hold to try voice" } else { "Click to try voice" }).into(), text: (if v.hold { "Hold to talk" } else { "Click to talk" }).into() }, Lead::None)];
    if let Some(t) = t { rows.extend(t.lines.iter().map(|(l, v)| row(l, Some(v.clone()), Control::None, Lead::None))); }
    b.push(Block::Group(rows));
    let start = match v.countdown { 0 => "and waits for Start".to_owned(), n => format!("and starts it after {}", secs_words(n)) };
    b.push(Block::Footnote(format!("Use the shortcut, say what to do (“in Hover, fix the notch blink”) and finish. A card shows the folder, agent, access and task, \
        {start}. Enter starts it now, editing the task stops the countdown, and Esc cancels. With a chat open in the office and its reply box open, \
        use the shortcut with the pointer over the chat to write into the reply instead.")));
}

/// "5 seconds", "1 second".
fn secs_words(n: u32) -> String { format!("{n} second{}", if n == 1 { "" } else { "s" }) }

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
    let mut first = vec![row(name, Some(status), Control::Button { id: format!("{id}Recheck"), name: format!("Check {name} again"), text: "Check again".into(), enabled: true },
        Lead::Mark(tool.id()))];
    first.extend(setup_row(tool, ready.as_ref(), i));
    b.push(Block::Group(first));
    let usable = !bad;
    // Only Kiro reports credits, and only kiro-cli tells the account's total.
    if tool == AgentTool::Kiro {
        b.push(Block::Credits(Box::new(credits_card(i.credits, i.settings.has_notch_item(item::KIRO), (i.reading)(item::KIRO).as_ref(), i.live.credits_range))));
        b.push(Block::Footnote("Kiro total is read from \"kiro-cli /usage\" every five minutes while Hover runs. Outside is that total minus Hover’s own tasks: \
            the Kiro IDE, kiro-cli on its own and Kiro Web. A day Hover wasn’t running counts toward the next day it was.".into()));
    }

    heading(b, "Model");
    let models = models(tool, &offers);
    let current = o.model.clone().unwrap_or_else(|| models[0].0.clone());
    let shown = models.iter().find(|m| m.0 == current).map_or(current.clone(), |m| m.1.clone());
    let model = Control::Picker { id: format!("{id}Model"), name: "Model".into(), shown, options: models.iter().map(|m| (m.1.clone(), m.0 == current)).collect() };
    let eff = effort_offer(&offers);
    // OpenCode's variants and Claude Code's efforts belong to each model: only the picked model's are offered.
    let per_model = hover_agents::runtime::per_model_effort(tool);
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
            else if tool == AgentTool::OpenCode { "Your OpenCode providers’ models: API keys, sign-ins and local models. Default is your opencode config’s.".into() }
            else if tool == AgentTool::Claude { "Claude Code’s own models, as your plan or key offers them. Default is its recommended one.".into() }
            else { format!("The first is {name}’s own choice for each task.") }), model, Lead::Tile("brain", Tint::Purple)),
        row(hover_agents::runtime::caps(tool).effort_label, Some(if tool == AgentTool::OpenCode {
            if levels.is_empty() { "Pick a model with variants to choose one. Default leaves it to OpenCode.".into() } else { "The picked model’s own variants, from OpenCode.".into() }
        } else if levels.is_empty() {
            if tool == AgentTool::Cursor { "Cursor’s models carry their effort in their name.".into() }
            else if per_model && has_models { "This model takes no effort setting.".into() }
            else { "Shown once a task has run with a model that takes one.".into() }
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
        "read" if tool == AgentTool::Claude => "Claude Code can only read and search. Its edit and command tools are switched off, and Hover refuses anything else that would change something.".into(),
        "read" => format!("{name} can only read and search. It can’t change files or run commands."),
        // Codex decides what to ask about itself in this mode: its sandbox lets commands
        // inside the folder run, and asks to go past it.
        "risky" if tool == AgentTool::Codex => "Codex asks in the notch before it writes outside the folder or goes online. Inside the folder its sandbox lets it edit and run commands.".into(),
        "risky" if tool == AgentTool::Claude => "Claude Code asks in the notch before it runs a command that changes something, deletes or moves files, goes online or touches anything outside the folder. Reading, editing in the folder and commands it knows only read go ahead.".into(),
        "risky" => format!("{name} asks in the notch before it runs a command, deletes or moves files, goes online or touches anything outside the folder. Reading and editing in the folder go ahead."),
        "always" => format!("{name} asks in the notch before any change or command. Reading and searching go ahead."),
        _ if tool == AgentTool::OpenCode => "OpenCode can edit files and run commands without asking. Deny rules in your OpenCode config still win, and it still asks when it repeats a tool call over and over.".into(),
        _ if tool == AgentTool::Claude => "Claude Code can edit files and run commands without asking. A question it has for you still shows in the notch.".into(),
        _ => format!("{name} can edit files and run commands without asking."),
    } + if ro { "" } else { " Read only isn’t offered, because Codex’s read-only mode needs a sandbox it doesn’t have on Windows." };
    let picked = ["full", "risky", "always", "read"].iter().position(|a| *a == access).unwrap_or(0) as i32;
    rows.push(row("Tool access", Some(text), segments(&format!("{id}Tools"), &labels, picked), Lead::Tile("shield", Tint::Green)));
    rows.push(row("Show the tools it runs", Some(if o.hide_steps { format!("The chat shows only what you asked and {name}’s answers. The steps are still kept.") } else { format!("The chat lists each file {name} reads or edits and each command it runs.") }),
        switch(&format!("{id}ShowSteps"), "Show the tools it runs", !o.hide_steps), Lead::Tile("lines", Tint::Blue)));
    let idle: Vec<String> = AgentOptions::IDLE_CHOICES.iter().map(|m| format!("{m} min")).collect();
    rows.push(row("Keep it running", Some(format!("How long {name} stays open with nothing to do. A reply after that starts it again and picks the conversation back up.")),
        Control::Segments { id: format!("{id}Idle"), labels: idle, picked: AgentOptions::IDLE_CHOICES.iter().position(|m| *m == o.idle_minutes).map_or(-1, |p| p as i32) },
        Lead::Tile("clock", Tint::Gray)));
    if tool == AgentTool::Kiro { rows.extend(compact_rows(&i.settings)); }
    for r in &mut rows { r.enabled = usable; }
    b.push(Block::Group(rows));

    let args = agents::arguments(tool).join(" ");
    if tool == AgentTool::OpenCode {
        b.push(Block::Footnote("OpenCode runs in the background as its own server (\"opencode serve\"), one for all its tasks, on this PC only \
            (127.0.0.1, with a password made for each start), with no terminal window. Your OpenCode providers, agents, skills and MCP servers \
            work as they do in OpenCode. It uses about 0.5 to 1 GB while it runs, so it stops when idle. Changes apply to the next task.".into()));
        return;
    }
    if tool == AgentTool::Claude {
        b.push(Block::Footnote("Claude Code runs in the background in its SDK mode (\"claude --output-format stream-json\"), one process for each \
            conversation in its folder (up to 3 at once), with no terminal window. Your CLAUDE.md, settings, hooks, skills and MCP servers apply \
            as they do in Claude Code. Prompts go to it on its input, never on a command line. Changes apply to the next task.".into()));
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

/// The switch, and once it is on, the share that calls for it (a slider, COMPACT_MIN to 100 %). Kiro only compacts by itself at 100 %.
fn compact_rows(s: &Settings) -> Vec<Row> {
    let on = s.kiro_auto_compact();
    let mut rows = vec![row("Compact automatically", Some("Before the next reply, Hover asks Kiro to compact once its context is this full. Kiro compacts by itself only when it is full.".into()),
        switch("KiroAutoCompact", "Compact automatically", on), Lead::Tile("brain", Tint::Teal))];
    if on {
        let at = s.kiro_compact_at();
        rows.push(row("Compact at", Some(format!("{at} % of the context window. The lowest is {COMPACT_MIN} %.")),
            Control::Slider { id: "KiroCompactAt".into(), name: "Compact at".into(), value: at as i32, min: COMPACT_MIN as i32, max: 100 }, Lead::Tile("gauge", Tint::Teal)));
    }
    rows.push(row("Continue when high usage encountered", Some("When Kiro stops because too many people are using the model, Hover sends “continue” straight away, again and again until it works or you press Stop.".into()),
        switch("KiroRetryBusy", "Continue when high usage encountered", s.kiro_retry_busy()), Lead::Tile("sparkles", Tint::Orange)));
    rows
}

/// The switch was clicked: true when `id` was auto compact's.
pub fn set_compact(s: &Settings, id: &str, on: bool) -> bool {
    if id != "KiroAutoCompact" { return false; }
    s.set_kiro_auto_compact(on);
    true
}

/// A share was set on the slider (a percent; below COMPACT_MIN it is COMPACT_MIN): true when `id` was auto compact's.
pub fn pick_compact_at(s: &Settings, id: &str, percent: usize) -> bool {
    if id != "KiroCompactAt" { return false; }
    s.set_kiro_compact_at(percent.min(100) as u8);
    true
}

/// One click installs a tool with its maker's own installer and opens its sign-in (a Mac's);
/// off, with why, where the system can't do it. None where the tool is ready already.
fn setup_row(tool: AgentTool, ready: Option<&AgentReady>, i: &Input) -> Option<Row> {
    let name = tool.name();
    let n = &i.live.integ;
    let card = n.setup_of(tool);
    let what = format!("Installs {name} if it is missing, with its maker’s own installer, then opens its sign-in.");
    if !n.caps.setup {
        let mut r = row("Set up", Some(format!("{}\n{what}", hover_agents::setup::UNSUPPORTED)),
            Control::Button { id: format!("integ.setup.{}", tool.id()), name: format!("Set up {name}"), text: "Set up".into(), enabled: false }, Lead::Tile("plug", Tint::Blue));
        r.enabled = false;
        return Some(r);
    }
    if ready.is_some_and(|r| r.ok()) && !card.busy && card.error.is_none() { return None; }
    let (text, id) = if card.busy { ("Cancel", format!("integ.setupcancel.{}", tool.id())) } else { ("Set up", format!("integ.setup.{}", tool.id())) };
    let sub = if card.busy { if card.line.is_empty() { "Setting up…".to_owned() } else { card.line.clone() } } else if let Some(e) = &card.error { e.clone() } else { what };
    Some(row("Set up", Some(sub), Control::Button { id, name: format!("{text} {name}"), text: text.into(), enabled: true }, Lead::Tile("plug", Tint::Blue)))
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

/// The efforts Settings offers: the tool's own list, or for OpenCode and Claude Code
/// the picked model's own (Claude Code's Default is its first model).
pub fn effort_levels(tool: AgentTool, o: &AgentOptions, offers: &[AcpOption]) -> Vec<String> {
    if hover_agents::runtime::per_model_effort(tool) {
        let Some(m) = offer(offers, "model", &["model"]) else { return vec![] };
        let picked = m.choices.iter().find(|c| Some(&c.value) == o.model.as_ref())
            .or_else(|| m.choices.first().filter(|_| o.model.is_none() && tool == AgentTool::Claude));
        return picked.and_then(|c| c.levels.clone()).unwrap_or_default();
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
            voice_shortcut: s.voice().shortcut.label(), has_secret: &no, secrets_kept: true, project: None, note: None, live: LIVE.get_or_init(Live::default), credits: None }
    }

    fn settings() -> std::sync::Arc<Settings> {
        let d = std::env::temp_dir().join(format!("hover-pages-{}", hover_core::guid_n()));
        std::fs::create_dir_all(&d).unwrap();
        Settings::load(d.join("settings.json"))
    }

    fn rows_all(b: &[Block]) -> Vec<&Row> { b.iter().filter_map(|x| if let Block::Group(r) = x { Some(r) } else { None }).flatten().collect() }

    /// The rows of the page, less the one-click setup row every agent page has (its own tests look at it).
    fn rows(b: &[Block]) -> Vec<&Row> { rows_all(b).into_iter().filter(|r| r.label != "Set up").collect() }

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

    /// Kiro's auto compact: a switch, off; its share appears once it is on, and a click
    /// reaches settings.json. No other tool has it.
    #[test]
    fn kiros_page_has_auto_compact_off_until_switched_on() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let i = input(&s, &[], &none, &ready);
        let k = build(Section::Kiro, &i);
        let row = rows(&k).into_iter().find(|r| r.label == "Compact automatically").expect("the switch");
        assert!(matches!(&row.control, Control::Switch { id, on: false, .. } if id == "KiroAutoCompact"));
        assert_eq!(row.sub.as_deref(), Some("Before the next reply, Hover asks Kiro to compact once its context is this full. Kiro compacts by itself only when it is full."));
        assert!(!ids(&k).iter().any(|x| x.starts_with("KiroCompactAt")), "the share waits for the switch");
        assert!(set_compact(&s, "KiroAutoCompact", true) && s.kiro_auto_compact());
        let k = build(Section::Kiro, &i);
        let at = rows(&k).into_iter().find(|r| r.label == "Compact at").expect("the share");
        assert!(matches!(&at.control, Control::Slider { id, value: 80, min: 20, max: 100, .. } if id == "KiroCompactAt"));
        assert!(pick_compact_at(&s, "KiroCompactAt", 35) && s.kiro_compact_at() == 35);
        let k = build(Section::Kiro, &i);
        assert!(matches!(&rows(&k).into_iter().find(|r| r.label == "Compact at").unwrap().control, Control::Slider { value: 35, .. }));
        // The slider cannot go under 20 %, nor over 100 %.
        assert!(pick_compact_at(&s, "KiroCompactAt", 3) && s.kiro_compact_at() == 20);
        assert!(pick_compact_at(&s, "KiroCompactAt", 400) && s.kiro_compact_at() == 100);
        assert!(!set_compact(&s, "Sandbox", true) && !pick_compact_at(&s, "KiroIdle", 0));
        for other in [Section::Codex, Section::Cursor, Section::OpenCode, Section::Claude] {
            assert!(!ids(&build(other, &i)).iter().any(|x| x.contains("Compact")), "{other:?}");
        }
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
        let kb = build(Section::Kiro, &i);
        assert!(kb.iter().any(|x| matches!(x, Block::Credits(_))), "Kiro's page has its credits");
        let k = ids(&kb);
        for id in ["KiroRecheck", "KiroModel", "KiroAgent", "KiroToolsFull", "KiroToolsRead only", "KiroShowSteps", "KiroIdle5 min", "KiroIdle15 min", "SettingsKiroFolder", "KiroNoticeAgain"] {
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
        // Not ready: the hint under the tool's own mark, and the rest greyed.
        let c = build(Section::Cursor, &i);
        let r = rows(&c);
        assert_eq!((r[0].sub.as_deref(), &r[0].lead), (Some("Install the Cursor CLI."), &Lead::Mark("cursor")));
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
        for id in ["VoiceEnabled", "VoiceShortcut", "VoiceModeToggle (Click on/off)", "VoiceMicrophone", "VoiceSpeechLocal (Phonon)", "VoiceGroqKey", "groq.check", "VoiceModel", "VoiceCleanup", "VoiceCleanupKey", "VoiceAgentTool", "VoiceAgent", "voice.try"] {
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

    /// Claude Code's page: its models with each one's efforts (Default's first), its
    /// read only, and how it runs.
    #[test]
    fn voice_starts_after_five_seconds_until_set_otherwise() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let row = |s: &Settings| {
            let i = input(s, &[], &none, &ready);
            let b = build(Section::Voice, &i);
            let r = rows(&b).into_iter().find(|r| r.label == "Start on its own").cloned().expect("the countdown's row");
            let foot = b.iter().rev().find_map(|x| if let Block::Footnote(f) = x { Some(f.clone()) } else { None }).unwrap();
            (r, foot)
        };
        let (r, foot) = row(&s);
        let labels = vec!["Off".to_string(), "3 s".into(), "5 s".into(), "10 s".into()];
        assert_eq!(r.control, Control::Segments { id: "VoiceCountdown".into(), labels: labels.clone(), picked: 2 });
        assert!(foot.contains("starts it after 5 seconds") && foot.contains("reply"), "{foot}");
        s.set_voice(VoiceSettings { countdown: 0, ..s.voice() });
        let (r, foot) = row(&s);
        assert_eq!(r.control, Control::Segments { id: "VoiceCountdown".into(), labels, picked: 0 });
        assert_eq!(r.sub.as_deref(), Some("The card waits for Start (or Enter)."));
        assert!(foot.contains("waits for Start"), "{foot}");
    }

    #[test]
    fn claude_code_has_its_own_page() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let i = input(&s, &[], &none, &ready);
        assert_eq!((Section::of(AgentTool::Claude), Section::Claude.tool(), Section::Claude.title()), (Section::Claude, AgentTool::Claude, "Claude Code"));
        let b0 = build(Section::Claude, &i);
        let r0 = rows(&b0);
        assert_eq!(r0[1].sub.as_deref(), Some("More models show here once Claude Code has run a task."));
        assert_eq!((r0[2].label.as_str(), &r0[2].control), ("Effort", &Control::Text("None for this model".into())));
        let levels = |l: &[&str]| Some(l.iter().map(|x| x.to_string()).collect());
        let offers = vec![AcpOption { id: "model".into(), category: Some("model".into()), current: None, choices: vec![
            AcpChoice { value: "default".into(), name: "Default (recommended)".into(), levels: levels(&["low", "high", "xhigh"]) },
            AcpChoice { value: "haiku".into(), name: "Haiku".into(), levels: levels(&[]) }] }];
        s.set_agent_offers(AgentTool::Claude, &offers);
        s.set_agent_options(AgentTool::Claude, AgentOptions { read_only: true, ..Default::default() });
        let b = build(Section::Claude, &i);
        let r = rows(&b);
        let Control::Picker { shown, options, .. } = &r[1].control else { panic!() };
        assert_eq!((shown.as_str(), options.len()), ("Default (recommended)", 2), "Default is its own first model, not one Hover adds");
        assert_eq!(r[2].control, Control::Segments { id: "Claude CodeEffort".into(), labels: vec!["Low".into(), "High".into(), "X-High".into()], picked: 0 });
        assert!(r[3].sub.as_deref().unwrap().starts_with("Claude Code can only read and search. Its edit and command tools are switched off"));
        assert!(matches!(b.last(), Some(Block::Footnote(f)) if f.contains("stream-json") && f.contains("CLAUDE.md")));
        s.set_agent_options(AgentTool::Claude, AgentOptions { model: Some("haiku".into()), ..Default::default() });
        let b = build(Section::Claude, &i);
        let r = rows(&b);
        assert_eq!((&r[2].control, r[2].sub.as_deref()), (&Control::Text("None for this model".into()), Some("This model takes no effort setting.")));
        let o = AgentOptions::default();
        assert_eq!(pick_effort(AgentTool::Claude, &o, &offers, 2).effort.as_deref(), Some("xhigh"));
        assert_eq!(pick_model(AgentTool::Claude, &o, &offers, 0).model, None, "Default sends no model");
    }

    // MARK: Kiro's credits

    use hover_core::ledger::{DayA, SessionCredits};
    use hover_quota::daily::Day;

    fn date(m: u32, d: u32) -> chrono::NaiveDate { chrono::NaiveDate::from_ymd_opt(2026, m, d).unwrap() }

    /// Three days of readings and Hover's share of the last two, as of Oct 6.
    fn view() -> CreditsView {
        let day = |d: u32, first: f64, used: f64| Day { date: date(10, d), first, used, limit: 50.0, reset: Some("10/20".into()), plan: Some("KIRO PRO".into()), at: String::new() };
        let mut a = std::collections::BTreeMap::new();
        a.insert(date(10, 5), DayA { credits: 1.0, turns: 1, sessions: vec![] });
        a.insert(date(10, 6), DayA { credits: 2.1, turns: 3, sessions: vec![
            SessionCredits { key: "a".into(), title: "Fix login redirect".into(), folder: r"C:\work\Hover\app".into(), credits: 1.2 },
            SessionCredits { key: "b".into(), title: "Add CSV export".into(), folder: "/home/me/billing-svc".into(), credits: 0.64 },
        ] });
        hover_quota::credits::combine(&a, &[day(4, 30.0, 35.0), day(5, 35.0, 37.5), day(6, 37.5, 41.0)], date(10, 6))
    }

    fn card(b: &[Block]) -> Option<&CreditsCard> { b.iter().find_map(|x| if let Block::Credits(c) = x { Some(&**c) } else { None }) }

    #[test]
    fn kiros_credits_come_after_its_status_and_before_the_model_and_only_on_kiros_page() {
        let s = settings();
        let none = |_: &str| None;
        let ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let i = input(&s, &[], &none, &ready);
        let k = build(Section::Kiro, &i);
        let at = k.iter().position(|x| matches!(x, Block::Credits(_))).unwrap();
        assert!(matches!(&k[at - 1], Block::Group(r) if r[0].label == "Kiro"), "right under the installed-and-signed-in group");
        assert!(matches!(&k[at + 1], Block::Footnote(t) if t.starts_with("Kiro total is read from")), "its footnote under the card");
        assert_eq!(k[at + 2], Block::Heading("MODEL".into(), false), "then the Model heading");
        for sec in [Section::Codex, Section::Cursor, Section::OpenCode, Section::Claude] {
            let b = build(sec, &i);
            assert!(card(&b).is_none(), "{sec:?} has no credits");
            assert!(matches!(&b[2], Block::Heading(h, _) if h == "MODEL"), "{sec:?} goes from its status to the Model heading as before");
        }
    }

    #[test]
    fn the_card_shows_kiros_total_hovers_share_and_the_month_while_the_quota_reads() {
        let v = view();
        let ok = Reading { used: Some(82.0), detail: "KIRO PRO · 41 of 50 credits · resets 10/20".into() };
        let c = credits_card(Some(&v), true, Some(&ok), 0);
        assert_eq!((c.today.as_str(), c.today_sub.as_str()), ("3.50", "2.10 Hover"));
        // Oct 4 is the first day on file (5 of its own), Oct 5 2.5, Oct 6 3.5: 11 over 3 days.
        assert_eq!((c.week.as_str(), c.week_sub.as_str()), ("11.00", "3.67 a day"));
        assert_eq!((c.month_title.as_str(), c.month.as_str(), c.month_pct.as_str()), ("This month · KIRO PRO", "41 of 50", "82 %"));
        assert!((c.month_progress - 0.82).abs() < 1e-6);
        // Since 9/20, 17 days at 41: 2.41 a day, so the 9 left last 4 days.
        assert_eq!(c.month_sub, "resets 10/20 · out by 10/10");
        assert_eq!(c.note, "");
        assert_eq!(c.label, "Last 14 days: 11.00 credits, 3.10 in Hover");
        assert_eq!(c.bars.len(), 14);
        let today = c.bars.last().unwrap();
        // Oct 4's 5 is the most: the chart's top is 5.
        assert_eq!((c.y_top.as_str(), c.y_mid.as_str()), ("5", "2.5"));
        assert!((today.hover - 2.1 / 5.0).abs() < 1e-6 && (today.outside - 1.4 / 5.0).abs() < 1e-6);
        assert_eq!(today.tip, "Tue, Oct 6 · Hover 2.10 · Outside 1.40 · Total 3.50");
        assert_eq!(c.bars[11].tip, "Sun, Oct 4 · Hover 0.00 · Outside 5.00 · Total 5.00 · partial");
        assert!(c.bars[11].partial && !today.partial);
        // 14 days: each labelled, the first with its month, as is the first of a month.
        assert_eq!((c.bars[0].label.as_str(), c.bars[13].label.as_str()), ("Sep 23", "6"));
        assert_eq!(c.bars.iter().find(|b| b.tip.starts_with("Thu, Oct 1")).unwrap().label, "Oct 1");
        assert_eq!(c.top, [TopRow { title: "Fix login redirect".into(), folder: "app".into(), credits: "1.20 credits".into() },
            TopRow { title: "Add CSV export".into(), folder: "billing-svc".into(), credits: "0.64 credits".into() }]);
        assert_eq!((c.empty.as_str(), c.top_empty.as_str()), ("", ""));
        // 30 days: every third day labelled, counted back from today.
        let c = credits_card(Some(&v), true, Some(&ok), 1);
        assert_eq!(c.bars.len(), 30);
        assert_eq!(c.bars.iter().filter(|b| !b.label.is_empty()).count(), 10);
        assert_eq!((c.bars[29].label.as_str(), c.bars[28].label.as_str()), ("6", ""));
        assert_eq!(c.label, "Last 30 days: 11.00 credits, 3.10 in Hover");
    }

    #[test]
    fn with_the_quota_failing_or_off_kiros_total_is_a_dash_and_hovers_numbers_stand() {
        let v = view();
        let fail = Reading::fail("kiro-cli isn’t installed or isn’t on PATH.");
        let c = credits_card(Some(&v), true, Some(&fail), 0);
        assert_eq!((c.today.as_str(), c.today_sub.as_str(), c.week.as_str(), c.week_sub.as_str()), ("—", "2.10 Hover", "—", "3.10 Hover"));
        assert_eq!((c.month.as_str(), c.month_progress, c.month_sub.as_str()), ("—", -1.0, ""));
        assert_eq!(c.note, "kiro-cli isn’t installed or isn’t on PATH.");
        assert!(c.bars.iter().all(|b| b.outside == 0.0 && !b.partial), "Hover-only bars");
        assert_eq!(c.bars[13].tip, "Tue, Oct 6 · Hover 2.10 · Kiro total —");
        assert_eq!(c.label, "Last 14 days: 3.10 credits in Hover");
        // The top is Hover's busiest day now: 2.1 rounds up to 3.
        assert_eq!(c.y_top, "3");
        let off = credits_card(Some(&v), false, None, 0);
        assert_eq!((off.today.as_str(), off.note.as_str()), ("—", "Switch on the Kiro quota in Integrations to see Kiro’s own total."));
        // Through the page: the reading Settings has is the one shown.
        let s = settings();
        s.set_notch_item("kiro", true);
        let reading = |id: &str| (id == "kiro").then(|| Reading::fail("Run “kiro-cli login” first."));
        let ready = |_| None;
        let mut i = input(&s, &[], &reading, &ready);
        i.credits = Some(&v);
        let b = build(Section::Kiro, &i);
        let c = card(&b).unwrap();
        assert_eq!((c.today.as_str(), c.note.as_str()), ("—", "Run “kiro-cli login” first."));
    }

    #[test]
    fn with_no_data_the_chart_is_empty_and_says_why() {
        for v in [None, Some(hover_quota::credits::combine(&Default::default(), &[], date(10, 6)))] {
            let c = credits_card(v.as_ref(), true, None, 0);
            assert_eq!(c.empty, "Credits show here once Kiro has run a task.");
            assert_eq!(c.top_empty, "No Kiro tasks in Hover today.");
            assert!(c.top.is_empty() && c.bars.iter().all(|b| b.hover == 0.0 && b.outside == 0.0));
            assert_eq!((c.today.as_str(), c.today_sub.as_str(), c.y_top.as_str()), ("—", "0.00 Hover", "1"));
        }
    }

    #[test]
    fn the_charts_top_is_a_round_number_with_a_round_half() {
        assert_eq!([0.0, 0.3, 1.0, 2.1, 3.5, 5.2, 7.0, 9.0, 13.0, 41.0].map(nice_top), [1.0, 0.3, 1.0, 3.0, 4.0, 6.0, 8.0, 10.0, 20.0, 50.0]);
    }

    // MARK: Integrations

    fn with_live<'a>(s: &'a Settings, live: &'a Live, ready: &'a dyn Fn(AgentTool) -> Option<AgentReady>) -> Input<'a> {
        fn no(_: &str) -> bool { false }
        static NONE: fn(&str) -> Option<Reading> = |_| None;
        Input { settings: s, launch_at_login: false, shortcut: s.sc_workspace().label(), reading: &NONE, ready, installed: &[], system_dark: true, import_status: String::new(), kiro_agents: vec![],
            voice_shortcut: s.voice().shortcut.label(), has_secret: &no, secrets_kept: true, project: None, note: None, live, credits: None }
    }

    fn row_of<'a>(b: &'a [Block], label: &str) -> Option<&'a Row> { rows_all(b).into_iter().find(|r| r.label == label) }

    fn on_a(mac: bool) -> Caps { Caps { sandbox: mac, browser: mac, setup: mac, computer_use: mac, mac } }

    #[test]
    fn computer_use_the_sandbox_and_the_agent_browser_are_switches_in_integrations() {
        let s = settings();
        s.set_computer_use(true);
        let live = Live { integ: Integ { caps: on_a(true), ..Default::default() }, ..Default::default() };
        let ready = |_| None;
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        for (label, id, on) in [("Computer use", "ComputerUse", true), ("Sandbox", "Sandbox", true), ("Agent browser", "AgentBrowser", true)] {
            let r = row_of(&b, label).unwrap_or_else(|| panic!("{label} in {:?}", rows(&b).iter().map(|r| &r.label).collect::<Vec<_>>()));
            assert!(matches!(&r.control, Control::Switch { id: i, on: o, .. } if i == id && *o == on), "{label}: {:?}", r.control);
            assert!(r.enabled, "{label} is on where the system runs it");
        }
        // Set off, they read off.
        s.set_sandbox(false);
        s.set_agent_browser(false);
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        assert!(matches!(&row_of(&b, "Sandbox").unwrap().control, Control::Switch { on: false, .. }));
        assert!(matches!(&row_of(&b, "Agent browser").unwrap().control, Control::Switch { on: false, .. }));
    }

    #[test]
    fn what_the_system_cannot_run_is_switched_off_with_its_note() {
        let s = settings();
        // Set on, and still off where it can't run.
        s.set_sandbox(true);
        s.set_agent_browser(true);
        let live = Live { integ: Integ { caps: on_a(false), ..Default::default() }, ..Default::default() };
        let ready = |_| None;
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        let sb = row_of(&b, "Sandbox").unwrap();
        assert!(!sb.enabled && matches!(&sb.control, Control::Switch { on: false, .. }));
        assert!(sb.sub.as_deref().unwrap().starts_with("The sandbox needs macOS or Linux."), "{:?}", sb.sub);
        let br = row_of(&b, "Agent browser").unwrap();
        assert!(!br.enabled && matches!(&br.control, Control::Switch { on: false, .. }));
        assert!(br.sub.as_deref().unwrap().starts_with("Agent browser needs macOS."), "{:?}", br.sub);
        // Computer use is a Mac’s: set on, it reads off with its note and shows no Cua Driver row.
        s.set_computer_use(true);
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        let cu = row_of(&b, "Computer use").unwrap();
        assert!(!cu.enabled && matches!(&cu.control, Control::Switch { on: false, .. }));
        assert!(cu.sub.as_deref().unwrap().starts_with("Computer use needs macOS.\nEach agent gets"), "{:?}", cu.sub);
        assert!(row_of(&b, "Cua Driver").is_none());
        // Agent desktops are the Mac's too: off, with its note.
        let ad = row_of(&b, "Agent desktops").unwrap();
        assert!(!ad.enabled && matches!(&ad.control, Control::Switch { on: false, .. }));
        assert!(ad.sub.as_deref().unwrap().starts_with("Agent desktops need macOS 26 or later on Apple silicon.\n"), "{:?}", ad.sub);
    }

    #[test]
    fn the_sandbox_says_what_it_lacks_when_it_is_on() {
        let s = settings();
        let missing = "Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78.";
        let live = Live { integ: Integ { caps: on_a(true), sandbox_missing: Some(missing.into()), ..Default::default() }, ..Default::default() };
        let ready = |_| None;
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        assert!(row_of(&b, "Sandbox").unwrap().sub.as_deref().unwrap().ends_with(missing));
        s.set_sandbox(false);
        let b = build(Section::Integrations, &with_live(&s, &live, &ready));
        assert!(!row_of(&b, "Sandbox").unwrap().sub.as_deref().unwrap().contains("isn’t set up yet"), "off: nothing to set up");
    }

    #[test]
    fn cua_driver_offers_install_cancel_or_grant_as_it_stands() {
        let s = settings();
        let ready = |_| None;
        let buttons = |live: &Live, s: &Settings| -> Vec<String> {
            let b = build(Section::Integrations, &with_live(s, live, &ready));
            match &row_of(&b, "Cua Driver").map(|r| &r.control) { Some(Control::Chips { buttons, .. }) => buttons.iter().map(|b| b.0.clone()).collect(), _ => vec!["(no row)".into()] }
        };
        let mk = |c: Option<Cua>, mac: bool| Live { integ: Integ { caps: on_a(mac), cua: c, ..Default::default() }, ..Default::default() };
        // Off: no card at all.
        assert_eq!(buttons(&mk(None, true), &s), ["(no row)"]);
        s.set_computer_use(true);
        assert!(buttons(&mk(None, true), &s).is_empty(), "checking: nothing to press yet");
        assert_eq!(buttons(&mk(Some(Cua::default()), true), &s), ["integ.cua.install"]);
        assert_eq!(buttons(&mk(Some(Cua { busy: true, ..Default::default() }), true), &s), ["integ.cua.cancel"]);
        let partial = Cua { installed: true, permissions: "partial".into(), ..Default::default() };
        assert_eq!(buttons(&mk(Some(partial.clone()), true), &s), ["integ.cua.grant"]);
        assert_eq!(buttons(&mk(Some(partial), false), &s), ["(no row)"], "no Cua Driver off a Mac");
        assert!(buttons(&mk(Some(Cua { installed: true, permissions: "granted".into(), ..Default::default() }), true), &s).is_empty());
    }

    #[test]
    fn cua_drivers_line_says_what_is_true() {
        assert_eq!(cua_line(None), "Checking…");
        assert!(cua_line(Some(&Cua::default())).starts_with("Not installed."));
        assert_eq!(cua_line(Some(&Cua { busy: true, line: "Installing Cua Driver…".into(), ..Default::default() })), "Installing Cua Driver…");
        assert_eq!(cua_line(Some(&Cua { error: Some("The installer failed.".into()), ..Default::default() })), "The installer failed.");
        let ok = Cua { installed: true, version: "0.3.1".into(), permissions: "granted".into(), ..Default::default() };
        assert_eq!(cua_line(Some(&ok)), "Cua Driver 0.3.1 · Accessibility and Screen Recording are granted.");
    }

    #[test]
    fn each_agent_page_has_its_setup_row_on_a_mac_and_a_note_elsewhere() {
        let s = settings();
        let not_ready = |_| Some(AgentReady { installed: false, signed_in: false, hint: "Install kiro-cli.".into() });
        let all_ready = |_| Some(AgentReady { installed: true, signed_in: true, hint: String::new() });
        let setup_row = |live: &Live, ready: &dyn Fn(AgentTool) -> Option<AgentReady>| -> Option<Row> {
            let b = build(Section::Kiro, &with_live(&s, live, ready));
            row_of(&b, "Set up").cloned()
        };
        // Elsewhere: off, with the note, on every agent's page.
        let off = Live { integ: Integ { caps: on_a(false), ..Default::default() }, ..Default::default() };
        let r = setup_row(&off, &all_ready).expect("the row shows even for a tool that is ready");
        assert!(!r.enabled && r.sub.as_deref().unwrap().starts_with("One-click setup is available on macOS."), "{:?}", r.sub);
        for t in AgentTool::ALL {
            let b = build(Section::of(t), &with_live(&s, &off, &all_ready));
            assert!(row_of(&b, "Set up").is_some_and(|r| !r.enabled), "{t:?}");
        }
        // On a Mac: a button while the tool isn't ready, Cancel while it goes, the error after, nothing when ready.
        let mac = Live { integ: Integ { caps: on_a(true), ..Default::default() }, ..Default::default() };
        let r = setup_row(&mac, &not_ready).unwrap();
        assert!(r.enabled && matches!(&r.control, Control::Button { id, text, enabled: true, .. } if id == "integ.setup.kiro" && text == "Set up"));
        let going = Live { integ: Integ { caps: on_a(true), setup: vec![(AgentTool::Kiro, SetupCard { busy: true, line: "Installing kiro-cli…".into(), error: None })], ..Default::default() }, ..Default::default() };
        let r = setup_row(&going, &not_ready).unwrap();
        assert!(matches!(&r.control, Control::Button { id, text, .. } if id == "integ.setupcancel.kiro" && text == "Cancel") && r.sub.as_deref() == Some("Installing kiro-cli…"));
        let failed = Live { integ: Integ { caps: on_a(true), setup: vec![(AgentTool::Kiro, SetupCard { busy: false, line: String::new(), error: Some("The installer exited with 1.".into()) })], ..Default::default() }, ..Default::default() };
        assert_eq!(setup_row(&failed, &all_ready).unwrap().sub.as_deref(), Some("The installer exited with 1."));
        assert!(setup_row(&mac, &all_ready).is_none(), "ready: nothing to set up");
    }

    #[test]
    fn local_speech_says_why_it_is_off_on_a_mac() {
        let s = settings();
        s.set_voice(VoiceSettings { speech: SpeechMode::Local, ..s.voice() });
        let ready = |_| None;
        let live = Live::default();
        let b = build(Section::Voice, &with_live(&s, &live, &ready));
        let r = row_of(&b, "Speech recognition").unwrap();
        let sub = r.sub.as_deref().unwrap();
        assert_eq!(sub.contains("isn't available on macOS") || sub.contains("isn’t available on macOS"), cfg!(target_os = "macos"), "{sub}");
    }
}

//! The windows' shared view state: the palette in use (Theme), Settings over the office
//! (OfficeView, SettingsPage) and what each click in it does (Pages.cs's handlers).
//! Every window with an office gets the same state through its own Slint globals.

use crate::ui::*;
use hover_app::keys::{self, Recorded};
use hover_app::pages::{self, Block as B, Control, Lead, Section, Tint};
use hover_core::model::{AgentTool, SavedTheme};
use hover_core::palette::{InstalledTheme, Palette};
use hover_core::platform::Autostart;
use hover_core::projects::{resolve_folder, CleanupProvider, Project, SpeechMode, VoiceSettings, Workspace, ACCESS_IDS, GROQ_SECRET, TRANSCRIBE_MODELS};
use slint::{Color, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::rc::Rc;

pub fn argb(c: u32) -> Color { Color::from_argb_u8((c >> 24) as u8, (c >> 16) as u8, (c >> 8) as u8, c as u8) }

/// Theme.Publish: the palette's colours into a window's Pal global.
macro_rules! publish {
    ($w:expr, $p:expr, $motion:expr) => {{
        let g = $w.global::<crate::ui::Pal>();
        let p = $p;
        g.set_dark(p.dark);
        g.set_ink(crate::view::argb(p.ink)); g.set_ink_dim(crate::view::argb(p.ink_dim)); g.set_ink_faint(crate::view::argb(p.ink_faint));
        g.set_fill(crate::view::argb(p.fill)); g.set_wash(crate::view::argb(p.wash)); g.set_wash_strong(crate::view::argb(p.wash_strong));
        g.set_separator(crate::view::argb(p.separator)); g.set_surface(crate::view::argb(p.surface)); g.set_sheet(crate::view::argb(p.sheet));
        g.set_sheet_edge(crate::view::argb(p.sheet_edge)); g.set_panel(crate::view::argb(p.panel)); g.set_panel_edge(crate::view::argb(p.panel_edge));
        g.set_thumb(crate::view::argb(p.thumb)); g.set_row_hover(crate::view::argb(p.row_hover)); g.set_switch_off(crate::view::argb(p.switch_off));
        g.set_handle(crate::view::argb(p.handle)); g.set_blue(crate::view::argb(p.blue)); g.set_green(crate::view::argb(p.green)); g.set_purple(crate::view::argb(p.purple));
        g.set_yellow(crate::view::argb(p.yellow)); g.set_teal(crate::view::argb(p.teal)); g.set_orange(crate::view::argb(p.orange)); g.set_red(crate::view::argb(p.red));
        g.set_motion($motion);
    }};
}
pub(crate) use publish;

/// The picker's menu while open: the row that asked, its options, where.
pub type OpenMenu = (String, Vec<(String, bool)>, f32, f32);

/// Page's state that isn't in the settings.
pub struct Pane {
    pub section: Section,
    pub recording: bool,
    /// The voice shortcut is the one recording, not the notch's.
    pub recording_voice: bool,
    /// The shortcut field's words while recording ("Press keys…", the modifier hint).
    pub field: Option<String>,
    pub import_status: String,
    pub menu: Option<OpenMenu>,
    /// Palette.Installed with each theme read, once per run (as the C#'s Lazy).
    pub installed: Option<Rc<Vec<(InstalledTheme, SavedTheme)>>>,
    /// The project open in Projects (its id).
    pub project: Option<String>,
    /// The last action's message, by the id of its control (pages::Input::note).
    pub note: Option<(String, String)>,
    /// What the running app knows about voice; the app sets it and calls refresh.
    pub live: pages::Live,
    /// The key under the finger, by position (winit's `KeyCode`), while a shortcut records
    /// on a Mac: Option-N types a dead key that the character alone can't name.
    pub physical: Option<String>,
    /// The text of Automation's "new task" and "new agent" boxes, until they are added.
    pub draft: std::collections::HashMap<String, String>,
}

impl Default for Pane {
    fn default() -> Self {
        Pane { section: Section::General, recording: false, recording_voice: false, field: None, import_status: String::new(), menu: None, installed: None,
            project: None, note: None, live: pages::Live::default(), physical: None, draft: Default::default() }
    }
}

/// The one secret store this run (keys kept only in memory live in it): Settings and
/// the voice flow must share it.
pub fn secrets() -> std::sync::Arc<hover_core::secrets::Secrets> {
    static S: std::sync::OnceLock<std::sync::Arc<hover_core::secrets::Secrets>> = std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Arc::new(hover_core::secrets::Secrets::system())).clone()
}

pub fn installed(p: &mut Pane) -> Rc<Vec<(InstalledTheme, SavedTheme)>> {
    p.installed.get_or_insert_with(|| {
        Rc::new(hover_core::palette::installed().into_iter().filter_map(|s| hover_core::palette::read(&s.path, Some(&s.label), Some(s.dark)).map(|t| (s, t))).collect())
    }).clone()
}

pub(crate) fn tint(t: Tint, p: &Palette) -> Color {
    match t {
        Tint::Gray => Color::from_rgb_u8(0x8e, 0x8e, 0x93),
        Tint::Bot => Color::from_rgb_u8(0x9b, 0x6b, 0xff),
        Tint::Purple => argb(p.purple), Tint::Green => argb(p.green), Tint::Blue => argb(p.blue),
        Tint::Orange => argb(p.orange), Tint::Teal => argb(p.teal),
        Tint::Pink => Color::from_rgb_u8(0xff, 0x37, 0x5f),
    }
}

fn s(v: impl AsRef<str>) -> SharedString { SharedString::from(v.as_ref()) }
fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { ModelRc::new(VecModel::from(v)) }

/// The rows of a model already shown, changed in place: a new model makes each repeated
/// element anew, and one made again between a press and its release (the office's tags
/// at 10–30 fps, rows while a session runs) loses the click. Returns the model to set
/// when there is none yet to change.
pub fn sync<T: Clone + PartialEq + 'static>(cur: ModelRc<T>, v: &[T]) -> Option<ModelRc<T>> {
    use slint::Model;
    let Some(m) = cur.as_any().downcast_ref::<VecModel<T>>() else { return Some(model(v.to_vec())) };
    for (i, t) in v.iter().enumerate() {
        if i < m.row_count() { if m.row_data(i).as_ref() != Some(t) { m.set_row_data(i, t.clone()); } } else { m.push(t.clone()); }
    }
    while m.row_count() > v.len() { m.remove(m.row_count() - 1); }
    None
}

/// Settings' blocks, changed in place down to their rows: the page is built again on
/// every change, and a row made anew loses what it holds (the shortcut field's
/// keyboard focus while it records, a press).
pub fn sync_blocks(cur: ModelRc<Block>, v: Vec<Block>) -> Option<ModelRc<Block>> {
    use slint::Model;
    let Some(m) = cur.as_any().downcast_ref::<VecModel<Block>>() else { return Some(model(v)) };
    let n = v.len();
    for (i, mut b) in v.into_iter().enumerate() {
        if let Some(old) = (i < m.row_count()).then(|| m.row_data(i)).flatten().filter(|o| o.kind == b.kind) {
            let rows: Vec<RowData> = b.rows.iter().collect();
            b.rows = sync(old.rows.clone(), &rows).unwrap_or(old.rows.clone());
            let tiles: Vec<TileData> = b.tiles.iter().collect();
            b.tiles = sync(old.tiles.clone(), &tiles).unwrap_or(old.tiles.clone());
            if old != b { m.set_row_data(i, b); }
        } else if i < m.row_count() { m.set_row_data(i, b); } else { m.push(b); }
    }
    while m.row_count() > n { m.remove(m.row_count() - 1); }
    None
}

/// pages.rs's blocks as Slint's.
pub fn blocks(bs: &[B], p: &Palette) -> Vec<Block> {
    bs.iter().enumerate().map(|(k, b)| {
        let mut o = Block::default();
        match b {
            // A title with a lead line under it sits close to it (first).
            B::Title(t) => { o.kind = 0; o.text = s(t); o.first = matches!(bs.get(k + 1), Some(B::Lead(_))); }
            B::Lead(t) => { o.kind = 6; o.text = s(t); }
            B::Credits(c) => {
                o.kind = 7;
                let labels: Vec<SharedString> = pages::CREDITS_RANGES.iter().map(|r| s(r.0)).collect();
                o.credits = CreditsData {
                    id: s(pages::CREDITS_RANGE), range: c.range, longest: labels.iter().max_by_key(|l| l.len()).cloned().unwrap_or_default(), labels: model(labels),
                    today: s(&c.today), today_sub: s(&c.today_sub), week: s(&c.week), week_sub: s(&c.week_sub),
                    month_title: s(&c.month_title), month: s(&c.month), month_progress: c.month_progress, month_pct: s(&c.month_pct), month_sub: s(&c.month_sub),
                    bars: model(c.bars.iter().map(|b| CreditBar { label: s(&b.label), hover: b.hover, outside: b.outside, partial: b.partial, tip: s(&b.tip) }).collect()),
                    y_top: s(&c.y_top), y_mid: s(&c.y_mid), empty: s(&c.empty),
                    top: model(c.top.iter().map(|t| TopRow { title: s(&t.title), folder: s(&t.folder), credits: s(&t.credits) }).collect()),
                    top_empty: s(&c.top_empty), note: s(&c.note), label: s(&c.label),
                };
            }
            B::Heading(t, first) => { o.kind = 1; o.text = s(t); o.first = *first; }
            B::Footnote(t) => { o.kind = 3; o.text = s(t); }
            B::Link { id, name, icon, text, dim, status } => {
                o.kind = 5; o.id = s(id); o.name = s(name); o.icon = s(icon_path(icon)); o.text = s(text); o.dim = *dim; o.status = s(status);
            }
            B::Tiles(ts) => {
                o.kind = 4;
                o.tiles = model(ts.iter().map(|t| TileData {
                    id: s(&t.id), name: s(&t.name), from: s(&t.from), picked: t.picked,
                    panel: argb(t.palette.panel), surface: argb(t.palette.surface), ink: argb(t.palette.ink), ink_dim: argb(t.palette.ink_dim),
                    blue: argb(t.palette.blue), green: argb(t.palette.green), orange: argb(t.palette.orange), red: argb(t.palette.red),
                    purple: argb(t.palette.purple), teal: argb(t.palette.teal),
                }).collect());
            }
            B::Group(rows) => {
                o.kind = 2;
                o.rows = model(rows.iter().map(|r| {
                    let mut d = RowData { label: s(&r.label), sub: s(r.sub.as_deref().unwrap_or("")), sub_id: s(r.sub_id.as_deref().unwrap_or("")), enabled: r.enabled, button_enabled: true, ring: -1.0, picked: -1,
                        progress: r.progress.unwrap_or(-1.0), ..Default::default() };
                    match &r.control {
                        Control::None => d.control = 0,
                        Control::Switch { id, name, on } => { d.control = 1; d.id = s(id); d.name = s(name); d.on = *on; }
                        Control::Button { id, name, text, enabled } => { d.control = 2; d.id = s(id); d.name = s(name); d.text = s(text); d.button_enabled = *enabled; }
                        Control::Shortcut { id, name, text } => { d.control = 3; d.id = s(id); d.name = s(name); d.text = s(text); }
                        Control::Segments { id, labels, picked } => {
                            d.control = 4; d.id = s(id); d.picked = *picked;
                            // Segments are as wide as the widest label; capitals count for more.
                            d.longest = s(labels.iter().max_by_key(|l| l.chars().map(|c| if c.is_uppercase() { 3 } else { 2 }).sum::<usize>()).cloned().unwrap_or_default());
                            d.labels = model(labels.iter().map(s).collect());
                        }
                        Control::Slider { id, name, value, min, max } => { d.control = 10; d.id = s(id); d.name = s(name); d.picked = *value; d.lo = *min; d.hi = *max; }
                        Control::Picker { id, name, shown, options } => {
                            d.control = 5; d.id = s(id); d.name = s(name); d.text = s(shown);
                            d.options = model(options.iter().map(|(l, on)| Opt { label: s(l), on: *on }).collect());
                        }
                        Control::Text(t) => { d.control = 6; d.text = s(t); }
                        Control::Field { id, name, value, placeholder, secret, on } => {
                            d.control = 7; d.id = s(id); d.name = s(name); d.text = s(value); d.placeholder = s(placeholder); d.secret = *secret; d.on = *on;
                        }
                        Control::Chips { badges, buttons, open } => {
                            d.control = 8; d.open = s(open.as_deref().unwrap_or(""));
                            d.badges = model(badges.iter().map(|(t, warn)| Opt { label: s(t), on: *warn }).collect());
                            d.buttons = model(buttons.iter().map(|(id, t, red)| Btn { id: s(id), text: s(t), red: *red }).collect());
                        }
                        Control::Hold { id, name, text } => { d.control = 9; d.id = s(id); d.name = s(name); d.text = s(text); d.icon = s(icon_path("mic")); }
                    }
                    match &r.lead {
                        Lead::None => d.lead = 0,
                        Lead::Tile(icon, t) => { d.lead = 1; d.icon = s(icon_path(icon)); d.tint = tint(*t, p); }
                        Lead::Ring(v) => { d.lead = 2; d.ring = v.map_or(-1.0, |x| x as f32); }
                        Lead::Letter(l, t) => { d.lead = 3; d.letter = s(l); d.tint = tint(*t, p); }
                        Lead::Mark(tool) => { d.lead = 4; d.tool = s(tool); }
                    }
                    d
                }).collect());
            }
        }
        o
    }).collect()
}

/// An icon's path data from icons.slint's table, by its Lucide name; the mic (Voice's
/// tile) is the mockup's, which that table lacks.
pub fn icon_path(name: &str) -> String {
    if name == "mic" { return "M 12 2 a 3 3 0 0 0 -3 3 v 6 a 3 3 0 0 0 6 0 V 5 a 3 3 0 0 0 -3 -3 Z M 5 11 a 7 7 0 0 0 14 0 M 12 18 v 4".into(); }
    crate::icons::path(name).to_owned()
}

pub fn sections(p: &Palette) -> Vec<Side> {
    Section::ALL.iter().map(|x| {
        let (icon, t) = x.glyph();
        Side { title: s(x.title()), icon: s(icon_path(icon)), tint: tint(t, p), tool: s(x.mark().unwrap_or("")) }
    }).collect()
}

/// A handler for what Settings asks (the app implements it).
pub trait Host {
    fn hover(&self) -> &hover_app::app::Hover;
    fn system_dark(&self) -> bool;
    /// Something the notch or the tray draw from changed (OwlApp.SettingsChanged).
    fn settings_changed(&self);
    fn theme_changed(&self);
    fn shortcut_changed(&self);
    fn quit(&self);
    fn choose_folder(&self) -> Option<String>;
    fn choose_theme_file(&self) -> Option<std::path::PathBuf>;
    /// Page's state was rebuilt: push it to the windows.
    fn refresh(&self);
    /// A tool's status check finished off the UI thread: rebuild if still showing it.
    fn recheck(&self, tool: AgentTool, fresh: bool);
    /// What Settings can't do on its own, by action id (the app implements it):
    ///   "phonon.download" / "phonon.retry": Phonon::download(); "phonon.cancel": cancel();
    ///   "phonon.repair": repair(); "phonon.remove": remove(), an Err shown in the card.
    ///   "voice.try.press" / "voice.try.release": Voice::press(true) / release().
    ///   "groq.check": Voice::check_groq with secrets()'s GROQ_SECRET, off the UI thread,
    ///     its answer in pane.live.groq_check ("Checking…" first).
    ///   "voice.shortcut": the voice shortcut changed: take it again, a refusal in
    ///     pane.live.shortcut_error.
    ///   "voice.changed": the voice settings changed (on/off, mode, microphone, model,
    ///     cleanup): take or let go of the shortcut, stop Phonon's helper when Local
    ///     was left or voice switched off.
    /// Then fill pane.live and refresh.
    fn action(&self, id: &str) { let _ = id; }
    /// Runs `work` off the UI thread and then rebuilds the page (checks, sign-ins, downloads, the service).
    fn later(&self, work: Box<dyn FnOnce() + Send>) { work(); }
}

/// What background work says, shown under the control that started it on the next rebuild.
static LATER_NOTE: std::sync::Mutex<Option<(String, String)>> = std::sync::Mutex::new(None);
/// The registry's last search: its entries and a line about it.
static REGISTRY: std::sync::Mutex<(Vec<hover_agents::custom::Entry>, String)> = std::sync::Mutex::new((Vec::new(), String::new()));

fn said(id: &str, text: impl Into<String>) { *LATER_NOTE.lock().unwrap() = Some((id.into(), text.into())); }

pub fn build(h: &dyn Host, pane: &mut Pane) -> Vec<B> {
    let hv = h.hover();
    if let Some(n) = LATER_NOTE.lock().unwrap().take() { pane.note = Some(n); }
    if pane.section == Section::Automation { pane.live.auto = auto_view(hv, &pane.draft); }
    let installed = installed(pane);
    let field = if pane.recording_voice { None } else { pane.field.clone() }.unwrap_or_else(|| hv.settings.sc_workspace().label());
    let voice_field = if pane.recording_voice { pane.field.clone() } else { None }.unwrap_or_else(|| hv.settings.voice().shortcut.label());
    let reading = |id: &str| hv.quotas.reading(id);
    // Only Kiro's page reads it, and the first look starts the thread that makes it.
    let credits = if pane.section == Section::Kiro { hv.credits.view() } else { None };
    let ready = |t: AgentTool| hover_agents::agents::known(t);
    let store = secrets();
    let has_secret = |n: &str| store.has(n);
    let input = pages::Input {
        settings: &hv.settings,
        launch_at_login: hover_core::platform::SystemAutostart.enabled(),
        shortcut: field,
        reading: &reading,
        ready: &ready,
        installed: &installed,
        system_dark: h.system_dark(),
        import_status: pane.import_status.clone(),
        kiro_agents: hover_agents::kiro_agents(hv.settings.kiro_folder().as_deref()),
        voice_shortcut: voice_field,
        has_secret: &has_secret,
        secrets_kept: store.persistent(),
        project: pane.project.clone(),
        note: pane.note.clone(),
        live: &pane.live,
        credits: credits.as_deref(),
    };
    pages::build(pane.section, &input)
}

/// What a click in Settings does; the page is rebuilt after each.
pub fn toggled(h: &dyn Host, pane: &RefCell<Pane>, id: &str, on: bool) {
    let hv = h.hover();
    let st = &hv.settings;
    pane.borrow_mut().note = None;
    match id {
        "LaunchAtLogin" => {
            if let Err(e) = hover_core::platform::SystemAutostart.set(on) { hover_core::log::line(&format!("launch at login: {e}")); }
        }
        // The notch reads it from its own copy (n.hover_opens): pass it on now, not at
        // the next restart or size change.
        "HoverOpens" => { st.set_hover_opens_workspace(on); h.settings_changed(); }
        _ if id.starts_with("NotchItem") => {
            st.set_notch_item(&id["NotchItem".len()..], on);
            h.settings_changed();
            hv.refresh_quotas(true);
        }
        _ if id.ends_with("ShowSteps") => {
            let t = tool_of(&id[..id.len() - "ShowSteps".len()]);
            st.set_agent_options(t, hover_core::model::AgentOptions { hide_steps: !on, ..st.agent_options(t) });
        }
        "VoiceEnabled" => { st.set_voice(VoiceSettings { enabled: on, ..st.voice() }); h.action("voice.changed"); }
        "VoiceCleanup" => { st.set_voice(VoiceSettings { cleanup: on, ..st.voice() }); h.action("voice.changed"); }
        "ProjectVoice" => edit_project(h, pane, id, |p| p.voice = on),
        // The agents' extras (Settings → Integrations): read when a tool starts.
        "ComputerUse" => { st.set_computer_use(on); if on { h.action("integ.look"); } }
        "Sandbox" => { st.set_sandbox(on); h.action("integ.look"); }
        "AgentBrowser" => st.set_agent_browser(on),
        "DiscordPresence" => { st.set_discord_presence(on); hover_agents::discord::wake(); }
        // Kiro's page: auto compact and continuing when the model is busy.
        "KiroAutoCompact" => { pages::set_compact(st, id, on); }
        "KiroRetryBusy" => st.set_kiro_retry_busy(on),
        "UseFolder" => st.set_automation(hover_core::model::AutomationSettings { use_folder: on, ..st.automation() }),
        "AutoResume" => st.set_automation(hover_core::model::AutomationSettings { auto_resume: on, ..st.automation() }),
        "WebhookPublic" => { st.set_automation(hover_core::model::AutomationSettings { webhook_public: on, ..st.automation() }); hv.apply_automation(); }
        "CaEnvSecret" => draft(pane, "ca_env_secret", if on { "1" } else { "0" }),
        _ => {}
    }
    h.refresh();
}

fn tool_of(name: &str) -> AgentTool { AgentTool::ALL.into_iter().find(|t| t.name() == name).unwrap_or(AgentTool::Kiro) }

fn note(pane: &RefCell<Pane>, id: &str, text: String) { pane.borrow_mut().note = Some((id.into(), text)); }

/// Changes the open project; a refusal (a folder another project has) shows under the
/// control that asked.
fn edit_project(h: &dyn Host, pane: &RefCell<Pane>, id: &str, f: impl FnOnce(&mut Project)) {
    let st = &h.hover().settings;
    let open = pane.borrow().project.clone();
    let Some(mut p) = open.and_then(|x| st.project(&x)) else { return };
    f(&mut p);
    if let Err(e) = st.update_project(p) { note(pane, id, e); }
}

/// A text box's new value (Enter or focus out). A key's box sends an empty value only
/// from Remove key.
fn edited(h: &dyn Host, pane: &RefCell<Pane>, id: &str, v: &str) {
    let st = &h.hover().settings;
    let text = || Some(v.trim().to_owned()).filter(|t| !t.is_empty());
    if auto_edited(h, pane, id, v) { return; }
    match id {
        "ProjectName" => edit_project(h, pane, id, |p| p.name = v.into()),
        "ProjectAliases" => edit_project(h, pane, id, |p| p.aliases = v.split(',').map(|a| a.trim().to_owned()).filter(|a| !a.is_empty()).collect()),
        "VoiceCleanupModel" => { st.set_voice(VoiceSettings { cleanup_model: text(), ..st.voice() }); h.action("voice.changed"); }
        "VoiceCleanupBase" => match text() {
            Some(b) if !(b.starts_with("https://") || b.starts_with("http://")) => note(pane, id, "Use the full address, starting with https://.".into()),
            b => { st.set_voice(VoiceSettings { cleanup_base: b.map(|b| b.trim_end_matches('/').to_owned()), ..st.voice() }); h.action("voice.changed"); }
        },
        // Empty goes back to the default colour.
        "VoiceAuraHex" => match text() {
            None => st.set_voice(VoiceSettings { aura_color: None, ..st.voice() }),
            Some(t) => match VoiceSettings::hex_color(&t) {
                Some(c) => st.set_voice(VoiceSettings { aura_color: Some(c), ..st.voice() }),
                None => note(pane, id, "Use a hex colour, like #1FD5F9.".into()),
            },
        },
        "VoiceGroqKey" | "VoiceCleanupKey" => {
            let name = if id == "VoiceGroqKey" { GROQ_SECRET } else { st.voice().cleanup_provider.secret() };
            // The error never holds the key (Secrets::set's promise).
            if let Err(e) = secrets().set(name, Some(v)) { note(pane, id, e); }
            // The last check's answer was about the old key.
            if id == "VoiceGroqKey" { pane.borrow_mut().live.groq_check = None; }
        }
        _ => {}
    }
}


// MARK: Automation

fn schedule_text(t: &hover_agents::sched::Task) -> String {
    use hover_agents::sched::{civil_text, Schedule};
    let zone = t.tz.name();
    match t.schedule {
        Schedule::Manual => "Runs when you press Run now, or a webhook calls".into(),
        Schedule::Once { at } => format!("Once, {} ({zone})", civil_text(at, t.tz)),
        Schedule::Every { minutes } => format!("Every {minutes} minutes"),
        Schedule::Daily { hour, minute, days } => format!("{} at {hour:02}:{minute:02} ({zone})", if days & 0x7f == 0x7f { "Every day" } else { "Weekdays" }),
    }
}

/// Automation's live state, read fresh at each rebuild (each is quick: files, a lookup on PATH, one service query).
fn auto_view(hv: &hover_app::app::Hover, draft: &std::collections::HashMap<String, String>) -> pages::AutoView {
    use hover_agents::{custom, editor, service, wake};
    let tasks = hv.sched.list().iter().map(|t| pages::TaskRow {
        id: t.id.clone(), name: t.name.clone(), when: schedule_text(t), enabled: t.enabled, state: if t.enabled { "On" } else { "Paused" }.into(),
        last: t.runs.last().map_or("Not run yet".to_owned(), |r| format!("Last run {}: {}{}", hover_agents::sched::civil_text(r.started, t.tz), r.state.name(), if r.note.is_empty() { String::new() } else { format!(" · {}", r.note) })),
    }).collect();
    let mut agents: Vec<(String, String)> = AgentTool::ALL.iter().filter(|t| hover_agents::agents::known(**t).is_some_and(|r| r.ok())).map(|t| (t.id().to_owned(), t.name().to_owned())).collect();
    agents.extend(hv.customs.providers().into_iter().filter(|p| p.ready).map(|p| (p.id, p.name)));
    let svc = service::state();
    let (service_line, installed) = match &svc {
        service::State::NotInstalled => ("Not installed. Tasks run only while Hover is open.".to_owned(), false),
        service::State::Installed { running } => (if *running { "Installed and running." } else { "Installed, not running right now." }.to_owned(), true),
        service::State::Unavailable(e) => (format!("Can’t be checked here: {e}"), false),
    };
    let timers = if hv.has_timers() { "Hover (this window) runs the timers now.".to_owned() } else { match wake::held(&hover_app::app::Hover::exec_dir()) { wake::Held::By(who) => format!("{who} runs the timers now; Hover takes them when it can."), wake::Held::Free => "Nothing runs the timers yet.".into() } };
    let customs = hv.customs.list().iter().map(|a| {
        let st = hv.customs.status(&a.id);
        let (status, sign_in) = match &st {
            custom::Status::Unknown => ("Not checked yet. Press Check.".to_owned(), false),
            custom::Status::Ready(c) => (format!("{}{} · {} model{}{}", c.name.clone().unwrap_or_else(|| a.exe.clone()), c.version.as_ref().map(|v| format!(" {v}")).unwrap_or_default(), c.models.len(), if c.models.len() == 1 { "" } else { "s" },
                [c.why_not("resume").map(|_| "can’t resume"), c.why_not("images").map(|_| "no pictures")].into_iter().flatten().map(|x| format!(" · {x}")).collect::<String>()), false),
            custom::Status::NeedsSignIn(c) => (format!("Needs a sign-in ({}).", c.auth.iter().map(|m| m.1.clone()).collect::<Vec<_>>().join(", ")), !c.auth.is_empty()),
            custom::Status::Failed(e) => (e.clone(), false),
        };
        pages::CustomRow { id: a.id.clone(), name: a.name.clone(), status, ready: st.ready(), sign_in }
    }).collect();
    let (entries, note) = REGISTRY.lock().unwrap().clone();
    let has = |c: &str| hover_agents::proc::on_path(c).is_some();
    let registry = entries.iter().take(6).map(|e| match custom::plan(e, &custom::target(), &has) {
        Ok(p) => {
            let needs = p.blockers().iter().map(|n| format!("needs {} first", n.name)).collect::<Vec<_>>().join(", ");
            pages::RegRow { id: e.id.clone(), name: e.name.clone(), can: p.blockers().is_empty(),
                note: format!("{} · version {} · {}{}{}{}", e.description, e.version, if p.kind == "binary" { "downloads a program" } else { "runs a package" }, if p.checked { " (checksum checked)" } else if p.kind == "binary" { " (no checksum given)" } else { "" }, if needs.is_empty() { String::new() } else { format!(" · {needs}") }, e.license.as_ref().map(|l| format!(" · {l}")).unwrap_or_default()) }
        }
        Err(e2) => pages::RegRow { id: e.id.clone(), name: e.name.clone(), can: false, note: e2 },
    }).collect();
    pages::AutoView { editors: editor::available(&hv.settings.editor()).into_iter().filter(|f| f.id != editor::CUSTOM).map(|f| (f.id, f.name)).collect(), tasks, agents, service: service_line, service_installed: installed, timers,
        webhook: hv.webhooks.addr().map_or("Off.".to_owned(), |a| format!("Listening on {a}.")), customs, registry, registry_note: note, draft: draft.clone() }
}

fn draft(pane: &RefCell<Pane>, k: &str, v: &str) { pane.borrow_mut().draft.insert(k.into(), v.into()); }
fn drafted(pane: &RefCell<Pane>, k: &str) -> String { pane.borrow().draft.get(k).cloned().unwrap_or_default() }

/// Builds the new task from its boxes and adds it. The secret of its webhook, if it has one, is shown once.
fn add_task(h: &dyn Host, pane: &RefCell<Pane>) {
    use hover_agents::sched::{Hook, NewTask, Schedule, Tz};
    let hv = h.hover();
    let d = |k: &str| drafted(pane, k);
    let kind: i32 = d("kind").parse().unwrap_or(0);
    let when = d("when");
    let schedule = match kind {
        1 => match hover_agents::sched::parse_local(when.trim()) { Some(ms) => Schedule::Once { at: ms }, None => return note(pane, "TaskAdd", "Write the date and time like 2026-11-02 09:30.".into()) },
        2 => match when.trim().parse::<u32>() { Ok(m) if m >= 5 => Schedule::Every { minutes: m }, _ => return note(pane, "TaskAdd", "Write how many minutes between runs: 5 or more.".into()) },
        3 => match when.trim().split_once(':').and_then(|(h, m)| Some((h.parse::<u8>().ok()?, m.parse::<u8>().ok()?))) { Some((hour, minute)) if hour < 24 && minute < 60 => Schedule::Daily { hour, minute, days: if d("days") == "1" { 0b0111110 } else { 0x7f } }, _ => return note(pane, "TaskAdd", "Write the time like 09:00.".into()) },
        _ => Schedule::Manual,
    };
    let fields: Vec<String> = d("hook").split(',').map(|f| f.trim().to_owned()).filter(|f| !f.is_empty() && f != "off").collect();
    let hook = (!d("hook").trim().is_empty() && d("hook").trim() != "off").then(|| Hook { enabled: true, events: vec![], fields });
    let agent = { let a = d("agent"); if a.is_empty() { hv.customs.providers().into_iter().filter(|p| p.ready).map(|p| p.id).next().or_else(|| AgentTool::ALL.iter().find(|t| hover_agents::agents::known(**t).is_some_and(|r| r.ok())).map(|t| t.id().to_owned())).unwrap_or_default() } else { a } };
    let t = NewTask { name: d("name"), folder: d("folder"), prompt: d("prompt"), provider: agent, workspace: "own".into(), access: if d("access").is_empty() { "risky".into() } else { d("access") }, schedule, tz: Tz::Local, hook: hook.clone() };
    match hv.sched.add(t) {
        Ok(id) => {
            let mut text = "Added.".to_owned();
            if hook.is_some() {
                match hv.webhooks.rotate(&id) {
                    Ok(secret) => text = format!("Added. Webhook address: http://{}/hook/{id}. Secret, shown once: {secret}. Sign the body with HMAC-SHA256 (GitHub’s X-Hub-Signature-256 works).", hv.webhooks.addr().map_or("<set a listen address below>".to_owned(), |a| a.to_string())),
                    Err(e) => text = format!("Added, but the webhook secret couldn’t be made: {e}"),
                }
            }
            pane.borrow_mut().draft.clear();
            note(pane, "TaskAdd", text);
        }
        Err(e) => note(pane, "TaskAdd", e),
    }
}

/// Adds the agent from its boxes and checks it in the background.
fn add_custom(h: &dyn Host, pane: &RefCell<Pane>) {
    use hover_agents::custom::{EnvInput, Source};
    let hv = h.hover();
    let d = |k: &str| drafted(pane, k);
    let args: Vec<String> = (1..=4).map(|n| d(&format!("ca_arg{n}"))).filter(|a| !a.is_empty()).collect();
    let env = if d("ca_env_name").is_empty() { vec![] } else { vec![EnvInput { name: d("ca_env_name"), value: d("ca_env_value"), secret: d("ca_env_secret") == "1" }] };
    match hv.customs.add(&d("ca_name"), &d("ca_exe"), args, env, Source::Local) {
        Ok(id) => {
            pane.borrow_mut().draft.clear();
            note(pane, "CaAdd", "Added. Checking it…".into());
            let (c, hv2) = (hv.customs.clone(), ());
            let _ = hv2;
            h.later(Box::new(move || { let st = c.check(&id, &hover_agents::cancel::Cancel::new()); said("CaAdd", match st { hover_agents::custom::Status::Ready(_) => "Added, and it is ready.".to_owned(), other => format!("Added. {}", match other { hover_agents::custom::Status::NeedsSignIn(_) => "It needs a sign-in: press Sign in.".to_owned(), hover_agents::custom::Status::Failed(e) => e, _ => String::new() }) }); }));
        }
        Err(e) => note(pane, "CaAdd", e),
    }
}

/// A click on one of Automation's row buttons; true when it was one of theirs.
fn auto_pressed(h: &dyn Host, pane: &RefCell<Pane>, id: &str) -> bool {
    let hv = h.hover();
    if let Some(t) = id.strip_prefix("Task.run.") { if let Err(e) = hv.sched.run_now(t) { note(pane, id, e); } return true; }
    if let Some(t) = id.strip_prefix("Task.toggle.") { let on = hv.sched.get(t).is_some_and(|x| !x.enabled); hv.sched.set_enabled(t, on); return true; }
    if let Some(t) = id.strip_prefix("Task.remove.") { hv.webhooks.forget(t); hv.sched.remove(t); return true; }
    if let Some(a) = id.strip_prefix("Custom.check.") {
        let (c, a) = (hv.customs.clone(), a.to_owned());
        h.later(Box::new(move || { let st = c.check(&a, &hover_agents::cancel::Cancel::new()); if let hover_agents::custom::Status::Failed(e) = st { said("CaAdd", e); } }));
        return true;
    }
    if let Some(a) = id.strip_prefix("Custom.signin.") {
        let (c, a) = (hv.customs.clone(), a.to_owned());
        note(pane, id, "Signing in… follow what the agent shows (a browser, a code).".into());
        h.later(Box::new(move || {
            let method = c.status(&a).caps().and_then(|k| k.auth.first().map(|m| m.0.clone()));
            let st = match method { Some(m) => c.sign_in(&a, &m, &hover_agents::cancel::Cancel::new()), None => c.check(&a, &hover_agents::cancel::Cancel::new()) };
            said("CaAdd", if st.ready() { "Signed in. The agent is ready.".to_owned() } else { "It is not ready yet.".to_owned() });
        }));
        return true;
    }
    if let Some(a) = id.strip_prefix("Custom.remove.") { hv.customs.remove(a); return true; }
    if let Some(e) = id.strip_prefix("Reg.install.") {
        let entry = REGISTRY.lock().unwrap().0.iter().find(|x| x.id == e).cloned();
        let Some(entry) = entry else { return true };
        let c = hv.customs.clone();
        note(pane, id, format!("Installing {}…", entry.name));
        h.later(Box::new(move || {
            let root = hover_core::paths::support().join("custom-agents");
            match hover_app::registry::add(&c, &entry, &root, &hover_agents::cancel::Cancel::new()) {
                Ok(id) => { let st = c.check(&id, &hover_agents::cancel::Cancel::new()); said("RegSearch", format!("{} is added{}", entry.name, match st { hover_agents::custom::Status::Ready(_) => " and ready.".to_owned(), hover_agents::custom::Status::NeedsSignIn(_) => ". It needs a sign-in: press Sign in under Agents of your own.".to_owned(), hover_agents::custom::Status::Failed(e) => format!(", but it isn’t ready: {e}"), _ => ".".into() })); }
                Err(e) => said("RegSearch", e),
            }
        }));
        return true;
    }
    match id {
        "TaskFolder" => { if let Some(f) = h.choose_folder() { draft(pane, "folder", &f); } }
        "TaskAdd" => add_task(h, pane),
        "CaAdd" => add_custom(h, pane),
        "Service.install" => {
            let exe = std::env::current_exe().unwrap_or_default();
            let data_dir = std::env::var_os("HOVER_DATA_DIR").map(std::path::PathBuf::from);
            h.later(Box::new(move || said("Service.install", match hover_agents::service::install(&hover_agents::service::Spec { exe, data_dir }) { Ok(()) => "Installed. It starts at login and runs while Hover is closed.".to_owned(), Err(e) => e })));
        }
        "Service.stop" => h.later(Box::new(|| said("Service.stop", match hover_agents::service::stop() { Ok(()) => "Stopped. It comes back at the next login, or when Hover quits.".to_owned(), Err(e) => e }))),
        "Service.remove" => h.later(Box::new(|| said("Service.remove", match hover_agents::service::uninstall() { Ok(()) => "Removed. Your tasks and their history are kept.".to_owned(), Err(e) => e }))),
        _ => return false,
    }
    true
}

/// A text box of Automation.
fn auto_edited(h: &dyn Host, pane: &RefCell<Pane>, id: &str, v: &str) -> bool {
    let hv = h.hover();
    let st = &hv.settings;
    let text = || Some(v.trim().to_owned()).filter(|t| !t.is_empty());
    match id {
        "EditorExe" => st.set_editor(hover_core::model::EditorSettings { custom_exe: text(), ..st.editor() }),
        _ if id.starts_with("EditorArg") => {
            let n: usize = id["EditorArg".len()..].parse().unwrap_or(1);
            let e = st.editor();
            let mut slots: Vec<String> = e.custom_args.as_deref().unwrap_or("").lines().map(str::to_owned).collect();
            slots.resize(4, String::new());
            slots[(n - 1).min(3)] = v.trim().to_owned();
            let joined = slots.into_iter().filter(|a| !a.is_empty()).collect::<Vec<_>>().join("\n");
            st.set_editor(hover_core::model::EditorSettings { custom_args: Some(joined).filter(|j| !j.is_empty()), ..e });
        }
        "WebhookAddr" => { st.set_automation(hover_core::model::AutomationSettings { webhook_addr: text(), ..st.automation() }); hv.apply_automation(); if text().is_some() && hv.webhooks.addr().is_none() { note(pane, id, "It couldn’t listen there (see the log), or another copy of Hover holds the timers.".into()); } }
        "TaskName" => draft(pane, "name", v.trim()),
        "TaskPrompt" => draft(pane, "prompt", v.trim()),
        "TaskWhen" => draft(pane, "when", v.trim()),
        "TaskHook" => draft(pane, "hook", v.trim()),
        "CaName" => draft(pane, "ca_name", v.trim()),
        "CaExe" => draft(pane, "ca_exe", v.trim()),
        "CaEnvName" => draft(pane, "ca_env_name", v.trim()),
        "CaEnvValue" => draft(pane, "ca_env_value", v),
        _ if id.starts_with("CaArg") => draft(pane, &format!("ca_arg{}", &id["CaArg".len()..]), v),
        "RegSearch" => {
            draft(pane, "reg_q", v.trim());
            let q = v.trim().to_owned();
            *REGISTRY.lock().unwrap() = (vec![], "Searching…".into());
            h.later(Box::new(move || {
                let r = hover_app::registry::catalog(hover_app::registry::URL);
                *REGISTRY.lock().unwrap() = match r {
                    Ok(all) => { let found: Vec<_> = hover_agents::custom::search(&all, &q).into_iter().cloned().collect(); let n = found.len(); (found, if n == 0 { "Nothing in the registry matches that.".into() } else { format!("{n} found in the ACP Registry. Nothing is downloaded until you press Install.") }) }
                    Err(e) => (vec![], e),
                };
            }));
        }
        _ => return false,
    }
    true
}

pub fn pressed(h: &dyn Host, pane: &RefCell<Pane>, id: &str) {
    let hv = h.hover();
    let st = &hv.settings;
    pane.borrow_mut().note = None;
    // A text box's commit comes as "{id}\u{1f}{value}": the page's one string callback.
    if let Some((field, value)) = id.split_once('\u{1f}') { edited(h, pane, field, value); h.refresh(); return; }
    if auto_pressed(h, pane, id) { h.refresh(); return; }
    match id {
        "Quit" => { h.quit(); return; }
        "RefreshQuotas" => hv.refresh_quotas(true),
        "ImportTheme" => {
            if let Some(f) = h.choose_theme_file() {
                match hover_core::palette::read(&f, None, None) {
                    Some(t) => { apply_theme(h, pane, Some(t)); return; }
                    None => pane.borrow_mut().import_status = "That file has no VS Code theme colours in it.".into(),
                }
            }
        }
        "SettingsKiroFolder" => { if let Some(f) = h.choose_folder() { hv.settings.set_kiro_folder(Some(&f)); } }
        "KiroNoticeAgain" => { hv.settings.set_kiro_notice_seen(false); hv.sessions.raise_changed(); }
        "ProjectAdd" => if let Some(f) = h.choose_folder() { if let Err(e) = st.add_project(&f) { note(pane, id, e); } },
        "ProjectBack" => pane.borrow_mut().project = None,
        "ProjectFolder" => if let Some(f) = h.choose_folder() { edit_project(h, pane, id, |p| p.folder = f); },
        // Only the entry goes: the folder, its sessions and any run are left alone.
        "ProjectRemove" => { let open = pane.borrow_mut().project.take(); if let Some(x) = open { st.remove_project(&x); } }
        "DefaultFolder" => if let Some(f) = h.choose_folder() {
            match resolve_folder(&f) {
                Ok(r) => st.set_default_workspace(Workspace { folder: Some(r.to_string_lossy().into_owned()), ..st.default_workspace() }),
                Err(e) => note(pane, id, e),
            }
        },
        "VoiceShortcut" => { record_as(pane, true); }
        "VoiceAgent" => { let s = Section::of(st.voice().agent.unwrap_or_else(|| st.agent_tool())); let mut p = pane.borrow_mut(); p.section = s; p.project = None; }
        "VoiceWorkspace" => { let mut p = pane.borrow_mut(); p.section = Section::Projects; p.project = None; }
        _ if id.starts_with("Project.") => pane.borrow_mut().project = Some(id["Project.".len()..].into()),
        _ if id.ends_with("Recheck") => { h.recheck(tool_of(&id[..id.len() - "Recheck".len()]), true); }
        _ if id.starts_with("phonon.") || id.starts_with("voice.") || id.starts_with("integ.") || id == "groq.check" => h.action(id),
        _ => {}
    }
    h.refresh();
}

pub fn apply_theme(h: &dyn Host, pane: &RefCell<Pane>, t: Option<SavedTheme>) {
    h.hover().settings.set_theme(t);
    pane.borrow_mut().import_status.clear();
    h.theme_changed();
    h.refresh();
}

pub fn tile(h: &dyn Host, pane: &RefCell<Pane>, id: &str) {
    if id == "ThemeHover" { return apply_theme(h, pane, None); }
    let inst = installed(&mut pane.borrow_mut());
    if let Some((_, t)) = inst.iter().find(|(s, _)| format!("Theme{}", s.label) == id) { apply_theme(h, pane, Some(t.clone())); }
}

pub fn picked_seg(h: &dyn Host, pane: &RefCell<Pane>, id: &str, i: usize) {
    let hv = h.hover();
    let st = &hv.settings;
    pane.borrow_mut().note = None;
    match id {
        "WorkspaceSize" => { st.set_workspace_size(pages::SIZES[i].0); h.settings_changed(); }
        "Appearance" => { st.set_theme(None); st.set_appearance(pages::APPEARANCES[i].0); h.theme_changed(); }
        "VoiceSpeech" => { st.set_voice(VoiceSettings { speech: SpeechMode::ALL[i], ..st.voice() }); h.action("voice.changed"); }
        "VoiceMode" => { st.set_voice(VoiceSettings { hold: i == 1, ..st.voice() }); h.action("voice.changed"); }
        "VoiceCleanupProvider" => { st.set_voice(VoiceSettings { cleanup_provider: CleanupProvider::ALL[i], ..st.voice() }); h.action("voice.changed"); }
        "VoiceCountdown" => if let Some(&n) = VoiceSettings::COUNTDOWNS.get(i) { st.set_voice(VoiceSettings { countdown: n, ..st.voice() }); h.action("voice.changed"); },
        "DefaultAccess" => st.set_default_workspace(Workspace { access: ACCESS_IDS[i].into(), ..st.default_workspace() }),
        "ProjectAccess" => edit_project(h, pane, id, |p| p.access = ACCESS_IDS[i].into()),
        // A slider's number (a percent), not a segment's index.
        "KiroCompactAt" => { pages::pick_compact_at(st, id, i); }
        "DelegMax" => if let Some(&n) = [2u32, 4, 6, 10].get(i) { st.set_delegation(hover_core::model::DelegationLimits { max_helpers: n, ..st.delegation() }); },
        "DelegParallel" => if let Some(&n) = [1u32, 2, 3, 4].get(i) { st.set_delegation(hover_core::model::DelegationLimits { max_parallel: n, ..st.delegation() }); },
        "DelegDepth" => if let Some(&n) = [1u32, 2, 3].get(i) { st.set_delegation(hover_core::model::DelegationLimits { max_depth: n, ..st.delegation() }); },
        // The chart's range is the page's own, not a setting.
        pages::CREDITS_RANGE => pane.borrow_mut().live.credits_range = i as i32,
        "TaskKind" => draft(pane, "kind", &i.to_string()),
        "TaskDays" => draft(pane, "days", &i.to_string()),
        _ => {
            for t in AgentTool::ALL {
                let o = st.agent_options(t);
                let n = t.name();
                let offers = st.agent_offers(t);
                let new = if id == format!("{n}Effort") { pages::pick_effort(t, &o, &offers, i) }
                    else if id == format!("{n}Tools") {
                        use hover_core::model::AgentApproval as A;
                        // Read only keeps the asking it had; the rest are full access.
                        let approval = match i { 1 => A::Risky, 2 => A::Always, 3 => o.approval, _ => A::Autopilot };
                        hover_core::model::AgentOptions { read_only: i == 3, approval, ..o }
                    }
                    else if id == format!("{n}Idle") { hover_core::model::AgentOptions { idle_minutes: hover_core::model::AgentOptions::IDLE_CHOICES[i], ..o } }
                    else { continue };
                st.set_agent_options(t, new);
            }
        }
    }
    h.refresh();
}

pub fn menu_pick(h: &dyn Host, pane: &RefCell<Pane>, id: &str, i: usize) {
    pane.borrow_mut().menu = None;
    let st = &h.hover().settings;
    match id {
        // 0 is the system default; past the devices is a saved one not plugged in now.
        "VoiceMicrophone" => {
            let v = st.voice();
            let m = if i == 0 { None } else { pane.borrow().live.mics.get(i - 1).cloned().or(v.microphone.clone()) };
            st.set_voice(VoiceSettings { microphone: m, ..v });
            h.action("voice.changed");
        }
        "VoiceModel" => { st.set_voice(VoiceSettings { model: TRANSCRIBE_MODELS[i.min(TRANSCRIBE_MODELS.len() - 1)].0.into(), ..st.voice() }); h.action("voice.changed"); }
        "VoiceAuraColor" => if let Some(c) = VoiceSettings::AURA_COLORS.get(i) { st.set_voice(VoiceSettings { aura_color: Some(c.1.into()), ..st.voice() }); },
        // Voice's own default agent from then on; the new-task box keeps its own.
        "VoiceAgentTool" => { if let Some(&t) = AgentTool::ALL.get(i) { st.set_voice(VoiceSettings { agent: Some(t), ..st.voice() }); } }
        "EditorDefault" => {
            let editors = hover_agents::editor::available(&st.editor()).into_iter().filter(|f| f.id != hover_agents::editor::CUSTOM).collect::<Vec<_>>();
            let pick = match i { 0 => None, n if n <= editors.len() => Some(editors[n - 1].id.clone()), _ => Some(hover_agents::editor::CUSTOM.to_owned()) };
            st.set_editor(hover_core::model::EditorSettings { default: pick, ..st.editor() });
        }
        "TaskAgent" => { let pane_agents = pane.borrow().live.auto.agents.clone(); if let Some(a) = pane_agents.get(i) { draft(pane, "agent", &a.0); } }
        "TaskAccess" => { if let Some(a) = ["risky", "always", "read", "full"].get(i) { draft(pane, "access", a); } }
        _ => {}
    }
    for t in AgentTool::ALL {
        let o = st.agent_options(t);
        let offers = st.agent_offers(t);
        if id == format!("{}Model", t.name()) { st.set_agent_options(t, pages::pick_model(t, &o, &offers, i)); }
        if t == AgentTool::OpenCode && id == "OpenCodeAgent" { st.set_agent_options(t, pages::pick_opencode_agent(&o, &offers, i)); }
        if t == AgentTool::Kiro && id == "KiroAgent" {
            st.set_agent_options(t, pages::pick_agent(&o, &offers, &hover_agents::kiro_agents(st.kiro_folder().as_deref()), i));
        }
    }
    h.refresh();
}

/// The shortcut field: click to record, then a chord (ShortcutField).
pub fn record(h: &dyn Host, pane: &RefCell<Pane>) {
    record_as(pane, false);
    h.refresh();
}

/// Starts recording the notch's shortcut or the voice one; stops either.
fn record_as(pane: &RefCell<Pane>, voice: bool) {
    let mut p = pane.borrow_mut();
    if p.recording { p.recording = false; p.field = None; } else { p.recording = true; p.recording_voice = voice; p.field = Some("Press keys…".into()); }
}

pub fn chord(h: &dyn Host, pane: &RefCell<Pane>, text: &str, m: hover_core::shortcut::Modifiers) -> bool {
    if !pane.borrow().recording { return false; }
    let physical = pane.borrow_mut().physical.take();
    match keys::record_at(text, physical.as_deref(), m) {
        Recorded::Wait => return true,
        Recorded::NeedModifier => { pane.borrow_mut().field = Some(if cfg!(windows) { "Add Ctrl, Alt, Shift or Win" } else if cfg!(target_os = "macos") { "Add ⌃, ⌥, ⇧ or ⌘" } else { "Add Ctrl, Alt, Shift or Super" }.into()); }
        Recorded::Stop => { let mut p = pane.borrow_mut(); p.recording = false; p.field = None; }
        Recorded::Chord(sc) => {
            let st = &h.hover().settings;
            let voice = pane.borrow().recording_voice;
            let mut v = st.voice();
            // One chord can't be both: the system gives it to whichever takes it first.
            if sc == if voice { st.sc_workspace() } else { v.shortcut } {
                pane.borrow_mut().field = Some(if voice { "Used by the notch" } else { "Used by Voice" }.into());
            } else if voice {
                let changed = sc != v.shortcut;
                if changed { v.shortcut = sc; st.set_voice(v); }
                { let mut p = pane.borrow_mut(); p.recording = false; p.field = None; }
                if changed { h.action("voice.shortcut"); }
            } else {
                let changed = sc != st.sc_workspace();
                if changed { st.set_sc_workspace(sc); }
                { let mut p = pane.borrow_mut(); p.recording = false; p.field = None; }
                if changed { h.shortcut_changed(); }
            }
        }
    }
    h.refresh();
    true
}

/// Pushes the page into one window's Page global.
macro_rules! show_page {
    ($w:expr, $pane:expr, $blocks:expr, $pal:expr) => {{
        let g = $w.global::<crate::ui::Page>();
        let pane = $pane;
        if let Some(m) = crate::view::sync(g.get_sections(), &crate::view::sections($pal)) { g.set_sections(m); }
        g.set_current(pane.section as i32);
        if let Some(m) = crate::view::sync_blocks(g.get_blocks(), $blocks) { g.set_blocks(m); }
        g.set_recording(pane.recording);
        match &pane.menu {
            Some((id, opts, x, y)) => {
                g.set_menu_open(true);
                g.set_menu_id(id.as_str().into());
                g.set_menu_x(*x);
                g.set_menu_y(*y);
                g.set_menu(crate::view::model_of(opts.iter().map(|(l, on)| crate::ui::Opt { label: l.as_str().into(), on: *on }).collect()));
            }
            None => g.set_menu_open(false),
        }
    }};
}
pub(crate) use show_page;

pub fn model_of<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> { model(v) }

/// The picker's options for a row, from the blocks shown.
pub fn picker_options(bs: &[B], id: &str) -> Vec<(String, bool)> {
    for b in bs {
        if let B::Group(rows) = b {
            for r in rows {
                if let Control::Picker { id: rid, options, .. } = &r.control { if rid == id { return options.clone(); } }
            }
        }
    }
    vec![]
}

/// Wires one window's Page and Office globals to the app.
macro_rules! wire_page {
    ($w:expr, $app:expr, $which:expr) => {{
        let g = $w.global::<crate::ui::Page>();
        let a = $app.clone();
        g.on_section(move |i| {
            { let mut p = a.pane.borrow_mut(); p.section = hover_app::pages::Section::ALL[i as usize]; p.menu = None; p.project = None; p.note = None; }
            a.refresh_page(true);
        });
        let a = $app.clone();
        g.on_toggled(move |id, on| crate::view::toggled(&*a, &a.pane, &id, on));
        let a = $app.clone();
        g.on_pressed(move |id| crate::view::pressed(&*a, &a.pane, &id));
        let a = $app.clone();
        g.on_picked_seg(move |id, i| crate::view::picked_seg(&*a, &a.pane, &id, i as usize));
        let a = $app.clone();
        g.on_open_picker(move |id, x, y| {
            let opts = crate::view::picker_options(&a.last_blocks.borrow(), &id);
            a.pane.borrow_mut().menu = Some((id.to_string(), opts, x, y));
            a.refresh_page(false);
        });
        let a = $app.clone();
        g.on_menu_pick(move |id, i| crate::view::menu_pick(&*a, &a.pane, &id, i as usize));
        let a = $app.clone();
        g.on_menu_close(move || { a.pane.borrow_mut().menu = None; a.refresh_page(false); });
        let a = $app.clone();
        g.on_tile(move |id| crate::view::tile(&*a, &a.pane, &id));
        let a = $app.clone();
        g.on_record(move || crate::view::record(&*a, &a.pane));
        let a = $app.clone();
        g.on_chord(move |e| {
            let m = hover_app::keys::modifiers(e.modifiers.alt, e.modifiers.control, e.modifiers.shift, e.modifiers.meta);
            crate::view::chord(&*a, &a.pane, &e.text, m)
        });
        let o = $w.global::<crate::ui::Office>();
        let a = $app.clone();
        o.on_toggle_beats(move || a.toggle_beats());
        let a = $app.clone();
        o.on_open_settings(move || a.show_settings_in($which, hover_app::pages::Section::General));
        let a = $app.clone();
        o.on_fold(move || a.collapse());
        let a = $app.clone();
        o.on_open_app(move || a.open_dashboard(false));
    }};
}
pub(crate) use wire_page;

// MARK: Integrations (Settings → Integrations, and each agent's setup row)

impl crate::App {
    /// What Settings' buttons ask of Cua Driver and the tools' installers; the work runs off
    /// the UI thread and reports through the modules' change hooks.
    pub fn integ_action(self: &Rc<Self>, id: &str) {
        use hover_agents::{computer_use as cu, setup};
        self.integ_wire();
        match id {
            "integ.look" => { self.integ_look(true); return; }
            "integ.cua.install" => { std::thread::spawn(cu::install); }
            "integ.cua.grant" => { std::thread::spawn(cu::grant); }
            "integ.cua.cancel" => cu::cancel(),
            _ => {
                if let Some(t) = id.strip_prefix("integ.setup.").and_then(|x| AgentTool::ALL.into_iter().find(|t| t.id() == x)) { std::thread::spawn(move || setup::run(t, None)); }
                else if let Some(t) = id.strip_prefix("integ.setupcancel.").and_then(|x| AgentTool::ALL.into_iter().find(|t| t.id() == x)) { setup::cancel(t); }
            }
        }
        self.integ_sync();
    }

    /// The change hooks, once.
    fn integ_wire(self: &Rc<Self>) {
        static ONCE: std::sync::Once = std::sync::Once::new();
        ONCE.call_once(|| {
            hover_agents::computer_use::on_change(|| crate::ui_do(|a| a.integ_sync()));
            hover_agents::setup::on_change(|_| crate::ui_do(|a| a.integ_sync()));
        });
    }

    /// Asks (off the UI thread) whether Cua Driver is there and what the sandbox lacks.
    pub fn integ_look(self: &Rc<Self>, fresh: bool) {
        // The screenshots show what they are handed.
        if self.headless { return; }
        self.integ_wire();
        self.integ_sync();
        std::thread::spawn(move || {
            hover_agents::computer_use::check(fresh);
            crate::ui_do(|a| a.integ_sync());
        });
    }

    /// Publishes what the modules know now to the pages.
    pub fn integ_sync(self: &Rc<Self>) {
        use hover_agents::{computer_use as cu, sandbox, setup};
        let p = cu::setup();
        let busy = cu::busy();
        let cua = cu::known().map(|s| pages::Cua { installed: s.installed, version: s.version, permissions: s.permissions.to_owned(), hint: s.hint, busy, line: p.line.clone(), error: p.error.clone() })
            .or_else(|| busy.then(|| pages::Cua { busy, line: p.line.clone(), ..Default::default() }));
        let setups = AgentTool::ALL.into_iter().map(|t| { let s = setup::of(t); (t, pages::SetupCard { busy: setup::busy(t), line: s.line, error: s.error }) }).collect();
        let missing = if self.hover.settings.sandbox() && sandbox::supported() { sandbox::missing() } else { None };
        {
            let mut pane = self.pane.borrow_mut();
            pane.live.integ = pages::Integ { caps: pages::Caps::here(), cua, setup: setups, sandbox_missing: missing };
        }
        let section = self.pane.borrow().section;
        if matches!(section, Section::Integrations | Section::Kiro | Section::Codex | Section::Cursor | Section::OpenCode | Section::Claude | Section::Agy) { self.refresh_page(false); }
    }
}
#[cfg(target_os = "macos")]
impl crate::App {
    /// While a shortcut records: the key under the finger by its position (Option-N is a dead key).
    pub fn note_physical(&self, e: &slint::winit_030::winit::event::KeyEvent) {
        use slint::winit_030::winit::keyboard::PhysicalKey;
        if let (PhysicalKey::Code(c), Ok(mut p)) = (e.physical_key, self.pane.try_borrow_mut()) { if p.recording { p.physical = Some(format!("{c:?}")); } }
    }
}

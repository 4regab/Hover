//! Core/Settings.cs: the handful of preferences in settings.json, written as
//! System.Text.Json writes the C# Model (indented, enums by name, every property in
//! declaration order, nulls included). Writes wait 400 ms for the value to settle;
//! flush forces them out. Keys an older build wrote are ignored and dropped.

use crate::json::{self, Json, Result};
use crate::model::{notch_item, opt_text, AcpOption, AgentApproval, AgentOptions, AgentTool, Appearance, EditorSettings, SavedTheme, WorkspaceSize};
use crate::projects::{Project, VoiceSettings, Workspace};
use crate::shortcut::Shortcut;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::time::{Duration, Instant};

/// Settings.Model, field for field.
#[derive(Clone, Debug, PartialEq)]
pub struct Model {
    pub hover_opens_workspace: bool,
    pub notch_items: Option<Vec<Option<String>>>,
    pub appearance: Appearance,
    pub theme: Option<SavedTheme>,
    pub workspace_size: WorkspaceSize,
    pub kiro_folder: Option<String>,
    pub kiro_notice_seen: bool,
    pub kiro_model: Option<String>,
    pub kiro_effort: Option<String>,
    pub kiro_agent: Option<String>,
    pub kiro_read_only: bool,
    /// Ignored, as AgentOptions::require_mcp is; kept so users' files still read.
    pub kiro_require_mcp: bool,
    pub kiro_idle_minutes: i32,
    pub kiro_hide_steps: bool,
    pub kiro_approval: AgentApproval,
    /// Codex's, Cursor's and OpenCode's settings, by tool id. Kiro's are the fields above.
    pub agents: Option<Vec<(String, Option<AgentOptions>)>>,
    /// What each tool last offered (models, efforts, modes), for its settings page.
    pub agent_offers: Option<Vec<(String, Option<Vec<AcpOption>>)>>,
    pub agent_tool: Option<String>,
    /// Agents get Cua Driver's MCP server (macOS build, Services/ComputerUse.cs). Off
    /// until switched on; written only once it is on.
    pub computer_use: bool,
    /// Agents run inside srt (Services/Sandbox.cs). None (never set) is on; written only
    /// once it has been set.
    pub sandbox: Option<bool>,
    /// Agents get Hover's own browser as an MCP server, where the host has one
    /// (Services/BrowserTool.cs). None (never set) is on; written only once set.
    pub agent_browser: Option<bool>,
    /// Each project gets a desktop of its own, a Cua Space, in place of the user's screen
    /// (hover-agents::spaces). Off until switched on; written only once it is on.
    pub agent_spaces: bool,
    /// The image a new Space starts from, `macos` or `linux`. None (never set) is `macos`;
    /// written only once set.
    pub space_image: Option<String>,
    /// Kiro is asked to compact its conversation before the next reply once its context is this
    /// full (Hover's own; Kiro compacts by itself only at 100 %). None (never set) is off;
    /// written only once set.
    pub kiro_auto_compact: Option<bool>,
    /// The share of the context window (percent) that triggers it. None is 80; written only once set.
    pub kiro_compact_at: Option<i32>,
    /// A Kiro turn that stops because the model is busy (too many users) is continued at
    /// once, until stopped (Hover's own). None (never set) is off; written only once set.
    pub kiro_retry_busy: Option<bool>,
    /// Hover shows on the user's Discord status (hover-agents::discord). None (never set) is
    /// off; written only once set.
    pub discord_presence: Option<bool>,
    /// Open in editor (hover-agents::editor): the default editor and the custom one. None (never
    /// set) is no default and no custom editor; written only once set.
    pub editor: Option<EditorSettings>,
    pub sc_workspace: Shortcut,
    /// The registered projects, voice's settings and its default workspace (new in 3.x;
    /// null in a file from before them).
    pub projects: Option<Vec<Project>>,
    pub voice: Option<VoiceSettings>,
    pub default_workspace: Option<Workspace>,
}

impl Default for Model {
    fn default() -> Self {
        Model {
            hover_opens_workspace: true, notch_items: None, appearance: Appearance::System, theme: None, workspace_size: WorkspaceSize::Default,
            kiro_folder: None, kiro_notice_seen: false, kiro_model: None, kiro_effort: Some("high".into()), kiro_agent: None,
            kiro_read_only: false, kiro_require_mcp: false, kiro_idle_minutes: 5, kiro_hide_steps: false, kiro_approval: AgentApproval::Autopilot, agents: None, agent_offers: None,
            agent_tool: None, computer_use: false, sandbox: None, agent_browser: None, agent_spaces: false, space_image: None, kiro_auto_compact: None, kiro_compact_at: None, kiro_retry_busy: None, discord_presence: None, editor: None, sc_workspace: Shortcut::DEFAULT, projects: None, voice: None, default_workspace: None,
        }
    }
}

fn enum_or<T>(v: &Json, names: &[&str], make: fn(usize) -> T, default: T) -> Result<T> {
    Ok(v.enum_of(names)?.map(make).unwrap_or(default))
}

impl Model {
    pub fn to_json(&self) -> Json {
        let s = |v: &Option<String>| Json::opt_str_of(v.as_deref());
        // The three toggles the macOS build added come after AgentTool (as Settings.cs
        // declares them) and are written only once set, so a file that never used them
        // stays as 3.x wrote it.
        let toggles: Vec<(&str, Json)> = [
            self.computer_use.then_some(("ComputerUse", Json::Bool(true))),
            self.sandbox.map(|v| ("Sandbox", Json::Bool(v))),
            self.agent_browser.map(|v| ("AgentBrowser", Json::Bool(v))),
            self.agent_spaces.then_some(("AgentSpaces", Json::Bool(true))),
            self.space_image.as_deref().map(|v| ("SpaceImage", Json::str(v))),
            self.kiro_auto_compact.map(|v| ("KiroAutoCompact", Json::Bool(v))),
            self.kiro_compact_at.map(|v| ("KiroCompactAt", Json::int(v as i64))),
            self.kiro_retry_busy.map(|v| ("KiroRetryBusy", Json::Bool(v))),
            self.discord_presence.map(|v| ("DiscordPresence", Json::Bool(v))),
            self.editor.as_ref().map(|v| ("Editor", v.to_json())),
        ].into_iter().flatten().collect();
        let mut props = vec![
            ("HoverOpensWorkspace", Json::Bool(self.hover_opens_workspace)),
            ("NotchItems", self.notch_items.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(s).collect()))),
            ("Appearance", Json::str(Appearance::NAMES[self.appearance as usize])),
            ("Theme", self.theme.as_ref().map_or(Json::Null, SavedTheme::to_json)),
            ("WorkspaceSize", Json::str(WorkspaceSize::NAMES[self.workspace_size as usize])),
            ("KiroFolder", s(&self.kiro_folder)),
            ("KiroNoticeSeen", Json::Bool(self.kiro_notice_seen)),
            ("KiroModel", s(&self.kiro_model)),
            ("KiroEffort", s(&self.kiro_effort)),
            ("KiroAgent", s(&self.kiro_agent)),
            ("KiroReadOnly", Json::Bool(self.kiro_read_only)),
            ("KiroRequireMcp", Json::Bool(self.kiro_require_mcp)),
            ("KiroIdleMinutes", Json::int(self.kiro_idle_minutes as i64)),
            ("KiroHideSteps", Json::Bool(self.kiro_hide_steps)),
            ("KiroApproval", Json::str(self.kiro_approval.name())),
            ("Agents", self.agents.as_ref().map_or(Json::Null, |m| Json::Obj(m.iter().map(|(k, v)| (k.clone(), v.as_ref().map_or(Json::Null, AgentOptions::to_json))).collect()))),
            ("AgentOffers", self.agent_offers.as_ref().map_or(Json::Null, |m| Json::Obj(m.iter().map(|(k, v)| (k.clone(),
                v.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(AcpOption::to_json).collect())))).collect()))),
            ("AgentTool", s(&self.agent_tool)),
            ("ScWorkspace", self.sc_workspace.to_json()),
            ("Projects", self.projects.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(Project::to_json).collect()))),
            ("Voice", self.voice.as_ref().map_or(Json::Null, VoiceSettings::to_json)),
            ("DefaultWorkspace", self.default_workspace.as_ref().map_or(Json::Null, Workspace::to_json)),
        ];
        let at = props.iter().position(|(k, _)| *k == "ScWorkspace").unwrap_or(props.len());
        props.splice(at..at, toggles);
        Json::obj(props)
    }

    /// Deserialize<Model>: the defaults, then each property the file names, in file
    /// order. Any value of the wrong kind fails the whole read, as the serializer does.
    pub fn from_json(v: &Json) -> Result<Model> {
        let mut m = Model::default();
        for (k, x) in v.props()? {
            let b = || x.bool();
            match k.as_str() {
                "HoverOpensWorkspace" => m.hover_opens_workspace = b()?,
                "NotchItems" => m.notch_items = x.opt_list(Json::opt_str)?,
                "Appearance" => m.appearance = enum_or(x, &Appearance::NAMES, Appearance::from_index, Appearance::System)?,
                "Theme" => m.theme = if x.is_null() { None } else { Some(SavedTheme::from_json(x)?) },
                "WorkspaceSize" => m.workspace_size = enum_or(x, &WorkspaceSize::NAMES, WorkspaceSize::from_index, WorkspaceSize::Default)?,
                "KiroFolder" => m.kiro_folder = opt_text(Some(x))?,
                "KiroNoticeSeen" => m.kiro_notice_seen = b()?,
                "KiroModel" => m.kiro_model = opt_text(Some(x))?,
                "KiroEffort" => m.kiro_effort = opt_text(Some(x))?,
                "KiroAgent" => m.kiro_agent = opt_text(Some(x))?,
                "KiroReadOnly" => m.kiro_read_only = b()?,
                "KiroRequireMcp" => m.kiro_require_mcp = b()?,
                "KiroIdleMinutes" => m.kiro_idle_minutes = x.i32()?,
                "KiroHideSteps" => m.kiro_hide_steps = b()?,
                "KiroApproval" => m.kiro_approval = AgentApproval::read(x)?,
                "Agents" => m.agents = x.opt_map(|o| if o.is_null() { Ok(None) } else { AgentOptions::from_json(o).map(Some) })?,
                "AgentOffers" => m.agent_offers = x.opt_map(|l| l.opt_list(|o| if o.is_null() { Ok(None) } else { AcpOption::from_json(o).map(Some) })
                    .map(|l| l.map(|l| l.into_iter().flatten().collect())))?,
                "AgentTool" => m.agent_tool = opt_text(Some(x))?,
                "ComputerUse" => m.computer_use = b()?,
                "Sandbox" => m.sandbox = if x.is_null() { None } else { Some(b()?) },
                "AgentBrowser" => m.agent_browser = if x.is_null() { None } else { Some(b()?) },
                "AgentSpaces" => m.agent_spaces = b()?,
                "SpaceImage" => m.space_image = opt_text(Some(x))?,
                "KiroAutoCompact" => m.kiro_auto_compact = if x.is_null() { None } else { Some(b()?) },
                "KiroCompactAt" => m.kiro_compact_at = if x.is_null() { None } else { Some(x.i32()?) },
                "KiroRetryBusy" => m.kiro_retry_busy = if x.is_null() { None } else { Some(b()?) },
                "DiscordPresence" => m.discord_presence = if x.is_null() { None } else { Some(b()?) },
                "Editor" => m.editor = if x.is_null() { None } else { Some(EditorSettings::from_json(x)?) },
                // A null shortcut would leave C# with none at all (and a crash where
                // it is read); here it is unset, as a cleared shortcut is.
                "ScWorkspace" => m.sc_workspace = if x.is_null() { Shortcut::default() } else { Shortcut::from_json(x)? },
                "Projects" => m.projects = x.opt_list(Project::from_json)?,
                "Voice" => m.voice = if x.is_null() { None } else { Some(VoiceSettings::from_json(x)?) },
                "DefaultWorkspace" => m.default_workspace = if x.is_null() { None } else { Some(Workspace::from_json(x)?) },
                _ => {}
            }
        }
        Ok(m)
    }

    /// Auto compact's percent (1 to 100; 80 unless set), whether or not it is on.
    pub fn compact_at(&self) -> u8 { self.kiro_compact_at.unwrap_or(80).clamp(1, 100) as u8 }

    /// Whether a Kiro turn stopped by a busy model is continued at once (off unless set).
    pub fn retry_busy(&self) -> bool { self.kiro_retry_busy.unwrap_or(false) }

    /// The percent at which Kiro is asked to compact, or None while auto compact is off.
    pub fn auto_compact(&self) -> Option<u8> { self.kiro_auto_compact.unwrap_or(false).then(|| self.compact_at()) }

    /// The file's text, with the platform's newline.
    pub fn text(&self) -> String { self.to_json().indented(json::NEWLINE) }
}

/// Settings.Load: the file's model, or the defaults when there is none or it can't be read.
pub fn load_model(file: &Path) -> Model {
    let Ok(bytes) = std::fs::read(file) else { return Model::default() };
    match json::parse(&json::text_of(&bytes)).and_then(|v| if v.is_null() { Ok(Model::default()) } else { Model::from_json(&v) }) {
        Ok(m) => m,
        Err(e) => { crate::log::line(&format!("settings load failed — {e}")); Model::default() }
    }
}

const SETTLE: Duration = Duration::from_millis(400);

pub struct Settings {
    file: PathBuf,
    m: Mutex<Model>,
    due: Arc<(Mutex<Option<Instant>>, Condvar)>,
    me: Weak<Settings>,
    writer: Mutex<bool>,
    autostart: Box<dyn crate::platform::Autostart + Send + Sync>,
}

impl Settings {
    pub fn load(file: PathBuf) -> Arc<Settings> {
        let m = load_model(&file);
        Arc::new_cyclic(|me| Settings {
            file, m: Mutex::new(m), due: Arc::new((Mutex::new(None), Condvar::new())), me: me.clone(), writer: Mutex::new(false),
            autostart: Box::new(crate::platform::SystemAutostart),
        })
    }

    /// The copy of the model the setters change.
    pub fn model(&self) -> Model { self.m.lock().unwrap().clone() }

    /// Settings.Save: the write waits until nothing has changed for 400 ms.
    pub fn save(&self) {
        let (lock, cv) = &*self.due;
        *lock.lock().unwrap() = Some(Instant::now() + SETTLE);
        cv.notify_all();
        let mut started = self.writer.lock().unwrap();
        if *started { return; }
        *started = true;
        let (due, me) = (self.due.clone(), self.me.clone());
        std::thread::Builder::new().name("settings".into()).spawn(move || loop {
            let (lock, cv) = &*due;
            let mut d = lock.lock().unwrap();
            loop {
                match *d {
                    None => d = cv.wait(d).unwrap(),
                    Some(t) if Instant::now() < t => d = cv.wait_timeout(d, t - Instant::now()).unwrap().0,
                    Some(_) => break,
                }
            }
            *d = None;
            drop(d);
            match me.upgrade() { Some(s) => s.write(), None => return }
        }).expect("a thread for the settings");
    }

    /// Settings.Flush: written now, a pending write dropped.
    pub fn flush(&self) {
        *self.due.0.lock().unwrap() = None;
        self.write();
    }

    fn write(&self) {
        let text = self.m.lock().unwrap().text();
        // A temporary file, then a rename over the old one: a crash mid-write never
        // leaves half a settings.json (which would read as all the defaults).
        let tmp = self.file.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, text).and_then(|_| std::fs::rename(&tmp, &self.file)) {
            crate::log::line(&format!("settings save failed — {e}"));
        }
    }

    fn change(&self, f: impl FnOnce(&mut Model)) {
        f(&mut self.m.lock().unwrap());
        self.save();
    }

    pub fn hover_opens_workspace(&self) -> bool { self.m.lock().unwrap().hover_opens_workspace }
    pub fn set_hover_opens_workspace(&self, v: bool) { self.change(|m| m.hover_opens_workspace = v) }

    /// What the resting notch shows, in canonical order. Ids an older build saved (the
    /// focus timer's) are left out, and, as the C# getter does, out of the model too.
    pub fn notch_items(&self) -> Vec<&'static str> {
        let mut m = self.m.lock().unwrap();
        let have = m.notch_items.clone().unwrap_or_default();
        let list: Vec<&'static str> = notch_item::ALL.into_iter().filter(|id| have.iter().any(|h| h.as_deref() == Some(*id))).collect();
        m.notch_items = Some(list.iter().map(|s| Some(s.to_string())).collect());
        list
    }

    pub fn set_notch_items(&self, value: &[&str]) {
        let list: Vec<Option<String>> = notch_item::ALL.into_iter().filter(|id| value.contains(id)).map(|s| Some(s.to_string())).collect();
        self.change(|m| m.notch_items = Some(list));
    }

    pub fn has_notch_item(&self, id: &str) -> bool { self.notch_items().contains(&id) }

    pub fn set_notch_item(&self, id: &str, on: bool) {
        let mut set = self.notch_items();
        if on { if let Some(k) = notch_item::ALL.into_iter().find(|k| *k == id) { if !set.contains(&k) { set.push(k); } } } else { set.retain(|k| *k != id); }
        self.set_notch_items(&set);
    }

    pub fn appearance(&self) -> Appearance { self.m.lock().unwrap().appearance }
    pub fn set_appearance(&self, v: Appearance) { self.change(|m| m.appearance = v) }
    pub fn theme(&self) -> Option<SavedTheme> { self.m.lock().unwrap().theme.clone() }
    pub fn set_theme(&self, v: Option<SavedTheme>) { self.change(|m| m.theme = v) }
    pub fn workspace_size(&self) -> WorkspaceSize { self.m.lock().unwrap().workspace_size }
    pub fn set_workspace_size(&self, v: WorkspaceSize) { self.change(|m| m.workspace_size = v) }
    pub fn sc_workspace(&self) -> Shortcut { self.m.lock().unwrap().sc_workspace }
    pub fn set_sc_workspace(&self, v: Shortcut) { self.change(|m| m.sc_workspace = v) }

    /// The folder the last task ran in, as picked, even when it has gone missing; a
    /// blank one is none.
    pub fn kiro_folder(&self) -> Option<String> { self.m.lock().unwrap().kiro_folder.clone() }
    pub fn set_kiro_folder(&self, v: Option<&str>) {
        let v = v.filter(|s| !s.trim().is_empty()).map(str::to_owned);
        self.change(|m| m.kiro_folder = v)
    }

    pub fn kiro_notice_seen(&self) -> bool { self.m.lock().unwrap().kiro_notice_seen }
    pub fn set_kiro_notice_seen(&self, v: bool) { self.change(|m| m.kiro_notice_seen = v) }

    /// How a tool's runs are set up (Settings → Kiro, Codex, Cursor).
    pub fn agent_options(&self, t: AgentTool) -> AgentOptions {
        let m = self.m.lock().unwrap();
        if t == AgentTool::Kiro {
            return AgentOptions { model: m.kiro_model.clone(), effort: m.kiro_effort.clone(), read_only: m.kiro_read_only, idle_minutes: m.kiro_idle_minutes,
                agent: m.kiro_agent.clone(), require_mcp: m.kiro_require_mcp, hide_steps: m.kiro_hide_steps, approval: m.kiro_approval };
        }
        m.agents.as_ref().and_then(|a| a.iter().find(|(k, _)| k == t.id())).and_then(|(_, v)| v.clone()).unwrap_or_default()
    }

    pub fn set_agent_options(&self, t: AgentTool, mut v: AgentOptions) {
        if v.model.as_deref() == Some("auto") { v.model = None; }
        if v.agent.as_deref().is_some_and(|a| a.trim().is_empty()) { v.agent = None; }
        if !AgentOptions::IDLE_CHOICES.contains(&v.idle_minutes) { v.idle_minutes = AgentOptions::IDLE_CHOICES[0]; }
        self.change(|m| {
            if t == AgentTool::Kiro {
                m.kiro_model = v.model;
                m.kiro_effort = Some(v.effort.unwrap_or_else(|| "high".into()));
                m.kiro_agent = v.agent;
                m.kiro_read_only = v.read_only;
                m.kiro_require_mcp = v.require_mcp;
                m.kiro_idle_minutes = v.idle_minutes;
                m.kiro_hide_steps = v.hide_steps;
                m.kiro_approval = v.approval;
            } else {
                // An agent is Kiro's (the fields above) and OpenCode's (Build, Plan, the user's own).
                let agent = if t == AgentTool::OpenCode { v.agent.clone() } else { None };
                let v = AgentOptions { agent, require_mcp: false, ..v };
                let a = m.agents.get_or_insert_with(Vec::new);
                match a.iter_mut().find(|(k, _)| k == t.id()) { Some(slot) => slot.1 = Some(v), None => a.push((t.id().into(), Some(v))) }
            }
        });
    }

    /// The models, efforts and modes the tool offered the last time it ran.
    pub fn agent_offers(&self, t: AgentTool) -> Vec<AcpOption> {
        let m = self.m.lock().unwrap();
        m.agent_offers.as_ref().and_then(|a| a.iter().find(|(k, _)| k == t.id())).and_then(|(_, v)| v.clone()).unwrap_or_default()
    }

    /// Every turn reports them; only a change is written.
    pub fn set_agent_offers(&self, t: AgentTool, offers: &[AcpOption]) {
        {
            let m = self.m.lock().unwrap();
            let old = m.agent_offers.as_ref().and_then(|a| a.iter().find(|(k, _)| k == t.id())).and_then(|(_, v)| v.as_ref());
            if old.is_some_and(|o| o.as_slice() == offers) { return; }
        }
        self.change(|m| {
            let a = m.agent_offers.get_or_insert_with(Vec::new);
            match a.iter_mut().find(|(k, _)| k == t.id()) { Some(slot) => slot.1 = Some(offers.to_vec()), None => a.push((t.id().into(), Some(offers.to_vec()))) }
        });
    }

    /// The tool the last new task went to.
    pub fn agent_tool(&self) -> AgentTool { AgentTool::parse(self.m.lock().unwrap().agent_tool.as_deref()).unwrap_or(AgentTool::Kiro) }
    pub fn set_agent_tool(&self, t: AgentTool) { self.change(|m| m.agent_tool = Some(t.id().into())) }

    /// Agents get Cua Driver as an MCP server, to see and drive apps in the background
    /// (hover-agents::computer_use). Off until switched on; it reaches every tool from
    /// its next session.
    pub fn computer_use(&self) -> bool { self.m.lock().unwrap().computer_use }
    pub fn set_computer_use(&self, v: bool) { self.change(|m| m.computer_use = v) }

    /// Agents run inside Anthropic's sandbox-runtime (hover-agents::sandbox). On unless
    /// switched off; a tool picks it up when it next starts.
    pub fn sandbox(&self) -> bool { self.m.lock().unwrap().sandbox.unwrap_or(true) }
    pub fn set_sandbox(&self, v: bool) { self.change(|m| m.sandbox = Some(v)) }

    /// Agents get Hover's own browser as an MCP server, where the host has one
    /// (hover-agents::browser). On unless switched off; a tool picks it up from its next
    /// session.
    pub fn agent_browser(&self) -> bool { self.m.lock().unwrap().agent_browser.unwrap_or(true) }
    pub fn set_agent_browser(&self, v: bool) { self.change(|m| m.agent_browser = Some(v)) }

    /// Each project gets a desktop of its own, a Cua Space (hover-agents::spaces), for its
    /// agents' computer use in place of the user's screen. Off until switched on; a tool
    /// picks it up from its next session.
    pub fn agent_spaces(&self) -> bool { self.m.lock().unwrap().agent_spaces }
    pub fn set_agent_spaces(&self, v: bool) { self.change(|m| m.agent_spaces = v) }
    /// The image a new Space starts from: "macos" (a VM, two at most on a Mac) or "linux".
    pub fn space_image(&self) -> &'static str { if self.m.lock().unwrap().space_image.as_deref() == Some("linux") { "linux" } else { "macos" } }
    pub fn set_space_image(&self, v: &str) { let v = if v == "linux" { "linux" } else { "macos" }; self.change(|m| m.space_image = Some(v.into())) }

    /// Kiro only: compact the conversation before the next reply once the context is
    /// `kiro_compact_at` % full. Off unless switched on.
    pub fn kiro_auto_compact(&self) -> bool { self.m.lock().unwrap().kiro_auto_compact.unwrap_or(false) }
    pub fn set_kiro_auto_compact(&self, v: bool) { self.change(|m| m.kiro_auto_compact = Some(v)) }
    /// The percent of the context window that triggers it (1 to 100; 80 unless set).
    pub fn kiro_compact_at(&self) -> u8 { self.m.lock().unwrap().compact_at() }
    pub fn set_kiro_compact_at(&self, pct: u8) { self.change(|m| m.kiro_compact_at = Some(pct.clamp(1, 100) as i32)) }

    /// Kiro only: a turn that stops because the model is busy is continued at once, until stopped.
    pub fn kiro_retry_busy(&self) -> bool { self.m.lock().unwrap().retry_busy() }
    pub fn set_kiro_retry_busy(&self, v: bool) { self.change(|m| m.kiro_retry_busy = Some(v)) }

    /// Hover shows on the user's Discord status while this is on (hover-agents::discord).
    /// Off unless switched on.
    pub fn discord_presence(&self) -> bool { self.m.lock().unwrap().discord_presence.unwrap_or(false) }
    pub fn set_discord_presence(&self, v: bool) { self.change(|m| m.discord_presence = Some(v)) }

    /// Open in editor: the default editor and the custom one (EditorSettings).
    pub fn editor(&self) -> EditorSettings { self.m.lock().unwrap().editor.clone().unwrap_or_default() }
    pub fn set_editor(&self, v: EditorSettings) { self.change(|m| m.editor = Some(v)) }

    /// Launch at login: outside settings.json, in the platform's own place.
    pub fn launch_at_login(&self) -> bool { self.autostart.enabled() }
    pub fn set_launch_at_login(&self, on: bool) {
        if let Err(e) = self.autostart.set(on) { crate::log::line(&format!("launch-at-login toggle failed — {e}")); }
    }

    /// The registered projects, in the order they were added.
    pub fn projects(&self) -> Vec<Project> { self.m.lock().unwrap().projects.clone().unwrap_or_default() }

    pub fn project(&self, id: &str) -> Option<Project> { self.projects().into_iter().find(|p| p.id == id) }

    /// Registers a folder. Err when it can't be used, or is already registered (by
    /// whatever path it was written).
    pub fn add_project(&self, folder: &str) -> std::result::Result<Project, String> {
        let resolved = crate::projects::resolve_folder(folder)?;
        let f = resolved.to_string_lossy().into_owned();
        if let Some(p) = self.projects().into_iter().find(|p| crate::projects::same_folder(&p.folder, &f)) {
            return Err(format!("That folder is already registered as “{}”.", p.name));
        }
        let name = resolved.file_name().map(|n| n.to_string_lossy().into_owned()).filter(|n| !n.is_empty()).unwrap_or_else(|| f.clone());
        let p = Project::new(&name, &f);
        let added = p.clone();
        self.change(|m| m.projects.get_or_insert_with(Vec::new).push(p));
        Ok(added)
    }

    /// Changes a project in place (its id stays). Err for a folder another project has
    /// or that can't be used; a blank name keeps the old one.
    pub fn update_project(&self, p: Project) -> std::result::Result<(), String> {
        let old = self.project(&p.id).ok_or("That project isn’t registered any more.")?;
        let mut p = p;
        if p.name.trim().is_empty() { p.name = old.name.clone(); }
        p.name = p.name.trim().to_owned();
        if !crate::projects::same_folder(&old.folder, &p.folder) {
            let f = crate::projects::resolve_folder(&p.folder)?.to_string_lossy().into_owned();
            if let Some(o) = self.projects().into_iter().find(|o| o.id != p.id && crate::projects::same_folder(&o.folder, &f)) {
                return Err(format!("That folder is already registered as “{}”.", o.name));
            }
            p.folder = f;
        }
        let mut seen: Vec<String> = vec![];
        p.aliases.retain(|a| { let k = a.trim().to_lowercase(); let keep = !k.is_empty() && !seen.contains(&k); seen.push(k); keep });
        if !crate::projects::ACCESS_IDS.contains(&p.access.as_str()) { p.access = old.access; }
        self.change(|m| if let Some(slot) = m.projects.get_or_insert_with(Vec::new).iter_mut().find(|x| x.id == p.id) { *slot = p; });
        Ok(())
    }

    /// Forgets a project: its folder, its files and its sessions are left as they are.
    pub fn remove_project(&self, id: &str) { self.change(|m| if let Some(l) = &mut m.projects { l.retain(|p| p.id != id); }) }

    pub fn voice(&self) -> VoiceSettings { self.m.lock().unwrap().voice.clone().unwrap_or_default() }
    pub fn set_voice(&self, v: VoiceSettings) { self.change(|m| m.voice = Some(v)) }

    pub fn default_workspace(&self) -> Workspace { self.m.lock().unwrap().default_workspace.clone().unwrap_or_default() }
    pub fn set_default_workspace(&self, w: Workspace) { self.change(|m| m.default_workspace = Some(w)) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::shortcut::{Key, Modifiers};

    /// A fresh Model as `JsonSerializer.Serialize(new Model(), Json)` writes it, derived
    /// from Settings.cs: WriteIndented, JsonStringEnumConverter, declaration order,
    /// nulls written, and Environment.NewLine (CRLF on Windows).
    /// SettingsTests.Each_agents_approval_is_kept_and_asking_is_opt_in, ported.
    #[test]
    fn each_agents_approval_is_kept_and_asking_is_opt_in() {
        let dir = std::env::temp_dir().join(format!("hover-approval-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("settings.json");
        let s = Settings::load(file.clone());
        let kiro = s.agent_options(AgentTool::Kiro);
        let codex = s.agent_options(AgentTool::Codex);
        s.set_agent_options(AgentTool::Kiro, AgentOptions { approval: AgentApproval::Risky, ..kiro });
        s.set_agent_options(AgentTool::Codex, AgentOptions { approval: AgentApproval::Always, ..codex });
        s.flush();
        let json = std::fs::read_to_string(&file).unwrap();
        assert_eq!(s.agent_options(AgentTool::Kiro).approval, AgentApproval::Risky);
        assert_eq!(s.agent_options(AgentTool::Codex).approval, AgentApproval::Always);
        assert_eq!(AgentOptions::default().approval, AgentApproval::Autopilot, "asking is opt-in");
        assert!(json.contains("\"KiroApproval\": \"Risky\""));
        assert_eq!(Settings::load(file).agent_options(AgentTool::Codex).approval, AgentApproval::Always, "read back");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn a_sessions_access_overrides_the_tools_setting() {
        let o = AgentOptions::default();
        assert_eq!(o.with_access(Some("risky")).approval, AgentApproval::Risky);
        assert!(o.with_access(Some("read")).read_only);
        assert_eq!(o.with_access(Some("nonsense")), o);
        assert_eq!(o.with_access(Some("always")).access_id(true), "always");
        assert_eq!(AgentOptions { read_only: true, ..o.clone() }.access_id(false), "full", "read only that doesn't work isn't offered");
    }

    const DEFAULT_FILE: &str = "{\r\n  \"HoverOpensWorkspace\": true,\r\n  \"NotchItems\": null,\r\n  \"Appearance\": \"System\",\r\n  \"Theme\": null,\r\n  \"WorkspaceSize\": \"Default\",\r\n  \"KiroFolder\": null,\r\n  \"KiroNoticeSeen\": false,\r\n  \"KiroModel\": null,\r\n  \"KiroEffort\": \"high\",\r\n  \"KiroAgent\": null,\r\n  \"KiroReadOnly\": false,\r\n  \"KiroRequireMcp\": false,\r\n  \"KiroIdleMinutes\": 5,\r\n  \"KiroHideSteps\": false,\r\n  \"KiroApproval\": \"Autopilot\",\r\n  \"Agents\": null,\r\n  \"AgentOffers\": null,\r\n  \"AgentTool\": null,\r\n  \"ScWorkspace\": {\r\n    \"Key\": \"N\",\r\n    \"Modifiers\": \"Alt\"\r\n  },\r\n  \"Projects\": null,\r\n  \"Voice\": null,\r\n  \"DefaultWorkspace\": null\r\n}";

    /// A 3.0 file from before projects and voice reads with them off and empty, and
    /// writes them back only as nulls until they are used.
    #[test]
    fn projects_and_voice_are_new_keys_an_older_file_lacks() {
        let f = temp("migrate");
        let old = DEFAULT_FILE.replace(",\r\n  \"Projects\": null,\r\n  \"Voice\": null,\r\n  \"DefaultWorkspace\": null", "");
        std::fs::write(&f, &old).unwrap();
        let s = Settings::load(f.clone());
        assert_eq!(s.model(), Model::default());
        assert!(s.projects().is_empty() && !s.voice().enabled);
        assert_eq!(s.default_workspace().access, "risky");
        let dir = f.parent().unwrap().join("Proj ü");
        std::fs::create_dir_all(&dir).unwrap();
        let p = s.add_project(&dir.to_string_lossy()).unwrap();
        assert_eq!((p.name.as_str(), p.access.as_str(), p.voice), ("Proj ü", "risky", true));
        let again = format!("{}{}", dir.to_string_lossy(), std::path::MAIN_SEPARATOR);
        assert!(s.add_project(&again).unwrap_err().contains("already registered"));
        s.update_project(Project { aliases: vec!["one".into(), "One ".into(), "".into()], access: "bogus".into(), name: "  ".into(), ..p.clone() }).unwrap();
        let u = s.project(&p.id).unwrap();
        assert_eq!((u.name.as_str(), u.aliases.len(), u.access.as_str()), ("Proj ü", 1, "risky"));
        s.flush();
        let back = Settings::load(f.clone());
        assert_eq!(back.projects(), s.projects());
        back.remove_project(&p.id);
        assert!(back.projects().is_empty());
        assert!(dir.is_dir(), "forgetting a project leaves its folder");
        assert!(!f.with_extension("json.tmp").exists(), "the write replaced the file whole");
    }

    #[test]
    fn a_fresh_model_writes_as_system_text_json_writes_it() {
        assert_eq!(Model::default().to_json().indented("\r\n"), DEFAULT_FILE);
        assert_eq!(Model::from_json(&json::parse(DEFAULT_FILE).unwrap()).unwrap(), Model::default());
    }

    /// The macOS build's three toggles: computer use is off, the sandbox and the agent
    /// browser are on until set; none is written until it has been, in Settings.cs' order.
    #[test]
    fn the_integration_toggles_are_written_only_once_set() {
        let s = Settings::load(temp("toggles"));
        assert!(!s.computer_use() && s.sandbox() && s.agent_browser());
        assert_eq!(s.model().text(), Model::default().text(), "unset: the file is as 3.x wrote it");
        s.set_sandbox(false);
        s.set_computer_use(true);
        s.set_agent_browser(true);
        s.flush();
        let text = std::fs::read_to_string(&s.file).unwrap();
        let at = |k: &str| text.find(k).unwrap_or_else(|| panic!("{k} in {text}"));
        assert!(text.contains("\"ComputerUse\": true") && text.contains("\"Sandbox\": false") && text.contains("\"AgentBrowser\": true"));
        assert!(at("\"AgentTool\"") < at("\"ComputerUse\"") && at("\"ComputerUse\"") < at("\"Sandbox\"") && at("\"Sandbox\"") < at("\"AgentBrowser\"") && at("\"AgentBrowser\"") < at("\"ScWorkspace\""));
        let back = Settings::load(s.file.clone());
        assert!(back.computer_use() && !back.sandbox() && back.agent_browser());
        // 2.x's macOS file wrote Sandbox and AgentBrowser as null when never set.
        std::fs::write(&s.file, "{\"ComputerUse\": false, \"Sandbox\": null, \"AgentBrowser\": null}").unwrap();
        let old = Settings::load(s.file.clone());
        assert!(!old.computer_use() && old.sandbox() && old.agent_browser());
    }

    /// Agent desktops (Cua Spaces): off on the macOS image until set, and the file's bytes
    /// don't change until then; they follow AgentBrowser, before ScWorkspace, as Settings.cs
    /// declares them.
    #[test]
    fn agent_desktops_are_off_on_macos_and_written_only_once_set() {
        let s = Settings::load(temp("spaces"));
        assert!(!s.agent_spaces() && s.space_image() == "macos");
        assert_eq!(s.model().text(), Model::default().text(), "unset: the file is as before");
        s.set_agent_browser(true);
        s.set_agent_spaces(true);
        s.set_space_image("linux");
        s.flush();
        let text = std::fs::read_to_string(&s.file).unwrap();
        let at = |k: &str| text.find(k).unwrap_or_else(|| panic!("{k} in {text}"));
        assert!(text.contains("\"AgentSpaces\": true") && text.contains("\"SpaceImage\": \"linux\""), "{text}");
        assert!(at("\"AgentBrowser\"") < at("\"AgentSpaces\"") && at("\"AgentSpaces\"") < at("\"SpaceImage\"") && at("\"SpaceImage\"") < at("\"ScWorkspace\""));
        let back = Settings::load(s.file.clone());
        assert!(back.agent_spaces() && back.space_image() == "linux");
        // Anything but "linux" is the macOS image, and a 2.x file's explicit off / null read as unset.
        back.set_space_image("vmware");
        assert_eq!(back.space_image(), "macos");
        std::fs::write(&s.file, "{\"AgentSpaces\": false, \"SpaceImage\": null}").unwrap();
        let old = Settings::load(s.file.clone());
        assert!(!old.agent_spaces() && old.space_image() == "macos");
    }

    /// Kiro's auto compact: off and 80 % until set, and the file's bytes don't change
    /// until then.
    #[test]
    fn auto_compact_is_off_at_80_and_written_only_once_set() {
        let s = Settings::load(temp("compact"));
        assert!(!s.kiro_auto_compact() && s.kiro_compact_at() == 80 && s.model().auto_compact().is_none());
        assert_eq!(s.model().text(), Model::default().text(), "unset: the file is as before");
        assert!(!Model::default().text().contains("Compact"));
        // A file from before the keys reads back to the same bytes.
        let old = Model::default().text();
        let m = Model::from_json(&json::parse(&old).unwrap()).unwrap();
        assert_eq!(m.text(), old);
        s.set_kiro_auto_compact(true);
        s.set_kiro_compact_at(60);
        s.flush();
        let text = std::fs::read_to_string(&s.file).unwrap();
        let at = |k: &str| text.find(k).unwrap_or_else(|| panic!("{k} in {text}"));
        assert!(text.contains("\"KiroAutoCompact\": true") && text.contains("\"KiroCompactAt\": 60"));
        assert!(at("\"AgentTool\"") < at("\"KiroAutoCompact\"") && at("\"KiroAutoCompact\"") < at("\"KiroCompactAt\"") && at("\"KiroCompactAt\"") < at("\"ScWorkspace\""));
        let back = Settings::load(s.file.clone());
        assert!(back.kiro_auto_compact() && back.kiro_compact_at() == 60 && back.model().auto_compact() == Some(60));
        // On with no percent: 80. A percent off the scale is pulled onto it.
        std::fs::write(&s.file, "{\"KiroAutoCompact\": true, \"KiroCompactAt\": 500}").unwrap();
        assert_eq!(Settings::load(s.file.clone()).model().auto_compact(), Some(100));
        std::fs::write(&s.file, "{\"KiroAutoCompact\": true}").unwrap();
        assert_eq!(Settings::load(s.file.clone()).model().auto_compact(), Some(80));
        std::fs::write(&s.file, "{\"KiroAutoCompact\": null, \"KiroCompactAt\": null}").unwrap();
        assert_eq!(Settings::load(s.file.clone()).model().auto_compact(), None);
    }

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d.join("settings.json")
    }

    /// SettingsTests.Flush_writes_readable_JSON_with_the_shortcut_and_notch_items.
    #[test]
    fn the_shortcut_and_notch_items_as_settings_tests_expect() {
        let s = Settings::load(temp("items"));
        s.set_sc_workspace(Shortcut { key: Key::letter('H').unwrap(), modifiers: Modifiers::CONTROL | Modifiers::SHIFT });
        s.m.lock().unwrap().notch_items = Some(["kiro", "bogus", "clock", "timer", "claude"].iter().map(|x| Some(x.to_string())).collect());
        s.flush();
        let text = std::fs::read_to_string(&s.file).unwrap();
        assert!(text.contains("\"ScWorkspace\""));
        assert!(text.contains("\"Key\": \"H\""));
        assert!(text.contains("\"NotchItems\""));
        assert_eq!(s.notch_items(), vec!["claude", "kiro"]);
        s.set_notch_items(&["kiro"]);
        s.set_notch_item("codex", true);
        assert!(s.has_notch_item("codex"));
        s.set_notch_item("kiro", false);
        assert_eq!(s.notch_items(), vec!["codex"]);
    }

    /// SettingsTests.Kiros_folder_and_the_note_are_saved_and_a_blank_folder_is_none.
    #[test]
    fn the_folder_escapes_its_backslashes_and_blank_is_none() {
        let s = Settings::load(temp("folder"));
        s.set_kiro_folder(Some(r"C:\Projects\Hover"));
        s.set_kiro_notice_seen(true);
        s.flush();
        let text = std::fs::read_to_string(&s.file).unwrap();
        assert!(text.contains("\"KiroFolder\": \"C:\\\\Projects\\\\Hover\""));
        assert!(text.contains("\"KiroNoticeSeen\": true"));
        s.set_kiro_folder(Some("   "));
        assert_eq!(s.kiro_folder(), None);
    }

    /// SetAgentOptions' normalising, and the Agents dictionary's shape.
    #[test]
    fn agent_options_are_kept_as_set_agent_options_keeps_them() {
        let s = Settings::load(temp("agents"));
        s.set_agent_options(AgentTool::Codex, AgentOptions { model: Some("auto".into()), agent: Some("x".into()), require_mcp: true, idle_minutes: 7, read_only: true, ..Default::default() });
        s.set_agent_options(AgentTool::Kiro, AgentOptions { model: Some("claude-opus-5.5".into()), effort: None, agent: Some(" ".into()), idle_minutes: 15, ..Default::default() });
        assert_eq!(s.agent_options(AgentTool::Codex), AgentOptions { read_only: true, ..Default::default() });
        assert_eq!(s.agent_options(AgentTool::Cursor), AgentOptions::default());
        let k = s.agent_options(AgentTool::Kiro);
        assert_eq!((k.model.as_deref(), k.effort.as_deref(), k.agent, k.idle_minutes), (Some("claude-opus-5.5"), Some("high"), None, 15));
        let text = s.model().to_json().indented("\n");
        assert!(text.contains("  \"Agents\": {\n    \"codex\": {\n      \"Model\": null,\n      \"Effort\": null,\n      \"ReadOnly\": true,\n      \"IdleMinutes\": 5,\n      \"Agent\": null,\n      \"RequireMcp\": false,\n      \"HideSteps\": false,\n      \"Approval\": \"Autopilot\"\n    }\n  },"), "{text}");
        let offers = vec![AcpOption { id: "model".into(), category: Some("model".into()), current: Some("a".into()),
            choices: vec![crate::model::AcpChoice { value: "a".into(), name: "A <1>".into(), levels: None }] }];
        s.set_agent_offers(AgentTool::Kiro, &offers);
        assert_eq!(s.agent_offers(AgentTool::Kiro), offers);
        let text = s.model().to_json().indented("\n");
        assert!(text.contains("\"AgentOffers\": {\n    \"kiro\": [\n      {\n        \"Id\": \"model\",\n        \"Category\": \"model\",\n        \"Current\": \"a\",\n        \"Choices\": [\n          {\n            \"Value\": \"a\",\n            \"Name\": \"A \\u003C1\\u003E\",\n            \"Levels\": null\n          }\n        ]\n      }\n    ]\n  },"), "{text}");
        // AcpChoice's Levels (from 55111fc): null for a tool that lists effort apart.
        let with_levels = vec![AcpOption { id: "model".into(), category: Some("model".into()), current: None,
            choices: vec![crate::model::AcpChoice { value: "p/m".into(), name: "M · P".into(), levels: Some(vec!["high".into(), "max".into()]) }] }];
        s.set_agent_offers(AgentTool::OpenCode, &with_levels);
        assert_eq!(s.agent_offers(AgentTool::OpenCode), with_levels);
        assert!(s.model().to_json().indented("\n").contains("\"Levels\": [\n              \"high\",\n              \"max\"\n            ]"));
        // OpenCode keeps its agent (Build, Plan, the user's own); Codex and Cursor don't.
        s.set_agent_options(AgentTool::OpenCode, AgentOptions { agent: Some("plan".into()), require_mcp: true, ..Default::default() });
        assert_eq!(s.agent_options(AgentTool::OpenCode), AgentOptions { agent: Some("plan".into()), ..Default::default() });
        s.set_agent_tool(AgentTool::Cursor);
        assert_eq!(s.agent_tool(), AgentTool::Cursor);
        // A file the model wrote reads back to the same model, and the same text.
        let again = Model::from_json(&json::parse(&s.model().text()).unwrap()).unwrap();
        assert_eq!(again, s.model());
        assert_eq!(again.text(), s.model().text());
    }

    /// Settings.Load: an unreadable file, or a value of the wrong kind anywhere, gives
    /// the defaults; unknown keys and case-different names are ignored.
    #[test]
    fn a_bad_file_gives_the_defaults() {
        let f = temp("bad");
        for bad in ["{", "[]", "{\"KiroIdleMinutes\": 5.0}", "{\"HoverOpensWorkspace\": null}", "{\"Appearance\": \"Blue\"}", "{\"KiroFolder\": 3}"] {
            std::fs::write(&f, bad).unwrap();
            assert_eq!(load_model(&f), Model::default(), "{bad}");
        }
        std::fs::write(&f, "\u{feff}{\"hoverOpensWorkspace\": false, \"Deck\": [1], \"Appearance\": \"dark\", \"WorkspaceSize\": 3, \"KiroFolder\": \"a\", \"KiroFolder\": \"b\"}").unwrap();
        let m = load_model(&f);
        assert!(m.hover_opens_workspace);
        assert_eq!((m.appearance, m.workspace_size, m.kiro_folder.as_deref()), (Appearance::Dark, WorkspaceSize::ExtraLarge, Some("b")));
        std::fs::write(&f, "null").unwrap();
        assert_eq!(load_model(&f), Model::default());
    }

    #[test]
    fn writes_wait_for_the_value_to_settle() {
        let s = Settings::load(temp("debounce"));
        s.set_hover_opens_workspace(false);
        std::thread::sleep(Duration::from_millis(200));
        s.set_kiro_notice_seen(true);
        std::thread::sleep(Duration::from_millis(250));
        assert!(!s.file.exists(), "a change 250 ms ago holds the write back");
        std::thread::sleep(Duration::from_millis(400));
        let m = load_model(&s.file);
        assert!(!m.hover_opens_workspace && m.kiro_notice_seen);
    }
}

//! Voice in the app: Phonon and the voice flow made once, the hold-to-talk shortcut,
//! what Settings asks of them (view::Host::action), and the notch's card for each stage.
//! The flow's state is Voice's own; this only draws it and passes clicks and keys on.

use crate::ui::*;
use crate::{ui_do, view, App};
use hover_app::app::Hover;
use hover_app::pages::{self, PhononAction, PhononCard, Section, TryCard};
use hover_app::phonon::{Install, Phonon};
use hover_app::voice::{Hooks, Preview, Stage, Voice};
use hover_core::model::{AgentOptions, AgentTool, KiroState};
use hover_core::projects::{self, SpeechMode, GROQ_SECRET};
use hover_core::shortcut::Shortcut;
use hover_notch::State;
use slint::{Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Takes the voice chord (Some) or lets go of it (None); Err names the conflict.
pub type HoldFn = Box<dyn Fn(Option<&Shortcut>) -> Result<(), String>>;

/// What the app keeps for voice, beside Voice's own state.
#[derive(Default)]
pub struct Ui {
    /// The interaction now is Settings' Try it: its stages show there, not in the notch.
    trial: Cell<bool>,
    /// The notch took the keyboard for this interaction (and gives it back after).
    focus: Cell<bool>,
    /// The interaction is dictation into the open chat's reply box: no card, its words go there.
    dictating: Cell<bool>,
    /// Clears what dictation said under the reply box (a failure stays a moment).
    dictate_timer: Timer,
    /// The project open in the office when the interaction began (its id).
    active: Arc<Mutex<Option<String>>>,
    /// A change is on its way to the UI thread: the rest wait for it (recording says
    /// so 20 times a second).
    queued: Arc<AtomicBool>,
    /// Why the shortcut couldn't be taken, shown once in the notch until dismissed.
    hold_error: RefCell<Option<String>>,
    hold_error_timer: Timer,
    /// What Remove said when it couldn't.
    phonon_error: RefCell<Option<String>>,
    mics_at: Cell<Option<Instant>>,
    busy_seen: Cell<Option<Instant>>,
    busy_timer: Timer,
    /// Closes the Started and Cancelled cards.
    close_timer: Timer,
    /// The card's kind as last drawn (a level or a second only sets two properties).
    kind: Cell<i32>,
    /// The listening and working cards' aura, between frames.
    aura: RefCell<hover_app::aura::Aura>,
    /// Its colour, read from the settings each time the card is drawn.
    aura_rgb: Cell<[u8; 3]>,
    /// A preview folder's look on the card: (folder, (~/path, letter, tint, home)).
    look: RefCell<Option<(String, (slint::SharedString, slint::SharedString, slint::Color, bool))>>,
    /// A stage to draw instead of Voice's (the shots).
    pub shot: RefCell<Option<Stage>>,
    /// The preview's open menu (0 none, 1 the agents, 2 the model) and the interaction
    /// it was opened for: another interaction finds it closed.
    pub menu: Cell<(i32, u64)>,
    /// The agents ready for that interaction, once checked (off the UI thread).
    pub ready: RefCell<Option<(u64, Vec<AgentTool>)>>,
    /// The stage last written to the log.
    logged: Cell<Option<std::mem::Discriminant<Stage>>>,
    pub hold: RefCell<Option<HoldFn>>,
}

/// Phonon (from the disk only) and Voice with the app's hooks, once at start.
pub fn make(hover: &Arc<Hover>) -> (Ui, Arc<Phonon>, Arc<Voice>) {
    let ui = Ui::default();
    let active = ui.active.clone();
    let phonon = Phonon::new(hover.settings.clone());
    let (h1, h2) = (hover.clone(), hover.clone());
    let hooks = Hooks {
        router: Box::new(move |t| h1.runner(t)),
        start: Box::new(move |t, folder, prompt, access, cloud| start(&h2, t, folder, prompt, access, cloud)),
        // Installed and signed in, by the tool's own status command (kept five minutes).
        available: Box::new(|t| hover_agents::agents::check(t, false).ok()),
        active_project: Box::new(move || active.lock().unwrap().clone()),
    };
    let voice = Voice::new(hover.settings.clone(), view::secrets(), phonon.clone(), hooks);
    (ui, phonon, voice)
}

/// A new chat for a voice task, through the same start the office's new-task box uses.
/// Ok only once the tool took it (it named the conversation, or the turn ended well).
/// Called on Voice's worker thread, so it may wait.
fn start(h: &Hover, tool: AgentTool, folder: &str, prompt: &str, access: &str, cloud: bool) -> Result<i32, String> {
    // In Kiro Web: the folder's GitHub repo, else an empty workspace; always Full.
    let (access, cloud) = if cloud { ("full", Some(hover_agents::desk::Desk::shared().github_repo(folder).into_iter().collect::<Vec<_>>())) } else { (access, None) };
    // with_access("read") on a tool with no read only mode would run it with its own
    // setting: more than the target allows.
    if access == "read" && !hover_agents::agents::read_only_works(tool) {
        return Err(format!("{} has no read only mode on this computer, so Hover won’t start it here. Change the target’s access in Settings → Projects, or pick another agent.", tool.name()));
    }
    let Some(s) = h.sessions.start_in(tool, folder, prompt, vec![], Some(access), cloud) else {
        return Err(if h.sessions.can_start() { "Hover couldn’t start the task." } else { "Three tasks are running already. Start this one when one of them ends." }.into());
    };
    hover_core::log::line(&format!("voice: run {} started ({}, access {access})", s.id, tool.name().to_lowercase()));
    let t0 = Instant::now();
    loop {
        match h.sessions.get(s.id) {
            None => return Err("The task’s chat was closed before the agent took it.".into()),
            Some(x) if x.kiro_id.is_some() => return Ok(s.id),
            Some(x) if !x.busy() => {
                return match x.result() {
                    Some(r) if r.state == KiroState::Completed => Ok(s.id),
                    Some(r) => Err(Some(hover_agents::text::first_line(&hover_agents::text::plain(&r.text))).filter(|t| !t.is_empty())
                        .unwrap_or_else(|| format!("{} couldn’t start the task.", tool.name()))),
                    None => Err(format!("{} couldn’t start the task.", tool.name())),
                };
            }
            _ => {}
        }
        // ponytail: a tool that neither answers nor fails for ten minutes is treated as
        // having taken it; its chat in the office says how it is going.
        if t0.elapsed() > Duration::from_secs(600) { return Ok(s.id); }
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// A folder for the card: under the home folder as ~/…
fn tilde(folder: &str) -> String {
    let home = hover_core::platform::home().map(|h| h.to_string_lossy().into_owned()).unwrap_or_default();
    let under = !home.is_empty() && folder.get(..home.len()).is_some_and(|h| projects::same_folder(h, &home))
        && folder[home.len()..].chars().next().is_none_or(std::path::is_separator);
    if under { format!("~{}", &folder[home.len()..]).replace('\\', "/") } else { folder.to_owned() }
}

fn s(v: impl AsRef<str>) -> slint::SharedString { slint::SharedString::from(v.as_ref()) }

/// The card's state words while Hover works on what was said.
fn working(stage: &Stage) -> Option<&'static str> {
    Some(match stage {
        Stage::Loading => "Starting local speech…",
        Stage::Transcribing => "Transcribing…",
        Stage::Cleaning => "Cleaning up the text…",
        Stage::Resolving => "Finding the project…",
        _ => return None,
    })
}

impl App {
    /// Called once the notch's window exists: the hooks from Voice and Phonon, and the
    /// shortcut.
    pub fn voice_start(self: &Rc<Self>) {
        let q = self.voice_ui.queued.clone();
        self.voice.on_change(move || {
            if q.swap(true, Ordering::SeqCst) { return; }
            ui_do(|a| { a.voice_ui.queued.store(false, Ordering::SeqCst); a.voice_changed(); });
        });
        self.phonon.on_change(|| ui_do(|a| a.phonon_changed()));
        self.fill_phonon();
    }

    /// Takes the voice chord when voice is on (letting go of it otherwise). A refusal
    /// shows under the shortcut in Settings, and once in the notch.
    pub fn register_voice(self: &Rc<Self>) {
        let v = self.hover.settings.voice();
        let r = match &*self.voice_ui.hold.borrow() { Some(f) => f(v.enabled.then_some(&v.shortcut)), None => Ok(()) };
        let err = r.err();
        let changed = self.pane.borrow().live.shortcut_error != err;
        if !changed { return; }
        self.pane.borrow_mut().live.shortcut_error = err.clone();
        if let Some(e) = err {
            *self.voice_ui.hold_error.borrow_mut() = Some(e);
            let a = self.clone();
            self.voice_ui.hold_error_timer.start(TimerMode::SingleShot, Duration::from_secs(12), move || { a.voice_ui.hold_error.borrow_mut().take(); a.update_rest(); });
        } else {
            self.voice_ui.hold_error.borrow_mut().take();
        }
        self.update_rest();
        self.refresh_page(false);
    }

    /// The chord went down (or Try it was pressed).
    pub fn voice_press(self: &Rc<Self>, trial: bool) {
        let stage = self.voice.stage();
        // A Try it's result waits in Settings until the next press: the shortcut's press
        // replaces it rather than being turned away as busy.
        if !trial && self.voice_ui.trial.get() && matches!(stage, Stage::Preview(_)) { self.voice.cancel(); }
        let fresh = matches!(self.voice.stage(), Stage::Idle | Stage::Started { .. } | Stage::Dictated(_) | Stage::Cancelled | Stage::Error { .. });
        // Over an open chat with its reply box open, the words go into the reply: the office
        // stays open and nothing is routed or started.
        if !trial && fresh && self.dictation_here() {
            self.voice_ui.trial.set(false);
            self.voice_ui.dictating.set(true);
            self.voice_ui.dictate_timer.stop();
            self.voice_ui.hold_error.borrow_mut().take();
            self.voice.dictate();
            return;
        }
        if fresh {
            self.voice_ui.trial.set(trial);
            *self.voice_ui.active.lock().unwrap() = self.active_project();
            self.voice_ui.hold_error.borrow_mut().take();
            self.voice_ui.close_timer.stop();
            if !trial {
                // The card is at rest in the notch: an open office folds away first (its
                // sessions go on). The keyboard comes here for Esc, Enter and the task.
                if self.n.borrow().hover.state != State::Rest { self.collapse(); }
                if self.card.get() { self.close_card(false); }
                if !self.voice_ui.focus.get() && !self.headless {
                    let n = self.n.borrow();
                    n.plat.remember_foreground();
                    n.plat.set_accepts_keys(true);
                    n.plat.focus();
                    self.voice_ui.focus.set(true);
                }
                self.notch.invoke_focus_voice();
            }
        }
        self.voice.press(trial);
    }

    /// Dictation's place: an office in view (the open notch's or the app window's, not
    /// under Settings) whose chat has its reply box open, with the pointer over that chat.
    pub(crate) fn dictation_here(&self) -> bool {
        let ready = |g: Office| g.get_drawer() && g.get_d_compose() && g.get_d_hover();
        let notch = (self.n.borrow().hover.state != State::Rest || self.headless) && !self.notch_settings.get() && ready(self.notch.global::<Office>());
        notch || self.dash.borrow().as_ref().is_some_and(|d| d.window().is_visible() && !self.dash_settings.get() && ready(d.global::<Office>()))
    }

    /// A dictation stage as if Voice had reached it (the shots, which have no microphone).
    pub(crate) fn dictation_shot(self: &Rc<Self>, stage: &Stage) { self.voice_ui.dictating.set(true); self.dictation_changed(stage); }

    /// Dictation's stages, under the reply box; its words, once heard, written into it.
    fn dictation_changed(self: &Rc<Self>, stage: &Stage) {
        let say = |t: &str| { self.notch.global::<Office>().set_d_voice(s(t)); if let Some(d) = &*self.dash.borrow() { d.global::<Office>().set_d_voice(s(t)); } };
        match stage {
            Stage::Recording { .. } => say("Listening…"),
            Stage::Loading | Stage::Transcribing | Stage::Cleaning => say("Writing it down…"),
            Stage::Dictated(text) => {
                let put = |g: Office| {
                    let d = g.get_d_draft();
                    let gap = if d.is_empty() || d.ends_with(char::is_whitespace) { "" } else { " " };
                    g.set_d_draft(s(format!("{d}{gap}{text}")));
                    g.set_d_draft_to_end(g.get_d_draft_to_end().wrapping_add(1));
                    g.set_d_compose(true);
                };
                put(self.notch.global::<Office>());
                if let Some(d) = &*self.dash.borrow() { put(d.global::<Office>()); }
                say("");
                self.voice_ui.dictating.set(false);
                self.voice.dismiss();
            }
            Stage::Error { message, .. } => {
                say(message);
                let a = self.clone();
                self.voice_ui.dictate_timer.start(TimerMode::SingleShot, Duration::from_secs(5), move || {
                    if !a.voice_ui.dictating.get() { a.notch.global::<Office>().set_d_voice(s("")); if let Some(d) = &*a.dash.borrow() { d.global::<Office>().set_d_voice(s("")); } }
                });
                self.voice_ui.dictating.set(false);
                self.voice.dismiss();
            }
            Stage::Idle | Stage::Cancelled => { say(""); self.voice_ui.dictating.set(false); self.voice.dismiss(); }
            _ => {}
        }
    }

    /// The registered, voice-enabled project whose folder is the chat open in the office
    /// (or the new-task box's folder), if any.
    fn active_project(&self) -> Option<String> {
        let folder = self.selected_folder()?;
        self.hover.settings.projects().into_iter().find(|p| p.voice && projects::same_folder(&p.folder, &folder)).map(|p| p.id)
    }

    /// Voice changed (on the UI thread): the notch's card or Settings' Try it.
    pub fn voice_changed(self: &Rc<Self>) {
        let stage = self.shown();
        // Each new stage in the log (its name only: never what was said).
        let d = std::mem::discriminant(&stage);
        if self.voice_ui.logged.replace(Some(d)) != Some(d) {
            let name = format!("{stage:?}");
            hover_core::log::line(&format!("voice: {}", name.split([' ', '(', '{']).next().unwrap_or("").to_lowercase()));
        }
        if let Some(b) = self.voice.busy_since() {
            if self.voice_ui.busy_seen.replace(Some(b)) != Some(b) { self.flash_busy(); }
        }
        if self.voice_ui.dictating.get() { return self.dictation_changed(&stage); }
        if self.voice_ui.trial.get() {
            let t = try_card(&stage, &self.hover.settings);
            if self.pane.borrow().live.voice_try != t {
                self.pane.borrow_mut().live.voice_try = t;
                if self.pane.borrow().section == Section::Voice { self.refresh_page(false); }
            }
            return;
        }
        match &stage {
            Stage::Started { session, .. } => {
                let (a, id) = (self.clone(), *session);
                self.voice_ui.close_timer.start(TimerMode::SingleShot, Duration::from_secs(4), move || {
                    if matches!(a.voice.stage(), Stage::Started { session, .. } if session == id) { a.voice.dismiss(); }
                });
            }
            Stage::Cancelled => {
                let a = self.clone();
                self.voice_ui.close_timer.start(TimerMode::SingleShot, Duration::from_millis(1500), move || {
                    if a.voice.stage() == Stage::Cancelled { a.voice.dismiss(); }
                });
            }
            _ => {}
        }
        // The card has nothing more to type into: the keyboard goes back to what had it.
        if matches!(stage, Stage::Idle | Stage::Started { .. } | Stage::Cancelled) { self.voice_give_back(); }
        let card = self.voice_card(&stage);
        let kind = card.as_ref().map_or(0, |c| c.kind);
        // The same card again (a level, a second, the countdown): its properties only.
        // The poll picks up a new height and springs the shape to it.
        if kind != 0 && kind == self.voice_ui.kind.get() {
            if let Some(c) = card { self.notch.set_voice(c); }
            if let Stage::Recording { level, .. } = stage { self.notch.set_voice_level(level); }
            self.voice_menu_draw(&stage);
            // With the clock off (animations off) the aura still answers the voice.
            if !self.clock_timer.running() { self.aura_draw(); }
            return;
        }
        self.update_rest();
        // The card is in now: the keyboard the press took for the notch goes to it.
        if kind != 0 && self.voice_ui.focus.get() { self.notch.invoke_focus_voice(); }
    }

    fn voice_give_back(self: &Rc<Self>) {
        if !self.voice_ui.focus.replace(false) { return; }
        let at_rest = self.n.borrow().hover.state == State::Rest;
        if at_rest && !self.card.get() {
            let n = self.n.borrow();
            n.plat.restore_foreground();
            n.plat.set_accepts_keys(false);
        }
    }

    /// A press while one is in progress: the card glows amber a moment.
    fn flash_busy(self: &Rc<Self>) {
        self.notch.set_voice_busy(true);
        if self.n.borrow().hover.state == State::Rest { self.notch.set_glow(slint::Color::from_rgb_u8(0xff, 0xb3, 0x40)); }
        let a = self.clone();
        self.voice_ui.busy_timer.start(TimerMode::SingleShot, Duration::from_millis(600), move || { a.notch.set_voice_busy(false); a.update_rest(); });
    }

    /// The notch's voice card for a stage: None when it shows nothing (idle, or Try it's).
    pub fn voice_card(&self, stage: &Stage) -> Option<VoiceCard> {
        let mut c = VoiceCard { ring: -1.0, ..Default::default() };
        if self.voice_ui.trial.get() && !matches!(stage, Stage::Idle) { return None; }
        match stage {
            Stage::Idle => {
                let e = self.voice_ui.hold_error.borrow().clone()?;
                c.kind = 7;
                c.words = s(e);
                c.sub = s("Voice can’t listen until another shortcut is picked.");
                c.settings = true;
            }
            Stage::Recording { secs, .. } => { c.kind = 1; c.words = s("Listening…"); c.sub = s(hover_app::rest::clock(*secs as f64)); }
            st if working(st).is_some() => { c.kind = 2; c.words = s(working(st).unwrap_or_default()); }
            Stage::Preview(p) | Stage::Editing(p) | Stage::Starting(p) => {
                c.kind = 3;
                self.fill_preview(&mut c, p);
                c.starting = matches!(stage, Stage::Starting(_));
                c.can_start = if self.voice_ui.shot.borrow().is_some() { !p.trial && !c.starting } else { self.voice.can_start() };
                c.start = s(if c.starting { "Starting…" } else if matches!(stage, Stage::Editing(_)) && !c.can_start { "Checking…" } else { "Start" });
            }
            Stage::ChooseAgent(p) => {
                c.kind = 4;
                c.heard = s(&p.text);
                let st = &self.hover.settings;
                let t = self.voice.tool().unwrap_or_else(|| st.voice().agent.unwrap_or_else(|| st.agent_tool()));
                c.words = s(if p.tools.is_empty() { format!("{} isn’t ready, and no other agent is. Set one up in Settings.", t.name()) }
                    else { format!("{} isn’t ready. Pick an agent for this task.", t.name()) });
                c.settings = p.tools.is_empty();
            }
            Stage::Started { session, folder } => {
                c.kind = 5;
                let x = self.hover.sessions.get(*session);
                c.tool = s(x.as_ref().map_or("kiro", |x| x.tool.id()));
                c.words = s(x.map(|x| x.title()).unwrap_or_default());
                let st = &self.hover.settings;
                let name = match st.projects().into_iter().find(|p| projects::same_folder(&p.folder, folder)) {
                    Some(p) => p.name,
                    None if st.default_workspace().path().is_some_and(|w| projects::same_folder(&w.to_string_lossy(), folder)) => "the default workspace".into(),
                    None => hover_office::office::short(folder),
                };
                c.sub = s(format!("Started in {name}"));
            }
            Stage::Cancelled => { c.kind = 6; c.words = s("Cancelled"); c.sub = s("Nothing started"); }
            Stage::Error { message, retry, transcript } => {
                c.kind = 7;
                c.words = s(message);
                c.sub = s(transcript.as_ref().map_or_else(|| "Nothing started".to_owned(), |t| format!("“{t}”")));
                c.retry = *retry;
                c.settings = !*retry && message.contains("Settings");
            }
            _ => return None,
        }
        Some(c)
    }

    fn fill_preview(&self, c: &mut VoiceCard, p: &Preview) {
        let st = &self.hover.settings;
        c.heard = s(&p.heard);
        c.note = s([p.note.clone(), p.cleanup_note.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "));
        c.target = s(&p.target_name);
        // The folder's look is read from the disk once per folder, not per countdown tick.
        let cached = self.voice_ui.look.borrow().as_ref().filter(|(f, _)| *f == p.folder).map(|x| x.1.clone());
        let look = cached.unwrap_or_else(|| {
            let look = match st.projects().into_iter().find(|x| projects::same_folder(&x.folder, &p.folder)).map(|x| pages::letter(&x)) {
                Some(pages::Lead::Letter(l, t)) => (s(tilde(&p.folder)), s(l), view::tint(t, &self.palette.borrow()), false),
                _ => (s(tilde(&p.folder)), s(""), slint::Color::default(), true),
            };
            *self.voice_ui.look.borrow_mut() = Some((p.folder.clone(), look.clone()));
            look
        });
        (c.folder, c.letter, c.tint, c.home) = look;
        c.tool = s(p.tool.id());
        c.agent = s(p.tool.name());
        // The office's pill: the model's name ("Default" when the tool lists none).
        c.model = s(self.pill(p.tool).0);
        let cloud = p.cloud && p.tool == AgentTool::Kiro;
        c.access = s(pages::access_label(if cloud { "full" } else { &p.access }));
        c.full = cloud || p.access == "full";
        c.cloud_shown = p.tool == AgentTool::Kiro;
        c.cloud = cloud;
        c.task = s(&p.task);
        // What is left of the countdown the preview started with (a shot's has none: Settings').
        let total = match self.voice.countdown_total().as_secs_f32() { t if t > 0.0 => t, _ => st.voice().countdown.max(1) as f32 };
        c.ring = p.countdown.map_or(-1.0, |l| (l / total * 100.0).clamp(0.0, 100.0));
    }

    /// The stage drawn: Voice's, or the one a shot set.
    fn shown(&self) -> Stage { self.voice_ui.shot.borrow().clone().unwrap_or_else(|| self.voice.stage()) }

    /// The agents the card lists: ChooseAgent's, or the open agent menu's (the one in use
    /// while the rest are checked).
    fn voice_tools(&self, stage: &Stage) -> Vec<VoiceTool> {
        let row = |t: &AgentTool| VoiceTool { id: s(t.id()), name: s(t.name()) };
        match stage {
            Stage::ChooseAgent(p) => p.tools.iter().map(row).collect(),
            Stage::Preview(p) | Stage::Editing(p) if self.voice_ui.menu.get() == (1, p.id) => match &*self.voice_ui.ready.borrow() {
                Some((id, l)) if *id == p.id => l.iter().map(row).collect(),
                _ => vec![row(&p.tool)],
            },
            _ => vec![],
        }
    }

    /// The preview a menu can be open on: one counting down or stopped, not a trial.
    fn menu_preview(&self, stage: &Stage) -> Option<Preview> {
        match stage { Stage::Preview(p) | Stage::Editing(p) if !p.trial => Some(p.clone()), _ => None }
    }

    /// The card's menu as it is now; one for an interaction gone (or a card past its
    /// preview) is closed.
    fn voice_menu_draw(&self, stage: &Stage) {
        let p = self.menu_preview(stage);
        let (which, id) = self.voice_ui.menu.get();
        let which = match &p { Some(p) if id == p.id => which, _ => { self.voice_ui.menu.set((0, 0)); 0 } };
        let n = &self.notch;
        n.set_voice_menu(which);
        if let Some(m) = view::sync(n.get_voice_tools(), &self.voice_tools(stage)) { n.set_voice_tools(m); }
        let (mut head, mut models, mut effort_head, mut efforts, mut note) = (String::new(), vec![], String::new(), vec![], String::new());
        match (which, &p) {
            (1, Some(p)) => {
                head = "AGENT FOR THIS TASK".into();
                let checked = self.voice_ui.ready.borrow().as_ref().is_some_and(|r| r.0 == p.id);
                note = if checked { "Agents that are installed and signed in. The default is in Settings → Voice." } else { "Checking which agents are ready…" }.into();
            }
            (2, Some(p)) => (head, models, effort_head, efforts, note) = self.model_rows(p.tool),
            _ => {}
        }
        n.set_voice_menu_head(s(head));
        n.set_voice_menu_note(s(note));
        n.set_voice_mm_effort_head(s(effort_head));
        if let Some(m) = view::sync(n.get_voice_mm_models(), &models) { n.set_voice_mm_models(m); }
        if let Some(m) = view::sync(n.get_voice_mm_efforts(), &efforts) { n.set_voice_mm_efforts(m); }
    }

    /// Opens a menu on the preview (0 closes it). Opening stops the countdown for good,
    /// so it can't start the task mid-choice; the agents are checked off the UI thread.
    fn voice_open_menu(self: &Rc<Self>, which: i32) {
        let stage = self.shown();
        let Some(p) = self.menu_preview(&stage) else { self.voice_ui.menu.set((0, 0)); return self.voice_menu_draw(&stage); };
        self.voice_ui.menu.set((which, p.id));
        if which != 0 { self.voice.hold(); }
        let checked = self.voice_ui.ready.borrow().as_ref().is_some_and(|r| r.0 == p.id);
        if which == 1 && !checked && self.voice_ui.shot.borrow().is_none() {
            let (v, id) = (self.voice.clone(), p.id);
            std::thread::spawn(move || {
                let tools = v.ready_tools();
                ui_do(move |a| { *a.voice_ui.ready.borrow_mut() = Some((id, tools)); a.voice_menu_draw(&a.shown()); });
            });
        }
        self.voice_menu_draw(&self.shown());
    }

    /// update_rest's part: the card for the stage now, drawn; its kind (0: none).
    pub fn voice_draw(&self) -> i32 {
        let stage = self.shown();
        let card = self.voice_card(&stage);
        let kind = card.as_ref().map_or(0, |c| c.kind);
        if let Some(c) = card { self.notch.set_voice(c); }
        self.notch.set_voice_level(if let Stage::Recording { level, .. } = stage { level } else { 0.0 });
        self.voice_menu_draw(&stage);
        self.voice_ui.kind.set(kind);
        self.voice_ui.aura_rgb.set(projects::VoiceSettings::rgb(self.hover.settings.voice().aura()));
        self.aura_draw();
        kind
    }

    /// The aura's next frame, while the listening or working card shows (the clock's
    /// tick calls it too). Gone, it starts afresh the next time.
    pub fn aura_draw(&self) {
        use hover_app::aura::Mode;
        let mode = match self.voice_ui.kind.get() { 1 => Mode::Listening, 2 => Mode::Working, _ => return self.voice_ui.aura.borrow_mut().reset() };
        let n = &self.notch;
        let px = (66.0 * n.window().scale_factor()).round() as u32;
        let snap = self.voice_ui.shot.borrow().is_some() || !self.look.get().animations;
        let img = self.voice_ui.aura.borrow_mut().frame(mode, n.global::<Clock>().get_t(), n.get_voice_level(), px, snap, self.voice_ui.aura_rgb.get());
        n.set_voice_aura(img);
    }

    /// The card's buttons and keys.
    pub fn wire_voice(self: &Rc<Self>) {
        let a = self.clone();
        self.notch.on_voice_start(move || a.voice.start_now());
        let a = self.clone();
        self.notch.on_voice_toggle_cloud(move || a.voice.toggle_cloud());
        let a = self.clone();
        self.notch.on_voice_cancel(move || {
            if a.voice.stage() == Stage::Idle { a.voice_ui.hold_error.borrow_mut().take(); a.update_rest(); } else { a.voice.cancel(); }
        });
        let a = self.clone();
        self.notch.on_voice_edit(move |t| a.voice.edit(&t));
        let a = self.clone();
        self.notch.on_voice_retry(move || a.voice.retry());
        let a = self.clone();
        self.notch.on_voice_pick(move |id| {
            let Some(t) = AgentTool::parse(Some(&id)) else { return };
            // ChooseAgent's pick; otherwise the preview's agent menu, for this task only.
            if matches!(a.shown(), Stage::ChooseAgent(_)) {
                a.voice.choose_agent(t);
            } else {
                a.voice_ui.menu.set((0, 0));
                a.voice.change_agent(t);
                a.voice_menu_draw(&a.shown());
            }
        });
        let a = self.clone();
        self.notch.on_voice_open_menu(move |which| a.voice_open_menu(which));
        // The model and effort picks are the tool's own from then on, as the office's
        // new-task box writes them ("Used by … from its next turn").
        let a = self.clone();
        self.notch.on_voice_pick_model(move |id| {
            let Some(p) = a.menu_preview(&a.shown()) else { return };
            let o = a.hover.settings.agent_options(p.tool);
            // Default is no model: an empty id would be sent as one.
            a.hover.settings.set_agent_options(p.tool, AgentOptions { model: (!id.is_empty()).then(|| id.to_string()), ..o });
            a.voice_ui.menu.set((0, 0));
            a.voice.change_agent(p.tool);
            a.voice_menu_draw(&a.shown());
            a.update_rest();
        });
        let a = self.clone();
        self.notch.on_voice_pick_effort(move |e| {
            let Some(p) = a.menu_preview(&a.shown()) else { return };
            let o = a.hover.settings.agent_options(p.tool);
            a.hover.settings.set_agent_options(p.tool, AgentOptions { effort: Some(e.to_string()), ..o });
            a.voice.hold();
            a.voice_menu_draw(&a.shown());
        });
        let a = self.clone();
        self.notch.on_voice_settings(move || {
            a.voice_ui.hold_error.borrow_mut().take();
            a.voice.dismiss();
            if matches!(a.voice.stage(), Stage::ChooseAgent(_)) { a.voice.cancel(); }
            // The office takes the keyboard from here.
            a.voice_ui.focus.set(false);
            a.expand(false, true);
            a.show_settings_in(0, Section::Voice);
        });
    }

    /// A click in the card while the keyboard is elsewhere: it comes to the card.
    pub fn voice_clicked(self: &Rc<Self>) {
        if self.voice_ui.focus.get() || self.headless || self.n.borrow().rest_kind != 3 { return; }
        if !matches!(self.voice.stage(), Stage::Preview(_) | Stage::Editing(_) | Stage::ChooseAgent(_) | Stage::Error { .. }) { return; }
        let n = self.n.borrow();
        n.plat.remember_foreground();
        n.plat.set_accepts_keys(true);
        n.plat.focus();
        self.voice_ui.focus.set(true);
    }

    // MARK: Settings

    /// Settings' actions (view::Host::action).
    pub fn voice_action(self: &Rc<Self>, id: &str) {
        match id {
            "phonon.download" | "phonon.retry" => { self.voice_ui.phonon_error.borrow_mut().take(); self.phonon.download(); }
            "phonon.cancel" => self.phonon.cancel(),
            "phonon.repair" => { self.voice_ui.phonon_error.borrow_mut().take(); self.phonon.repair(); }
            "phonon.remove" => *self.voice_ui.phonon_error.borrow_mut() = self.phonon.remove().err(),
            "voice.try.press" => self.voice_press(true),
            "voice.try.release" => self.voice.release(),
            "groq.check" => {
                self.pane.borrow_mut().live.groq_check = Some("Checking…".into());
                let key = view::secrets().get(GROQ_SECRET);
                std::thread::spawn(move || {
                    let r = match key { Some(k) => Voice::check_groq(&k), None => Err("Add your Groq key first.".into()) };
                    let said = r.map_or_else(|e| e, |_| "The key works.".to_owned());
                    ui_do(move |a| { a.pane.borrow_mut().live.groq_check = Some(said); a.refresh_page(false); });
                });
            }
            "voice.shortcut" => self.register_voice(),
            "voice.changed" => {
                let v = self.hover.settings.voice();
                if !v.enabled && !self.voice_ui.trial.get() { self.voice.cancel(); }
                self.register_voice();
                // Phonon's helper runs only while it transcribes, and Voice stops it after
                // each one; an interaction under way finishes in the mode it began in.
                let idle = matches!(self.voice.stage(), Stage::Idle | Stage::Started { .. } | Stage::Cancelled | Stage::Error { .. });
                if (!v.enabled || v.speech != SpeechMode::Local) && idle { self.phonon.shutdown(); }
            }
            _ => {}
        }
        self.fill_phonon();
    }

    fn phonon_changed(self: &Rc<Self>) {
        self.fill_phonon();
        if self.pane.borrow().section == Section::Voice { self.refresh_page(false); }
    }

    /// The Phonon card from its state and facts.
    pub fn fill_phonon(&self) {
        let card = phonon_card(&self.phonon, self.voice_ui.phonon_error.borrow().clone());
        self.pane.borrow_mut().live.phonon = Some(card);
    }

    /// The microphones, read off the UI thread when the Voice page opens (at most every 10 s).
    pub fn load_mics(&self) {
        if self.headless || self.voice_ui.mics_at.get().is_some_and(|t| t.elapsed() < Duration::from_secs(10)) { return; }
        self.voice_ui.mics_at.set(Some(Instant::now()));
        std::thread::spawn(|| {
            let mics = Voice::microphones();
            ui_do(move |a| {
                if a.pane.borrow().live.mics == mics { return; }
                a.pane.borrow_mut().live.mics = mics;
                a.refresh_page(false);
            });
        });
    }

    /// On quit: a recording or a transcription stops, and Phonon's helper with it.
    pub fn voice_quit(&self) {
        if let Some(f) = &*self.voice_ui.hold.borrow() { let _ = f(None); }
        self.voice.cancel();
        self.phonon.shutdown();
    }
}

pub fn phonon_card(p: &Arc<Phonon>, removing: Option<String>) -> PhononCard {
    use PhononAction::*;
    let f = p.facts();
    let state = p.state();
    // A failed or cancelled repair leaves a working install: it can still be removed.
    let kept = || p.speech().is_some();
    let (label, progress, mut actions, error) = match state {
        Install::NotInstalled => ("Not installed", None, vec![Download], None),
        // The Visual C++ runtime can be installed and Download pressed again (it checks
        // again before anything is fetched).
        Install::Unsupported(why) => ("Can’t run on this computer", None, if why.contains("Visual C++") { vec![Download] } else { vec![] }, Some(why)),
        Install::Downloading { done, total } => ("Downloading", Some((done, total)), vec![Cancel], None),
        Install::Verifying => ("Verifying…", None, vec![Cancel], None),
        Install::Installing => ("Installing…", None, vec![Cancel], None),
        Install::Ready => ("Ready", None, vec![Repair, Remove], None),
        Install::Cancelled => ("Cancelled", None, if kept() { vec![Retry, Remove] } else { vec![Retry] }, None),
        Install::Failed(e) => ("Failed", None, if kept() { vec![Retry, Remove] } else { vec![Retry] }, Some(e)),
    };
    if removing.is_some() && !actions.contains(&Remove) { actions.push(Remove); }
    PhononCard {
        model: f.model.into(), state: label.into(), progress,
        facts: vec![
            ("Version".into(), f.version),
            ("Download".into(), pages::size(f.download_bytes)),
            ("Installed size".into(), pages::size(f.disk_bytes)),
            ("Free space for setup".into(), pages::size(f.peak_disk_bytes)),
            ("Runs on".into(), "This computer’s processor, offline".into()),
            ("Folder".into(), f.folder.to_string_lossy().into_owned()),
        ],
        actions, error: removing.or(error),
    }
}

/// Try it's card for a stage of a trial.
pub fn try_card(stage: &Stage, settings: &hover_core::settings::Settings) -> Option<TryCard> {
    let mut t = TryCard::default();
    match stage {
        Stage::Idle => return None,
        Stage::Recording { secs, .. } => t.status = format!("Listening… {} s", secs.floor() as i64),
        st if working(st).is_some() => t.status = working(st).unwrap_or_default().into(),
        Stage::Preview(p) | Stage::Editing(p) | Stage::Starting(p) => {
            t.status = "Done. Nothing was started.".into();
            let models = pages::models(p.tool, &settings.agent_offers(p.tool));
            let model = models.iter().find(|m| m.0 == p.model).map_or_else(|| "Default".to_owned(), |m| m.1.clone());
            t.lines = vec![("Heard".into(), p.heard.clone())];
            if let Some(n) = &p.cleanup_note { t.lines.push(("Cleanup".into(), n.clone())); }
            t.lines.push(("Folder".into(), format!("{} · {}", p.target_name, p.folder)));
            if let Some(n) = &p.note { t.lines.push(("Why".into(), n.clone())); }
            t.lines.push(("Agent".into(), format!("{} · {model}", p.tool.name())));
            t.lines.push(("Access".into(), pages::access_label(&p.access).into()));
            t.lines.push(("Task".into(), p.task.clone()));
        }
        Stage::ChooseAgent(p) => { t.status = "The default agent isn’t ready.".into(); t.lines = vec![("Heard".into(), p.text.clone())]; }
        Stage::Started { .. } => t.status = "Done.".into(),
        Stage::Cancelled => t.status = "Cancelled.".into(),
        Stage::Error { message, transcript, .. } => {
            t.error = Some(message.clone());
            if let Some(x) = transcript { t.lines = vec![("Heard".into(), x.clone())]; }
        }
        _ => return None,
    }
    Some(t)
}

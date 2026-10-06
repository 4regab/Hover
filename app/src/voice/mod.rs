//! Voice: press the shortcut, speak, press it again (or hold it and let go, in hold mode). The recording becomes text (Groq in the
//! cloud, or Phonon on this computer: the user's choice, read once per recording and
//! never switched behind their back), is optionally tidied, routed to a registered
//! project or the default workspace, and shown as a preview that starts a new chat
//! after three seconds unless it is edited or cancelled. One interaction at a time; all
//! of it on worker threads, the UI only told something changed.
//!
//! Every interaction has an id. Cancel moves the id on, so whatever an old worker
//! finishes later is dropped; the countdown, Start and Enter all go through one locked
//! transition, so only one of them starts the task.

pub mod audio;
pub mod cleanup;
pub mod groq;
pub mod wav;

use crate::speech::{Speech, SpeechError};
use hover_agents::cancel::Cancel;
use hover_agents::route::{self, Routed, Target, Why};
use hover_agents::session::RunTask;
use hover_core::model::AgentTool;
use hover_core::projects::{self, Project, SpeechMode, VoiceSettings, Workspace};
use hover_core::secrets::Secrets;
use hover_core::settings::Settings;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

/// How long the default agent gets to say which project a request is for.
const ROUTE_LIMIT: Duration = Duration::from_secs(60);
/// An edit is routed again once typing pauses this long.
const EDIT_SETTLE: Duration = Duration::from_millis(300);

const NOTHING: &str = "Nothing was heard. Start again with the shortcut, and speak a little closer to the microphone.";

#[derive(Clone, Debug, PartialEq)]
pub enum Stage {
    Idle,
    /// level 0..1 from the real audio; seconds recorded.
    Recording { level: f32, secs: f32 },
    /// The local model is starting (and transcribing: Phonon does both in one call).
    Loading,
    Transcribing,
    Cleaning,
    Resolving,
    /// The default agent isn't available: pick another for this task.
    ChooseAgent(Pending),
    /// Counting down (countdown = Some(seconds left)); a trial or a review has None.
    Preview(Preview),
    /// The countdown stopped for good; Start once `can_start`.
    Editing(Preview),
    Starting(Preview),
    Started { session: i32, folder: String },
    /// Dictation: the words for the chat's reply box (the UI writes them in, then dismisses).
    Dictated(String),
    Cancelled,
    Error { message: String, retry: bool, transcript: Option<String> },
}

/// Which GitHub repo a Kiro Web task is given.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum Repo {
    /// The one the folder's own remote points at (an empty workspace when it has none).
    #[default]
    Folder,
    /// No repo: the agent starts in an empty folder.
    Empty,
    /// One of the repos connected to Kiro ("owner/name").
    Named(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Preview {
    pub id: u64,
    /// The transcript (after cleanup when it worked).
    pub heard: String,
    pub cleanup_note: Option<String>,
    pub task: String,
    pub folder: String,
    pub target_name: String,
    /// Why the default workspace (Routed.note()), whether it is still to be made, or
    /// what changed since.
    pub note: Option<String>,
    pub tool: AgentTool,
    /// The tool's model as set in its settings; empty is the tool's own default.
    pub model: String,
    /// An access id (projects::ACCESS_IDS).
    pub access: String,
    pub countdown: Option<f32>,
    /// Try it: Start disabled, never dispatches.
    pub trial: bool,
    /// Run in Kiro Web (Kiro only), switched on from the preview or by saying so.
    pub cloud: bool,
    /// The repo Kiro Web is given, when it runs there.
    pub repo: Repo,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pending { pub id: u64, pub text: String, pub tools: Vec<AgentTool> }

/// What voice needs from the rest of the app (app.rs owns the sessions).
pub struct Hooks {
    /// The tool's runner for a routing turn (access "none"), when it has one.
    pub router: Box<dyn Fn(AgentTool) -> Option<RunTask> + Send + Sync>,
    /// A new chat: tool, folder, prompt, access, the repo when it runs in Kiro Web, and the
    /// screenshots to send with it (files). Ok(session id) once the provider took it.
    pub start: Box<dyn Fn(AgentTool, &str, &str, &str, Option<Repo>, Vec<String>) -> Result<i32, String> + Send + Sync>,
    pub available: Box<dyn Fn(AgentTool) -> bool + Send + Sync>,
    /// The project open in Hover now (its id), which wins a tie.
    pub active_project: Box<dyn Fn() -> Option<String> + Send + Sync>,
}

/// Local speech as voice uses it: Phonon, or a test's fake.
pub trait Local: Send + Sync {
    /// Some only when it is Ready.
    fn speech(self: Arc<Self>) -> Option<Arc<dyn Speech>>;
    /// Stops its helper now (after each transcription, a cancel, a failure).
    fn shutdown(&self);
}

// The one place voice reaches Phonon.
impl Local for crate::phonon::Phonon {
    fn speech(self: Arc<Self>) -> Option<Arc<dyn Speech>> { crate::phonon::Phonon::speech(&self) }
    fn shutdown(&self) { crate::phonon::Phonon::shutdown(self) }
}

/// Makes the cloud engine from a key and a model (Groq; a fake in tests).
type Cloud = Box<dyn Fn(&str, &str) -> Arc<dyn Speech> + Send + Sync>;

/// Where a retry goes back to.
#[derive(Clone)]
enum Resume { Record, Route, Preview(Preview) }

/// One interaction: what was read at the press, and how far it got.
struct Run {
    trial: bool,
    /// Dictation into the open chat's reply box: no routing, no preview, no task.
    dictate: bool,
    voice: VoiceSettings,
    /// The voice-enabled projects and the default workspace, at the press.
    projects: Vec<Project>,
    workspace: Workspace,
    /// The default agent, or the one picked for this task only.
    tool: AgentTool,
    released: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    ct: Cancel,
    /// What is routed: the transcript after cleanup (or the edited task, after a pick).
    text: String,
    heard: String,
    cleanup_note: Option<String>,
    /// The engine said it cut the audio: shown for review, never counted down.
    review: bool,
    /// The target last shown: Some(None) is the default workspace.
    target: Option<Option<String>>,
    /// Kiro Web was asked for in words ("use Kiro Web").
    cloud: bool,
    resume: Resume,
    /// Screenshots taken by saying so, as files in kiro-images, sent with the task.
    shots: Arc<Mutex<Vec<String>>>,
    /// Stretches of the recording being read for "take a screenshot" now.
    reading: Arc<std::sync::atomic::AtomicUsize>,
}

struct St {
    stage: Stage,
    id: u64,
    deadline: Option<Instant>,
    /// The countdown the preview started with (Settings → Voice), for its ring.
    total: Duration,
    /// An edit not routed yet (or that failed to): Start waits.
    checking: bool,
    edit: u64,
    busy: Option<Instant>,
    run: Option<Run>,
    /// Counts the screenshots taken by voice (one each, or one that failed), with what the card
    /// says of the last: SHOT_TAKEN, or why there is none. The UI chimes, flashes and says it
    /// when the count moves.
    shot: (u64, String),
}

pub struct Voice {
    me: Weak<Voice>,
    settings: Arc<Settings>,
    secrets: Arc<Secrets>,
    local: Arc<dyn Local>,
    hooks: Hooks,
    cloud: Cloud,
    open: audio::Open,
    /// A test's own countdown; None is Settings' (VoiceSettings::countdown).
    countdown: Option<Duration>,
    max: usize,
    st: Mutex<St>,
    listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
}

/// An HTTP client for Groq and cleanup, as net.rs makes its own: the system's TLS on
/// Windows, a time limit on the whole call, and statuses read rather than thrown.
pub(crate) fn agent(limit: Duration) -> ureq::Agent {
    let cfg = ureq::Agent::config_builder();
    #[cfg(windows)]
    let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
    cfg.timeout_global(Some(limit)).timeout_connect(Some(limit.min(Duration::from_secs(10)))).http_status_as_error(false).build().into()
}

/// Speech somewhere in it: a 30 ms window above −50 dBFS, and at least a quarter second.
// ponytail: a fixed threshold, not a voice detector; a very quiet microphone reads as
// silence. A per-device noise floor is the upgrade.
fn audible(s: &[i16]) -> bool {
    s.len() >= audio::RATE as usize / 4
        && s.chunks(480).any(|w| (w.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / w.len() as f64).sqrt() > 0.003 * 32768.0)
}

/// "Screenshot attached", or why there is none: what the card says after a capture.
pub const SHOT_TAKEN: &str = "Screenshot attached";
/// The largest a screenshot is kept (it is sent to the agent as it is).
const SHOT_MAX: (u32, u32) = (1920, 1200);
/// The task when nothing was said but "take a screenshot".
const SHOT_ONLY: &str = "Take a look at the attached screenshot.";

/// "take a screenshot" and "take a screenshot of this", in any case and spacing ("screen shot"
/// too, as speech engines write it), as whole words: the text without them (and the comma or
/// full stop that ended them), and how many there were. What is left is tidied after by cleanup.
pub fn take_screenshots(text: &str) -> (String, usize) {
    // The words, with where each starts and ends.
    let mut words: Vec<(usize, usize)> = vec![];
    let mut start = None;
    for (i, c) in text.char_indices().chain(std::iter::once((text.len(), ' '))) {
        let w = c.is_alphanumeric() || c == '\'';
        match (w, start) { (true, None) => start = Some(i), (false, Some(s)) => { words.push((s, i)); start = None; } _ => {} }
    }
    let lower = |k: usize| words.get(k).map(|&(a, b)| text[a..b].to_lowercase());
    let is = |k: usize, w: &str| lower(k).as_deref() == Some(w);
    let mut cut: Vec<(usize, usize)> = vec![];
    let mut k = 0;
    while k < words.len() {
        // take a screenshot | take a screen shot, then "of this" if it follows.
        let shot = if is(k, "take") && is(k + 1, "a") && is(k + 2, "screenshot") { Some(k + 3) }
            else if is(k, "take") && is(k + 1, "a") && is(k + 2, "screen") && is(k + 3, "shot") { Some(k + 4) } else { None };
        let Some(mut next) = shot else { k += 1; continue };
        if is(next, "of") && is(next + 1, "this") { next += 2; }
        let mut end = words[next - 1].1;
        // The punctuation that closed the phrase goes with it.
        if let Some(c) = text[end..].chars().next().filter(|c| matches!(c, '.' | ',' | '!' | '?' | ';' | ':')) { end += c.len_utf8(); }
        cut.push((words[k].0, end));
        k = next;
    }
    if cut.is_empty() { return (text.to_owned(), 0); }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for (a, b) in &cut { out.push_str(&text[at..*a]); out.push(' '); at = *b; }
    out.push_str(&text[at..]);
    // One space between words, none before punctuation, none left dangling at either end.
    let mut tidy = out.split_whitespace().collect::<Vec<_>>().join(" ");
    for p in [" .", " ,", " !", " ?", " ;", " :"] { tidy = tidy.replace(p, &p[1..]); }
    let tidy = tidy.trim_start_matches([',', '.', ';', ':', ' ']).trim().to_owned();
    (tidy, cut.len())
}

/// Where a recording pauses, for voice to listen for "take a screenshot" while it goes on:
/// fed the audio as it comes, it says when a stretch of speech has ended (a pause after it,
/// or twelve seconds without one), so that stretch can be read on its own.
#[derive(Default)]
pub struct Pauses { cut: usize, spoke: f32, quiet: f32 }

impl Pauses {
    /// The audio from `at - chunk.len()` to `at`; Some((from, to)) when a stretch ended there.
    /// `ready` false (stretches already being read) lets the stretch grow instead.
    pub fn feed(&mut self, chunk: &[i16], at: usize, ready: bool) -> Option<(usize, usize)> {
        if chunk.is_empty() { return None; }
        let secs = chunk.len() as f32 / audio::RATE as f32;
        let loud = (chunk.iter().map(|&x| (x as f64).powi(2)).sum::<f64>() / chunk.len() as f64).sqrt() > 0.003 * 32768.0;
        if loud { self.spoke += secs; self.quiet = 0.0; } else { self.quiet += secs; }
        let long = at.saturating_sub(self.cut) >= audio::RATE as usize * 12;
        if !ready || self.spoke < 0.6 || !(self.quiet >= 0.6 || long) { return None; }
        let r = (self.cut, at);
        *self = Pauses { cut: at, spoke: 0.0, quiet: 0.0 };
        Some(r)
    }
}

impl Voice {
    pub fn new<L: Local + 'static>(settings: Arc<Settings>, secrets: Arc<Secrets>, phonon: Arc<L>, hooks: Hooks) -> Arc<Voice> {
        wav::sweep();
        Voice::build(settings, secrets, phonon, hooks, Box::new(|k: &str, m: &str| Arc::new(groq::Groq::new(k, m)) as Arc<dyn Speech>), Box::new(audio::open), None, audio::MAX_SAMPLES)
    }

    #[allow(clippy::too_many_arguments)]
    fn build(settings: Arc<Settings>, secrets: Arc<Secrets>, local: Arc<dyn Local>, hooks: Hooks, cloud: Cloud, open: audio::Open, countdown: Option<Duration>, max: usize) -> Arc<Voice> {
        Arc::new_cyclic(|me| Voice {
            me: me.clone(), settings, secrets, local, hooks, cloud, open, countdown, max,
            st: Mutex::new(St { stage: Stage::Idle, id: 0, deadline: None, total: Duration::ZERO, checking: false, edit: 0, busy: None, run: None, shot: (0, String::new()) }),
            listeners: Mutex::new(vec![]),
        })
    }

    /// The stage now, with the countdown's seconds left.
    pub fn stage(&self) -> Stage {
        let st = self.st.lock().unwrap();
        match (&st.stage, st.deadline) {
            (Stage::Preview(p), Some(d)) => Stage::Preview(Preview { countdown: Some(d.saturating_duration_since(Instant::now()).as_secs_f32()), ..p.clone() }),
            (s, _) => s.clone(),
        }
    }

    /// How long the preview counts down in all (its ring is what is left of it).
    pub fn countdown_total(&self) -> Duration { self.st.lock().unwrap().total }

    /// When a press was last turned away because one was in progress (the notch flashes).
    pub fn busy_since(&self) -> Option<Instant> { self.st.lock().unwrap().busy }

    /// Start (or Enter) would start now: a preview that isn't a trial, and after an edit
    /// only once it has been routed again.
    pub fn can_start(&self) -> bool {
        let st = self.st.lock().unwrap();
        match &st.stage { Stage::Preview(p) => !p.trial, Stage::Editing(p) => !p.trial && !st.checking, _ => false }
    }

    /// The agent this interaction uses now (the default, or one picked for it).
    pub fn tool(&self) -> Option<AgentTool> { self.st.lock().unwrap().run.as_ref().map(|r| r.tool) }

    /// The screenshots this interaction has taken so far (files), oldest first.
    pub fn shots(&self) -> Vec<String> { self.st.lock().unwrap().run.as_ref().map(|r| r.shots.lock().unwrap().clone()).unwrap_or_default() }

    /// How many screenshots voice has tried to take, and what the card says of the last.
    pub fn shot(&self) -> (u64, String) { self.st.lock().unwrap().shot.clone() }

    /// The card's × on a screenshot: it isn't sent. A preview counting down stops for good,
    /// as after an edit, so what goes is what was looked at.
    pub fn unattach(&self, i: usize) {
        {
            let st = self.st.lock().unwrap();
            let Some(r) = &st.run else { return };
            let mut l = r.shots.lock().unwrap();
            if i >= l.len() { return; }
            l.remove(i);
        }
        self.hold();
        self.notify();
    }

    /// A picture of the screen now, kept with this interaction. Off the UI thread.
    fn screenshot(&self, id: u64) {
        let Some(shots) = self.with_run(id, |r| r.shots.clone()) else { return };
        // Into kiro-images, where the office keeps pasted pictures (and the agent reads them).
        let got = crate::screen::whole(SHOT_MAX).and_then(|img| {
            let url = hover_core::json::Json::str(crate::screen::data_url(&img, 85));
            hover_core::images::save(&[url], &hover_core::images::folder(hover_core::paths::support())).into_iter().next()
                .map(|p| p.to_string_lossy().into_owned()).ok_or_else(|| "it couldn’t be kept.".to_owned())
        });
        let said = match got {
            Ok(file) => { shots.lock().unwrap().push(file); SHOT_TAKEN.to_owned() }
            Err(e) => { hover_core::log::line(&format!("voice: screenshot failed - {e}")); format!("Couldn’t take a screenshot: {e}") }
        };
        {
            let mut st = self.st.lock().unwrap();
            if st.id != id { return; }
            st.shot = (st.shot.0 + 1, said);
        }
        self.notify();
    }

    /// Called from any thread when the stage changes (and while recording or counting
    /// down, ten to twenty times a second).
    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) { self.listeners.lock().unwrap().push(Arc::new(f)); }

    fn notify(&self) {
        let l: Vec<_> = self.listeners.lock().unwrap().clone();
        for f in l { f(); }
    }

    fn spawn(&self, name: &str, f: impl FnOnce(Arc<Voice>) + Send + 'static) {
        let Some(me) = self.me.upgrade() else { return };
        let _ = std::thread::Builder::new().name(name.into()).spawn(move || f(me));
    }

    /// Sets the stage when `id` is still the interaction; false when it has moved on.
    fn set(&self, id: u64, stage: Stage) -> bool {
        {
            let mut st = self.st.lock().unwrap();
            if st.id != id || st.run.is_none() { return false; }
            st.stage = stage;
        }
        self.notify();
        true
    }

    fn with_run<R>(&self, id: u64, f: impl FnOnce(&mut Run) -> R) -> Option<R> {
        let mut st = self.st.lock().unwrap();
        if st.id != id { return None; }
        st.run.as_mut().map(f)
    }

    fn fail(&self, id: u64, message: String, retry: bool, transcript: Option<String>, resume: Resume) {
        self.with_run(id, |r| r.resume = resume);
        self.set(id, Stage::Error { message, retry, transcript });
    }

    /// The shortcut went down (or Try it). While one is in progress it is kept and the
    /// press only flashes busy.
    pub fn press(&self, trial: bool) { self.begin(trial, false) }

    /// The shortcut went down over an open chat's reply box: what is said is written
    /// there (Stage::Dictated), never routed or started.
    pub fn dictate(&self) { self.begin(false, true) }

    fn begin(&self, trial: bool, dictate: bool) {
        {
            let mut st = self.st.lock().unwrap();
            if !matches!(st.stage, Stage::Idle | Stage::Started { .. } | Stage::Dictated(_) | Stage::Cancelled | Stage::Error { .. }) {
                st.busy = Some(Instant::now());
                drop(st);
                self.notify();
                return;
            }
            st.id += 1;
            let ws = self.settings.default_workspace();
            let voice = self.settings.voice();
            let tool = voice.agent.unwrap_or_else(|| self.settings.agent_tool());
            st.run = Some(Run {
                trial, dictate, voice, projects: self.settings.projects().into_iter().filter(|p| p.voice).collect(), workspace: ws,
                tool, released: Default::default(), cancel: Default::default(), ct: Cancel::new(),
                text: String::new(), heard: String::new(), cleanup_note: None, review: false, target: None, cloud: false, resume: Resume::Record,
                shots: Default::default(), reading: Default::default(),
            });
            st.stage = Stage::Recording { level: 0.0, secs: 0.0 };
            st.deadline = None;
            st.checking = false;
            let id = st.id;
            drop(st);
            self.notify();
            self.spawn("voice", move |v| v.record(id));
        }
    }

    /// The shortcut came up. After the ten-minute stop it does nothing.
    pub fn release(&self) {
        let st = self.st.lock().unwrap();
        if let (Stage::Recording { .. }, Some(r)) = (&st.stage, &st.run) { r.released.store(true, Ordering::Relaxed); }
    }

    /// Escape or Cancel: drops the interaction (late results are ignored). Once the task
    /// is starting it is the chat's, and Escape leaves it alone; a card is closed.
    pub fn cancel(&self) {
        let mut st = self.st.lock().unwrap();
        match st.stage {
            Stage::Idle | Stage::Starting(_) => return,
            Stage::Started { .. } | Stage::Dictated(_) | Stage::Cancelled | Stage::Error { .. } => { drop(st); return self.dismiss(); }
            _ => {}
        }
        st.id += 1;
        if let Some(r) = st.run.take() { r.cancel.store(true, Ordering::Relaxed); r.ct.cancel(); }
        st.deadline = None;
        st.stage = Stage::Cancelled;
        drop(st);
        self.notify();
    }

    /// Closes a Started, Dictated, Cancelled or Error card.
    pub fn dismiss(&self) {
        let mut st = self.st.lock().unwrap();
        if !matches!(st.stage, Stage::Started { .. } | Stage::Dictated(_) | Stage::Cancelled | Stage::Error { .. }) { return; }
        st.stage = Stage::Idle;
        st.run = None;
        drop(st);
        self.notify();
    }

    /// Start or Enter.
    pub fn start_now(&self) {
        let id = self.st.lock().unwrap().id;
        self.begin_start(id, false);
    }

    /// The one way into Starting: from a counting-down preview (the countdown's end,
    /// Start, Enter), or from an edited one that has been routed again. Locked, so
    /// whichever comes first wins and the rest find it already starting.
    fn begin_start(&self, id: u64, expiry: bool) -> bool {
        let p = {
            let mut st = self.st.lock().unwrap();
            if st.id != id { return false; }
            let p = match &st.stage {
                Stage::Preview(p) if !p.trial && (!expiry || st.deadline.is_some_and(|d| Instant::now() >= d)) => p.clone(),
                Stage::Editing(p) if !expiry && !p.trial && !st.checking => p.clone(),
                _ => return false,
            };
            let p = Preview { countdown: None, ..p };
            st.deadline = None;
            st.stage = Stage::Starting(p.clone());
            p
        };
        self.notify();
        self.spawn("voice-start", move |v| v.start(id, p));
        true
    }

    /// An edit of the task: the countdown stops for good, and once typing pauses the
    /// edited text is routed again and the card updated; Start is needed after.
    pub fn edit(&self, task: &str) {
        let (id, gen) = {
            let mut st = self.st.lock().unwrap();
            let p = match &st.stage { Stage::Preview(p) | Stage::Editing(p) => p.clone(), _ => return };
            st.deadline = None;
            st.checking = true;
            st.edit += 1;
            st.stage = Stage::Editing(Preview { task: task.to_owned(), countdown: None, ..p });
            (st.id, st.edit)
        };
        self.notify();
        let task = task.to_owned();
        self.spawn("voice-edit", move |v| {
            std::thread::sleep(EDIT_SETTLE);
            if v.st.lock().unwrap().edit != gen { return; }
            v.reroute(id, gen, &task);
        });
    }

    /// The agent picked for this task when the default one isn't available.
    pub fn choose_agent(&self, tool: AgentTool) {
        let id = {
            let mut st = self.st.lock().unwrap();
            let Stage::ChooseAgent(p) = &st.stage else { return };
            if p.id != st.id { return; }
            let text = p.text.clone();
            let Some(r) = st.run.as_mut() else { return };
            r.tool = tool;
            r.text = text;
            st.stage = Stage::Resolving;
            st.id
        };
        self.notify();
        self.spawn("voice-route", move |v| v.resolve(id));
    }

    /// Another agent (or its model, just picked) for this task only, from the preview:
    /// the countdown stops for good and Start is needed, as after an edit. The target
    /// stays as shown, so nothing is routed again; the default agent is left alone.
    pub fn change_agent(&self, tool: AgentTool) {
        let model = self.settings.agent_options(tool).model.unwrap_or_default();
        {
            let mut st = self.st.lock().unwrap();
            let p = match &st.stage { Stage::Preview(p) | Stage::Editing(p) if p.id == st.id => p.clone(), _ => return };
            let Some(r) = st.run.as_mut() else { return };
            r.tool = tool;
            st.deadline = None;
            st.stage = Stage::Editing(Preview { tool, model, countdown: None, ..p });
        }
        self.notify();
    }

    /// Kiro Web on or off for this task, from the preview: as another agent, the
    /// countdown stops for good and Start is needed.
    pub fn toggle_cloud(&self) {
        {
            let mut st = self.st.lock().unwrap();
            let p = match &st.stage { Stage::Preview(p) | Stage::Editing(p) if p.id == st.id && p.tool == AgentTool::Kiro => p.clone(), _ => return };
            st.deadline = None;
            st.stage = Stage::Editing(Preview { cloud: !p.cloud, countdown: None, ..p });
        }
        self.notify();
    }

    /// Another folder for this task, from the preview: a voice project, or the default
    /// workspace (None). The countdown stops for good and Start is needed, as after an
    /// edit; what was picked on the card (agent, Kiro Web, repo) stays.
    pub fn change_folder(&self, target: Option<String>) {
        let (id, old) = {
            let st = self.st.lock().unwrap();
            match &st.stage { Stage::Preview(p) | Stage::Editing(p) if p.id == st.id && !p.trial => (st.id, p.clone()), _ => return }
        };
        let Some(r) = self.copy(id) else { return };
        // Only the note for the default workspace is read from it: "Using default workspace."
        let routed = Routed { project: target.clone(), why: Why::Active, task: old.task.clone() };
        let built = self.preview(id, &r, &target, &routed, old.task.clone());
        {
            let mut st = self.st.lock().unwrap();
            if st.id != id { return; }
            st.deadline = None;
            match built {
                Ok(p) => {
                    st.stage = Stage::Editing(Preview { tool: old.tool, model: old.model.clone(), cloud: old.cloud, repo: old.repo.clone(), countdown: None, ..p });
                    if let Some(run) = st.run.as_mut() { run.target = Some(target); }
                }
                // Shown on the card; the folder stays as it was.
                Err(e) => st.stage = Stage::Editing(Preview { note: Some(e), countdown: None, ..old }),
            }
        }
        self.notify();
    }

    /// Another repo for this Kiro Web task, from the preview: as Kiro Web itself, the
    /// countdown stops for good and Start is needed.
    pub fn change_repo(&self, repo: Repo) {
        {
            let mut st = self.st.lock().unwrap();
            let p = match &st.stage { Stage::Preview(p) | Stage::Editing(p) if p.id == st.id && p.cloud && p.tool == AgentTool::Kiro => p.clone(), _ => return };
            st.deadline = None;
            st.stage = Stage::Editing(Preview { repo, countdown: None, ..p });
        }
        self.notify();
    }

    /// The voice projects a task can be moved to from the card (the default workspace is
    /// the one not in the list), as they were at the press.
    pub fn folder_choices(&self) -> Vec<Project> {
        self.st.lock().unwrap().run.as_ref().map(|r| r.projects.clone()).unwrap_or_default()
    }

    /// Stops the countdown for good (a menu on the card opened): Start is needed after.
    pub fn hold(&self) {
        {
            let mut st = self.st.lock().unwrap();
            let p = match &st.stage { Stage::Preview(p) if p.id == st.id && st.run.is_some() => p.clone(), _ => return };
            st.deadline = None;
            st.stage = Stage::Editing(Preview { countdown: None, ..p });
        }
        self.notify();
    }

    /// The agents a task could go to now: installed and signed in. Blocks (each tool's
    /// status command, kept five minutes); call it off the UI thread.
    pub fn ready_tools(&self) -> Vec<AgentTool> { AgentTool::ALL.into_iter().filter(|t| (self.hooks.available)(*t)).collect() }

    /// Retry on an error card: routing again, or back to the preview (Start needed, never
    /// resent on its own); after a recording or transcription error, to Idle, to speak again.
    pub fn retry(&self) {
        let (id, resume) = {
            let st = self.st.lock().unwrap();
            let (Stage::Error { retry: true, .. }, Some(r)) = (&st.stage, &st.run) else { return };
            (st.id, r.resume.clone())
        };
        match resume {
            Resume::Record => { self.st.lock().unwrap().run = None; self.set_idle(); }
            Resume::Route => { if self.set(id, Stage::Resolving) { self.spawn("voice-route", move |v| v.resolve(id)); } }
            Resume::Preview(p) => {
                let mut st = self.st.lock().unwrap();
                if st.id != id { return; }
                st.checking = false;
                st.stage = Stage::Editing(Preview { countdown: None, ..p });
                drop(st);
                self.notify();
            }
        }
    }

    fn set_idle(&self) { self.st.lock().unwrap().stage = Stage::Idle; self.notify(); }

    /// Input devices for Settings (the system's default isn't listed).
    pub fn microphones() -> Vec<String> { audio::microphones() }

    /// Settings' key check: an authenticated call to Groq. Blocking.
    pub fn check_groq(key: &str) -> Result<(), String> { groq::check(groq::BASE, key) }

    // MARK: The worker

    fn record(&self, id: u64) {
        let Some((voice, released, cancel, dictate)) = self.with_run(id, |r| (r.voice.clone(), r.released.clone(), r.cancel.clone(), r.dictate)) else { return };
        // The engine first: a mode that isn't set up says so before anything is recorded.
        let (engine, local): (Arc<dyn Speech>, bool) = match voice.speech {
            SpeechMode::Cloud => match self.secrets.get(projects::GROQ_SECRET) {
                Some(k) => ((self.cloud)(&k, &voice.model), false),
                None => return self.fail(id, SpeechError::NotReady("Add your Groq key in Settings → Voice.".into()).message(), false, None, Resume::Record),
            },
            // Never the cloud instead: a local recording isn't uploaded behind the user's back.
            SpeechMode::Local => match self.local.clone().speech() {
                Some(s) => (s, true),
                None => return self.fail(id, "Set up local speech in Settings → Voice.".into(), false, None, Resume::Record),
            },
        };
        let src = match (self.open)(voice.microphone.as_deref(), self.max) {
            Ok(s) => s,
            Err(e) => return self.fail(id, e, true, None, Resume::Record),
        };
        let t0 = Instant::now();
        // "Take a screenshot" is heard while it records: each stretch of speech, once it pauses,
        // is read by the same engine on its own thread, and the phrase takes a picture then.
        // Cloud only: Local starts Phonon's Python and model for each reading (seconds), so it
        // can't keep up; there the picture is taken at the end, from the whole transcript.
        let reading = self.with_run(id, |r| r.reading.clone()).unwrap_or_default();
        let (mut pauses, mut seen) = (Pauses::default(), 0usize);
        loop {
            std::thread::sleep(Duration::from_millis(50));
            if cancel.load(Ordering::Relaxed) { drop(src.finish()); return; }
            if let Some(e) = src.failed() { drop(src.finish()); return self.fail(id, e, true, None, Resume::Record); }
            let n = src.samples();
            // Ten minutes by the samples, or by the clock should the device stall.
            if released.load(Ordering::Relaxed) || n >= self.max || t0.elapsed() > Duration::from_secs(601) { break; }
            if !local {
                let chunk = src.since(seen);
                seen += chunk.len();
                // ponytail: at most two stretches read at once; past that the next one grows until
                // one is done. Each is a request to Groq, which counts toward its limits.
                if let Some((from, to)) = pauses.feed(&chunk, seen, reading.load(Ordering::Relaxed) < 2) {
                    let stretch: Vec<i16> = src.since(from).into_iter().take(to - from).collect();
                    let (engine, cancel, reading) = (engine.clone(), cancel.clone(), reading.clone());
                    reading.fetch_add(1, Ordering::Relaxed);
                    self.spawn("voice-listen", move |v| {
                        let heard = wav::write(&stretch, audio::RATE).ok().map(|f| engine.transcribe(f.path(), &cancel));
                        match heard {
                            Some(Ok(t)) => { for _ in 0..take_screenshots(&t.text).1 { v.screenshot(id); } }
                            Some(Err(SpeechError::Cancelled)) | None => {}
                            Some(Err(e)) => hover_core::log::line(&format!("voice: a stretch couldn’t be read for “take a screenshot” - {}", e.message())),
                        }
                        reading.fetch_sub(1, Ordering::Relaxed);
                    });
                }
            }
            self.set(id, Stage::Recording { level: src.level(), secs: n as f32 / audio::RATE as f32 });
        }
        let samples = src.finish();
        if !audible(&samples) { return self.fail(id, NOTHING.into(), true, None, Resume::Record); }
        let file = wav::write(&samples, audio::RATE);
        drop(samples);
        let file = match file { Ok(f) => f, Err(e) => return self.fail(id, format!("The recording couldn’t be saved for transcription: {e}"), true, None, Resume::Record) };
        if !self.set(id, if local { Stage::Loading } else { Stage::Transcribing }) { return; }
        let got = engine.transcribe(file.path(), &cancel);
        drop(file);
        drop(engine);
        if local { self.local.shutdown(); }
        let t = match got {
            Ok(t) => t,
            Err(SpeechError::Cancelled) => return,
            Err(e) => { let retry = !matches!(e, SpeechError::NotReady(_) | SpeechError::BadKey); return self.fail(id, e.message(), retry, None, Resume::Record); }
        };
        let heard = t.text.trim().to_owned();
        if heard.is_empty() { return self.fail(id, NOTHING.into(), true, None, Resume::Record); }
        // A stretch still being read may take a picture yet: it waits for them (a few seconds at most).
        let wait = Instant::now();
        while reading.load(Ordering::Relaxed) > 0 && wait.elapsed() < Duration::from_secs(8) && !cancel.load(Ordering::Relaxed) { std::thread::sleep(Duration::from_millis(50)); }
        // The phrase never reaches the task. Said but not caught on the way (Local, or a stretch
        // that couldn't be read), the picture is taken now.
        let (heard, asked) = take_screenshots(&heard);
        if asked > 0 && self.shots().is_empty() { self.screenshot(id); }
        let heard = if heard.is_empty() && asked > 0 { SHOT_ONLY.to_owned() } else { heard };
        let (text, note) = if voice.cleanup {
            if !self.set(id, Stage::Cleaning) { return; }
            match self.cleanup_service(&voice) {
                Some(s) => cleanup::tidy(&s, &heard, &cancel),
                None => (heard.clone(), Some(cleanup::FAILED)),
            }
        } else { (heard.clone(), None) };
        if cancel.load(Ordering::Relaxed) { return; }
        if dictate { self.set(id, Stage::Dictated(text)); return; }
        if self.with_run(id, |r| { r.text = text.clone(); r.heard = text; r.cleanup_note = note.map(str::to_owned); r.review = t.truncated; }).is_none() { return; }
        if self.set(id, Stage::Resolving) { self.resolve(id); }
    }

    /// The cleanup service as set up; None when something it needs is missing.
    fn cleanup_service(&self, v: &VoiceSettings) -> Option<cleanup::Service> {
        let base = v.cleanup_provider.base().map(str::to_owned).or_else(|| v.cleanup_base.clone())?;
        let key = self.secrets.get(v.cleanup_provider.secret())?;
        Some(cleanup::Service { base, key, model: v.cleanup_model.clone()?, timeout: cleanup::TIMEOUT })
    }

    fn targets(r: &Run) -> Vec<Target> {
        r.projects.iter().map(|p| Target { id: p.id.clone(), name: p.name.clone(), aliases: p.aliases.clone() }).collect()
    }

    /// Routes `text` with the run's agent: by the words, then the agent when they leave
    /// it open. A trial without an agent goes by the words alone.
    fn route_text(&self, r: &RunCopy, text: &str) -> Result<Routed, String> {
        let active = (self.hooks.active_project)();
        if !(self.hooks.available)(r.tool) {
            return Ok(match route::decide(text, &r.targets, active.as_deref()) {
                route::Decision::Done(x) => x,
                route::Decision::AskAgent { .. } => Routed { project: None, why: Why::Ambiguous, task: text.trim().to_owned() },
            });
        }
        route::route((self.hooks.router)(r.tool).as_ref(), text, &r.targets, active.as_deref(), &r.ct, ROUTE_LIMIT)
            .map_err(|e| format!("The agent couldn’t route the request: {e}"))
    }

    fn copy(&self, id: u64) -> Option<RunCopy> {
        self.with_run(id, |r| RunCopy {
            trial: r.trial, tool: r.tool, targets: Voice::targets(r), projects: r.projects.clone(), workspace: r.workspace.clone(),
            ct: r.ct.clone(), cancel: r.cancel.clone(), text: r.text.clone(), heard: r.heard.clone(), cleanup_note: r.cleanup_note.clone(),
            review: r.review, target: r.target.clone(), cloud: r.cloud,
        })
    }

    fn resolve(&self, id: u64) {
        // "Use Kiro Web" / "use cloud agent": the task goes to Kiro, in Kiro Web, less those
        // words. The card shows it (the agent and the blue cloud) and can undo it.
        self.with_run(id, |x| if let Some(t) = route::take_cloud(&x.text) { x.text = t; x.cloud = true; x.tool = AgentTool::Kiro; });
        let Some(r) = self.copy(id) else { return };
        self.with_run(id, |x| x.resume = Resume::Route);
        if !r.trial && !(self.hooks.available)(r.tool) {
            let tools = self.ready_tools();
            self.set(id, Stage::ChooseAgent(Pending { id, text: r.text.clone(), tools }));
            return;
        }
        let routed = self.route_text(&r, &r.text);
        if r.cancel.load(Ordering::Relaxed) { return; }
        let routed = match routed { Ok(x) => x, Err(e) => return self.fail(id, e, true, Some(r.text.clone()), Resume::Route) };
        let target = self.keep_target(&r, &routed);
        let p = match self.preview(id, &r, &target, &routed, routed.task.clone()) {
            Ok(p) => p,
            Err(e) => return self.fail(id, e, true, Some(r.text.clone()), Resume::Route),
        };
        let mut st = self.st.lock().unwrap();
        if st.id != id { return; }
        let Some(run) = st.run.as_mut() else { return };
        run.target = Some(target);
        let total = self.countdown.unwrap_or(Duration::from_secs(run.voice.countdown as u64));
        // A trial never counts down; a cut-short transcript waits for a look and a Start.
        let count = !r.trial && !r.review;
        st.checking = false;
        // Off (0 s): the preview waits for Start, as after an edit.
        let count = count && !total.is_zero();
        if count {
            st.total = total;
            st.deadline = Some(Instant::now() + total);
            st.stage = Stage::Preview(Preview { countdown: Some(total.as_secs_f32()), ..p });
        } else {
            st.deadline = None;
            st.stage = if r.trial { Stage::Preview(p) } else { Stage::Editing(p) };
        }
        drop(st);
        self.notify();
        if count { self.tick(id); }
    }

    /// An edit that names no project keeps the target already shown: taking the project's
    /// words out of the task shouldn't move it to the default workspace.
    fn keep_target(&self, r: &RunCopy, routed: &Routed) -> Option<String> {
        match (&r.target, &routed.why) {
            (Some(t), Why::NoneNamed) => t.clone(),
            _ => routed.project.clone(),
        }
    }

    /// The card for a target. Err for a project folder that can't be used (the prompt
    /// is kept; nothing else is put in its place) or no home for the default workspace.
    fn preview(&self, id: u64, r: &RunCopy, target: &Option<String>, routed: &Routed, task: String) -> Result<Preview, String> {
        let model = self.settings.agent_options(r.tool).model.unwrap_or_default();
        let (folder, target_name, access, note) = match target {
            Some(pid) => {
                let p = r.projects.iter().find(|p| &p.id == pid).ok_or("That project isn’t a voice project any more.")?;
                let f = projects::resolve_folder(&p.folder)?;
                (f.to_string_lossy().into_owned(), p.name.clone(), p.access.clone(), None)
            }
            None => {
                let path = r.workspace.path().ok_or("Hover can’t find your home folder for the default workspace.")?;
                let note = routed.note().to_owned();
                let mut note = if note.is_empty() { "Using default workspace.".to_owned() } else { note };
                if !path.is_dir() {
                    note.push_str(if r.trial { " It would be made when a task starts." } else { " It will be made when the task starts." });
                }
                (path.to_string_lossy().into_owned(), "Default workspace".to_owned(), r.workspace.access.clone(), Some(note.trim().to_owned()))
            }
        };
        Ok(Preview { id, heard: r.heard.clone(), cleanup_note: r.cleanup_note.clone(), task, folder, target_name, note: note.filter(|n| !n.is_empty()),
            tool: r.tool, model, access, countdown: None, trial: r.trial, cloud: r.cloud && r.tool == AgentTool::Kiro, repo: Repo::Folder })
    }

    /// The countdown, ten updates a second; its end starts the task (unless something
    /// else got there first).
    fn tick(&self, id: u64) {
        self.spawn("voice-countdown", move |v| loop {
            // Read before the sleep: a guard in its argument would be held through it.
            let total = v.st.lock().unwrap().total;
            std::thread::sleep(Duration::from_millis(100).min(total));
            let due = {
                let st = v.st.lock().unwrap();
                match (&st.stage, st.deadline) {
                    (Stage::Preview(_), Some(d)) if st.id == id => Instant::now() >= d,
                    _ => return,
                }
            };
            if due { v.begin_start(id, true); return; }
            v.notify();
        });
    }

    fn reroute(&self, id: u64, gen: u64, task: &str) {
        let Some(r) = self.copy(id) else { return };
        let routed = self.route_text(&r, task);
        let target = routed.as_ref().ok().map(|x| self.keep_target(&r, x));
        let built = match (&routed, &target) {
            (Ok(x), Some(t)) => self.preview(id, &r, t, x, task.to_owned()),
            (Err(e), _) => Err(e.clone()),
            _ => Err("The task couldn’t be checked.".into()),
        };
        let mut st = self.st.lock().unwrap();
        if st.id != id || st.edit != gen { return; }
        let Stage::Editing(old) = &st.stage else { return };
        match built {
            Ok(p) => {
                // An agent, Kiro Web or a repo picked on the card while this was routed stay picked.
                st.stage = Stage::Editing(Preview { tool: old.tool, model: old.model.clone(), cloud: old.cloud, repo: old.repo.clone(), ..p });
                st.checking = false;
                if let (Some(run), Some(t)) = (st.run.as_mut(), target) { run.target = Some(t); }
            }
            // Shown on the card; Start stays off until an edit that checks out.
            Err(e) => { let p = Preview { note: Some(e), ..old.clone() }; st.stage = Stage::Editing(p); }
        }
        drop(st);
        self.notify();
    }

    /// Start: everything checked again as it is now (the project still takes voice, its
    /// folder is there, the access is what the card said), then a new chat. A change
    /// shows the updated card for another Start; a failure keeps the prompt.
    fn start(&self, id: u64, p: Preview) {
        let Some(r) = self.copy(id) else { return };
        let keep = |v: &Voice, m: String| v.fail(id, m, true, Some(p.task.clone()), Resume::Preview(p.clone()));
        if !(self.hooks.available)(p.tool) {
            let tools = self.ready_tools();
            self.set(id, Stage::ChooseAgent(Pending { id, text: p.task.clone(), tools }));
            return;
        }
        let target = r.target.clone().flatten();
        let (folder, access) = match &target {
            Some(pid) => match self.settings.project(pid) {
                None => return keep(self, format!("“{}” isn’t registered any more.", p.target_name)),
                Some(x) if !x.voice => return keep(self, format!("“{}” no longer takes voice tasks.", x.name)),
                Some(x) => match projects::resolve_folder(&x.folder) {
                    Err(e) => return keep(self, e),
                    Ok(f) => (f.to_string_lossy().into_owned(), x.access),
                },
            },
            None => {
                let w = self.settings.default_workspace();
                let Some(path) = w.path() else { return keep(self, "Hover can’t find your home folder for the default workspace.".into()) };
                if !projects::same_folder(&path.to_string_lossy(), &p.folder) || w.access != p.access {
                    (path.to_string_lossy().into_owned(), w.access)
                } else {
                    match projects::ensure_folder(&path) { Ok(f) => (f.to_string_lossy().into_owned(), w.access), Err(e) => return keep(self, e) }
                }
            }
        };
        if !projects::same_folder(&folder, &p.folder) || access != p.access {
            let mut st = self.st.lock().unwrap();
            if st.id != id { return; }
            st.checking = false;
            st.stage = Stage::Editing(Preview { folder, access, note: Some("The target’s settings changed. Check them, then Start.".into()), ..p });
            drop(st);
            return self.notify();
        }
        match (self.hooks.start)(p.tool, &folder, &p.task, &access, (p.cloud && p.tool == AgentTool::Kiro).then(|| p.repo.clone()), self.shots()) {
            Ok(session) => { self.set(id, Stage::Started { session, folder }); }
            Err(e) => keep(self, e),
        }
    }
}

/// What a worker needs of the run, copied out so the lock isn't held while it works.
struct RunCopy {
    trial: bool,
    tool: AgentTool,
    targets: Vec<Target>,
    projects: Vec<Project>,
    workspace: Workspace,
    ct: Cancel,
    cancel: Arc<AtomicBool>,
    text: String,
    heard: String,
    cleanup_note: Option<String>,
    review: bool,
    target: Option<Option<String>>,
    cloud: bool,
}

/// A local HTTP server for the tests: answers each connection with the next reply, and
/// keeps every request (head and body) as text.
#[cfg(test)]
pub(crate) mod fake {
    use std::io::{Read, Write};
    use std::sync::{Arc, Mutex};

    pub fn reply(status: u16, headers: &str, body: &str) -> String {
        format!("HTTP/1.1 {status} X\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len())
    }

    pub fn serve(replies: Vec<String>) -> (String, Arc<Mutex<Vec<String>>>) {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        let seen: Arc<Mutex<Vec<String>>> = Default::default();
        let s2 = seen.clone();
        std::thread::spawn(move || {
            for (c, r) in l.incoming().zip(replies) {
                let Ok(mut c) = c else { return };
                let mut buf = vec![];
                let mut b = [0u8; 8192];
                // The head, then as much body as it says.
                let body_at = loop {
                    let n = c.read(&mut b).unwrap_or(0);
                    if n == 0 { break None; }
                    buf.extend_from_slice(&b[..n]);
                    if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") { break Some(i + 4); }
                };
                if let Some(at) = body_at {
                    let head = String::from_utf8_lossy(&buf[..at]).to_lowercase();
                    let len: usize = head.lines().find_map(|l| l.strip_prefix("content-length:")).and_then(|v| v.trim().parse().ok()).unwrap_or(0);
                    while buf.len() < at + len { let n = c.read(&mut b).unwrap_or(0); if n == 0 { break; } buf.extend_from_slice(&b[..n]); }
                }
                s2.lock().unwrap().push(String::from_utf8_lossy(&buf).into_owned());
                let _ = c.write_all(r.as_bytes());
            }
        });
        (url, seen)
    }
}

#[cfg(test)]
mod tests;

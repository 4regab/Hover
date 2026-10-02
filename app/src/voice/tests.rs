//! The voice flow end to end with fakes: a microphone that fills at a set pace, speech
//! that answers a set text (or waits to be cancelled), and hooks that count starts.

use super::*;
use crate::speech::Transcript;
use std::cell::Cell;
use std::path::PathBuf;
use std::sync::atomic::AtomicUsize;

struct FakeSpeech { text: String, delay: Duration, calls: AtomicUsize, saw_cancel: AtomicBool }

impl Speech for FakeSpeech {
    fn transcribe(&self, wav: &std::path::Path, cancel: &AtomicBool) -> Result<Transcript, SpeechError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert!(wav.is_file(), "a real WAV to read");
        let t0 = Instant::now();
        while t0.elapsed() < self.delay {
            if cancel.load(Ordering::Relaxed) { self.saw_cancel.store(true, Ordering::SeqCst); return Err(SpeechError::Cancelled); }
            std::thread::sleep(Duration::from_millis(5));
        }
        Ok(Transcript { text: self.text.clone(), language: None, truncated: false, audio: None, took: t0.elapsed() })
    }
}

struct FakeLocal { speech: Option<Arc<FakeSpeech>>, shutdowns: AtomicUsize }

impl Local for FakeLocal {
    fn speech(self: Arc<Self>) -> Option<Arc<dyn Speech>> { self.speech.clone().map(|s| s as Arc<dyn Speech>) }
    fn shutdown(&self) { self.shutdowns.fetch_add(1, Ordering::SeqCst); }
}

/// Fills `step` samples of a tone each time it is looked at, up to its cap.
struct FakeSrc { n: Cell<usize>, step: usize, max: usize }

impl audio::Source for FakeSrc {
    fn level(&self) -> f32 { 0.5 }
    fn samples(&self) -> usize { self.n.set((self.n.get() + self.step).min(self.max)); self.n.get() }
    fn failed(&self) -> Option<String> { None }
    fn finish(self: Box<Self>) -> Vec<i16> { (0..self.n.get()).map(|i| ((i as f32 * 0.3).sin() * 3000.0) as i16).collect() }
}

/// countdown: ZERO is Settings' own (VoiceSettings::countdown).
struct Opt { mode: SpeechMode, local_ready: bool, countdown: Duration, max: usize, step: usize, delay: Duration, available: fn(AgentTool) -> bool }

impl Default for Opt {
    fn default() -> Self {
        Opt { mode: SpeechMode::Cloud, local_ready: true, countdown: Duration::from_secs(10), max: audio::MAX_SAMPLES, step: 8_000, delay: Duration::ZERO, available: |_| true }
    }
}

type Starts = Arc<Mutex<Vec<(AgentTool, String, String, String)>>>;

struct H { v: Arc<Voice>, settings: Arc<Settings>, starts: Starts, cloud: Arc<AtomicUsize>, opens: Arc<AtomicUsize>, speech: Arc<FakeSpeech>, local: Arc<FakeLocal>, dir: PathBuf }

impl Drop for H {
    fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
}

fn harness(text: &str, o: Opt) -> H {
    let dir = std::env::temp_dir().join(format!("hover-voice-{}", hover_core::guid_n()));
    std::fs::create_dir_all(dir.join("demo")).unwrap();
    let settings = Settings::load(dir.join("settings.json"));
    settings.add_project(&dir.join("demo").to_string_lossy()).unwrap();
    // Never the user's own ~/Hover.
    settings.set_default_workspace(Workspace { folder: Some(dir.join("ws").to_string_lossy().into_owned()), access: "risky".into() });
    settings.set_voice(VoiceSettings { enabled: true, speech: o.mode, ..VoiceSettings::default() });
    let secrets = Arc::new(Secrets::new(dir.join("secrets.dat"), None));
    secrets.set(projects::GROQ_SECRET, Some("gsk_test")).unwrap();
    let speech = Arc::new(FakeSpeech { text: text.into(), delay: o.delay, calls: AtomicUsize::new(0), saw_cancel: AtomicBool::new(false) });
    let local = Arc::new(FakeLocal { speech: o.local_ready.then(|| speech.clone()), shutdowns: AtomicUsize::new(0) });
    let starts: Starts = Default::default();
    let (cloud, opens) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let s2 = starts.clone();
    let available = o.available;
    let hooks = Hooks {
        router: Box::new(|_| None),
        start: Box::new(move |t: AgentTool, f: &str, p: &str, a: &str| { let mut s = s2.lock().unwrap(); s.push((t, f.into(), p.into(), a.into())); Ok(s.len() as i32) }),
        available: Box::new(move |t| available(t)),
        active_project: Box::new(|| None),
    };
    let (c2, sp2, o2, step) = (cloud.clone(), speech.clone(), opens.clone(), o.step);
    let v = Voice::build(settings.clone(), secrets, local.clone(), hooks,
        Box::new(move |k: &str, _: &str| { assert_eq!(k, "gsk_test"); c2.fetch_add(1, Ordering::SeqCst); sp2.clone() as Arc<dyn Speech> }),
        Box::new(move |_: Option<&str>, max: usize| { o2.fetch_add(1, Ordering::SeqCst); Ok(Box::new(FakeSrc { n: Cell::new(0), step, max }) as Box<dyn audio::Source>) }),
        (!o.countdown.is_zero()).then_some(o.countdown), o.max);
    H { v, settings, starts, cloud, opens, speech, local, dir }
}

fn wait(v: &Voice, f: impl Fn(&Stage) -> bool) -> Stage {
    let t0 = Instant::now();
    loop {
        let s = v.stage();
        if f(&s) { return s; }
        assert!(t0.elapsed() < Duration::from_secs(5), "stuck at {s:?}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn preview(v: &Voice) -> Preview { match wait(v, |s| matches!(s, Stage::Preview(_))) { Stage::Preview(p) => p, _ => unreachable!() } }

fn say(h: &H, trial: bool) -> Preview {
    h.v.press(trial);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    preview(&h.v)
}

#[test]
fn the_countdown_and_start_race_starts_once() {
    let h = harness("go to demo and fix the footer", Opt { countdown: Duration::from_millis(80), ..Opt::default() });
    let p = say(&h, false);
    assert_eq!((p.target_name.as_str(), p.task.as_str(), p.access.as_str()), ("demo", "fix the footer", "risky"));
    assert!(p.countdown.is_some_and(|c| c > 0.0 && c <= 0.08));
    let racers: Vec<_> = (0..4).map(|i| { let v = h.v.clone(); std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(60 + i * 10)); v.start_now(); }) }).collect();
    for r in racers { r.join().unwrap(); }
    let s = wait(&h.v, |s| matches!(s, Stage::Started { .. }));
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.starts.lock().unwrap().len(), 1, "one dispatch");
    assert_eq!(s, Stage::Started { session: 1, folder: p.folder.clone() });
    assert!(projects::same_folder(&p.folder, &h.dir.join("demo").to_string_lossy()));
}

#[test]
fn an_edit_stops_the_countdown_for_good_and_needs_start() {
    let h = harness("go to demo and fix the footer", Opt { countdown: Duration::from_millis(300), ..Opt::default() });
    say(&h, false);
    h.v.edit("fix the footer and the header");
    assert!(matches!(h.v.stage(), Stage::Editing(Preview { countdown: None, .. })));
    assert!(!h.v.can_start(), "not until the edit is checked");
    h.v.start_now();
    std::thread::sleep(Duration::from_millis(700));
    assert!(h.starts.lock().unwrap().is_empty(), "the countdown never starts an edited task");
    wait(&h.v, |_| h.v.can_start());
    let Stage::Editing(p) = h.v.stage() else { panic!() };
    assert_eq!(p.target_name, "demo", "an edit that names no project keeps the target");
    h.v.start_now();
    wait(&h.v, |s| matches!(s, Stage::Started { .. }));
    assert_eq!(h.starts.lock().unwrap()[0].2, "fix the footer and the header");
}

#[test]
fn a_cancel_drops_the_interaction_and_ignores_what_comes_later() {
    let h = harness("go to demo and fix the footer", Opt { delay: Duration::from_millis(400), ..Opt::default() });
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    wait(&h.v, |s| *s == Stage::Transcribing);
    h.v.cancel();
    assert_eq!(h.v.stage(), Stage::Cancelled);
    std::thread::sleep(Duration::from_millis(600));
    assert_eq!(h.v.stage(), Stage::Cancelled, "a late transcript changes nothing");
    assert!(h.speech.saw_cancel.load(Ordering::SeqCst));
    assert!(h.starts.lock().unwrap().is_empty());
    h.v.cancel();
    assert_eq!(h.v.stage(), Stage::Idle, "Escape again closes the card");
}

#[test]
fn a_failed_cleanup_keeps_the_original_transcript() {
    let (url, seen) = fake::serve(vec![fake::reply(500, "", "{}")]);
    let h = harness("um go to demo and fix the footer", Opt::default());
    let v = h.settings.voice();
    h.settings.set_voice(VoiceSettings { cleanup: true, cleanup_provider: projects::CleanupProvider::Custom, cleanup_base: Some(url), cleanup_model: Some("m".into()), ..v });
    h.v.secrets.set("cleanup.custom", Some("sk-c")).unwrap();
    let p = say(&h, false);
    assert_eq!((p.heard.as_str(), p.cleanup_note.as_deref()), ("um go to demo and fix the footer", Some(cleanup::FAILED)));
    assert_eq!(p.target_name, "demo", "and goes on");
    assert_eq!(seen.lock().unwrap().len(), 1);
    h.v.cancel();
}

#[test]
fn local_that_isnt_ready_never_uses_groq_and_local_is_let_go_after() {
    let h = harness("hello", Opt { mode: SpeechMode::Local, local_ready: false, ..Opt::default() });
    h.v.press(false);
    let s = wait(&h.v, |s| matches!(s, Stage::Error { .. }));
    assert_eq!(s, Stage::Error { message: "Set up local speech in Settings → Voice.".into(), retry: false, transcript: None });
    assert_eq!((h.cloud.load(Ordering::SeqCst), h.opens.load(Ordering::SeqCst)), (0, 0), "nothing recorded, nothing uploaded");
    let h = harness("write a haiku about rain", Opt { mode: SpeechMode::Local, ..Opt::default() });
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    wait(&h.v, |s| *s == Stage::Loading || matches!(s, Stage::Preview(_)));
    let p = preview(&h.v);
    assert_eq!((h.cloud.load(Ordering::SeqCst), h.local.shutdowns.load(Ordering::SeqCst)), (0, 1));
    assert_eq!(p.note.as_deref(), Some("Using default workspace: no project named. It will be made when the task starts."));
    h.v.cancel();
}

#[test]
fn a_trial_never_dispatches_or_makes_folders() {
    let h = harness("write a haiku about rain", Opt { countdown: Duration::from_millis(50), ..Opt::default() });
    let p = say(&h, true);
    assert!(p.trial && p.countdown.is_none());
    assert_eq!(p.note.as_deref(), Some("Using default workspace: no project named. It would be made when a task starts."));
    assert!(!h.v.can_start());
    h.v.start_now();
    std::thread::sleep(Duration::from_millis(300));
    assert!(h.starts.lock().unwrap().is_empty());
    assert!(!h.dir.join("ws").exists(), "the default workspace isn't made by a trial");
    assert!(matches!(h.v.stage(), Stage::Preview(_)));
}

#[test]
fn the_ten_minute_cap_stops_and_processes_once() {
    // A 16 000-sample cap stands in for ten minutes; nobody lets go.
    let h = harness("go to demo and fix the footer", Opt { max: 16_000, step: 6_000, ..Opt::default() });
    h.v.press(false);
    let p = preview(&h.v);
    assert_eq!(p.task, "fix the footer");
    h.v.release();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(h.speech.calls.load(Ordering::SeqCst), 1, "processed once");
    assert!(matches!(h.v.stage(), Stage::Preview(_)), "a later release does nothing");
    h.v.cancel();
}

#[test]
fn start_refuses_a_project_that_no_longer_takes_voice() {
    let h = harness("go to demo and fix the footer", Opt::default());
    say(&h, false);
    let mut pr = h.settings.projects()[0].clone();
    pr.voice = false;
    h.settings.update_project(pr).unwrap();
    h.v.start_now();
    let s = wait(&h.v, |s| matches!(s, Stage::Error { .. }));
    assert_eq!(s, Stage::Error { message: "“demo” no longer takes voice tasks.".into(), retry: true, transcript: Some("fix the footer".into()) });
    assert!(h.starts.lock().unwrap().is_empty());
    h.v.retry();
    assert!(matches!(h.v.stage(), Stage::Editing(_)), "back to the card, never resent on its own");
    assert!(h.starts.lock().unwrap().is_empty());
}

#[test]
fn an_unavailable_default_agent_asks_for_another_for_this_task() {
    let h = harness("go to demo and fix the footer", Opt { available: |t| t == AgentTool::Codex, ..Opt::default() });
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    let Stage::ChooseAgent(p) = wait(&h.v, |s| matches!(s, Stage::ChooseAgent(_))) else { unreachable!() };
    assert_eq!((p.text.as_str(), p.tools.clone()), ("go to demo and fix the footer", vec![AgentTool::Codex]));
    h.v.choose_agent(AgentTool::Codex);
    let pv = preview(&h.v);
    assert_eq!(pv.tool, AgentTool::Codex);
    h.v.start_now();
    wait(&h.v, |s| matches!(s, Stage::Started { .. }));
    assert_eq!(h.starts.lock().unwrap()[0].0, AgentTool::Codex);
    assert_eq!(h.settings.agent_tool(), AgentTool::Kiro, "the default is left as it was");
}

#[test]
fn an_empty_transcript_starts_nothing() {
    let h = harness("   ", Opt { countdown: Duration::from_millis(30), ..Opt::default() });
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    let s = wait(&h.v, |s| matches!(s, Stage::Error { .. }));
    assert_eq!(s, Stage::Error { message: NOTHING.into(), retry: true, transcript: None });
    std::thread::sleep(Duration::from_millis(100));
    assert!(h.starts.lock().unwrap().is_empty());
    h.v.retry();
    assert_eq!(h.v.stage(), Stage::Idle, "to speak again");
}

#[test]
fn a_press_while_busy_keeps_the_current_one() {
    let h = harness("go to demo and fix the footer", Opt::default());
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    assert!(h.v.busy_since().is_none());
    h.v.press(false);
    assert!(h.v.busy_since().is_some());
    h.v.release();
    preview(&h.v);
    assert_eq!(h.opens.load(Ordering::SeqCst), 1);
    h.v.cancel();
}

#[test]
fn another_agent_on_the_card_stops_the_countdown_and_start_uses_it_once() {
    let h = harness("go to demo and fix the footer", Opt { countdown: Duration::from_millis(300), ..Opt::default() });
    let p = say(&h, false);
    assert_eq!(p.tool, AgentTool::Kiro);
    h.v.change_agent(AgentTool::Codex);
    let Stage::Editing(e) = h.v.stage() else { panic!("{:?}", h.v.stage()) };
    assert_eq!((e.tool, e.countdown, e.target_name.as_str(), e.task.as_str()), (AgentTool::Codex, None, "demo", "fix the footer"), "the target isn't routed again");
    assert!(h.v.can_start(), "nothing to check: Start works at once");
    std::thread::sleep(Duration::from_millis(600));
    assert!(h.starts.lock().unwrap().is_empty(), "the countdown never fires after a change");
    h.v.start_now();
    h.v.start_now();
    wait(&h.v, |s| matches!(s, Stage::Started { .. }));
    std::thread::sleep(Duration::from_millis(100));
    let starts = h.starts.lock().unwrap();
    assert_eq!(starts.len(), 1, "one dispatch");
    assert_eq!(starts[0].0, AgentTool::Codex);
    assert_eq!((h.settings.agent_tool(), h.settings.voice().agent), (AgentTool::Kiro, None), "the defaults are left as they were");
}

#[test]
fn hold_stops_the_countdown_before_it_ends() {
    let h = harness("go to demo and fix the footer", Opt { countdown: Duration::from_millis(300), ..Opt::default() });
    say(&h, false);
    h.v.hold();
    assert!(matches!(h.v.stage(), Stage::Editing(Preview { countdown: None, tool: AgentTool::Kiro, .. })));
    std::thread::sleep(Duration::from_millis(600));
    assert!(h.starts.lock().unwrap().is_empty(), "no start when the countdown would have ended");
    assert!(h.v.can_start());
    h.v.cancel();
}

#[test]
fn a_change_after_cancel_is_ignored() {
    let h = harness("go to demo and fix the footer", Opt::default());
    say(&h, false);
    h.v.cancel();
    h.v.change_agent(AgentTool::Codex);
    h.v.hold();
    assert_eq!(h.v.stage(), Stage::Cancelled);
    assert!(h.starts.lock().unwrap().is_empty());
}

#[test]
fn voice_uses_its_own_default_agent_or_else_the_new_task_tool() {
    let h = harness("go to demo and fix the footer", Opt::default());
    h.settings.set_voice(VoiceSettings { agent: Some(AgentTool::Cursor), ..h.settings.voice() });
    assert_eq!(say(&h, false).tool, AgentTool::Cursor);
    h.v.cancel();
    h.settings.set_voice(VoiceSettings { agent: None, ..h.settings.voice() });
    h.settings.set_agent_tool(AgentTool::OpenCode);
    assert_eq!(say(&h, false).tool, AgentTool::OpenCode);
    h.v.cancel();
}

#[test]
fn the_countdown_is_settings_own_and_off_waits_for_start() {
    let h = harness("go to demo and fix the footer", Opt { countdown: Duration::ZERO, ..Opt::default() });
    let p = say(&h, false);
    assert!(p.countdown.is_some_and(|c| c > 4.0 && c <= 5.0), "five seconds by default: {:?}", p.countdown);
    assert_eq!(h.v.countdown_total(), Duration::from_secs(5));
    h.v.cancel();
    h.v.dismiss();
    h.settings.set_voice(VoiceSettings { countdown: 0, ..h.settings.voice() });
    h.v.press(false);
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    let e = match wait(&h.v, |s| matches!(s, Stage::Editing(_) | Stage::Preview(_))) { Stage::Editing(p) => p, s => panic!("Off counts down: {s:?}") };
    assert_eq!(e.countdown, None);
    std::thread::sleep(Duration::from_millis(200));
    assert!(h.starts.lock().unwrap().is_empty(), "nothing starts on its own");
    assert!(h.v.can_start());
    h.v.start_now();
    wait(&h.v, |s| matches!(s, Stage::Started { .. }));
    assert_eq!(h.starts.lock().unwrap().len(), 1);
}

#[test]
fn dictation_gives_the_words_and_starts_nothing() {
    let h = harness("go to demo and fix the footer", Opt::default());
    h.v.dictate();
    wait(&h.v, |s| matches!(s, Stage::Recording { .. }));
    h.v.release();
    let s = wait(&h.v, |s| matches!(s, Stage::Dictated(_)));
    assert_eq!(s, Stage::Dictated("go to demo and fix the footer".into()), "the words as heard, not routed into a task");
    std::thread::sleep(Duration::from_millis(100));
    assert!(h.starts.lock().unwrap().is_empty());
    // Dismissed, the next press is a fresh one.
    h.v.dismiss();
    assert_eq!(h.v.stage(), Stage::Idle);
    let p = say(&h, false);
    assert_eq!(p.task, "fix the footer");
}
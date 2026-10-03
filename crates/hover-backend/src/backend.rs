//! src/Hover.Backend/Program.cs's Backend: every command the host sends (Handle) and
//! every message it sends back, on hover-core, hover-agents and hover-quota. It lives on the
//! event loop's thread; what blocks (git, gh, a tool's check, the quotas, a setup) runs on a
//! thread of its own and posts or sends its answer when done. Callbacks from the sessions, the
//! history and the integrations are marshalled onto the loop (`Link::push` and `post`).

use crate::browser_host::{BrowserHost, Handle};
use crate::office::{self, Ctx};
use crate::panels;
use crate::prefs::MaxRunning;
use crate::quotas::{self, Credentials};
use crate::wire::{bool_of, int_of, str_of, Loop, Out};
use hover_agents::agents::{self, Toggles};
use hover_agents::ask::AskAnswer;
use hover_agents::desk::{self, Desk, Snap};
use hover_agents::github::{self, GitHubCli};
use hover_agents::runtime::Runtime;
use hover_agents::session::{KiroSession, KiroSessions, Rewind};
use hover_agents::{browser, computer_use, setup, spaces};
use hover_core::history::AgentHistory;
use hover_core::json::Json;
use hover_core::model::{notch_item, AgentTool, KiroState};
use hover_core::settings::Settings;
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{channel, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Result<T> = std::result::Result<T, String>;

fn st(s: &str) -> Json { Json::str(s) }
fn opt(s: Option<&str>) -> Json { Json::opt_str_of(s) }

/// The flags Backend keeps as fields (`checking`, `pushing`, `closing`, `readingQuotas`).
#[derive(Default)]
pub struct Flags {
    closing: AtomicBool,
    pushing: AtomicBool,
    checking: AtomicBool,
    reading_quotas: AtomicBool,
}

/// What a callback or a worker thread has of the backend: the loop to post to, the pipe to
/// the host, and the flags.
#[derive(Clone)]
pub struct Link {
    pub lp: Loop,
    pub out: Arc<Out>,
    pub flags: Arc<Flags>,
}

impl Link {
    pub fn new(lp: Loop, out: Arc<Out>) -> Link { Link { lp, out, flags: Arc::new(Flags::default()) } }

    /// Backend.Push: one `state` message for however many changes come before the loop
    /// gets to it.
    pub fn push(&self) {
        if self.flags.closing.load(Ordering::SeqCst) || self.flags.pushing.swap(true, Ordering::SeqCst) { return; }
        let flags = self.flags.clone();
        self.lp.post(move |h| {
            flags.pushing.store(false, Ordering::SeqCst);
            if !flags.closing.load(Ordering::SeqCst) { if let Some(b) = &h.backend { b.send_state(); } }
        });
    }

    fn closing(&self) -> bool { self.flags.closing.load(Ordering::SeqCst) }

    /// Runs `f` on the loop with the backend, unless it is closing.
    fn with(&self, f: impl FnOnce(&Backend) + Send + 'static) {
        let flags = self.flags.clone();
        self.lp.post(move |h| { if !flags.closing.load(Ordering::SeqCst) { if let Some(b) = &h.backend { f(b); } } });
    }
}

/// Clears a flag when the thread that held it ends, however it ends.
struct Hold(Arc<Flags>, fn(&Flags) -> &AtomicBool);

impl Drop for Hold {
    fn drop(&mut self) { (self.1)(&self.0).store(false, Ordering::SeqCst); }
}

pub struct Backend {
    link: Link,
    settings: Arc<Settings>,
    history: Arc<AgentHistory>,
    runtimes: Vec<Runtime>,
    sessions: KiroSessions,
    max_running: Arc<MaxRunning>,
    desk: Arc<Desk>,
    gh: Arc<GitHubCli>,
    credentials: Arc<Credentials>,
    browser: Arc<BrowserHost>,
    /// Ends the five-minute quota timer.
    quota_stop: Mutex<Option<Sender<()>>>,
    /// Ends the one-minute timer that turns idle desktops off.
    idle_stop: Mutex<Option<Sender<()>>>,
    /// The last time each project's desktop (by name) had an agent at work or was looked at.
    space_busy: Arc<Mutex<HashMap<String, Instant>>>,
}

/// Decoding of the host's key: Convert.FromBase64String.
fn decode_key(text: &str) -> Result<Vec<u8>> { hover_core::images::from_base64(text).ok_or_else(|| "The history key isn't valid base64.".to_owned()) }

/// The first command: `initialize` with the Keychain key as base64. The key is used once,
/// in place of note.key; history that can't be opened with it stops the backend rather
/// than be replaced by an empty one.
pub fn initialize(m: &Json, link: &Link) -> Result<Backend> {
    if str_of(m, "type") != Some("initialize") { return Err("Initialize the backend first.".into()); }
    let key = decode_key(str_of(m, "key").unwrap_or(""))?;
    hover_core::crypto::use_host_key(&key)?;
    Backend::new(link.clone())
}

impl Backend {
    fn new(link: Link) -> Result<Backend> {
        let crypto = hover_core::crypto::global().ok_or("The history key is not available.")?;
        // Do not let an invalid Keychain key turn existing encrypted history into an empty
        // index that a later session would overwrite.
        let dir = hover_core::paths::agents();
        let index = dir.join("index.dat");
        if index.is_file() {
            let plain = crypto.open(&std::fs::read(&index).map_err(|e| e.to_string())?);
            if plain.is_empty() { return Err("The history key cannot decrypt existing history.".into()); }
            hover_core::json::parse(&plain).map_err(|e| format!("The history index can't be read: {e}"))?;
        }
        let _ = std::fs::create_dir_all(&dir);
        let settings = Settings::load(hover_core::paths::settings_file());
        let history = Arc::new(AgentHistory::new(dir, crypto));
        let max_running = Arc::new(MaxRunning::load(hover_core::paths::support().join("backend.json")));

        let runtimes: Vec<Runtime> = AgentTool::ALL.iter().map(|&t| { let s = settings.clone(); Runtime::new(t, move || s.agent_options(t)) }).collect();
        // Computer use, the sandbox and the agent browser are read at every tool start.
        let st_ = settings.clone();
        agents::set_toggles(move || Toggles { computer_use: st_.computer_use(), sandbox: st_.sandbox(), agent_browser: st_.agent_browser(), folder: st_.kiro_folder() });
        let by_tool: Vec<(AgentTool, Runtime)> = runtimes.iter().map(|r| (r.tool(), r.clone())).collect();
        // Agent desktops (Cua Spaces) are read from the live settings too; a session's run
        // makes or starts its project's desktop first.
        let st_ = settings.clone();
        spaces::set_source(move || spaces::Switches { on: st_.agent_spaces(), linux: st_.space_image() == "linux" });
        let sessions = KiroSessions::new(move |tool| spaces::around(by_tool.iter().find(|(t, _)| *t == tool).expect("a runtime per tool").1.runner()), Some(history.clone()));
        let space_busy: Arc<Mutex<HashMap<String, Instant>>> = Arc::default();
        // OpenCode's one server serves every folder: its desktop is that of its newest
        // session at work.
        let ks = sessions.clone();
        spaces::set_opencode_folder(move || {
            ks.all().into_iter().filter(|x| x.tool == AgentTool::OpenCode && x.busy())
                .max_by_key(|x| x.current().map_or(0, |t| t.started_at.ticks)).map(|x| x.folder)
        });
        // A desktop may be turned off for another project's (two macOS desktops at most)
        // when none of its agents is at work and nobody has looked at it for a few minutes.
        let (ks, seen) = (sessions.clone(), space_busy.clone());
        spaces::set_can_pause(move |folder| {
            !ks.all().iter().any(|x| x.busy() && spaces::same_project(&x.folder, folder))
                && seen.lock().unwrap().get(&spaces::name_for(folder)).is_none_or(|t| t.elapsed() > Duration::from_secs(3 * 60))
        });
        // Chats keep the project folder before and after each turn, so they can go back to it;
        // without git there are none.
        sessions.set_max_running(max_running.get());
        if let Some(c) = hover_agents::checkpoint::Checkpoints::new(hover_core::paths::support().join("checkpoints")) { sessions.set_checkpoints(Arc::new(c)); }
        // Kiro's auto compact (Settings, off until switched on) is read from the live settings
        // at each prompt, not from the file they are written to after a pause.
        let s = settings.clone();
        sessions.set_auto_compact(move || s.kiro_auto_compact().then(|| s.kiro_compact_at()));
        for r in &runtimes {
            // What the tool offers (models, efforts) fills in its settings page.
            let (s, l) = (settings.clone(), link.clone());
            r.on_options_seen(move |t, offers| { s.set_agent_offers(t, offers); l.push(); });
            // A question goes to the session whose conversation it is; one nobody holds is turned down.
            let (ks, tool) = (sessions.clone(), r.tool());
            r.set_asking(Arc::new(move |sid, ask, ct, reply| ks.ask(tool, sid, ask, ct, reply)));
            let ks = sessions.clone();
            r.set_questioning(Arc::new(move |sid, ask, ct, reply| ks.ask_question(tool, sid, ask, ct, reply)));
        }

        // The Mac app drives a browser per session; agents get it as an MCP server.
        let browser_host = BrowserHost::new(link.out.clone(), sessions.clone());
        if std::env::var("HOVER_NO_BROWSER").as_deref() != Ok("1") { browser::set_host(Box::new(Handle(browser_host.clone()))); }

        let gh = github::shared();
        let l = link.clone();
        gh.on_changed(move || l.with(|b| b.send_github()));
        let l = link.clone();
        setup::on_change(move |_| l.push());
        let l = link.clone();
        setup::on_sandbox_change(move || l.with(|b| b.send_machine()));
        let l = link.clone();
        computer_use::on_change(move || l.with(|b| b.send_computer_use()));
        let l = link.clone();
        spaces::on_change(move || l.with(|b| { b.send_spaces(); b.link.push(); }));
        let l = link.clone();
        crate::updates::on_change(move || l.with(|b| { b.link.push(); b.send_computer_use(); }));
        let l = link.clone();
        sessions.on_changed(move || l.push());
        let l = link.clone();
        history.on_changed(move || l.push());
        // The tool and outcome let the native island show the tool's logo with a badge.
        let l = link.clone();
        sessions.on_ended(move |s, r| {
            if l.closing() { return; }
            l.out.send(&Json::obj(vec![
                ("type", st("ended")), ("title", st(&format!("{}: {}", s.tool.name(), s.title()))), ("text", st(r.state.name())),
                ("tool", st(s.tool.id())), ("task", st(&s.title())), ("ok", Json::Bool(r.state == KiroState::Completed)),
            ]));
        });

        let (stop, stopped) = channel::<()>();
        let l = link.clone();
        std::thread::Builder::new().name("quota-timer".into()).spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = stopped.recv_timeout(Duration::from_secs(5 * 60)) {
                l.with(|b| b.refresh_quotas());
            }
        }).map_err(|e| e.to_string())?;
        // A project's desktop holds 8 GB while on: off after 15 minutes with none of its
        // agents at work (its next task, or opening its Screen panel, starts it again).
        let (idle, idled) = channel::<()>();
        let l = link.clone();
        std::thread::Builder::new().name("spaces-idle".into()).spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = idled.recv_timeout(Duration::from_secs(60)) {
                l.with(|b| b.stop_idle_spaces());
            }
        }).map_err(|e| e.to_string())?;

        link.out.send(&Json::obj(vec![("type", st("initialized")), ("version", Json::int(1))]));
        // Whether this Mac runs Spaces is read once, from sw_vers: here, not on the loop.
        std::thread::spawn(|| { spaces::supported(); });
        Ok(Backend {
            link, settings, history, runtimes, sessions, max_running, desk: Desk::shared(), gh, credentials: Arc::new(Credentials::default()),
            browser: browser_host, quota_stop: Mutex::new(Some(stop)), idle_stop: Mutex::new(Some(idle)), space_busy,
        })
    }

    pub fn closing(&self) -> bool { self.link.closing() }

    // MARK: Messages out

    /// The office's state: every session, the tools and the history.
    pub fn send_state(&self) {
        let sessions = self.sessions.all();
        let history = self.history.entries();
        let ctx = self.ctx(&sessions, &history);
        self.link.out.send(&office::snapshot(&ctx));
    }

    fn ctx<'a>(&'a self, sessions: &'a [KiroSession], history: &'a [hover_core::history::HistoryEntry]) -> Ctx<'a> {
        Ctx {
            settings: &self.settings, sessions, history, can_start: self.can_start(), max_running: self.max_running.get(),
            checkpoints: self.sessions.checkpoints().is_some(), known: &agents::known,
        }
    }

    fn can_start(&self) -> bool { self.sessions.can_start() && self.sessions.running() < self.max_running.get() }

    /// SendGitHub: what is known of the GitHub CLI, and its setup's progress.
    pub fn send_github(&self) {
        let s = self.gh.known();
        let p = self.gh.setup();
        self.link.out.send(&Json::obj(vec![
            ("type", st("gh")), ("checked", Json::Bool(s.is_some())), ("installed", Json::Bool(s.as_ref().is_some_and(|s| s.installed))),
            ("signedIn", Json::Bool(s.as_ref().is_some_and(|s| s.signed_in))), ("user", opt(s.as_ref().and_then(|s| s.user.as_deref()))),
            ("version", opt(s.as_ref().and_then(|s| s.version.as_deref()))),
            ("step", opt(p.step.map(|s| s.name()))), ("line", st(&p.line)), ("code", opt(p.code.as_deref())), ("error", opt(p.error.as_deref())),
            ("busy", Json::Bool(self.gh.busy())), ("url", st(p.url.as_deref().unwrap_or(github::DEVICE_URL))),
        ]));
    }

    fn send_preferences(&self) {
        let s = &self.settings;
        let mut fields = vec![
            ("type", st("preferences")), ("maxRunning", Json::int(self.max_running.get() as i64)), ("hover", Json::Bool(s.hover_opens_workspace())),
            ("noticeSeen", Json::Bool(s.kiro_notice_seen())), ("quotaItems", Json::Arr(s.notch_items().iter().map(|i| st(i)).collect())),
            ("computerUse", Json::Bool(s.computer_use())), ("sandbox", Json::Bool(s.sandbox())), ("agentBrowser", Json::Bool(s.agent_browser())),
            ("agentSpaces", Json::Bool(s.agent_spaces())), ("spaceImage", st(s.space_image())), ("spacesSupported", Json::Bool(spaces::supported())),
            // Kiro's auto compact: off until switched on, at this percent of the context (added for Rust).
            ("kiroAutoCompact", Json::Bool(s.kiro_auto_compact())), ("kiroCompactAt", Json::int(s.kiro_compact_at() as i64)),
            ("tools", Json::Arr(AgentTool::ALL.iter().map(|&t| {
                let o = s.agent_options(t);
                Json::obj(vec![("id", st(t.id())), ("access", st(o.access_id(true))), ("idle", Json::int(o.idle_minutes as i64)), ("hideSteps", Json::Bool(o.hide_steps))])
            }).collect())),
        ];
        fields.extend(Self::machine());
        self.link.out.send(&Json::obj(fields));
    }

    /// What a fresh Mac may still lack, each set up from Settings → General in one click:
    /// the sandbox's srt and ripgrep (with its setup's progress) and git (checkpoints, the
    /// desk's Diff and pull requests; the Command Line Tools on a Mac).
    fn machine() -> Vec<(&'static str, Json)> {
        let needs = setup::sandbox_needs().unwrap_or(setup::SandboxNeeds { srt_missing: false, rg_missing: false });
        let (p, busy) = setup::sandbox_progress();
        vec![
            ("sandboxNeeds", Json::Arr([(needs.srt_missing, "srt"), (needs.rg_missing, "ripgrep")].iter().filter(|x| x.0).map(|x| st(x.1)).collect())),
            ("sandboxSetup", Json::obj(vec![("busy", Json::Bool(busy)), ("line", st(&p.line)), ("error", opt(p.error.as_deref()))])),
            ("gitInstalled", Json::Bool(hover_agents::desk::find_git().is_some())),
        ]
    }

    /// The same, on its own (a setup's progress, a fresh look from Settings).
    fn send_machine(&self) {
        let mut fields = vec![("type", st("machine"))];
        fields.extend(Self::machine());
        self.link.out.send(&Json::obj(fields));
    }

    /// Cua Driver as Settings → Computer Use shows it: installed, its grants, and a setup's
    /// progress. Checked is false until a check has finished.
    pub fn send_computer_use(&self) {
        let s = computer_use::known();
        let p = computer_use::setup();
        self.link.out.send(&Json::obj(vec![
            ("type", st("computerUse")), ("on", Json::Bool(self.settings.computer_use())), ("checked", Json::Bool(s.is_some())),
            ("installed", Json::Bool(s.as_ref().is_some_and(|s| s.installed))), ("version", st(s.as_ref().map_or("", |s| s.version.as_str()))),
            ("permissions", st(s.as_ref().map_or("unknown", |s| s.permissions))), ("ready", Json::Bool(s.as_ref().is_some_and(|s| s.ready()))),
            ("hint", st(s.as_ref().map_or("", |s| s.hint.as_str()))), ("canGrant", Json::Bool(computer_use::can_grant())),
            ("installHint", st(computer_use::install_hint())),
            ("step", opt(p.step.as_deref())), ("line", st(&p.line)), ("error", opt(p.error.as_deref())), ("busy", Json::Bool(computer_use::busy())),
            ("update", crate::updates::state(crate::updates::CUA_DRIVER)),
        ]));
    }

    // MARK: Agent desktops

    /// SendSpaces: agent desktops as Settings → Computer Use shows them: installed, whether
    /// the image is on this Mac, how many run, and a setup's progress. Checked is false until
    /// a check has finished; where Spaces can't run, the hint says so from the start.
    pub fn send_spaces(&self) {
        let k = spaces::known();
        let p = spaces::setup();
        self.link.out.send(&Json::obj(vec![
            ("type", st("spaces")), ("on", Json::Bool(self.settings.agent_spaces())), ("image", st(self.settings.space_image())),
            ("supported", Json::Bool(spaces::supported())), ("checked", Json::Bool(k.is_some())),
            ("installed", Json::Bool(k.as_ref().is_some_and(|k| k.installed))), ("ready", Json::Bool(k.as_ref().is_some_and(|k| k.ready))),
            ("version", opt(k.as_ref().and_then(|k| k.version.as_deref()))),
            ("hint", st(k.as_ref().map_or_else(|| spaces::note().unwrap_or(""), |k| k.hint.as_str()))),
            ("running", Json::int(k.as_ref().map_or(0, |k| k.running) as i64)),
            ("step", opt(p.step.as_deref())), ("line", st(&p.line)), ("fraction", p.fraction.map_or(Json::Null, Json::double)),
            ("error", opt(p.error.as_deref())), ("busy", Json::Bool(spaces::busy())),
        ]));
    }

    /// StopIdleSpaces: a project's desktop is turned off once none of its agents has worked
    /// for 15 minutes (the clock starts when it is first seen, and runs on from the last
    /// time an agent was at work or its panel was opened).
    fn stop_idle_spaces(&self) {
        if self.link.closing() || !spaces::wanted() { return; }
        let now = Instant::now();
        let all = self.sessions.all();
        let mut seen = self.space_busy.lock().unwrap();
        // Every desktop that is on, with or without agents in the office (a drop on a
        // project with none of its agents there starts one too).
        for folder in spaces::ready_folders() {
            let name = spaces::name_for(&folder);
            if all.iter().any(|x| x.busy() && spaces::same_project(&x.folder, &folder)) || !seen.contains_key(&name) { seen.insert(name, now); continue; }
            if now.duration_since(seen[&name]) > Duration::from_secs(15 * 60) && !spaces::in_use(&folder) {
                seen.insert(name, now);
                self.spawn("space-stop", move || spaces::stop(&folder));
            }
        }
    }

    /// Any session of this project left, in the office or the history.
    fn uses_folder(&self, folder: &str) -> bool {
        self.sessions.all().iter().any(|x| spaces::same_project(&x.folder, folder)) || self.history.entries().iter().any(|e| spaces::same_project(&e.folder, folder))
    }

    /// `spaceView`: the session's desktop viewer, answered as `space` when it is ready.
    /// Opening the panel makes or starts the desktop when it isn't on (the viewer does),
    /// and counts as the project being in use.
    fn space_view(&self, s: &KiroSession) {
        self.space_busy.lock().unwrap().insert(spaces::name_for(&s.folder), Instant::now());
        let (folder, id, out) = (s.folder.clone(), s.id, self.link.out.clone());
        self.spawn("space-view", move || {
            let data = guarded(|| spaces::viewer(&folder), "The desktop’s viewer didn’t open.");
            out.send(&Json::obj(vec![("type", st("space")), ("id", Json::int(id as i64)), ("data", data)]));
        });
    }

    /// The project a teleport or a file drop is for: the session's, or (no agent at work
    /// yet) a project folder the message names.
    fn drop_target(m: &Json, s: Option<&KiroSession>) -> Option<String> {
        s.map(|s| s.folder.clone()).or_else(|| str_of(m, "folder").filter(|f| hover_agents::usable_folder(Some(f))).map(str::to_owned))
    }

    /// `teleport`: an app sent to the project's desktop (dragged to the notch, or Send to
    /// Hover VM). First its plan: an app whose data could go too (Safari's tabs, an app Cua
    /// teleports) comes back as `review` for the user to tick; any other goes at once. The
    /// answer comes back with `include`, exactly the items ticked. `teleport` messages for
    /// each step and the end.
    fn teleport(&self, m: &Json, s: Option<&KiroSession>) {
        let (Some(folder), Some(path)) = (Self::drop_target(m, s), str_of(m, "path").map(str::to_owned)) else { return };
        self.space_busy.lock().unwrap().insert(spaces::name_for(&folder), Instant::now());
        let id = s.map_or(0, |s| s.id);
        let app = str_of(m, "app").map(str::to_owned).unwrap_or_else(|| std::path::Path::new(&path).file_stem().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default());
        let bundle = str_of(m, "bundle").filter(|b| !b.is_empty() && b.len() < 256).map(str::to_owned);
        let strings = |key: &str, max: usize| -> Option<Vec<String>> {
            match m.get(key) { Some(Json::Arr(a)) => Some(a.iter().filter_map(Json::as_str).take(max).map(str::to_owned).collect()), _ => None }
        };
        let urls = strings("urls", 200).unwrap_or_default();
        let include = strings("include", 64).map(|i| i.into_iter().filter(|x| !x.is_empty() && x.len() < 400).collect::<Vec<_>>());
        // The browser's tabs as the host read them (`tabs`), or an older message's addresses.
        // Answering the review, `tabs` are the ones picked; an older answer ticked "tabs" for all.
        let tabs = match (&include, m.get("tabs")) {
            (Some(_), Some(t)) => spaces::tabs_of(Some(t), &[]),
            (Some(picked), None) if picked.iter().any(|i| i == "tabs") => spaces::tabs_of(None, &urls),
            (Some(_), None) => vec![],
            (None, t) => spaces::tabs_of(t, &urls),
        };
        let include = include.map(|i| i.into_iter().filter(|x| x != "tabs").collect::<Vec<_>>());
        let to = str_of(m, "folder").map(str::to_owned);
        let out = self.link.out.clone();
        let sending = move |out: &Out, app: &str, line: &str| {
            out.send(&Json::obj(vec![("type", st("teleport")), ("id", Json::int(id as i64)), ("phase", st("sending")), ("app", st(app)), ("line", st(line))]));
        };
        // With the bundle, so the host knows which of its apps (or tabs) went.
        let done = move |out: &Out, app: &str, bundle: Option<&str>, data: Json| {
            out.send(&Json::obj(vec![("type", st("teleport")), ("id", Json::int(id as i64)), ("app", st(app)), ("bundle", Json::opt_str_of(bundle)), ("phase", st("done")), ("data", data)]));
        };
        match include {
            Some(picked) => {
                sending(&out, &app, &if picked.is_empty() && tabs.is_empty() { format!("Sending {app}…") } else { format!("Teleporting {app}…") });
                self.spawn("teleport", move || {
                    let data = guarded(|| spaces::teleport(&folder, bundle.as_deref(), &path, &app, &picked, &tabs, &|line| sending(&out, &app, line)), "The app didn’t go.");
                    done(&out, &app, bundle.as_deref(), data);
                });
            }
            None => {
                sending(&out, &app, &format!("Looking at {app}…"));
                self.spawn("teleport", move || {
                    let plan = guarded(|| spaces::plan(bundle.as_deref(), &app, &tabs), "");
                    if matches!(plan.get("review"), Some(Json::Bool(true))) {
                        out.send(&Json::obj(vec![
                            ("type", st("teleport")), ("id", Json::int(id as i64)), ("folder", Json::opt_str_of(to.as_deref())), ("app", st(&app)),
                            ("bundle", Json::opt_str_of(bundle.as_deref())), ("path", st(&path)),
                            ("urls", Json::Arr(tabs.iter().map(|t| Json::str(&t.url)).collect())), ("phase", st("review")), ("plan", plan),
                        ]));
                        return;
                    }
                    // Nothing to pick: it goes as it is (a browser with its one tab).
                    let data = guarded(|| spaces::teleport(&folder, bundle.as_deref(), &path, &app, &[], &tabs, &|line| sending(&out, &app, line)), "The app didn’t go.");
                    done(&out, &app, bundle.as_deref(), data);
                });
            }
        }
    }

    /// `spaceFiles`: files dropped on the project's desktop in the notch.
    fn space_files(&self, m: &Json, s: Option<&KiroSession>) {
        let Some(folder) = Self::drop_target(m, s) else { return };
        let Some(Json::Arr(items)) = m.get("paths") else { return };
        self.space_busy.lock().unwrap().insert(spaces::name_for(&folder), Instant::now());
        let paths: Vec<String> = items.iter().filter_map(|x| x.as_str()).filter(|x| !x.is_empty()).map(str::to_owned).collect();
        let (id, out) = (s.map_or(0, |s| s.id), self.link.out.clone());
        self.spawn("space-files", move || {
            let data = guarded(|| spaces::send_files(&folder, &paths), "The files didn’t go.");
            out.send(&Json::obj(vec![("type", st("teleport")), ("id", Json::int(id as i64)), ("phase", st("done")), ("app", st("files")), ("data", data)]));
        });
    }

    // MARK: Work off the loop

    /// Check: every tool's install and sign-in, together; then the state.
    fn check(&self) {
        if self.link.flags.checking.swap(true, Ordering::SeqCst) { return; }
        let link = self.link.clone();
        std::thread::spawn(move || {
            let _hold = Hold(link.flags.clone(), |f| &f.checking);
            let all: Vec<_> = AgentTool::ALL.iter().map(|&t| std::thread::spawn(move || { agents::check(t, true); })).collect();
            for h in all { let _ = h.join(); }
            drop(_hold);
            link.push();
        });
    }

    /// RefreshQuotas: on a thread, since kiro-cli takes seconds and Claude's sign-in is the host's to give.
    pub fn refresh_quotas(&self) {
        if self.link.closing() || self.link.flags.reading_quotas.swap(true, Ordering::SeqCst) { return; }
        let (settings, credentials, link) = (self.settings.clone(), self.credentials.clone(), self.link.clone());
        std::thread::spawn(move || {
            let _hold = Hold(link.flags.clone(), |f| &f.reading_quotas);
            let m = quotas::read_all(&settings, &credentials, &link.out);
            if !link.closing() { link.out.send(&m); }
        });
    }

    fn spawn(&self, name: &str, f: impl FnOnce() + Send + 'static) { let _ = std::thread::Builder::new().name(name.into()).spawn(f); }

    // MARK: Commands

    /// Backend.Handle. An error is shown to the user as a toast.
    pub fn handle(&mut self, m: &Json) -> Result<()> {
        if !matches!(m, Json::Obj(_)) { return Err("Invalid host message.".into()); }
        let s = int_of(m, "id").and_then(|id| self.sessions.get(id));
        match str_of(m, "type") {
            Some("ready") => {
                self.link.push();
                self.check();
                self.refresh_quotas();
                self.spawn("update-check", || crate::updates::check(false));
                if self.settings.computer_use() { self.spawn("computer-use", || { computer_use::check(false); }); }
            }
            Some("new") => self.start(m)?,
            Some("reply") => {
                let s = s.or_else(|| str_of(m, "key").and_then(|k| self.sessions.wake(k)));
                let images = save_images(m);
                if !s.is_some_and(|s| self.sessions.reply(s.id, str_of(m, "text").unwrap_or(""), images)) {
                    return Err("Could not send this reply. A desk may be busy.".into());
                }
            }
            Some("stop") => { if let Some(s) = s { self.sessions.stop(s.id); } }
            Some("answer") => self.answer(m, s)?,
            Some("delete") => {
                if let Some(key) = s.as_ref().map(|s| s.key.as_str()).or_else(|| str_of(m, "key")) {
                    let gone = s.as_ref().map(|s| s.folder.clone()).or_else(|| self.history.entries().into_iter().find(|e| e.key == key).map(|e| e.folder));
                    self.sessions.delete(key);
                    desk::forget_apps(key);
                    // A project's desktop goes with its last session, in the office or the history.
                    if let Some(folder) = gone.filter(|f| !self.uses_folder(f)) { self.spawn("space-delete", move || spaces::delete(&folder)); }
                }
            }
            Some("remove") => {
                if let Some(s) = s {
                    self.sessions.dismiss(s.id);
                    // Off once no agent of the project is left in the office.
                    if !self.sessions.all().iter().any(|x| spaces::same_project(&x.folder, &s.folder)) { self.spawn("space-stop", move || spaces::stop(&s.folder)); }
                }
            }
            Some("history") => {
                if let Some(saved) = str_of(m, "key").and_then(|k| self.sessions.saved(k)) {
                    let mut view = KiroSession::new(saved.tool);
                    view.restore(&saved);
                    let ctx = self.ctx(&[], &[]);
                    self.link.out.send(&office::transcript(&ctx, &view));
                }
            }
            Some("open") => self.sessions.select(s.map(|s| s.id)),
            // The desk menu's panels: read here, answered when git or gh are done.
            Some("desk") => { if let Some(s) = s { self.desk_panel(&s, str_of(m, "what"), str_of(m, "arg")); } }
            // Hover's browser answered a call (BrowserTool).
            Some("browserResult") => self.browser.complete(m),
            // The desk's buttons that change the folder: Create pull request.
            Some("deskAction") => {
                if let (Some(s), Some("prCreate")) = (s, str_of(m, "what")) { self.desk_action(&s, m.get("args").cloned().unwrap_or(Json::Null)); }
            }
            // The session's own desktop (a Cua Space): its live viewer, an app teleported into
            // it from the notch, files dropped on it, and Settings → Computer Use's setup.
            Some("spaceView") => { if let Some(s) = s { self.space_view(&s); } }
            Some("teleport") => self.teleport(m, s.as_ref()),
            Some("spaceFiles") => self.space_files(m, s.as_ref()),
            Some("spaces") => match str_of(m, "step") {
                Some("setup") => self.spawn("spaces-setup", spaces::run_setup),
                Some("cancel") => spaces::cancel(),
                _ => {
                    self.send_spaces();
                    self.spawn("spaces-check", || { spaces::check(true); });
                }
            },
            // The GitHub CLI: what is known, a fresh check, and its one-click setup.
            Some("gh") => match str_of(m, "step") {
                Some("setup") => { self.gh.start(); }
                Some("cancel") => self.gh.cancel(),
                _ => {
                    self.send_github();
                    let gh = self.gh.clone();
                    self.spawn("github-check", move || { gh.check(true); });
                }
            },
            Some("setModel") => {
                if let Some(t) = AgentTool::parse(str_of(m, "tool")) {
                    let mut o = self.settings.agent_options(t);
                    o.model = str_of(m, "model").filter(|x| !x.is_empty()).map(str::to_owned);
                    o.effort = str_of(m, "effort").map(str::to_owned);
                    self.settings.set_agent_options(t, o);
                }
                self.link.push();
            }
            Some("getSettings") => self.send_preferences(),
            Some("saveSettings") => {
                self.save_preferences(m);
                self.send_preferences();
                self.link.push();
                self.refresh_quotas();
            }
            Some("refresh") => {
                self.check();
                self.refresh_quotas();
                self.spawn("update-check", || crate::updates::check(false));
                if self.settings.computer_use() { self.spawn("computer-use", || { computer_use::check(true); }); }
            }
            // Settings → Computer Use: what is known now, then a fresh check.
            Some("computerUse") => {
                self.send_computer_use();
                self.spawn("computer-use", || { computer_use::check(true); });
            }
            Some("computerUseSetup") => {
                match str_of(m, "step") {
                    Some("install") => self.spawn("computer-use-install", computer_use::install),
                    Some("grant") => self.spawn("computer-use-grant", computer_use::grant),
                    Some("cancel") => computer_use::cancel(),
                    _ => {}
                }
                self.send_computer_use();
            }
            // Settings → General's Set Up beside the sandbox switch, and a fresh look at what
            // this Mac lacks (Settings shown, the Command Line Tools' installer closed).
            Some("machine") => self.send_machine(),
            Some("sandboxSetup") => {
                if str_of(m, "step") == Some("cancel") { setup::cancel_sandbox(); return Ok(()); }
                let link = self.link.clone();
                self.spawn("sandbox-setup", move || { setup::run_sandbox(); link.with(|b| b.send_machine()); });
            }
            Some("setup") => {
                let Some(t) = AgentTool::parse(str_of(m, "tool")) else { return Ok(()) };
                if str_of(m, "step") == Some("cancel") { setup::cancel(t); return Ok(()); }
                let link = self.link.clone();
                self.spawn("agent-setup", move || {
                    setup::run(t, None);
                    link.with(|b| { b.link.push(); b.refresh_quotas(); });
                });
                self.link.push();
            }
            // A tool's one-click update (its logo's badge, Settings, the menu bar): never
            // under a task of it, and its process starts afresh on its next task.
            Some("update") => {
                let Some(id) = str_of(m, "tool").and_then(|i| crate::updates::ids().find(|x| *x == i)) else { return Ok(()) };
                let (tool, sessions, link) = (AgentTool::parse(Some(id)), self.sessions.clone(), self.link.clone());
                self.spawn("update", move || {
                    let busy = || tool.is_some_and(|t| sessions.all().iter().any(|x| x.tool == t && x.busy()));
                    crate::updates::update(id, busy);
                    link.with(move |b| {
                        if let Some(t) = tool.filter(|t| !b.sessions.all().iter().any(|x| x.tool == *t && x.busy())) {
                            if let Some(r) = b.runtimes.iter().find(|r| r.tool() == t) { r.shutdown("updated"); }
                        }
                        b.link.push();
                    });
                });
            }
            Some("claudeCredentials") => self.credentials.answer(str_of(m, "json").map(str::to_owned)),
            // Restore to just after an answer, and Try again from a message: the chat and
            // its folder go back to a checkpoint (added for Rust).
            Some(kind @ ("restore" | "again")) => {
                let Some(s) = s else { return Err("That chat isn't here.".into()) };
                let Some(turn) = int_of(m, "turn").and_then(|n| usize::try_from(n).ok()) else { return Err("That message isn't here.".into()) };
                let to = if kind == "restore" { Rewind::After(turn) } else { Rewind::Before(turn) };
                let (sessions, out) = (self.sessions.clone(), self.link.out.clone());
                self.spawn("rewind", move || { if let Err(e) = sessions.rewind(s.id, to) { out.toast(&e); } });
            }
            Some("shutdown") => self.shutdown(),
            _ => {}
        }
        Ok(())
    }

    /// `new`: a task in a folder, for a tool, with the access picked (or the tool's own).
    fn start(&self, m: &Json) -> Result<()> {
        let folder = str_of(m, "folder");
        let tool = AgentTool::parse(str_of(m, "tool")).unwrap_or_else(|| self.settings.agent_tool());
        if !hover_agents::usable_folder(folder) { return Err("Choose an existing project folder.".into()); }
        if !self.settings.kiro_notice_seen() { return Err("Review agent access in Settings before starting your first task.".into()); }
        let known = agents::known(tool);
        if !known.as_ref().is_some_and(|k| k.ok()) { return Err(known.map_or_else(|| "The tool is not ready yet.".into(), |k| k.hint)); }
        let read_only_works = agents::read_only_works(tool);
        let access = str_of(m, "access").filter(|a| matches!(*a, "full" | "risky" | "always" | "read")).unwrap_or_else(|| self.settings.agent_options(tool).access_id(read_only_works));
        let folder = folder.unwrap();
        self.settings.set_kiro_folder(Some(folder));
        self.settings.set_agent_tool(tool);
        let busy = !self.can_start();
        if busy || self.sessions.start_as(tool, folder, str_of(m, "prompt").unwrap_or(""), save_images(m), Some(access)).is_none() {
            return Err("All available desks are busy, or the prompt is empty.".into());
        }
        Ok(())
    }

    fn answer(&self, m: &Json, s: Option<KiroSession>) -> Result<()> {
        let (Some(s), Some(ask)) = (s, str_of(m, "ask")) else { return Ok(()) };
        if let Some(Json::Arr(answers)) = m.get("answers") {
            let lists: Vec<Vec<String>> = answers.iter().map(|a| match a {
                Json::Arr(xs) => xs.iter().map(|x| x.as_str().unwrap_or("").to_owned()).filter(|x| x.encode_utf16().count() <= 4000).collect(),
                _ => vec![],
            }).collect();
            if !self.sessions.answer_question(s.id, ask, lists) { return Err("Choose an answer first.".into()); }
        } else {
            self.sessions.answer(s.id, ask, match str_of(m, "answer") {
                Some("allow") => AskAnswer::Allow, Some("trust") => AskAnswer::Trust, Some("trustAll") => AskAnswer::TrustAll, _ => AskAnswer::Deny,
            });
        }
        Ok(())
    }

    /// `desk`: a panel of the desk menu, read off the loop and answered as `desk`.
    fn desk_panel(&self, s: &KiroSession, what: Option<&str>, arg: Option<&str>) {
        let (snap, desk, out) = (Snap::of(s), self.desk.clone(), self.link.out.clone());
        let (id, what, arg) = (s.id, what.map(str::to_owned), arg.map(str::to_owned));
        self.spawn("desk", move || {
            let data = guarded(|| panels::answer(&desk, &snap, what.as_deref(), arg.as_deref()), "Couldn’t read that.");
            out.send(&Json::obj(vec![("type", st("desk")), ("id", Json::int(id as i64)), ("what", opt(what.as_deref())), ("arg", opt(arg.as_deref())), ("data", data)]));
        });
    }

    /// `deskAction`: Create pull request, answered as `deskAction`.
    fn desk_action(&self, s: &KiroSession, args: Json) {
        let (snap, desk, out, id) = (Snap::of(s), self.desk.clone(), self.link.out.clone(), s.id);
        self.spawn("desk-action", move || {
            let data = guarded(|| panels::created(&desk, &snap, &args), "That didn’t work.");
            out.send(&Json::obj(vec![("type", st("deskAction")), ("id", Json::int(id as i64)), ("what", st("prCreate")), ("data", data)]));
        });
    }

    /// SavePreferences: what the Settings window sent, each part when it is there.
    fn save_preferences(&self, m: &Json) {
        let s = &self.settings;
        if let Some(cap) = int_of(m, "maxRunning") { self.max_running.set(cap); self.sessions.set_max_running(self.max_running.get()); }
        if let Some(on) = bool_of(m, "hover") { s.set_hover_opens_workspace(on); }
        if bool_of(m, "noticeSeen") == Some(true) { s.set_kiro_notice_seen(true); }
        // Every tool picks it up from its next session (a running one when it is next idle).
        if let Some(on) = bool_of(m, "computerUse").filter(|on| *on != s.computer_use()) {
            s.set_computer_use(on);
            if on { self.spawn("computer-use", || { computer_use::check(true); }); }
        }
        // Each tool picks it up when it next starts, and what it needs installed is checked again.
        if let Some(on) = bool_of(m, "sandbox").filter(|on| *on != s.sandbox()) {
            s.set_sandbox(on);
            self.check();
        }
        // Each session gets its project's desktop from its next run.
        if let Some(on) = bool_of(m, "agentSpaces") {
            s.set_agent_spaces(on);
            self.spawn("spaces-check", || { spaces::check(true); });
        }
        if let Some(image) = str_of(m, "spaceImage") { s.set_space_image(image); }
        // Each session gets it from its next run.
        if let Some(on) = bool_of(m, "agentBrowser") { s.set_agent_browser(on); }
        if let Some(on) = bool_of(m, "kiroAutoCompact") { s.set_kiro_auto_compact(on); }
        if let Some(pct) = int_of(m, "kiroCompactAt") { s.set_kiro_compact_at(pct.clamp(1, 100) as u8); }
        if let Some(Json::Arr(items)) = m.get("quotaItems") {
            let ids: Vec<&str> = items.iter().map(|x| x.as_str().unwrap_or("")).filter(|x| notch_item::ALL.contains(x)).collect();
            s.set_notch_items(&ids);
        }
        if let Some(Json::Arr(tools)) = m.get("tools") {
            for t in tools {
                let Some(tool) = AgentTool::parse(str_of(t, "id")) else { continue };
                let mut o = s.agent_options(tool).with_access(str_of(t, "access"));
                if let Some(minutes) = int_of(t, "idle") { o.idle_minutes = minutes; }
                if let Some(hide) = bool_of(t, "hideSteps") { o.hide_steps = hide; }
                s.set_agent_options(tool, o);
            }
        }
    }

    /// Backend.Shutdown: the quota timer, the browser, every run and every tool, then the
    /// history and the settings are written out.
    pub fn shutdown(&self) {
        if self.link.flags.closing.swap(true, Ordering::SeqCst) { return; }
        drop(self.quota_stop.lock().unwrap().take());
        drop(self.idle_stop.lock().unwrap().take());
        // The desktops themselves are turned off by the Mac host as Hover quits (a VM takes
        // a while to stop, and this process is about to end); nothing waits on them here.
        browser::clear_host();
        self.browser.stop();
        browser::stop();
        self.sessions.stop_all();
        for r in &self.runtimes { r.shutdown("Hover quit"); }
        // The runs that just ended save themselves; the history is written after them.
        let until = Instant::now() + Duration::from_secs(3);
        while self.sessions.running() > 0 && Instant::now() < until { std::thread::sleep(Duration::from_millis(25)); }
        self.history.flush();
        self.settings.flush();
    }
}

/// A panel's answer, or `{error}` if reading it panicked.
fn guarded(f: impl FnOnce() -> Json, fallback: &str) -> Json {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)).unwrap_or_else(|p| {
        let why = p.downcast_ref::<String>().cloned().or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string())).unwrap_or_else(|| fallback.to_owned());
        Json::obj(vec![("error", Json::Str(why))])
    })
}

/// Backend.SaveImages: a message's pictures (data: URLs, or `{data}`), as files for the agent.
fn save_images(m: &Json) -> Vec<String> {
    let Some(Json::Arr(items)) = m.get("images") else { return vec![] };
    let items: Vec<Json> = items.iter().map(|i| match i {
        Json::Str(_) => i.clone(),
        other => Json::Str(str_of(other, "data").unwrap_or("").to_owned()),
    }).collect();
    hover_core::images::save(&items, &hover_core::images::folder(hover_core::paths::support())).into_iter().map(|p| p.to_string_lossy().into_owned()).collect()
}

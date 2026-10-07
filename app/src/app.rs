//! Owl/OwlApp.cs: the shared state every view draws from (the settings, the agents'
//! processes, their sessions and history, the quota readings), the ends announced
//! while nobody watches, and the orderly quit. No UI here: the views register hooks.

use hover_agents::orch::{Orch, SystemEnv};
use hover_agents::runtime::Runtime;
use hover_agents::session::{KiroSession, KiroSessions, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::text;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use hover_core::settings::Settings;
use hover_quota::credits::Credits;
use hover_quota::schedule::Poller;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

type Hook = Arc<dyn Fn() + Send + Sync>;
type Notify = Arc<dyn Fn(&str, &str) + Send + Sync>;
/// A quota reader by notch id (tests hand in a stand-in).
pub type Reader = Arc<dyn Fn(&str) -> hover_quota::Reading + Send + Sync>;

#[derive(Default)]
struct Hooks {
    notify: Option<Notify>,
    quotas: Vec<Hook>,
    sessions: Vec<Hook>,
}

#[derive(Default)]
struct Unseen { count: usize, tool: Option<&'static str>, last: Option<UnseenEnd> }

/// OwlApp.KiroUnseenLast: the latest end nobody saw, for the notch: which tool, the
/// task, how it went and how long it took.
#[derive(Clone, Debug, PartialEq)]
pub struct UnseenEnd { pub tool: AgentTool, pub title: String, pub state: KiroState, pub took_secs: f64 }

pub struct Hover {
    pub settings: Arc<Settings>,
    pub history: Option<Arc<AgentHistory>>,
    /// Each tool's runtime: an ACP server for Kiro, Codex and Cursor, OpenCode's own
    /// server for OpenCode.
    pub hosts: Vec<Runtime>,
    pub sessions: KiroSessions,
    /// Helpers: who asked whom for what (hover-agents::orch).
    pub orch: Arc<Orch>,
    pub quotas: Poller,
    /// Kiro's credits by day (Settings → Kiro), made off the UI thread; Settings only reads the latest.
    pub credits: Arc<Credits>,
    /// The tests' stand-in for every tool's runner (None: each host's own).
    run: Option<RunTask>,
    unseen: Mutex<Unseen>,
    /// KiroPage.Watching: an office is in view (the open notch, or the app window
    /// not minimised), so an end is seen as it happens and not announced.
    watching: AtomicBool,
    hooks: Arc<Mutex<Hooks>>,
}

/// Hover has no background service now. One a user installed with an earlier version is a Windows scheduled task
/// ("Hover Service") or a systemd user unit (hover.service) that starts `hoverai --service` at log-on, and that flag now
/// only opens the app. The first start after the update undoes exactly what the installer made, with no window and no
/// question. It runs a program, so it goes on a thread of its own. The marker file is written once the service is gone
/// (not before), so a removal that failed is tried again at the next start. Saved tasks and the rest of its data stay on disk.
fn remove_old_service() {
    let marker = hover_core::paths::support().join("old-service-removed");
    if marker.exists() { return; }
    std::thread::Builder::new().name("old-service".into()).spawn(move || {
        let (said, gone) = old_service_removal();
        hover_core::log::line(&format!("old background service: {said}"));
        if gone { let _ = std::fs::write(&marker, "The background service of earlier versions was looked for and is not there.\n"); }
    }).ok();
}

/// What was done, and whether the service is gone now.
#[cfg(windows)]
fn old_service_removal() -> (String, bool) {
    use std::os::windows::process::CommandExt;
    const TASK: &str = "Hover Service";
    let schtasks = |args: &[&str]| std::process::Command::new("schtasks").args(args).stdin(std::process::Stdio::null()).creation_flags(0x0800_0000 /* CREATE_NO_WINDOW */).output();
    match schtasks(&["/Query", "/TN", TASK]) {
        Err(e) => (format!("schtasks didn’t start ({e}), so it was not looked for"), false),
        Ok(o) if !o.status.success() => ("not installed".into(), true),
        Ok(_) => {
            let _ = schtasks(&["/End", "/TN", TASK]);
            match schtasks(&["/Delete", "/TN", TASK, "/F"]) {
                Ok(o) if o.status.success() => ("removed the scheduled task “Hover Service”".into(), true),
                Ok(o) => (format!("couldn’t remove the scheduled task: {}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("failed").trim()), false),
                Err(e) => (format!("couldn’t remove the scheduled task: {e}"), false),
            }
        }
    }
}

#[cfg(target_os = "linux")]
fn old_service_removal() -> (String, bool) {
    const UNIT: &str = "hover.service";
    let unit = hover_agents::proc::home().join(".config/systemd/user").join(UNIT);
    if !unit.is_file() { return ("not installed".into(), true); }
    let systemctl = |args: &[&str]| std::process::Command::new("systemctl").arg("--user").args(args).stdin(std::process::Stdio::null()).output();
    let _ = systemctl(&["disable", "--now", UNIT]);
    match std::fs::remove_file(&unit) {
        Ok(()) => { let _ = systemctl(&["daemon-reload"]); (format!("removed the systemd user unit {UNIT}"), true) }
        Err(e) => (format!("couldn’t remove {}: {e}", unit.display()), false),
    }
}

/// The Mac app is not this one, and no other system ever had the service.
#[cfg(not(any(windows, target_os = "linux")))]
fn old_service_removal() -> (String, bool) { ("nothing to remove on this system".into(), true) }

impl Hover {
    /// The real thing: settings.json, the key and history in the data folder, one
    /// runtime per tool.
    pub fn start() -> Arc<Hover> {
        hover_core::paths::drop_planner(hover_core::paths::support());
        let settings = Settings::load(hover_core::paths::settings_file());
        let history = hover_core::crypto::global().map(|c| Arc::new(AgentHistory::new(hover_core::paths::agents(), c)));
        if history.is_none() { hover_core::log::line("no key this run: sessions aren't kept"); }
        let hosts: Vec<Runtime> = AgentTool::ALL.iter().map(|&t| {
            let s = settings.clone();
            Runtime::new(t, move || s.agent_options(t))
        }).collect();
        let me = Hover::with(settings, history, hosts, None, None);
        hover_agents::discord::start(me.settings.clone(), me.sessions.clone());
        remove_old_service();
        // Results of helpers that finished while no lead was there to hear them.
        let o = me.orch.clone();
        std::thread::Builder::new().name("orch-deliver".into()).spawn(move || o.deliver_pending()).ok();
        me
    }

    /// With the parts given (tests hand in stand-in hosts and a reader).
    pub fn with(settings: Arc<Settings>, history: Option<Arc<AgentHistory>>, hosts: Vec<Runtime>, run: Option<RunTask>,
                reader: Option<Reader>) -> Arc<Hover> {
        let hooks: Arc<Mutex<Hooks>> = Default::default();
        for h in &hosts {
            // What the tool offers (models, efforts) fills in its settings page.
            let s = settings.clone();
            h.on_options_seen(move |tool, offers| s.set_agent_offers(tool, offers));
        }
        // Computer use, the sandbox and the agent browser (Settings → Integrations) are read at
        // every tool start.
        let st = settings.clone();
        hover_agents::agents::set_toggles(move || hover_agents::agents::Toggles { computer_use: st.computer_use(), sandbox: st.sandbox(), agent_browser: st.agent_browser(), folder: st.kiro_folder() });
        let runners: Vec<(AgentTool, Runtime)> = hosts.iter().map(|h| (h.tool(), h.clone())).collect();
        let run2 = run.clone();
        let sessions = KiroSessions::new(move |tool| match &run2 {
            Some(r) => r.clone(),
            None => runners.iter().find(|(t, _)| *t == tool).expect("a host per tool").1.runner(),
        }, history.clone());
        // Chats keep the project folder before and after each turn, so they can go back to it;
        // without git there are none, and nothing else changes.
        if let Some(c) = hover_agents::checkpoint::Checkpoints::new(hover_core::paths::support().join("checkpoints")) { sessions.set_checkpoints(Arc::new(c)); }
        for h in &hosts {
            // A question goes to the session whose conversation it is, where the notch
            // and the office show it. One nobody holds is turned down.
            let (ks, tool) = (sessions.clone(), h.tool());
            h.set_asking(Arc::new(move |sid, ask, ct, reply| ks.ask(tool, sid, ask, ct, reply)));
            let ks = sessions.clone();
            h.set_questioning(Arc::new(move |sid, ask, ct, reply| ks.ask_question(tool, sid, ask, ct, reply)));
        }
        let fire = |hooks: &Arc<Mutex<Hooks>>, pick: fn(&Hooks) -> &Vec<Hook>| {
            let list: Vec<Hook> = pick(&hooks.lock().unwrap()).clone();
            for f in list { f(); }
        };
        let qh = hooks.clone();
        let is_on = { let s = settings.clone(); Arc::new(move |id: &str| s.has_notch_item(id)) };
        let changed: Arc<dyn Fn() + Send + Sync> = Arc::new(move || fire(&qh, |h| &h.quotas));
        // Kiro's credits by day are made again when the history changes and after each Kiro reading, and ask Settings to redraw as the quota poll does.
        let credits = Arc::new(Credits::new(history.clone(), hover_quota::daily::path(), changed.clone()));
        if let Some(h) = &history {
            let w = Arc::downgrade(&credits);
            h.on_changed(move || if let Some(c) = w.upgrade() { c.poke(); });
        }
        let w = Arc::downgrade(&credits);
        let quotas = match reader {
            Some(r) => Poller::new(r, is_on, changed),
            None => Poller::system_with(is_on, changed, Arc::new(move |u| if let Some(c) = w.upgrade() { c.on_usage(&u); })),
        };
        // Helpers: kept in the data folder, sealed, when there is a key this run; in memory for this run when not.
        let doc = hover_core::crypto::global().filter(|_| run.is_none()).map(|c| hover_core::store::Sealed::in_dir(&hover_core::paths::support().join("orch"), "runs", c));
        let env = Arc::new(SystemEnv::new(settings.clone()));
        let orch = Orch::new(sessions.clone(), env, doc);
        orch.install();
        let me = Arc::new(Hover { settings, history, hosts, sessions, orch, quotas, credits, run, unseen: Default::default(), watching: AtomicBool::new(false), hooks });
        let sh = me.hooks.clone();
        me.sessions.on_changed(move || fire(&sh, |h| &h.sessions));
        let weak = Arc::downgrade(&me);
        me.sessions.on_ended(move |s, r| { if let Some(me) = weak.upgrade() { me.ended(s, r); } });
        me
    }

    pub fn on_notify(&self, f: impl Fn(&str, &str) + Send + Sync + 'static) { self.hooks.lock().unwrap().notify = Some(Arc::new(f)); }
    pub fn on_quotas(&self, f: impl Fn() + Send + Sync + 'static) { self.hooks.lock().unwrap().quotas.push(Arc::new(f)); }
    pub fn on_sessions(&self, f: impl Fn() + Send + Sync + 'static) { self.hooks.lock().unwrap().sessions.push(Arc::new(f)); }

    pub fn set_watching(&self, on: bool) {
        self.watching.store(on, Ordering::SeqCst);
        if on { self.seen(); }
    }

    /// Kiro tasks that ended while no office was in view, and their tool when all
    /// were the same one (OwlApp.KiroUnseen, KiroUnseenTool).
    pub fn unseen(&self) -> (usize, Option<&'static str>) { let u = self.unseen.lock().unwrap(); (u.count, u.tool) }

    /// The latest of those ends (OwlApp.KiroUnseenLast).
    pub fn unseen_last(&self) -> Option<UnseenEnd> { self.unseen.lock().unwrap().last.clone() }

    /// An office came into view: the ends it announced have been seen.
    pub fn seen(&self) {
        {
            let mut u = self.unseen.lock().unwrap();
            if u.count == 0 { return; }
            u.count = 0;
            u.last = None;
        }
        self.sessions.raise_changed();
    }

    /// A task can take minutes; the notch has usually been folded away by the time it
    /// ends, so the end is announced, unless an office is in view.
    fn ended(&self, s: &KiroSession, r: &KiroResult) {
        if self.watching.load(Ordering::SeqCst) { return; }
        let who = s.tool.name();
        {
            let mut u = self.unseen.lock().unwrap();
            u.count += 1;
            u.tool = if u.count == 1 || u.tool == Some(who) { Some(who) } else { None };
            let took = s.current().map_or(0.0, |t| t.ended_at.unwrap_or_else(|| self.sessions.now()).secs_since(&t.started_at));
            u.last = Some(UnseenEnd { tool: s.tool, title: s.title(), state: r.state, took_secs: took });
        }
        let title = match r.state {
            KiroState::Completed => format!("{who} is done"),
            KiroState::Cancelled => format!("{who} stopped"),
            _ => format!("{who} couldn't finish"),
        } + ": " + &s.title();
        let body = text::first_line(&text::plain(&r.text));
        let notify = self.hooks.lock().unwrap().notify.clone();
        if let Some(n) = notify { n(&title, &body); }
    }

    /// The tool's runner as the sessions use it, for voice's routing turn (which sets
    /// its own access, "none").
    pub fn runner(&self, tool: AgentTool) -> Option<RunTask> {
        self.run.clone().or_else(|| self.hosts.iter().find(|h| h.tool() == tool).map(Runtime::runner))
    }

    /// RefreshQuotas: on the 30 s tick, or forced from Settings.
    pub fn refresh_quotas(&self, force: bool) { self.quotas.refresh(force); }

    /// The notch's text for the newest task at work, and how many more are.
    pub fn working_text(&self) -> Option<String> {
        let busy: Vec<KiroSession> = self.sessions.all().into_iter().filter(KiroSession::busy).collect();
        let last = busy.last()?;
        Some(format!("{} · {}{}", last.tool.name(), text::status(last), if busy.len() > 1 { format!(" · {}", busy.len()) } else { String::new() }))
    }

    /// Called as the app quits. A running task is stopped rather than left working
    /// with nobody watching, except a Kiro Web one, which goes on in the cloud and is
    /// followed on at the next start; then the tools, the history and the settings.
    pub fn shutdown(&self) {
        self.sessions.stop_all();
        for h in &self.hosts { h.shutdown("Hover quit"); }
        // The agent browser's socket and its relay (a Mac's).
        hover_agents::browser::stop();
        self.orch.flush();
        if let Some(h) = &self.history { h.flush(); }
        self.settings.flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hover_agents::session::RunArgs;
    use std::time::Duration;

    fn hover(answer: &'static str, state: KiroState) -> (Arc<Hover>, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("hover-app-{}-{}", std::process::id(), hover_core::guid_n()));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = Settings::load(dir.join("settings.json"));
        let run: RunTask = Arc::new(move |a: RunArgs| {
            std::thread::sleep(Duration::from_millis(50));
            let _ = a;
            KiroResult::new(state, answer)
        });
        (Hover::with(settings, None, vec![], Some(run), Some(Arc::new(|id: &str| hover_quota::Reading { used: Some(10.0), detail: id.into() }))), dir)
    }

    fn wait(f: impl Fn() -> bool) { for _ in 0..200 { if f() { return; } std::thread::sleep(Duration::from_millis(10)); } panic!("timed out"); }

    /// OwlApp.Start's Ended handler: the title names the tool and the outcome, the text
    /// is the answer's first plain line, and the ends are counted until seen.
    #[test]
    fn an_end_nobody_saw_is_announced_and_counted() {
        let (h, dir) = hover("## Fixed **it**\n\nMore words.", KiroState::Completed);
        let said: Arc<Mutex<Vec<(String, String)>>> = Default::default();
        let s2 = said.clone();
        h.on_notify(move |t, b| s2.lock().unwrap().push((t.into(), b.into())));
        let folder = dir.to_string_lossy().into_owned();
        h.sessions.start(AgentTool::Kiro, &folder, "Tidy the imports", vec![]).unwrap();
        assert_eq!(h.working_text().as_deref(), Some("Kiro · Waking up…"));
        wait(|| said.lock().unwrap().len() == 1);
        assert_eq!(said.lock().unwrap()[0], ("Kiro is done: Tidy the imports".into(), "Fixed it".into()));
        assert_eq!(h.unseen(), (1, Some("Kiro")));
        assert_eq!(h.working_text(), None);
        h.sessions.start(AgentTool::Codex, &folder, "Second", vec![]).unwrap();
        wait(|| said.lock().unwrap().len() == 2);
        // Two different tools: no one name.
        assert_eq!(h.unseen(), (2, None));
        h.set_watching(true);
        assert_eq!(h.unseen().0, 0);
        // Watched: seen as it happens, not announced.
        h.sessions.start(AgentTool::Kiro, &folder, "Third", vec![]).unwrap();
        wait(|| h.sessions.running() == 0);
        std::thread::sleep(Duration::from_millis(50));
        assert_eq!((h.unseen().0, said.lock().unwrap().len()), (0, 2));
        h.shutdown();
    }

    #[test]
    fn failures_and_stops_say_so() {
        let (h, dir) = hover("", KiroState::Failed);
        let said: Arc<Mutex<Vec<String>>> = Default::default();
        let s2 = said.clone();
        h.on_notify(move |t, _| s2.lock().unwrap().push(t.into()));
        h.sessions.start(AgentTool::Cursor, &dir.to_string_lossy(), "Look", vec![]).unwrap();
        wait(|| !said.lock().unwrap().is_empty());
        assert_eq!(said.lock().unwrap()[0], "Cursor couldn't finish: Look");
    }

    #[test]
    fn quotas_read_what_is_switched_on() {
        let (h, _) = hover("", KiroState::Completed);
        let (tx, rx) = std::sync::mpsc::channel();
        let tx = Mutex::new(tx);
        h.on_quotas(move || { let _ = tx.lock().unwrap().send(()); });
        h.settings.set_notch_item("codex", true);
        h.refresh_quotas(true);
        rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(h.quotas.reading("codex").unwrap().detail, "codex");
        assert!(h.quotas.reading("claude").is_none());
    }
}

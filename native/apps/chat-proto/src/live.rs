//! The drawer on a real session: an ACP agent (kiro-cli, or port/tools/FakeAcp in its
//! place) run by hover-agents, and the state message KiroPage would send the page,
//! built by hover-agents and read by hover-chat as the page reads the C# one.

use hover_agents::acp::AcpHost;
use hover_agents::session::KiroSessions;
use hover_core::model::{AgentOptions, AgentTool};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

pub struct Live {
    pub sessions: KiroSessions,
    pub host: AcpHost,
    pub folder: String,
    dirty: Arc<AtomicBool>,
}

impl Live {
    /// `exe acp` as the Kiro tool, working in folder.
    pub fn new(exe: PathBuf, folder: String) -> Live {
        let host = AcpHost::with_connect(AgentTool::Kiro, AgentOptions::default, move || hover_agents::proc::launch(&exe, &["acp"], &[]).map(Some));
        let h = host.clone();
        let sessions = KiroSessions::new(move |_| h.runner(), None);
        let dirty = Arc::new(AtomicBool::new(true));
        let d = dirty.clone();
        sessions.on_changed(move || d.store(true, Ordering::SeqCst));
        Live { sessions, host, folder, dirty }
    }

    /// A new task, or a reply in the one shown; its id, or a toast's text.
    pub fn send(&self, current: Option<i32>, text: &str, images: Vec<String>) -> Result<i32, String> {
        match current {
            None => self.sessions.start(AgentTool::Kiro, &self.folder, text, images).map(|s| s.id).ok_or_else(|| "Couldn’t start that task.".into()),
            Some(id) if self.sessions.reply(id, text, images) => Ok(id),
            Some(_) => Err(format!("{} tasks are running. Reply when one is done.", hover_agents::session::MAX_RUNNING)),
        }
    }

    /// Something changed since the last call (KiroPage's push timer reads it).
    pub fn take_dirty(&self) -> bool { self.dirty.swap(false, Ordering::SeqCst) }

    /// The session as the state message carries it (KiroPage.State), read back as JSON.
    pub fn state_of(&self, id: i32) -> Option<serde_json::Value> {
        let s = self.sessions.get(id)?;
        let files = |s: &hover_agents::session::KiroSession| hover_agents::usable_folder(Some(&s.folder)).then(|| format!("f{}.hover", &s.key[..12]));
        serde_json::from_str(&hover_agents::state::state(&s, &files).compact()).ok()
    }

}

impl Drop for Live {
    fn drop(&mut self) {
        self.sessions.stop_all();
        self.host.shutdown("Hover quit");
    }
}

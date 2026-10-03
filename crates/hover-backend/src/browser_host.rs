//! BrowserTool.ToHost, Resolve and Complete: Hover's browser lives in the Mac app (a
//! WKWebView per session). hover-agents answers the agent's MCP calls and hands each to a
//! `browser::Host`; this one sends it to the host as `{type:"browser", call, id, op, args}`
//! (id is the office's session id) and completes on the host's `{type:"browserResult",
//! call, ok, text, image, mime}`.

use crate::wire::Out;
use hover_agents::browser::{Host, Reply};
use hover_agents::session::KiroSessions;
use hover_core::json::Json;
use hover_core::model::AgentTool;
use std::collections::HashMap;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub struct BrowserHost {
    out: Arc<Out>,
    sessions: KiroSessions,
    calls: AtomicI64,
    pending: Mutex<HashMap<i64, Sender<Reply>>>,
}

impl BrowserHost {
    pub fn new(out: Arc<Out>, sessions: KiroSessions) -> Arc<BrowserHost> {
        Arc::new(BrowserHost { out, sessions, calls: AtomicI64::new(0), pending: Mutex::new(HashMap::new()) })
    }

    /// The office's id of the session a tool's tag names: its key, or for OpenCode (one
    /// server for all its sessions) the one at work.
    fn resolve(&self, tag: &str) -> Option<i32> {
        let all = self.sessions.all();
        if tag == "opencode" {
            return all.iter().filter(|s| s.tool == AgentTool::OpenCode && s.busy()).max_by_key(|s| s.current().map(|t| t.started_at.ticks)).map(|s| s.id);
        }
        all.iter().find(|s| s.key == tag).map(|s| s.id)
    }

    /// The host's answer to a call (BrowserTool.Complete). Unknown calls are ignored.
    pub fn complete(&self, m: &Json) {
        let Some(Json::Num(n)) = m.get("call") else { return };
        let Ok(call) = n.parse::<i64>() else { return };
        let Some(tx) = self.pending.lock().unwrap().remove(&call) else { return };
        let text = |k: &str| m.get(k).and_then(Json::as_str).map(str::to_owned);
        let _ = tx.send(Reply { ok: matches!(m.get("ok"), Some(Json::Bool(true))), text: text("text").unwrap_or_default(), image: text("image"), mime: text("mime") });
    }

    /// Hover is closing: the calls waiting on the host fail.
    pub fn stop(&self) { self.pending.lock().unwrap().clear(); }
}

/// What `browser::set_host` takes: the backend keeps its own handle to complete calls.
pub struct Handle(pub Arc<BrowserHost>);

impl Host for Handle {
    fn has_session(&self, tag: &str) -> bool { self.0.has_session(tag) }
    fn call(&self, tag: &str, op: &str, args: &Json) -> Reply { self.0.call(tag, op, args) }
}

impl Host for BrowserHost {
    fn has_session(&self, tag: &str) -> bool { self.resolve(tag).is_some() }

    fn call(&self, tag: &str, op: &str, args: &Json) -> Reply {
        let Some(id) = self.resolve(tag) else { return Reply::text(false, "Hover's browser isn't open for this session right now.") };
        let call = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let (tx, rx) = channel();
        self.pending.lock().unwrap().insert(call, tx);
        self.out.send(&Json::obj(vec![("type", Json::str("browser")), ("call", Json::int(call)), ("id", Json::int(id as i64)), ("op", Json::str(op)), ("args", args.clone())]));
        // hover-agents gives up after 90 s (40 for a wait); a little later so its own
        // message is the one that is read.
        let r = rx.recv_timeout(Duration::from_secs(if op == "wait" { 45 } else { 95 }));
        self.pending.lock().unwrap().remove(&call);
        match r {
            Ok(reply) => reply,
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Reply::text(false, "The browser didn’t answer in time."),
            Err(_) => Reply::text(false, "Hover is closing."),
        }
    }
}

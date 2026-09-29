//! IAgentRuntime (KiroRunner.cs): one agent tool as Hover runs it, an ACP server
//! (Kiro, Codex, Cursor) or OpenCode's own server. Shared by all of that tool's
//! sessions; the sessions and the views only ever see this.

use crate::acp::{AcpHost, Asking};
use crate::opencode::{OpenCodeHost, Questioning};
use crate::session::RunTask;
use hover_core::model::{AcpOption, AgentTool};

/// AgentCaps: what a tool can really do, so the office only shows what works.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentCaps { pub questions: bool, pub read_only: bool, pub resume: bool, pub effort_label: &'static str }

/// The caps a tool has, without its runtime (AcpHost.Caps, OpenCodeHost.Caps).
pub fn caps(t: AgentTool) -> AgentCaps {
    match t {
        // OpenCode's variants are the model's own, so they aren't called Effort.
        AgentTool::OpenCode => AgentCaps { questions: true, read_only: true, resume: true, effort_label: "Variant" },
        // ACP has no questions of its own; effort is a session option where offered.
        _ => AgentCaps { questions: false, read_only: crate::agents::read_only_works(t), resume: true, effort_label: "Effort" },
    }
}

#[derive(Clone)]
pub enum Runtime { Acp(AcpHost), OpenCode(OpenCodeHost) }

impl Runtime {
    /// The tool as Agents finds and starts it: OpenCode's own server, or an ACP one.
    pub fn new(t: AgentTool, options: impl Fn() -> hover_core::model::AgentOptions + Send + Sync + 'static) -> Runtime {
        if t == AgentTool::OpenCode { Runtime::OpenCode(OpenCodeHost::new(options)) } else { Runtime::Acp(AcpHost::new(t, options)) }
    }

    pub fn tool(&self) -> AgentTool { match self { Runtime::Acp(h) => h.tool(), Runtime::OpenCode(h) => h.tool() } }
    pub fn caps(&self) -> AgentCaps { caps(self.tool()) }
    /// The tool's process is up.
    pub fn alive(&self) -> bool { match self { Runtime::Acp(h) => h.alive(), Runtime::OpenCode(h) => h.alive() } }
    /// The models, efforts and modes it offers, whenever they are read. Off the UI thread.
    pub fn on_options_seen(&self, f: impl Fn(AgentTool, &[AcpOption]) + Send + Sync + 'static) {
        match self { Runtime::Acp(h) => h.on_options_seen(f), Runtime::OpenCode(h) => h.on_options_seen(f) }
    }
    pub fn set_asking(&self, f: Asking) { match self { Runtime::Acp(h) => h.set_asking(f), Runtime::OpenCode(h) => h.set_asking(f) } }
    /// ACP agents don't ask questions; only OpenCode's are passed on.
    pub fn set_questioning(&self, f: Questioning) { if let Runtime::OpenCode(h) = self { h.set_questioning(f) } }
    /// End the tool's process now. Runs still going fail; the next one starts it again.
    pub fn shutdown(&self, why: &str) { match self { Runtime::Acp(h) => h.shutdown(why), Runtime::OpenCode(h) => h.shutdown(why) } }
    pub fn runner(&self) -> RunTask { match self { Runtime::Acp(h) => h.runner(), Runtime::OpenCode(h) => h.runner() } }
}

impl From<AcpHost> for Runtime { fn from(h: AcpHost) -> Runtime { Runtime::Acp(h) } }
impl From<OpenCodeHost> for Runtime { fn from(h: OpenCodeHost) -> Runtime { Runtime::OpenCode(h) } }

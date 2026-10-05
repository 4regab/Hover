//! IAgentRuntime (KiroRunner.cs): one agent tool as Hover runs it, an ACP server
//! (Kiro, Codex, Cursor), OpenCode's own server, or Claude Code in its SDK mode. Shared
//! by all of that tool's sessions; the sessions and the views only ever see this.

use crate::acp::{AcpHost, Asking};
use crate::claude::ClaudeHost;
use crate::opencode::{OpenCodeHost, Questioning};
use crate::session::{RunArgs, RunTask};
use hover_core::model::{AcpOption, AgentTool};

/// AgentCaps: what a tool can really do, so the office only shows what works.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AgentCaps { pub questions: bool, pub read_only: bool, pub resume: bool, pub effort_label: &'static str }

/// The caps a tool has, without its runtime (AcpHost.Caps, OpenCodeHost.Caps).
pub fn caps(t: AgentTool) -> AgentCaps {
    match t {
        // OpenCode's variants are the model's own, so they aren't called Effort.
        AgentTool::OpenCode => AgentCaps { questions: true, read_only: true, resume: true, effort_label: "Variant" },
        // AskUserQuestion; its efforts are each model's, but they are efforts.
        AgentTool::Claude => AgentCaps { questions: true, read_only: true, resume: true, effort_label: "Effort" },
        // ACP has no questions of its own; effort is a session option where offered.
        _ => AgentCaps { questions: false, read_only: crate::agents::read_only_works(t), resume: true, effort_label: "Effort" },
    }
}

/// Whether a tool's efforts belong to each model (its offered models carry their
/// levels) rather than being one list for all of them.
pub fn per_model_effort(t: AgentTool) -> bool { matches!(t, AgentTool::OpenCode | AgentTool::Claude) }

/// The Hover session (its key) a run is for: the tag Hover's browser server is made for,
/// so the agent's browser calls reach that session's page. A run that is no session's (voice's
/// routing turn) has none, and the browser is not offered to it; OpenCode's one server passes
/// its own tag.
pub(crate) fn tag_of(a: &RunArgs) -> Option<String> { a.tag.clone() }

#[derive(Clone)]
pub enum Runtime { Acp(AcpHost), OpenCode(OpenCodeHost), Claude(ClaudeHost) }

impl Runtime {
    /// The tool as Agents finds and starts it: OpenCode's own server, Claude Code, or an ACP one.
    pub fn new(t: AgentTool, options: impl Fn() -> hover_core::model::AgentOptions + Send + Sync + 'static) -> Runtime {
        match t {
            AgentTool::OpenCode => Runtime::OpenCode(OpenCodeHost::new(options)),
            AgentTool::Claude => Runtime::Claude(ClaudeHost::new(options)),
            _ => Runtime::Acp(AcpHost::new(t, options)),
        }
    }

    pub fn tool(&self) -> AgentTool { match self { Runtime::Acp(h) => h.tool(), Runtime::OpenCode(h) => h.tool(), Runtime::Claude(h) => h.tool() } }
    pub fn caps(&self) -> AgentCaps { caps(self.tool()) }
    /// The tool's process is up.
    pub fn alive(&self) -> bool { match self { Runtime::Acp(h) => h.alive(), Runtime::OpenCode(h) => h.alive(), Runtime::Claude(h) => h.alive() } }
    /// The models, efforts and modes it offers, whenever they are read. Off the UI thread.
    pub fn on_options_seen(&self, f: impl Fn(AgentTool, &[AcpOption]) + Send + Sync + 'static) {
        match self { Runtime::Acp(h) => h.on_options_seen(f), Runtime::OpenCode(h) => h.on_options_seen(f), Runtime::Claude(h) => h.on_options_seen(f) }
    }
    pub fn set_asking(&self, f: Asking) { match self { Runtime::Acp(h) => h.set_asking(f), Runtime::OpenCode(h) => h.set_asking(f), Runtime::Claude(h) => h.set_asking(f) } }
    /// ACP agents don't ask questions; OpenCode's and Claude Code's are passed on.
    pub fn set_questioning(&self, f: Questioning) {
        match self { Runtime::OpenCode(h) => h.set_questioning(f), Runtime::Claude(h) => h.set_questioning(f), Runtime::Acp(_) => {} }
    }
    /// End the tool's process now. Runs still going fail; the next one starts it again.
    pub fn shutdown(&self, why: &str) { match self { Runtime::Acp(h) => h.shutdown(why), Runtime::OpenCode(h) => h.shutdown(why), Runtime::Claude(h) => h.shutdown(why) } }
    pub fn runner(&self) -> RunTask { match self { Runtime::Acp(h) => h.runner(), Runtime::OpenCode(h) => h.runner(), Runtime::Claude(h) => h.runner() } }
    /// The GitHub repos a Kiro Web session can be given (AcpHost::repos). Blocks.
    pub fn repos(&self) -> Result<Vec<String>, String> { match self { Runtime::Acp(h) => h.repos(), _ => Err("Only Kiro runs Kiro Web sessions.".into()) } }
    /// The user's Kiro Web sessions (AcpHost::cloud_sessions). Blocks.
    pub fn cloud_sessions(&self) -> Result<Vec<crate::acp::CloudSession>, String> { match self { Runtime::Acp(h) => h.cloud_sessions(), _ => Err("Only Kiro runs Kiro Web sessions.".into()) } }
    /// A Kiro Web session's conversation (AcpHost::cloud_transcript). Blocks.
    pub fn cloud_transcript(&self, id: &str, folder: &str) -> Result<Vec<crate::acp::CloudTurn>, String> {
        match self { Runtime::Acp(h) => h.cloud_transcript(id, folder), _ => Err("Only Kiro runs Kiro Web sessions.".into()) }
    }
}

impl From<AcpHost> for Runtime { fn from(h: AcpHost) -> Runtime { Runtime::Acp(h) } }
impl From<OpenCodeHost> for Runtime { fn from(h: OpenCodeHost) -> Runtime { Runtime::OpenCode(h) } }
impl From<ClaudeHost> for Runtime { fn from(h: ClaudeHost) -> Runtime { Runtime::Claude(h) } }

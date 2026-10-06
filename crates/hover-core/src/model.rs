//! The records settings.json and the history hold (Services/KiroRunner.cs,
//! Services/Agents.cs, Services/AcpHost.cs, Core/Palette.cs, Core/Settings.cs), with
//! their JSON in declaration order, as the serializer writes records.

use crate::json::{Json, JsonError, Result};

/// Services.AgentTool. New tools go at the end: the names are saved in settings and
/// history. `Custom` stands for every agent the user added (hover-agents::custom); which one is
/// in the session's `ext.provider`. `ALL` lists the five Hover ships, which is what every
/// per-tool list and page goes through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum AgentTool { Kiro, Codex, Cursor, OpenCode, Claude, Custom }

impl AgentTool {
    pub const ALL: [AgentTool; 5] = [AgentTool::Kiro, AgentTool::Codex, AgentTool::Cursor, AgentTool::OpenCode, AgentTool::Claude];
    const EVERY: [AgentTool; 6] = [AgentTool::Kiro, AgentTool::Codex, AgentTool::Cursor, AgentTool::OpenCode, AgentTool::Claude, AgentTool::Custom];
    // Claude Code is new in 3.x, so its saved name can be its product's (2.x never wrote one).
    const NAMES: [&'static str; 6] = ["Kiro", "Codex", "Cursor", "OpenCode", "Claude Code", "Custom"];

    /// Agents.Name: the enum's name.
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    /// Agents.Id: the name in lower case.
    pub fn id(self) -> &'static str { ["kiro", "codex", "cursor", "opencode", "claude", "custom"][self as usize] }
    /// Agents.Parse: the exact id, or none. Only the five Hover ships: a custom agent is named by its own id.
    pub fn parse(id: Option<&str>) -> Option<AgentTool> { Self::ALL.into_iter().find(|t| Some(t.id()) == id) }

    pub fn to_json(self) -> Json { Json::str(self.name()) }
    pub fn from_json(v: &Json) -> Result<AgentTool> {
        v.enum_of(&Self::NAMES)?.map(|i| Self::EVERY[i]).ok_or_else(|| JsonError("not an AgentTool".into()))
    }
}

/// Services.KiroState.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KiroState { Idle, Running, Completed, Failed, Cancelled }

impl KiroState {
    const ALL: [KiroState; 5] = [KiroState::Idle, KiroState::Running, KiroState::Completed, KiroState::Failed, KiroState::Cancelled];
    const NAMES: [&'static str; 5] = ["Idle", "Running", "Completed", "Failed", "Cancelled"];
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    pub fn to_json(self) -> Json { Json::str(self.name()) }
    pub fn from_json(v: &Json) -> Result<KiroState> {
        v.enum_of(&Self::NAMES)?.map(|i| Self::ALL[i]).ok_or_else(|| JsonError("not a KiroState".into()))
    }
    pub fn opt_from_json(v: &Json) -> Result<Option<KiroState>> { if v.is_null() { Ok(None) } else { Self::from_json(v).map(Some) } }
}

/// A string C# declares non-nullable but that null in the file would make null: read
/// as empty, since C# written by Hover never holds null there.
pub(crate) fn text(v: Option<&Json>) -> Result<String> {
    Ok(v.map(Json::opt_str).transpose()?.flatten().unwrap_or_default())
}

pub(crate) fn opt_text(v: Option<&Json>) -> Result<Option<String>> {
    Ok(v.map(Json::opt_str).transpose()?.flatten())
}

/// Services.KiroStep(Id, Kind, Title, Target, Status, Added, Removed, Diff, Output,
/// Exit, Ms). An edit carries the lines it adds and removes and a short preview ("- old",
/// "+ new", "  context"); a command the end of its output and its exit code; Ms is how
/// long it took. The last six are optional in C#, so older files read without them. Input
/// (the call's raw input as JSON, cut) and Log (the longer end of what it printed) are the
/// macOS build's, for the desk's panels; they are written only when a step has them, so a
/// file that never did stays as it was.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KiroStep {
    pub id: String, pub kind: String, pub title: String, pub target: Option<String>, pub status: String,
    pub added: i32, pub removed: i32, pub diff: Option<String>, pub output: Option<String>, pub exit: Option<i32>, pub ms: Option<f64>,
    pub input: Option<String>, pub log: Option<String>,
}

impl KiroStep {
    pub fn new(id: &str, kind: &str, title: &str, target: Option<String>, status: &str) -> KiroStep {
        KiroStep { id: id.into(), kind: kind.into(), title: title.into(), target, status: status.into(), ..Default::default() }
    }

    pub fn to_json(&self) -> Json {
        let mut props = vec![("Id", Json::str(&self.id)), ("Kind", Json::str(&self.kind)), ("Title", Json::str(&self.title)),
            ("Target", Json::opt_str_of(self.target.as_deref())), ("Status", Json::str(&self.status)),
            ("Added", Json::int(self.added as i64)), ("Removed", Json::int(self.removed as i64)),
            ("Diff", Json::opt_str_of(self.diff.as_deref())), ("Output", Json::opt_str_of(self.output.as_deref())),
            ("Exit", self.exit.map_or(Json::Null, |e| Json::int(e as i64))), ("Ms", self.ms.map_or(Json::Null, Json::double))];
        if let Some(i) = &self.input { props.push(("Input", Json::str(i))); }
        if let Some(l) = &self.log { props.push(("Log", Json::str(l))); }
        Json::obj(props)
    }
    pub fn from_json(v: &Json) -> Result<KiroStep> {
        v.props()?;
        let opt = |k: &str| -> Result<Option<&Json>> { Ok(v.get(k).filter(|x| !x.is_null())) };
        Ok(KiroStep { id: text(v.get("Id"))?, kind: text(v.get("Kind"))?, title: text(v.get("Title"))?, target: opt_text(v.get("Target"))?,
            status: text(v.get("Status"))?,
            added: v.get("Added").map(Json::i32).transpose()?.unwrap_or(0), removed: v.get("Removed").map(Json::i32).transpose()?.unwrap_or(0),
            diff: opt_text(v.get("Diff"))?, output: opt_text(v.get("Output"))?,
            exit: opt("Exit")?.map(Json::i32).transpose()?, ms: opt("Ms")?.map(Json::f64).transpose()?,
            input: opt_text(v.get("Input"))?, log: opt_text(v.get("Log"))? })
    }
}

/// Services.AgentApproval: when an agent with full access stops to ask. Autopilot
/// never asks (what 2.0 did, and the default). Risky asks for commands, deletes, moves,
/// the network and anything outside the folder. Always asks before anything but
/// reading and searching.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum AgentApproval { #[default] Autopilot, Risky, Always }

impl AgentApproval {
    pub const NAMES: [&'static str; 3] = ["Autopilot", "Risky", "Always"];
    const ALL: [AgentApproval; 3] = [AgentApproval::Autopilot, AgentApproval::Risky, AgentApproval::Always];
    pub fn from_index(i: usize) -> AgentApproval { Self::ALL[i] }
    pub fn name(self) -> &'static str { Self::NAMES[self as usize] }
    pub fn read(v: &Json) -> Result<AgentApproval> { Ok(v.enum_of(&Self::NAMES)?.map(Self::from_index).unwrap_or_default()) }
}

/// Services.AgentOptions. A property missing from the file takes the constructor's
/// default, as the serializer honours optional parameters.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentOptions {
    pub model: Option<String>,
    pub effort: Option<String>,
    pub read_only: bool,
    pub idle_minutes: i32,
    pub agent: Option<String>,
    /// Ignored: an MCP server that doesn't start no longer ends the turn
    /// (the chat says which one). Still read and written, so the settings.json files
    /// that have it read and save as before.
    pub require_mcp: bool,
    pub hide_steps: bool,
    /// When the agent stops to ask the user first; read only overrules it.
    pub approval: AgentApproval,
}

impl Default for AgentOptions {
    fn default() -> Self { AgentOptions { model: None, effort: None, read_only: false, idle_minutes: 5, agent: None, require_mcp: false, hide_steps: false, approval: AgentApproval::Autopilot } }
}

impl AgentOptions {
    pub const IDLE_CHOICES: [i32; 2] = [5, 15];

    /// AgentOptions.WithAccess: a session's own tool access, picked when it started
    /// (full, risky, always or read). Anything else keeps the tool's setting.
    pub fn with_access(&self, access: Option<&str>) -> AgentOptions {
        let mut o = self.clone();
        match access {
            Some("full") => { o.read_only = false; o.approval = AgentApproval::Autopilot; }
            Some("risky") => { o.read_only = false; o.approval = AgentApproval::Risky; }
            Some("always") => { o.read_only = false; o.approval = AgentApproval::Always; }
            Some("read") => o.read_only = true,
            // Voice's routing turn: read only, and Hover turns down every request.
            Some("none") => o.read_only = true,
            _ => {}
        }
        o
    }

    /// AgentOptions.AccessId: the id with_access takes for these options.
    pub fn access_id(&self, read_only_works: bool) -> &'static str {
        if self.read_only && read_only_works { return "read"; }
        match self.approval { AgentApproval::Risky => "risky", AgentApproval::Always => "always", AgentApproval::Autopilot => "full" }
    }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Model", Json::opt_str_of(self.model.as_deref())), ("Effort", Json::opt_str_of(self.effort.as_deref())),
            ("ReadOnly", Json::Bool(self.read_only)), ("IdleMinutes", Json::int(self.idle_minutes as i64)),
            ("Agent", Json::opt_str_of(self.agent.as_deref())), ("RequireMcp", Json::Bool(self.require_mcp)), ("HideSteps", Json::Bool(self.hide_steps)),
            ("Approval", Json::str(self.approval.name()))])
    }

    pub fn from_json(v: &Json) -> Result<AgentOptions> {
        v.props()?;
        let d = AgentOptions::default();
        Ok(AgentOptions {
            model: opt_text(v.get("Model"))?,
            effort: opt_text(v.get("Effort"))?,
            read_only: v.get("ReadOnly").map(Json::bool).transpose()?.unwrap_or(d.read_only),
            idle_minutes: v.get("IdleMinutes").map(Json::i32).transpose()?.unwrap_or(d.idle_minutes),
            agent: opt_text(v.get("Agent"))?,
            require_mcp: v.get("RequireMcp").map(Json::bool).transpose()?.unwrap_or(d.require_mcp),
            hide_steps: v.get("HideSteps").map(Json::bool).transpose()?.unwrap_or(d.hide_steps),
            approval: v.get("Approval").map(AgentApproval::read).transpose()?.unwrap_or(d.approval),
        })
    }
}

/// Services.AcpChoice(Value, Name, Levels). Levels are the efforts this choice takes,
/// where they differ by choice (OpenCode's variants belong to each model); none when
/// the tool lists effort on its own.
#[derive(Clone, Debug, PartialEq)]
pub struct AcpChoice { pub value: String, pub name: String, pub levels: Option<Vec<String>> }

impl AcpChoice {
    pub fn new(value: &str, name: &str) -> AcpChoice { AcpChoice { value: value.into(), name: name.into(), levels: None } }
}

/// Services.AcpOption(Id, Category, Current, Choices).
#[derive(Clone, Debug, PartialEq)]
pub struct AcpOption { pub id: String, pub category: Option<String>, pub current: Option<String>, pub choices: Vec<AcpChoice> }

impl AcpOption {
    pub fn has(&self, value: &str) -> bool { self.choices.iter().any(|c| c.value == value) }

    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Id", Json::str(&self.id)), ("Category", Json::opt_str_of(self.category.as_deref())),
            ("Current", Json::opt_str_of(self.current.as_deref())),
            ("Choices", Json::Arr(self.choices.iter().map(|c| Json::obj(vec![("Value", Json::str(&c.value)), ("Name", Json::str(&c.name)),
                ("Levels", c.levels.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(Json::str).collect())))])).collect()))])
    }

    pub fn from_json(v: &Json) -> Result<AcpOption> {
        v.props()?;
        let choice = |c: &Json| -> Result<AcpChoice> {
            c.props()?;
            Ok(AcpChoice { value: text(c.get("Value"))?, name: text(c.get("Name"))?,
                levels: c.get("Levels").map(|l| l.opt_list(|x| Ok(x.opt_str()?.unwrap_or_default()))).transpose()?.flatten() })
        };
        Ok(AcpOption {
            id: text(v.get("Id"))?,
            category: opt_text(v.get("Category"))?,
            current: opt_text(v.get("Current"))?,
            choices: v.get("Choices").map(|c| c.opt_list(choice)).transpose()?.flatten().unwrap_or_default(),
        })
    }
}

/// Open in editor (hover-agents::editor): which editor opens a desk's folder by default, and
/// the custom editor's program and arguments. `default` is an editor id ("vscode", "zed",
/// "cursor", "kiro" or "custom"); none means ask each time. `custom_args` is one argument per
/// line, each a literal with {folder}, {file}, {line} and {column} to fill in: it is never run
/// through a shell.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct EditorSettings { pub default: Option<String>, pub custom_exe: Option<String>, pub custom_args: Option<String> }

impl EditorSettings {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Default", Json::opt_str_of(self.default.as_deref())), ("CustomExe", Json::opt_str_of(self.custom_exe.as_deref())),
            ("CustomArgs", Json::opt_str_of(self.custom_args.as_deref()))])
    }

    pub fn from_json(v: &Json) -> Result<EditorSettings> {
        v.props()?;
        Ok(EditorSettings { default: opt_text(v.get("Default"))?, custom_exe: opt_text(v.get("CustomExe"))?, custom_args: opt_text(v.get("CustomArgs"))? })
    }
}

/// How far an agent may delegate (hover-agents::orch), set by the user: helpers in all for one task, helpers
/// working at once, and how deep helpers may delegate in turn (1: only the task itself may).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DelegationLimits { pub max_helpers: u32, pub max_parallel: u32, pub max_depth: u32 }

impl Default for DelegationLimits {
    fn default() -> Self { DelegationLimits { max_helpers: 6, max_parallel: 2, max_depth: 1 } }
}

impl DelegationLimits {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("MaxHelpers", Json::int(self.max_helpers as i64)), ("MaxParallel", Json::int(self.max_parallel as i64)), ("MaxDepth", Json::int(self.max_depth as i64))])
    }

    pub fn from_json(v: &Json) -> Result<DelegationLimits> {
        v.props()?;
        let d = DelegationLimits::default();
        let n = |k: &str, d: u32, hi: u32| -> Result<u32> { Ok(v.get(k).map(Json::i32).transpose()?.map_or(d, |x| x.clamp(0, hi as i32) as u32)) };
        Ok(DelegationLimits { max_helpers: n("MaxHelpers", d.max_helpers, 50)?, max_parallel: n("MaxParallel", d.max_parallel, 10)?, max_depth: n("MaxDepth", d.max_depth, 4)? })
    }
}

/// What runs by itself (hover-agents::sched, webhook, limit): continue a task when its provider's usage limit lifts (the default for a new
/// limit), the webhook listener's address (off unless set; this computer's own unless `public` is chosen), and whether a new task
/// works in the project folder itself (`use_folder`) instead of in a worktree of its own, which is the default (workspace.rs).
#[derive(Clone, Debug, PartialEq, Default)]
pub struct AutomationSettings { pub auto_resume: bool, pub webhook_addr: Option<String>, pub webhook_public: bool, pub use_folder: bool }

impl AutomationSettings {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("AutoResume", Json::Bool(self.auto_resume)), ("WebhookAddr", Json::opt_str_of(self.webhook_addr.as_deref())), ("WebhookPublic", Json::Bool(self.webhook_public)), ("UseFolder", Json::Bool(self.use_folder))])
    }
    pub fn from_json(v: &Json) -> Result<AutomationSettings> {
        v.props()?;
        Ok(AutomationSettings { auto_resume: v.get("AutoResume").map(Json::bool).transpose()?.unwrap_or(false), webhook_addr: opt_text(v.get("WebhookAddr"))?, webhook_public: v.get("WebhookPublic").map(Json::bool).transpose()?.unwrap_or(false), use_folder: v.get("UseFolder").map(Json::bool).transpose()?.unwrap_or(false) })
    }
}

/// Core.SavedTheme(Name, Dark, Colors): a VS Code theme's few colours, kept.
#[derive(Clone, Debug, PartialEq)]
pub struct SavedTheme { pub name: String, pub dark: bool, pub colors: Vec<(String, String)> }

impl SavedTheme {
    pub fn to_json(&self) -> Json {
        Json::obj(vec![("Name", Json::str(&self.name)), ("Dark", Json::Bool(self.dark)),
            ("Colors", Json::Obj(self.colors.iter().map(|(k, v)| (k.clone(), Json::str(v))).collect()))])
    }
    pub fn from_json(v: &Json) -> Result<SavedTheme> {
        v.props()?;
        Ok(SavedTheme {
            name: text(v.get("Name"))?,
            dark: v.get("Dark").map(Json::bool).transpose()?.unwrap_or(false),
            colors: v.get("Colors").map(|c| c.opt_map(|x| Ok(x.opt_str()?.unwrap_or_default()))).transpose()?.flatten().unwrap_or_default(),
        })
    }
}

/// Core.Appearance.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Appearance { #[default] System, Light, Dark }

/// Core.WorkspaceSize.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum WorkspaceSize { #[default] Default, Small, Large, ExtraLarge }

impl Appearance {
    pub const NAMES: [&'static str; 3] = ["System", "Light", "Dark"];
    const ALL: [Appearance; 3] = [Appearance::System, Appearance::Light, Appearance::Dark];
    pub fn from_index(i: usize) -> Appearance { Self::ALL[i] }
}

impl WorkspaceSize {
    pub const NAMES: [&'static str; 4] = ["Default", "Small", "Large", "ExtraLarge"];
    const ALL: [WorkspaceSize; 4] = [WorkspaceSize::Default, WorkspaceSize::Small, WorkspaceSize::Large, WorkspaceSize::ExtraLarge];
    pub fn from_index(i: usize) -> WorkspaceSize { Self::ALL[i] }
}

/// Core.NotchItem: what the resting notch can show, in its canonical order.
pub mod notch_item {
    pub const KIRO: &str = "kiro";
    pub const CODEX: &str = "codex";
    pub const CURSOR: &str = "cursor";
    pub const CLAUDE: &str = "claude";
    pub const ALL: [&str; 4] = [CLAUDE, KIRO, CODEX, CURSOR];

    pub fn title(id: &str) -> &str {
        match id { CLAUDE => "Claude Code quota", KIRO => "Kiro CLI quota", CODEX => "Codex quota", CURSOR => "Cursor quota", _ => id }
    }

    /// The name beside a quota on the notch.
    pub fn short(id: &str) -> &'static str {
        match id { CLAUDE => "Claude", KIRO => "Kiro", CODEX => "Codex", _ => "Cursor" }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Input and Log are the macOS build's: written only when a step has them, so a file
    /// that never did is byte for byte what 3.x wrote, and either kind of file reads.
    #[test]
    fn a_steps_input_and_log_are_written_only_when_it_has_them() {
        let plain = KiroStep::new("1", "execute", "Run", Some("ls".into()), "completed");
        let text = plain.to_json().compact();
        assert!(!text.contains("Input") && !text.contains("Log"), "{text}");
        assert_eq!(KiroStep::from_json(&crate::json::parse(&text).unwrap()).unwrap(), plain);
        let kept = KiroStep { input: Some("{\"command\":\"ls\"}".into()), log: Some("a\nb".into()), ..plain.clone() };
        let back = KiroStep::from_json(&crate::json::parse(&kept.to_json().compact()).unwrap()).unwrap();
        assert_eq!(back, kept);
        // 2.x's macOS build wrote them as null when a step had none.
        let old = KiroStep::from_json(&crate::json::parse(&text.replace("}", ",\"Input\":null,\"Log\":null}")).unwrap()).unwrap();
        assert_eq!(old, plain);
    }
}
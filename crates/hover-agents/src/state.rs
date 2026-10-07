//! KiroPage.Push and KiroPage.State: everything the office draws, as one message,
//! written as JsonSerializer writes the C# anonymous objects (compact, camelCase names
//! as declared, JavaScriptEncoder.Default escaping).

use crate::agents::AgentReady;
use crate::session::{KiroSession, MAX_RUNNING};
use crate::stream::{head_units, units, KiroPhase};
use hover_core::history::HistoryEntry;
use hover_core::json::Json;
use hover_core::model::{AcpOption, AgentTool, KiroState, KiroStep};
use hover_core::settings::Settings;
use hover_core::time::Stamp;

fn st(s: &str) -> Json { Json::str(s) }
fn opt(s: Option<&str>) -> Json { Json::opt_str_of(s) }

/// What Push reads besides the sessions.
pub struct Office<'a> {
    /// In the app window rather than the notch.
    pub window: bool,
    /// The session whose chat was open.
    pub open: Option<i32>,
    pub settings: &'a Settings,
    /// Settings.KiroFolder when it is usable, else none (the caller checks: a Windows
    /// fixture's folder isn't usable on Linux).
    pub folder: Option<String>,
    /// The whole history, only when it changed since the page last had it.
    pub history: Option<Vec<HistoryEntry>>,
    /// Agents.Known.
    pub ready: &'a dyn Fn(AgentTool) -> Option<AgentReady>,
    /// The session's files host (FilesHost), when it has one.
    pub files: &'a dyn Fn(&KiroSession) -> Option<String>,
}

pub fn push(o: &Office, sessions: &[KiroSession]) -> Json {
    let running = sessions.iter().filter(|s| s.busy()).count();
    Json::obj(vec![
        ("type", st("state")),
        ("window", Json::Bool(o.window)),
        ("canStart", Json::Bool(running < MAX_RUNNING)),
        ("maxRunning", Json::int(MAX_RUNNING as i64)),
        ("folder", opt(o.folder.as_deref())),
        ("tool", st(o.settings.agent_tool().id())),
        ("open", o.open.map_or(Json::Null, |i| Json::int(i as i64))),
        ("tools", Json::Arr(AgentTool::ALL.iter().map(|&t| tool(o, t)).collect())),
        ("sessions", Json::Arr(sessions.iter().map(|s| state_with(s, o.files, Some(tool_access(o.settings, s.tool)))).collect())),
        ("history", o.history.as_ref().map_or(Json::Null, |h| Json::Arr(h.iter().map(history_row).collect()))),
    ])
}

/// {type: "transcript", session}: one saved session, whole, for the chat to show.
pub fn transcript(s: &KiroSession, files: &dyn Fn(&KiroSession) -> Option<String>, settings: &Settings) -> Json {
    Json::obj(vec![("type", st("transcript")), ("session", state_with(s, files, Some(tool_access(settings, s.tool))))])
}

/// The tool's own setting as an access id (AgentOptions.AccessId).
pub fn tool_access(settings: &Settings, t: AgentTool) -> &'static str { settings.agent_options(t).access_id(crate::agents::read_only_works(t)) }

/// {type, text}: a toast, a picked folder (KiroPage.Say).
pub fn say(kind: &str, text: &str) -> Json { Json::obj(vec![("type", st(kind)), ("text", st(text))]) }

const EFFORT_IDS: [&str; 3] = ["effortLevel", "reasoning_effort", "effort"];

fn offer(settings: &Settings, t: AgentTool, category: &str, ids: &[&str]) -> Option<AcpOption> {
    let offers = settings.agent_offers(t);
    offers.iter().find(|x| x.category.as_deref() == Some(category)).or_else(|| offers.iter().find(|x| ids.contains(&x.id.as_str()))).cloned()
}

/// KiroPage.Models: what the tool offered, Kiro's own list before it has run; a
/// Default that sends none comes first unless the first is the tool's "auto". A model
/// with levels of its own (OpenCode's variants) carries them.
pub fn models(settings: &Settings, t: AgentTool) -> Vec<(String, String)> {
    models_with_levels(settings, t).into_iter().map(|(a, b, _)| (a, b)).collect()
}

pub fn models_with_levels(settings: &Settings, t: AgentTool) -> Vec<(String, String, Option<Vec<String>>)> {
    let mut list: Vec<(String, String, Option<Vec<String>>)> = match offer(settings, t, "model", &["model"]) {
        Some(o) => o.choices.iter().map(|c| (c.value.clone(), c.name.clone(), c.levels.clone())).collect(),
        None if t == AgentTool::Kiro => crate::KIRO_MODELS.iter().map(|(a, b)| (a.to_string(), b.to_string(), None)).collect(),
        None => vec![],
    };
    if list.is_empty() || !(list[0].0 == "auto" || list[0].0.starts_with("default")) { list.insert(0, (String::new(), "Default".into(), None)); }
    list
}

/// The efforts the tool's effort option lists, and the one it has now.
pub fn efforts(settings: &Settings, t: AgentTool) -> (Vec<String>, Option<String>) {
    let e = offer(settings, t, "thought_level", &EFFORT_IDS);
    (e.as_ref().map_or(vec![], |e| e.choices.iter().map(|c| c.value.clone()).collect()), e.and_then(|e| e.current))
}

/// effortsOf: the efforts for the picked model, its own levels (OpenCode's variants),
/// or the tool's list when models don't carry any. Auto picks the model per task, so it has none.
pub fn efforts_of(models: &[(String, String, Option<Vec<String>>)], model: &str, tool_efforts: &[String]) -> Vec<String> {
    if model.eq_ignore_ascii_case("auto") { return vec![]; }
    let m = models.iter().find(|m| m.0 == model);
    if m.is_some_and(|m| m.2.is_some()) || models.iter().any(|m| m.2.is_some()) { return m.and_then(|m| m.2.clone()).unwrap_or_default(); }
    tool_efforts.to_vec()
}

/// effortNow: the effort in force for a model that offers `levels`: the one picked if it is among
/// them, else High (or the first, where there is no High). None for a model that offers none.
pub fn effort_now(levels: &[String], picked: Option<&str>) -> Option<String> {
    if levels.is_empty() { return None; }
    picked.filter(|p| levels.iter().any(|l| l == p)).map(str::to_owned)
        .or_else(|| levels.iter().find(|l| l.as_str() == "high").cloned())
        .or_else(|| levels.first().cloned())
}

fn tool(o: &Office, t: AgentTool) -> Json {
    let opts = o.settings.agent_options(t);
    let known = (o.ready)(t);
    let models = models_with_levels(o.settings, t);
    let effort = offer(o.settings, t, "thought_level", &EFFORT_IDS);
    let caps = crate::runtime::caps(t);
    Json::obj(vec![
        ("id", st(t.id())),
        ("name", st(t.name())),
        // Unknown until checked; the picker offers it meanwhile.
        ("ready", Json::Bool(known.as_ref().is_none_or(AgentReady::ok))),
        ("hint", st(known.as_ref().map_or("", |k| k.hint.as_str()))),
        // The tool access a new task starts with, unless the box picks another.
        ("access", st(opts.access_id(crate::agents::read_only_works(t)))),
        ("readOnly", Json::Bool(crate::agents::read_only_works(t))),
        ("hideSteps", Json::Bool(opts.hide_steps)),
        // The composer's model and effort picks. A model with levels of its own
        // (OpenCode's variants) takes those instead of the tool's efforts.
        ("models", Json::Arr(models.iter().map(|(id, name, levels)| Json::obj(vec![("id", st(id)), ("name", st(name)),
            ("levels", levels.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(|x| st(x)).collect())))])).collect())),
        ("model", opt(opts.model.as_deref().or(models.first().map(|m| m.0.as_str())))),
        ("efforts", Json::Arr(effort.as_ref().map_or(vec![], |e| e.choices.iter().map(|c| st(&c.value)).collect()))),
        ("effort", opt(opts.effort.as_deref().or(effort.as_ref().and_then(|e| e.current.as_deref())))),
        ("effortLabel", st(caps.effort_label)),
        ("questions", Json::Bool(caps.questions)),
    ])
}

fn history_row(e: &HistoryEntry) -> Json {
    Json::obj(vec![
        ("key", st(&e.key)), ("tool", st(e.tool.id())), ("title", st(&e.title)), ("folder", st(&e.folder)),
        ("at", Json::int(ms(e.updated))), ("stage", st(stage(e.state, KiroPhase::Working))), ("turns", Json::int(e.turns as i64)),
    ])
}

/// One session as the office draws it. `access` is the tool's setting as an access
/// id, for a session that picked none.
pub fn state(s: &KiroSession, files: &dyn Fn(&KiroSession) -> Option<String>) -> Json { state_with(s, files, None) }

pub fn state_with(s: &KiroSession, files: &dyn Fn(&KiroSession) -> Option<String>, tool_access: Option<&str>) -> Json {
    let last = s.current().and_then(|t| t.steps.last());
    let waiting = s.waiting();
    Json::obj(vec![
        ("id", Json::int(s.id as i64)),
        ("key", st(&s.key)),
        ("files", opt(files(s).as_deref())),
        ("tool", st(s.tool.id())),
        ("bot", Json::int(s.bot as i64)),
        ("seat", Json::int(s.seat as i64)),
        ("title", st(&s.title())),
        ("folder", st(&s.folder)),
        // (int?)Math.Round(c): to even at the half, as .NET rounds.
        ("ctx", s.context.map_or(Json::Null, |c| Json::int(c.round_ties_even() as i64))),
        // The session's own tool access, or the tool's setting.
        ("access", st(s.access.as_deref().or(tool_access).unwrap_or("full"))),
        ("stage", st(if waiting { "waiting" } else { stage(s.state, s.phase) })),
        // Asked to stop or pause, and the tool hasn't said it has.
        ("stopping", Json::Bool(s.stopping)),
        ("act", st(act(s.phase))),
        // What the agent is waiting on the user for, and how many more are behind it.
        ("ask", s.asking().map_or(Json::Null, |a| {
            let (verb, obj) = crate::words::ask_line(a);
            Json::obj(vec![
                ("id", st(&a.id)), ("kind", st(&a.kind)), ("title", st(&crate::words::ask_title(a))),
                ("line", st(format!("{verb} {obj}").trim())), ("command", opt(a.command.as_deref())), ("path", opt(a.path.as_deref())),
                ("preview", opt(a.preview.as_deref())), ("added", Json::int(a.added as i64)), ("removed", Json::int(a.removed as i64)),
                ("reason", st(&a.reason)), ("danger", Json::Bool(a.danger)), ("allow", st(crate::words::ask_allow(a))),
                ("more", Json::int(s.asks.len() as i64 - 1)),
                // A question's own choices, which the office shows as buttons.
                ("questions", a.questions.as_ref().map_or(Json::Null, |qs| Json::Arr(qs.iter().map(|q| Json::obj(vec![
                    ("header", st(&q.header)), ("question", st(&q.question)),
                    ("options", Json::Arr(q.options.iter().map(|(l, d)| Json::obj(vec![("label", st(l)), ("description", st(d))])).collect())),
                    ("multiple", Json::Bool(q.multiple)), ("custom", Json::Bool(q.custom)),
                ])).collect()))),
            ])
        })),
        ("pose", st(pose(s.phase))),
        ("file", st(&last.and_then(|l| short(l.target.as_deref())).unwrap_or_default())),
        ("turns", Json::Arr(s.turns.iter().map(|t| {
            let stage_of = match (&t.result, t.queued) {
                (Some(r), _) => stage(r.state, KiroPhase::Working),
                (None, true) => "queued",
                (None, false) => if waiting { "waiting" } else { stage(s.state, s.phase) },
            };
            Json::obj(vec![
                ("prompt", st(&t.prompt)),
                ("images", Json::Arr(t.images.iter().map(|p| st(&format!("https://hover.images/{}", escape_data(&file_name(p))))).collect())),
                ("queued", Json::Bool(t.queued)),
                ("stage", st(stage_of)),
                ("steps", Json::Arr(t.steps.iter().map(|x| row(x, &s.folder)).collect())),
                // Markdown as the tool wrote it; the page renders it.
                ("answer", st(t.result.as_ref().map_or("", |r| r.text.as_str()))),
                ("t0", Json::int(ms(t.started_at))),
                ("woke", t.woke_at.map_or(Json::Null, |w| Json::double(w.secs_since(&t.started_at)))),
                ("took", t.ended_at.map_or(Json::Null, |e| Json::double(e.secs_since(&t.started_at) * 1000.0))),
                ("credits", t.credits.map_or(Json::Null, Json::double)),
            ])
        }).collect())),
    ])
}

/// Ms: 0 for default(DateTime), else Unix milliseconds.
pub fn ms(t: Stamp) -> i64 { if t.ticks == 0 { 0 } else { t.unix_ms() } }

pub fn stage(state: KiroState, phase: KiroPhase) -> &'static str {
    match state {
        KiroState::Running => if phase == KiroPhase::Starting { "waking" } else { "working" },
        KiroState::Completed => "done",
        KiroState::Failed => "failed",
        KiroState::Cancelled => "stopped",
        KiroState::Idle => "waking",
    }
}

pub fn act(p: KiroPhase) -> &'static str {
    match p {
        KiroPhase::Thinking | KiroPhase::Planning | KiroPhase::Starting => "Thinking",
        KiroPhase::Reading => "Reading",
        KiroPhase::Searching => "Searching",
        KiroPhase::Editing => "Editing",
        KiroPhase::Running => "Running",
        KiroPhase::Writing => "Writing",
        KiroPhase::Working => "Working",
    }
}

/// How the bot sits: the office has four ways of working.
pub fn pose(p: KiroPhase) -> &'static str {
    match p {
        KiroPhase::Reading | KiroPhase::Searching => "Reading",
        KiroPhase::Editing | KiroPhase::Writing | KiroPhase::Working => "Editing",
        KiroPhase::Running => "Running",
        _ => "Thinking",
    }
}

/// A call that hands work to a subagent (DeskInfo.IsSubagent): Claude Code's and
/// OpenCode's task tool arrive as kind "agent"; Codex's spawn_agent and Kiro's subagent
/// tool come as "other" or "think" with the name in the title; and a subagent_type (or
/// its like) in the call's raw input says so whatever the call is named.
pub fn is_subagent(x: &KiroStep) -> bool {
    x.kind == "agent" || crate::desk::field(x.input.as_deref(), &crate::desk::AGENT_KEYS).is_some()
        || (matches!(x.kind.as_str(), "other" | "think") && title_hands_off(&x.title))
}

/// AgentTitle: \b(sub-?agents?|use_subagent|spawn_agent|delegat(e|ing))\b, any case.
fn title_hands_off(title: &str) -> bool {
    let t = title.to_lowercase();
    let word = |c: char| c.is_alphanumeric() || c == '_';
    ["subagents", "subagent", "sub-agents", "sub-agent", "use_subagent", "spawn_agent", "delegating", "delegate"].iter().any(|w| {
        t.match_indices(w).any(|(i, _)| !t[..i].chars().next_back().is_some_and(word) && !t[i + w.len()..].chars().next().is_some_and(word))
    })
}

/// The subagents the session's live turn has out: its subagent steps not yet completed
/// or failed. The office shows each as a helper at the desk while the bot is at work.
pub fn subagents_out(s: &KiroSession) -> usize {
    s.current().map_or(0, |t| t.steps.iter().filter(|x| is_subagent(x) && !matches!(x.status.as_str(), "completed" | "failed")).count())
}

/// A step as the chat's timeline shows it: its kind's icon, a verb, and the file (its
/// name bright, its folder dim) or the command it was about, with the change it made or
/// what the command printed, and how it went.
pub fn row(x: &KiroStep, folder: &str) -> Json {
    // Reasoning the tool exposed, and a subagent (OpenCode's task tool): their own rows.
    if x.kind == "thought" || is_subagent(x) {
        return Json::obj(vec![
            ("k", st(if x.kind == "thought" { "thought" } else { "agent" })), ("verb", st(&x.title)), ("name", Json::Null), ("dir", Json::Null),
            ("cmd", opt(x.target.as_deref())), ("status", st(&x.status)), ("add", Json::int(0)), ("del", Json::int(0)), ("diff", Json::Null),
            ("out", opt(x.output.as_deref())), ("exit", Json::Null), ("ms", x.ms.map_or(Json::Null, Json::double)),
        ]);
    }
    let icon = match x.kind.as_str() { "read" => "read", "edit" | "delete" | "move" => "edit", "execute" => "run", "search" | "fetch" => "search", _ => "think" };
    let verb = match x.kind.as_str() {
        "read" => Some("Read"), "edit" => Some("Edited"), "delete" => Some("Deleted"), "move" => Some("Moved"),
        "execute" => Some("Ran"), "search" => Some("Searched"), "fetch" => Some("Fetched"), _ => None,
    };
    let target = relative(x.target.as_deref(), folder);
    let (mut name, mut dir, mut cmd) = (None, None, None);
    if matches!(x.kind.as_str(), "execute" | "search") {
        cmd = relative_whole(x.target.as_deref(), folder).or_else(|| verb.map(|_| x.title.clone()));
    } else if let (Some(t), true) = (&target, matches!(x.kind.as_str(), "read" | "edit" | "delete" | "move")) {
        let t = t.replace('\\', "/");
        match t.rfind('/') { None => name = Some(t), Some(i) => { name = Some(t[i + 1..].to_owned()); dir = Some(t[..i].to_owned()); } }
    } else if target.is_some() {
        cmd = target;
    }
    Json::obj(vec![
        ("k", st(icon)), ("verb", st(verb.unwrap_or(&x.title))), ("name", opt(name.as_deref())), ("dir", opt(dir.as_deref())), ("cmd", opt(cmd.as_deref())),
        ("status", st(&x.status)), ("add", Json::int(x.added as i64)), ("del", Json::int(x.removed as i64)),
        ("diff", opt(x.diff.as_deref())), ("out", opt(x.output.as_deref())), ("exit", x.exit.map_or(Json::Null, |e| Json::int(e as i64))),
        ("ms", x.ms.map_or(Json::Null, Json::double)),
    ])
}

/// The target inside the folder, relative to it with forward slashes; one line, 90
/// characters at most. Windows compares as C# does (backslashes, any case). On Linux
/// C#'s backslash root could never match a path, so a target there was never made
/// relative; the port compares with the platform's own separator instead.
pub fn relative(target: Option<&str>, folder: &str) -> Option<String> {
    relative_whole(target, folder).map(|t| if units(&t) > 90 { format!("{}…", head_units(&t, 89)) } else { t })
}

/// `relative`, never cut: a command or pattern the chat shows whole (it wraps there).
fn relative_whole(target: Option<&str>, folder: &str) -> Option<String> {
    let t0 = target.filter(|t| !t.trim().is_empty())?;
    let mut t = t0.trim().replace('\n', " ");
    if cfg!(windows) {
        let root = format!("{}\\", folder.trim_end_matches(['\\', '/']));
        let norm = t.replace('/', "\\");
        if norm.len() >= root.len() && norm.is_char_boundary(root.len()) && norm[..root.len()].to_lowercase() == root.to_lowercase() {
            t = t[root.len()..].replace('\\', "/");
        }
    } else {
        let root = format!("{}/", folder.trim_end_matches('/'));
        if root.len() > 1 { if let Some(rest) = t.strip_prefix(&root) { t = rest.to_owned(); } }
    }
    Some(t)
}

/// The file a step was about, or its command cut short.
pub fn short(target: Option<&str>) -> Option<String> {
    let t = target.filter(|t| !t.trim().is_empty())?.trim();
    if t.contains(' ') || units(t) > 40 { return Some(if units(t) > 28 { format!("{}…", head_units(t, 27)) } else { t.to_owned() }); }
    Some(file_name(if cfg!(windows) { t.trim_end_matches(['\\', '/']) } else { t.trim_end_matches('/') }))
}

/// Path.GetFileName: after the last separator (\ and / on Windows, / elsewhere).
fn file_name(p: &str) -> String {
    let cut = if cfg!(windows) { p.rfind(['\\', '/', ':']) } else { p.rfind('/') };
    cut.map_or(p, |i| &p[i + 1..]).to_owned()
}

/// Uri.EscapeDataString: RFC 3986 unreserved characters kept, the rest as %XX of UTF-8.
pub fn escape_data(s: &str) -> String {
    s.bytes().map(|c| if c.is_ascii_alphanumeric() || b"-_.~".contains(&c) { (c as char).to_string() } else { format!("%{c:02X}") }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(kind: &str, title: &str, target: Option<&str>, status: &str) -> KiroStep {
        KiroStep::new("x", kind, title, target.map(str::to_owned), status)
    }

    #[test]
    fn rows_lines_and_tags_as_kiro_page_writes_them() {
        let dir = if cfg!(windows) { r"C:\p" } else { "/p" };
        let inside = if cfg!(windows) { r"c:\P\src\a.ts" } else { "/p/src/a.ts" };
        assert_eq!(row(&step("read", "Read File", Some(inside), "completed"), dir).compact(), r#"{"k":"read","verb":"Read","name":"a.ts","dir":"src","cmd":null,"status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}"#);
        assert_eq!(row(&step("execute", "Run", Some("npm\ntest"), "failed"), dir).compact(), r#"{"k":"run","verb":"Ran","name":null,"dir":null,"cmd":"npm test","status":"failed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}"#);
        assert_eq!(row(&step("think", "Planning", Some("x"), "completed"), dir).compact(), r#"{"k":"think","verb":"Planning","name":null,"dir":null,"cmd":"x","status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}"#);
        assert_eq!(row(&step("other", "Working", None, "completed"), dir).compact(), r#"{"k":"think","verb":"Working","name":null,"dir":null,"cmd":null,"status":"completed","add":0,"del":0,"diff":null,"out":null,"exit":null,"ms":null}"#);
        assert_eq!(relative(Some(&"a".repeat(95)), dir).unwrap(), format!("{}…", "a".repeat(89)));
        // A command reaches the chat whole; the chat wraps it.
        let long = format!("cargo test {}", "x".repeat(120));
        assert_eq!(row(&step("execute", "Run", Some(&long), "completed"), dir).get("cmd").and_then(|c| c.as_str()), Some(long.as_str()));
        assert_eq!(short(Some("npm run test -- --watch=false")), Some("npm run test -- --watch=fal…".into()));
        assert_eq!(short(Some("src/auth/refresh.ts")), Some("refresh.ts".into()));
        assert_eq!(escape_data("a b+é.png"), "a%20b%2B%C3%A9.png");
    }

    #[test]
    fn stage_act_and_pose() {
        assert_eq!((stage(KiroState::Running, KiroPhase::Starting), stage(KiroState::Running, KiroPhase::Reading)), ("waking", "working"));
        assert_eq!((act(KiroPhase::Planning), pose(KiroPhase::Writing), pose(KiroPhase::Planning)), ("Thinking", "Editing", "Thinking"));
    }
}

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
        ("sessions", Json::Arr(sessions.iter().map(|s| state(s, o.files)).collect())),
        ("history", o.history.as_ref().map_or(Json::Null, |h| Json::Arr(h.iter().map(history_row).collect()))),
    ])
}

/// {type: "transcript", session}: one saved session, whole, for the chat to show.
pub fn transcript(s: &KiroSession, files: &dyn Fn(&KiroSession) -> Option<String>) -> Json {
    Json::obj(vec![("type", st("transcript")), ("session", state(s, files))])
}

/// {type, text}: a toast, a picked folder (KiroPage.Say).
pub fn say(kind: &str, text: &str) -> Json { Json::obj(vec![("type", st(kind)), ("text", st(text))]) }

const EFFORT_IDS: [&str; 3] = ["effortLevel", "reasoning_effort", "effort"];

fn offer(settings: &Settings, t: AgentTool, category: &str, ids: &[&str]) -> Option<AcpOption> {
    let offers = settings.agent_offers(t);
    offers.iter().find(|x| x.category.as_deref() == Some(category)).or_else(|| offers.iter().find(|x| ids.contains(&x.id.as_str()))).cloned()
}

/// KiroPage.Models: what the tool offered, Kiro's own list before it has run; a
/// Default that sends none comes first unless the first is the tool's "auto".
pub fn models(settings: &Settings, t: AgentTool) -> Vec<(String, String)> {
    let mut list: Vec<(String, String)> = match offer(settings, t, "model", &["model"]) {
        Some(o) => o.choices.iter().map(|c| (c.value.clone(), c.name.clone())).collect(),
        None if t == AgentTool::Kiro => crate::KIRO_MODELS.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect(),
        None => vec![],
    };
    if list.is_empty() || !(list[0].0 == "auto" || list[0].0.starts_with("default")) { list.insert(0, (String::new(), "Default".into())); }
    list
}

fn tool(o: &Office, t: AgentTool) -> Json {
    let opts = o.settings.agent_options(t);
    let known = (o.ready)(t);
    let models = models(o.settings, t);
    let effort = offer(o.settings, t, "thought_level", &EFFORT_IDS);
    Json::obj(vec![
        ("id", st(t.id())),
        ("name", st(t.name())),
        // Unknown until checked; the picker offers it meanwhile.
        ("ready", Json::Bool(known.as_ref().is_none_or(AgentReady::ok))),
        ("hint", st(known.as_ref().map_or("", |k| k.hint.as_str()))),
        ("access", st(if opts.read_only && crate::agents::read_only_works(t) { "read only" } else { "full tool access" })),
        ("hideSteps", Json::Bool(opts.hide_steps)),
        ("models", Json::Arr(models.iter().map(|(id, name)| Json::obj(vec![("id", st(id)), ("name", st(name))])).collect())),
        ("model", opt(opts.model.as_deref().or(models.first().map(|m| m.0.as_str())))),
        ("efforts", Json::Arr(effort.as_ref().map_or(vec![], |e| e.choices.iter().map(|c| st(&c.value)).collect()))),
        ("effort", opt(opts.effort.as_deref().or(effort.as_ref().and_then(|e| e.current.as_deref())))),
    ])
}

fn history_row(e: &HistoryEntry) -> Json {
    Json::obj(vec![
        ("key", st(&e.key)), ("tool", st(e.tool.id())), ("title", st(&e.title)), ("folder", st(&e.folder)),
        ("at", Json::int(ms(e.updated))), ("stage", st(stage(e.state, KiroPhase::Working))), ("turns", Json::int(e.turns as i64)),
    ])
}

/// One session as the office draws it.
pub fn state(s: &KiroSession, files: &dyn Fn(&KiroSession) -> Option<String>) -> Json {
    let last = s.current().and_then(|t| t.steps.last());
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
        ("stage", st(stage(s.state, s.phase))),
        ("act", st(act(s.phase))),
        ("pose", st(pose(s.phase))),
        ("file", st(&last.and_then(|l| short(l.target.as_deref())).unwrap_or_default())),
        ("turns", Json::Arr(s.turns.iter().map(|t| {
            let stage_of = match (&t.result, t.queued) {
                (Some(r), _) => stage(r.state, KiroPhase::Working),
                (None, true) => "queued",
                (None, false) => stage(s.state, s.phase),
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

/// A step as the page lists it: [icon, line, "failed" or null].
pub fn row(x: &KiroStep, folder: &str) -> Json {
    let icon = match x.kind.as_str() { "read" => "read", "edit" | "delete" | "move" => "edit", "execute" => "run", "search" | "fetch" => "search", _ => "think" };
    let verb = match x.kind.as_str() {
        "read" => Some("Read"), "edit" => Some("Edited"), "delete" => Some("Deleted"), "move" => Some("Moved"),
        "execute" => Some("Ran"), "search" => Some("Searched"), "fetch" => Some("Fetched"), _ => None,
    };
    let target = relative(x.target.as_deref(), folder);
    let text = match (verb, &target) {
        (Some(v), Some(t)) => format!("{v} {t}"),
        (_, t) => format!("{}{}", x.title, t.as_ref().map_or(String::new(), |t| format!(" {t}"))),
    };
    Json::Arr(vec![st(icon), st(&text), if x.status == "failed" { st("failed") } else { Json::Null }])
}

/// The target inside the folder, relative to it with forward slashes; one line, 90
/// characters at most. Windows compares as C# does (backslashes, any case). On Linux
/// C#'s backslash root could never match a path, so a target there was never made
/// relative; the port compares with the platform's own separator instead.
pub fn relative(target: Option<&str>, folder: &str) -> Option<String> {
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
    Some(if units(&t) > 90 { format!("{}…", head_units(&t, 89)) } else { t })
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
        assert_eq!(row(&step("read", "Read File", Some(inside), "completed"), dir).compact(), r#"["read","Read src/a.ts",null]"#);
        assert_eq!(row(&step("execute", "Run", Some("npm\ntest"), "failed"), dir).compact(), r#"["run","Ran npm test","failed"]"#);
        assert_eq!(row(&step("think", "Planning", Some("x"), "completed"), dir).compact(), r#"["think","Planning x",null]"#);
        assert_eq!(row(&step("other", "Working", None, "completed"), dir).compact(), r#"["think","Working",null]"#);
        assert_eq!(relative(Some(&"a".repeat(95)), dir).unwrap(), format!("{}…", "a".repeat(89)));
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

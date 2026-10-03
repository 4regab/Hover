//! src/Hover.Backend/OfficeState.cs: everything the web office draws, as the one `state`
//! message (and `transcript`'s session), with the C# anonymous objects' property names
//! and order. The page is Arz's (web/office), which reads these names, so they are kept
//! as the serializer wrote them (camelCase as declared, nulls included).
//!
//! What Rust has that the page's C# did not is added, never changed: Claude Code among the
//! tools, `restore` and `again` on a turn (its checkpoints exist, so Restore and Try again
//! can be offered), and `stopping` on a session.

use crate::screen;
use hover_agents::agents::AgentReady;
use hover_agents::ask::AgentAsk;
use hover_agents::desk::{self, DeskStep, Snap};
use hover_agents::session::{KiroSession, KiroTurn};
use hover_agents::state::{act, escape_data, ms, pose, relative, short, stage};
use hover_agents::stream::KiroPhase;
use hover_agents::spaces::{self, SpaceState};
use hover_agents::{setup, words};
use hover_core::history::HistoryEntry;
use hover_core::json::Json;
use hover_core::model::{AcpOption, AgentTool, KiroStep};
use hover_core::settings::Settings;

fn st(s: &str) -> Json { Json::str(s) }
fn opt(s: Option<&str>) -> Json { Json::opt_str_of(s) }
fn int(n: impl Into<i64>) -> Json { Json::int(n.into()) }
fn num(n: Option<f64>) -> Json { n.map_or(Json::Null, Json::double) }

/// What a snapshot reads besides the sessions and the settings.
pub struct Ctx<'a> {
    pub settings: &'a Settings,
    /// Oldest first (KiroSessions.All).
    pub sessions: &'a [KiroSession],
    /// Newest first (AgentHistory.Entries); none when the history is off.
    pub history: &'a [HistoryEntry],
    pub can_start: bool,
    pub max_running: usize,
    /// Sessions keep checkpoints of their folder (git is there): Restore and Try again.
    pub checkpoints: bool,
    /// Agents.Known.
    pub known: &'a dyn Fn(AgentTool) -> Option<AgentReady>,
}

/// OfficeState.Snapshot.
pub fn snapshot(c: &Ctx) -> Json {
    Json::obj(vec![
        ("type", st("state")),
        ("canStart", Json::Bool(c.can_start)),
        ("maxRunning", int(c.max_running as i64)),
        // Agents have desktops of their own (Cua Spaces): the office says so.
        ("spaces", Json::Bool(spaces::wanted())),
        ("folder", opt(c.settings.kiro_folder().as_deref())),
        ("tool", st(c.settings.agent_tool().id())),
        ("tools", Json::Arr(AgentTool::ALL.iter().map(|&t| tool(c, t)).collect())),
        ("sessions", Json::Arr(c.sessions.iter().map(|s| session(c, s)).collect())),
        ("history", Json::Arr(c.history.iter().map(history_row).collect())),
    ])
}

/// {type: "transcript", session}: one saved session, whole.
pub fn transcript(c: &Ctx, s: &KiroSession) -> Json { Json::obj(vec![("type", st("transcript")), ("session", session(c, s))]) }

fn history_row(e: &HistoryEntry) -> Json {
    Json::obj(vec![
        ("key", st(&e.key)), ("tool", st(e.tool.id())), ("title", st(&e.title)), ("folder", st(&e.folder)),
        ("at", int(ms(e.updated))), ("stage", st(stage(e.state, KiroPhase::Working))), ("turns", int(e.turns as i64)),
    ])
}

// MARK: Tools

/// OfficeState.Offer: the option of a category the tool listed, else the one with such an id.
fn offer(settings: &Settings, t: AgentTool, category: &str, ids: &[&str]) -> Option<AcpOption> {
    let offers = settings.agent_offers(t);
    offers.iter().find(|x| x.category.as_deref() == Some(category)).or_else(|| offers.iter().find(|x| ids.contains(&x.id.as_str()))).cloned()
}

/// OfficeState.Models: what the tool offered (Kiro's own list before it has run), and a
/// Default that sends none first unless the first is the tool's "auto".
fn models(settings: &Settings, t: AgentTool) -> Json {
    let mut list: Vec<(String, String, Option<Vec<String>>)> = match offer(settings, t, "model", &["model"]) {
        Some(o) => o.choices.iter().map(|c| (c.value.clone(), c.name.clone(), c.levels.clone())).collect(),
        None if t == AgentTool::Kiro => hover_agents::KIRO_MODELS.iter().map(|(a, b)| (a.to_string(), b.to_string(), None)).collect(),
        None => vec![],
    };
    if list.first().is_none_or(|m| m.0 != "auto") { list.insert(0, (String::new(), "Default".into(), None)); }
    Json::Arr(list.iter().map(|(id, name, levels)| Json::obj(vec![
        ("id", st(id)), ("name", st(name)), ("levels", levels.as_ref().map_or(Json::Null, |l| Json::Arr(l.iter().map(|x| st(x)).collect()))),
    ])).collect())
}

/// What one-click setup has to do (Settings shows it), and how it is going.
fn setup_state(t: AgentTool) -> Json {
    let p = setup::of(t);
    Json::obj(vec![
        ("step", opt(p.step.as_deref())), ("line", st(&p.line)), ("error", opt(p.error.as_deref())), ("busy", Json::Bool(setup::busy(t))),
        ("needs", Json::Arr(setup::plan(t).iter().map(|s| st(&s.title)).collect())),
    ])
}

fn tool(c: &Ctx, t: AgentTool) -> Json {
    let known = (c.known)(t);
    let opts = c.settings.agent_options(t);
    let read_only = hover_agents::agents::read_only_works(t);
    let effort = offer(c.settings, t, "thought_level", &["effortLevel", "reasoning_effort", "effort"]);
    let caps = hover_agents::runtime::caps(t);
    Json::obj(vec![
        ("id", st(t.id())),
        ("name", st(t.name())),
        ("ready", Json::Bool(known.as_ref().is_some_and(AgentReady::ok))),
        ("hint", st(known.as_ref().map_or("Checking installation…", |k| k.hint.as_str()))),
        ("checkedYet", Json::Bool(known.is_some())),
        ("installed", Json::Bool(known.as_ref().is_some_and(|k| k.installed))),
        ("signedIn", Json::Bool(known.as_ref().is_some_and(|k| k.signed_in))),
        ("canSetup", Json::Bool(setup::supported())),
        ("setup", setup_state(t)),
        // A newer release out, and its one-click update going on (a red "!" on the logo).
        ("update", crate::updates::state(t.id())),
        ("access", st(opts.access_id(read_only))),
        ("readOnly", Json::Bool(read_only)),
        ("hideSteps", Json::Bool(opts.hide_steps)),
        ("models", models(c.settings, t)),
        ("model", st(opts.model.as_deref().unwrap_or(""))),
        ("efforts", Json::Arr(effort.as_ref().map_or(vec![], |e| e.choices.iter().map(|x| st(&x.value)).collect()))),
        ("effort", opt(opts.effort.as_deref())),
        ("effortLabel", st(caps.effort_label)),
        ("questions", Json::Bool(caps.questions)),
    ])
}

// MARK: Sessions

/// The question's one-line reason: the tool's own, unless it only names the kind of call,
/// and the lines it changes (AgentWords.AskWhy).
pub fn ask_why(a: &AgentAsk) -> String {
    let lines = if a.added + a.removed > 0 && a.kind != "edit" { format!("+{} −{}", a.added, a.removed) } else { String::new() };
    let reason = if matches!(a.reason.as_str(), "Runs a command" | "Uses a tool" | "Edits a file" | "Deletes files" | "Moves or renames files" | "Uses the network") { "" } else { a.reason.as_str() };
    if !reason.is_empty() && !lines.is_empty() { format!("{reason} · {lines}") } else { format!("{reason}{lines}") }
}

fn ask(s: &KiroSession, a: &AgentAsk) -> Json {
    let (verb, obj) = words::ask_line(a);
    Json::obj(vec![
        ("id", st(&a.id)),
        ("kind", st(&a.kind)),
        ("title", st(&words::ask_title(a))),
        ("line", st(format!("{verb} {obj}").trim())),
        ("command", opt(a.command.as_deref())),
        ("path", opt(a.path.as_deref())),
        ("preview", opt(a.preview.as_deref())),
        ("added", int(a.added as i64)),
        ("removed", int(a.removed as i64)),
        ("reason", st(&ask_why(a))),
        ("danger", Json::Bool(a.danger)),
        ("allow", st(words::ask_allow(a))),
        ("more", int(s.asks.len() as i64 - 1)),
        // A question's own choices, which the office shows as buttons.
        ("questions", a.questions.as_ref().map_or(Json::Null, |qs| Json::Arr(qs.iter().map(|q| Json::obj(vec![
            ("header", st(&q.header)),
            ("question", st(&q.question)),
            ("options", Json::Arr(q.options.iter().map(|(l, d)| Json::obj(vec![("label", st(l)), ("description", st(d))])).collect())),
            ("multiple", Json::Bool(q.multiple)),
            ("custom", Json::Bool(q.custom)),
        ])).collect()))),
    ])
}

/// The address the page reads the session's files from (the host serves it), when its
/// folder is still there.
fn files(s: &KiroSession) -> Option<String> { hover_agents::usable_folder(Some(&s.folder)).then(|| format!("hover://files/{}/", s.key)) }

/// Path.GetFileName: after the last separator (\ and / on Windows, / elsewhere).
fn file_name(p: &str) -> &str {
    let cut = if cfg!(windows) { p.rfind(['\\', '/', ':']) } else { p.rfind('/') };
    cut.map_or(p, |i| &p[i + 1..])
}

pub fn session(c: &Ctx, s: &KiroSession) -> Json {
    let snap = Snap::of(s);
    let last = s.current().and_then(|t| t.steps.last());
    let waiting = s.waiting();
    Json::obj(vec![
        ("id", int(s.id)),
        ("key", st(&s.key)),
        ("files", opt(files(s).as_deref())),
        ("tool", st(s.tool.id())),
        ("bot", int(s.bot as i64)),
        ("seat", int(s.seat as i64)),
        ("title", st(&s.title())),
        ("folder", st(&s.folder)),
        // (int?)Math.Round(c): to even at the half, as .NET rounds.
        ("ctx", s.context.map_or(Json::Null, |x| int(x.round_ties_even() as i64))),
        // The session's own tool access, or the tool's setting.
        ("access", st(s.access.as_deref().unwrap_or_else(|| c.settings.agent_options(s.tool).access_id(hover_agents::agents::read_only_works(s.tool))))),
        ("stage", st(if waiting { "waiting" } else { stage(s.state, s.phase) })),
        // Asked to stop or pause and the tool hasn't said it has (added for Rust).
        ("stopping", Json::Bool(s.stopping)),
        ("act", st(act(s.phase))),
        // What the agent is waiting on the user for, and how many more are behind it.
        ("ask", s.asking().map_or(Json::Null, |a| ask(s, a))),
        ("pose", st(pose(s.phase))),
        ("file", st(&last.and_then(|l| short(l.target.as_deref())).unwrap_or_default())),
        // Computer use among its last steps: the desk's screen panel goes live.
        ("testing", Json::Bool(snap.testing())),
        // The project's desktop (a Cua Space) it shares with the other agents in its folder,
        // when agents have them: how it is getting on.
        ("space", if spaces::wanted() { space(s, c.sessions, spaces::state_of(&s.folder).as_ref()) } else { Json::Null }),
        // Hover's browser among its last steps: the desk's Browser row says so.
        ("browsing", Json::Bool(snap.browsing())),
        // The apps its computer use opened: the screen shows only these over the desktop.
        ("apps", desk::apps(&snap).map_or(Json::Null, |a| Json::obj(vec![
            ("pids", Json::Arr(a.pids.iter().map(|&p| int(p)).collect())),
            ("bundles", Json::Arr(a.bundles.iter().map(|x| st(x)).collect())),
            ("names", Json::Arr(a.names.iter().map(|x| st(x)).collect())),
        ]))),
        ("turns", Json::Arr(s.turns.iter().map(|t| turn(c, s, t, waiting)).collect())),
    ])
}

/// The session's project desktop: its name and the project's, how it is getting on (phase
/// "none" before it has been asked for anything), and the other sessions in the same project
/// that share it. `state` is the project's Space state (`spaces::state_of`).
pub fn space(s: &KiroSession, all: &[KiroSession], state: Option<&SpaceState>) -> Json {
    let name = spaces::name_for(&s.folder);
    Json::obj(vec![
        ("name", st(&name)), ("project", st(&spaces::title(&s.folder))),
        ("phase", st(state.map_or("none", |x| x.phase.as_str()))), ("line", st(state.map_or("", |x| x.line.as_str()))),
        ("fraction", num(state.and_then(|x| x.fraction))), ("error", opt(state.and_then(|x| x.error.as_deref()))),
        ("with", Json::Arr(all.iter().filter(|x| x.id != s.id && spaces::name_for(&x.folder) == name).map(|x| int(x.id)).collect())),
    ])
}

fn turn(c: &Ctx, s: &KiroSession, t: &KiroTurn, waiting: bool) -> Json {
    let stage_of = match (&t.result, t.queued) {
        (Some(r), _) => stage(r.state, KiroPhase::Working),
        (None, true) => "queued",
        (None, false) => if waiting { "waiting" } else { stage(s.state, s.phase) },
    };
    Json::obj(vec![
        ("prompt", st(&t.prompt)),
        ("images", Json::Arr(t.images.iter().map(|p| st(&format!("hover://images/{}", escape_data(file_name(p))))).collect())),
        ("queued", Json::Bool(t.queued)),
        ("stage", st(stage_of)),
        ("steps", Json::Arr(t.steps.iter().map(|x| row(x, &s.folder)).collect())),
        // Markdown as the tool wrote it; the page renders it.
        ("answer", st(t.result.as_ref().map_or("", |r| r.text.as_str()))),
        ("t0", int(ms(t.started_at))),
        ("woke", t.woke_at.map_or(Json::Null, |w| Json::double(w.secs_since(&t.started_at)))),
        ("took", t.ended_at.map_or(Json::Null, |e| Json::double(e.secs_since(&t.started_at) * 1000.0))),
        ("credits", num(t.credits)),
        // Restore to just after this answer, and Try again from this message: offered
        // when the folder's checkpoints were kept (added for Rust).
        ("restore", Json::Bool(c.checkpoints && t.result.is_some() && t.after.is_some())),
        ("again", Json::Bool(c.checkpoints && t.before.is_some())),
    ])
}

// MARK: Steps

/// String.Length-style cut: at most `n` UTF-16 units, kept whole code points.
fn cut(t: &str, n: usize) -> &str {
    let mut used = 0;
    for (i, ch) in t.char_indices() {
        used += ch.len_utf16();
        if used > n { return &t[..i]; }
    }
    t
}

fn units(t: &str) -> usize { t.encode_utf16().count() }

/// `t[..n-1] + "…"` when longer than n, as the C# cuts a label.
fn clip(t: &str, n: usize) -> String { if units(t) > n { format!("{}…", cut(t, n - 1)) } else { t.to_owned() } }

/// A step as the chat's timeline shows it: its kind's icon, a verb, and the file (its
/// name bright, its folder dim) or the command it was about, with the change it made or
/// what the command printed, and how it went.
pub fn row(x: &KiroStep, folder: &str) -> Json {
    // What the tool thought, as it showed it: the text folds under the row.
    if x.kind == "thought" {
        return Json::obj(vec![("k", st("thought")), ("verb", st("Thought")), ("status", st(&x.status)), ("out", opt(x.output.as_deref())), ("ms", num(x.ms))]);
    }
    let d = DeskStep::from(x);
    // A subagent it started: its task, and what it came back with.
    if desk::is_subagent(&d) {
        let log = x.log.as_deref().map(|l| if units(l) > 2000 { format!("{}…", cut(l, 2000)) } else { l.to_owned() });
        return Json::obj(vec![
            ("k", st("agent")), ("verb", st("Subagent")),
            ("agent", opt(desk::field(x.input.as_deref(), &["subagent_type", "subagent", "agent_type", "agent_name", "agentName"]).as_deref())),
            ("cmd", st(&desk::field(x.input.as_deref(), &["description"]).unwrap_or_else(|| x.title.clone()))),
            ("status", st(&x.status)), ("out", opt(log.as_deref())), ("ms", num(x.ms)),
        ]);
    }
    // Hover's browser: what it did on the page, and where.
    if let Some(op) = desk::browser_op(&x.title) {
        let input = x.input.as_deref();
        let on = match op.as_str() {
            "open" => desk::field(input, &["url"]),
            "click" | "scroll" | "wait" => desk::field(input, &["text", "selector", "label"]).or_else(|| screen::number(input, &["ref"]).map(|r| format!("[{r}]"))),
            "type" => desk::field(input, &["text"]),
            "press" => desk::field(input, &["key"]),
            _ => None,
        };
        let said = match op.as_str() {
            "open" => "Opened", "snapshot" => "Read the page", "click" => "Clicked", "type" => "Typed", "press" => "Pressed",
            "scroll" => "Scrolled", "screenshot" => "Took a screenshot", "evaluate" => "Ran a script on the page", "wait" => "Waited for",
            "console" => "Read the console", "back" => "Went back", _ => "Reloaded the page",
        };
        return Json::obj(vec![
            ("k", st("web")), ("verb", st(said)), ("cmd", on.map_or(Json::Null, |o| st(&clip(&o, 90)))), ("status", st(&x.status)),
            ("out", opt(x.output.as_deref())), ("ms", num(x.ms)),
        ]);
    }
    // Computer use: what it did on the agent's desktop, for the screen panel's activity.
    if desk::is_screen(&d) {
        let (did, what) = screen::action(&d);
        let out = x.output.as_deref().map(|o| if units(o) > 600 { format!("{}…", cut(o, 600)) } else { o.to_owned() });
        return Json::obj(vec![
            ("k", st("screen")), ("verb", st(&did)), ("cmd", opt(what.as_deref())), ("status", st(&x.status)), ("out", opt(out.as_deref())), ("ms", num(x.ms)),
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
        cmd = target.clone().or_else(|| verb.map(|_| x.title.clone()));
    } else if let (Some(t), true) = (&target, matches!(x.kind.as_str(), "read" | "edit" | "delete" | "move")) {
        let t = t.replace('\\', "/");
        match t.rfind('/') { None => name = Some(t), Some(i) => { name = Some(t[i + 1..].to_owned()); dir = Some(t[..i].to_owned()); } }
    } else if target.is_some() {
        cmd = target;
    }
    Json::obj(vec![
        ("k", st(icon)),
        ("verb", st(verb.unwrap_or(&x.title))),
        ("name", opt(name.as_deref())),
        ("dir", opt(dir.as_deref())),
        ("cmd", opt(cmd.as_deref())),
        ("status", st(&x.status)),
        ("add", int(x.added as i64)),
        ("del", int(x.removed as i64)),
        ("diff", opt(x.diff.as_deref())),
        ("out", opt(x.output.as_deref())),
        ("exit", x.exit.map_or(Json::Null, |e| int(e as i64))),
        ("ms", num(x.ms)),
    ])
}

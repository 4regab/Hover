//! Services/KiroRunner.cs: KiroStream, which reads a run's ACP updates loosely (every
//! field optional, the unknown skipped), and the result types it hands on.

use crate::proc::strip_ansi;
use hover_core::json::{self, Json};
use hover_core::model::{KiroState, KiroStep};

/// What the agent is broadly busy with, read from its tool calls.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KiroPhase { Starting, Thinking, Planning, Reading, Searching, Editing, Running, Writing, Working }

/// How a run ended: the answer when it completed, a readable reason otherwise.
#[derive(Clone, Debug, PartialEq)]
pub struct KiroResult {
    pub state: KiroState, pub text: String, pub exit_code: Option<i32>,
    /// Asked to stop, the tool never said it had: what it was doing may still go on, so
    /// nothing queued behind it is sent.
    pub unconfirmed: bool,
}

impl KiroResult {
    pub fn new(state: KiroState, text: impl Into<String>) -> KiroResult { KiroResult { state, text: text.into(), exit_code: None, unconfirmed: false } }
}

/// Detail from a run as it goes: a step that started or ended, the context (0 to
/// 100), the tool's session id, and what a turn cost in the tool's credits, as Kiro
/// says at its end.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KiroEvent { pub step: Option<KiroStep>, pub context: Option<f64>, pub session_id: Option<String>, pub credits: Option<f64> }

/// UTF-16 length, as C# counts a string.
pub(crate) fn units(s: &str) -> usize { s.encode_utf16().count() }

/// The first max UTF-16 units of s (never half a character, where C# could cut one).
pub(crate) fn head_units(s: &str, max: usize) -> &str {
    let mut n = 0;
    for (i, c) in s.char_indices() {
        n += c.len_utf16();
        if n > max { return &s[..i]; }
    }
    s
}

/// s[..max] + "…" when longer than max (Clip).
pub(crate) fn clip(s: &str, max: usize) -> String { if units(s) <= max { s.to_owned() } else { format!("{}…", head_units(s, max)) } }

/// line[..(max - 1)] + "…" when longer than max (KiroSession.Title).
pub(crate) fn clip_to(s: &str, max: usize) -> String { if units(s) > max { format!("{}…", head_units(s, max - 1)) } else { s.to_owned() } }

const SAID_LIMIT: usize = 64 * 1024;

pub struct KiroStream {
    /// Who is talking, for the messages a result carries.
    pub name: String,
    pub phase: KiroPhase,
    pub final_text: Option<String>,
    pub stop_reason: Option<String>,
    pub error: Option<String>,
    pub interrupted: bool,
    pub finished: bool,
    pub session_id: Option<String>,
    pub context: Option<f64>,
    said: String,
    plain: std::collections::VecDeque<String>,
    events: Vec<KiroEvent>,
    steps: std::collections::HashMap<String, KiroStep>,
    began: std::collections::HashMap<String, std::time::Instant>,
    after_tool: bool,
    message: Option<String>,
    is_final: bool,
    /// The reasoning being said now (a "thought" step's id), and when it was last sent on.
    thought: Option<String>,
    thoughts: usize,
    thought_sent: Option<std::time::Instant>,
}

/// A thought step's text is kept to this many bytes; past it the step says so.
const THOUGHT_LIMIT: usize = 256 * 1024;
/// Streaming reasoning is passed on at most this often (each pass copies the step).
const THOUGHT_EVERY: std::time::Duration = std::time::Duration::from_millis(80);

/// A content block's text (ACP's ContentBlock, or a list of them).
fn content_text(c: Option<&Json>) -> String {
    match c {
        Some(Json::Arr(parts)) => parts.iter().map(|p| content_text(Some(p))).collect(),
        Some(c) => s(c, "text").unwrap_or("").to_owned(),
        None => String::new(),
    }
}

fn s<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }

fn num(e: &Json, name: &str) -> Option<f64> { match e.get(name) { Some(v @ Json::Num(_)) => v.f64().ok(), _ => None } }

impl KiroStream {
    pub fn new(name: &str) -> KiroStream {
        KiroStream { name: name.into(), phase: KiroPhase::Starting, final_text: None, stop_reason: None, error: None, interrupted: false, finished: false,
            session_id: None, context: None, said: String::new(), plain: Default::default(), events: vec![], steps: Default::default(), began: Default::default(), after_tool: false,
            message: None, is_final: false, thought: None, thoughts: 0, thought_sent: None }
    }

    /// The reasoning the tool exposed (agent_thought_chunk), as a "thought" step whose
    /// output is the text: one step from its first chunk until the agent does something
    /// else, in its place among the tool calls. Only what the tool sends; nothing is
    /// made up from the answer.
    fn think(&mut self, text: &str) {
        if self.thought.is_none() && text.trim().is_empty() { return; }
        let id = match &self.thought {
            Some(id) => id.clone(),
            None => {
                self.thoughts += 1;
                let id = format!("hover-thought-{}", self.thoughts);
                self.began.insert(id.clone(), std::time::Instant::now());
                self.steps.insert(id.clone(), KiroStep { output: Some(String::new()), ..KiroStep::new(&id, "thought", "Thinking", None, "in_progress") });
                self.thought = Some(id.clone());
                self.thought_sent = None;
                id
            }
        };
        let step = self.steps.get_mut(&id).unwrap();
        let out = step.output.get_or_insert_with(String::new);
        if out.len() < THOUGHT_LIMIT {
            let room = THOUGHT_LIMIT - out.len();
            let mut cut = text.len().min(room);
            while !text.is_char_boundary(cut) { cut -= 1; }
            out.push_str(&text[..cut]);
            if cut < text.len() { out.push_str("\n\n[Hover keeps the first 256 KB of a thought; the rest wasn’t saved.]"); }
        }
        if self.thought_sent.is_none_or(|t| t.elapsed() >= THOUGHT_EVERY) {
            self.thought_sent = Some(std::time::Instant::now());
            self.events.push(KiroEvent { step: Some(step.clone()), ..Default::default() });
        }
    }

    /// The agent moved on: the thought is done, with how long it took.
    fn close_thought(&mut self) {
        let Some(id) = self.thought.take() else { return };
        let ms = self.began.get(&id).map(|t| t.elapsed().as_secs_f64() * 1000.0);
        if let Some(step) = self.steps.get_mut(&id) {
            step.status = "completed".into();
            step.ms = ms;
            self.events.push(KiroEvent { step: Some(step.clone()), ..Default::default() });
        }
    }

    /// The turn is over: a thought still open ends here.
    pub fn end(&mut self) { self.close_thought(); }

    /// The steps, context and session id seen since the last call.
    pub fn drain(&mut self) -> Vec<KiroEvent> { std::mem::take(&mut self.events) }

    /// Everything said so far, from the message chunks.
    pub fn said(&self) -> &str { &self.said }

    /// One line of output; the new phase when it changed.
    pub fn feed(&mut self, line: &str) -> Option<KiroPhase> {
        let line = line.trim();
        if line.is_empty() { return None; }
        if !line.starts_with('{') { self.keep(line); return None; }
        let before = self.phase;
        match json::parse(line) {
            Ok(root @ Json::Obj(_)) => self.read(&root),
            Ok(_) => return None,
            Err(_) => { self.keep(line); return None; }
        }
        (self.phase != before).then_some(self.phase)
    }

    fn keep(&mut self, line: &str) {
        self.plain.push_back(strip_ansi(line));
        while self.plain.len() > 12 { self.plain.pop_front(); }
    }

    fn read(&mut self, root: &Json) {
        let (name, body) = envelope(root);
        let n = name.to_lowercase();
        if n.contains("error") && self.error.is_none() { self.error = Some(message(body).or_else(|| message(root)).unwrap_or_else(|| "Kiro reported an error.".into())); }
        if n.contains("interrupt") || n.contains("cancel") { self.interrupted = true; }
        if n.contains("finish") || n.contains("complete") { self.finished = true; }
        if let Some(f) = s(body, "finalText").or_else(|| s(root, "finalText")) { self.final_text = Some(f.into()); }
        if let Some(r) = s(body, "stopReason").or_else(|| s(root, "stopReason")) {
            self.stop_reason = Some(r.into());
            if r == "cancelled" { self.interrupted = true; }
        }
        if let Some(u) = find_update(root, 0) { self.update(u); }
        if let Some(id) = s(body, "sessionId").or_else(|| s(root, "sessionId")).filter(|i| !i.is_empty()) {
            if self.session_id.as_deref() != Some(id) {
                self.session_id = Some(id.into());
                self.events.push(KiroEvent { session_id: Some(id.into()), ..Default::default() });
            }
        }
    }

    fn update(&mut self, u: &Json) {
        let kind = s(u, "sessionUpdate");
        if !matches!(kind, Some("agent_thought_chunk" | "usage_update" | "session_info_update" | "config_option_update" | "available_commands_update" | "current_mode_update")) {
            self.close_thought();
        }
        match kind {
            Some("agent_message_chunk") => {
                // Text after a tool call, or under a new message id, is a new message;
                // the answer is the last one (Codex says a warning first).
                let mid = s(u, "messageId").map(str::to_owned);
                // Codex marks its answer (final_answer) apart from what it says first.
                let fin = u.get("_meta").filter(|m| matches!(m, Json::Obj(_))).and_then(|m| m.get("codex")).and_then(|c| s(c, "phase")) == Some("final_answer");
                if self.after_tool || (mid.is_some() && self.message.is_some() && mid != self.message) || (fin && !self.is_final) {
                    self.said.clear();
                    self.after_tool = false;
                }
                self.is_final |= fin;
                if mid.is_some() { self.message = mid; }
                if let Some(c) = u.get("content") { self.append(c); }
                self.phase = KiroPhase::Writing;
            }
            Some("usage_update") => {
                if let (Some(used), Some(size)) = (num(u, "used"), num(u, "size")) {
                    if size > 0.0 { self.set_context(used * 100.0 / size); }
                }
            }
            Some("agent_thought_chunk") => {
                self.phase = KiroPhase::Thinking;
                let t = content_text(u.get("content"));
                self.think(&t);
            }
            Some("plan") => self.phase = KiroPhase::Planning,
            Some("tool_call" | "tool_call_update" | "tool_call_chunk") => {
                if let Some(p) = tool_phase(s(u, "kind"), s(u, "title")) { self.phase = p; }
                if !self.said.is_empty() { self.after_tool = true; }
                self.step(u);
            }
            Some("session_info_update") => {
                // {"_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}
                let kiro = u.get("_meta").filter(|m| matches!(m, Json::Obj(_))).and_then(|m| m.get("kiro")).filter(|k| matches!(k, Json::Obj(_)));
                let pct = kiro.and_then(|k| k.get("contextUsage")).filter(|c| matches!(c, Json::Obj(_))).and_then(|c| num(c, "usagePercentage"));
                if let Some(p) = pct { self.set_context(p); }
                // At a turn's end: {"_meta":{"kiro":{"kind":"turn_completion",
                // "promptTurnSummaries":[{"unit":"credit","usage":0.087}]}}}.
                if let Some(Json::Arr(sums)) = kiro.filter(|k| s(k, "kind") == Some("turn_completion")).and_then(|k| k.get("promptTurnSummaries")) {
                    let credits = sums.iter().filter(|x| s(x, "unit") == Some("credit")).filter_map(|x| num(x, "usage")).reduce(|a, b| a + b);
                    if let Some(spent) = credits { self.events.push(KiroEvent { credits: Some(spent), ..Default::default() }); }
                }
            }
            _ => {}
        }
    }

    fn set_context(&mut self, pct: f64) {
        let v = pct.clamp(0.0, 100.0);
        if self.context.is_none_or(|old| (old - v).abs() >= 0.5) {
            self.context = Some(v);
            self.events.push(KiroEvent { context: Some(v), ..Default::default() });
        }
    }

    /// A tool call starts a step; its updates carry the status. The first one names it.
    /// A tool call starts a step; its updates carry the status, and at the end the
    /// change it made (ACP diff content) or what the command printed (rawOutput).
    fn step(&mut self, u: &Json) {
        let Some(id) = s(u, "toolCallId").filter(|i| !i.is_empty()) else { return };
        let status = s(u, "status");
        let seen = self.steps.contains_key(id);
        let known = match self.steps.get(id) {
            Some(k) => k.clone(),
            None => {
                self.began.insert(id.into(), std::time::Instant::now());
                KiroStep::new(id, s(u, "kind").unwrap_or("other"), s(u, "title").unwrap_or("Working"), target(u), status.unwrap_or("in_progress"))
            }
        };
        let mut next = KiroStep {
            status: status.map_or_else(|| known.status.clone(), str::to_owned),
            title: s(u, "title").map_or_else(|| known.title.clone(), str::to_owned),
            target: known.target.clone().or_else(|| target(u)),
            ..known.clone()
        };
        if let Some((added, removed, preview)) = diff_of(u) { next.added = added; next.removed = removed; next.diff = Some(preview); }
        if next.kind == "execute" {
            let (o, exit) = output_of(u);
            if let Some(o) = o { next.output = Some(o); }
            if exit.is_some() { next.exit = exit; }
        }
        if matches!(next.status.as_str(), "completed" | "failed") && known.ms.is_none() {
            if let Some(t0) = self.began.get(id) { next.ms = Some(t0.elapsed().as_secs_f64() * 1000.0); }
        }
        if seen && next == known { return; }
        self.steps.insert(id.into(), next.clone());
        self.events.push(KiroEvent { step: Some(next), ..Default::default() });
    }

    fn append(&mut self, content: &Json) {
        match content {
            Json::Arr(parts) => for p in parts { self.append(p); },
            _ => if let Some(t) = s(content, "text") {
                self.said.push_str(t);
                // A long run can say a lot; only the end is ever shown.
                let n = units(&self.said);
                if n > SAID_LIMIT {
                    let mut cut = n - SAID_LIMIT;
                    let mut at = 0;
                    for (i, c) in self.said.char_indices() {
                        if cut == 0 { at = i; break; }
                        cut = cut.saturating_sub(c.len_utf16());
                        at = i + c.len_utf8();
                    }
                    self.said.drain(..at);
                }
            },
        }
    }

    /// The run's result once the tool is done.
    pub fn outcome(&self, exit_code: i32, cancelled: bool, stderr: &str) -> KiroResult {
        let said = clip(self.final_text.as_deref().unwrap_or(&self.said).trim(), 20000);
        let r = |state, text: String| KiroResult { state, text, exit_code: Some(exit_code), unconfirmed: false };
        let name = &self.name;
        if cancelled || self.interrupted {
            return r(KiroState::Cancelled, if !said.is_empty() { said } else { format!("Stopped before {name} finished.") });
        }
        if self.stop_reason.as_deref() == Some("refusal") { return r(KiroState::Failed, format!("{name} declined this request.")); }
        if exit_code == 0 && self.error.is_none() {
            return r(KiroState::Completed, if !said.is_empty() { said } else { format!("Done. {name} didn’t leave a summary.") });
        }
        r(KiroState::Failed, self.explain(exit_code, stderr))
    }

    fn explain(&self, exit_code: i32, stderr: &str) -> String {
        let text = strip_ansi(&format!("{stderr}\n{}", self.plain.iter().cloned().collect::<Vec<_>>().join("\n")));
        let lower = text.to_lowercase();
        if ["kiro-cli login", "not logged in", "login required", "authentication"].iter().any(|k| lower.contains(k)) {
            return "Kiro needs you to sign in. Run “kiro-cli login” in a terminal, then try again.".into();
        }
        if let Some(e) = &self.error { return clip(e, 20000); }
        if exit_code == 3 { return "An MCP server Kiro depends on didn’t start.".into(); }
        let lines: Vec<&str> = text.split('\n').map(str::trim).filter(|l| !l.is_empty()).collect();
        if !lines.is_empty() { return clip(&lines[lines.len().saturating_sub(3)..].join("\n"), 600); }
        format!("kiro-cli stopped with exit code {exit_code}.")
    }
}

/// The event's name and payload: {"type": …, "data": {…}} and the like, or an object
/// with one key: {"runFinished": {…}}.
/// The file or command a tool call is about: its first location, else the input's
/// command, path, file_path, pattern, query or url. An empty one is none.
fn target(u: &Json) -> Option<String> {
    let mut t = None;
    if let Some(Json::Arr(locs)) = u.get("locations") {
        for l in locs { t = s(l, "path").map(str::to_owned); if t.as_deref().is_some_and(|x| !x.is_empty()) { break; } }
    }
    if t.as_deref().is_none_or(str::is_empty) {
        if let Some(raw) = u.get("rawInput") {
            t = ["command", "path", "file_path", "pattern", "query", "url"].iter().find_map(|k| s(raw, k)).map(str::to_owned);
        }
    }
    t.filter(|x| !x.is_empty())
}

fn lines_of(t: &str) -> Vec<String> { t.replace('\r', "").trim_end_matches('\n').split('\n').map(str::to_owned).collect() }

/// KiroStream.DiffOf: the change in a tool call's diff content, lines added and
/// removed, and the changed part with a line of context before it (up to PREVIEW
/// lines). When the line numbers are known (the call's location names its line, or
/// the file is new) the part starts with "@@ -old +new @@", the numbers of its first
/// line; a tool that sends only the replaced snippet gives none, and none are made up.
pub fn diff_of(u: &Json) -> Option<(i32, i32, String)> {
    const PREVIEW: usize = 400;
    let Some(Json::Arr(content)) = u.get("content") else { return None };
    let at_line = match u.get("locations") {
        Some(Json::Arr(l)) => l.iter().find_map(|x| match x.get("line") { Some(v @ Json::Num(_)) => v.i64().ok().filter(|n| *n >= 1), _ => None }),
        _ => None,
    };
    let (mut added, mut removed) = (0usize, 0usize);
    let mut lines: Vec<String> = vec![];
    for item in content {
        if s(item, "type") != Some("diff") { continue; }
        let old = s(item, "oldText");
        let new = s(item, "newText").unwrap_or("");
        // Kiro sends an empty diff while the edit is still pending.
        if old.is_none_or(str::is_empty) && new.is_empty() { continue; }
        let a = match old { None | Some("") => vec![], Some(o) => lines_of(o) };
        let bb = lines_of(new);
        // What is the same at both ends is not the change.
        let mut head = 0;
        while head < a.len() && head < bb.len() && a[head] == bb[head] { head += 1; }
        let mut tail = 0;
        while tail < a.len() - head && tail < bb.len() - head && a[a.len() - tail - 1] == bb[bb.len() - tail - 1] { tail += 1; }
        let gone = &a[head..a.len() - tail];
        let came = &bb[head..bb.len() - tail];
        removed += gone.len();
        added += came.len();
        if lines.len() >= PREVIEW { continue; }
        let ctx = head > 0 && !a[head - 1].trim().is_empty();
        let base = if a.is_empty() { Some(1) } else { at_line };
        if let Some(b) = base {
            let first = b + head as i64 - ctx as i64;
            lines.push(format!("@@ -{first} +{first} @@"));
        }
        if ctx { lines.push(format!("  {}", clip(a[head - 1].trim_end(), 160))); }
        lines.extend(gone.iter().take(PREVIEW / 2).map(|x| format!("- {}", clip(x.trim_end(), 160))));
        let room = PREVIEW - lines.len().min(PREVIEW);
        lines.extend(came.iter().take(room).map(|x| format!("+ {}", clip(x.trim_end(), 160))));
    }
    if added + removed == 0 { return None; }
    lines.truncate(PREVIEW);
    Some((added as i32, removed as i32, lines.join("\n")))
}

/// How many lines of a command's output a step keeps (its end).
pub const OUTPUT_LINES: usize = 400;

/// KiroStream.OutputOf: the end of what a command printed, and its exit code, from
/// rawOutput: Kiro's {output, exitCode}, Codex's {formatted_output, exit_code}, or text.
pub fn output_of(u: &Json) -> (Option<String>, Option<i32>) {
    let mut exit = None;
    let mut text: Option<String> = None;
    match u.get("rawOutput") {
        Some(Json::Str(t)) => text = Some(t.clone()),
        Some(ro @ Json::Obj(_)) => {
            text = ["formatted_output", "output", "aggregated_output", "stdout"].iter().find_map(|k| s(ro, k)).map(str::to_owned);
            if let Some(err) = s(ro, "stderr").filter(|e| !e.is_empty()) {
                text = Some(match text { Some(t) if !t.is_empty() => format!("{t}\n{err}"), _ => err.to_owned() });
            }
            for n in ["exitCode", "exit_code"] {
                if let Some(v @ Json::Num(_)) = ro.get(n) { if let Ok(v) = v.i32() { exit = Some(v); } }
            }
        }
        _ => {}
    }
    let Some(text) = text else { return (None, exit) };
    let mut rows: Vec<String> = strip_ansi(&text).replace('\r', "").split('\n').map(|l| l.trim_end().to_owned()).collect();
    while rows.last().is_some_and(String::is_empty) { rows.pop(); }
    while rows.first().is_some_and(String::is_empty) { rows.remove(0); }
    if rows.is_empty() { return (None, exit); }
    let from = rows.len().saturating_sub(OUTPUT_LINES);
    let mut kept: Vec<String> = rows[from..].iter().map(|l| clip(l, 200)).collect();
    // What was cut is said, so the fold never claims it shows everything.
    if from > 0 { kept.insert(0, format!("… {from} earlier line{} not kept", if from == 1 { "" } else { "s" })); }
    (Some(kept.join("\n")), exit)
}

fn envelope(root: &Json) -> (&str, &Json) {
    for key in ["type", "event", "method"] {
        if let Some(name) = s(root, key) {
            for inner in ["data", "payload", "params"] {
                if let Some(b @ Json::Obj(_)) = root.get(inner) { return (name, b); }
            }
            return (name, root);
        }
    }
    if let Json::Obj(p) = root {
        if p.len() == 1 && matches!(p[0].1, Json::Obj(_)) { return (&p[0].0, &p[0].1); }
    }
    ("", root)
}

fn find_update(e: &Json, depth: usize) -> Option<&Json> {
    let Json::Obj(props) = e else { return None };
    if depth > 5 { return None; }
    if s(e, "sessionUpdate").is_some() { return Some(e); }
    props.iter().find_map(|(_, v)| find_update(v, depth + 1))
}

fn message(e: &Json) -> Option<String> {
    if !matches!(e, Json::Obj(_)) { return None; }
    if let Some(m) = s(e, "message").filter(|m| !m.is_empty()) { return Some(m.into()); }
    match e.get("error") {
        Some(Json::Str(x)) if !x.is_empty() => Some(x.clone()),
        Some(err) => message(err),
        None => None,
    }
}

/// ACP's tool kinds, with the title for a tool that gives none.
pub fn tool_phase(kind: Option<&str>, title: Option<&str>) -> Option<KiroPhase> {
    match kind {
        Some("read") => return Some(KiroPhase::Reading),
        Some("edit" | "delete" | "move") => return Some(KiroPhase::Editing),
        Some("execute") => return Some(KiroPhase::Running),
        Some("search" | "fetch") => return Some(KiroPhase::Searching),
        Some("think") => return Some(KiroPhase::Thinking),
        _ => {}
    }
    let t = title.unwrap_or("").to_lowercase();
    if t.is_empty() { return kind.map(|_| KiroPhase::Working); }
    // An MCP tool is only itself: Kiro titles it "@playwriter/execute", whose
    // "playwriter" held "write" and read as Editing.
    if crate::words::mcp_name(&t).is_some() { return Some(KiroPhase::Working); }
    // Whole words, so a name that only contains one ("playwriter", "rerun") isn't it.
    let words: Vec<&str> = t.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    let has = |ks: &[&str]| words.iter().any(|w| ks.contains(w));
    // "Create" is a write only of a file or a folder; an MCP's create_entities isn't.
    let creates = has(&["create", "creates", "creating"]) && has(&["file", "files", "folder", "directory"]);
    Some(if has(&["read", "reads", "reading"]) { KiroPhase::Reading }
        else if creates || has(&["write", "writes", "writing", "edit", "edits", "editing", "replace", "replacing"]) { KiroPhase::Editing }
        else if has(&["grep", "glob", "search", "searching", "find", "finding", "fetch", "fetching"]) { KiroPhase::Searching }
        else if has(&["shell", "bash", "command", "run", "runs", "running"]) { KiroPhase::Running }
        else { KiroPhase::Working })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(u: &str) -> String { format!(r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{u}}}}}"#) }

    /// What KiroStream makes of ACP updates (the cases KiroRunnerTests covers, derived
    /// from the C# source where no test pins them).
    /// KiroStream.Step's details, from the C# (no C# test covers them): an edit's
    /// counts and preview with its line of context, a command's output tail and exit
    /// code, and a step that ends carries how long it took.
    #[test]
    fn a_step_carries_its_change_or_its_output() {
        let mut k = KiroStream::new("Kiro");
        k.feed(&update(r#"{"sessionUpdate":"tool_call","toolCallId":"e","kind":"edit","title":"Edit","status":"pending","content":[{"type":"diff","path":"a.rs","oldText":"","newText":""}]}"#));
        k.feed(&update(r#"{"sessionUpdate":"tool_call_update","toolCallId":"e","status":"completed","content":[{"type":"diff","path":"a.rs","oldText":"fn a() {\n    one();\n}\n","newText":"fn a() {\n    two();\n    three();\n}\n"}]}"#));
        k.feed(&update(r#"{"sessionUpdate":"tool_call","toolCallId":"x","kind":"execute","title":"Run","status":"in_progress","rawInput":{"command":"cargo test"}}"#));
        k.feed(&update(r#"{"sessionUpdate":"tool_call_update","toolCallId":"x","status":"failed","rawOutput":{"formatted_output":"\n\u001b[32mok\u001b[0m\r\nFAILED   \n\n","exit_code":101}}"#));
        let steps: Vec<KiroStep> = k.drain().into_iter().filter_map(|e| e.step).collect();
        assert_eq!(steps[0].diff, None, "an empty pending diff is no change");
        let e = &steps[1];
        assert_eq!((e.added, e.removed, e.diff.as_deref()), (2, 1, Some("  fn a() {\n-     one();\n+     two();\n+     three();")));
        assert!(e.ms.is_some());
        let x = steps.last().unwrap();
        assert_eq!((x.target.as_deref(), x.output.as_deref(), x.exit, x.status.as_str()), (Some("cargo test"), Some("ok\nFAILED"), Some(101), "failed"));
        // The same update again is no news.
        k.feed(&update(r#"{"sessionUpdate":"tool_call_update","toolCallId":"x","status":"failed"}"#));
        assert!(k.drain().is_empty());
    }

    #[test]
    fn reads_steps_phases_context_and_the_last_message() {
        let mut k = KiroStream::new("Codex");
        assert_eq!(k.feed(&update(r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}"#)), Some(KiroPhase::Thinking));
        let first = k.drain();
        assert_eq!(first.iter().filter_map(|e| e.step.as_ref()).map(|s| (s.kind.as_str(), s.output.as_deref())).collect::<Vec<_>>(), vec![("thought", Some("hm"))]);
        assert_eq!(first.iter().filter_map(|e| e.session_id.clone()).collect::<Vec<_>>(), vec!["s1".to_string()]);
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","content":{"type":"text","text":"Warning first. "}}"#));
        assert_eq!(k.feed(&update(r#"{"sessionUpdate":"tool_call","toolCallId":"t0","kind":"read","title":"Read","status":"in_progress","locations":[{"path":"src/a.cs"}]}"#)), Some(KiroPhase::Reading));
        k.feed(&update(r#"{"sessionUpdate":"tool_call_update","toolCallId":"t0","status":"in_progress"}"#));
        k.feed(&update(r#"{"sessionUpdate":"tool_call_update","toolCallId":"t0","status":"completed","title":"Read a.cs"}"#));
        k.feed(&update(r#"{"sessionUpdate":"tool_call","toolCallId":"t1","title":"Run shell","rawInput":{"command":"npm test"}}"#));
        k.feed(&update(r#"{"sessionUpdate":"usage_update","used":1000,"size":200000}"#));
        k.feed(&update(r#"{"sessionUpdate":"usage_update","used":1400,"size":200000}"#));
        k.feed(&update(r#"{"sessionUpdate":"usage_update","used":3000,"size":200000}"#));
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","content":[{"type":"text","text":"The "},{"type":"text","text":"answer."}]}"#));
        let ev = k.drain();
        let steps: Vec<_> = ev.iter().filter_map(|e| e.step.as_ref()).filter(|s| s.kind != "thought").map(|s| (s.id.as_str(), s.title.as_str(), s.status.as_str(), s.target.as_deref(), s.kind.as_str())).collect();
        assert_eq!(steps, [("t0", "Read", "in_progress", Some("src/a.cs"), "read"), ("t0", "Read a.cs", "completed", Some("src/a.cs"), "read"),
            ("t1", "Run shell", "in_progress", Some("npm test"), "other")]);
        let ctx: Vec<f64> = ev.iter().filter_map(|e| e.context).collect();
        assert_eq!(ctx, [0.5, 1.5], "0.7 is within half a point of 0.5");
        assert_eq!(k.said(), "The answer.");
        assert_eq!(k.phase, KiroPhase::Writing);
        assert_eq!(k.outcome(0, false, "").text, "The answer.");
    }

    #[test]
    fn codex_final_answer_and_message_ids_start_new_messages() {
        let mut k = KiroStream::new("Codex");
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"a"}}"#));
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","messageId":"m1","content":{"type":"text","text":"b"}}"#));
        assert_eq!(k.said(), "ab");
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","messageId":"m2","content":{"type":"text","text":"c"}}"#));
        assert_eq!(k.said(), "c");
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","messageId":"m2","_meta":{"codex":{"phase":"final_answer"}},"content":{"type":"text","text":"F"}}"#));
        k.feed(&update(r#"{"sessionUpdate":"agent_message_chunk","messageId":"m2","_meta":{"codex":{"phase":"final_answer"}},"content":{"type":"text","text":"G"}}"#));
        assert_eq!(k.said(), "FG");
        k.feed(&update(r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}"#));
        assert_eq!(k.context, Some(3.37));
        k.drain();
        k.feed(&update(r#"{"sessionUpdate":"session_info_update","_meta":{"kiro":{"kind":"turn_completion","promptTurnSummaries":[{"unit":"credit","usage":0.087},{"unit":"token","usage":900},{"unit":"credit","usage":0.013}]}}}"#));
        assert_eq!(k.drain().iter().filter_map(|e| e.credits).map(|c| (c * 1000.0).round()).collect::<Vec<_>>(), vec![100.0]);
    }

    #[test]
    fn outcomes_as_kiro_stream_gives_them() {
        let mut k = KiroStream::new("Kiro");
        assert_eq!(k.outcome(0, false, "").text, "Done. Kiro didn’t leave a summary.");
        assert_eq!(k.outcome(0, true, "").text, "Stopped before Kiro finished.");
        assert_eq!(k.outcome(1, false, "Error: not logged in").text, "Kiro needs you to sign in. Run “kiro-cli login” in a terminal, then try again.");
        assert_eq!(k.outcome(3, false, "").text, "An MCP server Kiro depends on didn’t start.");
        k.feed("plain line one");
        k.feed("\u{1b}[31mred\u{1b}[0m line");
        assert_eq!(k.outcome(2, false, "").text, "plain line one\nred line");
        k.feed(r#"{"type":"runError","data":{"error":{"message":"boom"}}}"#);
        assert_eq!(k.outcome(0, false, "").state, KiroState::Failed);
        assert_eq!(k.outcome(0, false, "").text, "boom");
        let mut r = KiroStream::new("Cursor");
        r.feed(r#"{"runFinished":{"stopReason":"refusal"}}"#);
        assert_eq!(r.outcome(0, false, "").text, "Cursor declined this request.");
        assert!(r.finished);
        let big = "x".repeat(20005);
        let mut c = KiroStream::new("Kiro");
        c.feed(&format!(r#"{{"finalText":"{big}"}}"#));
        assert_eq!(units(&c.outcome(0, false, "").text), 20001);
    }

    #[test]
    fn tool_phases_follow_kind_then_title() {
        assert_eq!(tool_phase(Some("move"), None), Some(KiroPhase::Editing));
        assert_eq!(tool_phase(Some("other"), None), Some(KiroPhase::Working));
        assert_eq!(tool_phase(None, None), None);
        assert_eq!(tool_phase(None, Some("Grep files")), Some(KiroPhase::Searching));
        assert_eq!(tool_phase(None, Some("Create file")), Some(KiroPhase::Editing));
        assert_eq!(tool_phase(Some("x"), Some("Bash")), Some(KiroPhase::Running));
        // Titles Kiro gave kind "other" steps in a real history.
        assert_eq!(tool_phase(Some("other"), Some("@playwriter/execute")), Some(KiroPhase::Working));
        assert_eq!(tool_phase(Some("other"), Some("Read File")), Some(KiroPhase::Reading));
        assert_eq!(tool_phase(Some("other"), Some("Write File")), Some(KiroPhase::Editing));
        assert_eq!(tool_phase(Some("other"), Some("Loaded skill: unslop")), Some(KiroPhase::Working));
        assert_eq!(tool_phase(Some("other"), Some("Update Session Information")), Some(KiroPhase::Working));
        // A word inside a name is not that word.
        assert_eq!(tool_phase(Some("other"), Some("browser_type")), Some(KiroPhase::Working));
        assert_eq!(tool_phase(Some("other"), Some("create_entities")), Some(KiroPhase::Working));
        assert_eq!(tool_phase(Some("other"), Some("Rerun the build")), Some(KiroPhase::Working));
    }

    /// Reasoning the tool sends is kept, in order among the tool calls, as one thought
    /// per run of chunks, closed when the agent does something else, with its time.
    #[test]
    fn thoughts_are_kept_in_order_and_closed_by_what_follows() {
        let mut k = KiroStream::new("Kiro");
        let th = |t: &str| update(&format!(r#"{{"sessionUpdate":"agent_thought_chunk","content":{{"type":"text","text":"{t}"}}}}"#));
        k.feed(&th(" "));
        assert!(k.drain().iter().all(|e| e.step.is_none()), "blank reasoning starts no thought");
        k.feed(&th("First "));
        k.feed(&th("idea."));
        k.feed(&update(r#"{"sessionUpdate":"tool_call","toolCallId":"r","kind":"read","title":"Read","status":"in_progress"}"#));
        k.feed(&th("Second."));
        k.feed(&update(r#"{"sessionUpdate":"usage_update","used":1,"size":10}"#));
        k.feed(&th(" More."));
        k.end();
        let mut last: Vec<KiroStep> = vec![];
        for e in k.drain() { if let Some(s) = e.step { match last.iter().position(|x| x.id == s.id) { Some(i) => last[i] = s, None => last.push(s) } } }
        let got: Vec<_> = last.iter().map(|s| (s.kind.as_str(), s.status.as_str(), s.output.as_deref())).collect();
        assert_eq!(got, vec![("thought", "completed", Some("First idea.")), ("read", "in_progress", None), ("thought", "completed", Some("Second. More."))]);
        assert!(last[0].ms.is_some() && last[2].ms.is_some());
    }

    #[test]
    fn a_diff_says_its_line_numbers_only_when_it_knows_them() {
        let call = |loc: &str, old: &str| json::parse(&format!(r#"{{"locations":[{loc}],"content":[{{"type":"diff","path":"a","oldText":{old},"newText":"a\nB\nc\n"}}]}}"#)).unwrap();
        assert_eq!(diff_of(&call(r#"{"path":"a","line":40}"#, r#""a\nb\nc\n""#)).unwrap().2, "@@ -40 +40 @@\n  a\n- b\n+ B");
        assert_eq!(diff_of(&call(r#"{"path":"a"}"#, r#""a\nb\nc\n""#)).unwrap().2, "  a\n- b\n+ B", "a snippet's own numbers aren't the file's");
        assert!(diff_of(&call(r#"{"path":"a"}"#, "null")).unwrap().2.starts_with("@@ -1 +1 @@\n+ a"), "a new file starts at 1");
        let long: String = (0..450).map(|i| format!("line {i}\\n")).collect();
        let (o, _) = output_of(&json::parse(&format!(r#"{{"rawOutput":"{long}"}}"#)).unwrap());
        let o = o.unwrap();
        assert!(o.starts_with("… 50 earlier lines not kept\nline 50"));
        assert_eq!(o.lines().count(), OUTPUT_LINES + 1);
    }

    #[test]
    fn what_is_said_keeps_its_last_64k() {
        let mut k = KiroStream::new("Kiro");
        let chunk = "é".repeat(40_000);
        for _ in 0..2 { k.feed(&update(&format!(r#"{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{chunk}"}}}}"#))); }
        assert_eq!(units(k.said()), 64 * 1024);
    }
}

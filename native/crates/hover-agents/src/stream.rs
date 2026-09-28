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
pub struct KiroResult { pub state: KiroState, pub text: String, pub exit_code: Option<i32> }

impl KiroResult {
    pub fn new(state: KiroState, text: impl Into<String>) -> KiroResult { KiroResult { state, text: text.into(), exit_code: None } }
}

/// Detail from a run as it goes: a step that started or ended, the context (0 to
/// 100), the tool's session id.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct KiroEvent { pub step: Option<KiroStep>, pub context: Option<f64>, pub session_id: Option<String> }

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
    after_tool: bool,
    message: Option<String>,
    is_final: bool,
}

fn s<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }

fn num(e: &Json, name: &str) -> Option<f64> { match e.get(name) { Some(v @ Json::Num(_)) => v.f64().ok(), _ => None } }

impl KiroStream {
    pub fn new(name: &str) -> KiroStream {
        KiroStream { name: name.into(), phase: KiroPhase::Starting, final_text: None, stop_reason: None, error: None, interrupted: false, finished: false,
            session_id: None, context: None, said: String::new(), plain: Default::default(), events: vec![], steps: Default::default(), after_tool: false,
            message: None, is_final: false }
    }

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
        match s(u, "sessionUpdate") {
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
            Some("agent_thought_chunk") => self.phase = KiroPhase::Thinking,
            Some("plan") => self.phase = KiroPhase::Planning,
            Some("tool_call" | "tool_call_update" | "tool_call_chunk") => {
                if let Some(p) = tool_phase(s(u, "kind"), s(u, "title")) { self.phase = p; }
                if !self.said.is_empty() { self.after_tool = true; }
                self.step(u);
            }
            Some("session_info_update") => {
                // {"_meta":{"kiro":{"contextUsage":{"usagePercentage":3.37}}}}
                let pct = u.get("_meta").filter(|m| matches!(m, Json::Obj(_))).and_then(|m| m.get("kiro")).filter(|k| matches!(k, Json::Obj(_)))
                    .and_then(|k| k.get("contextUsage")).filter(|c| matches!(c, Json::Obj(_))).and_then(|c| num(c, "usagePercentage"));
                if let Some(p) = pct { self.set_context(p); }
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
    fn step(&mut self, u: &Json) {
        let Some(id) = s(u, "toolCallId").filter(|i| !i.is_empty()) else { return };
        let status = s(u, "status").unwrap_or("in_progress").to_owned();
        let step = match self.steps.get(id) {
            Some(known) => {
                if known.status == status { return; }
                KiroStep { status, title: s(u, "title").map_or_else(|| known.title.clone(), str::to_owned), ..known.clone() }
            }
            None => {
                let mut target = None;
                if let Some(Json::Arr(locs)) = u.get("locations") {
                    for l in locs { target = s(l, "path").map(str::to_owned); if target.is_some() { break; } }
                }
                if target.is_none() {
                    if let Some(raw) = u.get("rawInput") {
                        target = ["command", "path", "pattern", "query", "url"].iter().find_map(|k| s(raw, k)).map(str::to_owned);
                    }
                }
                KiroStep { id: id.into(), kind: s(u, "kind").unwrap_or("other").into(), title: s(u, "title").unwrap_or("Working").into(), target, status }
            }
        };
        self.steps.insert(id.into(), step.clone());
        self.events.push(KiroEvent { step: Some(step), ..Default::default() });
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
        let r = |state, text: String| KiroResult { state, text, exit_code: Some(exit_code) };
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
    let has = |ks: &[&str]| ks.iter().any(|k| t.contains(k));
    Some(if has(&["read"]) { KiroPhase::Reading }
        else if has(&["write", "edit", "replace", "creat"]) { KiroPhase::Editing }
        else if has(&["grep", "glob", "search", "find", "fetch"]) { KiroPhase::Searching }
        else if has(&["shell", "bash", "command", "run"]) { KiroPhase::Running }
        else { KiroPhase::Working })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(u: &str) -> String { format!(r#"{{"jsonrpc":"2.0","method":"session/update","params":{{"sessionId":"s1","update":{u}}}}}"#) }

    /// What KiroStream makes of ACP updates (the cases KiroRunnerTests covers, derived
    /// from the C# source where no test pins them).
    #[test]
    fn reads_steps_phases_context_and_the_last_message() {
        let mut k = KiroStream::new("Codex");
        assert_eq!(k.feed(&update(r#"{"sessionUpdate":"agent_thought_chunk","content":{"type":"text","text":"hm"}}"#)), Some(KiroPhase::Thinking));
        assert_eq!(k.drain(), vec![KiroEvent { session_id: Some("s1".into()), ..Default::default() }]);
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
        let steps: Vec<_> = ev.iter().filter_map(|e| e.step.as_ref()).map(|s| (s.id.as_str(), s.title.as_str(), s.status.as_str(), s.target.as_deref(), s.kind.as_str())).collect();
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
    }

    #[test]
    fn what_is_said_keeps_its_last_64k() {
        let mut k = KiroStream::new("Kiro");
        let chunk = "é".repeat(40_000);
        for _ in 0..2 { k.feed(&update(&format!(r#"{{"sessionUpdate":"agent_message_chunk","content":{{"type":"text","text":"{chunk}"}}}}"#))); }
        assert_eq!(units(k.said()), 64 * 1024);
    }
}

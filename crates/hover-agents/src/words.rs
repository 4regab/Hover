//! Owl/KiroSession.cs's AgentWords: what the notch and the office say about a session
//! in a few words: what its agent is doing (a verb, and the file or command it is
//! about), and what it asks for.

use crate::ask::AgentAsk;
use crate::session::KiroSession;
use crate::stream::{clip_to, tool_phase, KiroPhase};
use hover_core::model::KiroState;

/// Path.GetFileName: after the last separator (both kinds on Windows, '/' elsewhere).
fn file_name(p: &str) -> &str {
    #[cfg(windows)]
    let at = p.rfind(['\\', '/']);
    #[cfg(not(windows))]
    let at = p.rfind('/');
    at.map_or(p, |i| &p[i + 1..])
}

/// The verb and its object: ("Editing", "refresh.ts"), ("Running", "npm test"),
/// ("Thinking", "").
pub fn activity(s: &KiroSession) -> (&'static str, String) {
    if s.state != KiroState::Running {
        return (match s.state {
            KiroState::Completed => "Done", KiroState::Failed => "Couldn’t finish", KiroState::Cancelled => "Stopped", _ => "Ready",
        }, String::new());
    }
    if s.phase == KiroPhase::Starting { return ("Waking up", String::new()); }
    let steps = s.current().map(|t| &t.steps);
    let mut step = steps.and_then(|st| st.iter().rev().find(|x| matches!(x.status.as_str(), "in_progress" | "pending")));
    if step.is_none() && matches!(s.phase, KiroPhase::Reading | KiroPhase::Searching | KiroPhase::Editing | KiroPhase::Running) {
        step = steps.and_then(|st| st.last());
    }
    let Some(step) = step else {
        return (match s.phase {
            KiroPhase::Thinking => "Thinking", KiroPhase::Planning => "Making a plan", KiroPhase::Writing => "Writing it up", _ => "Working",
        }, String::new());
    };
    let verb = match step.kind.as_str() {
        "read" => "Reading", "edit" => "Editing", "delete" => "Deleting", "move" => "Moving", "execute" => "Running",
        "search" => "Searching", "fetch" => "Fetching", "think" => "Thinking",
        _ => {
            if let Some(name) = mcp_name(&step.title) { return ("Using", clip_to(&name, 28)); }
            match tool_phase(Some(&step.kind), Some(&step.title)) {
                Some(KiroPhase::Reading) => "Reading", Some(KiroPhase::Editing) => "Editing", Some(KiroPhase::Running) => "Running",
                Some(KiroPhase::Searching) => "Searching",
                _ => {
                    // The tool's own title says more than "Working" ("Loaded skill: unslop",
                    // "Serve the mockup on localhost"); a many-line one is a message, not a name.
                    let t = step.title.trim();
                    if !t.is_empty() && t != "Working" && !t.contains('\n') { return ("Working on", clip_to(t, 28)); }
                    "Working"
                }
            }
        }
    };
    (verb, short(step.target.as_deref()).unwrap_or_default())
}

/// An MCP tool call's name from its title, as "server: tool": Kiro titles one
/// "@playwriter/execute" (seen in a real history); Cursor "MCP: tool" (its forum's report).
pub(crate) fn mcp_name(title: &str) -> Option<String> {
    let t = title.trim();
    if let Some((server, tool)) = t.strip_prefix('@').and_then(|r| r.split_once('/')) {
        let plain = |x: &str| !x.is_empty() && !x.contains(char::is_whitespace);
        return (plain(server) && plain(tool)).then(|| format!("{server}: {tool}"));
    }
    t.strip_prefix("MCP: ").map(str::trim).filter(|x| !x.is_empty()).map(str::to_owned)
}

/// A file's name, or a command's program and first word, short enough for the notch.
pub fn short(target: Option<&str>) -> Option<String> {
    let target = target.filter(|t| !t.trim().is_empty())?;
    let t = target.trim().replace('\n', " ");
    if t.contains(' ') {
        let words: Vec<&str> = t.split(' ').filter(|w| !w.is_empty()).collect();
        let head = format!("{}{}", file_name(words[0].trim_matches(['"', '\''])), words.get(1).map_or(String::new(), |w| format!(" {w}")));
        return Some(clip_to(&head, 26));
    }
    let slashed = t.trim_end_matches(['\\', '/']).replace('\\', "/");
    let mut name = file_name(&slashed).to_owned();
    if name.is_empty() { name = t.clone(); }
    Some(clip_to(&name, 28))
}

/// The question in one line: ("Wants to run", "npm install").
pub fn ask_line(a: &AgentAsk) -> (&'static str, String) {
    let or = |p: &Option<String>, d: &str| short(p.as_deref()).unwrap_or_else(|| d.into());
    match a.kind.as_str() {
        "question" => ("Asks you", a.title.clone()),
        "execute" => ("Wants to run", or(&a.command, "a command")),
        "edit" => ("Wants to edit", or(&a.path, "a file")),
        "delete" => ("Wants to delete", or(&a.path, "files")),
        "move" => ("Wants to move", or(&a.path, "files")),
        "fetch" => ("Wants to go online", String::new()),
        _ => ("Wants to use", a.title.clone()),
    }
}

/// The question as its card's title.
pub fn ask_title(a: &AgentAsk) -> String {
    let p = |d: &str| short(a.path.as_deref()).unwrap_or_else(|| d.into());
    match a.kind.as_str() {
        "question" => if a.questions.as_ref().is_some_and(|q| q.len() > 1) { format!("Asks you {} questions", a.questions.as_ref().unwrap().len()) } else { "Asks you a question".into() },
        "execute" => "Wants to run a command".into(),
        "edit" => format!("Wants to edit {}", p("a file")),
        "delete" => format!("Wants to delete {}", p("files")),
        "move" => format!("Wants to move {}", p("files")),
        "fetch" => "Wants to use the network".into(),
        _ => format!("Wants to use {}", a.title),
    }
}

/// The word on the button that allows it.
pub fn ask_allow(a: &AgentAsk) -> &'static str {
    match a.kind.as_str() { "execute" => "Run", "edit" => "Allow edit", "delete" => "Delete", "move" => "Move", "question" => "Answer", _ => "Allow" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_a_file_or_a_command() {
        assert_eq!(short(Some("src/app/refresh.ts")).as_deref(), Some("refresh.ts"));
        assert_eq!(short(Some("npm test --watch")).as_deref(), Some("npm test"));
        assert_eq!(short(Some("/usr/bin/cargo build")).as_deref(), Some("cargo build"));
        assert_eq!(short(Some("src/deep/")).as_deref(), Some("deep"));
        assert_eq!(short(Some("  ")), None);
        assert_eq!(short(Some("a-really-long-file-name-for-the-notch.rs")).as_deref(), Some("a-really-long-file-name-for…"));
    }
}

//! Asking before acting (Services/AcpHost.cs's Permission, Describe and NeedsAsking;
//! KiroRunner.cs's AgentAsk and AskAnswer): what a tool call an agent wants to make
//! looks like to the user, and which calls wait for them under each setting.

use hover_core::json::Json;
use hover_core::model::AgentApproval;
use std::sync::LazyLock;

/// A tool call an agent is waiting on the user for (ACP session/request_permission),
/// told the way the notch and the office show it. Kind is ACP's (execute, edit,
/// delete...). Command is the command line, path the file (relative to the folder when
/// inside it), preview a few lines of the change with +/- before each, added and
/// removed how many lines it changes. Reason is Hover's own few words on why it asks;
/// danger marks what can't be taken back easily.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentAsk {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub command: Option<String>,
    pub path: Option<String>,
    pub preview: Option<String>,
    pub added: i32,
    pub removed: i32,
    pub reason: String,
    pub danger: bool,
    /// A question for the user to answer (kind "question"), not a tool call to allow.
    pub questions: Option<Vec<AgentQuestion>>,
}

impl AgentAsk {
    /// A question for the user to answer, not a tool call to allow.
    pub fn is_question(&self) -> bool { self.questions.as_ref().is_some_and(|q| !q.is_empty()) }
}

/// One question an agent asks the user (OpenCode's question tool): a short header,
/// the question, its choices (label, description), whether several may be picked, and
/// whether the user may type an answer of their own.
#[derive(Clone, Debug, PartialEq)]
pub struct AgentQuestion {
    pub header: String,
    pub question: String,
    pub options: Vec<(String, String)>,
    pub multiple: bool,
    pub custom: bool,
}

/// The answer to a question: each question's picked (or typed) labels, in order;
/// none when the user skipped it or it was withdrawn.
pub type Answers = Option<Vec<Vec<String>>>;

/// The user's answer. Trust allows this one and the same again for the rest of the
/// session; TrustAll allows everything the session asks from now on.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AskAnswer { Allow, Trust, TrustAll, Deny }

/// Whether a tool call of this kind waits for the user under this setting.
pub fn needs_asking(approval: AgentApproval, kind: &str, outside: bool) -> bool {
    let quiet = matches!(kind, "read" | "search" | "think" | "switch_mode");
    match approval {
        AgentApproval::Autopilot => false,
        AgentApproval::Risky => if quiet { false } else if kind == "edit" { outside } else { true },
        AgentApproval::Always => !quiet,
    }
}

/// What "the same again" means for Trust: the kind, and the command, the file or the title.
pub fn key(a: &AgentAsk) -> String { format!("{}:{}", a.kind, a.command.as_deref().or(a.path.as_deref()).unwrap_or(&a.title)) }

static DESTRUCTIVE: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(
    r"(?i)(^|[\s;&|(])(rm|rmdir|del|erase|rd|remove-item|format|mkfs|shutdown|git\s+(push|reset|clean|checkout\s+--))\b").unwrap());
static NETWORK: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(
    r"(?i)\b((npm|pnpm|yarn|bun|pip|pip3|uv|cargo|dotnet|nuget|gem|go)\s+(i|install|add|restore|get|update|upgrade)|curl|wget|invoke-webrequest|iwr|git\s+(push|pull|fetch|clone))\b").unwrap());
static SH: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(r"(?s)^(ba|z|)sh\s+-l?c\s+(.+)$").unwrap());
static PWSH: LazyLock<fancy_regex::Regex> = LazyLock::new(|| fancy_regex::Regex::new(
    r#"(?si)^"?[^"]*?(pwsh|powershell)(\.exe)?"?\s+(-NoProfile\s+)?-(Command|c)\s+(.+)$"#).unwrap());

fn is_match(r: &fancy_regex::Regex, s: &str) -> bool { r.is_match(s).unwrap_or(false) }

/// AcpHost.Destructive: a command that can delete or overwrite things.
pub fn destructive(command: &str) -> bool { is_match(&DESTRUCTIVE, command) }
/// AcpHost.Network: a command that installs packages or uses the network.
pub fn network(command: &str) -> bool { is_match(&NETWORK, command) }
/// Path.GetFullPath, lexically (no disk).
pub fn full(p: &str) -> String { full_path(p) }

fn s<'a>(e: &'a Json, name: &str) -> Option<&'a str> { e.get(name).and_then(Json::as_str) }

/// AcpHost.Clip: the first max − 1 UTF-16 units and an ellipsis.
fn clip(t: &str, max: usize) -> String { crate::stream::clip_to(t, max) }

/// .NET's Trim('\'', '"') on both ends.
fn unquote(t: &str) -> &str { t.trim_matches(|c| c == '\'' || c == '"') }

/// Path.GetFullPath's lexical part: `.` and `..` resolved, separators made one kind.
fn full_path(p: &str) -> String {
    #[cfg(windows)]
    let (p, sep) = (p.replace('/', "\\"), '\\');
    #[cfg(not(windows))]
    let (p, sep) = (p.to_owned(), '/');
    #[allow(unused_mut)]
    let mut prefix = String::new();
    #[allow(unused_mut)]
    let mut rest = p.as_str();
    #[cfg(windows)]
    if rest.len() >= 2 && rest.as_bytes()[1] == b':' { prefix = rest[..2].to_owned(); rest = &rest[2..]; }
    let mut parts: Vec<&str> = vec![];
    for part in rest.split(sep) {
        match part { "" | "." => {} ".." => { parts.pop(); } x => parts.push(x) }
    }
    format!("{prefix}{sep}{}", parts.join(&sep.to_string()))
}

/// AcpHost.Describe: the tool call as the user is asked about it, and whether the file
/// it names is outside the session's folder.
pub fn describe(call: &Json, kind: &str, folder: &str) -> (AgentAsk, bool) {
    let title = s(call, "title").filter(|t| !t.is_empty()).unwrap_or("Use a tool").to_owned();
    let raw = call.get("rawInput").filter(|r| matches!(r, Json::Obj(_)));
    let mut command: Option<String> = None;
    if let Some(raw) = raw {
        for name in ["command", "cmd"] {
            if let Some(cv) = raw.get(name) {
                command = match cv {
                    Json::Str(t) => Some(t.clone()),
                    Json::Arr(xs) => Some(xs.iter().filter_map(Json::as_str).collect::<Vec<_>>().join(" ")),
                    _ => None,
                };
                if command.as_deref().is_some_and(|c| !c.is_empty()) { break; }
            }
        }
        // Codex sends ["bash", "-lc", "the command"]; the command is what matters.
        if let Some(c) = &command {
            if let Ok(Some(m)) = SH.captures(c) { command = Some(unquote(m.get(2).unwrap().as_str().trim()).to_owned()); }
        }
        // On Windows it wraps it in "…\pwsh.exe" [-NoProfile] -Command "the command".
        if let Some(c) = &command {
            if let Ok(Some(m)) = PWSH.captures(c) { command = Some(unquote(m.get(5).unwrap().as_str().trim()).to_owned()); }
        }
    }
    // Cursor's question carries no input; its title is the command, in backticks.
    if command.is_none() && kind == "execute" && crate::stream::units(&title) > 2 && title.starts_with('`') && title.ends_with('`') {
        command = Some(title[1..title.len() - 1].to_owned());
    }
    let mut path: Option<String> = None;
    if let Some(Json::Arr(locs)) = call.get("locations") {
        for l in locs { path = s(l, "path").map(str::to_owned); if path.is_some() { break; } }
    }
    if path.is_none() { path = raw.and_then(|r| ["path", "file_path", "filePath"].iter().find_map(|k| s(r, k))).map(str::to_owned); }

    // A change comes with its old and new text (ACP diff content).
    let (mut added, mut removed) = (0usize, 0usize);
    let mut preview: Vec<String> = vec![];
    if let Some(Json::Arr(content)) = call.get("content") {
        for item in content {
            if s(item, "type") != Some("diff") { continue; }
            if path.is_none() { path = s(item, "path").map(str::to_owned); }
            let split = |t: &str| t.replace('\r', "").split('\n').map(str::to_owned).collect::<Vec<_>>();
            let before = if s(item, "oldText").is_none() { vec![] } else { split(s(item, "oldText").unwrap_or("")) };
            let after = split(s(item, "newText").unwrap_or(""));
            // List.Remove: each line of the other side takes out one equal line.
            let mut gone = before.clone();
            for line in &after { if let Some(i) = gone.iter().position(|x| x == line) { gone.remove(i); } }
            let mut came = after.clone();
            for line in &before { if let Some(i) = came.iter().position(|x| x == line) { came.remove(i); } }
            removed += gone.len();
            added += came.len();
            preview.extend(gone.iter().filter(|x| !x.trim().is_empty()).take(3).map(|x| format!("- {}", clip(x.trim(), 110))));
            let room = 6 - preview.len().min(3);
            preview.extend(came.iter().filter(|x| !x.trim().is_empty()).take(room).map(|x| format!("+ {}", clip(x.trim(), 110))));
        }
    }

    let mut outside = false;
    if let Some(p) = path.clone().filter(|p| !p.is_empty()) {
        let full = if crate::fully_qualified(&p) { full_path(&p) } else { full_path(&format!("{folder}{}{p}", std::path::MAIN_SEPARATOR)) };
        let root = format!("{}{}", full_path(folder).trim_end_matches(['/', '\\']), std::path::MAIN_SEPARATOR);
        if full.len() >= root.len() && full.is_char_boundary(root.len()) && full[..root.len()].eq_ignore_ascii_case(&root) {
            path = Some(full[root.len()..].replace('\\', "/"));
        } else {
            outside = true;
        }
    }

    let danger = kind == "delete" || command.as_deref().is_some_and(|c| is_match(&DESTRUCTIVE, c));
    let n = added + removed;
    let mut reason: String = match kind {
        "execute" => if danger { "Can delete or overwrite things".into() }
            else if command.as_deref().is_some_and(|c| is_match(&NETWORK, c)) { "Installs packages or uses the network".into() }
            else { "Runs a command".into() },
        "delete" => "Deletes files".into(),
        "move" => "Moves or renames files".into(),
        "fetch" => "Uses the network".into(),
        "edit" => if outside { "Edits a file outside the folder".into() } else if n > 0 { format!("Changes {n} line{}", if n == 1 { "" } else { "s" }) } else { "Edits a file".into() },
        _ => "Uses a tool".into(),
    };
    if outside && kind != "edit" { reason.push_str(" · outside the folder"); }
    let id = s(call, "toolCallId").filter(|t| !t.is_empty()).map_or_else(hover_core::guid_n, str::to_owned);
    let ask = AgentAsk {
        id, kind: kind.into(), title, command: command.filter(|c| !c.is_empty()).map(|c| clip(&c, 400)), path,
        preview: (!preview.is_empty()).then(|| preview.join("\n")), added: added as i32, removed: removed as i32, reason, danger, questions: None,
    };
    (ask, outside)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn call(j: &str) -> Json { hover_core::json::parse(j).unwrap() }

    #[test]
    fn what_each_setting_asks_about() {
        use AgentApproval::*;
        for k in ["read", "search", "think", "switch_mode"] { assert!(!needs_asking(Always, k, true), "{k}"); }
        assert!(!needs_asking(Autopilot, "execute", true));
        assert!(needs_asking(Risky, "execute", false));
        assert!(!needs_asking(Risky, "edit", false), "an edit inside the folder goes ahead");
        assert!(needs_asking(Risky, "edit", true));
        assert!(needs_asking(Always, "edit", false));
        assert!(needs_asking(Risky, "delete", false));
    }

    #[test]
    fn codex_commands_are_unwrapped_and_judged() {
        let (a, out) = describe(&call(r#"{"toolCallId":"c1","kind":"execute","title":"Run","rawInput":{"command":["bash","-lc","rm -rf build"]}}"#), "execute", "/p");
        assert_eq!((a.command.as_deref(), a.danger, a.reason.as_str(), out), (Some("rm -rf build"), true, "Can delete or overwrite things", false));
        let (a, _) = describe(&call(r#"{"kind":"execute","rawInput":{"command":"\"C:\\Program Files\\PowerShell\\7\\pwsh.exe\" -NoProfile -Command \"npm install left-pad\""}}"#), "execute", "/p");
        assert_eq!((a.command.as_deref(), a.reason.as_str()), (Some("npm install left-pad"), "Installs packages or uses the network"));
        assert_eq!(a.title, "Use a tool");
        assert_eq!(a.id.len(), 32, "a question without an id gets one");
    }

    #[test]
    fn cursors_command_comes_from_its_title() {
        let (a, _) = describe(&call(r#"{"toolCallId":"x","kind":"execute","title":"`cargo test`"}"#), "execute", "/p");
        assert_eq!((a.command.as_deref(), a.reason.as_str(), key(&a).as_str()), (Some("cargo test"), "Runs a command", "execute:cargo test"));
    }

    #[cfg(not(windows))]
    #[test]
    fn an_edit_shows_its_change_and_where_it_is() {
        let j = r#"{"toolCallId":"e","kind":"edit","title":"Edit","locations":[{"path":"/p/src/a.rs"}],"content":[{"type":"diff","path":"/p/src/a.rs","oldText":"a\nb\nc","newText":"a\nB\nc\nd"}]}"#;
        let (a, out) = describe(&call(j), "edit", "/p");
        assert_eq!((a.path.as_deref(), out, a.added, a.removed, a.reason.as_str()), (Some("src/a.rs"), false, 2, 1, "Changes 3 lines"));
        assert_eq!(a.preview.as_deref(), Some("- b\n+ B\n+ d"));
        let (a, out) = describe(&call(r#"{"kind":"edit","locations":[{"path":"/p/../etc/hosts"}]}"#), "edit", "/p");
        assert_eq!((a.path.as_deref(), out, a.reason.as_str()), (Some("/p/../etc/hosts"), true, "Edits a file outside the folder"));
        let (_, out) = describe(&call(r#"{"kind":"edit","locations":[{"path":"sub/./x.txt"}]}"#), "edit", "/p");
        assert!(!out, "a relative path is in the folder");
        let (a, _) = describe(&call(r#"{"kind":"fetch","locations":[{"path":"/elsewhere/x"}]}"#), "fetch", "/p");
        assert_eq!(a.reason, "Uses the network · outside the folder");
    }
}

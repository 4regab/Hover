//! What the notch says while an agent works, for the steps tools really report. The
//! kinds and titles are as Kiro, Codex and Cursor left them in a real history
//! (`hover-data steps`), with personal paths taken out.

use hover_agents::session::{KiroSession, KiroTurn};
use hover_agents::stream::{KiroPhase, KiroStream};
use hover_agents::words::activity;
use hover_core::model::{AgentTool, KiroState, KiroStep};

/// A running session whose one turn has this step going.
fn doing(kind: &str, title: &str, target: Option<&str>) -> KiroSession {
    let mut s = KiroSession::new(AgentTool::Kiro);
    s.state = KiroState::Running;
    s.phase = KiroPhase::Working;
    let mut t = KiroTurn::new("task", vec![]);
    t.steps.push(KiroStep::new("t1", kind, title, target.map(str::to_owned), "in_progress"));
    s.turns.push(t);
    s
}

fn says(kind: &str, title: &str, target: Option<&str>) -> (&'static str, String) { activity(&doing(kind, title, target)) }

#[test]
fn an_mcp_tool_reads_as_using_it_not_editing() {
    // Kiro running the playwriter MCP: kind "other", no target (its input is code).
    assert_eq!(says("other", "@playwriter/execute", None), ("Using", "playwriter: execute".to_owned()));
    assert_eq!(says("other", "@playwriter/reset", None), ("Using", "playwriter: reset".to_owned()));
    assert_eq!(says("other", "@playwright/browser_navigate", None), ("Using", "playwright: browser_navigate".to_owned()));
    assert_eq!(says("other", "@memory/create_entities", None), ("Using", "memory: create_entities".to_owned()));
    assert_eq!(says("other", "@some-long-server-name/some_long_tool_name", None).1, "some-long-server-name: some…");
    // Cursor names one "MCP: tool".
    assert_eq!(says("other", "MCP: browser_type", None), ("Using", "browser_type".to_owned()));
}

#[test]
fn the_stream_does_not_call_an_mcp_tool_editing() {
    let mut k = KiroStream::new("Kiro");
    let call = r#"{"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"s1","update":{"sessionUpdate":"tool_call","toolCallId":"m1","kind":"other","title":"@playwriter/execute","status":"in_progress","rawInput":{"code":"await page.goto('https://example.com')","timeout":30000}}}}"#;
    assert_eq!(k.feed(call), Some(KiroPhase::Working));
}

#[test]
fn other_steps_say_their_own_title() {
    assert_eq!(says("other", "Loaded skill: unslop", None), ("Working on", "Loaded skill: unslop".to_owned()));
    assert_eq!(says("other", "Update Session Information", None), ("Working on", "Update Session Information".to_owned()));
    assert_eq!(says("other", "Serve the mockup on localhost for testing", Some("python -m http.server 8765 --bind 127.0.0.1")),
        ("Working on", "Serve the mockup on localho…".to_owned()));
    // Kiro's own message when it is stuck is not a name.
    assert_eq!(says("other", "I've been trying to use \"mcp_playwriter_execute\" but it's failed 3 times in a row.\n\nWhat would you like me to do?", None), ("Working", String::new()));
    assert_eq!(says("other", "Working", None), ("Working", String::new()));
    // Kiro's read and write tools, when it reports them as "other".
    assert_eq!(says("other", "Read File", Some("src/app/refresh.ts")), ("Reading", "refresh.ts".to_owned()));
    assert_eq!(says("other", "Write File", Some("src/app/refresh.ts")), ("Editing", "refresh.ts".to_owned()));
}

#[test]
fn reads_edits_runs_and_searches_are_unchanged() {
    assert_eq!(says("read", "Read File", Some("src/app/refresh.ts")), ("Reading", "refresh.ts".to_owned()));
    assert_eq!(says("edit", "Replace in refresh.ts", Some("src/app/refresh.ts")), ("Editing", "refresh.ts".to_owned()));
    assert_eq!(says("edit", "Write File", Some("src/file.ts")), ("Editing", "file.ts".to_owned()));
    assert_eq!(says("execute", "Run the tests", Some("npm test --watch")), ("Running", "npm test".to_owned()));
    assert_eq!(says("execute", "Rerun the build", Some("cargo build --release")), ("Running", "cargo build".to_owned()));
    assert_eq!(says("search", "Grep Search", Some("tool_phase")), ("Searching", "tool_phase".to_owned()));
    assert_eq!(says("fetch", "Fetch URL", Some("https://example.com/docs")), ("Fetching", "docs".to_owned()));
}

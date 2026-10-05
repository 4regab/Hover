//! DeskInfo.Answer's data as the page's desk reads it: the names of the C# anonymous
//! objects, for the panels that need neither git nor gh (hover-agents' own tests cover
//! what git and gh say; tests/protocol.rs runs them end to end).

use hover_agents::desk::{Desk, DeskStep, Item, Snap};
use hover_agents::github::GitHubCli;
use hover_backend::panels;
use hover_core::json::Json;
use std::sync::Arc;

/// A desk with no git and no gh.
fn bare() -> Desk { Desk::new(Arc::new(GitHubCli::new().with(Some(std::env::temp_dir().join("hover-backend-no-gh.exe")), vec![])), None) }

fn step(id: &str, kind: &str, title: &str, target: Option<&str>, input: Option<&str>, log: Option<&str>) -> DeskStep {
    DeskStep { id: id.into(), kind: kind.into(), title: title.into(), target: target.map(str::to_owned), status: "completed".into(),
        input: input.map(str::to_owned), log: log.map(str::to_owned), ..Default::default() }
}

fn snap(folder: &str, steps: Vec<DeskStep>) -> Snap {
    Snap { key: "k".into(), folder: folder.into(), busy: false, current: Some(0), steps: steps.into_iter().map(|step| Item { turn: 0, step }).collect(), texts: vec![], cloud: None }
}

fn temp(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hover-backend-panels-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn steps() -> Vec<DeskStep> {
    vec![
        step("run-1", "execute", "npm run dev", Some("npm run dev"), Some(r#"{"command":["bash","-lc","npm run dev"]}"#), Some("ready on http://localhost:5173/")),
        step("agent-1", "other", "Task", None, Some(r#"{"subagent_type":"explore","description":"Find the config","prompt":"Look for config files"}"#), Some("Found config.json")),
        step("fetch-1", "fetch", "Fetch docs", Some("https://example.com/docs"), Some(r#"{"url":"https://example.com/docs"}"#), None),
        step("cua-1", "other", "cua-driver: screenshot", None, Some("{}"), None),
    ]
}

#[test]
fn the_terminal_lists_commands_and_not_computer_use() {
    let d = bare();
    let s = snap(&std::env::temp_dir().to_string_lossy(), steps());
    let t = panels::answer(&d, &s, Some("terminal"), None);
    assert_eq!(t.compact(), r#"{"commands":[{"id":"run-1","turn":0,"cmd":"npm run dev","status":"completed","exit":null,"ms":null,"out":"ready on http://localhost:5173/"}]}"#);
}

#[test]
fn subagents_and_pages() {
    let d = bare();
    let s = snap(&std::env::temp_dir().to_string_lossy(), steps());
    let a = panels::answer(&d, &s, Some("agents"), None);
    assert_eq!(a.compact(), r#"{"agents":[{"id":"agent-1","turn":0,"name":"explore","task":"Find the config","prompt":"Look for config files","status":"completed","ms":null,"out":"Found config.json"}],"running":0}"#);
    let b = panels::answer(&d, &s, Some("browser"), None);
    let pages = b.get("pages").unwrap().items().unwrap();
    assert_eq!(pages.iter().map(|p| p.get("url").unwrap().as_str().unwrap()).collect::<Vec<_>>(), ["https://example.com/docs", "http://localhost:5173/"], "newest first");
    assert_eq!(pages[1].compact(), r#"{"url":"http://localhost:5173/","kind":"server","local":true,"title":null,"status":"completed","turn":0}"#);
    assert_eq!(pages[0].get("kind").unwrap().as_str(), Some("fetch"));
    assert_eq!(pages[0].get("title").unwrap().as_str(), Some("Fetch docs"));
    assert_eq!(panels::answer(&d, &s, Some("nothing"), None).compact(), r#"{"error":"Unknown panel."}"#);
    assert_eq!(panels::answer(&d, &s, None, None).compact(), r#"{"error":"Unknown panel."}"#);
}

#[test]
fn a_folder_without_git_lists_its_files_and_reads_one() {
    let dir = temp("files");
    std::fs::write(dir.join("a.txt"), "hello\n").unwrap();
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::write(dir.join("src/b.rs"), "fn b() {}\n").unwrap();
    std::fs::write(dir.join("bin.dat"), [0u8, 1, 2]).unwrap();
    let d = bare();
    let s = snap(&dir.to_string_lossy(), steps());
    let f = panels::answer(&d, &s, Some("files"), None);
    assert_eq!(f.props().unwrap().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(), ["git", "branch", "changed", "touched", "tree", "more"]);
    assert_eq!(f.get("git"), Some(&Json::Bool(false)));
    let tree: Vec<&str> = f.get("tree").unwrap().items().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
    assert_eq!(tree, ["a.txt", "bin.dat", "src/b.rs"]);
    // One file: its text; a binary one: that it is; outside the folder: refused.
    assert_eq!(panels::answer(&d, &s, Some("file"), Some("a.txt")).compact(), r#"{"path":"a.txt","text":"hello\n","truncated":false,"size":6}"#);
    assert_eq!(panels::answer(&d, &s, Some("file"), Some("bin.dat")).compact(), r#"{"path":"bin.dat","binary":true,"size":3}"#);
    let out = panels::answer(&d, &s, Some("file"), Some("../x"));
    assert_eq!(out.get("error").unwrap().as_str(), Some("That file isn’t in the session’s folder."));
    assert!(panels::answer(&d, &s, Some("file"), Some("gone.txt")).get("error").is_some());
    // No git: the diff is what the session's own edits made, and says it is partial.
    let diff = panels::answer(&d, &s, Some("diff"), None);
    assert_eq!(diff.compact(), r#"{"git":false,"partial":true,"files":[]}"#);
    // A folder that is gone.
    let gone = snap(&dir.join("gone").to_string_lossy(), vec![]);
    assert_eq!(panels::answer(&d, &gone, Some("files"), None).get("error").unwrap().as_str(), Some("The session's folder isn’t there any more."));
}

#[test]
fn the_probe_without_git_or_gh() {
    let dir = temp("probe");
    let d = bare();
    let s = snap(&dir.to_string_lossy(), steps());
    let p = panels::answer(&d, &s, Some("probe"), None);
    assert_eq!(p.props().unwrap().iter().map(|(k, _)| k.as_str()).collect::<Vec<_>>(),
        ["folder", "git", "branch", "changed", "add", "del", "gh", "ghAuth", "ghUser", "pr", "prReason", "commands", "agents", "running", "pages", "linked"]);
    assert_eq!(p.get("folder"), Some(&Json::Bool(true)));
    assert_eq!(p.get("git"), Some(&Json::Bool(false)));
    assert_eq!(p.get("prReason").unwrap().as_str(), Some("Not a Git repository."));
    assert_eq!((p.get("commands").unwrap().compact(), p.get("agents").unwrap().compact(), p.get("pages").unwrap().compact()), ("1".to_owned(), "1".to_owned(), "2".to_owned()));
    // The pull request panel and the linked ones, with no git or gh to ask (and here no git installed).
    assert_eq!(panels::answer(&d, &s, Some("pr"), None).get("error").unwrap().as_str(), Some("Git isn’t installed."));
    let linked = panels::answer(&d, &s, Some("linked"), None);
    assert_eq!(linked.compact(), r#"{"gh":false,"prs":[]}"#);
}

#[test]
fn a_pull_request_linked_in_the_steps_is_listed() {
    let d = bare();
    let s = snap(&std::env::temp_dir().to_string_lossy(), vec![step("r", "execute", "gh pr create", Some("gh pr create"), None, Some("https://github.com/octo/demo/pull/7\n"))]);
    assert_eq!(panels::answer(&d, &s, Some("linked"), None).compact(), r#"{"gh":false,"prs":[{"url":"https://github.com/octo/demo/pull/7","repo":"octo/demo","number":7}]}"#);
}

#[test]
fn create_pull_request_refuses_what_it_cant_do() {
    let d = bare();
    let args = Json::obj(vec![("title", Json::str("  ")), ("body", Json::str(""))]);
    let s = snap(&std::env::temp_dir().to_string_lossy(), vec![]);
    let r = panels::created(&d, &s, &args);
    assert!(r.get("error").and_then(Json::as_str).is_some(), "{}", r.compact());
    assert!(r.get("ok").is_none());
    let mut busy = s.clone();
    busy.busy = true;
    let r = panels::created(&d, &busy, &Json::obj(vec![("title", Json::str("Add b"))]));
    assert_eq!(r.get("error").and_then(Json::as_str), Some("Wait for the agent to finish first: it is still working in this folder."));
}

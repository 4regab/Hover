//! The desk card's backend: DeskInfoTests, ported, and the panels against real git
//! repositories in temp folders and a stand-in gh (fixtures/fakegh.rs).

#[path = "fixtures/common.rs"]
mod common;

use common::*;
use hover_agents::desk as d;
use hover_agents::desk::{DeskStep, Snap};
use hover_agents::session::{KiroSession, KiroTurn};
use hover_agents::stream::KiroResult;
use hover_core::model::{AgentTool, KiroState, KiroStep};

fn step(id: &str, kind: &str, title: &str, target: Option<&str>, status: &str) -> DeskStep {
    DeskStep { id: id.into(), kind: kind.into(), title: title.into(), target: target.map(str::to_owned), status: status.into(), ..Default::default() }
}

fn snap(folder: &str, steps: Vec<DeskStep>) -> Snap {
    Snap {
        key: "test".into(), folder: folder.into(), busy: false, current: Some(0),
        steps: steps.into_iter().map(|step| d::Item { turn: 0, step }).collect(),
        texts: vec!["Fix it".into(), "Opened https://github.com/acme/app/pull/7 for review.".into()],
        cloud: None,
    }
}

// MARK: Reading git's text

#[test]
fn a_unified_diff_becomes_one_entry_per_file() {
    let patch = [
        "diff --git a/src/a.cs b/src/a.cs", "index 1..2 100644", "--- a/src/a.cs", "+++ b/src/a.cs",
        "@@ -1,3 +1,3 @@", " keep", "-old", "+new", " end",
        "diff --git a/new.txt b/new.txt", "new file mode 100644", "--- /dev/null", "+++ b/new.txt", "@@ -0,0 +1,2 @@", "+one", "+two",
        "diff --git a/old.md b/renamed.md", "similarity index 90%", "rename from old.md", "rename to renamed.md",
        "diff --git a/logo.png b/logo.png", "Binary files a/logo.png and b/logo.png differ",
    ].join("\n");
    let files = d::parse_diff(&patch);
    let got: Vec<_> = files.iter().map(|f| (f.path.as_str(), f.status, f.add, f.del)).collect();
    assert_eq!(got, [("src/a.cs", 'M', 1, 1), ("new.txt", 'A', 2, 0), ("renamed.md", 'R', 0, 0), ("logo.png", 'M', 0, 0)]);
    assert!(files[0].patch.starts_with("@@ -1,3 +1,3 @@") && files[0].patch.contains("-old\n+new"));
    assert_eq!(files[2].old.as_deref(), Some("old.md"));
    assert_eq!(files[0].old, None);
    assert!(files[3].binary && !files[0].binary);
    // A deleted file, and a removed line that looks like a file header, are counted as lines.
    let gone = d::parse_diff("diff --git a/x b/x\ndeleted file mode 100644\n--- a/x\n+++ /dev/null\n@@ -1,2 +0,0 @@\n-a\n--- b");
    assert_eq!((gone[0].path.as_str(), gone[0].status, gone[0].add, gone[0].del), ("x", 'D', 0, 2));
    assert!(d::parse_diff("").is_empty());
}

#[test]
fn status_paths_are_relative_to_the_folder_in_the_repository() {
    let z = " M app/src/a.cs\0?? app/notes.txt\0R  app/b.cs\0app/a_old.cs\0 D app/gone.cs\0A  app/added.cs\0";
    let got: Vec<_> = d::parse_status(z, "app/").into_iter().map(|c| (c.path, c.status, c.old)).collect();
    let want = [("src/a.cs", 'M', None), ("notes.txt", '?', None), ("b.cs", 'R', Some("a_old.cs")), ("gone.cs", 'D', None), ("added.cs", 'A', None)];
    assert_eq!(got, want.map(|(p, s, o)| (p.to_owned(), s, o.map(str::to_owned))));
    assert_eq!(d::parse_status(" M a.txt\0", "")[0].path, "a.txt");
    assert!(d::parse_status("", "").is_empty());
}

#[test]
fn small_helpers_read_as_the_csharp_ones_do() {
    assert!(d::valid_ref("hover/fix-notch") && d::valid_ref("v1.2_rc"));
    for bad in ["--upload-pack=evil", "-x", "a..b", "has space", "x/", "x.lock", "", "a\nb", "é"] { assert!(!d::valid_ref(bad), "{bad:?}"); }
    assert_eq!(d::slug("Fix the notch flicker on resize!"), "fix-the-notch-flicker-on-resize");
    assert_eq!(d::slug("  --Hello,   World--  "), "hello-world");
    assert_eq!(d::slug(&"word ".repeat(30)).len(), 39, "cut at 40 without a trailing dash");
    assert!(d::slug("ñandú ☃").starts_with("and"), "{}", d::slug("ñandú ☃"));
    let fallback = d::slug("☃☃☃");
    assert!(fallback.starts_with("changes-") && fallback.len() == "changes-0102-0304".len(), "{fallback}");

    assert_eq!(d::relative(Some("C:\\work\\app\\src\\a.rs"), "C:\\work\\app").as_deref(), Some("src/a.rs"));
    assert_eq!(d::relative(Some("c:/WORK/app/src/a.rs"), "C:\\work\\app\\").as_deref(), Some("src/a.rs"));
    assert_eq!(d::relative(Some("./src/a.rs"), "/w").as_deref(), Some("src/a.rs"));
    assert_eq!(d::relative(Some("/other/x.rs"), "/w"), None);
    assert_eq!(d::relative(Some("D:\\other\\x.rs"), "C:\\w"), None);
    assert_eq!(d::relative(Some("../x.rs"), "/w"), None);
    assert_eq!(d::relative(Some("  "), "/w"), None);
    assert_eq!(d::relative(None, "/w"), None);
    assert_eq!(d::relative(Some("/w"), "/w"), None, "the folder itself is not a file");

    assert_eq!(d::num(0), "0");
    assert_eq!(d::num(999), "999");
    assert_eq!(d::num(1234567), "1,234,567");
    assert_eq!(d::num(-4321), "-4,321");

    assert_eq!(d::label("https://threejs.org/docs/"), "threejs.org/docs/");
    assert_eq!(d::label("http://localhost:5173/"), "localhost:5173");
    assert_eq!(d::label("http://user@host.io:8080/a/b?q=1#h"), "host.io:8080/a/b");
    assert_eq!(d::label("not a url"), "not a url");
    assert_eq!(d::urls(Some("see https://a.io/x, and (http://b.io). Also ftp://c.io and https:// bad")), ["https://a.io/x", "http://b.io"]);
    assert!(d::urls(None).is_empty() && d::urls(Some("")).is_empty());
    assert!(d::is_local("http://localhost:3000/x") && d::is_local("HTTP://127.0.0.1") && d::is_local("http://[::1]:80/") && !d::is_local("http://localhost.evil.com/"));

    assert_eq!(d::gh_reason("no pull requests found for branch \"x\""), d::NO_PR);
    assert_eq!(d::gh_reason("To get started with GitHub CLI, please run:  gh auth login"), "Sign in to GitHub to see pull requests.");
    assert_eq!(d::gh_reason("fatal: not a git repository"), "Not a Git repository.");
    assert_eq!(d::gh_reason("none of the git remotes configured for this repository point to a known GitHub host"), "This repository has no GitHub remote.");
    assert_eq!(d::gh_reason("\n  boom: it broke\nmore"), "boom: it broke");
    assert_eq!(d::gh_reason("  "), "gh couldn’t read the pull request.");
    assert_eq!(d::gh_reason(&"x".repeat(300)).chars().count(), 200);
}

// MARK: What a session's steps say

#[test]
fn subagents_and_computer_use_are_told_apart_from_other_calls() {
    let mut task = step("1", "other", "Find the notch code", None, "completed");
    task.input = Some(r#"{"description":"Find it","prompt":"Look","subagent_type":"explore"}"#.into());
    task.log = Some("Found it in notch.rs".into());
    let spawn = step("2", "other", "spawn_agent", None, "in_progress");
    let task_call = step("2b", "agent", "Explore the notch", None, "completed");
    let click = step("3", "other", "mcp__cua-driver__click", None, "completed");
    let shot = step("4", "other", "screenshot", None, "completed");
    let read = step("5", "read", "Read src/a.cs", Some("src/a.cs"), "completed");
    let clicked = step("6", "other", "Clicked through the docs", None, "completed");
    let all = [&task, &spawn, &click, &shot, &read, &clicked];
    assert_eq!(all.map(d::is_subagent), [true, true, false, false, false, false]);
    assert!(d::is_subagent(&task_call), "Claude Code's and OpenCode's task calls are kind agent");
    assert!(!d::is_screen(&task_call));
    assert_eq!(all.map(d::is_screen), [false, false, true, true, false, false]);
    // Hover's own browser is a page, not the screen, even when its name has a click in it.
    assert!(!d::is_screen(&step("7", "other", "mcp__hover-browser__browser_click", None, "completed")));
    assert!(!d::is_screen(&step("8", "other", "browser_click", None, "completed")));
    assert_eq!(d::browser_op("mcp__hover-browser__browser_open").as_deref(), Some("open"));
    assert_eq!(d::browser_op("browser_screenshot").as_deref(), Some("screenshot"));
    assert_eq!(d::browser_op("my_browser_openx"), None);
    // Computer use by its input alone.
    let mut by_input = step("9", "other", "Tool", None, "completed");
    by_input.input = Some(r#"{"driver":"cua-driver"}"#.into());
    assert!(d::is_screen(&by_input));

    let s = d::subagents(&snap("/w", vec![task, spawn, read]));
    assert_eq!(s.running, 1);
    assert_eq!(s.agents.len(), 2);
    let a = &s.agents[0];
    assert_eq!((a.name.as_str(), a.task.as_str(), a.prompt.as_deref(), a.out.as_deref()), ("explore", "Find it", Some("Look"), Some("Found it in notch.rs")));
    assert_eq!((s.agents[1].name.as_str(), s.agents[1].task.as_str(), s.agents[1].status.as_str()), ("Subagent", "spawn_agent", "in_progress"));
}

#[test]
fn commands_pages_and_linked_pull_requests_come_from_the_steps() {
    let mut dev = step("1", "execute", "Run", None, "completed");
    dev.exit = Some(0);
    dev.input = Some(r#"{"command":["bash","-lc","npm run dev"]}"#.into());
    dev.log = Some("VITE ready\n  Local:   http://localhost:5173/\n  Network: http://192.168.1.4:5173/".into());
    let fetch = step("2", "fetch", "three.js docs", Some("https://threejs.org/docs/"), "completed");
    let mut pr = step("3", "execute", "Run", Some("gh pr create"), "completed");
    pr.log = Some("https://github.com/acme/app/pull/12".into());
    assert_eq!(d::command_of(&dev), "npm run dev");
    let s = snap("/w", vec![dev, fetch, pr]);
    let term = d::terminal(&s);
    assert_eq!(term.commands.len(), 2);
    assert!(term.commands[0].cmd == "npm run dev" && term.commands[0].out.contains("VITE ready") && term.commands[0].exit == Some(0));
    assert_eq!(term.commands[1].cmd, "gh pr create");
    let pages = d::pages(&s);
    // The fetch is newest; the dev server's local address counts, its network one doesn't.
    assert_eq!(pages.len(), 2);
    assert_eq!((pages[0].url.as_str(), pages[0].kind, pages[0].title.as_deref()), ("https://threejs.org/docs/", d::PageKind::Fetch, Some("three.js docs")));
    assert_eq!((pages[1].url.as_str(), pages[1].kind, pages[1].local), ("http://localhost:5173/", d::PageKind::Server, true));
    assert_eq!(d::PageKind::Server.name(), "server");
    let linked = d::linked_urls(&s);
    assert_eq!(linked.iter().map(|u| (u.1.as_str(), u.2)).collect::<Vec<_>>(), [("acme/app", 12), ("acme/app", 7)]);
}

#[test]
fn pages_are_the_last_seen_newest_first_with_a_servers_address_made_local() {
    let mut a = step("1", "execute", "Run", None, "completed");
    a.output = Some("Listening on http://0.0.0.0:3000/app and http://127.0.0.1:3000".into());
    let mut b = step("2", "other", "Open", None, "completed");
    b.input = Some(r#"{"url":"https://example.com/a"}"#.into());
    let mut c = step("3", "other", "mcp__cua-driver__launch_app", None, "completed");
    c.input = Some(r#"{"url":"https://example.com/b"}"#.into());
    let mut again = step("4", "fetch", "Again", None, "completed");
    again.input = Some(r#"{"url":"https://example.com/a"}"#.into());
    let pages = d::pages(&snap("/w", vec![a, b, c, again]));
    let got: Vec<_> = pages.iter().map(|p| (p.url.as_str(), p.kind)).collect();
    assert_eq!(got, [
        ("https://example.com/a", d::PageKind::Fetch), ("https://example.com/b", d::PageKind::Screen),
        ("http://127.0.0.1:3000", d::PageKind::Server), ("http://localhost:3000/app", d::PageKind::Server)]);
    // Many pages: the newest forty.
    let many: Vec<_> = (0..60).map(|i| { let mut x = step(&i.to_string(), "fetch", "p", Some(&format!("https://e.io/{i}")), "completed"); x.input = None; x }).collect();
    let p = d::pages(&snap("/w", many));
    assert_eq!((p.len(), p[0].url.as_str()), (40, "https://e.io/59"));
}

#[test]
fn the_terminal_keeps_the_newest_output_whole_and_cuts_the_older() {
    let long = |n: usize, mark: char| { let mut t = "x".repeat(n - 1); t.push(mark); t };
    let mut steps = vec![];
    for (i, m) in ['a', 'b', 'c'].into_iter().enumerate() {
        let mut x = step(&i.to_string(), "execute", "Run", Some(&format!("cmd {i}")), "completed");
        x.log = Some(long(300_000, m));
        steps.push(x);
    }
    let t = d::terminal(&snap("/w", steps));
    let lens: Vec<_> = t.commands.iter().map(|c| c.out.chars().count()).collect();
    assert_eq!(lens, [2000, 400 * 1024 - 300_000, 300_000], "newest first for the budget");
    assert!(t.commands.iter().zip(['a', 'b', 'c']).all(|(c, m)| c.out.ends_with(m)), "the end of the output is what is kept");
    // Only the last eighty commands, and computer use is not a command.
    let mut many: Vec<_> = (0..100).map(|i| step(&format!("c{i}"), "execute", "Run", Some("ls"), "completed")).collect();
    many.push(step("shot", "execute", "screenshot", None, "completed"));
    let t = d::terminal(&snap("/w", many));
    assert_eq!((t.commands.len(), t.commands[0].id.as_str(), t.commands[79].id.as_str()), (80, "c20", "c99"));
    // A running command, and an exit code, come through.
    let mut run = step("r", "execute", "Run", Some("sleep 9"), "in_progress");
    run.exit = None;
    run.ms = Some(1500.0);
    let t = d::terminal(&snap("/w", vec![run]));
    assert_eq!((t.commands[0].status.as_str(), t.commands[0].ms, t.commands[0].out.as_str()), ("in_progress", Some(1500.0), ""));
}

#[test]
fn a_command_is_read_from_its_input_target_or_title() {
    let with = |input: Option<&str>, target: Option<&str>| DeskStep { input: input.map(str::to_owned), target: target.map(str::to_owned), title: "Run shell".into(), ..Default::default() };
    assert_eq!(d::command_of(&with(Some(r#"{"command":"cargo test"}"#), Some("other"))), "cargo test");
    assert_eq!(d::command_of(&with(Some(r#"{"command":["bash","-lc","npm run dev"]}"#), None)), "npm run dev");
    assert_eq!(d::command_of(&with(Some(r#"{"command":["sh","-c","ls -la"]}"#), None)), "ls -la");
    assert_eq!(d::command_of(&with(Some(r#"{"command":["ls","-la"]}"#), None)), "ls -la");
    assert_eq!(d::command_of(&with(Some(r#"{"command":["a","b","c"]}"#), None)), "a b c");
    assert_eq!(d::command_of(&with(Some(r#"{"path":"x"}"#), Some("the target"))), "the target");
    assert_eq!(d::command_of(&with(Some("not json"), None)), "Run shell");
    assert_eq!(d::command_of(&with(None, None)), "Run shell");
}

#[test]
fn the_apps_computer_use_opened_come_from_its_calls_and_from_what_the_integrations_noted() {
    let mut launch = step("1", "other", "mcp__cua-driver__launch_app", None, "completed");
    launch.input = Some(r#"{"app_name":"Safari","bundle_id":"com.apple.Safari"}"#.into());
    launch.log = Some(r#"{"pid": 4242, "name": "Safari"}"#.into());
    let mut click = step("2", "other", "click", None, "completed");
    click.input = Some(r#"{"pid":4242,"app":"safari"}"#.into());
    let mut other = step("3", "other", "mcp__cua-driver__list_apps", None, "completed");
    other.output = Some(r#"[{"pid":1,"name":"launchd"},{"pid":777}]"#.into());
    let not_screen = step("4", "read", "Read", Some("a.rs"), "completed");
    let a = d::apps_of_steps(&[launch.clone(), click, other, not_screen]).unwrap();
    assert_eq!(a.pids, [4242, 777], "pid 1 is not an app of the user's");
    assert_eq!(a.bundles, ["com.apple.Safari"]);
    assert_eq!(a.names, ["Safari"], "the same name once, however it is cased");
    assert_eq!(d::apps_of_steps(&[step("5", "read", "Read", Some("a.rs"), "completed")]), None);
    assert_eq!(d::apps_of_steps(&[step("6", "other", "click", None, "completed")]), None, "a click that names nothing");

    // The registry: what the integrations push, by the session's key, merged with the steps.
    let key = "apps-test-session-1";
    assert_eq!(d::noted_apps(key), None);
    d::note_app(key, 9001, "Notes");
    d::note_app(key, 9001, "notes");
    d::note_app(key, 4242, "");
    d::note_bundle(key, "com.apple.Notes");
    d::note_app("", 5, "ignored");
    let n = d::noted_apps(key).unwrap();
    assert_eq!((n.pids, n.bundles, n.names), (vec![9001, 4242], vec!["com.apple.Notes".to_owned()], vec!["Notes".to_owned()]));
    let mut s = snap("/w", vec![launch]);
    s.key = key.into();
    let m = d::apps(&s).unwrap();
    assert_eq!(m.pids, [4242, 9001]);
    assert_eq!(m.bundles, ["com.apple.Safari", "com.apple.Notes"]);
    assert_eq!(m.names, ["Safari", "Notes"]);
    // Only the newest sixteen of each.
    for p in 100..130 { d::note_app(key, p, &format!("App {p}")); }
    let n = d::noted_apps(key).unwrap();
    assert_eq!((n.pids.len(), n.names.len(), *n.pids.last().unwrap(), n.names.last().unwrap().as_str()), (16, 16, 129, "App 129"));
    d::forget_apps(key);
    assert_eq!(d::noted_apps(key), None);
    assert_eq!(d::apps(&snap("/w", vec![])), None);
}

#[test]
fn a_session_is_copied_as_the_panels_read_it() {
    let mut s = KiroSession::new(AgentTool::Codex);
    s.folder = "/w".into();
    s.key = "keykey".into();
    s.state = KiroState::Running;
    let mut t = KiroTurn::new("Fix it", vec![]);
    t.steps.push(KiroStep { id: "1".into(), kind: "execute".into(), title: "Run".into(), target: Some("cargo test".into()), status: "in_progress".into(), output: Some("ok".into()), exit: Some(0), ms: Some(5.0), ..Default::default() });
    t.result = Some(KiroResult::new(KiroState::Completed, "Done."));
    let mut queued = KiroTurn::new("Also this", vec![]);
    queued.queued = true;
    s.turns = vec![t, queued];
    let sn = Snap::of(&s);
    assert_eq!((sn.key.as_str(), sn.folder.as_str(), sn.busy, sn.current), ("keykey", "/w", true, Some(0)));
    assert_eq!(sn.texts, ["Fix it", "Done.", "Also this", ""]);
    assert_eq!(sn.steps.len(), 1);
    let x = &sn.steps[0].step;
    assert_eq!((x.kind.as_str(), x.target.as_deref(), x.output.as_deref(), x.exit, x.input.as_deref()), ("execute", Some("cargo test"), Some("ok"), Some(0), None));
    // The output stands in for the log, so the terminal has it.
    assert_eq!(d::terminal(&sn).commands[0].out, "ok");
    // Testing and browsing look at the current turn's last three steps, while it runs.
    let mut s2 = sn.clone();
    s2.steps.push(d::Item { turn: 0, step: step("2", "other", "mcp__cua-driver__click", None, "completed") });
    assert!(s2.testing() && !s2.browsing());
    s2.steps.push(d::Item { turn: 0, step: step("3", "other", "mcp__hover-browser__browser_open", None, "completed") });
    assert!(s2.testing() && s2.browsing());
    s2.busy = false;
    assert!(!s2.testing() && !s2.browsing(), "only while it runs");
    let mut s3 = sn.clone();
    s3.steps.insert(0, d::Item { turn: 0, step: step("0", "other", "click", None, "completed") });
    s3.steps.extend((0..3).map(|i| d::Item { turn: 0, step: step(&format!("r{i}"), "read", "Read", Some("a"), "completed") }));
    assert!(!s3.testing(), "a click more than three steps ago is not now");
    s3.current = Some(1);
    assert!(!s3.testing() && !s3.browsing(), "steps of another turn are not now");
}

// MARK: One file, inside the folder

fn link_file(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(unix)] { std::os::unix::fs::symlink(target, link).is_ok() }
    #[cfg(windows)] { std::os::windows::fs::symlink_file(target, link).is_ok() }
}

fn link_dir(target: &std::path::Path, link: &std::path::Path) -> bool {
    #[cfg(unix)] { std::os::unix::fs::symlink(target, link).is_ok() }
    #[cfg(windows)] { std::os::windows::fs::symlink_dir(target, link).is_ok() }
}

#[test]
fn a_file_request_stays_inside_the_folder() {
    let dir = Dir::new("desk-inside");
    let folder = dir.join("work");
    std::fs::create_dir_all(folder.join("src")).unwrap();
    std::fs::write(folder.join("src/ok.txt"), "hello").unwrap();
    let outside = dir.join("outside.txt");
    std::fs::write(&outside, "secret").unwrap();
    std::fs::create_dir_all(dir.join("outdir")).unwrap();
    std::fs::write(dir.join("outdir/x.txt"), "secret too").unwrap();
    let f = folder.to_string_lossy().into_owned();

    assert!(d::inside(&f, Some("src/ok.txt")).is_some());
    assert!(d::inside(&f, Some("src\\ok.txt")).is_some(), "a backslash is a separator");
    assert!(d::inside(&f, Some("./src/ok.txt")).is_some());
    assert!(d::inside(&f, Some("src/missing.txt")).is_some(), "it may not exist yet; it is still inside");
    for bad in ["../outside.txt", "src/../../outside.txt", "/etc/passwd", "", "  ", "a\0b", "C:\\Windows\\win.ini", "c:x", "src/ok.txt:stream", "."] {
        assert!(d::inside(&f, Some(bad)).is_none(), "{bad:?}");
    }
    assert!(d::inside(&f, None).is_none());
    assert!(d::inside(&dir.join("not-a-folder").to_string_lossy(), Some("x")).is_none());
    assert!(d::inside("relative/folder", Some("x")).is_none());

    // A link in the folder that points out of it is refused too.
    let file_link = link_file(&outside, &folder.join("leak.txt"));
    let dir_link = link_dir(&dir.join("outdir"), &folder.join("up"));
    if file_link {
        assert!(d::inside(&f, Some("leak.txt")).is_none());
        assert!(!format!("{:?}", d::file_text(&f, Some("leak.txt"))).contains("secret"));
    }
    if dir_link {
        assert!(d::inside(&f, Some("up/x.txt")).is_none());
        assert!(!format!("{:?}", d::file_text(&f, Some("up/x.txt"))).contains("secret"));
    }
    // One that stays inside is fine.
    if link_file(&folder.join("src/ok.txt"), &folder.join("alias.txt")) {
        assert!(d::inside(&f, Some("alias.txt")).is_some());
        assert!(matches!(d::file_text(&f, Some("alias.txt")), d::FileView::Text { text, .. } if text == "hello"));
    }
    if !(file_link && dir_link) { eprintln!("links can't be made here: that part of the test was skipped"); }

    match d::file_text(&f, Some("src/ok.txt")) {
        d::FileView::Text { text, truncated, size, path } => assert_eq!((text.as_str(), truncated, size, path.as_str()), ("hello", false, 5, "src/ok.txt")),
        other => panic!("{other:?}"),
    }
    assert!(matches!(d::file_text(&f, Some("../outside.txt")), d::FileView::Error { error, .. } if error == "That file isn’t in the session’s folder."));
    assert!(matches!(d::file_text(&f, Some("src/missing.txt")), d::FileView::Error { error, .. } if error == "That file isn’t there any more."));
    assert!(matches!(d::file_text(&f, Some("src")), d::FileView::Error { error, .. } if error == "That file isn’t there any more."), "a folder is not a file");
}

#[test]
fn a_binary_file_is_not_shown_and_a_long_one_is_cut() {
    let dir = Dir::new("desk-file");
    let f = dir.s();
    std::fs::write(dir.join("bin.dat"), [1u8, 2, 0, 3]).unwrap();
    assert_eq!(d::file_text(&f, Some("bin.dat")), d::FileView::Binary { path: "bin.dat".into(), size: 4 });
    std::fs::write(dir.join("big.txt"), "y".repeat(d::FILE_LIMIT + 10)).unwrap();
    match d::file_text(&f, Some("big.txt")) {
        d::FileView::Text { text, truncated, size, .. } => assert_eq!((text.len(), truncated, size), (d::FILE_LIMIT, true, d::FILE_LIMIT as u64 + 10)),
        other => panic!("{other:?}"),
    }
    std::fs::write(dir.join("utf8.txt"), "héllo wörld ✓\r\n").unwrap();
    assert!(matches!(d::file_text(&f, Some("utf8.txt")), d::FileView::Text { text, .. } if text == "héllo wörld ✓\r\n"));
}

// MARK: Real repositories

#[test]
fn the_diff_and_files_of_a_real_repository() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let r = Repo::new("desk-real");
    std::fs::create_dir_all(r.repo.join("src")).unwrap();
    std::fs::write(r.repo.join("src/a.txt"), "one\ntwo\n").unwrap();
    git(&r.repo, &["add", "."]);
    git(&r.repo, &["commit", "-q", "-m", "second"]);
    std::fs::write(r.repo.join("src/a.txt"), "one\nTWO\n").unwrap();
    std::fs::write(r.repo.join("new.txt"), "fresh\n").unwrap();
    std::fs::write(r.repo.join("blob.bin"), [0u8, 1, 2]).unwrap();
    let mut edit = step("1", "edit", "Edit", Some(&r.repo.join("src/a.txt").to_string_lossy()), "completed");
    edit.added = 1;
    edit.removed = 1;
    let read = step("2", "read", "Read", Some("a.txt"), "completed");
    let outside = step("3", "read", "Read", Some("/elsewhere/x"), "completed");
    let s = snap(&r.folder(), vec![edit, read.clone(), read, outside]);
    let fake = Fake::new();
    let desk = fake.desk();

    let diff = desk.diff(&s);
    assert_eq!((diff.git, diff.partial, diff.truncated, diff.error.as_deref(), diff.branch.as_deref()), (true, false, false, None, Some("main")));
    let a = diff.files.iter().find(|f| f.path == "src/a.txt").expect("the edit");
    assert_eq!((a.status, a.add, a.del), ('M', 1, 1));
    assert!(a.patch.contains("-two\n+TWO"), "{}", a.patch);
    // A file git doesn't track is shown whole, as added lines; a binary one is only named.
    let new = diff.files.iter().find(|f| f.path == "new.txt").expect("the new file");
    assert_eq!((new.status, new.add, new.del, new.binary, new.patch.as_str()), ('A', 1, 0, false, "@@ -0,0 +1,1 @@\n+fresh"));
    let blob = diff.files.iter().find(|f| f.path == "blob.bin").expect("the binary file");
    assert!(blob.binary && blob.patch.is_empty() && blob.status == 'A');

    let files = desk.files(&s);
    assert_eq!((files.git, files.more, files.error.as_deref()), (true, false, None));
    assert_eq!(files.tree, ["a.txt", "blob.bin", "new.txt", "src/a.txt"]);
    let ch = files.changed.iter().find(|c| c.path == "src/a.txt").unwrap();
    assert_eq!((ch.status, ch.add, ch.del), ('M', 1, 1));
    assert!(files.changed.iter().any(|c| c.path == "new.txt" && c.status == '?'));
    assert_eq!(files.touched, [d::Touched { path: "src/a.txt".into(), read: 0, edit: 1 }, d::Touched { path: "a.txt".into(), read: 2, edit: 0 }]);

    let probe = desk.probe(&s);
    assert!(probe.folder && probe.git && probe.git_installed);
    assert_eq!((probe.branch.as_deref(), probe.changed, probe.add, probe.del, probe.commands), (Some("main"), 3, 1, 1, 0));
    assert!(!probe.gh || probe.pr.is_none(), "no pull request from a stand-in without one");
}

#[test]
fn a_subfolder_shows_its_own_paths_and_only_its_changes() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let r = Repo::new("desk-sub");
    std::fs::create_dir_all(r.repo.join("app/src")).unwrap();
    std::fs::write(r.repo.join("app/src/x.rs"), "a\n").unwrap();
    std::fs::write(r.repo.join("other.rs"), "a\n").unwrap();
    git(&r.repo, &["add", "."]);
    git(&r.repo, &["commit", "-q", "-m", "more"]);
    std::fs::write(r.repo.join("app/src/x.rs"), "b\n").unwrap();
    std::fs::write(r.repo.join("other.rs"), "b\n").unwrap();
    let sub = r.repo.join("app").to_string_lossy().into_owned();
    let s = snap(&sub, vec![]);
    let fake = Fake::new();
    let desk = fake.desk();
    assert_eq!(desk.repo_of(&sub).prefix, "app/");
    let files = desk.files(&s);
    assert_eq!(files.tree, ["src/x.rs"]);
    assert_eq!(files.changed.iter().map(|c| (c.path.as_str(), c.status, c.add, c.del)).collect::<Vec<_>>(), [("src/x.rs", 'M', 1, 1)]);
    let diff = desk.diff(&s);
    assert_eq!(diff.files.iter().map(|f| f.path.as_str()).collect::<Vec<_>>(), ["src/x.rs"]);
}

#[test]
fn outside_git_the_tree_is_walked_and_the_diff_is_the_sessions_edits() {
    let dir = Dir::new("desk-walk");
    std::fs::create_dir_all(dir.join("src")).unwrap();
    std::fs::create_dir_all(dir.join("node_modules/dep")).unwrap();
    std::fs::create_dir_all(dir.join("target")).unwrap();
    std::fs::write(dir.join("src/b.txt"), "b").unwrap();
    std::fs::write(dir.join("A.txt"), "a").unwrap();
    std::fs::write(dir.join("node_modules/dep/i.js"), "x").unwrap();
    std::fs::write(dir.join("target/t.o"), "x").unwrap();
    let outside = Dir::new("desk-walk-out");
    std::fs::write(outside.join("secret.txt"), "s").unwrap();
    let linked = link_dir(outside.path(), &dir.join("link"));
    let mut e = step("1", "edit", "Edit", Some(&dir.join("src/b.txt").to_string_lossy()), "completed");
    e.diff = Some("  context\n- old\n+ new".into());
    e.added = 1;
    e.removed = 1;
    let s = snap(&dir.s(), vec![e]);
    // Where git isn't installed either, this is what there is.
    let desk = d::Desk::new(std::sync::Arc::new(hover_agents::github::GitHubCli::new()), None);
    let files = desk.files(&s);
    assert_eq!((files.git, files.more), (false, false));
    assert_eq!(files.tree, ["A.txt", "src/b.txt"], "build and tool folders skipped{}", if linked { ", the link not followed" } else { "" });
    let diff = desk.diff(&s);
    assert_eq!((diff.git, diff.partial), (false, true));
    assert_eq!(diff.files.len(), 1);
    assert_eq!((diff.files[0].path.as_str(), diff.files[0].status, diff.files[0].add, diff.files[0].del), ("src/b.txt", 'M', 1, 1));
    assert_eq!(diff.files[0].patch, "@@ edit @@\n context\n-old\n+new");
    // No folder: a sentence, not a crash.
    let gone = desk.files(&snap(&dir.join("gone").to_string_lossy(), vec![]));
    assert_eq!(gone.error.as_deref(), Some("The session's folder isn’t there any more."));
    let p = desk.probe(&snap(&dir.join("gone").to_string_lossy(), vec![]));
    assert!(!p.folder && !p.git && !p.git_installed);
}

#[test]
fn a_long_tree_is_cut_at_five_thousand() {
    let dir = Dir::new("desk-tree");
    for i in 0..5100 { std::fs::write(dir.join(&format!("f{i:05}.txt")), "").unwrap(); }
    let desk = d::Desk::new(std::sync::Arc::new(hover_agents::github::GitHubCli::new()), None);
    let files = desk.files(&snap(&dir.s(), vec![]));
    assert_eq!((files.tree.len(), files.more), (5000, true));
}

// MARK: Pull requests

const PR_JSON: &str = r#"out={"number":12,"title":"Fix the notch","state":"OPEN","isDraft":false,"url":"https://github.com/acme/app/pull/12","headRefName":"feat","baseRefName":"main","additions":10,"deletions":2,"changedFiles":3,"body":"Body text","author":{"login":"octocat"},"reviewDecision":"APPROVED","updatedAt":"2026-10-01T10:00:00Z","comments":[{},{}],"statusCheckRollup":[{"name":"build","conclusion":"SUCCESS","detailsUrl":"https://ci/1"},{"name":"lint","conclusion":"FAILURE"},{"context":"ci/legacy","state":"PENDING","targetUrl":"https://ci/2"},{"name":"docs","conclusion":"SKIPPED"},{"name":"test","status":"IN_PROGRESS","conclusion":""}]}"#;

fn ready(f: &Fake) {
    f.script("version", &["out=gh version 2.102.0 (2026-09-30)"]);
    f.script("auth_status", &["out=✓ Logged in to github.com account octocat (keyring)"]);
}

#[test]
fn the_pull_request_of_the_branch_is_read_through_gh() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    ready(&f);
    f.script("pr_view", &[PR_JSON]);
    let r = Repo::new("desk-pr");
    git(&r.repo, &["switch", "-q", "-c", "feat"]);
    let desk = f.desk();
    let s = r.snap();
    let d::PrPanel::Open(pr) = desk.pr(&s) else { panic!("{:?}", desk.pr(&s)) };
    assert_eq!((pr.number, pr.title.as_str(), pr.state.as_str(), pr.is_draft, pr.head.as_str(), pr.base.as_str()), (12, "Fix the notch", "open", false, "feat", "main"));
    assert_eq!((pr.additions, pr.deletions, pr.changed_files, pr.comments), (10, 2, 3, 2));
    assert_eq!((pr.author.as_deref(), pr.review.as_deref(), pr.body.as_str(), pr.url.as_str()), (Some("octocat"), Some("APPROVED"), "Body text", "https://github.com/acme/app/pull/12"));
    assert_eq!((pr.pass, pr.fail, pr.pending, pr.skip), (1, 1, 2, 1));
    assert_eq!(pr.checks.iter().map(|c| (c.name.as_str(), c.state.as_str())).collect::<Vec<_>>(), [("build", "pass"), ("lint", "fail"), ("ci/legacy", "pending"), ("docs", "skip"), ("test", "pending")]);
    assert_eq!(pr.checks[0].url.as_deref(), Some("https://ci/1"));
    assert_eq!(pr.checks[2].url.as_deref(), Some("https://ci/2"));
    let call = f.calls().into_iter().find(|c| c[0] == "pr").unwrap();
    assert_eq!(&call[..3], ["pr", "view", "--json"]);
    assert!(call[3].contains("statusCheckRollup"));

    let p = desk.probe(&s);
    assert!(p.gh && p.gh_auth);
    assert_eq!(p.gh_user.as_deref(), Some("octocat"));
    assert_eq!(p.pr, Some(d::PrBrief { number: 12, title: "Fix the notch".into(), state: "open".into(), is_draft: false }));
    assert_eq!(p.pr_reason, None);
}

#[test]
fn with_no_pull_request_the_form_starts_from_the_session() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    ready(&f);
    f.script("pr_view", &[r#"err=no pull requests found for branch "feature""#, "exit=1"]);
    let r = Repo::new("desk-nopr");
    let desk = f.desk();
    let s = r.snap();

    // On the default branch a new branch is suggested.
    let d::PrPanel::NoPr { message, create } = desk.pr(&s) else { panic!() };
    assert_eq!(message, "This branch has no pull request yet.");
    assert_eq!((create.branch.as_deref(), create.base.as_str(), create.on_default, create.suggest.as_deref(), create.ahead, create.changed), (Some("main"), "main", true, Some("hover/change-a"), 0, 0));
    assert_eq!((create.title.as_str(), create.body.as_str(), create.busy), ("Change a", "Made a two.", false));

    // On a branch with a commit ahead and a change not committed.
    git(&r.repo, &["switch", "-q", "-c", "feature"]);
    std::fs::write(r.repo.join("b.txt"), "b\n").unwrap();
    git(&r.repo, &["add", "."]);
    git(&r.repo, &["commit", "-q", "-m", "b"]);
    std::fs::write(r.repo.join("a.txt"), "two\n").unwrap();
    let desk = f.desk();
    let mut busy = r.snap();
    busy.busy = true;
    let c = desk.create_info(&busy);
    assert_eq!((c.branch.as_deref(), c.on_default, c.suggest, c.ahead, c.changed, c.busy), (Some("feature"), false, None, 1, 1, true));
    // A long first prompt is cut to a title of 72.
    let mut long = r.snap();
    long.texts[0] = format!("{}\nsecond line", "word ".repeat(30));
    let c = desk.create_info(&long);
    assert_eq!(c.title.chars().count(), 72);
    assert!(c.title.ends_with('…') && !c.title.contains('\n'));
}

#[test]
fn the_tab_asks_for_gh_before_it_shows_anything() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let r = Repo::new("desk-setup");
    let f = Fake::new();
    // Not a repository.
    let plain = Dir::new("desk-plain");
    let desk = f.desk();
    assert_eq!(desk.pr(&snap(&plain.s(), vec![])), d::PrPanel::Error("Not a Git repository.".into()));
    // gh isn't installed.
    let none = d::Desk::new(std::sync::Arc::new(hover_agents::github::GitHubCli::new().with(Some(plain.join("no-gh")), vec![])), hover_agents::desk::find_git());
    let d::PrPanel::Setup { need, message } = none.pr(&r.snap()) else { panic!() };
    assert_eq!((need, message.as_str()), (d::Setup::Install, "Install the GitHub CLI to see and open pull requests."));
    let p = none.probe(&r.snap());
    assert!(!p.gh && !p.gh_auth);
    assert_eq!(p.pr_reason.as_deref(), Some("Install the GitHub CLI (gh) to see pull requests."));
    // gh isn't signed in.
    f.script("version", &["out=gh version 2.102.0"]);
    f.script("auth_status", &["exit=1"]);
    let d::PrPanel::Setup { need, message } = f.desk().pr(&r.snap()) else { panic!() };
    assert_eq!((need, message.as_str()), (d::Setup::SignIn, "Sign in to GitHub to see and open pull requests."));
    let p = f.desk().probe(&r.snap());
    assert!(p.gh && !p.gh_auth);
    assert_eq!(p.pr_reason.as_deref(), Some("Sign in to GitHub to see pull requests."));
    assert!(!f.calls().iter().any(|c| c[0] == "pr"), "no pull request is asked for before gh is ready");
    // Signed in, but gh has another reason.
    ready(&f);
    f.script("pr_view", &["err=none of the git remotes configured for this repository point to a known GitHub host", "exit=1"]);
    assert_eq!(f.desk().pr(&r.snap()), d::PrPanel::Error("This repository has no GitHub remote.".into()));
}

#[test]
fn the_pull_requests_a_session_mentions_are_looked_up_each() {
    let f = Fake::new();
    ready(&f);
    f.script("pr_view_httpsgithubcomacmeapppull7", &[r#"out={"number":7,"title":"Add the desk","state":"MERGED","isDraft":false,"url":"https://github.com/acme/app/pull/7","additions":30,"deletions":4,"headRefName":"desk"}"#]);
    f.script("pr_view_httpsgithubcomacmeapppull12", &["err=GraphQL: Could not resolve to a PullRequest with the number of 12.", "exit=1"]);
    let mut pr = step("3", "execute", "Run", Some("gh pr create"), "completed");
    pr.log = Some("https://github.com/acme/app/pull/12".into());
    let s = snap("/w", vec![pr]);
    let desk = f.desk();
    let l = desk.linked(&s);
    assert!(l.gh);
    assert_eq!(l.prs.iter().map(|p| (p.repo.as_str(), p.number)).collect::<Vec<_>>(), [("acme/app", 12), ("acme/app", 7)]);
    assert_eq!(l.prs[0].error.as_deref(), Some("GraphQL: Could not resolve to a PullRequest with the number of 12."));
    assert_eq!(l.prs[0].title, None);
    let seven = &l.prs[1];
    assert_eq!((seven.title.as_deref(), seven.state.as_deref(), seven.additions, seven.deletions, seven.head.as_deref(), seven.error.as_deref()), (Some("Add the desk"), Some("merged"), 30, 4, Some("desk"), None));
    // Asked once each; a second reading within a minute is from what was kept.
    let asked = f.calls().iter().filter(|c| c[0] == "pr").count();
    assert_eq!(asked, 2);
    desk.linked(&s);
    assert_eq!(f.calls().iter().filter(|c| c[0] == "pr").count(), 2);
    // Without gh the mentions are listed as they are.
    let none = d::Desk::new(std::sync::Arc::new(hover_agents::github::GitHubCli::new().with(Some(std::path::PathBuf::from("/no/such/gh")), vec![])), None);
    let l = none.linked(&s);
    assert!(!l.gh);
    assert_eq!(l.prs.len(), 2);
    assert!(l.prs.iter().all(|p| p.state.is_none() && p.title.is_none() && p.error.is_none()));
}

#[test]
fn a_chat_shows_its_own_pull_request_and_a_kiro_web_chat_needs_no_folder_for_it() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    ready(&f);
    // The branch here has #12, but this chat opened #7 (and #3 is another repository's).
    f.script("pr_view", &[PR_JSON]);
    f.script("pr_view_httpsgithubcomacmeapppull7", &[r#"out={"number":7,"title":"Chat's own","state":"OPEN","isDraft":false,"url":"https://github.com/acme/app/pull/7","headRefName":"chat","baseRefName":"main","additions":5,"deletions":1,"changedFiles":2}"#]);
    f.script("pr_diff", &["out=diff --git a/a.txt b/a.txt", "out=--- a/a.txt", "out=+++ b/a.txt", "out=@@ -1 +1 @@", "out=-old", "out=+new"]);
    let r = Repo::new("desk-own-pr");
    git(&r.repo, &["switch", "-q", "-c", "feat"]);
    let mut made = step("9", "execute", "Run", Some("gh pr create --fill"), "completed");
    made.log = Some("https://github.com/acme/app/pull/7".into());
    let mut local = r.snap();
    local.steps = vec![d::Item { turn: 0, step: made.clone() }];
    let desk = f.desk();
    let d::PrPanel::Open(pr) = desk.pr(&local) else { panic!("{:?}", desk.pr(&local)) };
    assert_eq!((pr.number, pr.title.as_str()), (7, "Chat's own"), "the chat's pull request, not the branch's");

    // Kiro Web: its folder is not a repository, and only its own repos' links count.
    let plain = Dir::new("desk-cloud");
    let mut cloud = snap(&plain.s(), vec![]);
    cloud.cloud = Some(vec!["acme/app".into()]);
    cloud.texts = vec!["Fix it".into(), "See https://github.com/other/thing/pull/3 and https://github.com/acme/app/pull/7".into()];
    let d::PrPanel::Open(pr) = desk.pr(&cloud) else { panic!("{:?}", desk.pr(&cloud)) };
    assert_eq!(pr.number, 7);
    let p = desk.probe(&cloud);
    assert_eq!((p.pr.as_ref().map(|b| b.number), p.add, p.del, p.changed), (Some(7), 5, 1, 2), "its changes are the pull request's, not this folder's");
    let tiles = d::tiles(Some(&p), &cloud, &d::TileContext::default());
    assert!(["diff", "pr"].iter().all(|id| tiles.iter().find(|t| t.id == *id).unwrap().enabled));
    let diff = desk.diff(&cloud);
    assert_eq!(diff.files.iter().map(|x| (x.path.as_str(), x.add, x.del)).collect::<Vec<_>>(), [("a.txt", 1, 1)]);
    let call = f.calls().into_iter().find(|c| c.len() > 2 && c[..2] == ["pr", "diff"]).unwrap();
    assert_eq!(call[2], "https://github.com/acme/app/pull/7");

    // Before it has opened one, the panel says so and the diff is what it reported.
    let mut early = snap(&plain.s(), vec![]);
    early.cloud = Some(vec!["acme/app".into()]);
    early.texts = vec!["Fix it".into(), "Working on it.".into()];
    assert_eq!(desk.pr(&early), d::PrPanel::Error(d::CLOUD_NO_PR.into()));
    assert!(desk.diff(&early).partial);
}

// MARK: The tiles

#[test]
fn the_tiles_say_what_each_holds_and_why_one_is_grey() {
    let mut run = step("1", "execute", "Run", Some("ls"), "completed");
    run.exit = Some(0);
    let s = snap("/w", vec![run.clone(), run]);
    let ctx = d::TileContext::default();
    let by = |t: &[d::Tile], id: &str| t.iter().find(|x| x.id == id).unwrap().clone();

    // Before the probe is back the steps answer for what they can.
    let t = d::tiles(None, &s, &ctx);
    assert_eq!(t.iter().map(|x| (x.title, x.letter)).collect::<Vec<_>>(), [
        ("Browser", 'B'), ("Terminal", 'T'), ("Files", 'F'), ("Diff", 'D'), ("Pull request", 'P'), ("Linked pull requests", 'L'), ("Agents", 'A'), ("Screen", 'S')]);
    let terminal = by(&t, "terminal");
    assert_eq!((terminal.enabled, terminal.detail.as_str(), terminal.reason.as_str()), (true, "2 commands", ""));
    let linked = by(&t, "linked");
    assert_eq!((linked.enabled, linked.detail.as_str()), (true, "1 mentioned"), "the answer names a pull request");
    let agents = by(&t, "agents");
    assert_eq!((agents.enabled, agents.reason.as_str(), agents.detail.as_str()), (false, "Checking…", "None yet"));
    assert_eq!((by(&t, "pr").enabled, by(&t, "pr").detail.as_str()), (true, "…"));
    assert_eq!((by(&t, "browser").detail.as_str(), by(&t, "screen").detail.as_str(), by(&t, "files").detail.as_str()), ("Open a page", "Desktop", "Browse"));

    // With it.
    let p = d::Probe { folder: true, git_installed: true, git: true, branch: Some("main".into()), changed: 3, add: 1200, del: 4, gh: true, gh_auth: true, commands: 1, agents: 2, running: 1, linked: 0, ..Default::default() };
    let t = d::tiles(Some(&p), &s, &ctx);
    assert_eq!((by(&t, "files").detail.as_str(), by(&t, "diff").detail.as_str(), by(&t, "terminal").detail.as_str()), ("3 changed", "+1,200 −4", "1 command"));
    assert_eq!((by(&t, "agents").enabled, by(&t, "agents").detail.as_str()), (true, "1 working"));
    assert_eq!((by(&t, "linked").enabled, by(&t, "linked").reason.as_str(), by(&t, "linked").detail.as_str()), (false, "No pull requests mentioned in this session.", "None"));
    assert_eq!(by(&t, "pr").detail, "Open one");
    let mut q = p.clone();
    q.gh_auth = false;
    assert_eq!(by(&d::tiles(Some(&q), &s, &ctx), "pr").detail, "Sign in");
    q.gh = false;
    assert_eq!(by(&d::tiles(Some(&q), &s, &ctx), "pr").detail, "Set up GitHub");
    q.pr = Some(d::PrBrief { number: 12, title: "T".into(), state: "merged".into(), is_draft: false });
    assert_eq!(by(&d::tiles(Some(&q), &s, &ctx), "pr").detail, "#12 merged");
    q.pr.as_mut().unwrap().is_draft = true;
    q.pr.as_mut().unwrap().state = "open".into();
    assert_eq!(by(&d::tiles(Some(&q), &s, &ctx), "pr").detail, "#12 draft");

    // Not a repository, folder gone, no git.
    let not_git = d::Probe { folder: true, git_installed: true, commands: 2, ..Default::default() };
    let t = d::tiles(Some(&not_git), &s, &ctx);
    assert_eq!((by(&t, "pr").enabled, by(&t, "pr").reason.as_str(), by(&t, "pr").detail.as_str()), (false, "Not a Git repository.", "No repository"));
    assert_eq!((by(&t, "diff").enabled, by(&t, "diff").reason.as_str()), (false, "No changes yet."));
    let mut edited = s.clone();
    edited.steps.push(d::Item { turn: 0, step: step("e", "edit", "Edit", Some("a"), "completed") });
    assert!(by(&d::tiles(Some(&not_git), &edited, &ctx), "diff").enabled, "its own edits are a diff");
    let no_git = d::Probe { folder: true, git_installed: false, ..Default::default() };
    assert_eq!(by(&d::tiles(Some(&no_git), &s, &ctx), "pr").reason, "Git isn’t installed.");
    let gone = d::Probe::default();
    let t = d::tiles(Some(&gone), &s, &ctx);
    assert_eq!((by(&t, "files").enabled, by(&t, "files").reason.as_str()), (false, "The folder isn’t there any more."));
    assert_eq!(by(&t, "diff").reason, "The folder isn’t there any more.");

    // What this system can't run is grey, with its note, whatever the probe says.
    let off = [("browser", "Agent browser needs macOS."), ("screen", "Screen needs macOS.")];
    let t = d::tiles(Some(&p), &s, &d::TileContext { off: &off, ..Default::default() });
    assert_eq!((by(&t, "browser").enabled, by(&t, "browser").reason.as_str()), (false, "Agent browser needs macOS."));
    assert_eq!((by(&t, "screen").enabled, by(&t, "screen").reason.as_str()), (false, "Screen needs macOS."));
    assert!(by(&t, "terminal").enabled);

    // The browser tile says what is open, or the newest page; the screen tile says it is live.
    let pages = d::pages(&snap("/w", vec![step("f", "fetch", "Docs", Some("https://threejs.org/docs/"), "completed")]));
    assert_eq!(by(&d::tiles(Some(&p), &s, &d::TileContext { pages: &pages, ..Default::default() }), "browser").detail, "threejs.org/docs/");
    assert_eq!(by(&d::tiles(Some(&p), &s, &d::TileContext { browser_url: Some("http://localhost:5173/"), pages: &pages, ..Default::default() }), "browser").detail, "localhost:5173");
    let mut live = s.clone();
    live.busy = true;
    live.steps.push(d::Item { turn: 0, step: step("c", "other", "mcp__cua-driver__click", None, "in_progress") });
    assert_eq!(by(&d::tiles(Some(&p), &live, &ctx), "screen").detail, "Live");
    live.steps.push(d::Item { turn: 0, step: step("b", "other", "browser_open", None, "in_progress") });
    assert_eq!(by(&d::tiles(Some(&p), &live, &ctx), "browser").detail, "In use now");
}

//! SandboxTests (tests/Hover.Tests/SandboxTests.cs), ported: what srt's settings allow
//! and refuse, which folders a tool started for some covers, and how a tool's start is
//! wrapped. Nothing is run but the relay; the settings text and the argument list are
//! made from values given here, so they are checked on every OS.

use hover_agents::sandbox::{self, Boxed, Ctx, Fit};
use hover_core::json::{self, Json};
use hover_core::model::AgentTool;

fn ctx() -> Ctx {
    Ctx { home: "/Users/me".into(), support: "/Users/me/Library/Application Support/Hover".into(), macos: true, darwin_temp: Some("/var/folders/ab/cd/T".into()) }
}

fn list(c: &Json, a: &str, b: &str) -> Vec<String> {
    c.get(a).unwrap().get(b).unwrap().items().unwrap().iter().map(|x| x.as_str().unwrap().to_owned()).collect()
}

fn config(tool: AgentTool, folders: &[&str], sockets: &[&str], extra: &[&str]) -> Json {
    let folders: Vec<String> = folders.iter().map(|s| s.to_string()).collect();
    let sockets: Vec<String> = sockets.iter().map(|s| s.to_string()).collect();
    let extra: Vec<String> = extra.iter().map(|s| s.to_string()).collect();
    json::parse(&sandbox::config(tool, &folders, "/private/tmp/claude/hover-codex", &sockets, &extra, &ctx())).unwrap()
}

#[test]
fn the_settings_open_the_folders_and_close_the_rest() {
    let cua = sandbox::cua_socket(&ctx());
    let c = config(AgentTool::Codex, &["/work/project"], &["/private/tmp/claude/hover-codex", &cua], &[]);
    let write = list(&c, "filesystem", "allowWrite");
    for w in ["/work/project", "/Users/me/.codex", "/private/tmp/claude/hover-codex", "/var/folders/ab/cd/T"] { assert!(write.contains(&w.to_owned()), "{w} in {write:?}"); }
    assert!(!write.contains(&"/Users/me".to_owned()), "the home folder itself isn't writable");
    assert!(write.iter().all(|w| !w.contains("Library/Containers")));
    let deny = list(&c, "filesystem", "denyRead");
    for p in [".ssh", "Library/Group Containers", "Library/Containers", "Library/Mail", "Library/Messages", "Library/Cookies"] {
        assert!(deny.contains(&format!("/Users/me/{p}")), "{p}");
    }
    assert!(deny.contains(&"/Users/me/Library/Application Support/Hover".to_owned()), "Hover's own data");
    let read = list(&c, "filesystem", "allowRead");
    assert!(read.contains(&"/work/project".to_owned()));
    // Hover's own folder is closed, and the few things in it an agent needs are let through.
    for d in ["kiro-images", "cua", "browser", "mcp"] { assert!(read.contains(&format!("/Users/me/Library/Application Support/Hover/{d}")), "{d}"); }
    let domains = list(&c, "network", "allowedDomains");
    for d in ["api.openai.com", "registry.npmjs.org", "github.com", "localhost"] { assert!(domains.contains(&d.to_owned()), "{d}"); }
    assert!(!domains.contains(&"*.cursor.sh".to_owned()), "another tool's service");
    assert_eq!(list(&c, "network", "allowUnixSockets"), vec!["/private/tmp/claude/hover-codex".to_owned(), cua]);
    assert_eq!(c.get("allowAppleEvents"), Some(&Json::Bool(false)));
    assert_eq!(c.get("enableWeakerNetworkIsolation"), Some(&Json::Bool(true)), "a Mac's certificate service");
    assert_eq!(c.get("allowPty"), Some(&Json::Bool(true)));
    assert_eq!(c.get("network").unwrap().get("allowLocalBinding"), Some(&Json::Bool(true)));
}

#[test]
fn linux_has_no_weaker_isolation_and_no_darwin_temp() {
    let linux = Ctx { home: "/home/me".into(), support: "/home/me/.local/share/Hover".into(), macos: false, darwin_temp: None };
    let c = json::parse(&sandbox::config(AgentTool::Kiro, &[], "/tmp/claude/hover-kiro", &[], &[], &linux)).unwrap();
    assert_eq!(c.get("enableWeakerNetworkIsolation"), Some(&Json::Bool(false)));
    let write = list(&c, "filesystem", "allowWrite");
    assert!(write.contains(&"/home/me/.kiro".to_owned()) && write.contains(&"/tmp/claude/hover-kiro".to_owned()) && !write.iter().any(|w| w.starts_with("/var/folders")));
}

#[test]
fn each_tool_gets_its_own_service_and_claude_code_too() {
    let has = |t: AgentTool, d: &str| list(&config(t, &[], &[], &[]), "network", "allowedDomains").contains(&d.to_owned());
    assert!(has(AgentTool::Kiro, "*.amazonaws.com") && !has(AgentTool::Codex, "*.amazonaws.com"));
    assert!(has(AgentTool::Cursor, "*.cursor.sh") && has(AgentTool::OpenCode, "models.dev"));
    assert!(has(AgentTool::Claude, "api.anthropic.com") || has(AgentTool::Claude, "*.anthropic.com"));
    assert!(has(AgentTool::Claude, "github.com"), "every tool gets the registries and GitHub");
    let write = list(&config(AgentTool::Claude, &[], &[], &[]), "filesystem", "allowWrite");
    assert!(write.contains(&"/Users/me/.claude".to_owned()), "its state is writable");
}

#[test]
fn the_users_own_hosts_are_added_and_nonsense_is_dropped() {
    let text = "# mine\ndocs.example.com\n*.example.org  # wildcard\n*\nhttps://bad.example.com/x\n*.com\nlocalhost:8080\n";
    assert_eq!(sandbox::parse_extra(text), vec!["docs.example.com", "*.example.org", "localhost:8080"]);
    let c = config(AgentTool::Kiro, &[], &[], &["docs.example.com"]);
    let domains = list(&c, "network", "allowedDomains");
    assert!(domains.contains(&"docs.example.com".to_owned()) && domains.contains(&"*.amazonaws.com".to_owned()));
    // A host given twice (by case) is listed once.
    let twice = list(&config(AgentTool::Kiro, &[], &[], &["GitHub.com"]), "network", "allowedDomains");
    assert_eq!(twice.iter().filter(|d| d.eq_ignore_ascii_case("github.com")).count(), 1);
}

#[test]
fn a_tool_covers_its_folders_and_what_is_inside_them() {
    let a = vec!["/work/project".to_owned()];
    assert!(sandbox::covers(&a, "/work/project"));
    assert!(sandbox::covers(&a, "/work/project/sub"));
    assert!(sandbox::covers(&a, "/work/project/"));
    assert!(!sandbox::covers(&a, "/work/project-other"));
    assert!(!sandbox::covers(&a, "/work"));
    assert!(sandbox::covers(&["/".to_owned()], "/anything"));
}

#[test]
fn a_start_is_wrapped_in_srt_with_the_tools_own_arguments() {
    let args = sandbox::srt_args("/data/sandbox/kiro.json", "/t/hover-kiro", Some(("/usr/bin/perl", "/t/hover-kiro/relay.pl")), "/usr/local/bin/kiro-cli", &["acp", "--agent-engine", "v3"]);
    assert_eq!(args, vec!["--settings", "/data/sandbox/kiro.json", "--", "/usr/bin/env", "TMPDIR=/t/hover-kiro/", "/usr/bin/perl", "/t/hover-kiro/relay.pl",
        "/usr/local/bin/kiro-cli", "acp", "--agent-engine", "v3"]);
    // Without perl the tool runs directly.
    let plain = sandbox::srt_args("/s.json", "/t", None, "/bin/tool", &[]);
    assert_eq!(plain, vec!["--settings", "/s.json", "--", "/usr/bin/env", "TMPDIR=/t/", "/bin/tool"]);
    // The environment the sandbox adds tells the agent where it is, and keeps dotnet in one process.
    let env: Vec<&str> = sandbox::ENV.iter().map(|(k, _)| *k).collect();
    assert!(env.contains(&"HOVER_SANDBOXED") && env.contains(&"MSBUILDDISABLENODEREUSE"));
    assert!(sandbox::ENV.contains(&("HOVER_SANDBOXED", "1")));
}

#[test]
fn a_sandboxed_tool_that_no_longer_fits_is_started_again_only_when_idle() {
    let b = Boxed::default();
    // A process Hover didn't start (a test's stand-in) is left alone.
    assert_eq!(b.fit("/elsewhere", false, true), Fit::Fits);
    b.started(Some(vec!["/work/a".into()]));
    assert_eq!(b.fit("/work/a/sub", true, true), Fit::Fits, "inside its folders");
    assert_eq!(b.fit("/work/b", false, true), Fit::Restart, "another folder, nothing running");
    assert_eq!(b.fit("/work/b", true, true), Fit::Outside, "another folder, busy: it waits");
    // The sandbox switched off since: restart when idle, carry on when busy.
    assert_eq!(b.fit("/work/a", false, false), Fit::Restart);
    assert_eq!(b.fit("/work/a", true, false), Fit::Fits);
    // Started without one, and wanted now.
    b.started(None);
    assert_eq!(b.fit("/work/a", false, true), Fit::Restart);
    assert_eq!(b.fit("/work/a", false, false), Fit::Fits);
    assert!(sandbox::outside_message("Kiro").contains("Kiro is working on a task in another folder"));
}

#[test]
fn what_is_missing_is_said_in_one_line() {
    assert_eq!(sandbox::missing_line(&[]), None);
    let line = sandbox::missing_line(&[format!("npm install -g {}@{}", sandbox::PACKAGE, sandbox::VERSION), "brew install ripgrep".into()]).unwrap();
    assert_eq!(line, "Hover runs agents in a sandbox, which isn’t set up yet: npm install -g @anthropic-ai/sandbox-runtime@0.0.78, then brew install ripgrep. (Or turn the sandbox off in Settings.)");
}

#[test]
fn the_sandbox_is_for_macos_and_linux() {
    assert_eq!(sandbox::supported(), cfg!(any(target_os = "macos", target_os = "linux")));
    assert_eq!(sandbox::note(), if sandbox::supported() { None } else { Some("The sandbox needs macOS or Linux.") });
    assert_eq!(sandbox::UNSUPPORTED, "The sandbox needs macOS or Linux.");
}

#[cfg(windows)]
#[test]
fn on_windows_a_tool_starts_as_it_was_and_nothing_is_wanted() {
    assert!(!sandbox::wanted() && !sandbox::active() && sandbox::missing().is_none());
    let exe = std::path::Path::new(r"C:\tools\kiro-cli.exe");
    let s = sandbox::plan(AgentTool::Kiro, exe, &["acp"], &[("A".into(), "b".into())], &[]);
    assert_eq!((s.exe.as_path(), s.args.as_slice(), s.boxed), (exe, &["acp".to_owned()][..], false));
    assert_eq!(s.env, vec![("A".to_owned(), "b".to_owned())]);
    sandbox::remember(r"C:\work");
    assert!(sandbox::folders().is_empty(), "nothing is remembered where there is no sandbox");
}

/// srt's stdio is non-blocking: a tool's write past the pipe's 64 KB buffer failed with
/// EAGAIN and the tool died (Kiro with Cua Driver's 67 KB tool list). Through the relay the
/// same write arrives whole, and the tool's stdin closes with ours.
#[cfg(unix)]
#[test]
fn the_relay_carries_a_big_write_through_a_nonblocking_pipe() {
    use std::io::{BufRead, BufReader, Write};
    use std::process::{Command, Stdio};
    let have = |p: &str| std::path::Path::new(p).is_file();
    let python = ["/usr/bin/python3", "/usr/local/bin/python3", "/opt/homebrew/bin/python3"].into_iter().find(|p| have(p));
    let (true, Some(python)) = (have("/usr/bin/perl"), python) else { eprintln!("needs perl and python3"); return };
    let dir = std::env::temp_dir().join(format!("hover-relay-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let relay = dir.join("relay.pl");
    std::fs::write(&relay, sandbox::RELAY).unwrap();
    // Like srt: stdout made non-blocking, then 200 KB written in one go, after a line read from stdin.
    let mut p = Command::new("/usr/bin/perl")
        .args(["-e", "use Fcntl; for (0,1) { open(my $h,'+<&=',$_) or next; fcntl($h,F_SETFL,fcntl($h,F_GETFL,0)|O_NONBLOCK) } exec @ARGV", "/usr/bin/perl"])
        .arg(&relay).arg(python)
        .args(["-c", "import sys; sys.stdin.readline(); sys.stdout.write('x'*200000+'\\n'); sys.stdout.flush(); sys.stdin.read(); print('eof')"])
        .stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().unwrap();
    let mut stdin = p.stdin.take().unwrap();
    writeln!(stdin, "go").unwrap();
    stdin.flush().unwrap();
    // Read slowly, so the pipe fills up while the tool writes.
    std::thread::sleep(std::time::Duration::from_millis(300));
    let mut out = BufReader::new(p.stdout.take().unwrap());
    let mut line = String::new();
    out.read_line(&mut line).unwrap();
    drop(stdin);
    let mut rest = String::new();
    out.read_line(&mut rest).unwrap();
    let status = p.wait().unwrap();
    std::fs::remove_dir_all(&dir).ok();
    assert_eq!(line.trim_end().len(), 200000, "the whole write arrived");
    assert_eq!(rest.trim(), "eof", "our stdin's end reached the tool");
    assert!(status.success());
}

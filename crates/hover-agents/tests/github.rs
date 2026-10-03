//! The GitHub CLI's setup and Create pull request, against a stand-in gh (fixtures/
//! fakegh.rs) and real git repositories in temp folders: BrowserAndGitHubTests, ported,
//! and the sign-in, the cancel and the install that test never ran.

#[path = "fixtures/common.rs"]
mod common;

use common::*;
use hover_agents::desk::{CreatePrArgs, Desk, Snap};
use hover_agents::github::{self, GitHubCli, InstallPlan, Step};
use std::sync::Arc;
use std::time::Duration;

const SIGNED_IN: &[&str] = &["out=github.com", "out=  ✓ Logged in to github.com account octocat (keyring)", "out=  - Active account: true"];

fn signed_in(f: &Fake) {
    f.script("version", &["out=gh version 2.102.0 (2026-09-30)", "out=https://github.com/cli/cli/releases/tag/v2.102.0"]);
    f.script("auth_status", SIGNED_IN);
}

// MARK: Parsing (BrowserAndGitHubTests.Gh_output_gives_the_device_code_and_the_account)

#[test]
fn gh_output_gives_the_device_code_and_the_account() {
    assert_eq!(github::parse_code("! First copy your one-time code: 1A2B-3C4D").as_deref(), Some("1A2B-3C4D"));
    assert_eq!(github::parse_code("Open this URL to continue in your web browser: https://github.com/login/device"), None);
    // A code-shaped word on a line that isn't about the code is not the code.
    assert_eq!(github::parse_code("Updating ABCD-1234 in the keyring"), None);
    assert_eq!(github::parse_user("github.com\n  ✓ Logged in to github.com account octocat (keyring)\n  - Active account: true").as_deref(), Some("octocat"));
    assert_eq!(github::parse_user("✓ Logged in to github.com as mona-lisa (oauth_token)").as_deref(), Some("mona-lisa"));
    assert_eq!(github::parse_user("You are not logged into any GitHub hosts."), None);
}

#[test]
fn linux_is_told_the_line_for_its_package_manager_and_never_runs_it() {
    let line = |text: &str| github::linux_install_line(text);
    assert_eq!(line("NAME=\"Ubuntu\"\nID=ubuntu\nID_LIKE=debian\n"), "sudo apt install gh");
    assert_eq!(line("ID=linuxmint\nID_LIKE=\"ubuntu debian\""), "sudo apt install gh");
    assert_eq!(line("ID=fedora\n"), "sudo dnf install gh");
    assert_eq!(line("ID=rocky\nID_LIKE=\"rhel centos fedora\""), "sudo dnf install gh");
    assert_eq!(line("ID=arch\n"), "sudo pacman -S github-cli");
    assert_eq!(line("ID=opensuse-tumbleweed\nID_LIKE=\"opensuse suse\""), "sudo zypper install gh");
    assert_eq!(line("ID=alpine\n"), "sudo apk add github-cli");
    assert_eq!(line("ID=plan9\n"), "");
    assert_eq!(line(""), "");
}

#[test]
fn the_places_gh_is_looked_for_fit_the_system() {
    let places = github::known_places();
    assert!(!places.is_empty());
    assert!(places.iter().all(|p| p.is_absolute()), "{places:?}");
    if cfg!(windows) {
        assert!(places.iter().all(|p| p.ends_with("gh.exe")));
    } else if cfg!(target_os = "macos") {
        // A Mac app started from the Dock has no Homebrew on its PATH.
        assert!(places.iter().any(|p| p.starts_with("/opt/homebrew/bin")));
    } else {
        assert!(places.iter().any(|p| p.starts_with("/usr/bin")));
    }
    // One click installs where the system has winget (Windows) or Homebrew (a Mac) and says what to do elsewhere.
    let cli = GitHubCli::new();
    match cli.install_plan() {
        InstallPlan::Manual(hint) => { assert!(hint.contains("cli.github.com"), "{hint}"); assert!(!cli.can_install()); assert_eq!(cli.install_hint(), Some(hint)); }
        InstallPlan::Winget(_) => assert!(cfg!(windows) && cli.can_install() && cli.install_hint().is_none()),
        InstallPlan::Homebrew(_) => assert!(cfg!(target_os = "macos") && cli.can_install()),
        InstallPlan::Release(r) => assert!(cfg!(target_os = "macos") && cli.can_install() && r == hover_agents::setup::GH, "a Mac without Homebrew gets gh's own release"),
    }
    if cfg!(target_os = "linux") { assert!(matches!(cli.install_plan(), InstallPlan::Manual(_)), "Hover never installs with sudo"); }
}

// MARK: Status

#[test]
fn the_status_comes_from_gh_and_is_kept_a_minute() {
    let f = Fake::new();
    signed_in(&f);
    let cli = f.cli();
    let s = cli.check(false);
    assert_eq!((s.installed, s.signed_in, s.user.as_deref(), s.version.as_deref(), s.hint.as_str()), (true, true, Some("octocat"), Some("2.102.0"), ""));
    assert_eq!(cli.known(), Some(s.clone()));
    let calls = f.calls().len();
    // Signed out meanwhile: the kept answer stands until it is asked fresh.
    f.script("auth_status", &["err=You are not logged into any GitHub hosts. To log in, run: gh auth login", "exit=1"]);
    assert_eq!(cli.check(false), s);
    assert_eq!(f.calls().len(), calls, "no new call");
    let fresh = cli.check(true);
    assert_eq!((fresh.installed, fresh.signed_in, fresh.user.clone()), (true, false, None));
    assert_eq!(fresh.hint, "Sign in to GitHub to see and open pull requests.");
    assert_eq!(fresh.version.as_deref(), Some("2.102.0"));
    assert!(f.calls().iter().any(|c| c == &["auth", "status", "--hostname", "github.com"]));
}

#[test]
fn no_gh_is_not_installed() {
    let d = Dir::new("nogh");
    let cli = GitHubCli::new().with(Some(d.join("gh-is-not-here")), vec![]);
    let s = cli.check(true);
    assert_eq!((s.installed, s.signed_in), (false, false));
    assert_eq!(s.hint, "Install the GitHub CLI to see and open pull requests.");
    assert!(cli.exe().is_none());
}

// MARK: Sign in

fn wait_idle(cli: &GitHubCli) { wait_for("the setup to end", || !cli.busy()); }

#[test]
fn sign_in_shows_the_code_presses_enter_and_sets_git_up() {
    let f = Fake::new();
    f.script("version", &["out=gh version 2.102.0 (2026-09-30)"]);
    f.script("auth_status", &["err=You are not logged into any GitHub hosts.", "exit=1"]);
    // gh prints the code and waits for Enter (and the user); afterwards the account is signed in.
    f.script("auth_login", &[
        "sleep=300", "err=! First copy your one-time code: 1A2B-3C4D", "err=Press Enter to open github.com in your browser...", "sleep=1000", "read",
        "out=✓ Authentication complete.",
        r"write=script/auth_status.txt|out=✓ Logged in to github.com account octocat (keyring)\nexit=0",
    ]);
    f.script("auth_setup-git", &["sleep=300"]);
    let cli = f.cli();
    let seen: Arc<std::sync::Mutex<Vec<github::Progress>>> = Default::default();
    let (c2, s2) = (cli.clone(), seen.clone());
    cli.on_changed(move || s2.lock().unwrap().push(c2.setup()));
    assert!(cli.start());
    wait_for("the code", || cli.setup().code.is_some());
    let code = cli.setup();
    assert_eq!((code.step, code.code.as_deref(), code.url.as_deref(), code.error), (Some(Step::SigningIn), Some("1A2B-3C4D"), Some(github::DEVICE_URL), None));
    assert_eq!(code.line, "Enter the code at github.com/login/device, then come back.");
    assert_eq!(Step::SigningIn.name(), "signing-in");
    wait_idle(&cli);
    assert_eq!(cli.setup(), github::Progress::default(), "finished: nothing left to show");
    let k = cli.known().unwrap();
    assert_eq!((k.signed_in, k.user.as_deref()), (true, Some("octocat")));
    let calls = f.calls();
    assert!(calls.iter().any(|c| c == &["auth", "login", "--hostname", "github.com", "--git-protocol", "https", "--web"]), "{calls:?}");
    assert!(calls.iter().any(|c| c == &["auth", "setup-git", "--hostname", "github.com"]), "git is set to use gh: {calls:?}");
    let lines: Vec<String> = seen.lock().unwrap().iter().map(|p| p.line.clone()).collect();
    assert!(lines.iter().any(|l| l.contains("Asking GitHub for a sign-in code")), "{lines:?}");
    assert!(lines.iter().any(|l| l.contains("Setting up git to use your GitHub sign-in")), "{lines:?}");
    // Already signed in: nothing more to do, and nothing is started.
    assert!(cli.start());
    wait_idle(&cli);
    assert_eq!(f.calls().iter().filter(|c| c.get(1).map(String::as_str) == Some("login")).count(), 1);
}

#[test]
fn a_sign_in_that_fails_says_so_and_a_later_click_can_try_again() {
    let f = Fake::new();
    f.script("version", &["out=gh version 2.102.0"]);
    f.script("auth_status", &["exit=1"]);
    f.script("auth_login", &["err=! First copy your one-time code: AAAA-1111", "err=failed to authenticate", "exit=1"]);
    let cli = f.cli();
    cli.run_setup();
    let p = cli.setup();
    assert_eq!(p.error.as_deref(), Some("GitHub sign-in didn’t finish."));
    assert_eq!((p.step, p.code), (None, None));
    assert!(!cli.busy());
    cli.run_setup();
    assert_eq!(f.calls().iter().filter(|c| c.get(1).map(String::as_str) == Some("login")).count(), 2);
}

#[test]
fn cancel_ends_the_sign_in_and_the_gh_it_started() {
    let f = Fake::new();
    f.script("version", &["out=gh version 2.102.0"]);
    f.script("auth_status", &["exit=1"]);
    f.script("auth_login", &["err=! First copy your one-time code: 1A2B-3C4D", "hold"]);
    let cli = f.cli();
    assert!(cli.start());
    wait_for("the setup to start", || cli.busy());
    assert!(!cli.start(), "one setup at a time");
    wait_for("the code", || cli.setup().code.is_some());
    wait_for("gh to wait", || f.dir.join("alive.txt").exists());
    cli.cancel();
    wait_idle(&cli);
    assert_eq!(cli.setup(), github::Progress::default(), "a cancel is not an error");
    // gh is gone with it: its heartbeat has stopped.
    std::thread::sleep(Duration::from_millis(300));
    let a = std::fs::read_to_string(f.dir.join("alive.txt")).unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(std::fs::read_to_string(f.dir.join("alive.txt")).unwrap(), a, "still running");
    assert!(!f.calls().iter().any(|c| c.get(1).map(String::as_str) == Some("setup-git")), "git is not set up after a cancel");
}

// MARK: Install

#[test]
fn the_install_streams_its_lines_and_then_signs_in() {
    let f = Fake::new();
    // No gh yet: the stand-in installer (winget's place) makes it.
    let gh = f.gh();
    let winget = f.also("winget");
    std::fs::remove_file(&gh).unwrap();
    let copy = format!("copy=winget{EXE}|gh{EXE}");
    f.script("install", &["out=Found GitHub CLI [GitHub.cli] Version 2.102.0", "out=Successfully installed", &copy]);
    f.script("version", &["out=gh version 2.102.0"]);
    f.script("auth_status", SIGNED_IN);
    let cli = Arc::new(GitHubCli::new().with(Some(gh), git_env(f.dir.path())).with_install(InstallPlan::Winget(winget)));
    assert!(cli.can_install());
    let seen: Arc<std::sync::Mutex<Vec<(Option<Step>, String)>>> = Default::default();
    let (c2, s2) = (cli.clone(), seen.clone());
    cli.on_changed(move || { let p = c2.setup(); s2.lock().unwrap().push((p.step, p.line)); });
    assert!(!cli.check(true).installed);
    cli.run_setup();
    assert_eq!(cli.setup(), github::Progress::default());
    let seen = seen.lock().unwrap();
    assert!(seen.iter().any(|(s, l)| *s == Some(Step::Installing) && l == "Installing the GitHub CLI…"), "{seen:?}");
    assert!(seen.iter().any(|(s, l)| *s == Some(Step::Installing) && l == "Successfully installed"), "{seen:?}");
    assert!(cli.known().unwrap().installed && cli.known().unwrap().signed_in);
    let calls = f.calls();
    assert_eq!(calls[0][0], "install", "{calls:?}");
    assert!(calls[0].windows(2).any(|w| w == ["--id", "GitHub.cli"]));
    // Signed in already: no sign-in was started.
    assert!(!calls.iter().any(|c| c.get(1).map(String::as_str) == Some("login")));
}

#[test]
fn an_install_that_fails_says_why_and_without_a_package_manager_says_what_to_run() {
    let f = Fake::new();
    let gh = f.gh();
    let winget = f.also("winget");
    std::fs::remove_file(&gh).unwrap();
    f.script("install", &["err=No package found matching input criteria.", "exit=1"]);
    let cli = GitHubCli::new().with(Some(gh.clone()), vec![]).with_install(InstallPlan::Winget(winget));
    cli.run_setup();
    assert_eq!(cli.setup().error.as_deref(), Some("Couldn’t install the GitHub CLI: No package found matching input criteria."));
    assert!(!cli.busy());
    // By hand: Hover installs nothing and shows the hint.
    let by_hand = GitHubCli::new().with(Some(gh), vec![]).with_install(InstallPlan::Manual("Install it with `sudo apt install gh`.".into()));
    assert!(!by_hand.can_install());
    by_hand.run_setup();
    assert_eq!(by_hand.setup().error.as_deref(), Some("Install it with `sudo apt install gh`."));
}

// MARK: Running a program

#[test]
fn a_program_is_stopped_at_its_time_and_at_its_cap_and_takes_its_input_on_stdin() {
    let f = Fake::new();
    let cli = f.cli();
    let gh = f.gh();
    let env = cli.env().to_vec();
    f.script("slow_run", &["hold"]);
    let t = std::time::Instant::now();
    let r = github::run(&gh, None, Duration::from_millis(400), 1024, &["slow", "run"], None, &env);
    assert_eq!((r.code, r.err.as_str()), (-1, "Timed out."));
    assert!(t.elapsed() < Duration::from_secs(5));
    // Its output past the cap ends it; what was read stays, and the code is 0.
    f.script("big_out", &["flood=100000"]);
    let r = github::run(&gh, None, Duration::from_secs(20), 10_000, &["big", "out"], None, &env);
    assert!(r.capped && r.code == 0 && r.out.len() == 10_000, "{} {} {}", r.capped, r.code, r.out.len());
    // Long text goes in on stdin, not on a command line.
    f.script("pr_create", &["readall", "out=done"]);
    let body = "x".repeat(300_000);
    let r = github::run(&gh, None, Duration::from_secs(20), 1024, &["pr", "create", "--body-file", "-"], Some(body.as_bytes()), &env);
    assert_eq!((r.code, r.out.trim()), (0, "done"));
    assert_eq!(f.stdin_of("pr_create").unwrap().len(), 300_000);
    // A program that can't start is a failure with its reason, not a panic.
    let r = github::run(&f.dir.join("nothing-here"), None, Duration::from_secs(5), 1024, &[], None, &env);
    assert_eq!(r.code, -1);
    assert!(!r.err.is_empty());
}

// MARK: Create pull request

fn args(title: &str) -> CreatePrArgs { CreatePrArgs { title: title.into(), ..Default::default() } }

#[test]
fn create_pull_request_makes_the_branch_commits_pushes_and_opens_it() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    f.script("pr_create", &["readall", "out=https://github.com/acme/app/pull/12"]);
    let desk = f.desk();
    let r = Repo::new("create");
    std::fs::write(r.repo.join("a.txt"), "two\n").unwrap();
    let a = CreatePrArgs { title: "Change a".into(), body: "Made a two.".into(), branch: Some("hover/change-a".into()), commit: true, draft: true, ..Default::default() };
    let out = desk.create_pr(&r.snap(), &a);
    assert_eq!(out.error, None);
    assert!(out.ok);
    assert_eq!(out.url.as_deref(), Some("https://github.com/acme/app/pull/12"));
    assert_eq!(out.steps, ["Made branch hover/change-a", "Committed the changes", "Pushed hover/change-a to origin"]);
    assert!(git(&r.remote, &["branch", "--list", "hover/change-a"]).contains("hover/change-a"), "the branch was pushed");
    assert_eq!(git(&r.repo, &["status", "--porcelain"]), "", "the change was committed");
    // The commit message came in on stdin, with the title first.
    assert_eq!(git(&r.repo, &["log", "-1", "--format=%B"]).trim_end(), "Change a\n\nCommitted from Hover.");
    let gh = f.calls().into_iter().find(|c| c[0] == "pr" && c[1] == "create").unwrap();
    // Neither the description nor the commit message is an argument.
    assert_eq!(gh, ["pr", "create", "--title", "Change a", "--body-file", "-", "--base", "main", "--head", "hover/change-a", "--draft"]);
    assert_eq!(f.stdin_of("pr_create").as_deref(), Some("Made a two."));
}

#[test]
fn an_empty_description_is_the_title_and_a_pull_request_is_not_a_draft_unless_asked() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    f.script("pr_create", &["readall", "out=Creating pull request for feature into main in acme/app", "out=", "out=https://github.com/acme/app/pull/5"]);
    let r = Repo::new("create-plain");
    git(&r.repo, &["switch", "-q", "-c", "feature"]);
    let out = f.desk().create_pr(&r.snap(), &args("Just the title"));
    assert_eq!((out.error, out.ok, out.url.as_deref()), (None, true, Some("https://github.com/acme/app/pull/5")));
    assert_eq!(out.steps, ["Pushed feature to origin"], "nothing to commit, no new branch");
    let gh = f.calls().into_iter().find(|c| c[0] == "pr").unwrap();
    assert!(!gh.contains(&"--draft".to_owned()));
    assert_eq!(f.stdin_of("pr_create").as_deref(), Some("Just the title"));
}

#[test]
fn create_pull_request_says_no_before_it_changes_anything() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    f.script("pr_create", &["out=https://github.com/acme/app/pull/1"]);
    let desk = f.desk();
    let r = Repo::new("create-no");
    std::fs::write(r.repo.join("a.txt"), "two\n").unwrap();
    let branches = || git(&r.repo, &["branch", "--list"]);
    let err = |a: &CreatePrArgs, snap: &Snap| desk.create_pr(snap, a).error.unwrap_or_else(|| "it went ahead".into());

    // Never while the agent works in the folder.
    let mut busy = r.snap();
    busy.busy = true;
    assert_eq!(err(&args("x"), &busy), "Wait for the agent to finish first: it is still working in this folder.");
    assert_eq!(err(&args("   "), &r.snap()), "Give the pull request a title.");
    // A branch name that could be an option, or isn't a name at all, is refused, as a base is ignored.
    let bad = CreatePrArgs { branch: Some("--evil".into()), ..args("x") };
    assert_eq!(err(&bad, &r.snap()), "That branch name isn’t valid.");
    assert_eq!(err(&CreatePrArgs { branch: Some("has space".into()), ..args("x") }, &r.snap()), "That branch name isn’t valid.");
    assert_eq!(err(&CreatePrArgs { branch: Some("a..b".into()), ..args("x") }, &r.snap()), "That branch name isn’t valid.");
    // On the default branch without a new branch there is nothing to open.
    assert_eq!(err(&args("x"), &r.snap()), "The pull request needs a branch other than main.");
    // A base that isn't a branch name falls back to the default one.
    assert_eq!(err(&CreatePrArgs { base: Some("--upload-pack=evil".into()), ..args("x") }, &r.snap()), "The pull request needs a branch other than main.");
    // Not a repository.
    let plain = Dir::new("create-plain-dir");
    let snap = Snap { folder: plain.s(), ..Default::default() };
    assert_eq!(err(&args("x"), &snap), "Not a Git repository.");

    assert_eq!(branches().trim(), "* main", "no branch was made");
    assert_eq!(git(&r.repo, &["status", "--porcelain"]).trim(), "M a.txt", "nothing was committed");
    assert!(!f.calls().iter().any(|c| c[0] == "pr"), "gh was never asked");
}

#[test]
fn a_step_that_fails_comes_back_as_its_reason_with_the_steps_before_it() {
    if !git_available() { eprintln!("git isn't installed"); return; }
    let f = Fake::new();
    let desk = f.desk();
    let r = Repo::new("create-fail");
    std::fs::write(r.repo.join("a.txt"), "two\n").unwrap();
    let a = CreatePrArgs { title: "Change a".into(), branch: Some("hover/change-a".into()), commit: true, ..Default::default() };

    // The push fails: the remote is gone.
    git(&r.repo, &["remote", "set-url", "origin", &r.root.join("gone.git").to_string_lossy()]);
    let out = desk.create_pr(&r.snap(), &a);
    assert!(!out.ok && out.url.is_none());
    assert!(out.error.as_deref().unwrap().starts_with("Couldn’t push the branch: "), "{:?}", out.error);
    assert_eq!(out.steps, ["Made branch hover/change-a", "Committed the changes"]);

    // The push works and gh says no.
    git(&r.repo, &["remote", "set-url", "origin", &r.remote.to_string_lossy()]);
    f.script("pr_create", &["readall", "err=GraphQL: Resource not accessible by personal access token", "exit=1"]);
    let out = desk.create_pr(&r.snap(), &CreatePrArgs { branch: None, ..a.clone() });
    assert_eq!(out.error.as_deref(), Some("gh couldn’t open the pull request: GraphQL: Resource not accessible by personal access token"));
    assert_eq!(out.steps, ["Pushed hover/change-a to origin"], "already committed and on the branch");

    // No gh at all.
    let none = Desk::new(Arc::new(GitHubCli::new().with(Some(r.root.join("no-gh")), git_env(f.dir.path()))), hover_agents::desk::find_git());
    assert_eq!(none.create_pr(&r.snap(), &a).error.as_deref(), Some("Install the GitHub CLI first."));
}

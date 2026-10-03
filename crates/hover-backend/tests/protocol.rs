//! The built `hover-backend` driven over its stdin and stdout, as the Mac app's Swift
//! drives it (tests/macos/backend-smoke.py, ported): a data folder of its own, a PATH of
//! stand-ins (tools/hover-measure's fake-agent as codex-acp, kiro-cli and cua-driver,
//! fake-opencode as opencode, and hover-agents' scripted gh), and git repositories in
//! temp folders. Nothing here reaches the network, the user's tools or their data.

use hover_core::json::{self, Json};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{channel, Receiver, RecvTimeoutError};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

const EXE: &str = if cfg!(windows) { ".exe" } else { "" };
const WAIT: Duration = Duration::from_secs(40);

// MARK: The stand-ins

struct Tools { agent: PathBuf, opencode: PathBuf, gh: PathBuf }

fn manifest() -> PathBuf { PathBuf::from(env!("CARGO_MANIFEST_DIR")) }

/// fake-agent and fake-opencode from tools/hover-measure, built (or found built) in this
/// run's own target folder; gh's stand-in compiled by rustc from hover-agents' fixtures.
fn tools() -> &'static Tools {
    static T: OnceLock<Tools> = OnceLock::new();
    T.get_or_init(|| {
        // target/<profile>/deps/protocol-<hash>: the profile folder is two up.
        let exe = std::env::current_exe().unwrap();
        let profile_dir = exe.parent().unwrap().parent().unwrap().to_path_buf();
        let target = profile_dir.parent().unwrap().to_path_buf();
        let profile = match profile_dir.file_name().unwrap().to_str().unwrap() { "debug" => "dev".to_owned(), p => p.to_owned() };
        let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
        let built = Command::new(cargo).args(["build", "-p", "hover-measure", "--bin", "fake-agent", "--bin", "fake-opencode", "--profile", &profile])
            .env("CARGO_TARGET_DIR", &target).current_dir(manifest()).output().expect("cargo runs");
        assert!(built.status.success(), "building the stand-in tools: {}", String::from_utf8_lossy(&built.stderr));
        let agent = profile_dir.join(format!("fake-agent{EXE}"));
        let opencode = profile_dir.join(format!("fake-opencode{EXE}"));
        assert!(agent.is_file() && opencode.is_file(), "the stand-in tools are in {}", profile_dir.display());
        Tools { agent, opencode, gh: build_gh() }
    })
}

/// hover-agents' scripted gh, built once per machine (and when its source is newer).
fn build_gh() -> PathBuf {
    let out = std::env::temp_dir().join("hover-backend-fakegh-build");
    std::fs::create_dir_all(&out).unwrap();
    let src = manifest().join("../hover-agents/tests/fixtures/fakegh.rs");
    let exe = out.join(format!("fakegh{EXE}"));
    let newer = |a: &Path, b: &Path| std::fs::metadata(a).and_then(|m| m.modified()).ok() > std::fs::metadata(b).and_then(|m| m.modified()).ok();
    if exe.is_file() && !newer(&src, &exe) { return exe; }
    let mine = out.join(format!("fakegh-{}{EXE}", std::process::id()));
    let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
    let r = Command::new(rustc).args(["--edition", "2021", "-C", "debuginfo=0"]).arg(&src).arg("-o").arg(&mine).output().expect("rustc runs");
    assert!(r.status.success(), "building the stand-in gh: {}", String::from_utf8_lossy(&r.stderr));
    if std::fs::rename(&mine, &exe).is_ok() { exe } else { mine }
}

// MARK: A sandbox

static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// A folder of a test's own: the backend's data, a home, the PATH of stand-ins, and a
/// project that is a git repository with one uncommitted change.
struct Sandbox { root: PathBuf }

impl Sandbox {
    fn new(name: &str) -> Sandbox {
        let root = std::env::temp_dir().join(format!("hover-backend-{name}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&root);
        for d in ["data", "home", "bin", "project"] { std::fs::create_dir_all(root.join(d)).unwrap(); }
        let t = tools();
        for name in ["codex-acp", "kiro-cli", "cua-driver"] { std::fs::copy(&t.agent, root.join("bin").join(format!("{name}{EXE}"))).unwrap(); }
        std::fs::copy(&t.opencode, root.join("bin").join(format!("opencode{EXE}"))).unwrap();
        std::fs::copy(&t.gh, root.join("bin").join(format!("gh{EXE}"))).unwrap();
        std::fs::create_dir_all(root.join("bin/script")).unwrap();
        std::fs::write(root.join("gitconfig"), "").unwrap();
        let sb = Sandbox { root };
        if hover_agents::desk::find_git().is_some() {
            std::fs::write(sb.project().join("app.js"), "a\n").unwrap();
            sb.git(&["init", "-q"]);
            sb.git(&["add", "."]);
            sb.git(&["commit", "-qm", "init"]);
            std::fs::write(sb.project().join("app.js"), "a\nb\n").unwrap();
        }
        sb
    }

    fn project(&self) -> PathBuf { self.root.join("project") }
    fn data(&self) -> PathBuf { self.root.join("data") }
    fn bin(&self) -> PathBuf { self.root.join("bin") }
    fn folder(&self) -> String { self.project().to_string_lossy().into_owned() }

    fn git_env(&self) -> Vec<(&'static str, String)> {
        vec![("GIT_CONFIG_GLOBAL", self.root.join("gitconfig").to_string_lossy().into_owned()), ("GIT_CONFIG_NOSYSTEM", "1".into()),
            ("GIT_AUTHOR_NAME", "Test".into()), ("GIT_AUTHOR_EMAIL", "t@example.com".into()),
            ("GIT_COMMITTER_NAME", "Test".into()), ("GIT_COMMITTER_EMAIL", "t@example.com".into())]
    }

    fn git(&self, args: &[&str]) {
        let mut c = Command::new(hover_agents::desk::find_git().expect("git"));
        c.args(args).current_dir(self.project());
        for (k, v) in self.git_env() { c.env(k, v); }
        let r = c.output().unwrap();
        assert!(r.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&r.stderr));
    }

    /// What `gh <words>` does, as hover-agents' stand-in reads it.
    fn gh_script(&self, key: &str, lines: &[&str]) { std::fs::write(self.bin().join(format!("script/{key}.txt")), lines.join("\n")).unwrap(); }

    fn start(&self) -> Run { self.start_with(&[]) }

    fn start_with(&self, env: &[(&str, &str)]) -> Run {
        let home = self.root.join("home");
        // PATH's own separator can't be inside one entry: each system folder is its own.
        let system: Vec<PathBuf> = match std::env::var("SystemRoot") {
            Ok(s) => vec![PathBuf::from(format!("{s}\\System32"))],
            Err(_) => vec![PathBuf::from("/usr/bin"), PathBuf::from("/bin")],
        };
        let path = std::env::join_paths(std::iter::once(self.bin()).chain(system)).unwrap();
        let mut c = Command::new(env!("CARGO_BIN_EXE_hover-backend"));
        c.env("HOVER_DATA_DIR", self.data()).env("PATH", path).env("USERPROFILE", &home).env("HOME", &home)
            .env("LOCALAPPDATA", home.join("local")).env("APPDATA", home.join("roaming")).env("XDG_RUNTIME_DIR", &home).env_remove("CODEX_HOME").env_remove("CLAUDE_CONFIG_DIR");
        for (k, v) in self.git_env() { c.env(k, v); }
        for (k, v) in env { c.env(k, v); }
        let mut child = c.stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::inherit()).spawn().expect("hover-backend starts");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().unwrap();
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                match json::parse(&line) { Ok(m) => { let _ = tx.send(m); } Err(e) => panic!("the backend wrote a line that is not JSON ({e}): {line}") }
            }
        });
        Run { child, stdin, rx, seen: vec![], bin: self.bin() }
    }

    /// Programs of this sandbox's bin folder that are still running.
    fn running(&self) -> usize { running_from(&self.bin()) }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        for _ in 0..30 {
            if std::fs::remove_dir_all(&self.root).is_ok() || !self.root.exists() { return; }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

#[cfg(windows)]
fn running_from(dir: &Path) -> usize {
    let script = format!("@(Get-Process | Where-Object {{ $_.Path -and $_.Path.StartsWith('{}', [StringComparison]::OrdinalIgnoreCase) }}).Count", dir.display());
    let r = Command::new("powershell").args(["-NoProfile", "-Command", &script]).output().unwrap();
    String::from_utf8_lossy(&r.stdout).trim().parse().unwrap_or(0)
}

#[cfg(not(windows))]
fn running_from(dir: &Path) -> usize {
    let r = Command::new("pgrep").args(["-f", &dir.to_string_lossy()]).output().unwrap();
    String::from_utf8_lossy(&r.stdout).lines().count()
}

// MARK: A run of the backend

struct Run { child: Child, stdin: Option<ChildStdin>, rx: Receiver<Json>, seen: Vec<Json>, bin: PathBuf }

fn key_of(byte: u8) -> [u8; 32] { std::array::from_fn(|i| if byte == 0 { i as u8 } else { byte }) }

fn jo(props: Vec<(&str, Json)>) -> Json { Json::obj(props) }
fn js(s: &str) -> Json { Json::str(s) }
fn ty(t: &str) -> Json { jo(vec![("type", js(t))]) }
fn get<'a>(m: &'a Json, k: &str) -> &'a Json { m.get(k).unwrap_or_else(|| panic!("no `{k}` in {}", m.compact())) }
fn text<'a>(m: &'a Json, k: &str) -> &'a str { get(m, k).as_str().unwrap_or_else(|| panic!("`{k}` is not a string in {}", m.compact())) }
fn flag(m: &Json, k: &str) -> bool { matches!(get(m, k), Json::Bool(true)) }
fn int(m: &Json, k: &str) -> i64 { match get(m, k) { Json::Num(n) => n.parse::<f64>().unwrap() as i64, o => panic!("`{k}` is {}", o.compact()) } }
fn list<'a>(m: &'a Json, k: &str) -> &'a [Json] { get(m, k).items().unwrap_or_else(|_| panic!("`{k}` is not a list in {}", m.compact())) }
fn is(m: &Json, t: &str) -> bool { m.get("type").and_then(Json::as_str) == Some(t) }

impl Run {
    fn send(&mut self, m: Json) {
        let w = self.stdin.as_mut().expect("stdin is open");
        writeln!(w, "{}", m.compact()).unwrap();
        w.flush().unwrap();
    }

    fn send_raw(&mut self, line: &str) {
        let w = self.stdin.as_mut().expect("stdin is open");
        writeln!(w, "{line}").unwrap();
        w.flush().unwrap();
    }

    /// The first message after this point for which `f` holds. A toast or a backend failure
    /// that isn't what is asked for fails the test, as backend-smoke.py's `until` does.
    fn until(&mut self, what: &str, f: impl Fn(&Json) -> bool) -> Json {
        let end = Instant::now() + WAIT;
        loop {
            let left = end.saturating_duration_since(Instant::now());
            match self.rx.recv_timeout(left) {
                Ok(m) => {
                    self.seen.push(m.clone());
                    if f(&m) { return m; }
                    if is(&m, "toast") || is(&m, "backendFailure") { panic!("waiting for {what}: {}", m.compact()); }
                }
                Err(RecvTimeoutError::Timeout) => panic!("timed out waiting for {what}; saw {}", self.seen.iter().rev().take(6).map(Json::compact).collect::<Vec<_>>().join("\n")),
                Err(RecvTimeoutError::Disconnected) => panic!("the backend ended while waiting for {what} ({:?})", self.child.try_wait()),
            }
        }
    }

    fn message(&mut self, t: &'static str) -> Json { self.until(t, |m| is(m, t)) }

    fn state(&mut self, what: &str, f: impl Fn(&Json) -> bool) -> Json { self.until(what, |m| is(m, "state") && f(m)) }

    /// `initialize` with a key, and `initialized` back.
    fn initialize(&mut self, key: &[u8; 32]) -> Json {
        self.send(jo(vec![("type", js("initialize")), ("key", js(&hover_agents::http::base64(key)))]));
        self.message("initialized")
    }

    fn ready_tool(&mut self, id: &str) -> Json {
        self.send(ty("ready"));
        let id = id.to_owned();
        self.state("the tool ready", move |m| list(m, "tools").iter().any(|t| text(t, "id") == id && flag(t, "ready")))
    }

    /// The host hangs up (EOF): the backend ends, with code 0.
    fn hang_up(&mut self) -> std::process::ExitStatus {
        self.stdin.take();
        self.exit()
    }

    fn exit(&mut self) -> std::process::ExitStatus {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(s) = self.child.try_wait().unwrap() { return s; }
            assert!(Instant::now() < end, "the backend did not exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    fn no_tools_left(&self) {
        let end = Instant::now() + Duration::from_secs(10);
        while running_from(&self.bin) > 0 && Instant::now() < end { std::thread::sleep(Duration::from_millis(200)); }
        assert_eq!(running_from(&self.bin), 0, "a tool outlived the backend");
    }
}

impl Drop for Run {
    fn drop(&mut self) { let _ = self.child.kill(); let _ = self.child.wait(); }
}

/// The settings a first task needs (the access notice), and a tool for it.
fn accept_notice(r: &mut Run) {
    r.send(jo(vec![("type", js("saveSettings")), ("noticeSeen", Json::Bool(true))]));
    r.message("preferences");
}

fn new_task(r: &mut Run, tool: &str, folder: &str, prompt: &str, access: &str) {
    r.send(jo(vec![("type", js("new")), ("tool", js(tool)), ("folder", js(folder)), ("prompt", js(prompt)), ("access", js(access))]));
}

fn first_session(m: &Json) -> &Json { list(m, "sessions").first().unwrap_or_else(|| panic!("no session in {}", m.compact())) }

fn history_bytes(data: &Path) -> Vec<u8> {
    let mut all = vec![];
    let mut files: Vec<_> = std::fs::read_dir(data.join("agents")).into_iter().flatten().flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "dat")).collect();
    files.sort();
    for f in files { all.extend(std::fs::read(f).unwrap()); }
    all
}

// MARK: Tests

/// backend-smoke.py's first half: the preferences a host saves come back, persist, and
/// Cua Driver is checked (the stand-in answers as installed and granted).
#[test]
fn preferences_round_trip_and_computer_use() {
    let sb = Sandbox::new("prefs");
    let mut r = sb.start();
    let init = r.initialize(&key_of(0));
    assert_eq!(int(&init, "version"), 1);
    r.send(ty("getSettings"));
    let first = r.message("preferences");
    assert!(!flag(&first, "noticeSeen") && !flag(&first, "computerUse"), "{}", first.compact());
    assert_eq!(int(&first, "maxRunning"), 3);
    assert!(flag(&first, "hover") && flag(&first, "sandbox") && flag(&first, "agentBrowser"));
    // Agent desktops (Cua Spaces): off, on the macOS image, and only a Mac 26 on Apple silicon can run them.
    assert!(!flag(&first, "agentSpaces") && text(&first, "spaceImage") == "macos", "{}", first.compact());
    assert_eq!(flag(&first, "spacesSupported"), hover_agents::spaces::supported());
    assert_eq!(list(&first, "tools").len(), 5, "Kiro, Codex, Cursor, OpenCode and Claude Code");
    r.send(jo(vec![("type", js("saveSettings")), ("noticeSeen", Json::Bool(false)), ("hover", Json::Bool(true))]));
    let same = r.message("preferences");
    assert!(!flag(&same, "noticeSeen") && !flag(&same, "computerUse"));

    let tools = Json::Arr(vec![jo(vec![("id", js("codex")), ("access", js("always")), ("idle", Json::int(15)), ("hideSteps", Json::Bool(true))])]);
    r.send(jo(vec![("type", js("saveSettings")), ("noticeSeen", Json::Bool(true)), ("hover", Json::Bool(false)), ("maxRunning", Json::int(4)),
        ("quotaItems", Json::Arr(vec![])), ("computerUse", Json::Bool(true)), ("agentBrowser", Json::Bool(false)), ("tools", tools)]));
    let prefs = r.message("preferences");
    assert!(flag(&prefs, "noticeSeen") && !flag(&prefs, "hover") && flag(&prefs, "computerUse") && !flag(&prefs, "agentBrowser"), "{}", prefs.compact());
    assert_eq!(int(&prefs, "maxRunning"), 4);
    let codex = list(&prefs, "tools").iter().find(|t| text(t, "id") == "codex").unwrap();
    assert_eq!((text(codex, "access"), int(codex, "idle"), flag(codex, "hideSteps")), ("always", 15, true));

    r.send(ty("computerUse"));
    let cu = r.until("the computer use check", |m| is(m, "computerUse") && flag(m, "checked"));
    if hover_agents::computer_use::supported() {
        assert!(flag(&cu, "on") && flag(&cu, "installed") && flag(&cu, "ready"), "{}", cu.compact());
        assert_eq!((text(&cu, "permissions"), text(&cu, "version")), ("granted", "fake-agent 1.0.0"));
    } else {
        // Cua is kept to the Mac: the stand-in on PATH is not asked, and the hint says why.
        assert!(flag(&cu, "on") && !flag(&cu, "installed") && !flag(&cu, "ready"), "{}", cu.compact());
        assert_eq!((text(&cu, "permissions"), text(&cu, "version"), text(&cu, "hint")), ("unknown", "", "Computer use needs macOS."));
    }
    assert!(get(&cu, "installHint").as_str().is_some() && get(&cu, "canGrant") != &Json::Null);

    r.send(jo(vec![("type", js("saveSettings")), ("quotaItems", Json::Arr(vec![js("kiro"), js("nonsense")]))]));
    assert_eq!(list(&r.message("preferences"), "quotaItems"), &[js("kiro")], "only the notch's own quotas");

    // Kiro's auto compact: off until switched on, at 80 % unless told otherwise.
    assert!(!flag(&prefs, "kiroAutoCompact") && int(&prefs, "kiroCompactAt") == 80, "{}", prefs.compact());
    r.send(jo(vec![("type", js("saveSettings")), ("kiroAutoCompact", Json::Bool(true)), ("kiroCompactAt", Json::int(60))]));
    let compact = r.message("preferences");
    assert!(flag(&compact, "kiroAutoCompact") && int(&compact, "kiroCompactAt") == 60);
    r.send(jo(vec![("type", js("saveSettings")), ("kiroCompactAt", Json::int(500))]));
    assert_eq!(int(&r.message("preferences"), "kiroCompactAt"), 100, "a percent, at most 100");
    r.send(jo(vec![("type", js("saveSettings")), ("kiroCompactAt", Json::int(60))]));
    r.message("preferences");
    assert!(r.hang_up().success());

    // Persisted: a new run reads the same preferences.
    let mut again = sb.start();
    again.initialize(&key_of(0));
    again.send(ty("getSettings"));
    let p2 = again.message("preferences");
    assert_eq!(p2.compact().replace(r#""quotaItems":["kiro"]"#, r#""quotaItems":[]"#).replace(r#""kiroAutoCompact":true"#, r#""kiroAutoCompact":false"#).replace(r#""kiroCompactAt":60"#, r#""kiroCompactAt":80"#), prefs.compact());
    assert!(flag(&p2, "kiroAutoCompact") && int(&p2, "kiroCompactAt") == 60);
    assert!(again.hang_up().success());
}

/// Agent desktops (Program.cs's `spaces`, `spaceView`, `teleport` and `spaceFiles`) where Cua
/// Spaces can't run: Settings' switch is remembered but not wanted, the office says there
/// are no desktops, the setup says why it can't, and the drops are answered with Cua's absence.
/// (On a Mac 26 with Cua installed the same commands make and drive real desktops; nothing
/// here needs one.)
#[test]
fn agent_desktops_are_off_with_their_note_and_their_commands_answer() {
    let sb = Sandbox::new("spaces");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);

    // Settings → Computer Use asks: what is known comes at once, the check after it.
    r.send(jo(vec![("type", js("spaces"))]));
    let first = r.message("spaces");
    let keys: Vec<&str> = first.props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
    assert_eq!(keys, ["type", "on", "image", "supported", "checked", "installed", "ready", "version", "hint", "running", "step", "line", "fraction", "error", "busy"]);
    let supported = hover_agents::spaces::supported();
    assert_eq!((flag(&first, "on"), text(&first, "image"), flag(&first, "supported"), flag(&first, "checked")), (false, "macos", supported, false), "{}", first.compact());
    if !supported {
        assert_eq!(text(&first, "hint"), hover_agents::spaces::UNSUPPORTED, "the note is there before any check");
        let checked = r.until("the check", |m| is(m, "spaces") && flag(m, "checked"));
        assert!(!flag(&checked, "installed") && !flag(&checked, "ready") && int(&checked, "running") == 0, "{}", checked.compact());
        assert_eq!(text(&checked, "hint"), "Agent desktops need macOS 26 or later on Apple silicon.");
        assert_eq!(get(&checked, "version"), &Json::Null);
        assert!(!flag(&checked, "busy") && get(&checked, "step") == &Json::Null && get(&checked, "fraction") == &Json::Null && get(&checked, "error") == &Json::Null);

        // Its one-click setup says why it can't run, and is not left busy.
        r.send(jo(vec![("type", js("spaces")), ("step", js("setup"))]));
        let failed = r.until("the setup's error", |m| is(m, "spaces") && get(m, "error") != &Json::Null);
        assert_eq!(text(&failed, "error"), hover_agents::spaces::UNSUPPORTED);
        assert!(!flag(&failed, "busy"));
        r.send(jo(vec![("type", js("spaces")), ("step", js("cancel"))]));
    }

    // Switched on and set to Linux: remembered and persisted, yet the office has no desktops.
    r.send(jo(vec![("type", js("saveSettings")), ("agentSpaces", Json::Bool(true)), ("spaceImage", js("linux"))]));
    let prefs = r.message("preferences");
    assert!(flag(&prefs, "agentSpaces") && text(&prefs, "spaceImage") == "linux" && flag(&prefs, "spacesSupported") == supported, "{}", prefs.compact());
    r.send(jo(vec![("type", js("saveSettings")), ("spaceImage", js("floppy"))]));
    assert_eq!(text(&r.message("preferences"), "spaceImage"), "macos", "only macos or linux");
    r.send(jo(vec![("type", js("saveSettings")), ("spaceImage", js("linux"))]));
    r.message("preferences");

    r.ready_tool("codex");
    new_task(&mut r, "codex", &folder, "Look [seconds:0.1]", "full");
    let done = r.state("the task done", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "done")));
    assert_eq!(flag(&done, "spaces"), supported, "{}", done.compact());
    let s = first_session(&done);
    if !supported { assert_eq!(get(s, "space"), &Json::Null); }
    let id = int(s, "id");

    if !supported {
        // Cua isn't here: its viewer, a dropped app and dropped files are each answered.
        r.send(jo(vec![("type", js("spaceView")), ("id", Json::int(id))]));
        let view = r.message("space");
        assert_eq!(int(&view, "id"), id);
        assert_eq!(text(get(&view, "data"), "error"), "Cua’s desktop tools aren’t installed.");

        let app = if cfg!(windows) { "C:\\Apps\\Tiny.app" } else { "/Applications/Tiny.app" };
        r.send(jo(vec![("type", js("teleport")), ("id", Json::int(id)), ("path", js(app))]));
        let sending = r.message("teleport");
        let keys: Vec<&str> = sending.props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["type", "id", "phase", "app", "line"]);
        assert_eq!((int(&sending, "id"), text(&sending, "phase"), text(&sending, "app"), text(&sending, "line")), (id, "sending", "Tiny", "Sending Tiny…"));
        let end = r.until("the teleport's end", |m| is(m, "teleport") && text(m, "phase") == "done");
        let keys: Vec<&str> = end.props().unwrap().iter().map(|(k, _)| k.as_str()).collect();
        assert_eq!(keys, ["type", "id", "app", "phase", "data"]);
        assert_eq!((text(&end, "app"), text(get(&end, "data"), "error")), ("Tiny", "Cua’s desktop tools aren’t installed."));

        r.send(jo(vec![("type", js("spaceFiles")), ("id", Json::int(id)), ("paths", Json::Arr(vec![js(&format!("{folder}/app.js")), js("")]))]));
        let sent = r.until("the files' end", |m| is(m, "teleport") && text(m, "app") == "files");
        assert_eq!((int(&sent, "id"), text(&sent, "phase")), (id, "done"));
        assert_eq!(text(get(&sent, "data"), "error"), "Cua’s desktop tools aren’t installed.");
    }

    // Without a session, a drop names the project folder; a folder that isn't there, or no path, is ignored.
    r.send(jo(vec![("type", js("spaceFiles")), ("folder", js(&sb.root.join("nowhere").to_string_lossy())), ("paths", Json::Arr(vec![js("x")]))]));
    r.send(jo(vec![("type", js("teleport")), ("folder", js(&folder))]));
    r.send(jo(vec![("type", js("spaceView")), ("id", Json::int(9999))]));
    if !supported {
        r.send(jo(vec![("type", js("spaceFiles")), ("folder", js(&folder)), ("paths", Json::Arr(vec![js("x")]))]));
        let files = r.until("the files' end", |m| is(m, "teleport") && text(m, "app") == "files");
        assert_eq!(int(&files, "id"), 0, "no agent at work yet: the project's folder is the target");
    }

    // Deleting the only session leaves no desktop to delete, and nothing to break.
    let key = text(first_session(&done), "key").to_owned();
    r.send(jo(vec![("type", js("delete")), ("key", js(&key))]));
    r.state("the session gone", |m| list(m, "sessions").is_empty());
    r.send(ty("getSettings"));
    r.message("preferences");
    assert!(r.hang_up().success());

    // Persisted: a new run has the switch on, and the Linux image.
    let mut again = sb.start();
    again.initialize(&key_of(0));
    again.send(ty("getSettings"));
    let p = again.message("preferences");
    assert!(flag(&p, "agentSpaces") && text(&p, "spaceImage") == "linux", "{}", p.compact());
    assert!(again.hang_up().success());
}

/// The rest of backend-smoke.py: an approval, the stream, the desk panels, a reply, the
/// encrypted history across a restart, a wrong key, and deletion.
#[test]
fn a_task_runs_asks_answers_replies_and_is_kept_encrypted() {
    let sb = Sandbox::new("task");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);
    let ready = r.ready_tool("codex");
    let tools = list(&ready, "tools");
    assert_eq!(tools.iter().map(|t| text(t, "id")).collect::<Vec<_>>(), ["kiro", "codex", "cursor", "opencode", "claude"]);
    let codex = tools.iter().find(|t| text(t, "id") == "codex").unwrap();
    assert_eq!((text(codex, "name"), flag(codex, "checkedYet"), flag(codex, "installed"), flag(codex, "signedIn")), ("Codex", true, true, true));
    assert_eq!(text(codex, "effortLabel"), "Effort");
    assert!(flag(&ready, "canStart"));
    assert_eq!(int(&ready, "maxRunning"), 3);

    new_task(&mut r, "codex", &folder, "Sandbox secret [ask:edit:fixture.txt]", "always");
    let waiting = r.state("the approval", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "waiting")));
    let s = first_session(&waiting);
    assert_eq!((text(s, "tool"), text(s, "access")), ("codex", "always"));
    assert_eq!(text(s, "folder"), folder);
    assert!(text(s, "files").starts_with("hover://files/"));
    let ask = get(s, "ask");
    assert_eq!(text(ask, "kind"), "edit");
    assert_eq!(text(ask, "allow"), "Allow edit");
    assert!(text(ask, "title").contains("fixture.txt"), "{}", ask.compact());
    let id = int(s, "id");
    let key = text(s, "key").to_owned();
    r.send(jo(vec![("type", js("answer")), ("id", Json::int(id)), ("ask", js(text(ask, "id"))), ("answer", js("allow"))]));
    let done = r.state("the task done", |m| first_session(m).get("stage").and_then(Json::as_str) == Some("done"));
    let s = first_session(&done);
    let turn = &list(s, "turns")[0];
    assert!(text(turn, "answer").contains("allowed edit fixture.txt"), "{}", turn.compact());
    assert_eq!(text(turn, "stage"), "done");
    assert!(int(turn, "t0") > 0 && get(turn, "took") != &Json::Null);
    // The Rust-only, additive fields are there (no git in this folder's checkpoints is fine: false).
    assert!(matches!((get(turn, "restore"), get(turn, "again")), (Json::Bool(_), Json::Bool(_))));
    let kinds: Vec<&str> = list(turn, "steps").iter().map(|x| text(x, "k")).collect();
    assert!(kinds.contains(&"read") && kinds.contains(&"search"), "{kinds:?}");
    assert!(sb.project().join("fixture.txt").is_file(), "the allowed edit was made");
    assert_eq!(text(&done, "folder"), folder, "the folder the task started in is remembered");

    // The desk's panels (DeskInfo.Answer).
    let has_git = hover_agents::desk::find_git().is_some();
    let desk = |r: &mut Run, what: &str, arg: Option<&str>| -> Json {
        let mut m = vec![("type", js("desk")), ("id", Json::int(id)), ("what", js(what))];
        if let Some(a) = arg { m.push(("arg", js(a))); }
        r.send(jo(m));
        let w = what.to_owned();
        let d = r.until("the panel", move |m| is(m, "desk") && text(m, "what") == w);
        assert_eq!(int(&d, "id"), id);
        get(&d, "data").clone()
    };
    let probe = desk(&mut r, "probe", None);
    assert!(flag(&probe, "folder") && flag(&probe, "gh"), "{}", probe.compact());
    assert_eq!(int(&probe, "commands"), 0);
    assert_eq!(int(&probe, "agents"), 0);
    let terminal = desk(&mut r, "terminal", None);
    assert_eq!(list(&terminal, "commands").len(), 0);
    let agents = desk(&mut r, "agents", None);
    assert_eq!((list(&agents, "agents").len(), int(&agents, "running")), (0, 0));
    assert_eq!(list(&desk(&mut r, "browser", None), "pages").len(), 0);
    assert!(desk(&mut r, "nonsense", None).compact().contains("Unknown panel."));
    if has_git {
        assert!(flag(&probe, "git") && int(&probe, "changed") >= 1, "{}", probe.compact());
        let files = desk(&mut r, "files", None);
        assert!(files.compact().contains("app.js") && files.compact().contains("fixture.txt"), "{}", files.compact());
        let touched = list(&files, "touched");
        assert!(touched.len() == 1 && text(&touched[0], "path") == "src/file0.rs" && int(&touched[0], "read") == 1, "{}", files.compact());
        let diff = desk(&mut r, "diff", None);
        assert!(list(&diff, "files").iter().any(|f| text(f, "path") == "app.js" && text(f, "patch").contains("+b")), "{}", diff.compact());
        assert!(flag(&diff, "git"));
        let file = desk(&mut r, "file", Some("app.js"));
        assert_eq!(text(&file, "text"), "a\nb\n");
        let outside = desk(&mut r, "file", Some("../secret.txt"));
        assert!(outside.compact().contains("isn’t in the session’s folder") || outside.compact().contains("isn\\u2019t"), "{}", outside.compact());
    }

    // A reply carries on the conversation.
    r.send(jo(vec![("type", js("reply")), ("id", Json::int(id)), ("text", js("Continue [ask:edit:second.txt]"))]));
    let waiting = r.state("the second approval", |m| first_session(m).get("stage").and_then(Json::as_str) == Some("waiting"));
    let ask2 = get(first_session(&waiting), "ask").clone();
    r.send(jo(vec![("type", js("answer")), ("id", Json::int(id)), ("ask", js(text(&ask2, "id"))), ("answer", js("allow"))]));
    r.state("two turns done", |m| { let s = first_session(m); list(s, "turns").len() == 2 && text(s, "stage") == "done" });
    let ended = r.seen.iter().filter(|m| is(m, "ended")).count();
    assert_eq!(ended, 2, "an `ended` for each turn");
    let e = r.seen.iter().find(|m| is(m, "ended")).unwrap();
    assert_eq!((text(e, "tool"), text(e, "task"), text(e, "text"), flag(e, "ok")), ("codex", "Sandbox secret [ask:edit:fixture.txt]", "Completed", true));
    assert!(text(e, "title").starts_with("Codex: "));

    r.send(ty("shutdown"));
    assert!(r.exit().success());
    r.no_tools_left();
    let raw = history_bytes(&sb.data());
    assert!(!raw.is_empty() && !raw.windows(14).any(|w| w == b"Sandbox secret"), "the history is sealed");

    // Another key must not open, or replace, the history.
    let mut bad = sb.start();
    bad.send(jo(vec![("type", js("initialize")), ("key", js(&hover_agents::http::base64(&key_of(255))))]));
    let failure = bad.message("backendFailure");
    assert!(text(&failure, "text").contains("cannot decrypt"), "{}", failure.compact());
    assert!(bad.hang_up().success());
    assert_eq!(raw, history_bytes(&sb.data()), "the history is as it was");

    // A restart finds the preferences and the history again.
    let mut r = sb.start();
    r.initialize(&key_of(0));
    r.send(jo(vec![("type", js("ready"))]));
    let st = r.state("the history", |m| list(m, "history").len() == 1);
    let h = &list(&st, "history")[0];
    assert_eq!((text(h, "key"), text(h, "tool"), text(h, "stage"), int(h, "turns")), (key.as_str(), "codex", "done", 2));
    r.send(jo(vec![("type", js("history")), ("key", js(&key))]));
    let transcript = r.message("transcript");
    assert_eq!(list(get(&transcript, "session"), "turns").len(), 2);
    assert_eq!(text(get(&transcript, "session"), "key"), key);
    // A reply to the old session wakes it at a desk, and carries the conversation on.
    r.send(jo(vec![("type", js("reply")), ("key", js(&key)), ("text", js("Once more [seconds:0.2]"))]));
    let woke = r.state("the old session on a desk, replied to", |m| list(m, "sessions").first().is_some_and(|s| text(s, "key") == key && list(s, "turns").len() == 3 && text(s, "stage") == "done"));
    assert_eq!(text(first_session(&woke), "stage"), "done");
    r.send(jo(vec![("type", js("delete")), ("key", js(&key))]));
    r.state("the history empty", |m| list(m, "history").is_empty() && list(m, "sessions").is_empty());
    assert!(r.hang_up().success());
}

/// A picture pasted into a prompt is saved as a file the agent is told of, and the page
/// is given its address.
#[test]
fn a_pasted_picture_is_kept_for_the_agent() {
    const PNG: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNkYPhfDwAChwGA60e6kgAAAABJRU5ErkJggg==";
    let sb = Sandbox::new("image");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);
    r.ready_tool("codex");
    r.send(jo(vec![("type", js("new")), ("tool", js("codex")), ("folder", js(&folder)), ("prompt", js("Look [seconds:0.2]")),
        ("images", Json::Arr(vec![js(PNG), jo(vec![("data", js(PNG))]), js("data:text/plain;base64,AAAA"), js("not an image")]))]));
    let done = r.state("the task done", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "done")));
    let images = list(&list(first_session(&done), "turns")[0], "images");
    assert_eq!(images.len(), 2, "the two pictures, not the text or the junk: {}", done.compact());
    assert!(images.iter().all(|i| i.as_str().is_some_and(|u| u.starts_with("hover://images/") && u.ends_with(".png"))));
    let kept = std::fs::read_dir(sb.data().join("kiro-images")).unwrap().flatten().count();
    assert_eq!(kept, 2);
    assert!(r.hang_up().success());
}

/// Stop ends a turn that waits on an approval; the host hanging up (EOF) ends the
/// backend and every tool it started.
#[test]
fn stop_and_a_hang_up_leave_nothing_running() {
    let sb = Sandbox::new("stop");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);
    r.ready_tool("codex");
    new_task(&mut r, "codex", &folder, "Cancel me [ask:edit:a.txt]", "always");
    let waiting = r.state("the approval", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "waiting")));
    assert!(sb.running() >= 1, "the tool is running");
    r.send(jo(vec![("type", js("stop")), ("id", Json::int(int(first_session(&waiting), "id")))]));
    r.state("stopped", |m| first_session(m).get("stage").and_then(Json::as_str) == Some("stopped"));
    assert!(!sb.project().join("a.txt").exists(), "a stopped approval is turned down");
    // A second task is left waiting when the host goes away.
    new_task(&mut r, "codex", &folder, "UI crash [ask:edit:b.txt]", "always");
    r.state("the second approval", |m| list(m, "sessions").iter().any(|s| text(s, "stage") == "waiting"));
    assert!(r.hang_up().success());
    r.no_tools_left();
    // The session that was stopped is in the history, stopped.
    let mut again = sb.start();
    again.initialize(&key_of(0));
    again.send(ty("ready"));
    let st = again.state("the history", |m| !list(m, "history").is_empty());
    assert!(list(&st, "history").iter().any(|h| text(h, "stage") == "stopped"), "{}", st.compact());
    assert!(again.hang_up().success());
}

/// The host's lines before `initialize`, and bad ones, as Program.cs treats them.
#[test]
fn commands_before_initialize_and_bad_lines() {
    let sb = Sandbox::new("init");
    let mut r = sb.start();
    r.send(ty("ready"));
    let f = r.message("backendFailure");
    assert_eq!(text(&f, "text"), "Initialize the backend first.");
    r.send_raw("this is not json");
    assert_eq!(text(&r.message("toast"), "text"), "Invalid host message.");
    // A key of the wrong size is refused, and the backend is still not started.
    r.send(jo(vec![("type", js("initialize")), ("key", js(&hover_agents::http::base64(&[1, 2, 3])))]));
    assert!(text(&r.message("backendFailure"), "text").contains("Invalid or repeated history key"));
    r.send(jo(vec![("type", js("initialize")), ("key", js("not base64!"))]));
    assert!(text(&r.message("backendFailure"), "text").contains("base64"));
    r.initialize(&key_of(7));
    // A second initialize is nothing to Handle (the key is used once): no new backend, no message.
    r.send(jo(vec![("type", js("initialize")), ("key", js(&hover_agents::http::base64(&key_of(9))))]));
    r.send(ty("getSettings"));
    r.message("preferences");
    assert!(r.hang_up().success());
}

/// What `new` refuses, and says.
#[test]
fn a_new_task_says_why_it_cant_start() {
    let sb = Sandbox::new("refuse");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    let says = |r: &mut Run, m: Json, want: &str| {
        r.send(m);
        let t = r.message("toast");
        assert!(text(&t, "text").contains(want), "{} should say {want}", t.compact());
    };
    says(&mut r, jo(vec![("type", js("new")), ("tool", js("codex")), ("folder", js("relative/dir")), ("prompt", js("x"))]), "Choose an existing project folder.");
    says(&mut r, jo(vec![("type", js("new")), ("tool", js("codex")), ("folder", js(&folder)), ("prompt", js("x"))]), "Review agent access in Settings");
    accept_notice(&mut r);
    // Before the check has finished a tool is not known to be ready; after, its hint says what to do.
    says(&mut r, jo(vec![("type", js("new")), ("tool", js("codex")), ("folder", js(&folder)), ("prompt", js("x"))]), "The tool is not ready yet.");
    r.ready_tool("codex");
    says(&mut r, jo(vec![("type", js("new")), ("tool", js("cursor")), ("folder", js(&folder)), ("prompt", js("x"))]), "Install the Cursor CLI");
    says(&mut r, jo(vec![("type", js("new")), ("tool", js("codex")), ("folder", js(&folder)), ("prompt", js("  "))]), "All available desks are busy, or the prompt is empty.");
    says(&mut r, jo(vec![("type", js("reply")), ("id", Json::int(9999)), ("text", js("hi"))]), "Could not send this reply.");
    assert!(r.hang_up().success());
}

/// The GitHub CLI, through a scripted gh: what is known, a fresh check, and the desk's
/// pull request panel.
#[test]
fn github_status_through_a_scripted_gh() {
    let sb = Sandbox::new("gh");
    sb.gh_script("version", &["out=gh version 2.102.0 (2026-09-30)"]);
    sb.gh_script("auth_status", &["out=github.com", "out=  ✓ Logged in to github.com account octocat (keyring)"]);
    let mut r = sb.start();
    r.initialize(&key_of(0));
    r.send(ty("gh"));
    let unknown = r.message("gh");
    assert!(!flag(&unknown, "checked") && !flag(&unknown, "installed") && !flag(&unknown, "busy"), "{}", unknown.compact());
    assert_eq!(text(&unknown, "url"), "https://github.com/login/device");
    let known = r.until("the checked status", |m| is(m, "gh") && flag(m, "checked"));
    assert!(flag(&known, "installed") && flag(&known, "signedIn"), "{}", known.compact());
    assert_eq!((text(&known, "user"), text(&known, "version")), ("octocat", "2.102.0"));
    assert_eq!(get(&known, "code"), &Json::Null);
    assert_eq!(text(&known, "line"), "");
    assert!(r.hang_up().success());
}

/// Create pull request and the pull request panel against a scripted gh and a bare remote.
#[test]
fn the_pull_request_panel_and_create() {
    if hover_agents::desk::find_git().is_none() { return; }
    let sb = Sandbox::new("pr");
    let folder = sb.folder();
    sb.gh_script("version", &["out=gh version 2.102.0 (2026-09-30)"]);
    sb.gh_script("auth_status", &["out=  ✓ Logged in to github.com account octocat (keyring)"]);
    sb.gh_script("pr_view", &["err=no pull requests found for branch \"main\"", "exit=1"]);
    sb.gh_script("pr_create", &["out=https://github.com/octo/demo/pull/7"]);
    let remote = sb.root.join("remote.git");
    let g = |args: &[&str], dir: &Path| { let mut c = Command::new(hover_agents::desk::find_git().unwrap()); c.args(args).current_dir(dir); for (k, v) in sb.git_env() { c.env(k, v); } assert!(c.output().unwrap().status.success(), "git {args:?}"); };
    std::fs::create_dir_all(&remote).unwrap();
    g(&["init", "-q", "--bare"], &remote);
    sb.git(&["branch", "-M", "main"]);
    sb.git(&["remote", "add", "origin", &remote.to_string_lossy()]);
    sb.git(&["push", "-q", "-u", "origin", "main"]);

    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);
    r.ready_tool("codex");
    new_task(&mut r, "codex", &folder, "Change it", "full");
    let done = r.state("the task done", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "done")));
    let id = int(first_session(&done), "id");
    r.send(jo(vec![("type", js("desk")), ("id", Json::int(id)), ("what", js("pr"))]));
    let pr = r.until("the pr panel", |m| is(m, "desk") && text(m, "what") == "pr");
    let data = get(&pr, "data");
    assert!(flag(data, "none"), "{}", pr.compact());
    assert_eq!(text(get(data, "create"), "base"), "main");
    assert!(flag(get(data, "create"), "onDefault"));
    // Create: a new branch, the changes committed, pushed, and gh opens the request.
    let args = jo(vec![("title", js("Add b")), ("body", js("It adds b.")), ("branch", js("hover/add-b")), ("commit", Json::Bool(true)), ("draft", Json::Bool(false))]);
    r.send(jo(vec![("type", js("deskAction")), ("id", Json::int(id)), ("what", js("prCreate")), ("args", args)]));
    let made = r.message("deskAction");
    let data = get(&made, "data");
    assert!(flag(data, "ok"), "{}", made.compact());
    assert_eq!(text(data, "url"), "https://github.com/octo/demo/pull/7");
    assert_eq!(list(data, "steps").len(), 3, "{}", made.compact());
    // No early answer for an unknown action.
    r.send(jo(vec![("type", js("deskAction")), ("id", Json::int(id)), ("what", js("nothing"))]));
    r.send(ty("shutdown"));
    assert!(r.exit().success());
}

/// OpenCode's own server (fake-opencode): a task, and its tool listed by the check.
#[test]
fn an_opencode_task_runs() {
    let sb = Sandbox::new("opencode");
    let folder = sb.folder();
    let mut r = sb.start();
    r.initialize(&key_of(0));
    accept_notice(&mut r);
    let ready = r.ready_tool("opencode");
    let oc = list(&ready, "tools").iter().find(|t| text(t, "id") == "opencode").unwrap().clone();
    assert_eq!((text(&oc, "effortLabel"), flag(&oc, "questions")), ("Variant", true));
    new_task(&mut r, "opencode", &folder, "Say hello", "full");
    let done = r.state("the task done", |m| m.get("sessions").and_then(|s| s.items().ok()).is_some_and(|s| s.first().is_some_and(|s| text(s, "stage") == "done")));
    assert_eq!(text(first_session(&done), "tool"), "opencode");
    assert!(!text(&list(first_session(&done), "turns")[0], "answer").is_empty());
    // The models OpenCode listed in its run come to the tool's entry.
    let oc = list(&done, "tools").iter().find(|t| text(t, "id") == "opencode").unwrap().clone();
    assert!(list(&oc, "models").len() > 1, "{}", oc.compact());
    assert_eq!(text(&list(&oc, "models")[0], "id"), "", "Default comes first");
    r.send(ty("shutdown"));
    assert!(r.exit().success());
    r.no_tools_left();
}

/// The quotas switched on are read and sent as one `quotas` message (Kiro's by its stand-in).
#[test]
fn quotas_are_read_for_the_items_switched_on() {
    let sb = Sandbox::new("quotas");
    let mut r = sb.start();
    r.initialize(&key_of(0));
    r.send(jo(vec![("type", js("saveSettings")), ("quotaItems", Json::Arr(vec![js("kiro"), js("codex")]))]));
    let q = r.message("quotas");
    let v = get(&q, "values");
    assert!(flag(get(v, "kiro"), "ok"), "{}", q.compact());
    assert!(matches!(get(get(v, "kiro"), "used"), Json::Num(_)));
    assert!(!flag(get(v, "codex"), "ok"), "no Codex sessions in this home: {}", q.compact());
    assert!(v.get("claude").is_none(), "only what is switched on");
    assert!(r.hang_up().success());
}

/// `setModel` and `setup`, which a host sends from its settings.
#[test]
fn a_model_pick_is_kept_and_setup_is_for_a_mac() {
    let sb = Sandbox::new("model");
    let mut r = sb.start();
    r.initialize(&key_of(0));
    r.send(jo(vec![("type", js("setModel")), ("tool", js("kiro")), ("model", js("claude-sonnet-5")), ("effort", js("low"))]));
    let st = r.message("state");
    let kiro = list(&st, "tools").iter().find(|t| text(t, "id") == "kiro").unwrap().clone();
    assert_eq!((text(&kiro, "model"), text(&kiro, "effort")), ("claude-sonnet-5", "low"));
    assert!(list(&kiro, "models").len() > 5);
    assert_eq!(text(&list(&kiro, "models")[0], "id"), "auto", "Kiro's own list leads with Auto, so no Default is added");
    assert_eq!(flag(&kiro, "canSetup"), cfg!(target_os = "macos"));
    r.send(jo(vec![("type", js("setModel")), ("tool", js("kiro")), ("model", js("")), ("effort", Json::Null)]));
    let st = r.message("state");
    let kiro = list(&st, "tools").iter().find(|t| text(t, "id") == "kiro").unwrap().clone();
    assert_eq!(text(&kiro, "model"), "");
    assert!(r.hang_up().success());
}

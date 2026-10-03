//! The Spaces tests of tests/Hover.Tests/BrowserAndGitHubTests.cs (Arz's 8d55562), ported,
//! and the rest of what spaces.rs can say without a Mac: names, what Cua and Lume print,
//! the install script, the progress lines, the viewer's address, and that off a Mac agent
//! desktops are off with their note. Nothing of Cua is run.

use hover_agents::cancel::Cancel;
use hover_agents::spaces::{self, Frame, SpaceInfo, VmInfo};
use std::sync::Mutex;

/// The settings and PATH are the process's: one test at a time.
static LOCK: Mutex<()> = Mutex::new(());

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

fn dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("hover-spaces-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn each_project_has_its_own_space_and_the_list_is_read_loosely() {
    // One desktop per project: the same folder (however it is written) gives one Space,
    // two folders with the same name give two.
    let base = dir("names");
    let app = base.join("My App");
    let other = base.join("x").join("My App");
    let name = |p: &std::path::Path| spaces::name_for(&p.to_string_lossy());
    let n = name(&app);
    assert!(n.starts_with("hover-my-app-") && n.len() == "hover-my-app-".len() + 6 && n["hover-my-app-".len()..].chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()), "{n}");
    assert_eq!(spaces::name_for(&format!("{}{}", app.display(), std::path::MAIN_SEPARATOR)), n, "a trailing separator");
    assert_eq!(name(&base.join("x").join("..").join("My App")), n, "a .. in the path");
    assert_ne!(name(&other), n);
    assert_eq!(spaces::id_for(&app.to_string_lossy()), format!("local:{n}"));
    assert_eq!(spaces::title(&app.to_string_lossy()), "My App");
    assert_eq!(spaces::title(&format!("{}{}", app.display(), std::path::MAIN_SEPARATOR)), "My App");
    assert!(spaces::same_project(&app.to_string_lossy(), &format!("{}{}", app.display(), std::path::MAIN_SEPARATOR)) && !spaces::same_project(&app.to_string_lossy(), &other.to_string_lossy()));

    let list = spaces::parse_list("note: signed out\n[{\"id\":\"local:hover-1\",\"name\":\"hover-1\",\"os\":\"macos\",\"power_state\":\"running\"},{\"id\":\"local:x\",\"power_state\":\"stopped\"}]");
    assert_eq!(list.iter().map(|x| (x.id.as_str(), x.name.as_str(), x.running)).collect::<Vec<_>>(), [("local:hover-1", "hover-1", true), ("local:x", "x", false)]);
    assert_eq!(list[0].os.as_deref(), Some("macos"));
    assert_eq!(spaces::parse_list("{\"spaces\":[{\"id\":\"local:a\",\"name\":\"a\"}]}").len(), 1);
    assert!(spaces::parse_list("not json").is_empty());
    // What `cua spaces ls` prints: telemetry notice, then the list, with no power state. One
    // with none isn't taken for on (a stopped one taken for on was never started).
    let real = spaces::parse_list("Cua collects anonymous usage data…\n{\"relay_error\":null,\"spaces\":[{\"id\":\"local:hover-hover-9a332d\",\"name\":\"Apple-Virtual-Machine-1.local\",\"os\":\"macos\",\"kind\":\"vm\"}]}");
    assert_eq!(real, [SpaceInfo { id: "local:hover-hover-9a332d".into(), name: "Apple-Virtual-Machine-1.local".into(), running: false, os: Some("macos".into()) }]);
    // `cua sb ls --local`: this Mac's own, with Lume's status; Lume's runtime is macOS.
    let local = spaces::parse_list("[{\"id\":\"local:hover-a\",\"runtime\":\"lume\",\"kind\":\"vm\",\"status\":\"running\"},{\"id\":\"local:hover-b\",\"runtime_type\":\"docker\",\"status\":\"stopped\"}]");
    assert_eq!(local.iter().map(|x| (x.name.as_str(), x.running, x.os.as_deref())).collect::<Vec<_>>(), [("hover-a", true, Some("macos")), ("hover-b", false, Some("docker"))]);
    // Lume knows whether it is on, and its size.
    let vm = spaces::parse_vm("{\"name\":\"hover-hover-9a332d\",\"status\":\"stopped\",\"cpuCount\":2,\"memorySize\":4294967296,\"display\":\"1024x768\"}");
    assert_eq!(vm, Some(VmInfo { exists: true, running: false, cpus: 2, memory_gb: 4, display: Some("1024x768".into()) }));
    assert!(spaces::parse_vm("[{\"status\":\"running\",\"cpuCount\":4,\"memorySize\":8589934592}]").unwrap().running);
    assert_eq!(spaces::parse_vm("warning: x\n[]"), None);
    assert_eq!(spaces::parse_vm("no json at all"), None);
    let size = spaces::target();
    assert!((2..=6).contains(&size.cpus) && (4..=8).contains(&size.memory_gb), "{size:?}");
}

#[test]
fn each_agent_drives_the_desktop_in_a_session_of_its_own() {
    use hover_agents::space_driver::{arguments, exclusive, refusal, tools_for_agent, DENIED};
    use hover_core::json::{self, Json};
    let j = |s: &str| json::parse(s).unwrap();
    // Whatever session the agent names, it acts in its own.
    let a = arguments(Some(&j(r#"{"session":"someone-else","pid":7}"#)), "hover-abc-01ff");
    assert_eq!(a.get("session").and_then(Json::as_str), Some("hover-abc-01ff"));
    assert_eq!(a.get("pid").and_then(|p| p.i64().ok()), Some(7));
    assert_eq!(arguments(Some(&Json::str("junk")), "x").get("session").and_then(Json::as_str), Some("x"));
    assert_eq!(arguments(None, "x").get("session").and_then(Json::as_str), Some("x"));
    // Whole-desktop input and foreground delivery are one agent at a time.
    assert!(exclusive(&j(r#"{"delivery_mode":"foreground"}"#)) && exclusive(&j(r#"{"scope":"desktop"}"#)) && exclusive(&j(r#"{"target":{"kind":"desktop"}}"#)));
    assert!(!exclusive(&j(r#"{"delivery_mode":"background","pid":3}"#)) && !exclusive(&Json::Null));
    assert_eq!(refusal(&j(r#"{"result":{"structuredContent":{"refusal":{"code":"session_expired"}}}}"#)), Some("session_expired"));
    assert_eq!(refusal(&j(r#"{"result":{"content":[]}}"#)), None);
    // The agent never sees the tools that would reach another's work, nor the session argument.
    let tools = tools_for_agent(&j(r#"[
        {"name":"click","inputSchema":{"type":"object","properties":{"session":{"type":"string"},"pid":{"type":"integer"}},"required":["session","pid"]}},
        {"name":"kill_app","inputSchema":{"type":"object"}},{"name":"end_session"},{"name":"bring_to_front"}]"#));
    let Json::Arr(t) = &tools else { panic!("{tools:?}") };
    assert_eq!(t.len(), 1, "{}", tools.compact());
    assert_eq!(t[0].compact(), r#"{"name":"click","inputSchema":{"type":"object","properties":{"pid":{"type":"integer"}},"required":["pid"]}}"#);
    for d in ["kill_app", "end_session", "bring_to_front", "set_config"] { assert!(DENIED.contains(&d), "{d}"); }
    assert_eq!(tools_for_agent(&Json::Null).compact(), "[]");
}

#[test]
fn the_desktops_driver_is_only_ever_a_local_one() {
    let said = "notice: x\n{\"type\":\"http\",\"url\":\"http://192.168.64.7:3211/mcp\",\"headers\":{\"x-cua-env-authorization\":\"Bearer t0k\",\"x-hidden\":\"****\"}}";
    let e = spaces::parse_driver(said).unwrap();
    assert_eq!((e.url.as_str(), e.path.as_str()), ("http://192.168.64.7:3211/mcp", "/mcp"));
    assert_eq!(e.headers, [("x-cua-env-authorization".to_owned(), "Bearer t0k".to_owned())], "a masked secret isn't sent");
    assert_eq!(spaces::parse_driver("{\"url\":\"http://127.0.0.1:9\"}").unwrap().path, "/");
    assert!(spaces::parse_driver("{\"url\":\"http://example.com/mcp\"}").is_none(), "never a token to anywhere else");
    assert!(spaces::parse_driver("{\"url\":\"https://127.0.0.1/mcp\"}").is_none());
    assert!(spaces::parse_driver("no json").is_none());
}

#[test]
fn a_teleport_is_planned_from_what_cua_says() {
    let p = spaces::parse_providers(r#"[{"id":"chrome","display_name":"Google Chrome","app_ids":["com.google.Chrome",""],"macos":true},{"id":"x","app_ids":["a"],"macos":false},{"id":"noapps"}]"#);
    assert_eq!(p, [spaces::Provider { id: "chrome".into(), name: "Google Chrome".into(), apps: vec!["com.google.Chrome".into()] }]);
    assert!(spaces::parse_providers("oops").is_empty());
    let (provider, items, notes) = spaces::parse_manifest(r#"{"provider_id":"chrome","notes":["Full profile",""],"items":[
        {"label":"Open tabs","rel_path":"tabs.json","count":3,"count_noun":"tabs","default_checked":true},
        {"label":"Cookies","rel_path":"Default/Cookies","sensitive":true,"default_checked":true},{"label":"no path"}]}"#);
    assert_eq!(provider.as_deref(), Some("chrome"));
    assert_eq!(notes, ["Full profile"]);
    assert_eq!(items.len(), 2);
    assert_eq!((items[0].count, items[0].noun.as_deref(), items[0].default, items[0].sensitive), (Some(3), Some("tabs"), true, false));
    assert!(items[1].sensitive && items[1].path == "Default/Cookies");
    // Only web addresses, once each, at most 50; each one quoted for the desktop's shell.
    let mut urls: Vec<String> = vec!["https://a.test/".into(), "file:///etc/passwd".into(), "https://a.test/".into(), "http://b.test/x?y=1&z='2'".into(), "https://c.test/\nrm".into()];
    assert_eq!(spaces::web(&urls), ["https://a.test/", "http://b.test/x?y=1&z='2'"]);
    urls.extend((0..80).map(|i| format!("https://many.test/{i}")));
    assert_eq!(spaces::web(&urls).len(), 50);
    assert_eq!(spaces::safari_script(&["http://b.test/?z='2'".into()]), r"/usr/bin/open -b com.apple.Safari 'http://b.test/?z='\''2'\'''");
    // Safari with more than one tab asks which: each tab to pick, all picked to start with.
    let tab = |id: &str, url: &str| spaces::Tab { id: id.into(), title: format!("Tab {id}"), url: url.into() };
    let plan = spaces::plan(Some(spaces::SAFARI), "Safari", &[tab("7:1", "https://a.test/"), tab("7:2", "https://b.test/")]);
    assert_eq!(plan.get("review").map(|r| r.compact()).as_deref(), Some("true"));
    assert_eq!(plan.get("provider").and_then(hover_core::json::Json::as_str), Some("safari"));
    assert_eq!(plan.get("tabs").map(|t| t.compact()).as_deref(),
        Some(r#"[{"id":"7:1","title":"Tab 7:1","url":"https://a.test/","on":true},{"id":"7:2","title":"Tab 7:2","url":"https://b.test/","on":true}]"#));
    // One tab (or none) needs no asking: it just goes.
    assert_eq!(spaces::plan(Some(spaces::SAFARI), "Safari", &[tab("7:1", "https://a.test/")]).compact(), r#"{"review":false}"#);
    assert_eq!(spaces::plan(Some(spaces::SAFARI), "Safari", &[]).compact(), r#"{"review":false}"#, "no tabs, nothing to review");
    // A Chromium browser's tabs are picked the same way (here with no Cua teleport to ask:
    // with one, the plan would ask the real Cua).
    if spaces::teleport_exe().is_none() {
        let p = spaces::plan(Some("com.google.Chrome"), "Google Chrome", &[tab("1:10", "https://a.test/"), tab("1:11", "https://b.test/")]);
        assert_eq!(p.get("provider").and_then(hover_core::json::Json::as_str), Some("browser"));
        assert_eq!(p.get("tabs").map(|t| matches!(t, hover_core::json::Json::Arr(a) if a.len() == 2)), Some(true));
        assert_eq!(spaces::plan(Some("com.google.Chrome"), "Google Chrome", &[tab("1:10", "https://a.test/")]).compact(), r#"{"review":false}"#, "one tab just goes");
        assert_eq!(spaces::plan(Some("com.tinyspeck.slackmacgap"), "Slack", &[tab("1:1", "https://a.test/"), tab("1:2", "https://b.test/")]).compact(), r#"{"review":false}"#,
            "tabs mean nothing for an app that isn't a browser");
    }

    // Which apps are browsers whose tabs Hover moves.
    assert_eq!(spaces::browser(Some(spaces::SAFARI)), Some(spaces::Browser::Safari));
    assert_eq!(spaces::browser(Some("com.brave.Browser")), Some(spaces::Browser::Chromium));
    assert_eq!(spaces::browser(Some("org.mozilla.firefox")), None);
    assert_eq!(spaces::browser(None), None);
    // Cua's items that would bring every tab of the last session back are left to Hover.
    for s in ["tabs.json", ".config/google-chrome/Default/Sessions", ".config/google-chrome/Default/Session Storage/", "Default/Current Tabs"] { assert!(spaces::session_item(s), "{s}"); }
    for s in ["Default/Bookmarks", ".config/google-chrome/Default/Cookies", "Sessions.bak"] { assert!(!spaces::session_item(s), "{s}"); }
}

#[test]
fn a_browsers_tabs_are_read_loosely_and_opened_together() {
    use hover_core::json;
    let tabs = spaces::tabs_of(Some(&json::parse(r#"[{"id":"3:1","title":"  A  ","url":"https://a.test/"},{"id":"3:1","url":"https://dup.test/"},
        {"id":"3:2","url":"chrome://settings"},{"id":"3:3","url":"https://a.test/"},{"title":"no address"}]"#).unwrap()), &[]);
    assert_eq!(tabs.iter().map(|t| (t.id.as_str(), t.title.as_str(), t.url.as_str())).collect::<Vec<_>>(),
        [("3:1", "A", "https://a.test/"), ("3:3", "", "https://a.test/")], "web tabs only, each tab once; two tabs may share an address");
    // An older message's bare addresses: each address once.
    let old = spaces::tabs_of(None, &["https://a.test/".into(), "https://a.test/".into(), "file:///etc/passwd".into(), "http://b.test/".into()]);
    assert_eq!(old.iter().map(|t| t.url.as_str()).collect::<Vec<_>>(), ["https://a.test/", "http://b.test/"]);
    assert!(old.iter().all(|t| t.id.is_empty()));
    let many: Vec<String> = (0..80).map(|i| format!("https://m.test/{i}")).collect();
    assert_eq!(spaces::tabs_of(None, &many).len(), 50);

    // Chromium: one new window with every tab, past its first-run welcome; each word quoted.
    let l = spaces::Launch::chromium(&["https://a.test/?q='x'".into(), "https://b.test/".into()]);
    assert_eq!(l.command("'com.google.Chrome'", true),
        r"/usr/bin/open -n -b 'com.google.Chrome' --args '--no-first-run' '--no-default-browser-check' '--new-window' 'https://a.test/?q='\''x'\''' 'https://b.test/'");
    // Safari: the addresses as documents, which its first window opens as tabs.
    assert_eq!(spaces::Launch::safari(&["https://a.test/".into()]).command(spaces::SAFARI, true), "/usr/bin/open -b com.apple.Safari 'https://a.test/'");
    assert_eq!(spaces::Launch::default().command("\"$d/$a\"", false), "/usr/bin/open \"$d/$a\"");
    assert!(spaces::install_script_with("/z", "Google Chrome.app", &l).ends_with(r#"rm -f "$z"; /usr/bin/open -n "$d/$a" --args '--no-first-run' '--no-default-browser-check' '--new-window' 'https://a.test/?q='\''x'\''' 'https://b.test/'"#));
}

#[test]
fn a_desktop_in_use_is_marked_until_its_work_ends() {
    let f = "/tmp/hover-in-use-project";
    assert!(!spaces::in_use(f));
    let a = spaces::use_space(f);
    let b = spaces::use_space(&format!("{f}/"));
    assert!(spaces::in_use(f));
    drop(a);
    assert!(spaces::in_use(f), "still one at work");
    drop(b);
    assert!(!spaces::in_use(f));
}

#[test]
fn a_space_is_sized_from_the_mac() {
    let t = |gb, cores| { let s = spaces::target_for(gb, cores); (s.cpus, s.memory_gb) };
    assert_eq!(t(8, 8), (2, 4));
    assert_eq!(t(16, 10), (3, 6));
    assert_eq!(t(24, 12), (4, 8));
    assert_eq!(t(128, 64), (6, 8), "at most six cores");
    assert_eq!(t(0, 1), (2, 4), "at least two, when the Mac can't be read");
    assert_eq!(spaces::target_for(16, 8).display, "1024x768");
}

#[test]
fn an_app_is_unpacked_and_opened_with_every_name_quoted() {
    let script = spaces::install_script("/Users/lume/Downloads/.hover-x.zip", "It's; rm -rf ~.app");
    assert!(script.contains(r"a='It'\''s; rm -rf ~.app'"), "a quote in a name can't end the string: {script}");
    assert!(script.contains("z='/Users/lume/Downloads/.hover-x.zip'"), "{script}");
    assert!(script.contains("/usr/bin/ditto -x -k \"$z\" \"$d\""));
    assert!(script.ends_with("/usr/bin/open \"$d/$a\""));
    // Exactly as the C# built it.
    assert_eq!(spaces::install_script("/z", "A.app"),
        "set -e; z='/z'; a='A.app'; d=/Applications; [ -w \"$d\" ] || { d=\"$HOME/Applications\"; mkdir -p \"$d\"; }; rm -rf \"$d/$a\"; /usr/bin/ditto -x -k \"$z\" \"$d\"; rm -f \"$z\"; /usr/bin/open \"$d/$a\"");
}

#[test]
fn names_hash_as_sha_256_does() {
    assert_eq!(hex(&spaces::sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    assert_eq!(hex(&spaces::sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    // Two blocks' worth (the padding spills into a second one).
    assert_eq!(hex(&spaces::sha256(&[b'a'; 64])), "ffe054fe7ae0cb6dc65c3af9b61d5209f439851db43d0ba5997337df154668eb");
    // The two paths as System.Security.Cryptography hashed them in the C# (lowercased).
    assert_eq!(hex(&spaces::sha256(b"c:\\users\\test\\my app")), "c981cdd438ef886d55b0ec8c1176960b9b5219a270f50aac2fbfc61bb269cc77");
    assert_eq!(hex(&spaces::sha256(b"/users/me/projects/my app")), "cf21fc87ec64d4fbaa09dc1ad193431db56b006f184ed88eb80e34fd0a6877d4");
    // A Mac (and Windows) compares paths without case, as the C# did; Linux keeps it.
    let (a, b) = if cfg!(windows) { ("C:\\Users\\Test\\My App", "c:\\users\\test\\my app") } else { ("/Users/Me/Projects/My App", "/users/me/projects/my app") };
    if cfg!(target_os = "linux") { assert_ne!(spaces::name_for(a), spaces::name_for(b)); } else { assert_eq!(spaces::name_for(a), spaces::name_for(b)); }
    if cfg!(windows) { assert_eq!(spaces::name_for(a), "hover-my-app-c981cd"); }
    if cfg!(target_os = "macos") { assert_eq!(spaces::name_for(a), "hover-my-app-cf21fc"); }
}

#[test]
fn a_name_is_a_short_slug_and_a_hash() {
    let (root, sep) = if cfg!(windows) { ("C:\\", "\\") } else { ("/", "/") };
    let n = |tail: &str| spaces::name_for(&format!("{root}{}", tail.replace('/', sep)));
    assert!(n("w/ÄÖ Ünï").starts_with("hover-"), "{}", n("w/ÄÖ Ünï"));
    assert!(n("w/---").len() == "hover-".len() + 6 && !n("w/---")["hover-".len()..].contains('-'), "nothing to slug: hover-<hash> ({})", n("w/---"));
    let long = n("w/A very long project name indeed");
    let slug = long.strip_prefix("hover-").unwrap().rsplit_once('-').unwrap().0;
    assert_eq!(slug, "a-very-long-project", "cut at 20, without a trailing dash ({long})");
    assert!(n("w/x").starts_with("hover-x-"));
    assert_eq!(spaces::title(&format!("{root}w{sep}Project{sep}")), "Project");
    assert_eq!(spaces::title(root), root, "a root has no name: the path itself");
}

#[test]
fn only_a_mac_26_on_apple_silicon_runs_spaces() {
    assert!(spaces::supported_on("macos", 26, "aarch64"));
    assert!(spaces::supported_on("macos", 27, "arm64"));
    assert!(!spaces::supported_on("macos", 15, "aarch64"), "too old");
    assert!(!spaces::supported_on("macos", 26, "x86_64"), "Intel");
    assert!(!spaces::supported_on("linux", 26, "aarch64"), "upstream also ran on Linux; Cua is kept to the Mac here");
    assert!(!spaces::supported_on("windows", 26, "aarch64"));
    assert_eq!(spaces::product_major("26.0.1\n"), 26);
    assert_eq!(spaces::product_major("15.6"), 15);
    assert_eq!(spaces::product_major("nonsense"), 0);
    assert_eq!(spaces::UNSUPPORTED, "Agent desktops need macOS 26 or later on Apple silicon.");
    if !cfg!(target_os = "macos") {
        assert!(!spaces::supported() && spaces::note() == Some(spaces::UNSUPPORTED));
    }
}

#[test]
fn off_a_mac_the_switch_is_off_with_its_note() {
    if cfg!(target_os = "macos") { return; }
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    // Switched on in Settings, and still not wanted; no server, no setup, and the hint says why.
    spaces::set_source(|| spaces::Switches { on: true, linux: false });
    assert!(!spaces::wanted());
    assert!(spaces::servers(Some("/tmp/project"), "tag").is_empty());
    assert!(spaces::servers(None, spaces::OPENCODE).is_empty());
    assert_eq!(spaces::ensure("/tmp/project", &Cancel::new()).as_deref(), Some("Agent desktops are off."));
    let s = spaces::check(true);
    assert_eq!((s.installed, s.ready, s.running, s.hint.as_str()), (false, false, 0, spaces::UNSUPPORTED));
    assert_eq!(spaces::known(), Some(s));
    spaces::run_setup();
    assert_eq!(spaces::setup().error.as_deref(), Some(spaces::UNSUPPORTED));
    assert!(!spaces::busy());
    // Not on Linux either, whatever the image.
    spaces::set_source(|| spaces::Switches { on: true, linux: true });
    assert!(!spaces::wanted() && spaces::image() == "linux");
    spaces::set_source(spaces::Switches::default);
}

#[test]
fn cua_and_lume_are_found_on_path_first() {
    let _g = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let bin = dir("path");
    let old = std::env::var_os("PATH");
    std::env::set_var("PATH", &bin);
    assert!(spaces::exe().is_none_or(|p| !p.starts_with(&bin)) && spaces::lume_exe().is_none_or(|p| !p.starts_with(&bin)), "an empty folder has neither");
    let file = |n: &str| bin.join(if cfg!(windows) { format!("{n}.exe") } else { n.to_owned() });
    std::fs::write(file("cua"), "").unwrap();
    std::fs::write(file("lume"), "").unwrap();
    // PATHEXT may spell the suffix in capitals; the file is the same.
    let same = |found: Option<std::path::PathBuf>, want: std::path::PathBuf| found.map(|p| p.to_string_lossy().to_lowercase()) == Some(want.to_string_lossy().to_lowercase());
    assert!(same(spaces::exe(), file("cua")) && same(spaces::lume_exe(), file("lume")), "{:?} {:?}", spaces::exe(), spaces::lume_exe());
    match old { Some(p) => std::env::set_var("PATH", p), None => std::env::remove_var("PATH") }
}

#[test]
fn progress_lines_give_their_fraction() {
    let f = |raw: &str| spaces::frame_of(raw).map(|(_, f)| f);
    assert_eq!(f("   \u{1b}[2K  \r"), None);
    assert_eq!(f(r#"{"phase":"pulling","fraction":0.3}"#), Some(Frame { line: "Downloading the desktop image…".into(), fraction: Some(0.3) }));
    assert_eq!(f(r#"{"phase":"creating","fraction":0.7}"#).unwrap().line, "Making the desktop…");
    assert_eq!(f(r#"{"phase":"booting"}"#), Some(Frame { line: "Starting it up…".into(), fraction: None }));
    assert_eq!(f(r#"{"phase":"waiting_for_services","fraction":0.95}"#).unwrap().line, "Almost ready…");
    assert_eq!(f(r#"{"phase":"ready","fraction":1}"#), Some(Frame { line: "Ready.".into(), fraction: Some(1.0) }));
    assert_eq!(f(r#"{"phase":"unpacking"}"#).unwrap().line, "unpacking");
    assert_eq!(f("{broken").unwrap(), Frame { line: "{broken".into(), fraction: None });
    assert_eq!(f("Downloading 42% of 23 GB"), Some(Frame { line: "Downloading 42% of 23 GB".into(), fraction: Some(0.42) }));
    assert_eq!(f("\u{1b}[32m 7.5 %\u{1b}[0m").unwrap().fraction, Some(0.075));
    assert_eq!(f("copied 512 2048").unwrap().fraction, Some(0.25));
    assert_eq!(f("files 3 0").unwrap().fraction, None, "no total");
    assert_eq!(f("nothing to measure").unwrap().fraction, None);
    // What an error says is the line itself, not what is shown of it.
    let long = "x".repeat(300);
    let (tail, frame) = spaces::frame_of(&long).unwrap();
    assert_eq!((tail.len(), frame.line.chars().count()), (300, 140));
    assert!(frame.line.ends_with('…'));
    assert_eq!(spaces::frame_of(r#"{"phase":"failed","error":"x"}"#).unwrap().0, r#"{"phase":"failed","error":"x"}"#);
}

#[test]
fn the_viewers_address_is_found_and_only_a_local_one_is_shown() {
    let said = "Viewer for local:hover-x: http://127.0.0.1:8080/viewer/#ticket=abc&files=%2Fhome\nexpires in 12h";
    assert_eq!(spaces::viewer_url(said).as_deref(), Some("http://127.0.0.1:8080/viewer/#ticket=abc&files=%2Fhome"));
    assert_eq!(spaces::viewer_url("open \"https://192.168.64.5:6080/viewer/#t=1\" now").as_deref(), Some("https://192.168.64.5:6080/viewer/#t=1"));
    // The leftmost address that is a viewer's.
    assert_eq!(spaces::viewer_url("see http://example.com/docs then http://10.0.0.2/viewer/#a").as_deref(), Some("http://10.0.0.2/viewer/#a"));
    assert_eq!(spaces::viewer_url("http://127.0.0.1/viewer/#"), None, "nothing after the #");
    assert_eq!(spaces::viewer_url("http:///viewer/#x"), None);
    assert_eq!(spaces::viewer_url("no address, http only"), None);

    let (scheme, host, port, rest) = spaces::split_url("http://192.168.64.5:6080/viewer/#t=1").unwrap();
    assert_eq!((scheme, host.as_str(), port, rest), ("http", "192.168.64.5", 6080, "/viewer/#t=1"));
    assert_eq!(spaces::split_url("https://LocalHost/viewer/#t").map(|(s, h, p, r)| (s.to_owned(), h, p, r.to_owned())), Some(("https".into(), "localhost".into(), 443, "/viewer/#t".into())));
    assert_eq!(spaces::split_url("http://[::1]:9/viewer/#t").map(|(_, h, p, _)| (h, p)), Some(("[::1]".into(), 9)));
    assert_eq!(spaces::split_url("http://user@host/viewer/#t"), None, "no login in an address");
    assert_eq!(spaces::split_url("http://host:notaport/viewer/#t"), None);
    assert_eq!(spaces::split_url("not a url"), None);

    for local in ["127.0.0.1", "localhost", "[::1]", "10.1.2.3", "192.168.64.5", "172.16.0.1", "172.31.255.1"] { assert!(spaces::local_host(local), "{local}"); }
    for away in ["example.com", "10.evil.com", "192.169.0.1", "172.32.0.1", "172.15.0.1", "8.8.8.8", "localhost.evil.com"] { assert!(!spaces::local_host(away), "{away}"); }
}

//! Open in editor: a desk's folder, or a file in it, in VS Code, Zed, Cursor, Kiro IDE or a
//! custom program. Editors are found by the places their installers use and by PATH; nothing
//! is installed. The folder is the session's own (its worktree, once it has one), never the
//! source checkout. Every argument goes to the program as it is (no shell), so a path with
//! spaces, Unicode or shell characters is one argument. The editor is started and let go: it
//! is no child of any agent and ends with nobody.
//!
//! Launch forms, from each editor's own docs: VS Code and its forks (Cursor, Kiro IDE) take
//! `<folder> --goto <file>:<line>:<column>`; Zed takes `<folder> <file>:<line>:<column>`.
//! Kiro IDE's command is `kiro`, which is not kiro-cli (the agent Hover runs): a `kiro` that
//! turns out to be kiro-cli's own file is refused.

use crate::proc::{home, on_path};
use hover_core::model::EditorSettings;
use std::path::{Path, PathBuf};

pub const CUSTOM: &str = "custom";

/// The editors Hover knows: id and the name shown.
pub const BUILTIN: [(&str, &str); 4] = [("vscode", "VS Code"), ("zed", "Zed"), ("cursor", "Cursor"), ("kiro", "Kiro IDE")];

pub fn name_of(id: &str) -> &str {
    if id == CUSTOM { return "Custom editor"; }
    BUILTIN.iter().find(|(i, _)| *i == id).map_or(id, |(_, n)| n)
}

/// What to open: the folder, and perhaps a file in it (relative to it) with a line and column.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Target { pub folder: String, pub file: Option<String>, pub line: Option<u32>, pub column: Option<u32> }

impl Target {
    pub fn folder(folder: &str) -> Target { Target { folder: folder.into(), ..Default::default() } }

    /// The folder with a file in it. A file outside the folder (or a link out of it) is dropped and the
    /// folder opens alone.
    pub fn file(folder: &str, rel: &str, line: Option<u32>, column: Option<u32>) -> Target {
        let ok = crate::desk::inside(folder, Some(rel)).is_some();
        Target { folder: folder.into(), file: ok.then(|| rel.to_owned()), line: line.filter(|_| ok), column: column.filter(|_| ok) }
    }
}

/// The folder an open can use, or why not. `cloud` is a Kiro Web session: its work is in Kiro's
/// cloud, so there is nothing here to open (and nothing is cloned to make one).
pub fn check_folder(folder: &str, cloud: bool) -> Result<(), String> {
    if cloud { return Err("This task runs in Kiro's cloud, so there is no local folder to open.".into()); }
    if !crate::usable_folder(Some(folder)) { return Err(format!("The folder isn’t there any more: {folder}")); }
    Ok(())
}

// MARK: Finding

fn file(p: PathBuf) -> Option<PathBuf> { p.is_file().then_some(p) }

#[cfg(windows)]
fn places(id: &str) -> Vec<PathBuf> {
    let local = std::env::var_os("LOCALAPPDATA").map(PathBuf::from);
    let pf = |v: &str| std::env::var_os(v).map(PathBuf::from);
    let under = |root: Option<PathBuf>, rest: &str| root.map(|r| r.join(rest));
    match id {
        "vscode" => vec![under(local.clone(), r"Programs\Microsoft VS Code\Code.exe"), under(pf("ProgramFiles"), r"Microsoft VS Code\Code.exe")],
        "cursor" => vec![under(local.clone(), r"Programs\cursor\Cursor.exe"), under(local.clone(), r"Programs\Cursor\Cursor.exe")],
        "kiro" => vec![under(local.clone(), r"Programs\Kiro\Kiro.exe"), under(pf("ProgramFiles"), r"Kiro\Kiro.exe")],
        "zed" => vec![under(local.clone(), r"Programs\Zed\zed.exe"), under(pf("ProgramFiles"), r"Zed\zed.exe")],
        _ => vec![],
    }.into_iter().flatten().collect()
}

#[cfg(target_os = "macos")]
fn places(id: &str) -> Vec<PathBuf> {
    let app = |a: &str, rest: &str| [PathBuf::from("/Applications").join(a), home().join("Applications").join(a)].map(|p| p.join(rest));
    match id {
        "vscode" => app("Visual Studio Code.app", "Contents/Resources/app/bin/code").to_vec(),
        "cursor" => app("Cursor.app", "Contents/Resources/app/bin/cursor").to_vec(),
        "kiro" => app("Kiro.app", "Contents/Resources/app/bin/kiro").to_vec(),
        "zed" => app("Zed.app", "Contents/MacOS/cli").to_vec(),
        _ => vec![],
    }
}

#[cfg(all(unix, not(target_os = "macos")))]
fn places(id: &str) -> Vec<PathBuf> {
    let h = home();
    match id {
        "vscode" => vec!["/usr/bin/code".into(), "/usr/share/code/bin/code".into(), "/snap/bin/code".into(), "/opt/visual-studio-code/bin/code".into()],
        "cursor" => vec!["/usr/bin/cursor".into(), "/opt/Cursor/cursor".into(), h.join(".local/bin/cursor")],
        "kiro" => vec!["/usr/bin/kiro".into(), "/opt/Kiro/bin/kiro".into(), "/usr/share/kiro/bin/kiro".into(), h.join(".local/bin/kiro")],
        "zed" => vec![h.join(".local/bin/zed"), "/usr/bin/zed".into(), "/usr/bin/zeditor".into(), "/usr/bin/zed-editor".into(), "/usr/local/bin/zed".into()],
        _ => vec![],
    }
}

/// The names an editor answers to on PATH.
fn commands(id: &str) -> &'static [&'static str] {
    match id { "vscode" => &["code"], "cursor" => &["cursor"], "kiro" => &["kiro"], "zed" => &["zed", "zeditor", "zed-editor"], _ => &[] }
}

/// The program that opens the editor, or none when it isn't found. The places its installer uses
/// come first (a real program over a PATH shim), then PATH.
pub fn find(id: &str, s: &EditorSettings) -> Option<PathBuf> {
    if id == CUSTOM {
        let exe = s.custom_exe.as_deref().map(str::trim).filter(|e| !e.is_empty())?;
        let p = PathBuf::from(exe);
        return if p.components().count() > 1 { file(p) } else { on_path(exe) };
    }
    let found = places(id).into_iter().find_map(file).or_else(|| commands(id).iter().find_map(|c| on_path(c)));
    found.filter(|p| id != "kiro" || !is_kiro_cli(p))
}

/// `kiro` on PATH can be kiro-cli's own file (Hover's agent), which opens no window.
fn is_kiro_cli(p: &Path) -> bool {
    let real = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let name = |p: &Path| p.file_stem().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
    let (me, cli) = (real(p), crate::agents::exe(hover_core::model::AgentTool::Kiro).map(|c| real(&c)));
    name(&me).starts_with("kiro-cli") || cli.is_some_and(|c| c == me)
}

/// An editor Hover found: its id, the name shown and its program.
#[derive(Clone, Debug, PartialEq)]
pub struct Found { pub id: String, pub name: String, pub exe: PathBuf }

/// The editors that can be opened now: the four built in that were found, and the custom one when its
/// program is there. Looks at the disk and PATH, so call it off the UI thread.
pub fn available(s: &EditorSettings) -> Vec<Found> {
    BUILTIN.iter().map(|(i, _)| *i).chain([CUSTOM]).filter_map(|id| find(id, s).map(|exe| Found { id: id.into(), name: name_of(id).into(), exe })).collect()
}

/// Which editor a click opens: the one named, else the default, else the only one found. Err says
/// what is missing, so the message can tell the user what to do.
pub fn pick(s: &EditorSettings, choice: Option<&str>, found: &[Found]) -> Result<Found, String> {
    let want = choice.or(s.default.as_deref()).filter(|c| !c.is_empty());
    match want {
        Some(id) => found.iter().find(|f| f.id == id).cloned().ok_or_else(|| if id == CUSTOM {
            "The custom editor's program isn’t there. Set it in Settings → General → Open in editor.".to_owned()
        } else {
            format!("{} wasn’t found on this computer. Install it, or pick another editor or a custom program.", name_of(id))
        }),
        None => match found {
            [one] => Ok(one.clone()),
            [] => Err("No editor was found. Install VS Code, Zed, Cursor or Kiro IDE, or set a custom program in Settings → General → Open in editor.".into()),
            _ => Err("Pick an editor first.".into()),
        },
    }
}

// MARK: Arguments

/// The program's arguments, and a note when the editor couldn't do all that was asked.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan { pub args: Vec<String>, pub note: Option<String> }

const LIMIT: &str = "This editor can’t open a file at a line here, so the folder was opened.";

/// The arguments for `id`. A file is opened at its line and column only when the editor can; the
/// note says when it couldn't.
pub fn plan(id: &str, t: &Target, custom_args: Option<&str>) -> Plan {
    let abs = t.file.as_deref().and_then(|f| crate::desk::inside(&t.folder, Some(f)));
    let place = abs.map(|p| {
        let mut s = p.to_string_lossy().into_owned();
        if let Some(l) = t.line { s += &format!(":{l}"); if let Some(c) = t.column { s += &format!(":{c}"); } }
        s
    });
    match id {
        "zed" => Plan { args: [Some(t.folder.clone()), place].into_iter().flatten().collect(), note: None },
        CUSTOM => custom(t, custom_args.unwrap_or("")),
        _ => match place {
            Some(p) => Plan { args: vec![t.folder.clone(), "--goto".into(), p], note: None },
            None => Plan { args: vec![t.folder.clone()], note: None },
        },
    }
}

/// A custom editor's arguments: one per line, with {folder}, {file}, {line} and {column} filled in. A
/// line that names something there is none of (no file was asked for) is left out, and the note says
/// so. With no lines at all the folder is the one argument.
fn custom(t: &Target, template: &str) -> Plan {
    let lines: Vec<&str> = template.lines().map(|l| l.trim_end_matches('\r')).filter(|l| !l.trim().is_empty()).take(32).collect();
    if lines.is_empty() { return Plan { args: vec![t.folder.clone()], note: t.file.is_some().then(|| LIMIT.into()) }; }
    let abs = t.file.as_deref().and_then(|f| crate::desk::inside(&t.folder, Some(f))).map(|p| p.to_string_lossy().into_owned());
    let (mut args, mut dropped) = (vec![], false);
    for l in lines {
        let needs = |k: &str| l.contains(k);
        if (needs("{file}") && abs.is_none()) || (needs("{line}") && t.line.is_none()) || (needs("{column}") && t.column.is_none()) {
            dropped = true;
            continue;
        }
        args.push(l.replace("{folder}", &t.folder).replace("{file}", abs.as_deref().unwrap_or("")).replace("{line}", &t.line.unwrap_or(0).to_string())
            .replace("{column}", &t.column.unwrap_or(0).to_string()));
    }
    let asked = t.file.is_some() && !template.contains("{file}");
    let note = if asked { Some(LIMIT.to_owned()) } else if dropped { Some("Part of the custom editor’s arguments was left out: no file was chosen.".to_owned()) } else { None };
    Plan { args, note }
}

// MARK: Launching

/// Windows runs `.cmd` and `.bat` through cmd.exe, which reads `& | < > ^ % "` as its own. An argument
/// with one can't be passed as it is, so it is refused rather than changed.
fn shim_safe(exe: &Path, args: &[String]) -> Result<(), String> {
    let shim = exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if cfg!(windows) && shim && args.iter().any(|a| a.contains(['&', '|', '<', '>', '^', '%', '"'])) {
        return Err(format!("Windows can't pass that path to {} safely. Set the editor's program (Code.exe, not code.cmd) in Settings.", exe.display()));
    }
    Ok(())
}

/// Starts the editor and lets go of it. No shell; stdio closed; its own process group, so Hover's
/// signals and the agents' stops never reach it. A thread reaps it when the launcher exits.
pub fn launch(exe: &Path, args: &[String]) -> Result<(), String> {
    shim_safe(exe, args)?;
    let mut cmd = std::process::Command::new(exe);
    cmd.args(args).stdin(std::process::Stdio::null()).stdout(std::process::Stdio::null()).stderr(std::process::Stdio::null());
    for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE"] { cmd.env_remove(v); }
    #[cfg(unix)]
    { use std::os::unix::process::CommandExt; cmd.process_group(0); }
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; cmd.creation_flags(0x0800_0000 | 0x0000_0200 /* CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP */); }
    let mut child = cmd.spawn().map_err(|e| format!("{} didn’t start: {e}", exe.display()))?;
    std::thread::Builder::new().name("editor-reap".into()).spawn(move || { let _ = child.wait(); }).ok();
    Ok(())
}

/// Opens `t` in the editor: the one named, else the default. What to tell the user: where it opened,
/// and the limit that applied. Blocks briefly (it looks at the disk): call it off the UI thread.
pub fn open(s: &EditorSettings, choice: Option<&str>, t: &Target, cloud: bool) -> Result<String, String> {
    check_folder(&t.folder, cloud)?;
    let f = pick(s, choice, &available(s))?;
    let p = plan(&f.id, t, s.custom_args.as_deref());
    launch(&f.exe, &p.args)?;
    hover_core::log::line(&format!("editor: {} opened {}", f.name, t.folder));
    Ok(match p.note { Some(n) => format!("Opened in {}. {n}", f.name), None => format!("Opened in {}.", f.name) })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-editor-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    /// A stand-in editor that writes each argument it gets on a line of its own.
    fn recorder(dir: &Path) -> (PathBuf, PathBuf) {
        let (exe, log) = (dir.join("fake-editor"), dir.join("args.log"));
        std::fs::write(&exe, format!("#!/bin/sh\nfor a in \"$@\"; do printf '%s\\n' \"$a\" >> '{}'; done\n", log.display())).unwrap();
        std::fs::set_permissions(&exe, std::fs::Permissions::from_mode(0o755)).unwrap();
        (exe, log)
    }

    fn wait_for(log: &Path) -> Vec<String> {
        for _ in 0..200 {
            if let Ok(t) = std::fs::read_to_string(log) { if !t.is_empty() { return t.lines().map(str::to_owned).collect(); } }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("the stand-in editor never ran");
    }

    /// The folder and the file reach the program as one argument each, whatever is in their names.
    #[test]
    fn paths_with_spaces_unicode_and_shell_characters_arrive_as_they_are() {
        let d = temp("literal");
        let folder = d.join("my proj ü; touch pwned $(touch pwned2) `x`");
        std::fs::create_dir_all(folder.join("src")).unwrap();
        std::fs::write(folder.join("src/a b.rs"), "x").unwrap();
        let (exe, log) = recorder(&d);
        let f = folder.to_string_lossy().into_owned();
        let p = plan("vscode", &Target::file(&f, "src/a b.rs", Some(7), Some(3)), None);
        launch(&exe, &p.args).unwrap();
        let got = wait_for(&log);
        let real = crate::desk::inside(&f, Some("src/a b.rs")).unwrap();
        assert_eq!(got, [f.clone(), "--goto".to_owned(), format!("{}:7:3", real.display())]);
        assert!(!d.join("pwned").exists() && !d.join("pwned2").exists() && !folder.join("pwned").exists(), "nothing was run as a command");
    }

    #[test]
    fn each_editor_gets_its_own_form_and_a_file_outside_the_folder_is_dropped() {
        let d = temp("forms");
        let f = d.to_string_lossy().into_owned();
        std::fs::write(d.join("x.txt"), "x").unwrap();
        let x = crate::desk::inside(&f, Some("x.txt")).unwrap().to_string_lossy().into_owned();
        let t = Target::file(&f, "x.txt", Some(4), None);
        assert_eq!(plan("zed", &t, None).args, [f.clone(), format!("{x}:4")]);
        assert_eq!(plan("cursor", &t, None).args, [f.clone(), "--goto".into(), format!("{x}:4")]);
        assert_eq!(plan("kiro", &Target::folder(&f), None).args, [f.clone()]);
        let out = Target::file(&f, "../../etc/passwd", Some(1), Some(1));
        assert_eq!((out.file, out.line), (None, None));
    }

    #[test]
    fn a_custom_editor_fills_its_lines_and_says_what_it_could_not_do() {
        let d = temp("custom");
        let f = d.to_string_lossy().into_owned();
        std::fs::write(d.join("m.rs"), "x").unwrap();
        let m = crate::desk::inside(&f, Some("m.rs")).unwrap().to_string_lossy().into_owned();
        let tpl = "--new-window\n{folder}\n--at\n{file}:{line}";
        assert_eq!(plan(CUSTOM, &Target::file(&f, "m.rs", Some(9), None), Some(tpl)).args, ["--new-window".to_owned(), f.clone(), "--at".into(), format!("{m}:9")]);
        // No file asked for: the lines that need one are left out, with a note.
        let p = plan(CUSTOM, &Target::folder(&f), Some(tpl));
        assert_eq!(p.args, ["--new-window".to_owned(), f.clone(), "--at".into()]);
        assert!(p.note.is_some());
        // Arguments that never name a file can't open one: the folder opens and it says so.
        let p = plan(CUSTOM, &Target::file(&f, "m.rs", Some(1), None), Some("{folder}"));
        assert_eq!(p.args, [f.clone()]);
        assert!(p.note.unwrap().contains("folder was opened"));
        assert_eq!(plan(CUSTOM, &Target::folder(&f), None).args, [f]);
    }

    #[test]
    fn the_choice_default_and_missing_editors_say_what_to_do() {
        let found = vec![Found { id: "zed".into(), name: "Zed".into(), exe: "/x/zed".into() }, Found { id: "vscode".into(), name: "VS Code".into(), exe: "/x/code".into() }];
        let s = EditorSettings { default: Some("vscode".into()), ..Default::default() };
        assert_eq!(pick(&s, None, &found).unwrap().id, "vscode");
        assert_eq!(pick(&s, Some("zed"), &found).unwrap().id, "zed", "a one-off choice beats the default");
        assert!(pick(&s, Some("cursor"), &found).unwrap_err().contains("Cursor wasn’t found"));
        assert_eq!(pick(&EditorSettings::default(), None, &found).unwrap_err(), "Pick an editor first.");
        assert_eq!(pick(&EditorSettings::default(), None, &found[..1]).unwrap().id, "zed");
        assert!(pick(&EditorSettings::default(), None, &[]).unwrap_err().contains("No editor was found"));
        assert!(pick(&s, Some(CUSTOM), &found).unwrap_err().contains("custom editor"));
    }

    #[test]
    fn a_cloud_task_and_a_gone_folder_are_refused_with_a_reason() {
        let d = temp("refuse");
        assert!(check_folder(&d.to_string_lossy(), true).unwrap_err().contains("no local folder"));
        assert!(check_folder(&d.join("gone").to_string_lossy(), false).unwrap_err().contains("isn’t there"));
        assert!(check_folder(&d.to_string_lossy(), false).is_ok());
    }

    #[test]
    fn a_custom_program_is_found_by_path_and_settings_survive_json() {
        let d = temp("find");
        let (exe, _) = recorder(&d);
        let s = EditorSettings { default: Some(CUSTOM.into()), custom_exe: Some(exe.to_string_lossy().into()), custom_args: Some("{folder}\n--x".into()) };
        assert_eq!(find(CUSTOM, &s), Some(exe));
        assert!(available(&s).iter().any(|f| f.id == CUSTOM));
        assert_eq!(EditorSettings::from_json(&hover_core::json::parse(&s.to_json().compact()).unwrap()).unwrap(), s);
        assert_eq!(find(CUSTOM, &EditorSettings::default()), None);
    }
}

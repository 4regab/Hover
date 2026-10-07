//! Open in editor: a desk's folder, or a file in it, in VS Code, Zed, Cursor or Kiro IDE.
//! Editors are found by the places their installers use and by PATH; nothing
//! is installed. The folder is the session's own. Every argument goes to the program as it is (no shell), so a path with
//! spaces, Unicode or shell characters is one argument. The editor is started and let go: it
//! is no child of any agent and ends with nobody.
//!
//! Launch forms, from each editor's own docs: VS Code and its forks (Cursor, Kiro IDE) take
//! `<folder> --goto <file>:<line>:<column>`; Zed takes `<folder> <file>:<line>:<column>`.
//! Kiro IDE's command is `kiro`, which is not kiro-cli (the agent Hover runs): a `kiro` that
//! turns out to be kiro-cli's own file is refused.

use crate::proc::on_path;
#[cfg(unix)]
use crate::proc::home;
use std::path::{Path, PathBuf};

/// The editors Hover knows: id and the name shown.
pub const BUILTIN: [(&str, &str); 4] = [("vscode", "VS Code"), ("zed", "Zed"), ("cursor", "Cursor"), ("kiro", "Kiro IDE")];

pub fn name_of(id: &str) -> &str {
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
pub fn find(id: &str) -> Option<PathBuf> {
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

/// The editors that can be opened now: the four Hover knows that were found, in the order of `BUILTIN`.
/// Looks at the disk and PATH, so call it off the UI thread.
pub fn available() -> Vec<Found> {
    BUILTIN.iter().filter_map(|(id, name)| find(id).map(|exe| Found { id: (*id).into(), name: (*name).into(), exe })).collect()
}

/// Which editor a click opens: the one named, else the first one found. Err says
/// what is missing, so the message can tell the user what to do.
pub fn pick(choice: Option<&str>, found: &[Found]) -> Result<Found, String> {
    match choice.filter(|c| !c.is_empty()) {
        Some(id) => found.iter().find(|f| f.id == id).cloned().ok_or_else(|| format!("{} wasn’t found on this computer. Install it, or pick another editor.", name_of(id))),
        None => found.first().cloned().ok_or_else(|| "No editor was found. Install VS Code, Zed, Cursor or Kiro IDE.".into()),
    }
}

// MARK: Arguments

/// The arguments for `id`. A file is opened at its line and column.
pub fn args(id: &str, t: &Target) -> Vec<String> {
    let abs = t.file.as_deref().and_then(|f| crate::desk::inside(&t.folder, Some(f)));
    let place = abs.map(|p| {
        let mut s = p.to_string_lossy().into_owned();
        if let Some(l) = t.line { s += &format!(":{l}"); if let Some(c) = t.column { s += &format!(":{c}"); } }
        s
    });
    match (id, place) {
        ("zed", place) => [Some(t.folder.clone()), place].into_iter().flatten().collect(),
        (_, Some(p)) => vec![t.folder.clone(), "--goto".into(), p],
        (_, None) => vec![t.folder.clone()],
    }
}

// MARK: Launching

/// Windows runs `.cmd` and `.bat` through cmd.exe, which reads `& | < > ^ % "` as its own. An argument
/// with one can't be passed as it is, so it is refused rather than changed.
fn shim_safe(exe: &Path, args: &[String]) -> Result<(), String> {
    let shim = exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    if cfg!(windows) && shim && args.iter().any(|a| a.contains(['&', '|', '<', '>', '^', '%', '"'])) {
        return Err(format!("Windows can't pass that path to {} safely.", exe.display()));
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

/// Opens `t` in the editor: the one named, else the first found. What to tell the user: where it opened.
/// Blocks briefly (it looks at the disk): call it off the UI thread.
pub fn open(choice: Option<&str>, t: &Target, cloud: bool) -> Result<String, String> {
    check_folder(&t.folder, cloud)?;
    let f = pick(choice, &available())?;
    launch(&f.exe, &args(&f.id, t))?;
    hover_core::log::line(&format!("editor: {} opened {}", f.name, t.folder));
    Ok(format!("Opened in {}.", f.name))
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
        launch(&exe, &args("vscode", &Target::file(&f, "src/a b.rs", Some(7), Some(3)))).unwrap();
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
        assert_eq!(args("zed", &t), [f.clone(), format!("{x}:4")]);
        assert_eq!(args("cursor", &t), [f.clone(), "--goto".into(), format!("{x}:4")]);
        assert_eq!(args("kiro", &Target::folder(&f)), [f.clone()]);
        let out = Target::file(&f, "../../etc/passwd", Some(1), Some(1));
        assert_eq!((out.file, out.line), (None, None));
    }

    #[test]
    fn the_choice_and_missing_editors_say_what_to_do() {
        let found = vec![Found { id: "zed".into(), name: "Zed".into(), exe: "/x/zed".into() }, Found { id: "vscode".into(), name: "VS Code".into(), exe: "/x/code".into() }];
        assert_eq!(pick(None, &found).unwrap().id, "zed", "the first one found");
        assert_eq!(pick(Some("vscode"), &found).unwrap().id, "vscode", "a choice beats that");
        assert!(pick(Some("cursor"), &found).unwrap_err().contains("Cursor wasn’t found"));
        assert!(pick(None, &[]).unwrap_err().contains("No editor was found"));
    }

    #[test]
    fn a_cloud_task_and_a_gone_folder_are_refused_with_a_reason() {
        let d = temp("refuse");
        assert!(check_folder(&d.to_string_lossy(), true).unwrap_err().contains("no local folder"));
        assert!(check_folder(&d.join("gone").to_string_lossy(), false).unwrap_err().contains("isn’t there"));
        assert!(check_folder(&d.to_string_lossy(), false).is_ok());
    }
}

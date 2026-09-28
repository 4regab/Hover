//! Core/Paths.cs: everything Hover owns lives in one folder. On Windows that is
//! %APPDATA%\Hover; on Linux $XDG_DATA_HOME/Hover (~/.local/share/Hover). HOVER_DATA_DIR
//! overrides both.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// Paths.Init, with the platform's base folder given, so it can be tested anywhere.
pub fn resolve(overridden: Option<&std::ffi::OsStr>, app_data: Option<PathBuf>) -> std::io::Result<PathBuf> {
    if let Some(o) = overridden.filter(|o| !o.to_string_lossy().trim().is_empty()) {
        let forced = crate::platform::full_path(Path::new(o));
        std::fs::create_dir_all(&forced)?;
        return Ok(forced);
    }
    let base = app_data.ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no application data folder"))?;
    let dir = base.join("Hover");
    // The app used to be called Noty. An existing install's settings and key come
    // across the first time the renamed build runs, only when there is an old folder
    // and no new one yet. There was never a Linux Noty, so there it never fires.
    let legacy = base.join("Noty");
    if !dir.is_dir() && legacy.is_dir() {
        // Not the log: logging needs this very folder.
        if let Err(e) = std::fs::rename(&legacy, &dir) { eprintln!("hover: data migration failed — {e}"); }
    }
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

static SUPPORT: OnceLock<PathBuf> = OnceLock::new();

/// Paths.Support, found once. A folder that can't be made stops the app, as the C#
/// static initialiser does.
pub fn support() -> &'static Path {
    SUPPORT.get_or_init(|| {
        resolve(std::env::var_os("HOVER_DATA_DIR").as_deref(), crate::platform::app_data())
            .unwrap_or_else(|e| panic!("Hover couldn't make its data folder: {e}"))
    })
}

pub fn key() -> PathBuf { support().join("note.key") }
pub fn settings_file() -> PathBuf { support().join("settings.json") }
pub fn log() -> PathBuf { support().join("hover.log") }
pub fn agents() -> PathBuf { support().join("agents") }

/// Path.GetFullPath on Unix: rooted at the working folder and with "." and ".."
/// taken out by the text alone (links are not followed), a trailing separator kept.
pub fn lexical_full_path(p: &Path, cwd: &Path) -> PathBuf {
    use std::path::Component;
    let joined = if p.is_absolute() { p.to_path_buf() } else { cwd.join(p) };
    let mut out = PathBuf::from("/");
    for c in joined.components() {
        match c {
            Component::ParentDir => { out.pop(); }
            Component::Normal(n) => out.push(n),
            _ => {}
        }
    }
    let s = p.to_string_lossy();
    if (s.ends_with('/') || s.ends_with("/.")) && out != Path::new("/") {
        let mut t = out.into_os_string();
        t.push("/");
        return PathBuf::from(t);
    }
    out
}

/// The old planner went with the workspace in 2.0 and the user chose to have it
/// deleted (OwlApp.DropPlanner). The key stays: the history is sealed with it.
pub fn drop_planner(dir: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().into_owned();
        if name.starts_with("planner.dat") && e.path().is_file() {
            match std::fs::remove_file(e.path()) {
                Ok(()) => crate::log::line(&format!("removed {name} (the workspace is gone)")),
                Err(err) => crate::log::line(&format!("couldn't remove the old planner - {err}")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-paths-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn the_override_wins_and_blank_is_none() {
        let base = temp("override");
        let forced = base.join("forced/../data");
        assert_eq!(resolve(Some(forced.as_os_str()), Some(base.clone())).unwrap(), base.join("data"));
        assert!(base.join("data").is_dir());
        assert_eq!(resolve(Some(std::ffi::OsStr::new("  ")), Some(base.clone())).unwrap(), base.join("Hover"));
    }

    #[test]
    fn a_noty_install_moves_across_once() {
        let base = temp("noty");
        std::fs::create_dir_all(base.join("Noty")).unwrap();
        std::fs::write(base.join("Noty/note.key"), b"k").unwrap();
        let dir = resolve(None, Some(base.clone())).unwrap();
        assert_eq!(std::fs::read(dir.join("note.key")).unwrap(), b"k");
        assert!(!base.join("Noty").exists());
        // With both there, the old one is left alone.
        std::fs::create_dir_all(base.join("Noty")).unwrap();
        resolve(None, Some(base.clone())).unwrap();
        assert!(base.join("Noty").is_dir());
    }

    #[test]
    fn full_paths_as_getfullpath_makes_them_on_unix() {
        let cwd = Path::new("/home/u/work");
        assert_eq!(lexical_full_path(Path::new("data"), cwd), Path::new("/home/u/work/data"));
        assert_eq!(lexical_full_path(Path::new("../x/./y"), cwd), Path::new("/home/u/x/y"));
        assert_eq!(lexical_full_path(Path::new("/../../a"), cwd), Path::new("/a"));
        assert_eq!(lexical_full_path(Path::new("d/"), cwd).to_string_lossy(), "/home/u/work/d/");
    }

    #[test]
    fn the_planner_goes_and_the_key_stays() {
        let d = temp("planner");
        for f in ["planner.dat", "planner.dat.tmp", "planner.dat.unreadable-20260101000000", "note.key"] { std::fs::write(d.join(f), "x").unwrap(); }
        drop_planner(&d);
        let left: Vec<_> = std::fs::read_dir(&d).unwrap().flatten().map(|e| e.file_name().into_string().unwrap()).collect();
        assert_eq!(left, vec!["note.key"]);
    }
}

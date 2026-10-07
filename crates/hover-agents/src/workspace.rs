//! What Git says about a task's folder (its branch, whether it is a linked worktree), the Git folders a sandboxed
//! tool must be able to write in a linked worktree, and the holds that keep a checkpoint restore from sharing a folder
//! with a running task.
//!
//! Tasks work in the folder they were given. Hover no longer makes a worktree for a task; a chat made by an earlier
//! version may still sit in one, and these functions still read it.
//!
//! Everything here blocks (git runs); call it off the UI thread.

use crate::github::{run, Ran};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const QUICK: u64 = 20;
const MAX: usize = 4 * 1024 * 1024;

fn git(dir: &Path, args: &[&str], secs: u64) -> Ran {
    match crate::desk::find_git() {
        None => Ran::failed("Git isn’t installed."),
        Some(g) => run(&g, Some(dir), Duration::from_secs(secs), MAX, args, None, &[]),
    }
}

fn last_line(r: &Ran) -> String {
    let t = if r.err.trim().is_empty() { &r.out } else { &r.err };
    t.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("git failed").trim().to_owned()
}

fn plain(p: &str) -> PathBuf { PathBuf::from(p.trim().trim_start_matches(r"\\?\")) }

// MARK: Looking at a folder

/// What a folder is to Git.
#[derive(Clone, Debug, PartialEq)]
pub struct Info {
    /// The branch checked out; none when detached.
    pub branch: Option<String>,
    /// The folder is a linked worktree, not the main checkout.
    pub linked: bool,
}

/// Why a folder has no `Info`.
#[derive(Clone, Debug, PartialEq)]
pub enum Not { GitMissing, NotRepo, Other(String) }

pub fn inspect(folder: &str) -> Result<Info, Not> {
    let dir = Path::new(folder);
    if crate::desk::find_git().is_none() { return Err(Not::GitMissing); }
    let top = git(dir, &["rev-parse", "--show-toplevel"], QUICK);
    if !top.ok() { return Err(if top.err.to_lowercase().contains("not a git repository") { Not::NotRepo } else { Not::Other(last_line(&top)) }); }
    let root = plain(top.out.trim());
    let one = |args: &[&str]| { let r = git(dir, args, QUICK); r.ok().then(|| r.out.trim().to_owned()).filter(|s| !s.is_empty()) };
    let git_dir = one(&["rev-parse", "--git-dir"]).map(|g| abs(dir, &g));
    let common = one(&["rev-parse", "--git-common-dir"]).map(|g| abs(dir, &g)).or_else(|| git_dir.clone()).unwrap_or_else(|| root.join(".git"));
    let linked = git_dir.as_ref().is_some_and(|g| real(g) != real(&common));
    Ok(Info {
        branch: one(&["symbolic-ref", "--short", "-q", "HEAD"]),
        linked,
    })
}

fn abs(dir: &Path, p: &str) -> PathBuf { let p = plain(p); if p.is_absolute() { p } else { dir.join(p) } }
fn real(p: &Path) -> PathBuf { std::fs::canonicalize(p).map(|c| plain(&c.to_string_lossy())).unwrap_or_else(|_| p.to_path_buf()) }

/// The Git folders a worktree's commits write to: its own `.git/worktrees/<name>` and the main
/// `.git`. The sandbox must let a tool write there, or `git commit` fails in the worktree. Empty
/// for a folder that is no linked worktree.
pub fn git_dirs(folder: &str) -> Vec<String> {
    let dot = Path::new(folder).join(".git");
    let Ok(text) = std::fs::read_to_string(&dot) else { return vec![] };
    let Some(gd) = text.lines().find_map(|l| l.strip_prefix("gitdir:")).map(str::trim) else { return vec![] };
    let gd = abs(Path::new(folder), gd);
    let mut out = vec![gd.to_string_lossy().into_owned()];
    if let Ok(c) = std::fs::read_to_string(gd.join("commondir")) {
        let common = abs(&gd, c.trim());
        out.push(real(&common).to_string_lossy().into_owned());
    }
    out
}

// MARK: Locks

/// Folders that are held while something that must not share them runs (a checkpoint restore).
static HELD: Mutex<Vec<(PathBuf, String)>> = Mutex::new(Vec::new());

/// A path with its links followed as far as it exists, for comparing two folders.
fn key(p: &str) -> PathBuf { crate::desk::real(Path::new(p)) }

/// Whether two folders are the same or one holds the other.
pub fn overlaps(a: &str, b: &str) -> bool {
    let (a, b) = (key(a), key(b));
    let same = |x: &std::ffi::OsStr, y: &std::ffi::OsStr| if cfg!(target_os = "linux") { x == y } else { x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase() };
    let (ca, cb): (Vec<_>, Vec<_>) = (a.components().collect(), b.components().collect());
    ca.iter().zip(&cb).all(|(x, y)| same(x.as_os_str(), y.as_os_str()))
}

/// A hold on a folder (and everything that overlaps it), released when dropped.
#[derive(Debug)]
pub struct Hold(usize);

/// Holds `folder` for `why`. Err names what already holds an overlapping folder.
pub fn hold(folder: &str, why: &str) -> Result<Hold, String> {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(1);
    let mut g = HELD.lock().unwrap();
    if let Some((f, w)) = g.iter().find(|(f, _)| overlaps(&f.to_string_lossy(), folder)) { return Err(format!("{} is in progress in {}.", w.split('\u{0}').next().unwrap_or(""), f.display())); }
    let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    g.push((key(folder), format!("{why}\u{0}{id}")));
    Ok(Hold(id))
}

impl Drop for Hold {
    fn drop(&mut self) { HELD.lock().unwrap().retain(|(_, w)| !w.ends_with(&format!("\u{0}{}", self.0))); }
}

/// What holds `folder` (or a folder that overlaps it) now, if anything.
pub fn held(folder: &str) -> Option<String> {
    HELD.lock().unwrap().iter().find(|(f, _)| overlaps(&f.to_string_lossy(), folder)).map(|(_, w)| w.split('\u{0}').next().unwrap_or("").to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-ws-{name}-{}-{}", std::process::id(), hover_core::guid_n().chars().take(6).collect::<String>()));
        std::fs::create_dir_all(&d).unwrap();
        // Without Windows' \\?\ prefix, which git can't take in a path.
        PathBuf::from(std::fs::canonicalize(&d).unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = Command::new("git").current_dir(dir).args(["-c", "user.name=T", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_owned()
    }

    #[test]
    fn a_folder_is_looked_at_for_its_branch_and_whether_it_is_a_linked_worktree() {
        let d = temp("inspect");
        assert_eq!(inspect(&d.to_string_lossy()), Err(Not::NotRepo));
        sh(&d, &["init", "-q"]);
        let i = inspect(&d.to_string_lossy()).unwrap();
        assert!(!i.linked, "no commit yet: {i:?}");
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        sh(&d, &["add", "."]);
        sh(&d, &["commit", "-qm", "first"]);
        let wt = d.join("linked");
        sh(&d, &["worktree", "add", "-q", "-b", "side", &wt.to_string_lossy()]);
        let (main, side) = (inspect(&d.to_string_lossy()).unwrap(), inspect(&wt.to_string_lossy()).unwrap());
        assert_eq!((main.branch.as_deref(), main.linked), (Some("main"), false));
        assert_eq!((side.branch.as_deref(), side.linked), (Some("side"), true));
    }

    #[test]
    fn a_linked_worktrees_git_folders_are_found_for_the_sandbox() {
        let d = temp("gitdirs");
        let (gd, wt) = (d.join("main.git/worktrees/wt"), d.join("wt"));
        std::fs::create_dir_all(&gd).unwrap();
        std::fs::create_dir_all(&wt).unwrap();
        std::fs::write(gd.join("commondir"), "../..\n").unwrap();
        std::fs::write(wt.join(".git"), format!("gitdir: {}\n", gd.display())).unwrap();
        let got = git_dirs(&wt.to_string_lossy());
        assert!(got.len() == 2 && got[1].ends_with("main.git"), "{got:?}");
        assert!(git_dirs(&d.to_string_lossy()).is_empty(), "a folder that is no linked worktree has none");
    }

    #[test]
    fn folders_overlap_when_one_holds_the_other_and_a_hold_blocks_both_ways() {
        let d = temp("lock");
        let (a, b, c) = (d.join("proj"), d.join("proj/sub"), d.join("other"));
        for p in [&a, &b, &c] { std::fs::create_dir_all(p).unwrap(); }
        let (sa, sb, sc) = (a.to_string_lossy().into_owned(), b.to_string_lossy().into_owned(), c.to_string_lossy().into_owned());
        assert!(overlaps(&sa, &sb) && overlaps(&sb, &sa) && overlaps(&sa, &sa));
        assert!(!overlaps(&sa, &sc) && !overlaps(&sa, &format!("{sa}2")), "a name that merely starts the same is another folder");
        let h = hold(&sa, "A restore").unwrap();
        assert_eq!(held(&sb).as_deref(), Some("A restore"), "a folder inside is held too");
        assert!(hold(&sb, "Another").unwrap_err().contains("A restore is in progress"));
        assert!(hold(&sc, "Elsewhere").is_ok());
        drop(h);
        assert_eq!(held(&sb), None);
        assert!(hold(&sb, "Now free").is_ok());
    }
}

//! Checkpoints: the project folder as it was before and after each turn, so the chat can
//! put it back. A shadow git store in Hover's own data folder, one per session (named by
//! its key), with the project as its work tree: the project's own `.git` is never read or
//! written, its `.gitignore` decides what is left out, and a checkpoint is a tree id.
//! Needs `git` on PATH; without it there are no checkpoints and nothing else changes.
//!
//! Nothing here runs a shell: git gets an argument list, and a tree id is checked to be
//! hex before it goes to one.

use crate::proc::{hidden, home};
use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// The longest one git command may take. A huge folder that doesn't finish in this time
/// gets no checkpoint for that turn; the turn itself goes on.
const LIMIT: Duration = Duration::from_secs(90);

pub struct Checkpoints {
    dir: PathBuf,
    git: PathBuf,
    /// One command at a time: they share each store's index.
    lock: Mutex<()>,
    /// Sessions whose folder was too slow to keep: not tried again, so a huge folder costs
    /// one wait, not one per turn.
    slow: Mutex<HashSet<String>>,
}

const TOO_LONG: &str = "git took too long";

/// What a git command said.
struct Ran { ok: bool, out: String, err: String }

impl Checkpoints {
    /// The stores go under `dir`. None when git isn't installed (a Mac's /usr/bin/git
    /// stub without the Command Line Tools would open Apple's install dialog every turn).
    pub fn new(dir: PathBuf) -> Option<Checkpoints> { crate::desk::find_git().map(|git| Checkpoints { dir, git, lock: Mutex::new(()), slow: Mutex::new(HashSet::new()) }) }

    /// The folder as it is now, kept: its tree id. None, with the reason in the log, when
    /// it can't be (no such folder, one too broad to keep, git failing or too slow).
    pub fn snapshot(&self, key: &str, folder: &str) -> Option<String> {
        if self.slow.lock().unwrap().contains(key) { return None; }
        match self.try_snapshot(key, folder) {
            Ok(t) => Some(t),
            Err(e) => {
                hover_core::log::line(&format!("checkpoint: {e}"));
                if e == TOO_LONG { self.slow.lock().unwrap().insert(key.to_owned()); }
                None
            }
        }
    }

    fn try_snapshot(&self, key: &str, folder: &str) -> Result<String, String> {
        let folder = usable(folder)?;
        let repo = self.repo(key)?;
        let _one = self.lock.lock().unwrap();
        self.ensure(&repo, &folder)?;
        self.sync(&repo, &folder)?;
        self.tree(&repo, &folder)
    }

    /// Puts the folder back to a checkpoint: files that changed are as they were, ones
    /// added since are gone, ones deleted since are back. What `.gitignore` leaves out is
    /// not touched. The folder as it was just before is kept as [`Checkpoints::undo_tree`].
    pub fn restore(&self, key: &str, folder: &str, tree: &str) -> Result<(), String> {
        if !is_id(tree) { return Err("That checkpoint is not valid.".into()); }
        let folder = usable(folder)?;
        let repo = self.repo(key)?;
        if !repo.join("HEAD").is_file() { return Err("This chat has no checkpoints kept.".into()); }
        let _one = self.lock.lock().unwrap();
        let kind = self.run(&repo, &folder, &["cat-file", "-t", tree])?;
        if !kind.ok || kind.out.trim() != "tree" { return Err("That checkpoint is no longer kept.".into()); }
        // The index becomes the folder as it is, so that `read-tree --reset -u` knows every
        // file to take out as well as every file to put back.
        self.sync(&repo, &folder)?;
        let now = self.tree(&repo, &folder)?;
        let _ = std::fs::write(repo.join("hover-undo"), &now);
        let r = self.run(&repo, &folder, &["read-tree", "--reset", "-u", tree])?;
        if !r.ok { return Err(format!("The files couldn't be put back: {}", last_line(&r.err))); }
        Ok(())
    }

    /// The folder as it was just before the last restore: a safety net kept in the store,
    /// to put files back if a restore took more than was meant.
    pub fn undo_tree(&self, key: &str) -> Option<String> {
        let t = std::fs::read_to_string(self.repo(key).ok()?.join("hover-undo")).ok()?;
        is_id(t.trim()).then(|| t.trim().to_owned())
    }

    /// A session was deleted: its checkpoints go with it.
    pub fn delete(&self, key: &str) {
        let Ok(repo) = self.repo(key) else { return };
        let _one = self.lock.lock().unwrap();
        if repo.exists() { let _ = std::fs::remove_dir_all(&repo); }
    }

    fn repo(&self, key: &str) -> Result<PathBuf, String> {
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') { return Err("That session has no usable key.".into()); }
        Ok(self.dir.join(format!("{key}.git")))
    }

    fn ensure(&self, repo: &Path, folder: &Path) -> Result<(), String> {
        if repo.join("HEAD").is_file() { return Ok(()); }
        std::fs::create_dir_all(repo).map_err(|e| format!("couldn't make the store: {e}"))?;
        let r = self.run(repo, folder, &["init", "-q", "--template="])?;
        if r.ok { Ok(()) } else { Err(format!("git init: {}", last_line(&r.err))) }
    }

    /// The index becomes the folder as it is. A file git can't read is left out, not fatal.
    fn sync(&self, repo: &Path, folder: &Path) -> Result<(), String> {
        let _ = std::fs::remove_file(repo.join("index.lock"));
        self.run(repo, folder, &["add", "-A", "--ignore-errors"])?;
        Ok(())
    }

    fn tree(&self, repo: &Path, folder: &Path) -> Result<String, String> {
        let r = self.run(repo, folder, &["write-tree"])?;
        let t = r.out.trim();
        if r.ok && is_id(t) { Ok(t.to_owned()) } else { Err(format!("git write-tree: {}", last_line(&r.err))) }
    }

    /// One git command against the store, with the folder as its work tree. Err when git
    /// doesn't start or takes over `LIMIT` (it is ended then).
    fn run(&self, repo: &Path, folder: &Path, args: &[&str]) -> Result<Ran, String> {
        let (gd, wt) = (repo.to_string_lossy().into_owned(), folder.to_string_lossy().into_owned());
        let mut all: Vec<&str> = vec!["--git-dir", &gd, "--work-tree", &wt];
        // Bytes exactly as they are (no line-ending changes), long paths on Windows, no
        // background work, and no refusal over who owns the folder.
        for c in ["core.autocrlf=false", "core.safecrlf=false", "core.quotepath=off", "core.longpaths=true", "core.fsmonitor=false", "gc.auto=0", "maintenance.auto=false", "safe.directory=*"] {
            all.push("-c");
            all.push(c);
        }
        all.extend_from_slice(args);
        let mut cmd = hidden(&self.git, &all);
        cmd.current_dir(folder).env("GIT_TERMINAL_PROMPT", "0").env("GIT_OPTIONAL_LOCKS", "0");
        for v in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_OBJECT_DIRECTORY", "GIT_COMMON_DIR"] { cmd.env_remove(v); }
        let mut child = cmd.spawn().map_err(|e| format!("git didn't start: {e}"))?;
        drop(child.stdin.take());
        let drain = |mut p: Box<dyn Read + Send>| std::thread::spawn(move || { let mut b = vec![]; let _ = p.read_to_end(&mut b); String::from_utf8_lossy(&b).into_owned() });
        let out = drain(Box::new(child.stdout.take().expect("piped")));
        let err = drain(Box::new(child.stderr.take().expect("piped")));
        let t0 = Instant::now();
        let status = loop {
            match child.try_wait() {
                Ok(Some(s)) => break s,
                Ok(None) if t0.elapsed() > LIMIT => { let _ = child.kill(); let _ = child.wait(); return Err(TOO_LONG.into()); }
                Ok(None) => std::thread::sleep(Duration::from_millis(15)),
                Err(e) => return Err(format!("git: {e}")),
            }
        };
        Ok(Ran { ok: status.success(), out: out.join().unwrap_or_default(), err: err.join().unwrap_or_default() })
    }
}

/// The folder as a path git can work in: it exists, and is not a whole drive or the user's
/// home (far too much to keep a copy of for every turn).
fn usable(folder: &str) -> Result<PathBuf, String> {
    let p = std::fs::canonicalize(folder).map_err(|_| "The folder isn't there.".to_string())?;
    let plain = |p: &Path| -> String { p.to_string_lossy().trim_start_matches(r"\\?\").trim_end_matches(['\\', '/']).to_lowercase() };
    if p.parent().is_none() || plain(&p) == plain(&std::fs::canonicalize(home()).unwrap_or_else(|_| home())) {
        return Err("That folder is too broad to keep checkpoints of.".into());
    }
    Ok(PathBuf::from(p.to_string_lossy().trim_start_matches(r"\\?\")))
}

/// A git object id: 40 (SHA-1) or 64 (SHA-256) hex digits.
fn is_id(s: &str) -> bool { (s.len() == 40 || s.len() == 64) && s.bytes().all(|b| b.is_ascii_hexdigit()) }

fn last_line(s: &str) -> &str { s.lines().rev().find(|l| !l.trim().is_empty()).unwrap_or("").trim() }

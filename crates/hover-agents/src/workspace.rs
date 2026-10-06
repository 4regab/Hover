//! Task workspaces: a task that edits a Git project gets a branch and a folder (a Git worktree)
//! of its own, so two tasks never write over each other and a checkpoint restore in one cannot
//! touch the other. The session's folder *is* the worktree, so its terminal, files, diff,
//! checkpoints and the editor launcher follow it without further work.
//!
//! What is promised, and what is not:
//! - A worktree starts from a *committed* state (the base ref the user picked, by default the
//!   branch the checkout is on). Changes not yet committed in the original checkout are neither
//!   included nor moved, discarded or touched; the plan says how many there are.
//! - Making a worktree copies no secrets and runs no setup script. Submodules are not checked
//!   out; initialising them is a separate action the user asks for (`init_submodules`).
//! - A folder that is not a Git project, a repository with no commit, a machine without git and a
//!   Kiro Web task each get an honest alternative (the folder itself, or none), said in words.
//!   Hover never runs `git init`.
//! - Removing a worktree is refused while a task runs there or anything would be lost, and keeps
//!   the branch unless asked. The session's history stays either way, and `recreate` makes the
//!   checkout again from the saved branch (else the saved base commit) and says what was missing.
//! - A worktree isolates files, not processes: it is no OS sandbox.
//!
//! Everything here blocks (git runs); call it off the UI thread.

use crate::cancel::Cancel;
use crate::github::{run, Ran};
use hover_core::ext::WorkspaceBinding;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::Duration;

const QUICK: u64 = 20;
const SLOW: u64 = 180;
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
    /// The checkout's top folder.
    pub root: PathBuf,
    /// Where the repository's objects live (the main checkout's `.git`, also from inside a worktree).
    pub common: PathBuf,
    /// The branch checked out; none when detached.
    pub branch: Option<String>,
    /// HEAD's commit; none before the first commit.
    pub head: Option<String>,
    /// Changed and untracked paths now.
    pub changes: usize,
    pub submodules: bool,
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
    let status = git(dir, &["status", "--porcelain=v1", "--untracked-files=normal"], QUICK);
    Ok(Info {
        branch: one(&["symbolic-ref", "--short", "-q", "HEAD"]),
        head: one(&["rev-parse", "--verify", "-q", "HEAD"]),
        changes: status.out.lines().filter(|l| !l.trim().is_empty()).count(),
        submodules: root.join(".gitmodules").is_file(),
        root, common, linked,
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

// MARK: Planning

/// What a task's workspace will be, shown before it starts.
#[derive(Clone, Debug, PartialEq)]
pub struct Plan {
    pub source: PathBuf,
    pub source_branch: Option<String>,
    /// The ref the task starts from, and the commit it is now.
    pub base: String,
    pub base_commit: String,
    pub branch: String,
    pub folder: PathBuf,
    /// Changed files in the original checkout that the task does not get.
    pub uncommitted: usize,
    pub submodules: bool,
}

impl Plan {
    /// The line the new-task box shows.
    pub fn summary(&self) -> String {
        let short = &self.base_commit[..self.base_commit.len().min(8)];
        let mut s = format!("From {} ({short}) on branch {}, in {}.", self.base, self.branch, self.folder.display());
        if self.uncommitted > 0 {
            let n = self.uncommitted;
            s += &format!(" {n} uncommitted change{} in your checkout {} not included and {} left as {} {}.", if n == 1 { "" } else { "s" }, if n == 1 { "is" } else { "are" },
                if n == 1 { "is" } else { "are" }, if n == 1 { "it" } else { "they" }, if n == 1 { "is" } else { "are" });
        }
        if self.submodules { s += " Submodules are not checked out."; }
        s
    }
}

fn hex(n: usize) -> String { hover_core::guid_n().chars().take(n).collect() }

/// Plans a worktree for a task in `folder`, under `root` (Hover's worktrees folder). `base` is the
/// ref to start from; none means the branch the checkout is on. Err says why it can't be done, in a
/// sentence for the user.
pub fn plan(folder: &str, base: Option<&str>, title: &str, root: &Path) -> Result<Plan, String> {
    let info = inspect(folder).map_err(|n| match n {
        Not::GitMissing => "Git isn’t installed.".to_owned(),
        Not::NotRepo => "This folder isn’t a Git project.".to_owned(),
        Not::Other(e) => e,
    })?;
    if info.head.is_none() { return Err("This repository has no commit yet, so there is nothing to start a branch from.".into()); }
    let base = base.map(str::trim).filter(|b| !b.is_empty()).map(str::to_owned).or_else(|| info.branch.clone()).unwrap_or_else(|| "HEAD".into());
    if base != "HEAD" && !crate::desk::valid_ref(&base) { return Err(format!("“{base}” isn’t a branch or ref name Git takes.")); }
    let resolved = git(&info.root, &["rev-parse", "--verify", "--quiet", &format!("{base}^{{commit}}")], QUICK);
    let commit = resolved.out.trim().to_owned();
    if !resolved.ok() || commit.is_empty() { return Err(format!("There is no branch or ref called “{base}” in this project.")); }
    let repo = info.root.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_else(|| "project".into());
    let repo: String = repo.chars().map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') { c } else { '-' }).take(40).collect();
    let slug = crate::desk::slug(title);
    let branch = (0..20).map(|_| format!("hover/{}-{}", slug.chars().take(30).collect::<String>().trim_end_matches('-'), hex(4)))
        .find(|b| !git(&info.root, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{b}")], QUICK).ok())
        .ok_or("Couldn’t find a free branch name.")?;
    let dest = (0..20).map(|_| root.join(format!("{repo}-{}", hex(8)))).find(|p| !p.exists()).ok_or("Couldn’t find a free folder.")?;
    Ok(Plan { source: info.root, source_branch: info.branch, base, base_commit: commit, branch, folder: dest, uncommitted: info.changes, submodules: info.submodules })
}

// MARK: Making and removing

/// Makes the plan's worktree. All or nothing: a failure, or a cancel while it ran, leaves no folder
/// and no branch behind, so no task starts in something that only looks usable.
pub fn create(p: &Plan, cancel: &Cancel) -> Result<WorkspaceBinding, String> { create_with(p, cancel, || {}) }

/// `create`, with `made` called once git has made the folder (a test cancels there).
fn create_with(p: &Plan, cancel: &Cancel, made: impl FnOnce()) -> Result<WorkspaceBinding, String> {
    if cancel.is_cancelled() { return Err("Cancelled.".into()); }
    if let Some(parent) = p.folder.parent() { std::fs::create_dir_all(parent).map_err(|e| format!("Couldn’t make {}: {e}", parent.display()))?; }
    let dest = p.folder.to_string_lossy().into_owned();
    let r = git(&p.source, &["worktree", "add", "-b", &p.branch, &dest, &p.base_commit], SLOW);
    let fail = |why: String| { rollback(p); Err(why) };
    if !r.ok() { return fail(format!("Couldn’t make the task’s own folder: {}", last_line(&r))); }
    made();
    if cancel.is_cancelled() { return fail("Cancelled.".into()); }
    let checked = git(&p.folder, &["rev-parse", "--is-inside-work-tree"], QUICK);
    if !checked.ok() || checked.out.trim() != "true" { return fail("The new folder isn’t a working checkout.".into()); }
    hover_core::log::line(&format!("workspace: {} on {} from {} ({})", p.folder.display(), p.branch, p.base, &p.base_commit[..8.min(p.base_commit.len())]));
    Ok(WorkspaceBinding { kind: "worktree".into(), source: p.source.to_string_lossy().into_owned(), branch: Some(p.branch.clone()), base: Some(p.base.clone()), base_commit: Some(p.base_commit.clone()) })
}

fn rollback(p: &Plan) {
    let dest = p.folder.to_string_lossy().into_owned();
    let _ = git(&p.source, &["worktree", "remove", "--force", &dest], QUICK);
    let _ = std::fs::remove_dir_all(&p.folder);
    let _ = git(&p.source, &["worktree", "prune"], QUICK);
    // The branch was made just now at the base commit, with nobody on it.
    if git(&p.source, &["rev-parse", "--verify", "--quiet", &format!("refs/heads/{}", p.branch)], QUICK).out.trim() == p.base_commit {
        let _ = git(&p.source, &["branch", "-D", &p.branch], QUICK);
    }
}

/// An optional step after making a worktree, asked for by the user and shown while it runs: the
/// project's submodules, checked out into the new folder.
pub fn init_submodules(folder: &str, cancel: &Cancel) -> Result<(), String> {
    if cancel.is_cancelled() { return Err("Cancelled.".into()); }
    let r = git(Path::new(folder), &["submodule", "update", "--init", "--recursive"], SLOW);
    if r.ok() { Ok(()) } else { Err(format!("Submodules couldn’t be checked out: {}", last_line(&r))) }
}

/// Linked worktrees of the project at `folder`: path, branch, commit. Includes the main checkout.
#[derive(Clone, Debug, PartialEq)]
pub struct Listed { pub path: String, pub branch: Option<String>, pub head: String }

pub fn list(folder: &str) -> Vec<Listed> {
    let r = git(Path::new(folder), &["worktree", "list", "--porcelain"], QUICK);
    let (mut out, mut cur): (Vec<Listed>, Option<Listed>) = (vec![], None);
    for l in r.out.lines() {
        if let Some(p) = l.strip_prefix("worktree ") {
            out.extend(cur.take());
            cur = Some(Listed { path: p.trim().to_owned(), branch: None, head: String::new() });
        } else if let (Some(c), Some(h)) = (cur.as_mut(), l.strip_prefix("HEAD ")) {
            c.head = h.trim().to_owned();
        } else if let (Some(c), Some(b)) = (cur.as_mut(), l.strip_prefix("branch ")) {
            c.branch = Some(b.trim().trim_start_matches("refs/heads/").to_owned());
        }
    }
    out.extend(cur);
    out
}

/// The binding for a task that works in a worktree that is already there (another task's, by the
/// user's choice). Nothing is made or moved; the branch it is on is recorded as it is.
pub fn existing(folder: &str) -> Result<WorkspaceBinding, String> {
    let info = inspect(folder).map_err(|_| "That folder isn’t a Git working folder.".to_owned())?;
    let source = list(folder).into_iter().next().map(|m| m.path).unwrap_or_else(|| info.root.to_string_lossy().into_owned());
    Ok(WorkspaceBinding { kind: "existing".into(), source, branch: info.branch, base: None, base_commit: info.head })
}

/// What stands in the way of removing a task's worktree, and what would be lost.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Removal {
    /// Things that make it unsafe: a run, a terminal, changes that exist nowhere else.
    pub blockers: Vec<String>,
    /// Commits on the task branch that no other branch has.
    pub unmerged: usize,
    pub changed: usize,
    pub untracked: usize,
    pub ignored: usize,
}

impl Removal {
    pub fn safe(&self) -> bool { self.blockers.is_empty() }
}

/// Looks before removing `folder`, the worktree of `b`. `busy` is a task still running there,
/// `terminals` is one open there.
pub fn removal(b: &WorkspaceBinding, folder: &str, busy: bool, terminals: bool) -> Removal {
    let mut r = Removal::default();
    if !b.is_worktree() { r.blockers.push("Hover didn’t make this folder, so it won’t remove it.".into()); return r; }
    if busy { r.blockers.push("A task is still running in it.".into()); }
    if terminals { r.blockers.push("A terminal is open in it.".into()); }
    let dir = Path::new(folder);
    if !dir.is_dir() { return r; }
    let st = git(dir, &["status", "--porcelain=v1", "--untracked-files=all", "--ignored=matching"], SLOW);
    for l in st.out.lines() {
        if l.starts_with("??") { r.untracked += 1; } else if l.starts_with("!!") { r.ignored += 1; } else if !l.trim().is_empty() { r.changed += 1; }
    }
    if let Some(br) = &b.branch {
        if crate::desk::valid_ref(br) {
            let n = git(Path::new(&b.source), &["rev-list", "--count", br, "--not", &format!("--exclude={br}"), "--branches", "--remotes"], QUICK);
            r.unmerged = n.out.trim().parse().unwrap_or(0);
        }
    }
    let plural = |n: usize, w: &str| format!("{n} {w}{}", if n == 1 { "" } else { "s" });
    if r.changed > 0 { r.blockers.push(format!("{} not committed.", plural(r.changed, "changed file"))); }
    if r.untracked > 0 { r.blockers.push(format!("{} that Git doesn’t track.", plural(r.untracked, "new file"))); }
    if r.ignored > 0 { r.blockers.push(format!("{} ignored by Git (build output, caches, local settings) that would be deleted.", plural(r.ignored, "file or folder"))); }
    r
}

#[derive(Clone, Copy, Debug, Default)]
pub struct RemoveOpts {
    /// Remove although there are uncommitted, untracked or ignored files. A running task or an open terminal is never overridden.
    pub force: bool,
    /// Also delete the task branch. Only when its commits are merged elsewhere, unless `discard_unmerged`.
    pub delete_branch: bool,
    pub discard_unmerged: bool,
}

/// Removes the worktree of `b` at `folder`. The branch and the session's history stay unless
/// `delete_branch`. Returns what was done, in sentences.
pub fn remove(b: &WorkspaceBinding, folder: &str, busy: bool, terminals: bool, o: RemoveOpts) -> Result<Vec<String>, String> {
    let before = removal(b, folder, busy, terminals);
    let hard = busy || terminals || !b.is_worktree();
    if (hard && !before.safe()) || (!o.force && !before.safe()) { return Err(before.blockers.join(" ")); }
    if o.delete_branch && before.unmerged > 0 && !o.discard_unmerged {
        return Err(format!("The branch has {} commit{} no other branch has. Keep the branch, or choose to discard them.", before.unmerged, if before.unmerged == 1 { "" } else { "s" }));
    }
    let mut done = vec![];
    if Path::new(folder).exists() {
        let mut args = vec!["worktree", "remove"];
        if o.force { args.push("--force"); }
        args.push(folder);
        let r = git(Path::new(&b.source), &args, SLOW);
        if !r.ok() { return Err(format!("Git wouldn’t remove it: {}", last_line(&r))); }
        done.push("The task’s folder was removed.".to_owned());
    }
    let _ = git(Path::new(&b.source), &["worktree", "prune"], QUICK);
    match (&b.branch, o.delete_branch) {
        (Some(br), true) if crate::desk::valid_ref(br) => {
            let r = git(Path::new(&b.source), &["branch", if o.discard_unmerged { "-D" } else { "-d" }, br], QUICK);
            if r.ok() { done.push(format!("Branch {br} was deleted.")); } else { done.push(format!("Branch {br} was kept: {}", last_line(&r))); }
        }
        (Some(br), _) => done.push(format!("Branch {br} was kept.")),
        _ => {}
    }
    Ok(done)
}

/// Makes a removed (or lost) worktree again at `folder`, from the saved branch; else from the saved
/// base commit, as a new branch of the same name. Says what was missing. Err when neither is left.
pub fn recreate(b: &WorkspaceBinding, folder: &str) -> Result<String, String> {
    if !b.is_worktree() { return Err("This task didn’t have a folder of its own to make again.".into()); }
    let src = Path::new(&b.source);
    if !src.is_dir() || inspect(&b.source).is_err() { return Err(format!("The project it came from isn’t there any more: {}", b.source)); }
    if Path::new(folder).join(".git").exists() && inspect(folder).is_ok() { return Ok("The task’s folder is already there.".into()); }
    let Some(br) = b.branch.as_deref().filter(|br| crate::desk::valid_ref(br)) else { return Err("The task’s branch wasn’t saved.".into()) };
    let _ = git(src, &["worktree", "prune"], QUICK);
    if Path::new(folder).exists() && std::fs::read_dir(folder).map_or(true, |mut d| d.next().is_some()) { return Err(format!("{folder} is there but isn’t the task’s checkout.")); }
    if let Some(parent) = Path::new(folder).parent() { let _ = std::fs::create_dir_all(parent); }
    if git(src, &["show-ref", "--verify", "--quiet", &format!("refs/heads/{br}")], QUICK).ok() {
        let r = git(src, &["worktree", "add", folder, br], SLOW);
        return if r.ok() { Ok(format!("The folder was made again from branch {br}.")) } else { Err(format!("Couldn’t make it again: {}", last_line(&r))) };
    }
    let Some(sha) = b.base_commit.as_deref().filter(|s| s.len() >= 7 && s.bytes().all(|c| c.is_ascii_hexdigit())) else { return Err(format!("Branch {br} is gone and no base commit was saved.")) };
    if !git(src, &["cat-file", "-e", &format!("{sha}^{{commit}}")], QUICK).ok() { return Err(format!("Branch {br} and its base commit {} are both gone from the project.", &sha[..8.min(sha.len())])); }
    let r = git(src, &["worktree", "add", "-b", br, folder, sha], SLOW);
    if r.ok() { Ok(format!("Branch {br} was gone, so the folder was made again from its base commit {}. Work that was only on the old branch is lost.", &sha[..8.min(sha.len())])) } else { Err(format!("Couldn’t make it again: {}", last_line(&r))) }
}

// MARK: Bringing a helper's work back

#[derive(Clone, Debug, PartialEq)]
pub struct BroughtBack { pub merged: bool, pub conflicts: Vec<String>, pub message: String }

/// Merges the helper's branch into the worktree at `into` (an explicit action, never automatic).
/// A conflict ends it: nothing is left half merged, and the files in conflict are listed. The
/// receiving folder must have no uncommitted changes.
pub fn bring_back(branch: &str, into: &str) -> Result<BroughtBack, String> {
    if !crate::desk::valid_ref(branch) { return Err("That branch name isn’t valid.".into()); }
    let dir = Path::new(into);
    let info = inspect(into).map_err(|_| "The folder to bring the changes into isn’t a Git working folder.".to_owned())?;
    if info.changes > 0 { return Err(format!("{} change{} not committed in {into}. Commit or set them aside first.", info.changes, if info.changes == 1 { " is" } else { "s are" })); }
    if info.branch.as_deref() == Some(branch) { return Err("That is the branch this folder is on.".into()); }
    let ident: Vec<&str> = if git(dir, &["config", "user.email"], QUICK).out.trim().is_empty() { vec!["-c", "user.name=Hover", "-c", "user.email=hover@localhost"] } else { vec![] };
    let msg = format!("Bring back the work on {branch}");
    let mut args = ident.clone();
    args.extend(["merge", "--no-ff", "-m", &msg, branch]);
    let r = git(dir, &args, SLOW);
    if r.ok() { return Ok(BroughtBack { merged: true, conflicts: vec![], message: format!("Merged {branch}.") }); }
    let conflicts: Vec<String> = git(dir, &["diff", "--name-only", "--diff-filter=U"], QUICK).out.lines().map(str::to_owned).filter(|l| !l.is_empty()).collect();
    let _ = git(dir, &["merge", "--abort"], QUICK);
    if conflicts.is_empty() { return Err(format!("The merge failed: {}", last_line(&r))); }
    Ok(BroughtBack { merged: false, message: format!("{} file{} in conflict. Nothing was changed.", conflicts.len(), if conflicts.len() == 1 { " is" } else { "s are" }), conflicts })
}

// MARK: Choosing

/// How the user wants a new task's workspace.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    /// A worktree of its own when the folder is a Git project (the default).
    Own { base: Option<String> },
    /// The chosen folder itself.
    Folder,
    /// A worktree that is already there.
    Existing(String),
}

/// A task's folder and workspace, decided.
#[derive(Clone, Debug, PartialEq)]
pub struct Prepared { pub folder: String, pub binding: Option<WorkspaceBinding>, pub note: Option<String> }

/// Decides where a new task works. `read_only` tasks and Kiro Web (`cloud`) tasks edit nothing
/// here, so they get no worktree; a folder that can't have one gets the folder itself, and the
/// note says why. Err only for a choice that was made and can't be met (a missing base ref, a
/// worktree that failed to form, a cancel).
pub fn prepare(folder: &str, choice: &Choice, title: &str, root: &Path, read_only: bool, cloud: bool, cancel: &Cancel) -> Result<Prepared, String> {
    let plain_folder = |note: &str| Ok(Prepared { folder: folder.into(), binding: Some(WorkspaceBinding::folder(folder)), note: Some(note.into()) });
    if cloud { return Ok(Prepared { folder: folder.into(), binding: None, note: Some("A Kiro Web task runs in Kiro’s cloud, so it has no folder here.".into()) }); }
    match choice {
        Choice::Folder => plain_folder("The task works in this folder itself, as you chose."),
        Choice::Existing(path) => {
            let b = existing(path)?;
            Ok(Prepared { folder: path.clone(), binding: Some(b), note: Some("The task works in a worktree that was already there.".into()) })
        }
        Choice::Own { .. } if read_only => Ok(Prepared { folder: folder.into(), binding: Some(WorkspaceBinding::folder(folder)), note: Some("A read-only task changes nothing, so it looks at the folder itself.".into()) }),
        Choice::Own { base } => match inspect(folder) {
            Err(Not::GitMissing) => plain_folder("Git isn’t installed, so the task works in this folder itself."),
            Err(Not::NotRepo) => plain_folder("This isn’t a Git project, so the task works in this folder itself. Hover doesn’t turn it into one."),
            Err(Not::Other(e)) => Err(e),
            Ok(i) if i.head.is_none() => plain_folder("This repository has no commit yet, so the task works in the folder itself."),
            Ok(_) => {
                let p = plan(folder, base.as_deref(), title, root)?;
                let b = create(&p, cancel)?;
                Ok(Prepared { folder: p.folder.to_string_lossy().into_owned(), binding: Some(b), note: Some(p.summary()) })
            }
        },
    }
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
        let d = std::env::temp_dir().join(format!("hover-ws-{name}-{}-{}", std::process::id(), hex(6)));
        std::fs::create_dir_all(&d).unwrap();
        // Without Windows' \\?\ prefix, which git can't take in a path.
        PathBuf::from(std::fs::canonicalize(&d).unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
    }

    fn sh(dir: &Path, args: &[&str]) -> String {
        let o = Command::new("git").current_dir(dir).args(["-c", "user.name=T", "-c", "user.email=t@t", "-c", "commit.gpgsign=false", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
        assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
        String::from_utf8_lossy(&o.stdout).trim().to_owned()
    }

    /// A repository with one commit on main, the way a project folder would be.
    fn repo(name: &str) -> PathBuf {
        let d = temp(name);
        sh(&d, &["init", "-q"]);
        // Git for Windows turns \n into \r\n on checkout by default; the tests compare files byte for byte.
        sh(&d, &["config", "core.autocrlf", "false"]);
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        sh(&d, &["add", "."]);
        sh(&d, &["commit", "-qm", "first"]);
        d
    }

    fn own() -> Choice { Choice::Own { base: None } }

    #[test]
    fn two_tasks_get_their_own_branch_and_folder_and_the_checkout_is_untouched() {
        let src = repo("iso");
        std::fs::write(src.join("a.txt"), "one\nedited but not committed\n").unwrap();
        std::fs::write(src.join("new.txt"), "untracked\n").unwrap();
        let root = temp("iso-root");
        let s = src.to_string_lossy().into_owned();
        let a = prepare(&s, &own(), "Fix the login", &root, false, false, &Cancel::new()).unwrap();
        let b = prepare(&s, &own(), "Fix the login", &root, false, false, &Cancel::new()).unwrap();
        assert_ne!(a.folder, b.folder);
        let (ba, bb) = (a.binding.clone().unwrap(), b.binding.clone().unwrap());
        assert_ne!(ba.branch, bb.branch);
        assert!(ba.is_worktree() && ba.base.as_deref() == Some("main") && ba.base_commit.as_ref().is_some_and(|c| c.len() == 40));
        // Each starts from the committed state: the uncommitted edit is not there.
        assert_eq!(std::fs::read_to_string(Path::new(&a.folder).join("a.txt")).unwrap(), "one\n");
        assert!(!Path::new(&a.folder).join("new.txt").exists());
        // A write in one does not show in the other, nor in the checkout.
        std::fs::write(Path::new(&a.folder).join("a.txt"), "task A\n").unwrap();
        assert_eq!(std::fs::read_to_string(Path::new(&b.folder).join("a.txt")).unwrap(), "one\n");
        assert_eq!(std::fs::read_to_string(src.join("a.txt")).unwrap(), "one\nedited but not committed\n");
        assert_eq!(std::fs::read_to_string(src.join("new.txt")).unwrap(), "untracked\n");
        assert_eq!(sh(&src, &["branch", "--show-current"]), "main");
        // The summary says what is left out.
        assert!(a.note.unwrap().contains("2 uncommitted changes"));
        // Each worktree's folder says where its commits go, for the sandbox.
        assert!(git_dirs(&a.folder).len() == 2 && git_dirs(&a.folder)[1].ends_with(".git"), "{:?}", git_dirs(&a.folder));
        assert!(git_dirs(&s).is_empty());
    }

    #[test]
    fn a_base_ref_is_chosen_checked_and_cannot_be_an_option() {
        let src = repo("base");
        sh(&src, &["branch", "topic"]);
        let root = temp("base-root");
        let s = src.to_string_lossy().into_owned();
        let p = plan(&s, Some("topic"), "x", &root).unwrap();
        assert_eq!(p.base, "topic");
        assert!(plan(&s, Some("nope"), "x", &root).unwrap_err().contains("no branch or ref called “nope”"));
        assert!(plan(&s, Some("--upload-pack=touch /tmp/x"), "x", &root).unwrap_err().contains("isn’t a branch"));
        assert!(plan(&s, Some("a..b"), "x", &root).is_err());
    }

    #[test]
    fn folders_that_cannot_have_a_worktree_get_the_folder_and_a_reason() {
        let root = temp("alt-root");
        let plain_dir = temp("alt-plain");
        let p = prepare(&plain_dir.to_string_lossy(), &own(), "t", &root, false, false, &Cancel::new()).unwrap();
        assert_eq!(p.folder, plain_dir.to_string_lossy());
        assert!(p.note.unwrap().contains("isn’t a Git project") && !plain_dir.join(".git").exists(), "no git init");
        let unborn = temp("alt-unborn");
        sh(&unborn, &["init", "-q"]);
        assert!(prepare(&unborn.to_string_lossy(), &own(), "t", &root, false, false, &Cancel::new()).unwrap().note.unwrap().contains("no commit yet"));
        let src = repo("alt-read");
        let s = src.to_string_lossy().into_owned();
        assert_eq!(prepare(&s, &own(), "t", &root, true, false, &Cancel::new()).unwrap().folder, s, "read only looks at the folder");
        let cloud = prepare(&s, &own(), "t", &root, false, true, &Cancel::new()).unwrap();
        assert!(cloud.binding.is_none() && cloud.note.unwrap().contains("cloud"));
        assert_eq!(prepare(&s, &Choice::Folder, "t", &root, false, false, &Cancel::new()).unwrap().binding.unwrap().kind, "folder");
        assert!(prepare(&s, &Choice::Own { base: Some("missing".into()) }, "t", &root, false, false, &Cancel::new()).is_err());
    }

    #[test]
    fn a_cancel_or_a_failure_leaves_no_folder_and_no_branch() {
        let src = repo("cancel");
        let root = temp("cancel-root");
        let s = src.to_string_lossy().into_owned();
        let c = Cancel::new();
        let p = plan(&s, None, "x", &root).unwrap();
        c.cancel();
        assert_eq!(create(&p, &c).unwrap_err(), "Cancelled.");
        assert!(!p.folder.exists());
        // Cancelled only after git had finished: rolled back.
        let p2 = plan(&s, None, "y", &root).unwrap();
        let c2 = Cancel::new();
        let flag = c2.clone();
        assert_eq!(create_with(&p2, &c2, move || flag.cancel()).unwrap_err(), "Cancelled.");
        assert!(!p2.folder.exists());
        assert!(sh(&src, &["branch", "--list", "hover/*"]).is_empty(), "no stray branch");
        assert_eq!(sh(&src, &["worktree", "list"]).lines().count(), 1, "no stray worktree record");
        // A folder that is in the way makes git fail; nothing is left either.
        let p3 = plan(&s, None, "z", &root).unwrap();
        std::fs::create_dir_all(&p3.folder).unwrap();
        std::fs::write(p3.folder.join("in-the-way"), "x").unwrap();
        assert!(create(&p3, &Cancel::new()).is_err());
        assert!(sh(&src, &["branch", "--list", "hover/*"]).is_empty());
    }

    #[test]
    fn removal_is_refused_while_anything_would_be_lost_and_keeps_the_branch() {
        let src = repo("rm");
        let root = temp("rm-root");
        let s = src.to_string_lossy().into_owned();
        let w = prepare(&s, &own(), "Work", &root, false, false, &Cancel::new()).unwrap();
        let b = w.binding.clone().unwrap();
        let dir = Path::new(&w.folder);
        assert!(removal(&b, &w.folder, true, false).blockers[0].contains("still running"));
        assert!(remove(&b, &w.folder, true, false, RemoveOpts { force: true, ..Default::default() }).is_err(), "a run is never overridden");
        // Work that exists nowhere else.
        std::fs::write(dir.join("a.txt"), "changed\n").unwrap();
        std::fs::write(dir.join("fresh.txt"), "fresh\n").unwrap();
        std::fs::write(dir.join(".gitignore"), "cache/\n").unwrap();
        std::fs::create_dir_all(dir.join("cache")).unwrap();
        std::fs::write(dir.join("cache/x"), "x").unwrap();
        let r = removal(&b, &w.folder, false, false);
        assert!(!r.safe() && r.changed == 1 && r.untracked == 2 && r.ignored == 1, "{r:?}");
        assert!(remove(&b, &w.folder, false, false, RemoveOpts::default()).is_err());
        assert!(dir.exists());
        // Commit it: the branch now has a commit no other branch has.
        sh(dir, &["add", "-A"]);
        sh(dir, &["commit", "-qm", "work"]);
        std::fs::remove_dir_all(dir.join("cache")).unwrap();
        let r = removal(&b, &w.folder, false, false);
        assert!(r.safe() && r.unmerged == 1, "{r:?}");
        let err = remove(&b, &w.folder, false, false, RemoveOpts { delete_branch: true, ..Default::default() }).unwrap_err();
        assert!(err.contains("1 commit"), "{err}");
        assert!(dir.exists());
        // Removing keeps the branch unless asked.
        let done = remove(&b, &w.folder, false, false, RemoveOpts::default()).unwrap();
        assert!(!dir.exists() && done.iter().any(|d| d.contains("was kept")));
        assert!(!sh(&src, &["branch", "--list", b.branch.as_ref().unwrap()]).is_empty());
        // A folder Hover did not make is never removed.
        assert!(!removal(&WorkspaceBinding::folder(&s), &s, false, false).safe());
    }

    #[test]
    fn a_removed_checkout_is_made_again_from_the_branch_or_the_base_and_says_what_was_missing() {
        let src = repo("re");
        let root = temp("re-root");
        let s = src.to_string_lossy().into_owned();
        let w = prepare(&s, &own(), "Again", &root, false, false, &Cancel::new()).unwrap();
        let b = w.binding.clone().unwrap();
        assert!(recreate(&b, &w.folder).unwrap().contains("already there"));
        std::fs::write(Path::new(&w.folder).join("kept.txt"), "k").unwrap();
        sh(Path::new(&w.folder), &["add", "-A"]);
        sh(Path::new(&w.folder), &["commit", "-qm", "kept"]);
        remove(&b, &w.folder, false, false, RemoveOpts::default()).unwrap();
        assert!(recreate(&b, &w.folder).unwrap().contains("from branch"));
        assert!(Path::new(&w.folder).join("kept.txt").exists(), "the committed work came back");
        remove(&b, &w.folder, false, false, RemoveOpts::default()).unwrap();
        sh(&src, &["branch", "-D", b.branch.as_ref().unwrap()]);
        let note = recreate(&b, &w.folder).unwrap();
        assert!(note.contains("gone") && note.contains("lost"), "{note}");
        assert!(!Path::new(&w.folder).join("kept.txt").exists());
        // Nothing left to go by.
        remove(&b, &w.folder, false, false, RemoveOpts::default()).unwrap();
        sh(&src, &["branch", "-D", b.branch.as_ref().unwrap()]);
        let none = WorkspaceBinding { base_commit: Some("f".repeat(40)), ..b.clone() };
        assert!(recreate(&none, &w.folder).unwrap_err().contains("both gone"));
        let no_source = WorkspaceBinding { source: "/no/such/place".into(), ..b };
        assert!(recreate(&no_source, &w.folder).unwrap_err().contains("isn’t there any more"));
    }

    #[test]
    fn a_helpers_work_comes_back_only_on_request_and_a_conflict_changes_nothing() {
        let src = repo("bb");
        let root = temp("bb-root");
        let s = src.to_string_lossy().into_owned();
        let parent = prepare(&s, &own(), "Lead", &root, false, false, &Cancel::new()).unwrap();
        let helper = prepare(&s, &own(), "Helper", &root, false, false, &Cancel::new()).unwrap();
        let (pf, hf) = (Path::new(&parent.folder), Path::new(&helper.folder));
        let hb = helper.binding.unwrap().branch.unwrap();
        std::fs::write(hf.join("helper.txt"), "h\n").unwrap();
        std::fs::write(hf.join("a.txt"), "helper version\n").unwrap();
        sh(hf, &["add", "-A"]);
        sh(hf, &["commit", "-qm", "helper"]);
        assert!(!pf.join("helper.txt").exists(), "nothing comes back by itself");
        std::fs::write(pf.join("a.txt"), "lead version\n").unwrap();
        assert!(bring_back(&hb, &parent.folder).unwrap_err().contains("not committed"));
        sh(pf, &["commit", "-qam", "lead"]);
        let r = bring_back(&hb, &parent.folder).unwrap();
        assert!(!r.merged && r.conflicts == ["a.txt"], "{r:?}");
        assert_eq!(std::fs::read_to_string(pf.join("a.txt")).unwrap(), "lead version\n");
        assert_eq!(sh(pf, &["status", "--porcelain"]), "", "nothing half merged");
        // Without the clash it merges.
        sh(hf, &["checkout", "-q", "HEAD~1"]);
        sh(hf, &["checkout", "-q", "-B", &hb]);
        std::fs::write(hf.join("only.txt"), "o\n").unwrap();
        sh(hf, &["add", "-A"]);
        sh(hf, &["commit", "-qm", "only"]);
        let r = bring_back(&hb, &parent.folder).unwrap();
        assert!(r.merged && pf.join("only.txt").exists());
    }

    #[test]
    fn an_existing_worktree_is_bound_as_it_is_and_listed() {
        let src = repo("ex");
        let root = temp("ex-root");
        let s = src.to_string_lossy().into_owned();
        let w = prepare(&s, &own(), "One", &root, false, false, &Cancel::new()).unwrap();
        let all = list(&s);
        assert_eq!(all.len(), 2);
        // As paths: git lists C:/… on Windows, where the folder is C:\….
        assert!(all.iter().any(|m| Path::new(&m.path) == Path::new(&w.folder) && m.branch == w.binding.as_ref().unwrap().branch));
        let e = prepare(&s, &Choice::Existing(w.folder.clone()), "Two", &root, false, false, &Cancel::new()).unwrap();
        assert_eq!(e.folder, w.folder);
        assert_eq!(e.binding.unwrap().kind, "existing");
        assert!(existing(&temp("ex-plain").to_string_lossy()).is_err());
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

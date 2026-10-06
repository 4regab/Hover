//! Task workspaces at the session level: a worktree bound to a session is what the agent runs in,
//! it comes back after a restart, and a checkpoint restore checks for tasks in overlapping folders
//! and holds its folder. These run the real git.

use hover_agents::cancel::Cancel;
use hover_agents::checkpoint::Checkpoints;
use hover_agents::session::{KiroSessions, Rewind, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::workspace::{self, Choice};
use hover_core::crypto::Crypto;
use hover_core::history::AgentHistory;
use hover_core::model::{AgentTool, KiroState};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hover-wsit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Without Windows' \\?\ prefix, which git can't take in a path.
    PathBuf::from(std::fs::canonicalize(&d).unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
}

fn git(d: &Path, args: &[&str]) {
    let o = Command::new("git").current_dir(d).args(["-c", "user.name=T", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"]).args(args).output().unwrap();
    assert!(o.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&o.stderr));
}

fn repo(root: &Path) -> PathBuf {
    let p = root.join("project");
    std::fs::create_dir_all(&p).unwrap();
    git(&p, &["init", "-q"]);
    std::fs::write(p.join("a.txt"), "one\n").unwrap();
    git(&p, &["add", "."]);
    git(&p, &["commit", "-qm", "first"]);
    p
}

fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } }

/// A runner that writes `done.txt` in the folder it was given, and notes the folders it saw.
fn runner(seen: Arc<Mutex<Vec<String>>>) -> impl Fn(AgentTool) -> RunTask + Send + Sync + 'static {
    move |_| {
        let seen = seen.clone();
        Arc::new(move |a: RunArgs| {
            seen.lock().unwrap().push(a.folder.clone());
            std::fs::write(Path::new(&a.folder).join("done.txt"), "done").unwrap();
            KiroResult::new(KiroState::Completed, "Done.")
        })
    }
}

#[test]
fn a_task_runs_in_its_worktree_and_the_binding_comes_back_after_a_restart() {
    let root = dir("bind");
    let src = repo(&root);
    let hist = root.join("history");
    let crypto = Arc::new(Crypto::with_key([7; 32]));
    let seen: Arc<Mutex<Vec<String>>> = Default::default();
    let k = KiroSessions::new(runner(seen.clone()), Some(Arc::new(AgentHistory::new(hist.clone(), crypto.clone()))));
    let prep = workspace::prepare(&src.to_string_lossy(), &Choice::Own { base: None }, "Add a file", &root.join("worktrees"), false, false, &Cancel::new()).unwrap();
    let ext = hover_core::ext::SessionExt { workspace: prep.binding.clone(), ..Default::default() };
    let s = k.start_bound(AgentTool::Codex, &prep.folder, "Add a file", vec![], None, None, ext).unwrap();
    wait_for(|| k.get(s.id).is_some_and(|x| !x.busy()));
    assert_eq!(seen.lock().unwrap().as_slice(), [prep.folder.clone()], "the agent ran in the worktree");
    assert!(Path::new(&prep.folder).join("done.txt").exists());
    assert!(!src.join("done.txt").exists(), "the original checkout is untouched");
    // A new start of the app: the same binding, from the sealed history.
    let key = k.get(s.id).unwrap().key;
    let again = KiroSessions::new(runner(seen), Some(Arc::new(AgentHistory::new(hist, crypto))));
    let woke = again.wake(&key).expect("the session is in the history");
    assert_eq!(woke.folder, prep.folder);
    assert_eq!(woke.ext.workspace, prep.binding, "branch, base and source are the same");
    assert_eq!(woke.ext.workspace.as_ref().unwrap().kind, "worktree");
}

#[test]
fn a_restore_is_refused_while_a_task_runs_in_an_overlapping_folder_and_holds_its_folder() {
    let root = dir("lock");
    let proj = root.join("proj");
    let inner = proj.join("inner");
    std::fs::create_dir_all(&inner).unwrap();
    std::fs::write(proj.join("seed.txt"), "seed").unwrap();
    let (pf, inf) = (proj.to_string_lossy().into_owned(), inner.to_string_lossy().into_owned());
    let release = Arc::new(Mutex::new(false));
    let r2 = release.clone();
    // A task started in the folder *inside* the project waits; the others finish at once.
    let k = KiroSessions::new(move |_| -> RunTask {
        let r2 = r2.clone();
        Arc::new(move |a: RunArgs| {
            if a.prompt == "hold" { let t = Instant::now(); while !*r2.lock().unwrap() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } }
            KiroResult::new(KiroState::Completed, "ok")
        })
    }, None);
    k.set_checkpoints(Arc::new(Checkpoints::new(root.join("stores")).expect("git on PATH")));
    let a = k.start(AgentTool::Kiro, &pf, "first", vec![]).unwrap().id;
    wait_for(|| k.get(a).is_some_and(|s| !s.busy() && s.turns[0].after.is_some()));
    let b = k.start(AgentTool::Codex, &inf, "hold", vec![]).unwrap().id;
    wait_for(|| k.get(b).is_some_and(|s| s.busy() && s.turns[0].before.is_some()));
    // The project's own chat may not be put back while a task works in a folder inside it.
    let err = k.rewind(a, Rewind::After(0)).unwrap_err();
    assert!(err.contains("overlaps this folder") && err.contains(&inf), "{err}");
    *release.lock().unwrap() = true;
    wait_for(|| k.get(b).is_some_and(|s| !s.busy()));
    // While a restore holds the project, nothing starts in it or inside it, and a reply waits for the hold to go.
    let hold = workspace::hold(&pf, "A checkpoint restore").unwrap();
    assert!(k.start(AgentTool::Kiro, &inf, "x", vec![]).is_none(), "no start in a folder inside the held one");
    assert!(!k.reply(a, "more", vec![]), "no reply either");
    drop(hold);
    assert!(k.reply(a, "more", vec![]));
    wait_for(|| k.get(a).is_some_and(|s| !s.busy() && s.turns.len() == 2));
    assert!(k.rewind(a, Rewind::After(0)).is_ok(), "and the restore works once nothing overlaps");
}

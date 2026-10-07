//! A checkpoint restore at the session level: it checks for tasks in overlapping folders and holds its folder.
//! This runs the real git.

use hover_agents::checkpoint::Checkpoints;
use hover_agents::session::{KiroSessions, Rewind, RunArgs, RunTask};
use hover_agents::stream::KiroResult;
use hover_agents::workspace;
use hover_core::model::{AgentTool, KiroState};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hover-wsit-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // Without Windows' \\?\ prefix, which git can't take in a path.
    PathBuf::from(std::fs::canonicalize(&d).unwrap().to_string_lossy().trim_start_matches(r"\\?\"))
}

fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } }

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

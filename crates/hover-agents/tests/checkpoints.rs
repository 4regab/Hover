//! Checkpoints: the store (a shadow git repository per chat) and a chat put back to one.
//! These run the real git; CI's runners have it.

use hover_agents::checkpoint::Checkpoints;
use hover_agents::session::{KiroSessions, Rewind, RunArgs, RunTask};
use hover_agents::stream::{KiroEvent, KiroResult};
use hover_core::model::{AgentTool, KiroState};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

fn dir(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("hover-checkpoints-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn store(name: &str) -> (Checkpoints, PathBuf, PathBuf) {
    let root = dir(name);
    let (stores, project) = (root.join("stores"), root.join("project"));
    std::fs::create_dir_all(&project).unwrap();
    (Checkpoints::new(stores).expect("git on PATH"), project, root)
}

/// Each prompt a runner was sent, with the conversation it was asked to resume.
type Log = Arc<Mutex<Vec<(String, Option<String>)>>>;

fn read(p: &Path) -> String { std::fs::read_to_string(p).unwrap() }
fn wait_for(f: impl Fn() -> bool) { let t = Instant::now(); while !f() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } }

#[test]
fn a_folder_goes_back_to_a_checkpoint_and_the_restore_can_be_undone() {
    let (c, p, _root) = store("roundtrip");
    std::fs::create_dir_all(p.join("sub")).unwrap();
    std::fs::create_dir_all(p.join("ignored")).unwrap();
    std::fs::write(p.join(".gitignore"), "ignored/\n").unwrap();
    std::fs::write(p.join("a.txt"), "one\r\ntwo\r\n").unwrap();
    std::fs::write(p.join("sub").join("b.txt"), "bee").unwrap();
    std::fs::write(p.join("ignored").join("x.txt"), "keep me as I am").unwrap();
    let folder = p.to_string_lossy().into_owned();

    let t1 = c.snapshot("k1", &folder).expect("a checkpoint");
    assert_eq!(c.snapshot("k1", &folder).as_deref(), Some(t1.as_str()), "the same folder is the same checkpoint");

    // What an agent might do: change a file, delete one, add one, touch an ignored one.
    std::fs::write(p.join("a.txt"), "changed").unwrap();
    std::fs::remove_file(p.join("sub").join("b.txt")).unwrap();
    std::fs::write(p.join("new.txt"), "new").unwrap();
    std::fs::write(p.join("ignored").join("x.txt"), "written since").unwrap();
    let t2 = c.snapshot("k1", &folder).unwrap();
    assert_ne!(t1, t2);

    c.restore("k1", &folder, &t1).unwrap();
    assert_eq!(std::fs::read(p.join("a.txt")).unwrap(), b"one\r\ntwo\r\n", "bytes exactly as they were, line endings too");
    assert_eq!(read(&p.join("sub").join("b.txt")), "bee", "a deleted file is back");
    assert!(!p.join("new.txt").exists(), "a file added since is gone");
    assert_eq!(read(&p.join("ignored").join("x.txt")), "written since", "what .gitignore leaves out is not touched");

    // The folder as it was just before the restore is kept to put back.
    let undo = c.undo_tree("k1").expect("a safety copy");
    assert_eq!(undo, t2);
    c.restore("k1", &folder, &undo).unwrap();
    assert_eq!(read(&p.join("a.txt")), "changed");
    assert_eq!(read(&p.join("new.txt")), "new");
    assert!(!p.join("sub").join("b.txt").exists());
}

#[test]
fn the_projects_own_git_is_never_touched() {
    let (c, p, _root) = store("owngit");
    let git = |args: &[&str]| std::process::Command::new("git").args(args).current_dir(&p).output().unwrap();
    git(&["init", "-q"]);
    std::fs::write(p.join("a.txt"), "a").unwrap();
    let before = String::from_utf8_lossy(&git(&["status", "--porcelain"]).stdout).into_owned();
    let index_before = std::fs::metadata(p.join(".git").join("index")).ok().map(|m| m.len());
    let t = c.snapshot("k2", &p.to_string_lossy()).unwrap();
    std::fs::write(p.join("a.txt"), "b").unwrap();
    c.restore("k2", &p.to_string_lossy(), &t).unwrap();
    assert_eq!(read(&p.join("a.txt")), "a");
    assert_eq!(String::from_utf8_lossy(&git(&["status", "--porcelain"]).stdout), before, "its status is as it was");
    assert_eq!(std::fs::metadata(p.join(".git").join("index")).ok().map(|m| m.len()), index_before, "its index was not written");
    assert!(!p.join(".git").join("refs").join("hover").exists());
}

#[test]
fn folders_too_broad_and_bad_ids_are_refused_and_delete_removes_the_store() {
    let (c, p, root) = store("refuse");
    let top = std::env::temp_dir().ancestors().last().unwrap().to_string_lossy().into_owned();
    assert!(c.snapshot("k3", &top).is_none(), "a whole drive is not kept");
    let home = std::env::var(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).unwrap();
    assert!(c.snapshot("k3", &home).is_none(), "nor is the home folder");
    assert!(c.snapshot("k3", &p.join("missing").to_string_lossy()).is_none());
    assert!(c.snapshot("../evil", &p.to_string_lossy()).is_none(), "a key is a plain name");
    let folder = p.to_string_lossy().into_owned();
    let t = c.snapshot("k3", &folder);
    assert!(t.is_some());
    assert!(c.restore("k3", &folder, "--help").is_err());
    assert!(c.restore("k3", &folder, &"0".repeat(40)).is_err(), "an id the store doesn't have");
    assert!(root.join("stores").join("k3.git").exists());
    c.delete("k3");
    assert!(!root.join("stores").join("k3.git").exists());
}

/// A runner that writes the file a "write NAME" prompt (its last line) names, and notes each
/// prompt it was sent with the conversation it was asked to resume.
fn writer(log: Log) -> impl Fn(AgentTool) -> RunTask + Send + Sync + 'static {
    move |_| {
        let log = log.clone();
        Arc::new(move |a: RunArgs| {
            log.lock().unwrap().push((a.prompt.clone(), a.resume.clone()));
            (a.events)(KiroEvent { session_id: Some("conversation-1".into()), ..Default::default() });
            if let Some(name) = a.prompt.lines().last().and_then(|l| l.strip_prefix("write ")) { std::fs::write(Path::new(&a.folder).join(name), name).unwrap(); }
            KiroResult { state: KiroState::Completed, text: "Done.".into(), exit_code: Some(0), unconfirmed: false }
        })
    }
}

#[test]
fn a_chat_goes_back_to_an_answer_or_tries_a_message_again() {
    let root = dir("chat");
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    std::fs::write(project.join("seed.txt"), "seed").unwrap();
    let folder = project.to_string_lossy().into_owned();
    let log: Log = Default::default();
    let k = KiroSessions::new(writer(log.clone()), None);
    k.set_checkpoints(Arc::new(Checkpoints::new(root.join("stores")).expect("git on PATH")));

    let id = k.start(AgentTool::Kiro, &folder, "write one.txt", vec![]).unwrap().id;
    wait_for(|| k.get(id).is_some_and(|s| !s.busy()));
    for name in ["two.txt", "three.txt"] {
        assert!(k.reply(id, &format!("write {name}"), vec![]));
        wait_for(|| k.get(id).is_some_and(|s| !s.busy() && s.turns.last().is_some_and(|t| t.prompt.ends_with(name) && t.result.is_some())));
    }
    let s = k.get(id).unwrap();
    assert_eq!(s.turns.len(), 3);
    assert!(s.turns.iter().all(|t| t.before.is_some() && t.after.is_some()), "every turn has both checkpoints");
    assert_eq!(s.turns[1].before, s.turns[0].after, "a turn starts where the one before ended");
    assert_ne!(s.turns[0].before, s.turns[0].after, "and the first turn changed the folder");
    assert!(project.join("three.txt").exists());

    // Back to just after the first answer: its file stays, the later ones go, the chat is cut.
    k.rewind(id, Rewind::After(0)).unwrap();
    assert!(project.join("seed.txt").exists() && project.join("one.txt").exists());
    assert!(!project.join("two.txt").exists() && !project.join("three.txt").exists());
    assert_eq!(k.get(id).unwrap().turns.len(), 1);
    assert_eq!(k.get(id).unwrap().state, KiroState::Completed);

    // The next message carries one note about it, then the message, on the same conversation.
    assert!(k.reply(id, "write four.txt", vec![]));
    wait_for(|| k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == 2 && s.turns[1].result.is_some()));
    let sent = log.lock().unwrap().last().cloned().unwrap();
    assert!(sent.0.starts_with("[Hover handoff] The project's files were just put back"), "{}", sent.0);
    assert!(sent.0.contains("“write one.txt”") && sent.0.ends_with("write four.txt"), "{}", sent.0);
    assert_eq!(sent.1, None, "the agent still remembered the removed turns, so it starts a new conversation from an account of the ones that remain");
    assert!(k.reply(id, "write five.txt", vec![]));
    wait_for(|| k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == 3 && s.turns[2].result.is_some()));
    assert_eq!(log.lock().unwrap().last().unwrap().0, "write five.txt", "the note is sent once");

    // Try the second message again: the files as they were before it, and it goes again.
    k.rewind(id, Rewind::Before(1)).unwrap();
    wait_for(|| k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == 2 && s.turns[1].result.is_some()));
    let s = k.get(id).unwrap();
    assert_eq!(s.turns[1].prompt, "write four.txt", "the same message, sent again");
    assert!(project.join("four.txt").exists() && !project.join("five.txt").exists());
    assert!(log.lock().unwrap().last().unwrap().0.ends_with("write four.txt"));

    // The very first message again: a new conversation, and the folder as it was before anything.
    k.rewind(id, Rewind::Before(0)).unwrap();
    wait_for(|| k.get(id).is_some_and(|s| !s.busy() && s.turns.len() == 1 && s.turns[0].result.is_some()));
    assert_eq!(log.lock().unwrap().last().unwrap().1, None, "nothing to resume");
    assert!(project.join("one.txt").exists() && !project.join("four.txt").exists() && project.join("seed.txt").exists());

    // Deleting the chat takes its checkpoints.
    let key = k.get(id).unwrap().key;
    assert!(root.join("stores").join(format!("{key}.git")).exists());
    k.delete(&key);
    assert!(!root.join("stores").join(format!("{key}.git")).exists());
}

#[test]
fn a_running_chat_or_a_turn_without_a_checkpoint_is_not_rewound() {
    let root = dir("refused");
    let project = root.join("project");
    std::fs::create_dir_all(&project).unwrap();
    let folder = project.to_string_lossy().into_owned();
    // With no store at all (git missing, or not switched on) there is nothing to go back to.
    let bare = KiroSessions::new(writer(Default::default()), None);
    let id = bare.start(AgentTool::Kiro, &folder, "write a.txt", vec![]).unwrap().id;
    wait_for(|| bare.get(id).is_some_and(|s| !s.busy()));
    assert!(bare.rewind(id, Rewind::After(0)).unwrap_err().contains("git"));

    // One that is running is left alone: the agent could be writing.
    let release = Arc::new(Mutex::new(false));
    let r2 = release.clone();
    let k = KiroSessions::new(move |_| -> RunTask {
        let r2 = r2.clone();
        Arc::new(move |_| { let t = Instant::now(); while !*r2.lock().unwrap() && t.elapsed() < Duration::from_secs(20) { std::thread::sleep(Duration::from_millis(10)); } KiroResult::new(KiroState::Completed, "ok") })
    }, None);
    k.set_checkpoints(Arc::new(Checkpoints::new(root.join("stores")).expect("git on PATH")));
    let id = k.start(AgentTool::Kiro, &folder, "wait", vec![]).unwrap().id;
    wait_for(|| k.get(id).is_some_and(|s| s.turns[0].before.is_some()));
    assert_eq!(k.rewind(id, Rewind::Before(0)).unwrap_err(), "Stop the run first.");
    *release.lock().unwrap() = true;
    wait_for(|| k.get(id).is_some_and(|s| !s.busy()));
    assert!(k.get(id).unwrap().turns[0].after.is_some());
    assert!(k.rewind(id, Rewind::After(5)).is_err(), "no such message");
}

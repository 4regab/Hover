//! What the github and desk tests share: a stand-in gh (fixtures/fakegh.rs, built once
//! with rustc), a folder of its scripts, and real git repositories in temp folders.
//! Nothing here reaches the network or the user's gh, git config or sign-in.

#![allow(dead_code)]

use hover_agents::desk::{Desk, Snap};
use hover_agents::github::GitHubCli;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, OnceLock};

pub const EXE: &str = if cfg!(windows) { ".exe" } else { "" };

/// The built stand-in, once per test run: kept between runs in one folder, rebuilt when
/// its source is newer.
fn built() -> &'static Path {
    static BUILT: OnceLock<PathBuf> = OnceLock::new();
    BUILT.get_or_init(|| {
        let out = std::env::temp_dir().join("hover-fakegh-build");
        std::fs::create_dir_all(&out).unwrap();
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/fakegh.rs");
        let exe = out.join(format!("fakegh{EXE}"));
        let newer = |a: &Path, b: &Path| std::fs::metadata(a).and_then(|m| m.modified()).ok() > std::fs::metadata(b).and_then(|m| m.modified()).ok();
        if exe.is_file() && !newer(&src, &exe) { return exe; }
        // Built beside it and moved into place, so two test programs starting together can't meet half a file.
        let mine = out.join(format!("fakegh-{}{EXE}", std::process::id()));
        let rustc = std::env::var("RUSTC").unwrap_or_else(|_| "rustc".into());
        let r = Command::new(rustc).args(["--edition", "2021", "-C", "debuginfo=0"]).arg(&src).arg("-o").arg(&mine).output().expect("rustc runs");
        assert!(r.status.success(), "building the stand-in gh: {}", String::from_utf8_lossy(&r.stderr));
        if std::fs::rename(&mine, &exe).is_ok() { exe } else { mine }
    })
}

/// A folder of a test's own, gone when it drops.
pub struct Dir(pub PathBuf);

impl Dir {
    pub fn new(name: &str) -> Dir {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let d = std::env::temp_dir().join(format!("hover-{name}-{}-{}", std::process::id(), N.fetch_add(1, std::sync::atomic::Ordering::SeqCst)));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }
    pub fn path(&self) -> &Path { &self.0 }
    pub fn join(&self, p: &str) -> PathBuf { self.0.join(p) }
    pub fn s(&self) -> String { self.0.to_string_lossy().into_owned() }
}

impl Drop for Dir {
    fn drop(&mut self) {
        // A program that was just stopped can hold its folder for a moment (Windows).
        for _ in 0..20 {
            if std::fs::remove_dir_all(&self.0).is_ok() || !self.0.exists() { return; }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

/// A stand-in gh with its scripts, in a folder of its own.
pub struct Fake {
    pub dir: Dir,
}

impl Fake {
    pub fn new() -> Fake {
        let dir = Dir::new("fakegh");
        std::fs::create_dir_all(dir.join("script")).unwrap();
        std::fs::copy(built(), dir.join(&format!("gh{EXE}"))).unwrap();
        Fake { dir }
    }

    pub fn gh(&self) -> PathBuf { self.dir.join(&format!("gh{EXE}")) }

    /// The same stand-in under another name (winget).
    pub fn also(&self, name: &str) -> PathBuf {
        let p = self.dir.join(&format!("{name}{EXE}"));
        std::fs::copy(built(), &p).unwrap();
        p
    }

    /// What `gh <key words>` does: the script's lines.
    pub fn script(&self, key: &str, lines: &[&str]) {
        std::fs::write(self.dir.join(&format!("script/{key}.txt")), lines.join("\n")).unwrap();
    }

    /// Each call so far, as its arguments.
    pub fn calls(&self) -> Vec<Vec<String>> {
        std::fs::read_to_string(self.dir.join("calls.log")).unwrap_or_default().lines().map(|l| l.split('\u{1f}').map(str::to_owned).collect()).collect()
    }

    /// What a call sent on stdin (the script's `readall`).
    pub fn stdin_of(&self, key: &str) -> Option<String> {
        std::fs::read_to_string(self.dir.join(&format!("stdin.{key}.txt"))).ok()
    }

    /// GitHubCli using it, with git kept away from the user's configuration and given
    /// a name to commit as.
    pub fn cli(&self) -> Arc<GitHubCli> { Arc::new(GitHubCli::new().with(Some(self.gh()), git_env(self.dir.path()))) }

    pub fn desk(&self) -> Desk { Desk::new(self.cli(), hover_agents::desk::find_git()) }
}

pub fn git_env(dir: &Path) -> Vec<(String, String)> {
    let config = dir.join("gitconfig");
    if !config.exists() { std::fs::write(&config, "").unwrap(); }
    [("GIT_CONFIG_GLOBAL", config.to_string_lossy().into_owned()), ("GIT_CONFIG_NOSYSTEM", "1".into()),
        ("GIT_AUTHOR_NAME", "Test".into()), ("GIT_AUTHOR_EMAIL", "t@example.com".into()),
        ("GIT_COMMITTER_NAME", "Test".into()), ("GIT_COMMITTER_EMAIL", "t@example.com".into())]
        .into_iter().map(|(k, v)| (k.to_owned(), v)).collect()
}

pub fn git_available() -> bool { hover_agents::desk::find_git().is_some() }

/// Runs git in a folder as a test setup, with the same isolation as `git_env`.
pub fn git(dir: &Path, args: &[&str]) -> String {
    let mut c = Command::new(hover_agents::desk::find_git().expect("git"));
    c.args(args).current_dir(dir);
    for (k, v) in git_env(dir.parent().unwrap_or(dir)) { c.env(k, v); }
    let r = c.output().unwrap();
    assert!(r.status.success(), "git {args:?}: {}", String::from_utf8_lossy(&r.stderr));
    String::from_utf8_lossy(&r.stdout).into_owned()
}

/// A repository with one commit on main, and a bare remote called origin that has it.
pub struct Repo {
    pub root: Dir,
    pub repo: PathBuf,
    pub remote: PathBuf,
}

impl Repo {
    pub fn new(name: &str) -> Repo {
        let root = Dir::new(name);
        let (repo, remote) = (root.join("repo"), root.join("remote.git"));
        std::fs::create_dir_all(&repo).unwrap();
        std::fs::create_dir_all(&remote).unwrap();
        git(&remote, &["init", "-q", "--bare", "-b", "main"]);
        git(&repo, &["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("a.txt"), "one\n").unwrap();
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "first"]);
        git(&repo, &["remote", "add", "origin", &remote.to_string_lossy()]);
        git(&repo, &["push", "-q", "-u", "origin", "main"]);
        Repo { root, repo, remote }
    }

    pub fn folder(&self) -> String { self.repo.to_string_lossy().into_owned() }

    pub fn snap(&self) -> Snap {
        Snap { folder: self.folder(), texts: vec!["Change a".into(), "Made a two.".into()], ..Default::default() }
    }
}

/// Waits for a condition, up to ten seconds.
pub fn wait_for(what: &str, f: impl Fn() -> bool) {
    let t = std::time::Instant::now();
    while !f() {
        assert!(t.elapsed() < std::time::Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

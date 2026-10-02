//! Starting a tool as a hidden child (Quota.Hidden, AcpHost.Launch) and making sure
//! it and everything it starts goes with Hover (ChildJob). Windows: a job object per
//! tool, set to kill on close, so a killed or crashed Hover leaves none behind.
//! Linux: the tool leads a process group of its own (setsid), dies with Hover
//! (PR_SET_PDEATHSIG), and a small watchdog shell kills the whole group when
//! Hover's end of its pipe closes, however Hover ended.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};

/// The pipes to a running agent, and how to end it (AcpLink).
pub struct Link {
    pub to_agent: Box<dyn Write + Send>,
    pub from_agent: Box<dyn Read + Send>,
    pub kill: Box<dyn Fn() + Send + Sync>,
    /// The end of what the agent printed on stderr, to say why it gave up.
    pub errors: Box<dyn Fn() -> String + Send + Sync>,
}

/// Escape codes out of a tool's output: `\x1B\[[0-9;?]*[A-Za-z]` and `\x1B\][^\x07]*\x07`.
pub fn strip_ansi(text: &str) -> String {
    let b: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == '\u{1b}' && i + 1 < b.len() {
            if b[i + 1] == '[' {
                let mut j = i + 2;
                while j < b.len() && (b[j].is_ascii_digit() || b[j] == ';' || b[j] == '?') { j += 1; }
                if j < b.len() && b[j].is_ascii_alphabetic() { i = j + 1; continue; }
            } else if b[i + 1] == ']' {
                if let Some(k) = b[i + 2..].iter().position(|&c| c == '\u{7}') { i = i + 2 + k + 1; continue; }
            }
        }
        out.push(b[i]);
        i += 1;
    }
    out
}

/// Quota.OnPath: the first match on PATH, with Windows' executable suffixes there.
pub fn on_path(name: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT").unwrap_or_else(|_| ".EXE;.CMD;.BAT".into()).split(';').filter(|e| !e.is_empty()).map(str::to_owned).collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path) {
        let d = dir.to_string_lossy();
        let d = d.trim_matches('"');
        if d.is_empty() { continue; }
        for ext in &exts {
            let p = Path::new(d).join(format!("{name}{ext}"));
            if p.is_file() { return Some(p); }
        }
    }
    None
}

/// The user's home: where tools are started (Environment.SpecialFolder.UserProfile).
pub fn home() -> PathBuf {
    let v = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
    std::env::var_os(v).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

/// Quota.Hidden: no console window, all three pipes, no colour codes. A .cmd or .bat
/// shim goes through cmd.
pub fn hidden(exe: &Path, args: &[&str]) -> Command {
    let shim = exe.extension().is_some_and(|e| e.eq_ignore_ascii_case("cmd") || e.eq_ignore_ascii_case("bat"));
    let mut c = if shim {
        let mut c = Command::new("cmd.exe");
        c.args(["/d", "/c"]).arg(exe);
        c
    } else {
        Command::new(exe)
    };
    c.args(args).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).env("NO_COLOR", "1").env("TERM", "dumb");
    #[cfg(windows)]
    { use std::os::windows::process::CommandExt; c.creation_flags(0x0800_0000 /* CREATE_NO_WINDOW */); }
    c
}

/// A started tool, grouped so that killing it kills what it started.
pub struct Group {
    child: Mutex<Option<Child>>,
    imp: imp::Group,
}

impl Group {
    pub fn spawn(mut cmd: Command) -> std::io::Result<Group> {
        imp::prepare(&mut cmd);
        let child = imp::spawn(cmd)?;
        let imp = imp::Group::attach(&child)?;
        Ok(Group { child: Mutex::new(Some(child)), imp })
    }

    pub fn pid(&self) -> Option<u32> { self.child.lock().unwrap().as_ref().map(Child::id) }

    pub fn take_pipes(&self) -> (Option<std::process::ChildStdin>, Option<std::process::ChildStdout>, Option<std::process::ChildStderr>) {
        let mut g = self.child.lock().unwrap();
        let c = g.as_mut().unwrap();
        (c.stdin.take(), c.stdout.take(), c.stderr.take())
    }

    /// Ends the tool and everything in its group; the child is reaped off this thread.
    pub fn kill(&self) {
        self.imp.kill();
        if let Some(mut c) = self.child.lock().unwrap().take() {
            let _ = c.kill();
            std::thread::spawn(move || { let _ = c.wait(); });
        }
    }

    /// Waits for the tool to exit, up to a limit; its code, or None on timeout.
    pub fn wait_timeout(&self, limit: std::time::Duration) -> Option<i32> {
        let t = std::time::Instant::now();
        loop {
            if let Some(c) = self.child.lock().unwrap().as_mut() {
                if let Ok(Some(s)) = c.try_wait() { return Some(s.code().unwrap_or(-1)); }
            } else {
                return Some(-1);
            }
            if t.elapsed() >= limit { return None; }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
    }
}

impl Drop for Group {
    fn drop(&mut self) { self.kill(); }
}

/// AcpHost.Launch: the tool running as an ACP server, its stderr's last 8 KB kept.
pub fn launch(exe: &Path, args: &[&str], env: &[(String, String)]) -> std::io::Result<Link> { launch_grouped(exe, args, env).map(|(l, _)| l) }

/// launch, with the group handed back too (a test kills the tool from outside).
pub fn launch_grouped(exe: &Path, args: &[&str], env: &[(String, String)]) -> std::io::Result<(Link, Arc<Group>)> { launch_at(exe, args, env, &home()) }

/// launch, started in a folder of its own: Claude Code takes its project from the folder
/// it starts in (it has no --cwd).
pub fn launch_in(exe: &Path, args: &[&str], env: &[(String, String)], dir: &Path) -> std::io::Result<Link> { launch_at(exe, args, env, dir).map(|(l, _)| l) }

fn launch_at(exe: &Path, args: &[&str], env: &[(String, String)], dir: &Path) -> std::io::Result<(Link, Arc<Group>)> {
    let mut cmd = hidden(exe, args);
    cmd.current_dir(dir);
    for (k, v) in env { cmd.env(k, v); }
    let group = Arc::new(Group::spawn(cmd)?);
    let (stdin, stdout, stderr) = group.take_pipes();
    let tail = Arc::new(Mutex::new(String::new()));
    if let Some(mut err) = stderr {
        let tail = tail.clone();
        std::thread::Builder::new().name("acp-stderr".into()).spawn(move || {
            let mut buf = [0u8; 2048];
            while let Ok(n) = err.read(&mut buf) {
                if n == 0 { break; }
                let mut t = tail.lock().unwrap();
                t.push_str(&String::from_utf8_lossy(&buf[..n]));
                let over = crate::stream::units(&t).saturating_sub(8192);
                if over > 0 {
                    let cut = t.char_indices().scan(0usize, |u, (i, c)| { *u += c.len_utf16(); Some((i + c.len_utf8(), *u)) })
                        .find(|&(_, u)| u >= over).map_or(0, |(i, _)| i);
                    t.drain(..cut);
                }
            }
        })?;
    }
    let g = group.clone();
    Ok((Link {
        to_agent: Box::new(stdin.expect("piped stdin")),
        from_agent: Box::new(stdout.expect("piped stdout")),
        kill: Box::new(move || g.kill()),
        errors: Box::new(move || tail.lock().unwrap().clone()),
    }, group))
}

#[cfg(unix)]
mod imp {
    use std::os::unix::process::CommandExt;
    use std::process::{Child, ChildStdin, Command, Stdio};
    use std::sync::Mutex;

    type Job = (Command, std::sync::mpsc::Sender<std::io::Result<Child>>);

    /// PR_SET_PDEATHSIG fires when the thread that forked ends, not the process: a tool
    /// started from a turn's thread died with that turn. So every child is forked from
    /// one thread that lives as long as Hover.
    pub fn spawn(cmd: Command) -> std::io::Result<Child> {
        static SPAWNER: std::sync::OnceLock<Mutex<std::sync::mpsc::Sender<Job>>> = std::sync::OnceLock::new();
        let tx = SPAWNER.get_or_init(|| {
            let (tx, rx) = std::sync::mpsc::channel::<Job>();
            std::thread::Builder::new().name("child-spawner".into()).spawn(move || {
                for (mut c, back) in rx { let _ = back.send(c.spawn()); }
            }).expect("a thread for starting tools");
            Mutex::new(tx)
        });
        let (back, got) = std::sync::mpsc::channel();
        tx.lock().unwrap().send((cmd, back)).map_err(|_| std::io::Error::other("the spawner is gone"))?;
        got.recv().map_err(|_| std::io::Error::other("the spawner is gone"))?
    }

    pub fn prepare(cmd: &mut Command) {
        let parent = unsafe { libc::getpid() };
        unsafe {
            cmd.pre_exec(move || {
                // A group of its own: a kill of the group reaches whatever the tool starts,
                // and Ctrl+C in Hover's terminal doesn't.
                if libc::setsid() < 0 { return Err(std::io::Error::last_os_error()); }
                libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL);
                // Hover died between the fork and the prctl.
                if libc::getppid() != parent { libc::_exit(1); }
                Ok(())
            });
        }
    }

    pub struct Group { pgid: i32, watchdog: Mutex<Option<(Child, ChildStdin)>> }

    impl Group {
        pub fn attach(child: &Child) -> std::io::Result<Group> {
            let pgid = child.id() as i32;
            // Reads until Hover's end closes (Hover exited, however), then kills the
            // group. In a session of its own too, so a terminal's signals miss it.
            let mut w = Command::new("/bin/sh");
            w.args(["-c", "read _; kill -KILL -- -\"$0\" 2>/dev/null", &pgid.to_string()]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null());
            unsafe { w.pre_exec(|| { libc::setsid(); Ok(()) }); }
            let watchdog = match w.spawn() {
                Ok(mut c) => { let i = c.stdin.take().unwrap(); Some((c, i)) }
                Err(e) => { hover_core::log::line(&format!("child group: no watchdog - {e}")); None }
            };
            Ok(Group { pgid, watchdog: Mutex::new(watchdog) })
        }

        pub fn kill(&self) {
            // The watchdog first, so its kill can never land on a reused group id.
            if let Some((mut c, i)) = self.watchdog.lock().unwrap().take() {
                let _ = c.kill();
                drop(i);
                std::thread::spawn(move || { let _ = c.wait(); });
            }
            unsafe { libc::kill(-self.pgid, libc::SIGKILL); }
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::os::windows::io::AsRawHandle;
    use std::process::{Child, Command};
    use windows::Win32::Foundation::{CloseHandle, HANDLE};
    use windows::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    pub fn prepare(_: &mut Command) {}

    pub fn spawn(mut cmd: Command) -> std::io::Result<Child> { cmd.spawn() }

    /// One job per tool, killing everything in it when its last handle closes: when
    /// Hover exits, however it exits (ChildJob, which C# keeps as one job for all).
    pub struct Group { job: isize }

    unsafe impl Send for Group {}
    unsafe impl Sync for Group {}

    impl Group {
        pub fn attach(child: &Child) -> std::io::Result<Group> {
            unsafe {
                let job = CreateJobObjectW(None, None).map_err(std::io::Error::other)?;
                let mut info = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
                info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
                SetInformationJobObject(job, JobObjectExtendedLimitInformation, &info as *const _ as *const _, std::mem::size_of_val(&info) as u32)
                    .map_err(std::io::Error::other)?;
                if let Err(e) = AssignProcessToJobObject(job, HANDLE(child.as_raw_handle())) {
                    hover_core::log::line(&format!("child job: couldn't add pid {} ({e})", child.id()));
                }
                Ok(Group { job: job.0 as isize })
            }
        }

        pub fn kill(&self) {
            unsafe { let _ = TerminateJobObject(HANDLE(self.job as *mut _), 1); }
        }
    }

    impl Drop for Group {
        fn drop(&mut self) { unsafe { let _ = CloseHandle(HANDLE(self.job as *mut _)); } }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escape_codes_go_as_the_regex_takes_them() {
        assert_eq!(strip_ansi("\u{1b}[31merror:\u{1b}[0m x\u{1b}[?25l ▰\u{1b}]0;title\u{7}!"), "error: x ▰!");
        assert_eq!(strip_ansi("\u{1b}[31"), "\u{1b}[31");
    }

    #[cfg(unix)]
    #[test]
    fn a_killed_group_takes_what_it_started() {
        let mut c = hidden(Path::new("/bin/sh"), &["-c", "sleep 300 & echo $!; wait"]);
        c.stdin(Stdio::null());
        let g = Group::spawn(c).unwrap();
        let (_, out, _) = g.take_pipes();
        let mut line = String::new();
        std::io::BufRead::read_line(&mut std::io::BufReader::new(out.unwrap()), &mut line).unwrap();
        let grandchild: i32 = line.trim().parse().unwrap();
        assert_eq!(unsafe { libc::kill(grandchild, 0) }, 0, "the grandchild runs");
        g.kill();
        let t = std::time::Instant::now();
        while unsafe { libc::kill(grandchild, 0) } == 0 && t.elapsed().as_secs() < 5 { std::thread::sleep(std::time::Duration::from_millis(20)); }
        // Killed, and reaped by init (it was re-parented), or a zombie at worst.
        let state = std::fs::read_to_string(format!("/proc/{grandchild}/stat")).unwrap_or_default();
        assert!(state.is_empty() || state.contains(") Z"), "{state}");
    }
}

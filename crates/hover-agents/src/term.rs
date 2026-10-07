//! The Terminal panel's "My commands" tab: the user's own shell, in the session's folder,
//! run as the user (no sandbox: it is theirs, not an agent's).
//!
//! One long-lived shell per chat, so `cd` and variables stay from one command to the next:
//! PowerShell on Windows (`powershell.exe -NoLogo -NoProfile -NonInteractive -Command -`, which
//! reads a command from its input as each line arrives and runs it), bash on Linux and macOS.
//! Started through `proc::Group`, so it and whatever it starts end with Hover.
//!
//! A command goes to the shell's stdin as data, never on a command line and never pasted into
//! the shell's own syntax: the line sent is a fixed wrapper around the command in base64, which
//! the shell decodes and runs (`Invoke-Expression` / `eval`, in the shell's own scope). So a
//! quote or a half-typed string in a command can't break the wrapper. After the command the
//! wrapper prints a marker with the exit code and the folder the shell is in; `Framer` finds it.
//!
//! Ctrl+C ends the running command by ending the shell and starting a new one, in the folder the
//! last command left it in. (Variables set in the old shell go with it.) A command that waits for
//! typed input is not supported: its input is nothing.

use crate::proc::{self, Group};
use std::io::{Read, Write};
use std::process::ChildStdin;
use std::sync::{Arc, Mutex};
use std::time::Instant;

/// Output kept for a command, and commands kept.
const LINES_KEPT: usize = 2000;
const ENTRIES_KEPT: usize = 200;
const HISTORY_KEPT: usize = 200;

#[derive(Clone, Debug, PartialEq)]
pub struct Line { pub text: String, pub err: bool }

#[derive(Clone, Debug, PartialEq)]
pub enum Run {
    Running(Instant),
    /// Ended on its own: its exit code and how long it took (ms).
    Done(i32, u64),
    /// Ended by Ctrl+C.
    Stopped(u64),
}

/// A command the user ran: where, what, what it printed, how it ended.
#[derive(Clone, Debug)]
pub struct Entry { pub cwd: String, pub cmd: String, pub lines: Vec<Line>, pub run: Run, pub cut: usize }

/// What the shell printed, cut into events.
#[derive(Debug, PartialEq)]
pub enum Ev { Line(String), Done(i32, String) }

/// Finds the end-of-command marker in the shell's output. The marker is `<mark>|<code>|<folder>`
/// and a newline; it may arrive in pieces and may follow output that had no newline.
pub struct Framer { mark: String, buf: String }

impl Framer {
    pub fn new(mark: &str) -> Framer { Framer { mark: mark.to_owned(), buf: String::new() } }

    pub fn feed(&mut self, chunk: &str) -> Vec<Ev> {
        self.buf.push_str(chunk);
        let mut out = vec![];
        loop {
            if let Some(at) = self.buf.find(&self.mark) {
                // The record is complete once its line ends.
                let rest = &self.buf[at + self.mark.len()..];
                let Some(nl) = rest.find('\n') else { break };
                let record = rest[..nl].trim_end_matches('\r').to_owned();
                let before = self.buf[..at].to_owned();
                self.buf.drain(..at + self.mark.len() + nl + 1);
                for l in before.split('\n') {
                    let l = l.trim_end_matches('\r');
                    // The last piece is the start of the marker's own line (empty) or output with no newline.
                    out.push(Ev::Line(l.to_owned()));
                }
                // `before` ended at the marker: a trailing empty piece is not a line of output.
                if let Some(Ev::Line(l)) = out.last() { if l.is_empty() { out.pop(); } }
                let mut parts = record.splitn(3, '|');
                let _ = parts.next();
                let code = parts.next().and_then(|c| c.trim().parse().ok()).unwrap_or(1);
                out.push(Ev::Done(code, parts.next().unwrap_or("").to_owned()));
                continue;
            }
            // No marker (yet): whole lines are output. A partial line stays, since the marker may be in it.
            let Some(nl) = self.buf.rfind('\n') else { break };
            let keep = self.buf.split_off(nl + 1);
            let done = std::mem::replace(&mut self.buf, keep);
            for l in done[..done.len() - 1].split('\n') { out.push(Ev::Line(l.trim_end_matches('\r').to_owned())); }
            break;
        }
        out
    }
}

/// A marker no command has printed by accident: it has this shell's start in it.
fn new_mark() -> String {
    let n = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
    format!("@@hover-{:x}-{:x}@@", std::process::id(), n)
}

/// The line that runs `cmd` in the shell and then prints the marker.
pub fn wrapper(cmd: &str, mark: &str, windows: bool) -> String {
    let b64 = crate::http::base64(cmd.as_bytes());
    if windows {
        // $? after Invoke-Expression; a native program's own code wins when it set one.
        format!("$global:LASTEXITCODE=$null; $hvok=$true; try {{ Invoke-Expression ([Text.Encoding]::UTF8.GetString([Convert]::FromBase64String('{b64}'))); $hvok=$? }} catch {{ $hvok=$false; [Console]::Error.WriteLine($_.Exception.Message) }}; \
Write-Output ('{mark}|' + $(if ($null -ne $global:LASTEXITCODE) {{ $global:LASTEXITCODE }} elseif ($hvok) {{ 0 }} else {{ 1 }}) + '|' + (Get-Location).Path)\n")
    } else {
        // The command gets no input: it must not read the lines meant for the shell.
        format!("hvc=$(printf %s '{b64}' | base64 -d); eval \"$hvc\" </dev/null; hvs=$?; printf '%s|%s|%s\\n' '{mark}' \"$hvs\" \"$PWD\"\n")
    }
}

/// A path cut to about `max` characters by taking out middle folders: `C:\Users\…\Hover`.
pub fn shorten(path: &str, max: usize) -> String {
    if path.chars().count() <= max { return path.to_owned(); }
    let sep = if path.contains('\\') { '\\' } else { '/' };
    let parts: Vec<&str> = path.split(sep).filter(|p| !p.is_empty()).collect();
    if parts.len() < 3 { return path.to_owned(); }
    let lead = if path.starts_with(sep) { sep.to_string() } else { String::new() };
    format!("{lead}{}{sep}…{sep}{}", parts[0], parts[parts.len() - 1])
}

/// The prompt the shell shows: `PS C:\…\project> ` or `user@host:~/project$ `.
pub fn prompt(cwd: &str) -> String {
    if cfg!(windows) { return format!("PS {}> ", shorten(cwd, 44)); }
    let home = proc::home().to_string_lossy().into_owned();
    let shown = match cwd.strip_prefix(home.trim_end_matches('/')) { Some(r) if r.is_empty() || r.starts_with('/') => format!("~{r}"), _ => cwd.to_owned() };
    let user = std::env::var("USER").or_else(|_| std::env::var("USERNAME")).unwrap_or_else(|_| "user".into());
    let host = std::fs::read_to_string("/etc/hostname").ok().map(|h| h.trim().to_owned()).filter(|h| !h.is_empty())
        .or_else(|| std::env::var("HOSTNAME").ok()).unwrap_or_else(|| "localhost".into());
    format!("{user}@{host}:{}$ ", shorten(&shown, 40))
}

/// The dim line at the top: the shell, and that it runs as the user.
pub fn banner() -> &'static str {
    if cfg!(windows) { "PowerShell · runs as you (Windows has no sandbox)" }
    else { "bash · runs as you (your own shell, outside the agents’ sandbox)" }
}

struct Shell { _group: Group, stdin: ChildStdin, mark: String }

struct Inner {
    folder: String,
    cwd: String,
    entries: Vec<Entry>,
    history: Vec<String>,
    /// Which shell is current; a shell that was ended by Ctrl+C has an older number, and what it
    /// still prints is dropped.
    gen: u64,
    shell: Option<Shell>,
    /// Where the history walk (↑/↓) is: the number of steps back, 0 at the prompt.
    back: usize,
}

/// The user's shell for one chat.
pub struct Term {
    inner: Arc<Mutex<Inner>>,
    /// Called from the shell's threads when something changed.
    notify: Arc<dyn Fn() + Send + Sync>,
}

impl Term {
    pub fn new(folder: &str, notify: impl Fn() + Send + Sync + 'static) -> Term {
        let inner = Inner { folder: folder.to_owned(), cwd: folder.to_owned(), entries: vec![], history: vec![], gen: 0, shell: None, back: 0 };
        Term { inner: Arc::new(Mutex::new(inner)), notify: Arc::new(notify) }
    }

    /// Reads the entries, the folder the shell is in, and whether a command runs.
    pub fn view<T>(&self, f: impl FnOnce(&[Entry], &str, bool) -> T) -> T {
        let g = self.inner.lock().unwrap();
        let running = g.entries.last().is_some_and(|e| matches!(e.run, Run::Running(_)));
        f(&g.entries, &g.cwd, running)
    }

    pub fn running(&self) -> bool { self.view(|_, _, r| r) }

    /// Runs a command. Ignored when empty or when one runs already. `clear`, `cls` and `Clear-Host` empty the screen.
    pub fn run(&self, cmd: &str) {
        let cmd = cmd.trim();
        if cmd.is_empty() { return; }
        let mut g = self.inner.lock().unwrap();
        if g.entries.last().is_some_and(|e| matches!(e.run, Run::Running(_))) { return; }
        if g.history.last().map(String::as_str) != Some(cmd) { g.history.push(cmd.to_owned()); let n = g.history.len(); if n > HISTORY_KEPT { g.history.drain(..n - HISTORY_KEPT); } }
        g.back = 0;
        if matches!(cmd.to_ascii_lowercase().as_str(), "clear" | "cls" | "clear-host") { g.entries.clear(); drop(g); (self.notify)(); return; }
        let cwd = g.cwd.clone();
        g.entries.push(Entry { cwd, cmd: cmd.to_owned(), lines: vec![], run: Run::Running(Instant::now()), cut: 0 });
        let n = g.entries.len();
        if n > ENTRIES_KEPT { g.entries.drain(..n - ENTRIES_KEPT); }
        if let Err(e) = self.ensure(&mut g) { finish(&mut g, 1, Some(format!("Couldn’t start the shell: {e}"))); drop(g); (self.notify)(); return; }
        let sh = g.shell.as_mut().unwrap();
        let line = wrapper(cmd, &sh.mark, cfg!(windows));
        if let Err(e) = sh.stdin.write_all(line.as_bytes()).and_then(|_| sh.stdin.flush()) {
            g.shell = None;
            finish(&mut g, 1, Some(format!("The shell isn’t taking commands: {e}")));
        }
        drop(g);
        (self.notify)();
    }

    /// Ctrl+C: ends the running command (and the shell with it; the next command starts a new one in the same folder).
    pub fn interrupt(&self) {
        let mut g = self.inner.lock().unwrap();
        let Some(e) = g.entries.last_mut() else { return };
        let Run::Running(t) = e.run else { return };
        e.run = Run::Stopped(t.elapsed().as_millis() as u64);
        e.lines.push(Line { text: "^C".into(), err: false });
        g.gen += 1;
        g.shell = None;
        drop(g);
        (self.notify)();
    }

    /// The screenshots: these commands show, without a shell behind them.
    pub fn seed(&self, entries: Vec<Entry>) { self.inner.lock().unwrap().entries = entries; }

    pub fn clear(&self) {
        let mut g = self.inner.lock().unwrap();
        if g.entries.last().is_some_and(|e| matches!(e.run, Run::Running(_))) { return; }
        g.entries.clear();
        drop(g);
        (self.notify)();
    }

    /// ↑ (`dir` -1) and ↓ (+1) through the commands run, newest first. The text to put in the box.
    pub fn history(&self, dir: i32) -> String {
        let mut g = self.inner.lock().unwrap();
        let n = g.history.len();
        g.back = if dir < 0 { (g.back + 1).min(n) } else { g.back.saturating_sub(1) };
        if g.back == 0 { String::new() } else { g.history[n - g.back].clone() }
    }

    /// Starts the shell if there is none.
    fn ensure(&self, g: &mut Inner) -> std::io::Result<()> {
        if g.shell.is_some() { return Ok(()); }
        if !crate::usable_folder(Some(&g.cwd)) { g.cwd = g.folder.clone(); }
        let mut cmd = if cfg!(windows) {
            proc::hidden(std::path::Path::new("powershell.exe"), &["-NoLogo", "-NoProfile", "-NonInteractive", "-Command", "-"])
        } else {
            proc::hidden(std::path::Path::new("/bin/bash"), &["--norc", "--noprofile"])
        };
        cmd.current_dir(&g.cwd);
        // Colour codes are for a terminal; this one is drawn by Hover.
        cmd.env("NO_COLOR", "1").env("TERM", "dumb");
        let group = Group::spawn(cmd)?;
        let (stdin, stdout, stderr) = group.take_pipes();
        let (mut stdin, stdout, stderr) = (stdin.ok_or_else(|| std::io::Error::other("no input"))?, stdout.ok_or_else(|| std::io::Error::other("no output"))?, stderr);
        if cfg!(windows) {
            // Output in UTF-8, and no progress bars in it.
            stdin.write_all(b"[Console]::OutputEncoding=[Text.Encoding]::UTF8; $OutputEncoding=[Text.Encoding]::UTF8; $ProgressPreference='SilentlyContinue'\n")?;
            stdin.flush()?;
        }
        let mark = new_mark();
        let gen = g.gen;
        self.read_out(stdout, gen, mark.clone());
        if let Some(e) = stderr { self.read_err(e, gen); }
        g.shell = Some(Shell { _group: group, stdin, mark });
        Ok(())
    }

    fn read_out(&self, mut out: impl Read + Send + 'static, gen: u64, mark: String) {
        let (inner, notify) = (self.inner.clone(), self.notify.clone());
        std::thread::Builder::new().name("term-out".into()).spawn(move || {
            let mut framer = Framer::new(&mark);
            let mut buf = [0u8; 4096];
            let mut pending: Vec<u8> = vec![];
            loop {
                let n = match out.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
                pending.extend_from_slice(&buf[..n]);
                // A character cut between two reads waits for its other half.
                let keep = match std::str::from_utf8(&pending) { Ok(_) => 0, Err(e) if e.error_len().is_none() => pending.len() - e.valid_up_to(), Err(_) => 0 };
                let text = String::from_utf8_lossy(&pending[..pending.len() - keep]).into_owned();
                pending.drain(..pending.len() - keep);
                let evs = framer.feed(&text);
                if evs.is_empty() { continue; }
                let mut g = inner.lock().unwrap();
                if g.gen != gen { return; }
                for ev in evs {
                    match ev {
                        Ev::Line(l) => push_line(&mut g, l, false),
                        Ev::Done(code, cwd) => { if !cwd.is_empty() { g.cwd = cwd; } finish(&mut g, code, None); }
                    }
                }
                drop(g);
                notify();
            }
            // The shell's output ended: it is gone.
            let mut g = inner.lock().unwrap();
            if g.gen != gen { return; }
            g.shell = None;
            if g.entries.last().is_some_and(|e| matches!(e.run, Run::Running(_))) { finish(&mut g, 1, Some("The shell ended.".into())); }
            drop(g);
            notify();
        }).ok();
    }

    fn read_err(&self, mut err: impl Read + Send + 'static, gen: u64) {
        let (inner, notify) = (self.inner.clone(), self.notify.clone());
        std::thread::Builder::new().name("term-err".into()).spawn(move || {
            let mut buf = [0u8; 4096];
            let mut carry = String::new();
            loop {
                let n = match err.read(&mut buf) { Ok(0) | Err(_) => break, Ok(n) => n };
                carry.push_str(&String::from_utf8_lossy(&buf[..n]));
                let Some(nl) = carry.rfind('\n') else { continue };
                let rest = carry.split_off(nl + 1);
                let done = std::mem::replace(&mut carry, rest);
                let mut g = inner.lock().unwrap();
                if g.gen != gen { return; }
                for l in done[..done.len() - 1].split('\n') { push_line(&mut g, l.trim_end_matches('\r').to_owned(), true); }
                drop(g);
                notify();
            }
        }).ok();
    }
}

fn push_line(g: &mut Inner, text: String, err: bool) {
    let Some(e) = g.entries.last_mut() else { return };
    if !matches!(e.run, Run::Running(_)) { return; }
    e.lines.push(Line { text, err });
    if e.lines.len() > LINES_KEPT { e.lines.remove(0); e.cut += 1; }
}

/// The running command ended with `code` (and perhaps a last word).
fn finish(g: &mut Inner, code: i32, say: Option<String>) {
    let Some(e) = g.entries.last_mut() else { return };
    let Run::Running(t) = e.run else { return };
    if let Some(s) = say { e.lines.push(Line { text: s, err: true }); }
    // The blank lines PowerShell puts around a table are not output.
    while e.lines.last().is_some_and(|l| l.text.trim().is_empty()) { e.lines.pop(); }
    let skip = e.lines.iter().take_while(|l| l.text.trim().is_empty()).count();
    e.lines.drain(..skip);
    e.run = Run::Done(code, t.elapsed().as_millis() as u64);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lines(evs: &[Ev]) -> Vec<&str> { evs.iter().filter_map(|e| if let Ev::Line(l) = e { Some(l.as_str()) } else { None }).collect() }

    /// The marker ends a command however the shell's output is cut into reads, and a marker that
    /// follows output with no newline still ends it.
    #[test]
    fn the_end_of_a_command_is_found_however_the_output_arrives() {
        let m = "@@hover-1-2@@";
        let whole = format!("one\r\ntwo\r\n{m}|3|C:\\work dir\r\n");
        for cut in 0..=whole.len() {
            if !whole.is_char_boundary(cut) { continue; }
            let mut f = Framer::new(m);
            let mut evs = f.feed(&whole[..cut]);
            evs.extend(f.feed(&whole[cut..]));
            assert_eq!(lines(&evs), ["one", "two"], "cut at {cut}");
            assert_eq!(evs.last(), Some(&Ev::Done(3, "C:\\work dir".into())), "cut at {cut}");
            assert_eq!(evs.iter().filter(|e| matches!(e, Ev::Done(..))).count(), 1);
        }
        // Output without a newline, then the marker on the same line.
        let mut f = Framer::new(m);
        let evs = f.feed(&format!("no newline{m}|0|/tmp\n"));
        assert_eq!((lines(&evs), evs.last()), (vec!["no newline"], Some(&Ev::Done(0, "/tmp".into()))));
        // Two commands in one read, and nothing left over for the next.
        let mut f = Framer::new(m);
        let evs = f.feed(&format!("a\n{m}|0|/x\nb\n{m}|1|/y\n"));
        assert_eq!(evs, [Ev::Line("a".into()), Ev::Done(0, "/x".into()), Ev::Line("b".into()), Ev::Done(1, "/y".into())]);
        assert!(f.feed("").is_empty());
    }

    /// The command is in the wrapper as base64 only, so what it contains can't reach the shell as syntax.
    #[test]
    fn a_command_is_sent_as_data_not_as_shell_text() {
        let cmd = "echo \"it's\" `x` $(y); 'unclosed";
        for windows in [true, false] {
            let line = wrapper(cmd, "@@m@@", windows);
            assert!(line.ends_with('\n') && line.matches('\n').count() == 1, "one line");
            assert!(!line.contains("unclosed") && !line.contains("it's"), "{line}");
            assert!(line.contains(&crate::http::base64(cmd.as_bytes())));
            assert!(line.contains("@@m@@"));
        }
    }

    #[test]
    fn a_long_path_loses_its_middle_folders() {
        assert_eq!(shorten("C:\\a\\b", 44), "C:\\a\\b");
        assert_eq!(shorten("C:\\Users\\james\\code\\deep\\deeper\\Hover-rust-8fb777af", 30), "C:\\…\\Hover-rust-8fb777af");
        assert_eq!(shorten("/home/james/code/deep/deeper/Hover-rust", 20), "/home/…/Hover-rust");
    }

    /// A real shell: a command's output and exit code come back, `cd` lasts to the next command, and
    /// Ctrl+C ends a command that would never end.
    #[test]
    fn the_shell_keeps_its_folder_and_can_be_stopped() {
        let dir = std::env::temp_dir().join(format!("hover-term-{}", std::process::id()));
        let sub = dir.join("sub");
        std::fs::create_dir_all(&sub).unwrap();
        let folder = dir.to_string_lossy().into_owned();
        let (tx, rx) = std::sync::mpsc::channel::<()>();
        let tx = Mutex::new(tx);
        let t = Term::new(&folder, move || { let _ = tx.lock().unwrap().send(()); });
        let wait = |until: &dyn Fn() -> bool| { for _ in 0..600 { if until() { return true; } let _ = rx.recv_timeout(std::time::Duration::from_millis(50)); } false };
        t.run(if cfg!(windows) { "echo hello; Set-Location sub" } else { "echo hello; cd sub" });
        assert!(wait(&|| !t.running()), "the command ended");
        let got = t.view(|e, cwd, _| (e.last().unwrap().lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(), e.last().unwrap().run.clone(), cwd.to_owned()));
        assert_eq!(got.0, ["hello"]);
        assert!(matches!(got.1, Run::Done(0, _)), "{:?}", got.1);
        assert!(got.2.ends_with("sub"), "the shell moved: {}", got.2);
        t.run(if cfg!(windows) { "(Get-Location).Path; cmd /c exit 7" } else { "pwd; (exit 7)" });
        assert!(wait(&|| !t.running()));
        let got = t.view(|e, _, _| (e.last().unwrap().lines.first().map(|l| l.text.clone()), e.last().unwrap().run.clone()));
        assert!(got.0.unwrap().ends_with("sub"), "it ran where cd left it");
        assert!(matches!(got.1, Run::Done(7, _)), "{:?}", got.1);
        t.run(if cfg!(windows) { "Start-Sleep 60" } else { "sleep 60" });
        assert!(wait(&|| t.running()));
        t.interrupt();
        assert!(!t.running());
        t.view(|e, _, _| assert!(matches!(e.last().unwrap().run, Run::Stopped(_)) && e.last().unwrap().lines.last().unwrap().text == "^C"));
        t.run("echo again");
        assert!(wait(&|| !t.running()));
        let got = t.view(|e, cwd, _| (e.last().unwrap().lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>(), cwd.to_owned()));
        assert_eq!(got.0, ["again"], "a new shell works after Ctrl+C");
        assert!(got.1.ends_with("sub"), "and starts where the last one was");
        drop(t);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

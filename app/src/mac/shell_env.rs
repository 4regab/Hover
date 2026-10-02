//! ShellEnvironment.swift: a Finder- or login-item-launched app gets launchd's minimal
//! PATH, so kiro-cli, codex, cursor-agent and opencode (Homebrew, npm, ~/.local/bin, nvm,
//! bun...) look missing. T3 Code's desktop app reads the variables from the user's own
//! login shell (`$SHELL -ilc`, values between markers, 5 s timeout), then launchctl's
//! PATH, merged ahead of what was inherited; Hover does the same, once, before anything
//! starts an agent, and puts the result in its own environment so every tool it runs
//! inherits it (pingdotgg/t3code packages/shared/src/shell.ts, MIT).
//!
//! The text handling (the probe's command, reading its answer, merging PATHs) is plain
//! and runs on every OS, tested; running the shell is Unix's.

use std::collections::HashMap;

/// What is asked of the shell.
pub const NAMES: [&str; 12] = [
    "PATH", "SSH_AUTH_SOCK", "HOMEBREW_PREFIX", "HOMEBREW_CELLAR", "HOMEBREW_REPOSITORY",
    "XDG_CONFIG_HOME", "XDG_DATA_HOME", "LANG", "LC_ALL", "LC_CTYPE", "NVM_DIR", "BUN_INSTALL",
];

/// The shell's time to answer; one that ignores SIGTERM must not hold Hover's start.
pub const TIMEOUT_SECS: u64 = 5;

/// The script the login shell runs: each variable between its own markers, so anything
/// an rc file prints is ignored. Names that aren't plain upper-case words are left out
/// (they go into a shell command).
pub fn command(names: &[&str]) -> String {
    names.iter().filter(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')).map(|n| {
        format!("printf '%s\\n' '__HOVER_ENV_{n}_START__'; printenv {n} || true; printf '%s\\n' '__HOVER_ENV_{n}_END__'")
    }).collect::<Vec<_>>().join("; ")
}

/// The values between the markers. An empty or missing value is left out.
pub fn extract(output: &str, names: &[&str]) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for n in names {
        let start = format!("__HOVER_ENV_{n}_START__\n");
        let end = format!("\n__HOVER_ENV_{n}_END__");
        let Some(a) = output.find(&start).map(|i| i + start.len()) else { continue };
        let Some(b) = output[a..].find(&end).map(|i| i + a) else { continue };
        let v = &output[a..b];
        if !v.is_empty() { values.insert((*n).to_owned(), v.to_owned()); }
    }
    values
}

/// PATH-like lists joined, first occurrence kept, empty entries dropped.
pub fn merge(values: &[Option<&str>]) -> String {
    let mut seen: Vec<String> = vec![];
    for v in values.iter().flatten() {
        for e in v.split(':').map(str::trim).filter(|e| !e.is_empty()) {
            if !seen.iter().any(|s| s == e) { seen.push(e.to_owned()); }
        }
    }
    seen.join(":")
}

/// The shells to try, in order, each once: $SHELL, the user record's, zsh. Only absolute
/// paths that are executable (`exists`).
pub fn candidates(shell: Option<&str>, user_shell: Option<&str>, exists: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut out: Vec<String> = vec![];
    for c in [shell, user_shell, Some("/bin/zsh")].into_iter().flatten() {
        let c = c.trim();
        if c.starts_with('/') && !out.iter().any(|o| o == c) && exists(c) { out.push(c.to_owned()); }
    }
    out
}

/// Common install locations, last, so a GUI launch still finds tools if the probe fails.
pub fn known_dirs(home: &str) -> Vec<String> {
    ["/opt/homebrew/bin", "/opt/homebrew/sbin", "/usr/local/bin"].iter().map(|s| s.to_string())
        .chain([".local/bin", ".bun/bin", ".npm-global/bin", ".cargo/bin", ".opencode/bin"].iter().map(|d| format!("{home}/{d}")))
        .chain(["/usr/bin", "/bin", "/usr/sbin", "/sbin"].iter().map(|s| s.to_string())).collect()
}

/// The environment the agents should see: `base` with the shell's values filled in where
/// `base` has none, PATH merged (shell's, launchctl's, inherited, known places), and a UTF-8
/// locale when there is none.
pub fn combine(base: &HashMap<String, String>, shell: &HashMap<String, String>, launchctl_path: Option<&str>, home: &str) -> HashMap<String, String> {
    let mut env = base.clone();
    for (k, v) in shell {
        if k != "PATH" && env.get(k).is_none_or(|x| x.is_empty()) { env.insert(k.clone(), v.clone()); }
    }
    let known = known_dirs(home).join(":");
    let path = merge(&[shell.get("PATH").map(String::as_str), launchctl_path, base.get("PATH").map(String::as_str), Some(&known)]);
    env.insert("PATH".into(), path);
    // Agents print UTF-8; without a locale some CLIs fall back to ASCII.
    if ["LANG", "LC_ALL", "LC_CTYPE"].iter().all(|k| !env.contains_key(*k)) { env.insert("LC_CTYPE".into(), "en_US.UTF-8".into()); }
    env
}

/// Runs the probe and gives the environment to use. Blocks up to a few seconds (the
/// shell's timeout, and launchctl's two); call it before the UI and the agents start.
#[cfg(unix)]
pub fn resolve(base: &HashMap<String, String>) -> HashMap<String, String> {
    let names: Vec<&str> = NAMES.to_vec();
    let user = user_shell();
    let exists = |p: &str| { use std::os::unix::fs::PermissionsExt; std::fs::metadata(p).is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0) };
    let mut from_shell = HashMap::new();
    for sh in candidates(base.get("SHELL").map(String::as_str), user.as_deref(), &exists) {
        if let Some(out) = run(&sh, &["-ilc", &command(&names)], TIMEOUT_SECS) {
            let v = extract(&out, &names);
            if !v.is_empty() { from_shell = v; break; }
        }
    }
    let launchctl = run("/bin/launchctl", &["getenv", "PATH"], 2).map(|s| s.trim().to_owned());
    let home = base.get("HOME").cloned().or_else(|| hover_core::platform::home().map(|h| h.to_string_lossy().into_owned())).unwrap_or_default();
    combine(base, &from_shell, launchctl.as_deref(), &home)
}

/// The login shell from the user record, for launches where SHELL isn't set.
#[cfg(unix)]
fn user_shell() -> Option<String> {
    let mut buf = vec![0u8; 4096];
    let mut pw: libc::passwd = unsafe { std::mem::zeroed() };
    let mut out: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe { libc::getpwuid_r(libc::getuid(), &mut pw, buf.as_mut_ptr().cast(), buf.len(), &mut out) };
    if rc != 0 || out.is_null() || pw.pw_shell.is_null() { return None; }
    Some(unsafe { std::ffi::CStr::from_ptr(pw.pw_shell) }.to_string_lossy().into_owned())
}

/// Runs with no stdin (an interactive rc that prompts gets EOF) and stderr dropped, and
/// kills it on the timeout; its output, if it finished in time.
#[cfg(unix)]
fn run(exe: &str, args: &[&str], timeout_secs: u64) -> Option<String> {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut child = Command::new(exe).args(args).stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let mut out = child.stdout.take()?;
    let reader = std::thread::spawn(move || { let mut v = vec![]; let _ = out.read_to_end(&mut v); v });
    let end = Instant::now() + Duration::from_secs(timeout_secs);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < end => std::thread::sleep(Duration::from_millis(20)),
            _ => { let _ = child.kill(); let _ = child.wait(); let _ = reader.join(); return None; }
        }
    }
    String::from_utf8(reader.join().ok()?).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_command_asks_for_each_name_between_markers() {
        let c = command(&["PATH", "NVM_DIR"]);
        assert_eq!(c, "printf '%s\\n' '__HOVER_ENV_PATH_START__'; printenv PATH || true; printf '%s\\n' '__HOVER_ENV_PATH_END__'; \
printf '%s\\n' '__HOVER_ENV_NVM_DIR_START__'; printenv NVM_DIR || true; printf '%s\\n' '__HOVER_ENV_NVM_DIR_END__'");
        // Nothing but plain upper-case words reaches the shell.
        assert_eq!(command(&["PATH; rm -rf ~", "lower", "", "OK_1"]), command(&["OK_1"]));
    }

    #[test]
    fn values_are_read_from_between_their_markers_and_noise_is_ignored() {
        let out = "Last login: Mon\nmotd from .zshrc\n__HOVER_ENV_PATH_START__\n/opt/homebrew/bin:/usr/bin\n__HOVER_ENV_PATH_END__\n\
__HOVER_ENV_LANG_START__\n\n__HOVER_ENV_LANG_END__\n__HOVER_ENV_NVM_DIR_START__\n/Users/me/.nvm\n__HOVER_ENV_NVM_DIR_END__\n";
        let v = extract(out, &["PATH", "LANG", "NVM_DIR", "BUN_INSTALL"]);
        assert_eq!(v.get("PATH").map(String::as_str), Some("/opt/homebrew/bin:/usr/bin"));
        assert_eq!(v.get("NVM_DIR").map(String::as_str), Some("/Users/me/.nvm"));
        assert!(!v.contains_key("LANG"), "an empty value is left out");
        assert!(!v.contains_key("BUN_INSTALL"), "a missing one too");
        // A start without its end gives nothing.
        assert!(extract("__HOVER_ENV_PATH_START__\n/a\n", &["PATH"]).is_empty());
    }

    #[test]
    fn paths_merge_in_order_without_repeats() {
        assert_eq!(merge(&[Some("/a:/b"), None, Some("/b: /c :"), Some("")]), "/a:/b:/c");
    }

    #[test]
    fn shells_are_tried_once_each_and_only_if_they_run() {
        let exists = |p: &str| p != "/bin/gone";
        assert_eq!(candidates(Some("/bin/zsh"), Some("/bin/bash"), &exists), ["/bin/zsh", "/bin/bash"]);
        assert_eq!(candidates(Some("/bin/gone"), Some("zsh"), &exists), ["/bin/zsh"]);
        assert_eq!(candidates(None, None, &|_| false), Vec::<String>::new());
    }

    #[test]
    fn the_agents_environment_puts_the_shells_path_first_and_keeps_what_was_set() {
        let base: HashMap<String, String> = [("PATH", "/usr/bin:/bin"), ("LANG", "fr_FR.UTF-8"), ("HOME", "/Users/me")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let shell: HashMap<String, String> = [("PATH", "/Users/me/.nvm/bin:/opt/homebrew/bin"), ("LANG", "en_US.UTF-8"), ("NVM_DIR", "/Users/me/.nvm")].iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        let env = combine(&base, &shell, Some("/usr/local/bin:/usr/bin"), "/Users/me");
        assert!(env["PATH"].starts_with("/Users/me/.nvm/bin:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:"), "{}", env["PATH"]);
        assert!(env["PATH"].contains(":/Users/me/.local/bin:") && env["PATH"].ends_with(":/sbin"));
        assert_eq!(env["LANG"], "fr_FR.UTF-8", "a value the app was launched with wins");
        assert_eq!(env["NVM_DIR"], "/Users/me/.nvm");
        assert!(!env.contains_key("LC_CTYPE"));
        // With no locale at all, UTF-8.
        let bare: HashMap<String, String> = [("PATH".to_string(), "/usr/bin".to_string())].into();
        assert_eq!(combine(&bare, &HashMap::new(), None, "/h")["LC_CTYPE"], "en_US.UTF-8");
    }

    /// Runs a real shell where there is one: the probe, end to end.
    #[cfg(unix)]
    #[test]
    fn a_real_shell_answers_the_probe() {
        let script = format!("HOVER_PROBE_TEST=hello; export HOVER_PROBE_TEST; {}", command(&["HOVER_PROBE_TEST", "HOVER_PROBE_NONE"]));
        let out = run("/bin/sh", &["-c", &script], 5).expect("sh answers");
        let v = extract(&out, &["HOVER_PROBE_TEST", "HOVER_PROBE_NONE"]);
        assert_eq!(v.get("HOVER_PROBE_TEST").map(String::as_str), Some("hello"));
        assert!(!v.contains_key("HOVER_PROBE_NONE"));
        assert!(run("/bin/sleep", &["30"], 1).is_none(), "a program that never answers is cut off");
    }
}

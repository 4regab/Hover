//! The optional background service: `hoverai --service`, a copy of Hover with no window that runs saved tasks, webhooks, pull request
//! watches and quota resumes while the app is closed. It is off until the user installs it, and installing is the only thing that
//! starts it at login; closing the app never stops it, and stopping or uninstalling it never touches the history or the tasks.
//!
//! One owner at a time (wake.rs’s executor lock), and the app wins:
//! - The service takes the lock when it starts, unless the app has it (then it exits and leaves it be).
//! - When the app starts and finds the service holding the lock, it leaves a *handover request*. The service stops starting new runs,
//!   lets the ones it has finish, lets go of the lock and exits; the app, which has been trying meanwhile, takes over. So approvals are
//!   answered where the user is, and no task is ever run by two copies.
//! - The service runs only tasks that don't ask: access `full`, or `read`. A task that asks before it acts waits, with a note, for the
//!   app to be open. No one being at the screen never answers for the user.
//! - When the app quits, the unit starts the service again a few seconds later.
//!
//! Per-user units per platform (nothing is installed system-wide, nothing needs admin rights): a systemd user unit on Linux, a
//! LaunchAgent on macOS, a scheduled task at log-on on Windows. What is tested here is the text of each unit, the exact commands, and the
//! service's own start, take-over and hand-over (headless.rs). None of the three was started through its real service manager: the
//! build machine had no systemd, and no Mac or Windows was used. Those steps are unverified until run on each system.

use std::path::{Path, PathBuf};
use std::process::Command;

pub const UNIT: &str = "hover.service";
pub const LABEL: &str = "dev.hover.service";
pub const TASK: &str = "Hover Service";

/// Where the handover request is left.
pub fn handover_file(dir: &Path) -> PathBuf { dir.join("handover.request") }

/// The app asks the service to let go. It is a plain file so that no listener is needed on either side.
pub fn request_handover(dir: &Path, who: &str) { let _ = std::fs::create_dir_all(dir); let _ = std::fs::write(handover_file(dir), who); }

pub fn handover_requested(dir: &Path) -> bool { handover_file(dir).exists() }

/// The request is answered (the service has let go) or withdrawn (the app is leaving).
pub fn clear_handover(dir: &Path) { let _ = std::fs::remove_file(handover_file(dir)); }

/// What the service would be installed as.
#[derive(Clone, Debug, PartialEq)]
pub struct Spec {
    pub exe: PathBuf,
    /// Set when the data folder is not the default one, so the service and the app share one.
    pub data_dir: Option<PathBuf>,
}

/// A systemd ExecStart word: quoted, with `\`, `"`, `%` and `$` made literal.
fn systemd_word(s: &str) -> String { format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('%', "%%").replace('$', "$$")) }

fn xml(s: &str) -> String { s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;") }

impl Spec {
    pub fn systemd_unit(&self) -> String {
        let env = self.data_dir.as_ref().map(|d| format!("Environment={}\n", systemd_word(&format!("HOVER_DATA_DIR={}", d.display())))).unwrap_or_default();
        format!("[Unit]\nDescription=Hover background service (saved tasks, webhooks, watches)\n\n[Service]\nType=simple\nExecStart={} --service\n{env}Restart=always\nRestartSec=5\n\n[Install]\nWantedBy=default.target\n",
            systemd_word(&self.exe.to_string_lossy()))
    }

    pub fn launch_agent(&self) -> String {
        let env = self.data_dir.as_ref().map(|d| format!("  <key>EnvironmentVariables</key>\n  <dict><key>HOVER_DATA_DIR</key><string>{}</string></dict>\n", xml(&d.to_string_lossy()))).unwrap_or_default();
        format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\">\n<dict>\n  <key>Label</key><string>{LABEL}</string>\n  <key>ProgramArguments</key>\n  <array><string>{}</string><string>--service</string></array>\n{env}  <key>RunAtLoad</key><true/>\n  <key>KeepAlive</key><true/>\n  <key>ThrottleInterval</key><integer>5</integer>\n</dict>\n</plist>\n", xml(&self.exe.to_string_lossy()))
    }

    /// `schtasks` arguments for a task that starts at log-on, one argument each.
    pub fn schtasks_create(&self) -> Vec<String> {
        let tr = format!("\"{}\" --service", self.exe.display());
        ["/Create", "/TN", TASK, "/SC", "ONLOGON", "/TR", &tr, "/RL", "LIMITED", "/F"].iter().map(|s| s.to_string()).collect()
    }
}

fn home() -> PathBuf { crate::proc::home() }

fn unit_path() -> PathBuf { home().join(".config/systemd/user").join(UNIT) }
fn agent_path() -> PathBuf { home().join("Library/LaunchAgents").join(format!("{LABEL}.plist")) }

/// What the user is told about the service.
#[derive(Clone, Debug, PartialEq)]
pub enum State { NotInstalled, Installed { running: bool }, Unavailable(String) }

fn run(cmd: &str, args: &[&str]) -> Result<String, String> {
    let o = Command::new(cmd).args(args).stdin(std::process::Stdio::null()).output().map_err(|e| format!("{cmd} didn’t start: {e}"))?;
    let out = String::from_utf8_lossy(&o.stdout).trim().to_owned();
    if o.status.success() { Ok(out) } else { Err(format!("{cmd} {}: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("failed").trim())) }
}

/// Installs the service for this user and starts it. This is the user's own action; nothing else calls it.
pub fn install(spec: &Spec) -> Result<(), String> {
    if cfg!(target_os = "linux") {
        let p = unit_path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, spec.systemd_unit()).map_err(|e| format!("Couldn’t write {}: {e}", p.display()))?;
        run("systemctl", &["--user", "daemon-reload"])?;
        run("systemctl", &["--user", "enable", "--now", UNIT])?;
        Ok(())
    } else if cfg!(target_os = "macos") {
        let p = agent_path();
        std::fs::create_dir_all(p.parent().unwrap()).map_err(|e| e.to_string())?;
        std::fs::write(&p, spec.launch_agent()).map_err(|e| format!("Couldn’t write {}: {e}", p.display()))?;
        let uid = run("id", &["-u"])?;
        let _ = run("launchctl", &["bootout", &format!("gui/{uid}/{LABEL}")]);
        run("launchctl", &["bootstrap", &format!("gui/{uid}"), &p.to_string_lossy()]).map(|_| ())
    } else {
        let args = spec.schtasks_create();
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        run("schtasks", &a)?;
        run("schtasks", &["/Run", "/TN", TASK]).map(|_| ())
    }
}

/// Stops it from starting at login and stops it now. Tasks, run records and history stay as they are.
pub fn uninstall() -> Result<(), String> {
    if cfg!(target_os = "linux") {
        let _ = run("systemctl", &["--user", "disable", "--now", UNIT]);
        let _ = std::fs::remove_file(unit_path());
        let _ = run("systemctl", &["--user", "daemon-reload"]);
        Ok(())
    } else if cfg!(target_os = "macos") {
        if let Ok(uid) = run("id", &["-u"]) { let _ = run("launchctl", &["bootout", &format!("gui/{uid}/{LABEL}")]); }
        let _ = std::fs::remove_file(agent_path());
        Ok(())
    } else {
        let _ = run("schtasks", &["/End", "/TN", TASK]);
        run("schtasks", &["/Delete", "/TN", TASK, "/F"]).map(|_| ())
    }
}

/// Stops the service now without uninstalling it (it comes back at the next login, or when the app quits).
pub fn stop() -> Result<(), String> {
    if cfg!(target_os = "linux") { run("systemctl", &["--user", "stop", UNIT]).map(|_| ()) }
    else if cfg!(target_os = "macos") { let uid = run("id", &["-u"])?; run("launchctl", &["kill", "TERM", &format!("gui/{uid}/{LABEL}")]).map(|_| ()) }
    else { run("schtasks", &["/End", "/TN", TASK]).map(|_| ()) }
}

pub fn state() -> State {
    if cfg!(target_os = "linux") {
        if !unit_path().is_file() { return State::NotInstalled; }
        match run("systemctl", &["--user", "is-active", UNIT]) { Ok(_) => State::Installed { running: true }, Err(e) if e.contains("didn’t start") => State::Unavailable(e), Err(_) => State::Installed { running: false } }
    } else if cfg!(target_os = "macos") {
        if agent_path().is_file() { State::Installed { running: run("launchctl", &["list", LABEL]).is_ok() } } else { State::NotInstalled }
    } else {
        match run("schtasks", &["/Query", "/TN", TASK]) { Ok(_) => State::Installed { running: true }, Err(e) if e.contains("didn’t start") => State::Unavailable(e), Err(_) => State::NotInstalled }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec() -> Spec { Spec { exe: PathBuf::from("/opt/My Apps/hover \"x\"/hoverai"), data_dir: Some(PathBuf::from("/home/a b/Hover data")) } }

    #[test]
    fn the_systemd_unit_quotes_paths_literally_and_restarts_after_the_app_quits() {
        let u = spec().systemd_unit();
        assert!(u.contains("ExecStart=\"/opt/My Apps/hover \\\"x\\\"/hoverai\" --service\n"), "{u}");
        assert!(u.contains("Environment=\"HOVER_DATA_DIR=/home/a b/Hover data\"\n") && u.contains("Restart=always") && u.contains("WantedBy=default.target"));
        let plain = Spec { exe: "/usr/bin/hover".into(), data_dir: None }.systemd_unit();
        assert!(!plain.contains("Environment") && plain.contains("ExecStart=\"/usr/bin/hover\" --service"));
        // % and $ would be read by systemd.
        assert!(Spec { exe: "/a/100%/$HOME/h".into(), data_dir: None }.systemd_unit().contains("\"/a/100%%/$$HOME/h\""));
    }

    #[test]
    fn the_launch_agent_and_the_windows_task_name_the_same_command() {
        let p = spec().launch_agent();
        assert!(p.contains("<string>dev.hover.service</string>") && p.contains("<string>--service</string>") && p.contains("<key>KeepAlive</key><true/>") && p.contains("HOVER_DATA_DIR"));
        assert!(Spec { exe: "/a&b/<h>".into(), data_dir: None }.launch_agent().contains("/a&amp;b/&lt;h&gt;"));
        let w = Spec { exe: PathBuf::from(r"C:\Program Files\Hover\hoverai.exe"), data_dir: None }.schtasks_create();
        assert_eq!(w[..4], ["/Create", "/TN", "Hover Service", "/SC"]);
        assert!(w.contains(&"ONLOGON".to_string()) && w.contains(&r#""C:\Program Files\Hover\hoverai.exe" --service"#.to_string()) && w.contains(&"LIMITED".to_string()), "{w:?}");
    }

    #[test]
    fn the_handover_is_a_file_that_is_asked_for_and_cleared() {
        let d = std::env::temp_dir().join(format!("hover-service-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        assert!(!handover_requested(&d));
        request_handover(&d, "the app");
        assert!(handover_requested(&d));
        clear_handover(&d);
        assert!(!handover_requested(&d));
    }
}

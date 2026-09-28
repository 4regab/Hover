//! One copy of Hover at a time (App.OnStartup). A second launch asks the running copy
//! to open its window, then exits. Windows uses the C# app's own names
//! (Local\HoverRunningInstance, Local\HoverShowApp), so the two builds also keep out
//! of each other's way. Linux holds a lock on hover.lock in $XDG_RUNTIME_DIR and
//! listens on hover.sock beside it; the lock goes with the process, however it ends.

pub enum Claim {
    /// This is the only copy; keep the value alive for as long as the app runs.
    First(Instance),
    /// Another copy runs and has been asked to show its window.
    Second,
}

pub struct Instance {
    #[allow(dead_code)]
    inner: imp::Held,
}

/// Claims the instance under the app's name; on_show runs (on a thread of its own)
/// whenever a later launch asks. The token is the launch's activation token on
/// Linux (XDG_ACTIVATION_TOKEN or DESKTOP_STARTUP_ID), for the window to take focus.
pub fn claim(on_show: impl Fn(Option<String>) + Send + 'static) -> std::io::Result<Claim> { claim_named("Hover", on_show) }

pub fn claim_named(name: &str, on_show: impl Fn(Option<String>) + Send + 'static) -> std::io::Result<Claim> {
    imp::claim(name, Box::new(on_show))
}

#[cfg(windows)]
mod imp {
    use super::{Claim, Instance};
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS, HANDLE, WAIT_OBJECT_0};
    use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, OpenEventW, SetEvent, WaitForSingleObject, EVENT_MODIFY_STATE, INFINITE};
    use windows::Win32::UI::WindowsAndMessaging::{AllowSetForegroundWindow, ASFW_ANY};

    pub struct Held { mutex: HANDLE }

    impl Drop for Held {
        fn drop(&mut self) { unsafe { let _ = CloseHandle(self.mutex); } }
    }

    struct Send_(HANDLE);
    unsafe impl Send for Send_ {}

    pub fn claim(name: &str, on_show: Box<dyn Fn(Option<String>) + Send>) -> std::io::Result<Claim> {
        let (mutex_name, show_name) = if name == "Hover" {
            (HSTRING::from("Local\\HoverRunningInstance"), HSTRING::from("Local\\HoverShowApp"))
        } else {
            (HSTRING::from(format!("Local\\{name}RunningInstance")), HSTRING::from(format!("Local\\{name}ShowApp")))
        };
        unsafe {
            let mutex = CreateMutexW(None, true, &mutex_name).map_err(std::io::Error::other)?;
            if GetLastError() == ERROR_ALREADY_EXISTS {
                let _ = CloseHandle(mutex);
                // This process was just launched by the user, so it may pass on the
                // right to take focus.
                if let Ok(show) = OpenEventW(EVENT_MODIFY_STATE, false, &show_name) {
                    let _ = AllowSetForegroundWindow(ASFW_ANY);
                    let _ = SetEvent(show);
                    let _ = CloseHandle(show);
                }
                return Ok(Claim::Second);
            }
            // Auto-reset, as EventResetMode.AutoReset.
            let event = CreateEventW(None, false, false, &show_name).map_err(std::io::Error::other)?;
            let ev = Send_(event);
            std::thread::Builder::new().name("single-instance".into()).spawn(move || {
                let ev = ev;
                while WaitForSingleObject(ev.0, INFINITE) == WAIT_OBJECT_0 { on_show(None); }
            })?;
            Ok(Claim::First(Instance { inner: Held { mutex } }))
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::{Claim, Instance};
    use std::io::{BufRead, BufReader, Write};
    use std::os::unix::net::{UnixListener, UnixStream};
    use std::path::PathBuf;

    pub struct Held { _lock: std::fs::File, sock: PathBuf }

    impl Drop for Held {
        fn drop(&mut self) { let _ = std::fs::remove_file(&self.sock); }
    }

    /// $XDG_RUNTIME_DIR (this user's, 0700), else a 0700 folder of this user's in /tmp.
    fn runtime_dir() -> std::io::Result<PathBuf> {
        if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir()) { return Ok(d); }
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        let uid = std::fs::metadata("/proc/self").map(|m| m.uid()).unwrap_or(0);
        let d = std::env::temp_dir().join(format!("hover-{uid}"));
        std::fs::create_dir_all(&d)?;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))?;
        Ok(d)
    }

    pub fn claim(name: &str, on_show: Box<dyn Fn(Option<String>) + Send>) -> std::io::Result<Claim> {
        let dir = runtime_dir()?;
        let low = name.to_lowercase();
        let lock = std::fs::OpenOptions::new().create(true).truncate(false).write(true).open(dir.join(format!("{low}.lock")))?;
        let sock = dir.join(format!("{low}.sock"));
        if lock.try_lock().is_err() {
            // The token lets the running copy's window take focus on Wayland (and
            // startup notification on X11): this launch's right, passed on.
            let token = std::env::var("XDG_ACTIVATION_TOKEN").or_else(|_| std::env::var("DESKTOP_STARTUP_ID")).unwrap_or_default();
            if let Ok(mut s) = UnixStream::connect(&sock) { let _ = writeln!(s, "show {token}"); }
            return Ok(Claim::Second);
        }
        // Held: any socket left there is a crashed copy's.
        let _ = std::fs::remove_file(&sock);
        let listener = UnixListener::bind(&sock)?;
        std::thread::Builder::new().name("single-instance".into()).spawn(move || {
            for s in listener.incoming().flatten() {
                let mut line = String::new();
                let _ = BufReader::new(&s).read_line(&mut line);
                if let Some(rest) = line.trim_end().strip_prefix("show") {
                    let t = rest.trim();
                    on_show((!t.is_empty()).then(|| t.to_owned()));
                }
            }
        })?;
        Ok(Claim::First(Instance { inner: Held { _lock: lock, sock } }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_second_claim_asks_the_first_to_show() {
        let name = format!("HoverTest{}", std::process::id());
        let (tx, rx) = std::sync::mpsc::channel();
        let first = claim_named(&name, move |t| { let _ = tx.send(t); }).unwrap();
        assert!(matches!(first, Claim::First(_)));
        assert!(matches!(claim_named(&name, |_| {}).unwrap(), Claim::Second));
        assert!(rx.recv_timeout(std::time::Duration::from_secs(5)).is_ok(), "the first copy was asked to show");
        drop(first);
        // Once the first has gone, the name is free again.
        assert!(matches!(claim_named(&name, |_| {}).unwrap(), Claim::First(_)));
    }
}

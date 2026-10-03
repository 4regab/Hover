//! One copy of Hover at a time (App.OnStartup). A second launch asks the running copy
//! to open its window, then exits. Windows uses the C# app's own names
//! (Local\HoverRunningInstance, Local\HoverShowApp), so the two builds also keep out
//! of each other's way. Linux holds a lock on hover.lock in $XDG_RUNTIME_DIR and
//! listens on hover.sock beside it; macOS does the same in $TMPDIR (it has no
//! $XDG_RUNTIME_DIR). The lock goes with the process, however it ends.

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

/// Room in a Unix socket's path (sun_path is 104 bytes on macOS, 108 on Linux, with the
/// NUL): a folder whose path leaves too little for the file is passed over.
#[cfg_attr(windows, allow(dead_code))]
fn socket_fits(dir: &std::path::Path, file: &str) -> bool { dir.as_os_str().len() + 1 + file.len() < 100 }

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

    /// This user's id: the owner of /proc/self on Linux, of the home folder on macOS.
    /// This process's user, from the C library. Reading $HOME's owner (as it once did)
    /// gave 0, root's, wherever $HOME can't be read (a sandbox), and the lock then went
    /// to a folder of root's in /tmp that this user can't make.
    fn uid() -> u32 {
        extern "C" { fn getuid() -> u32; }
        // SAFETY: getuid takes no arguments, never fails and touches no memory of ours.
        unsafe { getuid() }
    }

    /// Where the lock and the socket live, for this user alone: $XDG_RUNTIME_DIR (0700 by
    /// the spec) where there is one; on macOS, which has none, $TMPDIR (a per-user 0700
    /// folder under /var/folders) when the socket's path fits in it; else a 0700 folder
    /// of this user's in /tmp.
    fn runtime_dir(sock: &str) -> std::io::Result<PathBuf> {
        use std::os::unix::fs::{MetadataExt, PermissionsExt};
        if let Some(d) = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).filter(|d| d.is_absolute() && d.is_dir()) { return Ok(d); }
        let uid = uid();
        let mine = |d: &std::path::Path| std::fs::metadata(d).is_ok_and(|m| m.is_dir() && m.uid() == uid && m.permissions().mode() & 0o077 == 0);
        if cfg!(target_os = "macos") {
            if let Some(d) = std::env::var_os("TMPDIR").map(PathBuf::from).filter(|d| d.is_absolute() && mine(d) && super::socket_fits(d, sock)) { return Ok(d); }
        }
        let base = if cfg!(target_os = "macos") { PathBuf::from("/tmp") } else { std::env::temp_dir() };
        let d = base.join(format!("hover-{uid}"));
        std::fs::create_dir_all(&d)?;
        std::fs::set_permissions(&d, std::fs::Permissions::from_mode(0o700))?;
        // A folder someone else made first is not ours to put a lock in.
        if !mine(&d) { return Err(std::io::Error::new(std::io::ErrorKind::PermissionDenied, format!("{} belongs to someone else", d.display()))); }
        Ok(d)
    }

    pub fn claim(name: &str, on_show: Box<dyn Fn(Option<String>) + Send>) -> std::io::Result<Claim> {
        let low = name.to_lowercase();
        let dir = runtime_dir(&format!("{low}.sock"))?;
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
    fn a_socket_path_must_fit_sun_path() {
        let long = std::path::PathBuf::from(format!("/{}", "d".repeat(90)));
        assert!(socket_fits(std::path::Path::new("/var/folders/zz/zyxvpxvq6csfxvn_n0000000000000/T"), "hover.sock"));
        assert!(!socket_fits(&long, "hover.sock"));
        assert!(socket_fits(std::path::Path::new("/tmp/hover-501"), "hovertest123456.sock"));
    }

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

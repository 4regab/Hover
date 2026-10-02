//! Local speech (Phonon): its setup from Settings and the on-demand engine.
//!
//! Phonon-2 runs in the official `fermion` CLI (Python and CPU PyTorch), so Hover keeps a
//! Python of its own for it in `<support>/phonon/installs/<id>`: python-build-standalone's
//! CPython, the pinned wheels (pip from local, hash-checked files only), the model as
//! fermion's own verifying unpacker writes it, and the model's NOTICE and licences. Every
//! byte comes from a pinned URL with a pinned size and SHA-256. Nothing runs at login:
//! each recording starts `fermion transcribe --json`, offline, and it exits when done.
//!
//! Setup builds a new folder in `installs/`, checks that it turns the bundled sample into
//! the expected words, writes `ready.json` there, and only then points `current` (a small
//! file naming the folder) at it, by an atomic rename of that file. Folders are never
//! moved: Windows refuses to rename one while a virus scanner still reads a file in it,
//! and the check runs where the install will live. So a failed or cancelled repair
//! leaves the working install as it was, and a crash at any point leaves a disk that
//! says what it holds; folders `current` doesn't name are swept at the next setup.

use std::ffi::OsString;
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};
use std::time::{Duration, Instant};

use hover_agents::proc::Group;
use hover_core::projects::LocalModel;
use hover_core::settings::Settings;
use sha2::{Digest, Sha256};

use crate::speech::{Speech, SpeechError, Transcript};

/// The card's state.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    NotInstalled,
    /// Why this device can't run it, said before any download.
    Unsupported(String),
    Downloading { done: u64, total: Option<u64> },
    Verifying,
    Installing,
    Ready,
    Cancelled,
    /// Keeps a working older install usable (`speech()` still gives it).
    Failed(String),
}

/// What the card shows; real numbers from the pins.
#[derive(Clone, Debug, PartialEq)]
pub struct Facts {
    pub model: &'static str,
    pub version: String,
    /// Runtime, wheels, model and its attribution files, for this platform.
    pub download_bytes: u64,
    /// Installed: the runtime, the model and the plane cache fermion writes beside it.
    pub disk_bytes: u64,
    /// During setup: the downloads and the staged install side by side.
    pub peak_disk_bytes: u64,
    /// Hover's own folder for it (`<support>/phonon`); the install is in it.
    pub folder: PathBuf,
}

const ID: &str = "phonon-2";
const REVISION: &str = "9c7fef3584499a88fe8d394427f45851bbb8b446";
const FERMION: &str = "0.2.5";
const MODEL_DIR: &str = "model_phonon2_c4c_int6";

/// python-build-standalone 20260929, CPython 3.12.14, install_only: (triple, size, sha256).
const PYTHONS: [(&str, u64, &str); 3] = [
    ("x86_64-pc-windows-msvc", 46_425_719, "28728baf30b65e263f0b25c5a85be8226e7ab0d212fbadd6a8f0f796139fa804"),
    ("x86_64-unknown-linux-gnu", 66_919_180, "06c90b93f419b63371c18f20fed0558a1a901f6518c3c24f755077e048447e7f"),
    ("aarch64-unknown-linux-gnu", 52_300_852, "9c797cf657f6dced51d3e74eeabd7c1ca742d5bf10f66080a290e424ced8edaf"),
];
/// The wheels per triple, from assets/phonon/make-lock.py: name version size sha256 url.
const WHEELS: [(&str, &str); 3] = [
    ("x86_64-pc-windows-msvc", include_str!("../assets/phonon/wheels-x86_64-pc-windows-msvc.txt")),
    ("x86_64-unknown-linux-gnu", include_str!("../assets/phonon/wheels-x86_64-unknown-linux-gnu.txt")),
    ("aarch64-unknown-linux-gnu", include_str!("../assets/phonon/wheels-aarch64-unknown-linux-gnu.txt")),
];
/// From FermionResearch/Phonon-2 at REVISION: the model archive first, then the files
/// that must stay with the weights (CC-BY-4.0) and the code (Apache-2.0).
const REPO_FILES: [(&str, u64, &str); 4] = [
    ("phonon-2.bps.tar.zst", 163_515_201, "98125795b6dda72f5c6eee9ba33d19815df65dcb18b50a357bf9f73c9935309e"),
    ("NOTICE", 2_741, "00624a5043e7ce74029317b024132ca5286116d6fbf3191d226374bc8273789f"),
    ("LICENSE-WEIGHTS-CC-BY-4.0.txt", 18_657, "9ba9550ad48438d0836ddab3da480b3b69ffa0aac7b7878b5a0039e7ab429411"),
    ("LICENSE-CODE-Apache-2.0.txt", 11_358, "cfc7749b96f63bd31c3c42b5c471bf756814053e847c10f3eb003417bc523d30"),
];
/// What fermion's unpacker writes into MODEL_DIR (it checks every member itself; this is
/// Hover's check of the result, and what a later start looks for).
const MODEL_FILES: [(&str, u64, &str); 3] = [
    ("config.json", 277_493, "d0daad3b2a182893844f4abdc11e4f5b7083f7d42c8ad7e8203f71559785a31b"),
    ("model.fermion", 177_438_361, "4b6bfa3a12cc3c4e0a54f2ab3ec4ca7a842b09e5c7ecfc8e7ca0ac6cc8c11468"),
    ("packed_manifest.json", 821, "3c1874501a1efb6ef569eab082aa68d91b93ec6e4d2fb00dbef7851edf2ae510"),
];
/// Installed size, measured after setup (evidence/voice-chat/phonon-proto.md); most of it
/// is PyTorch and the 304 MB plane cache fermion writes beside the model on its first run.
/// Linux arm64 is an estimate (not measured yet): its torch wheel is 37 MB smaller.
const DISK: [(&str, u64); 3] = [
    ("x86_64-pc-windows-msvc", 1_455_000_000),
    ("x86_64-unknown-linux-gnu", 1_826_000_000),
    ("aarch64-unknown-linux-gnu", 1_780_000_000),
];
/// Headroom on top of the peak: pip's temp files, the file system's own rounding.
const MARGIN: u64 = 300 << 20;

/// The bundled check: Windows' David voice, 16 kHz mono 16-bit.
const SAMPLE: &[u8] = include_bytes!("../assets/phonon/check.wav");
const SAMPLE_TEXT: &str = "Open the notes folder and add a list of the open tasks.";
/// Voice stops recording at ten minutes; a few seconds over is the WAV's own slack.
const MAX_AUDIO: Duration = Duration::from_secs(605);

const MAIN: &str = "import sys; sys.argv[0] = 'fermion'; from fermion.cli import main; sys.exit(main())";
/// fermion's own unpacker (byte-plane join, every member's SHA-256 checked before it is
/// written, a `.partial` sibling renamed at the end). Private, so pinned with fermion.
const UNPACK: &str = "import sys; from pathlib import Path; from fermion._speech.fetch import _unpack; print(_unpack(Path(sys.argv[1]), Path(sys.argv[2])))";

const VC_MSG: &str = "Phonon needs the Microsoft Visual C++ Redistributable (x64). Install it from https://aka.ms/vs/17/release/vc_redist.x64.exe, then press Download again.";
const DAMAGED: &str = "Phonon’s files are missing or damaged. Press Repair in Settings → Voice.";

/// One file to fetch: where it goes under `downloads/`, and its pins.
#[derive(Clone, Debug)]
struct Dl { name: String, url: String, size: u64, sha256: String }

/// Everything one platform's install is made of.
#[derive(Clone)]
struct Pins {
    triple: &'static str,
    python: Dl,
    /// (name, version, file)
    wheels: Vec<(String, String, Dl)>,
    /// The model archive, then the attribution files.
    repo: Vec<Dl>,
    model: Vec<(String, u64, String)>,
    disk: u64,
}

impl Pins {
    fn official(triple: &'static str) -> Pins {
        let py = PYTHONS.iter().find(|p| p.0 == triple).unwrap_or(&PYTHONS[0]);
        let name = format!("cpython-3.12.14+20260929-{}-install_only.tar.gz", py.0);
        let python = Dl {
            url: format!("https://github.com/astral-sh/python-build-standalone/releases/download/20260929/{}", name.replace('+', "%2B")),
            name, size: py.1, sha256: py.2.into(),
        };
        let lock = WHEELS.iter().find(|w| w.0 == triple).unwrap_or(&WHEELS[0]).1;
        let repo = REPO_FILES.iter().map(|(n, size, sha)| Dl {
            name: n.to_string(), url: format!("https://huggingface.co/FermionResearch/Phonon-2/resolve/{REVISION}/{n}"), size: *size, sha256: sha.to_string(),
        }).collect();
        Pins {
            triple, python, wheels: parse_lock(lock), repo,
            model: MODEL_FILES.iter().map(|(n, s, h)| (n.to_string(), *s, h.to_string())).collect(),
            disk: DISK.iter().find(|d| d.0 == triple).map_or(DISK[0].1, |d| d.1),
        }
    }

    fn downloads(&self) -> impl Iterator<Item = &Dl> {
        std::iter::once(&self.python).chain(self.wheels.iter().map(|w| &w.2)).chain(&self.repo)
    }

    fn download_bytes(&self) -> u64 { self.downloads().map(|d| d.size).sum() }

    /// Changes when any pinned byte does: what `ready.json` must say for an install to count.
    fn identity(&self) -> String {
        let mut h = Sha256::new();
        h.update(self.triple);
        for d in self.downloads() { h.update(&d.sha256); }
        for m in &self.model { h.update(&m.2); }
        hex(h)
    }
}

fn parse_lock(text: &str) -> Vec<(String, String, Dl)> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).filter_map(|l| {
        let f: Vec<&str> = l.split_whitespace().collect();
        let [name, version, size, sha, url] = f[..] else { return None };
        let file = url.rsplit('/').next()?.replace("%2B", "+");
        Some((name.into(), version.into(), Dl { name: format!("wheels/{file}"), url: url.into(), size: size.parse().ok()?, sha256: sha.into() }))
    }).collect()
}

fn version_label() -> String { format!("{ID} @ {} · fermion {FERMION}", &REVISION[..7]) }

fn local_model(home: &Path) -> LocalModel {
    LocalModel { id: ID.into(), version: version_label(), folder: home.to_string_lossy().into_owned() }
}

/// This build's platform, when Phonon has wheels for it.
fn triple() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", "x86_64") => Some("x86_64-pc-windows-msvc"),
        ("linux", "x86_64") => Some("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Some("aarch64-unknown-linux-gnu"),
        _ => None,
    }
}

/// Why Local speech is off on this OS, for Settings to show beside its choice: on a Mac
/// there is no build of the runtime pinned (no wheels, no measured install), so Cloud it is.
pub fn local_note() -> Option<&'static str> { cfg!(target_os = "macos").then_some(crate::mac::notes::LOCAL_SPEECH) }

/// Why this device can't run Phonon, known before anything is downloaded.
fn unsupported() -> Option<String> {
    if let Some(note) = local_note() { return Some(note.into()); }
    if triple().is_none() {
        return Some(format!("Local speech runs on Windows x64 and on Linux (x64 or arm64), not {} {}.", std::env::consts::OS, std::env::consts::ARCH));
    }
    #[cfg(target_arch = "x86_64")]
    if !std::arch::is_x86_feature_detected!("sse4.1") { return Some("Phonon needs a processor with SSE4.1.".into()); }
    #[cfg(windows)]
    {
        if os::arm64() { return Some("Phonon doesn’t run on Windows on Arm.".into()); }
        let sys = PathBuf::from(std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into())).join("System32");
        if !["msvcp140.dll", "vcruntime140.dll", "vcruntime140_1.dll"].iter().all(|d| sys.join(d).is_file()) { return Some(VC_MSG.into()); }
    }
    #[cfg(target_os = "linux")]
    if os::glibc() < (2, 28) { return Some("Phonon needs glibc 2.28 or newer (Ubuntu 20.04, Debian 10, Fedora 29 or later).".into()); }
    None
}

#[cfg(windows)]
mod os {
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    extern "system" {
        fn GetCurrentProcess() -> isize;
        fn IsWow64Process2(process: isize, process_machine: *mut u16, native_machine: *mut u16) -> i32;
        fn GetDiskFreeSpaceExW(dir: *const u16, avail: *mut u64, total: *mut u64, free: *mut u64) -> i32;
    }

    /// An x64 build under Windows on Arm's emulation still says x86_64; the machine doesn't.
    pub fn arm64() -> bool {
        let (mut p, mut n) = (0u16, 0u16);
        unsafe { IsWow64Process2(GetCurrentProcess(), &mut p, &mut n) != 0 && n == 0xAA64 }
    }

    pub fn free(dir: &std::path::Path) -> Option<u64> {
        let w: Vec<u16> = dir.as_os_str().encode_wide().chain([0]).collect();
        let mut avail = 0u64;
        (unsafe { GetDiskFreeSpaceExW(w.as_ptr(), &mut avail, std::ptr::null_mut(), std::ptr::null_mut()) } != 0).then_some(avail)
    }
}

#[cfg(unix)]
mod os {
    pub fn free(dir: &std::path::Path) -> Option<u64> {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(dir.as_os_str().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        (unsafe { libc::statvfs(c.as_ptr(), &mut s) } == 0).then(|| s.f_bavail as u64 * s.f_frsize as u64)
    }

    /// PyTorch's Linux wheels are manylinux_2_28.
    #[cfg(target_os = "linux")]
    pub fn glibc() -> (u32, u32) {
        let v = unsafe { std::ffi::CStr::from_ptr(libc::gnu_get_libc_version()) }.to_string_lossy().into_owned();
        let mut n = v.split('.').map(|x| x.parse().unwrap_or(0));
        (n.next().unwrap_or(0), n.next().unwrap_or(0))
    }
}

fn free_bytes(dir: &Path) -> Option<u64> {
    let mut d = dir;
    while !d.exists() { d = d.parent()?; }
    os::free(d)
}

fn hex(h: Sha256) -> String { format!("{:x}", h.finalize()) }

fn hash_file(p: &Path, cancel: &AtomicBool) -> Result<String, Fail> {
    let mut f = std::fs::File::open(p).map_err(|e| Fail::Failed(format!("{}: {e}", p.display())))?;
    let (mut h, mut buf) = (Sha256::new(), vec![0u8; 1 << 20]);
    loop {
        if cancel.load(Ordering::Relaxed) { return Err(Fail::Cancelled); }
        let n = f.read(&mut buf).map_err(|e| Fail::Failed(format!("{}: {e}", p.display())))?;
        if n == 0 { return Ok(hex(h)); }
        h.update(&buf[..n]);
    }
}

fn python_exe(home: &Path) -> PathBuf {
    if cfg!(windows) { home.join("python").join("python.exe") } else { home.join("python").join("bin").join("python3") }
}

/// The words, for comparing what was heard with what was said.
fn words(s: &str) -> Vec<String> {
    s.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).map(str::to_lowercase).collect()
}

#[derive(Debug)]
enum Fail { Cancelled, Failed(String) }

impl From<std::io::Error> for Fail {
    fn from(e: std::io::Error) -> Fail { Fail::Failed(e.to_string()) }
}

/// Stands in for the managed Python in tests: (home, args) -> stdout.
type Fake = Box<dyn Fn(&Path, &[OsString]) -> Result<String, String> + Send + Sync>;

struct St {
    state: Install,
    /// The running setup's cancel flag.
    job: Option<Arc<AtomicBool>>,
}

pub struct Phonon {
    me: Weak<Phonon>,
    settings: Arc<Settings>,
    root: PathBuf,
    pins: Pins,
    st: Mutex<St>,
    listeners: Mutex<Vec<Arc<dyn Fn() + Send + Sync>>>,
    /// Held by a setup thread from start to clean-up: a cancelled one may still be
    /// deleting its staging when the next starts.
    setup: Mutex<()>,
    /// Held while a transcription reads `current/`, so a promotion or Remove never
    /// moves files under it.
    busy: Mutex<()>,
    in_use: AtomicUsize,
    /// The running transcription's process, for shutdown().
    engine: Mutex<Option<Arc<Group>>>,
    fake: Option<Fake>,
}

impl Phonon {
    /// Reads the disk only.
    pub fn new(settings: Arc<Settings>) -> Arc<Phonon> {
        Phonon::build(settings, hover_core::paths::support().join("phonon"), Pins::official(triple().unwrap_or(PYTHONS[0].0)), None)
    }

    fn build(settings: Arc<Settings>, root: PathBuf, pins: Pins, fake: Option<Fake>) -> Arc<Phonon> {
        let p = Arc::new_cyclic(|me| Phonon {
            me: me.clone(), settings, root, pins, st: Mutex::new(St { state: Install::NotInstalled, job: None }),
            listeners: Mutex::new(vec![]), setup: Mutex::new(()), busy: Mutex::new(()), in_use: AtomicUsize::new(0),
            engine: Mutex::new(None), fake,
        });
        let state = p.read_disk();
        // Settings say what is installed only when the disk agrees.
        let mut v = p.settings.voice();
        let want = p.current().filter(|_| state == Install::Ready).map(|h| local_model(&h));
        if (state == Install::Ready || matches!(state, Install::NotInstalled | Install::Unsupported(_))) && v.local != want {
            v.local = want;
            p.settings.set_voice(v);
        }
        p.st.lock().unwrap().state = state;
        p
    }

    /// The install `current` names, if it names one.
    fn current(&self) -> Option<PathBuf> {
        let name = std::fs::read_to_string(self.root.join("current")).ok()?;
        let name = name.trim();
        let plain = !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
        plain.then(|| self.root.join("installs").join(name))
    }

    fn read_disk(&self) -> Install {
        match self.current() {
            Some(h) if self.usable(&h) => Install::Ready,
            Some(_) => Install::Failed(DAMAGED.into()),
            None => unsupported().map_or(Install::NotInstalled, Install::Unsupported),
        }
    }

    /// `ready.json` says this exact set of pins passed the check.
    fn marker(&self, home: &Path) -> bool {
        std::fs::read(home.join("ready.json")).ok().and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
            .is_some_and(|v| v["pins"].as_str() == Some(self.pins.identity().as_str()))
    }

    /// The marker, the interpreter and the model's files at their sizes: cheap, so it is
    /// asked before every transcription.
    fn usable(&self, home: &Path) -> bool {
        let model = home.join("model").join(MODEL_DIR);
        self.marker(home) && python_exe(home).is_file()
            && self.pins.model.iter().all(|(n, size, _)| std::fs::metadata(model.join(n)).is_ok_and(|m| m.len() == *size))
    }

    pub fn state(&self) -> Install { self.st.lock().unwrap().state.clone() }

    pub fn facts(&self) -> Facts {
        let d = self.pins.download_bytes();
        Facts { model: "Phonon-2", version: version_label(), download_bytes: d, disk_bytes: self.pins.disk, peak_disk_bytes: d + self.pins.disk, folder: self.root.clone() }
    }

    pub fn on_change(&self, f: impl Fn() + Send + Sync + 'static) { self.listeners.lock().unwrap().push(Arc::new(f)); }

    fn notify(&self) {
        let ls = self.listeners.lock().unwrap().clone();
        for l in ls { l(); }
    }

    /// Download (and Retry): off the UI thread. Nothing happens while a setup runs.
    pub fn download(&self) { self.start(); }

    /// The same full setup; the install there stays usable until the new one passed.
    pub fn repair(&self) { self.start(); }

    fn start(&self) {
        let Some(me) = self.me.upgrade() else { return };
        let job = {
            let mut s = self.st.lock().unwrap();
            if s.job.is_some() { return; }
            if let Some(why) = unsupported() {
                s.state = Install::Unsupported(why);
                drop(s);
                self.notify();
                return;
            }
            let job = Arc::new(AtomicBool::new(false));
            s.job = Some(job.clone());
            s.state = Install::Downloading { done: 0, total: Some(self.pins.download_bytes()) };
            job
        };
        self.notify();
        let _ = std::thread::Builder::new().name("phonon-setup".into()).spawn(move || {
            let _one = me.setup.lock().unwrap();
            let home = me.root.join("installs").join(hover_core::guid_n());
            let r = me.install(&job, &home);
            if r.is_err() { let _ = std::fs::remove_dir_all(&home); }
            if let Err(Fail::Failed(e)) = &r { hover_core::log::line(&format!("phonon: setup failed - {e}")); }
            let mut s = me.st.lock().unwrap();
            // A cancelled job already said Cancelled, and a newer one may own the card now.
            // A cancel that came too late to stop the switch to the new install says Ready.
            if !s.job.as_ref().is_some_and(|j| Arc::ptr_eq(j, &job)) {
                if r.is_ok() && s.job.is_none() { s.state = Install::Ready; drop(s); me.notify(); }
                return;
            }
            s.job = None;
            s.state = match r {
                Ok(()) => Install::Ready,
                Err(Fail::Cancelled) => Install::Cancelled,
                Err(Fail::Failed(e)) => Install::Failed(e),
            };
            drop(s);
            me.notify();
        });
    }

    /// The state, if this job still owns the card.
    fn set(&self, job: &Arc<AtomicBool>, state: Install) {
        let mut s = self.st.lock().unwrap();
        if !s.job.as_ref().is_some_and(|j| Arc::ptr_eq(j, job)) { return; }
        s.state = state;
        drop(s);
        self.notify();
    }

    pub fn cancel(&self) {
        let mut s = self.st.lock().unwrap();
        let Some(job) = s.job.take() else { return };
        job.store(true, Ordering::Relaxed);
        s.state = Install::Cancelled;
        drop(s);
        self.notify();
    }

    /// Sets up a new install in `staging` (a fresh folder under installs/).
    fn install(&self, job: &Arc<AtomicBool>, staging: &Path) -> Result<(), Fail> {
        let (root, pins) = (&self.root, &self.pins);
        let dl = root.join("downloads");
        std::fs::create_dir_all(root)?;
        self.sweep();

        // Space for every download and the staged install side by side, before a byte comes.
        let need = pins.download_bytes() + pins.disk + MARGIN;
        if let Some(free) = free_bytes(root) {
            if free < need {
                return Err(Fail::Failed(format!("Phonon needs {} free on this drive during setup; {} is free.", gb(need), gb(free))));
            }
        }

        // 1. Downloading. A file a cancelled or failed run finished is kept (Verifying
        //    hashes it again); a partial one is never resumed.
        let total = pins.download_bytes();
        let agent = agent();
        let mut done = 0u64;
        let mut shown = Instant::now();
        for f in pins.downloads() {
            let dst = dl.join(&f.name);
            if std::fs::metadata(&dst).is_ok_and(|m| m.len() == f.size) { done += f.size; continue; }
            fetch(&agent, f, &dst, job, |n| {
                done += n;
                if shown.elapsed() >= Duration::from_millis(100) {
                    shown = Instant::now();
                    self.set(job, Install::Downloading { done, total: Some(total) });
                }
            })?;
        }
        self.set(job, Install::Downloading { done: total, total: Some(total) });

        // 2. Verifying: every file against its pin, as it is on disk now.
        self.set(job, Install::Verifying);
        for f in pins.downloads() {
            let p = dl.join(&f.name);
            if hash_file(&p, job)? != f.sha256 {
                let _ = std::fs::remove_file(&p);
                return Err(Fail::Failed(format!("{} didn’t match its pinned checksum. Press Retry.", f.name)));
            }
        }

        // 3. Installing, all in staging.
        self.set(job, Install::Installing);
        std::fs::create_dir_all(staging.join("tmp"))?;
        let mut t = Instant::now();
        let mut took = |what: &str| { hover_core::log::line(&format!("phonon: {what} took {:.1}s", t.elapsed().as_secs_f64())); t = Instant::now(); };
        untar_gz(&dl.join(&pins.python.name), &staging, job)?;
        took("unpacking the runtime");
        let reqs: String = pins.wheels.iter().map(|(n, v, d)| format!("{n}=={v} --hash=sha256:{}\n", d.sha256)).collect();
        std::fs::write(staging.join("requirements.txt"), reqs)?;
        let wheels = dl.join("wheels");
        // --no-compile: pip byte-compiles all of torch and transformers one file at a time
        // (9.5 min here, with Windows' scanner); the check below compiles only what Phonon
        // imports, and the whole setup took half as long (evidence/voice-chat/phonon-proto.md).
        self.python(&staging, &[
            "-I", "-m", "pip", "install", "--no-index", "--no-deps", "--require-hashes", "--only-binary", ":all:",
            "--no-compile", "--no-warn-script-location", "--find-links",
        ].map(OsString::from).into_iter().chain([wheels.into(), "-r".into(), staging.join("requirements.txt").into()]).collect::<Vec<_>>(), job, false)?;
        took("pip");

        let model = staging.join("model").join(MODEL_DIR);
        self.python(&staging, &["-I".into(), "-c".into(), UNPACK.into(), dl.join(&pins.repo[0].name).into(), model.clone().into()], job, false)?;
        took("unpacking the model");
        for (n, size, sha) in &pins.model {
            let p = model.join(n);
            if std::fs::metadata(&p).map(|m| m.len()).ok() != Some(*size) || hash_file(&p, job)? != *sha {
                return Err(Fail::Failed(format!("Phonon’s model file {n} didn’t match its pin.")));
            }
        }
        let lic = staging.join("licenses");
        std::fs::create_dir_all(&lic)?;
        for f in &pins.repo[1..] { std::fs::copy(dl.join(&f.name), lic.join(&f.name))?; }

        // The check: Ready means the runtime loads the model and hears the sample right.
        // Its first run also writes the plane cache, so the user's first recording is warm.
        std::fs::write(staging.join("check.wav"), SAMPLE)?;
        let out = self.python(&staging, &transcribe_args(&staging, &staging.join("check.wav")), job, false)?;
        took("the check");
        let heard = parse(&out).map_err(|e| Fail::Failed(format!("Phonon’s check failed: {}", e.message())))?.0;
        if words(&heard) != words(SAMPLE_TEXT) {
            return Err(Fail::Failed(format!("Phonon’s check heard “{heard}”, not the sample’s words.")));
        }
        let marker = serde_json::json!({
            "id": ID, "version": version_label(), "revision": REVISION, "fermion": FERMION, "triple": pins.triple,
            "pins": pins.identity(), "heard": heard,
        });
        std::fs::write(staging.join("ready.json"), marker.to_string())?;
        if job.load(Ordering::Relaxed) { return Err(Fail::Cancelled); }

        self.promote(staging).map_err(Fail::Failed)?;
        let _ = std::fs::remove_dir_all(&dl);
        let mut v = self.settings.voice();
        v.local = Some(local_model(staging));
        self.settings.set_voice(v);
        Ok(())
    }

    /// Points `current` at the checked install, then deletes the one it named before.
    fn promote(&self, home: &Path) -> Result<(), String> {
        let _busy = self.busy.lock().unwrap();
        let (old, tmp) = (self.current(), self.root.join("current.tmp"));
        let name = home.file_name().unwrap_or_default().to_string_lossy().into_owned();
        std::fs::write(&tmp, &name).and_then(|_| std::fs::rename(&tmp, self.root.join("current")))
            .map_err(|e| format!("Couldn’t switch to the new Phonon ({e})."))?;
        if let Some(old) = old.filter(|o| o != home) { let _ = std::fs::remove_dir_all(old); }
        Ok(())
    }

    /// Folders under installs/ that `current` doesn't name: an interrupted setup's, or an
    /// old install a scanner kept from being deleted.
    fn sweep(&self) {
        let cur = self.current();
        let Ok(rd) = std::fs::read_dir(self.root.join("installs")) else { return };
        for e in rd.flatten() {
            if Some(e.path()) != cur { let _ = std::fs::remove_dir_all(e.path()); }
        }
    }

    /// Err while a setup runs or a transcription reads the files. Forgets the install at
    /// once (`current` goes), then deletes Hover's own folders here on a worker.
    pub fn remove(&self) -> Result<(), String> {
        let mut s = self.st.lock().unwrap();
        if s.job.is_some() { return Err("Phonon is being set up. Cancel it first.".into()); }
        let Ok(_setup) = self.setup.try_lock() else { return Err("Phonon’s setup is still stopping. Try again in a moment.".into()) };
        let busy = self.busy.try_lock();
        if busy.is_err() || self.in_use.load(Ordering::Relaxed) > 0 { return Err("Phonon is transcribing. Try again when it’s done.".into()); }
        match std::fs::remove_file(self.root.join("current")) {
            Err(e) if e.kind() != std::io::ErrorKind::NotFound => return Err(format!("Couldn’t remove Phonon ({e}).")),
            _ => {}
        }
        s.state = unsupported().map_or(Install::NotInstalled, Install::Unsupported);
        drop(s);
        let mut v = self.settings.voice();
        if v.local.is_some() { v.local = None; self.settings.set_voice(v); }
        let Some(me) = self.me.upgrade() else { return Ok(()) };
        let _ = std::thread::Builder::new().name("phonon-remove".into()).spawn(move || {
            // A Download pressed meanwhile waits: it would build in the folder being deleted.
            let _one = me.setup.lock().unwrap();
            let root = &me.root;
            for d in ["installs", "downloads"] { let _ = std::fs::remove_dir_all(root.join(d)); }
            let _ = std::fs::remove_file(root.join("current.tmp"));
            let _ = std::fs::remove_dir(root);
        });
        self.notify();
        Ok(())
    }

    /// Some when an install that passed its check is on disk: Ready, or a failed or
    /// cancelled repair that left the working one alone.
    pub fn speech(self: &Arc<Self>) -> Option<Arc<dyn Speech>> {
        if !self.current().is_some_and(|h| self.usable(&h)) { return None; }
        Some(Arc::new(Local(self.clone())))
    }

    /// Stops a running transcription's process now.
    pub fn shutdown(&self) {
        if let Some(g) = self.engine.lock().unwrap().take() { g.kill(); }
    }

    fn transcribe(&self, wav: &Path, cancel: &AtomicBool) -> Result<Transcript, SpeechError> {
        let t = Instant::now();
        self.in_use.fetch_add(1, Ordering::Relaxed);
        struct Done<'a>(&'a AtomicUsize);
        impl Drop for Done<'_> { fn drop(&mut self) { self.0.fetch_sub(1, Ordering::Relaxed); } }
        let _done = Done(&self.in_use);
        let _busy = self.busy.lock().unwrap();
        let Some(home) = self.current().filter(|h| self.usable(h)) else { return Err(SpeechError::NotReady(DAMAGED.into())) };
        let len = std::fs::metadata(wav).map_err(|e| SpeechError::Engine(format!("The recording is gone ({e}).")))?.len();
        if Duration::from_secs_f64(len.saturating_sub(44) as f64 / 32_000.0) > MAX_AUDIO {
            return Err(SpeechError::Unsupported("Local speech takes up to ten minutes of audio.".into()));
        }
        let out = match self.python(&home, &transcribe_args(&home, wav), cancel, true) {
            Ok(o) => o,
            Err(Fail::Cancelled) => return Err(SpeechError::Cancelled),
            Err(Fail::Failed(_)) if cancel.load(Ordering::Relaxed) => return Err(SpeechError::Cancelled),
            Err(Fail::Failed(e)) => return Err(SpeechError::Engine(e)),
        };
        let (text, truncated, audio) = parse(&out)?;
        Ok(Transcript { text, language: None, truncated, audio, took: t.elapsed() })
    }

    /// Runs the managed Python in `home`, offline, and waits; killed when `cancel` is set.
    /// `engine`: a transcription, which shutdown() may stop.
    fn python(&self, home: &Path, args: &[OsString], cancel: &AtomicBool, engine: bool) -> Result<String, Fail> {
        if let Some(f) = &self.fake {
            if cancel.load(Ordering::Relaxed) { return Err(Fail::Cancelled); }
            return f(home, args).map_err(Fail::Failed);
        }
        let mut c = hover_agents::proc::hidden(&python_exe(home), &[]);
        c.args(args).current_dir(home).stdin(Stdio::null());
        let tmp = home.join("tmp");
        let _ = std::fs::create_dir_all(&tmp);
        for k in ["PYTHONPATH", "PYTHONHOME", "PYTHONSTARTUP", "PIP_INDEX_URL", "PIP_EXTRA_INDEX_URL", "PIP_FIND_LINKS"] { c.env_remove(k); }
        c.env("FERMION_CACHE_DIR", home.join("cache")).env("HF_HOME", home.join("hf")).env("HF_HUB_OFFLINE", "1").env("TRANSFORMERS_OFFLINE", "1")
            .env("HF_HUB_DISABLE_TELEMETRY", "1").env("DO_NOT_TRACK", "1").env("FERMION_QUIET_DEPRECATIONS", "1")
            .env("PIP_NO_CACHE_DIR", "1").env("PIP_DISABLE_PIP_VERSION_CHECK", "1").env("PIP_NO_INPUT", "1")
            .env("PIP_CONFIG_FILE", if cfg!(windows) { "nul" } else { "/dev/null" })
            .env("PYTHONNOUSERSITE", "1").env("PYTHONUTF8", "1").env("TMP", &tmp).env("TEMP", &tmp).env("TMPDIR", &tmp);
        let g = Arc::new(Group::spawn(c).map_err(|e| Fail::Failed(format!("Phonon’s Python didn’t start ({e}). {DAMAGED}")))?);
        let (_, out, err) = g.take_pipes();
        let out = std::thread::spawn(move || { let mut s = String::new(); if let Some(mut o) = out { let _ = o.read_to_string(&mut s); } s });
        let err = std::thread::spawn(move || {
            let mut b = vec![];
            if let Some(mut e) = err { let _ = e.read_to_end(&mut b); }
            String::from_utf8_lossy(&b[b.len().saturating_sub(4096)..]).into_owned()
        });
        if engine { *self.engine.lock().unwrap() = Some(g.clone()); }
        let code = loop {
            if cancel.load(Ordering::Relaxed) { g.kill(); break None; }
            if let Some(c) = g.wait_timeout(Duration::from_millis(100)) { break Some(c); }
        };
        if engine { let mut e = self.engine.lock().unwrap(); if e.as_ref().is_some_and(|x| Arc::ptr_eq(x, &g)) { *e = None; } }
        let (out, err) = (out.join().unwrap_or_default(), err.join().unwrap_or_default());
        match code {
            None => Err(Fail::Cancelled),
            Some(0) => Ok(out),
            Some(c) => Err(Fail::Failed(explain(c, &err))),
        }
    }
}

fn transcribe_args(home: &Path, wav: &Path) -> Vec<OsString> {
    vec!["-I".into(), "-c".into(), MAIN.into(), "transcribe".into(), "--json".into(), home.join("model").join(MODEL_DIR).into(), wav.into()]
}

/// What a failed run said, in a line.
fn explain(code: i32, err: &str) -> String {
    if err.contains("WinError 126") || err.contains("msvcp140") { return VC_MSG.into(); }
    let last = err.lines().rev().map(str::trim).find(|l| !l.is_empty()).unwrap_or("");
    let last: String = last.chars().take(300).collect();
    if last.is_empty() { format!("Phonon stopped (exit {code}).") } else { format!("Phonon stopped (exit {code}): {last}") }
}

/// `fermion transcribe --json`'s answer: text, truncated, the audio's length.
fn parse(out: &str) -> Result<(String, bool, Option<Duration>), SpeechError> {
    let line = out.lines().rev().find(|l| l.trim_start().starts_with('{')).ok_or_else(|| SpeechError::Engine("Phonon gave no answer.".into()))?;
    let v: serde_json::Value = serde_json::from_str(line).map_err(|e| SpeechError::Engine(format!("Phonon’s answer wasn’t readable ({e}).")))?;
    let text = v["text"].as_str().ok_or_else(|| SpeechError::Engine("Phonon’s answer had no text.".into()))?.trim().to_string();
    if text.is_empty() { return Err(SpeechError::Engine("Phonon heard no words.".into())); }
    Ok((text, v["truncated"].as_bool().unwrap_or(false), v["duration_seconds"].as_f64().filter(|d| d.is_finite() && *d >= 0.0).map(Duration::from_secs_f64)))
}

struct Local(Arc<Phonon>);

impl Speech for Local {
    fn transcribe(&self, wav: &Path, cancel: &AtomicBool) -> Result<Transcript, SpeechError> { self.0.transcribe(wav, cancel) }
}

fn agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder();
    #[cfg(windows)]
    let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls)
        .root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
    // No overall limit: a 200 MB wheel on a slow line is fine. A body that stalls for
    // an hour is not.
    cfg.timeout_connect(Some(Duration::from_secs(30))).timeout_recv_response(Some(Duration::from_secs(60)))
        .timeout_recv_body(Some(Duration::from_secs(3600))).http_status_as_error(true)
        .user_agent(format!("Hover/{}", env!("CARGO_PKG_VERSION"))).build().into()
}

/// One pinned file into `dst`, hashed as it comes; `seen` gets each chunk's size.
fn fetch(agent: &ureq::Agent, f: &Dl, dst: &Path, cancel: &AtomicBool, mut seen: impl FnMut(u64)) -> Result<(), Fail> {
    let part = PathBuf::from(format!("{}.part", dst.display()));
    if let Some(d) = dst.parent() { std::fs::create_dir_all(d)?; }
    let r = (|| -> Result<(), Fail> {
        let resp = agent.get(&f.url).call().map_err(|e| Fail::Failed(format!("Couldn’t download {}: {e}", f.name)))?;
        let mut body = resp.into_body().into_reader();
        let mut out = BufWriter::new(std::fs::File::create(&part)?);
        let (mut h, mut n, mut buf) = (Sha256::new(), 0u64, vec![0u8; 256 << 10]);
        loop {
            if cancel.load(Ordering::Relaxed) { return Err(Fail::Cancelled); }
            let k = body.read(&mut buf).map_err(|e| Fail::Failed(format!("Couldn’t download {}: {e}", f.name)))?;
            if k == 0 { break; }
            n += k as u64;
            if n > f.size { return Err(Fail::Failed(format!("{} is larger than its pin.", f.name))); }
            h.update(&buf[..k]);
            out.write_all(&buf[..k])?;
            seen(k as u64);
        }
        out.flush()?;
        drop(out);
        if n != f.size || hex(h) != f.sha256 { return Err(Fail::Failed(format!("{} didn’t match its pinned checksum. Press Retry.", f.name))); }
        Ok(())
    })();
    if r.is_err() { let _ = std::fs::remove_file(&part); return r; }
    std::fs::rename(&part, dst)?;
    Ok(())
}

/// The runtime's .tar.gz into `into`; an entry that would land outside it fails setup.
fn untar_gz(file: &Path, into: &Path, cancel: &AtomicBool) -> Result<(), Fail> {
    let mut ar = tar::Archive::new(flate2::read::GzDecoder::new(BufReader::new(std::fs::File::open(file)?)));
    for e in ar.entries()? {
        if cancel.load(Ordering::Relaxed) { return Err(Fail::Cancelled); }
        let mut e = e?;
        if !e.unpack_in(into)? { return Err(Fail::Failed("Phonon’s runtime archive has a path outside its folder.".into())); }
    }
    Ok(())
}

fn gb(b: u64) -> String { format!("{:.1} GB", b as f64 / 1e9) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::net::TcpListener;
    use std::sync::atomic::AtomicU64;

    fn sha(b: &[u8]) -> String { let mut h = Sha256::new(); h.update(b); hex(h) }

    /// A Mac has no pinned runtime: Local is off there with a note, and Cloud is the way.
    #[test]
    fn local_speech_is_off_on_a_mac_with_a_note() {
        assert_eq!(local_note().is_some(), cfg!(target_os = "macos"));
        assert_eq!(crate::mac::notes::LOCAL_SPEECH, "Local speech isn’t available on macOS yet; use Cloud (Groq).");
        if cfg!(target_os = "macos") {
            assert_eq!(unsupported().as_deref(), Some(crate::mac::notes::LOCAL_SPEECH));
            assert!(triple().is_none());
        }
    }

    /// Serves fixed files over plain HTTP; `slow` sleeps between 16 KB chunks.
    struct Server { port: u16, hits: Arc<AtomicU64> }

    fn serve(files: HashMap<String, Vec<u8>>, slow: bool) -> Server {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        let hits = Arc::new(AtomicU64::new(0));
        let (files, h) = (Arc::new(files), hits.clone());
        std::thread::spawn(move || for s in l.incoming() {
            let (Ok(mut s), files, h) = (s, files.clone(), h.clone()) else { continue };
            std::thread::spawn(move || {
                let mut buf = [0u8; 4096];
                let n = s.read(&mut buf).unwrap_or(0);
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let path = req.split_whitespace().nth(1).unwrap_or("").to_string();
                h.fetch_add(1, Ordering::Relaxed);
                let Some(body) = files.get(&path) else {
                    let _ = s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                    return;
                };
                let _ = s.write_all(format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes());
                for c in body.chunks(16 << 10) {
                    if s.write_all(c).is_err() { return; }
                    if slow { std::thread::sleep(Duration::from_millis(50)); }
                }
            });
        });
        Server { port, hits }
    }

    /// A small fake install: a runtime .tar.gz with an interpreter file in it, two wheels,
    /// the archive and a NOTICE. The fake Python writes the model the pins expect.
    fn fixture(big_wheel: usize) -> (HashMap<String, Vec<u8>>, impl Fn(u16) -> Pins) {
        let mut tgz = vec![];
        {
            let gz = flate2::write::GzEncoder::new(&mut tgz, flate2::Compression::fast());
            let mut b = tar::Builder::new(gz);
            let rel = if cfg!(windows) { "python/python.exe" } else { "python/bin/python3" };
            let mut h = tar::Header::new_gnu();
            h.set_size(2);
            h.set_mode(0o755);
            h.set_cksum();
            b.append_data(&mut h, rel, &b"py"[..]).unwrap();
            b.into_inner().unwrap().finish().unwrap();
        }
        let files: Vec<(&str, Vec<u8>)> = vec![
            ("/py.tar.gz", tgz), ("/a-1-py3-none-any.whl", vec![7u8; big_wheel]), ("/b-2-py3-none-any.whl", b"wheel b".to_vec()),
            ("/m.bps.tar.zst", b"archive".to_vec()), ("/NOTICE", b"notice".to_vec()),
        ];
        let map: HashMap<String, Vec<u8>> = files.iter().cloned().map(|(k, v)| (k.to_string(), v)).collect();
        let pins = move |port: u16| {
            let dl = |name: &str, path: &str| {
                let b = &files.iter().find(|f| f.0 == path).unwrap().1;
                Dl { name: name.into(), url: format!("http://127.0.0.1:{port}{path}"), size: b.len() as u64, sha256: sha(b) }
            };
            Pins {
                triple: "test", python: dl("py.tar.gz", "/py.tar.gz"),
                wheels: vec![("a".into(), "1".into(), dl("wheels/a-1-py3-none-any.whl", "/a-1-py3-none-any.whl")),
                             ("b".into(), "2".into(), dl("wheels/b-2-py3-none-any.whl", "/b-2-py3-none-any.whl"))],
                repo: vec![dl("m.bps.tar.zst", "/m.bps.tar.zst"), dl("NOTICE", "/NOTICE")],
                model: MODEL_FAKE.iter().map(|(n, b)| (n.to_string(), b.len() as u64, sha(b))).collect(),
                disk: 1000,
            }
        };
        (map, pins)
    }

    const MODEL_FAKE: [(&str, &[u8]); 3] = [("config.json", b"{}"), ("model.fermion", b"MODEL"), ("packed_manifest.json", b"{\"m\":1}")];

    /// What the managed Python does, minus Python: pip and the unpacker succeed (the
    /// unpacker writes the model), and transcribe answers `heard`.
    fn fake(heard: Arc<Mutex<String>>) -> Fake {
        Box::new(move |_home, args| {
            let a: Vec<String> = args.iter().map(|a| a.to_string_lossy().into_owned()).collect();
            if a.get(2).is_some_and(|x| x == "pip") { return Ok(String::new()); }
            if a.get(2).is_some_and(|x| x == UNPACK) {
                let dest = PathBuf::from(&a[4]);
                std::fs::create_dir_all(&dest).unwrap();
                for (n, b) in MODEL_FAKE { std::fs::write(dest.join(n), b).unwrap(); }
                return Ok("3".into());
            }
            assert_eq!(a[3], "transcribe");
            Ok(serde_json::json!({"text": *heard.lock().unwrap(), "duration_seconds": 3.845, "truncated": false}).to_string())
        })
    }

    struct T { dir: PathBuf, settings: Arc<Settings>, heard: Arc<Mutex<String>> }

    fn setup(name: &str) -> T {
        let dir = std::env::temp_dir().join(format!("hover-phonon-{name}-{}", hover_core::guid_n()));
        std::fs::create_dir_all(&dir).unwrap();
        T { settings: Settings::load(dir.join("settings.json")), dir, heard: Arc::new(Mutex::new(SAMPLE_TEXT.into())) }
    }

    impl T {
        fn phonon(&self, pins: Pins) -> Arc<Phonon> { Phonon::build(self.settings.clone(), self.dir.join("phonon"), pins, Some(fake(self.heard.clone()))) }
    }

    impl Drop for T {
        fn drop(&mut self) { let _ = std::fs::remove_dir_all(&self.dir); }
    }

    fn wait(p: &Phonon, ok: impl Fn(&Install) -> bool) -> Install {
        let t = Instant::now();
        loop {
            let s = p.state();
            if ok(&s) { return s; }
            assert!(t.elapsed() < Duration::from_secs(30), "stuck at {s:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    fn settled(p: &Phonon) -> Install {
        let s = wait(p, |s| !matches!(s, Install::Downloading { .. } | Install::Verifying | Install::Installing));
        drop(p.setup.lock().unwrap()); // the setup thread has cleaned up
        s
    }

    /// The folders under installs/.
    fn installs(root: &Path) -> usize { std::fs::read_dir(root.join("installs")).map_or(0, |r| r.count()) }

    /// Remove deletes on a worker.
    fn removed(root: &Path) {
        let t = Instant::now();
        while root.exists() { assert!(t.elapsed() < Duration::from_secs(10)); std::thread::sleep(Duration::from_millis(20)); }
    }

    #[test]
    fn installs_checks_survives_a_restart_and_removes() {
        let t = setup("install");
        let (files, pins) = fixture(10);
        let srv = serve(files, false);
        let p = t.phonon(pins(srv.port));
        assert_eq!(p.state(), Install::NotInstalled);
        assert!(p.speech().is_none());
        let seen = Arc::new(AtomicU64::new(0));
        let s2 = seen.clone();
        p.on_change(move || { s2.fetch_add(1, Ordering::Relaxed); });
        p.download();
        assert_eq!(settled(&p), Install::Ready);
        assert!(seen.load(Ordering::Relaxed) >= 4, "Downloading, Verifying, Installing, Ready");
        let root = t.dir.join("phonon");
        let cur = p.current().unwrap();
        assert!(cur.join("ready.json").is_file() && cur.join("licenses/NOTICE").is_file());
        assert_eq!(installs(&root), 1);
        assert!(!root.join("downloads").exists());
        assert_eq!(t.settings.voice().local, Some(local_model(&cur)));

        // A restart reads the disk: Ready, and the engine works.
        drop(p);
        let p = t.phonon(pins(srv.port));
        assert_eq!(p.state(), Install::Ready);
        let wav = t.dir.join("a.wav");
        std::fs::write(&wav, SAMPLE).unwrap();
        let got = p.speech().unwrap().transcribe(&wav, &AtomicBool::new(false)).unwrap();
        assert_eq!((got.text.as_str(), got.truncated, got.audio), (SAMPLE_TEXT, false, Some(Duration::from_secs_f64(3.845))));
        assert_eq!(p.speech().unwrap().transcribe(&wav, &AtomicBool::new(true)), Err(SpeechError::Cancelled));

        // A damaged install says Repair, and the engine says so too.
        std::fs::remove_file(cur.join("model").join(MODEL_DIR).join("model.fermion")).unwrap();
        assert!(p.speech().is_none());
        assert_eq!(t.phonon(pins(srv.port)).state(), Install::Failed(DAMAGED.into()));

        p.remove().unwrap();
        assert_eq!(p.state(), Install::NotInstalled);
        assert!(p.current().is_none() && p.speech().is_none());
        assert_eq!(t.settings.voice().local, None);
        removed(&root);
    }

    #[test]
    fn a_hash_mismatch_fails_and_installs_nothing() {
        let t = setup("hash");
        let (mut files, pins) = fixture(10);
        files.insert("/b-2-py3-none-any.whl".into(), b"wheel B".to_vec());
        let srv = serve(files, false);
        let p = t.phonon(pins(srv.port));
        p.download();
        let s = settled(&p);
        assert!(matches!(&s, Install::Failed(m) if m.contains("checksum")), "{s:?}");
        let root = t.dir.join("phonon");
        assert!(p.current().is_none());
        assert_eq!(installs(&root), 0);
        assert!(!root.join("downloads/wheels/b-2-py3-none-any.whl.part").exists());
        assert!(p.speech().is_none());
        assert_eq!(t.settings.voice().local, None);
    }

    #[test]
    fn cancel_stops_the_download_and_leaves_nothing_half_done() {
        let t = setup("cancel");
        let (files, pins) = fixture(2 << 20);
        let srv = serve(files, true);
        let p = t.phonon(pins(srv.port));
        p.download();
        wait(&p, |s| matches!(s, Install::Downloading { done, .. } if *done > 64 << 10));
        p.cancel();
        assert_eq!(p.state(), Install::Cancelled);
        assert_eq!(settled(&p), Install::Cancelled);
        let root = t.dir.join("phonon");
        assert!(!root.join("downloads/wheels/a-1-py3-none-any.whl.part").exists());
        assert!(p.current().is_none());
        assert_eq!(installs(&root), 0);
        // Retry finishes, and reuses the runtime the first run had fully fetched.
        let before = srv.hits.load(Ordering::Relaxed);
        p.download();
        assert_eq!(settled(&p), Install::Ready);
        assert_eq!(srv.hits.load(Ordering::Relaxed) - before, 4, "everything but the runtime again");
    }

    #[test]
    fn an_interrupted_setup_reads_back_truthfully() {
        let t = setup("interrupt");
        let (files, pins) = fixture(10);
        let srv = serve(files, false);
        let root = t.dir.join("phonon");
        // A crash mid-setup with nothing installed: Not installed; the next run clears up.
        std::fs::create_dir_all(root.join("installs/0123abcd/python")).unwrap();
        std::fs::create_dir_all(root.join("downloads")).unwrap();
        std::fs::write(root.join("downloads/py.tar.gz.part"), b"half").unwrap();
        let p = t.phonon(pins(srv.port));
        assert_eq!(p.state(), Install::NotInstalled);
        p.download();
        assert_eq!(settled(&p), Install::Ready);
        assert!(!root.join("downloads").exists() && !root.join("installs/0123abcd").exists());
        // A crash mid-repair: the half-built folder doesn't count, the working one does.
        drop(p);
        std::fs::create_dir_all(root.join("installs/4567ef/python")).unwrap();
        let p = t.phonon(pins(srv.port));
        assert_eq!(p.state(), Install::Ready);
        assert!(p.speech().is_some());
        // `current` naming a half-built folder, or a name that isn't plain: Repair, or
        // nothing at all; never Ready.
        std::fs::write(root.join("current"), "4567ef").unwrap();
        assert_eq!(t.phonon(pins(srv.port)).state(), Install::Failed(DAMAGED.into()));
        std::fs::write(root.join("current"), "../x").unwrap();
        assert_eq!(t.phonon(pins(srv.port)).state(), Install::NotInstalled);
    }

    #[test]
    fn a_failed_repair_keeps_the_working_install() {
        let t = setup("repair");
        let (files, pins) = fixture(10);
        let srv = serve(files, false);
        let p = t.phonon(pins(srv.port));
        p.download();
        assert_eq!(settled(&p), Install::Ready);
        let first = p.current().unwrap();
        let marker = std::fs::read(first.join("ready.json")).unwrap();
        *t.heard.lock().unwrap() = "something else".into();
        p.repair();
        let s = settled(&p);
        assert!(matches!(&s, Install::Failed(m) if m.contains("something else")), "{s:?}");
        assert_eq!(p.current().as_ref(), Some(&first));
        assert_eq!(std::fs::read(first.join("ready.json")).unwrap(), marker);
        assert_eq!(installs(&t.dir.join("phonon")), 1, "the failed one is gone");
        assert!(p.speech().is_some(), "the old install still works");
        assert_eq!(t.settings.voice().local, Some(local_model(&first)));
        // A repair that passes moves `current` to the new folder and deletes the old one.
        *t.heard.lock().unwrap() = SAMPLE_TEXT.into();
        p.repair();
        assert_eq!(settled(&p), Install::Ready);
        let second = p.current().unwrap();
        assert!(second != first && !first.exists() && installs(&t.dir.join("phonon")) == 1);
        assert_eq!(t.settings.voice().local, Some(local_model(&second)));
        // Remove refuses while a transcription holds the files.
        let busy = p.busy.lock().unwrap();
        assert!(p.remove().is_err());
        drop(busy);
        p.remove().unwrap();
    }

    #[test]
    fn too_little_disk_fails_before_any_download() {
        let t = setup("disk");
        let (files, pins) = fixture(10);
        let srv = serve(files, false);
        let p = t.phonon(Pins { disk: u64::MAX / 4, ..pins(srv.port) });
        p.download();
        let s = settled(&p);
        assert!(matches!(&s, Install::Failed(m) if m.contains("free")), "{s:?}");
        assert_eq!(srv.hits.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn answers_and_failures_read_as_the_spec_says() {
        assert_eq!(parse(r#"{"text": " Hi there. ", "truncated": true, "duration_seconds": 2.5}"#).unwrap(), ("Hi there.".into(), true, Some(Duration::from_millis(2500))));
        assert!(matches!(parse(r#"{"text": "  "}"#), Err(SpeechError::Engine(m)) if m.contains("no words")));
        assert!(matches!(parse("Traceback"), Err(SpeechError::Engine(_))));
        assert_eq!(explain(1, "x\nOSError: [WinError 126] The specified module could not be found\n"), VC_MSG);
        assert_eq!(explain(2, "usage: fermion\nfermion: error: bad\n"), "Phonon stopped (exit 2): fermion: error: bad");
        assert_eq!(words("Open the notes-folder, NOW."), ["open", "the", "notes", "folder", "now"]);
        // Every platform's lock reads, and its files are named as pip expects.
        for (tr, _) in WHEELS {
            let p = Pins::official(tr);
            assert_eq!(p.wheels.len(), if tr.contains("windows") { 41 } else { 40 }, "{tr}");
            assert!(p.wheels.iter().all(|(_, _, d)| d.name.ends_with(".whl") && d.sha256.len() == 64 && d.size > 0));
            assert!(p.wheels.iter().any(|(n, v, _)| n == "fermion-research" && v == FERMION));
        }
        assert_eq!(&SAMPLE[..4], b"RIFF");
    }

    /// The real thing: downloads, installs and checks Phonon into a temp folder (or
    /// PHONON_LIVE_DIR), then transcribes the sample. Prints sizes and times.
    #[test]
    #[ignore]
    fn live_install() {
        let dir = std::env::var_os("PHONON_LIVE_DIR").map(PathBuf::from).unwrap_or_else(|| std::env::temp_dir().join("hover-phonon-live"));
        std::fs::create_dir_all(&dir).unwrap();
        let settings = Settings::load(dir.join("settings.json"));
        let p = Phonon::build(settings.clone(), dir.join("phonon"), Pins::official(triple().unwrap()), None);
        let facts = p.facts();
        println!("facts {facts:?}");
        let (t, peak, stop) = (Instant::now(), Arc::new(AtomicU64::new(0)), Arc::new(AtomicBool::new(false)));
        let (root, pk, st) = (dir.join("phonon"), peak.clone(), stop.clone());
        let sampler = std::thread::spawn(move || while !st.load(Ordering::Relaxed) {
            pk.fetch_max(du(&root), Ordering::Relaxed);
            std::thread::sleep(Duration::from_secs(1));
        });
        let last = Arc::new(Mutex::new(String::new()));
        let (p2, l2, t2) = (Arc::downgrade(&p), last.clone(), t);
        p.on_change(move || {
            let Some(p) = p2.upgrade() else { return };
            let s = format!("{:?}", p.state());
            let kind = s.split([' ', '(']).next().unwrap_or("").to_string();
            let mut l = l2.lock().unwrap();
            if *l != kind { println!("{:>7.1}s {s}", t2.elapsed().as_secs_f64()); *l = kind; }
        });
        if p.state() != Install::Ready { p.download(); }
        let t0 = Instant::now();
        loop {
            match p.state() {
                Install::Ready | Install::Failed(_) | Install::Cancelled | Install::Unsupported(_) if t0.elapsed() > Duration::from_millis(200) => break,
                _ => std::thread::sleep(Duration::from_millis(200)),
            }
            assert!(t0.elapsed() < Duration::from_secs(3600));
        }
        drop(p.setup.lock().unwrap());
        stop.store(true, Ordering::Relaxed);
        sampler.join().unwrap();
        println!("state {:?} after {:.1}s; installed {} B; peak {} B", p.state(), t.elapsed().as_secs_f64(), p.current().map_or(0, |c| du(&c)), peak.load(Ordering::Relaxed));
        assert_eq!(p.state(), Install::Ready);
        let wav = std::env::var_os("PHONON_WAV").map(PathBuf::from).unwrap_or_else(|| { let w = dir.join("check.wav"); std::fs::write(&w, SAMPLE).unwrap(); w });
        let got = p.speech().unwrap().transcribe(&wav, &AtomicBool::new(false)).unwrap();
        println!("heard in {:.1}s (audio {:?}, truncated {}): {}", got.took.as_secs_f64(), got.audio, got.truncated, got.text.chars().take(300).collect::<String>());
        // Cancel kills it promptly.
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_secs(3)); c2.store(true, Ordering::Relaxed); });
        let t1 = Instant::now();
        assert_eq!(p.speech().unwrap().transcribe(&wav, &cancel), Err(SpeechError::Cancelled));
        println!("cancelled after {:.1}s", t1.elapsed().as_secs_f64());
        assert_eq!(settings.voice().local.map(|l| l.id), Some(ID.to_string()));
    }

    fn du(p: &Path) -> u64 {
        let Ok(m) = std::fs::symlink_metadata(p) else { return 0 };
        if !m.is_dir() { return m.len(); }
        std::fs::read_dir(p).map(|rd| rd.flatten().map(|e| du(&e.path())).sum()).unwrap_or(0)
    }
}

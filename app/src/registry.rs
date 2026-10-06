//! The ACP Registry, fetched and installed on the user's say-so: the catalog is read when Settings → Agents
//! asks for it, and an entry is installed only by the button that names it. A binary entry is downloaded,
//! checked against the checksum the registry gave (when it gave one), unpacked into a folder of its own under
//! Hover's data folder and found by the program inside it; a package entry (npx, uvx) downloads nothing and
//! needs its runtime already there, which is shown before anything happens. Hover never installs Node.js or uv.
//!
//! Unpacking is the system's `tar` (which reads .tar.gz everywhere and .zip on Windows and macOS), given its
//! arguments one by one; what it made is checked to lie inside the folder before the program in it is used.

use hover_agents::cancel::Cancel;
use hover_agents::custom::{self, Dist, Entry, Plan};
use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub const URL: &str = "https://cdn.agentclientprotocol.com/registry/v1/latest/registry.json";
/// The catalog is a small file; one past this is not it.
const CATALOG_MAX: u64 = 8 << 20;
/// No agent archive is larger than this.
const ARCHIVE_MAX: u64 = 1 << 30;

fn agent() -> ureq::Agent {
    let cfg = ureq::Agent::config_builder();
    #[cfg(windows)]
    let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).root_certs(ureq::tls::RootCerts::PlatformVerifier).build());
    cfg.timeout_connect(Some(Duration::from_secs(30))).timeout_recv_response(Some(Duration::from_secs(60))).timeout_recv_body(Some(Duration::from_secs(1800)))
        .http_status_as_error(true).user_agent(format!("Hover/{}", env!("CARGO_PKG_VERSION"))).build().into()
}

/// The registry’s agents. Blocks: run it off the UI thread.
pub fn catalog(url: &str) -> Result<Vec<Entry>, String> {
    let resp = agent().get(url).call().map_err(|e| format!("Couldn’t reach the registry: {e}"))?;
    let mut text = String::new();
    resp.into_body().into_reader().take(CATALOG_MAX).read_to_string(&mut text).map_err(|e| format!("The registry couldn’t be read: {e}"))?;
    custom::parse_registry(&text)
}

fn hex(bytes: &[u8]) -> String { bytes.iter().map(|b| format!("{b:02x}")).collect() }

/// What installing left: the program and its arguments, ready to be a record.
#[derive(Clone, Debug, PartialEq)]
pub struct Installed { pub exe: String, pub args: Vec<String>, pub env: Vec<(String, String)>, pub dir: Option<PathBuf> }

/// Installs the plan. `root` is the folder agents go under. Cancelling leaves nothing behind. Blocks.
pub fn install(plan: &Plan, root: &Path, cancel: &Cancel) -> Result<Installed, String> {
    match &plan.dist {
        Dist::Package { .. } => {
            if let Some(n) = plan.blockers().first() { return Err(format!("{} is needed first. {}", n.name, n.hint)); }
            let (exe, args, env) = custom::command_of(&plan.dist, root);
            Ok(Installed { exe, args, env, dir: None })
        }
        Dist::Binary { archive, sha256, .. } => {
            let dir = root.join(format!("{}-{}", plan.entry, plan.version));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).map_err(|e| format!("Couldn’t make {}: {e}", dir.display()))?;
            let r = (|| -> Result<(), String> {
                let file = dir.join("download.part");
                let got = download(archive, &file, cancel)?;
                match sha256 {
                    Some(want) if *want != got => return Err("The download doesn’t match the checksum the registry gave, so it was thrown away.".into()),
                    Some(_) => {}
                    None => hover_core::log::line(&format!("registry: {} has no checksum to check", plan.entry)),
                }
                let ext = if archive.ends_with(".zip") { "zip" } else { "tar.gz" };
                let named = dir.join(format!("download.{ext}"));
                std::fs::rename(&file, &named).map_err(|e| e.to_string())?;
                unpack(&named, &dir)?;
                let _ = std::fs::remove_file(&named);
                Ok(())
            })();
            if let Err(e) = r { let _ = std::fs::remove_dir_all(&dir); return Err(e); }
            let (exe, args, env) = custom::command_of(&plan.dist, &dir);
            // The program is inside the folder it was unpacked into, links followed, and runs.
            let inside = std::fs::canonicalize(&exe).ok().filter(|p| std::fs::canonicalize(&dir).is_ok_and(|d| p.starts_with(d)));
            let Some(real) = inside.filter(|p| p.is_file()) else { let _ = std::fs::remove_dir_all(&dir); return Err("The program named in the registry isn’t in the download.".into()) };
            #[cfg(unix)]
            { use std::os::unix::fs::PermissionsExt; let _ = std::fs::set_permissions(&real, std::fs::Permissions::from_mode(0o755)); }
            Ok(Installed { exe, args, env, dir: Some(dir) })
        }
    }
}

/// The archive into `dst`, hashed as it comes: its SHA-256 in lower-case hex.
fn download(url: &str, dst: &Path, cancel: &Cancel) -> Result<String, String> {
    if !(url.starts_with("https://") || url.starts_with("http://127.0.0.1")) { return Err("The registry’s download isn’t an https address, so Hover won’t use it.".into()); }
    let resp = agent().get(url).call().map_err(|e| format!("Couldn’t download it: {e}"))?;
    let mut body = resp.into_body().into_reader().take(ARCHIVE_MAX + 1);
    let mut out = std::io::BufWriter::new(std::fs::File::create(dst).map_err(|e| e.to_string())?);
    let (mut h, mut n, mut buf) = (Sha256::new(), 0u64, vec![0u8; 256 << 10]);
    loop {
        if cancel.is_cancelled() { return Err("Cancelled.".into()); }
        let k = body.read(&mut buf).map_err(|e| format!("The download broke off: {e}"))?;
        if k == 0 { break; }
        n += k as u64;
        if n > ARCHIVE_MAX { return Err("The download is larger than any agent should be, so it was stopped.".into()); }
        h.update(&buf[..k]);
        std::io::Write::write_all(&mut out, &buf[..k]).map_err(|e| e.to_string())?;
    }
    std::io::Write::flush(&mut out).map_err(|e| e.to_string())?;
    Ok(hex(&h.finalize()))
}

fn unpack(archive: &Path, into: &Path) -> Result<(), String> {
    let o = std::process::Command::new("tar").arg("-xf").arg(archive).arg("-C").arg(into).stdin(std::process::Stdio::null()).output()
        .map_err(|e| format!("Couldn’t unpack it (the system’s tar didn’t start): {e}"))?;
    if o.status.success() { Ok(()) } else { Err(format!("Couldn’t unpack it: {}", String::from_utf8_lossy(&o.stderr).lines().next().unwrap_or("tar failed").trim())) }
}

/// Installs an entry and keeps it as an agent: the one button’s whole job. Returns the agent’s id.
pub fn add(store: &custom::Store, e: &Entry, root: &Path, cancel: &Cancel) -> Result<String, String> {
    let have = |c: &str| hover_agents::proc::on_path(c).is_some();
    let plan = custom::plan(e, &custom::target(), &have)?;
    let done = install(&plan, root, cancel)?;
    let source = custom::Source::Registry { id: e.id.clone(), version: e.version.clone(), kind: plan.kind.clone() };
    let env = done.env.iter().map(|(k, v)| custom::EnvInput { name: k.clone(), value: v.clone(), secret: false }).collect();
    store.add(&e.name, &done.exe, done.args, env, source).inspect_err(|_| { if let Some(d) = &done.dir { let _ = std::fs::remove_dir_all(d); } })
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::io::Write;
    use std::net::TcpListener;

    fn temp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("hover-registry-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        std::fs::canonicalize(&d).unwrap()
    }

    /// Serves `body` to one request on a free loopback port; its address.
    fn serve(body: Vec<u8>) -> String {
        let l = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = l.local_addr().unwrap();
        std::thread::spawn(move || {
            if let Ok((mut s, _)) = l.accept() {
                let mut req = [0u8; 2048];
                let _ = s.read(&mut req);
                let _ = write!(s, "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len());
                let _ = s.write_all(&body);
            }
        });
        format!("http://{addr}/a.tar.gz")
    }

    /// A .tar.gz with `bin/agent` (a script) and a link that leaves the folder.
    fn archive(dir: &Path, with_program: bool) -> Vec<u8> {
        let src = dir.join("src");
        let _ = std::fs::remove_dir_all(&src);
        std::fs::create_dir_all(src.join("bin")).unwrap();
        if with_program { std::fs::write(src.join("bin/agent"), "#!/bin/sh\necho hi\n").unwrap(); } else { std::fs::write(src.join("bin/other"), "x").unwrap(); }
        let out = dir.join("a.tar.gz");
        assert!(std::process::Command::new("tar").args(["-czf"]).arg(&out).arg("-C").arg(&src).arg(".").status().unwrap().success());
        std::fs::read(out).unwrap()
    }

    fn plan_for(url: &str, sha: Option<String>, program: &str) -> Plan {
        let e = Entry { id: "demo".into(), name: "Demo".into(), version: "1.0.0".into(), description: String::new(), license: None, repository: None,
            binary: vec![("linux-x86_64".into(), Dist::Binary { archive: url.into(), cmd: program.into(), args: vec!["--acp".into()], env: vec![], sha256: sha })], packages: vec![] };
        custom::plan(&e, "linux-x86_64", &|_| false).unwrap()
    }

    #[test]
    fn a_binary_entry_is_downloaded_checked_unpacked_and_found() {
        let d = temp("ok");
        let bytes = archive(&d, true);
        let sha = hex(&Sha256::digest(&bytes));
        let url = serve(bytes);
        // ureq reads only https or loopback http here; the loopback one is for this test.
        let root = d.join("agents");
        let done = install(&plan_for(&url, Some(sha), "./bin/agent"), &root, &Cancel::new()).unwrap();
        assert!(done.exe.ends_with("demo-1.0.0/bin/agent") && std::path::Path::new(&done.exe).is_file());
        assert_eq!(done.args, ["--acp"]);
        assert!(!root.join("demo-1.0.0/download.tar.gz").exists(), "the archive is not kept");
    }

    #[test]
    fn a_wrong_checksum_a_missing_program_and_a_cancel_leave_nothing_behind() {
        let d = temp("bad");
        let bytes = archive(&d, true);
        let root = d.join("agents");
        let e = install(&plan_for(&serve(bytes.clone()), Some("0".repeat(64)), "./bin/agent"), &root, &Cancel::new()).unwrap_err();
        assert!(e.contains("doesn’t match the checksum"), "{e}");
        assert!(!root.join("demo-1.0.0").exists());
        let e = install(&plan_for(&serve(archive(&d, false)), None, "./bin/agent"), &root, &Cancel::new()).unwrap_err();
        assert!(e.contains("isn’t in the download"), "{e}");
        assert!(!root.join("demo-1.0.0").exists());
        let c = Cancel::new();
        c.cancel();
        assert_eq!(install(&plan_for(&serve(bytes), None, "./bin/agent"), &root, &c).unwrap_err(), "Cancelled.");
        assert!(!root.join("demo-1.0.0").exists());
        // Not https: refused before any request.
        assert!(install(&plan_for("http://example.com/a.tar.gz", None, "./bin/agent"), &root, &Cancel::new()).unwrap_err().contains("isn’t an https address"));
        // A program path that climbs out of the folder is not used.
        let e = install(&plan_for(&serve(archive(&d, true)), None, "../../../../bin/sh"), &root, &Cancel::new()).unwrap_err();
        assert!(e.contains("isn’t in the download"), "{e}");
    }

    #[test]
    fn a_package_entry_needs_its_runtime_and_downloads_nothing() {
        let e = Entry { id: "n".into(), name: "N".into(), version: "1".into(), description: String::new(), license: None, repository: None, binary: vec![],
            packages: vec![Dist::Package { runner: "npx".into(), package: "n@1".into(), args: vec![], env: vec![] }] };
        let plan = custom::plan(&e, "linux-x86_64", &|_| false).unwrap();
        let err = install(&plan, &temp("pkg"), &Cancel::new()).unwrap_err();
        assert!(err.contains("Node.js (npx) is needed first") && err.contains("doesn’t install it"), "{err}");
        let ok = install(&custom::plan(&e, "linux-x86_64", &|c| c == "npx").unwrap(), &temp("pkg2"), &Cancel::new()).unwrap();
        assert_eq!((ok.exe.as_str(), ok.args.as_slice(), ok.dir), ("npx", ["-y".to_owned(), "n@1".into()].as_slice(), None));
    }

    #[test]
    fn the_catalog_is_read_from_an_address() {
        let body = br#"{"version":"1.0.0","agents":[{"id":"a","name":"A","version":"1","description":"d","distribution":{"npx":{"package":"a"}}}]}"#.to_vec();
        let got = catalog(&serve(body)).unwrap();
        assert_eq!(got.len(), 1);
        assert!(catalog("http://127.0.0.1:1/none").unwrap_err().contains("Couldn’t reach the registry"));
    }
}

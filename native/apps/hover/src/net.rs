//! Where the chat's images come from, as WebView2 served them to 2.x's page (chat-proto's
//! net.rs, which this is, made lazy and forgetful for the app):
//! - `https://hover.images/<name>`: a prompt's pasted or attached image, in Hover's
//!   `kiro-images` folder (KiroPage.ImagesFolder);
//! - `https://f<key12>.hover/<path>`: a file in the session's own folder (FilesHost);
//! - any other http(s) address: the web.
//!
//! Everything is read on worker threads (the UI never waits), started the first time an
//! image is asked for: most chats have none. When the bytes (or the failure) are in, the
//! UI thread is told the URL. The bytes are handed over once and not kept: the chat's own
//! cache holds the decoded picture, and a chat opened again reads its images again.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use hover_chat::Fetch;

enum St {
    Pending,
    Done(Vec<u8>),
    Failed,
}

/// A web image larger than this is treated as broken (the page had no limit; the
/// drawer never shows more than a few hundred pixels of one).
const MAX_BYTES: u64 = 32 << 20;
/// Workers: images come a few at a time, and a slow web one shouldn't hold up the rest.
const WORKERS: usize = 2;

pub struct Net {
    state: Arc<Mutex<HashMap<String, St>>>,
    jobs: mpsc::Sender<String>,
}

type Hosts = Arc<Mutex<HashMap<String, PathBuf>>>;

/// The virtual hosts, kept apart from the loader: naming a session's folder host (every
/// chat laid out does) doesn't start the loader's threads; only an image asked for does.
fn hosts() -> &'static Hosts {
    static HOSTS: OnceLock<Hosts> = OnceLock::new();
    HOSTS.get_or_init(|| {
        let h: Hosts = Default::default();
        h.lock().unwrap().insert("hover.images".into(), hover_core::images::folder(hover_core::paths::support()));
        h
    })
}

/// The process's loader, made the first time an image is asked for.
fn net() -> &'static Net {
    static NET: OnceLock<Net> = OnceLock::new();
    NET.get_or_init(|| Net::new(hosts().clone(), |url| crate::ui_do(move |a| a.image_arrived(&url))))
}

/// A new cache for one chat, loading through the process's loader.
pub fn images() -> hover_chat::images::Shared {
    hover_chat::Images::new(Box::new(|u| net().fetch(u)))
}

/// The session's files host (`f<key12>.hover`), serving its folder, when the folder is
/// one a session may use.
pub fn files_host(key: &str, folder: &str) -> Option<String> {
    if !hover_agents::usable_folder(Some(folder)) { return None; }
    let host = format!("f{}.hover", key.get(..12)?.to_ascii_lowercase());
    hosts().lock().unwrap().insert(host.clone(), PathBuf::from(folder));
    Some(host)
}

impl Net {
    fn new(hosts: Hosts, arrived: impl Fn(String) + Send + Sync + 'static) -> Self {
        let state: Arc<Mutex<HashMap<String, St>>> = Default::default();
        let (jobs, rx) = mpsc::channel::<String>();
        let rx = Arc::new(Mutex::new(rx));
        let arrived = Arc::new(arrived);
        let cfg = ureq::Agent::config_builder();
        #[cfg(windows)]
        let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build());
        let agent: ureq::Agent = cfg.timeout_global(Some(Duration::from_secs(30))).http_status_as_error(true).build().into();
        for _ in 0..WORKERS {
            let (rx, state, hosts, arrived, agent) = (rx.clone(), state.clone(), hosts.clone(), arrived.clone(), agent.clone());
            let _ = std::thread::Builder::new().name("hover-images".into()).spawn(move || loop {
                let Ok(url) = rx.lock().unwrap().recv() else { return };
                let roots = hosts.lock().unwrap().clone();
                let got = load(&agent, &roots, &url);
                if let Err(e) = &got { hover_core::log::line(&format!("image {url}: {e}")); }
                state.lock().unwrap().insert(url.clone(), got.map_or(St::Failed, St::Done));
                arrived(url);
            });
        }
        Net { state, jobs }
    }

    /// What there is for a URL; the first ask starts its load. Bytes are given once.
    fn fetch(&self, url: &str) -> Fetch {
        let mut s = self.state.lock().unwrap();
        match s.remove(url) {
            Some(St::Done(b)) => Fetch::Bytes(b),
            Some(St::Failed) => Fetch::Failed,
            Some(St::Pending) => { s.insert(url.to_string(), St::Pending); Fetch::Pending }
            None => {
                s.insert(url.to_string(), St::Pending);
                let _ = self.jobs.send(url.to_string());
                Fetch::Pending
            }
        }
    }
}

fn load(agent: &ureq::Agent, hosts: &HashMap<String, PathBuf>, url: &str) -> Result<Vec<u8>, String> {
    let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://")).ok_or("not http(s)")?;
    let (host, path) = rest.split_once('/').unwrap_or((rest, ""));
    if let Some(root) = hosts.get(&host.to_ascii_lowercase()) {
        return std::fs::read(local(root, path)?).map_err(|e| e.to_string());
    }
    // A virtual host that isn't served (a session no longer open) never goes to the web.
    if host.eq_ignore_ascii_case("hover.images") || host.to_ascii_lowercase().ends_with(".hover") { return Err("not served".into()); }
    let mut r = agent.get(url).call().map_err(|e| e.to_string())?;
    let mut out = vec![];
    r.body_mut().as_reader().take(MAX_BYTES + 1).read_to_end(&mut out).map_err(|e| e.to_string())?;
    if out.len() as u64 > MAX_BYTES { return Err("too large".into()); }
    Ok(out)
}

/// A virtual host's path as a file under its folder: percent-decoded per segment, with
/// no way out of the folder (no `..`, and no link that resolves outside it).
fn local(root: &Path, path: &str) -> Result<PathBuf, String> {
    let path = path.split(['?', '#']).next().unwrap_or("");
    let mut p = root.to_path_buf();
    for seg in path.split('/').filter(|s| !s.is_empty()) {
        let seg = decode(seg).ok_or("bad escape")?;
        if seg == ".." || seg == "." || seg.contains(['/', '\\']) || seg.contains(':') { return Err("outside the folder".into()); }
        p.push(seg);
    }
    let (full, base) = (p.canonicalize().map_err(|e| e.to_string())?, root.canonicalize().map_err(|e| e.to_string())?);
    if !full.starts_with(&base) { return Err("outside the folder".into()); }
    Ok(full)
}

fn decode(s: &str) -> Option<String> {
    let b = s.as_bytes();
    let (mut out, mut i) = (vec![], 0);
    while i < b.len() {
        if b[i] == b'%' {
            out.push(u8::from_str_radix(s.get(i + 1..i + 3)?, 16).ok()?);
            i += 3;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A web image comes from a (local) server on a worker; a files host reads its folder
    /// and nothing outside it; the bytes are handed over once; an unserved virtual host
    /// never goes to the web.
    #[test]
    fn loads_from_the_web_and_the_virtual_hosts_once_each() {
        let srv = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = srv.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for s in srv.incoming().take(2) {
                let mut s = s.unwrap();
                let mut buf = [0u8; 1024];
                let n = s.read(&mut buf).unwrap();
                let req = String::from_utf8_lossy(&buf[..n]).to_string();
                let r = if req.starts_with("GET /ok.png") { "HTTP/1.1 200 OK\r\nContent-Length: 4\r\nConnection: close\r\n\r\nPNG!" }
                    else { "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n" };
                s.write_all(r.as_bytes()).unwrap();
            }
        });
        let dir = std::env::temp_dir().join(format!("hover-app-net-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/a b.png"), b"LOCAL").unwrap();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let hosts: Hosts = Default::default();
        hosts.lock().unwrap().insert("fabc123def456.hover".into(), dir.clone());
        let net = Net::new(hosts, move |u| { tx.lock().unwrap().send(u).unwrap(); });
        let urls = [format!("http://127.0.0.1:{port}/ok.png"), format!("http://127.0.0.1:{port}/missing.png"),
            "https://fabc123def456.hover/docs/a%20b.png".to_string(), "https://fabc123def456.hover/docs/%2e%2e/%2e%2e/etc/passwd".to_string(),
            "https://f000000000000.hover/x.png".to_string()];
        for u in &urls { assert!(matches!(net.fetch(u), Fetch::Pending)); }
        for _ in &urls { rx.recv_timeout(Duration::from_secs(10)).unwrap(); }
        assert!(matches!(net.fetch(&urls[0]), Fetch::Bytes(b) if b == b"PNG!"));
        assert!(matches!(net.fetch(&urls[1]), Fetch::Failed));
        assert!(matches!(net.fetch(&urls[2]), Fetch::Bytes(b) if b == b"LOCAL"));
        assert!(matches!(net.fetch(&urls[3]), Fetch::Failed));
        assert!(matches!(net.fetch(&urls[4]), Fetch::Failed));
        // Handed over: asked again, it is read again.
        assert!(matches!(net.fetch(&urls[2]), Fetch::Pending));
        rx.recv_timeout(Duration::from_secs(10)).unwrap();
        assert!(matches!(net.fetch(&urls[2]), Fetch::Bytes(b) if b == b"LOCAL"));
        let _ = std::fs::remove_dir_all(dir);
    }
}

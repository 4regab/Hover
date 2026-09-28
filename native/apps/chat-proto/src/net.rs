//! Where the chat's images come from, as WebView2 serves them to the page:
//! - `https://hover.images/<name>`: a prompt's pasted image, in Hover's `kiro-images`
//!   folder (KiroPage.ImagesFolder);
//! - `https://f<key12>.hover/<path>`: a file in the session's own folder (FilesHost);
//! - any other http(s) address: the web.
//!
//! Everything is read on four worker threads (the UI never waits), and `arrived` is
//! called on the UI thread with the URL when its bytes, or its failure, are in.

use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use hover_chat::Fetch;

enum St {
    Pending,
    Done(Arc<Vec<u8>>),
    Failed,
}

/// A web image larger than this is treated as broken (the page has no limit; the
/// drawer never shows more than a few hundred pixels of one).
const MAX_BYTES: u64 = 32 << 20;

pub struct Net {
    state: Arc<Mutex<HashMap<String, St>>>,
    jobs: mpsc::Sender<String>,
    /// Virtual host name to folder.
    pub hosts: Arc<Mutex<HashMap<String, PathBuf>>>,
}

impl Net {
    pub fn new(arrived: impl Fn(String) + Send + Sync + 'static) -> Self {
        let state: Arc<Mutex<HashMap<String, St>>> = Default::default();
        let hosts: Arc<Mutex<HashMap<String, PathBuf>>> = Default::default();
        let (jobs, rx) = mpsc::channel::<String>();
        let rx = Arc::new(Mutex::new(rx));
        let arrived = Arc::new(arrived);
        let cfg = ureq::Agent::config_builder();
        #[cfg(windows)]
        let cfg = cfg.tls_config(ureq::tls::TlsConfig::builder().provider(ureq::tls::TlsProvider::NativeTls).build());
        let agent: ureq::Agent = cfg
            .timeout_global(Some(Duration::from_secs(30)))
            .http_status_as_error(true)
            .build()
            .into();
        for _ in 0..4 {
            let (rx, state, hosts, arrived, agent) = (rx.clone(), state.clone(), hosts.clone(), arrived.clone(), agent.clone());
            std::thread::spawn(move || loop {
                let Ok(url) = rx.lock().unwrap().recv() else { return };
                let got = load(&agent, &hosts.lock().unwrap().clone(), &url);
                if let Err(e) = &got { eprintln!("image {url}: {e}"); }
                state.lock().unwrap().insert(url.clone(), got.map_or(St::Failed, |b| St::Done(Arc::new(b))));
                arrived(url);
            });
        }
        Net { state, jobs, hosts }
    }

    /// Whether a load is still running.
    pub fn busy(&self) -> bool {
        self.state.lock().unwrap().values().any(|s| matches!(s, St::Pending))
    }

    /// What there is for a URL; the first ask starts its load.
    pub fn fetch(&self, url: &str) -> Fetch {
        let mut s = self.state.lock().unwrap();
        match s.get(url) {
            Some(St::Done(b)) => Fetch::Bytes(b.to_vec()),
            Some(St::Failed) => Fetch::Failed,
            Some(St::Pending) => Fetch::Pending,
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

/// Hover's data folder (Paths.Support: HOVER_DATA_DIR, else %APPDATA%\\Hover or the XDG data folder).
pub fn support() -> Option<PathBuf> { Some(hover_core::paths::support().to_path_buf()) }

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// A web image comes from a (local) server on a worker; the session's files host
    /// reads its folder and nothing outside it.
    #[test]
    fn loads_from_the_web_and_the_virtual_hosts() {
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
        let dir = std::env::temp_dir().join(format!("hover-net-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("docs")).unwrap();
        std::fs::write(dir.join("docs/a b.png"), b"LOCAL").unwrap();
        let (tx, rx) = mpsc::channel();
        let tx = Mutex::new(tx);
        let net = Net::new(move |u| { tx.lock().unwrap().send(u).unwrap(); });
        net.hosts.lock().unwrap().insert("fabc123def456.hover".into(), dir.clone());
        let urls = [format!("http://127.0.0.1:{port}/ok.png"), format!("http://127.0.0.1:{port}/missing.png"),
            "https://fabc123def456.hover/docs/a%20b.png".to_string(), "https://fabc123def456.hover/docs/%2e%2e/%2e%2e/etc/passwd".to_string()];
        for u in &urls { assert!(matches!(net.fetch(u), Fetch::Pending)); }
        for _ in &urls { rx.recv_timeout(Duration::from_secs(10)).unwrap(); }
        assert!(matches!(net.fetch(&urls[0]), Fetch::Bytes(b) if b == b"PNG!"));
        assert!(matches!(net.fetch(&urls[1]), Fetch::Failed));
        assert!(matches!(net.fetch(&urls[2]), Fetch::Bytes(b) if b == b"LOCAL"));
        assert!(matches!(net.fetch(&urls[3]), Fetch::Failed));
        let _ = std::fs::remove_dir_all(dir);
    }
}

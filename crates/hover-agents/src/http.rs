//! Plain HTTP/1.1 to OpenCode's own server on this PC (what OpenCodeHost's HttpClient
//! does): one connection per request, Basic auth, a deadline, and a stop that closes the
//! socket. Only loopback is ever spoken to, so there is no TLS, and no crate is needed.
//! The event stream is the same request with its body read line by line.

use crate::cancel::{Cancel, Registration};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpStream, ToSocketAddrs};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, PartialEq)]
pub enum HttpErr {
    /// Connecting, reading or writing failed (HttpRequestException).
    Io(String),
    /// The deadline passed.
    Timeout,
    /// The stop closed the socket.
    Cancelled,
}

/// Where the server listens and how to sign in to it, and any headers of its own (an MCP
/// server's token and session).
#[derive(Clone)]
pub struct Client { host: String, port: u16, auth: String, extra: Vec<(String, String)> }

/// Uri.EscapeDataString: everything but the unreserved characters, as %XX of UTF-8.
pub fn escape_data(s: &str) -> String {
    let mut o = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') { o.push(b as char) } else { o.push_str(&format!("%{b:02X}")) }
    }
    o
}

/// Convert.ToBase64String.
pub fn base64(b: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut o = String::new();
    for c in b.chunks(3) {
        let n = (c[0] as u32) << 16 | (*c.get(1).unwrap_or(&0) as u32) << 8 | *c.get(2).unwrap_or(&0) as u32;
        for i in 0..4 {
            if i <= c.len() { o.push(T[(n >> (18 - 6 * i) & 63) as usize] as char) } else { o.push('=') }
        }
    }
    o
}

/// The host of an http:// URL, and its port (80 when none is named).
pub fn host_port(url: &str) -> Option<(String, u16)> {
    let rest = url.strip_prefix("http://")?;
    let auth = rest.split(['/', '?', '#']).next()?;
    let (host, port) = if let Some(h) = auth.strip_prefix('[') {
        let (h, p) = h.split_once(']')?;
        (h.to_owned(), p.strip_prefix(':').map_or(Some(80), |p| p.parse().ok())?)
    } else {
        match auth.rsplit_once(':') { Some((h, p)) => (h.to_owned(), p.parse().ok()?), None => (auth.to_owned(), 80) }
    };
    (!host.is_empty()).then_some((host, port))
}

/// Uri.IsLoopback for a host name.
pub fn is_loopback(host: &str) -> bool {
    host.eq_ignore_ascii_case("localhost") || host.parse::<std::net::IpAddr>().is_ok_and(|a| a.is_loopback())
}

/// A socket read with a deadline, and stopped by closing it.
struct Timed { s: TcpStream, deadline: Option<Instant>, stopped: Arc<AtomicBool> }

impl Read for Timed {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        // On Windows the stop's shutdown doesn't wake a recv that is already waiting, so
        // Stop hung until the server next sent something. Wait for data in short peeks
        // instead, looking at the stop between them. A peek that times out takes nothing,
        // so no byte is lost (a timed-out recv can lose one on Windows).
        // ponytail: wakes every 100 ms per open request; few are open at once.
        #[cfg(windows)]
        loop {
            if self.stopped.load(Ordering::SeqCst) { return Err(std::io::ErrorKind::ConnectionAborted.into()); }
            let tick = Duration::from_millis(100);
            let wait = match self.deadline {
                Some(d) => {
                    let left = d.saturating_duration_since(Instant::now());
                    if left.is_zero() { return Err(std::io::ErrorKind::TimedOut.into()); }
                    left.min(tick)
                }
                None => tick,
            };
            self.s.set_read_timeout(Some(wait))?;
            match self.s.peek(&mut [0u8; 1]) {
                Ok(_) => { if self.deadline.is_none() { self.s.set_read_timeout(None)?; } break; }
                Err(e) if matches!(e.kind(), std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock) => {}
                Err(e) => return Err(e),
            }
        }
        if let Some(d) = self.deadline {
            let left = d.saturating_duration_since(Instant::now());
            if left.is_zero() { return Err(std::io::ErrorKind::TimedOut.into()); }
            self.s.set_read_timeout(Some(left))?;
        }
        self.s.read(buf)
    }
}

enum Mode { Length(u64), Chunked { left: u64, done: bool }, Eof }

/// A response's body: by its length, in chunks, or to the end.
pub struct Body { r: BufReader<Timed>, mode: Mode }

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let eof = || std::io::Error::from(std::io::ErrorKind::UnexpectedEof);
        match &mut self.mode {
            Mode::Length(0) => Ok(0),
            Mode::Length(n) => {
                let m = (buf.len() as u64).min(*n) as usize;
                let k = self.r.read(&mut buf[..m])?;
                if k == 0 { return Err(eof()); }
                *n -= k as u64;
                Ok(k)
            }
            Mode::Chunked { left, done } => {
                if *done { return Ok(0); }
                if *left == 0 {
                    let mut line = String::new();
                    if self.r.read_line(&mut line)? == 0 { return Err(eof()); }
                    let size = u64::from_str_radix(line.trim().split(';').next().unwrap_or("").trim(), 16)
                        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?;
                    if size == 0 {
                        loop {
                            let mut t = String::new();
                            if self.r.read_line(&mut t)? == 0 || t.trim().is_empty() { break; }
                        }
                        *done = true;
                        return Ok(0);
                    }
                    *left = size;
                }
                let m = (buf.len() as u64).min(*left) as usize;
                let k = self.r.read(&mut buf[..m])?;
                if k == 0 { return Err(eof()); }
                *left -= k as u64;
                if *left == 0 { let mut crlf = [0u8; 2]; self.r.read_exact(&mut crlf)?; }
                Ok(k)
            }
            Mode::Eof => self.r.read(buf),
        }
    }
}

/// An open response: its status and its body, and the stop's hold on the socket.
pub struct Response { pub status: u16, pub body: BufReader<Body>, headers: Vec<(String, String)>, stopped: Arc<AtomicBool>, _reg: Option<Registration> }

impl Response {
    /// A header of the answer by name (any case), as it came.
    pub fn header(&self, name: &str) -> Option<&str> { self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str()) }

    /// The whole body as text (bad UTF-8 replaced).
    pub fn text(mut self) -> Result<(u16, String), HttpErr> {
        let mut b = Vec::new();
        match self.body.read_to_end(&mut b) {
            Ok(_) if self.stopped.load(Ordering::SeqCst) => Err(HttpErr::Cancelled),
            Ok(_) => Ok((self.status, String::from_utf8_lossy(&b).into_owned())),
            Err(e) => Err(self.why(e)),
        }
    }

    /// The next line of the body without its end, or none at its end.
    pub fn line(&mut self) -> Result<Option<String>, HttpErr> {
        let mut b = Vec::new();
        match self.body.read_until(b'\n', &mut b) {
            Ok(0) if self.stopped.load(Ordering::SeqCst) => Err(HttpErr::Cancelled),
            Ok(0) => Ok(None),
            Ok(_) => {
                while b.last().is_some_and(|c| *c == b'\n' || *c == b'\r') { b.pop(); }
                Ok(Some(String::from_utf8_lossy(&b).into_owned()))
            }
            Err(e) => Err(self.why(e)),
        }
    }

    fn why(&self, e: std::io::Error) -> HttpErr { why(&self.stopped, e) }
}

fn why(stopped: &AtomicBool, e: std::io::Error) -> HttpErr {
    if stopped.load(Ordering::SeqCst) { return HttpErr::Cancelled; }
    match e.kind() {
        std::io::ErrorKind::TimedOut | std::io::ErrorKind::WouldBlock => HttpErr::Timeout,
        _ => HttpErr::Io(e.to_string()),
    }
}

impl Client {
    /// The server at an http:// URL, signed in as `user` with `password`.
    pub fn new(url: &str, user: &str, password: &str) -> Option<Client> {
        let (host, port) = host_port(url)?;
        Some(Client { host, port, auth: format!("Basic {}", base64(format!("{user}:{password}").as_bytes())), extra: vec![] })
    }

    /// The server at an http:// URL with no sign-in of Hover's (its headers say who asks).
    pub fn bare(url: &str) -> Option<Client> {
        let (host, port) = host_port(url)?;
        Some(Client { host, port, auth: String::new(), extra: vec![] })
    }

    /// The same server, with one more header on every request (one named Accept replaces
    /// the default). Names and values with a line break are left out.
    pub fn with_header(mut self, name: &str, value: &str) -> Client {
        if ![name, value].iter().any(|s| s.contains(['\r', '\n'])) { self.extra.push((name.to_owned(), value.to_owned())); }
        self
    }

    /// Sends a request and reads the answer's head; the body is read from the response.
    /// Past `timeout` (none: no limit) it gives up; `stop` closes the socket.
    pub fn open(&self, method: &str, target: &str, body: Option<&str>, timeout: Option<Duration>, stop: Option<&Cancel>) -> Result<Response, HttpErr> {
        let deadline = timeout.map(|t| Instant::now() + t);
        if stop.is_some_and(Cancel::is_cancelled) { return Err(HttpErr::Cancelled); }
        let addr = (self.host.as_str(), self.port).to_socket_addrs().map_err(|e| HttpErr::Io(e.to_string()))?.next()
            .ok_or_else(|| HttpErr::Io(format!("no address for {}", self.host)))?;
        let s = match deadline {
            Some(d) => TcpStream::connect_timeout(&addr, d.saturating_duration_since(Instant::now()).max(Duration::from_millis(1))),
            None => TcpStream::connect(addr),
        }.map_err(|e| if e.kind() == std::io::ErrorKind::TimedOut { HttpErr::Timeout } else { HttpErr::Io(e.to_string()) })?;
        let _ = s.set_nodelay(true);
        let stopped = Arc::new(AtomicBool::new(false));
        let reg = match stop {
            Some(c) => {
                let (sock, flag) = (s.try_clone().map_err(|e| HttpErr::Io(e.to_string()))?, stopped.clone());
                Some(c.on_cancel(move || { flag.store(true, Ordering::SeqCst); let _ = sock.shutdown(Shutdown::Both); }))
            }
            None => None,
        };
        let mut head = format!("{method} {target} HTTP/1.1\r\nHost: {}:{}\r\nConnection: close\r\n", self.host, self.port);
        if !self.auth.is_empty() { head.push_str(&format!("Authorization: {}\r\n", self.auth)); }
        if !self.extra.iter().any(|(k, _)| k.eq_ignore_ascii_case("accept")) { head.push_str("Accept: */*\r\n"); }
        for (k, v) in &self.extra { head.push_str(&format!("{k}: {v}\r\n")); }
        if let Some(b) = body { head.push_str(&format!("Content-Type: application/json; charset=utf-8\r\nContent-Length: {}\r\n", b.len())); }
        head.push_str("\r\n");
        let mut w = s.try_clone().map_err(|e| HttpErr::Io(e.to_string()))?;
        if let Some(d) = deadline { let _ = w.set_write_timeout(Some(d.saturating_duration_since(Instant::now()).max(Duration::from_millis(1)))); }
        let sent = w.write_all(head.as_bytes()).and_then(|_| w.write_all(body.unwrap_or("").as_bytes())).and_then(|_| w.flush());
        if let Err(e) = sent { return Err(why(&stopped, e)); }
        let mut r = BufReader::new(Timed { s, deadline, stopped: stopped.clone() });
        let mut status_line = String::new();
        match r.read_line(&mut status_line) {
            Ok(0) => return Err(if stopped.load(Ordering::SeqCst) { HttpErr::Cancelled } else { HttpErr::Io("the server closed the connection".into()) }),
            Ok(_) => {}
            Err(e) => return Err(why(&stopped, e)),
        }
        let status: u16 = status_line.split_whitespace().nth(1).and_then(|c| c.parse().ok()).ok_or_else(|| HttpErr::Io(format!("not HTTP: {}", status_line.trim())))?;
        let (mut length, mut chunked, mut headers) = (None, false, Vec::new());
        loop {
            let mut h = String::new();
            match r.read_line(&mut h) { Ok(0) => break, Ok(_) => {} Err(e) => return Err(why(&stopped, e)) }
            let h = h.trim_end();
            if h.is_empty() { break; }
            if let Some((k, v)) = h.split_once(':') {
                let (k, v) = (k.trim(), v.trim());
                if k.eq_ignore_ascii_case("content-length") { length = v.parse::<u64>().ok(); }
                if k.eq_ignore_ascii_case("transfer-encoding") && v.to_ascii_lowercase().contains("chunked") { chunked = true; }
                // Kept for the caller (a few short ones): an MCP server's session id.
                if headers.len() < 32 { headers.push((k.to_owned(), v.to_owned())); }
            }
        }
        let mode = if method == "HEAD" || status == 204 || status == 304 || (100..200).contains(&status) { Mode::Length(0) }
            else if chunked { Mode::Chunked { left: 0, done: false } }
            else { length.map_or(Mode::Eof, Mode::Length) };
        let stopped2 = r.get_ref().stopped.clone();
        Ok(Response { status, body: BufReader::new(Body { r, mode }), headers, stopped: stopped2, _reg: reg })
    }

    /// A whole request: the status and the body's text.
    pub fn call(&self, method: &str, target: &str, body: Option<&str>, timeout: Option<Duration>, stop: Option<&Cancel>) -> Result<(u16, String), HttpErr> {
        self.open(method, target, body, timeout, stop)?.text()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers_match_dotnet() {
        assert_eq!(base64(b"opencode:pw"), "b3BlbmNvZGU6cHc=");
        assert_eq!(base64(b"ab"), "YWI=");
        assert_eq!(escape_data("/tmp/hover oc ü"), "%2Ftmp%2Fhover%20oc%20%C3%BC");
        assert_eq!(host_port("http://127.0.0.1:4096"), Some(("127.0.0.1".into(), 4096)));
        assert_eq!(host_port("http://localhost:9/x"), Some(("localhost".into(), 9)));
        assert_eq!(host_port("http://[::1]:5/"), Some(("::1".into(), 5)));
        assert!(is_loopback("127.0.0.1") && is_loopback("::1") && is_loopback("LOCALHOST") && !is_loopback("0.0.0.0") && !is_loopback("10.0.0.2"));
    }

    #[test]
    fn chunked_and_sized_bodies_and_a_stop() {
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for (i, s) in l.incoming().enumerate() {
                let mut s = s.unwrap();
                let mut r = BufReader::new(s.try_clone().unwrap());
                loop { let mut h = String::new(); r.read_line(&mut h).unwrap(); if h.trim().is_empty() { break; } }
                match i {
                    0 => { s.write_all(b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n").unwrap(); }
                    1 => { s.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 2\r\n\r\n{}").unwrap(); }
                    _ => { s.write_all(b"HTTP/1.1 200 OK\r\n\r\ndata: a\n\n").unwrap(); std::thread::sleep(Duration::from_secs(5)); }
                }
            }
        });
        let c = Client::new(&format!("http://127.0.0.1:{port}"), "opencode", "pw").unwrap();
        assert_eq!(c.call("GET", "/a", None, Some(Duration::from_secs(5)), None).unwrap(), (200, "hello world".into()));
        assert_eq!(c.call("GET", "/b", None, Some(Duration::from_secs(5)), None).unwrap(), (404, "{}".into()));
        let stop = Cancel::new();
        let mut r = c.open("GET", "/event", None, None, Some(&stop)).unwrap();
        assert_eq!(r.line().unwrap().as_deref(), Some("data: a"));
        assert_eq!(r.line().unwrap().as_deref(), Some(""));
        let s2 = stop.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(100)); s2.cancel(); });
        let t = Instant::now();
        assert_eq!(r.line(), Err(HttpErr::Cancelled));
        assert!(t.elapsed() < Duration::from_secs(2));
    }
}

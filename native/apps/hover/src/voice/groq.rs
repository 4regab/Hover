//! Cloud speech: the recording to Groq's OpenAI-compatible transcription endpoint, with
//! the user's own key. No language is sent, so Groq detects it (and mixed speech stays
//! as said); verbose_json says which language it heard. The key is sent only as the
//! Authorization header and is never logged or put in an error.

use crate::speech::{Speech, SpeechError, Transcript};
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

pub const BASE: &str = "https://api.groq.com/openai/v1";
/// Groq's attachment limit (console.groq.com/docs/speech-to-text, checked 2026-10-01:
/// 25 MB on the free tier). Ten minutes of 16 kHz mono 16-bit is 19.2 MB.
pub const MAX_UPLOAD: u64 = 25_000_000;
/// Groq's answers are a few KB; more than this is not an answer.
const MAX_ANSWER: u64 = 1 << 20;

pub struct Groq {
    pub base: String,
    key: String,
    model: String,
    /// The whole request's time limit; None is a minute plus the upload at 100 KB/s.
    pub limit: Option<Duration>,
}

impl Groq {
    pub fn new(key: &str, model: &str) -> Groq { Groq { base: base(), key: key.into(), model: model.into(), limit: None } }
}

/// Groq's address, or for measuring against a fake Groq on this computer, HOVER_GROQ_BASE.
/// Only a plain-HTTP loopback address is taken (http://127.0.0.1:PORT/…), so the
/// variable can't send a recording or a key anywhere else; anything other is ignored.
fn base() -> String {
    std::env::var("HOVER_GROQ_BASE").ok()
        .filter(|b| b.strip_prefix("http://127.0.0.1:").and_then(|r| r.split('/').next()).is_some_and(|p| p.parse::<u16>().is_ok()))
        .map(|b| b.trim_end_matches('/').to_owned())
        .unwrap_or_else(|| BASE.into())
}

/// The key never shows in a message, whatever the server echoes.
fn scrub(s: &str, key: &str) -> String { if key.is_empty() { s.into() } else { s.replace(key, "…") } }

/// The server's own error text, when it gave one.
fn server_message(body: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(body).ok()?;
    v.pointer("/error/message").and_then(|m| m.as_str()).map(|m| m.chars().take(300).collect())
}

/// Reads stop with an error once `stop` is set, so a cancelled upload ends at once.
struct Stoppable<R> { r: R, stop: Arc<AtomicBool> }

impl<R: Read> Read for Stoppable<R> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.stop.load(Ordering::Relaxed) { return Err(std::io::Error::other("cancelled")); }
        self.r.read(buf)
    }
}

/// The file, opened so it can still be deleted while open (Windows): a cancelled
/// upload that hasn't let go yet never keeps the recording on disk.
fn open_shared(p: &Path) -> std::io::Result<std::fs::File> {
    let mut o = std::fs::OpenOptions::new();
    o.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
        o.share_mode(0x1 | 0x2 | 0x4);
    }
    o.open(p)
}

impl Speech for Groq {
    fn transcribe(&self, wav: &Path, cancel: &AtomicBool) -> Result<Transcript, SpeechError> {
        let size = std::fs::metadata(wav).map_err(|e| SpeechError::Engine(format!("The recording couldn’t be read: {e}")))?.len();
        if size > MAX_UPLOAD {
            return Err(SpeechError::Unsupported(format!("The recording is {:.1} MB; Groq takes up to 25 MB.", size as f64 / 1e6)));
        }
        let file = open_shared(wav).map_err(|e| SpeechError::Engine(format!("The recording couldn’t be read: {e}")))?;
        let (base, key, model) = (self.base.clone(), self.key.clone(), self.model.clone());
        let limit = self.limit.unwrap_or(Duration::from_secs(60 + size / 100_000));
        let stop = Arc::new(AtomicBool::new(false));
        let s2 = stop.clone();
        let started = Instant::now();
        let (tx, rx) = std::sync::mpsc::channel();
        // The request on a thread of its own, so a cancel returns now rather than when
        // the server answers; its late answer goes nowhere.
        std::thread::Builder::new().name("voice-groq".into()).spawn(move || { let _ = tx.send(post(&base, &key, &model, file, size, s2, limit)); })
            .map_err(|e| SpeechError::Network(e.to_string()))?;
        loop {
            if cancel.load(Ordering::Relaxed) { stop.store(true, Ordering::Relaxed); return Err(SpeechError::Cancelled); }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(r) => return r.map(|mut t| { t.took = started.elapsed(); t }),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err(SpeechError::Network("The request to Groq stopped.".into())),
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn post(base: &str, key: &str, model: &str, file: std::fs::File, size: u64, stop: Arc<AtomicBool>, limit: Duration) -> Result<Transcript, SpeechError> {
    let b = format!("hover{}", hover_core::guid_n());
    let field = |n: &str, v: &str| format!("--{b}\r\nContent-Disposition: form-data; name=\"{n}\"\r\n\r\n{v}\r\n");
    let head = format!("{}{}{}--{b}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"voice.wav\"\r\nContent-Type: audio/wav\r\n\r\n",
        field("model", model), field("response_format", "verbose_json"), field("temperature", "0"));
    let tail = format!("\r\n--{b}--\r\n");
    let len = head.len() as u64 + size + tail.len() as u64;
    let mut body = Stoppable { r: std::io::Cursor::new(head.into_bytes()).chain(file).chain(std::io::Cursor::new(tail.into_bytes())), stop };
    let r = super::agent(limit).post(format!("{base}/audio/transcriptions"))
        .header("Authorization", format!("Bearer {key}"))
        .header("Content-Type", format!("multipart/form-data; boundary={b}"))
        .header("Content-Length", len.to_string())
        .send(ureq::SendBody::from_reader(&mut body));
    let mut r = match r {
        Ok(r) => r,
        Err(ureq::Error::Timeout(_)) => return Err(SpeechError::Network("Groq didn’t answer in time.".into())),
        Err(e) if body.stop.load(Ordering::Relaxed) => { drop(e); return Err(SpeechError::Cancelled); }
        Err(e) => return Err(SpeechError::Network(scrub(&format!("Groq couldn’t be reached: {e}"), key))),
    };
    let status = r.status().as_u16();
    let retry = r.headers().get("retry-after").and_then(|v| v.to_str().ok()).and_then(|v| v.trim().parse::<u64>().ok());
    let text = r.body_mut().with_config().limit(MAX_ANSWER).read_to_string().unwrap_or_default();
    match status {
        200 => {}
        401 => return Err(SpeechError::BadKey),
        429 => return Err(SpeechError::RateLimited(retry)),
        413 => return Err(SpeechError::Unsupported("The recording is too large for Groq.".into())),
        400 | 415 | 422 => return Err(SpeechError::Unsupported(scrub(&server_message(&text).unwrap_or_else(|| format!("Groq couldn’t use the recording ({status}).")), key))),
        _ => return Err(SpeechError::Network(scrub(&format!("Groq had a problem ({status}){}. Try again.", server_message(&text).map(|m| format!(": {m}")).unwrap_or_default()), key))),
    }
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|_| SpeechError::Network("Groq’s answer couldn’t be read.".into()))?;
    let t = v.get("text").and_then(|t| t.as_str()).ok_or_else(|| SpeechError::Network("Groq’s answer had no text.".into()))?;
    Ok(Transcript {
        text: t.trim().to_owned(),
        language: v.get("language").and_then(|l| l.as_str()).map(str::to_owned),
        truncated: false,
        audio: v.get("duration").and_then(|d| d.as_f64()).filter(|d| d.is_finite() && *d >= 0.0).map(Duration::from_secs_f64),
        took: Duration::ZERO,
    })
}

/// Settings' check of a key: an authenticated list of models. Blocking.
pub fn check(base: &str, key: &str) -> Result<(), String> {
    let key = key.trim();
    if key.is_empty() { return Err("Enter a Groq key first.".into()); }
    let r = super::agent(Duration::from_secs(15)).get(format!("{base}/models")).header("Authorization", format!("Bearer {key}")).call();
    match r {
        Ok(r) if r.status().as_u16() == 200 => Ok(()),
        Ok(r) if r.status().as_u16() == 401 => Err("Groq didn’t accept the key.".into()),
        Ok(r) if r.status().as_u16() == 429 => Err("Groq’s limit was reached. Try again shortly.".into()),
        Ok(r) => Err(format!("Groq answered {}. Try again.", r.status().as_u16())),
        Err(ureq::Error::Timeout(_)) => Err("Groq didn’t answer in time.".into()),
        Err(e) => Err(scrub(&format!("Groq couldn’t be reached: {e}"), key)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::voice::fake;

    fn wav() -> crate::voice::wav::TempWav { crate::voice::wav::write(&[0i16; 1600], 16_000).unwrap() }

    #[test]
    fn groq_answers_become_transcripts_or_the_right_errors() {
        let (url, seen) = fake::serve(vec![
            fake::reply(200, "", r#"{"text":" Fix the notch blink. ","language":"english","duration":2.5}"#),
            fake::reply(401, "", r#"{"error":{"message":"Invalid API Key"}}"#),
            fake::reply(429, "Retry-After: 7\r\n", "{}"),
            fake::reply(500, "", r#"{"error":{"message":"boom"}}"#),
            fake::reply(413, "", "{}"),
        ]);
        let mut g = Groq::new("gsk_SECRET", "whisper-large-v3-turbo");
        g.base = url;
        let w = wav();
        let no = AtomicBool::new(false);
        let t = g.transcribe(w.path(), &no).unwrap();
        assert_eq!((t.text.as_str(), t.language.as_deref(), t.audio), ("Fix the notch blink.", Some("english"), Some(Duration::from_millis(2500))));
        assert_eq!(g.transcribe(w.path(), &no), Err(SpeechError::BadKey));
        assert_eq!(g.transcribe(w.path(), &no), Err(SpeechError::RateLimited(Some(7))));
        let e = g.transcribe(w.path(), &no).unwrap_err();
        assert!(matches!(&e, SpeechError::Network(m) if m.contains("500") && m.contains("boom")), "{e:?}");
        assert!(matches!(g.transcribe(w.path(), &no), Err(SpeechError::Unsupported(_))));
        let reqs = seen.lock().unwrap();
        let first = &reqs[0];
        assert!(first.starts_with("POST /audio/transcriptions"));
        assert!(first.contains("Bearer gsk_SECRET") && first.contains("name=\"model\"\r\n\r\nwhisper-large-v3-turbo") && first.contains("verbose_json"));
        assert!(!first.contains("name=\"language\""), "no language: Groq detects it");
        assert!(first.contains("RIFF"), "the WAV is in the body");
    }

    #[test]
    fn a_silent_server_times_out_and_a_cancel_returns_at_once() {
        // Accepts and never answers.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", l.local_addr().unwrap());
        std::thread::spawn(move || { let _held: Vec<_> = l.incoming().take(2).collect(); std::thread::sleep(Duration::from_secs(30)); });
        let mut g = Groq::new("k", "m");
        g.base = url.clone();
        let w = wav();
        let cancel = Arc::new(AtomicBool::new(false));
        let c2 = cancel.clone();
        std::thread::spawn(move || { std::thread::sleep(Duration::from_millis(200)); c2.store(true, Ordering::Relaxed); });
        let t0 = Instant::now();
        assert_eq!(g.transcribe(w.path(), &cancel), Err(SpeechError::Cancelled));
        assert!(t0.elapsed() < Duration::from_secs(2));
        let p = w.path().to_path_buf();
        drop(w);
        assert!(!p.exists(), "deleted even while the upload thread may hold it");
        let w = wav();
        g.limit = Some(Duration::from_millis(400));
        let t0 = Instant::now();
        let e = g.transcribe(w.path(), &AtomicBool::new(false));
        assert_eq!(e, Err(SpeechError::Network("Groq didn’t answer in time.".into())));
        assert!(t0.elapsed() < Duration::from_secs(5));
    }

    #[test]
    fn a_key_check_is_an_authenticated_list_of_models() {
        let (url, seen) = fake::serve(vec![fake::reply(200, "", r#"{"data":[]}"#), fake::reply(401, "", "{}")]);
        assert_eq!(check(&url, " gsk_A "), Ok(()));
        assert_eq!(check(&url, "gsk_B").unwrap_err(), "Groq didn’t accept the key.");
        assert!(check(&url, "  ").is_err());
        let r = seen.lock().unwrap();
        assert!(r[0].starts_with("GET /models") && r[0].contains("Bearer gsk_A\r\n"));
    }
}

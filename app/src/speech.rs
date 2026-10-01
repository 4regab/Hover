//! What voice sees of speech recognition: one finished recording in, its text out.
//! Cloud (Groq, `voice::groq`) and Local (Phonon, `phonon`) both sit behind it, so
//! cleanup, routing and the preview never know which one ran. The mode is picked once
//! per recording and never switched behind the user's back.

use std::path::Path;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// A transcript and what the engine said about it.
#[derive(Clone, Debug, PartialEq)]
pub struct Transcript {
    pub text: String,
    /// The language the engine reported, when it reports one (Groq's verbose output
    /// does; Phonon is English only and says nothing).
    pub language: Option<String>,
    /// The engine said it cut the audio short. Shown for review; never started on its own.
    pub truncated: bool,
    /// How long the audio was, when the engine said.
    pub audio: Option<Duration>,
    /// How long the engine took, start to text (cold start included for Local).
    pub took: Duration,
}

/// Why there is no transcript. Each is shown as it is; none becomes a prompt.
#[derive(Clone, Debug, PartialEq)]
pub enum SpeechError {
    /// The user (or Escape) cancelled it.
    Cancelled,
    /// The selected mode isn't set up: no Groq key, or Phonon isn't Ready. The text says
    /// what to do (add a key, Download, Repair).
    NotReady(String),
    /// Groq refused the key.
    BadKey,
    /// Groq's rate limit; the seconds to wait when it said.
    RateLimited(Option<u64>),
    /// The network, a timeout, a 5xx.
    Network(String),
    /// The local helper crashed, its files are missing or broken, or it said it failed.
    Engine(String),
    /// Too long or the wrong shape for the engine.
    Unsupported(String),
}

impl SpeechError {
    pub fn message(&self) -> String {
        match self {
            SpeechError::Cancelled => "Cancelled.".into(),
            SpeechError::NotReady(s) | SpeechError::Network(s) | SpeechError::Engine(s) | SpeechError::Unsupported(s) => s.clone(),
            SpeechError::BadKey => "Groq didn’t accept the key. Check it in Settings → Voice.".into(),
            SpeechError::RateLimited(Some(s)) => format!("Groq’s limit was reached. Try again in {s} s."),
            SpeechError::RateLimited(None) => "Groq’s limit was reached. Try again shortly.".into(),
        }
    }
}

/// One engine. `transcribe` blocks (it runs on voice's worker thread, never the UI's),
/// takes a 16 kHz mono 16-bit PCM WAV, and returns soon after `cancel` is set.
pub trait Speech: Send + Sync {
    fn transcribe(&self, wav: &Path, cancel: &AtomicBool) -> Result<Transcript, SpeechError>;
}

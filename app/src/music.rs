//! The office's chill beats (web/office/main.js `beats`): a CC0 lofi loop, off until
//! switched on, remembered, faded in to 0.32 and out, and silent while no office is
//! in view. The page played it through <audio>; here it is decoded as it plays (Ogg
//! Vorbis, lewton) into the system's output (cpal), and the device is let go while
//! silent, so a quiet Hover holds no audio stream. The same code plays through CoreAudio on a
//! Mac.

use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const LOOP: &[u8] = include_bytes!("../assets/office-beats.ogg");
pub const FULL: f64 = 0.32;

/// The page's ramp, one step per animation frame: up by 0.02, down by 0.03, within 0
/// and 0.32, snapped to the target once within 0.02. Returns the new volume and
/// whether the ramp has arrived. In doubles, as audio.volume is.
pub fn ramp_step(volume: f64, to: f64) -> (f64, bool) {
    let v = (volume + if to > volume { 0.02 } else { -0.03 }).clamp(0.0, FULL);
    if (v - to).abs() > 0.02 { (v, false) } else { (to, true) }
}

/// want (the button, remembered) and seen (an office in view): what should sound.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Want { pub want: bool, pub seen: bool }

impl Want {
    pub fn target(&self) -> f64 { if self.want && self.seen { FULL } else { 0.0 } }
}

struct Shared { volume: AtomicU32, playing: AtomicBool }

pub struct Beats {
    state: Mutex<Want>,
    shared: Arc<Shared>,
    output: Mutex<Option<Output>>,
    /// The ramp's own volume, in doubles; the audio thread reads it as an f32.
    ramp: Mutex<f64>,
}

fn f(v: &AtomicU32) -> f32 { f32::from_bits(v.load(Ordering::Relaxed)) }

impl Beats {
    pub fn new(want: bool) -> Beats {
        Beats {
            state: Mutex::new(Want { want, seen: true }),
            shared: Arc::new(Shared { volume: AtomicU32::new(0f32.to_bits()), playing: AtomicBool::new(false) }),
            output: Mutex::new(None),
            ramp: Mutex::new(0.0),
        }
    }

    pub fn want(&self) -> bool { self.state.lock().unwrap().want }
    pub fn volume(&self) -> f64 { *self.ramp.lock().unwrap() }

    /// The button: on or off, remembered by the caller. False when no output could be
    /// opened (the page's play() rejecting): the button goes back off.
    pub fn toggle(&self, want: bool) -> bool {
        self.state.lock().unwrap().want = want;
        self.sync()
    }

    /// An office came into view or went (the page's 'visible' message).
    pub fn follow(&self, seen: bool) { self.state.lock().unwrap().seen = seen; self.sync(); }

    fn sync(&self) -> bool {
        let t = self.state.lock().unwrap().target();
        if t > 0.0 {
            let mut o = self.output.lock().unwrap();
            if o.is_none() {
                match Output::open(self.shared.clone()) {
                    Ok(out) => *o = Some(out),
                    Err(e) => {
                        hover_core::log::line(&format!("beats: no audio output — {e}"));
                        self.state.lock().unwrap().want = false;
                        return false;
                    }
                }
            }
            self.shared.playing.store(true, Ordering::Relaxed);
        }
        true
    }

    /// One animation frame of the fade (about 60 a second, as requestAnimationFrame).
    /// True while it still moves, so the caller keeps its timer only as long as that.
    pub fn frame(&self) -> bool {
        let to = self.state.lock().unwrap().target();
        let now = self.volume();
        if now == to { return false; }
        let (v, done) = ramp_step(now, to);
        *self.ramp.lock().unwrap() = v;
        self.shared.volume.store((v as f32).to_bits(), Ordering::Relaxed);
        if done && to == 0.0 {
            // audio.pause(): the stream and the device go.
            self.shared.playing.store(false, Ordering::Relaxed);
            *self.output.lock().unwrap() = None;
        }
        !done
    }
}

/// The open device and the decoder feeding it.
struct Output { _stream: cpal::Stream }

impl Output {
    fn open(shared: Arc<Shared>) -> Result<Output, String> {
        use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
        let mut dec = Decoder::new()?;
        let device = cpal::default_host().default_output_device().ok_or("no output device")?;
        let cfg = device.default_output_config().map_err(|e| e.to_string())?;
        let channels = cfg.channels() as usize;
        let rate = cfg.sample_rate() as f64;
        let src_rate = dec.rate as f64;
        let mut pos = 0f64;
        let stream = device.build_output_stream(
            cfg.config(),
            move |out: &mut [f32], _| {
                let vol = f(&shared.volume);
                for frame in out.chunks_mut(channels) {
                    // Nearest-sample resampling is enough for a quiet loop in the background.
                    let (l, r) = dec.at(pos as usize);
                    pos += src_rate / rate;
                    if pos as usize >= dec.buffered() { pos -= dec.consume(pos as usize) as f64; }
                    for (c, s) in frame.iter_mut().enumerate() { *s = if c % 2 == 0 { l } else { r } * vol; }
                }
            },
            |e| hover_core::log::line(&format!("beats: {e}")),
            None,
        ).map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;
        Ok(Output { _stream: stream })
    }
}

/// The loop, decoded a packet at a time and started again at its end.
pub struct Decoder { r: lewton::inside_ogg::OggStreamReader<std::io::Cursor<&'static [u8]>>, buf: Vec<(f32, f32)>, pub rate: u32 }

impl Decoder {
    pub fn new() -> Result<Decoder, String> {
        let r = lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(LOOP)).map_err(|e| e.to_string())?;
        let rate = r.ident_hdr.audio_sample_rate;
        Ok(Decoder { r, buf: Vec::with_capacity(8192), rate })
    }

    fn fill(&mut self, upto: usize) {
        let mut empty = 0;
        while self.buf.len() <= upto {
            match self.r.read_dec_packet_itl() {
                Ok(Some(p)) if !p.is_empty() => {
                    empty = 0;
                    let ch = self.r.ident_hdr.audio_channels as usize;
                    for s in p.chunks(ch) {
                        let l = s[0] as f32 / 32768.0;
                        self.buf.push((l, s.get(1).map_or(l, |r| *r as f32 / 32768.0)));
                    }
                }
                Ok(Some(_)) => {}
                // The end (or a broken page): from the top again, with a new reader (a
                // seek to granule 0 left lewton at the end). Twice with nothing in
                // between means nothing will come: silence, rather than a spin.
                _ => {
                    empty += 1;
                    match lewton::inside_ogg::OggStreamReader::new(std::io::Cursor::new(LOOP)) {
                        Ok(n) if empty < 2 => self.r = n,
                        _ => { self.buf.resize(upto + 1, (0.0, 0.0)); }
                    }
                }
            }
        }
    }

    pub fn at(&mut self, i: usize) -> (f32, f32) { self.fill(i); self.buf[i] }
    pub fn buffered(&self) -> usize { self.buf.len() }
    /// Drops what has been played; how many frames went.
    pub fn consume(&mut self, played: usize) -> usize { let n = played.min(self.buf.len()); self.buf.drain(..n); n }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// main.js's ramp: +0.02 a frame up to 0.32, −0.03 down, snapped within 0.02. The
/// frame counts are the page's own expression run in Node (doubles): 16 up, 11 down.
    #[test]
    fn fades_as_the_page_fades() {
        let (mut v, mut n) = (0.0f64, 0);
        loop { let (x, done) = ramp_step(v, FULL); v = x; n += 1; if done { break; } }
        assert_eq!((v, n), (FULL, 16));
        let mut n = 0;
        loop { let (x, done) = ramp_step(v, 0.0); v = x; n += 1; if done { break; } }
        assert_eq!((v, n), (0.0, 11));
        assert_eq!(Want { want: true, seen: false }.target(), 0.0);
        assert_eq!(Want { want: true, seen: true }.target(), FULL);
    }

    /// The loop decodes and comes round again at its end.
    #[test]
    fn the_loop_decodes_and_loops() {
        let mut d = Decoder::new().unwrap();
        // office-beats.ogg is 22.05 kHz stereo.
        assert_eq!(d.rate, 22050);
        let (l, _) = d.at(d.rate as usize * 2);
        assert!(l.abs() <= 1.0);
        let mut total = 0usize;
        // Several minutes' worth: more than the loop holds, so it must have looped.
        for _ in 0..400 { total += d.consume(d.buffered()); d.at(d.rate as usize); }
        assert!(total > d.rate as usize * 200, "{total}");
    }
}

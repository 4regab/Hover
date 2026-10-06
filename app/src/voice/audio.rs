//! The microphone, while the shortcut is held: the chosen input (or the system's
//! default) through cpal, mixed down to mono and brought to 16 kHz 16-bit as it comes,
//! into one buffer that stops growing at ten minutes. The level the notch shows is the
//! real RMS of what just came in. Finishing drops the stream at once, so the device is
//! let go the moment the key is released, the cap is reached or the voice is cancelled.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};

pub const RATE: u32 = 16_000;
/// Ten minutes at 16 kHz: 9.6 M samples, 19.2 MB.
pub const MAX_SAMPLES: usize = RATE as usize * 600;

/// A recording in progress, as the voice worker sees it (the microphone, or a test's fake).
pub trait Source {
    /// 0..1 from the latest audio.
    fn level(&self) -> f32;
    /// How many 16 kHz samples are in so far.
    fn samples(&self) -> usize;
    /// The device failed (unplugged, taken away); the recording stops.
    fn failed(&self) -> Option<String>;
    /// Stops at once and hands the samples over.
    fn finish(self: Box<Self>) -> Vec<i16>;
    /// A copy of the samples from `from` on, while it records (voice listens for "take a
    /// screenshot" in it). None where the source can't give them.
    fn since(&self, _from: usize) -> Vec<i16> { vec![] }
}

/// Opens a source: the device's name (None is the default) and the most samples to keep.
pub type Open = Box<dyn Fn(Option<&str>, usize) -> Result<Box<dyn Source>, String> + Send + Sync>;

/// The input devices' names, for Settings (the system's default isn't listed; it is None).
pub fn microphones() -> Vec<String> {
    use cpal::traits::{DeviceTrait, HostTrait};
    let mut out: Vec<String> = vec![];
    if let Ok(list) = cpal::default_host().input_devices() {
        for d in list {
            if let Ok(n) = d.description() { let n = n.name().to_owned(); if !n.is_empty() && !out.contains(&n) { out.push(n); } }
        }
    }
    out
}

/// What the stream's callback writes and the worker reads.
struct Shared { buf: Mutex<Vec<i16>>, level: AtomicU32, failed: Mutex<Option<String>>, max: usize }

struct Mic { stream: Option<cpal::Stream>, shared: Arc<Shared> }

impl Source for Mic {
    fn level(&self) -> f32 { f32::from_bits(self.shared.level.load(Ordering::Relaxed)) }
    fn samples(&self) -> usize { self.shared.buf.lock().unwrap().len() }
    fn failed(&self) -> Option<String> { self.shared.failed.lock().unwrap().clone() }
    fn since(&self, from: usize) -> Vec<i16> { self.shared.buf.lock().unwrap().get(from..).map(<[i16]>::to_vec).unwrap_or_default() }
    fn finish(mut self: Box<Self>) -> Vec<i16> {
        // The stream first: the device is let go before anything else happens.
        drop(self.stream.take());
        std::mem::take(&mut *self.shared.buf.lock().unwrap())
    }
}

fn said(e: &cpal::Error) -> String {
    match e.kind() {
        cpal::ErrorKind::PermissionDenied => "Hover isn’t allowed to use the microphone. Allow it in the system’s privacy settings.".into(),
        cpal::ErrorKind::DeviceNotAvailable => "The microphone isn’t available any more.".into(),
        cpal::ErrorKind::DeviceBusy => "Another app is using the microphone.".into(),
        _ => format!("The microphone failed: {e}"),
    }
}

/// The real microphone.
pub fn open(device: Option<&str>, max: usize) -> Result<Box<dyn Source>, String> {
    use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
    let host = cpal::default_host();
    let dev = match device {
        None => host.default_input_device().ok_or("No microphone is connected.")?,
        // The one picked, or an error: recording another microphone without saying so isn't on.
        Some(name) => host.input_devices().map_err(|e| said(&e))?
            .find(|d| d.description().is_ok_and(|x| x.name() == name))
            .ok_or_else(|| format!("The microphone “{name}” isn’t connected. Pick another in Settings → Voice."))?,
    };
    let cfg = dev.default_input_config().map_err(|e| said(&e))?;
    let shared = Arc::new(Shared { buf: Mutex::new(Vec::new()), level: AtomicU32::new(0), failed: Mutex::new(None), max });
    let sc = cfg.config();
    let s = shared.clone();
    let stream = match cfg.sample_format() {
        cpal::SampleFormat::F32 => build::<f32>(&dev, sc, s),
        cpal::SampleFormat::I16 => build::<i16>(&dev, sc, s),
        cpal::SampleFormat::U16 => build::<u16>(&dev, sc, s),
        cpal::SampleFormat::I32 => build::<i32>(&dev, sc, s),
        cpal::SampleFormat::U8 => build::<u8>(&dev, sc, s),
        cpal::SampleFormat::F64 => build::<f64>(&dev, sc, s),
        f => return Err(format!("The microphone’s format ({f:?}) isn’t supported.")),
    }.map_err(|e| said(&e))?;
    stream.play().map_err(|e| said(&e))?;
    Ok(Box::new(Mic { stream: Some(stream), shared }))
}

fn build<T>(dev: &cpal::Device, cfg: cpal::StreamConfig, sh: Arc<Shared>) -> Result<cpal::Stream, cpal::Error>
where T: cpal::SizedSample, f32: cpal::FromSample<T> {
    use cpal::traits::DeviceTrait;
    let ch = (cfg.channels as usize).max(1);
    let mut rs = Resampler::new(cfg.sample_rate);
    let err = sh.clone();
    let mut mono: Vec<f32> = Vec::new();
    dev.build_input_stream::<T, _, _>(cfg, move |data: &[T], _| {
        mono.clear();
        mono.extend(data.chunks(ch).map(|f| f.iter().map(|s| s.to_sample::<f32>()).sum::<f32>() / ch as f32));
        sh.level.store(level(&mono).to_bits(), Ordering::Relaxed);
        let mut buf = sh.buf.lock().unwrap();
        rs.push(&mono, &mut buf, sh.max);
    }, move |e: cpal::Error| {
        // An overrun loses a few samples; the recording goes on. Anything else ends it.
        if e.kind() == cpal::ErrorKind::Xrun { return; }
        hover_core::log::line(&format!("voice: microphone — {e}"));
        err.failed.lock().unwrap().get_or_insert_with(|| said(&e));
    }, None)
}

/// The RMS of a block as 0..1 on a decibel scale: −60 dBFS and below is 0, full scale 1.
pub fn level(x: &[f32]) -> f32 {
    if x.is_empty() { return 0.0; }
    let rms = (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
    if rms <= 1e-6 { return 0.0; }
    ((20.0 * rms.log10() + 60.0) / 60.0).clamp(0.0, 1.0)
}

/// Any rate to 16 kHz, a block at a time: each output sample is the mean of the input
/// samples its slot covers (a box filter, so higher rates don't fold their top end
/// into speech as plain dropping would); below 16 kHz a sample is held.
// ponytail: a box filter, not a windowed-sinc one; some aliasing stays above 6 kHz,
// which speech models shrug off. A polyphase FIR is the upgrade if accuracy suffers.
pub struct Resampler { step: f64, t: f64, sum: f32, n: u32, last: f32 }

impl Resampler {
    pub fn new(rate: u32) -> Resampler { Resampler { step: rate.max(1) as f64 / RATE as f64, t: 0.0, sum: 0.0, n: 0, last: 0.0 } }

    /// Adds `mono` at the input rate to `out` at 16 kHz, never past `max` samples. The
    /// buffer grows ten seconds at a time, so its spare room stays small.
    pub fn push(&mut self, mono: &[f32], out: &mut Vec<i16>, max: usize) {
        for &s in mono {
            self.sum += s;
            self.n += 1;
            self.t += 1.0;
            while self.t >= self.step {
                self.t -= self.step;
                if out.len() >= max { return; }
                if self.n > 0 { self.last = self.sum / self.n as f32; self.sum = 0.0; self.n = 0; }
                if out.len() == out.capacity() { out.reserve_exact((max - out.len()).min(RATE as usize * 10)); }
                out.push((self.last.clamp(-1.0, 1.0) * 32767.0) as i16);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn any_rate_becomes_16k_and_the_buffer_stops_at_its_cap() {
        for rate in [8_000u32, 16_000, 44_100, 48_000] {
            let mut r = Resampler::new(rate);
            let mut out = vec![];
            // One second, in odd-sized blocks.
            let sine: Vec<f32> = (0..rate).map(|i| (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin() * 0.5).collect();
            for c in sine.chunks(441) { r.push(c, &mut out, usize::MAX); }
            assert!((out.len() as i64 - 16_000).abs() <= 1, "{rate}: {}", out.len());
            let peak = out.iter().map(|s| s.unsigned_abs()).max().unwrap();
            assert!((14_000..=16_400).contains(&peak), "{rate}: the 440 Hz tone survives ({peak})");
        }
        let mut r = Resampler::new(48_000);
        let mut out = vec![];
        r.push(&vec![0.1; 48_000], &mut out, 1_000);
        assert_eq!(out.len(), 1_000);
        assert!(out.capacity() <= 1_000, "no room past the cap");
    }

    #[test]
    fn the_level_is_the_real_rms() {
        assert_eq!(level(&[0.0; 64]), 0.0);
        assert_eq!(level(&[1.0, -1.0]), 1.0);
        // 0.01 RMS is −40 dBFS: a third of the way up.
        assert!((level(&[0.01, -0.01]) - 1.0 / 3.0).abs() < 1e-3);
    }

    /// A real capture, when a microphone is there: `cargo test -- --ignored voice::audio`.
    #[test]
    #[ignore]
    fn a_real_microphone_records() {
        let src = open(None, MAX_SAMPLES).expect("a microphone");
        let mut levels = vec![];
        for _ in 0..20 { std::thread::sleep(std::time::Duration::from_millis(100)); levels.push(src.level()); }
        assert!(src.failed().is_none());
        let s = src.finish();
        let peak = s.iter().map(|x| x.unsigned_abs()).max().unwrap_or(0);
        println!("devices {:?}; {} samples in 2 s; peak {peak}; levels {levels:.2?}", microphones(), s.len());
        assert!(s.len() > 16_000, "{}", s.len());
    }
}

//! The one recording on disk: a 16 kHz mono 16-bit PCM WAV in Hover's own temp folder,
//! made just before transcription and deleted when dropped, on every path. Nothing is
//! kept: a file left by a crash is swept the next time voice starts.

use std::io::Write;
use std::path::{Path, PathBuf};

/// Hover's own folder for it, so a sweep only ever touches Hover's files.
pub fn dir() -> PathBuf { std::env::temp_dir().join("hover-voice") }

/// Removes recordings an earlier run left behind (a crash between write and drop).
pub fn sweep() {
    let Ok(rd) = std::fs::read_dir(dir()) else { return };
    for e in rd.flatten() {
        if e.path().extension().is_some_and(|x| x == "wav") { let _ = std::fs::remove_file(e.path()); }
    }
}

/// The file; deleted when dropped.
pub struct TempWav(PathBuf);

impl TempWav {
    pub fn path(&self) -> &Path { &self.0 }
}

impl Drop for TempWav {
    fn drop(&mut self) { let _ = std::fs::remove_file(&self.0); }
}

/// The WAV's size for this many samples: the 44-byte header and two bytes each.
pub fn size(samples: usize) -> u64 { 44 + 2 * samples as u64 }

/// Writes the samples (16 kHz mono) as a new file. Err leaves nothing behind.
pub fn write(samples: &[i16], rate: u32) -> std::io::Result<TempWav> {
    std::fs::create_dir_all(dir())?;
    let f = TempWav(dir().join(format!("{}.wav", hover_core::guid_n())));
    let data = (2 * samples.len()) as u32;
    let mut w = std::io::BufWriter::new(std::fs::File::create(&f.0)?);
    w.write_all(b"RIFF")?;
    w.write_all(&(36 + data).to_le_bytes())?;
    w.write_all(b"WAVEfmt ")?;
    // PCM, one channel, the rate, bytes a second, block align 2, 16 bits.
    w.write_all(&16u32.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&1u16.to_le_bytes())?;
    w.write_all(&rate.to_le_bytes())?;
    w.write_all(&(rate * 2).to_le_bytes())?;
    w.write_all(&2u16.to_le_bytes())?;
    w.write_all(&16u16.to_le_bytes())?;
    w.write_all(b"data")?;
    w.write_all(&data.to_le_bytes())?;
    for s in samples { w.write_all(&s.to_le_bytes())?; }
    w.into_inner().map_err(|e| e.into_error())?.sync_all()?;
    Ok(f)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_wav_is_well_formed_and_gone_when_dropped() {
        let f = write(&[0, 1, -1, i16::MAX], 16_000).unwrap();
        let b = std::fs::read(f.path()).unwrap();
        assert_eq!(b.len() as u64, size(4));
        assert_eq!((&b[0..4], &b[8..16], &b[36..40]), (&b"RIFF"[..], &b"WAVEfmt "[..], &b"data"[..]));
        assert_eq!(u32::from_le_bytes([b[24], b[25], b[26], b[27]]), 16_000);
        assert_eq!(i16::from_le_bytes([b[50], b[51]]), i16::MAX);
        let p = f.path().to_path_buf();
        drop(f);
        assert!(!p.exists());
        // Ten minutes: 19.2 MB, under Groq's 25 MB upload limit.
        assert_eq!(size(16_000 * 600), 19_200_044);
    }
}

//! Standard audio wrappers (`*.wav`, `*.mp3`; M2-11).
//!
//! No exe-side WAV/MP3 decoder is ported here: the exe mentions `.wav`/`.mp3`,
//! `DSOUND.dll` and `mciSendCommandA`, but its audio-loading path has not been
//! located (open follow-up), so nothing in this crate claims exe fidelity.
//! Decoding goes through maintained
//! pure-Rust crates (`hound`, Apache-2.0; `nanomp3`, MIT OR Apache-2.0,
//! ADR 0008) and decoded samples are tier D (ADR 0009): what is asserted is
//! channel count, sample rate, bit depth and frame/sample counts against the
//! file headers. Video (Cinepak AVI) is NOT here; it lives in its own crate.

pub mod mp3;
pub mod wav;

/// Interleaved signed 16-bit PCM plus its format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pcm16 {
    pub channels: u16,
    pub sample_rate: u32,
    /// Interleaved samples; `len() == frames * channels`.
    pub samples: Vec<i16>,
}

impl Pcm16 {
    /// Sample frames (samples per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / usize::from(self.channels.max(1))
    }
}

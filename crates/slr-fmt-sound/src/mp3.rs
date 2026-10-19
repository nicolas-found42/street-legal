//! MPEG audio Layer III (`*.mp3`) over `nanomp3` (MIT OR Apache-2.0), a
//! bit-exact safe-Rust port of minimp3.

use crate::Pcm16;
use slr_fmt_core::{FormatError, Result};

/// Header-level facts from a frame walk (no audio decode).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Mp3Info {
    pub channels: u16,
    pub sample_rate: u32,
    /// Frames found, including a leading Xing/Info/LAME tag frame if present.
    pub frames: usize,
    /// Samples per channel per frame (1152 for MPEG-1, 576 for MPEG-2/2.5).
    pub samples_per_frame: usize,
}

fn bad(detail: impl Into<String>) -> FormatError {
    FormatError::Inconsistent {
        what: "mp3",
        detail: detail.into(),
    }
}

/// Walk the frames (tags and junk skipped, false syncs rejected by
/// minimp3's consistency rule) and report their common format.
pub fn info(bytes: &[u8]) -> Result<Mp3Info> {
    let mut it = nanomp3::frames(bytes);
    let first = it.next().ok_or_else(|| bad("no MPEG frames found"))?;
    if first.layer != 3 {
        return Err(bad(format!("layer {} is not Layer III", first.layer)));
    }
    let mut frames = 1;
    for f in it {
        if f.layer != first.layer
            || f.sample_rate != first.sample_rate
            || f.channels != first.channels
            || f.samples != first.samples
        {
            return Err(bad("stream format changes mid-stream"));
        }
        frames += 1;
    }
    Ok(Mp3Info {
        channels: u16::from(first.channels.num()),
        sample_rate: first.sample_rate,
        frames,
        samples_per_frame: first.samples,
    })
}

/// Decode a whole stream to interleaved i16. Tags are skipped and the
/// encoder delay/padding are trimmed when a LAME/Xing tag states them.
pub fn decode(bytes: &[u8]) -> Result<(Mp3Info, Pcm16)> {
    let hdr = info(bytes)?;
    let d = nanomp3::decode_all::<i16>(bytes);
    if d.format_changed {
        return Err(bad("format changed during decode"));
    }
    let channels = d.channels.map(|c| u16::from(c.num())).unwrap_or(0);
    if channels != hdr.channels || d.sample_rate != hdr.sample_rate {
        return Err(bad(format!(
            "decoder format {channels} ch {} Hz != header {} ch {} Hz",
            d.sample_rate, hdr.channels, hdr.sample_rate
        )));
    }
    if d.samples.is_empty() {
        return Err(bad("no audio decoded"));
    }
    Ok((
        hdr,
        Pcm16 {
            channels,
            sample_rate: d.sample_rate,
            samples: d.samples,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `n` MPEG-1 Layer III, 128 kbps, 44.1 kHz, stereo, no-padding frames
    /// (417 bytes each) whose side info and main data are all zero, which
    /// decodes to silence. Header bytes FF FB 90 00 written by hand from the
    /// ISO 11172-3 header layout.
    fn silent_frames(n: usize) -> Vec<u8> {
        let mut v = Vec::new();
        for _ in 0..n {
            let mut f = vec![0u8; 417];
            f[..4].copy_from_slice(&[0xFF, 0xFB, 0x90, 0x00]);
            v.extend_from_slice(&f);
        }
        v
    }

    #[test]
    fn frame_walk_reports_header_facts() {
        let i = info(&silent_frames(6)).unwrap();
        assert_eq!(
            i,
            Mp3Info {
                channels: 2,
                sample_rate: 44100,
                frames: 6,
                samples_per_frame: 1152
            }
        );
    }

    #[test]
    fn silent_stream_decodes_to_silence_of_expected_length() {
        let (i, pcm) = decode(&silent_frames(6)).unwrap();
        assert_eq!((pcm.channels, pcm.sample_rate), (2, 44100));
        assert!(pcm.frames() <= i.frames * i.samples_per_frame);
        assert!(pcm.frames() >= (i.frames - 1) * i.samples_per_frame);
        assert!(pcm.samples.iter().all(|&s| s == 0));
    }

    #[test]
    fn id3v2_prefix_is_skipped() {
        let mut v = b"ID3\x03\x00\x00\x00\x00\x00\x0A".to_vec();
        v.extend_from_slice(&[0; 10]);
        v.extend_from_slice(&silent_frames(6));
        assert_eq!(info(&v).unwrap().frames, 6);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(info(&[0u8; 2000]).is_err());
        assert!(decode(b"").is_err());
    }
}

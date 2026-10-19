//! RIFF/WAVE PCM (8- and 16-bit) over `hound`.

use crate::Pcm16;
use slr_fmt_core::{FormatError, Result};
use std::io::Cursor;

/// Header facts read before any sample decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WavInfo {
    pub channels: u16,
    pub sample_rate: u32,
    pub bits_per_sample: u16,
    /// Sample frames in the `data` chunk (`data_bytes / block_align`).
    pub frames: u32,
}

fn err(e: hound::Error) -> FormatError {
    FormatError::Inconsistent {
        what: "wav",
        detail: e.to_string(),
    }
}

fn check(spec: &hound::WavSpec) -> Result<()> {
    if spec.sample_format != hound::SampleFormat::Int || !matches!(spec.bits_per_sample, 8 | 16) {
        return Err(FormatError::Inconsistent {
            what: "wav format",
            detail: format!(
                "only 8/16-bit integer PCM is supported, found {:?} {}-bit",
                spec.sample_format, spec.bits_per_sample
            ),
        });
    }
    Ok(())
}

/// Read the header facts without decoding samples.
pub fn info(bytes: &[u8]) -> Result<WavInfo> {
    let r = hound::WavReader::new(Cursor::new(bytes)).map_err(err)?;
    let spec = r.spec();
    check(&spec)?;
    Ok(WavInfo {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: spec.bits_per_sample,
        frames: r.duration(),
    })
}

/// Decode to interleaved i16. 8-bit (unsigned in the file) samples are
/// widened as `signed << 8`.
pub fn decode(bytes: &[u8]) -> Result<(WavInfo, Pcm16)> {
    let mut r = hound::WavReader::new(Cursor::new(bytes)).map_err(err)?;
    let spec = r.spec();
    check(&spec)?;
    let hdr = WavInfo {
        channels: spec.channels,
        sample_rate: spec.sample_rate,
        bits_per_sample: spec.bits_per_sample,
        frames: r.duration(),
    };
    let samples = if spec.bits_per_sample == 8 {
        r.samples::<i8>()
            .map(|s| s.map(|v| i16::from(v) << 8))
            .collect::<std::result::Result<Vec<_>, _>>()
    } else {
        r.samples::<i16>()
            .collect::<std::result::Result<Vec<_>, _>>()
    }
    .map_err(err)?;
    if samples.len() != hdr.frames as usize * usize::from(hdr.channels) {
        return Err(FormatError::Inconsistent {
            what: "wav sample count",
            detail: format!(
                "{} samples decoded, header implies {} frames x {} ch",
                samples.len(),
                hdr.frames,
                hdr.channels
            ),
        });
    }
    Ok((
        hdr,
        Pcm16 {
            channels: spec.channels,
            sample_rate: spec.sample_rate,
            samples,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built canonical RIFF/WAVE (independent of `hound`'s writer).
    fn riff(channels: u16, rate: u32, bits: u16, data: &[u8]) -> Vec<u8> {
        let block = channels * bits / 8;
        let mut v = b"RIFF".to_vec();
        v.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&channels.to_le_bytes());
        v.extend_from_slice(&rate.to_le_bytes());
        v.extend_from_slice(&(rate * u32::from(block)).to_le_bytes());
        v.extend_from_slice(&block.to_le_bytes());
        v.extend_from_slice(&bits.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&(data.len() as u32).to_le_bytes());
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn pcm16_stereo_decodes_interleaved() {
        let data: Vec<u8> = [1i16, -2, 300, -400, 32767, -32768]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        let (i, pcm) = decode(&riff(2, 44100, 16, &data)).unwrap();
        assert_eq!(
            i,
            WavInfo {
                channels: 2,
                sample_rate: 44100,
                bits_per_sample: 16,
                frames: 3
            }
        );
        assert_eq!(pcm.samples, vec![1, -2, 300, -400, 32767, -32768]);
        assert_eq!(pcm.frames(), 3);
    }

    #[test]
    fn pcm8_is_unsigned_and_widened() {
        // 0x80 is silence, 0xFF is +127, 0x00 is -128.
        let (i, pcm) = decode(&riff(1, 11025, 8, &[0x80, 0xFF, 0x00])).unwrap();
        assert_eq!((i.bits_per_sample, i.frames), (8, 3));
        assert_eq!(pcm.samples, vec![0, 127 << 8, -128 << 8]);
    }

    #[test]
    fn info_does_not_need_sample_decode() {
        let i = info(&riff(1, 22050, 16, &[0; 10])).unwrap();
        assert_eq!((i.frames, i.sample_rate), (5, 22050));
    }

    #[test]
    fn non_pcm_and_garbage_are_rejected() {
        assert!(info(b"RIFF").is_err());
        assert!(decode(&[]).is_err());
        // 24-bit PCM is outside the shipped set.
        assert!(info(&riff(1, 22050, 24, &[0; 6])).is_err());
    }
}

//! Wall-clock playback of a Cinepak AVI as RGBA frames.
//!
//! The exe plays video through WINMM MCI, which paces frames by the AVI's
//! own timeline (`dwMicroSecPerFrame`) and, for the in-game texture path,
//! repeats it when asked. This is the same timeline: [`Player::advance_to`]
//! takes elapsed microseconds since playback began and decodes every frame up
//! to the one that is due (Cinepak inter frames depend on their predecessors,
//! so nothing is skipped), never ahead of the clock.
use crate::{avi::Avi, cinepak::Decoder};
use slr_fmt_core::{FormatError, Result};

pub struct Player {
    frames: Vec<Vec<u8>>,
    decoder: Decoder,
    micros_per_frame: u64,
    repeat: bool,
    /// Index of the next frame chunk to decode.
    next: usize,
    /// Frames decoded since the start, counting repeats.
    decoded: u64,
    rgba: Vec<u8>,
    serial: u64,
    finished: bool,
}

impl Player {
    pub fn open(data: &[u8], repeat: bool) -> Result<Self> {
        let avi = Avi::parse(data)?;
        let stream = avi.video_stream().ok_or(FormatError::Inconsistent {
            what: "avi",
            detail: "no video stream".into(),
        })?;
        let (w, h) = (stream.width as usize, stream.height as usize);
        let frames: Vec<Vec<u8>> = avi.video_frames().map(<[u8]>::to_vec).collect();
        if frames.is_empty() || avi.header.micros_per_frame == 0 {
            return Err(FormatError::Inconsistent {
                what: "avi",
                detail: "no frames or zero frame duration".into(),
            });
        }
        Ok(Self {
            frames,
            decoder: Decoder::new(w, h)?,
            micros_per_frame: u64::from(avi.header.micros_per_frame),
            repeat,
            next: 0,
            decoded: 0,
            rgba: vec![255; w * h * 4],
            serial: 0,
            finished: false,
        })
    }

    pub fn width(&self) -> usize {
        self.decoder.width()
    }
    pub fn height(&self) -> usize {
        self.decoder.height()
    }
    pub fn frame_count(&self) -> usize {
        self.frames.len()
    }
    pub fn micros_per_frame(&self) -> u64 {
        self.micros_per_frame
    }
    pub fn finished(&self) -> bool {
        self.finished
    }
    /// Increments each time the picture changes.
    pub fn serial(&self) -> u64 {
        self.serial
    }
    /// Frames decoded so far, counting repeats (a late caller decodes several
    /// per picture change).
    pub fn frames_decoded(&self) -> u64 {
        self.decoded
    }
    /// Current picture, RGBA8, opaque, top row first.
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }

    /// Decode through the frame due at `elapsed_us`. Returns whether the
    /// picture changed.
    pub fn advance_to(&mut self, elapsed_us: u64) -> Result<bool> {
        let due = elapsed_us / self.micros_per_frame + 1;
        let mut changed = false;
        while !self.finished && self.decoded < due {
            if self.next == self.frames.len() {
                if self.repeat {
                    self.next = 0;
                } else {
                    self.finished = true;
                    break;
                }
            }
            self.decoder.decode(&self.frames[self.next])?;
            self.next += 1;
            self.decoded += 1;
            changed = true;
        }
        if changed {
            for (dst, src) in self
                .rgba
                .as_chunks_mut::<4>()
                .0
                .iter_mut()
                .zip(self.decoder.frame().as_chunks::<3>().0.iter())
            {
                dst[..3].copy_from_slice(src);
            }
            self.serial += 1;
        }
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_prime() -> Option<Vec<u8>> {
        let root = std::env::var_os("SLR_STOCK")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock")
            });
        let path = root.join("Data/Fmv/Prime.avi");
        match std::fs::read(&path) {
            Ok(data) => Some(data),
            Err(error)
                if error.kind() == std::io::ErrorKind::NotFound
                    && std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none() =>
            {
                eprintln!(
                    "SLR_ORACLE_SKIPPED: stock Prime AVI missing at {}",
                    path.display()
                );
                None
            }
            Err(error) => panic!("required/readable Prime AVI {}: {error}", path.display()),
        }
    }

    #[test]
    fn prime_follows_its_own_timeline_and_stops_or_repeats() {
        let Some(data) = stock_prime() else { return };
        let mut p = Player::open(&data, false).unwrap();
        assert_eq!((p.width(), p.height(), p.frame_count()), (160, 120, 834));
        assert_eq!(p.micros_per_frame(), 41667);
        assert!(p.advance_to(0).unwrap());
        assert_eq!(p.frames_decoded(), 1);
        assert!(!p.advance_to(41666).unwrap(), "not due yet");
        assert!(p.advance_to(41667).unwrap());
        // A late caller still decodes every intermediate frame, in order.
        assert!(p.advance_to(41667 * 10).unwrap());
        assert_eq!(p.frames_decoded(), 11);
        assert_eq!(p.serial(), 3);
        assert!(p.advance_to(41667 * 2000).unwrap());
        assert!(p.finished());
        assert_eq!(p.frames_decoded(), 834);
        let mut r = Player::open(&data, true).unwrap();
        r.advance_to(41667 * 900).unwrap();
        assert!(!r.finished());
        assert_eq!(r.frames_decoded(), 901);
    }
}

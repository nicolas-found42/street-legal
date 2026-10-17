//! PTX texture container (`*.ptx`).
//!
//! The exe's texture pipeline (`FUN_00505960` factory, `FUN_0053cc70` extension
//! dispatch: `"ptx"` -> type 1 -> `FUN_00506980`) reads each file into memory
//! and hands the buffer to the type-1 loader `FUN_00507110` (vtable slot at
//! `0x5f2d58`). That loader ports here as [`parse`]: the container framing
//! (header + per-mip table + JPEG payload slicing) is exact; JPEG *decoding*
//! is tier D (a maintained crate) because the exe links its own static
//! libjpeg (error strings like `"Bogus JPEG colorspace"`, `"JPEGMEM"` in the
//! exe) whose pixel output no third-party decoder can claim to replicate.
//!
//! Container layout, verified over all 127 stock + 127 modded `.ptx` files
//! (336 mips stock; zero leftover, `total == rgb_size + alpha_size` everywhere):
//!
//! ```text
//! 0x00 u32 0, u32 mip_count, u32 width, u32 height
//! per mip: u32 total, u32 rgb_size, u32 alpha_size, u32 0,
//!          f32 level (= (index+1)/mip_count),
//!          u8[total] payload = rgb_size bytes JPEG + alpha_size bytes JPEG
//! ```
//!
//! `width`/`height` are the largest mip's JPEG dimensions (SOF0); per-mip JPEG
//! dimensions halve down to 4x4 (non-square bases like 512x256 keep aspect per
//! level, e.g. 4x8 ... 256x512). A mip carries an alpha JPEG iff
//! `alpha_size > 0` (97 of 127 stock files have alpha on every mip; 30 are
//! RGB-only).

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// One mip level: the 20-byte table entry plus the sliced payloads.
///
/// The per-mip table ports the header/per-mip reader; `rgb_size` feeds the
/// JPEG decode request, and `alpha_size` gates the second-JPEG branch.
///
/// exe: FUN_00507110
/// exe: FUN_00506ad0
/// exe: FUN_00506c70
#[derive(Debug, Clone)]
pub struct Mip {
    /// Total payload bytes after this 20-byte entry
    /// (the loader advances by total + `0x14`).
    pub total: u32,
    /// Bytes of the RGB (colour) JPEG starting at the payload (fed to the
    /// JPEG decode request).
    pub rgb_size: u32,
    /// Bytes of the optional alpha JPEG following the RGB JPEG (`0` = none;
    /// nonzero selects the second-JPEG branch).
    pub alpha_size: u32,
    /// Normalised mip level: `(index + 1) / mip_count` as f32 (read from the
    /// table but never consumed by the loader; every shipped file stores it
    /// exactly, f32-bit-exact).
    pub level: f32,
    /// `rgb_size` bytes of baseline JPEG (RGB), starting with `FFD8`, ending `FFD9`.
    pub rgb: Vec<u8>,
    /// `alpha_size` bytes of grayscale JPEG (`FFD8`..`FFD9`), or empty when
    /// `alpha_size == 0`.
    pub alpha: Vec<u8>,
}

impl Mip {
    /// True when this mip carries a second (alpha) JPEG.
    pub fn has_alpha(&self) -> bool {
        self.alpha_size > 0
    }
}

/// A decoded PTX container: header plus per-mip table and payloads.
/// The header words port the type-1 loader; per-mip JPEG branches are
/// decoded via [`Mip`].
///
/// exe: FUN_00507110
#[derive(Debug, Clone)]
pub struct Ptx {
    /// Mip-level count from header word 1 (clamped by `texture_size`: fewer
    /// levels keep the low small mips).
    pub mip_count: u32,
    /// Base (largest-mip) width in pixels, header word 2 (right-shifted with
    /// height when levels are clamped).
    pub width: u32,
    /// Base (largest-mip) height in pixels, header word 3 (right-shifted with
    /// width when levels are clamped).
    pub height: u32,
    /// One entry per mip level, smallest first, largest (base) last.
    pub mips: Vec<Mip>,
}

/// Parse the PTX container: header, per-mip table, JPEG payload slicing.
/// Byte-exact accounting: the cursor must end exactly at EOF, and every
/// entry must satisfy `total == rgb_size + alpha_size` with `rgb_size > 0`.
/// The RGB payload must start with the JPEG SOI marker (`FFD8`); the alpha
/// payload, when present, likewise. Pixel decoding is NOT done here (tier D;
/// see [`decode_rgb`]). Header words and the per-mip advance port the
/// type-1 loader; the `"ptx"` extension dispatch and the type-1 factory
/// select it.
///
/// exe: FUN_0053cc70
/// exe: FUN_00505960
/// exe: FUN_00507110
pub fn parse(data: &[u8]) -> Result<Ptx> {
    let mut r = Reader::new(data);
    let zero = r.u32()?;
    if zero != 0 {
        return Err(FormatError::Inconsistent {
            what: "ptx header word 0",
            detail: format!("expected 0, found {zero:#x}"),
        });
    }
    let mip_count = r.u32()?;
    let width = r.u32()?;
    let height = r.u32()?;
    if mip_count == 0 {
        return Err(FormatError::Inconsistent {
            what: "ptx mip count",
            detail: "mip_count is 0".to_string(),
        });
    }
    if width == 0 || height == 0 {
        return Err(FormatError::Inconsistent {
            what: "ptx dimensions",
            detail: format!("{width}x{height}"),
        });
    }
    let mut mips = Vec::with_capacity(mip_count as usize);
    for index in 0..mip_count as usize {
        let total = r.u32()?;
        let rgb_size = r.u32()?;
        let alpha_size = r.u32()?;
        let word3 = r.u32()?;
        if word3 != 0 {
            return Err(FormatError::Inconsistent {
                what: "ptx mip word 3",
                detail: format!("mip {index}: expected 0, found {word3:#x}"),
            });
        }
        let level = r.f32()?;
        let expected = (index as f32 + 1.0) / mip_count as f32;
        if level.to_bits() != expected.to_bits() {
            return Err(FormatError::Inconsistent {
                what: "ptx mip level",
                detail: format!("mip {index}: found {level}, expected {expected}"),
            });
        }
        if total != rgb_size.saturating_add(alpha_size) {
            return Err(FormatError::Inconsistent {
                what: "ptx mip total",
                detail: format!(
                    "mip {index}: total {total} != rgb {rgb_size} + alpha {alpha_size}"
                ),
            });
        }
        if rgb_size == 0 {
            return Err(FormatError::Inconsistent {
                what: "ptx mip rgb_size",
                detail: format!("mip {index}: rgb_size is 0"),
            });
        }
        let payload = r.bytes(total as usize)?;
        let (rgb, alpha) = payload.split_at(rgb_size as usize);
        if rgb.len() < 2 || rgb[0] != 0xFF || rgb[1] != 0xD8 {
            return Err(FormatError::Inconsistent {
                what: "ptx rgb jpeg",
                detail: format!("mip {index}: missing SOI marker"),
            });
        }
        if alpha_size > 0 && (alpha.len() < 2 || alpha[0] != 0xFF || alpha[1] != 0xD8) {
            return Err(FormatError::Inconsistent {
                what: "ptx alpha jpeg",
                detail: format!("mip {index}: missing SOI marker"),
            });
        }
        mips.push(Mip {
            total,
            rgb_size,
            alpha_size,
            level,
            rgb: rgb.to_vec(),
            alpha: alpha.to_vec(),
        });
    }
    if r.remaining() != 0 {
        return Err(FormatError::Inconsistent {
            what: "ptx trailing bytes",
            detail: format!("{} leftover bytes", r.remaining()),
        });
    }
    Ok(Ptx {
        mip_count,
        width,
        height,
        mips,
    })
}

/// Dimensions of a decoded mip: `(width, height, components)`.
///
/// Tier D helper: decodes through the `zune-jpeg` crate (pure Rust,
/// maintained; see `Cargo.toml`). The exe links its own static libjpeg
/// (error strings `"Bogus JPEG colorspace"`, `"JPEGMEM"` at file offset
/// `0x21c9e0` of `StreetLegal_Redline.bak.exe`; decode requested from
/// `FUN_00506ad0` via `FUN_00564b50`), so pixel-exactness is NOT claimed:
/// only the container above is exe-ported. This helper asserts dimensions
/// only, never pixels.
pub fn decode_rgb(mip: &Mip) -> std::result::Result<(u16, u16, u16), String> {
    use zune_core::bytestream::ZCursor;
    use zune_jpeg::JpegDecoder;
    let cursor = ZCursor::new(&mip.rgb);
    let mut dec = JpegDecoder::new(cursor);
    dec.decode_headers()
        .map_err(|e| format!("jpeg headers: {e:?}"))?;
    let info = dec
        .info()
        .ok_or_else(|| "jpeg headers not decoded".to_string())?;
    Ok((info.width, info.height, u16::from(info.components)))
}

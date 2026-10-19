//! Truevision TGA (`*.tga`) wrapper over `tinytga` (MIT OR Apache-2.0).
//!
//! Shipped files are uncompressed: colour-mapped 8-bit with a 24-bit palette
//! (image type 1), grayscale 8-bit (type 3) and true-colour 32-bit (type 2).
//! The exe has no TGA loader of its own that we have located; pixels are
//! tier D (ADR 0009), the header facts are what is asserted.

use crate::image::Rgba8Image;
use slr_fmt_core::{FormatError, Result};
use tinytga::{Bpp, DataType, RawTga};

/// Image type as stored in the TGA header (byte 2, RLE bit stripped).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TgaKind {
    ColorMapped,
    TrueColor,
    Gray,
}

/// Header facts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TgaInfo {
    pub width: u32,
    pub height: u32,
    pub kind: TgaKind,
    /// Bits per stored pixel (index width for colour-mapped images).
    pub pixel_bits: u8,
    /// Bits per palette entry for colour-mapped images.
    pub palette_bits: Option<u8>,
    pub rle: bool,
}

fn bad(detail: impl Into<String>) -> FormatError {
    FormatError::Inconsistent {
        what: "tga",
        detail: detail.into(),
    }
}

fn header(raw: &RawTga<'_>) -> Result<TgaInfo> {
    let kind = match raw.data_type() {
        DataType::ColorMapped => TgaKind::ColorMapped,
        DataType::TrueColor => TgaKind::TrueColor,
        DataType::BlackAndWhite => TgaKind::Gray,
        DataType::NoData => return Err(bad("image has no data")),
    };
    let size = raw.size();
    if !matches!(raw.compression(), tinytga::Compression::Rle) {
        let need =
            size.width as usize * size.height as usize * usize::from(raw.image_data_bpp().bytes());
        if raw.image_data().len() < need {
            return Err(bad(format!(
                "truncated: {} bytes of pixel data, {need} needed",
                raw.image_data().len()
            )));
        }
    }
    Ok(TgaInfo {
        width: size.width,
        height: size.height,
        kind,
        pixel_bits: raw.image_data_bpp().bits(),
        palette_bits: raw.color_map().map(|m| m.entry_bpp().bits()),
        rle: matches!(raw.compression(), tinytga::Compression::Rle),
    })
}

/// Read header facts without decoding.
pub fn info(bytes: &[u8]) -> Result<TgaInfo> {
    let raw = RawTga::from_slice(bytes).map_err(|e| bad(format!("{e:?}")))?;
    header(&raw)
}

/// Expand one raw colour value of `bpp` bits to RGBA8. `gray` selects the
/// grayscale interpretation of 8-bit values (otherwise 8-bit is rejected).
fn expand(value: u32, bpp: Bpp, gray: bool) -> Option<[u8; 4]> {
    Some(match bpp {
        Bpp::Bits8 => {
            if !gray {
                return None;
            }
            let g = value as u8;
            [g, g, g, 255]
        }
        Bpp::Bits16 => {
            // ARRRRRGG GGGBBBBB (A1R5G5B5).
            let e = |v: u32| ((v << 3) | (v >> 2)) as u8;
            [
                e((value >> 10) & 0x1F),
                e((value >> 5) & 0x1F),
                e(value & 0x1F),
                if value & 0x8000 != 0 { 255 } else { 0 },
            ]
        }
        Bpp::Bits24 => [(value >> 16) as u8, (value >> 8) as u8, value as u8, 255],
        Bpp::Bits32 => [
            (value >> 16) as u8,
            (value >> 8) as u8,
            value as u8,
            (value >> 24) as u8,
        ],
        _ => return None,
    })
}

/// Decode to RGBA8, top row first regardless of the stored origin.
pub fn decode(bytes: &[u8]) -> Result<(TgaInfo, Rgba8Image)> {
    let raw = RawTga::from_slice(bytes).map_err(|e| bad(format!("{e:?}")))?;
    let hdr = header(&raw)?;
    let (w, h) = (hdr.width, hdr.height);
    let mut pixels = vec![0u8; w as usize * h as usize * 4];
    let gray = hdr.kind == TgaKind::Gray;
    let mut seen = 0usize;
    for p in raw.pixels() {
        let (x, y) = (p.position.x, p.position.y);
        if x < 0 || y < 0 || x as u32 >= w || y as u32 >= h {
            return Err(bad(format!("pixel at ({x}, {y}) outside {w}x{h}")));
        }
        let rgba = match raw.color_map() {
            Some(map) => {
                let v = map
                    .get_raw(p.color as usize)
                    .ok_or_else(|| bad(format!("palette index {} out of range", p.color)))?;
                expand(v, map.entry_bpp(), false)
            }
            None => expand(p.color, raw.image_data_bpp(), gray),
        }
        .ok_or_else(|| bad(format!("unsupported pixel depth {}", hdr.pixel_bits)))?;
        let i = (y as usize * w as usize + x as usize) * 4;
        pixels[i..i + 4].copy_from_slice(&rgba);
        seen += 1;
    }
    if seen != w as usize * h as usize {
        return Err(bad(format!("{seen} pixels decoded for {w}x{h}")));
    }
    Ok((hdr, Rgba8Image::new(w, h, pixels)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tga(
        img_type: u8,
        cmap: Option<(&[u8], u8)>,
        w: u16,
        h: u16,
        bpp: u8,
        desc: u8,
        data: &[u8],
    ) -> Vec<u8> {
        let mut v = vec![0u8; 18];
        v[2] = img_type;
        if let Some((pal, depth)) = cmap {
            v[1] = 1;
            let entries = pal.len() / (depth as usize / 8);
            v[5..7].copy_from_slice(&(entries as u16).to_le_bytes());
            v[7] = depth;
        }
        v[12..14].copy_from_slice(&w.to_le_bytes());
        v[14..16].copy_from_slice(&h.to_le_bytes());
        v[16] = bpp;
        v[17] = desc;
        if let Some((pal, _)) = cmap {
            v.extend_from_slice(pal);
        }
        v.extend_from_slice(data);
        v
    }

    #[test]
    fn truecolor32_bgra_bottom_left_origin_flips_to_top_first() {
        // 1x2, origin bottom-left (desc 8 = 8 alpha bits): first stored row is the BOTTOM row.
        let bytes = tga(2, None, 1, 2, 32, 0x08, &[3, 2, 1, 200, 30, 20, 10, 100]);
        let (i, img) = decode(&bytes).unwrap();
        assert_eq!(
            (i.kind, i.pixel_bits, i.width, i.height, i.rle),
            (TgaKind::TrueColor, 32, 1, 2, false)
        );
        assert_eq!(img.pixel(0, 1), [1, 2, 3, 200]);
        assert_eq!(img.pixel(0, 0), [10, 20, 30, 100]);
    }

    #[test]
    fn truecolor32_top_left_origin_keeps_row_order() {
        let bytes = tga(2, None, 1, 2, 32, 0x28, &[3, 2, 1, 200, 30, 20, 10, 100]);
        let (_, img) = decode(&bytes).unwrap();
        assert_eq!(img.pixel(0, 0), [1, 2, 3, 200]);
        assert_eq!(img.pixel(0, 1), [10, 20, 30, 100]);
    }

    #[test]
    fn colormapped8_with_24bit_palette() {
        let pal = [0, 0, 255, 0, 255, 0]; // entry0 = BGR(0,0,255)=red, entry1 = green
        let bytes = tga(1, Some((&pal, 24)), 2, 1, 8, 0x20, &[1, 0]);
        let (i, img) = decode(&bytes).unwrap();
        assert_eq!(
            (i.kind, i.pixel_bits, i.palette_bits),
            (TgaKind::ColorMapped, 8, Some(24))
        );
        assert_eq!(img.pixel(0, 0), [0, 255, 0, 255]);
        assert_eq!(img.pixel(1, 0), [255, 0, 0, 255]);
    }

    #[test]
    fn gray8_replicates() {
        let bytes = tga(3, None, 2, 1, 8, 0x20, &[7, 200]);
        let (i, img) = decode(&bytes).unwrap();
        assert_eq!(i.kind, TgaKind::Gray);
        assert_eq!(img.pixel(1, 0), [200, 200, 200, 255]);
    }

    #[test]
    fn truncated_and_garbage_are_errors() {
        assert!(info(&[0; 4]).is_err());
        let bytes = tga(2, None, 4, 4, 32, 0x28, &[0; 8]);
        assert!(decode(&bytes).is_err());
    }
}

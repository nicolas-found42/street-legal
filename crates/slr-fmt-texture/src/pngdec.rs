//! PNG wrapper (`*.png`) over the `png` crate (MIT OR Apache-2.0).
//!
//! The exe statically links libpng (error strings in the image), so there is
//! no exe-side loader to port; decoded pixels are tier D (ADR 0009) and only
//! the header (dimensions, colour type, bit depth) is asserted.

use crate::image::Rgba8Image;
use slr_fmt_core::{FormatError, Result};

pub use png::{BitDepth, ColorType};

/// Header facts read before any pixel decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PngInfo {
    pub width: u32,
    pub height: u32,
    pub color_type: ColorType,
    pub bit_depth: BitDepth,
}

fn err(e: png::DecodingError) -> FormatError {
    FormatError::Inconsistent {
        what: "png",
        detail: e.to_string(),
    }
}

/// Read the IHDR facts without decoding pixels.
pub fn info(bytes: &[u8]) -> Result<PngInfo> {
    let reader = png::Decoder::new(std::io::Cursor::new(bytes))
        .read_info()
        .map_err(err)?;
    let i = reader.info();
    Ok(PngInfo {
        width: i.width,
        height: i.height,
        color_type: i.color_type,
        bit_depth: i.bit_depth,
    })
}

/// Decode to RGBA8 (palette expanded, 16-bit stripped to 8, gray replicated,
/// opaque alpha when the source has none).
pub fn decode(bytes: &[u8]) -> Result<(PngInfo, Rgba8Image)> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder.read_info().map_err(err)?;
    let hdr = {
        let i = reader.info();
        PngInfo {
            width: i.width,
            height: i.height,
            color_type: i.color_type,
            bit_depth: i.bit_depth,
        }
    };
    let mut buf = vec![
        0u8;
        reader
            .output_buffer_size()
            .ok_or(FormatError::Inconsistent {
                what: "png",
                detail: "output buffer size overflows".to_string(),
            })?
    ];
    let frame = reader.next_frame(&mut buf).map_err(err)?;
    let (w, h) = (frame.width, frame.height);
    let data = &buf[..frame.buffer_size()];
    let n = w as usize * h as usize;
    let mut pixels = Vec::with_capacity(n * 4);
    match frame.color_type {
        ColorType::Rgba => pixels.extend_from_slice(data),
        ColorType::Rgb => data
            .as_chunks::<3>()
            .0
            .iter()
            .for_each(|p| pixels.extend_from_slice(&[p[0], p[1], p[2], 255])),
        ColorType::Grayscale => data
            .iter()
            .for_each(|&g| pixels.extend_from_slice(&[g, g, g, 255])),
        ColorType::GrayscaleAlpha => data
            .as_chunks::<2>()
            .0
            .iter()
            .for_each(|p| pixels.extend_from_slice(&[p[0], p[0], p[0], p[1]])),
        ColorType::Indexed => {
            return Err(FormatError::Inconsistent {
                what: "png",
                detail: "indexed output after EXPAND".to_string(),
            })
        }
    }
    if pixels.len() != n * 4 {
        return Err(FormatError::Inconsistent {
            what: "png pixel count",
            detail: format!("{} bytes for {w}x{h}", pixels.len()),
        });
    }
    Ok((hdr, Rgba8Image::new(w, h, pixels)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn encode(w: u32, h: u32, ct: ColorType, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        let mut e = png::Encoder::new(&mut out, w, h);
        e.set_color(ct);
        e.set_depth(BitDepth::Eight);
        e.write_header().unwrap().write_image_data(data).unwrap();
        out
    }

    #[test]
    fn rgb_gets_opaque_alpha_and_header_facts() {
        let bytes = encode(2, 1, ColorType::Rgb, &[1, 2, 3, 4, 5, 6]);
        let i = info(&bytes).unwrap();
        assert_eq!(
            i,
            PngInfo {
                width: 2,
                height: 1,
                color_type: ColorType::Rgb,
                bit_depth: BitDepth::Eight
            }
        );
        let (_, img) = decode(&bytes).unwrap();
        assert_eq!(img.pixel(0, 0), [1, 2, 3, 255]);
        assert_eq!(img.pixel(1, 0), [4, 5, 6, 255]);
    }

    #[test]
    fn rgba_is_passed_through() {
        let bytes = encode(1, 2, ColorType::Rgba, &[9, 8, 7, 6, 1, 2, 3, 4]);
        let (_, img) = decode(&bytes).unwrap();
        assert_eq!((img.width, img.height), (1, 2));
        assert_eq!(img.pixel(0, 0), [9, 8, 7, 6]);
        assert_eq!(img.pixel(0, 1), [1, 2, 3, 4]);
    }

    #[test]
    fn gray_alpha_replicates_gray() {
        let bytes = encode(1, 1, ColorType::GrayscaleAlpha, &[100, 50]);
        let (hdr, img) = decode(&bytes).unwrap();
        assert_eq!(hdr.color_type, ColorType::GrayscaleAlpha);
        assert_eq!(img.pixel(0, 0), [100, 100, 100, 50]);
    }

    #[test]
    fn garbage_is_an_error() {
        assert!(info(b"not a png").is_err());
        assert!(decode(&[]).is_err());
    }
}

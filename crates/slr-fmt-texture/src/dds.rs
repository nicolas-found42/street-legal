//! DirectDraw Surface (`*.dds`) wrapper: DXT1/DXT3/DXT5 (BC1/BC2/BC3).
//!
//! The exe's texture-level error strings name D3DX (`"Could not get level
//! Description of D3DX texture!"`), i.e. DDS decoding is the Direct3D 9
//! runtime's, not exe code, so there is nothing to port: container parsing is `ddsfile`, block
//! decompression is `texture2ddecoder` (both MIT/Apache-compatible, ADR
//! 0008), and decoded pixels are tier D (ADR 0009). What is asserted is the
//! header: dimensions, block format and mip count, plus an independent
//! re-derivation of the mip chain's byte size from those header fields.

use crate::image::Rgba8Image;
use slr_fmt_core::{FormatError, Result};

/// Block-compressed pixel formats present in the shipped game.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum DdsFormat {
    /// BC1: 8 bytes per 4x4 block, 1-bit alpha.
    Dxt1,
    /// BC2: 16 bytes per block, explicit 4-bit alpha.
    Dxt3,
    /// BC3: 16 bytes per block, interpolated alpha.
    Dxt5,
}

impl DdsFormat {
    /// Bytes per 4x4 block.
    pub fn block_bytes(self) -> usize {
        match self {
            DdsFormat::Dxt1 => 8,
            DdsFormat::Dxt3 | DdsFormat::Dxt5 => 16,
        }
    }
}

/// A parsed DDS: header facts plus the raw mip chain (base level first).
#[derive(Debug, Clone)]
pub struct Dds {
    pub width: u32,
    pub height: u32,
    pub format: DdsFormat,
    /// Number of mip levels including the base (1 when the header has none).
    pub mip_count: u32,
    data: Vec<u8>,
}

fn dims(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

fn mip_bytes(width: u32, height: u32, level: u32, format: DdsFormat) -> usize {
    let (w, h) = dims(width, height, level);
    w.div_ceil(4) as usize * h.div_ceil(4) as usize * format.block_bytes()
}

/// Parse a DDS file. The payload must be exactly the sum of the block-sized
/// mip levels the header implies (no trailing or missing bytes), cube maps
/// and volumes are rejected (none ship).
pub fn parse(bytes: &[u8]) -> Result<Dds> {
    let dds = ddsfile::Dds::read(bytes).map_err(|e| FormatError::Inconsistent {
        what: "dds container",
        detail: e.to_string(),
    })?;
    let format = match dds.get_d3d_format() {
        Some(ddsfile::D3DFormat::DXT1) => DdsFormat::Dxt1,
        Some(ddsfile::D3DFormat::DXT3) => DdsFormat::Dxt3,
        Some(ddsfile::D3DFormat::DXT5) => DdsFormat::Dxt5,
        other => {
            return Err(FormatError::Inconsistent {
                what: "dds pixel format",
                detail: format!("unsupported {other:?}"),
            })
        }
    };
    if dds.get_num_array_layers() != 1 || dds.get_depth() != 1 {
        return Err(FormatError::Inconsistent {
            what: "dds layout",
            detail: "cube maps / volumes / arrays are not supported".to_string(),
        });
    }
    let (width, height) = (dds.get_width(), dds.get_height());
    let mip_count = dds.get_num_mipmap_levels();
    if width == 0 || height == 0 || mip_count == 0 {
        return Err(FormatError::Inconsistent {
            what: "dds header",
            detail: format!("{width}x{height} mips {mip_count}"),
        });
    }
    let expect: usize = (0..mip_count)
        .map(|l| mip_bytes(width, height, l, format))
        .sum();
    if dds.data.len() != expect {
        return Err(FormatError::Inconsistent {
            what: "dds payload",
            detail: format!(
                "{} bytes, header implies {expect} ({width}x{height} {format:?} x{mip_count} mips)",
                dds.data.len()
            ),
        });
    }
    Ok(Dds {
        width,
        height,
        format,
        mip_count,
        data: dds.data,
    })
}

impl Dds {
    /// Pixel size of `level` (`max(1, base >> level)`).
    pub fn mip_size(&self, level: u32) -> (u32, u32) {
        dims(self.width, self.height, level)
    }

    /// Decode one mip level to RGBA8.
    pub fn decode_mip(&self, level: u32) -> Result<Rgba8Image> {
        if level >= self.mip_count {
            return Err(FormatError::Inconsistent {
                what: "dds mip level",
                detail: format!("{level} >= {}", self.mip_count),
            });
        }
        let offset: usize = (0..level)
            .map(|l| mip_bytes(self.width, self.height, l, self.format))
            .sum();
        let len = mip_bytes(self.width, self.height, level, self.format);
        let (w, h) = self.mip_size(level);
        let mut argb = vec![0u32; w as usize * h as usize];
        let block = &self.data[offset..offset + len];
        let res = match self.format {
            DdsFormat::Dxt1 => {
                texture2ddecoder::decode_bc1(block, w as usize, h as usize, &mut argb)
            }
            DdsFormat::Dxt3 => {
                texture2ddecoder::decode_bc2(block, w as usize, h as usize, &mut argb)
            }
            DdsFormat::Dxt5 => {
                texture2ddecoder::decode_bc3(block, w as usize, h as usize, &mut argb)
            }
        };
        res.map_err(|e| FormatError::Inconsistent {
            what: "dds block decode",
            detail: e.to_string(),
        })?;
        let mut pixels = Vec::with_capacity(argb.len() * 4);
        for p in argb {
            // texture2ddecoder yields 0xAARRGGBB-in-BGRA-byte-order u32s.
            let [b, g, r, a] = p.to_le_bytes();
            pixels.extend_from_slice(&[r, g, b, a]);
        }
        Ok(Rgba8Image::new(w, h, pixels))
    }

    /// Decode the base level.
    pub fn decode(&self) -> Result<Rgba8Image> {
        self.decode_mip(0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-built DDS header (independent of `ddsfile`): 128 bytes.
    fn header(w: u32, h: u32, mips: u32, fourcc: &[u8; 4], linear: u32) -> Vec<u8> {
        let mut v = Vec::new();
        let mut put = |x: u32| v.extend_from_slice(&x.to_le_bytes());
        put(0x2053_4444); // "DDS "
        put(124);
        // CAPS|HEIGHT|WIDTH|PIXELFORMAT|LINEARSIZE (+MIPMAPCOUNT)
        put(0x1 | 0x2 | 0x4 | 0x1000 | 0x8_0000 | if mips > 1 { 0x2_0000 } else { 0 });
        put(h);
        put(w);
        put(linear);
        put(0);
        put(if mips > 1 { mips } else { 0 });
        for _ in 0..11 {
            put(0);
        }
        put(32); // pixel format size
        put(0x4); // DDPF_FOURCC
        put(u32::from_le_bytes(*fourcc));
        for _ in 0..5 {
            put(0);
        }
        put(0x1000 | if mips > 1 { 0x40_0008 } else { 0 }); // caps
        for _ in 0..4 {
            put(0);
        }
        assert_eq!(v.len(), 128);
        v
    }

    fn file(w: u32, h: u32, mips: u32, fourcc: &[u8; 4], payload: &[u8]) -> Vec<u8> {
        let mut v = header(w, h, mips, fourcc, payload.len() as u32);
        v.extend_from_slice(payload);
        v
    }

    #[test]
    fn dxt1_solid_red_block_decodes_to_red() {
        // colour0 = RGB565 0xF800 (red), colour1 = 0x001F (blue), all indices 0.
        let block = [0x00, 0xF8, 0x1F, 0x00, 0, 0, 0, 0];
        let dds = parse(&file(4, 4, 1, b"DXT1", &block)).unwrap();
        assert_eq!(
            (dds.width, dds.height, dds.format, dds.mip_count),
            (4, 4, DdsFormat::Dxt1, 1)
        );
        let img = dds.decode().unwrap();
        assert_eq!((img.width, img.height), (4, 4));
        for y in 0..4 {
            for x in 0..4 {
                assert_eq!(img.pixel(x, y), [255, 0, 0, 255]);
            }
        }
    }

    #[test]
    fn dxt3_explicit_alpha_nibbles() {
        // 8 bytes alpha (nibble per pixel, first row 0xF,0x0,0x8,0xF), then a
        // DXT1 colour block of solid green (0x07E0).
        let mut block = vec![0x0F, 0xF8, 0, 0, 0, 0, 0, 0];
        block.extend_from_slice(&[0xE0, 0x07, 0x00, 0x00, 0, 0, 0, 0]);
        let img = parse(&file(4, 4, 1, b"DXT3", &block))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(img.pixel(0, 0), [0, 255, 0, 255]);
        assert_eq!(img.pixel(1, 0)[3], 0);
        assert_eq!(img.pixel(2, 0)[3], 0x88);
        assert_eq!(img.pixel(3, 0)[3], 255);
    }

    #[test]
    fn dxt5_interpolated_alpha_endpoints() {
        // alpha0=255, alpha1=0, 3-bit indices: pixel0 -> 0 (255), pixel1 -> 1 (0).
        let mut block = vec![255, 0, 0b0000_1000, 0, 0, 0, 0, 0];
        block.extend_from_slice(&[0x1F, 0x00, 0x1F, 0x00, 0, 0, 0, 0]); // solid blue
        let img = parse(&file(4, 4, 1, b"DXT5", &block))
            .unwrap()
            .decode()
            .unwrap();
        assert_eq!(img.pixel(0, 0), [0, 0, 255, 255]);
        assert_eq!(img.pixel(1, 0), [0, 0, 255, 0]);
    }

    #[test]
    fn mip_chain_sizes_follow_the_header() {
        // 8x4 DXT1 with 3 mips: 8x4 (2 blocks), 4x2 (1 block), 2x1 (1 block).
        let payload = vec![0u8; (2 + 1 + 1) * 8];
        let dds = parse(&file(8, 4, 3, b"DXT1", &payload)).unwrap();
        assert_eq!(dds.mip_count, 3);
        assert_eq!(dds.mip_size(1), (4, 2));
        assert_eq!(dds.mip_size(2), (2, 1));
        let m2 = dds.decode_mip(2).unwrap();
        assert_eq!((m2.width, m2.height), (2, 1));
        assert!(dds.decode_mip(3).is_err());
    }

    #[test]
    fn wrong_payload_length_is_rejected() {
        let payload = vec![0u8; 8 + 3];
        assert!(parse(&file(4, 4, 1, b"DXT1", &payload)).is_err());
        assert!(parse(&file(4, 4, 1, b"DXT1", &[])).is_err());
    }

    #[test]
    fn bad_magic_and_unsupported_format_are_rejected() {
        assert!(parse(b"NOPE").is_err());
        assert!(parse(&file(4, 4, 1, b"ATI2", &[0; 16])).is_err());
    }
}

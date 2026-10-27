//! Version-4 material chunk (inner tag 0): the payload handed to the original
//! graphics device's material factory.
//!
//! `FUN_00502de0` passes the chunk payload to the device vtable slot `+0xcc`
//! (`0x4beef0`), which reads three header words and then `count` parameter
//! records through `FUN_004e6c50` (one record per iteration, the cursor
//! advancing by the size that routine returns):
//!
//! ```text
//! u32 kind    first word, kept for the device factory (`0x4befc0` dispatch)
//! u32 flags   bit 0 asks the device to keep a copy of the payload bytes
//! u32 count   number of parameter records
//! count * record: u32 tag, tag >> 24 (signed) selects the layout
//!   0 colour   u32 rgba, converted to four floats          (8 bytes)
//!   1 float    f32                                          (8 bytes)
//!   2 vec2     2 x f32                                      (12 bytes)
//!   3 vec3     3 x f32                                      (16 bytes)
//!   4 vec4     4 x f32                                      (20 bytes)
//!   5 int      u32                                          (8 bytes)
//!   6 block    7 raw words (stage descriptor)               (32 bytes)
//!   7 skipped  u32 consumed, not stored                     (8 bytes)
//!   8 name     32 bytes, at most 31 copied and NUL-capped   (36 bytes)
//!   other      tag word only                                (4 bytes)
//! ```
//!
//! exe: FUN_004e6c50
//! exe: FUN_004beef0

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// One parameter record. `tag` is the original word (`type << 24 | id`).
#[derive(Debug, Clone, PartialEq)]
pub enum Param {
    Colour {
        tag: u32,
        rgba: u32,
    },
    Float {
        tag: u32,
        value: f32,
    },
    Vec2 {
        tag: u32,
        value: [f32; 2],
    },
    Vec3 {
        tag: u32,
        value: [f32; 3],
    },
    Vec4 {
        tag: u32,
        value: [f32; 4],
    },
    Int {
        tag: u32,
        value: u32,
    },
    Block {
        tag: u32,
        words: [u32; 7],
    },
    /// Type 7: eight bytes consumed, nothing stored by the exe.
    Skipped {
        tag: u32,
        word: u32,
    },
    Name {
        tag: u32,
        text: String,
    },
    /// Tag outside `0..=8`: only the tag word is consumed.
    Unknown {
        tag: u32,
    },
}

impl Param {
    pub fn tag(&self) -> u32 {
        match self {
            Param::Colour { tag, .. }
            | Param::Float { tag, .. }
            | Param::Vec2 { tag, .. }
            | Param::Vec3 { tag, .. }
            | Param::Vec4 { tag, .. }
            | Param::Int { tag, .. }
            | Param::Block { tag, .. }
            | Param::Skipped { tag, .. }
            | Param::Name { tag, .. }
            | Param::Unknown { tag } => *tag,
        }
    }

    /// Bytes this record occupies in the payload (`FUN_004e6c50`'s return value).
    pub fn size(&self) -> usize {
        match self {
            Param::Colour { .. }
            | Param::Float { .. }
            | Param::Int { .. }
            | Param::Skipped { .. } => 8,
            Param::Vec2 { .. } => 12,
            Param::Vec3 { .. } => 16,
            Param::Vec4 { .. } => 20,
            Param::Block { .. } => 32,
            Param::Name { .. } => 36,
            Param::Unknown { .. } => 4,
        }
    }
}

/// A decoded material payload.
#[derive(Debug, Clone, PartialEq)]
pub struct Material {
    pub kind: u32,
    pub flags: u32,
    pub params: Vec<Param>,
    /// Payload bytes after the last record (never read by the exe).
    pub trailing: Vec<u8>,
}

/// Decode one record at the reader's cursor.
fn param(r: &mut Reader<'_>) -> Result<Param> {
    let tag = r.u32()?;
    // `sar 0x18` then unsigned `> 8`: negative types fall in the default arm.
    let kind = ((tag as i32) >> 24) as u32;
    Ok(match kind {
        0 => Param::Colour {
            tag,
            rgba: r.u32()?,
        },
        1 => Param::Float {
            tag,
            value: r.f32()?,
        },
        2 => Param::Vec2 {
            tag,
            value: [r.f32()?, r.f32()?],
        },
        3 => Param::Vec3 {
            tag,
            value: [r.f32()?, r.f32()?, r.f32()?],
        },
        4 => Param::Vec4 {
            tag,
            value: [r.f32()?, r.f32()?, r.f32()?, r.f32()?],
        },
        5 => Param::Int {
            tag,
            value: r.u32()?,
        },
        6 => {
            let mut words = [0u32; 7];
            for w in &mut words {
                *w = r.u32()?;
            }
            Param::Block { tag, words }
        }
        7 => Param::Skipped {
            tag,
            word: r.u32()?,
        },
        8 => {
            let raw = r.bytes(32)?;
            let end = raw.iter().take(31).position(|&b| b == 0).unwrap_or(31);
            Param::Name {
                tag,
                text: raw[..end].iter().map(|&b| b as char).collect(),
            }
        }
        _ => Param::Unknown { tag },
    })
}

/// Decode a material chunk payload; the byte after the last record are kept
/// in `trailing` so callers can account for every byte.
pub fn parse(payload: &[u8]) -> Result<Material> {
    let mut r = Reader::new(payload);
    let kind = r.u32()?;
    let flags = r.u32()?;
    let count = r.u32()? as usize;
    if count > payload.len() / 4 {
        return Err(FormatError::Inconsistent {
            what: "scx material",
            detail: format!("{count} records in {} bytes", payload.len()),
        });
    }
    let mut params = Vec::with_capacity(count);
    for _ in 0..count {
        params.push(param(&mut r)?);
    }
    let rest = r.remaining();
    Ok(Material {
        kind,
        flags,
        params,
        trailing: r.bytes(rest)?.to_vec(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn every_record_layout_advances_exactly_and_names_are_latin1_capped() {
        let mut bytes = Vec::new();
        let mut push = |word: u32| bytes.extend(word.to_le_bytes());
        for word in [3, 1, 11] {
            push(word);
        }
        for (kind, payload) in [
            (0, vec![0x11223344]),
            (1, vec![1f32.to_bits()]),
            (2, vec![2f32.to_bits(); 2]),
            (3, vec![3f32.to_bits(); 3]),
            (4, vec![4f32.to_bits(); 4]),
            (5, vec![19]),
            (6, vec![23; 7]),
            (7, vec![29]),
            (8, vec![0xe9e9e9e9; 8]),
            (9, vec![]),
            (255, vec![]),
        ] {
            push((kind << 24) | 5);
            for word in payload {
                push(word);
            }
        }
        let consumed = bytes.len();
        bytes.extend([7, 8, 9]);
        let m = parse(&bytes).unwrap();
        assert_eq!((m.kind, m.flags), (3, 1));
        assert_eq!(
            m.params.iter().map(Param::size).sum::<usize>() + 12,
            consumed
        );
        assert_eq!(m.trailing, [7, 8, 9]);
        assert!(matches!(&m.params[8], Param::Name{text,..} if text == &"é".repeat(31)));
        assert!(matches!(m.params[9], Param::Unknown { .. }));
        assert!(matches!(m.params[10], Param::Unknown { .. }));
        for n in 0..consumed {
            assert!(parse(&bytes[..n]).is_err(), "truncation {n}");
        }
    }
}

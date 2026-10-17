//! `INVO` mesh container (`*.scx`).
//!
//! Version 4 (2,587 of 3,028 shipped files) is a chunk table followed by contiguous chunks:
//!
//! ```text
//! 0x00 "INVO"  u32 version(=4)  u32 chunk_count  u32 0
//! 0x10 chunk_count * { u32 offset, u32 tag }      (chunks are contiguous, last ends at EOF)
//! tag 5 (vertex buffer): u32 ?, u32 chunk_len, u32 vertex_count, u32 fvf_flags, vertices...
//! tag 0 (index buffer) : u32 ?, u32 chunk_len, u32 index_count, u16 indices...  (triangle list)
//! tags 1/2/3/4        : material / object metadata, kept opaque here
//! ```
//!
//! Vertex stride is `(chunk_len - 16) / vertex_count`; the FVF flags (0x1 position,
//! 0x40 normal, 0x200 texcoord) place the attributes. Version 3 (441 files, all map pieces)
//! uses a different record layout and is only header-validated.

use crate::reader::Reader;
use crate::{FormatError, Result};

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub flags: u32,
    pub stride: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

#[derive(Debug, Clone)]
pub enum Scx {
    V4 {
        meshes: Vec<Mesh>,
        chunk_tags: Vec<u32>,
    },
    /// Version 3 map-piece file: header validated only.
    V3 { size: usize },
}

pub fn parse(data: &[u8]) -> Result<Scx> {
    let mut r = Reader::new(data);
    let magic = r.bytes(4)?;
    if magic != b"INVO" {
        return Err(FormatError::BadMagic {
            expected: "INVO",
            found: String::from_utf8_lossy(magic).into_owned(),
        });
    }
    let version = r.u32()?;
    match version {
        3 => Ok(Scx::V3 { size: data.len() }),
        4 => parse_v4(data),
        v => Err(FormatError::UnsupportedVersion(v)),
    }
}

fn parse_v4(data: &[u8]) -> Result<Scx> {
    let mut r = Reader::at(data, 8);
    let count = r.u32()? as usize;
    let _zero = if r.remaining() >= 4 { r.u32()? } else { 0 };
    let mut table = Vec::with_capacity(count);
    for _ in 0..count {
        let off = r.u32()? as usize;
        let tag = r.u32()?;
        table.push((off, tag));
    }
    let mut meshes = Vec::new();
    let mut pending: Option<Mesh> = None;
    for (i, &(off, tag)) in table.iter().enumerate() {
        let end = table.get(i + 1).map(|t| t.0).unwrap_or(data.len());
        if off > end || end > data.len() {
            return Err(FormatError::Inconsistent {
                what: "scx chunk range",
                detail: format!("chunk {i} {off}..{end} in {} bytes", data.len()),
            });
        }
        let chunk = &data[off..end];
        match tag {
            5 => {
                let mut c = Reader::new(chunk);
                let _a = c.u32()?;
                let len = c.u32()? as usize;
                let n = c.u32()? as usize;
                let flags = c.u32()?;
                if len != chunk.len() || n == 0 || !(len - 16).is_multiple_of(n) {
                    return Err(FormatError::Inconsistent {
                        what: "scx vertex chunk",
                        detail: format!("len {len} chunk {} verts {n}", chunk.len()),
                    });
                }
                let stride = (len - 16) / n;
                let mut m = Mesh {
                    flags,
                    stride,
                    ..Default::default()
                };
                for v in 0..n {
                    let base = 16 + v * stride;
                    let mut vr = Reader::at(chunk, base);
                    m.positions.push([vr.f32()?, vr.f32()?, vr.f32()?]);
                    if flags & 0x40 != 0 && stride >= 24 {
                        m.normals.push([vr.f32()?, vr.f32()?, vr.f32()?]);
                    }
                    if flags & 0x200 != 0 && stride >= 32 {
                        m.uvs.push([vr.f32()?, vr.f32()?]);
                    }
                }
                if let Some(p) = pending.replace(m) {
                    meshes.push(p);
                }
            }
            0 => {
                let mut c = Reader::new(chunk);
                let _a = c.u32()?;
                let len = c.u32()? as usize;
                let n = c.u32()? as usize;
                if len != chunk.len() || len != 12 + 2 * n {
                    return Err(FormatError::Inconsistent {
                        what: "scx index chunk",
                        detail: format!("len {len} chunk {} indices {n}", chunk.len()),
                    });
                }
                let mut idx = Vec::with_capacity(n);
                for _ in 0..n {
                    idx.push(c.u16()?);
                }
                if let Some(mut m) = pending.take() {
                    let nv = m.positions.len();
                    if idx.iter().any(|&i| i as usize >= nv) {
                        return Err(FormatError::Inconsistent {
                            what: "scx index range",
                            detail: format!("index beyond {nv} vertices"),
                        });
                    }
                    m.indices = idx;
                    meshes.push(m);
                }
            }
            _ => {}
        }
    }
    if let Some(p) = pending.take() {
        meshes.push(p);
    }
    Ok(Scx::V4 {
        meshes,
        chunk_tags: table.iter().map(|t| t.1).collect(),
    })
}

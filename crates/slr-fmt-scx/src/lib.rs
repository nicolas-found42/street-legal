//! `INVO` mesh container (`*.scx`, `*.scy`).
//!
//! Version 4 (2,531 stock files) is a chunk table followed by contiguous chunks:
//!
//! ```text
//! 0x00 "INVO"  u32 version(=4)  u32 chunk_count
//! 0x0c chunk_count * { u32 tag, u32 offset }    (last chunk ends at EOF)
//! tag 0 (material): u32 tag, u32 chunk_len, material payload...
//! tags 1/2/3: u32 tag, u32 chunk_len, object metadata...
//! tag 4 (vertices): u32 tag, u32 chunk_len, u32 vertex_count, u32 fvf_flags, vertices...
//! tag 5 (indices): u32 tag, u32 chunk_len, u32 index_count, u16 indices...
//! ```
//!
//! Vertex stride is `(chunk_len - 16) / vertex_count`; the FVF flags (0x1 position,
//! 0x40 normal, 0x80/0x100 packed colors, 0x200 texcoord) place the attributes.
//!
//! Version 3 (390 stock files: map pieces, fonts, the water plane; plus all 13
//! stock `*.scy` collision-surface files, which share the container) is a flat
//! sequence of material groups:
//!
//! ```text
//! 0x00 "INVO"  u32 version(=3)
//! per group: u32 material_size (0x38 | 0x68 | 0x88, size word included)
//!            u8[material_size - 4] material payload (kept opaque; see `Material`)
//!            u32 vertex_count, vertex_count * stride bytes of vertices
//!            u32 face_count, face_count * 3 u32 indices (triangle list)
//! optional trailing u32 0; the last group may instead end exactly at EOF
//! ```
//!
//! The vertex stride is `0x2c` after a `0x38` material block, otherwise the u32
//! at material offset 60 (always 64 in the shipped corpus). Each vertex holds
//! `pos[3f] normal[3f] uv[2f]` followed by `stride - 32` raw bytes (a second UV
//! pair, a colour-like word which is `0xffffffff` for unpainted vertices, and a
//! 20-byte tail are visible in garage meshes, but the exe's loader reads only
//! the positions, so the remainder is preserved opaquely; see `V3Mesh::extra`).
//!
//! File units are centimetres: the exe multiplies stored positions by `0.01`
//! (`_DAT_005f1118` at file offset `0x1f1118` of
//! `StreetLegal_Redline.bak.exe`); like the v4 decoder, positions are kept here
//! in raw file units.
//!
//! `*.scy` files use the identical container with every group carrying zero
//! vertices and zero faces; their payload is the named surface-type table
//! (`Material::name`, e.g. `"aszfalt"`, `"szalagkorlat"`, `"Material #0"`).
//! The collision geometry itself is not in these files.

pub mod material;

use slr_fmt_core::reader::{latin1_cstr, Reader};
use slr_fmt_core::{FormatError, Result};

#[derive(Debug, Clone, Default)]
pub struct Mesh {
    pub flags: u32,
    pub stride: usize,
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub indices: Vec<u16>,
}

/// One version-3 material + geometry group. Ports the v3 loop: per iteration
/// the material block is skipped by its size word, the vertex count is the
/// next word, and the face count sits after the vertex array.
///
/// exe: FUN_004b5c30
#[derive(Debug, Clone)]
pub struct V3Group {
    /// Material block size word (`0x38`, `0x68` or `0x88`): the loader
    /// dispatches on it (`FUN_004b5c30` `uVar3` block).
    pub material_size: u32,
    /// Raw material payload (block after the size word): skipped by size,
    /// keeping only the stride; nothing else is read from it.
    pub material: Vec<u8>,
    /// Vertex stride in bytes: `0x2c` after a `0x38` block, else the u32 at
    /// material offset 60.
    pub stride: usize,
    /// Triangle-list vertices: `pos normal uv` + raw remainder per vertex.
    /// (First three words per stride, scaled by `0.01`.)
    pub mesh: V3Mesh,
}

/// Decoded version-3 vertex data of one group.
#[derive(Debug, Clone, Default)]
pub struct V3Mesh {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    /// Raw bytes `32..stride` of each vertex, preserved verbatim (second UV
    /// pair, colour-like word, tail; semantics undecoded).
    pub extra: Vec<Vec<u8>>,
    /// Triangle list, `u32` indices plus the group's base vertex offset.
    pub indices: Vec<u32>,
}

impl V3Group {
    /// Best-effort material name: the NUL-padded text field at payload offset
    /// `0x64` of `0x88` blocks (`"23 - Default"`, `"aszfalt"`, ...); `""`
    /// when absent (all `0x38`/`0x68` blocks).
    pub fn name(&self) -> String {
        if self.material.len() < 0x64 {
            return String::new();
        }
        let raw = &self.material[0x64..];
        let start = raw.iter().position(|&b| b != 0).unwrap_or(raw.len());
        latin1_cstr(&raw[start..])
    }
}

/// Tag 1/2/3 chunk: an object descriptor whose ids the exe feeds to the
/// object's `+0x3c` method (`FUN_00502de0` cases 1..3). Word 0 of the payload
/// is never read.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Object {
    pub kind: u32,
    pub ids: Vec<u32>,
}

#[derive(Debug, Clone)]
pub enum Scx {
    V4 {
        meshes: Vec<Mesh>,
        /// Inner tag of each chunk in file order.
        chunk_tags: Vec<u32>,
        /// Tag 0 chunks, in file order.
        materials: Vec<material::Material>,
        /// Tag 1/2/3 chunks, in file order.
        objects: Vec<Object>,
    },
    V3 {
        groups: Vec<V3Group>,
        /// Trailing word: `None` when the last group ends exactly at EOF,
        /// `Some(0)` for the usual zero terminator (the loader breaks when
        /// the cursor reaches the end or reads a zero word).
        trailing: Option<u32>,
    },
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
        3 => parse_v3(data),
        4 => parse_v4(data),
        v => Err(FormatError::UnsupportedVersion(v)),
    }
}

/// v3 loop: per iteration the material block is skipped by its size word,
/// the vertex count is the next word, and the face count sits right after
/// the vertex array; faces are bare `u32` triples.
///
/// exe: FUN_004b5c30
fn parse_v3(data: &[u8]) -> Result<Scx> {
    let mut groups = Vec::new();
    let mut cur = 8usize;
    loop {
        let size = Reader::at(data, cur).u32()? as usize;
        if !matches!(size, 0x38 | 0x68 | 0x88) {
            return Err(FormatError::Inconsistent {
                what: "scx v3 material size",
                detail: format!("group {} block {size:#x} at {cur:#x}", groups.len()),
            });
        }
        let stride = if size == 0x38 {
            0x2c
        } else {
            Reader::at(data, cur + 60).u32()? as usize
        };
        if stride < 32 || stride % 4 != 0 {
            return Err(FormatError::Inconsistent {
                what: "scx v3 vertex stride",
                detail: format!("group {} stride {stride}", groups.len()),
            });
        }
        let material = data
            .get(cur + 4..cur + size)
            .ok_or(FormatError::Truncated {
                at: cur + 4,
                need: size - 4,
                have: data.len().saturating_sub(cur + 4),
            })?
            .to_vec();
        cur += size;
        let nv = Reader::at(data, cur).u32()? as usize;
        let vo = cur + 4;
        let fco = vo
            .checked_add(nv.checked_mul(stride).ok_or(FormatError::Inconsistent {
                what: "scx v3 vertex range",
                detail: format!("group {} {nv} verts x {stride}", groups.len()),
            })?)
            .ok_or(FormatError::Inconsistent {
                what: "scx v3 vertex range",
                detail: format!("group {} ends past EOF", groups.len()),
            })?;
        if fco + 4 > data.len() {
            return Err(FormatError::Truncated {
                at: fco,
                need: 4,
                have: data.len().saturating_sub(fco),
            });
        }
        let nf = Reader::at(data, fco).u32()? as usize;
        let fo = fco + 4;
        let end = fo
            .checked_add(nf.checked_mul(12).ok_or(FormatError::Inconsistent {
                what: "scx v3 face range",
                detail: format!("group {} {nf} faces", groups.len()),
            })?)
            .ok_or(FormatError::Inconsistent {
                what: "scx v3 face range",
                detail: format!("group {} ends past EOF", groups.len()),
            })?;
        if end > data.len() {
            return Err(FormatError::Truncated {
                at: fo,
                need: nf * 12,
                have: data.len().saturating_sub(fo),
            });
        }
        let mut mesh = V3Mesh::default();
        for v in 0..nv {
            let o = vo + v * stride;
            let mut vr = Reader::at(data, o);
            let pos = [vr.f32()?, vr.f32()?, vr.f32()?];
            let nrm = [vr.f32()?, vr.f32()?, vr.f32()?];
            let uv = [vr.f32()?, vr.f32()?];
            mesh.positions.push(pos);
            mesh.normals.push(nrm);
            mesh.uvs.push(uv);
            mesh.extra.push(data[o + 32..o + stride].to_vec());
        }
        let base: u32 = groups
            .iter()
            .map(|g: &V3Group| g.mesh.positions.len() as u32)
            .sum();
        let mut ir = Reader::at(data, fo);
        for _ in 0..nf * 3 {
            let i = ir.u32()?;
            if i as usize >= nv {
                return Err(FormatError::Inconsistent {
                    what: "scx v3 index range",
                    detail: format!("group {} index {i} beyond {nv} vertices", groups.len()),
                });
            }
            mesh.indices.push(i + base);
        }
        groups.push(V3Group {
            material_size: size as u32,
            material,
            stride,
            mesh,
        });
        if end == data.len() {
            return Ok(Scx::V3 {
                groups,
                trailing: None,
            });
        }
        if end + 4 == data.len() {
            let t = Reader::at(data, end).u32()?;
            if t != 0 {
                return Err(FormatError::Inconsistent {
                    what: "scx v3 trailer",
                    detail: format!("expected zero terminator, found {t:#x}"),
                });
            }
            return Ok(Scx::V3 {
                groups,
                trailing: Some(0),
            });
        }
        cur = end;
    }
}

/// Version 4, as the exe walks it (`FUN_00502de0`): `count` chunks follow
/// the `(tag, offset)` table, each `u32 tag, u32 size` (size includes these
/// two words) plus payload; the table repeats each chunk's tag and offset and
/// the last chunk ends exactly at EOF.
///
/// exe: FUN_00502de0
fn parse_v4(data: &[u8]) -> Result<Scx> {
    let mut r = Reader::at(data, 8);
    let count = r.u32()? as usize;
    let mut table = Vec::new();
    for _ in 0..count {
        let tag = r.u32()?;
        let off = r.u32()? as usize;
        table.push((tag, off));
    }
    let mut meshes = Vec::new();
    let mut materials = Vec::new();
    let mut objects = Vec::new();
    let mut pending: Option<Mesh> = None;
    let mut cursor = r.pos;
    let mut chunk_tags = Vec::with_capacity(count);
    for (i, &(table_tag, table_off)) in table.iter().enumerate() {
        let mut h = Reader::at(data, cursor);
        let tag = h.u32()?;
        let size = h.u32()? as usize;
        if tag != table_tag || cursor != table_off || size < 8 || cursor + size > data.len() {
            return Err(FormatError::Inconsistent {
                what: "scx chunk range",
                detail: format!(
                    "chunk {i} tag {tag} at {cursor} size {size}; table says tag {table_tag} at {table_off}"
                ),
            });
        }
        chunk_tags.push(tag);
        let chunk = &data[cursor..cursor + size];
        let payload = &chunk[8..];
        match tag {
            0 => materials.push(material::parse(payload)?),
            1 => objects.push(Object {
                kind: 1,
                ids: Vec::new(),
            }),
            2 => {
                let mut c = Reader::new(payload);
                let _reserved = c.u32()?;
                objects.push(Object {
                    kind: 2,
                    ids: vec![c.u32()?, c.u32()?],
                });
            }
            3 => {
                let mut c = Reader::new(payload);
                let _reserved = c.u32()?;
                let n = c.u32()? as usize;
                if payload.len() != 8 + 4 * n {
                    return Err(FormatError::Inconsistent {
                        what: "scx object chunk",
                        detail: format!("{n} ids in {} payload bytes", payload.len()),
                    });
                }
                let mut ids = Vec::with_capacity(n);
                for _ in 0..n {
                    ids.push(c.u32()?);
                }
                objects.push(Object { kind: 3, ids });
            }
            4 => {
                let mut c = Reader::new(payload);
                let n = c.u32()? as usize;
                let flags = c.u32()?;
                let len = size;
                if n == 0 || !(len - 16).is_multiple_of(n) {
                    return Err(FormatError::Inconsistent {
                        what: "scx vertex chunk",
                        detail: format!("len {len} verts {n}"),
                    });
                }
                let stride = (len - 16) / n;
                let attributes = 12
                    + usize::from(flags & 0x40 != 0) * 12
                    + usize::from(flags & 0x80 != 0) * 4
                    + usize::from(flags & 0x100 != 0) * 4
                    + usize::from(flags & 0x200 != 0) * 8;
                if stride < attributes {
                    return Err(FormatError::Inconsistent {
                        what: "scx vertex attributes",
                        detail: format!("stride {stride} needs {attributes} bytes"),
                    });
                }
                let mut m = Mesh {
                    flags,
                    stride,
                    ..Default::default()
                };
                for v in 0..n {
                    let mut vr = Reader::at(chunk, 16 + v * stride);
                    m.positions.push([vr.f32()?, vr.f32()?, vr.f32()?]);
                    if flags & 0x40 != 0 {
                        m.normals.push([vr.f32()?, vr.f32()?, vr.f32()?]);
                    }
                    // Custom FVF: packed diffuse/specular words follow normals.
                    // Reading them as UVs yields NaNs for opaque white vertices.
                    if flags & 0x80 != 0 {
                        vr.u32()?;
                    }
                    if flags & 0x100 != 0 {
                        vr.u32()?;
                    }
                    if flags & 0x200 != 0 {
                        m.uvs.push([vr.f32()?, vr.f32()?]);
                    }
                }
                if let Some(p) = pending.replace(m) {
                    meshes.push(p);
                }
            }
            5 => {
                let mut c = Reader::new(payload);
                let n = c.u32()? as usize;
                if size != 12 + 2 * n {
                    return Err(FormatError::Inconsistent {
                        what: "scx index chunk",
                        detail: format!("len {size} indices {n}"),
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
            // The exe's switch has no default arm: an unknown chunk is skipped
            // by its size word.
            _ => {}
        }
        cursor += size;
    }
    if cursor != data.len() {
        return Err(FormatError::Inconsistent {
            what: "scx trailing bytes",
            detail: format!("{} bytes after the last chunk", data.len() - cursor),
        });
    }
    if let Some(p) = pending.take() {
        meshes.push(p);
    }
    Ok(Scx::V4 {
        meshes,
        chunk_tags,
        materials,
        objects,
    })
}

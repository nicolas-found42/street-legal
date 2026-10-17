//! Map binary payload (`*.rpk` root-entry tree) reader.
//!
//! The four stock map packages (`city`, `city2`, `ROC_track`, `test_track`) hold a
//! serialized object tree in the gap between the root entry's descriptor and the tail
//! descriptors. Every byte of that gap belongs to exactly one range, and every range
//! has the same shape (verified on all four maps, zero overlaps):
//!
//! ```text
//! range  ::= u32 zero(=0)  u32 count  (count x record)  chunk*
//! record ::= u32 k1  u32 k2  u8 a(=1)  u8 b  u32 z/f1  u32 x  u32 y
//!            u8 n  u8 name[n]  f32 bbox[6]  u8 pad[24]      (stride 71+n)
//! chunk  ::= u32 tag  u32 size  u8 payload[size]
//! ```
//!
//! A record's `(x, y)` is the absolute file range of a child range (`b == 0x01` links
//! inner nodes, `b == 0x21` links named placements); a range with `count == 0` holds
//! only chunks. The root entry's descriptor has the same shape (`count` records, then
//! an `RSD` chunk and, in `city` only, an `EXTP` chunk).
//!
//! Chunk tags seen in the corpus: `RSD\0`, `PHYS`, `POLY` (plus `EXTP` in the city root
//! descriptor only). Layouts below mirror the exe's map loader:
//!
//! - `RSD\0`: text lines (`gametype ...`, `params ...`, sometimes `flags ...`); the
//!   `0x352`/`0x15` gametypes carry opaque token bytes instead of text.
//!
//!   exe: FUN_0053dee0 (RSD line parse via FUN_005d7190/FUN_00551240)
//! - `PHYS`: `u32 nplanes`, then `nplanes` x 7-dword planes
//!   (`u32 w0`, `u32 i0/i1/i2`, `f32 nx/ny/nz`), then `u32 nverts`, then `nverts`
//!   x `f32 x/y/z` vertices. `w0`/`i*` meanings are open; positions are centimetres.
//!
//!   exe: FUN_0053dee0 (PHYS branch: n1 x 7 dwords, n2 x 3 dwords)
//! - `POLY`: `u32 1`, `u32 ngroups`, then per group `u32 f0(=1)`, `u32 nvert`,
//!   `u32 nidx`, `nvert` x 56-byte vertices, `nidx` x 12-byte index triples.
//!   Vertex attribute split is open (first 6 floats read as position+normal).
//!
//!   exe: FUN_0053dee0 (POLY branch: groups of 14-dword verts, 3-dword indices)
//! - `EXTP`: `size/4` x `u32` exterior-package type ids.
//!
//!   exe: FUN_0053dee0 (EXTP branch via FUN_00544370)
//!
//! Record field offsets (`a`/`b` at +8/+9, `x`/`y` at +14/+18, name length at +22,
//! name/bbox at +23, stride `71+n`) mirror the exe's record parser; `k1` chains match
//! (`k1` of a child equals the parent record's `k2`).
//! exe: FUN_00544010 (record walk, stride puVar2+0xc)

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// A placement record: links a child `(offset, size)` range with bounds and a name.
#[derive(Debug, Clone)]
pub struct Record {
    pub k1: u32,
    pub k2: u32,
    /// Record kind: `0x01` inner link, `0x21` named placement (exe treats both alike).
    pub b: u8,
    /// Absolute file offset of the child range.
    pub x: u32,
    /// Byte size of the child range.
    pub y: u32,
    /// Placement name without the trailing NUL (`road320`, `ambience_plh07`, ...).
    pub name: Vec<u8>,
    /// Placement bounds, 6 floats (units are centimetres; axes open).
    pub bbox: [f32; 6],
    /// File offset of this record.
    pub offset: usize,
}
impl Record {
    /// Name length including the trailing NUL (empty names stored none).
    fn n_len(&self) -> u8 {
        u8::try_from(self.name.len() + usize::from(!self.name.is_empty())).unwrap()
    }

    /// Byte size of this record on disk.
    #[inline]
    pub fn stride(&self) -> usize {
        record_stride(self.n_len())
    }
}

/// Byte size of a record once its name length `n` is known.
#[inline]
pub fn record_stride(n: u8) -> usize {
    71 + n as usize
}

fn parse_record_at(data: &[u8], offset: usize) -> Result<(Record, usize)> {
    let mut r = Reader::at(data, offset);
    let k1 = r.u32()?;
    let k2 = r.u32()?;
    let a = r.u8()?;
    let b = r.u8()?;
    if a != 1 || (b != 0x01 && b != 0x21) {
        return Err(FormatError::Inconsistent {
            what: "map record kind",
            detail: format!("a={a:#x} b={b:#x} at {offset}"),
        });
    }
    if b == 0x01 {
        let z = r.u32()?;
        if z != 0 {
            return Err(FormatError::Inconsistent {
                what: "map link record",
                detail: format!("z={z:#x} at {offset}"),
            });
        }
    } else {
        let f1 = r.f32()?;
        if f1 != 1.0 {
            return Err(FormatError::Inconsistent {
                what: "map placement record",
                detail: format!("f1={f1} at {offset}"),
            });
        }
    }
    let x = r.u32()?;
    let y = r.u32()?;
    let n = r.u8()? as usize;
    let name_nul = r.bytes(n)?;
    if n > 0 && (name_nul[n - 1] != 0 || name_nul[..n - 1].iter().any(|&c| !(32..127).contains(&c)))
    {
        return Err(FormatError::Inconsistent {
            what: "map record name",
            detail: format!("{n} bytes at {offset}"),
        });
    }
    let mut bbox = [0f32; 6];
    for v in bbox.iter_mut() {
        *v = r.f32()?;
    }
    if bbox.iter().any(|v| !v.is_finite()) {
        return Err(FormatError::Inconsistent {
            what: "map record bbox",
            detail: format!("non-finite at {offset}"),
        });
    }
    let pad = r.bytes(24)?;
    if pad.iter().any(|&c| c != 0) {
        return Err(FormatError::Inconsistent {
            what: "map record pad",
            detail: format!("nonzero at {offset}"),
        });
    }
    let stride = record_stride(n as u8);
    debug_assert_eq!(r.pos - offset, stride);
    Ok((
        Record {
            k1,
            k2,
            b,
            x,
            y,
            name: name_nul[..n.saturating_sub(1)].to_vec(),
            bbox,
            offset,
        },
        stride,
    ))
}

/// A typed payload chunk.
#[derive(Debug, Clone)]
pub enum Chunk {
    /// `RSD\0`: raw payload bytes plus the text lines when the payload is clean text.
    Rsd {
        offset: usize,
        bytes: Vec<u8>,
        lines: Option<Vec<String>>,
    },
    /// `PHYS`: collision planes plus vertices. `plane = [w0, i0, i1, i2]` semantics open.
    Phys {
        offset: usize,
        planes: Vec<PhysPlane>,
        verts: Vec<[f32; 3]>,
    },
    /// `POLY`: render groups. Vertex attribute split open; kept as raw 56-byte verts.
    Poly {
        offset: usize,
        groups: Vec<PolyGroup>,
    },
    /// `EXTP`: exterior-package type ids.
    Extp { offset: usize, ids: Vec<u32> },
    /// Any other tag: opaque bytes (none occur in the stock corpus).
    Unknown {
        tag: [u8; 4],
        offset: usize,
        bytes: Vec<u8>,
    },
}

impl Chunk {
    pub fn tag(&self) -> [u8; 4] {
        match self {
            Chunk::Rsd { .. } => *b"RSD\x00",
            Chunk::Phys { .. } => *b"PHYS",
            Chunk::Poly { .. } => *b"POLY",
            Chunk::Extp { .. } => *b"EXTP",
            Chunk::Unknown { tag, .. } => *tag,
        }
    }

    pub fn offset(&self) -> usize {
        match self {
            Chunk::Rsd { offset, .. }
            | Chunk::Phys { offset, .. }
            | Chunk::Poly { offset, .. }
            | Chunk::Extp { offset, .. }
            | Chunk::Unknown { offset, .. } => *offset,
        }
    }

    pub fn len(&self) -> usize {
        self.payload_len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.payload_len() == 0
    }

    fn payload_len(&self) -> usize {
        match self {
            Chunk::Rsd { bytes, .. } => bytes.len(),
            Chunk::Phys { planes, verts, .. } => 4 + planes.len() * 28 + 4 + verts.len() * 12,
            Chunk::Poly { groups, .. } => {
                8 + groups
                    .iter()
                    .map(|g| 12 + g.verts.len() * 56 + g.indices.len() * 12)
                    .sum::<usize>()
            }
            Chunk::Extp { ids, .. } => ids.len() * 4,
            Chunk::Unknown { bytes, .. } => bytes.len(),
        }
    }
}

/// One `PHYS` plane: open id word, three open refs, unit-ish normal.
#[derive(Debug, Clone)]
pub struct PhysPlane {
    pub w0: u32,
    pub refs: [u32; 3],
    pub normal: [f32; 3],
}

/// One `POLY` group: raw vertices (56 bytes each) and index triples (12 bytes each).
#[derive(Debug, Clone)]
pub struct PolyGroup {
    pub f0: u32,
    pub verts: Vec<[u8; 56]>,
    pub indices: Vec<[u8; 12]>,
}

pub fn parse_chunk_at(data: &[u8], offset: usize, end: usize) -> Result<(Chunk, usize)> {
    let mut r = Reader::at(data, offset);
    let tag: [u8; 4] = r.bytes(4)?.try_into().unwrap();
    let size = r.u32()? as usize;
    if r.pos + size > end {
        return Err(FormatError::Inconsistent {
            what: "map chunk range",
            detail: format!("{tag:?} size {size} at {offset} overruns range end {end}"),
        });
    }
    let payload = r.bytes(size)?;
    let chunk = match &tag {
        b"RSD\x00" => {
            let lines = if payload
                .iter()
                .all(|&c| c == b'\r' || c == b'\n' || (32..127).contains(&c))
            {
                Some(
                    payload
                        .split(|&c| c == b'\n')
                        .map(|l| {
                            let l = l.strip_suffix(b"\r").unwrap_or(l);
                            String::from_utf8_lossy(l).into_owned()
                        })
                        .collect(),
                )
            } else {
                None
            };
            Chunk::Rsd {
                offset: offset + 8,
                bytes: payload.to_vec(),
                lines,
            }
        }
        b"PHYS" => {
            let mut p = Reader::new(payload);
            let nplanes = p.u32()? as usize;
            let mut planes = Vec::with_capacity(nplanes);
            for _ in 0..nplanes {
                let w0 = p.u32()?;
                let refs = [p.u32()?, p.u32()?, p.u32()?];
                let normal = [p.f32()?, p.f32()?, p.f32()?];
                planes.push(PhysPlane { w0, refs, normal });
            }
            let nverts = p.u32()? as usize;
            let mut verts = Vec::with_capacity(nverts);
            for _ in 0..nverts {
                verts.push([p.f32()?, p.f32()?, p.f32()?]);
            }
            if p.remaining() != 0 {
                return Err(FormatError::Inconsistent {
                    what: "map PHYS body",
                    detail: format!("{} trailing bytes at {offset}", p.remaining()),
                });
            }
            Chunk::Phys {
                offset: offset + 8,
                planes,
                verts,
            }
        }
        b"POLY" => {
            let mut p = Reader::new(payload);
            let w0 = p.u32()?;
            let ngroups = p.u32()? as usize;
            if w0 != 1 {
                return Err(FormatError::Inconsistent {
                    what: "map POLY header",
                    detail: format!("w0={w0} at {offset}"),
                });
            }
            let mut groups = Vec::with_capacity(ngroups);
            for _ in 0..ngroups {
                let f0 = p.u32()?;
                let nvert = p.u32()? as usize;
                let nidx = p.u32()? as usize;
                let mut verts = Vec::with_capacity(nvert);
                for _ in 0..nvert {
                    verts.push(p.bytes(56)?.try_into().unwrap());
                }
                let mut indices = Vec::with_capacity(nidx);
                for _ in 0..nidx {
                    indices.push(p.bytes(12)?.try_into().unwrap());
                }
                groups.push(PolyGroup { f0, verts, indices });
            }
            if p.remaining() != 0 {
                return Err(FormatError::Inconsistent {
                    what: "map POLY body",
                    detail: format!("{} trailing bytes at {offset}", p.remaining()),
                });
            }
            Chunk::Poly {
                offset: offset + 8,
                groups,
            }
        }
        b"EXTP" => {
            if !size.is_multiple_of(4) {
                return Err(FormatError::Inconsistent {
                    what: "map EXTP body",
                    detail: format!("size {size} at {offset}"),
                });
            }
            let mut p = Reader::new(payload);
            let mut ids = Vec::with_capacity(size / 4);
            while p.remaining() > 0 {
                ids.push(p.u32()?);
            }
            Chunk::Extp {
                offset: offset + 8,
                ids,
            }
        }
        _ => Chunk::Unknown {
            tag,
            offset: offset + 8,
            bytes: payload.to_vec(),
        },
    };
    Ok((chunk, 8 + size))
}

/// One payload range: its records plus the trailing chunk stream.
///
/// `header_len` is `8 + sum(record strides)`; the chunk stream is
/// `[offset + header_len, offset + size)`.
#[derive(Debug, Clone)]
pub struct Range {
    pub offset: usize,
    pub size: usize,
    pub records: Vec<Record>,
    pub header_len: usize,
    pub chunks: Vec<Chunk>,
}

impl Range {
    pub fn tail(&self) -> (usize, usize) {
        (self.offset + self.header_len, self.offset + self.size)
    }
}

pub fn parse_range_at(data: &[u8], offset: usize, size: usize) -> Result<Range> {
    let end = offset + size;
    if end > data.len() {
        return Err(FormatError::Truncated {
            at: offset,
            need: size,
            have: data.len().saturating_sub(offset),
        });
    }
    let mut r = Reader::at(data, offset);
    let zero = r.u32()?;
    if zero != 0 {
        return Err(FormatError::Inconsistent {
            what: "map range header",
            detail: format!("zero={zero:#x} at {offset}"),
        });
    }
    let count = r.u32()? as usize;
    if count > 2000 {
        return Err(FormatError::Inconsistent {
            what: "map range header",
            detail: format!("count={count} at {offset}"),
        });
    }
    let mut records = Vec::with_capacity(count);
    for _ in 0..count {
        let (rec, stride) = parse_record_at(data, r.pos)?;
        let _ = stride;
        r.pos += record_stride(rec.n_len());
        records.push(rec);
    }
    let header_len = r.pos - offset;
    if header_len > size {
        return Err(FormatError::Inconsistent {
            what: "map range records",
            detail: format!("records run past range end at {offset}"),
        });
    }
    let mut chunks = Vec::new();
    let mut pos = r.pos;
    while pos < end {
        let (chunk, stride) = parse_chunk_at(data, pos, end)?;
        chunks.push(chunk);
        pos += stride;
    }
    Ok(Range {
        offset,
        size,
        records,
        header_len,
        chunks,
    })
}

/// Walk the whole map payload: the root descriptor's records seed a traversal where
/// every record's `(x, y)` range is parsed once (keyed by offset).
///
/// Returns all visited ranges (root descriptor excluded) in visit order.
/// Every range's chunks are fully parsed, so a clean walk accounts for every payload
/// byte exactly once (records via [`Range::header_len`], the rest via chunks).
pub fn walk_payload(data: &[u8], root_offset: usize, root_size: usize) -> Result<Vec<Range>> {
    let root = parse_range_at(data, root_offset, root_size)?;
    walk_ranges(data, &root.records)
}

/// Walk ranges seeded by explicit records (the root descriptor's, or any subset).
/// Returns all visited ranges in visit order; each offset is visited at most once.
pub fn walk_ranges(data: &[u8], seed: &[Record]) -> Result<Vec<Range>> {
    let mut out = Vec::new();
    let mut stack: Vec<Record> = seed.to_vec();
    let mut seen = std::collections::BTreeSet::new();
    while let Some(rec) = stack.pop() {
        if !seen.insert(rec.x as usize) {
            continue;
        }
        let range = parse_range_at(data, rec.x as usize, rec.y as usize)?;
        stack.extend(range.records.iter().cloned());
        out.push(range);
    }
    Ok(out)
}

/// Parse the root descriptor's record list (the payload walk seeds).
/// Returns the records and the byte length they occupy (`8 + sum(strides)`).
pub fn parse_root_records(
    data: &[u8],
    root_offset: usize,
    root_size: usize,
) -> Result<(Vec<Record>, usize)> {
    let root = parse_range_at(data, root_offset, root_size)?;
    Ok((root.records.clone(), root.header_len))
}

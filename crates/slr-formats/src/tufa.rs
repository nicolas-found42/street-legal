//! `TUFA` compiled script classes (`*.class`) of the Invictus engine.
//!
//! The file is a flat chunk list, `tag[4] u32 size payload`, with no gaps (verified on
//! all 1,501 shipped classes). The first chunk is `TUFA` with a 4-byte payload holding the
//! compiler version (`0x108f5` for 1 file, `0x11ae1` for 1,500). Then come
//! `CONS` (constant pool), optional `FILD` (fields), `MTHD` (methods), `CLSS` (class
//! record) and `TREE` (statement/expression trees, version-specific node encoding).
//! A few files concatenate several modules.
//!
//! The constant pool interleaves length-prefixed strings with typed records. This crate
//! extracts the strings (`[u32 len][bytes][NUL]`) with a validating scan; the first two
//! are the class name and its base class.
//!
//! `TREE` (version 0x11ae1) is `u32 count` bodies, each `u32 n` nodes. A node is an opcode byte
//! (index into the exe's table at VA 0x5f0914, see [`OPWORDS`]), a `u16` operand and, only when the
//! opword's class nibble is 1, 2 or 4, a `u32` immediate (exe `FUN_0041a5f0` / `FUN_0041a5c0`).

use crate::reader::Reader;
use crate::{FormatError, Result};

#[derive(Debug, Clone)]
pub struct Chunk {
    pub tag: [u8; 4],
    pub offset: usize,
    pub size: usize,
}

#[derive(Debug, Clone)]
pub struct Module {
    pub version: u32,
    pub chunks: Vec<Chunk>,
    pub strings: Vec<String>,
}

impl Module {
    pub fn class_name(&self) -> Option<&str> {
        self.strings.first().map(String::as_str)
    }

    pub fn base_class(&self) -> Option<&str> {
        self.strings.get(1).map(String::as_str)
    }

    pub fn has_chunk(&self, tag: &[u8; 4]) -> bool {
        self.chunks.iter().any(|c| &c.tag == tag)
    }
}

pub fn parse(data: &[u8]) -> Result<Vec<Module>> {
    let mut r = Reader::new(data);
    let mut modules: Vec<Module> = Vec::new();
    let mut cur: Option<Module> = None;
    while r.remaining() > 0 {
        let offset = r.pos;
        let tag: [u8; 4] = r.bytes(4)?.try_into().unwrap();
        let size = r.u32()? as usize;
        let payload = r.bytes(size)?;
        if &tag == b"TUFA" {
            if size != 4 {
                return Err(FormatError::Inconsistent {
                    what: "tufa header",
                    detail: format!("size {size}"),
                });
            }
            let version = u32::from_le_bytes(payload.try_into().unwrap());
            if version != 0x108f5 && version != 0x11ae1 {
                return Err(FormatError::UnsupportedVersion(version));
            }
            if let Some(m) = cur.take() {
                modules.push(m);
            }
            cur = Some(Module {
                version,
                chunks: Vec::new(),
                strings: Vec::new(),
            });
            continue;
        }
        let m = cur.as_mut().ok_or(FormatError::BadMagic {
            expected: "TUFA",
            found: tag_str(&tag),
        })?;
        if !matches!(&tag, b"CONS" | b"FILD" | b"MTHD" | b"CLSS" | b"TREE") {
            return Err(FormatError::Inconsistent {
                what: "tufa chunk tag",
                detail: tag_str(&tag),
            });
        }
        if &tag == b"CONS" {
            m.strings = scan_strings(payload);
        }
        m.chunks.push(Chunk {
            tag,
            offset: offset + 8,
            size,
        });
    }
    if let Some(m) = cur.take() {
        modules.push(m);
    }
    if modules.is_empty() {
        return Err(FormatError::BadMagic {
            expected: "TUFA",
            found: String::new(),
        });
    }
    for m in &modules {
        if !m.has_chunk(b"CONS") || !m.has_chunk(b"CLSS") || !m.has_chunk(b"TREE") {
            return Err(FormatError::Inconsistent {
                what: "tufa module",
                detail: "missing CONS/CLSS/TREE".into(),
            });
        }
    }
    Ok(modules)
}

fn tag_str(t: &[u8; 4]) -> String {
    String::from_utf8_lossy(t).into_owned()
}

fn scan_strings(p: &[u8]) -> Vec<String> {
    let mut out = Vec::new();
    let mut i = 0;
    while i + 5 <= p.len() {
        let len = u32::from_le_bytes(p[i..i + 4].try_into().unwrap()) as usize;
        let end = i + 4 + len;
        if (1..=2048).contains(&len)
            && end < p.len()
            && p[end] == 0
            && p[i + 4..end].iter().all(|&b| (0x20..0x7f).contains(&b))
        {
            out.push(String::from_utf8_lossy(&p[i + 4..end]).into_owned());
            i = end + 1;
        } else {
            i += 1;
        }
    }
    out
}

/// The exe's opcode table (u16 per opcode byte, VA 0x5f0914, 47 entries). The low byte equals the
/// index; the high nibble is the operand class (1, 2 and 4 carry a `u32` immediate).
pub const OPWORDS: [u16; 47] = [
    0x0000, 0x1001, 0x1002, 0x1003, 0x0004, 0x0005, 0x1006, 0x1007, 0x1008, 0x4009, 0x200a, 0x100b,
    0x000c, 0x100d, 0x100e, 0x100f, 0x0010, 0x1011, 0x4012, 0x0013, 0x1014, 0x1015, 0x1016, 0x1017,
    0x0018, 0x1019, 0x101a, 0x101b, 0x401c, 0x001d, 0x101e, 0x101f, 0x0020, 0x0021, 0x0022, 0x0023,
    0x0024, 0x4025, 0x4026, 0x1027, 0x0028, 0x0029, 0x002a, 0x002b, 0x002c, 0x002d, 0x402e,
];

/// One decoded tree node: the exe's in-memory 8-byte form.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Node {
    pub opword: u16,
    pub operand: u16,
    pub imm: u32,
}

impl Node {
    pub fn has_imm(opword: u16) -> bool {
        matches!(opword & 0xf000, 0x1000 | 0x2000 | 0x4000)
    }
}

impl Module {
    /// Decode every `TREE` body of a version 0x11ae1 module. `data` is the whole file.
    pub fn trees(&self, data: &[u8]) -> Result<Vec<Vec<Node>>> {
        if self.version != 0x11ae1 {
            return Err(FormatError::UnsupportedVersion(self.version));
        }
        let ch =
            self.chunks
                .iter()
                .find(|c| &c.tag == b"TREE")
                .ok_or(FormatError::Inconsistent {
                    what: "tufa tree",
                    detail: "no TREE chunk".into(),
                })?;
        let mut r = Reader::new(&data[ch.offset..ch.offset + ch.size]);
        let bodies = r.u32()?;
        let mut out = Vec::with_capacity(bodies as usize);
        for _ in 0..bodies {
            let n = r.u32()?;
            let mut nodes = Vec::with_capacity(n as usize);
            for _ in 0..n {
                let op = r.bytes(1)?[0] as usize;
                let opword = *OPWORDS.get(op).ok_or(FormatError::Inconsistent {
                    what: "tufa opcode",
                    detail: format!("{op:#x}"),
                })?;
                let operand = u16::from_le_bytes(r.bytes(2)?.try_into().unwrap());
                let imm = if Node::has_imm(opword) { r.u32()? } else { 0 };
                nodes.push(Node {
                    opword,
                    operand,
                    imm,
                });
            }
            out.push(nodes);
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "tufa tree",
                detail: format!("{} trailing bytes", r.remaining()),
            });
        }
        Ok(out)
    }
}

/// One constant-pool entry. Layout from the exe's `ConstantPool` deserializer (`FUN_00407660`)
/// and its per-type readers (`0x4070f0`, `0x4071d0`, `0x407330`, `0x407270`, `0x4073d0`,
/// `0x407540`, `0x407470`): `u32 type` then the payload below.
#[derive(Debug, Clone, PartialEq)]
pub enum PoolEntry {
    /// type 0: `u32 len`, `len` bytes, NUL.
    Str(String),
    /// type 1: `i32`.
    Int(i32),
    /// type 2: `f32`.
    Float(f32),
    /// type 3: two `u32`s (first = string-pool index of the name, second = scalar).
    Member { name: u32, extra: u32 },
    /// type 4: `u32` string-pool index of a class name.
    Class(u32),
    /// type 5: two `u32`s (first at +8, second at +0xc; +0x10 is runtime-resolved).
    Type5 { a: u32, b: u32 },
    /// type 7: two `u32`s.
    Type7 { a: u32, b: u32 },
}

impl Module {
    /// Parse the `CONS` chunk exactly. `data` is the whole file.
    pub fn pool(&self, data: &[u8]) -> Result<Vec<PoolEntry>> {
        let ch =
            self.chunks
                .iter()
                .find(|c| &c.tag == b"CONS")
                .ok_or(FormatError::Inconsistent {
                    what: "tufa pool",
                    detail: "no CONS chunk".into(),
                })?;
        let mut r = Reader::new(&data[ch.offset..ch.offset + ch.size]);
        let n = r.u32()?;
        let mut out = Vec::with_capacity(n as usize);
        for _ in 0..n {
            let ty = r.u32()?;
            out.push(match ty {
                0 => {
                    let len = r.u32()? as usize;
                    let b = r.bytes(len)?;
                    if r.bytes(1)?[0] != 0 {
                        return Err(FormatError::Inconsistent {
                            what: "tufa pool",
                            detail: "string not NUL terminated".into(),
                        });
                    }
                    PoolEntry::Str(String::from_utf8_lossy(b).into_owned())
                }
                1 => PoolEntry::Int(r.u32()? as i32),
                2 => PoolEntry::Float(f32::from_bits(r.u32()?)),
                3 => PoolEntry::Member {
                    name: r.u32()?,
                    extra: r.u32()?,
                },
                4 => PoolEntry::Class(r.u32()?),
                5 => PoolEntry::Type5 {
                    a: r.u32()?,
                    b: r.u32()?,
                },
                7 => PoolEntry::Type7 {
                    a: r.u32()?,
                    b: r.u32()?,
                },
                t => {
                    return Err(FormatError::Inconsistent {
                        what: "tufa pool type",
                        detail: format!("{t}"),
                    });
                }
            });
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "tufa pool",
                detail: format!("{} trailing bytes", r.remaining()),
            });
        }
        Ok(out)
    }
}

/// `MTHD` record: five raw dwords (exe `0x4195b0` stores them at +4..+0x14; meanings not yet
/// decoded, so they stay positional).
pub type MethodRec = [u32; 5];
/// `FILD` record: four raw dwords.
pub type FieldRec = [u32; 4];

/// `CLSS` record: five dwords followed by `tail_count` dwords (loader `FUN_00411b20`, case `CLSS`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClassRec {
    pub head: [u32; 5],
    pub tail: Vec<u32>,
}

impl Module {
    fn chunk_reader<'a>(&self, data: &'a [u8], tag: &[u8; 4]) -> Option<Reader<'a>> {
        let ch = self.chunks.iter().find(|c| &c.tag == tag)?;
        Some(Reader::new(&data[ch.offset..ch.offset + ch.size]))
    }

    fn two_arrays<const N: usize>(
        &self,
        data: &[u8],
        tag: &[u8; 4],
    ) -> Result<Option<[Vec<[u32; N]>; 2]>> {
        let Some(mut r) = self.chunk_reader(data, tag) else {
            return Ok(None);
        };
        let mut out: [Vec<[u32; N]>; 2] = [Vec::new(), Vec::new()];
        for arr in &mut out {
            let n = r.u32()?;
            for _ in 0..n {
                let mut rec = [0u32; N];
                for v in &mut rec {
                    *v = r.u32()?;
                }
                arr.push(rec);
            }
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "tufa array chunk",
                detail: format!("{} trailing bytes", r.remaining()),
            });
        }
        Ok(Some(out))
    }

    /// `MTHD`: two arrays of 5-dword records (order as in the file: exe object +0x16 then +0x14).
    pub fn methods(&self, data: &[u8]) -> Result<Option<[Vec<MethodRec>; 2]>> {
        self.two_arrays::<5>(data, b"MTHD")
    }

    /// `FILD`: two arrays of 4-dword records.
    pub fn fields(&self, data: &[u8]) -> Result<Option<[Vec<FieldRec>; 2]>> {
        self.two_arrays::<4>(data, b"FILD")
    }

    pub fn class_rec(&self, data: &[u8]) -> Result<ClassRec> {
        let mut r = self
            .chunk_reader(data, b"CLSS")
            .ok_or(FormatError::Inconsistent {
                what: "tufa class",
                detail: "no CLSS chunk".into(),
            })?;
        let mut head = [0u32; 5];
        for v in &mut head {
            *v = r.u32()?;
        }
        let mut tail = Vec::new();
        for _ in 0..head[4] {
            tail.push(r.u32()?);
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "tufa class",
                detail: "trailing bytes".into(),
            });
        }
        Ok(ClassRec { head, tail })
    }
}

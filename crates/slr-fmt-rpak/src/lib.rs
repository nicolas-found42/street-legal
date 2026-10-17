//! `RPAK` resource package (`*.rpk`) reader.
//!
//! Layout (all little-endian), verified on all 80 packages in the reference install:
//!
//! ```text
//! 0x00  "RPAK"  u32 version(=0x200)  u32 external_link_count  u32 reserved(=0)
//! 0x10  external_link_count * { u32 package_id_shifted, char name[60] }
//!       u32 headers_byte_size, u32 total_entries, u32 type1_entries, u32 other_entries
//!       total_entries * entry header
//! entry header: u32 super_id, u32 type_id, u8 typeof, u8 flags, f32 is_parent_compatible,
//!               u32 rsd_offset, u32 rsd_size, u8 alias_len, char alias[alias_len],
//!               if typeof == 1 { f32 spatial[12] }
//! ```
//!
//! `rsd_offset`/`rsd_size` are absolute file offsets of the entry's resource descriptor
//! text (`sourcefile ...`, `texture ...`), which reference loose files by relative path.

pub mod payload;

use slr_fmt_core::reader::{latin1_cstr, Reader};
use slr_fmt_core::{FormatError, Result};

#[derive(Debug, Clone)]
pub struct ExternalLink {
    pub package_id_shifted: u32,
    pub name: String,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub super_id: u32,
    pub type_id: u32,
    pub typeof_: u8,
    pub flags: u8,
    pub is_parent_compatible: f32,
    pub rsd_offset: u32,
    pub rsd_size: u32,
    pub alias: String,
    pub spatial: Option<[f32; 12]>,
}

#[derive(Debug, Clone)]
pub struct Package {
    pub links: Vec<ExternalLink>,
    pub entries: Vec<Entry>,
    data: Vec<u8>,
}

impl Package {
    pub fn parse(data: Vec<u8>) -> Result<Self> {
        let mut r = Reader::new(&data);
        let magic = r.bytes(4)?;
        if magic != b"RPAK" {
            return Err(FormatError::BadMagic {
                expected: "RPAK",
                found: String::from_utf8_lossy(magic).into_owned(),
            });
        }
        let version = r.u32()?;
        if version != 0x200 {
            return Err(FormatError::UnsupportedVersion(version));
        }
        let link_count = r.u32()?;
        let _reserved = r.u32()?;
        let mut links = Vec::with_capacity(link_count as usize);
        for _ in 0..link_count {
            let id = r.u32()?;
            let name = latin1_cstr(r.bytes(60)?);
            links.push(ExternalLink {
                package_id_shifted: id,
                name,
            });
        }
        let headers_size = r.u32()? as usize;
        let total = r.u32()?;
        let type1 = r.u32()?;
        let other = r.u32()?;
        if total != type1 + other {
            return Err(FormatError::Inconsistent {
                what: "rpak entry counts",
                detail: format!("{total} != {type1} + {other}"),
            });
        }
        let headers_start = r.pos;
        let mut entries = Vec::with_capacity(total as usize);
        for _ in 0..total {
            let super_id = r.u32()?;
            let type_id = r.u32()?;
            let typeof_ = r.u8()?;
            let flags = r.u8()?;
            let is_parent_compatible = r.f32()?;
            let rsd_offset = r.u32()?;
            let rsd_size = r.u32()?;
            let alias_len = r.u8()? as usize;
            let alias = latin1_cstr(r.bytes(alias_len)?);
            let spatial = if typeof_ == 1 {
                let mut f = [0f32; 12];
                for v in f.iter_mut() {
                    *v = r.f32()?;
                }
                Some(f)
            } else {
                None
            };
            entries.push(Entry {
                super_id,
                type_id,
                typeof_,
                flags,
                is_parent_compatible,
                rsd_offset,
                rsd_size,
                alias,
                spatial,
            });
        }
        if r.pos - headers_start != headers_size {
            return Err(FormatError::Inconsistent {
                what: "rpak header block size",
                detail: format!("{} != {}", r.pos - headers_start, headers_size),
            });
        }
        let t1 = entries.iter().filter(|e| e.typeof_ == 1).count() as u32;
        if t1 != type1 {
            return Err(FormatError::Inconsistent {
                what: "rpak type1 count",
                detail: format!("{t1} != {type1}"),
            });
        }
        for e in &entries {
            let end = e.rsd_offset as usize + e.rsd_size as usize;
            if end > data.len() {
                return Err(FormatError::Inconsistent {
                    what: "rpak descriptor range",
                    detail: format!("entry {:?} ends at {end}, file is {}", e.alias, data.len()),
                });
            }
        }
        Ok(Self {
            links,
            entries,
            data,
        })
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Self> {
        Self::parse(std::fs::read(path)?)
    }

    /// Raw bytes of the whole package (entry headers, descriptors and payloads).
    pub fn data(&self) -> &[u8] {
        &self.data
    }

    /// The payload walk seeds: records of the single `typeof == 1` root entry.
    /// Returns `(root records, descriptor-chunks)`; the records' `(x, y)` ranges tile
    /// the payload gap between the root descriptor and the tail descriptors.
    pub fn map_payload_seed(&self) -> Result<(Vec<payload::Record>, Vec<payload::Chunk>)> {
        let root = self.entries.iter().find(|e| e.typeof_ == 1).ok_or(
            slr_fmt_core::FormatError::Inconsistent {
                what: "map payload root",
                detail: "no typeof==1 entry".to_string(),
            },
        )?;
        let off = root.rsd_offset as usize;
        let (records, header_len) =
            payload::parse_root_records(&self.data, off, root.rsd_size as usize)?;
        let mut chunks = Vec::new();
        let mut pos = off + header_len;
        let end = off + root.rsd_size as usize;
        while pos < end {
            let (chunk, stride) = payload::parse_chunk_at(&self.data, pos, end)?;
            chunks.push(chunk);
            pos += stride;
        }
        Ok((records, chunks))
    }

    /// Resource descriptor text of an entry (empty for pure folder nodes).
    pub fn descriptor(&self, e: &Entry) -> String {
        let s = e.rsd_offset as usize;
        let t = &self.data[s..s + e.rsd_size as usize];
        t.iter().map(|&b| b as char).collect()
    }

    /// Slash-joined alias path of an entry, following `super_id` -> `type_id` links.
    pub fn path_of(&self, e: &Entry) -> String {
        let mut parts = vec![e.alias.clone()];
        let mut cur = e.super_id;
        let mut guard = 0;
        while let Some(p) = self
            .entries
            .iter()
            .find(|x| x.type_id == cur && x.typeof_ == e.typeof_)
        {
            parts.push(p.alias.clone());
            cur = p.super_id;
            guard += 1;
            if guard > 32 {
                break;
            }
        }
        parts.reverse();
        parts.join("/")
    }
}

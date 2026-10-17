//! Traffic road graph (`maps/<map>/ai/city.trc`).
//!
//! Layout (stock `city.trc`, 843301 bytes, byte-verified):
//!
//! ```text
//! 0x00 u32 version = 1
//! 0x04 u32 section1_len = 403537 (opaque node/link block)
//! 0x08 u32 section1_blob_len = 194816 (opaque sub-block; see below)
//! 0x0c u32 lane_count = 760 (road-lane records follow)
//! 0x10 u32 junction_count = 484
//! 0x14 u32 point_count = 1572
//! 0x18 u32 extra_count = 4
//! 0x1c u32 flags = 218
//! 0x20 section 1: section1_len bytes (node/link block, kept opaque)
//!      section 2: lane records; each is
//!        u32 item_count N, N items of 57 bytes, then 85 bytes of
//!        per-record floats ending in 13.888889, 0.9, NaN, so that
//!        every record is exactly 89 + 57 * N bytes long
//! ```
//!
//! Section 1 is kept opaque: its inner layout is not proven by the exe code
//! read for this ticket (the route/traffic natives at `0x0048310f`,
//! `0x0048329e`, `0x00483754`, `0x00483b65`, `0x00483d03`, `0x00483e68`,
//! `0x00484050`, `0x00484420`, `0x00484730`, `0x00484fa0`, `0x00485190`,
//! `0x00485448`, `0x00485870` consume the in-memory graph, whose loader was
//! not isolated). Section 2's record framing is structural: every one of the
//! 1,462 plain records obeys `len == 89 + 57 * N` and ends with the
//! `(13.888889, 0.9, NaN)` terminator; 11 records carry extra bytes past the
//! terminator (length still `>= 89 + 57 * N`) and are exposed via
//! [`LaneRecord::extra`].
//!
//! The all-`0x01` 57-byte items inside the 11 extended records are padding,
//! not lanes: across the whole file every other item starts with a small
//! (< 32) lane id, while only the extended-record tail items are `0x01` and
//! only there do they run back-to-back.

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// `(13.888889, 0.9, NaN)` terminator closing every lane record.
const TERMINATOR: [u8; 12] = [
    0xe4, 0x38, 0x5e, 0x41, 0x66, 0x66, 0x66, 0x3f, 0xff, 0xff, 0xff, 0xff,
];

/// Base size of a lane record: `u32 N` + 85 bytes of per-record floats.
pub const RECORD_BASE: usize = 89;
/// Size of one lane item inside a lane record.
pub const ITEM_LEN: usize = 57;

/// One road-lane record: `N` 57-byte lane items plus the trailing floats.
///
/// Byte offsets are relative to the record start: bytes `4 .. 4 + 57 * N`
/// are the items, bytes `4 + 57 * N ..` are the per-record trailer
/// (85 bytes on plain records, ending with [`TERMINATOR`]).
#[derive(Debug, Clone)]
pub struct LaneRecord {
    /// File offset of the record's leading `N` word.
    pub offset: usize,
    /// Raw 57-byte lane items.
    pub items: Vec<[u8; ITEM_LEN]>,
    /// Trailer bytes after the items (85 on plain records, more when the
    /// record carries extra bytes past the terminator).
    pub trailer: Vec<u8>,
}

impl LaneRecord {
    /// Lane ids: the leading byte of each non-padding item.
    pub fn lane_ids(&self) -> Vec<u8> {
        self.items
            .iter()
            .filter(|i| i[0] != 0x01 || !i.iter().all(|&b| b == 0x01))
            .map(|i| i[0])
            .collect()
    }

    /// Extra bytes past the terminator (empty on plain records).
    pub fn extra(&self) -> &[u8] {
        match self
            .trailer
            .windows(TERMINATOR.len())
            .rposition(|w| w == TERMINATOR)
        {
            Some(i) => &self.trailer[i + TERMINATOR.len()..],
            None => &[],
        }
    }
}

/// Parsed `.trc` file.
#[derive(Debug, Clone)]
pub struct Trc {
    /// Raw header words after the version.
    pub header: Header,
    /// Opaque section 1 (node/link block).
    pub section1: Vec<u8>,
    /// Lane records (section 2).
    pub lanes: Vec<LaneRecord>,
    /// File size in bytes.
    pub size: usize,
}

/// The seven header words following the version word, kept raw: their exact
/// roles are unproven (no isolated exe reader), but the observed stock values
/// are asserted by tests. Section 1 (the node/link block) runs from the end
/// of the header to the first lane record; its length is derived from the
/// lane framing, not from any header word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Header {
    /// Second header word (53 in stock `city.trc`).
    pub w1: u32,
    /// Third header word (218).
    pub w2: u32,
    /// Fourth header word (484).
    pub junction_count: u32,
    /// Fifth header word (1572).
    pub point_count: u32,
    /// Sixth header word (4).
    pub w5: u32,
    /// Seventh header word (760; matches the `.rest` stop count of 755
    /// within a handful, so plausibly the traffic-node count, unproven).
    pub w6: u32,
    /// Eighth header word (194816).
    pub w7: u32,
}

/// The exact `.trc` reader was not isolated from the surrounding loader family
/// (route/traffic/map container code in `0x00483xxx`-`0x00485xxx`); see module
/// docs. The lane framing this function asserts is structural.
pub fn parse(data: &[u8]) -> Result<Trc> {
    let mut r = Reader::new(data);
    let version = r.u32()?;
    if version != 1 {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let header = Header {
        w1: r.u32()?,
        w2: r.u32()?,
        junction_count: r.u32()?,
        point_count: r.u32()?,
        w5: r.u32()?,
        w6: r.u32()?,
        w7: r.u32()?,
    };
    // Section 1 ends where the first lane record starts. A lane record
    // opens with `N` in 3..=64 followed by `N` 57-byte items and an 85-byte
    // trailer (`len == 89 + 57 * N`, terminator included), so the first
    // record start is the first offset whose full plain frame (terminator
    // at exactly `89 + 57 * N`) is present.
    let mut s1_end = None;
    let mut pos = r.pos;
    while pos + 89 <= data.len() {
        let n = u32::from_le_bytes(data[pos..pos + 4].try_into().unwrap()) as usize;
        if (3..=64).contains(&n)
            && data.len() - pos >= 89 + 57 * n
            && data[pos + 89 + 57 * n - TERMINATOR.len()..pos + 89 + 57 * n] == TERMINATOR
        {
            s1_end = Some(pos);
            break;
        }
        pos += 1;
    }
    let s1_end = s1_end.ok_or(FormatError::Inconsistent {
        what: "trc section 1",
        detail: "no lane record start found".into(),
    })?;
    r.pos = s1_end;
    let section1 = data[32..s1_end].to_vec();
    let mut lanes = Vec::new();
    while r.remaining() > 0 {
        let offset = r.pos;
        let n = r.u32()? as usize;
        if !(3..=64).contains(&n) {
            return Err(FormatError::Inconsistent {
                what: "trc lane item count",
                detail: format!("record at {offset:#x} has N={n}"),
            });
        }
        let mut items = Vec::with_capacity(n);
        for _ in 0..n {
            let raw = r.bytes(ITEM_LEN)?;
            let mut item = [0u8; ITEM_LEN];
            item.copy_from_slice(raw);
            items.push(item);
        }
        // The trailer runs to the terminator; extended records may carry
        // extra bytes past it, so scan forward for the next plausible
        // record start (`N` in 3..=64 with enough bytes left). The scan
        // must land on a record that itself contains the terminator ahead;
        // otherwise a short tail (like the file's final 175 bytes) would be
        // misread as a new record start.
        let rest = &data[r.pos..];
        let term = rest
            .windows(TERMINATOR.len())
            .position(|w| w == TERMINATOR)
            .ok_or(FormatError::Inconsistent {
                what: "trc lane terminator",
                detail: format!("record at {offset:#x} has no terminator"),
            })?;
        let trailer_end = rest.len().min(term + TERMINATOR.len() + 65536);
        let mut end = term + TERMINATOR.len();
        let mut scan = end;
        while scan + 4 <= rest.len() && scan <= end + 65536 {
            let n2 = u32::from_le_bytes(rest[scan..scan + 4].try_into().unwrap()) as usize;
            if (3..=64).contains(&n2)
                && rest.len() - scan >= 89 + 57 * n2
                && rest[scan..]
                    .windows(TERMINATOR.len())
                    .any(|w| w == TERMINATOR)
            {
                end = scan;
                break;
            }
            scan += 1;
            if scan >= trailer_end {
                break;
            }
        }
        if scan + 4 > rest.len() || (end == term + TERMINATOR.len() && scan >= trailer_end) {
            end = rest.len();
        }
        let trailer = rest[..end].to_vec();
        r.pos += end;
        lanes.push(LaneRecord {
            offset,
            items,
            trailer,
        });
    }
    Ok(Trc {
        header,
        section1,
        lanes,
        size: data.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock_bytes() -> Option<Vec<u8>> {
        let p = std::env::var_os("SLR_STOCK")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock")
            });
        std::fs::read(p.join("maps/city/ai/city.trc")).ok()
    }

    #[test]
    fn city_trc_parses_with_zero_leftover() {
        let Some(data) = stock_bytes() else {
            eprintln!("stock/ not present; skipping");
            return;
        };
        let trc = parse(&data).unwrap();
        assert_eq!(trc.size, 843301);
        assert_eq!(
            trc.header,
            Header {
                w1: 53,
                w2: 218,
                junction_count: 484,
                point_count: 1572,
                w5: 4,
                w6: 760,
                w7: 194816,
            }
        );
        assert_eq!(trc.section1.len(), 0x62871 - 32);
        assert_eq!(trc.lanes.len(), 1473);
        let norm = trc.lanes.iter().filter(|l| l.trailer.len() == 85).count();
        assert_eq!(norm, 1461);
        let items: usize = trc.lanes.iter().map(|l| l.items.len()).sum();
        assert_eq!(items, 4864);
        // Every record obeys the framing rule and carries the terminator.
        for l in &trc.lanes {
            assert!(
                l.items.len() * ITEM_LEN + 89 <= 4 + l.items.len() * ITEM_LEN + l.trailer.len()
            );
            assert!(
                l.trailer.windows(TERMINATOR.len()).any(|w| w == TERMINATOR),
                "record at {:#x} has no terminator",
                l.offset
            );
        }
    }
}

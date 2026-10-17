//! Pedestrian areas (`maps/<map>/ai/city.walk`, `city.rest`).
//!
//! Both files are line-structured ASCII: six comma-separated floats (an
//! axis-aligned bounding box `x1,y1,z1,x2,y2,z2` with `x1 <= x2`, `z1 <= z2`)
//! then a space, a 1-based line id, a `;`, and a payload.
//!
//! ```text
//! head  := float `,` float `,` float `,` float `,` float `,` float
//! walk  := head ` ` id `;` nibble-blob
//! rest  := head ` ` area-id `;1,` index
//! ```
//!
//! `rest` payloads are `1,<index>` with the index dense 0-based per area id
//! (stock: id 1 x484, id 2 x53, id 3 x218). `walk` payloads are
//! nibble-encoded binary: each `A`-`P` char is one nibble (`A` = 0 ...
//! `P` = 15), pairs little-endian (`AB` = byte `0x01`, `BA` = `0x10`).
//! Decoded bytes are:
//!
//! ```text
//! blob   := u32 total_len (= decoded length - 4) then records
//! record := record-bytes `FF FF FF FF` (terminator; every record but the
//!           tail carries one, so a line is first + zero or more mids + tail)
//! first  := f32[4] f32 f32 X:u32 chain
//! mid    := 21-record | 22-chain | 23-sidewalk
//! tail   := index-run | terminal-22 | nested-22 (no terminator)
//! 21-record    := u32(21) f32[4]                          (20 bytes)
//! 22-chain     := u32(22) f32[4] u32 id body              (body follows)
//!   body       := 21-record | terminal-22 | nested-22 | 23-sidewalk
//! terminal-22  := u32(22) f32[4] u32 id u32[k]            (k in 4..=6)
//! nested-22    := u32(22) f32[4] u32 id terminal-22
//! 23-sidewalk  := u32(23) u32 n, (n+1) x f32[6], f32[4]   (n in 3..=6)
//! index-run    := u32 k u32[k]                            (k in 1..=7)
//! ```
//!
//! The `f32[6]` waypoint is `(x, y, z, dx, dy, dz)` (position + direction);
//! every `f32[4]` is two `(x, z)` points (segment corners/endpoints); the
//! trailing `f32[4]` of a sidewalk repeats its first/last waypoints' `(x,z)`.
//! The first record's `f32[4]`/`f32`/`X` triple is structural only (its floats
//! are near but not equal to the head box; `X` is 3 on two-record lines and 4
//! otherwise). Roles beyond framing (which corner is which, what the indices
//! address) are unproven: no isolated exe reader was found (see below).
//!
//! Stock evidence: `city.walk` (1799 lines, ids dense 1..=1799, every blob
//! even-length with `total_len == decoded-4`, `X` in {3,4}, first cores in
//! {44,68,92,168,192,216}, mid cores 21:(20) 22:(44,144,168,192,216)
//! 23:(120,144,168,192), tails 1..7:(8..32) 22:(40,44,68,72));
//! `city.rest` (755 lines, per-id dense 0-based indices, middle word always 1).
//!
//! exe: no isolated `.walk`/`.rest` reader in the shipped binary. The
//! filename extensions never appear in the exe's strings; the pedestrian
//! natives (`GroundRef.setPedestrianDensityN` `FUN_00484d30`,
//! `addPedestrianType` `FUN_00484d90`, `removePedestrianType`
//! `FUN_00484e20`, `pedestrianDistance` `LAB_00484c60`) only tune density,
//! not geometry. The crossing/route natives (`getNearestCross`
//! `LAB_00483d00`, `haltTrafficCross` `LAB_00484b90`, `haltTrafficPath`
//! `LAB_004835e0`, `getStartDirection` `LAB_00483400`, `findRoute`/
//! `getRouteLength` `LAB_00483750`, `alignToRoad` `LAB_00483e60`) consume the
//! in-memory graph, whose file reader was not isolated from the map
//! container code (`FUN_00484fa0`/`FUN_00485190` family, shared with `.trc`).
//! Field order, separators (` `/`,`/`;`), and the nibble alphabet are ported
//! from the files, not the exe; semantic roles above are structural
//! inferences and are marked as such on each type.

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// `FF FF FF FF` terminator closing every record but the tail.
const TERMINATOR: [u8; 4] = [0xFF; 4];

/// Record types of the mid/tail grammar.
const REC_CROSSING: u32 = 21;
const REC_WALK: u32 = 22;
const REC_SIDEWALK: u32 = 23;

/// Axis-aligned bounding box heading every line (`x1,y1,z1,x2,y2,z2`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BBox {
    pub x1: f32,
    pub y1: f32,
    pub z1: f32,
    pub x2: f32,
    pub y2: f32,
    pub z2: f32,
}

/// One `(x, z)` point pair (two corners or two endpoints), field roles
/// structural (see module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Quad(pub [f32; 4]);

/// One sidewalk waypoint: `(x, y, z)` position plus `(dx, dy, dz)`
/// direction, structurally inferred (see module docs).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Waypoint {
    pub pos: [f32; 3],
    pub dir: [f32; 3],
}

/// A `21` crossing record: one [`Quad`], structurally inferred.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Crossing(pub Quad);

/// A `23` sidewalk run: `n` with `n+1` waypoints plus the endpoint [`Quad`].
#[derive(Debug, Clone, PartialEq)]
pub struct Sidewalk {
    pub n: u32,
    pub waypoints: Vec<Waypoint>,
    pub ends: Quad,
}

/// A `22` span: corner [`Quad`], id, and what follows the header. Past the
/// header a word in 21..=23 opens a chained record, any other word opens
/// the index run (stock: leaves never open with 21..=23, spans always do).
#[derive(Debug, Clone, PartialEq)]
pub struct Span {
    pub corners: Quad,
    pub id: u32,
    pub body: SpanBody,
}

/// The body of a [`Span`]: a chained record or a leaf index run (1-based
/// walk line numbers in stock, 4..=6 entries).
#[derive(Debug, Clone, PartialEq)]
pub enum SpanBody {
    Crossing(Crossing),
    Span(Box<Span>),
    Sidewalk(Sidewalk),
    Leaf(Vec<u32>),
}

/// The body chained after a first record: like [`SpanBody`] but without
/// the header (a bare crossing, span, or sidewalk).
#[derive(Debug, Clone, PartialEq)]
pub enum Chain {
    Crossing(Crossing),
    Span(Box<Span>),
    Sidewalk(Sidewalk),
}

/// The first record of a `.walk` line: origin [`Quad`], one float, the `X`
/// tag (3 on two-record lines, 4 otherwise), and the chained body.
#[derive(Debug, Clone, PartialEq)]
pub struct First {
    pub origin: Quad,
    pub width: f32,
    pub x: u32,
    pub chain: Chain,
}

/// A middle record: any typed record.
#[derive(Debug, Clone, PartialEq)]
pub enum Mid {
    Crossing(Crossing),
    Span(Span),
    Sidewalk(Sidewalk),
}

/// The tail record: a bare index run or a `22` span.
#[derive(Debug, Clone, PartialEq)]
pub enum Tail {
    Indices(Vec<u32>),
    Span(Span),
}

/// One decoded `.walk` line.
#[derive(Debug, Clone, PartialEq)]
pub struct WalkLine {
    /// 1-based line number from the text (dense 1..=N in stock).
    pub id: u32,
    pub bbox: BBox,
    pub first: First,
    pub mids: Vec<Mid>,
    pub tail: Tail,
}
impl WalkLine {
    /// Every leaf index referenced anywhere in this line (1-based walk
    /// line numbers in stock).
    pub fn leaf_indices(&self) -> Vec<u32> {
        fn span_ids(s: &Span, out: &mut Vec<u32>) {
            match &s.body {
                SpanBody::Leaf(v) => out.extend_from_slice(v),
                SpanBody::Span(inner) => span_ids(inner, out),
                SpanBody::Crossing(_) | SpanBody::Sidewalk(_) => {}
            }
        }
        let mut out = Vec::new();
        match &self.first.chain {
            Chain::Span(s) => span_ids(s, &mut out),
            Chain::Crossing(_) | Chain::Sidewalk(_) => {}
        }
        for m in &self.mids {
            if let Mid::Span(s) = m {
                span_ids(s, &mut out);
            }
        }
        if let Tail::Span(s) = &self.tail {
            span_ids(s, &mut out);
        }
        out
    }
}

/// One decoded `.rest` line: bbox, area id (1..=3 in stock), and the
/// dense 0-based index within that area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RestLine {
    pub bbox: BBox,
    pub area: u32,
    pub index: u32,
}

/// Decode one nibble char (`A`-`P`) to its 4-bit value.
fn nibble(c: u8) -> Result<u8> {
    if (b'A'..=b'P').contains(&c) {
        Ok(c - b'A')
    } else {
        Err(FormatError::Inconsistent {
            what: "walk nibble",
            detail: format!("byte {c:#x} outside A..=P"),
        })
    }
}

/// exe: nibble alphabet `A`-`P` ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
pub fn decode_nibbles(blob: &[u8]) -> Result<Vec<u8>> {
    if !blob.len().is_multiple_of(2) {
        return Err(FormatError::Inconsistent {
            what: "walk blob length",
            detail: format!("odd nibble count {}", blob.len()),
        });
    }
    let mut out = Vec::with_capacity(blob.len() / 2);
    let (chunks, _) = blob.as_chunks::<2>();
    for pair in chunks {
        out.push(nibble(pair[0])? << 4 | nibble(pair[1])?);
    }
    Ok(out)
}

fn quad(r: &mut Reader<'_>) -> Result<Quad> {
    Ok(Quad([r.f32()?, r.f32()?, r.f32()?, r.f32()?]))
}

fn waypoint(r: &mut Reader<'_>) -> Result<Waypoint> {
    Ok(Waypoint {
        pos: [r.f32()?, r.f32()?, r.f32()?],
        dir: [r.f32()?, r.f32()?, r.f32()?],
    })
}

/// exe: `23`-sidewalk framing ported from the shipped `city.walk`
/// (`[23][n]`, `(n+1)` waypoints, endpoint quad; no isolated exe reader).
fn sidewalk(r: &mut Reader<'_>, limit: usize) -> Result<Sidewalk> {
    let n = r.u32()?;
    if !(3..=6).contains(&n) {
        return Err(FormatError::Inconsistent {
            what: "walk sidewalk count",
            detail: format!("n={n}"),
        });
    }
    let mut waypoints = Vec::with_capacity(n as usize + 1);
    for _ in 0..=n {
        waypoints.push(waypoint(r)?);
    }
    let ends = quad(r)?;
    if r.pos > limit {
        return Err(FormatError::Inconsistent {
            what: "walk sidewalk length",
            detail: format!("overran record end {limit}"),
        });
    }
    Ok(Sidewalk { n, waypoints, ends })
}

/// Parse a first-record body opening with its own type word at `r.pos`:
/// a bare `21` crossing, a `22` span, or a standalone `23` sidewalk.
/// exe: `21`/`22`/`23` record types ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
fn chain(r: &mut Reader<'_>, limit: usize) -> Result<Chain> {
    let ty = r.u32()?;
    match ty {
        REC_CROSSING => {
            let q = quad(r)?;
            if r.pos != limit {
                return Err(unexplained(r, limit));
            }
            Ok(Chain::Crossing(Crossing(q)))
        }
        REC_WALK => {
            r.pos -= 4;
            Ok(Chain::Span(Box::new(span(r, limit)?)))
        }
        REC_SIDEWALK => {
            r.pos -= 4;
            let sw = sidewalk_at(r, limit)?;
            if r.pos != limit {
                return Err(unexplained(r, limit));
            }
            Ok(Chain::Sidewalk(sw))
        }
        _ => Err(FormatError::Inconsistent {
            what: "walk chain type",
            detail: format!("type={ty}"),
        }),
    }
}

/// Parse a standalone `23` sidewalk including its type word.
/// exe: `23`-sidewalk framing ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
fn sidewalk_at(r: &mut Reader<'_>, limit: usize) -> Result<Sidewalk> {
    let ty = r.u32()?;
    if ty != REC_SIDEWALK {
        return Err(FormatError::Inconsistent {
            what: "walk sidewalk type",
            detail: format!("type={ty}"),
        });
    }
    sidewalk(r, limit)
}

/// Parse one `22` span: `[22][quad][id]` header plus body. Past the header
/// a word in 21..=23 opens a chained record, any other word opens the
/// index run (stock: leaves never open with 21..=23, spans always do).
/// Minimum chained bodies: 21 needs a quad (20 bytes); an inner 22 needs
/// its own header plus at least one more word (28 bytes); 23 needs count
/// + waypoint + quad (48 bytes).
///
/// exe: `21`/`22`/`23` record types ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
fn span(r: &mut Reader<'_>, limit: usize) -> Result<Span> {
    let ty = r.u32()?;
    if ty != REC_WALK {
        return Err(FormatError::Inconsistent {
            what: "walk span type",
            detail: format!("type={ty}"),
        });
    }
    let corners = quad(r)?;
    let id = r.u32()?;
    let body = span_body(r, limit)?;
    Ok(Span { corners, id, body })
}

/// Parse the body past a `[22][quad][id]` header (see [`span`]).
fn span_body(r: &mut Reader<'_>, limit: usize) -> Result<SpanBody> {
    let need = limit - r.pos;
    if need >= 8 {
        let peek = u32::from_le_bytes(r.data[r.pos..r.pos + 4].try_into().unwrap());
        match peek {
            REC_CROSSING if need >= 20 => {
                r.u32()?;
                let q = quad(r)?;
                if r.pos != limit {
                    return Err(unexplained(r, limit));
                }
                return Ok(SpanBody::Crossing(Crossing(q)));
            }
            REC_WALK if need >= 28 => {
                return Ok(SpanBody::Span(Box::new(span(r, limit)?)));
            }
            REC_SIDEWALK if need >= 48 => {
                r.u32()?;
                let sw = sidewalk(r, limit)?;
                if r.pos != limit {
                    return Err(unexplained(r, limit));
                }
                return Ok(SpanBody::Sidewalk(sw));
            }
            _ => {}
        }
    }
    let mut indices = Vec::new();
    while r.pos < limit {
        indices.push(r.u32()?);
    }
    if r.pos != limit {
        return Err(unexplained(r, limit));
    }
    if !(4..=6).contains(&indices.len()) {
        return Err(FormatError::Inconsistent {
            what: "walk leaf indices",
            detail: format!("k={}", indices.len()),
        });
    }
    Ok(SpanBody::Leaf(indices))
}

fn unexplained(r: &Reader<'_>, limit: usize) -> FormatError {
    FormatError::Inconsistent {
        what: "walk record",
        detail: format!("{} unexplained bytes to {limit}", limit - r.pos),
    }
}

/// Parse one `.walk` text line (without the trailing newline).
/// exe: `head SP id ; blob` separators ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
pub fn parse_walk_line(line: &[u8]) -> Result<WalkLine> {
    let space = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(FormatError::Inconsistent {
            what: "walk line",
            detail: "missing head/blob space".into(),
        })?;
    let bbox = parse_bbox(&line[..space])?;
    let rest = &line[space + 1..];
    let semi = rest
        .iter()
        .position(|&b| b == b';')
        .ok_or(FormatError::Inconsistent {
            what: "walk line",
            detail: "missing id/blob semicolon".into(),
        })?;
    let id: u32 = std::str::from_utf8(&rest[..semi])
        .map_err(|_| FormatError::Inconsistent {
            what: "walk line id",
            detail: "non-UTF8 id".into(),
        })?
        .parse()
        .map_err(|_| FormatError::Inconsistent {
            what: "walk line id",
            detail: "id is not a u32".into(),
        })?;
    let bytes = decode_nibbles(&rest[semi + 1..])?;
    let mut r = Reader::new(&bytes);
    let total = r.u32()? as usize;
    if total != bytes.len() - 4 {
        return Err(FormatError::Inconsistent {
            what: "walk total length",
            detail: format!("prefix={total} actual={}", bytes.len() - 4),
        });
    }
    // Split records on the terminator; the tail carries none.
    let mut bounds = Vec::new();
    let mut search = 0;
    while let Some(off) = bytes[search.max(4)..]
        .windows(TERMINATOR.len())
        .position(|w| w == TERMINATOR)
    {
        let at = search.max(4) + off;
        bounds.push(at);
        search = at + TERMINATOR.len();
    }
    if bounds.is_empty() {
        return Err(FormatError::Inconsistent {
            what: "walk records",
            detail: "no terminator".into(),
        });
    }
    let body = &bytes[4..];
    let mut starts = vec![0usize];
    for b in &bounds {
        starts.push(*b - 4 + TERMINATOR.len());
    }
    // segments in body coordinates: [starts[i] .. bounds[i]-4] + tail
    let mut segs = Vec::new();
    for (i, b) in bounds.iter().enumerate() {
        let rel = *b - 4;
        segs.push(&body[starts[i]..rel]);
    }
    let tail_bytes = &body[*starts.last().unwrap()..];
    // First record: [quad][f32][X][chain].
    let first_seg = segs[0];
    let mut fr = Reader::new(first_seg);
    let origin = quad(&mut fr)?;
    let width = fr.f32()?;
    let x = fr.u32()?;
    if x != 3 && x != 4 {
        return Err(FormatError::Inconsistent {
            what: "walk first X",
            detail: format!("X={x}"),
        });
    }
    let first = First {
        origin,
        width,
        x,
        chain: chain(&mut fr, first_seg.len())?,
    };
    if fr.pos != first_seg.len() {
        return Err(unexplained(&fr, first_seg.len()));
    }
    // Middle records: typed records with the same shapes as chains.
    let mut mids = Vec::new();
    for seg in &segs[1..] {
        let mut mr = Reader::new(seg);
        let ty = mr.u32()?;
        match ty {
            REC_CROSSING => {
                let q = quad(&mut mr)?;
                if mr.pos != seg.len() {
                    return Err(unexplained(&mr, seg.len()));
                }
                mids.push(Mid::Crossing(Crossing(q)));
            }
            REC_WALK => {
                mr.pos -= 4;
                mids.push(Mid::Span(span(&mut mr, seg.len())?));
            }
            REC_SIDEWALK => {
                mr.pos -= 4;
                let sw = sidewalk_at(&mut mr, seg.len())?;
                if mr.pos != seg.len() {
                    return Err(unexplained(&mr, seg.len()));
                }
                mids.push(Mid::Sidewalk(sw));
            }
            _ => {
                return Err(FormatError::Inconsistent {
                    what: "walk mid type",
                    detail: format!("type={ty}"),
                });
            }
        }
        if mr.pos != seg.len() {
            return Err(unexplained(&mr, seg.len()));
        }
    }
    // Tail: index run or 22 span.
    let mut tr = Reader::new(tail_bytes);
    let ty = tr.u32()?;
    let tail = if ty <= 7 && tail_bytes.len() == 4 + ty as usize * 4 {
        let mut v = Vec::with_capacity(ty as usize);
        for _ in 0..ty {
            v.push(tr.u32()?);
        }
        Tail::Indices(v)
    } else if ty == REC_WALK {
        tr.pos -= 4;
        Tail::Span(span(&mut tr, tail_bytes.len())?)
    } else {
        return Err(FormatError::Inconsistent {
            what: "walk tail type",
            detail: format!("type={ty}"),
        });
    };
    if tr.pos != tail_bytes.len() {
        return Err(unexplained(&tr, tail_bytes.len()));
    }
    Ok(WalkLine {
        id,
        bbox,
        first,
        mids,
        tail,
    })
}

fn parse_bbox(head: &[u8]) -> Result<BBox> {
    let s = std::str::from_utf8(head).map_err(|_| FormatError::Inconsistent {
        what: "walk bbox",
        detail: "non-UTF8 head".into(),
    })?;
    let mut fs = Vec::with_capacity(6);
    for part in s.split(',') {
        fs.push(part.parse::<f32>().map_err(|_| FormatError::Inconsistent {
            what: "walk bbox",
            detail: format!("bad float {part:?}"),
        })?);
    }
    if fs.len() != 6 {
        return Err(FormatError::Inconsistent {
            what: "walk bbox",
            detail: format!("{} fields, want 6", fs.len()),
        });
    }
    if !fs.iter().all(|f| f.is_finite()) {
        return Err(FormatError::Inconsistent {
            what: "walk bbox",
            detail: "non-finite float".into(),
        });
    }
    if !(fs[0] <= fs[3] && fs[2] <= fs[5]) {
        return Err(FormatError::Inconsistent {
            what: "walk bbox",
            detail: "x1>x2 or z1>z2".into(),
        });
    }
    Ok(BBox {
        x1: fs[0],
        y1: fs[1],
        z1: fs[2],
        x2: fs[3],
        y2: fs[4],
        z2: fs[5],
    })
}

/// Parse one `.rest` text line (without the trailing newline).
/// exe: `head SP area ; 1 , index` separators ported from the shipped
/// `city.rest` (no isolated exe reader; see module docs).
pub fn parse_rest_line(line: &[u8]) -> Result<RestLine> {
    let space = line
        .iter()
        .position(|&b| b == b' ')
        .ok_or(FormatError::Inconsistent {
            what: "rest line",
            detail: "missing head/payload space".into(),
        })?;
    let bbox = parse_bbox(&line[..space])?;
    let rest = &line[space + 1..];
    let semi = rest
        .iter()
        .position(|&b| b == b';')
        .ok_or(FormatError::Inconsistent {
            what: "rest line",
            detail: "missing area/payload semicolon".into(),
        })?;
    let area: u32 = std::str::from_utf8(&rest[..semi])
        .map_err(|_| FormatError::Inconsistent {
            what: "rest area",
            detail: "non-UTF8 area".into(),
        })?
        .parse()
        .map_err(|_| FormatError::Inconsistent {
            what: "rest area",
            detail: "area is not a u32".into(),
        })?;
    let payload =
        std::str::from_utf8(&rest[semi + 1..]).map_err(|_| FormatError::Inconsistent {
            what: "rest payload",
            detail: "non-UTF8 payload".into(),
        })?;
    let (one, index) = payload.split_once(',').ok_or(FormatError::Inconsistent {
        what: "rest payload",
        detail: "missing comma".into(),
    })?;
    if one != "1" {
        return Err(FormatError::Inconsistent {
            what: "rest payload",
            detail: format!("middle word {one:?}, want \"1\""),
        });
    }
    let index: u32 = index.parse().map_err(|_| FormatError::Inconsistent {
        what: "rest index",
        detail: "index is not a u32".into(),
    })?;
    Ok(RestLine { bbox, area, index })
}

/// Parse a whole `.walk` file (UTF-8, newline-separated, no header).
/// exe: line framing ported from the shipped `city.walk`
/// (no isolated exe reader; see module docs).
pub fn parse_walk(data: &[u8]) -> Result<Vec<WalkLine>> {
    let text = std::str::from_utf8(data).map_err(|_| FormatError::Inconsistent {
        what: "walk file",
        detail: "non-UTF8 bytes".into(),
    })?;
    text.lines()
        .map(|l| parse_walk_line(l.as_bytes()))
        .collect()
}

/// Parse a whole `.rest` file (UTF-8, newline-separated, no header).
/// exe: line framing ported from the shipped `city.rest`
/// (no isolated exe reader; see module docs).
pub fn parse_rest(data: &[u8]) -> Result<Vec<RestLine>> {
    let text = std::str::from_utf8(data).map_err(|_| FormatError::Inconsistent {
        what: "rest file",
        detail: "non-UTF8 bytes".into(),
    })?;
    text.lines()
        .map(|l| parse_rest_line(l.as_bytes()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stock(path: &str) -> Option<Vec<u8>> {
        let p = std::env::var_os("SLR_STOCK")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| {
                std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock")
            });
        std::fs::read(p.join(path)).ok()
    }

    #[test]
    fn city_walk_parses_fully() {
        let Some(data) = stock("maps/city/ai/city.walk") else {
            eprintln!("stock/ not present; skipping");
            return;
        };
        let lines = parse_walk(&data).unwrap();
        assert_eq!(lines.len(), 1799);
        // Ids are dense 1-based.
        for (i, l) in lines.iter().enumerate() {
            assert_eq!(l.id, i as u32 + 1);
        }
        // Record census (every stock line obeys the grammar in the module
        // docs with zero unexplained bytes).
        let mut first = [0usize; 3];
        let (mut cross, mut span, mut side) = (0, 0, 0);
        let (mut mid_cross, mut mid_span, mut mid_side) = (0, 0, 0);
        let (mut tail_idx, mut tail_span) = (0, 0);
        let (mut depth0, mut depth1, mut depth2) = (0, 0, 0);
        fn depth(s: &Span) -> usize {
            match &s.body {
                SpanBody::Span(inner) => 1 + depth(inner),
                SpanBody::Crossing(_) | SpanBody::Sidewalk(_) | SpanBody::Leaf(_) => 1,
            }
        }
        for l in &lines {
            match l.first.x {
                3 => first[0] += 1,
                4 => first[1] += 1,
                _ => first[2] += 1,
            }
            match &l.first.chain {
                Chain::Crossing(_) => cross += 1,
                Chain::Span(_) => span += 1,
                Chain::Sidewalk(_) => side += 1,
            }
            for m in &l.mids {
                match m {
                    Mid::Crossing(_) => mid_cross += 1,
                    Mid::Span(_) => mid_span += 1,
                    Mid::Sidewalk(_) => mid_side += 1,
                }
            }
            match &l.tail {
                Tail::Indices(v) => {
                    tail_idx += 1;
                    assert!((1..=7).contains(&v.len()));
                }
                Tail::Span(_) => tail_span += 1,
            }
            // Span nesting depth over the whole line (header levels).
            let mut spans = Vec::new();
            if let Chain::Span(s) = &l.first.chain {
                spans.push(&**s);
            }
            for m in &l.mids {
                if let Mid::Span(s) = m {
                    spans.push(s);
                }
            }
            if let Tail::Span(s) = &l.tail {
                spans.push(s);
            }
            for s in spans {
                match depth(s) {
                    1 => depth0 += 1,
                    2 => depth1 += 1,
                    _ => depth2 += 1,
                }
            }
            // Leaf indices address walk lines 1-based.
            for idx in l.leaf_indices() {
                assert!((1..=1799).contains(&idx), "line {} idx {idx}", l.id);
            }
        }
        assert_eq!(first, [278, 1521, 0]);
        // First chains: 846 bare crossings, 953 spans (64 leaves, 75
        // nests, 814 sidewalks inside), no standalone sidewalks.
        assert_eq!((cross, span, side), (846, 953, 0));
        assert_eq!((mid_cross, mid_span, mid_side), (32, 1489, 64));
        assert_eq!((tail_idx, tail_span), (921, 878));
        // Span depths: every span nests at most once (213 nested total:
        // 75 first-record + 138 tail + 1 triple-nested spans).
        assert_eq!((depth0, depth1, depth2), (3106, 214, 0));
        // X=3 marks the 278 two-record lines; every other line has mids.
        for l in &lines {
            assert_eq!(l.first.x == 3, l.mids.is_empty());
        }
    }

    #[test]
    fn city_rest_parses_fully() {
        let Some(data) = stock("maps/city/ai/city.rest") else {
            eprintln!("stock/ not present; skipping");
            return;
        };
        let lines = parse_rest(&data).unwrap();
        assert_eq!(lines.len(), 755);
        let mut per = [0usize; 4];
        for l in &lines {
            per[l.area as usize] += 1;
        }
        assert_eq!((per[1], per[2], per[3]), (484, 53, 218));
        // Indices are dense 0-based within each area.
        let mut seen = [Vec::new(), Vec::new(), Vec::new(), Vec::new()];
        for l in &lines {
            seen[l.area as usize].push(l.index);
        }
        for (area, want) in seen.iter().enumerate().skip(1) {
            let mut got = want.clone();
            got.sort_unstable();
            assert_eq!(
                got,
                (0..got.len() as u32).collect::<Vec<_>>(),
                "area {area}"
            );
        }
    }
}

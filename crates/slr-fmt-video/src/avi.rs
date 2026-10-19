//! AVI / RIFF demuxer (classic `idx1` files and the OpenDML extension).
//!
//! Layout handled (all little-endian, chunks word-aligned):
//!
//! ```text
//! RIFF "AVI "
//!   LIST hdrl   avih, LIST strl { strh, strf, ... } per stream, LIST odml { dmlh }
//!   JUNK ...    padding, skipped
//!   LIST movi   NNdc / NNdb / NNwb chunks (NN = decimal stream number),
//!               optionally nested in LIST "rec ", OpenDML ixNN chunks skipped
//!   idx1        { ckid, flags, offset, size } per chunk
//! RIFF "AVIX"  (OpenDML continuation segments: LIST movi again)
//! JUNK         trailing data outside the RIFF, skipped
//! ```
//!
//! The authoritative frame list is the walk of every `movi` list in file
//! order; `idx1` is parsed and can be cross-checked with
//! [`Avi::verify_index`]. The OpenDML super index (`indx`) and standard
//! index (`ixNN`) chunks are not interpreted (the shipped files carry
//! none); `dmlh` (true total frame count) is read.
//!
//! The stock `Prime.avi` stores its Cinepak frames under `00db` chunk ids
//! (the "uncompressed DIB" id) although the stream handler and
//! `biCompression` are `cvid`, so both `dc` and `db` chunks of the video
//! stream are treated as frames.

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

/// `avih` main header (fields the decoder and the checks use).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MainHeader {
    pub micros_per_frame: u32,
    pub flags: u32,
    pub total_frames: u32,
    pub streams: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StreamKind {
    Video,
    Audio,
    Other([u8; 4]),
}

/// One `strl` list: `strh` plus the interesting parts of `strf`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stream {
    pub kind: StreamKind,
    pub fcc_type: [u8; 4],
    pub fcc_handler: [u8; 4],
    pub scale: u32,
    pub rate: u32,
    /// `strh.dwLength`: frames for video, samples for audio.
    pub length: u32,
    /// Video: `BITMAPINFOHEADER.biCompression` fourcc; zero otherwise.
    pub compression: [u8; 4],
    /// Video: `biWidth`, `biHeight` (absolute value), `biBitCount`.
    pub width: u32,
    pub height: u32,
    pub bit_count: u16,
    /// Audio: `WAVEFORMATEX` tag, channels, samples per second.
    pub wave_format: u16,
    pub channels: u16,
    pub samples_per_sec: u32,
}

/// A chunk of a `movi` list.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Chunk<'a> {
    pub id: [u8; 4],
    /// File offset of the chunk header.
    pub offset: usize,
    pub data: &'a [u8],
}

impl Chunk<'_> {
    /// Stream number from the `NN` prefix of the chunk id.
    pub fn stream(&self) -> Option<usize> {
        let (a, b) = (self.id[0], self.id[1]);
        (a.is_ascii_digit() && b.is_ascii_digit())
            .then(|| ((a - b'0') as usize) * 10 + (b - b'0') as usize)
    }

    fn is_video_data(&self) -> bool {
        matches!(&self.id[2..], b"dc" | b"db")
    }
}

/// One `idx1` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexEntry {
    pub id: [u8; 4],
    pub flags: u32,
    pub offset: u32,
    pub size: u32,
}

#[derive(Debug)]
pub struct Avi<'a> {
    pub header: MainHeader,
    pub streams: Vec<Stream>,
    /// `dmlh.dwTotalFrames` (OpenDML), when present.
    pub odml_total_frames: Option<u32>,
    /// Every chunk of every `movi` list, in file order.
    pub chunks: Vec<Chunk<'a>>,
    pub index: Vec<IndexEntry>,
    /// File offset of the first `movi` list's type fourcc (the base of
    /// `idx1` offsets in conforming files).
    pub movi_base: Option<usize>,
}

fn fcc(b: &[u8]) -> [u8; 4] {
    [b[0], b[1], b[2], b[3]]
}

fn truncated(at: usize, need: usize, have: usize) -> FormatError {
    FormatError::Truncated { at, need, have }
}

/// `(id, header file offset, payload)` of one RIFF sub-chunk.
type SubChunk<'a> = ([u8; 4], usize, &'a [u8]);

/// The sub-chunks of `data`, a RIFF/LIST payload starting at file offset
/// `base`.
fn sub_chunks(data: &[u8], base: usize) -> Result<Vec<SubChunk<'_>>> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 8 <= data.len() {
        let id = fcc(&data[pos..]);
        let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let start = pos + 8;
        let end = start
            .checked_add(size)
            .filter(|&e| e <= data.len())
            .ok_or_else(|| truncated(base + pos, size + 8, data.len() - pos))?;
        out.push((id, base + pos, &data[start..end]));
        pos = end + (size & 1);
    }
    Ok(out)
}

fn parse_avih(d: &[u8]) -> Result<MainHeader> {
    let mut r = Reader::new(d);
    let micros_per_frame = r.u32()?;
    let _max_bytes = r.u32()?;
    let _padding = r.u32()?;
    let flags = r.u32()?;
    let total_frames = r.u32()?;
    let _initial = r.u32()?;
    let streams = r.u32()?;
    let _suggested = r.u32()?;
    let width = r.u32()?;
    let height = r.u32()?;
    Ok(MainHeader {
        micros_per_frame,
        flags,
        total_frames,
        streams,
        width,
        height,
    })
}

fn parse_strl(data: &[u8], base: usize) -> Result<Stream> {
    let mut strh = None;
    let mut strf = None;
    for (id, _, d) in sub_chunks(data, base)? {
        match &id {
            b"strh" => strh = Some(d),
            b"strf" => strf = Some(d),
            _ => {}
        }
    }
    let h = strh.ok_or_else(|| FormatError::Inconsistent {
        what: "strl",
        detail: "missing strh".into(),
    })?;
    let mut r = Reader::new(h);
    let fcc_type = fcc(r.bytes(4)?);
    let fcc_handler = fcc(r.bytes(4)?);
    let _flags = r.u32()?;
    let _prio_lang = r.u32()?;
    let _initial = r.u32()?;
    let scale = r.u32()?;
    let rate = r.u32()?;
    let _start = r.u32()?;
    let length = r.u32()?;
    let kind = match &fcc_type {
        b"vids" => StreamKind::Video,
        b"auds" => StreamKind::Audio,
        _ => StreamKind::Other(fcc_type),
    };
    let mut s = Stream {
        kind,
        fcc_type,
        fcc_handler,
        scale,
        rate,
        length,
        compression: [0; 4],
        width: 0,
        height: 0,
        bit_count: 0,
        wave_format: 0,
        channels: 0,
        samples_per_sec: 0,
    };
    if let Some(f) = strf {
        let mut r = Reader::new(f);
        match kind {
            StreamKind::Video => {
                let _size = r.u32()?;
                s.width = (r.u32()? as i32).unsigned_abs();
                s.height = (r.u32()? as i32).unsigned_abs();
                let _planes = r.u16()?;
                s.bit_count = r.u16()?;
                s.compression = fcc(r.bytes(4)?);
            }
            StreamKind::Audio => {
                s.wave_format = r.u16()?;
                s.channels = r.u16()?;
                s.samples_per_sec = r.u32()?;
            }
            StreamKind::Other(_) => {}
        }
    }
    Ok(s)
}

/// Collect the chunks of a `movi` list (payload after the type fourcc).
fn walk_movi<'a>(data: &'a [u8], base: usize, out: &mut Vec<Chunk<'a>>) -> Result<()> {
    for (id, off, d) in sub_chunks(data, base)? {
        match &id {
            b"LIST" => {
                if d.len() >= 4 && &d[..4] == b"rec " {
                    walk_movi(&d[4..], off + 12, out)?;
                }
            }
            b"JUNK" => {}
            _ if id.starts_with(b"ix") => {}
            _ => out.push(Chunk {
                id,
                offset: off,
                data: d,
            }),
        }
    }
    Ok(())
}

impl<'a> Avi<'a> {
    pub fn parse(data: &'a [u8]) -> Result<Self> {
        if data.len() < 12 {
            return Err(truncated(0, 12, data.len()));
        }
        if &data[..4] != b"RIFF" {
            return Err(FormatError::BadMagic {
                expected: "RIFF",
                found: String::from_utf8_lossy(&data[..4]).into_owned(),
            });
        }
        let mut header = None;
        let mut streams = Vec::new();
        let mut odml_total_frames = None;
        let mut chunks = Vec::new();
        let mut index = Vec::new();
        let mut movi_base = None;
        let mut pos = 0usize;
        let mut first = true;
        while pos + 8 <= data.len() {
            let id = fcc(&data[pos..]);
            let size = u32::from_le_bytes(data[pos + 4..pos + 8].try_into().unwrap()) as usize;
            let start = pos + 8;
            let end = start
                .checked_add(size)
                .filter(|&e| e <= data.len())
                .ok_or_else(|| truncated(pos, size + 8, data.len() - pos))?;
            if &id == b"RIFF" {
                let form = &data[start..start + 4.min(size)];
                let want: &[u8] = if first { b"AVI " } else { b"AVIX" };
                if form != want {
                    return Err(FormatError::BadMagic {
                        expected: if first { "AVI " } else { "AVIX" },
                        found: String::from_utf8_lossy(form).into_owned(),
                    });
                }
                for (cid, off, d) in sub_chunks(&data[start + 4..end], start + 4)? {
                    match &cid {
                        b"LIST" if d.len() >= 4 => {
                            let (lt, body, body_off) = (&d[..4], &d[4..], off + 12);
                            match lt {
                                b"hdrl" => {
                                    for (hid, hoff, hd) in sub_chunks(body, body_off)? {
                                        match &hid {
                                            b"avih" => header = Some(parse_avih(hd)?),
                                            b"LIST" if hd.len() >= 4 => {
                                                let (t, b) = (&hd[..4], &hd[4..]);
                                                if t == b"strl" {
                                                    streams.push(parse_strl(b, hoff + 12)?);
                                                } else if t == b"odml" {
                                                    for (oid, _, od) in sub_chunks(b, hoff + 12)? {
                                                        if &oid == b"dmlh" && od.len() >= 4 {
                                                            odml_total_frames =
                                                                Some(u32::from_le_bytes(
                                                                    od[..4].try_into().unwrap(),
                                                                ));
                                                        }
                                                    }
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                b"movi" => {
                                    movi_base.get_or_insert(off + 8);
                                    walk_movi(body, body_off, &mut chunks)?;
                                }
                                _ => {}
                            }
                        }
                        b"idx1" => {
                            let mut r = Reader::new(d);
                            while r.remaining() >= 16 {
                                index.push(IndexEntry {
                                    id: fcc(r.bytes(4)?),
                                    flags: r.u32()?,
                                    offset: r.u32()?,
                                    size: r.u32()?,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            first = false;
            pos = end + (size & 1);
        }
        let header = header.ok_or_else(|| FormatError::Inconsistent {
            what: "AVI",
            detail: "missing hdrl/avih".into(),
        })?;
        Ok(Self {
            header,
            streams,
            odml_total_frames,
            chunks,
            index,
            movi_base,
        })
    }

    /// Stream number and description of the first video stream.
    pub fn video_stream_number(&self) -> Option<usize> {
        self.streams
            .iter()
            .position(|s| s.kind == StreamKind::Video)
    }

    pub fn video_stream(&self) -> Option<&Stream> {
        self.video_stream_number().map(|i| &self.streams[i])
    }

    /// Payloads of the video stream's frame chunks (`dc` and `db`), in file
    /// order. Zero-length chunks are kept: they are dropped frames.
    pub fn video_frames(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        let n = self.video_stream_number();
        self.chunks
            .iter()
            .filter(move |c| c.is_video_data() && c.stream() == n)
            .map(|c| c.data)
    }

    /// Audio data chunks (`wb`), any stream, in file order.
    pub fn audio_chunks(&self) -> impl Iterator<Item = &'a [u8]> + '_ {
        self.chunks
            .iter()
            .filter(|c| &c.id[2..] == b"wb")
            .map(|c| c.data)
    }

    /// Number of video-frame records in `idx1`.
    pub fn index_video_count(&self) -> usize {
        let n = self.video_stream_number();
        self.index
            .iter()
            .filter(|e| {
                let c = Chunk {
                    id: e.id,
                    offset: 0,
                    data: &[],
                };
                c.is_video_data() && c.stream() == n
            })
            .count()
    }

    /// Cross-check `idx1` against the walked `movi` lists: every entry must
    /// name a chunk with the same id and size at its offset. Offsets are
    /// relative to the `movi` fourcc in conforming files and absolute in
    /// some writers; the base is chosen by the first entry.
    pub fn verify_index(&self) -> Result<()> {
        use std::collections::HashMap;
        let by_off: HashMap<usize, &Chunk> = self.chunks.iter().map(|c| (c.offset, c)).collect();
        let entries: Vec<&IndexEntry> = self.index.iter().filter(|e| &e.id != b"rec ").collect();
        let Some(first) = entries.first() else {
            return Ok(());
        };
        let ok = |base: usize, e: &IndexEntry| {
            by_off
                .get(&(base + e.offset as usize))
                .is_some_and(|c| c.id == e.id && c.data.len() == e.size as usize)
        };
        let base = [self.movi_base.unwrap_or(0), 0]
            .into_iter()
            .find(|&b| ok(b, first))
            .ok_or_else(|| FormatError::Inconsistent {
                what: "idx1",
                detail: format!(
                    "first entry offset {:#x} matches no movi chunk",
                    first.offset
                ),
            })?;
        for (i, e) in entries.iter().enumerate() {
            if !ok(base, e) {
                return Err(FormatError::Inconsistent {
                    what: "idx1",
                    detail: format!(
                        "entry {i} ({}, offset {:#x}, size {}) matches no movi chunk",
                        String::from_utf8_lossy(&e.id),
                        e.offset,
                        e.size
                    ),
                });
            }
        }
        Ok(())
    }
}

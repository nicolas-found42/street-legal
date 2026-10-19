//! Cinepak (`cvid`) video decoder.
//!
//! The exe plays its FMV through WINMM (`mciSendCommandA`), i.e. Windows'
//! own Cinepak codec, so there is no decoder in the exe to port or to
//! differential-test against. This is an own implementation of the public
//! Cinepak bitstream (all multi-byte fields big-endian):
//!
//! ```text
//! frame  : u8 flags, u24 size, u16 width, u16 height, u16 strips, strips...
//! strip  : u8 id (0x10 key, 0x11 inter), u24 size (incl. these 12
//!          bytes), u16 y1, x1, y2, x2, chunks...
//! chunk  : u8 id, u24 size (incl. these 4 bytes), payload
//! ```
//!
//! A strip with `y1 == 0` sits directly below the previous strip and `y2`
//! is its height; otherwise `y1..y2` are absolute rows. `x1..x2` are columns.
//!
//! Chunk ids: `0x20`/`0x21` V4 codebook (full / selective update),
//! `0x22`/`0x23` V1 codebook, `0x24..=0x27` the same four as 8-bit
//! grayscale books (4-byte entries); `0x30` intra vectors, `0x31` inter
//! vectors (with skip), `0x32` V1-only vectors. Codebook entries are six
//! bytes `y0 y1 y2 y3 u v` (u, v signed) describing a 2x2 pixel block;
//! V1 blocks are those entries doubled to 4x4, V4 blocks take four entries,
//! one per 2x2 quadrant.
//!
//! Colour transform: `r = y + 2v`, `g = y - trunc(u / 2) - v`,
//! `b = y + 2u`, clipped to a byte. The output is RGB24, top-down.
//!
//! Codebooks persist per strip index across frames; unless frame flag bit
//! 0 is set, strip `n > 0` starts from strip `n - 1`'s books.

use slr_fmt_core::{FormatError, Result};

/// One 2x2 block of RGB pixels, row-major (TL, TR, BL, BR).
type Block = [[u8; 3]; 4];

#[derive(Clone)]
struct Books {
    v1: Vec<Block>,
    v4: Vec<Block>,
}

impl Books {
    fn new() -> Self {
        Self {
            v1: vec![[[0; 3]; 4]; 256],
            v4: vec![[[0; 3]; 4]; 256],
        }
    }
}

pub struct Decoder {
    width: usize,
    height: usize,
    frame: Vec<u8>,
    books: Vec<Books>,
}

fn bad(what: &'static str, detail: impl Into<String>) -> FormatError {
    FormatError::Inconsistent {
        what,
        detail: detail.into(),
    }
}

fn be16(d: &[u8], at: usize) -> usize {
    u16::from_be_bytes([d[at], d[at + 1]]) as usize
}

fn be24(d: &[u8], at: usize) -> usize {
    u32::from_be_bytes([0, d[at], d[at + 1], d[at + 2]]) as usize
}

/// Strip rectangle `(x1, x2, y1, y2)`, half-open.
type Rect = (usize, usize, usize, usize);

fn clip(v: i32) -> u8 {
    v.clamp(0, 255) as u8
}

/// Sequential reader of the per-block flag bits (32-bit big-endian words,
/// most significant bit first).
struct Flags {
    word: u32,
    mask: u32,
}

impl Flags {
    fn new() -> Self {
        Self { word: 0, mask: 0 }
    }

    fn next(&mut self, d: &[u8], pos: &mut usize) -> Result<bool> {
        self.mask >>= 1;
        if self.mask == 0 {
            let w = d
                .get(*pos..*pos + 4)
                .ok_or_else(|| FormatError::Truncated {
                    at: *pos,
                    need: 4,
                    have: d.len().saturating_sub(*pos),
                })?;
            self.word = u32::from_be_bytes(w.try_into().unwrap());
            self.mask = 0x8000_0000;
            *pos += 4;
        }
        Ok(self.word & self.mask != 0)
    }
}

fn take<'a>(d: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8]> {
    let s = d
        .get(*pos..*pos + n)
        .ok_or_else(|| FormatError::Truncated {
            at: *pos,
            need: n,
            have: d.len().saturating_sub(*pos),
        })?;
    *pos += n;
    Ok(s)
}

/// Load a (full or selective) codebook chunk; `id & 1` = selective update,
/// `id & 4` = grayscale entries.
fn load_codebook(book: &mut [Block], id: u8, d: &[u8]) -> Result<()> {
    let n = if id & 4 != 0 { 4 } else { 6 };
    let mut pos = 0;
    let mut flags = Flags::new();
    for entry in book.iter_mut() {
        if id & 1 != 0 {
            // Selective update: flag bit says whether this entry follows.
            // A chunk that ends exactly at a flag boundary ends the update.
            if pos + 4 > d.len() && flags.mask <= 1 {
                break;
            }
            if !flags.next(d, &mut pos)? {
                continue;
            }
        } else if pos + n > d.len() {
            break; // a full book may carry fewer than 256 entries
        }
        let e = take(d, &mut pos, n)?;
        let ys = [e[0], e[1], e[2], e[3]];
        let (u, v) = if n == 6 {
            (e[4] as i8 as i32, e[5] as i8 as i32)
        } else {
            (0, 0)
        };
        for (px, &y) in entry.iter_mut().zip(&ys) {
            let y = y as i32;
            *px = if n == 6 {
                [clip(y + v * 2), clip(y - u / 2 - v), clip(y + u * 2)]
            } else {
                [y as u8; 3]
            };
        }
    }
    Ok(())
}

impl Decoder {
    pub fn new(width: usize, height: usize) -> Result<Self> {
        if width == 0 || height == 0 || !width.is_multiple_of(4) || !height.is_multiple_of(4) {
            return Err(bad(
                "cinepak size",
                format!("{width}x{height}: need non-zero multiples of 4"),
            ));
        }
        Ok(Self {
            width,
            height,
            frame: vec![0; width * height * 3],
            books: Vec::new(),
        })
    }

    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// The current frame, RGB24, `width * height * 3` bytes, top row first.
    pub fn frame(&self) -> &[u8] {
        &self.frame
    }

    fn put(&mut self, x: usize, y: usize, b: &Block) {
        for (i, px) in b.iter().enumerate() {
            let o = ((y + i / 2) * self.width + x + i % 2) * 3;
            self.frame[o..o + 3].copy_from_slice(px);
        }
    }

    fn put_v1(&mut self, x: usize, y: usize, b: &Block) {
        for (i, px) in b.iter().enumerate() {
            for (dy, dx) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
                let o = ((y + (i / 2) * 2 + dy) * self.width + x + (i % 2) * 2 + dx) * 3;
                self.frame[o..o + 3].copy_from_slice(px);
            }
        }
    }

    /// Decode one frame chunk into the persistent frame buffer. An empty
    /// chunk is a dropped frame: the previous picture stays.
    pub fn decode(&mut self, data: &[u8]) -> Result<()> {
        if data.is_empty() {
            return Ok(());
        }
        if data.len() < 10 {
            return Err(FormatError::Truncated {
                at: 0,
                need: 10,
                have: data.len(),
            });
        }
        let flags = data[0];
        let size = u32::from_be_bytes([0, data[1], data[2], data[3]]) as usize;
        let (w, h, strips) = (be16(data, 4), be16(data, 6), be16(data, 8));
        if (w, h) != (self.width, self.height) {
            return Err(bad(
                "cinepak frame",
                format!("header {w}x{h}, decoder {}x{}", self.width, self.height),
            ));
        }
        if size > data.len() {
            return Err(FormatError::Truncated {
                at: 0,
                need: size,
                have: data.len(),
            });
        }
        let data = &data[..size.max(10)];
        if self.books.len() < strips {
            self.books.resize(strips, Books::new());
        }
        let mut pos = 10;
        let mut y0 = 0usize;
        for si in 0..strips {
            let s = data.get(pos..pos + 12).ok_or(FormatError::Truncated {
                at: pos,
                need: 12,
                have: data.len().saturating_sub(pos),
            })?;
            let (id, ssize) = (s[0], be24(s, 1));
            if id != 0x10 && id != 0x11 {
                return Err(bad("cinepak strip", format!("id {id:#x}")));
            }
            if ssize < 12 || pos + ssize > data.len() {
                return Err(FormatError::Truncated {
                    at: pos,
                    need: ssize,
                    have: data.len() - pos,
                });
            }
            // A zero y1 means "directly below the previous strip" and y2
            // is then the strip height; otherwise both are absolute.
            let (mut y1, mut y2, x1, x2) = (be16(s, 4), be16(s, 8), be16(s, 6), be16(s, 10));
            if y1 == 0 {
                y1 = y0;
                y2 += y0;
            }
            if x2 > self.width
                || y2 > self.height
                || x1 >= x2
                || y1 >= y2
                || !(x2 - x1).is_multiple_of(4)
                || !(y2 - y1).is_multiple_of(4)
            {
                return Err(bad(
                    "cinepak strip",
                    format!(
                        "rect x {x1}..{x2}, y {y1}..{y2} in {}x{}",
                        self.width, self.height
                    ),
                ));
            }
            if si > 0 && flags & 1 == 0 {
                self.books[si] = self.books[si - 1].clone();
            }
            self.decode_strip(si, (x1, x2, y1, y2), &data[pos + 12..pos + ssize])?;
            y0 = y2;
            pos += ssize;
        }
        Ok(())
    }

    fn decode_strip(&mut self, si: usize, rect: Rect, d: &[u8]) -> Result<()> {
        let mut pos = 0;
        while pos + 4 <= d.len() {
            let (id, csize) = (d[pos], be24(d, pos + 1));
            if csize < 4 || pos + csize > d.len() {
                return Err(bad(
                    "cinepak chunk",
                    format!("id {id:#x} size {csize} at {pos} of {}", d.len()),
                ));
            }
            let body = &d[pos + 4..pos + csize];
            match id {
                0x20 | 0x21 | 0x24 | 0x25 => load_codebook(&mut self.books[si].v4, id, body)?,
                0x22 | 0x23 | 0x26 | 0x27 => load_codebook(&mut self.books[si].v1, id, body)?,
                0x30..=0x32 => self.decode_vectors(si, rect, id, body)?,
                _ => {} // unknown chunk ids are skipped by size
            }
            pos += csize;
        }
        Ok(())
    }

    fn decode_vectors(&mut self, si: usize, rect: Rect, id: u8, d: &[u8]) -> Result<()> {
        let (x1, x2, y1, y2) = rect;
        let mut pos = 0;
        let mut flags = Flags::new();
        for y in (y1..y2).step_by(4) {
            for x in (x1..x2).step_by(4) {
                if id & 1 != 0 && !flags.next(d, &mut pos)? {
                    continue; // 0x31: skipped block keeps the old pixels
                }
                if id & 2 != 0 || !flags.next(d, &mut pos)? {
                    let i = take(d, &mut pos, 1)?[0] as usize;
                    let b = self.books[si].v1[i];
                    self.put_v1(x, y, &b);
                } else {
                    let idx = take(d, &mut pos, 4)?;
                    for (q, &i) in idx.iter().enumerate() {
                        let b = self.books[si].v4[i as usize];
                        self.put(x + (q % 2) * 2, y + (q / 2) * 2, &b);
                    }
                }
            }
        }
        Ok(())
    }
}

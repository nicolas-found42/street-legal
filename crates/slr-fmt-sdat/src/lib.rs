//! `SDAT` save / configuration containers (`save/game/options`, `save/controls/*`,
//! per-vehicle save files such as `cars/racers/ROC_cars/ROC_*`).
//!
//! The header is `"SDAT" u32 version(=0x00020100) u32 type`, where `type` is either a
//! four-character tag (`CTRL`) or a magic number (`0xFEDCBA98` options,
//! `0x88664422` vehicle). The body is a type-specific sequence of little-endian records;
//! only the header is decoded here and the body is kept raw.

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

pub const VERSION: u32 = 0x0002_0100;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Options,
    Controls,
    Vehicle,
    Other(u32),
}

#[derive(Debug, Clone)]
pub struct Sdat {
    pub kind: Kind,
    pub type_word: u32,
    pub body: Vec<u8>,
}

pub fn parse(data: &[u8]) -> Result<Sdat> {
    let mut r = Reader::new(data);
    let magic = r.bytes(4)?;
    if magic != b"SDAT" {
        return Err(FormatError::BadMagic {
            expected: "SDAT",
            found: String::from_utf8_lossy(magic).into_owned(),
        });
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let type_word = r.u32()?;
    let kind = match type_word {
        0xFEDC_BA98 => Kind::Options,
        0x4C52_5443 => Kind::Controls, // "CTRL"
        0x8866_4422 => Kind::Vehicle,
        other => Kind::Other(other),
    };
    Ok(Sdat {
        kind,
        type_word,
        body: data[r.pos..].to_vec(),
    })
}

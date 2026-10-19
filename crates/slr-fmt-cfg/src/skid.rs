//! Skidmark scripts (`particles/scripts/skids/{sliding,frontend}/*.cfg`), parsed by
//! `FUN_00521e10` into a 0xb0-byte object.
//!
//! ```text
//! maxcount   100
//! texture    0
//! width      0.300  0.400
//! height     0.300
//! uv_normal  0.625 0.000  0.875 1.000
//! uv_end     0.125 0.000  0.375 1.000
//! fade       0.100
//! ```
//!
//! Unlike the emitter scripts there are no sections: every line is `keyword values`, fields
//! split on ` ,\t\r\n`, keywords case-insensitive, unknown lines skipped. `width` is stored
//! halved (`0.5 *`). Slots a script never sets are heap garbage in the exe; the port keeps
//! them at 0. `texture` needs a `0x` prefix to convert (`texture 0` -> id 0; see
//! [`crate::scan`]); each `texture` line calls the texture-slot setter once with the id.

use crate::particle::{Diagnostics, Ignored};
use crate::scan::{scan_f32, scan_hex, scan_i32, Tokens};
use slr_fmt_core::Result;
use slr_x87::Precision;

/// A parsed skidmark script.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SkidCfg {
    /// `maxcount` (100 when the keyword has no convertible number; 0 when absent).
    pub maxcount: i32,
    /// `width a b`, each halved.
    pub width: [f32; 2],
    pub height: f32,
    pub uv_normal: [f32; 4],
    pub uv_end: [f32; 4],
    pub fade: f32,
    /// Ids passed to the texture-slot setter (`FUN_00545fc0`), one per `texture` line.
    pub textures: Vec<u32>,
}

fn floats<const N: usize>(toks: &mut Tokens, pc: Precision, out: &mut [f32; N]) {
    for slot in out.iter_mut() {
        if let Some(v) = toks.next_token().and_then(|t| scan_f32(t, pc)) {
            *slot = v;
        }
    }
}

impl SkidCfg {
    /// Parse at the game's x87 precision (24-bit, ADR 0007).
    pub fn parse(bytes: &[u8]) -> SkidCfg {
        Self::parse_with(bytes, Precision::F32)
    }

    /// Parse with an explicit x87 precision control (see `ParticleCfg::parse_with`).
    ///
    /// exe: FUN_00521e10
    pub fn parse_with(bytes: &[u8], pc: Precision) -> SkidCfg {
        Self::parse_diagnosed(bytes, pc).0
    }

    /// [`SkidCfg::parse_with`] plus a report of every line that was skipped.
    pub fn parse_diagnosed(bytes: &[u8], pc: Precision) -> (SkidCfg, Diagnostics) {
        let mut c = SkidCfg::default();
        let mut diag = Diagnostics::default();
        for (i, line) in bytes.split(|&b| b == b'\n' || b == 0).enumerate() {
            let mut toks = Tokens::new(line);
            let Some(kw) = toks.next_token() else {
                continue;
            };
            let is = |name: &str| kw.eq_ignore_ascii_case(name.as_bytes());
            if is("maxcount") {
                c.maxcount = 100;
                if let Some(v) = toks.next_token().and_then(scan_i32) {
                    c.maxcount = v;
                }
            } else if is("texture") {
                let id = toks.next_token().and_then(scan_hex).unwrap_or(0);
                c.textures.push(id);
            } else if is("width") {
                floats(&mut toks, pc, &mut c.width);
                // fld; fmul [0.5]; fstp: exact for f32
                c.width = [c.width[0] * 0.5, c.width[1] * 0.5];
            } else if is("height") {
                if let Some(v) = toks.next_token().and_then(|t| scan_f32(t, pc)) {
                    c.height = v;
                }
            } else if is("uv_normal") {
                floats(&mut toks, pc, &mut c.uv_normal);
            } else if is("uv_end") {
                floats(&mut toks, pc, &mut c.uv_end);
            } else if is("fade") {
                c.fade = 0.0;
                if let Some(v) = toks.next_token().and_then(|t| scan_f32(t, pc)) {
                    c.fade = v;
                }
            } else if kw[0] == b';' {
                diag.comments += 1;
            } else {
                diag.ignored.push(Ignored {
                    line: i + 1,
                    why: "unknown keyword",
                });
            }
        }
        (c, diag)
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<SkidCfg> {
        Ok(Self::parse(&std::fs::read(path)?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stock_shaped_script() {
        let c = SkidCfg::parse(
            b"maxcount\t100\r\ntexture\t0\r\nwidth\t0.5\t1.0\r\nheight\t0.25\r\n\
              uv_end\t0.125 0.0 0.375 1.0\r\nfade 0.5\r\n",
        );
        assert_eq!(c.maxcount, 100);
        assert_eq!(c.textures, [0]);
        assert_eq!(c.width, [0.25, 0.5]);
        assert_eq!(c.height, 0.25);
        assert_eq!(c.uv_end, [0.125, 0.0, 0.375, 1.0]);
        assert_eq!(c.fade, 0.5);
    }

    #[test]
    fn texture_id_needs_a_hex_prefix() {
        let c = SkidCfg::parse(b"texture 0x1f\ntexture 1f\n");
        assert_eq!(c.textures, [0x1f, 0]);
    }

    #[test]
    fn keyword_alone_defaults() {
        let c = SkidCfg::parse(b"MAXCOUNT\nFade\n");
        assert_eq!(c.maxcount, 100);
        assert_eq!(c.fade, 0.0);
    }
}

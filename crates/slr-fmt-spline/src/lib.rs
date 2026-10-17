//! AI spline polylines (`*.spl`): race lines and AI-follow paths.
//!
//! Each line is one spline control point:
//!
//! ```text
//! x y z dx dy dz\twidth\tspeed[\tflag_a[\text_a\text_b\tflag_b]]
//! ```
//!
//! - `x y z` are the point position (file units are metres; the exe uses them
//!   unscaled).
//! - `dx dy dz` are the segment direction, a unit vector up to `%lf`
//!   round-trip error (stock norms are within `0.988..1.022`).
//! - `width` is the road half-width at the point; `speed` the target speed.
//! - `flag_a` (default 1) opens a pit/branch segment where set contiguously
//!   (stock `race.spl` lines 21..33); `flag_b` (default 1) marks the twisty
//!   braking-profile sections of the ROC lines.
//! - `ext_a` / `ext_b` (default 10000.0) are per-car tuned distances: they are
//!   small (105..3000) only inside the twisty sections and 10000.0 (open)
//!   everywhere else; the seven stock ROC files differ only in these two
//!   columns and `flag_a`.
//!
//! Lines come in three arities: 8 fields (`test.spl`), 9 (`race.spl`, no
//! `ext_*`), 12 (ROC car lines). One stock ROC line carries an empty `width`
//! field (`\t\t`, parsed as the exe default); the file's first point repeats
//! as the last (`race.spl` re-emits it with the exit speed).
//!
//! The reader is `FUN_004337e0` (Ghidra `re/out/all_pseudo.c`): it loads the
//! file named by the `AI_spline` script line (`"%*s %f %s %d"`,
//! `.rdata 0x0060d974`; the `%f` is the default speed/width seed), splits it
//! into lines (`FUN_005511c0`) and scans each with `FUN_00551240` using
//!
//! ```text
//! "%lf %lf %lf %lf %lf %lf %lf %lf %d %lf %lf %d"  (.rdata 0x0060db98)
//! ```
//!
//! keeping the line only when more than 5 conversions succeed
//! (`if (5 < iVar3)`), defaulting the tail to
//! `(width = script %f, speed = 1.0, flag_a = 1, ext_a = ext_b = 10000.0,
//! flag_b = 1)` when the line is short. `FUN_00551240` forwards to the exe's
//! custom scanner `FUN_00553890`, which stores `%lf` as 4-byte `f32`
//! (differential-tested under the Unicorn oracle: `FUN_00551240` on stock
//! lines returns 8/9/12 with the exact `f32` field values this parser
//! produces). Accepted points are chained by `FUN_0057de20` into `0x54`-byte
//! nodes (13 floats + flag + 6 words) via `FUN_0057c590`; the first line only
//! seeds the previous point and emits nothing.

use slr_fmt_core::{FormatError, Result};

/// Exe default for a missing/short tail: `local_14c = param_2` (the
/// `AI_spline` script `%f`), then `1.0, 1, 10000.0, 10000.0, 1`.
/// exe: `FUN_004337e0` initialises `local_13c = 1.0`, `local_128 = 1`,
/// `local_140 = local_12c = 10000.0`, `local_118 = 1` and clamps a negative
/// scanned width back to the script value.
pub const DEFAULT_SPEED: f32 = 1.0;
/// exe: `FUN_004337e0` `local_140` / `local_12c` (`0x461c4000`).
pub const DEFAULT_EXT: f32 = 10_000.0;
/// exe: `FUN_004337e0` `local_128` / `local_118`.
pub const DEFAULT_FLAG: i32 = 1;

/// One `.spl` control point: `x y z dx dy dz` plus the trailing columns.
/// exe: one `FUN_0057de20` node; the six trailing words land at node
/// offsets `+0x3c..+0x50`, the first twelve floats are laid out by
/// `FUN_0057c590` (prev/current positions and directions, segment factor).
#[derive(Debug, Clone, PartialEq)]
pub struct SplinePoint {
    /// Position. exe: `FUN_0057c590` words 0..2 (previous) / 3..5 (current).
    pub pos: [f32; 3],
    /// Unit segment direction. exe: words 6..11 (previous/current, normalised).
    pub dir: [f32; 3],
    /// Road half-width. exe: `local_14c`, default the `AI_spline` script `%f`;
    /// a scanned negative value falls back to that default.
    pub width: f32,
    /// Target speed. exe: `local_13c`, default `1.0`.
    pub speed: f32,
    /// Branch/pit flag. exe: `local_128` (`%d`), default `1`.
    pub flag_a: i32,
    /// First tuned distance (per-car in the ROC files). exe: `local_140`
    /// (`%lf`), default `10000.0`. `None` when the line has no `ext_*`
    /// columns (`race.spl` / `test.spl` shape).
    pub ext_a: Option<f32>,
    /// Second tuned distance. exe: `local_12c`, default `10000.0`.
    pub ext_b: Option<f32>,
    /// Braking-profile flag. exe: `local_118` (`%d`), default `1`.
    pub flag_b: Option<i32>,
}

/// A parsed `.spl` file: the accepted control points in file order.
/// exe: `FUN_004337e0` emits one `FUN_0057de20` node per accepted line after
/// the first (the first line only seeds the previous point).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Spline {
    pub points: Vec<SplinePoint>,
}

/// Parse one `%lf`-style float the way the exe's custom scanner does:
/// `strtod`-shaped, stored as `f32` (`FUN_00553890` via `FUN_00551240`).
fn parse_f32(token: &str) -> Option<f32> {
    // The exe scanner accepts the same leading/trailing whitespace the
    // surrounding split already removed; require a full token match so
    // `100.00` parses but `100.00x` does not.
    let t = token.trim();
    if t.is_empty() {
        return None;
    }
    // `f32::from_str` rejects hex and `inf`/`NaN` spellings the C scanner
    // would take; every stock/modded token is decimal, so a strict parse
    // that still tolerates `+`/leading-`.` matches the corpus exactly.
    t.parse::<f32>().ok()
}

fn parse_i32(token: &str) -> Option<i32> {
    let t = token.trim();
    if t.is_empty() {
        return None;
    }
    // exe `%d`: optional sign + digits, full-token match.
    let s = t.strip_prefix('+').unwrap_or(t);
    if s.is_empty() {
        return None;
    }
    let neg = s.starts_with('-');
    let digits = if neg { &s[1..] } else { s };
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    t.parse::<i32>().ok()
}

/// Parse one `.spl` line; `script_default` seeds an empty/missing width the
/// way `param_2` does (`local_14c = param_2`, negative falls back too).
/// exe: `FUN_004337e0` + `FUN_00551240` with the `0x0060db98` format; the
/// caller keeps the point only when more than 5 conversions succeed.
fn parse_line(line: &str, script_default: f32) -> Option<SplinePoint> {
    // The exe scans with `%lf`/`%d`, which skip all whitespace (spaces and
    // tabs alike): tokenise on whitespace runs. A doubled tab (`\t\t`,
    // one stock ROC line) contributes no token, exactly as if the width
    // column were missing — the width then falls back to the script default.
    let toks: Vec<&str> = line.split_whitespace().collect();
    // Token counts on disk: 8 (`test.spl`), 9 (`race.spl`), 12 (ROC lines)
    // or 11 (one ROC line whose empty width column contributes no token).
    if toks.len() != 8 && toks.len() != 9 && toks.len() != 11 && toks.len() != 12 {
        return None;
    }
    // exe gating: the line is kept only when more than 5 conversions
    // succeed; the first six always convert here (else `None`).
    let mut f = [0f32; 6];
    for (i, tok) in toks[..6].iter().enumerate() {
        f[i] = parse_f32(tok)?;
    }
    // Tail parsing mirrors the fixed 12-slot scan: token 6 is width unless
    // the column was empty (11-token line), in which case the tail starts
    // at the speed token.
    let empty_width = toks.len() == 11;
    let raw: &[&str] = if empty_width {
        &toks[6..]
    } else {
        &toks[6.min(toks.len())..]
    };
    // Width: scanned, but a negative value falls back to the script default
    // (`if (local_14c < 0) local_14c = param_2`); missing/empty takes it directly.
    let (width, tail) = if empty_width || raw.is_empty() {
        (script_default, raw)
    } else {
        let w = parse_f32(raw[0])?;
        (if w < 0.0 { script_default } else { w }, &raw[1..])
    };
    // After the width: speed, flag_a, then the optional ext_a/ext_b/flag_b
    // triple (present only on full 12-slot lines).
    let speed = if !tail.is_empty() {
        parse_f32(tail[0])?
    } else {
        DEFAULT_SPEED
    };
    let flag_a = if tail.len() > 1 {
        parse_i32(tail[1])?
    } else {
        DEFAULT_FLAG
    };
    let rest: &[&str] = if tail.len() > 2 { &tail[2..] } else { &[] };
    let (ext_a, ext_b, flag_b) = if rest.len() == 3 {
        (
            Some(parse_f32(rest[0])?),
            Some(parse_f32(rest[1])?),
            Some(parse_i32(rest[2])?),
        )
    } else if rest.is_empty() {
        (None, None, None)
    } else {
        return None;
    };
    Some(SplinePoint {
        pos: [f[0], f[1], f[2]],
        dir: [f[3], f[4], f[5]],
        width,
        speed,
        flag_a,
        ext_a,
        ext_b,
        flag_b,
    })
}

/// Parse a `.spl` file. `script_default` is the `AI_spline` script `%f`
/// (`FUN_004337e0` `param_2`); pass `0.0` when no script context exists —
/// every stock line carries an explicit non-negative width except one ROC
/// line whose empty column then yields `0.0` (still parsed; flagged below).
/// exe: `FUN_004337e0` line loop; lines with 5 or fewer conversions are
/// dropped (`if (5 < iVar3)`), the rest are chained by `FUN_0057de20`.
pub fn parse(data: &[u8], script_default: f32) -> Result<Spline> {
    let text = data.iter().map(|&b| b as char).collect::<String>();
    let mut points = Vec::new();
    for (no, raw) in text.lines().enumerate() {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        match parse_line(line, script_default) {
            Some(p) => points.push(p),
            None => {
                return Err(FormatError::Inconsistent {
                    what: "spl line",
                    detail: format!(
                        "line {} has {} tab columns",
                        no + 1,
                        line.split('\t').count()
                    ),
                });
            }
        }
    }
    if points.is_empty() {
        return Err(FormatError::Inconsistent {
            what: "spl file",
            detail: "no spline points".to_string(),
        });
    }
    Ok(Spline { points })
}

/// Parse with the exe's own default seed: the stock `AI_spline` script `%f`
/// values are track speeds, but no stock line depends on the seed except the
/// single empty-width ROC column; callers that need that column exact pass
/// the script value to [`parse`].
pub fn parse_stock(data: &[u8]) -> Result<Spline> {
    parse(data, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_width_takes_script_default() {
        // Exact stock line 40 shape: `\t\t` leaves 11 tokens, so the tail
        // is (speed, flag_a, ext_a, ext_b, flag_b) and the width is seeded.
        let s = parse(b"9.823312 0.004147 35.329984 0.931514 0.000681 0.363704\t\t10.0\t0\t10000.0\t10000.0\t0", 7.5)
            .unwrap();
        assert_eq!(s.points.len(), 1);
        assert_eq!(s.points[0].width, 7.5);
        assert_eq!(s.points[0].speed, 10.0);
    }

    #[test]
    fn arities() {
        let t = parse_stock(b"0.0 0.0 -2000.0 0.0 0.0 -1.0\t10.0\t1.0").unwrap();
        assert_eq!(t.points.len(), 1);
        assert!(t.points[0].ext_a.is_none());
        let r = parse_stock(
            b"640.814938 3.966422 -1123.161435 0.478813 0.000000 0.877917\t5.0\t1.35\t0",
        )
        .unwrap();
        assert_eq!((r.points[0].flag_a, r.points[0].flag_b), (0, None));
    }
}

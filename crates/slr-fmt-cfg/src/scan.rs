//! The exe's private text-scanning primitives, as the particle/skid cfg parsers use them.
//!
//! The game does **not** use the CRT `sscanf`: `FUN_00553890` is its own implementation with
//! deliberately narrow behaviour that shows up in the data it accepts:
//!
//! * `%f`: optional `-` (a `+` fails), at least one digit, optional `.digits`; no exponent
//!   (`1e5` reads as `1`). The integer part accumulates in a wrapping `u32`; fraction digits
//!   are summed on the x87 stack (`acc += digit * scale; scale *= 0.1`) with the scale seeded
//!   from the f32 constant 0.1 and stepped by the f64 constant 0.1, so the result is *not*
//!   the correctly rounded value of the decimal string.
//! * `%d`: a leading `0x`/`0X` switches to hex (`FUN_00553ce0`), otherwise optional `-` and
//!   decimal digits (wrapping `u32`).
//! * `%x`: **requires** the `0x` prefix (`texture 1f` and `texture 0` do not convert).
//! * `%s`: leading whitespace skipped, then everything up to whitespace.
//!
//! A conversion that does not match leaves its destination untouched (uninitialised stack in
//! the exe); the callers model that as "keep the default", which is `0` for every slot the
//! exe leaves unset.
//!
//! Whitespace is the signed-char range 9..=13 plus space.

use slr_x87::{Precision, Rounding, X87};

const RC: Rounding = Rounding::NearestEven;

/// The `strtok` delimiter set, ` ,\t\n\r` (the exe's `FUN_00554c70` also treats NUL as one).
const DELIMS: &[u8] = b" ,\t\n\r";

fn is_space(b: u8) -> bool {
    (9..=13).contains(&b) || b == 0x20
}

/// Re-entrant `strtok` over one line (`FUN_00551200` -> `FUN_00554c70`).
///
/// exe: FUN_00554c70
pub struct Tokens<'a> {
    rest: &'a [u8],
}

impl<'a> Tokens<'a> {
    pub fn new(line: &'a [u8]) -> Self {
        Tokens { rest: line }
    }

    /// Next token, or `None` once the line is exhausted. Consecutive delimiters collapse, so
    /// the tab/space padding in the shipped scripts never yields empty fields.
    pub fn next_token(&mut self) -> Option<&'a [u8]> {
        let skip = self.rest.iter().take_while(|b| DELIMS.contains(b)).count();
        self.rest = &self.rest[skip..];
        // NUL ends the string exactly like the buffer end does.
        let line_end = self
            .rest
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.rest.len());
        let body = &self.rest[..line_end];
        if body.is_empty() {
            self.rest = &[];
            return None;
        }
        let len = body.iter().take_while(|b| !DELIMS.contains(b)).count();
        let tok = &body[..len];
        self.rest = &self.rest[len..];
        Some(tok)
    }
}

fn skip_spaces(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&b| is_space(b)).count();
    &s[n..]
}

fn digit(s: &[u8], i: usize) -> Option<u32> {
    match s.get(i) {
        Some(&c) if c.is_ascii_digit() => Some((c - b'0') as u32),
        _ => None,
    }
}

fn negate(v: X87) -> X87 {
    let mut b = v.to_bytes();
    b[9] ^= 0x80; // FCHS: flip the sign bit only
    X87::from_bytes(b)
}

/// `%f` (`FUN_00553890`, case `f`). `None` = no conversion.
///
/// exe: FUN_00553890
pub fn scan_f32(tok: &[u8], pc: Precision) -> Option<f32> {
    f32_at(skip_spaces(tok), pc).map(|(v, _)| v)
}

/// The `%f` body of `FUN_00553890` starting at the first non-space byte: the value and the
/// number of bytes consumed (sign, integer digits, `.`, every following digit).
fn f32_at(s: &[u8], pc: Precision) -> Option<(f32, usize)> {
    let (neg, mut i) = if s.first() == Some(&b'-') {
        (true, 1)
    } else {
        (false, 0)
    };
    digit(s, i)?;
    let mut int: u32 = 0;
    while let Some(d) = digit(s, i) {
        int = int.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    // `fild qword`: the integer part enters the register exactly (no f32 rounding).
    let mut acc = X87::from_i64(int as i64);
    if s.get(i) == Some(&b'.') {
        i += 1;
        let mut scale = X87::from_f32(f32::from_bits(0x3dcc_cccd)); // fld dword [0x5f08e8]
        let step = X87::from_f64(f64::from_bits(0x3fb9_9999_9999_999a)); // fmul qword [0x5f0d20]
        let mut precision = 0x400;
        while let Some(d) = digit(s, i) {
            if precision <= 0 {
                break;
            }
            let term = X87::from_i32(d as i32).mul(scale, pc, RC);
            acc = acc.add(term, pc, RC);
            scale = scale.mul(step, pc, RC);
            precision -= 1;
            i += 1;
        }
        // remaining digits are consumed and ignored
        while digit(s, i).is_some() {
            i += 1;
        }
    }
    if neg {
        acc = negate(acc);
    }
    Some((acc.to_f32(pc, RC), i))
}

/// `FUN_00553ce0`: `0x` + at least one hex digit.
///
/// exe: FUN_00553ce0
pub(crate) fn scan_hex_prefixed(s: &[u8]) -> Option<u32> {
    hex_at(s).map(|(v, _)| v)
}

/// `FUN_00553ce0` with the number of bytes consumed.
fn hex_at(s: &[u8]) -> Option<(u32, usize)> {
    if !(s.first() == Some(&b'0') && matches!(s.get(1), Some(b'x') | Some(b'X'))) {
        return None;
    }
    let mut i = 2;
    if !s.get(i).is_some_and(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let mut v: u32 = 0;
    while let Some(c) = s.get(i).filter(|c| c.is_ascii_hexdigit()) {
        v = v
            .wrapping_mul(16)
            .wrapping_add((*c as char).to_digit(16).unwrap());
        i += 1;
    }
    Some((v, i))
}

/// `%d` (`FUN_00553890`, case `d`). `None` = no conversion.
///
/// exe: FUN_00553890
pub fn scan_i32(tok: &[u8]) -> Option<i32> {
    let s = skip_spaces(tok);
    if s.first() == Some(&b'0') && matches!(s.get(1), Some(b'x') | Some(b'X')) {
        return scan_hex_prefixed(s).map(|v| v as i32);
    }
    let (neg, mut i) = if s.first() == Some(&b'-') {
        (true, 1)
    } else {
        (false, 0)
    };
    digit(s, i)?;
    let mut v: u32 = 0;
    while let Some(d) = digit(s, i) {
        v = v.wrapping_mul(10).wrapping_add(d);
        i += 1;
    }
    let v = v as i32;
    Some(if neg { v.wrapping_neg() } else { v })
}

/// `%x` (`FUN_00553890`, case `x`): needs the `0x` prefix. `None` = no conversion.
///
/// exe: FUN_00553890
pub fn scan_hex(tok: &[u8]) -> Option<u32> {
    scan_hex_prefixed(skip_spaces(tok))
}

/// `%s`: skip leading whitespace, take up to the next whitespace/NUL. The exe copies into a
/// 64-byte stack buffer without a width limit (stack overflow past 63 characters); callers
/// truncate to 63 bytes.
///
/// exe: FUN_00553890
pub fn scan_str(tok: &[u8]) -> &[u8] {
    let s = skip_spaces(tok);
    let n = s.iter().take_while(|&&b| !is_space(b) && b != 0).count();
    &s[..n]
}

/// One stored conversion of [`sscanf`].
#[derive(Debug, Clone, PartialEq)]
pub enum ScanVal {
    /// `%f`: the f32 the exe stores.
    F32(f32),
    /// `%d` / `%u` / `%x`: the 32-bit word the exe stores.
    I32(i32),
    /// `%c`: one byte.
    Chr(u8),
    /// `%s`: the bytes copied (the exe also writes a NUL after them).
    Str(Vec<u8>),
}

/// Result of [`sscanf`]: the return value and one slot per storing `%` conversion in format
/// order. A conversion that did not run (scanning stopped earlier) leaves its slot `None`:
/// the exe leaves the destination untouched.
#[derive(Debug, Clone, PartialEq)]
pub struct Scan {
    pub ret: i32,
    pub vals: Vec<Option<ScanVal>>,
}

impl Scan {
    pub fn f(&self, i: usize) -> Option<f32> {
        match self.vals.get(i)? {
            Some(ScanVal::F32(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn d(&self, i: usize) -> Option<i32> {
        match self.vals.get(i)? {
            Some(ScanVal::I32(v)) => Some(*v),
            _ => None,
        }
    }

    pub fn s(&self, i: usize) -> Option<&[u8]> {
        match self.vals.get(i)? {
            Some(ScanVal::Str(v)) => Some(v),
            _ => None,
        }
    }
}

fn is_ws(b: u8) -> bool {
    // `(char)b > 8 && (char)b < 14 || b == 0x20`: signed chars, so bytes >= 0x80 are not space.
    (9..=13).contains(&b) || b == 0x20
}

/// The exe's `sscanf` (`FUN_00551240` -> `FUN_00553890`): a private format interpreter, not the
/// CRT's. Whitespace in the format skips any whitespace in the input; other literal bytes must
/// match; `%f` (no sign `+`, no exponent), `%d`/`%u` (decimal wrapping u32, or `0x` hex),
/// `%x` (needs `0x`), `%s` (width, stops at whitespace, NUL or the next format byte), `%c`
/// (no whitespace skip), `%*` suppress, and `%#` which makes the return value the number of
/// input bytes consumed instead of the conversion count. Input beyond its end reads as NUL.
///
/// exe: FUN_00551240
/// exe: FUN_00553890
pub fn sscanf(input: &[u8], fmt: &[u8], pc: Precision) -> Scan {
    let inp = |i: usize| input.get(i).copied().unwrap_or(0);
    let fm = |i: usize| fmt.get(i).copied().unwrap_or(0);
    let n_dest = {
        let mut n = 0;
        let mut i = 0;
        while i < fmt.len() {
            if fmt[i] == b'%' {
                match fm(i + 1) {
                    b'#' => i += 1,
                    b'*' => {
                        i += 2;
                        while fm(i).is_ascii_digit() {
                            i += 1;
                        }
                    }
                    _ => n += 1,
                }
            }
            i += 1;
        }
        n
    };
    let mut vals: Vec<Option<ScanVal>> = vec![None; n_dest];
    let mut slot = 0usize;
    let mut count = 0i32;
    let mut hash = false;
    let mut p = 0usize; // input cursor
    let mut f = 0usize; // format cursor
    macro_rules! done {
        () => {
            return Scan { ret: count, vals }
        };
    }
    if fm(0) != 0 {
        loop {
            let c = fm(f);
            if is_ws(c) {
                while is_ws(inp(p)) {
                    p += 1;
                }
                while is_ws(fm(f)) {
                    f += 1;
                }
            }
            if fm(f) != b'%' {
                if fm(f) != inp(p) {
                    done!();
                }
                f += 1;
                p += 1;
            } else if fm(f + 1) == b'#' {
                f += 2;
                hash = true;
            } else {
                let mut q = f + 1;
                let suppress = fm(q) == b'*';
                if suppress {
                    q += 1;
                }
                // width
                let mut width: i32 = 0;
                if fm(q) >= b'0' && fm(q) < 0x80 {
                    while fm(q).is_ascii_digit() {
                        width = (fm(q) - b'0') as i32 + width.wrapping_mul(10);
                        q += 1;
                    }
                }
                if width == 0 {
                    width = 0x400;
                }
                // precision
                let mut prec: i32 = 0;
                if fm(q) == b'.' {
                    q += 1;
                    if fm(q) >= b'0' && fm(q) < 0x80 {
                        while fm(q).is_ascii_digit() {
                            prec = (fm(q) - b'0') as i32 + prec.wrapping_mul(10);
                            q += 1;
                        }
                    }
                }
                if prec == 0 {
                    prec = 0x400;
                }
                let _ = prec; // only bounds the fractional digits of `%f`; no format sets it
                if matches!(fm(q), b'h' | b'l' | b'L') {
                    q += 1;
                }
                let conv = fm(q);
                f = q + 1;
                let dest = if suppress {
                    None
                } else {
                    let d = slot;
                    slot += 1;
                    Some(d)
                };
                match conv | 0x20 {
                    b'c' => {
                        if let Some(d) = dest {
                            vals[d] = Some(ScanVal::Chr(inp(p)));
                        }
                        p += 1;
                        count += 1;
                    }
                    b'f' => {
                        while is_ws(inp(p)) {
                            p += 1;
                        }
                        match f32_at(&input[p.min(input.len())..], pc) {
                            None => done!(),
                            Some((v, n)) => {
                                p += n;
                                if let Some(d) = dest {
                                    vals[d] = Some(ScanVal::F32(v));
                                }
                                count += 1;
                            }
                        }
                    }
                    b's' => {
                        while is_ws(inp(p)) {
                            p += 1;
                        }
                        let mut w = width;
                        let mut out = Vec::new();
                        let next = fm(f);
                        loop {
                            let b = inp(p);
                            if is_ws(b) || b == 0 || w <= 0 || b == next {
                                break;
                            }
                            if dest.is_some() {
                                out.push(b);
                            }
                            p += 1;
                            w -= 1;
                        }
                        if let Some(d) = dest {
                            vals[d] = Some(ScanVal::Str(out));
                        }
                        count += 1;
                    }
                    b'u' | b'd' | b'x' => {
                        while is_ws(inp(p)) {
                            p += 1;
                        }
                        let rest = &input[p.min(input.len())..];
                        let hex = conv | 0x20 == b'x'
                            || (inp(p) == b'0' && matches!(inp(p + 1), b'x' | b'X'));
                        if hex {
                            match hex_at(rest) {
                                None => done!(),
                                Some((v, n)) => {
                                    p += n;
                                    if let Some(d) = dest {
                                        vals[d] = Some(ScanVal::I32(v as i32));
                                    }
                                    count += 1;
                                }
                            }
                        } else {
                            let unsigned = conv | 0x20 == b'u';
                            let neg = inp(p) == b'-';
                            let mut i = usize::from(neg);
                            if digit(rest, i).is_none() {
                                done!();
                            }
                            let mut v: u32 = 0;
                            while let Some(dg) = digit(rest, i) {
                                v = v.wrapping_mul(10).wrapping_add(dg);
                                i += 1;
                            }
                            p += i;
                            if let Some(d) = dest {
                                let v = v as i32;
                                let v = if !unsigned && neg {
                                    v.wrapping_neg()
                                } else {
                                    v
                                };
                                vals[d] = Some(ScanVal::I32(v));
                            }
                            count += 1;
                        }
                    }
                    _ => {}
                }
            }
            if fm(f) == 0 {
                break;
            }
        }
        if hash {
            return Scan {
                ret: p as i32,
                vals,
            };
        }
    }
    done!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(line: &str) -> Vec<String> {
        let mut t = Tokens::new(line.as_bytes());
        let mut out = Vec::new();
        while let Some(x) = t.next_token() {
            out.push(String::from_utf8_lossy(x).into_owned());
        }
        out
    }

    #[test]
    fn strtok_collapses_delimiters() {
        assert_eq!(toks("a\t\t1 2,3\r\n"), ["a", "1", "2", "3"]);
        assert_eq!(toks("  \r\n"), Vec::<String>::new());
        assert_eq!(toks("x\0y z"), ["x"]);
    }

    #[test]
    fn float_grammar_is_the_games_not_the_crts() {
        let f = |s: &str| scan_f32(s.as_bytes(), Precision::F32);
        assert_eq!(f("5."), Some(5.0));
        assert_eq!(f("-2.5"), Some(-2.5));
        assert_eq!(f("1e5"), Some(1.0)); // exponent is not parsed
        assert_eq!(f("+3"), None); // '+' is not a sign
        assert_eq!(f(".5"), None); // a digit must lead
        assert_eq!(f("abc"), None);
        assert_eq!(f("-0").map(f32::to_bits), Some(0x8000_0000));
        assert_eq!(f("0.5"), Some(0.5));
        assert_eq!(f("0.25"), Some(0.25));
    }

    #[test]
    fn int_and_hex_grammar() {
        assert_eq!(scan_i32(b"-12"), Some(-12));
        assert_eq!(scan_i32(b"1.9"), Some(1));
        assert_eq!(scan_i32(b"0x10"), Some(16)); // %d accepts a 0x prefix
        assert_eq!(scan_i32(b"+1"), None);
        assert_eq!(scan_hex(b"0x1F"), Some(0x1f));
        assert_eq!(scan_hex(b"1f"), None); // %x needs the prefix
        assert_eq!(scan_hex(b"0"), None);
        assert_eq!(scan_hex(b"0x"), None);
    }

    #[test]
    fn string_stops_at_whitespace() {
        assert_eq!(scan_str(b"s0"), b"s0");
        assert_eq!(scan_str(b"a\x0bb"), b"a");
    }
}

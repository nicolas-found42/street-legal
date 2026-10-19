//! The exe's mini-`sprintf` (`FUN_00553d70`) and decimal renderer
//! (`FUN_00554170`) — M1-06 slice 2.
//!
//! `FUN_00553d70` (reached through the import stub at `0x551220`) is the
//! formatter behind the three string-producing natives of this group:
//! `java.lang.Integer.toString` formats `"%d"` (`0x613398`),
//! `java.lang.Integer.toHexString` formats `"0x%08x"` (`0x61339c`) and
//! `java.lang.Float.toString` formats with the script-supplied descriptor
//! (`(FLjava.lang.String;)`). `FUN_00554170` (stub `0x551240`'s neighbour at
//! `0x554170`) is the decimal renderer it calls for the integer part of
//! `%d`/`%i` and of `%f`.
//!
//! The loop, from `re/oracle/oracle.py disasm 0x553d70`:
//!
//! ```text
//! 553d70  scan the format; every non-'%' byte is copied verbatim
//! 553db8  '0' flag -> pad = '0' (default ' '), at most one
//! 553dd8  width     -> decimal digits, `ebx = ebx*10 + (c-'0')` (wrapping)
//! 553df5  precision -> '.' then decimal digits (default -1, no digits -> 0)
//! 553e23  length    -> one 'h'/'l'/'L' is skipped
//! 553e42  dispatch table 0x5540f4, byte index table 0x554114 indexed by
//!         `(char - 0x25)` (so only 0x25..=0x78 reach it):
//!           '%%' 0x5540ce  'c' 0x553e57  'd'/'i' 0x553ec8  'f' 0x553fb3
//!           's'  0x553e6f  'x' 0x553efc  'X' 0x553ef8
//!         0x5540d8 (every other byte): nothing written, **no** argument read
//! 5540e5  NUL-terminate; the exe returns eax = 0
//! ```
//!
//! The supported set is therefore exactly `%% %c %d %i %f %s %x %X`. The
//! handoff lists `%u` as supported, but the index table's 'u' entry is the
//! default arm; probed on the exe, `"%u"` renders nothing and `"<%u|%d>"`
//! with `(300, 7)` renders `"<|300>"`, i.e. `%u` consumes no argument either.
//!
//! Argument cells are consumed in order, one per conversion: 4 bytes for
//! `%c`/`%d`/`%i`/`%s`/`%x`/`%X` and 8 bytes for `%f` (the exe walks a raw
//! pointer pair, read offset +4/+8 and advanced by 4/8). [`FormatArg`] models
//! that cell typing by kind; a kind that does not fit the conversion is a
//! typed error instead of the exe's reinterpretation of the same bytes.
//!
//! `%f` runs its arithmetic under the game's x87 configuration — PC=24
//! ([`crate::interp::SCRIPT_PRECISION`], ADR 0007) with RC=nearest — and the
//! two `__ftol` calls (`0x5d6750`, `FISTP` with a forced chop RC) truncate
//! toward zero. The probe cells quoted in the tests below were taken from the
//! exe with `fpu_cw = 0x007f` (the game word); Unicorn *does* honour PC for
//! these ops (checked: `%f` of `1/3` renders `"0.33333334"` at `0x7f` and
//! `"0.33333333"` at `0x27f`), so the vectors pin the PC=24 behaviour.

use crate::interp::SCRIPT_PRECISION;
use slr_x87::{Rounding, X87};

/// The formatter's x87 rounding control: the game FPCW is `0x007f`, whose
/// RC field is 0 (nearest-even) — the same control the language Math natives uses.
const RC: Rounding = Rounding::NearestEven;

/// `FMUL qword ptr [0x5f3ab8]` — the eight-fraction-digit scale (`1e8`,
/// verified in the image at that address).
const FRACTION_SCALE: f64 = 1e8;

/// `FCOMP qword ptr [0x5f0cf0]` — the fraction-rounding threshold `0.5`.
const ROUND_HALF: f64 = 0.5;

/// `FCOM qword ptr [0x5f0c18]` — the `%f` sign test, against `+0.0`.
const ZERO: f64 = 0.0;

/// The fraction buffer width (`push 8` at `0x554044`): the exe renders the
/// scaled fraction zero-padded to eight digits and then trims it.
const FRACTION_DIGITS: i32 = 8;

/// One variadic argument cell of the exe's `sprintf` call.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum FormatArg<'a> {
    /// A 4-byte cell. `%d`/`%i` read it as `i32` (sign at `ecx < 0`),
    /// `%x`/`%X` as its bits (the nibble loop shifts logically), `%c` as its
    /// low byte.
    Word(u32),
    /// The 8-byte cell `%f` loads (`FLD qword ptr [cursor+8]`).
    Double(f64),
    /// The bytes behind a `%s` char pointer, up to the NUL. The exe tests the
    /// pointer for null before dereferencing, so a null pointer renders
    /// exactly like an empty string.
    Str(&'a [u8]),
}

/// Why a format did not render.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FormatError {
    /// A conversion needed an argument the call did not supply. The exe reads
    /// on past the argument block instead (operand memory, not a value); the
    /// port fails closed rather than fabricate one.
    MissingArgument,
    /// The next argument's kind does not fit the conversion (e.g. a
    /// [`FormatArg::Word`] where `%f` needs an 8-byte cell).
    WrongKind,
}

/// The next format byte, advancing the cursor only when one was read.
struct Format<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Format<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, at: 0 }
    }

    /// The byte at the cursor, or `None` at the end. The exe keeps reading
    /// past the format's NUL (undefined operand bytes); the port stops.
    fn next(&mut self) -> Option<u8> {
        let byte = self.bytes.get(self.at).copied()?;
        self.at += 1;
        Some(byte)
    }
}

/// `FUN_00554170`: append the decimal rendering of `value` to `out`.
///
/// exe: FUN_00554170
///
/// ```text
/// 554170  ecx = value; `NEG ecx` when ecx < 0
/// 5541a2  do { edx:eax = IDIV ecx/10; *--buf = (dl) + '0'; ecx /= 10 }
///         while ecx != 0                       ; at least one digit
/// 5541d4  ebp = width, al = pad (' ' or '0')
/// 5541e4  sign: when value < 0, width -= 1 (if width > 0) and, unless the
///         pad is '0', pad spaces *before* the '-' up to the digit count
/// 55422d  pad the remaining width with `pad`, then copy the digits
/// 55426f  returns the end pointer
/// ```
///
/// The division is signed (`IDIV`), and the magnitude is the wrapped
/// two's-complement negation, so `i32::MIN` keeps its sign bit and renders the
/// exe's wrapped digits (`-./,),(-*,(`) rather than its true magnitude.
pub fn format_int(out: &mut Vec<u8>, value: i32, width: i32, pad: u8) {
    let negative = value < 0;
    let mut magnitude = if negative {
        value.wrapping_neg()
    } else {
        value
    };
    // At most ten digits: `|i32::MIN|` renders ten, and the loop halves the
    // magnitude's digit count each pass.
    let mut digits = [0u8; 10];
    let mut count = 0usize;
    loop {
        let quotient = magnitude / 10;
        let remainder = magnitude % 10;
        digits[count] = (remainder as u8).wrapping_add(b'0');
        count += 1;
        magnitude = quotient;
        if magnitude == 0 {
            break;
        }
    }
    let mut room = width;
    if negative {
        if room > 0 {
            room -= 1;
        }
        if pad != b'0' && room > count as i32 {
            let spaces = (room - count as i32) as usize;
            out.resize(out.len() + spaces, pad);
            room = count as i32;
        }
        out.push(b'-');
    }
    if room > count as i32 {
        let spaces = (room - count as i32) as usize;
        out.resize(out.len() + spaces, pad);
    }
    out.extend(digits[..count].iter().rev());
}

/// `FUN_00553d70`: render `fmt` with `args` into a fresh byte string.
///
/// exe: FUN_00553d70
///
/// Conversions, with the exe's per-conversion behaviour:
///
/// * `%%` writes `%` and consumes nothing (`0x5540ce`).
/// * `%c` writes the low byte of the cell; width and precision are parsed but
///   ignored (`0x553e57`).
/// * `%d`/`%i` are [`format_int`] with the parsed width and pad
///   (`0x553ec8`).
/// * `%x`/`%X` render the cell's nibbles (no sign), padding to width with
///   `pad` before the digits; `%X` uses `A`-`F` (`0x553ef8`, `0x553efc`).
/// * `%s` copies the pointed-at bytes and then pads with **spaces** (the pad
///   byte is hard-coded `0x20202020`) to width; width never truncates and
///   precision is ignored (`0x553e6f`).
/// * `%f` writes the sign, then [`format_int`] of the low 32 bits of the
///   truncated magnitude (so a magnitude outside `i32` is not the printed
///   integer part), then `.` and up to eight fraction digits — see
///   [`format_fixed`].
/// * every other conversion byte writes nothing and consumes nothing
///   (`0x5540d8`).
pub fn sprintf(fmt: &[u8], args: &[FormatArg<'_>]) -> Result<Vec<u8>, FormatError> {
    let mut out = Vec::with_capacity(fmt.len());
    let mut cursor = 0usize;
    let mut format = Format::new(fmt);
    while let Some(byte) = format.next() {
        if byte != b'%' {
            out.push(byte);
            continue;
        }
        let Some(mut ch) = format.next() else { break };
        let mut pad = b' ';
        if ch == b'0' {
            pad = b'0';
            let Some(next) = format.next() else { break };
            ch = next;
        }
        let mut width: i32 = 0;
        while ch.is_ascii_digit() {
            width = width.wrapping_mul(10).wrapping_add(i32::from(ch - b'0'));
            let Some(next) = format.next() else { break };
            ch = next;
        }
        let mut precision: i32 = -1;
        if ch == b'.' {
            precision = 0;
            let Some(next) = format.next() else { break };
            ch = next;
            while ch.is_ascii_digit() {
                precision = precision
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(ch - b'0'));
                let Some(next) = format.next() else { break };
                ch = next;
            }
        }
        if matches!(ch, b'h' | b'l' | b'L') {
            let Some(next) = format.next() else { break };
            ch = next;
        }
        match ch {
            b'%' => out.push(b'%'),
            b'c' => out.push(take_word(args, &mut cursor)? as u8),
            b's' => {
                let bytes = take_str(args, &mut cursor)?;
                out.extend_from_slice(bytes);
                let remaining = width.wrapping_sub(bytes.len() as i32);
                if remaining > 0 {
                    out.resize(out.len() + remaining as usize, b' ');
                }
            }
            b'd' | b'i' => format_int(&mut out, take_word(args, &mut cursor)? as i32, width, pad),
            b'x' | b'X' => {
                let word = take_word(args, &mut cursor)?;
                format_hex(&mut out, word, ch == b'X', width, pad);
            }
            b'f' => {
                let value = take_double(args, &mut cursor)?;
                format_fixed(&mut out, value, width, pad, precision);
            }
            _ => {}
        }
    }
    Ok(out)
}

/// `0x553ef8`/`0x553efc`: the hex arm of the dispatch. Digits are produced
/// from the low nibble up, so the first digit written is the least
/// significant; the buffer is read back most significant first.
fn format_hex(out: &mut Vec<u8>, word: u32, upper: bool, width: i32, pad: u8) {
    let mut digits = [0u8; 8];
    let mut count = 0usize;
    let mut value = word;
    loop {
        let nibble = (value & 0xf) as u8;
        digits[count] = if nibble < 10 {
            b'0' + nibble
        } else if upper {
            b'A' + (nibble - 10)
        } else {
            b'a' + (nibble - 10)
        };
        count += 1;
        value >>= 4;
        if value == 0 {
            break;
        }
    }
    if width > count as i32 {
        out.resize(out.len() + (width - count as i32) as usize, pad);
    }
    out.extend(digits[..count].iter().rev());
}

/// The `%f` arm of the dispatch (`0x553fb3`).
///
/// ```text
/// 553fb7  FLD qword ptr [cell]              ; |v| is exact in the register
/// 553fbd  FCOM 0.0                          ; C0 (or unordered) -> negative
/// 553fd9  FCHS, write '-'                   ; magnitude in ST0
/// 553fe4  __ftol(|v|) -> edx:eax            ; chop, 64-bit
/// 553fef  FUN_00554170(dst, eax, width, pad) ; only eax: the low 32 bits
/// 553fff  precision == 0 -> stop (no '.')
/// 55400c  FILD eax; FSUBR |v| -> |v| - (double)eax
/// 554019  FMUL 1e8
/// 554021  __ftol -> eax                     ; the scaled fraction, chopped
/// 55402c  FILD eax; FSUBR -> x - (double)eax
/// 554032  FCOMP 0.5; C0 clear -> eax += 1   ; round the fragment
/// 554042  FUN_00554170(buf, eax, 8, '0'), NUL-terminated
/// 554054  precision > 8 -> 8; < 0 -> strip trailing zeros (keep >= 1)
/// 554082  copy the kept digits, then '0'-pad to the kept count
/// ```
///
/// Both conversions are `FISTP qword` through `FUN_005d6750`, which installs
/// RC=chop on a saved control word; a magnitude outside the `i64` range (or
/// NaN/infinity) yields the indefinite `0x8000000000000000`, whose low word
/// `eax = 0` is what reaches the renderer — so `%f` of `1e20` prints
/// `0.00000001`, the same shape as the exe.
fn format_fixed(out: &mut Vec<u8>, value: f64, width: i32, pad: u8, precision: i32) {
    let negative = X87::from_f64(value).fcom(X87::from_f64(ZERO)).0.c0;
    let magnitude = if negative { -value } else { value };
    let integer = X87::from_f64(magnitude).to_i64(Rounding::Zero) as i32;
    if negative {
        out.push(b'-');
    }
    format_int(out, integer, width, pad);
    if precision == 0 {
        return;
    }
    out.push(b'.');
    let fraction = X87::from_f64(magnitude)
        .sub(X87::from_i32(integer), SCRIPT_PRECISION, RC)
        .mul(X87::from_f64(FRACTION_SCALE), SCRIPT_PRECISION, RC);
    let truncated = fraction.to_i64(Rounding::Zero) as i32;
    let remainder = fraction.sub(X87::from_i32(truncated), SCRIPT_PRECISION, RC);
    let digits = if remainder.fcom(X87::from_f64(ROUND_HALF)).0.c0 {
        truncated
    } else {
        truncated.wrapping_add(1)
    };
    let mut buffer = Vec::new();
    format_int(&mut buffer, digits, FRACTION_DIGITS, b'0');
    let keep = if precision > FRACTION_DIGITS {
        FRACTION_DIGITS
    } else if precision < 0 {
        // Strip trailing zeros, never below one digit. The scan reads
        // indices 7..=1, so a buffer shorter than eight bytes (which
        // `format_int(.., 8, '0')` cannot produce) would read a '0' here.
        let mut keep = 1;
        for index in (1..FRACTION_DIGITS).rev() {
            if buffer.get(index as usize).copied().unwrap_or(b'0') != b'0' {
                keep = index + 1;
                break;
            }
        }
        keep
    } else {
        precision
    };
    let copied = keep.max(0).min(buffer.len() as i32) as usize;
    out.extend_from_slice(&buffer[..copied]);
    let padded = keep.max(0) as usize;
    if padded > copied {
        out.resize(out.len() + (padded - copied), b'0');
    }
}

/// The next argument cell of a `%c`/`%d`/`%i`/`%x`/`%X` conversion.
fn take_word(args: &[FormatArg<'_>], cursor: &mut usize) -> Result<u32, FormatError> {
    match args.get(*cursor) {
        Some(FormatArg::Word(word)) => {
            *cursor += 1;
            Ok(*word)
        }
        Some(_) => Err(FormatError::WrongKind),
        None => Err(FormatError::MissingArgument),
    }
}

/// The next argument cell of a `%f` conversion.
fn take_double(args: &[FormatArg<'_>], cursor: &mut usize) -> Result<f64, FormatError> {
    match args.get(*cursor) {
        Some(FormatArg::Double(value)) => {
            *cursor += 1;
            Ok(*value)
        }
        Some(_) => Err(FormatError::WrongKind),
        None => Err(FormatError::MissingArgument),
    }
}

/// The next argument cell of a `%s` conversion.
fn take_str<'a>(args: &[FormatArg<'a>], cursor: &mut usize) -> Result<&'a [u8], FormatError> {
    match args.get(*cursor) {
        Some(FormatArg::Str(bytes)) => {
            *cursor += 1;
            Ok(bytes)
        }
        Some(_) => Err(FormatError::WrongKind),
        None => Err(FormatError::MissingArgument),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Render and unwrap; every expected string below is ASCII.
    fn render(fmt: &str, args: &[FormatArg<'_>]) -> String {
        let bytes = sprintf(fmt.as_bytes(), args).expect("renders");
        String::from_utf8(bytes).expect("ascii output")
    }

    /// Non-conversion bytes and `%%` pass through unchanged; `%` alone stops
    /// the render (the exe reads past the format's NUL there).
    #[test]
    fn literals_and_percent_escapes_pass_through() {
        assert_eq!(render("hello", &[]), "hello");
        assert_eq!(render("100%%", &[]), "100%");
        assert_eq!(render("%5%", &[]), "%");
        assert_eq!(render("%%5d", &[FormatArg::Word(9)]), "%5d");
        assert_eq!(render("abc%", &[]), "abc");
    }

    /// `%d` and `%i` are the same arm. Cell `0x8000_0000` is the exe's
    /// wrapped-digit case (signed `IDIV` on a negated `i32::MIN`).
    #[test]
    fn decimal_conversions_render_signed_cells() {
        assert_eq!(
            render(
                "%d|%i|%d",
                &[
                    FormatArg::Word(42),
                    FormatArg::Word((-7i32) as u32),
                    FormatArg::Word(0)
                ]
            ),
            "42|-7|0"
        );
        assert_eq!(render("%d", &[FormatArg::Word(0x7fff_ffff)]), "2147483647");
        assert_eq!(render("%d", &[FormatArg::Word(0x8000_0000)]), "-./,),(-*,(");
        assert_eq!(render("%d", &[FormatArg::Word((-1i32) as u32)]), "-1");
    }

    /// Width pads with the flag byte: space by default, `0` after the sign
    /// when the zero flag is given.
    #[test]
    fn width_and_zero_flag_pad_decimal() {
        assert_eq!(render("%13d", &[FormatArg::Word(0)]), "            0");
        assert_eq!(render("%5d", &[FormatArg::Word(42)]), "   42");
        assert_eq!(render("%05d", &[FormatArg::Word(42)]), "00042");
        assert_eq!(render("%5d", &[FormatArg::Word((-42i32) as u32)]), "  -42");
        assert_eq!(render("%05d", &[FormatArg::Word((-42i32) as u32)]), "-0042");
        assert_eq!(render("%1d", &[FormatArg::Word((-42i32) as u32)]), "-42");
        assert_eq!(render("%0d", &[FormatArg::Word(42)]), "42");
    }

    /// One `h`/`l`/`L` length modifier is skipped.
    #[test]
    fn length_modifiers_are_skipped() {
        assert_eq!(
            render(
                "%ld|%hd|%Ld",
                &[FormatArg::Word(1), FormatArg::Word(2), FormatArg::Word(3)]
            ),
            "1|2|3"
        );
    }

    /// A conversion byte outside the table writes nothing and reads no
    /// argument — `%u` included (the handoff lists it, the table does not).
    #[test]
    fn unsupported_conversions_write_nothing_and_consume_nothing() {
        assert_eq!(render("%u", &[FormatArg::Word(300)]), "");
        assert_eq!(
            render("<%u|%d>", &[FormatArg::Word(300), FormatArg::Word(7)]),
            "<|300>"
        );
        assert_eq!(
            render("<%q|%d>", &[FormatArg::Word(300), FormatArg::Word(7)]),
            "<|300>"
        );
    }

    /// `%x`/`%X` render nibbles without a sign and pad with the flag byte.
    #[test]
    fn hex_pads_nibbles_to_width() {
        assert_eq!(
            render(
                "%x|%X|%x",
                &[
                    FormatArg::Word(0),
                    FormatArg::Word(255),
                    FormatArg::Word(0xdead_beef)
                ]
            ),
            "0|FF|deadbeef"
        );
        assert_eq!(render("%08x", &[FormatArg::Word(5)]), "00000005");
        assert_eq!(render("%010x", &[FormatArg::Word(5)]), "0000000005");
        assert_eq!(render("%8x", &[FormatArg::Word(5)]), "       5");
        assert_eq!(
            render("%20x", &[FormatArg::Word(0xdead_beef)]),
            "            deadbeef"
        );
        assert_eq!(render("%08X", &[FormatArg::Word(255)]), "000000FF");
        assert_eq!(render("%x", &[FormatArg::Word(0x8000_0000)]), "80000000");
    }

    /// `%c` writes the cell's low byte; width and precision are ignored.
    #[test]
    fn char_writes_the_low_byte_ignoring_width() {
        assert_eq!(
            render(
                "%c%c%c",
                &[
                    FormatArg::Word(65),
                    FormatArg::Word(0x141),
                    FormatArg::Word(0x30)
                ]
            ),
            "AA0"
        );
        assert_eq!(render("%5c", &[FormatArg::Word(65)]), "A");
    }

    /// `%s` copies the whole pointed-at string and pads with spaces (the pad
    /// byte is hard-coded); width never truncates and precision is ignored.
    /// A null pointer and an empty string render alike.
    #[test]
    fn string_copies_then_space_pads() {
        assert_eq!(render("[%s]", &[FormatArg::Str(b"abc")]), "[abc]");
        assert_eq!(render("%8s|", &[FormatArg::Str(b"abc")]), "abc     |");
        assert_eq!(render("%2s|", &[FormatArg::Str(b"abc")]), "abc|");
        assert_eq!(render("%.2s|", &[FormatArg::Str(b"abc")]), "abc|");
        assert_eq!(render("%10.3s|", &[FormatArg::Str(b"abc")]), "abc       |");
        assert_eq!(render("%8s|", &[FormatArg::Str(b"")]), "        |");
    }

    /// `%f` with the default precision (none) prints one or more fraction
    /// digits with trailing zeros stripped.
    #[test]
    fn float_default_precision_strips_trailing_zeros() {
        for (value, want) in [
            (3.25, "3.25"),
            (-1.5, "-1.5"),
            (0.0, "0.0"),
            (-0.0, "0.0"),
            (100.0, "100.0"),
            (0.1, "0.1"),
            (2.675, "2.675"),
            (7.0, "7.0"),
            (1e-9, "0.0"),
            (1e-300, "0.0"),
        ] {
            assert_eq!(render("%f", &[FormatArg::Double(value)]), want, "{value}");
        }
    }

    /// The fraction is truncated (not rounded) to the requested precision,
    /// clamped to the eight-digit buffer.
    #[test]
    fn float_precision_truncates_and_clamps() {
        assert_eq!(
            render(
                "%.2f|%.0f|%.3f|%.8f|%.9f|%.15f",
                &[
                    FormatArg::Double(4.14159),
                    FormatArg::Double(3.7),
                    FormatArg::Double(1.23456),
                    FormatArg::Double(0.1),
                    FormatArg::Double(0.1),
                    FormatArg::Double(0.1),
                ]
            ),
            "4.14|3|1.234|0.10000000|0.10000000|0.10000000"
        );
        assert_eq!(render("%.f|", &[FormatArg::Double(3.25)]), "3|");
    }

    /// The eight-digit buffer is trimmed from the top, so a rounding carry
    /// that fills it collapses to the first digit: `0.999999999` -> `0.1`
    /// (also for a carried integer part, unchanged by precision).
    #[test]
    fn float_fraction_carry_collapses_to_one_digit() {
        assert_eq!(render("%f", &[FormatArg::Double(0.999999_999)]), "0.1");
        assert_eq!(render("%f", &[FormatArg::Double(1.999_999_999)]), "1.1");
        assert_eq!(render("%.2f", &[FormatArg::Double(1.999_999_999)]), "1.10");
        assert_eq!(render("%f", &[FormatArg::Double(5e-9)]), "0.00000001");
    }

    /// The `%f` width applies to the integer part only, *after* the sign.
    #[test]
    fn float_width_pads_the_integer_part_only() {
        assert_eq!(
            render("%10.2f|", &[FormatArg::Double(4.14159)]),
            "         4.14|"
        );
        assert_eq!(
            render("%010.2f|", &[FormatArg::Double(4.14159)]),
            "0000000004.14|"
        );
        assert_eq!(
            render("%8.2f|", &[FormatArg::Double(-4.14159)]),
            "-       4.14|"
        );
        assert_eq!(
            render("%13.0f|", &[FormatArg::Double(0.0)]),
            "            0|"
        );
    }

    /// Only `eax` of the `__ftol` pair reaches the renderer, so a magnitude
    /// outside `i32` prints its low 32 bits as the integer part, and the
    /// fraction (`|v|` minus that low word, scaled at PC=24) is what the
    /// eight-digit buffer sees.
    #[test]
    fn float_integer_part_is_the_low_32_bits_of_the_truncation() {
        for (value, want) in [
            (1e10, "1410065408.00000001"),
            (1e15, "-1530494976.00000001"),
            (5e9, "705032704.00000001"),
            (4_294_967_296.5, "0.00000001"),
            (1e20, "0.00000001"),
            (2147483647.5, "2147483647.5"),
            (f64::INFINITY, "0.00000001"),
            (f64::NEG_INFINITY, "-0.00000001"),
            (f64::NAN, "-0.0"),
        ] {
            assert_eq!(render("%f", &[FormatArg::Double(value)]), want, "{value}");
        }
    }

    /// The fraction arithmetic runs at PC=24 (the game word `0x7f`), which is
    /// observable: `1/3` fills the eighth digit upwards at PC=24 and stays
    /// `33333333` at PC=53.
    #[test]
    fn float_fraction_is_rounded_at_the_game_precision() {
        assert_eq!(render("%f", &[FormatArg::Double(1.0 / 3.0)]), "0.33333334");
    }

    /// Conversions consume one cell each, in order, `%f` two words' worth.
    #[test]
    fn conversions_consume_cells_in_order() {
        assert_eq!(
            render(
                "%d-%f-%s-%x",
                &[
                    FormatArg::Word(7),
                    FormatArg::Double(3.25),
                    FormatArg::Str(b"abc"),
                    FormatArg::Word(255)
                ]
            ),
            "7-3.25-abc-ff"
        );
        assert_eq!(
            render("%f|%f", &[FormatArg::Double(3.5), FormatArg::Double(-0.25)]),
            "3.5|-0.25"
        );
        assert_eq!(
            render(
                "%s|%d",
                &[FormatArg::Str(b"hello world"), FormatArg::Word(5)]
            ),
            "hello world|5"
        );
    }

    /// A conversion with no cell left, or with the wrong kind of cell, is a
    /// typed error — never a fabricated value.
    #[test]
    fn missing_or_mismatched_arguments_fail_closed() {
        assert_eq!(sprintf(b"%d", &[]), Err(FormatError::MissingArgument));
        assert_eq!(
            sprintf(b"%s", &[FormatArg::Word(1)]),
            Err(FormatError::WrongKind)
        );
        assert_eq!(
            sprintf(b"%f", &[FormatArg::Word(0)]),
            Err(FormatError::WrongKind)
        );
        assert_eq!(
            sprintf(b"%d%d", &[FormatArg::Word(1)]),
            Err(FormatError::MissingArgument)
        );
    }

    /// `FUN_00554170` directly: pad placement around the sign, the
    /// "at least one digit" loop, and no truncation when the digits exceed
    /// the width.
    #[test]
    fn format_int_matches_the_renderer_vectors() {
        let mut out = Vec::new();
        format_int(&mut out, 7, 4, b'0');
        assert_eq!(out, b"0007");

        out.clear();
        format_int(&mut out, 0, 0, b' ');
        assert_eq!(out, b"0");

        out.clear();
        format_int(&mut out, -5, 3, b' ');
        assert_eq!(out, b" -5");

        out.clear();
        format_int(&mut out, -5, 3, b'0');
        assert_eq!(out, b"-05");

        out.clear();
        format_int(&mut out, 12345, 0, b' ');
        assert_eq!(out, b"12345");

        out.clear();
        format_int(&mut out, -123, 5, b' ');
        assert_eq!(out, b" -123");

        out.clear();
        format_int(&mut out, 255, 8, b'0');
        assert_eq!(out, b"00000255");

        out.clear();
        format_int(&mut out, 100_000_000, 8, b'0');
        assert_eq!(out, b"100000000");

        out.clear();
        format_int(&mut out, 0x8000_0000u32 as i32, 0, b' ');
        assert_eq!(out, b"-./,),(-*,(");

        out.clear();
        format_int(&mut out, i32::MAX, 0, b' ');
        assert_eq!(out, b"2147483647");
    }
}

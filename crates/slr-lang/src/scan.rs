//! The exe's mini-`sscanf` (`FUN_00553890`) — M1-06 slice 3.
//!
//! `FUN_00553890` is the scanner behind two of this group's natives:
//! `java.lang.String.intValue` (`0x481ed0`, format `"%d"` at `0x6133a4`) and
//! `java.lang.String.floatValue` (`0x481f10`, `"%f"` at `0x6133a8`). Both call
//! the CRT stub at `0x551240`, which is not a plain thunk:
//!
//! ```text
//! 551240  mov ecx,[esp+8]      ; arg2 = format
//! 551244  mov edx,[esp+4]      ; arg1 = source
//! 551248  lea eax,[esp+0xc]    ; the varargs area itself
//! 55124c  push eax / push ecx / push edx      ; sscanf_core(src, fmt, char**)
//! 55124f  call 0x553890
//! ```
//!
//! The core takes the argument area *by pointer* and walks it one 4-byte cell
//! per conversion (`0x5539bc`: `mov edx,[esp+0x34]; mov ebp,[edx]; add edx,4`),
//! which is what [`sscanf`]'s `args` slice models.
//!
//! The loop, from `re/oracle/oracle.py disasm 0x553890`:
//!
//! ```text
//! 553890  eax = src, [esp+0x30] = fmt; a null src/fmt returns 0
//! 5538ca  a whitespace format byte eats any whitespace run in the input,
//!         then collapses the format's own run (`\t`..`\r` and space)
//! 55390d  '%' -> conversion, anything else -> a literal that must match
//! 55391c  '%#' sets the "return the consumed length" flag and loops
//! 553930  optional '*' (suppress: no cell is taken)
//! 553943  width, wrapping `*10 + digit`; 0 (or absent) -> 0x400
//! 55396a  '.' + precision, same accumulation and the same 0 -> 0x400
//! 553998  one 'h'/'l'/'L' is skipped
//! 5539aa  byte | 0x20, take the next cell, dispatch on (byte - 0x63) through
//!         the byte table at 0x553cbc into the jump table at 0x553ca0:
//!           'c' 0x5539e7  'd' 0x553aa0 (unsigned flag clear)
//!           's' 0x553a02  'u' 0x553a9b (sets the unsigned flag, falls into 'd')
//!           'f' 0x553b70  'x' 0x553b54 (calls the 0x553ce0 hex reader)
//!           0x553c62 every other byte: no input consumed, no store
//! 553c62  format exhausted -> eax = match count, or `cursor - src` when '%#'
//!         was seen; a conversion that fails outright returns the match count
//!         at once (0x553c71, 0x553c95) with the '%#' flag ignored
//! ```
//!
//! The supported set is therefore exactly `%c %d %f %s %u %x`. `%u` *is* in
//! this table (index 4 of `0x553cbc`, probed: `"%u"` over `"-5"` matches once
//! and stores 5) — unlike the sibling `sprintf`, where M6 found `%u` is the
//! default arm. `%i` is *not* here (its slot is the pass-through arm; probed:
//! `"%i"` over `"7"` matches zero times and leaves the cell alone), and neither
//! are `%e`/`%g`/`%o`/`%n`/`%[`. All the same, the unknown arm still takes an
//! argument cell — probed: `"%d% %d"` over `"5 6"` writes cells 0 and 2 and
//! leaves cell 1 alone.
//!
//! Per-conversion notes:
//!
//! * `%c` copies exactly one byte, ignores width and precision, and skips no
//!   whitespace (`mov byte ptr [ebp], al` — the cell's other three bytes are
//!   left as they were, see [`ScanArg::Word`]).
//! * `%d`/`%u` skip whitespace, then take the `0x553ce0` hex reader when the
//!   input starts `0x`/`0X` (so `"%d"` over `"0x10"` stores 16, probed);
//!   otherwise an optional `-` (consumed but *not* applied for `%u`) and at
//!   least one decimal digit, accumulated with `lea ecx,[ecx+ecx*4]` /
//!   `lea ecx,[eax+edx*2-0x30]` = `value*10 + digit` in a wrapping `u32`, with
//!   no overflow check (`"999999999999"` stores `0xd4a50fff`, probed). `+` is
//!   not handled at all: `"+5"` matches zero times (probed).
//! * `%x` takes the same hex reader directly, which insists on the `0x`/`0X`
//!   prefix and one hex digit (`"%x"` over `"1f"` matches zero times, probed)
//!   and accumulates `value << 4 | nibble`, wrapping.
//! * `%s` skips whitespace and copies until a whitespace byte, the input's NUL,
//!   the width, or **the format byte that follows the conversion** (the
//!   delimiter at `0x553a3d`); a target gets the bytes plus a NUL, and an empty
//!   token still counts (probed: `""` with `"%s"` matches once).
//! * `%f` needs an integer digit (no `+`, no leading `.`, no exponent), then
//!   `FILD qword [esp+0x20]` of the wrapping `u32` integer part zero-extended,
//!   and accumulates fraction digits as `sum += digit * weight` with `weight`
//!   starting at the **f32** `0.1` (`0x5f08e8`) and multiplied by the double
//!   `0.1` (`0x5f0d20`) after every digit — so `"8388609.5"` becomes
//!   `8388610.0` (the f32 weight's `+1.5e-9` tips the tie) while `"0.1"`
//!   stores `0x3dcccccd` exactly (probed). The store is `FSTP dword [ebp]`
//!   under the game's x87 word, i.e. [`SCRIPT_PRECISION`] with RC=nearest.
//!
//! The probe cells quoted in the tests below were taken from the exe with
//! `fpu_cw = 0x7f` (the game word) by calling the real stub `0x551240`
//! (`sscanf(src, fmt, &cell0, &cell1, ..)`) with the targets primed `0xaa` so
//! partial writes are visible. The whole `%f` path is PC-sensitive: the same
//! call under the CRT's own word `0x27f` (PC=53, the mingw default) gives
//! `"%f"` of `"1.2345"` as `0x3f9e0419` where the game word gives `0x3f9e0418`,
//! so the vectors pin [`SCRIPT_PRECISION`]. The port is bit-exact against a
//! 463-vector sweep at `0x7f` (300 random fraction strings of 1–14 digits with
//! random integer parts, the fixed cases below, and the
//! `%d`/`%u`/`%x`/`%c`/`%s`/`%i` cases), run from a throwaway harness that is
//! not part of this row.

use slr_vm::interp::SCRIPT_PRECISION;
use slr_x87::{Rounding, X87};

/// The scanner's x87 rounding control: the game FPCW is `0x007f`, whose RC
/// field is 0 (nearest-even) — the same control [`crate::format`] uses.
const RC: Rounding = Rounding::NearestEven;

/// `FLD dword ptr [0x5f08e8]` — the first fraction digit's weight, the **f32**
/// `0.1` (`0.100000001490116…`) widened to the register.
const FRACTION_FIRST: f32 = 0.1;

/// `FMUL qword ptr [0x5f0d20]` — every later fraction digit's weight is the
/// previous one times this double `0.1`.
const FRACTION_STEP: f64 = 0.1;

/// `0x553965`/`0x553993`: the width (`%s`) and precision (`%f`) used when the
/// format gives none — and what an explicit `0` is replaced by, so `"%.0f"`
/// accumulates up to 0x400 fraction digits rather than none (probed: `"%.0f"`
/// over `"1.2345"` stores `0x3f9e0419`, the full-precision value).
const DEFAULT_LIMIT: i32 = 0x400;

/// The dispatch (`0x5539cf`): `(byte | 0x20) - 0x63` indexes the byte table at
/// `0x553cbc`, whose values index the jump table at `0x553ca0`. Read out of the
/// image; `6` is the pass-through arm that lands at `0x553c62`.
const DISPATCH: [u8; 0x16] = [
    0, 1, 6, 2, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 6, 3, 6, 4, 6, 6, 5,
];

/// One argument cell of the exe's `sscanf` call — one 4-byte slot of the
/// caller's varargs area, which the core reads as a pointer and writes through.
#[derive(Debug)]
pub enum ScanArg<'a> {
    /// The cell of `%c`/`%d`/`%u`/`%x`. `%c` is `mov byte ptr [ebp], al`: it
    /// replaces the low byte and leaves the upper three as the caller had them
    /// (probed with a cell primed `0xaa`: `"%c"` over `"A"` gives `0xaaaaaa41`).
    Word(&'a mut u32),
    /// The cell `%f` stores into (`FSTP dword ptr [ebp]`, four bytes).
    Float(&'a mut f32),
    /// The buffer `%s` writes its token into. The exe copies the bytes through
    /// the cell's char pointer and NUL-terminates them; the port hands back the
    /// token alone, and an empty token is an empty buffer.
    Str(&'a mut Vec<u8>),
}

/// What a [`sscanf`] call observed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScanOutcome {
    /// The exe's `eax`, i.e. the number the native returns: [`matches`], or
    /// [`consumed`] once a `%#` flag was seen *and* the format ran to its end
    /// (`0x553c7d`). A conversion that fails outright returns the match count
    /// even with the flag set (`0x553c95`).
    ///
    /// [`matches`]: ScanOutcome::matches
    /// [`consumed`]: ScanOutcome::consumed
    pub returned: i32,
    /// Conversions that matched (`[esp+0x14]`) — suppressed (`*`) ones
    /// included, and `%s` counts even when it copies nothing.
    pub matches: i32,
    /// Where the input cursor stopped (`eax - src`), the length `%#` returns.
    /// A conversion that fails outright leaves the cursor where the failure was
    /// found: the hex reader only moves it once it has a digit (`0x553d5c`).
    pub consumed: usize,
}

/// Why a format did not scan.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    /// A conversion wanted a cell the call did not supply. The exe reads on past
    /// the argument area instead (operand memory, not a value); the port fails
    /// closed rather than fabricate one.
    MissingArgument,
    /// The next cell's kind does not fit the conversion (e.g. a
    /// [`ScanArg::Word`] where `%f` needs an [`ScanArg::Float`]).
    WrongKind,
}

/// `FUN_00553890`: scan `input` with `fmt`, storing into `args`.
///
/// exe: FUN_00553890
///
/// `input` and `fmt` are C strings without their terminators: a cursor at or
/// past the end reads the NUL, exactly like the exe. The one place the exe reads
/// *past* its buffers is a format that ends in whitespace while the input is
/// exhausted too (`0x5538f5` walks to the format's NUL, then compares the two
/// NULs and advances both); the port stops at the format's NUL and returns
/// there, which is the same result for every format a caller can mean to write.
pub fn sscanf(
    input: &[u8],
    fmt: &[u8],
    args: &mut [ScanArg<'_>],
) -> Result<ScanOutcome, ScanError> {
    let mut at = 0usize; // [esp+0x2c], the input cursor
    let mut fmt_at = 0usize; // [esp+0x30], the format cursor
    let mut matches: i32 = 0; // [esp+0x14]
    let mut hash = false; // [esp+0x13]
    let mut arg_at = 0usize; // [esp+0x34], the cell cursor

    loop {
        let byte = fmt_byte(fmt, fmt_at);
        if byte == 0 {
            break;
        }
        if is_space(byte) {
            // `0x5538d9`: the input's whitespace run first, then the format's
            // own run, which is why a run of format whitespace is one match.
            skip_space(input, &mut at);
            while is_space(fmt_byte(fmt, fmt_at)) {
                fmt_at += 1;
            }
            continue;
        }
        if byte != b'%' {
            // `0x553c54`: a literal has to match the input byte, NUL included.
            if byte != input_byte(input, at) {
                break;
            }
            fmt_at += 1;
            at += 1;
            continue;
        }

        // `0x553918`: the conversion specifier.
        fmt_at += 1;
        let mut ch = fmt_byte(fmt, fmt_at);
        if ch == b'#' {
            hash = true;
            fmt_at += 1;
            continue;
        }
        let mut suppressed = false;
        if ch == b'*' {
            suppressed = true;
            fmt_at += 1;
            ch = fmt_byte(fmt, fmt_at);
        }
        let mut width: i32 = 0;
        while ch.is_ascii_digit() {
            width = width.wrapping_mul(10).wrapping_add(i32::from(ch - b'0'));
            fmt_at += 1;
            ch = fmt_byte(fmt, fmt_at);
        }
        if width == 0 {
            width = DEFAULT_LIMIT;
        }
        let mut precision: i32 = 0;
        if ch == b'.' {
            fmt_at += 1;
            ch = fmt_byte(fmt, fmt_at);
            while ch.is_ascii_digit() {
                precision = precision
                    .wrapping_mul(10)
                    .wrapping_add(i32::from(ch - b'0'));
                fmt_at += 1;
                ch = fmt_byte(fmt, fmt_at);
            }
        }
        if precision == 0 {
            // `0x553993` is shared: no '.' takes the same default as `%.0f`.
            precision = DEFAULT_LIMIT;
        }
        if matches!(ch, b'h' | b'l' | b'L') {
            // `0x553998`: one length modifier is skipped and the exe never looks
            // at a second.
            fmt_at += 1;
            ch = fmt_byte(fmt, fmt_at);
        }
        let lower = ch | 0x20; // `or cl, 0x20`
        fmt_at += 1;
        // The byte after the conversion is `%s`'s delimiter.
        let delim = fmt_byte(fmt, fmt_at);
        let cell = if suppressed {
            None
        } else {
            // `0x5539bc`: a cell is taken before the dispatch, so an unknown
            // conversion byte consumes one too.
            if arg_at >= args.len() {
                return Err(ScanError::MissingArgument);
            }
            let index = arg_at;
            arg_at += 1;
            Some(index)
        };
        let arm = usize::from(lower.wrapping_sub(0x63));
        let arm = if arm < DISPATCH.len() {
            DISPATCH[arm]
        } else {
            6
        };
        match arm {
            // `%c` (`0x5539e7`).
            0 => {
                if let Some(index) = cell {
                    let word = word_cell(args, index)?;
                    let byte = input_byte(input, at);
                    *word = (*word & !0xff) | u32::from(byte);
                }
                at += 1;
                matches = matches.wrapping_add(1);
            }
            // `%d` (`0x553aa0`) and `%u` (`0x553a9b`).
            1 | 4 => {
                let unsigned = arm == 4;
                skip_space(input, &mut at);
                let mut hexed = None;
                if input_byte(input, at) == b'0' && matches!(input_byte(input, at + 1), b'x' | b'X')
                {
                    match scan_hex(input, &mut at) {
                        Some(value) => hexed = Some(value),
                        // `0x553c95`: the reader refused what it was pointed at,
                        // which ends the scan on the spot.
                        None => return Ok(failed(matches, at)),
                    }
                }
                let value = match hexed {
                    Some(value) => value,
                    None => {
                        let negative = input_byte(input, at) == b'-';
                        if negative {
                            at += 1;
                        }
                        if !input_byte(input, at).is_ascii_digit() {
                            // `0x553b06`: no digit behind the optional sign.
                            return Ok(failed(matches, at));
                        }
                        let mut digits: u32 = 0;
                        while input_byte(input, at).is_ascii_digit() {
                            digits = digits
                                .wrapping_mul(10)
                                .wrapping_add(u32::from(input_byte(input, at) - b'0'));
                            at += 1;
                        }
                        // `0x553b2f`: `%u` eats the '-' but never negates.
                        if negative && !unsigned {
                            digits = digits.wrapping_neg();
                        }
                        digits
                    }
                };
                if let Some(index) = cell {
                    *word_cell(args, index)? = value;
                }
                matches = matches.wrapping_add(1);
            }
            // `%f` (`0x553b88`).
            2 => {
                skip_space(input, &mut at);
                let negative = input_byte(input, at) == b'-';
                if negative {
                    at += 1;
                }
                if !input_byte(input, at).is_ascii_digit() {
                    return Ok(failed(matches, at));
                }
                // `FILD qword ptr [esp+0x20]`: the wrapping u32 accumulation,
                // zero-extended through the cleared high dword.
                let mut integer: u32 = 0;
                while input_byte(input, at).is_ascii_digit() {
                    integer = integer
                        .wrapping_mul(10)
                        .wrapping_add(u32::from(input_byte(input, at) - b'0'));
                    at += 1;
                }
                let mut value = X87::from_i64(i64::from(integer));
                if input_byte(input, at) == b'.' {
                    at += 1;
                    let mut weight = X87::from_f32(FRACTION_FIRST);
                    let mut left = precision;
                    loop {
                        let byte = input_byte(input, at);
                        if !byte.is_ascii_digit() || left <= 0 {
                            break;
                        }
                        let digit = X87::from_i32(i32::from(byte - b'0'));
                        at += 1;
                        left -= 1;
                        let term = digit.mul(weight, SCRIPT_PRECISION, RC);
                        value = value.add(term, SCRIPT_PRECISION, RC);
                        weight = weight.mul(X87::from_f64(FRACTION_STEP), SCRIPT_PRECISION, RC);
                    }
                    // `0x553c14`: digits past the precision are consumed but not
                    // accumulated.
                    while input_byte(input, at).is_ascii_digit() {
                        at += 1;
                    }
                }
                if negative {
                    value = fchs(value);
                }
                if let Some(index) = cell {
                    *float_cell(args, index)? = value.to_f32(SCRIPT_PRECISION, RC);
                }
                matches = matches.wrapping_add(1);
            }
            // `%s` (`0x553a02`).
            3 => {
                skip_space(input, &mut at);
                let mut target = match cell {
                    Some(index) => Some(str_cell(args, index)?),
                    None => None,
                };
                if let Some(target) = target.as_mut() {
                    target.clear();
                }
                let mut left = width;
                loop {
                    let byte = input_byte(input, at);
                    if is_space(byte) || byte == 0 || left <= 0 || byte == delim {
                        break;
                    }
                    left -= 1;
                    if let Some(target) = target.as_mut() {
                        target.push(byte);
                    }
                    at += 1;
                }
                matches = matches.wrapping_add(1);
            }
            // `%x` (`0x553b54`): whitespace, then the hex reader directly.
            5 => {
                skip_space(input, &mut at);
                match scan_hex(input, &mut at) {
                    Some(value) => {
                        if let Some(index) = cell {
                            *word_cell(args, index)? = value;
                        }
                        matches = matches.wrapping_add(1);
                    }
                    None => return Ok(failed(matches, at)),
                }
            }
            // Every other byte (`0x553c62`): the cell it took is dropped.
            _ => {}
        }
    }

    Ok(ScanOutcome {
        returned: if hash { at as i32 } else { matches },
        matches,
        consumed: at,
    })
}

/// The outcome of a conversion that failed after taking its cell: the exe
/// returns the match count at once (`0x553c71`, `0x553c95`), `%#` or not.
fn failed(matches: i32, at: usize) -> ScanOutcome {
    ScanOutcome {
        returned: matches,
        matches,
        consumed: at,
    }
}

/// The input byte at `at`, or the C string's terminator.
fn input_byte(input: &[u8], at: usize) -> u8 {
    input.get(at).copied().unwrap_or(0)
}

/// The format byte at `at`, or the terminator that ends the loop.
fn fmt_byte(fmt: &[u8], at: usize) -> u8 {
    fmt.get(at).copied().unwrap_or(0)
}

/// `0x5538ca`: `\t`..`\r` and space (`cmp cl,9` / `cmp cl,0xd` / `cmp cl,0x20`).
fn is_space(byte: u8) -> bool {
    (0x09..=0x0d).contains(&byte) || byte == 0x20
}

/// `0x5538d9`: skip the input's whitespace run.
fn skip_space(input: &[u8], at: &mut usize) {
    while is_space(input_byte(input, *at)) {
        *at += 1;
    }
}

/// `0x553ce0`: the hex reader shared by `%x` and the `0x` branch of `%d`/`%u`.
/// It insists on the `0x`/`0X` prefix and at least one hex digit, accumulates
/// `value << 4 | nibble` with no overflow check, and only then moves the cursor
/// (`0x553d5c`).
fn scan_hex(input: &[u8], at: &mut usize) -> Option<u32> {
    if input_byte(input, *at) != b'0' {
        return None;
    }
    if !matches!(input_byte(input, *at + 1), b'x' | b'X') {
        return None;
    }
    let mut cursor = *at + 2;
    if !is_hex_digit(input_byte(input, cursor)) {
        return None;
    }
    let mut value: u32 = 0;
    while is_hex_digit(input_byte(input, cursor)) {
        value = value << 4 | hex_digit(input_byte(input, cursor));
        cursor += 1;
    }
    *at = cursor;
    Some(value)
}

/// `0x553d02`/`0x553d1a`: `0`-`9`, `a`-`f`, `A`-`F`.
fn is_hex_digit(byte: u8) -> bool {
    byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte) || (b'A'..=b'F').contains(&byte)
}

/// `0x553d34`: the exe sign-extends the byte, subtracts `0x30`, then `7` above
/// `9` and `0x20` above `0xf` — for the three accepted ranges this is the plain
/// digit value.
fn hex_digit(byte: u8) -> u32 {
    if byte.is_ascii_digit() {
        u32::from(byte - b'0')
    } else {
        u32::from(byte | 0x20) - u32::from(b'a') + 10
    }
}

/// `FCHS` (`0x553c2b`): toggle the register's sign bit.
fn fchs(value: X87) -> X87 {
    let mut bytes = value.to_bytes();
    bytes[9] ^= 0x80;
    X87::from_bytes(bytes)
}

/// The 4-byte cell of `%c`/`%d`/`%u`/`%x`.
fn word_cell<'a>(args: &'a mut [ScanArg<'_>], index: usize) -> Result<&'a mut u32, ScanError> {
    match args.get_mut(index) {
        Some(ScanArg::Word(word)) => Ok(&mut **word),
        Some(_) => Err(ScanError::WrongKind),
        None => Err(ScanError::MissingArgument),
    }
}

/// The 4-byte cell of `%f`.
fn float_cell<'a>(args: &'a mut [ScanArg<'_>], index: usize) -> Result<&'a mut f32, ScanError> {
    match args.get_mut(index) {
        Some(ScanArg::Float(value)) => Ok(&mut **value),
        Some(_) => Err(ScanError::WrongKind),
        None => Err(ScanError::MissingArgument),
    }
}

/// The buffer of `%s`.
fn str_cell<'a>(args: &'a mut [ScanArg<'_>], index: usize) -> Result<&'a mut Vec<u8>, ScanError> {
    match args.get_mut(index) {
        Some(ScanArg::Str(bytes)) => Ok(&mut **bytes),
        Some(_) => Err(ScanError::WrongKind),
        None => Err(ScanError::MissingArgument),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A normal outcome: the exe returns the match count.
    fn scanned(matches: i32, consumed: usize) -> ScanOutcome {
        ScanOutcome {
            returned: matches,
            matches,
            consumed,
        }
    }

    /// An outcome with its own `returned` (the `%#` and failed-conversion
    /// paths).
    fn returned(returned: i32, matches: i32, consumed: usize) -> ScanOutcome {
        ScanOutcome {
            returned,
            matches,
            consumed,
        }
    }

    /// Scan one `%c`/`%d`/`%u`/`%x` conversion into a cell primed `0xaa`, the
    /// way the probe primes its target, so a partial write shows up.
    fn scan_word(input: &str, fmt: &str) -> (ScanOutcome, u32) {
        let mut cell = 0xaaaa_aaaa;
        let outcome = sscanf(
            input.as_bytes(),
            fmt.as_bytes(),
            &mut [ScanArg::Word(&mut cell)],
        );
        (outcome.expect("scans"), cell)
    }

    /// The same for `%f`; the returned word is the stored f32's bits.
    fn scan_float(input: &str, fmt: &str) -> (ScanOutcome, u32) {
        let mut cell = f32::from_bits(0xaaaa_aaaa);
        let outcome = sscanf(
            input.as_bytes(),
            fmt.as_bytes(),
            &mut [ScanArg::Float(&mut cell)],
        );
        (outcome.expect("scans"), cell.to_bits())
    }

    /// The same for `%s`; the buffer comes back cleared and refilled.
    fn scan_str(input: &str, fmt: &str) -> (ScanOutcome, Vec<u8>) {
        let mut cell = b"xxxxxxxx".to_vec();
        let outcome = sscanf(
            input.as_bytes(),
            fmt.as_bytes(),
            &mut [ScanArg::Str(&mut cell)],
        );
        (outcome.expect("scans"), cell)
    }

    /// `%d` skips leading whitespace, takes one `-`, and accumulates digits with
    /// no overflow check; `+` is not a sign at all.
    #[test]
    fn decimal_wraps_and_ignores_plus() {
        assert_eq!(scan_word(" -1234xyz", "%d"), (scanned(1, 6), 0xffff_fb2e));
        assert_eq!(
            scan_word("999999999999", "%d"),
            (scanned(1, 12), 0xd4a5_0fff)
        );
        assert_eq!(scan_word("007", "%d"), (scanned(1, 3), 7));
        assert_eq!(scan_word("+5", "%d"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("", "%d"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("abc", "%d"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("-", "%d"), (scanned(0, 1), 0xaaaa_aaaa));
    }

    /// `%u` is in this scanner's table (index 4 of `0x553cbc`) — the cell is
    /// taken and written — but it only clears the sign flag: the `-` is eaten
    /// and the magnitude stored as it stands.
    #[test]
    fn unsigned_is_present_and_never_negates() {
        assert_eq!(scan_word("-5", "%u"), (scanned(1, 2), 5));
        assert_eq!(scan_word("5", "%u"), (scanned(1, 1), 5));
        assert_eq!(scan_word("4294967297", "%u"), (scanned(1, 10), 1));
    }

    /// `%i` is the pass-through arm here (probed: zero matches, cell intact),
    /// unlike the sibling `sprintf`, where `%d`/`%i` share an arm.
    #[test]
    fn integer_conversion_is_absent() {
        assert_eq!(scan_word("7", "%i"), (scanned(0, 0), 0xaaaa_aaaa));
    }

    /// `%d`/`%u` take the hex reader when the input starts `0x`/`0X`, and `%x`
    /// takes it directly — where it also demands the prefix.
    #[test]
    fn hex_needs_its_prefix() {
        assert_eq!(scan_word("0x10", "%d"), (scanned(1, 4), 0x10));
        assert_eq!(scan_word("0x10", "%u"), (scanned(1, 4), 0x10));
        assert_eq!(scan_word("0x", "%d"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("0x1f", "%x"), (scanned(1, 4), 0x1f));
        assert_eq!(scan_word("0XFF", "%x"), (scanned(1, 4), 0xff));
        assert_eq!(scan_word("0Xff", "%X"), (scanned(1, 4), 0xff));
        assert_eq!(
            scan_word("0xfffffffff", "%x"),
            (scanned(1, 11), 0xffff_ffff)
        );
        assert_eq!(scan_word("1f", "%x"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("0xz", "%x"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("0xz", "%d"), (scanned(0, 0), 0xaaaa_aaaa));
        // The `-` blocks the hex branch, so this is decimal 0 followed by 'x'.
        assert_eq!(scan_word("-0x10", "%d"), (scanned(1, 2), 0));
        assert_eq!(scan_word("-0x10", "%u"), (scanned(1, 2), 0));
    }

    /// `%f` needs an integer digit, takes no `+` and no exponent, and stores an
    /// f32 (its cell's bits are the probe's).
    #[test]
    fn float_needs_an_integer_digit() {
        assert_eq!(scan_float("3.25", "%f"), (scanned(1, 4), 0x4050_0000));
        assert_eq!(scan_float("  3.5", "%f"), (scanned(1, 5), 0x4060_0000));
        assert_eq!(scan_float("-3.5", "%f"), (scanned(1, 4), 0xc060_0000));
        assert_eq!(scan_float("5.", "%f"), (scanned(1, 2), 0x40a0_0000));
        assert_eq!(scan_float("-0", "%f"), (scanned(1, 2), 0x8000_0000));
        assert_eq!(scan_float(".5", "%f"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_float("+1.5", "%f"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_float("1.5e3", "%f"), (scanned(1, 3), 0x3fc0_0000));
        assert_eq!(scan_float("1.2.3", "%f"), (scanned(1, 3), 0x3f99_999a));
    }

    /// The fraction's first weight is the **f32** `0.1`, later ones are the
    /// double `0.1` times the previous — the quirk that makes `0.1` round to the
    /// f32 `0.1` exactly but tips `8388609.5` up to `8388610.0`.
    #[test]
    fn float_fraction_uses_the_f32_weight() {
        assert_eq!(scan_float("0.1", "%f"), (scanned(1, 3), 0x3dcc_cccd));
        assert_eq!(scan_float("0.3", "%f"), (scanned(1, 3), 0x3e99_999a));
        assert_eq!(scan_float("0.5", "%f"), (scanned(1, 3), 0x3f00_0000));
        assert_eq!(scan_float("2.675", "%f"), (scanned(1, 5), 0x402b_3333));
        assert_eq!(scan_float("8388609.5", "%f"), (scanned(1, 9), 0x4b00_0002));
        assert_eq!(scan_float("123.456", "%f"), (scanned(1, 7), 0x42f6_e979));
        assert_eq!(scan_float("0.000001", "%f"), (scanned(1, 8), 0x3586_37bc));
        assert_eq!(
            scan_float("1.23456789012", "%f"),
            (scanned(1, 13), 0x3f9e_0652)
        );
    }

    /// The integer part is the wrapping `u32` accumulation, so a `%f` above 2^32
    /// keeps only what fits; digits past the precision are consumed.
    #[test]
    fn float_integer_part_wraps_and_precision_limits_the_fraction() {
        assert_eq!(scan_float("16777217", "%f"), (scanned(1, 8), 0x4b80_0000));
        assert_eq!(
            scan_float("4294967297.5", "%f"),
            (scanned(1, 12), 0x3fc0_0000)
        );
        assert_eq!(scan_float("1.2345", "%.2f"), (scanned(1, 6), 0x3f9d_70a4));
        assert_eq!(scan_float("1.2345", "%.1f"), (scanned(1, 6), 0x3f99_999a));
        assert_eq!(scan_float("1.23456", "%.3f"), (scanned(1, 7), 0x3f9d_f3b6));
        assert_eq!(scan_float("123.456", "%.2f"), (scanned(1, 7), 0x42f6_e667));
        assert_eq!(scan_float("-123.456", "%.2f"), (scanned(1, 8), 0xc2f6_e667));
        // A precision the accumulation never reaches keeps every digit.
        assert_eq!(
            scan_float("1.23456789012", "%.13f"),
            (scanned(1, 13), 0x3f9e_0652)
        );
        // `%.0f` is not "no fraction": a precision of 0 becomes 0x400, so this
        // is the same accumulation as the plain `"%f"` above.
        assert_eq!(scan_float("1.2345", "%.0f"), (scanned(1, 6), 0x3f9e_0418));
    }

    /// Longer sums, near-tie cases and the subnormal-ish end of the range.
    #[test]
    fn float_sum_keeps_the_full_accumulation() {
        assert_eq!(
            scan_float("3.14159265358979", "%f"),
            (scanned(1, 16), 0x4049_0fd9)
        );
        assert_eq!(
            scan_float("0.99999999999", "%f"),
            (scanned(1, 13), 0x3f80_0000)
        );
        assert_eq!(scan_float("1.0000001", "%f"), (scanned(1, 9), 0x3f80_0001));
        assert_eq!(
            scan_float("99999.99999", "%f"),
            (scanned(1, 11), 0x47c3_5000)
        );
        assert_eq!(
            scan_float("123456789.123456789", "%f"),
            (scanned(1, 19), 0x4ceb_79a3)
        );
        assert_eq!(scan_float("0.0000001", "%f"), (scanned(1, 9), 0x33d6_bf93));
        assert_eq!(
            scan_float("0.000000000001", "%f"),
            (scanned(1, 14), 0x2b8c_bccb)
        );
        assert_eq!(
            scan_float("-0.957551313735", "%f"),
            (scanned(1, 15), 0xbf75_2216)
        );
        assert_eq!(scan_float("0.0", "%f"), (scanned(1, 3), 0x0000_0000));
    }

    /// `%s` stops at whitespace, the input's NUL, the width, or the format byte
    /// after the conversion — and an empty token still matches.
    #[test]
    fn string_stops_at_whitespace_width_and_the_format_delimiter() {
        assert_eq!(
            scan_str("hello world", "%s"),
            (scanned(1, 5), b"hello".to_vec())
        );
        // The ',' is the delimiter (the byte after 's'), so it is left for the
        // format's own ',' and the cursor ends past it.
        assert_eq!(scan_str("abc,def", "%s,"), (scanned(1, 4), b"abc".to_vec()));
        assert_eq!(scan_str("hello", "%2s"), (scanned(1, 2), b"he".to_vec()));
        assert_eq!(scan_str("", "%s"), (scanned(1, 0), Vec::new()));
        assert_eq!(scan_str(",a", "%s,"), (scanned(1, 1), Vec::new()));
        assert_eq!(scan_str("a\tb", "%s"), (scanned(1, 1), b"a".to_vec()));
    }

    /// Two conversions take two cells in order; a literal between them has to
    /// match.
    #[test]
    fn conversions_take_cells_in_order() {
        let mut first = 0xaaaa_aaaa;
        let mut second = 0xaaaa_aaaa;
        let outcome = sscanf(
            b"7,9",
            b"%d,%d",
            &mut [ScanArg::Word(&mut first), ScanArg::Word(&mut second)],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(2, 3));
        assert_eq!((first, second), (7, 9));

        let mut first = Vec::new();
        let mut second = Vec::new();
        let outcome = sscanf(
            b"a,b",
            b"%s,%s",
            &mut [ScanArg::Str(&mut first), ScanArg::Str(&mut second)],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(2, 3));
        assert_eq!(
            (first.as_slice(), second.as_slice()),
            (&b"a"[..], &b"b"[..])
        );
    }

    /// `%c` writes one byte and leaves the rest of the cell alone; width and
    /// precision are parsed and ignored, and no whitespace is skipped.
    #[test]
    fn char_writes_one_byte_only() {
        assert_eq!(scan_word("A", "%c"), (scanned(1, 1), 0xaaaa_aa41));
        assert_eq!(scan_word("   5", "%c"), (scanned(1, 1), 0xaaaa_aa20));
        assert_eq!(scan_word("AB", "%2c"), (scanned(1, 1), 0xaaaa_aa41));
    }

    /// Whitespace in the format eats an input run of any length (`\v`/`\f`/`\r`
    /// included) and collapses the format's own run.
    #[test]
    fn format_whitespace_matches_any_run() {
        assert_eq!(scan_word("  5", " %d"), (scanned(1, 3), 5));
        assert_eq!(scan_word("5", "%d "), (scanned(1, 1), 5));
        assert_eq!(scan_word("5   ", "%d "), (scanned(1, 4), 5));
        assert_eq!(scan_word("5", "   "), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("\x0b\x0c\r5", "%d"), (scanned(1, 4), 5));
    }

    /// A literal byte has to match exactly, NUL included; a mismatch ends the
    /// scan with the matches so far.
    #[test]
    fn literals_must_match() {
        let mut cell = 0xaaaa_aaaa;
        assert_eq!(
            sscanf(b"abc", b"abc", &mut [ScanArg::Word(&mut cell)]).expect("scans"),
            scanned(0, 3)
        );
        assert_eq!(
            sscanf(b"abd", b"abc", &mut [ScanArg::Word(&mut cell)]).expect("scans"),
            scanned(0, 2)
        );
        assert_eq!(
            sscanf(b"5", b"%d,", &mut [ScanArg::Word(&mut cell)]).expect("scans"),
            scanned(1, 1)
        );
    }

    /// `%#` switches the return value from the match count to the consumed
    /// length, but only on the "format ran to its end" path — a conversion that
    /// fails outright still returns the match count.
    #[test]
    fn hash_flag_returns_the_consumed_length() {
        assert_eq!(scan_word("abc", "%#"), (scanned(0, 0), 0xaaaa_aaaa));
        assert_eq!(scan_word("5x", "%d%#"), (returned(1, 1, 1), 5));
        assert_eq!(
            scan_str("ab cd", "%s%#"),
            (returned(2, 1, 2), b"ab".to_vec())
        );
        assert_eq!(scan_word("A", "%c%#"), (returned(1, 1, 1), 0xaaaa_aa41));
        // `d` is a literal here (the flag loops instead of converting), and the
        // failing `%x` returns the count, not the length.
        assert_eq!(scan_word("d1f", "%#d%x"), (scanned(0, 1), 0xaaaa_aaaa));
        assert_eq!(scan_word("d0x1f", "%#d%x"), (returned(5, 1, 5), 0x1f));
        // The same for a failed conversion late in the format: the flag is
        // set, but the return is still the match count.
        let mut first = 0xaaaa_aaaa;
        let mut second = 0xaaaa_aaaa;
        let outcome = sscanf(
            b"5,",
            b"%d%#%x",
            &mut [ScanArg::Word(&mut first), ScanArg::Word(&mut second)],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(1, 1));
        assert_eq!((first, second), (5, 0xaaaa_aaaa));
    }

    /// `*` suppresses the store but still counts, and takes no cell.
    #[test]
    fn star_suppresses_the_store_only() {
        let mut first = 0xaaaa_aaaa;
        let mut second = 0xaaaa_aaaa;
        let outcome = sscanf(
            b"1 2",
            b"%*d %d",
            &mut [ScanArg::Word(&mut first), ScanArg::Word(&mut second)],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(2, 3));
        assert_eq!((first, second), (2, 0xaaaa_aaaa));

        let mut bytes = b"xxxx".to_vec();
        let outcome = sscanf(b"aa bb", b"%*s %s", &mut [ScanArg::Str(&mut bytes)]).expect("scans");
        assert_eq!(outcome, scanned(2, 5));
        assert_eq!(bytes, b"bb".to_vec());
    }

    /// A conversion byte outside the table consumes a cell all the same — the
    /// cell is taken before the dispatch — which is what moves the next
    /// conversion onto the following slot.
    #[test]
    fn unknown_conversions_still_take_a_cell() {
        let mut first = 0xaaaa_aaaa;
        let mut second = 0xaaaa_aaaa;
        let mut third = 0xaaaa_aaaa;
        let outcome = sscanf(
            b"5 6",
            b"%d% %d",
            &mut [
                ScanArg::Word(&mut first),
                ScanArg::Word(&mut second),
                ScanArg::Word(&mut third),
            ],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(2, 3));
        assert_eq!([first, second, third], [5, 0xaaaa_aaaa, 6]);
    }

    /// One `h`/`l`/`L` is skipped, and a trailing `%` is the pass-through arm —
    /// which takes a cell of its own — followed by the end of the format.
    #[test]
    fn length_modifiers_and_a_trailing_percent() {
        assert_eq!(scan_word("42", "%ld"), (scanned(1, 2), 42));
        let mut first = 0xaaaa_aaaa;
        let mut second = 0xaaaa_aaaa;
        let outcome = sscanf(
            b"5",
            b"%d%",
            &mut [ScanArg::Word(&mut first), ScanArg::Word(&mut second)],
        )
        .expect("scans");
        assert_eq!(outcome, scanned(1, 1));
        assert_eq!((first, second), (5, 0xaaaa_aaaa));
    }

    /// `%s` writes only through a real target: the suppressed form leaves its
    /// cell alone. `%f` and `%s` refuse a cell of the wrong kind rather than
    /// reinterpret its bytes.
    #[test]
    fn cells_are_typed() {
        let mut cell = 0xaaaa_aaaa;
        let outcome = sscanf(b"ab", b"%*s", &mut [ScanArg::Word(&mut cell)]).expect("scans");
        assert_eq!(outcome, scanned(1, 2));
        assert_eq!(cell, 0xaaaa_aaaa);

        assert_eq!(
            sscanf(b"5", b"%d", &mut [ScanArg::Float(&mut 0.0)]).unwrap_err(),
            ScanError::WrongKind
        );
        assert_eq!(
            sscanf(b"5", b"%f", &mut [ScanArg::Word(&mut cell)]).unwrap_err(),
            ScanError::WrongKind
        );
        assert_eq!(
            sscanf(b"5", b"%s", &mut [ScanArg::Word(&mut cell)]).unwrap_err(),
            ScanError::WrongKind
        );
    }

    /// A conversion whose cell is missing fails closed — the unknown arm, which
    /// still takes one, included.
    #[test]
    fn missing_cells_fail_closed() {
        assert_eq!(
            sscanf(b"5", b"%d", &mut []).unwrap_err(),
            ScanError::MissingArgument
        );
        assert_eq!(
            sscanf(b"5", b"%i", &mut []).unwrap_err(),
            ScanError::MissingArgument
        );
        assert_eq!(
            sscanf(b"5", b"%*d", &mut []).expect("suppressed needs no cell"),
            scanned(1, 1)
        );
    }

    /// The three outcome fields of the paths the probe pins: a normal end, the
    /// `%#` end, and a failed conversion.
    #[test]
    fn outcome_fields_match_the_exes_eax() {
        let (outcome, _) = scan_word("5x", "%d%#");
        assert_eq!(outcome.returned, 1);
        assert_eq!(outcome.matches, 1);
        assert_eq!(outcome.consumed, 1);

        let (outcome, _) = scan_word("abc", "%#");
        assert_eq!(outcome.returned, 0);
        assert_eq!(outcome.matches, 0);
        assert_eq!(outcome.consumed, 0);

        let (outcome, _) = scan_word("d1f", "%#d%x");
        assert_eq!(outcome.returned, 0);
        assert_eq!(outcome.matches, 0);
        assert_eq!(outcome.consumed, 1);
    }

    /// The two natives' own call shapes, end to end: `intValue` is `"%d"` and
    /// `floatValue` is `"%f"`.
    #[test]
    fn the_native_call_shapes() {
        let mut integer = 0u32;
        let outcome =
            sscanf(b"-1234xyz", b"%d", &mut [ScanArg::Word(&mut integer)]).expect("scans");
        assert_eq!(outcome.returned, 1);
        assert_eq!(integer as i32, -1234);

        let mut float = 0.0f32;
        let outcome = sscanf(b"3.25", b"%f", &mut [ScanArg::Float(&mut float)]).expect("scans");
        assert_eq!(outcome.returned, 1);
        assert_eq!(float, 3.25);
    }
}

//! `java.lang.Integer`/`java.lang.Float` string conversions (M1-06 rows 8-9).
//!
//! The three natives are thin wrappers around the exe's own mini-`sprintf`
//! ([`crate::format`], `FUN_00553d70`, reached through the import stub
//! `0x551220`), so the characters are produced by exactly the routine the exe
//! calls:
//!
//! | class | name | impl | format string |
//! |---|---|---|---|
//! | `java.lang.Integer` | `toString` | `LAB_00481e30` | `"%d"` (`0x613398`) |
//! | `java.lang.Integer` | `toHexString` | `LAB_00481e80` | `"0x%08x"` (`0x61339c`) |
//! | `java.lang.Float` | `toString` | `LAB_00481dd0` | the second script argument |
//!
//! Both `Integer` rows marshall their one `I` parameter, `sprintf` the
//! buffer, then hand the characters to the JVM's string interner
//! (`FUN_004174a0`) — the seam's string path, [`crate::seam::intern_string`] —
//! and return the interned `java.lang.String` object word in `eax`.
//!
//! ```text
//! 481e3e  lea  eax, [esp+4]     ; the native's 0x404-byte frame buffer
//! 481e42  push eax
//! 481e43  push 0                ; no receiver out
//! 481e45  push esi              ; CallInfo
//! 481e46  call 0x45d910         ; marshal (I): the parameter word
//! 481e52  push ecx              ; the marshalled word -- the %d argument
//! 481e57  push 0x613398         ; "%d"
//! 481e5d  call 0x551220         ; sprintf(buffer, "%d", word)
//! 481e62  mov  ecx, [esi]       ; jvm
//! 481e6b  call 0x4174a0         ; intern(jvm, buffer) -> String object
//! 481e78  ret                   ; eax = the String object
//! ```
//!
//! The body at `0x481e80` is the same ten instructions with the format
//! pointer `0x61339c`; the handoff states the same.
//!
//! # `Float.toString` (`(FLjava.lang.String;)Ljava.lang.String;`)
//!
//! ```text
//! 481dde  lea  eax, [esp+4]     ; out[1]: the F parameter
//! 481de2  push eax
//! 481de3  lea  ecx, [esp+0xc]   ; out[2]: the L parameter
//! 481de7  push ecx
//! 481de8  push 0                ; no receiver out
//! 481dea  push esi              ; CallInfo
//! 481deb  call 0x45d910         ; marshal (FLjava.lang.String;)
//! 481df0  fld  dword [esp+0x18] ; the F payload, as f32
//! 481df4  mov  edx, [esp+0x14]  ; the L payload: the format char buffer
//! 481df8  fstp qword [esp+8]    ; store it as the double vararg
//! 481dfc  add  esp, 8           ; (the two pushes below reuse the space)
//! 481dff  push edx              ; the format
//! 481e00  lea  eax, [esp+0x18]  ; the output buffer
//! 481e04  push eax
//! 481e05  call 0x551220         ; sprintf(buffer, format, (double)f32)
//! 481e0a  add  esp, 0x10
//! 481e0d  lea  ecx, [esp+0xc]
//! 481e12  mov  ecx, [esi]       ; jvm
//! 481e14  call 0x4174a0         ; intern -> String object
//! ```
//!
//! Two differences from the `Integer` pair matter for the port:
//!
//! - The format is **script input**, not a literal: the second parameter's
//!   marshalled word is handed to `sprintf` as a char pointer. The port's
//!   String values resolve their actual VM ptr cell through
//!   [`crate::seam::string_bytes`]; a null or unknown buffer reads as an empty
//!   format. The
//!   exe returns without writing its buffer for a null format pointer
//!   (`0x553d78`/`0x553d87` jump straight to the epilogue), so its intern
//!   there is uninitialized stack; the probe interned the empty string.
//! - The `F` payload is loaded as **f32** and stored as **f64** before the
//!   call: the value the formatter sees is exactly `(double)f32`, so
//!   `0.3f` renders the widened `0.30000001192092896`, not `0.3`. f32→f64 is
//!   exact (24 bits into 53), so no rounding happens at the widening itself;
//!   the exe's `%f` arithmetic then runs on the widened value.
//!
//! A format whose conversions the one `%f` argument cannot satisfy (e.g.
//! `"%d"`, which in the exe reads the widened double's low word, or `"%s"`,
//! which the exe dereferences) renders empty in the port: [`sprintf`] reports
//! the kind mismatch and the port does not fabricate argument words. The exe
//! reads them out of its own stack frame instead.
//!
//! Because the formatter is the ported [`sprintf`], the rendering is the
//! exe's own `%d`/`%x`/`%f` — signed decimals (so `0x8000_0000` renders the
//! shipped build's wrapped digits, `"-./,),(-*,("`, not `"-2147483648"`),
//! unsigned zero-padded hex, and the exe's `%f` (an eight-digit fraction
//! buffer, trailing zeros trimmed at the default precision, precision clamped
//! to 8) — not a libc `printf`.

use slr_vm::host::{marshal_args, CallInfo, VmHost};

use crate::format::{sprintf, FormatArg};
use crate::seam::{intern_string, string_bytes, NativeResult, NativeTable};

/// The format string `Integer.toString` passes to `sprintf` (`0x613398`).
const DECIMAL: &[u8] = b"%d";

/// The format string `Integer.toHexString` passes (`0x61339c`).
const HEX: &[u8] = b"0x%08x";

/// The marshalled `I` parameter of an `(I)Ljava.lang.String;` call.
///
/// `FUN_0045d910(info, 0, &out)` writes the receiver/return word first and
/// the parameter after it (`count` includes that trailing slot), so the word
/// `sprintf` receives is `out[1]` — the exe reads it back with
/// `mov ecx, [esp+0x10]` before pushing it as the variadic argument.
///
/// A container the descriptor cannot satisfy leaves `out[1]` at the zero the
/// port's array starts with; the exe reads its own uninitialized frame word
/// instead. This is the "the port reads 0 there" rule the other marshalling
/// natives state, and the VM's argument collector cannot build a short
/// container for a registered `(I)…` row anyway.
fn int_parameter(info: &CallInfo<'_>) -> u32 {
    let mut out = [0u32; 2];
    let _ = marshal_args(info, &mut out);
    out[1]
}

/// `sprintf(buf, fmt, word)`, then `FUN_004174a0` — the tail both rows share.
///
/// The two formats are fixed literals with exactly one word conversion, so
/// [`sprintf`] cannot fail here (it would need a missing argument or a
/// mismatched kind); a failure is a port bug, not a script input.
fn formatted(vm: &mut VmHost, info: &CallInfo<'_>, fmt: &[u8]) -> u32 {
    let word = int_parameter(info);
    let text = sprintf(fmt, &[FormatArg::Word(word)])
        .expect("a fixed literal format with its one word argument always renders");
    intern_string(vm, &text)
}

/// `java.lang.Integer.toString(I)Ljava.lang.String;`
///
/// exe: LAB_00481e30
pub fn to_string(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    Some(formatted(vm, info, DECIMAL))
}

/// `java.lang.Integer.toHexString(I)Ljava.lang.String;`
///
/// exe: LAB_00481e80
pub fn to_hex_string(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    Some(formatted(vm, info, HEX))
}

/// The marshalled parameters of a `(FLjava.lang.String;)…` call: the `F`
/// payload word and the format parameter's word.
///
/// `marshal_args` writes the receiver/return word first (`out[0]`) and then
/// one word per declared parameter, so the first parameter `F` is `out[1]`
/// and the second parameter `Ljava.lang.String;` is `out[2]` — the two words
/// the exe reads with `fld dword [esp+0x18]` and `mov edx, [esp+0x14]`.
///
/// A container the descriptor cannot satisfy leaves the rest of the port's
/// array at zero (the exe reads its own uninitialized frame words): the value
/// becomes `0.0f` and the format word `0`, the null String.
fn float_parameters(info: &CallInfo<'_>) -> (u32, u32) {
    let mut out = [0u32; 3];
    let _ = marshal_args(info, &mut out);
    (out[1], out[2])
}

/// `sprintf(buf, fmt, (double)f32)`: the exe's formatter over the widened
/// float, with the format coming from the script.
///
/// `word` is the marshalled `F` payload (`fld dword` reads exactly those
/// bits); the widening to `f64` is exact, so the formatter sees the same
/// double the exe's `fstp qword` produced.
///
/// The format resolves through [`string_bytes`]. A word with no bytes — the
/// null String or one the seam did not mint — is read as an empty format.
/// The exe's formatter tests its format pointer for null and returns without
/// writing the buffer (`0x553d78`/`0x553d87` both jump to the epilogue), so
/// what it interns there is uninitialized stack; the probe of a null format
/// word interned the empty string, and the port reproduces that memory-safe
/// reading. A format the single `%f` cell cannot satisfy is the port's typed
/// [`FormatError`](crate::format::FormatError) instead of the exe's read of
/// its own frame; the port renders the empty string there rather than
/// fabricate argument words.
fn float_text(vm: &VmHost, word: u32, format_word: u32) -> Vec<u8> {
    let value = f64::from(f32::from_bits(word));
    let format = string_bytes(vm, format_word).unwrap_or_default();
    sprintf(&format, &[FormatArg::Double(value)]).unwrap_or_default()
}

/// `java.lang.Float.toString(FLjava.lang.String;)Ljava.lang.String;`
///
/// `sprintf` the script-supplied format with `(double)f32`, then intern the
/// characters and return the `String` object word.
///
/// exe: LAB_00481dd0
pub fn float_to_string(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (word, format_word) = float_parameters(info);
    let text = float_text(vm, word, format_word);
    Some(intern_string(vm, &text))
}

/// Install the `java.lang.Integer`/`java.lang.Float` string ports into a
/// dispatch table.
pub fn install(table: &mut NativeTable) {
    /// `java.lang.Float.toString`
    const FLOAT_TO_STRING: u32 = 0x0048_1dd0;
    table.set_vm(FLOAT_TO_STRING, float_to_string);

    /// `java.lang.Integer.toString`
    const TO_STRING: u32 = 0x0048_1e30;
    table.set_vm(TO_STRING, to_string);

    /// `java.lang.Integer.toHexString`
    const TO_HEX_STRING: u32 = 0x0048_1e80;
    table.set_vm(TO_HEX_STRING, to_hex_string);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam::{self, DispatchError};
    use slr_vm::host::CallInfo;
    use slr_vm::values::{TypeTag, Value};

    /// The `(I)Ljava.lang.String;` container the VM builds for
    /// `Integer.toString`: parameter first, receiver/return slot last.
    fn args(word: u32) -> [Value; 2] {
        [Value::int(word as i32), Value::int(0x1234)]
    }

    fn call_info<'a>(args: &'a [Value], name: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: "java.lang.Integer",
            name,
            sig: "(I)Ljava.lang.String;",
        }
    }

    /// The bytes as text, for assertion messages.
    fn bytes_text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    /// The bytes interned under the returned word (the port's observable:
    /// the interner holds them, the object word carries the length).
    fn assert_interned(vm: &VmHost, word: u32, expected: &[u8]) {
        let obj = vm
            .objects
            .get((word - 0x1000) as usize)
            .expect("the returned word is an object word");
        assert_eq!(obj.class_name(&vm.loader), "java.lang.String");
        assert_eq!(vm.string_bytes(word), Some(expected));
        assert!(
            vm.strings.lookup(expected).is_some(),
            "the bytes {expected:?} are interned"
        );
    }

    /// Run one row and return its VM and the returned `String` object word.
    fn render(row: fn(&mut VmHost, &CallInfo<'_>) -> NativeResult, word: u32) -> (VmHost, u32) {
        let mut vm = crate::test_strings::vm();
        let args = args(word);
        let info = call_info(&args, "toString");
        let ret = row(&mut vm, &info).expect("a String-returning native");
        (vm, ret)
    }

    /// `Integer.toString` renders `"%d"` through the ported formatter: plain
    /// decimals, and the exe's signed-`IDIV` quirk for `0x8000_0000`.
    #[test]
    fn to_string_renders_the_exes_decimal() {
        for (word, expected) in [
            (0u32, &b"0"[..]),
            (42, &b"42"[..]),
            ((-1i32) as u32, &b"-1"[..]),
            (0x7fff_ffff, &b"2147483647"[..]),
            (0x8000_0000, &b"-./,),(-*,("[..]),
        ] {
            let (vm, ret) = render(to_string, word);
            assert_interned(&vm, ret, expected);
        }
    }

    /// `Integer.toHexString` renders `"0x%08x"`: unsigned, zero-padded to
    /// eight nibbles, lowercase.
    #[test]
    fn to_hex_string_renders_padded_hex() {
        for (word, expected) in [
            (0u32, &b"0x00000000"[..]),
            (1, &b"0x00000001"[..]),
            (0xcafe, &b"0x0000cafe"[..]),
            (0xdead_beef, &b"0xdeadbeef"[..]),
            ((-1i32) as u32, &b"0xffffffff"[..]),
            (0x8000_0000, &b"0x80000000"[..]),
        ] {
            let (vm, ret) = render(to_hex_string, word);
            assert_interned(&vm, ret, expected);
        }
    }

    /// The parameter word is the marshalled payload, read without inspecting
    /// the cell's tag (the exe's marshaller copies the word verbatim), and
    /// the receiver slot is never the parameter.
    #[test]
    fn the_parameter_is_the_marshalled_payload_word() {
        let mut vm = crate::test_strings::vm();
        let args = [Value::int(7), Value::int(0x4000_0000u32 as i32)];
        let info = call_info(&args, "toString");
        let ret = to_string(&mut vm, &info).expect("String");
        assert_interned(&vm, ret, b"7");
    }

    /// A container with no parameter leaves the port's word at zero, the same
    /// "reads 0 there" rule as `Math.sqrt`.
    #[test]
    fn a_missing_parameter_reads_zero() {
        let mut vm = crate::test_strings::vm();
        let info = call_info(&[], "toString");
        let ret = to_string(&mut vm, &info).expect("String");
        assert_interned(&vm, ret, b"0");
    }

    /// The `(FLjava.lang.String;)` container the VM builds for
    /// `Float.toString`: the parameters in reverse declaration order (the
    /// marshaller walks down from `count-2`), receiver/return slot last.
    fn float_args(format: u32, bits: u32) -> [Value; 3] {
        [
            Value::reference_named(format, TypeTag::STRING),
            Value::int(bits as i32),
            Value::int(0),
        ]
    }

    fn float_call_info<'a>(args: &'a [Value]) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: "java.lang.Float",
            name: "toString",
            sig: "(FLjava.lang.String;)Ljava.lang.String;",
        }
    }

    /// Run `Float.toString` with an interned `format` and return the VM plus
    /// the bytes it interned as the result.
    fn float_render(format: &[u8], bits: u32) -> (VmHost, Vec<u8>) {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let format_word = intern_string(&mut vm, format);
        let args = float_args(format_word, bits);
        let info = float_call_info(&args);
        let ret = float_to_string(&mut vm, &info).expect("a String-returning native");
        let obj = vm
            .objects
            .get((ret - 0x1000) as usize)
            .expect("the returned word is an object word");
        assert_eq!(obj.class_name(&vm.loader), "java.lang.String");
        let bytes = string_bytes(&vm, ret).expect("the result word has its bytes");
        assert_eq!(vm.string_bytes(ret), Some(bytes.as_slice()));
        seam::reset();
        (vm, bytes)
    }

    /// The rows registered here, and the addresses around them that are not.
    #[test]
    fn install_registers_the_three_rows() {
        let mut table = NativeTable::new();
        install(&mut table);
        assert_eq!(table.ported(), vec![0x0048_1dd0, 0x0048_1e30, 0x0048_1e80]);
        for address in [0x0048_1dd0, 0x0048_1e30, 0x0048_1e80] {
            assert!(table.get_vm(address).is_some(), "{address:08x}");
            assert!(table.get(address).is_none(), "{address:08x} is a VM port");
        }
        let mut host = crate::seam::LangHost::new();
        assert_eq!(
            table.dispatch(&mut host, 0x0048_1dcf, &[]),
            Err(DispatchError::NoImplementation {
                impl_addr: 0x0048_1dcf
            })
        );
        assert_eq!(
            table.dispatch(&mut host, 0x0048_1e31, &[]),
            Err(DispatchError::NoImplementation {
                impl_addr: 0x0048_1e31
            })
        );
    }

    /// `Float.toString` renders the script-supplied format through the
    /// ported `sprintf` with `(double)f32`. Every expected byte string below
    /// was probed from `0x481dd0` (Unicorn, `fpu_cw = 0x007f`, the native's
    /// own `sprintf` left real).
    #[test]
    fn float_to_string_renders_the_format_argument() {
        for (format, value, expected) in [
            (&b"%f"[..], 3.25f32, &b"3.25"[..]),
            (b"%f", 1.0 / 3.0, b"0.33333334"),
            (b"%f", 1234567.0, b"1234567.0"),
            (b"%f", 0.1, b"0.1"),
            (b"%.3f", 3.25, b"3.250"),
            (b"%.3f", 1.0 / 3.0, b"0.333"),
            (b"%.1f", 3.25, b"3.2"),
            (b"%.0f", 3.25, b"3"),
            // The fraction buffer is eight digits; a larger precision clamps.
            (b"%.10f", 3.25, b"3.25000000"),
            (b"%.8f", 1e-7, b"0.00000010"),
            (b"%f", 1e-7, b"0.0000001"),
            (b"[%f]", -2.5, b"[-2.5]"),
            // No conversion at all: the double is simply not consumed.
            (b"", 3.25, b""),
            (b"hello", 3.25, b"hello"),
            // `%g` is not in the exe's jump table: nothing is written and no
            // argument is read.
            (b"%g", 3.25, b""),
        ] {
            let (_vm, text) = float_render(format, value.to_bits());
            assert_eq!(text, expected, "format {:?} of {value}", bytes_text(format));
        }
    }

    /// The `F` payload is widened to a double before the formatter sees it:
    /// the value is `(double)f32`, so `1.1f` renders the f32's exact double
    /// `1.10000002384185791` — `1.10000002` at eight fraction digits — while a
    /// genuine f64 `1.1` renders `1.10000000`. The control below pins that
    /// distinction, so a port that read the parameter as an f64 (or as the
    /// low half of an f64) fails this test.
    #[test]
    fn the_f32_parameter_is_widened_to_a_double() {
        for (value, expected) in [
            (1.1f32, &b"1.10000002"[..]),
            (2.2, b"2.20000004"),
            (123.456, b"123.45600128"),
            (9.99, b"9.98999976"),
            (0.3, b"0.30000002"),
        ] {
            let (_vm, text) = float_render(b"%.8f", value.to_bits());
            assert_eq!(text, expected, "{value}");
        }
        let (_vm, text) = float_render(b"%f", 0.3f32.to_bits());
        assert_eq!(text, b"0.30000002");
        let (_vm, text) = float_render(b"%.3f", 0.3f32.to_bits());
        assert_eq!(text, b"0.300");

        // The control: the same format over a true f64 `1.1`. The oracle's
        // f32 row above and this one must differ.
        assert_eq!(
            sprintf(b"%.8f", &[FormatArg::Double(1.1)]).expect("renders"),
            b"1.10000000"
        );
    }

    /// `(double)f32` is exact, so every f32 value keeps its exact double,
    /// including the ends of the finite range and the zero sign. Oracle rows:
    /// the exe's `__ftol` arm renders infinity and out-of-range magnitudes
    /// through the indefinite `0x8000_0000_0000_0000` (low word 0), NaN as
    /// negative zero, and `-0.0`/subnormals as `0.0`.
    #[test]
    fn special_values_follow_the_exes_ftol_arm() {
        for (bits, format, expected) in [
            (0x7f80_0000u32, &b"%f"[..], &b"0.00000001"[..]),
            (0x7f80_0000, b"%.3f", b"0.000"),
            (0xff80_0000, b"%f", b"-0.00000001"),
            (0x7fc0_0000, b"%f", b"-0.0"),
            (0x7fc0_0000, b"%.3f", b"-0.000"),
            (0x0000_0001, b"%f", b"0.0"),
            (0x8000_0000, b"%f", b"0.0"),
            (0x7f7f_ffff, b"%f", b"0.00000001"),
        ] {
            let (_vm, text) = float_render(format, bits);
            assert_eq!(
                text,
                expected,
                "{bits:08x} through {:?}",
                bytes_text(format)
            );
        }
    }

    /// The format is the second parameter, the number the first: a different
    /// format word changes the rendering of the same value, and the format
    /// word is never used as the number.
    #[test]
    fn the_format_is_the_second_parameter() {
        let (_vm, text) = float_render(b"%.2f", 2.5f32.to_bits());
        assert_eq!(text, b"2.50");
        let (_vm, text) = float_render(b"<%.0f>", 2.5f32.to_bits());
        assert_eq!(text, b"<2>");
        // `%d` would print the format's own address if the two parameters
        // were swapped; the widening test pins the same distinction.
        let (_vm, text) = float_render(b"%f", 7.0f32.to_bits());
        assert_eq!(text, b"7.0");
    }

    /// A format word with no bytes — the null String, or a word the seam did
    /// not mint — reads as an empty format. The exe's formatter returns
    /// without writing its buffer for a null format pointer, so what it
    /// interns is uninitialized stack (empty in the probe); the port renders
    /// the empty string there.
    #[test]
    fn a_null_or_unminted_format_reads_empty() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        for format_word in [0u32, 0x2000_0000, 0x4000_0000] {
            let args = float_args(format_word, 3.25f32.to_bits());
            let info = float_call_info(&args);
            let ret = float_to_string(&mut vm, &info).expect("String");
            assert_eq!(
                string_bytes(&vm, ret).as_deref(),
                Some(&b""[..]),
                "{format_word:08x}"
            );
        }
        seam::reset();
    }

    /// The exe's `sprintf` reads a second conversion's operand out of its own
    /// stack frame; the port fails that format closed. `"%d"` (the widened
    /// double's low word in the exe) therefore renders empty instead of
    /// fabricating an argument word, and a second `%f` — the exe's probe read
    /// a zero word there, an uninitialized frame slot, not a script value —
    /// renders empty too.
    #[test]
    fn an_incompatible_conversion_renders_empty() {
        let (_vm, text) = float_render(b"%d", 3.25f32.to_bits());
        assert!(text.is_empty(), "the exe prints the double's low word here");
        let (_vm, text) = float_render(b"%s", 3.25f32.to_bits());
        assert!(
            text.is_empty(),
            "the exe dereferences the double's low word"
        );
        let (_vm, text) = float_render(b"%.1f|%.0f", 3.25f32.to_bits());
        assert!(
            text.is_empty(),
            "the exe reads its own frame for the second f"
        );
        // The failure drops the partial rendering; nothing is returned that
        // the exe would not have produced from a script value.
        let (_vm, text) = float_render(b"x=%d", 3.25f32.to_bits());
        assert!(text.is_empty());
    }

    /// A container with no parameters leaves both words at zero: `0.0f`
    /// through the null format, i.e. the empty string.
    #[test]
    fn missing_parameters_read_zero() {
        seam::reset();
        let mut vm = crate::test_strings::vm();
        let info = float_call_info(&[]);
        let ret = float_to_string(&mut vm, &info).expect("String");
        assert_eq!(string_bytes(&vm, ret).as_deref(), Some(&b""[..]));
        seam::reset();
    }

    /// End to end through the seam and the VM registry: `bridge` resolves a
    /// row's `record+8` and returns the interned `String` word, whose bytes
    /// are the exe's rendering under that row's format.
    #[test]
    fn the_vm_bridge_reaches_the_ports() {
        seam::reset();
        let mut table = NativeTable::new();
        install(&mut table);
        let mut vm = crate::test_strings::vm();
        seam::install(&mut vm, &table);
        let mut bridged: Vec<u32> = vm.native_impls.keys().copied().collect();
        bridged.sort_unstable();
        assert_eq!(bridged, vec![0x0048_1dd0, 0x0048_1e30, 0x0048_1e80]);

        assert_eq!(
            vm.natives
                .resolve("java.lang.Integer", "toString", "(I)Ljava.lang.String;")
                .expect("registered"),
            0x0048_1e30
        );
        assert_eq!(
            vm.natives
                .resolve("java.lang.Integer", "toHexString", "(I)Ljava.lang.String;")
                .expect("registered"),
            0x0048_1e80
        );

        let dec = args(1234);
        let ret = seam::bridge(&mut vm, &call_info(&dec, "toString")).expect("String");
        assert_interned(&vm, ret, b"1234");

        let hex = args(0x00c0_ffee);
        let ret = seam::bridge(&mut vm, &call_info(&hex, "toHexString")).expect("String");
        assert_interned(&vm, ret, b"0x00c0ffee");

        assert_eq!(
            vm.natives
                .resolve(
                    "java.lang.Float",
                    "toString",
                    "(FLjava.lang.String;)Ljava.lang.String;"
                )
                .expect("registered"),
            0x0048_1dd0
        );
        // The format is itself a script `String`, minted by the seam's intern
        // path, and the result is a second interned `String`.
        let format = intern_string(&mut vm, b"%.2f");
        let float = float_args(format, 2.5f32.to_bits());
        let ret = seam::bridge(&mut vm, &float_call_info(&float)).expect("String");
        assert_eq!(string_bytes(&vm, ret).as_deref(), Some(&b"2.50"[..]));
        seam::with_lang_host(|h| assert_eq!(h.last_error, None));
        seam::reset();
    }
}

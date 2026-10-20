//! `java.lang.String` natives (M1-06 rows M10/M11..M16).
//!
//! Six natives that all start the same way: marshal their `CallInfo`
//! container through `FUN_0045d910`, then resolve the receiver's char buffer
//! with `FUN_0042ab50(obj, [0x62e008])` — the exe's `String`-field descriptor.
//! The port resolves the actual inherited Native.ptr cell through the VM's
//! interned buffers ([`crate::seam::string_bytes`]). A missing buffer follows
//! each native's null-buffer branch.
//!
//! | class | name | impl | body |
//! |---|---|---|---|
//! | `java.lang.String` | `intValue` | `FUN_00481ed0` | `sscanf(buf, "%d", &local)` → `(I)local` |
//! | `java.lang.String` | `floatValue` | `FUN_00481f10` | `sscanf(buf, "%f", &local)` → `(F)local` |
//! | `java.lang.String` | `length` | `LAB_00481fe0` | `strlen(buf)` — the bytes, never the object field |
//! | `java.lang.String` | `append` | `LAB_00481f50` | `strcpy(buf, 0x100)` then a one-byte/one-char tail |
//! | `java.lang.String` | `finalize` | `LAB_00486190` | two `FUN_0041ecd0`/`FUN_0041e830` calls on the loader |
//! | `java.lang.String` | `token` | `LAB_00481cf0` | copy, `strtok` with the charset, intern |
//!
//! ```text
//! 481ed0  push ecx
//! 481ed1  mov  ecx, [esp+8]      ; the native's CallInfo
//! 481ed5  lea  eax, [esp+8]      ; out[0] (no receiver slot is read back)
//! 481ed9  push eax
//! 481eda  push ecx
//! 481edb  call 0x45d910          ; marshal (): the char-pointer word
//! 481ee0  mov  edx, [0x62e008]   ; the String field descriptor
//! 481ee6  mov  eax, [esp+0x10]   ; out[0]
//! 481eea  push edx
//! 481eeb  push eax
//! 481eec  call 0x42ab50          ; get_string_field -> the char buffer
//! 481ef1  add  esp, 0x10
//! 481ef4  lea  ecx, [esp]        ; the scanned word
//! 481ef8  push ecx
//! 481ef9  push 0x6133a4          ; "%d"
//! 481efe  push eax               ; the buffer
//! 481eff  call 0x551240          ; sscanf
//! 481f04  mov  eax, [esp+0xc]    ; the scanned word
//! 481f08  add  esp, 0x10
//! 481f0b  ret
//! ```
//!
//! `floatValue` is the same body with `"%f"` (`0x6133a8`) and
//! `fld dword [esp+0xc]` before the return: the `F` result is the scanned
//! **f32**, loaded as such (no widening), which the port returns as its bit
//! pattern.
//!
//! `length` (`0x481fe0`) marshals `()`, resolves the buffer, then runs
//! `repne scasb` over it — a plain `strlen` of the char buffer. The port
//! returns the byte length of the buffer named by the object's ptr cell.
//!
//! `append` (`0x481f50`) marshals `(I)`, copies the receiver's buffer into a
//! 0x108-byte frame buffer with the CRT bounded copy (`0x551120` →
//! `FUN_005548c0`, which writes at most `0x100` bytes **including** the NUL),
//! then appends its `I` parameter:
//!
//! ```text
//! 481f9e  mov  eax, [esp+8]      ; the parameter word
//! 481fa2  not  ecx / dec ecx     ; ecx = strlen(copy)
//! 481fa5  cmp  eax, 8
//! 481fa8  jne  481fb5
//! 481faa  test ecx, ecx
//! 481fac  jle  481fc3            ; word 8 with an empty receiver: nothing
//! 481fae  mov  byte [esp+ecx+0xf], 0   ; else truncate the last char
//! 481fb5  cmp  eax, 0x20
//! 481fb8  jl   481fc3            ; words below 0x20 are not appended
//! 481fba  mov  byte [esp+ecx+0x10], al ; append the low byte
//! 481fbe  mov  byte [esp+ecx+0x11], 0  ; NUL
//! ```
//!
//! So `append(8)` (backspace) deletes the receiver's last character, and any
//! word `>= 0x20` appends its low byte. The result is interned
//! (`FUN_004174a0`) and returned, exactly like the `toString` family.
//!
//! `finalize` (`0x486190`) marshals and resolves the buffer, then calls
//! `FUN_0041ecd0(jvm->[0x68], buf, 0)` and `FUN_0041e830(jvm->[0x68], buf)`:
//! the interner's release path. It returns no word. The port releases the
//! bytes from [`slr_vm::strings::Strings`] and returns `None`; a null buffer
//! (no object) does nothing, matching the `test esi, esi / je` guard.
//!
//! `token` (`0x481cf0`) marshals `(ILjava.lang.String;)`, resolves two
//! buffers (the receiver's and the charset's), copies the receiver into a
//! 0x100-byte frame buffer (heap when longer), then walks the copy with the
//! CRT `strtok` (`0x551200` → `FUN_00554c70`, the same bit-set separator
//! scan) `index` times: each call skips separators, cuts the token in place
//! and advances the saved cursor. After the last successful call it interns
//! the token, frees a heap copy and returns the `String` word; when a call
//! returns null (no more tokens) it interns whatever the cursor points at —
//! the empty string when the scan ran to the NUL.

use slr_vm::host::{marshal_args, CallInfo, VmHost};

use crate::scan::{sscanf, ScanArg};
use crate::seam::{intern_string, string_bytes, NativeResult, NativeTable};

/// The format `intValue` scans (`0x6133a4`).
const DECIMAL: &[u8] = b"%d";

/// The format `floatValue` scans (`0x6133a8`).
const FLOAT: &[u8] = b"%f";

/// The marshalled word of a `()`/`(I)`/`(ILjava.lang.String;)` native.
///
/// `FUN_0045d910(info, out, count)` writes the receiver/return word first and
/// the parameters after it, so `out[0]` is the receiver object word and
/// `out[1..]` the declared arguments. A container the descriptor cannot
/// satisfy leaves the words the port's array started with (0); the exe reads
/// its own frame instead, which the port cannot state truthfully.
fn marshalled<const N: usize>(info: &CallInfo<'_>) -> [u32; N] {
    let mut out = [0u32; N];
    match marshal_args(info, &mut out) {
        Ok(_) | Err(_) => out,
    }
}

/// The char buffer behind a `java.lang.String` object word, or `None` for the
/// exe's null pointer (a word the seam never minted).
fn text(vm: &VmHost, word: u32) -> Option<Vec<u8>> {
    string_bytes(vm, word)
}

/// The scanned `%d` word of a buffer, through the ported `sscanf`.
///
/// The exe scans into a frame word it never initialises: a buffer that does
/// not match leaves that word at whatever the caller's stack held. The port
/// starts it at 0 and returns the word the scanner left, which is the same
/// value for every buffer whose text converts.
fn scan_int(buf: &[u8], fmt: &[u8]) -> i32 {
    let mut word = 0u32;
    let _ = sscanf(buf, fmt, &mut [ScanArg::Word(&mut word)]);
    word as i32
}

/// The scanned `%f` word of a buffer.
fn scan_float(buf: &[u8], fmt: &[u8]) -> f32 {
    let mut value = 0f32;
    let _ = sscanf(buf, fmt, &mut [ScanArg::Float(&mut value)]);
    value
}

/// `java.lang.String.intValue()I`.
///
/// exe: FUN_00481ed0
///
/// ```text
/// 481eff  call 0x551240          ; sscanf(buf, "%d", &local)
/// 481f04  mov  eax, [esp+0xc]    ; eax = the scanned word
/// ```
pub fn int_value(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let [receiver] = marshalled::<1>(info);
    let buf = text(vm, receiver).unwrap_or_default();
    Some(scan_int(&buf, DECIMAL) as u32)
}

/// `java.lang.String.floatValue()F`.
///
/// exe: FUN_00481f10
///
/// ```text
/// 481f3f  call 0x551240          ; sscanf(buf, "%f", &local)
/// 481f44  fld  dword [esp+0xc]   ; the scanned f32
/// ```
pub fn float_value(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let [receiver] = marshalled::<1>(info);
    let buf = text(vm, receiver).unwrap_or_default();
    Some(scan_float(&buf, FLOAT).to_bits())
}

/// `java.lang.String.length()I`.
///
/// exe: LAB_00481fe0
///
/// ```text
/// 48200b  repne scasb al, es:[edi]   ; strlen(buf)
/// 48200f  dec  ecx
/// 482010  mov  eax, ecx
/// ```
pub fn length(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let [receiver] = marshalled::<1>(info);
    let len = text(vm, receiver).map_or(0, |b| b.len());
    Some(len as u32)
}

/// `java.lang.String.append(I)Ljava.lang.String;`.
///
/// exe: LAB_00481f50
///
/// The bounded copy keeps the first `0x100 - 1` bytes; `8` (backspace)
/// truncates the last character, any word `>= 0x20` appends its low byte, and
/// everything else leaves the copy alone. The result is interned.
pub fn append(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let [receiver, word] = marshalled::<2>(info);
    let src = text(vm, receiver).unwrap_or_default();
    // `FUN_005548c0(buf, src, 0x100)`: copies while `i < n - 1`, NUL after.
    let mut buf: Vec<u8> = src.iter().take(0x100 - 1).copied().collect();
    let word = word as i32;
    if word == 8 {
        // `test ecx, ecx / jle`: an empty copy keeps nothing to truncate.
        if !buf.is_empty() {
            buf.pop();
        }
    } else if word >= 0x20 {
        buf.push((word & 0xff) as u8);
    }
    Some(intern_string(vm, &buf))
}

/// `java.lang.String.finalize()V`.
///
/// exe: LAB_00486190
///
/// ```text
/// 4861a1  mov  edx, [0x62e008]   ; the String field descriptor
/// 4861a7  mov  edx, [esp+0x14]   ; the object word
/// 4861ad  call 0x42ab50          ; the char buffer
/// 4861b7  test esi, esi
/// 4861b9  je   4861d3            ; a null buffer does nothing
/// 4861bb  mov  eax, [edi]
/// 4861bd  mov  ecx, [eax+0x68]   ; the interner
/// 4861c3  call 0x41ecd0          ; release(jvm->[0x68], buf, 0)
/// 4861cd  call 0x41e830          ; release(jvm->[0x68], buf)
/// ```
pub fn finalize(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let [receiver] = marshalled::<1>(info);
    if let Some(buf) = text(vm, receiver) {
        vm.strings.clear_object(&buf);
        vm.strings.release(&buf);
    }
    None
}

/// `java.lang.String.token(ILjava.lang.String;)Ljava.lang.String;`.
///
/// exe: LAB_00481cf0
///
/// The receiver is copied, the copy is walked with the CRT `strtok` `index`
/// times using the charset's bytes as separators, and the token is interned.
/// The exe's loop calls `strtok` once, then keeps calling while the count it
/// decrements is non-zero, so index `n` reads the `n`-th token (0-based) and
/// stops early at the first token that has no successor.
///
/// Divergence (documented, intentional): the exe's `strtok` keeps its cursor
/// and separator set in CRT statics (`[0x6408d0]`, `[0x6408cc]`), so a null
/// charset reuses the previous call's set. The port keeps no CRT statics; a
/// null charset means "no separators", which is the first-call behaviour the
/// exe exhibits. Nothing in the stock scripts calls `token` twice with a null
/// charset, and the seam has no CRT static block to hang the state on.
pub fn token(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    // The VM container holds reversed parameters followed by the receiver.
    // FUN_0045d910 walks back through it in descriptor order: index, charset.
    let [receiver, index, charset] = marshalled::<3>(info);
    let src = text(vm, receiver).unwrap_or_default();
    let seps = text(vm, charset).unwrap_or_default();
    // `481d37 inc esi / 481d3a cmp esi,0x100 / jle 481d4f`: the frame buffer
    // holds `len + 1` bytes when that fits 0x100, otherwise `481d42` mallocs
    // `len + 1`. Either way the whole string is copied, and the buffer is
    // freed at `481db7` when it came from the heap.
    let copy: Vec<u8> = src.clone();
    let mut cursor = 0usize;

    // `FUN_00554c70`: skip separators, cut the token, advance past it.
    fn next_token(copy: &[u8], seps: &[u8], cursor: &mut usize) -> Option<Vec<u8>> {
        let mut at = *cursor;
        while at < copy.len() && seps.contains(&copy[at]) {
            at += 1;
        }
        if at >= copy.len() {
            return None;
        }
        let start = at;
        while at < copy.len() && !seps.contains(&copy[at]) {
            at += 1;
        }
        let token = copy[start..at].to_vec();
        if at < copy.len() {
            at += 1; // the exe writes the NUL over the separator
        }
        *cursor = at;
        Some(token)
    }

    // `481d5a call 0x551120` copies the receiver into the frame buffer (or a
    // heap block when it does not fit `0x100`), `481d6e call 0x551200` takes
    // the first token, and `481d76 test eax,eax / je 481da4` makes a null
    // first token the null result. `481d7e mov edx,ecx / dec ecx / test edx,edx
    // / je 481da4` then walks `index - 1` further calls, each of which ends the
    // walk (`481da0 test eax,eax / jne 481d7a`) as soon as it yields null. So
    // `index` is the 0-based token ordinal and past-the-end is the null
    // `String` word (`FUN_004174a0(0)` is 0).
    // Only the token the walk ends on is interned: `481da4` is the single
    // `call 0x4174a0`, and the `token-index-2` row records one intern (`c`).
    let mut last: Option<Vec<u8>> = None;
    for _ in 0..=index {
        match next_token(&copy, &seps, &mut cursor) {
            Some(tok) => last = Some(tok),
            None => return Some(0),
        }
    }
    Some(match last {
        Some(tok) => intern_string(vm, &tok),
        None => 0,
    })
}

/// Install the six rows in the host's native table.
///
/// All six need the world (they resolve the receiver's char buffer through
/// the seam and three of them intern a result), so they are [`VmNative`]
/// ports; the rows are the ones [`crate::seam::LANG_NATIVE_BINDINGS`] already
/// lists for `java.lang.String`.
pub fn install(table: &mut NativeTable) {
    /// `java.lang.String.intValue`
    const INT_VALUE: u32 = 0x0048_1ed0;
    table.set_vm(INT_VALUE, int_value);

    /// `java.lang.String.floatValue`
    const FLOAT_VALUE: u32 = 0x0048_1f10;
    table.set_vm(FLOAT_VALUE, float_value);

    /// `java.lang.String.length`
    const LENGTH: u32 = 0x0048_1fe0;
    table.set_vm(LENGTH, length);

    /// `java.lang.String.append`
    const APPEND: u32 = 0x0048_1f50;
    table.set_vm(APPEND, append);

    /// `java.lang.String.finalize`
    const FINALIZE: u32 = 0x0048_6190;
    table.set_vm(FINALIZE, finalize);

    /// `java.lang.String.token`
    const TOKEN_NATIVE: u32 = 0x0048_1cf0;
    table.set_vm(TOKEN_NATIVE, token);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seam;
    use slr_vm::host::VmHost;
    use slr_vm::values::Value;

    fn frame<'a>(args: &'a [Value], sig: &'a str, name: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: "java.lang.String",
            name,
            sig,
        }
    }

    /// Intern the text through the seam and use the returned word as a String
    /// argument (the same path a script's string literal takes).
    fn string_arg(vm: &mut VmHost, text: &[u8]) -> u32 {
        intern_string(vm, text)
    }

    fn bytes_of(vm: &VmHost, word: u32) -> Option<Vec<u8>> {
        string_bytes(vm, word)
    }

    #[test]
    fn int_value_scans_the_decimal_prefix() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"  -42abc");
        let args = [Value::reference(word)];
        assert_eq!(
            int_value(&mut vm, &frame(&args, "()I", "intValue")),
            Some((-42i32) as u32)
        );
    }

    #[test]
    fn int_value_on_a_null_string_reads_zero() {
        let mut vm = crate::test_strings::vm();
        let args = [Value::reference(0)];
        assert_eq!(
            int_value(&mut vm, &frame(&args, "()I", "intValue")),
            Some(0)
        );
    }

    #[test]
    fn float_value_scans_an_f32_not_a_double() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"3.25");
        let args = [Value::reference(word)];
        let got = float_value(&mut vm, &frame(&args, "()F", "floatValue")).expect("F result");
        assert_eq!(got, 3.25f32.to_bits());
        // A value that only differs in the f32 domain: 0.1f widened would be
        // 0x3fb99999a0000000; the scan stores the f32 0x3dcccccd.
        let word = string_arg(&mut vm, b"0.1");
        let args = [Value::reference(word)];
        let got = float_value(&mut vm, &frame(&args, "()F", "floatValue")).expect("F result");
        assert_eq!(got, 0.1f32.to_bits());
    }

    #[test]
    fn length_counts_the_char_buffer() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"hello");
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &frame(&args, "()I", "length")), Some(5));
        let args = [Value::reference(0)];
        assert_eq!(length(&mut vm, &frame(&args, "()I", "length")), Some(0));
    }

    #[test]
    fn append_concatenates_a_printable_byte() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"ab");
        let args = [Value::int(b'c' as i32), Value::reference(word)];
        let ret = append(&mut vm, &frame(&args, "(I)Ljava.lang.String;", "append"))
            .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b"abc"[..]));
    }

    #[test]
    fn append_backspace_truncates_and_never_appends_word_8() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"abc");
        let args = [Value::int(8), Value::reference(word)];
        let ret = append(&mut vm, &frame(&args, "(I)Ljava.lang.String;", "append"))
            .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b"ab"[..]));

        let word = string_arg(&mut vm, b"");
        let args = [Value::int(8), Value::reference(word)];
        let ret = append(&mut vm, &frame(&args, "(I)Ljava.lang.String;", "append"))
            .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b""[..]));
    }

    #[test]
    fn append_ignores_words_below_space() {
        let mut vm = crate::test_strings::vm();
        let word = string_arg(&mut vm, b"ab");
        let args = [Value::int(7), Value::reference(word)];
        let ret = append(&mut vm, &frame(&args, "(I)Ljava.lang.String;", "append"))
            .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b"ab"[..]));
    }

    #[test]
    fn append_copies_at_most_255_bytes() {
        let mut vm = crate::test_strings::vm();
        let long = vec![b'x'; 400];
        let word = string_arg(&mut vm, &long);
        let args = [Value::int(b'!' as i32), Value::reference(word)];
        let ret = append(&mut vm, &frame(&args, "(I)Ljava.lang.String;", "append"))
            .expect("String result");
        let bytes = bytes_of(&vm, ret).expect("bytes");
        assert_eq!(bytes.len(), 0x100); // 255 copied + the appended byte
        assert!(bytes[..255].iter().all(|&b| b == b'x'));
        assert_eq!(bytes[255], b'!');
    }

    #[test]
    fn finalize_releases_the_interned_bytes() {
        let mut vm = crate::test_strings::vm();
        let bytes = b"finalize-me";
        let word = string_arg(&mut vm, bytes);
        assert!(vm.strings.lookup(bytes).is_some());
        let args = [Value::reference(word)];
        assert_eq!(finalize(&mut vm, &frame(&args, "()V", "finalize")), None);
        assert!(
            vm.strings.lookup(bytes).is_none(),
            "the bytes were released"
        );
    }

    #[test]
    fn token_walks_the_separators() {
        let mut vm = crate::test_strings::vm();
        let text = string_arg(&mut vm, b"a,b c");
        let seps = string_arg(&mut vm, b", ");
        // Index 0: the first token. (The seam's argument order is the
        // reversed declared arguments followed by the receiver, as emitted by the VM.)
        let args = [
            Value::reference(seps),
            Value::int(0),
            Value::reference(text),
        ];
        let ret = token(
            &mut vm,
            &frame(&args, "(ILjava.lang.String;)Ljava.lang.String;", "token"),
        )
        .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b"a"[..]));
        // Index 2: the third token.
        let args = [
            Value::reference(seps),
            Value::int(2),
            Value::reference(text),
        ];
        let ret = token(
            &mut vm,
            &frame(&args, "(ILjava.lang.String;)Ljava.lang.String;", "token"),
        )
        .expect("String result");
        assert_eq!(bytes_of(&vm, ret).as_deref(), Some(&b"c"[..]));
    }

    #[test]
    fn token_past_the_end_is_the_null_string() {
        let mut vm = crate::test_strings::vm();
        let text = string_arg(&mut vm, b"a");
        let seps = string_arg(&mut vm, b",");
        let args = [
            Value::reference(seps),
            Value::int(5),
            Value::reference(text),
        ];
        let ret = token(
            &mut vm,
            &frame(&args, "(ILjava.lang.String;)Ljava.lang.String;", "token"),
        )
        .expect("result word");
        // `481da4` interns `eax`, which a failed `strtok` left at 0.
        assert_eq!(ret, 0);
    }

    #[test]
    fn install_registers_the_six_rows() {
        seam::reset();
        let mut table = NativeTable::new();
        install(&mut table);
        for addr in [
            0x0048_1ed0u32,
            0x0048_1f10,
            0x0048_1fe0,
            0x0048_1f50,
            0x0048_6190,
            0x0048_1cf0,
        ] {
            assert!(table.get_vm(addr).is_some(), "{addr:08x} installed");
            assert!(NativeTable::is_known(addr), "{addr:08x} known");
        }
        seam::reset();
    }
}

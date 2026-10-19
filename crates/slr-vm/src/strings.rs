//! Interned strings: the `FUN_0041e4d0` table behind `FUN_0041e610`.
//!
//! Machine model (all `[static]`, machine code read; oracle: Unicorn running
//! the exe's own interner, `re/oracle/vmclass_vectors.py`, local only):
//!
//! - The table object holds `0xa97` buckets of 12-byte entries
//!   (`hash % 0xa97`, `FUN_0041ef10`); each entry is `[0]` the hash,
//!   `[+4]` the entry record, `[+8]` the chain link. A record is
//!   `[+0]` vtable `0x5f0980`, `[+4]` the NUL-terminated bytes,
//!   `[+8]` the length, `[+0xc]` the reference count, `[+0x10]` spare.
//! - The hash is length-dependent: empty is 0, length < 4 is
//!   `first_byte + len`, else
//!   `(s[3] + s[len-4]) * 0x49 + s[len-2] + len` (all bytes signed).
//! - `FUN_0041e610` interns: on a hit it returns the record (refcount+1);
//!   on a miss it `malloc`s `len+1` bytes, copies, and inserts through
//!   `FUN_0041ef60`. `FUN_0041f210` advances the `+0x7f3c` cursor over
//!   same-hash candidates during lookup. `FUN_0041e830` removes
//!   (refcount-1, frees at 0).
//! - `FUN_0041e800` interns a NUL-terminated string (the registrar and the
//!   boot binder call it); `FUN_00423e10` binds the result into a cell.
//!
//! The port stores records in a hash map keyed by content; the exe's bucket
//! order is not observable through any ported interface, so iteration order
//! is unspecified.

use std::collections::HashMap;

/// One interned string record: `[+0]` vtable `0x5f0980`, `[+4]` the
/// NUL-terminated bytes, `[+8]` the length, `[+0xc]` the reference count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StrRecord {
    /// The string bytes (without the NUL terminator).
    pub bytes: Vec<u8>,
    /// Reference count (`[+0xc]`).
    pub refs: u32,
    /// Stable port word for the character buffer stored in String.ptr.
    pub word: u32,
    /// Weak String object association used by4174a0's lookup path.
    pub object: Option<u32>,
}

/// The intern table (`FUN_0041e4d0` object).
///
/// exe: FUN_0041e4d0
#[derive(Clone, Debug, Default)]
pub struct Strings {
    records: HashMap<Vec<u8>, StrRecord>,
    next_word: u32,
}

impl Strings {
    /// An empty table (matches the `FUN_0041e4d0` zeroed `0x7f40` block).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of live records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// `true` when no string is interned.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// The exe's string hash (`FUN_0041e610` header).
    ///
    /// exe: FUN_0041e610
    #[must_use]
    pub fn hash(s: &[u8]) -> u32 {
        if s.is_empty() {
            return 0;
        }
        let b = |i: usize| s[i] as i8 as i32;
        if s.len() < 4 {
            b(0) + s.len() as i32
        } else {
            (b(3) + b(s.len() - 4)) * 0x49 + b(s.len() - 2) + s.len() as i32
        }
        .cast_unsigned()
    }

    /// Intern `s` (`FUN_0041e610`): on a hit the refcount grows and the
    /// existing record is returned; on a miss a fresh record with
    /// refcount 1 is inserted (the exe `malloc`s `len+1` and copies).
    ///
    /// exe: FUN_0041e610
    pub fn intern(&mut self, s: &[u8]) -> &StrRecord {
        if !self.records.contains_key(s) {
            self.next_word = self
                .next_word
                .checked_add(1)
                .expect("String record words exhausted");
        }
        let word = 0x4000_0000 + self.next_word;
        let entry = self.records.entry(s.to_vec()).or_insert(StrRecord {
            bytes: s.to_vec(),
            refs: 0,
            word,
            object: None,
        });
        entry.refs += 1;
        entry
    }

    /// Associate an existing intern record with its allocated String object.
    pub fn bind_object(&mut self, bytes: &[u8], word: u32) {
        self.records
            .get_mut(bytes)
            .expect("String record must exist")
            .object = Some(word);
    }

    /// Resolve the authoritative String.ptr buffer word to its bytes.
    pub fn bytes_at(&self, word: u32) -> Option<&[u8]> {
        self.records
            .values()
            .find(|record| record.word == word)
            .map(|record| record.bytes.as_slice())
    }

    /// String finalization clears the weak object association before releasing
    /// its character-buffer reference, as41ecd0 precedes41e830.
    pub fn clear_object(&mut self, bytes: &[u8]) {
        if let Some(record) = self.records.get_mut(bytes) {
            record.object = None;
        }
    }

    /// Look up an interned string without touching the refcount
    /// (`FUN_0041ef10` hit path).
    ///
    /// exe: FUN_0041ef10
    #[must_use]
    pub fn lookup(&self, s: &[u8]) -> Option<&StrRecord> {
        self.records.get(s)
    }

    /// Release one reference (`FUN_0041e830`): refcount-1, dropping the
    /// record at 0 (the exe frees the bytes then). Returns the remaining
    /// refcount, or `None` when the string was not interned.
    ///
    /// exe: FUN_0041e830
    pub fn release(&mut self, s: &[u8]) -> Option<u32> {
        let refs = {
            let rec = self.records.get_mut(s)?;
            rec.refs = rec.refs.saturating_sub(1);
            rec.refs
        };
        if refs == 0 {
            self.records.remove(s);
            return Some(0);
        }
        Some(refs)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hash_matches_exe_rule() {
        // Worked from the disassembly (`movsx` bytes, `lea` 0x49 multiply):
        // empty 0; len<4 first+len; else (s[3]+s[len-4])*0x49+s[len-2]+len.
        assert_eq!(Strings::hash(b""), 0);
        assert_eq!(Strings::hash(b"I"), b'I' as u32 + 1);
        assert_eq!(Strings::hash(b"ab"), b'a' as u32 + 2);
        let s = b"java.lang";
        let want = ((s[3] as i8 as i32 + s[s.len() - 4] as i8 as i32) * 0x49
            + s[s.len() - 2] as i8 as i32
            + s.len() as i32) as u32;
        assert_eq!(Strings::hash(s), want);
        assert_eq!(Strings::hash(b"java.lang"), want);
    }

    #[test]
    fn intern_dedups_and_refcounts() {
        let mut t = Strings::new();
        assert!(t.is_empty());
        t.intern(b"I");
        t.intern(b"I");
        assert_eq!(t.len(), 1);
        let rec = t.lookup(b"I").expect("interned");
        assert_eq!(rec.bytes, b"I");
        assert_eq!(rec.refs, 2);
        assert_eq!(t.release(b"I"), Some(1));
        assert_eq!(t.release(b"I"), Some(0));
        assert!(t.lookup(b"I").is_none());
        assert_eq!(t.release(b"I"), None);
    }

    #[test]
    fn distinct_strings_coexist() {
        let mut t = Strings::new();
        t.intern(b"I");
        t.intern(b"java.lang");
        t.intern(b"Ljava.lang.String;");
        assert_eq!(t.len(), 3);
        assert_eq!(t.lookup(b"java.lang").expect("hit").refs, 1);
    }
}

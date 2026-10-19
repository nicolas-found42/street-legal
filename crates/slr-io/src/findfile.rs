//! `java.io.FindFile` natives (M1-09 slice I6) over a port-side directory search.
//!
//! Machine model (all `[static]`, machine code read; the find layer the natives
//! sit on is Windows-only — `FindFirstFileA`/`FindNextFileA`/`FindClose` through
//! the IAT slots `0x5e7098`/`0x5e709c`/`0x5e70a0` — so the port emulates the OS
//! search with `std::fs` instead of taking oracle rows):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`FIND_FIRST`] `0x487cb0` | `first(Ljava.lang.String;I)Ljava.lang.String;` | open the pattern's search, store `handle`/`flags`, filter, intern the first accepted name |
//! | [`FIND_NEXT`] `0x487da0` | `next()Ljava.lang.String;` | advance the stored search, filter, intern the next accepted name |
//! | [`FIND_CLOSE`] `0x487e70` | `close()V` | clear the `handle` field to `-1`, then free the slot |
//!
//! # `first(Ljava.lang.String;I)Ljava.lang.String;`
//!
//! ```text
//! 487cd1  marshal_args(container, ...)                ; out[0] this, out[1] name, out[2] flags
//! 487ce5  handle = 0x54cf50(name, &namebuf, &attr)    ; enumerate, sort, deliver entry 0
//! 487cf7  set_field(this, "handle", handle)           ; 0x42a170, unconditional (negative too)
//! 487d0b  set_field(this, "flags", flags)
//! 487d15  handle < 0 -> return 0
//! 487d22  name == "." (2-byte compare, 0x613b48) -> skip
//! 487d36  name == ".." (3-byte compare, 0x613b4c) -> skip
//! 487d4a  attr != 0 (a directory): accept iff flags != 1
//! 487d5c  attr == 0 (a file): accept iff flags <= 1 (signed)
//! 487d60  handle = 0x54d1c0(handle, &namebuf, &attr); loop
//! 487d75  return 0x4174a0(container[0], name)         ; the interned name
//! ```
//!
//! # `next()Ljava.lang.String;`
//!
//! ```text
//! 487db4  marshal_args(container, &out)            ; out[0] this
//! 487dca  handle = get_field(this, "handle")       ; 0x42a430
//! 487dd9  flags  = get_field(this, "flags")
//! 487de0  handle < 0 -> return 0
//! 487de8  r = 0x54d1c0(handle, &namebuf, &attr)    ; advance first, then filter
//! 487dfb  r != 0 -> return 0                       ; the search is exhausted
//!         the same "." / ".." skip and the same attr/flags filter as `first`
//! 487e3b  return 0x4174a0(container[0], name)
//! ```
//!
//! # `close()V`
//!
//! ```text
//! 487e7c  marshal_args(container, &out)            ; out[0] this
//! 487e8a  handle = get_field(this, "handle")
//! 487e9c  set_field(this, "handle", -1)            ; before the free, unconditionally
//! 487ea6  handle < 0 -> return
//! 487ea9  0x54d240(handle)                          ; free the slot
//! ```
//!
//! # The find layer (`0x54cf50` / `0x54d1c0` / `0x54d240`)
//!
//! The exe keeps 32 find slots in the parallel arrays `0x7663d8..0x766458`
//! (the slot blocks), `0x768660..` (each slot's record count) and
//! `0x7685e0..` (each slot's cursor). A slot's block starts at `0x32c8` bytes
//! = 50 records of `0x104` (`0x54d013`; `0x54d04b` grows it by another
//! `0x32c8` when it fills); a record is the entry name (`0x100` bytes) plus
//! its attr word at `+0x100`. Slot indices are `0..0x20` (`0` included),
//! `-1` means "no slot".
//!
//! `0x54cf50(pattern, &name, &attr)` converts the pattern (`0x554860` plus the
//! `'/'`→`'\\'` loop), opens the OS search on it, takes the *first* free slot
//! (`0x54cfe5`) and returns `-1` when the table is full or the block allocation
//! fails (`0x54f560`), the OS error (`0x559340` answers `0xfffffffe` for "no
//! match", so [`FIND_NOT_FOUND`] `-2`) when the search fails, else the slot
//! index after it has filled the block with the entries (`0x5593f0` per
//! further entry), closed the OS handle (`0x559480`), sorted the records with
//! `0x5d765d` (the CRT `qsort`) and delivered record 0: its name into `&name`,
//! its attr into `&attr`, cursor := 1.
//!
//! The sort comparator is `0x54d1a0(a, b)` = `0x554a10(a, b, 1)`, and `0x554a10`
//! is the **`strcmp`** this codebase shares (`0x554860` is its plain copy; the
//! third argument selects an ASCII `A-Z`→`a-z` fold): the entry records are
//! ordered by name, case-insensitively. It is *not* a wildcard matcher — the
//! handoff note called `0x554a10` the "wildcard matcher"; the wildcarding is
//! done by the OS inside `FindFirstFileA`, which the port reproduces with
//! [`pattern_match`] (see below).
//!
//! `0x54d1c0(handle, &name, &attr)` answers `-1` for a free slot or once the
//! cursor reaches the count, else copies the cursor's record and bumps the
//! cursor. `0x54d240(handle)` frees the slot's block and zeroes its count and
//! cursor (`-1` for an index outside `0..0x20`).
//!
//! The attr word is `(dwFileAttributes >> 4) & 1` (`0x55945b`): the
//! `FILE_ATTRIBUTE_DIRECTORY` bit, so `1` = directory and `0` = file, which is
//! what the two natives' filter compares ([`FindEntry::is_dir`],
//! [`accepts`]).
//!
//! # The wildcard mask
//!
//! The pattern goes to `FindFirstFileA` verbatim and the *OS* filters the
//! names; the port has no OS search, so [`pattern_match`] reproduces the
//! documented `FindFirstFileA` mask semantics against each `read_dir` entry:
//! `*` any run, `?` exactly one character, ASCII case-insensitive, and the
//! documented `*.*` match-all special case. The mask is the pattern's last
//! component ([`split_pattern`]); a pattern that ends at a separator means
//! "every entry", as it does for the OS. (No filesystem-side wildcard matcher
//! was found in the exe — `0x559340`/`0x5593f0` hand the converted pattern
//! straight to the imported `FindFirstFileA`/`FindNextFileA`, and the archive
//! branch `0x554630` asks the archive index `0x5542f0`; that is not a
//! binary-wide scan.)
//!
//! # Divergences (all "unmodelled or unspecified in the original" direction)
//!
//! - The port enumerates with `std::fs::read_dir` and orders the records with
//!   the same case-insensitive compare the exe's `qsort` uses, breaking ties by
//!   raw byte order: `qsort` is unstable, so the exe's order for two names that
//!   differ only by case is unspecified, while the port is deterministic.
//! - The exe's entry order is the OS's (`FindFirstFileA`), which the port
//!   replaces by that sort. `read_dir` order never escapes the port.
//! - Entry names are the OS long names; the exe's `cFileName` is the same, but
//!   its `cAlternateFileName` (8.3) and the DOS trailing-dot/space quirks of
//!   the mask are not modelled.
//! - A non-UTF-8 entry name is lossy-converted (the exe keeps raw bytes), and
//!   the sort's byte compares are unsigned (the exe's `strcmp` compares signed
//!   bytes), so names with bytes `>= 0x80` can order differently.
//! - The exe `open`s the OS search with `[0x7686e0] == 0` and its archive twin
//!   `0x554630`/`0x554740`/`0x554810` otherwise; the port always searches the
//!   filesystem (`crate::fileio`'s root), never a `stock/` archive.
//! - Paths resolve under the host's root with the file layer's policy
//!   ([`crate::path::dos_relative`]): no `..`, no drive-relative paths. The
//!   exe uses the process CWD and absolute paths.
//! - The class's fields `handle`/`flags` start at `-1`
//!   ([`FIELD_DEFAULT`]: the `FILD` record's fourth dword in
//!   `stock/system/Scripts/io/FindFile.class`, taken as the class default
//!   because `slr-vm` does not model field initialisers yet); the port has no
//!   script object model, so an unknown object word behaves like a fresh
//!   `FindFile` instance and the two fields live in [`FindHost`]
//!   (`crate::file::FileHost`'s `name`/handle seams are the precedent).
//! - The pattern arrives as a word registered through [`FindHost::set_string`]
//!   (the port has no VM-side `java.lang.String` object; the exe's marshalled
//!   value *is* the char buffer pointer). An unregistered word is the empty
//!   pattern, which no search matches — like the exe's own `FindFirstFileA`
//!   failure — where a null pointer in the exe is undefined.
//! - A slot whose OS search failed (no match) still stores the OS error in the
//!   exe; the port stores [`FIND_NOT_FOUND`]. Both are `< 0`, which is all
//!   `next` and `close` test.
//! - The two natives' `"."`/`".."` skip is defensive in the exe (`FindFirstFileA`
//!   never returns those two names); [`FindHost`]'s enumeration drops them
//!   (the OS behaviour) *and* the deliver loops keep the skip
//!   ([`is_dot_entry`]).

use std::array;
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashMap;
use std::path::PathBuf;

use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};

use crate::file::IoBinding;
use crate::path::{dos_path, dos_relative};

/// Exe address of `java.io.FindFile.first(Ljava.lang.String;I)Ljava.lang.String;`.
pub const FIND_FIRST: u32 = 0x0048_7cb0;
/// Exe address of `java.io.FindFile.next()Ljava.lang.String;`.
pub const FIND_NEXT: u32 = 0x0048_7da0;
/// Exe address of `java.io.FindFile.close()V`.
pub const FIND_CLOSE: u32 = 0x0048_7e70;

/// The class all three rows are registered under.
pub const FIND_FILE_CLASS: &str = "java.io.FindFile";

/// The `handle` field index-0 name in the class's constant pool (`0x613b38`).
pub const HANDLE_FIELD: &str = "handle";
/// The `flags` field name (`0x613b40`).
pub const FLAGS_FIELD: &str = "flags";

/// `FILES_DIRS`: files and directories. The class's first static `I` field
/// (`FILD` value 0); the filter's compares agree with that name/value pairing.
pub const FILES_DIRS: i32 = 0;
/// `FILES_ONLY` (the class's `FILD` value 1): files only
/// (`0x487d4a` rejects a directory when `flags == 1`).
pub const FILES_ONLY: i32 = 1;
/// `DIRS_ONLY` (the class's `FILD` value 2): directories only
/// (`0x487d5c` rejects a file when `flags > 1`).
pub const DIRS_ONLY: i32 = 2;

/// The class's default for both `handle` and `flags`: the `FILD` record's
/// fourth dword (`-1`). `slr-vm`'s class loader does not model field
/// initialisers yet, so the port takes that word as the class default; the
/// natives corroborate it (they read `handle` before any `first`, and `next`
/// requires it to be negative, `0x487de0`).
pub const FIELD_DEFAULT: i32 = -1;

/// Find slots (`0x7663d8..0x766458`); slot indices are `0..0x20`.
pub const FIND_SLOTS: usize = 0x20;
/// One entry record (`0x104`): a `0x100`-byte name plus the attr word.
pub const ENTRY_SIZE: usize = 0x104;
/// A slot block's allocation unit (`0x32c8`) = [`ENTRY_BATCH`] records.
pub const SLOT_BLOCK: usize = 0x32c8;
/// The records a slot block holds before it grows (`0x32`).
pub const ENTRY_BATCH: usize = 0x32;
/// `0x559340`'s "no match" return (`FindFirstFileA` failed), the exe's `-2`.
pub const FIND_NOT_FOUND: i32 = -2;
/// The 32-slot table's exhausted return (`0x54d007`), the exe's `-1`.
pub const FIND_NO_SLOT: i32 = -1;

const _: () = assert!(ENTRY_BATCH * ENTRY_SIZE == SLOT_BLOCK);

/// The three `java.io.FindFile` rows this slice ports, in the exe's
/// registration order (`re/out/native_api.tsv` 186-188).
pub const FIND_NATIVE_BINDINGS: [IoBinding; 3] = [
    IoBinding {
        class: FIND_FILE_CLASS,
        name: "first",
        sig: "(Ljava.lang.String;I)Ljava.lang.String;",
        impl_addr: FIND_FIRST,
    },
    IoBinding {
        class: FIND_FILE_CLASS,
        name: "next",
        sig: "()Ljava.lang.String;",
        impl_addr: FIND_NEXT,
    },
    IoBinding {
        class: FIND_FILE_CLASS,
        name: "close",
        sig: "()V",
        impl_addr: FIND_CLOSE,
    },
];

/// One entry the search delivered: the name and the attr bit
/// ([`FindEntry::is_dir`] is the exe's attr word `1`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindEntry {
    /// The entry name (`cFileName`, the long name).
    pub name: String,
    /// The `FILE_ATTRIBUTE_DIRECTORY` bit (`0x55945b`): `true` = the attr word 1.
    pub is_dir: bool,
}

impl FindEntry {
    /// The attr word the exe's filter reads: `1` for a directory, `0` for a file.
    #[must_use]
    pub fn attr(&self) -> u32 {
        u32::from(self.is_dir)
    }
}

/// One file-layer operation, in the order the exe issues it (the find-layer
/// twin of [`crate::file::IoEvent`]; no oracle row uses it yet, see the module
/// docs).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FindEvent {
    /// `0x559340`: the OS search was opened on the converted pattern.
    FindFirst {
        /// The DOS-converted pattern.
        pattern: String,
    },
    /// `0x559340`/`0x5593f0` delivered one entry (name plus the attr bit).
    Entry {
        /// The entry name.
        name: String,
        /// The attr bit: `true` = directory.
        dir: bool,
    },
    /// `0x54d013`/`0x54d04b`: the slot's record block was allocated.
    AllocSlot {
        /// The slot index.
        index: u32,
    },
    /// `0x559480`: the OS search was closed.
    FindClose,
    /// `0x5d765d`: the records were sorted with the case-insensitive
    /// `0x554a10` compare (`0x54d1a0`).
    Sort {
        /// How many records were sorted.
        entries: usize,
    },
    /// `0x54d240`: the slot's block was freed.
    FreeSlot {
        /// The slot index.
        index: u32,
    },
    /// `0x4174a0`: a delivered name was interned (the native's result word).
    Intern {
        /// The string word the intern returned.
        word: u32,
        /// The interned name.
        name: String,
    },
}

/// The class's two fields on one `java.io.FindFile` object.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FindObject {
    /// The `handle` field: the slot index, or negative when no search is open.
    pub handle: i32,
    /// The `flags` field: the `first` argument's word.
    pub flags: i32,
}

impl Default for FindObject {
    fn default() -> Self {
        FindObject {
            handle: FIELD_DEFAULT,
            flags: FIELD_DEFAULT,
        }
    }
}

/// One live search slot: the pattern it was opened on, its records and the
/// cursor `next` delivers from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FindSlot {
    /// The DOS-converted pattern the slot was opened on.
    pub pattern: String,
    /// The matching entries, ordered like the exe's `qsort` leaves its records.
    pub entries: Vec<FindEntry>,
    /// The index [`FindHost::advance`] delivers next (1 after `first` delivered
    /// record 0).
    pub cursor: usize,
}

/// The `FindFile` natives' runtime: the find slot table, the objects' two
/// fields and the string words the patterns and results travel as.
///
/// exe: the slot arrays `0x7663d8` / `0x7685e0` / `0x768660` plus the
/// per-thread JVM state
pub struct FindHost {
    root: PathBuf,
    slots: [Option<FindSlot>; FIND_SLOTS],
    objects: HashMap<u32, FindObject>,
    strings: HashMap<u32, Vec<u8>>,
    next_string: u32,
    events: Vec<FindEvent>,
}

impl FindHost {
    /// A host whose searches resolve under `root` (the file layer's policy).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> FindHost {
        FindHost {
            root: root.into(),
            slots: array::from_fn(|_| None),
            objects: HashMap::new(),
            strings: HashMap::new(),
            next_string: 0,
            events: Vec::new(),
        }
    }

    // ---- introspection (tests and the port's script side) -----------------

    /// The root every search resolves under.
    #[must_use]
    pub fn root(&self) -> &std::path::Path {
        &self.root
    }

    /// The operations recorded so far, in order.
    #[must_use]
    pub fn events(&self) -> &[FindEvent] {
        &self.events
    }

    /// Take the recorded operations (tests replay one native at a time).
    pub fn take_events(&mut self) -> Vec<FindEvent> {
        core::mem::take(&mut self.events)
    }

    /// One live slot.
    #[must_use]
    pub fn slot(&self, handle: i32) -> Option<&FindSlot> {
        let index = usize::try_from(handle).ok()?;
        self.slots.get(index).and_then(Option::as_ref)
    }

    /// How many slots are live.
    #[must_use]
    pub fn live_slots(&self) -> usize {
        self.slots.iter().filter(|slot| slot.is_some()).count()
    }

    /// One object's two fields (the class defaults `-1`/`-1` when the object
    /// was never touched).
    #[must_use]
    pub fn object(&self, object: u32) -> FindObject {
        self.objects.get(&object).copied().unwrap_or_default()
    }

    /// The object's `handle` field.
    #[must_use]
    pub fn handle(&self, object: u32) -> i32 {
        self.object(object).handle
    }

    /// The object's `flags` field.
    #[must_use]
    pub fn flags(&self, object: u32) -> i32 {
        self.object(object).flags
    }

    /// Set the object's `handle` field — the seam for the script-side
    /// constructor or a field write (`crate::file::FileHost::set_name`'s
    /// sibling).
    pub fn set_handle(&mut self, object: u32, handle: i32) {
        self.objects.entry(object).or_default().handle = handle;
    }

    /// Set the object's `flags` field.
    pub fn set_flags(&mut self, object: u32, flags: i32) {
        self.objects.entry(object).or_default().flags = flags;
    }

    /// Register the bytes behind a string word — the seam for a script-side
    /// `java.lang.String` argument (the exe's value *is* the char buffer
    /// pointer). [`FindHost::intern`] mints its words through the same table,
    /// so a delivered name can be used as the next pattern.
    pub fn set_string(&mut self, word: u32, bytes: &[u8]) {
        self.strings.insert(word, bytes.to_vec());
    }

    /// The bytes behind a string word; `None` for the null word 0 and for a
    /// word no path registered (the empty pattern, see the module docs).
    #[must_use]
    pub fn string(&self, word: u32) -> Option<&[u8]> {
        if word == 0 {
            return None;
        }
        self.strings.get(&word).map(Vec::as_slice)
    }

    // ---- the find layer ---------------------------------------------------

    /// `0x54cf50`'s port: open a search on `pattern` and return the slot index,
    /// [`FIND_NO_SLOT`] when every slot is taken, or [`FIND_NOT_FOUND`] when
    /// the pattern matched nothing. The slot's cursor is left at 1 (record 0 is
    /// the caller's to consume), and the slot is *not* allocated on a failed
    /// search, as in the exe.
    ///
    /// exe: 0x54cf50
    pub fn open_search(&mut self, pattern: &str) -> i32 {
        let dos = dos_path(pattern);
        self.events.push(FindEvent::FindFirst {
            pattern: dos.clone(),
        });
        if dos.is_empty() {
            // The exe hands the empty pattern straight to `FindFirstFileA`,
            // which fails; nothing is allocated.
            return FIND_NOT_FOUND;
        }
        let (dir, mask) = split_pattern(&dos);
        let entries = self.enumerate(dir, mask);
        if entries.is_empty() {
            return FIND_NOT_FOUND;
        }
        let Some(index) = self.slots.iter().position(Option::is_none) else {
            return FIND_NO_SLOT;
        };
        self.events.push(FindEvent::AllocSlot {
            index: index as u32,
        });
        for entry in &entries {
            self.events.push(FindEvent::Entry {
                name: entry.name.clone(),
                dir: entry.is_dir,
            });
        }
        self.events.push(FindEvent::FindClose);
        self.events.push(FindEvent::Sort {
            entries: entries.len(),
        });
        self.slots[index] = Some(FindSlot {
            pattern: dos,
            entries,
            cursor: 1,
        });
        index as i32
    }

    /// `0x54d1c0`'s port: deliver the entry the slot's cursor points at and bump
    /// the cursor; `None` for a free slot, an index outside `0..0x20`, or the
    /// end of the records (all `-1` in the exe).
    ///
    /// exe: 0x54d1c0
    pub fn advance(&mut self, handle: i32) -> Option<FindEntry> {
        let slot = self.slot_mut(handle)?;
        let entry = slot.entries.get(slot.cursor).cloned()?;
        slot.cursor += 1;
        Some(entry)
    }

    /// `0x54d240`'s port: free a slot; `false` for an index outside `0..0x20`
    /// (the exe's `-1`).
    ///
    /// exe: 0x54d240
    pub fn free(&mut self, handle: i32) -> bool {
        let Ok(index) = usize::try_from(handle) else {
            return false;
        };
        if index >= FIND_SLOTS {
            return false;
        }
        self.slots[index] = None;
        self.events.push(FindEvent::FreeSlot {
            index: index as u32,
        });
        true
    }

    // ---- the natives ------------------------------------------------------

    /// `FindFile.first(Ljava.lang.String;I)Ljava.lang.String;`: open the
    /// search, store both fields, and walk the records for the first one the
    /// filter accepts, returning its interned name (0 when the search failed or
    /// nothing was accepted).
    ///
    /// exe: LAB_00487cb0
    pub fn first(&mut self, object: u32, pattern_word: u32, flags: i32) -> u32 {
        let pattern = self
            .string(pattern_word)
            .map(|bytes| String::from_utf8_lossy(bytes).into_owned())
            .unwrap_or_default();
        let handle = self.open_search(&pattern);
        // `0x487cf7`/`0x487d0b`: both fields are stored whatever the search
        // answered, negative handles included.
        self.set_handle(object, handle);
        self.set_flags(object, flags);
        if handle < 0 {
            return 0;
        }
        // `0x54cf50` already delivered record 0; the loop filters it and only
        // then advances (`0x487d22`, `0x487d60`).
        let Some(entry) = self
            .slot(handle)
            .and_then(|slot| slot.entries.first())
            .cloned()
        else {
            return 0;
        };
        let mut delivered = entry;
        loop {
            if !is_dot_entry(&delivered.name) && accepts(&delivered, flags) {
                return self.intern(&delivered.name);
            }
            match self.advance(handle) {
                Some(entry) => delivered = entry,
                None => return 0,
            }
        }
    }

    /// `FindFile.next()Ljava.lang.String;`: advance the object's stored search
    /// and return the next accepted name's interned word (0 when the handle
    /// field is negative, the slot is free, or the records ran out).
    ///
    /// exe: LAB_00487da0
    pub fn next(&mut self, object: u32) -> u32 {
        let handle = self.handle(object);
        let flags = self.flags(object);
        if handle < 0 {
            return 0;
        }
        loop {
            let Some(entry) = self.advance(handle) else {
                return 0;
            };
            if !is_dot_entry(&entry.name) && accepts(&entry, flags) {
                return self.intern(&entry.name);
            }
        }
    }

    /// `FindFile.close()V`: clear the object's `handle` field to `-1` and, when
    /// it was a live slot, free it.
    ///
    /// exe: FUN_00487e70
    pub fn close(&mut self, object: u32) {
        let handle = self.handle(object);
        // `0x487e9c`: the field is cleared before the free, unconditionally.
        self.set_handle(object, FIELD_DEFAULT);
        if handle >= 0 {
            self.free(handle);
        }
    }

    // ---- internals --------------------------------------------------------

    fn slot_mut(&mut self, handle: i32) -> Option<&mut FindSlot> {
        let index = usize::try_from(handle).ok()?;
        self.slots.get_mut(index).and_then(Option::as_mut)
    }

    /// `0x4174a0`'s port: mint a word for a delivered name in the string table
    /// ([`crate::file::FileHost`]'s intern, in this host's table).
    fn intern(&mut self, name: &str) -> u32 {
        self.next_string = self.next_string.wrapping_add(1);
        let word = self.next_string;
        self.strings.insert(word, name.as_bytes().to_vec());
        self.events.push(FindEvent::Intern {
            word,
            name: name.to_owned(),
        });
        word
    }

    /// The OS search the exe delegates to `FindFirstFileA`: every entry of the
    /// resolved directory whose name the mask matches, ordered by the
    /// case-insensitive `0x554a10` compare (`0x54d1a0`'s sort).
    fn enumerate(&self, dos_dir: &str, mask: &str) -> Vec<FindEntry> {
        let Some(rel) = dos_relative(dos_dir) else {
            return Vec::new();
        };
        let Ok(read) = std::fs::read_dir(self.root.join(rel)) else {
            return Vec::new();
        };
        let mut entries: Vec<FindEntry> = Vec::new();
        for entry in read.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            // `FindFirstFileA` never answers the two dots; the natives skip
            // them again (`0x487d22`).
            if is_dot_entry(&name) || !pattern_match(mask, &name) {
                continue;
            }
            let is_dir = std::fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
            entries.push(FindEntry { name, is_dir });
        }
        entries.sort_by(|a, b| cmp_ci(&a.name, &b.name).then_with(|| a.name.cmp(&b.name)));
        entries
    }
}

/// The two dots the natives skip (`0x487d22`/`0x487d36`, the 2- and 3-byte
/// compares against `0x613b48`/`0x613b4c`).
#[must_use]
pub fn is_dot_entry(name: &str) -> bool {
    name == "." || name == ".."
}

/// The accept test both natives apply (`0x487d4a`, `0x487e27`): a directory
/// (`attr != 0`) is accepted unless `flags == `[`FILES_ONLY`], a file
/// (`attr == 0`) while `flags <= `[`FILES_ONLY`] (the exe's compare is signed).
///
/// With the class's constants that is: [`FILES_DIRS`] accepts both,
/// [`FILES_ONLY`] files, [`DIRS_ONLY`] directories.
#[must_use]
pub fn accepts(entry: &FindEntry, flags: i32) -> bool {
    if entry.is_dir {
        flags != FILES_ONLY
    } else {
        flags <= FILES_ONLY
    }
}

/// The pattern's directory part and file mask: the OS takes the mask from the
/// last separator, and a pattern that ends at one means "every entry".
#[must_use]
pub fn split_pattern(dos: &str) -> (&str, &str) {
    match dos.rfind('\\') {
        Some(i) if i + 1 < dos.len() => (&dos[..i], &dos[i + 1..]),
        Some(i) => (&dos[..i], "*"),
        None if dos.is_empty() => ("", "*"),
        None => ("", dos),
    }
}

/// `FindFirstFileA`'s mask match: `*` any run, `?` exactly one character,
/// ASCII case-insensitive, with the documented `*.*` match-all special case.
#[must_use]
pub fn pattern_match(mask: &str, name: &str) -> bool {
    // MSDN: `*.*` matches every name, extension or not.
    if mask == "*.*" {
        return true;
    }
    let mask = mask.as_bytes();
    let name = name.as_bytes();
    let (mut mi, mut ni) = (0, 0);
    // The `*` to fall back to when the run fails: (mask index, name index).
    let mut star: Option<(usize, usize)> = None;
    while ni < name.len() {
        match mask.get(mi) {
            Some(b'*') => {
                star = Some((mi, ni));
                mi += 1;
            }
            Some(&c) if matches_byte(c, name[ni]) => {
                mi += 1;
                ni += 1;
            }
            _ => match star {
                Some((star_mask, star_name)) => {
                    star = Some((star_mask, star_name + 1));
                    mi = star_mask + 1;
                    ni = star_name + 1;
                }
                None => return false,
            },
        }
    }
    // A trailing run of `*` matches the empty rest.
    while mask.get(mi) == Some(&b'*') {
        mi += 1;
    }
    mi == mask.len()
}

/// `0x554a10(a, b, 1)`: the ASCII-case-insensitive `strcmp` the record sort's
/// comparator (`0x54d1a0`) uses. The third argument of the exe's function
/// selects the `A-Z`→`a-z` fold; this port is the folded comparison only.
/// (Probed: `0x554a10` returns `eax` = `-1`/`0`/`1` for the name pairs the
/// unit tests pin, `ci=1` and `ci=0`.)
///
/// exe: 0x554a10
#[must_use]
pub fn cmp_ci(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let mut i = 0;
    loop {
        let (ca, cb) = (
            fold(a.get(i).copied().unwrap_or(0)),
            fold(b.get(i).copied().unwrap_or(0)),
        );
        if ca != cb || ca == 0 {
            return ca.cmp(&cb);
        }
        i += 1;
    }
}

/// `0x554a2f`/`0x554a39`: the `A-Z` → `a-z` fold the case-insensitive compare
/// applies to both bytes (`0x554a37` adds 0x20).
fn fold(byte: u8) -> u8 {
    if byte.is_ascii_uppercase() {
        byte + 0x20
    } else {
        byte
    }
}

/// The pattern byte vs name byte test of [`pattern_match`]: `?` matches
/// anything, everything else the folded byte.
fn matches_byte(pattern: u8, name: u8) -> bool {
    pattern == b'?' || fold(pattern) == fold(name)
}

thread_local! {
    /// The per-thread `FindFile` runtime (the exe's slot table is process-wide
    /// and its JVM state per-thread; see [`FindHost`]).
    static FINDS: RefCell<FindHost> = RefCell::new(FindHost::new("."));
}

/// Run `f` over this thread's [`FindHost`].
pub fn with_finds<R>(f: impl FnOnce(&mut FindHost) -> R) -> R {
    FINDS.with(|cell| f(&mut cell.borrow_mut()))
}

/// Reset this thread's [`FindHost`] to a fresh one rooted at `root`.
pub fn reset_finds(root: impl Into<PathBuf>) {
    FINDS.with(|cell| *cell.borrow_mut() = FindHost::new(root));
}

/// Register the three `java.io.FindFile` rows in the host's native registry
/// (`FUN_00416b00`). Idempotent: the exe's registrar overwrites a record.
pub fn register(vm: &mut VmHost) {
    for row in FIND_NATIVE_BINDINGS {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`NativeFn`] installed at every address.
///
/// The VM has already resolved the record and built the `CallInfo`; this
/// recovers `record+8` from the same registry (the `NativeFn` ABI carries no
/// address), marshals the container with the shared `FUN_0045d910` walk and
/// dispatches into the thread-local [`FindHost`]. `out[0]` is the receiver
/// (`container[count-1]`), `out[1]` the pattern word and `out[2]` the flags.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let impl_addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let mut out = [0u32; 3];
    // The exe reads the out slots whatever the descriptor walk wrote (stale
    // stack on a short container); the port reads 0 there.
    let written = marshal_args(info, &mut out).unwrap_or(0);
    match impl_addr {
        FIND_FIRST => {
            let pattern = if written > 1 { out[1] } else { 0 };
            let flags = (if written > 2 { out[2] } else { 0 }) as i32;
            Some(with_finds(|finds| finds.first(out[0], pattern, flags)))
        }
        FIND_NEXT => Some(with_finds(|finds| finds.next(out[0]))),
        FIND_CLOSE => {
            with_finds(|finds| finds.close(out[0]));
            None
        }
        _ => None,
    }
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in FIND_NATIVE_BINDINGS {
        vm.native_impls.insert(row.impl_addr, bridge as NativeFn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::host::VmHost;
    use slr_vm::values::Value;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};

    /// A scratch root, removed on drop.
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "slr-io-find-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, AtomicOrdering::Relaxed)
            ));
            let _ = fs::remove_dir_all(&p);
            fs::create_dir_all(&p).unwrap();
            Tree(p)
        }

        fn put(&self, rel: &str, data: &[u8]) {
            let p = self.0.join(rel);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            fs::write(p, data).unwrap();
        }

        fn mkdir(&self, rel: &str) {
            fs::create_dir_all(self.0.join(rel)).unwrap();
        }

        fn host(&self) -> FindHost {
            FindHost::new(&self.0)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const OBJ: u32 = 0x1000;
    /// The word the tests register the pattern under.
    const PAT: u32 = 7;

    /// The tree the ordering tests search: names chosen so byte order and
    /// case-insensitive order agree only after the fold.
    fn cars(tree: &Tree) {
        tree.mkdir("data/cars");
        tree.put("data/cars/m5.car", b"m");
        tree.put("data/cars/gti.car", b"g");
        tree.put("data/cars/Alpha.car", b"a");
        tree.put("data/cars/notes.txt", b"n");
        tree.mkdir("data/cars/sub");
        tree.put("data/cars/sub/inner.car", b"i");
    }

    fn entry(name: &str, is_dir: bool) -> FindEntry {
        FindEntry {
            name: name.to_owned(),
            is_dir,
        }
    }

    // ---- the mask ---------------------------------------------------------

    #[test]
    fn pattern_match_matches_wildcards() {
        assert!(pattern_match("*", "anything"));
        assert!(pattern_match("", ""));
        assert!(!pattern_match("", "x"));
        assert!(pattern_match("*.car", "gti.car"));
        assert!(!pattern_match("*.car", "gti.carp"));
        assert!(pattern_match("gti.car", "gti.car"));
        assert!(!pattern_match("gti.car", "gti.cars"));
        assert!(pattern_match("?ti.car", "gti.car"));
        assert!(!pattern_match("?ti.car", "gtti.car"));
        assert!(pattern_match("gti?car", "gti.car"), "? matches the dot too");
        assert!(pattern_match("g*i?c*", "gti.car"));
        assert!(pattern_match("a*b*c", "aXbYc"));
        assert!(!pattern_match("a*b*c", "aXbY"));
        assert!(pattern_match("*.*", "extensionless"));
        assert!(pattern_match("sub*", "sub"));
    }

    #[test]
    fn pattern_match_is_case_insensitive() {
        assert!(pattern_match("GTI.CAR", "gti.car"));
        assert!(pattern_match("gti.car", "GTI.CAR"));
        assert!(pattern_match("*.CAR", "Gti.car"));
        // only A-Z folds: a multi-byte name is compared byte-wise
        assert!(!pattern_match("é.car", "É.car"));
    }

    #[test]
    fn split_pattern_takes_the_last_component() {
        assert_eq!(split_pattern("data\\cars\\*.car"), ("data\\cars", "*.car"));
        assert_eq!(split_pattern("*.car"), ("", "*.car"));
        assert_eq!(split_pattern("data\\cars\\"), ("data\\cars", "*"));
        assert_eq!(split_pattern(""), ("", "*"));
    }

    #[test]
    fn cmp_ci_folds_case_and_orders_like_strcmp() {
        assert_eq!(cmp_ci("gti.car", "GTI.CAR"), Ordering::Equal);
        assert_eq!(cmp_ci("Alpha.car", "gti.car"), Ordering::Less);
        assert_eq!(cmp_ci("m5.car", "gti.car"), Ordering::Greater);
        assert_eq!(cmp_ci("sub", "sub/inner"), Ordering::Less);
        // Probed against the exe (`0x554a10(a, b, 1)` returns eax = -1/0/1,
        // this session): the empty/NUL tail and a byte pair outside the fold.
        assert_eq!(cmp_ci("", ""), Ordering::Equal);
        assert_eq!(cmp_ci("", "x"), Ordering::Less);
        assert_eq!(cmp_ci("[", "{"), Ordering::Less);
    }

    // ---- the filter -------------------------------------------------------

    #[test]
    fn the_two_dots_are_skipped() {
        assert!(is_dot_entry("."));
        assert!(is_dot_entry(".."));
        assert!(!is_dot_entry(".git"));
        assert!(!is_dot_entry("..."));
        assert!(!is_dot_entry("gti.car"));
    }

    #[test]
    fn the_flag_filter_selects_files_and_directories() {
        let file = entry("gti.car", false);
        let dir = entry("sub", true);
        // FILES_DIRS: both
        assert!(accepts(&file, FILES_DIRS));
        assert!(accepts(&dir, FILES_DIRS));
        // FILES_ONLY: the file only
        assert!(accepts(&file, FILES_ONLY));
        assert!(!accepts(&dir, FILES_ONLY));
        // DIRS_ONLY: the directory only
        assert!(!accepts(&file, DIRS_ONLY));
        assert!(accepts(&dir, DIRS_ONLY));
        // the class field default (-1) accepts both
        assert!(accepts(&file, FIELD_DEFAULT));
        assert!(accepts(&dir, FIELD_DEFAULT));
        // and the compare is signed, so anything above DIRS_ONLY is dirs only
        assert!(!accepts(&file, 3));
        assert!(accepts(&dir, 3));
    }

    // ---- the find layer ---------------------------------------------------

    #[test]
    fn open_search_enumerates_sorts_and_delivers_record_zero() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        let handle = host.open_search("data/cars/*");
        assert_eq!(handle, 0, "the first free slot is 0");
        let slot = host.slot(handle).unwrap();
        assert_eq!(slot.pattern, "data\\cars\\*");
        assert_eq!(slot.cursor, 1, "record 0 was delivered");
        assert_eq!(
            slot.entries,
            vec![
                entry("Alpha.car", false),
                entry("gti.car", false),
                entry("m5.car", false),
                entry("notes.txt", false),
                entry("sub", true),
            ]
        );
        assert_eq!(host.take_events(), {
            let mut want = vec![FindEvent::FindFirst {
                pattern: "data\\cars\\*".to_owned(),
            }];
            want.push(FindEvent::AllocSlot { index: 0 });
            for name in ["Alpha.car", "gti.car", "m5.car", "notes.txt"] {
                want.push(FindEvent::Entry {
                    name: name.to_owned(),
                    dir: false,
                });
            }
            want.push(FindEvent::Entry {
                name: "sub".to_owned(),
                dir: true,
            });
            want.push(FindEvent::FindClose);
            want.push(FindEvent::Sort { entries: 5 });
            want
        });
    }

    #[test]
    fn open_search_masks_the_last_component_and_carries_directories() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        assert_eq!(host.open_search("data/cars/*.car"), 0);
        let names: Vec<String> = host
            .slot(0)
            .unwrap()
            .entries
            .iter()
            .map(|e| e.name.clone())
            .collect();
        assert_eq!(names, vec!["Alpha.car", "gti.car", "m5.car"]);
        assert_eq!(host.open_search("data/cars/sub/*"), 1);
        assert_eq!(
            host.slot(1).unwrap().entries,
            vec![entry("inner.car", false)]
        );
    }

    #[test]
    fn open_search_reports_no_match_and_a_full_table() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        assert_eq!(host.open_search("data/cars/*.rpk"), FIND_NOT_FOUND);
        assert_eq!(host.open_search(""), FIND_NOT_FOUND);
        assert_eq!(host.open_search("missing/*"), FIND_NOT_FOUND);
        assert_eq!(host.live_slots(), 0, "no slot for a failed search");
        // the 32 slots: one live search each, the 33rd reports exhaustion
        for i in 0..FIND_SLOTS {
            assert_eq!(host.open_search("data/cars/*"), i as i32);
        }
        assert_eq!(host.open_search("data/cars/*"), FIND_NO_SLOT);
        assert_eq!(host.live_slots(), FIND_SLOTS);
    }

    #[test]
    fn advance_walks_the_records_and_free_releases_the_slot() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        let handle = host.open_search("data/cars/*.car");
        assert_eq!(handle, 0);
        assert_eq!(host.advance(handle), Some(entry("gti.car", false)));
        assert_eq!(host.advance(handle), Some(entry("m5.car", false)));
        assert_eq!(host.advance(handle), None, "the records ran out");
        assert_eq!(host.advance(handle), None, "and stay out");
        assert!(host.free(handle));
        assert_eq!(host.slot(handle), None);
        assert!(
            host.free(handle),
            "a valid index frees again (the exe's 0 return)"
        );
        assert!(!host.free(FIND_NO_SLOT));
        assert!(!host.free(FIND_SLOTS as i32));
    }

    // ---- the natives ------------------------------------------------------

    #[test]
    fn first_stores_the_fields_and_returns_the_first_accepted_name() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*");
        assert_eq!(host.first(OBJ, PAT, FILES_DIRS), 1);
        assert_eq!(host.handle(OBJ), 0);
        assert_eq!(host.flags(OBJ), FILES_DIRS);
        assert_eq!(host.string(1).unwrap(), b"Alpha.car");
        assert_eq!(
            host.take_events().last(),
            Some(&FindEvent::Intern {
                word: 1,
                name: "Alpha.car".to_owned()
            })
        );
    }

    #[test]
    fn first_filters_by_the_flags_it_stored() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*");

        // DIRS_ONLY skips every file and lands on the one directory
        assert_eq!(host.first(OBJ + 1, PAT, DIRS_ONLY), 1);
        assert_eq!(host.string(1).unwrap(), b"sub");
        assert_eq!(host.flags(OBJ + 1), DIRS_ONLY);

        // FILES_ONLY skips the directory when it comes up
        assert_eq!(host.first(OBJ + 2, PAT, FILES_ONLY), 2);
        // ... and a file-only mask never sees it
        host.set_string(PAT + 1, b"data/cars/sub/*");
        assert_eq!(host.first(OBJ + 3, PAT + 1, FILES_ONLY), 3);
        assert_eq!(host.string(3).unwrap(), b"inner.car");
    }

    #[test]
    fn first_without_a_match_stores_minus_two_and_returns_zero() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*.rpk");
        assert_eq!(host.first(OBJ, PAT, FILES_DIRS), 0);
        assert_eq!(host.handle(OBJ), FIND_NOT_FOUND);
        assert_eq!(host.flags(OBJ), FILES_DIRS);
        assert_eq!(host.live_slots(), 0);
        // an unregistered word is the empty pattern
        assert_eq!(host.first(OBJ + 1, 0x999, FILES_DIRS), 0);
        assert_eq!(host.handle(OBJ + 1), FIND_NOT_FOUND);
    }

    #[test]
    fn next_walks_the_remaining_entries_and_then_returns_zero() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*");
        assert_eq!(host.first(OBJ, PAT, FILES_DIRS), 1);
        assert_eq!(host.next(OBJ), 2);
        assert_eq!(host.next(OBJ), 3);
        assert_eq!(host.next(OBJ), 4);
        assert_eq!(host.next(OBJ), 5);
        assert_eq!(host.string(5).unwrap(), b"sub");
        assert_eq!(host.next(OBJ), 0, "the records ran out");
        assert_eq!(host.next(OBJ), 0, "and stay out");
        // the interned words are distinct per delivery
        for word in 1..=5 {
            assert!(host.string(word).is_some(), "word {word} was minted");
        }
    }

    #[test]
    fn next_filters_by_the_stored_flags() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*");
        // FILES_ONLY: the first file, then the next two, then the end (the
        // directory is skipped)
        assert_eq!(host.first(OBJ, PAT, FILES_ONLY), 1);
        assert_eq!(host.next(OBJ), 2);
        assert_eq!(host.next(OBJ), 3);
        assert_eq!(host.next(OBJ), 4);
        assert_eq!(host.string(4).unwrap(), b"notes.txt");
        assert_eq!(host.next(OBJ), 0, "the directory is not a file");

        // DIRS_ONLY from the start: only the directory is ever delivered
        assert_eq!(host.first(OBJ + 1, PAT, DIRS_ONLY), 5);
        assert_eq!(host.string(5).unwrap(), b"sub");
        assert_eq!(host.next(OBJ + 1), 0);
    }

    #[test]
    fn next_before_first_answers_zero() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        assert_eq!(host.handle(OBJ), FIELD_DEFAULT, "the class default");
        assert_eq!(host.next(OBJ), 0);
    }

    #[test]
    fn close_frees_the_slot_and_clears_the_handle_field() {
        let tree = Tree::new();
        cars(&tree);
        let mut host = tree.host();
        host.set_string(PAT, b"data/cars/*");
        assert_eq!(host.first(OBJ, PAT, FILES_DIRS), 1);
        host.close(OBJ);
        assert_eq!(host.handle(OBJ), FIELD_DEFAULT);
        assert_eq!(host.live_slots(), 0);
        assert_eq!(host.next(OBJ), 0, "the slot is gone");
        assert_eq!(
            host.take_events().last(),
            Some(&FindEvent::FreeSlot { index: 0 })
        );
        // closing twice is harmless, and so is closing a failed search
        host.close(OBJ);
        host.close(OBJ + 1);
        assert_eq!(host.live_slots(), 0);
        // a closed slot is reusable (the intern counter keeps counting)
        assert_eq!(host.first(OBJ + 2, PAT, FILES_DIRS), 2);
        assert_eq!(host.handle(OBJ + 2), 0);
        assert_eq!(host.string(2).unwrap(), b"Alpha.car");
    }

    // ---- registration -----------------------------------------------------

    /// Every row registers under `java.io.FindFile` and `install` wires the
    /// bridge at exactly those addresses.
    #[test]
    fn register_and_install_wire_the_rows() {
        reset_finds(".");
        let mut vm = VmHost::new();
        register(&mut vm);
        register(&mut vm);
        assert_eq!(vm.natives.len(), FIND_NATIVE_BINDINGS.len());
        for row in FIND_NATIVE_BINDINGS {
            assert_eq!(
                vm.natives.resolve(row.class, row.name, row.sig),
                Some(row.impl_addr),
                "{} {}{}",
                row.class,
                row.name,
                row.sig
            );
        }
        install(&mut vm);
        assert_eq!(vm.native_impls.len(), FIND_NATIVE_BINDINGS.len());
        for row in FIND_NATIVE_BINDINGS {
            assert!(
                vm.native_impls.contains_key(&row.impl_addr),
                "{} {}{} uninstalled",
                row.class,
                row.name,
                row.sig
            );
        }
    }

    /// The bridge marshals the container (`out[0]` the receiver, `out[1]` the
    /// pattern word, `out[2]` the flags) and dispatches into the thread-local
    /// host; `close` is a `V` native and pushes nothing.
    #[test]
    fn the_bridge_dispatches_first_next_and_close() {
        let tree = Tree::new();
        cars(&tree);
        reset_finds(&tree.0);
        let mut vm = VmHost::new();
        install(&mut vm);
        with_finds(|finds| finds.set_string(PAT, b"data/cars/*"));

        let args = [
            Value::int(FILES_DIRS),
            Value::reference(PAT),
            Value::reference(OBJ),
        ];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FIND_FILE_CLASS,
            name: "first",
            sig: "(Ljava.lang.String;I)Ljava.lang.String;",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));
        with_finds(|finds| {
            assert_eq!(finds.handle(OBJ), 0);
            assert_eq!(finds.string(1).unwrap(), b"Alpha.car");
        });

        let args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FIND_FILE_CLASS,
            name: "next",
            sig: "()Ljava.lang.String;",
        };
        assert_eq!(bridge(&mut vm, &info), Some(2));
        with_finds(|finds| assert_eq!(finds.string(2).unwrap(), b"gti.car"));

        let args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FIND_FILE_CLASS,
            name: "close",
            sig: "()V",
        };
        assert_eq!(bridge(&mut vm, &info), None, "V pushes nothing");
        with_finds(|finds| {
            assert_eq!(finds.handle(OBJ), FIELD_DEFAULT);
            assert_eq!(finds.live_slots(), 0);
        });
    }
}

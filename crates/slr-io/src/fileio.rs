//! The native file struct `S` and the 32-entry file-slot table below the `File` natives (M1-09).
//!
//! Everything here mirrors machine code of build 606. The `File` natives (slice I2) allocate `S`,
//! store it into the script object's `I` handle field and call these file-layer routines; the
//! natives themselves are not ported here.
//!
//! # Native struct `S` (allocated by `File.open`, `0x48549a`, size `0x14c`)
//!
//! | offset | word(s) | meaning |
//! |---|---|---|
//! | `+0x00` | [`NativeFile::slot`] | file-layer slot, 0 = closed |
//! | `+0x04` | [`NativeFile::open_word`] | word `File.open` stores here (write mode: 0); `close` never reads it |
//! | `+0x08` | [`NativeFile::resource_count`] | resource-table count word written by `close` |
//! | `+0x0c` | the resource list object | `open` builds it with `0x429350` then stores the `0x5f09a8` vtable (`0x4854b0`) |
//! | `+0x14` | the resource list's head word | [`NativeFile::resources`] (the first row's node) |
//! | `+0x28` | [`NativeFile::slot_count`] | slot-run count word written by `close` |
//! | `+0x2c` | the slot-list object | the same constructor and vtable as `+0x0c` |
//! | `+0x34` | the slot list's head word | [`NativeFile::slots`] (the first row's node) |
//! | `+0x48` | [`NativeFile::mode`] | `open` mode argument: 0 = read, 1 = write, other = no close |
//! | `+0x4c` | [`NativeFile::name`] | inline DOS file name, `0x100`-byte copy (`0x551120`) |
//!
//! Each of the two lists is a *linear* node chain: the head word holds the
//! first row's node (or the list object's end marker when it is empty), every
//! node's `+4` the next one, the last one the end marker, and the end marker's
//! own `+4` is the walk's terminator (0) — the shape every walk in
//! `File.write(ResourceRef)` (`0x485d39`/`0x485df0`) and `File.readResID`
//! (`0x485f06`/`0x485f52`) follows. The port keeps the rows themselves
//! ([`NativeFile::resources`] / [`NativeFile::slots`], `crate::resid`'s model)
//! and not the list objects, the nodes or their `0x18`/`0x20` links.
//!
//! The resource-table rows the exe *materialises* while scanning an opened
//! file (`0x485685` per entry, and the slot-run builder `0x485190`) are not
//! built here: see the divergences in `crate::file`'s module docs.
//!
//! # Slot table (`0x766460`, stride `0x10c`, entries `0..0x20`, slot 0 unused)
//!
//! | entry offset | field |
//! |---|---|
//! | `+0x000` | [`SlotEntry::handle`]: index into the CRT table `0x777558`, `-1` = none |
//! | `+0x004` | [`SlotEntry::name`]: `0x100`-byte DOS path (the `'/'`→`'\\'`-converted copy) |
//! | `+0x104` | [`SlotEntry::id`]: stamp from the counter `0x76645c` |
//! | `+0x108` | [`SlotEntry::in_use`] |
//! | `+0x109` | [`SlotEntry::mode`]: 0 = read, 1 = write |
//!
//! The CRT table next to it (`0x777558` handle, `0x77755c` size from `GetFileSize`, 64 entries,
//! `0` = free) is modelled by `CrtEntry`. Read slots keep their CRT handle after `close` (only
//! `in_use` is cleared), so reopening the same name rewinds that handle; write slots release the
//! handle on `close` and keep `in_use` set. A slot whose handle is `-1` is the only entry a
//! fresh open can claim, which is why a write slot becomes reusable and a read slot does not.
//!
//! # File layer
//!
//! | exe | port | behaviour |
//! |---|---|---|
//! | `0x54c880` | [`FileLayer::open_read`] | convert the path, then `0x54c8d0(name, 0)` |
//! | `0x54caf0` | [`FileLayer::open_write`] | convert, release stale same-name handles, create the parent directory, `0x54c8d0(name, 1)` |
//! | `0x54c8d0` | [`FileLayer::install`] | slot allocation: a closed same-name entry is reused on the spot (rewind when the mode matches, else reopen); otherwise the first free entry, and only with none the least-recently-opened closed entry is evicted |
//! | `0x54cca0` | [`FileLayer::close`] | write slot: release the handle; read slot: clear `in_use` |
//! | `0x54cec0` | [`FileLayer::read`] | `ReadFile`: bytes delivered, `0xffffffff` on error |
//! | `0x54cf00` | [`FileLayer::write`] | `WriteFile`: bytes written, `0xffffffff` on error |
//! | `0x54ce40` | [`FileLayer::seek`] | whence **0 = SET, 1 = END, 2 = CUR** (verified from `File.open`'s footer reads); returns the previous position |
//! | `0x54cdd0` | [`FileLayer::size`] | the size cached by `GetFileSize` at open time |
//!
//! Divergences from the exe (all in the "undefined or unsafe in the original" direction):
//!
//! - Paths are not limited to the `0x100`-byte stack buffers; the port keeps the whole string
//!   and truncates only the slot's stored name to 255 bytes plus NUL (the exe overflows).
//! - The path is resolved under a configurable root and no component may be `..`, a drive
//!   prefix is stripped and a leading separator is ignored, so the port never reads or writes
//!   outside the root (the exe uses the process CWD and absolute drive paths).
//! - A slot whose handle is `-1` (closed) returns `0xffffffff` from read/write/seek/size; the
//!   exe indexes the CRT table out of bounds (`0x777558 - 8`) instead.
//! - The exe's file lock (`0x5504c0` / `0x5504f0`) and CRT lock table are not modelled: the
//!   layer is single-threaded per instance.

use crate::path::{dos_path, dos_relative};
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Slot table entries, including the unused slot 0 (`0x766460..0x7685e0`).
pub const SLOT_COUNT: usize = 0x20;
/// First usable slot; the file layer rejects every slot `<= 0`.
pub const SLOT_FIRST: u32 = 1;
/// Last usable slot (`0x54cca0` rejects `>= 0x20`).
pub const SLOT_LAST: u32 = SLOT_COUNT as u32 - 1;
/// The slot's inline name buffer (`entry + 4 .. entry + 0x104`).
pub const NAME_CAP: usize = 0x100;
/// CRT table entries (`0x777558..0x777758`).
pub const CRT_COUNT: usize = 0x40;
/// The file layer's error value (`0xffffffff`).
pub const INVALID: u32 = u32::MAX;

/// `0x54ce40` whence: from the start.
pub const SEEK_SET: u32 = 0;
/// `0x54ce40` whence: from the cached end-of-file size.
pub const SEEK_END: u32 = 1;
/// `0x54ce40` whence: from the current position.
pub const SEEK_CUR: u32 = 2;

/// Size of the native struct `S` allocated by `File.open` (`0x54f560(0x14c)`).
pub const NATIVE_FILE_SIZE: usize = 0x14c;
/// Offset of the inline name inside `S`.
pub const NATIVE_FILE_NAME_OFFSET: usize = 0x4c;

const _: () = assert!(NATIVE_FILE_NAME_OFFSET + NAME_CAP == NATIVE_FILE_SIZE);

/// The native file struct `S` (`0x14c` bytes), see the module docs for the exe offsets.
#[derive(Clone, PartialEq, Eq)]
pub struct NativeFile {
    /// `+0x00`: file-layer slot, 0 = closed.
    pub slot: u32,
    /// `+0x04`: the word `File.open` stores (`0` in write mode, `0x485575`; read mode stores a
    /// value derived from the v2 footer scan, `0x48561e`). Slice I2 owns its computation; no
    /// ported file-layer routine reads it.
    pub open_word: u32,
    /// `+0x08`: resource-table count word.
    pub resource_count: u32,
    /// `+0x28`: slot-list count word.
    pub slot_count: u32,
    /// `+0x48`: the `open` mode argument (0 read, 1 write, other = no close).
    pub mode: u32,
    /// `+0x14` (the resource list's head word): the file's resource table, one
    /// row per resource *kind*, in insertion order. `File.write(ResourceRef)`
    /// appends to it and `File.readResID` reads it back through the row `id`
    /// (`crate::resid`); `File.close` writes it as the footer's resource
    /// entries. Entries the exe materialises while scanning an opened file
    /// (`0x485685`) are not built here (see the module docs).
    pub resources: Vec<crate::resid::ResourceRow>,
    /// `+0x34` (the slot run's head word): the file's script-side slot run,
    /// one row per unsaved `0xffff`-high `ResourceRef`, in insertion order.
    /// `File.write(ResourceRef)` appends to it and `File.readResID` reads it
    /// back; `File.close` writes it as the footer's slot run.
    pub slots: Vec<crate::resid::SlotRow>,
    name: [u8; NAME_CAP],
}

impl NativeFile {
    /// The struct `File.open` allocates and zero-initialises (`0x48549a..0x4854d5`): slot,
    /// resource count and slot count 0, name empty; the mode is stored before validation.
    pub fn new(mode: u32) -> NativeFile {
        NativeFile {
            slot: 0,
            open_word: 0,
            resource_count: 0,
            slot_count: 0,
            mode,
            resources: Vec::new(),
            slots: Vec::new(),
            name: [0; NAME_CAP],
        }
    }

    /// `+0x4c`: the inline name up to its NUL terminator.
    pub fn name_bytes(&self) -> &[u8] {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_CAP);
        &self.name[..end]
    }

    /// The inline name as UTF-8 (`""` if it is not valid UTF-8).
    pub fn name(&self) -> &str {
        std::str::from_utf8(self.name_bytes()).unwrap_or("")
    }

    /// `0x551120` `strncpy(name, src, 0x100)`: a byte copy that fills the buffer. The exe leaves
    /// the buffer unterminated for a 256-byte source; the port truncates to 255 bytes plus NUL.
    pub fn set_name(&mut self, name: &str) {
        self.name = [0; NAME_CAP];
        let n = name.len().min(NAME_CAP - 1);
        self.name[..n].copy_from_slice(&name.as_bytes()[..n]);
    }

    /// `[file+0] != 0`: a slot is open.
    pub fn is_open(&self) -> bool {
        self.slot != 0
    }

    /// Clears the slot word after closing (`0x4854e8`).
    pub fn clear_slot(&mut self) {
        self.slot = 0;
    }
}

impl core::fmt::Debug for NativeFile {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("NativeFile")
            .field("slot", &self.slot)
            .field("open_word", &self.open_word)
            .field("resource_count", &self.resource_count)
            .field("slot_count", &self.slot_count)
            .field("mode", &self.mode)
            .field("name", &self.name())
            .finish()
    }
}

/// One slot-table entry (`0x10c` bytes in the exe).
#[derive(Clone, PartialEq, Eq)]
pub struct SlotEntry {
    /// `+0x000`: CRT table index, `-1` when no handle is held.
    pub handle: i32,
    /// `+0x004`: the DOS name the slot was opened with (already `/`→`\` converted).
    pub name: [u8; NAME_CAP],
    /// `+0x104`: stamp from the counter `0x76645c`.
    pub id: u32,
    /// `+0x108`: set by `open`, cleared only by closing a read slot.
    pub in_use: bool,
    /// `+0x109`: 0 = read, 1 = write.
    pub mode: u8,
}

impl SlotEntry {
    fn free() -> SlotEntry {
        SlotEntry {
            handle: -1,
            name: [0; NAME_CAP],
            id: 0,
            in_use: false,
            mode: 0,
        }
    }

    fn name_bytes(&self) -> &[u8] {
        let end = self.name.iter().position(|&b| b == 0).unwrap_or(NAME_CAP);
        &self.name[..end]
    }
}

impl core::fmt::Debug for SlotEntry {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("SlotEntry")
            .field("handle", &self.handle)
            .field("name", &String::from_utf8_lossy(self.name_bytes()))
            .field("id", &self.id)
            .field("in_use", &self.in_use)
            .field("mode", &self.mode)
            .finish()
    }
}

/// One CRT table entry (`0x777558` handle plus the size `0x77755c` taken at open time).
#[derive(Debug)]
struct CrtEntry {
    file: File,
    size: u32,
}

/// The 32-entry file-slot table and the file layer below the `File` natives.
#[derive(Debug)]
pub struct FileLayer {
    root: PathBuf,
    slots: [SlotEntry; SLOT_COUNT],
    crt: [Option<CrtEntry>; CRT_COUNT],
    next_id: u32,
}

impl FileLayer {
    /// A table rooted at `root`; every DOS path resolves below it.
    pub fn new(root: impl Into<PathBuf>) -> FileLayer {
        FileLayer {
            root: root.into(),
            slots: core::array::from_fn(|_| SlotEntry::free()),
            crt: core::array::from_fn(|_| None),
            next_id: 0,
        }
    }

    // ---- introspection (tests and the `File` natives) -------------------------------------

    /// The entry for a usable slot (1..=31).
    pub fn entry(&self, slot: u32) -> Option<&SlotEntry> {
        slot_index(slot).map(|i| &self.slots[i])
    }

    /// The root every path is resolved under.
    pub fn root(&self) -> &Path {
        &self.root
    }

    // ---- file layer -------------------------------------------------------------------------

    /// `0x54c880`: convert `'/'` to `'\'` and open for reading; returns the 1-based slot or 0.
    ///
    /// exe: 0x54c880
    pub fn open_read(&mut self, path: &str) -> u32 {
        let name = slot_name(&dos_path(path));
        self.install(&name, 0)
    }

    /// `0x54caf0`: convert `'/'` to `'\'`, run the create helper on the path, release closed
    /// same-name handles, then open (truncating) for writing; returns the 1-based slot or 0.
    ///
    /// exe: 0x54caf0
    pub fn open_write(&mut self, path: &str) -> u32 {
        let name = slot_name(&dos_path(path));
        // `0x54cb3e..0x54cb8a`: a closed slot with this exact (case-sensitive) name loses its
        // CRT handle before anything else happens.
        for i in 1..SLOT_COUNT {
            if self.slots[i].handle >= 0 && !self.slots[i].in_use && self.slots[i].name == name {
                self.release_handle(i);
            }
        }
        // `0x558890(name, 0, 0)`, issued before the slot is allocated: it creates the missing
        // parent directories and `CreateFileA(CREATE_ALWAYS)`s the file, so the target is
        // truncated even when the table has no room and the open then fails.
        if let Some(host) = self.resolve(&name) {
            if let Some(parent) = host.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .open(&host);
        }
        self.install(&name, 1)
    }

    /// `0x54c8d0`: slot allocation and open in a single pass over slots 1..=31.
    ///
    /// Per entry:
    /// - no handle (`< 0`): the first such entry is latched as the free entry;
    /// - in use: skipped;
    /// - closed with the same name (`0x554a10` == 0, case-sensitive): reused on the spot — the
    ///   same mode rewinds the kept handle, a different mode releases it and reopens in place;
    /// - closed with another name: the entry with the smallest id (`+0x104`, i.e. the least
    ///   recently opened) is kept as the eviction candidate.
    ///
    /// After the pass the free entry wins; only when every entry holds a handle is the eviction
    /// candidate's handle released and the file opened there; otherwise the call returns 0.
    ///
    /// exe: 0x54c8d0
    fn install(&mut self, name: &[u8; NAME_CAP], mode: u8) -> u32 {
        let mut free: Option<usize> = None;
        let mut victim: Option<usize> = None;
        for i in 1..SLOT_COUNT {
            if self.slots[i].handle < 0 {
                free.get_or_insert(i);
                continue;
            }
            if self.slots[i].in_use {
                continue;
            }
            if self.slots[i].name == *name {
                if self.slots[i].mode == mode {
                    // `0x54c9d2`: rewind the kept handle; the name and mode are not rewritten.
                    self.rewind(i);
                    self.slots[i].id = self.bump_id();
                    self.slots[i].in_use = true;
                    return i as u32;
                }
                // `0x54ca12`: a different mode drops the handle and reopens on the same entry.
                self.release_handle(i);
                return self.open_into(i, name, mode);
            }
            let better = victim.is_none_or(|v| self.slots[i].id < self.slots[v].id);
            if better {
                victim = Some(i);
            }
        }
        match free {
            Some(i) => self.open_into(i, name, mode),
            None => match victim {
                Some(i) => {
                    self.release_handle(i);
                    self.open_into(i, name, mode)
                }
                None => 0,
            },
        }
    }

    /// `0x559150(handle, 0, 0, FILE_CURRENT)`'s first effect: seek the kept handle to 0.
    fn rewind(&mut self, i: usize) {
        let h = self.slots[i].handle as usize;
        if let Some(crt) = self.crt[h].as_mut() {
            let _ = crt.file.seek(SeekFrom::Start(0));
        }
    }

    /// The shared fresh-open tail (`0x54c99a..0x54cad3`): stores the handle, copies the name,
    /// stamps the id, and marks the entry in use with the requested mode.
    fn open_into(&mut self, i: usize, name: &[u8; NAME_CAP], mode: u8) -> u32 {
        match self.acquire(name, mode != 0) {
            Some(h) => {
                self.slots[i] = SlotEntry {
                    handle: h as i32,
                    name: *name,
                    id: self.bump_id(),
                    in_use: true,
                    mode,
                };
                i as u32
            }
            None => {
                self.slots[i].handle = -1;
                0
            }
        }
    }

    /// `0x54cca0`: close a slot. A write slot releases its CRT handle and keeps `in_use` (so the
    /// entry is free for the next open); a read slot only clears `in_use` and keeps the handle.
    /// Invalid slots and repeated closes do nothing.
    ///
    /// exe: 0x54cca0
    pub fn close(&mut self, slot: u32) {
        let Some(i) = slot_index(slot) else {
            return;
        };
        if self.slots[i].mode != 0 {
            self.release_handle(i);
        } else {
            self.slots[i].in_use = false;
        }
    }

    /// `0x54cec0`: `ReadFile`. Returns the bytes delivered, or [`INVALID`] for an invalid or
    /// closed slot.
    ///
    /// exe: 0x54cec0
    pub fn read(&mut self, slot: u32, buf: &mut [u8]) -> u32 {
        let Some(crt) = self.crt_entry_mut(slot) else {
            return INVALID;
        };
        match crt.file.read(buf) {
            Ok(n) => n as u32,
            Err(_) => INVALID,
        }
    }

    /// `0x54cf00`: `WriteFile`. Returns the bytes written, or [`INVALID`] for an invalid or
    /// closed slot.
    ///
    /// exe: 0x54cf00
    pub fn write(&mut self, slot: u32, data: &[u8]) -> u32 {
        let Some(crt) = self.crt_entry_mut(slot) else {
            return INVALID;
        };
        match crt.file.write(data) {
            Ok(n) => n as u32,
            Err(_) => INVALID,
        }
    }

    /// `0x54ce40`: seek to `base + offset`, where `base` is 0 (SET), the size cached at open
    /// (END) or the current position (CUR). Returns the previous position, or [`INVALID`] for an
    /// invalid slot, an unknown whence, or a failed query.
    ///
    /// exe: 0x54ce40
    pub fn seek(&mut self, slot: u32, offset: i32, whence: u32) -> u32 {
        let Some(crt) = self.crt_entry_mut(slot) else {
            return INVALID;
        };
        let base = match whence {
            SEEK_SET => 0,
            SEEK_END => crt.size,
            SEEK_CUR => match crt.file.stream_position() {
                Ok(p) => p as u32,
                Err(_) => return INVALID,
            },
            _ => return INVALID,
        };
        let new_pos = base.wrapping_add(offset as u32) as u64;
        // `0x559150` asks `SetFilePointer(handle, 0, 0, FILE_CURRENT)` first and returns that
        // value, then seeks to `offset` from the start.
        let previous = match crt.file.stream_position() {
            Ok(p) => p as u32,
            Err(_) => return INVALID,
        };
        let _ = crt.file.seek(SeekFrom::Start(new_pos));
        previous
    }

    /// `0x54cdd0`: the file size captured by `GetFileSize` when the slot was opened, or
    /// [`INVALID`] for an invalid or closed slot.
    ///
    /// exe: 0x54cdd0
    pub fn size(&self, slot: u32) -> u32 {
        self.crt_entry(slot).map(|crt| crt.size).unwrap_or(INVALID)
    }

    // ---- the CRT path rows (M1-09 I8) --------------------------------------------------------

    /// `File.exists(Ljava.lang.String;)I` — `0x486170`: convert `'/'` to
    /// `'\\'`, then `0x54c170(path)`, which answers 1 when the attribute
    /// probe (`0x5542d0`, i.e. the CRT's `_access`, run only when
    /// `[0x7686e0]` is set) or the open probe (`0x5586b0`: `CreateFileA`
    /// with `GENERIC_READ`, then `CloseHandle`) finds the file, else 0.
    ///
    /// The port answers with `Path::exists` under the root — the same two
    /// probes collapsed, since a path the port refuses ([`dos_relative`])
    /// can never exist inside the root.
    ///
    /// exe: LAB_00486170
    ///
    /// Also 0x54c170 (the wrapper) and 0x5586b0 (its open probe).
    pub fn exists(&self, path: &str) -> u32 {
        let name = slot_name(&dos_path(path));
        u32::from(self.probe(&name).is_some_and(|host| host.exists()))
    }

    /// `File.delete(Ljava.lang.String;)I` — `0x486150`: convert, then
    /// `0x54c7c0(path)`. That wrapper first closes any table entry still
    /// holding this exact name (releasing its CRT handle and marking the
    /// entry `-1`), then `0x558cf0(path)` deletes it. `0x558cf0` answers 0
    /// for a path that exists only as a directory (`GetFileAttributesA` and
    /// `GetFileAttributesExA` both succeed and `GetLastError` is not
    /// `ERROR_FILE_NOT_FOUND`/`ERROR_PATH_NOT_FOUND`), otherwise it issues
    /// `DeleteFileA` and returns 1 — *including* when the delete itself
    /// fails, because its return is a constant 1.
    ///
    /// The port keeps the close-then-delete order and the directory
    /// discrimination; it returns 1 whenever [`dos_relative`] accepted the
    /// path, which is what the exe's constant does.
    ///
    /// exe: LAB_00486150
    ///
    /// Also 0x54c7c0 (the wrapper) and 0x558cf0 (its delete).
    pub fn delete(&mut self, path: &str) -> u32 {
        let name = slot_name(&dos_path(path));
        let Some(host) = self.probe(&name) else {
            return 1;
        };
        // `0x54c813`-`0x54c854`: the table scan releases a matching entry
        // before the delete is issued.
        for i in 1..SLOT_COUNT {
            if self.slots[i].in_use || self.slots[i].handle < 0 {
                continue;
            }
            if self.slots[i].name == name {
                self.release_handle(i);
                self.slots[i].handle = -1;
            }
        }
        if !host.is_dir() {
            let _ = std::fs::remove_file(&host);
        }
        1
    }

    /// `File.copy(String;String)I`: native `0x4860f0`, layer `0x54c550`,
    /// helper `0x558a50`. Release the first closed destination handle, then
    /// copy with replacement. Source handles remain live. Return 0 on success
    /// and [`INVALID`] on failure. A missing parent triggers directory and
    /// destination creation before retrying the copy.
    ///
    /// exe: FUN_004860f0
    ///
    /// exe: FUN_0054c550
    /// exe: FUN_00558a50
    pub fn copy(&mut self, src: &str, dst: &str) -> u32 {
        let src_name = slot_name(&dos_path(src));
        let dst_name = slot_name(&dos_path(dst));
        if let Some(index) = (1..SLOT_COUNT).find(|&index| {
            let entry = &self.slots[index];
            entry.handle >= 0 && !entry.in_use && entry.name == dst_name
        }) {
            self.release_handle(index);
        }
        // Read handles share reads only (0x558d80); write handles share writes
        // only (0x558e40). A retained destination or source writer prevents the
        // corresponding CopyFileA open even when the host OS would allow it.
        if self.slots.iter().any(|entry| {
            entry.handle >= 0
                && (entry.name == dst_name || (entry.name == src_name && entry.mode == 1))
        }) {
            return INVALID;
        }
        let (Some(from), Some(to)) = (self.probe(&src_name), self.probe(&dst_name)) else {
            return INVALID;
        };
        // Host copy APIs may truncate the source when destination aliases it.
        // Windows refuses copying a file onto itself.
        if same_file(&from, &to) {
            return INVALID;
        }
        match std::fs::copy(&from, &to) {
            Ok(_) => 0,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && from.is_file() => {
                let Some(parent) = to.parent() else {
                    return INVALID;
                };
                if std::fs::create_dir_all(parent).is_err() || File::create(&to).is_err() {
                    return INVALID;
                }
                std::fs::copy(from, to).map_or(INVALID, |_| 0)
            }
            Err(_) => INVALID,
        }
    }

    /// `File.move(String;String;)I`: native `0x486120`, layer `0x54c650`,
    /// helper `0x558ba0`. Release retained closed handles for both paths,
    /// then move without replacing a destination. Return 0 on success and
    /// [`INVALID`] on failure, including identical names or sharing conflicts.
    ///
    /// exe: FUN_00486120
    ///
    /// The original's missing-parent retry creates an empty destination before
    /// calling MoveFileA again, so the retry fails because the destination now
    /// exists. Under ADR 0007 the port creates only the parent directories.
    /// `re/oracle/io_vectors.py` records the original failure and file effects.
    ///
    /// exe: FUN_0054c650
    /// exe: FUN_00558ba0
    pub fn move_file(&mut self, src: &str, dst: &str) -> u32 {
        let src_name = slot_name(&dos_path(src));
        let dst_name = slot_name(&dos_path(dst));
        // The real layer flushes source then destination and drops only the
        // first matching closed handle for each. A live/in-use read handle
        // lacks FILE_SHARE_DELETE (0x558d80), so Windows refuses the move.
        for name in [&src_name, &dst_name] {
            if let Some(index) = (1..SLOT_COUNT).find(|&index| {
                let entry = &self.slots[index];
                entry.handle >= 0 && !entry.in_use && entry.name == *name
            }) {
                self.release_handle(index);
            }
        }
        if self
            .slots
            .iter()
            .any(|entry| entry.handle >= 0 && (entry.name == src_name || entry.name == dst_name))
        {
            return INVALID;
        }
        let (Some(from), Some(to)) = (self.probe(&src_name), self.probe(&dst_name)) else {
            return INVALID;
        };
        if to.try_exists().unwrap_or(true) {
            return INVALID;
        }
        match rename_exclusive(&from, &to) {
            Ok(()) => 0,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound && from.exists() => {
                // ADR 0007 intentional divergence: the original creates an
                // empty destination before retrying, making MoveFileA fail.
                // Create only the parent directories, then retry exclusively.
                let Some(parent) = to.parent() else {
                    return INVALID;
                };
                if std::fs::create_dir_all(parent).is_err() {
                    return INVALID;
                }
                rename_exclusive(&from, &to).map_or(INVALID, |_| 0)
            }
            Err(_) => INVALID,
        }
    }

    // ---- internals --------------------------------------------------------------------------

    /// The resolved host path for a converted DOS name, when it is inside the
    /// root at all. `None` is the same refusal [`FileLayer::resolve`] makes,
    /// so the path rows never touch anything outside the root.
    fn probe(&self, name: &[u8; NAME_CAP]) -> Option<PathBuf> {
        self.resolve(name)
    }

    /// Resolves a converted DOS name under the root; `None` when it cannot name a file inside
    /// the root (empty, `..`, a stray `':'`, a non-relative prefix). The component walk itself
    /// is [`crate::path::dos_relative`], shared with `FindFile`'s directory resolution.
    fn resolve(&self, name: &[u8; NAME_CAP]) -> Option<PathBuf> {
        let end = name.iter().position(|&b| b == 0).unwrap_or(NAME_CAP);
        let rel = dos_relative(&String::from_utf8_lossy(&name[..end]))?;
        if rel.as_os_str().is_empty() {
            return None;
        }
        Some(self.root.join(rel))
    }

    /// Acquires a CRT entry for `name`; `None` when the file cannot be opened.
    fn acquire(&mut self, name: &[u8; NAME_CAP], write: bool) -> Option<usize> {
        let idx = self.crt.iter().position(|e| e.is_none())?;
        let host = self.resolve(name)?;
        let mut opts = OpenOptions::new();
        if write {
            opts.write(true).create(true).truncate(true);
        } else {
            opts.read(true);
        }
        let file = opts.open(&host).ok()?;
        let size = file.metadata().ok().map(|m| m.len() as u32).unwrap_or(0);
        self.crt[idx] = Some(CrtEntry { file, size });
        Some(idx)
    }

    /// `0x559090`: drop the slot's CRT handle and mark the entry as having none.
    fn release_handle(&mut self, i: usize) {
        if self.slots[i].handle >= 0 {
            self.crt[self.slots[i].handle as usize] = None;
        }
        self.slots[i].handle = -1;
    }

    fn crt_entry(&self, slot: u32) -> Option<&CrtEntry> {
        let i = slot_index(slot)?;
        let h = self.slots[i].handle;
        (h >= 0).then(|| self.crt.get(h as usize).and_then(Option::as_ref))?
    }

    fn crt_entry_mut(&mut self, slot: u32) -> Option<&mut CrtEntry> {
        let i = slot_index(slot)?;
        let h = self.slots[i].handle;
        (h >= 0).then(|| self.crt.get_mut(h as usize).and_then(Option::as_mut))?
    }

    fn bump_id(&mut self) -> u32 {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1);
        id
    }
}

fn same_file(from: &Path, to: &Path) -> bool {
    let (Ok(source), Ok(destination)) = (from.metadata(), to.metadata()) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        source.dev() == destination.dev() && source.ino() == destination.ino()
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        from.canonicalize().ok() == to.canonicalize().ok()
    }
}

/// macOS's atomic no-replace rename. The platform is fixed by ADR 0008.
#[cfg(target_os = "macos")]
fn rename_exclusive(from: &Path, to: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    unsafe extern "C" {
        fn renamex_np(
            from: *const std::ffi::c_char,
            to: *const std::ffi::c_char,
            flags: u32,
        ) -> i32;
    }
    let from = CString::new(from.as_os_str().as_bytes())?;
    let to = CString::new(to.as_os_str().as_bytes())?;
    // RENAME_EXCL = 0x4 in the macOS SDK's sys/stdio.h. Both C strings
    // remain alive for the call; renamex_np retains neither pointer.
    if unsafe { renamex_np(from.as_ptr(), to.as_ptr(), 0x4) } == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(not(target_os = "macos"))]
fn rename_exclusive(_from: &Path, _to: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "exact File.move requires macOS (ADR 0008)",
    ))
}

/// `1..=0x1f` as a table index.
fn slot_index(slot: u32) -> Option<usize> {
    (SLOT_FIRST..=SLOT_LAST)
        .contains(&slot)
        .then_some(slot as usize)
}

/// The `0x100`-byte name copy `0x554860` makes for the slot entry (truncated, NUL-padded).
fn slot_name(dos: &str) -> [u8; NAME_CAP] {
    let mut out = [0u8; NAME_CAP];
    let n = dos.len().min(NAME_CAP - 1);
    out[..n].copy_from_slice(&dos.as_bytes()[..n]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "slr-io-filelayer-test-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::Relaxed)
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
        fn layer(&self) -> FileLayer {
            FileLayer::new(&self.0)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn native_file_starts_closed_with_the_mode_stored() {
        let mut f = NativeFile::new(0);
        assert_eq!(f.slot, 0);
        assert_eq!(f.open_word, 0);
        assert_eq!(f.resource_count, 0);
        assert_eq!(f.slot_count, 0);
        assert_eq!(f.mode, 0);
        assert!(!f.is_open());
        assert_eq!(f.name(), "");
        f.slot = 7;
        assert!(f.is_open());
        f.clear_slot();
        assert!(!f.is_open());
        assert_eq!(NativeFile::new(1).mode, 1);
        assert_eq!(NativeFile::new(9).mode, 9);
    }

    #[test]
    fn native_file_name_copy_is_bytewise_and_terminated() {
        let mut f = NativeFile::new(0);
        f.set_name("data/cars/gti.car");
        assert_eq!(f.name(), "data/cars/gti.car");
        let long = "x".repeat(300);
        f.set_name(&long);
        assert_eq!(f.name_bytes().len(), NAME_CAP - 1);
        assert_eq!(f.name_bytes(), &[b'x'; NAME_CAP - 1]);
    }

    #[test]
    fn open_read_returns_slots_and_converts_the_path() {
        let t = Tree::new();
        t.put("sub/x.bin", b"abc");
        let mut l = t.layer();
        assert_eq!(l.open_read("sub/x.bin"), 1);
        assert_eq!(l.entry(1).unwrap().name_bytes(), b"sub\\x.bin");
        assert_eq!(l.entry(1).unwrap().mode, 0);
        assert!(l.entry(1).unwrap().in_use);
        assert_eq!(l.size(1), 3);
        // An in-use slot is never reused, not even for the same name: the open takes the next
        // free entry and the name comparison sees the converted spelling.
        let mut buf = [0u8; 2];
        assert_eq!(l.read(1, &mut buf), 2);
        assert_eq!(l.open_read("sub\\x.bin"), 2);
        assert_eq!(l.entry(2).unwrap().name_bytes(), b"sub\\x.bin");
        assert_eq!(l.read(2, &mut buf), 2, "a fresh open starts at 0");
        assert_eq!(&buf, b"ab");
        // Once closed, the name match reuses slot 1 and rewinds it.
        l.close(1);
        assert_eq!(l.open_read("sub/x.bin"), 1);
        assert_eq!(l.read(1, &mut buf), 2);
        assert_eq!(&buf, b"ab");
    }

    #[test]
    fn missing_file_and_escaping_paths_open_nothing() {
        let t = Tree::new();
        let mut l = t.layer();
        assert_eq!(l.open_read("nope.bin"), 0);
        assert_eq!(l.open_read("../../etc/passwd"), 0);
        assert_eq!(l.open_read(""), 0);
        assert_eq!(l.open_read("a:b"), 0);
        assert_eq!(l.open_write("../escape.bin"), 0);
    }

    #[test]
    fn read_write_roundtrip_through_a_write_slot() {
        let t = Tree::new();
        let mut l = t.layer();
        let s = l.open_write("d/f.bin");
        assert_eq!(s, 1);
        assert_eq!(l.write(s, b"abcd"), 4);
        assert_eq!(l.seek(s, 0, SEEK_SET), 4);
        // A write handle is opened `GENERIC_WRITE` only, so reads on it fail.
        assert_eq!(l.read(s, &mut [0u8; 4]), INVALID);
        l.close(s);
        let r = l.open_read("d/f.bin");
        assert_eq!(r, s);
        let mut buf = [0u8; 4];
        assert_eq!(l.read(r, &mut buf), 4);
        assert_eq!(&buf, b"abcd");
        assert_eq!(l.read(r, &mut buf), 0);
    }

    #[test]
    fn write_open_creates_parents_and_truncates() {
        let t = Tree::new();
        t.put("save/old.dat", b"0123456789");
        let mut l = t.layer();
        let s = l.open_write("save/new/der/file.dat");
        assert!(s >= 1);
        assert!(t.0.join("save/new/der").is_dir());
        assert_eq!(l.size(s), 0);
        assert_eq!(l.write(s, b"xy"), 2);
        // The size is the `GetFileSize` word captured at open, not the current length.
        assert_eq!(l.size(s), 0);
        l.close(s);
        assert_eq!(fs::read(t.0.join("save/new/der/file.dat")).unwrap(), b"xy");

        let s = l.open_write("save/old.dat");
        l.close(s);
        assert_eq!(fs::read(t.0.join("save/old.dat")).unwrap(), b"");
    }

    #[test]
    fn short_read_returns_the_bytes_delivered() {
        let t = Tree::new();
        t.put("f.bin", b"12345");
        let mut l = t.layer();
        let s = l.open_read("f.bin");
        let mut buf = [0u8; 8];
        assert_eq!(l.read(s, &mut buf), 5);
        assert_eq!(&buf[..5], b"12345");
        assert_eq!(l.read(s, &mut buf), 0);
    }

    #[test]
    fn invalid_slots_return_minus_one_and_close_is_a_noop() {
        let t = Tree::new();
        t.put("f.bin", b"1");
        let mut l = t.layer();
        let mut buf = [0u8; 1];
        for slot in [0u32, 32, 100, u32::MAX] {
            assert_eq!(l.read(slot, &mut buf), INVALID, "read {slot}");
            assert_eq!(l.write(slot, b"x"), INVALID, "write {slot}");
            assert_eq!(l.seek(slot, 0, SEEK_SET), INVALID, "seek {slot}");
            assert_eq!(l.size(slot), INVALID, "size {slot}");
            l.close(slot);
            assert!(l.entry(slot).is_none());
        }
    }

    #[test]
    fn seek_whence_is_set_end_cur_and_returns_the_previous_position() {
        let t = Tree::new();
        t.put("f.bin", b"0123456789");
        let mut l = t.layer();
        let s = l.open_read("f.bin");
        assert_eq!(l.seek(s, -4, SEEK_END), 0, "from an untouched file");
        let mut buf = [0u8; 4];
        assert_eq!(l.read(s, &mut buf), 4);
        assert_eq!(&buf, b"6789");
        assert_eq!(l.seek(s, 0, SEEK_CUR), 10);
        assert_eq!(l.seek(s, 2, SEEK_SET), 10);
        assert_eq!(l.seek(s, 0, SEEK_END), 2);
        assert_eq!(l.seek(s, 1, 3), INVALID, "unknown whence");
    }

    #[test]
    fn read_close_keeps_the_handle_and_reopen_rewinds_it() {
        let t = Tree::new();
        t.put("f.bin", b"abcdef");
        let mut l = t.layer();
        let s = l.open_read("f.bin");
        let mut buf = [0u8; 3];
        assert_eq!(l.read(s, &mut buf), 3);
        let id = l.entry(s).unwrap().id;
        l.close(s);
        let e = l.entry(s).unwrap();
        assert!(!e.in_use);
        assert!(e.handle >= 0, "read close keeps the CRT handle");
        // The layer still serves a closed read slot (the natives cannot reach it).
        assert_eq!(l.read(s, &mut buf), 3);
        assert_eq!(&buf, b"def");
        assert_eq!(l.open_read("f.bin"), s);
        assert!(l.entry(s).unwrap().id > id);
        assert_eq!(l.read(s, &mut buf), 3);
        assert_eq!(&buf, b"abc");
    }

    #[test]
    fn write_close_releases_the_handle_but_not_in_use() {
        let t = Tree::new();
        let mut l = t.layer();
        let s = l.open_write("w.bin");
        l.close(s);
        let e = l.entry(s).unwrap();
        assert_eq!(e.handle, -1);
        assert!(e.in_use, "only a read close clears in_use");
        assert_eq!(l.read(s, &mut [0u8; 1]), INVALID);
        assert_eq!(l.size(s), INVALID);
        assert_eq!(l.open_write("w.bin"), s);
        assert!(l.entry(s).unwrap().handle >= 0);
    }

    #[test]
    fn write_open_releases_a_closed_read_handle_for_the_same_name() {
        let t = Tree::new();
        t.put("both.bin", b"hello");
        let mut l = t.layer();
        let s = l.open_read("both.bin");
        assert!(l.entry(s).unwrap().handle >= 0);
        l.close(s);
        assert!(l.entry(s).unwrap().handle >= 0);
        assert_eq!(l.open_write("both.bin"), s);
        let e = l.entry(s).unwrap();
        assert_eq!(e.mode, 1);
        assert!(e.in_use);
        assert_eq!(l.size(s), 0);
    }

    #[test]
    fn slots_are_one_based_and_cap_at_thirty_one_open_files() {
        let t = Tree::new();
        let mut l = t.layer();
        for i in 0..SLOT_LAST {
            t.put(&format!("f{i}.bin"), b"x");
            assert_eq!(l.open_read(&format!("f{i}.bin")), i + 1);
        }
        t.put("extra.bin", b"x");
        assert_eq!(l.open_read("extra.bin"), 0, "no free slot");
        l.close(3);
        // A read close keeps the CRT handle, so no entry ever frees: only a name match reuses
        // a slot, and a different name still finds no free handle.
        assert!(l.entry(3).unwrap().handle >= 0);
        assert_eq!(l.open_read("f2.bin"), 3);
        assert_eq!(l.open_read("extra.bin"), 0);
    }

    #[test]
    fn ids_are_issued_from_a_monotonic_counter() {
        let t = Tree::new();
        t.put("a.bin", b"a");
        t.put("b.bin", b"b");
        let mut l = t.layer();
        assert_eq!(l.open_read("a.bin"), 1);
        assert_eq!(l.open_read("b.bin"), 2);
        assert_eq!(l.entry(1).unwrap().id, 0);
        assert_eq!(l.entry(2).unwrap().id, 1);
    }

    #[test]
    fn open_write_with_no_free_slot_still_truncates_and_creates_the_directory() {
        let t = Tree::new();
        let mut l = t.layer();
        // Read slots keep their handles while open, so the table fills up.
        for i in 0..SLOT_LAST {
            t.put(&format!("f{i}.bin"), b"x");
            assert_eq!(l.open_read(&format!("f{i}.bin")), i + 1);
        }
        t.put("victim.bin", b"0123456789");
        // `0x558890` runs before allocation: both effects happen even though nothing opens.
        assert_eq!(l.open_write("new/dir/victim.bin"), 0);
        assert!(t.0.join("new/dir").is_dir());
        assert_eq!(l.open_write("victim.bin"), 0);
        assert_eq!(fs::read(t.0.join("victim.bin")).unwrap(), b"");
    }

    #[test]
    fn a_full_table_evicts_the_least_recently_opened_closed_entry() {
        let t = Tree::new();
        let mut l = t.layer();
        for i in 0..SLOT_LAST {
            t.put(&format!("f{i}.bin"), b"x");
            assert_eq!(l.open_read(&format!("f{i}.bin")), i + 1);
        }
        t.put("other.bin", b"other");
        l.close(9);
        l.close(2);
        // Every entry still holds a handle, so a different name evicts the closed entry with the
        // smallest id: slot 2 was opened before slot 9.
        assert_eq!(l.open_read("other.bin"), 2);
        assert_eq!(l.entry(2).unwrap().name_bytes(), b"other.bin");
        assert!(!l.entry(9).unwrap().in_use);
        let mut buf = [0u8; 5];
        assert_eq!(l.read(2, &mut buf), 5);
        assert_eq!(&buf, b"other");
        // The next different name takes slot 9, the next-least-recently-opened closed entry.
        assert_eq!(l.open_read("f8.bin"), 9);
        // A closed same-name entry is reused on the spot, without touching the eviction order.
        l.close(2);
        assert_eq!(l.open_read("other.bin"), 2);
        assert_eq!(l.read(2, &mut buf), 5);
    }

    #[test]
    fn a_free_entry_is_preferred_over_evicting_a_closed_one() {
        let t = Tree::new();
        t.put("a.bin", b"a");
        t.put("b.bin", b"b");
        let mut l = t.layer();
        l.open_read("a.bin");
        l.close(1);
        // Slot 2 holds no handle, so `b.bin` takes it instead of reopening slot 1.
        assert_eq!(l.open_read("b.bin"), 2);
        assert!(!l.entry(1).unwrap().in_use);
        assert_eq!(l.open_read("a.bin"), 1);
    }
}

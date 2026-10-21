//! `java.io.File` natives (M1-09 slices I2-I5) over the I1 file layer.
//!
//! Machine model (all `[static]`, machine code read; oracle: the probe that
//! calls the exe's own natives under Unicorn, `re/oracle/io_vectors.py` for
//! `open`/`close` and the `ResourceRef` pair and `re/oracle/sdat_vectors.py`
//! for the four raw-word rows; `tests/support/file_path_oracle.py` for
//! exists/delete native marshalling and path wrappers, local only):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`FILE_OPEN`] `0x485440` | `open(I)I` | handle-field reuse/allocate, slot open, header/footer scan, handle-field store |
//! | [`FILE_CLOSE`] `0x485870` | `close()V` | slot close + SDAT footer, struct free, handle-field clear |
//! | [`FILE_EXISTS`] `0x486170` | `exists(String)I` | read VM String bytes; probe root-confined filesystem |
//! | [`FILE_DELETE`] `0x486150` | `delete(String)I` | read VM String bytes; release retained handle then delete |
//! | [`FILE_MOVE`] `0x486120` | `move(String;String;)I` | release retained closed handles; non-overwriting move; 0 success / -1 failure |
//! | [`FILE_WRITE_INT`] `0x485b20` | `write(I)I` | the value word, four little-endian bytes |
//! | [`FILE_WRITE_FLOAT`] `0x485bc0` | `write(F)I` | the float's four raw IEEE bits |
//! | [`FILE_WRITE_STRING`] `0x485fd0` | `write(Ljava.lang.String;)I` | the length word, then the bytes including the NUL |
//! | [`FILE_READ_INT`] `0x485b70` | `readInt()I` | four bytes, little-endian |
//! | [`FILE_READ_FLOAT`] `0x485c10` | `readFloat()F` | four bytes as raw IEEE bits |
//! | [`FILE_READ_STRING`] `0x486060` | `readString()Ljava.lang.String;` | the length word, the buffer read, the intern, the free |
//!
//! The `write(Ljava.util.resource.ResourceRef;)I` / `readResID()I` pair
//! (`0x485c60` / `0x485ea0`) the same class registers is [`crate::resid`]'s;
//! it shares this module's [`FileHost`], file layer and events.
//!
//! # `open(I)I`
//!
//! ```text
//! 48546a  get_instance_field(this, "name", "java.lang.String")  ; the path
//! 485477  get_int_field(<that>, DAT_0062e008)                   ; name bytes
//! 48548a  get_int_field(this, DAT_0062e008)                     ; the handle word
//! 48549a  when the handle word is 0: malloc(0x14c) and zero it
//! 4854d9  a reused struct with a live slot closes that slot first
//! 4854f8  [S+0x48] = mode;  4854fb strncpy(S+0x4c, name, 0x100)
//! 48557d  mode 0: open-read, read the 8-byte header, footer scan
//! 485552  mode 1: open-write, write 8 header bytes from 0x612c70
//! 48550e  other: append "!" + "vm_file_open: invalid file open mode"
//! 4857be  slot == 0 -> cleanup, else store S into the object's handle field
//! 485855  return S != 0
//! ```
//!
//! The header is `"SDAT" + 0x00020100`; its byte 6 (`0x612c76`) is the
//! format major version. Version 2 leaves the stream at the trailing
//! `total` word: `seek(-4, END)`, read it, `seek(-4 - total, END)`, cache
//! the position (`S+4 = position - 8`, `[`open_word`]`), read the slot-count
//! word and the resource-count word, then `seek(8, SET)` — the body start.
//! Version 1 finds its footer in the last 8 bytes instead (`seek(-8, END)`,
//! two words: table size then entry count, `seek(-8 - size, END)`). Any
//! other version value rewinds to offset 8 without reading a footer
//! (`0x4857ae`), which is also where a short header read lands.
//!
//! # `close()V`
//!
//! The slot close *and* the SDAT footer are [`slr_fmt_sdat::store::close_file`]
//! (M2-05, oracle-differentially tested); this module maps its [`AppEvent`]s
//! onto the file layer and the handle field:
//!
//! ```text
//! 485890  get_int_field(this, DAT_0062e008)   ; no handle -> return
//! 4858b0  mode 0: close(slot); mode 1: footer + close; other: neither
//! 485a92  list cleanup, free S, clear the handle field
//! ```
//!
//! # `write(Ljava.lang.String;)I`
//!
//! ```text
//! 485ff6  S = get_int_field(this, DAT_0062e008)  ; no handle -> 0
//! 48600c  slot = [S]; slot == 0 -> 0 (nothing is written)
//! 486013  the string word is the char buffer itself
//! 486019  len = 0 for a null word, else strlen(buffer) + 1 (NUL included)
//! 486034  write(slot, &len, 4)                   ; the return is discarded
//! 486040  len <= 0 -> 0 (a null string writes the length word only)
//! 48604e  write(slot, buffer, len); return its file-layer value
//! ```
//!
//! The length gate on `0x486040` is the *length*, not the first write's
//! return value: a file layer that answers 0 or `0xffffffff` for the length
//! word still gets the byte write (probed, see [`FileHost::write_string`]).
//!
//! # `readString()Ljava.lang.String;`
//!
//! ```text
//! 48607e  S = get_int_field(this, DAT_0062e008)  ; no handle -> 0
//! 486088  [S] == 0 -> 0
//! 48609a  len = 0; read(slot, &len, 4)           ; the return is discarded
//! 4860ae  len == 0 -> 0
//! 4860b4  p = malloc(len)                        ; 0x54f560
//! 4860c4  read(slot, p, len)                     ; the return is discarded
//! 4860cf  word = intern(p)                       ; 0x4174a0, a C string
//! 4860d7  free(p)                                ; 0x54f5b0
//! 4860e0  return word
//! ```
//!
//! Both reads discard their file-layer return, so a short *length* read
//! leaves the zeroed tail in the length word and a short *buffer* read is
//! the exe's uninitialised `malloc` tail.
//!
//! # `write(I)I`, `write(F)I`, `readInt()I`, `readFloat()F`
//!
//! The four raw-4-byte natives *are* [`slr_fmt_sdat::store`]'s helpers —
//! [`file_write_int`] (`0x485b20`), [`file_write_float`] (`0x485bc0`),
//! [`file_read_int`] (`0x485b70`), [`file_read_float`] (`0x485c10`) — which
//! that crate differentially tests against these very exe addresses over the
//! real machine code (`re/oracle/sdat_vectors.py`, consumer
//! `crates/slr-fmt-sdat/tests/sdat_oracle.rs::file_natives_match_oracle_vectors`,
//! which also pins the `missing` / `closed` / wrong-type field arms). This
//! module supplies what those helpers take: the [`Handle`] the object's handle
//! word resolves to (`0x42ab50` plus the `[S]` slot word) and the file layer.
//!
//! ```text
//! 485b33  marshal_args(frame, &out)                  ; 485b31: the result word starts 0
//! 485b43  S = get_int_field(this, DAT_0062e008)      ; 0 -> return 0
//! 485b52  slot = [S]; slot == 0 -> return 0
//! 485b60  write(slot, &value, 4)                     ; the write return is the native's
//! ```
//!
//! `write` marshals one raw word: for `I` the integer, for `F` the float's
//! IEEE-754 bits ([`marshal_args`] copies the cell payload verbatim, so no
//! conversion happens anywhere — a signalling NaN keeps its payload).
//! `readInt` / `readFloat` return that same raw word; the VM builds the `F`
//! result cell from the descriptor (`0x42012f`, `FUN_0041fbc0`).
//!
//! A *short* read, a closed handle and the 4-byte read's own failure leave the
//! exe's result undefined (`fld dword ptr [esp]` over the native's uninitialised
//! stack local); the port returns its zeroed word there — 0 / 0.0f, the same
//! direction as the `readString` divergences below. The one arm the exe does
//! define is the *missing* handle field, and it is 0 / 0.0f
//! (`DAT_005e73cc`) there too.
//!
//! # Divergences (all "undefined or unmodelled in the original" direction)
//!
//! - The port keeps no script object `name` field: [`FileHost::set_name`] is
//!   the constructor's seam (the exe re-reads the field on every `open`). The
//!   file-layer path and every diagnostic still use it.
//! - The exe stores the struct pointer in the object's `java.lang.Native.ptr`
//!   field (`DAT_0062e008` = 0, the first *instance* field of the class
//!   chain). The port's object field vector does not model inherited instance
//!   fields yet, so the same word lives in [`FileHost`]'s `handle` per object
//!   word, which is what [`IoEvent::SetHandle`] records.
//! - The two SDAT lists (slot run at `S+0x34`, resource table at `S+0x14`)
//!   are modelled as row vectors ([`NativeFile::slots`] /
//!   [`NativeFile::resources`], `crate::resid`): `write(ResourceRef)` appends
//!   to them and `close` writes them as the footer's slot run and resource
//!   entries. `open` still materialises no rows — it reads the two count words
//!   into [`NativeFile::slot_count`] / [`NativeFile::resource_count`] and stops
//!   instead of reading the entries the exe would rebuild (`0x485190`,
//!   `0x485685`) — so a `readResID` after reopening a file misses where the exe
//!   would hit; a row appended after such an open continues the file's
//!   numbering, as it does in the exe.
//! - The slot rows `write(ResourceRef)` appends carry no object, so `close`
//!   issues no prepare, blob read or JPEG-companion save for them (the
//!   companion savers `0x48daa0` / `0x48da30` are not ported).
//! - A short header read leaves the version byte at 0 in the port (the exe
//!   reads whatever the stack held) and so takes the rewind path.
//! - The port has no VM-side `java.lang.String` object model: a string
//!   argument reaches a native as its char buffer word (the exe's stack value
//!   is the pointer; only the receiver goes through `0x42ab50`), so the two
//!   string natives use [`FileHost::set_string`] / [`FileHost::string`], a
//!   word-to-bytes table `readString` mints its words in — `set_name`'s
//!   sibling seam. An unregistered non-zero word behaves like the exe's null
//!   word (the exe logs its missing-field fatal instead).
//! - A short buffer read in `readString` leaves a zeroed tail in the port, so
//!   the interned C string ends where the delivered bytes end (the exe's
//!   `malloc` tail is uninitialised; probed: zero in the oracle arena). A
//!   buffer with no NUL at all interns all `len` bytes (the exe reads past
//!   the buffer).
//! - The four raw-word natives return 0 / 0.0f where the exe's result is
//!   undefined (a closed handle, a short read, or a failed 4-byte read); see
//!   the section above.
//! - `readString` allocates the length word's worth of zeroed bytes; the exe
//!   `malloc`s it and writes through a failed allocation.
//! - Paths resolve under [`FileLayer`]'s root (I1); the exe uses the process
//!   CWD.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use slr_fmt_sdat::store::{
    close_file, file_read_float, file_read_int, file_write_float, file_write_int,
    Event as AppEvent, FileIo, FileState, Handle, ResourceNode, SlotNode,
};
use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};

use crate::fileio::{
    FileLayer, NativeFile, NAME_CAP, NATIVE_FILE_NAME_OFFSET, SEEK_CUR, SEEK_END, SEEK_SET,
};
use crate::resid::ResRef;

/// Exe address of `java.io.File.open(I)I`.
pub const FILE_OPEN: u32 = 0x0048_5440;
/// Exe address of `java.io.File.close()V`.
pub const FILE_CLOSE: u32 = 0x0048_5870;
/// Exe address of `java.io.File.write(I)I`.
pub const FILE_WRITE_INT: u32 = 0x0048_5b20;
/// Exe address of `java.io.File.write(F)I`.
pub const FILE_WRITE_FLOAT: u32 = 0x0048_5bc0;
/// Exe address of `java.io.File.readInt()I`.
pub const FILE_READ_INT: u32 = 0x0048_5b70;
/// Exe address of `java.io.File.readFloat()F`.
pub const FILE_READ_FLOAT: u32 = 0x0048_5c10;
/// Exe address of `java.io.File.write(Ljava.lang.String;)I`.
pub const FILE_WRITE_STRING: u32 = 0x0048_5fd0;
/// Exe address of `java.io.File.readString()Ljava.lang.String;`.
pub const FILE_READ_STRING: u32 = 0x0048_6060;

/// Exe address of `java.io.File.move(String;String;)I`.
pub const FILE_MOVE: u32 = 0x0048_6120;

/// `java.io.File.copy(String;String)I` native.
pub const FILE_COPY: u32 = 0x0048_60f0;
/// Exe address of `java.io.File.delete(String)I`.
pub const FILE_DELETE: u32 = 0x0048_6150;
/// Exe address of `java.io.File.exists(String)I`.
pub const FILE_EXISTS: u32 = 0x0048_6170;

/// The class the File rows are registered under.
pub const FILE_CLASS: &str = "java.io.File";

/// The 8 header bytes `open` writes in write mode (`0x612c70`): `"SDAT"`
/// followed by the `0x00020100` version word.
pub const SDAT_HEADER: [u8; 8] = *b"SDAT\x00\x01\x02\x00";

/// Offset of the format major version byte inside the header (`0x612c76` =
/// header byte 6).
pub const SDAT_VERSION_OFFSET: usize = 6;

/// The build's format major version (`0x612c76`), the one `close` looks up.
pub const SDAT_VERSION: u8 = 2;

/// `File.MODE_READ`: `open` opens for reading and scans the footer.
pub const MODE_READ: u32 = 0;
/// `File.MODE_WRITE`: `open` truncates, writes [`SDAT_HEADER`], `open_word` = 0.
pub const MODE_WRITE: u32 = 1;

/// The header read length (`open` reads exactly this many bytes first).
pub const HEADER_LEN: usize = 8;

/// `0x613884`: the message `open` appends for a mode other than 0/1.
pub const INVALID_MODE_MESSAGE: &str = "vm_file_open: invalid file open mode";

/// `0x613868`: the `printf` format `open` reports a bad footer with.
pub const INVALID_DATAFILE_FORMAT: &str = "Invalid datafile: %s";

/// `0x613880`: the `"!"` prefix the log appender puts before every message.
pub const LOG_PREFIX: &str = "!";

/// `DAT_0062e008`: the handle field is index 0 of the object's flattened
/// instance fields (`java.lang.Native.ptr`).
pub const HANDLE_FIELD_INDEX: u32 = 0;

/// The lowest legal footer `total` word (`0x4855e1`).
pub const MIN_FOOTER_TOTAL: u32 = 8;

/// One registry row these ports answer (`re/out/native_api.tsv` 172-185).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IoBinding {
    /// Native class name, dotted as registered.
    pub class: &'static str,
    /// Method name.
    pub name: &'static str,
    /// Descriptor text.
    pub sig: &'static str,
    /// Exe address of the implementation (`record+8`).
    pub impl_addr: u32,
}

/// File native rows backed by the file host, including path operations.
pub const FILE_NATIVE_BINDINGS: [IoBinding; 12] = [
    IoBinding {
        class: FILE_CLASS,
        name: "open",
        sig: "(I)I",
        impl_addr: FILE_OPEN,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "close",
        sig: "()V",
        impl_addr: FILE_CLOSE,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "copy",
        sig: "(Ljava.lang.String;Ljava.lang.String;)I",
        impl_addr: FILE_COPY,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "move",
        sig: "(Ljava.lang.String;Ljava.lang.String;)I",
        impl_addr: FILE_MOVE,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "write",
        sig: "(I)I",
        impl_addr: FILE_WRITE_INT,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "write",
        sig: "(F)I",
        impl_addr: FILE_WRITE_FLOAT,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "write",
        sig: "(Ljava.lang.String;)I",
        impl_addr: FILE_WRITE_STRING,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "readInt",
        sig: "()I",
        impl_addr: FILE_READ_INT,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "readFloat",
        sig: "()F",
        impl_addr: FILE_READ_FLOAT,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "readString",
        sig: "()Ljava.lang.String;",
        impl_addr: FILE_READ_STRING,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "delete",
        sig: "(Ljava.lang.String;)I",
        impl_addr: FILE_DELETE,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "exists",
        sig: "(Ljava.lang.String;)I",
        impl_addr: FILE_EXISTS,
    },
];

/// One file-layer or native-level operation, in the order the exe issues it.
///
/// The shapes mirror the oracle's hook log (`re/oracle/io_vectors.py`), so a
/// row replays into this list after normalising the two arena-dependent
/// fields (the object word and the struct word).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoEvent {
    /// `0x54f560(0x14c)`: the native struct `S` was allocated.
    AllocStruct,
    /// `0x54cec0(slot, buf, n)`: `got` bytes were delivered.
    Read {
        /// File-layer slot.
        slot: u32,
        /// Requested byte count.
        want: u32,
        /// Bytes actually delivered.
        got: u32,
    },
    /// `0x54cf00(slot, buf, n)`.
    Write {
        /// File-layer slot.
        slot: u32,
        /// Bytes handed to the layer.
        data: Vec<u8>,
    },
    /// `0x54ce40(slot, whence, offset)`.
    Seek {
        /// File-layer slot.
        slot: u32,
        /// 0 = SET, 1 = END, 2 = CUR.
        whence: u32,
        /// Byte offset.
        offset: i32,
    },
    /// `0x54c880` / `0x54caf0`: the slot table entry open.
    Open {
        /// 0 = read, 1 = write.
        mode: u32,
        /// The converted DOS path.
        name: String,
    },
    /// `0x54cca0(slot)`.
    CloseSlot {
        /// File-layer slot.
        slot: u32,
    },
    /// `0x54f5b0(S)`: the native struct was freed.
    FreeStruct,
    /// `0x54f560(len)`: the `readString` byte buffer was allocated (the same
    /// allocator the struct comes from, with the length word as its size).
    AllocBuffer {
        /// The byte count the length word carried.
        len: u32,
    },
    /// `0x54f5b0(p)`: the `readString` byte buffer was freed.
    FreeBuffer,
    /// `0x4174a0(p)`: `readString` interned the buffer's C string.
    Intern {
        /// The string word the intern returned (the native's result).
        word: u32,
        /// The interned bytes: the buffer up to its first NUL.
        data: Vec<u8>,
    },
    /// `0x551120(S+0x4c, name, 0x100)`.
    Name {
        /// Destination offset inside `S`.
        offset: usize,
        /// The copied bytes.
        name: String,
        /// The copy length argument.
        len: u32,
    },
    /// `0x429390` / `0x45fa70`: one script-side list was released.
    ListFree {
        /// Offset of the list object inside `S`.
        offset: u32,
    },
    /// `0x42a9e0(object, DAT_0062e008, value)`: the handle field. `value` is
    /// 0 on the cleanup path and the struct word on the store path.
    SetHandle {
        /// The receiver object word.
        object: u32,
        /// The word stored into the field.
        value: u32,
    },
}

/// The port-side state of one `java.io.File` object: the exe's `name` field
/// and its handle field (`java.lang.Native.ptr`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FileObject {
    /// The exe's `name` field, re-read by every `open`.
    pub name: String,
    /// The handle field: the word of the live [`NativeFile`], 0 when none.
    pub handle: u32,
}

/// The `File` natives' per-thread runtime: the file layer, the live native
/// structs, the object handle fields, the event log and the exe's log sink.
///
/// exe: the slot table `0x766460` (I1) plus the per-thread JVM state
pub struct FileHost {
    layer: FileLayer,
    objects: HashMap<u32, FileObject>,
    structs: HashMap<u32, NativeFile>,
    strings: HashMap<u32, Vec<u8>>,
    res_refs: HashMap<u32, ResRef>,
    kind_paths: HashMap<u32, String>,
    next_word: u32,
    next_string: u32,
    events: Vec<IoEvent>,
    log: Vec<String>,
}

impl FileHost {
    /// A host whose files resolve under `root` (see [`FileLayer::new`]).
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> FileHost {
        FileHost {
            layer: FileLayer::new(root),
            objects: HashMap::new(),
            structs: HashMap::new(),
            strings: HashMap::new(),
            res_refs: HashMap::new(),
            kind_paths: HashMap::new(),
            next_word: 0,
            next_string: 0,
            events: Vec::new(),
            log: Vec::new(),
        }
    }

    // ---- introspection (tests and the `File` constructor's port) ----------

    /// The file layer (slot table) below the natives.
    #[must_use]
    pub fn layer(&self) -> &FileLayer {
        &self.layer
    }

    /// The mutable slot table: tests reposition the slot `open` left after its
    /// header/footer scan, and the replay needs the same handle the natives use.
    pub fn layer_mut(&mut self) -> &mut FileLayer {
        &mut self.layer
    }

    /// The operations recorded so far, in order.
    #[must_use]
    pub fn events(&self) -> &[IoEvent] {
        &self.events
    }

    /// Take the recorded operations (tests replay one native at a time).
    pub fn take_events(&mut self) -> Vec<IoEvent> {
        core::mem::take(&mut self.events)
    }

    /// The exe's log-sink messages (`0x62e018`), without the `"!"` prefix.
    #[must_use]
    pub fn log(&self) -> &[String] {
        &self.log
    }

    /// One object's `name` field and handle field.
    #[must_use]
    pub fn object(&self, object: u32) -> FileObject {
        self.objects.get(&object).cloned().unwrap_or_default()
    }

    /// Set the object's `name` field — the seam for the script-side
    /// `File.<init>(Ljava.lang.String;)` port (see the module docs).
    pub fn set_name(&mut self, object: u32, name: &str) {
        self.objects.entry(object).or_default().name = name.to_owned();
    }

    /// The object's handle field word (0 = no handle).
    #[must_use]
    pub fn handle(&self, object: u32) -> u32 {
        self.object(object).handle
    }

    /// The live native struct behind the object's handle field.
    #[must_use]
    pub fn native_file(&self, object: u32) -> Option<&NativeFile> {
        let handle = self.handle(object);
        (handle != 0).then(|| self.structs.get(&handle)).flatten()
    }

    /// The live native struct behind the object's handle field, mutably — the
    /// seam the `resid` natives and the port's own tests build a struct's
    /// resource tables with.
    pub fn native_file_mut(&mut self, object: u32) -> Option<&mut NativeFile> {
        let handle = self.handle(object);
        if handle == 0 {
            return None;
        }
        self.structs.get_mut(&handle)
    }

    /// `[S]` (`0x485b52`, `0x485ca0`): the object's open file-layer slot,
    /// `None` when the handle field is empty or the struct is closed. The
    /// `File.open`/`close`/`write` natives read the same word
    /// (`FileHost::handle_of` with the two non-open arms collapsed).
    #[must_use]
    pub fn open_slot(&self, object: u32) -> Option<u32> {
        match self.handle_of(object) {
            Handle::Open(slot) => Some(slot),
            Handle::Closed | Handle::Missing => None,
        }
    }

    /// Register a `java.util.resource.ResourceRef` object's fields — the seam
    /// for the script-side reference, the way [`FileHost::set_string`] is the
    /// seam for a `java.lang.String` argument. The exe reads the packed id at
    /// `[ref+8]` and the kind at `[[ref+0xc]+0x4c]` (`0x485cb7`, `0x485cda`).
    pub fn set_res_ref(&mut self, word: u32, id: u32, kind: u32) {
        self.res_refs.insert(word, ResRef { id, kind });
    }

    /// The `ResourceRef` fields behind a reference word, `None` for the null
    /// word 0 and for a word no path registered (see [`FileHost::set_res_ref`]).
    #[must_use]
    pub fn res_ref(&self, word: u32) -> Option<ResRef> {
        if word == 0 {
            return None;
        }
        self.res_refs.get(&word).copied()
    }

    /// Register the archive path of a resource kind — the seam for the exe's
    /// resource-manager descriptor (`[0x618d48]`, looked up by `0x5383f0`,
    /// registered by `FUN_00538440`): the kind's `+0xc` path becomes the
    /// `write(ResourceRef)` row's name and the SDAT footer's resource-entry
    /// path (`0x485e5d`).
    pub fn set_kind_path(&mut self, kind: u32, path: &str) {
        self.kind_paths.insert(kind, path.to_owned());
    }

    /// The registered archive path of a resource kind, `None` when the manager
    /// has no such kind (the exe's `0x5383f0` answers 0 there).
    #[must_use]
    pub fn kind_path(&self, kind: u32) -> Option<&str> {
        self.kind_paths.get(&kind).map(String::as_str)
    }

    /// Register the bytes behind a string word — the seam for a script-side
    /// `java.lang.String` argument (the exe's value *is* the char buffer
    /// pointer; `0x42ab50` on the String object yields it). [`FileHost::read_string`]
    /// mints its words through the same table, so a `readString` result can be
    /// written back; `set_name` is the sibling seam for the `name` field.
    pub fn set_string(&mut self, word: u32, bytes: &[u8]) {
        self.strings.insert(word, bytes.to_vec());
    }

    /// The bytes behind a string word: `None` for the null word 0 and for a
    /// word no path registered (the exe's missing-field fatal, see the module
    /// docs).
    #[must_use]
    pub fn string(&self, word: u32) -> Option<&[u8]> {
        if word == 0 {
            return None;
        }
        self.strings.get(&word).map(Vec::as_slice)
    }

    // ---- the natives ------------------------------------------------------

    /// `File.open(I)I`: 1 when the struct was stored into the handle field.
    ///
    /// exe: LAB_00485440
    pub fn open(&mut self, object: u32, mode: u32) -> u32 {
        let name = self.object(object).name;
        let existing = self.handle(object);
        // `0x48549a`: allocate `S` only when the handle field is empty; a
        // live handle is reused as-is (`0x485496`).
        let mut file = match self.structs.remove(&existing) {
            Some(file) if existing != 0 => file,
            _ => {
                self.events.push(IoEvent::AllocStruct);
                NativeFile::new(0)
            }
        };
        let word = if existing != 0 {
            existing
        } else {
            self.next_word = self.next_word.wrapping_add(1);
            self.next_word
        };
        // `0x4854d9`: a reused struct whose slot is still live is closed first.
        if file.slot != 0 {
            let slot = file.slot;
            self.layer.close(slot);
            self.events.push(IoEvent::CloseSlot { slot });
            file.clear_slot();
        }
        // `0x4854f8` / `0x4854fb`: the mode word and the inline name copy.
        file.mode = mode; // 0x4854f8
        file.set_name(&name);
        self.events.push(IoEvent::Name {
            offset: NATIVE_FILE_NAME_OFFSET,
            name: name.clone(),
            len: NAME_CAP as u32,
        });
        match mode {
            // `0x48557d`
            MODE_READ => {
                let slot = self.layer.open_read(&name);
                self.events.push(IoEvent::Open {
                    mode: MODE_READ,
                    name: name.clone(),
                });
                file.slot = slot;
                if slot == 0 {
                    return self.fail(object, file);
                }
                self.scan_header(&name, &mut file);
            }
            // `0x485552`
            MODE_WRITE => {
                let slot = self.layer.open_write(&name);
                self.events.push(IoEvent::Open {
                    mode: MODE_WRITE,
                    name: name.clone(),
                });
                file.slot = slot;
                if slot == 0 {
                    return self.fail(object, file);
                }
                self.write_slot(slot, &SDAT_HEADER); // 0x48556d
                file.open_word = 0; // 0x485575
            }
            // `0x48550e`: no slot is touched at all.
            _ => {
                self.append_log(INVALID_MODE_MESSAGE);
                return self.fail(object, file);
            }
        }
        if file.slot == 0 {
            return self.fail(object, file);
        }
        // `0x48583f`: the handle-field store, then `return S != 0`.
        self.structs.insert(word, file);
        self.set_handle(object, word);
        self.trace("open", &name, serde_json::json!({"mode": mode, "ok": true}));
        1
    }

    /// `File.close()V`: closes the slot, writes the SDAT footer in write mode,
    /// frees the struct and clears the handle field.
    ///
    /// exe: LAB_00485870
    pub fn close(&mut self, object: u32) {
        let handle = self.handle(object);
        if handle == 0 {
            return; // `0x48589c`: no struct, nothing happens
        }
        let Some(file) = self.structs.remove(&handle) else {
            // A handle word with no struct is the exe's dangling pointer; the
            // port clears the field instead of reading it.
            self.set_handle(object, 0);
            return;
        };
        let closed_name = file.name().to_owned();
        // `0x4858b0`..`0x485a90`: the slot close and the version-dependent
        // footer are M2-05's `close_file`, which writes the slot run
        // (`0x484fa0`) and the resource table (`0x4859dc`) the two lists hold,
        // with the counters at `S+0x28` / `S+8`.
        let state = FileState {
            slot: file.slot,
            mode: file.mode,
            version: SDAT_VERSION,
            name: file.name().to_owned(),
            slot_count_word: file.slot_count,
            resource_count_word: file.resource_count,
            slots: file
                .slots
                .iter()
                .map(|row| SlotNode {
                    id: row.id,
                    kind: row.kind,
                    // The nodes `write(ResourceRef)` appends carry no object
                    // (`0x485d5e` never stores one at `+0x20`): kind 5 and 7
                    // rows therefore take `store_slot_run`'s no-blob arm.
                    object: None,
                })
                .collect(),
            resources: file
                .resources
                .iter()
                .map(|row| ResourceNode {
                    id: row.id,
                    path: row.path.clone(),
                })
                .collect(),
        };
        if let Ok(events) = close_file(&state) {
            for event in events {
                match event {
                    AppEvent::Write { slot, data } => {
                        self.layer.write(slot, &data);
                        self.events.push(IoEvent::Write { slot, data });
                    }
                    AppEvent::Close { slot } => {
                        self.layer.close(slot);
                        self.events.push(IoEvent::CloseSlot { slot });
                    }
                    // No slot row carries an object (see `state` above), so no
                    // prepare, blob read or JPEG companion save is issued for
                    // them (the companion savers `0x48daa0` / `0x48da30` are
                    // not ported).
                    AppEvent::Read { .. } | AppEvent::Prepare | AppEvent::Companion { .. } => {}
                    AppEvent::ListFree { offset } => self.events.push(IoEvent::ListFree { offset }),
                    // `0x54f5b0` frees `S`; `0x42a9e0` clears the field.
                    AppEvent::Free => self.events.push(IoEvent::FreeStruct),
                    AppEvent::ClearHandleField => self.set_handle(object, 0),
                }
            }
        }
        self.trace(
            "close",
            &closed_name,
            serde_json::json!({"mode": state.mode}),
        );
    }

    // ---- internals --------------------------------------------------------

    /// `File.write(Ljava.lang.String;)I`: writes the length word (the byte
    /// count **including** the NUL) and then the bytes including that NUL,
    /// and returns the byte write's file-layer value; 0 when the handle is
    /// missing or closed, and 0 after writing a zero length word only when
    /// `word` resolves to no buffer (the exe's null string, `0x486019`).
    ///
    /// The length word's own file-layer return is discarded: the gate on the
    /// byte write is the length (`0x486040`), so a layer that answers 0 or
    /// `0xffffffff` there still gets the bytes (probed by making the oracle's
    /// `0x54cf00` hook fail the 4-byte call).
    ///
    /// exe: LAB_00485fd0
    pub fn write_string(&mut self, object: u32, word: u32) -> u32 {
        let Some(slot) = self.slot_of(object) else {
            return 0;
        };
        // `0x486013`: the string word *is* the buffer; `0x486019`: a null one
        // leaves `len` 0 and copies nothing.
        let bytes = self.string(word).map(|b| {
            let mut buffer = b.to_vec();
            buffer.push(0); // the C string's NUL, counted by `len`
            buffer
        });
        let len = bytes.as_ref().map_or(0, |b| b.len() as u32);
        let len_word = len.to_le_bytes();
        self.layer.write(slot, &len_word); // `0x486034`
        self.events.push(IoEvent::Write {
            slot,
            data: len_word.to_vec(),
        });
        // `0x486040`: `len <= 0` (the exe's signed `jle`) skips the bytes;
        // `len` is otherwise `strlen + 1`, so only the null word lands here.
        if (len as i32) <= 0 {
            return 0;
        }
        let data = bytes.unwrap_or_default();
        let written = self.layer.write(slot, &data); // `0x48604e`
        let len = data.len();
        self.events.push(IoEvent::Write { slot, data });
        let path = self.object(object).name;
        self.trace(
            "write",
            &path,
            serde_json::json!({"kind": "string", "bytes": len, "written": written}),
        );
        written
    }

    /// `File.readString()Ljava.lang.String;`: reads a `u32` length, allocates
    /// it (`0x54f560`), reads that many bytes, interns the buffer's C string
    /// (`0x4174a0`) and frees the buffer (`0x54f5b0`); returns the interned
    /// string word. 0 for a missing handle, a closed handle (slot 0) or a zero
    /// length word — which is also what a failed length read leaves behind,
    /// since the length word starts zeroed (`0x48609a`).
    ///
    /// exe: LAB_00486060
    pub fn read_string(&mut self, object: u32) -> u32 {
        let Some(slot) = self.slot_of(object) else {
            return 0;
        };
        let mut len_word = [0u8; 4];
        let got = self.layer.read(slot, &mut len_word); // `0x4860a2`
        self.events.push(IoEvent::Read { slot, want: 4, got });
        let len = u32::from_le_bytes(len_word);
        if len == 0 {
            return 0; // `0x4860b0`
        }
        self.events.push(IoEvent::AllocBuffer { len }); // `0x54f560`
        let mut buffer = vec![0u8; len as usize];
        let got = self.layer.read(slot, &mut buffer); // `0x4860c4`
        self.events.push(IoEvent::Read {
            slot,
            want: len,
            got,
        });
        let data = c_string(&buffer);
        let word = self.intern(&data); // `0x4174a0`
        self.events.push(IoEvent::Intern {
            word,
            data: data.clone(),
        });
        self.events.push(IoEvent::FreeBuffer); // `0x54f5b0`
        let path = self.object(object).name;
        self.trace(
            "read",
            &path,
            serde_json::json!({"kind": "string", "bytes": data.len()}),
        );
        word
    }

    // ---- the path natives (I8) --------------------------------------------

    /// `File.exists(Ljava.lang.String;)I` — `0x486170`. Marshals the `String`
    /// argument and hands its char buffer to `0x54c170`; the port reads the
    /// text through [`FileHost::string`] and asks the layer.
    ///
    /// ```text
    /// 486181  mov edx,[esp+0x10]   ; the marshalled String word
    /// 486185  push edx
    /// 486186  call 0x54c170        ; exists wrapper
    /// 48618e  ret
    /// ```
    ///
    /// exe: LAB_00486170
    pub fn exists(&self, path: &str) -> u32 {
        let result = self.layer.exists(path);
        self.trace("exists", path, serde_json::json!({"result": result}));
        result
    }

    /// `File.delete(Ljava.lang.String;)I` — `0x486150`. Same shape, calling
    /// `0x54c7c0`, which closes a matching table entry and then deletes.
    ///
    /// exe: LAB_00486150
    pub fn delete(&mut self, path: &str) -> u32 {
        let result = self.layer.delete(path);
        self.trace("delete", path, serde_json::json!({"result": result}));
        result
    }

    /// `File.copy(Ljava.lang.String;Ljava.lang.String;)I` — `0x4860f0`. The
    /// marshalled words hold source then destination; `0x54c550` releases a
    /// retained destination handle and copies. Return 0 on success or
    /// `u32::MAX` on failure, as the actual `0x558a50` helper does.
    ///
    /// exe: FUN_004860f0
    pub fn copy(&mut self, src: &str, dst: &str) -> u32 {
        let result = self.layer.copy(src, dst);
        self.trace(
            "copy",
            src,
            serde_json::json!({"destination": dst, "result": result}),
        );
        result
    }

    /// `File.move(String;String;)I`: 0 on success, `u32::MAX` on failure.
    /// Refuses to replace destinations and releases retained closed handles.
    /// See [`FileLayer::move_file`] for the missing-parent bug correction.
    ///
    /// exe: FUN_00486120
    pub fn move_file(&mut self, src: &str, dst: &str) -> u32 {
        let result = self.layer.move_file(src, dst);
        self.trace(
            "move",
            src,
            serde_json::json!({"destination": dst, "result": result}),
        );
        result
    }

    // ---- the raw-word natives (I4) ----------------------------------------

    /// `File.write(I)I`: the value's four little-endian bytes through the file
    /// layer, and that layer's return value; 0 for a missing or closed handle
    /// (`0x485b31` / `0x485b4f` / `0x485b56`).
    ///
    /// exe: FUN_00485b20
    pub fn write_int(&mut self, object: u32, value: u32) -> u32 {
        let handle = self.handle_of(object);
        let written = {
            let mut io = self.io();
            file_write_int(&mut io, handle, value)
        };
        let path = self.object(object).name;
        self.trace(
            "write",
            &path,
            serde_json::json!({"kind": "int", "value": value, "written": written}),
        );
        written
    }

    /// `File.write(F)I`: the float's four raw IEEE-754 bits through the file
    /// layer (no conversion — [`marshal_args`] already copied the cell payload
    /// verbatim, so a signalling NaN keeps its payload); 0 for a missing or
    /// closed handle.
    ///
    /// exe: FUN_00485bc0
    pub fn write_float(&mut self, object: u32, value: f32) -> u32 {
        let handle = self.handle_of(object);
        let written = {
            let mut io = self.io();
            file_write_float(&mut io, handle, value)
        };
        let path = self.object(object).name;
        self.trace(
            "write",
            &path,
            serde_json::json!({"kind": "float", "bits": value.to_bits(), "written": written}),
        );
        written
    }

    /// `File.readInt()I`: the next four bytes read little-endian; 0 for a
    /// missing handle field (the exe's constant 0) or a closed one, and 0 after
    /// a short or failed read — where the exe's result is undefined (see the
    /// module docs).
    ///
    /// exe: FUN_00485b70
    pub fn read_int(&mut self, object: u32) -> u32 {
        let handle = self.handle_of(object);
        let value = {
            let mut io = self.io();
            file_read_int(&mut io, handle).unwrap_or(0)
        };
        let path = self.object(object).name;
        self.trace(
            "read",
            &path,
            serde_json::json!({"kind": "int", "value": value}),
        );
        value
    }

    /// `File.readFloat()F`: the next four bytes as raw IEEE-754 bits; 0.0 for a
    /// missing handle field (the exe's `DAT_005e73cc`) or a closed one, and 0.0
    /// after a short or failed read (see [`FileHost::read_int`]).
    ///
    /// exe: FUN_00485c10
    pub fn read_float(&mut self, object: u32) -> f32 {
        let handle = self.handle_of(object);
        let value = {
            let mut io = self.io();
            file_read_float(&mut io, handle).unwrap_or(0.0)
        };
        let path = self.object(object).name;
        self.trace(
            "read",
            &path,
            serde_json::json!({"kind": "float", "bits": value.to_bits()}),
        );
        value
    }

    // ---- internals --------------------------------------------------------

    /// `0x42ab50(this, DAT_0062e008)` plus the `[S]` slot word (`0x485b43` /
    /// `0x485b52` on the raw-word natives): the object's handle word as the
    /// store helpers' [`Handle`]. A handle word with no live struct is the
    /// exe's dangling pointer, so the port reads it as an unopened struct
    /// (`Handle::Closed`) instead of dereferencing it.
    fn handle_of(&self, object: u32) -> Handle {
        let handle = self.handle(object);
        if handle == 0 {
            return Handle::Missing;
        }
        match self.structs.get(&handle) {
            Some(file) if file.slot != 0 => Handle::Open(file.slot),
            _ => Handle::Closed,
        }
    }

    /// The [`FileIo`] window the `slr_fmt_sdat::store` helpers write through:
    /// the file layer, plus the [`IoEvent`] record the oracle replay compares
    /// against the exe's hook log.
    fn io(&mut self) -> EventIo<'_> {
        EventIo {
            layer: &mut self.layer,
            events: &mut self.events,
        }
    }

    /// `[S]`: the slot of the object's live native struct, when it is open
    /// (`0x48600c` / `0x486088`) — [`FileHost::handle_of`] with the two
    /// non-open arms collapsed, which is what the two string natives need.
    fn slot_of(&self, object: u32) -> Option<u32> {
        match self.handle_of(object) {
            Handle::Open(slot) => Some(slot),
            Handle::Closed | Handle::Missing => None,
        }
    }

    /// `0x4174a0`: intern `bytes` and return this host's word for the string
    /// (the exe returns the interned `String` object; see the module docs).
    fn intern(&mut self, bytes: &[u8]) -> u32 {
        self.next_string = self.next_string.wrapping_add(1);
        self.strings.insert(self.next_string, bytes.to_vec());
        self.next_string
    }

    /// `0x485590`..`0x4857ae`: the 8-byte header read and the footer scan.
    fn scan_header(&mut self, name: &str, file: &mut NativeFile) {
        let slot = file.slot;
        let mut header = [0u8; HEADER_LEN];
        let got = self.layer.read(slot, &mut header);
        self.events.push(IoEvent::Read {
            slot,
            want: HEADER_LEN as u32,
            got,
        });
        // A short read leaves the version byte 0 (the exe reads stale stack
        // bytes there) and so takes the rewind arm below.
        match header[SDAT_VERSION_OFFSET] {
            // `0x4856d1`
            1 => {
                self.seek(slot, -8, SEEK_END);
                let mut size = [0u8; 4];
                let got = self.layer.read(slot, &mut size);
                self.events.push(IoEvent::Read { slot, want: 4, got });
                let mut count = [0u8; 4];
                let got = self.layer.read(slot, &mut count);
                self.events.push(IoEvent::Read { slot, want: 4, got });
                let table_size = u32::from_le_bytes(size);
                self.seek(slot, (-8i32).wrapping_sub(table_size as i32), SEEK_END);
                let pos = self.seek(slot, 0, SEEK_CUR);
                file.open_word = pos.wrapping_sub(HEADER_LEN as u32); // 0x485727
                                                                      // The v1 footer's second word is the resource count; the exe
                                                                      // adds one per parsed entry (`0x485685`).
                file.resource_count = file.resource_count.wrapping_add(u32::from_le_bytes(count));
            }
            // `0x4855b7`
            2 => {
                self.seek(slot, -4, SEEK_END);
                let mut total = [0u8; 4];
                let got = self.layer.read(slot, &mut total);
                self.events.push(IoEvent::Read { slot, want: 4, got });
                let total = u32::from_le_bytes(total);
                // `0x4855e1`: the footer cannot be shorter than its own words.
                if total < MIN_FOOTER_TOTAL {
                    self.log.push(INVALID_DATAFILE_FORMAT.replace("%s", name));
                }
                self.seek(slot, (-4i32).wrapping_sub(total as i32), SEEK_END);
                let pos = self.seek(slot, 0, SEEK_CUR);
                file.open_word = pos.wrapping_sub(HEADER_LEN as u32); // 0x48561e
                                                                      // `0x485625`: the slot-count word (`0x485190` materialises the
                                                                      // run from it) and `0x48563f`: the resource count.
                let mut slot_count = [0u8; 4];
                let got = self.layer.read(slot, &mut slot_count);
                self.events.push(IoEvent::Read { slot, want: 4, got });
                let mut resource_count = [0u8; 4];
                let got = self.layer.read(slot, &mut resource_count);
                self.events.push(IoEvent::Read { slot, want: 4, got });
                // `0x485190` (the script-side slot builder) writes `S+0x28`
                // from the slot-count word; the exe increments `S+8` per
                // parsed resource entry (`0x485685`), so the count word is
                // added (a fresh struct counts 0).
                file.slot_count = u32::from_le_bytes(slot_count);
                file.resource_count = file
                    .resource_count
                    .wrapping_add(u32::from_le_bytes(resource_count));
            }
            // `0x4857ae`: an unknown version reads no footer.
            _ => {}
        }
        // `0x4857ae`: leave the stream at the body start.
        self.seek(slot, HEADER_LEN as i32, SEEK_SET);
    }

    /// `0x54cf00`: hand `data` to the file layer and record the
    /// [`IoEvent::Write`]; returns the layer's own value (the write natives
    /// pass it through, `File.open` discards it).
    pub fn write_slot(&mut self, slot: u32, data: &[u8]) -> u32 {
        let written = self.layer.write(slot, data);
        self.events.push(IoEvent::Write {
            slot,
            data: data.to_vec(),
        });
        written
    }

    /// `0x54cec0`: fill `buf` from the file layer and record the
    /// [`IoEvent::Read`]; returns the number of bytes delivered.
    pub fn read_slot(&mut self, slot: u32, buf: &mut [u8]) -> u32 {
        let got = self.layer.read(slot, buf);
        self.events.push(IoEvent::Read {
            slot,
            want: buf.len() as u32,
            got,
        });
        got
    }

    /// `0x54ce40` plus its event record.
    fn seek(&mut self, slot: u32, offset: i32, whence: u32) -> u32 {
        let previous = self.layer.seek(slot, offset, whence);
        self.events.push(IoEvent::Seek {
            slot,
            whence,
            offset,
        });
        previous
    }

    /// `0x4857c2`..`0x48583b`: the cleanup path — the two list releases, the
    /// struct free, the handle-field clear — shared by every failure.
    fn fail(&mut self, object: u32, _file: NativeFile) -> u32 {
        self.events.push(IoEvent::ListFree { offset: 0x2c });
        self.events.push(IoEvent::ListFree { offset: 0xc });
        self.events.push(IoEvent::FreeStruct);
        self.set_handle(object, 0);
        0
    }

    /// `0x42a9e0(object, DAT_0062e008, value)`.
    fn set_handle(&mut self, object: u32, value: u32) {
        self.objects.entry(object).or_default().handle = value;
        self.events.push(IoEvent::SetHandle { object, value });
    }

    /// `0x551140(&0x62e018, "!")` and the message that follows it: the two log
    /// appends the exe issues for a diagnostic, kept unprefixed in
    /// [`FileHost::log`].
    pub fn append_log(&mut self, message: &str) {
        self.log.push(LOG_PREFIX.to_owned());
        self.log.push(message.to_owned());
    }

    /// Publish one port file-layer operation to the diagnostics stream.
    ///
    /// The original's file I/O is not reproduced by the headless
    /// materialisation, so this stream is deliberately separate from the
    /// native call log: it describes what *this* implementation touched, and
    /// is never compared against an original trace.
    fn trace(&self, op: &str, path: &str, detail: serde_json::Value) {
        slr_telemetry::file_io(op, path, detail);
    }
}

/// The [`FileIo`] window the `slr_fmt_sdat::store` helpers write through
/// ([`FileHost::io`]): the file layer, plus the [`IoEvent`] record of every
/// call the helpers issue (the shapes the oracle replay compares).
struct EventIo<'a> {
    layer: &'a mut FileLayer,
    events: &'a mut Vec<IoEvent>,
}

impl FileIo for EventIo<'_> {
    fn write(&mut self, slot: u32, data: &[u8]) -> u32 {
        let written = self.layer.write(slot, data);
        self.events.push(IoEvent::Write {
            slot,
            data: data.to_vec(),
        });
        written
    }

    fn read(&mut self, slot: u32, buf: &mut [u8]) -> u32 {
        let got = self.layer.read(slot, buf);
        self.events.push(IoEvent::Read {
            slot,
            want: buf.len() as u32,
            got,
        });
        got
    }
}

/// The C string `0x4174a0` interns from a `readString` buffer: its bytes up to
/// the first NUL. A buffer without a NUL interns all of it (the exe reads past
/// the allocation there; see the module docs).
fn c_string(buffer: &[u8]) -> Vec<u8> {
    match buffer.iter().position(|&b| b == 0) {
        Some(end) => buffer[..end].to_vec(),
        None => buffer.to_vec(),
    }
}

impl core::fmt::Debug for FileHost {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("FileHost")
            .field("root", &self.layer.root())
            .field("objects", &self.objects.len())
            .field("structs", &self.structs.len())
            .field("strings", &self.strings.len())
            .field("res_refs", &self.res_refs.len())
            .field("kind_paths", &self.kind_paths.len())
            .field("events", &self.events.len())
            .field("log", &self.log)
            .finish()
    }
}

thread_local! {
    /// The per-thread `File` runtime (the exe's slot table is process-wide and
    /// its JVM state per-thread; see [`FileLayer`]).
    static FILES: RefCell<FileHost> = RefCell::new(FileHost::new("."));
}

/// Run `f` over this thread's [`FileHost`].
pub fn with_files<R>(f: impl FnOnce(&mut FileHost) -> R) -> R {
    FILES.with(|cell| f(&mut cell.borrow_mut()))
}

/// Reset this thread's [`FileHost`] to a fresh one rooted at `root`.
pub fn reset(root: impl Into<PathBuf>) {
    FILES.with(|cell| *cell.borrow_mut() = FileHost::new(root));
}

/// Register the `java.io.File` rows in the host's native registry
/// (`FUN_00416b00`). Idempotent: the exe's registrar overwrites a record.
pub fn register(vm: &mut VmHost) {
    for row in FILE_NATIVE_BINDINGS {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`NativeFn`] installed at every address.
///
/// The VM has already resolved the record and built the `CallInfo`; this
/// recovers `record+8` from the same registry (the `NativeFn` ABI carries no
/// address), marshals the container with the shared `FUN_0045d910` walk and
/// dispatches into the thread-local [`FileHost`]. `out[0]` is the receiver
/// (`container[count-1]`), `out[1]` the first parameter.
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let impl_addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let mut out = [0u32; 3];
    // The exe reads the out slots whatever the descriptor walk wrote (stale
    // stack on a short container); the port reads 0 there.
    let written = marshal_args(info, &mut out).unwrap_or(0);
    match impl_addr {
        FILE_EXISTS | FILE_DELETE => {
            // Real VM String objects own bytes through inherited Native.ptr.
            // Native-only fixtures may still supply FileHost string words.
            let path = vm
                .string_bytes(out[1])
                .map(c_string)
                .or_else(|| with_files(|files| files.string(out[1]).map(c_string)))
                .unwrap_or_default();
            let path = String::from_utf8_lossy(&path);
            Some(with_files(|files| {
                if impl_addr == FILE_EXISTS {
                    files.exists(&path)
                } else {
                    files.delete(&path)
                }
            }))
        }
        FILE_COPY | FILE_MOVE => Some(with_files(|files| {
            let source = files.string(out[1]).map(c_string).unwrap_or_default();
            let destination = files.string(out[2]).map(c_string).unwrap_or_default();
            let source = String::from_utf8_lossy(&source);
            let destination = String::from_utf8_lossy(&destination);
            if impl_addr == FILE_COPY {
                files.copy(&source, &destination)
            } else {
                files.move_file(&source, &destination)
            }
        })),
        FILE_OPEN => {
            let mode = if written > 1 { out[1] } else { 0 };
            Some(with_files(|files| files.open(out[0], mode)))
        }
        FILE_CLOSE => {
            with_files(|files| files.close(out[0]));
            None
        }
        FILE_WRITE_INT => {
            // `out[1]` is the marshalled value word (`FUN_0045d910` copies the
            // cell payload verbatim).
            let value = if written > 1 { out[1] } else { 0 };
            Some(with_files(|files| files.write_int(out[0], value)))
        }
        FILE_WRITE_FLOAT => {
            // The same word, read as the float's raw bits.
            let bits = if written > 1 { out[1] } else { 0 };
            Some(with_files(|files| {
                files.write_float(out[0], f32::from_bits(bits))
            }))
        }
        FILE_READ_INT => Some(with_files(|files| files.read_int(out[0]))),
        // The `F` result cell is built from the descriptor by
        // `invoke_native`; the native hands over the bits.
        FILE_READ_FLOAT => Some(with_files(|files| files.read_float(out[0]).to_bits())),
        FILE_WRITE_STRING => {
            // `out[1]` is the string argument: the exe's value is the char
            // buffer (see the module docs), which is the port's string word.
            let word = if written > 1 { out[1] } else { 0 };
            Some(with_files(|files| files.write_string(out[0], word)))
        }
        FILE_READ_STRING => Some(with_files(|files| files.read_string(out[0]))),
        _ => None,
    }
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in FILE_NATIVE_BINDINGS {
        vm.native_impls.insert(row.impl_addr, bridge as NativeFn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::values::Value;
    use std::fs;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A scratch root, removed on drop.
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "slr-io-file-test-{}-{}",
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

        fn read(&self, rel: &str) -> Vec<u8> {
            fs::read(self.0.join(rel)).unwrap()
        }

        fn host(&self) -> FileHost {
            FileHost::new(&self.0)
        }
    }
    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const NAME: &str = "data/cars/gti.car";
    const OBJ: u32 = 0x1000;

    /// A version-2 container: `[header][body][slot count][resource count][total]`
    /// with the empty-table total.
    fn v2(body: &[u8], slot_count: u32, resource_count: u32) -> Vec<u8> {
        let mut f = SDAT_HEADER.to_vec();
        f.extend_from_slice(body);
        f.extend_from_slice(&slot_count.to_le_bytes());
        f.extend_from_slice(&resource_count.to_le_bytes());
        f.extend_from_slice(&MIN_FOOTER_TOTAL.to_le_bytes());
        f
    }

    /// A version-1 container: `[header(version 1)][body][table size][count]`.
    fn v1(body: &[u8], table_size: u32, count: u32) -> Vec<u8> {
        let mut f = SDAT_HEADER.to_vec();
        f[SDAT_VERSION_OFFSET] = 1;
        f.extend_from_slice(body);
        f.extend_from_slice(&table_size.to_le_bytes());
        f.extend_from_slice(&count.to_le_bytes());
        f
    }

    fn name_event(name: &str) -> IoEvent {
        IoEvent::Name {
            offset: NATIVE_FILE_NAME_OFFSET,
            name: name.to_owned(),
            len: NAME_CAP as u32,
        }
    }

    fn read_event(slot: u32, want: u32, got: u32) -> IoEvent {
        IoEvent::Read { slot, want, got }
    }

    fn seek_event(slot: u32, offset: i32, whence: u32) -> IoEvent {
        IoEvent::Seek {
            slot,
            whence,
            offset,
        }
    }

    fn set_handle(value: u32) -> IoEvent {
        IoEvent::SetHandle { object: OBJ, value }
    }

    /// `open` mode 0 on a version-2 container whose body is empty: the header
    /// read, the trailing-`total` scan, the two count words and the rewind to
    /// the body start, then the handle store.
    #[test]
    fn open_read_scans_the_version_2_footer_and_stores_the_handle() {
        let tree = Tree::new();
        tree.put(NAME, &v2(&[], 0, 0));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::AllocStruct,
                name_event(NAME),
                IoEvent::Open {
                    mode: MODE_READ,
                    name: NAME.to_owned()
                },
                read_event(1, 8, 8),
                seek_event(1, -4, SEEK_END),
                read_event(1, 4, 4),
                seek_event(1, -12, SEEK_END),
                seek_event(1, 0, SEEK_CUR),
                read_event(1, 4, 4),
                read_event(1, 4, 4),
                seek_event(1, 8, SEEK_SET),
                set_handle(1),
            ]
        );
        let file = host.native_file(OBJ).expect("stored");
        assert_eq!(file.slot, 1);
        assert_eq!(file.mode, MODE_READ);
        assert_eq!(file.open_word, 0);
        assert_eq!(file.name(), NAME);
        assert_eq!(host.handle(OBJ), 1);
        assert!(host.log().is_empty());
    }

    /// The `+4` word is the footer-start position less 8 (`0x48561e`): a
    /// 4-byte body puts it at 4.
    #[test]
    fn open_read_carries_the_open_word_from_the_footer_position() {
        let tree = Tree::new();
        tree.put(NAME, &v2(&[0; 4], 0, 0));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        let file = host.native_file(OBJ).unwrap();
        assert_eq!(file.open_word, 4);
    }

    /// Version 1 finds its footer in the last 8 bytes: table size then entry
    /// count, and the stream lands at the body start.
    #[test]
    fn open_read_scans_the_version_1_footer() {
        let tree = Tree::new();
        tree.put(NAME, &v1(&[0; 4], 4, 2));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        let events = host.take_events();
        let slots: Vec<IoEvent> = events
            .iter()
            .filter(|e| {
                matches!(
                    e,
                    IoEvent::Read { .. } | IoEvent::Seek { .. } | IoEvent::Open { .. }
                )
            })
            .cloned()
            .collect();
        // header, then the two footer words in the last 8 bytes
        assert_eq!(
            slots,
            vec![
                IoEvent::Open {
                    mode: MODE_READ,
                    name: NAME.to_owned()
                },
                read_event(1, 8, 8),
                seek_event(1, -8, SEEK_END),
                read_event(1, 4, 4),
                read_event(1, 4, 4),
                seek_event(1, -12, SEEK_END),
                seek_event(1, 0, SEEK_CUR),
                seek_event(1, 8, SEEK_SET),
            ]
        );
        let file = host.native_file(OBJ).unwrap();
        assert_eq!(file.open_word, 0); // footer start (8) less 8
        assert_eq!(file.resource_count, 2);
    }

    /// A short header read leaves the version byte 0 and takes the rewind arm;
    /// the exe reads stale stack bytes there (module docs).
    #[test]
    fn open_read_with_a_short_header_rewinds_and_succeeds() {
        let tree = Tree::new();
        tree.put(NAME, &[0u8; 4]);
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::AllocStruct,
                name_event(NAME),
                IoEvent::Open {
                    mode: MODE_READ,
                    name: NAME.to_owned()
                },
                read_event(1, 8, 4),
                seek_event(1, 8, SEEK_SET),
                set_handle(1),
            ]
        );
        assert!(host.log().is_empty(), "no diagnostic on a short header");
    }

    /// A footer `total` below 8 is the one diagnostic `open` emits.
    #[test]
    fn open_read_reports_an_undersized_footer_total() {
        let tree = Tree::new();
        let mut f = SDAT_HEADER.to_vec();
        f.extend_from_slice(&0u32.to_le_bytes()); // slot count
        f.extend_from_slice(&0u32.to_le_bytes()); // resource count
        f.extend_from_slice(&4u32.to_le_bytes()); // total < 8
        tree.put(NAME, &f);
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        assert_eq!(host.log(), &[INVALID_DATAFILE_FORMAT.replace("%s", NAME)]);
    }

    /// Write mode opens (truncating), writes the 8 header bytes and leaves the
    /// `+4` word 0.
    #[test]
    fn open_write_writes_the_sdat_header() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        let file = host.native_file(OBJ).unwrap();
        assert_eq!(file.mode, MODE_WRITE);
        assert_eq!(file.slot, 1);
        assert_eq!(file.open_word, 0);
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::AllocStruct,
                name_event(NAME),
                IoEvent::Open {
                    mode: MODE_WRITE,
                    name: NAME.to_owned()
                },
                IoEvent::Write {
                    slot: 1,
                    data: SDAT_HEADER.to_vec()
                },
                set_handle(1),
            ]
        );
        assert_eq!(tree.read(NAME), SDAT_HEADER.to_vec());
    }

    /// Any other mode writes no file, logs the exe's message and clears the
    /// handle field (`0x48550e`).
    #[test]
    fn open_with_an_unknown_mode_logs_and_returns_zero() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.open(OBJ, 2), 0);
        assert_eq!(
            host.log(),
            &[LOG_PREFIX.to_owned(), INVALID_MODE_MESSAGE.to_owned()]
        );
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::AllocStruct,
                name_event(NAME),
                IoEvent::ListFree { offset: 0x2c },
                IoEvent::ListFree { offset: 0xc },
                IoEvent::FreeStruct,
                set_handle(0),
            ]
        );
        assert_eq!(host.handle(OBJ), 0);
        assert!(host.native_file(OBJ).is_none());
        assert!(!tree.0.join(NAME).exists(), "no file was created");
    }

    /// A slot the file layer refuses (`0x54c880` returning 0) runs the same
    /// cleanup and returns 0.
    #[test]
    fn open_fails_closed_when_the_slot_is_not_allocated() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME); // no such file

        assert_eq!(host.open(OBJ, MODE_READ), 0);
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::AllocStruct,
                name_event(NAME),
                IoEvent::Open {
                    mode: MODE_READ,
                    name: NAME.to_owned()
                },
                IoEvent::ListFree { offset: 0x2c },
                IoEvent::ListFree { offset: 0xc },
                IoEvent::FreeStruct,
                set_handle(0),
            ]
        );
        assert!(host.log().is_empty());
    }

    /// A second `open` reuses the live struct (`0x485496`) and closes its
    /// stale slot first (`0x4854d9`) — no second allocation.
    #[test]
    fn open_reuses_the_live_struct_and_closes_its_slot() {
        let tree = Tree::new();
        tree.put(NAME, &v2(&[], 0, 0));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        let first = host.handle(OBJ);
        host.take_events();

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        let events = host.take_events();
        assert_eq!(host.handle(OBJ), first, "the same struct word is reused");
        assert_eq!(
            events[0],
            IoEvent::CloseSlot { slot: 1 },
            "the stale slot is closed first"
        );
        assert!(
            !events.contains(&IoEvent::AllocStruct),
            "no allocation on reuse: {events:?}"
        );
        assert!(events.contains(&read_event(1, 8, 8)));
    }

    /// Read-mode close releases the slot, frees the struct and clears the
    /// handle field.
    #[test]
    fn close_of_a_read_slot_clears_the_handle() {
        let tree = Tree::new();
        tree.put(NAME, &v2(&[], 0, 0));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        host.close(OBJ);
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::CloseSlot { slot: 1 },
                IoEvent::ListFree { offset: 0x2c },
                IoEvent::ListFree { offset: 0xc },
                IoEvent::FreeStruct,
                set_handle(0),
            ]
        );
        assert_eq!(host.handle(OBJ), 0);
        assert!(host.native_file(OBJ).is_none());
    }

    /// Write-mode close writes the version-2 footer (M2-05's `close_file`),
    /// closes the slot, frees the struct and clears the handle field.
    #[test]
    fn close_of_a_write_slot_writes_the_footer() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        host.close(OBJ);
        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&0u32.to_le_bytes()); // slot count
        expected.extend_from_slice(&0u32.to_le_bytes()); // resource count
        expected.extend_from_slice(&MIN_FOOTER_TOTAL.to_le_bytes());
        assert_eq!(
            host.take_events(),
            vec![
                IoEvent::Write {
                    slot: 1,
                    data: 0u32.to_le_bytes().to_vec()
                },
                IoEvent::Write {
                    slot: 1,
                    data: 0u32.to_le_bytes().to_vec()
                },
                IoEvent::Write {
                    slot: 1,
                    data: MIN_FOOTER_TOTAL.to_le_bytes().to_vec()
                },
                IoEvent::CloseSlot { slot: 1 },
                IoEvent::ListFree { offset: 0x2c },
                IoEvent::ListFree { offset: 0xc },
                IoEvent::FreeStruct,
                set_handle(0),
            ]
        );
        assert_eq!(tree.read(NAME), expected);
    }

    /// Closing an object with no handle field does nothing at all (`0x48589c`).
    #[test]
    fn close_without_a_handle_is_a_no_op() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        host.close(OBJ);
        assert!(host.take_events().is_empty());
        assert!(host.log().is_empty());
        assert_eq!(host.handle(OBJ), 0);
    }

    /// Every row registers under `java.io.File` and `install` wires the bridge
    /// at exactly those addresses.
    #[test]
    fn register_and_install_wire_the_rows() {
        reset(".");
        let mut vm = VmHost::new();
        register(&mut vm);
        register(&mut vm);
        assert_eq!(vm.natives.len(), FILE_NATIVE_BINDINGS.len());
        for row in FILE_NATIVE_BINDINGS {
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
        assert_eq!(vm.native_impls.len(), FILE_NATIVE_BINDINGS.len());
        for row in FILE_NATIVE_BINDINGS {
            assert!(
                vm.native_impls.contains_key(&row.impl_addr),
                "{} {}{} uninstalled",
                row.class,
                row.name,
                row.sig
            );
        }
    }

    /// The bridge marshals the container (`out[0]` is the receiver, `out[1]`
    /// the mode) and dispatches into the thread-local host; `close` returns
    /// nothing (a `V` native).
    #[test]
    fn the_bridge_dispatches_open_and_close() {
        let tree = Tree::new();
        tree.put(NAME, &v2(&[], 0, 0));
        reset(&tree.0);
        let mut vm = VmHost::new();
        install(&mut vm);
        with_files(|files| files.set_name(OBJ, NAME));

        let args = [Value::int(MODE_READ as i32), Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));
        with_files(|files| {
            assert_eq!(files.handle(OBJ), 1);
            assert_eq!(files.native_file(OBJ).unwrap().name(), NAME);
        });

        let args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "close",
            sig: "()V",
        };
        assert_eq!(bridge(&mut vm, &info), None, "V pushes nothing");
        with_files(|files| assert_eq!(files.handle(OBJ), 0));
    }

    // ---- the string natives (I3) ------------------------------------------

    /// A string word the tests register through [`FileHost::set_string`].
    const STR: u32 = 7;

    /// A data file for the read tests: the SDAT header, `body` as the body and
    /// an empty version-2 footer, so `open` leaves the stream at the body.
    fn data_file(body: &[u8]) -> Vec<u8> {
        v2(body, 0, 0)
    }

    /// A body of C-string records: `[u32 len][len bytes]`.
    fn record(text: &[u8]) -> Vec<u8> {
        let mut out = (text.len() as u32).to_le_bytes().to_vec();
        out.extend_from_slice(text);
        out
    }

    fn write_event(data: &[u8]) -> IoEvent {
        IoEvent::Write {
            slot: 1,
            data: data.to_vec(),
        }
    }

    /// `write(String)` writes the length word (the byte count including the
    /// NUL) and then the bytes including it, and returns the byte write's
    /// file-layer value (`0x485fd0`, probed: eax = 6 for `"hello"`).
    #[test]
    fn write_string_writes_the_length_word_then_the_bytes() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();
        host.set_string(STR, b"hello");

        assert_eq!(host.write_string(OBJ, STR), 6);
        assert_eq!(
            host.take_events(),
            vec![write_event(&6u32.to_le_bytes()), write_event(b"hello\0"),]
        );
        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&6u32.to_le_bytes());
        expected.extend_from_slice(b"hello\0");
        assert_eq!(tree.read(NAME), expected);
    }

    /// An empty string is length 1 and one NUL byte (probed).
    #[test]
    fn write_string_of_an_empty_string_is_one_nul() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();
        host.set_string(STR, b"");

        assert_eq!(host.write_string(OBJ, STR), 1);
        assert_eq!(
            host.take_events(),
            vec![write_event(&1u32.to_le_bytes()), write_event(&[0]),]
        );
    }

    /// A null string word writes only the zero length word and returns 0
    /// (`0x486019`/`0x486040`, probed: the byte write is skipped entirely).
    #[test]
    fn write_string_of_a_null_word_writes_only_the_zero_length() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        assert_eq!(host.write_string(OBJ, 0), 0);
        assert_eq!(host.take_events(), vec![write_event(&0u32.to_le_bytes())]);
        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(tree.read(NAME), expected);
    }

    /// A word no path registered behaves like the null word (the exe's
    /// missing-field fatal, see the module docs).
    #[test]
    fn write_string_of_an_unregistered_word_writes_only_the_zero_length() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        assert_eq!(host.write_string(OBJ, STR), 0);
        assert_eq!(host.take_events(), vec![write_event(&0u32.to_le_bytes())]);
    }

    /// No handle field, or a cleared one, writes nothing at all (`0x486000`).
    #[test]
    fn write_string_without_a_handle_writes_nothing() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        host.set_string(STR, b"hello");

        assert_eq!(host.write_string(OBJ, STR), 0);
        assert!(host.take_events().is_empty());

        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.close(OBJ);
        host.take_events();
        assert_eq!(host.write_string(OBJ, STR), 0);
        assert!(host.take_events().is_empty());
    }

    /// `readString` reads the length word, allocates, reads, interns the C
    /// string and frees the buffer; the result is the string word holding the
    /// bytes (`0x486060`, probed eax = 0x7000 for the canned intern).
    #[test]
    fn read_string_reads_interns_and_frees() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&record(b"hello\0")));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        let word = host.read_string(OBJ);
        assert_eq!(word, 1, "the first interned string word");
        assert_eq!(
            host.take_events(),
            vec![
                read_event(1, 4, 4),
                IoEvent::AllocBuffer { len: 6 },
                read_event(1, 6, 6),
                IoEvent::Intern {
                    word: 1,
                    data: b"hello".to_vec()
                },
                IoEvent::FreeBuffer,
            ]
        );
        assert_eq!(host.string(word), Some(b"hello".as_slice()));
        assert_eq!(host.string(0), None, "the null word has no bytes");
    }

    /// Two reads walk the body: the second record follows the first, and the
    /// interned words differ.
    #[test]
    fn read_string_advances_the_stream() {
        let tree = Tree::new();
        let mut body = record(b"hi\0");
        body.extend_from_slice(&record(b"yo\0"));
        tree.put(NAME, &data_file(&body));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        let first = host.read_string(OBJ);
        let second = host.read_string(OBJ);
        assert_eq!(host.string(first), Some(b"hi".as_slice()));
        assert_eq!(host.string(second), Some(b"yo".as_slice()));
        assert_ne!(first, second);
        host.take_events();
    }

    /// A zero length word returns 0 with no allocation (`0x4860b0`).
    #[test]
    fn read_string_of_a_zero_length_word_returns_null() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&0u32.to_le_bytes()));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        assert_eq!(host.read_string(OBJ), 0);
        assert_eq!(host.take_events(), vec![read_event(1, 4, 4)]);
    }

    /// A short length read leaves the zeroed tail and so returns 0. The port's
    /// own `open` always leaves a 12-byte footer after the body, so the file
    /// here is a 4-byte stub: `open` takes its short-header rewind to offset 8,
    /// past the end, and the 4-byte length read delivers nothing.
    #[test]
    fn read_string_with_a_short_length_read_returns_null() {
        let tree = Tree::new();
        tree.put(NAME, &[0u8; 4]);
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        assert_eq!(host.read_string(OBJ), 0);
        assert_eq!(host.take_events(), vec![read_event(1, 4, 0)]);
    }

    /// The intern is a C string: only the bytes before the first NUL are kept
    /// (probed).
    #[test]
    fn read_string_interns_up_to_the_first_nul() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(b"\x06\0\0\0ab\0def"));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        let word = host.read_string(OBJ);
        assert_eq!(host.string(word), Some(b"ab".as_slice()));
        assert_eq!(
            host.take_events(),
            vec![
                read_event(1, 4, 4),
                IoEvent::AllocBuffer { len: 6 },
                read_event(1, 6, 6),
                IoEvent::Intern {
                    word: 1,
                    data: b"ab".to_vec()
                },
                IoEvent::FreeBuffer,
            ]
        );
    }

    /// A buffer with no NUL at all interns all `len` bytes (the exe reads past
    /// the allocation; module docs).
    #[test]
    fn read_string_without_a_nul_interns_the_whole_buffer() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(b"\x04\0\0\0abcd"));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        let word = host.read_string(OBJ);
        assert_eq!(host.string(word), Some(b"abcd".as_slice()));
    }

    /// No handle field reads nothing at all (`0x48608a`).
    #[test]
    fn read_string_without_a_handle_returns_null() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&record(b"hello\0")));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.read_string(OBJ), 0);
        assert!(host.take_events().is_empty());
    }

    /// The two natives share the word table: `readString`'s result writes back
    /// the same bytes through `write(String)`.
    #[test]
    fn a_read_string_word_round_trips_through_write_string() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&record(b"round\0")));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        let word = host.read_string(OBJ);

        const OUT: &str = "data/cars/other.car";
        host.set_name(OBJ + 1, OUT);
        assert_eq!(host.open(OBJ + 1, MODE_WRITE), 1);
        host.take_events();
        assert_eq!(host.write_string(OBJ + 1, word), 6);

        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&6u32.to_le_bytes());
        expected.extend_from_slice(b"round\0");
        assert_eq!(tree.read(OUT), expected);
    }

    /// The bridge marshals `write(String)` (`out[0]` receiver, `out[1]` the
    /// string word) and `readString` (its result is the interned word), and
    /// `readString`'s `Ljava.lang.String;` result is a reference payload.
    #[test]
    fn the_bridge_dispatches_the_string_natives() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&record(b"hello\0")));
        reset(&tree.0);
        let mut vm = VmHost::new();
        install(&mut vm);
        with_files(|files| {
            files.set_name(OBJ, NAME);
            files.set_string(STR, b"written");
        });

        let args = [Value::int(MODE_READ as i32), Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));

        let args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "readString",
            sig: "()Ljava.lang.String;",
        };
        let word = bridge(&mut vm, &info).expect("readString returns a word");
        with_files(|files| assert_eq!(files.string(word), Some(b"hello".as_slice())));

        // The write side: a second object on a write-mode file.
        const OUT: &str = "data/cars/other.car";
        with_files(|files| files.set_name(OBJ + 1, OUT));
        let args = [Value::int(MODE_WRITE as i32), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));

        let args = [Value::reference(STR), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "write",
            sig: "(Ljava.lang.String;)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(8));
        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&8u32.to_le_bytes());
        expected.extend_from_slice(b"written\0");
        assert_eq!(tree.read(OUT), expected);
    }

    // ---- the raw-word natives (I4) ----------------------------------------

    /// `write(I)` hands the file layer four little-endian bytes and returns its
    /// value (`0x485b20`; the store helper's oracle rows probe eax = 4).
    #[test]
    fn write_int_writes_four_little_endian_bytes() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        assert_eq!(host.write_int(OBJ, 0x1122_3344), 4);
        assert_eq!(
            host.take_events(),
            vec![write_event(&[0x44, 0x33, 0x22, 0x11])]
        );
        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&[0x44, 0x33, 0x22, 0x11]);
        assert_eq!(tree.read(NAME), expected);
    }

    /// Every word is written raw: `0xffffffff` keeps all four bytes
    /// (`0x485b52` copies the marshalled word, no sign handling).
    #[test]
    fn write_int_keeps_the_raw_word() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        assert_eq!(host.write_int(OBJ, u32::MAX), 4);
        assert_eq!(host.take_events(), vec![write_event(&[0xff; 4])]);
    }

    /// `write(F)` writes the float's raw bits, not a converted value: a
    /// signalling NaN keeps its payload (`0x485bc0`).
    #[test]
    fn write_float_writes_the_raw_bits() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();

        assert_eq!(host.write_float(OBJ, 1.5), 4);
        assert_eq!(host.write_float(OBJ, f32::from_bits(0x7f80_0001)), 4);
        assert_eq!(
            host.take_events(),
            vec![
                write_event(&1.5f32.to_bits().to_le_bytes()),
                write_event(&0x7f80_0001u32.to_le_bytes()),
            ]
        );
    }

    /// A missing handle field is the exe's constant-0 arm (`0x485b4f` /
    /// `0x485bef`): nothing reaches the file layer.
    #[test]
    fn a_write_without_a_handle_returns_zero() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.write_int(OBJ, 7), 0);
        assert_eq!(host.write_float(OBJ, 7.0), 0);
        assert!(host.take_events().is_empty());
    }

    /// `readInt` reads four little-endian bytes and advances the stream.
    #[test]
    fn read_int_reads_four_little_endian_bytes() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&[0x44, 0x33, 0x22, 0x11, 8, 0, 0, 0]));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        assert_eq!(host.read_int(OBJ), 0x1122_3344);
        assert_eq!(host.take_events(), vec![read_event(1, 4, 4)]);
        assert_eq!(host.read_int(OBJ), 8, "the stream advanced four bytes");
        assert_eq!(host.take_events(), vec![read_event(1, 4, 4)]);
    }

    /// `readFloat` returns the raw bits (payload included), so the VM can build
    /// the `F` cell from them (`0x485c56`).
    #[test]
    fn read_float_reads_the_raw_bits() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&0x7f80_0001u32.to_le_bytes()));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        let value = host.read_float(OBJ);
        assert!(value.is_nan());
        assert_eq!(value.to_bits(), 0x7f80_0001, "the payload survived");
        assert_eq!(host.take_events(), vec![read_event(1, 4, 4)]);
    }

    /// A short read leaves the exe's result undefined (`fld dword ptr [esp]`
    /// over the native's stack local); the port answers 0 / 0.0f there (module
    /// docs — the same direction as the `readString` divergences).
    #[test]
    fn a_short_read_returns_zero() {
        // An unknown version byte reads no footer, so `open` leaves the stream
        // at the body start with two bytes before EOF (`0x4857ae`).
        let mut file = SDAT_HEADER.to_vec();
        file[SDAT_VERSION_OFFSET] = 3;
        file.extend_from_slice(&[0x44, 0x33]);
        let tree = Tree::new();
        tree.put(NAME, &file);
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();

        assert_eq!(host.read_int(OBJ), 0);
        assert_eq!(host.take_events(), vec![read_event(1, 4, 2)]);
        assert_eq!(host.read_float(OBJ), 0.0);
        assert_eq!(host.take_events(), vec![read_event(1, 4, 0)]);
    }

    /// A missing handle field is the exe's constant-0 / `DAT_005e73cc` arm
    /// (`0x485b94` / `0x485c34`): nothing is read.
    #[test]
    fn a_read_without_a_handle_returns_zero() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);

        assert_eq!(host.read_int(OBJ), 0);
        assert_eq!(host.read_float(OBJ), 0.0);
        assert!(host.take_events().is_empty());
    }

    /// The four natives round-trip through a file the port itself wrote and
    /// closed: `close` appends the footer, `open` scans back to the body.
    #[test]
    fn the_raw_word_natives_round_trip() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        assert_eq!(host.write_int(OBJ, 0xdead_beef), 4);
        assert_eq!(host.write_float(OBJ, -0.5), 4);
        host.close(OBJ);
        assert_eq!(host.handle(OBJ), 0);

        assert_eq!(host.open(OBJ, MODE_READ), 1);
        assert_eq!(host.read_int(OBJ), 0xdead_beef);
        assert_eq!(host.read_float(OBJ), -0.5);
    }

    /// The bridge marshals the raw-word natives: `out[0]` is the receiver,
    /// `out[1]` the value word (`I` the integer, `F` the float's bits), and
    /// `readFloat` answers with the bits the VM's `F` cell needs.
    #[test]
    fn the_bridge_dispatches_the_raw_word_natives() {
        let tree = Tree::new();
        tree.put(NAME, &data_file(&0x3f00_0000u32.to_le_bytes()));
        reset(&tree.0);
        let mut vm = VmHost::new();
        install(&mut vm);

        with_files(|files| files.set_name(OBJ, NAME));
        let args = [Value::int(MODE_READ as i32), Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));

        let args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "readFloat",
            sig: "()F",
        };
        assert_eq!(bridge(&mut vm, &info), Some(0x3f00_0000), "0.5f bits");

        // The write side: a second object on a write-mode file.
        const OUT: &str = "data/cars/other.car";
        with_files(|files| files.set_name(OBJ + 1, OUT));
        let args = [Value::int(MODE_WRITE as i32), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));

        let args = [Value::int(-1), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "write",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(4));

        let args = [Value::float(0.5), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "write",
            sig: "(F)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(4));

        let mut expected = SDAT_HEADER.to_vec();
        expected.extend_from_slice(&u32::MAX.to_le_bytes());
        expected.extend_from_slice(&0.5f32.to_bits().to_le_bytes());
        assert_eq!(tree.read(OUT), expected);

        // Read the same two words back through the bridge.
        let args = [Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "close",
            sig: "()V",
        };
        assert_eq!(bridge(&mut vm, &info), None);

        let args = [Value::int(MODE_READ as i32), Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(1));

        let args = [Value::reference(OBJ + 1)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "readInt",
            sig: "()I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(u32::MAX));

        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "readFloat",
            sig: "()F",
        };
        assert_eq!(bridge(&mut vm, &info), Some(0.5f32.to_bits()));
    }
}

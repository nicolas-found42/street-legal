//! `java.util.resource.ResourceRef` File natives (M1-09 slice I5).
//!
//! Machine model (all `[static]`, machine code read; oracle: `re/oracle/io_vectors.py`,
//! local only):
//!
//! | exe | registry row | body |
//! |---|---|---|
//! | [`FILE_WRITE_RES_REF`] `0x485c60` | `write(Ljava.util.resource.ResourceRef;)I` | the packed word through the resource table or the script-side slot run |
//! | [`FILE_READ_RES_ID`] `0x485ea0` | `readResID()I` | one word read back into a resource id |
//!
//! # The resource id
//!
//! A `ResourceRef`'s packed id is `kind << 16 | low16` (`[ref+8]`, `0x485cb7`).
//! The high half is either a *resource kind* the manager knows
//! (`0x5383f0(manager, id)` indexes the descriptor array at
//! `manager+0xffe94` by `id>>16` and answers the entry whose inline archive
//! path lives at `+0xc`), or [`SCRIPT_KIND`] `0xffff` for a reference the
//! script side created, whose kind is read from the *type object*
//! `[[ref+0xc]+0x4c]` (`0x485cda`) and must be [5 or 7](SAVEABLE_KINDS) to be
//! saveable.
//!
//! Both natives write and read exactly one `u32`, and which table produces it
//! depends on the high half — with the two sides differing on a `0` high half
//! exactly as the exe does:
//!
//! | selection | written word (`0x485c60`) | read word (`0x485ea0`) |
//! |---|---|---|
//! | high half `!= 0xffff` (a manager kind, `0` included) | the resource table: `table_id << 16 \| low16` (`0x485e71`; the row's `+0xc` in the high half, the reference's `low16` kept) | the resource table, matched by `[node+0xc] == word >> 16`: `kind << 16 \| low16` (`0x485f3b`) |
//! | high half `0xffff` | the slot run: the row's `+0xc` id alone (`0x485dd6` found, `0x485dc9` appended) | the same slot-run lookup (the read side groups `0` and `0xffff`, `0x485ef3`): the row's `+0x1c`, the reference id it stored (`0x485f7d`) |
//! | high half `0` | (the resource table, `kind` 0) | the slot run |
//!
//! A reference id of 0 and a read word of 0 are their own arms: the first
//! writes a zero word, the second returns 0 (`0x485cbf`, `0x485eed`).
//!
//! # The two tables
//!
//! The exe keeps them as `S`'s lists (`crate::fileio`): the resource table at
//! `S+0x14` (one row per kind, `+0xc` = the row id, `+0x10` = the kind, `+0x14`
//! = the archive path) and the script-side slot run at `S+0x34` (`+0xc` = the
//! slot id, `+0x10` = the kind, `+0x1c` = the reference id). The port keeps
//! [`ResourceRow`] / [`SlotRow`] vectors instead of the node chains; the row
//! *ids* are the counters `S+8` / `S+0x28` (`0x485e57` / `0x485d9f`), which
//! `File.close` writes as the footer's two count words.
//!
//! # `write(Ljava.util.resource.ResourceRef;)I`
//!
//! ```text
//! 485c96  S = get_int_field(this, DAT_0062e008)      ; 0 -> return 0, nothing written
//! 485ca0  [S] == 0 -> return 0                       ; closed handle
//! 485ca9  id = [ref+8] (0 for the null ref word)
//! 485cbf  id == 0 -> 485e84                          ; write the zero word
//! 485cca  kind = id >> 16
//! 485cd0  kind != 0xffff -> 485de1 (the resource table)
//!         kind == 0xffff: ref_kind = [[ref+0xc]+0x4c]
//! 485ce8  ref_kind != 5 and != 7 -> log "!" + "ERROR! Unsaveable resource!"
//! 485d31  and write the id unchanged
//! 485d36  else walk the slot run by the full id (`[node+0x1c]`): found -> the
//!         row's `+0xc`; else allocate a row (id = ++[S+0x28], kind, the type
//!         copy) and use that id
//! 485ded  the resource table: desc = 0x5383f0(manager, id), the key is
//!         `desc+8` (= the kind)
//! 485e03  walk it (`[node+0x10]`): found -> row id << 16 | low16
//! 485e16  else allocate a row (id = ++[S+8], kind, strncpy the descriptor's
//!         inline path) and use that id << 16 | low16
//! 485e84  write the word; the 4-byte write's return is the native's
//! ```
//!
//! # `readResID()I`
//!
//! ```text
//! 485ec7  S = get_int_field(this, DAT_0062e008)      ; 0 -> return 0
//! 485ecf  [S] == 0 -> return 0                       ; closed handle
//! 485ed9  read(slot, &word, 4)                       ; the return is discarded
//! 485eeb  word == 0 -> return 0
//! 485ef3  kind = word >> 16
//! 485f01  kind == 0 or 0xffff -> the slot run:
//!           walk it by `(node+0xc) & 0xffff` vs the word's low half: found ->
//!           `[node+0x1c]`; else (and an empty list) log "ERROR! Unsaved
//!           resource!" and return 0
//! 485f03  else the resource table: walk it by `[node+0xc]` == kind (the word's
//!         high half): found -> `[node+0x10] << 16 | low16`; a miss (including
//!         an empty table) returns 0 *without* a log
//! ```
//!
//! # Divergences (all "undefined or unmodelled in the original" direction)
//!
//! - The exe's lists are node chains with list objects at `S+0x0c`/`S+0x2c`;
//!   the port keeps the rows ([`ResourceRow`] / [`SlotRow`]) and drops the
//!   nodes, their links and the objects, whose fields the walks also touch
//!   (`0x485df0` starts at the head word, whose empty value is the end marker
//!   at `S+0x1c`/`S+0x3c`, whose `+0x10`/`+0x1c` the walks compare against).
//! - `File.open` materialises no rows (I2, module docs): a file opened for
//!   reading leaves both tables empty, so `readResID` misses where the exe —
//!   which rebuilds the tables from the footer (`0x485190`, `0x485685`) —
//!   would hit. The counters *are* read from the file, so a row appended after
//!   such an open continues the file's numbering.
//! - A kind the manager has no descriptor for is undefined in the exe
//!   (`0x5383f0` answers 0 and `0x485df7` then reads `[0+8]`); the port uses
//!   the id's high half as the row's kind and an empty path
//!   ([`FileHost::kind_path`] answers `None`).
//! - A reference word no path registered behaves like the exe's null word
//!   (`id` 0, and no kind on the `0xffff` arm); the exe reads `[word+8]`.
//! - A failed or short 4-byte read in `readResID` leaves the port's zeroed
//!   word (return 0) where the exe keeps whatever its stack slot held.
//! - The JPEG-companion savers (`0x48daa0` / `0x48da30`) are not ported:
//!   `File.close` still writes the slot run — whose rows carry no object, the
//!   state `write(ResourceRef)` leaves (`0x485d5e`) — but the companion files
//!   are not saved.
//! - Paths and the manager's inline archive path are byte strings in the exe;
//!   the port keeps them as `String`s ([`FileHost::set_kind_path`]).

use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};

use crate::file::{with_files, FileHost, IoBinding, FILE_CLASS};

/// Exe address of `java.io.File.write(Ljava.util.resource.ResourceRef;)I`.
pub const FILE_WRITE_RES_REF: u32 = 0x0048_5c60;
/// Exe address of `java.io.File.readResID()I`.
pub const FILE_READ_RES_ID: u32 = 0x0048_5ea0;

/// The `0xffff` high half: a script-side resource id, whose kind lives on the
/// ref's type object instead of the manager (`0x485cca`).
pub const SCRIPT_KIND: u32 = 0xffff;

/// The two kinds `write(ResourceRef)` can save (`0x485ce8`: the exe subtracts
/// 5 and 2 from the type's kind word and saves on either arm) — the SDAT
/// JPEG-slot kinds.
pub const SAVEABLE_KINDS: [u32; 2] = [5, 7];

/// The shift that packs a kind and its low half into a resource id.
pub const KIND_SHIFT: u32 = 16;

/// `0x6138cc`: what `write(ResourceRef)` reports for a `0xffff`-high id whose
/// type kind is neither 5 nor 7 (appended after the `"!"` prefix, `0x6138c8`).
pub const UNSAVEABLE_MESSAGE: &str = "ERROR! Unsaveable resource!";

/// `0x6138ec`: what `readResID` reports when the word matches no slot-run row
/// (appended after the `"!"` prefix, `0x6138e8`).
pub const UNSAVED_MESSAGE: &str = "ERROR! Unsaved resource!";

/// `kind << 16 | low16`.
#[must_use]
pub const fn pack(kind: u32, low16: u32) -> u32 {
    (kind << KIND_SHIFT) | (low16 & 0xffff)
}

/// The high half of a resource id (`id >> 16`).
#[must_use]
pub const fn kind_of(id: u32) -> u32 {
    id >> KIND_SHIFT
}

/// The low half of a resource id (`id & 0xffff`).
#[must_use]
pub const fn low16(id: u32) -> u32 {
    id & 0xffff
}

/// Whether `write(ResourceRef)` can save a script-side ref of this type kind
/// (`0x485ce8`: `kind - 5 == 0` or `kind - 7 == 0`).
#[must_use]
pub const fn saveable_kind(kind: u32) -> bool {
    kind == SAVEABLE_KINDS[0] || kind == SAVEABLE_KINDS[1]
}

/// One resource-table row: the exe's `S+0x14` list node, without the links.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct ResourceRow {
    /// `+0xc`: the row id written as the SDAT entry's own word (`0x4859f8`)
    /// and packed into the high half of every resID word that names this kind
    /// (`0x485e71`).
    pub id: u32,
    /// `+0x10`: the resource kind the row was opened for — the key
    /// `write(ResourceRef)` dedups on (`0x485e03`, against the descriptor's
    /// `+8`, which is the kind itself).
    pub kind: u32,
    /// `+0x14`: the descriptor's inline archive path (`0x551120` copies it).
    pub path: String,
}

/// One slot-run row: the exe's `S+0x34` list node, without the links.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct SlotRow {
    /// `+0xc`: the 1-based slot index written to the file, the value
    /// `write(ResourceRef)` returns for a script-side ref (`0x485dc9`).
    pub id: u32,
    /// `+0x10`: the ref's type kind (5 or 7), what `close` writes beside the
    /// id (`0x484fa0`).
    pub kind: u32,
    /// `+0x1c`: the reference id the row was opened for, the value
    /// `readResID` hands back (`0x485f7d`).
    pub res_id: u32,
}

/// A `java.util.resource.ResourceRef`'s two fields the natives read: the
/// packed id at `+8` (`0x485cb7`) and the type kind at `[[ref+0xc]+0x4c]`
/// (`0x485cda`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResRef {
    /// `[ref+8]`: `kind << 16 | low16`.
    pub id: u32,
    /// `[[ref+0xc]+0x4c]`: the resource kind the *type* carries; only the
    /// `0xffff` arm of `write(ResourceRef)` reads it.
    pub kind: u32,
}

/// `0x485e03`: the resource-table walk `write(ResourceRef)` runs — the first
/// row whose [`ResourceRow::kind`] matches; returns its [`ResourceRow::id`].
#[must_use]
pub fn find_resource(rows: &[ResourceRow], kind: u32) -> Option<u32> {
    rows.iter().find(|row| row.kind == kind).map(|row| row.id)
}

/// `0x485f1e`: the resource-table walk `readResID` runs by the read word's
/// high half (`[node+0xc] == word >> 16`); returns the matching row's kind
/// (`[node+0x10]`).
#[must_use]
pub fn resource_kind(rows: &[ResourceRow], word: u32) -> Option<u32> {
    let index = kind_of(word);
    rows.iter().find(|row| row.id == index).map(|row| row.kind)
}

/// `0x485d47`: the slot-run walk `write(ResourceRef)` runs by the *full*
/// reference id (`[node+0x1c]`); returns the matching row's slot id
/// (`[node+0xc]`).
#[must_use]
pub fn find_slot(rows: &[SlotRow], res_id: u32) -> Option<u32> {
    rows.iter()
        .find(|row| row.res_id == res_id)
        .map(|row| row.id)
}

/// `0x485f63`: the slot-run walk `readResID` runs — node `+0xc` compared on
/// the low half only (`xor` then `test 0xffff`), which is the slot index the
/// read word carries; returns the row's stored reference id (`[node+0x1c]`).
#[must_use]
pub fn slot_res_id(rows: &[SlotRow], word: u32) -> Option<u32> {
    let index = low16(word);
    rows.iter()
        .find(|row| low16(row.id) == index)
        .map(|row| row.res_id)
}

/// The two `java.util.resource` rows these slices port, in the exe's
/// registration order (`re/out/native_api.tsv` 180/184).
pub const RESID_NATIVE_BINDINGS: [IoBinding; 2] = [
    IoBinding {
        class: FILE_CLASS,
        name: "write",
        sig: "(Ljava.util.resource.ResourceRef;)I",
        impl_addr: FILE_WRITE_RES_REF,
    },
    IoBinding {
        class: FILE_CLASS,
        name: "readResID",
        sig: "()I",
        impl_addr: FILE_READ_RES_ID,
    },
];

/// `File.write(Ljava.util.resource.ResourceRef;)I`: the packed id word through
/// the object's file layer, and that layer's return value; 0 for a missing
/// handle field or a closed slot, where nothing is written (`0x485c9a` /
/// `0x485ca3`).
///
/// A zero id (a null ref word, or [`ResRef::id`] 0) writes a zero word. A
/// `0xffff`-high id whose type kind is not 5 or 7 logs
/// [`UNSAVEABLE_MESSAGE`] and writes the id unchanged; otherwise the word is
/// the row id of the resource table (a manager kind: `table_id << 16 | low16`)
/// or of the script-side slot run (the slot id alone), a row being appended
/// with `++resource_count` / `++slot_count` when the kind (or the ref id) is
/// new.
///
/// exe: FUN_00485c60
pub fn write_res_ref(host: &mut FileHost, object: u32, ref_word: u32) -> u32 {
    let Some(slot) = host.open_slot(object) else {
        return 0;
    };
    let res = host.res_ref(ref_word);
    let id = res.map_or(0, |res| res.id);
    let mut word = id;
    if id != 0 {
        let kind = kind_of(id);
        if kind == SCRIPT_KIND {
            let ref_kind = res.map_or(0, |res| res.kind);
            if saveable_kind(ref_kind) {
                let file = host
                    .native_file_mut(object)
                    .expect("an open slot implies a live struct");
                word = match find_slot(&file.slots, id) {
                    Some(row_id) => row_id,
                    None => {
                        // `0x485d9f`: the counter is the row's id.
                        file.slot_count = file.slot_count.wrapping_add(1);
                        let row_id = file.slot_count;
                        file.slots.push(SlotRow {
                            id: row_id,
                            kind: ref_kind,
                            res_id: id,
                        });
                        row_id
                    }
                };
            } else {
                // `0x485cf2`: the two appends and the flush; the id is written
                // unchanged (`0x485d31` jumps to the write with it).
                host.append_log(UNSAVEABLE_MESSAGE);
            }
        } else {
            let path = host.kind_path(kind).unwrap_or_default().to_owned();
            let file = host
                .native_file_mut(object)
                .expect("an open slot implies a live struct");
            word = match find_resource(&file.resources, kind) {
                Some(row_id) => pack(row_id, low16(id)),
                None => {
                    // `0x485e57`: the counter is the row's id.
                    file.resource_count = file.resource_count.wrapping_add(1);
                    let row_id = file.resource_count;
                    file.resources.push(ResourceRow {
                        id: row_id,
                        kind,
                        path,
                    });
                    pack(row_id, low16(id))
                }
            };
        }
    }
    // `0x485e84`: `write(slot, &word, 4)`; a null/zero id lands here too.
    host.write_slot(slot, &word.to_le_bytes())
}

/// `File.readResID()I`: reads one word and maps it back to a resource id; 0
/// for a missing handle field, a closed slot, or a zero word (`0x485eed`).
///
/// A word whose high half is 0 or `0xffff` names a slot-run row by its low
/// half and yields the reference id the row stored — with
/// [`UNSAVED_MESSAGE`] logged when no row matches (an empty list included). A
/// word with any other high half is a resource-table row's id and yields
/// `kind << 16 | low16`; a miss returns 0 without a log (`0x485f0b`,
/// `0x485f28`, `0x485f33` all jump straight to the return).
///
/// The exe discards the 4-byte read's own return; the port reads its zeroed
/// word there (see the module docs).
///
/// exe: FUN_00485ea0
pub fn read_res_id(host: &mut FileHost, object: u32) -> u32 {
    let Some(slot) = host.open_slot(object) else {
        return 0;
    };
    let mut buffer = [0u8; 4];
    host.read_slot(slot, &mut buffer);
    let word = u32::from_le_bytes(buffer);
    if word == 0 {
        return 0;
    }
    let kind = kind_of(word);
    if kind == 0 || kind == SCRIPT_KIND {
        let hit = host
            .native_file_mut(object)
            .and_then(|file| slot_res_id(&file.slots, word));
        match hit {
            Some(res_id) => res_id,
            None => {
                host.append_log(UNSAVED_MESSAGE);
                0
            }
        }
    } else {
        host.native_file_mut(object)
            .and_then(|file| resource_kind(&file.resources, word))
            .map_or(0, |kind| pack(kind, low16(word)))
    }
}

/// Register the two `ResourceRef` rows in the host's native registry
/// (`FUN_00416b00`). Idempotent: the exe's registrar overwrites a record.
pub fn register(vm: &mut VmHost) {
    for row in RESID_NATIVE_BINDINGS {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`NativeFn`] installed at both addresses.
///
/// The VM has already resolved the record and built the `CallInfo`; this
/// recovers `record+8` from the same registry (the `NativeFn` ABI carries no
/// address), marshals the container with the shared `FUN_0045d910` walk and
/// dispatches into the thread-local [`FileHost`]. `out[0]` is the receiver
/// (`container[count-1]`), `out[1]` the reference argument (the exe's value is
/// the `ResourceRef` object; the port's is the word
/// [`FileHost::set_res_ref`] registered).
///
/// exe: FUN_0041fbc0 (native call path)
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let impl_addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let mut out = [0u32; 2];
    // The exe reads the out slots whatever the descriptor walk wrote (stale
    // stack on a short container); the port reads 0 there.
    let written = marshal_args(info, &mut out).unwrap_or(0);
    match impl_addr {
        FILE_WRITE_RES_REF => {
            let ref_word = if written > 1 { out[1] } else { 0 };
            Some(with_files(|files| write_res_ref(files, out[0], ref_word)))
        }
        FILE_READ_RES_ID => Some(with_files(|files| read_res_id(files, out[0]))),
        _ => None,
    }
}

/// Register the rows and install [`bridge`] at both addresses.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in RESID_NATIVE_BINDINGS {
        vm.native_impls.insert(row.impl_addr, bridge as NativeFn);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::file::{
        install as file_install, reset, IoEvent, MODE_READ, MODE_WRITE, SDAT_HEADER,
    };
    use slr_vm::host::VmHost;
    use slr_vm::values::Value;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const OBJ: u32 = 0x1000;
    const REF: u32 = 0x2000;
    const NAME: &str = "data/cars/gti.car";
    /// A script-side reference: `0xffff` high, low half 7.
    const SCRIPT_ID: u32 = 0xffff_0007;
    /// A manager-side reference: kind 3, low half 7.
    const SAVED_ID: u32 = 3 << KIND_SHIFT | 7;
    const KIND: u32 = 3;
    const PATH: &str = "system.rpk";

    /// A scratch root, removed on drop.
    struct Tree(PathBuf);
    impl Tree {
        fn new() -> Tree {
            static N: AtomicUsize = AtomicUsize::new(0);
            let p = std::env::temp_dir().join(format!(
                "slr-io-resid-test-{}-{}",
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

    /// A version-2 container with `body` and empty tables.
    fn v2(body: &[u8]) -> Vec<u8> {
        let mut f = SDAT_HEADER.to_vec();
        f.extend_from_slice(body);
        f.extend_from_slice(&0u32.to_le_bytes()); // slot-count word (`S+0x28`)
        f.extend_from_slice(&0u32.to_le_bytes()); // resource-count word (`S+8`)
        f.extend_from_slice(&8u32.to_le_bytes()); // total: the two words
        f
    }

    /// A host with `OBJ` open for writing.
    fn write_host(tree: &Tree) -> FileHost {
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_WRITE), 1);
        host.take_events();
        host
    }

    /// A host with `OBJ` open for reading a v2 container whose body is `body`.
    fn read_host(tree: &Tree, body: &[u8]) -> FileHost {
        tree.put(NAME, &v2(body));
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        host.take_events();
        host
    }

    /// The two tables' rows, as `(resources, slots)`.
    fn tables(host: &FileHost) -> (&[ResourceRow], &[SlotRow]) {
        let file = host.native_file(OBJ).expect("open");
        (&file.resources, &file.slots)
    }

    fn write_event(data: &[u8]) -> IoEvent {
        IoEvent::Write {
            slot: 1,
            data: data.to_vec(),
        }
    }

    #[test]
    fn packing_helpers_split_the_resource_id() {
        assert_eq!(pack(3, 7), 0x0003_0007);
        assert_eq!(pack(0xffff, 0x1234), 0xffff_1234);
        assert_eq!(kind_of(SAVED_ID), KIND);
        assert_eq!(kind_of(SCRIPT_ID), SCRIPT_KIND);
        assert_eq!(low16(SAVED_ID), 7);
        assert_eq!(low16(SCRIPT_ID), 7);
        assert!(saveable_kind(5) && saveable_kind(7));
        assert!(!saveable_kind(0) && !saveable_kind(3) && !saveable_kind(6));
    }

    /// A manager kind packs the table row's id into the high half and keeps
    /// the reference's low half; the row carries the kind and the registered
    /// archive path.
    #[test]
    fn write_saved_id_packs_the_table_index() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_kind_path(KIND, PATH);
        host.set_res_ref(REF, SAVED_ID, 5);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(
            host.take_events(),
            vec![write_event(&pack(1, 7).to_le_bytes())]
        );
        let (resources, slots) = tables(&host);
        assert_eq!(
            resources,
            [ResourceRow {
                id: 1,
                kind: KIND,
                path: PATH.to_owned()
            }]
        );
        assert!(slots.is_empty());
        assert_eq!(host.native_file(OBJ).unwrap().resource_count, 1);
        assert!(host.log().is_empty());
    }

    /// One row per kind: a second reference of the same kind reuses the row
    /// (and its id) instead of appending.
    #[test]
    fn write_saved_id_reuses_the_row_of_its_kind() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_kind_path(KIND, PATH);
        host.set_res_ref(REF, SAVED_ID, 5);
        host.set_res_ref(REF + 1, 3 << KIND_SHIFT | 9, 5);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(write_res_ref(&mut host, OBJ, REF + 1), 4);
        assert_eq!(
            host.take_events(),
            vec![
                write_event(&pack(1, 7).to_le_bytes()),
                write_event(&pack(1, 9).to_le_bytes()),
            ]
        );
        assert_eq!(tables(&host).0.len(), 1);
        assert_eq!(host.native_file(OBJ).unwrap().resource_count, 1);
    }

    /// A small id (high half 0) is a *manager* kind on the write side — the
    /// read side groups 0 with `0xffff` instead (see the module docs) — so the
    /// row keys on kind 0.
    #[test]
    fn write_zero_high_id_takes_the_manager_path() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_res_ref(REF, 5, 5);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(host.events(), [write_event(&pack(1, 5).to_le_bytes())]);
        assert_eq!(
            tables(&host).0,
            [ResourceRow {
                id: 1,
                kind: 0,
                path: String::new()
            }]
        );
    }

    /// A `0xffff`-high reference packs the *slot* row's index alone, and the
    /// row keeps the reference id `readResID` hands back.
    #[test]
    fn write_script_id_packs_the_slot_index() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_res_ref(REF, SCRIPT_ID, 5);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(
            host.take_events(),
            vec![
                write_event(&1u32.to_le_bytes()),
                write_event(&1u32.to_le_bytes()),
            ]
        );
        let (resources, slots) = tables(&host);
        assert!(resources.is_empty());
        assert_eq!(
            slots,
            [SlotRow {
                id: 1,
                kind: 5,
                res_id: SCRIPT_ID
            }]
        );
        assert_eq!(host.native_file(OBJ).unwrap().slot_count, 1);
    }

    /// Kind 7 is the other saveable slot kind (`0x485ced`).
    #[test]
    fn write_script_id_of_kind_7_is_saveable() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_res_ref(REF, 0xffff_0003, 7);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(
            tables(&host).1,
            [SlotRow {
                id: 1,
                kind: 7,
                res_id: 0xffff_0003
            }]
        );
    }

    /// A `0xffff`-high reference whose type kind is not 5 or 7 is unsaveable:
    /// the diagnostic is logged and the id itself is written.
    #[test]
    fn write_unsaveable_script_id_logs_and_writes_the_raw_id() {
        for kind in [0, 3, 6] {
            let tree = Tree::new();
            let mut host = write_host(&tree);
            host.set_res_ref(REF, SCRIPT_ID, kind);

            assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
            assert_eq!(
                host.take_events(),
                vec![write_event(&SCRIPT_ID.to_le_bytes())]
            );
            assert_eq!(host.log(), ["!", UNSAVEABLE_MESSAGE]);
            let (resources, slots) = tables(&host);
            assert!(resources.is_empty() && slots.is_empty());
            assert_eq!(host.native_file(OBJ).unwrap().slot_count, 0);
        }
    }

    /// A zero id — a null reference word or an id that *is* 0 — writes a zero
    /// word and touches no table.
    #[test]
    fn write_zero_or_null_id_writes_a_zero_word() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_res_ref(REF, 0, 5);

        assert_eq!(write_res_ref(&mut host, OBJ, REF), 4);
        assert_eq!(write_res_ref(&mut host, OBJ, 0), 4);
        assert_eq!(
            host.take_events(),
            vec![
                write_event(&0u32.to_le_bytes()),
                write_event(&0u32.to_le_bytes())
            ]
        );
        assert!(host.log().is_empty());
        let (resources, slots) = tables(&host);
        assert!(resources.is_empty() && slots.is_empty());
    }

    /// No handle field, and a struct whose slot is 0: nothing is written.
    #[test]
    fn write_without_an_open_handle_writes_nothing() {
        let tree = Tree::new();
        let mut host = tree.host();
        host.set_res_ref(REF, SAVED_ID, 5);
        assert_eq!(write_res_ref(&mut host, OBJ, REF), 0);
        assert!(host.take_events().is_empty());

        let mut host = write_host(&tree);
        host.set_res_ref(REF, SAVED_ID, 5);
        host.close(OBJ);
        host.take_events();
        assert_eq!(write_res_ref(&mut host, OBJ, REF), 0);
        assert!(host.take_events().is_empty());
    }

    /// A saved word unpacks through the row the table id names: the kind comes
    /// back in the high half, the reference's low half is kept.
    #[test]
    fn read_res_id_unpacks_the_table_row() {
        let tree = Tree::new();
        let mut host = read_host(&tree, &pack(1, 7).to_le_bytes());
        host.native_file_mut(OBJ)
            .unwrap()
            .resources
            .push(ResourceRow {
                id: 1,
                kind: KIND,
                path: PATH.to_owned(),
            });

        assert_eq!(read_res_id(&mut host, OBJ), pack(KIND, 7));
        assert_eq!(
            host.take_events(),
            vec![IoEvent::Read {
                slot: 1,
                want: 4,
                got: 4
            }]
        );
        assert!(host.log().is_empty());
    }

    /// The row is found by the *table id* in the word's high half, not by
    /// position, and a row's own kind is what comes back.
    #[test]
    fn read_res_id_matches_the_table_id_not_the_order() {
        let tree = Tree::new();
        let mut host = read_host(&tree, &pack(2, 5).to_le_bytes());
        {
            let file = host.native_file_mut(OBJ).unwrap();
            file.resources.push(ResourceRow {
                id: 1,
                kind: 3,
                path: "a.rpk".into(),
            });
            file.resources.push(ResourceRow {
                id: 2,
                kind: 7,
                path: "b.rpk".into(),
            });
        }
        assert_eq!(read_res_id(&mut host, OBJ), pack(7, 5));
    }

    /// A word whose high half is 0 or `0xffff` names a slot row by its low
    /// half and yields the reference id the row stored.
    #[test]
    fn read_res_id_returns_the_script_row_id() {
        let tree = Tree::new();
        let mut host = read_host(&tree, &1u32.to_le_bytes());
        host.native_file_mut(OBJ).unwrap().slots.push(SlotRow {
            id: 1,
            kind: 5,
            res_id: SCRIPT_ID,
        });
        assert_eq!(read_res_id(&mut host, OBJ), SCRIPT_ID);

        // the same row, addressed by a `0xffff`-high word whose low half is
        // the slot index
        let mut host = read_host(&tree, &(SCRIPT_ID).to_le_bytes());
        host.native_file_mut(OBJ).unwrap().slots.push(SlotRow {
            id: 7,
            kind: 5,
            res_id: SCRIPT_ID,
        });
        assert_eq!(read_res_id(&mut host, OBJ), SCRIPT_ID);
    }

    /// The three misses: a zero word (0, no log), a slot-run miss (0 with
    /// [`UNSAVED_MESSAGE`], an empty list included), and a resource-table miss
    /// (0, *no* log — every jump on that path goes straight to the return).
    #[test]
    fn read_res_id_misses() {
        let tree = Tree::new();
        let mut host = read_host(&tree, &0u32.to_le_bytes());
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert!(host.log().is_empty());

        let mut host = read_host(&tree, &9u32.to_le_bytes());
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert_eq!(host.log(), ["!", UNSAVED_MESSAGE]);

        let mut host = read_host(&tree, &pack(4, 1).to_le_bytes());
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert!(host.log().is_empty());

        // a non-empty slot run that does not match logs too
        let mut host = read_host(&tree, &9u32.to_le_bytes());
        host.native_file_mut(OBJ).unwrap().slots.push(SlotRow {
            id: 1,
            kind: 5,
            res_id: SCRIPT_ID,
        });
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert_eq!(host.log(), ["!", UNSAVED_MESSAGE]);

        // a non-empty resource table that does not match stays silent
        let mut host = read_host(&tree, &pack(4, 1).to_le_bytes());
        host.native_file_mut(OBJ)
            .unwrap()
            .resources
            .push(ResourceRow {
                id: 1,
                kind: 3,
                path: String::new(),
            });
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert!(host.log().is_empty());
    }

    /// Neither native touches the file layer without an open slot.
    #[test]
    fn read_without_an_open_handle_reads_nothing() {
        let tree = Tree::new();
        let mut host = tree.host();
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert!(host.take_events().is_empty());

        let mut host = write_host(&tree);
        host.close(OBJ);
        host.take_events();
        assert_eq!(read_res_id(&mut host, OBJ), 0);
        assert!(host.take_events().is_empty());
    }

    /// The writer's write → reader's read round trip through the file: the
    /// saved id comes back with its kind, the script-side one with its own id.
    #[test]
    fn the_written_words_read_back_as_the_same_ids() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_kind_path(KIND, PATH);
        host.set_res_ref(REF, SAVED_ID, 5);
        host.set_res_ref(REF + 1, SCRIPT_ID, 7);
        write_res_ref(&mut host, OBJ, REF);
        write_res_ref(&mut host, OBJ, REF + 1);
        host.close(OBJ);

        // `close` keeps the body; re-open the same file for reading
        let mut host = tree.host();
        host.set_name(OBJ, NAME);
        assert_eq!(host.open(OBJ, MODE_READ), 1);
        // the tables the exe's `open` would materialise from the footer (I2
        // materialises none): hand them over
        {
            let file = host.native_file_mut(OBJ).unwrap();
            file.resources.push(ResourceRow {
                id: 1,
                kind: KIND,
                path: PATH.into(),
            });
            file.slots.push(SlotRow {
                id: 1,
                kind: 7,
                res_id: SCRIPT_ID,
            });
        }
        assert_eq!(read_res_id(&mut host, OBJ), SAVED_ID);
        assert_eq!(read_res_id(&mut host, OBJ), SCRIPT_ID);
    }

    /// `close` writes the rows into the version-2 footer: the slot-run count
    /// word and run, then the resource count and table, then the total.
    #[test]
    fn close_writes_both_tables_into_the_footer() {
        let tree = Tree::new();
        let mut host = write_host(&tree);
        host.set_kind_path(KIND, PATH);
        host.set_res_ref(REF, SAVED_ID, 5);
        host.set_res_ref(REF + 1, SCRIPT_ID, 5);
        write_res_ref(&mut host, OBJ, REF); // `07 00 01 00`
        write_res_ref(&mut host, OBJ, REF + 1); // `01 00 00 00`
        host.close(OBJ);

        let len = PATH.len() as u32 + 1;
        let mut want = SDAT_HEADER.to_vec();
        want.extend_from_slice(&pack(1, 7).to_le_bytes());
        want.extend_from_slice(&1u32.to_le_bytes());
        want.extend_from_slice(&1u32.to_le_bytes()); // slot count
        want.extend_from_slice(&1u32.to_le_bytes()); // slot id
        want.extend_from_slice(&5u32.to_le_bytes()); // kind 5: no blob
        want.extend_from_slice(&0u32.to_le_bytes());
        want.extend_from_slice(&1u32.to_le_bytes()); // resource count
        want.extend_from_slice(&1u32.to_le_bytes()); // resource entry id
        want.extend_from_slice(&len.to_le_bytes());
        want.extend_from_slice(PATH.as_bytes());
        want.push(0);
        let total = 4 + 12 + 4 + (8 + len);
        want.extend_from_slice(&total.to_le_bytes());
        assert_eq!(tree.read(NAME), want);
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
        assert_eq!(vm.natives.len(), RESID_NATIVE_BINDINGS.len());
        for row in RESID_NATIVE_BINDINGS {
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
        assert_eq!(vm.native_impls.len(), RESID_NATIVE_BINDINGS.len());
        for row in RESID_NATIVE_BINDINGS {
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
    /// the reference word) and dispatches into the thread-local host — the
    /// same host the `File` rows use, so both modules' `install`s combine.
    #[test]
    fn the_bridge_dispatches_write_and_read() {
        let tree = Tree::new();
        reset(&tree.0);
        let mut vm = VmHost::new();
        install(&mut vm);
        file_install(&mut vm);
        with_files(|files| {
            files.set_name(OBJ, NAME);
            files.set_res_ref(REF, SAVED_ID, 5);
            files.set_kind_path(KIND, PATH);
        });

        // `File.open(OBJ, 1)`, then the reference write
        let open_args = [Value::int(MODE_WRITE as i32), Value::reference(OBJ)];
        let open = CallInfo {
            jvm: 0,
            thread: 0,
            args: &open_args,
            class: FILE_CLASS,
            name: "open",
            sig: "(I)I",
        };
        assert_eq!(crate::file::bridge(&mut vm, &open), Some(1));

        let args = [Value::reference(REF), Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "write",
            sig: "(Ljava.util.resource.ResourceRef;)I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(4));
        with_files(|files| {
            let file = files.native_file(OBJ).unwrap();
            assert_eq!(
                file.resources.first().map(|row| (row.id, row.kind)),
                Some((1, KIND))
            );
            assert_eq!(file.resource_count, 1);
        });

        // the read arm, over a fresh read-mode file whose body is that word
        let tree = Tree::new();
        tree.put(NAME, &v2(&pack(1, 7).to_le_bytes()));
        reset(&tree.0);
        with_files(|files| {
            files.set_name(OBJ, NAME);
            assert_eq!(files.open(OBJ, MODE_READ), 1);
            files
                .native_file_mut(OBJ)
                .unwrap()
                .resources
                .push(ResourceRow {
                    id: 1,
                    kind: KIND,
                    path: PATH.to_owned(),
                });
            files.take_events();
        });
        let read_args = [Value::reference(OBJ)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &read_args,
            class: FILE_CLASS,
            name: "readResID",
            sig: "()I",
        };
        assert_eq!(bridge(&mut vm, &info), Some(pack(KIND, 7)));
        with_files(|files| {
            assert_eq!(
                files.take_events(),
                vec![IoEvent::Read {
                    slot: 1,
                    want: 4,
                    got: 4
                }]
            );
        });
    }
}

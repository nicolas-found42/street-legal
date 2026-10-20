//! Differential test for the `File.open` / `File.close` (M1-09 I2) and
//! `File.write(ResourceRef)` / `File.readResID` (M1-09 I5) natives.
//!
//! Replays `re/out/io/open_close.jsonl` and `re/out/io/resid.jsonl`, produced
//! by `re/oracle/io_vectors.py gen`: the REAL exe natives executed under
//! Unicorn on a hand-built `CallInfo` (no JVM boot, no game data). Override the
//! directory with `SLR_IO_VECTORS`. Skips with a note when the files are absent
//! so `cargo test --workspace` stays green without the exe/venv.
//!
//! Oracle boundary (see the harness docstring): the field plumbing
//! (`0x42a690`/`0x42ab50`/`0x42a9e0`), the file layer
//! (`0x54c880`/`0x54caf0`/`0x54cec0`/`0x54cf00`/`0x54ce40`/`0x54cca0`), the
//! script-side list model and, for the I5 rows, the resource-manager
//! descriptor lookup (`0x5383f0`) are hooked and logged; everything between
//! them is the real exe code.
//!
//! Normalisations:
//!
//! - the exe's `getfield`/`getint` events and its `log`/`log_append` texts are
//!   dropped: the port resolves the object's `name`/handle from [`FileHost`]
//!   (module docs) and its diagnostics are asserted here against the row's
//!   `log_append` texts and by the unit tests;
//! - the struct word is an arena pointer in the row and a table word in the
//!   port, so the handle-field store compares as 0 vs non-zero;
//! - the I5 rows' node plumbing (the `0x54f560` allocation, the sub-object
//!   constructor `0x42cec0`, its assignment `0x429060`, the descriptor lookup
//!   `0x5383f0` and the node's `strncpy`) has no port event: the port keeps
//!   the two resource tables as rows, which the row's `struct` carries and
//!   these tests compare instead.
//!
//! Rows that need a hand-built native struct (`close` with a slot other than
//! the first, an unknown mode, or slot 0) are not replayable through the
//! natives alone; `slr_fmt_sdat::store`'s own tests cover every `close_file`
//! arm for them. Two I5 rows are skipped likewise
//! (`write_res_no_descriptor`: the exe reads `[0+8]` when the manager has no
//! descriptor; `read_res_short_read`: its short read leaves the frame word
//! where the port reads its zeroed buffer).

use serde_json::Value;
use slr_calllog::LogRecord;
use slr_io::file::{FileHost, IoEvent, FILE_CLASS, HANDLE_FIELD_INDEX, MODE_READ, MODE_WRITE};
use slr_io::fileio::NATIVE_FILE_NAME_OFFSET;
use slr_io::input::{reset_input, with_input, InputHost};
use slr_io::resid::{
    read_res_id, write_res_ref, ResourceRow, SlotRow, FILE_READ_RES_ID, FILE_WRITE_RES_REF,
};
use slr_vm::host::{CallInfo, VmHost};
use std::path::PathBuf;
use std::sync::atomic::{AtomicUsize, Ordering};

const NAME: &str = "data/cars/gti.car";
const OBJ: u32 = 0x1000;
const REF: u32 = 0x2000;
/// The receiver word the input rows' frames carry.
const INPUT_RECEIVER: u32 = 0x4000;
/// The string word the write rows hand the layer (the exe's own interned word
/// is arbitrary; only the bytes behind it matter).
const WORD: u32 = 0x00AB_CDEF;

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_IO_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/io")
}

fn load(name: &str) -> Option<Vec<Value>> {
    let text = std::fs::read_to_string(vectors_dir().join(name)).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}")));
    }
    Some(rows)
}

fn load_rows() -> Option<Vec<Value>> {
    load("open_close.jsonl")
}

struct Tree(PathBuf);
impl Tree {
    fn new() -> Tree {
        static N: AtomicUsize = AtomicUsize::new(0);
        let p = std::env::temp_dir().join(format!(
            "slr-io-oracle-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        Tree(p)
    }

    fn put(&self, rel: &str, data: &[u8]) {
        let p = self.0.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, data).unwrap();
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn hex(s: &str) -> Vec<u8> {
    (0..s.len() / 2)
        .map(|i| u8::from_str_radix(&s[2 * i..2 * i + 2], 16).unwrap())
        .collect()
}

/// The port-side events with the arena-dependent fields normalised.
fn normalize_port(events: &[IoEvent]) -> Vec<IoEvent> {
    events
        .iter()
        .map(|e| match e {
            IoEvent::SetHandle { value, .. } => IoEvent::SetHandle {
                object: OBJ,
                value: u32::from(*value != 0),
            },
            other => other.clone(),
        })
        .collect()
}

/// One row event, `None` for the plumbing the port does not have.
fn normalize_row(event: &Value) -> Option<IoEvent> {
    let a = event.as_array().expect("event array");
    let u = |i: usize| a[i].as_u64().unwrap() as u32;
    match a[0].as_str().unwrap() {
        "alloc" => Some(IoEvent::AllocStruct),
        "read" => Some(IoEvent::Read {
            slot: u(1),
            want: u(2),
            got: u(3),
        }),
        "write" => Some(IoEvent::Write {
            slot: u(1),
            data: hex(a[3].as_str().unwrap()),
        }),
        "seek" => Some(IoEvent::Seek {
            slot: u(1),
            whence: u(2),
            // negative offsets: `as_u64` would drop them
            offset: a[3].as_i64().unwrap() as i32,
        }),
        "open_read" => Some(IoEvent::Open {
            mode: MODE_READ,
            name: a[1].as_str().unwrap().to_owned(),
        }),
        "open_write" => Some(IoEvent::Open {
            mode: MODE_WRITE,
            name: a[1].as_str().unwrap().to_owned(),
        }),
        "close_slot" => Some(IoEvent::CloseSlot { slot: u(1) }),
        "free" => Some(IoEvent::FreeStruct),
        "strncpy" => {
            assert_eq!(u(1), NATIVE_FILE_NAME_OFFSET as u32);
            Some(IoEvent::Name {
                offset: u(1) as usize,
                name: a[2].as_str().unwrap().to_owned(),
                len: u(3),
            })
        }
        "listfree" => Some(IoEvent::ListFree { offset: u(1) }),
        "setfield" => {
            assert_eq!(u(2), HANDLE_FIELD_INDEX);
            Some(IoEvent::SetHandle {
                object: OBJ,
                value: u32::from(u(3) != 0),
            })
        }
        "getfield" | "getint" | "log" | "log_append" => None,
        other => panic!("unknown event {other}"),
    }
}

fn normalize_row_events(row: &Value) -> Vec<IoEvent> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(normalize_row)
        .collect()
}

fn open_of(row: &Value) -> (u32, Vec<u8>, String) {
    let mode = row["args"]["mode"].as_u64().unwrap() as u32;
    let file = hex(row["args"]["file"].as_str().unwrap_or(""));
    let name = row["args"]["name"].as_str().unwrap_or(NAME).to_owned();
    (mode, file, name)
}

#[test]
fn the_open_rows_replay() {
    let Some(rows) = load_rows() else {
        eprintln!("SKIP: no re/out/io/open_close.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut replayed = 0;
    let opens: Vec<&Value> = rows
        .iter()
        .filter(|r| r["name"].as_str() == Some("File.open"))
        .collect();
    for row in opens {
        assert_eq!(row["exe_addr"].as_u64().unwrap(), 0x0048_5440);
        if row["result"].get("truncated").is_some() {
            // the exe runs into the script-side table loop after a malformed
            // footer; the port stops after the two count words
            continue;
        }
        let (mode, file, name) = open_of(row);
        let tree = Tree::new();
        tree.put(NAME, &file);
        let mut host = FileHost::new(&tree.0);
        host.set_name(OBJ, &name);
        if let Some(reuse) = row["args"].get("reuse_slot") {
            // the exe row is handed a live struct; the port builds the same
            // state with its own `open` and replays the second call.
            assert_eq!(reuse.as_u64().unwrap(), 1, "row {}", row["id"]);
            assert_eq!(host.open(OBJ, mode), 1, "row {} setup", row["id"]);
            host.take_events();
        }

        let got = host.open(OBJ, mode);
        assert_eq!(
            Some(got),
            row["result"]["eax"].as_u64().map(|v| v as u32),
            "row {}",
            row["id"]
        );
        assert_eq!(
            normalize_port(host.events()),
            normalize_row_events(row),
            "row {}",
            row["id"]
        );
        // the stored struct agrees with the exe's, minus the arena fields
        if let Some(want) = row["struct"].as_object() {
            let file = host.native_file(OBJ).expect("handle stored");
            assert_eq!(
                file.slot,
                want["slot"].as_u64().unwrap() as u32,
                "row {}",
                row["id"]
            );
            assert_eq!(
                file.mode,
                want["mode"].as_u64().unwrap() as u32,
                "row {}",
                row["id"]
            );
            assert_eq!(
                file.open_word,
                want["open_word"].as_u64().unwrap() as u32,
                "row {}",
                row["id"]
            );
            assert_eq!(
                file.name(),
                want["name"].as_str().unwrap(),
                "row {}",
                row["id"]
            );
        }
        replayed += 1;
    }
    assert!(replayed >= 5, "replayed only {replayed} open rows");
}

#[test]
fn the_close_rows_replay() {
    let Some(rows) = load_rows() else {
        eprintln!("SKIP: no re/out/io/open_close.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut replayed = 0;
    let mut skipped = 0;
    let closes: Vec<&Value> = rows
        .iter()
        .filter(|r| r["name"].as_str() == Some("File.close"))
        .collect();
    for row in closes {
        assert_eq!(row["exe_addr"].as_u64().unwrap(), 0x0048_5870);
        let slot = row["args"]["slot"].as_u64().unwrap() as u32;
        let mode = row["args"]["mode"].as_u64().unwrap() as u32;
        let missing = row["args"].get("missing").is_some();
        let tree = Tree::new();
        let mut host = FileHost::new(&tree.0);
        host.set_name(OBJ, NAME);

        if !missing {
            // The port builds the struct through its own `open`; the exe row
            // was handed one. Slot 1 (and slot 3, by opening two decoys
            // first) are what the port's own calls can produce.
            if slot == 3 && mode == MODE_WRITE {
                for (obj, decoy) in [(OBJ + 1, "a.dat"), (OBJ + 2, "b.dat")] {
                    host.set_name(obj, decoy);
                    assert_eq!(host.open(obj, MODE_WRITE), 1, "decoy {decoy}");
                }
            } else if slot != 1
                || (mode != MODE_READ && mode != MODE_WRITE)
                || row["args"]["slot_count"].as_u64().unwrap_or(0) != 0
            {
                skipped += 1;
                continue;
            }
            // write mode creates the file on open; read mode needs it present.
            if mode == MODE_READ {
                tree.put(NAME, &hex(row["args"]["file"].as_str().unwrap_or("")));
            }
            assert_eq!(host.open(OBJ, mode), 1, "row {} setup", row["id"]);
        }
        host.take_events();

        host.close(OBJ);
        assert_eq!(
            normalize_port(host.events()),
            normalize_row_events(row),
            "row {}",
            row["id"]
        );
        assert_eq!(host.handle(OBJ), 0, "row {}: handle cleared", row["id"]);
        assert!(
            host.native_file(OBJ).is_none(),
            "row {}: struct freed",
            row["id"]
        );
        replayed += 1;
    }
    assert!(replayed >= 4, "replayed only {replayed} close rows");
    assert!(skipped <= 2, "skipped {skipped} close rows");
}

/// `re/out/io/path.jsonl`: the five delete/exists wrapper rows (I8). Each row
/// calls the exe's wrapper and records its `eax`; the replay calls the port's
/// layer against a scratch tree holding the same file the oracle's probes
/// were told exists.
#[test]
fn the_path_rows_replay() {
    let Some(rows) = load("path.jsonl") else {
        eprintln!("SKIP: no re/out/io/path.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut checked = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let name = row["name"].as_str().expect("name");
        let args = row["args"]["args"].as_array().expect("args");
        let want = row["result"]["eax"].as_u64().expect("eax") as u32;
        let tree = Tree::new();
        // The oracle answered its exists probes from a set of "existing"
        // names; the replay puts exactly that one name on disk, so a row
        // asking about another name sees a genuine absence.
        let existing = row["args"]["existing"]
            .as_str()
            .unwrap_or("data/cars/gti.car");
        tree.put(existing, b"abc");
        let mut host = FileHost::new(tree.0.clone());
        let got = match name {
            "File.exists" => host.exists(args[0].as_str().expect("path")),
            "File.delete" => host.delete(args[0].as_str().expect("path")),
            other => panic!("{id}: unknown row {other}"),
        };
        assert_eq!(got, want, "{id}: {name}");
        checked += 1;
    }
    assert_eq!(checked, 5, "path rows");
}

/// `re/out/io/findfile.jsonl`: the three `java.io.FindFile` rows (I8). The
/// oracle served `FindFirstFileA`/`FindNextFileA`/`FindClose` from a synthetic
/// directory; the replay lays that same directory out on disk and drives the
/// port's `FindHost`.
#[test]
fn the_findfile_rows_replay() {
    use slr_io::findfile::FindHost;

    let Some(rows) = load("findfile.jsonl") else {
        eprintln!("SKIP: no re/out/io/findfile.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut checked = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let args = &row["args"];
        let pattern = args["pattern"].as_str().expect("pattern");
        let flags = args["flags"].as_i64().expect("flags") as i32;
        let want = row["result"]["steps"].as_array().expect("steps result");

        let tree = Tree::new();
        for entry in args["entries"].as_array().expect("entries") {
            let pair = entry.as_array().expect("entry pair");
            let name = pair[0].as_str().expect("entry name");
            let is_dir = pair[1].as_bool().expect("entry dir");
            // The oracle's names are relative to the searched directory, and
            // its patterns carry one (`data/cars`); the replay keeps the same
            // shape so the port's own pattern split does the work.
            let rel = format!(
                "{}/{}",
                pattern.rsplit_once('/').map_or("", |(d, _)| d),
                name
            );
            let path = tree.0.join(rel);
            if is_dir {
                std::fs::create_dir_all(&path).unwrap();
            } else {
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(&path, b"").unwrap();
            }
        }

        let mut host = FindHost::new(tree.0.clone());
        host.set_string(1, pattern.as_bytes());
        let object = 0x1000;
        // Each oracle step is one native call: the exe's own `first` loop
        // advances internally, and so does the port's. The port interns its
        // own words, so compare the bytes behind them (0 = "none").
        let name_of = |host: &FindHost, word: u32| {
            host.string(word)
                .map(|b| String::from_utf8_lossy(b).into_owned())
        };
        assert!(!want.is_empty(), "{id}: the row has steps");
        let mut matched_names = 0;
        for want in want {
            let step = want["step"].as_str().expect("step");
            let want_name = want["name"].as_str();
            let got = match step {
                "first" => Some(host.first(object, 1, flags)),
                "next" => Some(host.next(object)),
                "close" => {
                    host.close(object);
                    None
                }
                other => panic!("{id}: unknown step {other}"),
            };
            if let Some(word) = got {
                let got_name = name_of(&host, word);
                assert_eq!(got_name.as_deref(), want_name, "{id}: {step}");
                if got_name.is_some() {
                    matched_names += 1;
                }
            }
        }
        assert!(
            matched_names > 0 || want.iter().all(|w| w["name"].is_null()),
            "{id}: compared at least one delivered name"
        );
        checked += 1;
    }
    assert_eq!(checked, 7, "findfile rows");
}

#[test]
fn the_binding_rows_are_the_file_natives() {
    let rows = slr_io::FILE_NATIVE_BINDINGS;
    assert_eq!(rows.len(), 12);
    assert_eq!(rows[10].name, "delete");
    assert_eq!(rows[10].sig, "(Ljava.lang.String;)I");
    assert_eq!(rows[10].impl_addr, 0x0048_6150);
    assert_eq!(rows[11].name, "exists");
    assert_eq!(rows[11].sig, "(Ljava.lang.String;)I");
    assert_eq!(rows[11].impl_addr, 0x0048_6170);
    assert_eq!(rows[0].class, FILE_CLASS);
    assert_eq!(rows[0].name, "open");
    assert_eq!(rows[0].sig, "(I)I");
    assert_eq!(rows[0].impl_addr, 0x0048_5440);
    assert_eq!(rows[1].name, "close");
    assert_eq!(rows[1].sig, "()V");
    assert_eq!(rows[1].impl_addr, 0x0048_5870);
    assert_eq!(rows[2].name, "copy");
    assert_eq!(rows[2].sig, "(Ljava.lang.String;Ljava.lang.String;)I");
    assert_eq!(rows[2].impl_addr, 0x0048_60f0);
    assert_eq!(rows[3].name, "move");
    assert_eq!(rows[3].sig, "(Ljava.lang.String;Ljava.lang.String;)I");
    assert_eq!(rows[3].impl_addr, 0x0048_6120);
    // `write`'s `(I)` / `(F)` overloads sit between `close` and
    // `write(String)` (tsv 178-181).
    assert_eq!(rows[4].name, "write");
    assert_eq!(rows[4].sig, "(I)I");
    assert_eq!(rows[4].impl_addr, 0x0048_5b20);
    assert_eq!(rows[5].name, "write");
    assert_eq!(rows[5].sig, "(F)I");
    assert_eq!(rows[5].impl_addr, 0x0048_5bc0);
    assert_eq!(rows[6].class, FILE_CLASS);
    assert_eq!(rows[6].name, "write");
    assert_eq!(rows[6].sig, "(Ljava.lang.String;)I");
    assert_eq!(rows[6].impl_addr, 0x0048_5fd0);
    assert_eq!(rows[7].name, "readInt");
    assert_eq!(rows[7].sig, "()I");
    assert_eq!(rows[7].impl_addr, 0x0048_5b70);
    assert_eq!(rows[8].name, "readFloat");
    assert_eq!(rows[8].sig, "()F");
    assert_eq!(rows[8].impl_addr, 0x0048_5c10);
    assert_eq!(rows[9].name, "readString");
    assert_eq!(rows[9].sig, "()Ljava.lang.String;");
    assert_eq!(rows[9].impl_addr, 0x0048_6060);
}

/// The I5 binding rows: `write(ResourceRef)` and `readResID` under
/// `java.io.File` (tsv 180/184).
#[test]
fn the_binding_rows_are_the_resource_ref_natives() {
    let rows = slr_io::RESID_NATIVE_BINDINGS;
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].class, FILE_CLASS);
    assert_eq!(rows[0].name, "write");
    assert_eq!(rows[0].sig, "(Ljava.util.resource.ResourceRef;)I");
    assert_eq!(rows[0].impl_addr, 0x0048_5c60);
    assert_eq!(rows[1].class, FILE_CLASS);
    assert_eq!(rows[1].name, "readResID");
    assert_eq!(rows[1].sig, "()I");
    assert_eq!(rows[1].impl_addr, 0x0048_5ea0);
}

/// One I5 row event: the file-layer calls only — the node plumbing the port
/// keeps as table rows (`alloc`, `subctor`, `resassign`, `strncpy`), the
/// descriptor lookup (`resdesc`), the field reads (`getint`) and the log
/// appends have no port event.
fn normalize_resid_event(event: &Value) -> Option<IoEvent> {
    let a = event.as_array().expect("event array");
    let u = |i: usize| a[i].as_u64().unwrap() as u32;
    match a[0].as_str().unwrap() {
        "read" => Some(IoEvent::Read {
            slot: u(1),
            want: u(2),
            got: u(3),
        }),
        "write" => Some(IoEvent::Write {
            slot: u(1),
            data: hex(a[3].as_str().unwrap()),
        }),
        "getint" | "log_append" | "alloc" | "subctor" | "resassign" | "resdesc" | "strncpy" => None,
        other => panic!("unknown resid event {other}"),
    }
}

fn normalize_resid_events(row: &Value) -> Vec<IoEvent> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(normalize_resid_event)
        .collect()
}

/// The row's `log_append` texts, i.e. what the exe's log sink holds.
fn resid_log(row: &Value) -> Vec<String> {
    row["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e[0].as_str() == Some("log_append"))
        .map(|e| e[1].as_str().unwrap().to_owned())
        .collect()
}

/// The two tables the exe's row left: `(resources, slots, resource_count,
/// slot_count)`.
fn resid_tables(row: &Value) -> (Vec<ResourceRow>, Vec<SlotRow>, u32, u32) {
    let s = &row["struct"];
    let mut resources = Vec::new();
    for r in s["resources"].as_array().unwrap() {
        resources.push(ResourceRow {
            id: r[0].as_u64().unwrap() as u32,
            kind: r[1].as_u64().unwrap() as u32,
            path: r[2].as_str().unwrap().to_owned(),
        });
    }
    let mut slots = Vec::new();
    for r in s["slots"].as_array().unwrap() {
        slots.push(SlotRow {
            id: r[0].as_u64().unwrap() as u32,
            kind: r[1].as_u64().unwrap() as u32,
            res_id: r[2].as_u64().unwrap() as u32,
        });
    }
    (
        resources,
        slots,
        s["resource_count"].as_u64().unwrap() as u32,
        s["slot_count"].as_u64().unwrap() as u32,
    )
}

/// A host whose `OBJ` carries the row's args as the exe's `S` did: the tables,
/// the kind registry and the `ResourceRef` object. `write` opens the file for
/// writing (the rows always ran on slot 1); `read` opens the row's `file`
/// bytes as a version-2 body for reading.
fn resid_host(row: &Value, tree: &Tree, write: bool) -> Option<FileHost> {
    let args = &row["args"];
    let mut host = FileHost::new(&tree.0);
    host.set_name(OBJ, NAME);
    if write {
        assert_eq!(host.open(OBJ, MODE_WRITE), 1, "row {} setup", row["id"]);
    } else {
        let mut file = [b'S', b'D', b'A', b'T', 0, 1, 2, 0].to_vec();
        file.extend_from_slice(&hex(args["file"].as_str().unwrap_or("")));
        file.extend_from_slice(&0u32.to_le_bytes()); // slot-count word
        file.extend_from_slice(&0u32.to_le_bytes()); // resource-count word
        file.extend_from_slice(&8u32.to_le_bytes()); // total
        tree.put(NAME, &file);
        assert_eq!(host.open(OBJ, MODE_READ), 1, "row {} setup", row["id"]);
    }
    if args.get("slot").and_then(Value::as_u64) == Some(0) {
        // the exe row was handed a struct whose slot word is 0
        host.native_file_mut(OBJ).unwrap().slot = 0;
    }
    host.take_events();
    if args.get("missing").is_some() {
        // the exe row was handed a null handle field
        host.close(OBJ);
        host.take_events();
        return Some(host);
    }
    {
        let file = host.native_file_mut(OBJ).unwrap();
        for r in args
            .get("resources")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            file.resources.push(ResourceRow {
                id: r[0].as_u64().unwrap() as u32,
                kind: r[1].as_u64().unwrap() as u32,
                path: r[2].as_str().unwrap().to_owned(),
            });
        }
        for r in args
            .get("slots")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            file.slots.push(SlotRow {
                id: r[0].as_u64().unwrap() as u32,
                kind: r[1].as_u64().unwrap() as u32,
                res_id: r[2].as_u64().unwrap() as u32,
            });
        }
    }
    for (kind, path) in args
        .get("paths")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        host.set_kind_path(kind.parse().unwrap(), path.as_str().unwrap());
    }
    // The exe's manager (`[0x618d48]`, `0x5383f0`) answers the kind→path
    // descriptors; the port's registry is filled by the game's own
    // registration (`FUN_00538440`), which the port does not run yet, so the
    // row's own `resdesc` answers are handed over.
    for event in row["events"].as_array().unwrap() {
        if event[0].as_str() == Some("resdesc") {
            if let Some(path) = event[3].as_str() {
                host.set_kind_path(event[2].as_u64().unwrap() as u32, path);
            }
        }
    }
    if args.get("null_ref").is_none() {
        host.set_res_ref(
            REF,
            args["id"].as_u64().unwrap_or(0) as u32,
            args["kind"].as_u64().unwrap_or(5) as u32,
        );
    }
    Some(host)
}

/// The I5 `write(ResourceRef)` rows: the word the exe writes through the file
/// layer, the tables it leaves, the counters and its diagnostics.
#[test]
fn the_write_resource_ref_rows_replay() {
    let Some(rows) = load("resid.jsonl") else {
        eprintln!("SKIP: no re/out/io/resid.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut replayed = 0;
    for row in rows
        .iter()
        .filter(|r| r["exe_addr"].as_u64().unwrap() == u64::from(FILE_WRITE_RES_REF))
    {
        if row["result"].get("truncated").is_some() {
            // `write_res_no_descriptor`: the exe reads `[0+8]` when the kind
            // has no manager descriptor; the port uses the id's high half
            assert_eq!(row["id"].as_str().unwrap(), "write_res_no_descriptor");
            continue;
        }
        let tree = Tree::new();
        let mut host = resid_host(row, &tree, true).expect("host");
        let got = write_res_ref(&mut host, OBJ, REF);
        assert_eq!(
            got,
            row["result"]["eax"].as_u64().unwrap() as u32,
            "row {}",
            row["id"]
        );
        assert_eq!(
            host.events(),
            normalize_resid_events(row),
            "row {}",
            row["id"]
        );
        assert_eq!(host.log(), resid_log(row), "row {}", row["id"]);
        let (resources, slots, resource_count, slot_count) = resid_tables(row);
        let Some(file) = host.native_file(OBJ) else {
            // only the null-handle row has no struct to compare
            assert_eq!(row["id"].as_str().unwrap(), "write_res_no_handle");
            assert!(resources.is_empty() && slots.is_empty());
            replayed += 1;
            continue;
        };
        assert_eq!(file.resources, resources, "row {} resources", row["id"]);
        assert_eq!(file.slots, slots, "row {} slots", row["id"]);
        assert_eq!(file.resource_count, resource_count, "row {}", row["id"]);
        assert_eq!(file.slot_count, slot_count, "row {}", row["id"]);
        replayed += 1;
    }
    assert!(
        replayed >= 10,
        "replayed only {replayed} write(ResourceRef) rows"
    );
}

/// The I5 `readResID` rows: the word read back, the tables and the
/// diagnostics.
#[test]
fn the_read_res_id_rows_replay() {
    let Some(rows) = load("resid.jsonl") else {
        eprintln!("SKIP: no re/out/io/resid.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut replayed = 0;
    for row in rows
        .iter()
        .filter(|r| r["exe_addr"].as_u64().unwrap() == u64::from(FILE_READ_RES_ID))
    {
        if row["id"].as_str() == Some("read_res_short_read") {
            // the exe's short read leaves the frame word, the port its zeroed
            // buffer: the id (and so the path taken) differs
            continue;
        }
        let tree = Tree::new();
        let mut host = resid_host(row, &tree, false).expect("host");
        let got = read_res_id(&mut host, OBJ);
        assert_eq!(
            got,
            row["result"]["eax"].as_u64().unwrap() as u32,
            "row {}",
            row["id"]
        );
        assert_eq!(
            host.events(),
            normalize_resid_events(row),
            "row {}",
            row["id"]
        );
        assert_eq!(host.log(), resid_log(row), "row {}", row["id"]);
        replayed += 1;
    }
    assert!(replayed >= 8, "replayed only {replayed} readResID rows");
}

// ---- I7: the Input / MouseCursor / Controller logging stubs ---------------

/// The parameter values one `input.jsonl` row declares, in declaration order,
/// as (descriptor byte, marshalled word).
fn row_params(row: &Value) -> Vec<(char, u32)> {
    row["args"]["params"]
        .as_array()
        .expect("params")
        .iter()
        .map(|p| {
            let pair = p.as_array().expect("param pair");
            let ty = pair[0].as_str().expect("param type");
            (
                ty.chars().next().expect("a type byte"),
                pair[1].as_i64().expect("param word") as u32,
            )
        })
        .collect()
}

/// The `slr-vm` cell a row's parameter becomes (the VM's value model).
fn param_cell(ty: char, word: u32) -> slr_vm::values::Value {
    match ty {
        'F' => slr_vm::values::Value::float(f32::from_bits(word)),
        'L' | '[' => slr_vm::values::Value::reference(word),
        _ => slr_vm::values::Value::int(word as i32),
    }
}

/// The call-log value of a row's parameter, by the canonical hook-layer
/// convention (`docs/call-log.md` §2, `re/hook/hook_native.c`): `F` is the raw
/// `f32` bits, everything else the integer word.
fn param_log_value(ty: char, word: u32) -> slr_calllog::Value {
    match ty {
        'F' => slr_calllog::Value::FloatBits(word),
        _ => slr_calllog::Value::Int(word as i32),
    }
}

/// The 24 `input.jsonl` rows: the exe's own input natives called with the
/// device layer absent. Each row is the constant the port's stub must return,
/// the argument list it must record and the log appends it must make.
/// The I5 `write(String)` / `readString()` rows: the length-prefixed byte
/// stream the exe writes, and the reads + intern the other way. Vectors come
/// from the SDAT harness (`sdat_vectors.py`), which already drives this
/// family with a real string buffer and a hooked interner.
#[test]
fn the_string_rows_replay() {
    let mut replayed = 0;
    for name in ["write_string", "read_string"] {
        let path = vectors_dir()
            .join("../sdat")
            .join(format!("sdat_{name}.jsonl"));
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!(
                "SKIP: no {} (run re/oracle/sdat_vectors.py gen)",
                path.display()
            );
            return;
        };
        for line in text.lines().filter(|l| !l.trim().is_empty()) {
            let row: Value = serde_json::from_str(line).expect("row");
            let args = &row["args"];
            let ctx = format!("{name} row {}", row["id"]);
            let tree = Tree::new();
            let mut host = FileHost::new(&tree.0);
            host.set_name(OBJ, NAME);
            let open_state = args["state"].as_str().unwrap_or("open");
            if open_state == "open" {
                if name == "write_string" {
                    assert_eq!(host.open(OBJ, MODE_WRITE), 1, "{ctx} setup");
                } else {
                    // a row without `input` was handed an empty file, so the
                    // 4-byte length read comes back short
                    tree.put(NAME, &hex(args["input"].as_str().unwrap_or("")));
                    assert_eq!(host.open(OBJ, MODE_READ), 1, "{ctx} setup");
                    // `open` scans the 8-byte header and the footer; the
                    // harness hands `0x486060` a slot at position 0
                    let slot = host.native_file_mut(OBJ).unwrap().slot;
                    host.layer_mut().seek(slot, 0, 0);
                }
            }
            host.take_events();
            let want = row["result"]["eax"].as_u64().unwrap();
            if name == "write_string" {
                // the exe row hands the string word straight to the layer
                // a row without `string_hex` was handed the null string word,
                // which `0x486019` turns into a zero length and no copy
                let word = match args.get("string_hex").and_then(Value::as_str) {
                    Some(h) => {
                        // `string()` holds the C string *without* its NUL; the
                        // native appends the counted NUL (`0x486028` strlen+1)
                        host.set_string(WORD, &hex(h));
                        WORD
                    }
                    None => 0,
                };
                let got = host.write_string(OBJ, word);
                assert_eq!(u64::from(got), want, "{ctx}");
            } else {
                let got = host.read_string(OBJ);
                // the exe word is the harness's canned interner answer; the
                // port mints its own, so compare the bytes behind it
                if want == 0 {
                    assert_eq!(got, 0, "{ctx}: a zero length answers the null word");
                } else {
                    assert_ne!(got, 0, "{ctx}: interned word");
                    let bytes = hex(args["input"].as_str().unwrap_or(""));
                    let len = u32::from_le_bytes(bytes[..4].try_into().unwrap()) as usize;
                    let want_text = &bytes[4..4 + len];
                    let want_text = &want_text[..want_text
                        .iter()
                        .position(|b| *b == 0)
                        .unwrap_or(want_text.len())];
                    assert_eq!(host.string(got), Some(want_text), "{ctx}: interned bytes");
                }
            }
            // the byte stream the row recorded, in order: writes carry the
            // payload, reads carry the byte count, and the read arm's
            // alloc/intern/free triple is what the exe's CRT path implies
            let mut want_events: Vec<(String, u64, Option<Vec<u8>>)> = Vec::new();
            for e in row["events"].as_array().unwrap() {
                let kind = e[0].as_str().unwrap();
                match kind {
                    "w" => want_events.push((
                        "w".into(),
                        e[2].as_str().unwrap().len() as u64 / 2,
                        Some(hex(e[2].as_str().unwrap())),
                    )),
                    "r" => want_events.push(("r".into(), e[2].as_u64().unwrap(), None)),
                    "alloc" => want_events.push(("alloc".into(), e[1].as_u64().unwrap(), None)),
                    "intern" => want_events.push((
                        "intern".into(),
                        0,
                        Some(e[1].as_str().unwrap().as_bytes().to_vec()),
                    )),
                    "free" => want_events.push(("free".into(), 0, None)),
                    other => panic!("{ctx}: unhandled event {other}"),
                }
            }
            let mut got_events: Vec<(String, u64, Option<Vec<u8>>)> = Vec::new();
            for e in host.events() {
                match e {
                    IoEvent::Write { data, .. } => {
                        got_events.push(("w".into(), data.len() as u64, Some(data.clone())))
                    }
                    // the harness logs the *requested* count (`oc.read(buf,n)`),
                    // so compare `want`; `got` is a port-side detail
                    IoEvent::Read { want, .. } => {
                        got_events.push(("r".into(), u64::from(*want), None))
                    }
                    IoEvent::AllocBuffer { len } => {
                        got_events.push(("alloc".into(), u64::from(*len), None))
                    }
                    IoEvent::Intern { data, .. } => {
                        got_events.push(("intern".into(), 0, Some(data.clone())))
                    }
                    IoEvent::FreeBuffer => got_events.push(("free".into(), 0, None)),
                    _ => {}
                }
            }
            assert_eq!(got_events, want_events, "{ctx}: byte events");
            replayed += 1;
        }
    }
    eprintln!("replayed {replayed} string rows");
    assert!(replayed >= 10, "replayed only {replayed} string rows");
}

#[test]
fn the_input_stub_rows_replay() {
    let Some(rows) = load("input.jsonl") else {
        eprintln!("SKIP: no re/out/io/input.jsonl (run re/oracle/io_vectors.py gen)");
        return;
    };
    let mut vm = VmHost::new();
    slr_io::input::install(&mut vm);
    let mut replayed = 0usize;
    for row in &rows {
        let id = row["id"].as_str().unwrap();
        let addr = row["exe_addr"].as_u64().unwrap() as u32;
        let stub = slr_io::input::stub_by_addr(addr).unwrap_or_else(|| panic!("{id}: no stub"));
        assert_eq!(stub.binding.sig, row["sig"].as_str().unwrap(), "row {id}");
        let params = row_params(row);

        // The container the ABI walk reads: [last param .. first, receiver].
        let mut args: Vec<slr_vm::values::Value> = params
            .iter()
            .rev()
            .map(|(ty, word)| param_cell(*ty, *word))
            .collect();
        args.push(slr_vm::values::Value::reference(INPUT_RECEIVER));
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: stub.binding.class,
            name: stub.binding.name,
            sig: stub.binding.sig,
        };

        reset_input();
        let got = slr_io::input::bridge(&mut vm, &info);
        let want = row["result"]["word"].as_u64().map(|w| w as u32);
        let ret = slr_io::input::return_type(stub.binding.sig);
        match ret {
            // The two string rows answer an interned word and the cursor rows
            // null: the exe's row carries an arena word, the port its string
            // table's, so only "null vs non-null" is comparable.
            b'L' | b'[' => assert_eq!(
                got.map(|w| w != 0),
                want.map(|w| w != 0),
                "row {id}: result null-ness"
            ),
            _ => assert_eq!(got, want, "row {id}: result"),
        }

        let records = with_input(InputHost::take_records);
        assert_eq!(records.len(), 1, "row {id}: one record");
        let expected_args: Vec<slr_calllog::Value> = params
            .iter()
            .map(|(ty, word)| param_log_value(*ty, *word))
            .collect();
        match &records[0] {
            LogRecord::NativeCall {
                class,
                name,
                args,
                result,
            } => {
                assert_eq!(class, stub.binding.class, "row {id}: class");
                assert_eq!(name, stub.binding.name, "row {id}: name");
                assert_eq!(args, &expected_args, "row {id}: arguments");
                assert_eq!(result.is_some(), ret != b'V', "row {id}: result presence");
                if ret != b'V' {
                    let word = match result.as_ref().expect("a result") {
                        slr_calllog::Value::FloatBits(bits) => *bits,
                        slr_calllog::Value::Int(word) => *word as u32,
                        other => panic!("row {id}: unexpected result value {other:?}"),
                    };
                    match ret {
                        b'L' | b'[' => assert_eq!(
                            word != 0,
                            want.unwrap_or(0) != 0,
                            "row {id}: result null-ness"
                        ),
                        _ => assert_eq!(Some(word), want, "row {id}: recorded result"),
                    }
                }
            }
            other => panic!("row {id}: not a native call: {other:?}"),
        }

        let logs: Vec<String> = row["logs"]
            .as_array()
            .map(|a| a.iter().map(|v| v.as_str().unwrap().to_owned()).collect())
            .unwrap_or_default();
        assert_eq!(with_input(|h| h.log().to_vec()), logs, "row {id}: log");
        replayed += 1;
    }
    assert!(replayed >= 20, "replayed only {replayed} input rows");
}

/// Full File.move native ABI, with Win32 calls as the oracle boundary.
#[test]
fn the_move_rows_replay() {
    let Some(rows) = load("move.jsonl") else {
        eprintln!("SLR_ORACLE_SKIPPED: the_move_rows_replay");
        eprintln!("SKIP: run re/oracle/io_vectors.py gen --only move_");
        return;
    };
    assert_eq!(rows.len(), 13, "regenerate the full move scenario set");
    for row in rows {
        let id = row["id"].as_str().unwrap();
        let tree = Tree::new();
        for name in row["args"]["dirs"].as_array().unwrap() {
            std::fs::create_dir_all(tree.0.join(name.as_str().unwrap())).unwrap();
        }
        for (name, data) in row["args"]["files"].as_object().unwrap() {
            tree.put(name, &hex(data.as_str().unwrap()));
        }
        slr_io::file::reset(&tree.0);
        let mut vm = VmHost::new();
        slr_io::file::install(&mut vm);
        let sig = "(Ljava.lang.String;Ljava.lang.String;)I";
        let address = vm
            .natives
            .resolve(FILE_CLASS, "move", sig)
            .expect("File.move is installed");
        assert_eq!(address, 0x486120, "{id}: native address");
        assert_eq!(
            row["exe_addr"].as_u64(),
            Some(u64::from(address)),
            "{id}: vector address"
        );
        assert_eq!(row["sig"].as_str(), Some(sig), "{id}: vector descriptor");
        slr_io::file::with_files(|host| {
            if let Some(cache) = row["args"]["cache"].as_array() {
                for item in cache {
                    assert_ne!(
                        host.layer_mut().open_read(item["path"].as_str().unwrap()),
                        0,
                        "{id}: seed handle"
                    );
                }
                for (index, item) in cache.iter().enumerate() {
                    if !item["in_use"].as_bool().unwrap() {
                        host.layer_mut().close(index as u32 + 1);
                    }
                }
            }
            host.set_string(1, row["args"]["src"].as_str().unwrap().as_bytes());
            host.set_string(2, row["args"]["dst"].as_str().unwrap().as_bytes());
        });
        let args = [
            slr_vm::values::Value::reference(2),
            slr_vm::values::Value::reference(1),
            slr_vm::values::Value::reference(OBJ),
        ];
        let got = vm.native_impls[&address](
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: FILE_CLASS,
                name: "move",
                sig,
            },
        );
        let missing_parent_bug = id == "move_missing_parent";
        if missing_parent_bug {
            assert_eq!(
                row["result"]["eax"].as_u64(),
                Some(u32::MAX as u64),
                "original bug returns failure"
            );
            assert_eq!(
                row["result"]["files"]["save/missing/new.bin"], "",
                "original creates empty destination"
            );
        }
        let want_return = if missing_parent_bug {
            Some(0)
        } else {
            row["result"]["eax"].as_u64().map(|word| word as u32)
        };
        assert_eq!(got, want_return, "{id}: return");
        let want_cache = row["result"]["cache"].as_array().unwrap();
        let seeded_cache = row["args"]["cache"].as_array().map_or(0, Vec::len);
        assert_eq!(
            want_cache.len(),
            seeded_cache,
            "{id}: cache observation count"
        );
        slr_io::file::with_files(|host| {
            for (index, want) in want_cache.iter().enumerate() {
                let entry = host.layer().entry(index as u32 + 1).unwrap();
                assert_eq!(
                    entry.handle >= 0,
                    want["handle_live"].as_bool().unwrap(),
                    "{id}: cached handle {index}"
                );
                assert_eq!(
                    entry.in_use,
                    want["in_use"].as_bool().unwrap(),
                    "{id}: slot use flag {index}"
                );
            }
        });
        let (files, dirs) = snapshot_move_tree(&tree.0, &tree.0);
        let want_files = if missing_parent_bug {
            serde_json::json!({"save/missing/new.bin": "00534c52ff"})
        } else {
            row["result"]["files"].clone()
        };
        assert_eq!(
            serde_json::json!(files),
            want_files,
            "{id}: file names and bytes"
        );
        assert_eq!(
            serde_json::json!(dirs),
            row["result"]["dirs"],
            "{id}: directories"
        );
    }
}

/// Full File.copy native ABI, with Win32 calls as the oracle boundary.
#[test]
fn the_copy_rows_replay() {
    let Some(rows) = load("copy.jsonl") else {
        eprintln!("SLR_ORACLE_SKIPPED: the_copy_rows_replay");
        eprintln!("SKIP: run re/oracle/io_vectors.py gen --only copy_");
        return;
    };
    assert_eq!(rows.len(), 17, "regenerate the full copy scenario set");
    let ids: std::collections::BTreeSet<_> =
        rows.iter().map(|row| row["id"].as_str().unwrap()).collect();
    let expected: std::collections::BTreeSet<_> = [
        "copy_success",
        "copy_destination_exists",
        "copy_source_missing",
        "copy_same_name",
        "copy_mixed_slashes",
        "copy_destination_directory",
        "copy_missing_parent",
        "copy_parent_is_file",
        "copy_closed_source_handle",
        "copy_closed_destination_handle",
        "copy_active_source_handle",
        "copy_two_closed_source_handles",
        "copy_unrelated_closed_handle",
        "copy_active_destination_handle",
        "copy_two_closed_destination_handles",
        "copy_active_source_writer",
        "copy_empty_source",
    ]
    .into_iter()
    .collect();
    assert_eq!(ids, expected, "complete copy scenario set");
    for row in rows {
        let id = row["id"].as_str().unwrap();
        assert_eq!(row["name"], "File.copy", "{id}: vector native");
        assert_eq!(row["oracle"], "unicorn", "{id}: oracle provenance");
        let tree = Tree::new();
        for name in row["args"]["dirs"].as_array().unwrap() {
            std::fs::create_dir_all(tree.0.join(name.as_str().unwrap())).unwrap();
        }
        for (name, data) in row["args"]["files"].as_object().unwrap() {
            tree.put(name, &hex(data.as_str().unwrap()));
        }
        slr_io::file::reset(&tree.0);
        let mut vm = VmHost::new();
        slr_io::file::install(&mut vm);
        let sig = "(Ljava.lang.String;Ljava.lang.String;)I";
        let address = vm
            .natives
            .resolve(FILE_CLASS, "copy", sig)
            .expect("File.copy is installed");
        assert_eq!(address, 0x4860f0, "{id}: native address");
        assert_eq!(
            row["exe_addr"].as_u64(),
            Some(u64::from(address)),
            "{id}: vector address"
        );
        assert_eq!(row["sig"].as_str(), Some(sig), "{id}: vector descriptor");
        slr_io::file::with_files(|host| {
            if let Some(cache) = row["args"]["cache"].as_array() {
                for item in cache {
                    let name = item["path"].as_str().unwrap();
                    let slot = if item["mode"].as_u64() == Some(1) {
                        let slot = host.layer_mut().open_write(name);
                        let data = hex(row["args"]["files"][name].as_str().unwrap());
                        assert_eq!(host.layer_mut().write(slot, &data), data.len() as u32);
                        slot
                    } else {
                        host.layer_mut().open_read(name)
                    };
                    assert_ne!(slot, 0, "{id}: seed handle");
                }
                for (index, item) in cache.iter().enumerate() {
                    if !item["in_use"].as_bool().unwrap() {
                        host.layer_mut().close(index as u32 + 1);
                    }
                }
            }
            host.set_string(1, row["args"]["src"].as_str().unwrap().as_bytes());
            host.set_string(2, row["args"]["dst"].as_str().unwrap().as_bytes());
        });
        let args = [
            slr_vm::values::Value::reference(2),
            slr_vm::values::Value::reference(1),
            slr_vm::values::Value::reference(OBJ),
        ];
        let got = vm.native_impls[&address](
            &mut vm,
            &CallInfo {
                jvm: 0,
                thread: 0,
                args: &args,
                class: FILE_CLASS,
                name: "copy",
                sig,
            },
        );
        let want_return = row["result"]["eax"].as_u64().map(|word| word as u32);
        assert_eq!(got, want_return, "{id}: return");
        let want_cache = row["result"]["cache"].as_array().unwrap();
        let seeded_cache = row["args"]["cache"].as_array().map_or(0, Vec::len);
        assert_eq!(
            want_cache.len(),
            seeded_cache,
            "{id}: cache observation count"
        );
        slr_io::file::with_files(|host| {
            for (index, want) in want_cache.iter().enumerate() {
                let entry = host.layer().entry(index as u32 + 1).unwrap();
                assert_eq!(
                    u64::from(entry.mode),
                    want["mode"].as_u64().unwrap(),
                    "{id}: cache mode {index}"
                );
                assert_eq!(
                    entry.handle >= 0,
                    want["handle_live"].as_bool().unwrap(),
                    "{id}: cached handle {index}"
                );
                assert_eq!(
                    entry.in_use,
                    want["in_use"].as_bool().unwrap(),
                    "{id}: slot use flag {index}"
                );
            }
        });
        let (files, dirs) = snapshot_move_tree(&tree.0, &tree.0);
        let want_files = row["result"]["files"].clone();
        assert_eq!(
            serde_json::json!(files),
            want_files,
            "{id}: file names and bytes"
        );
        assert_eq!(
            serde_json::json!(dirs),
            row["result"]["dirs"],
            "{id}: directories"
        );
    }
}

#[test]
fn file_copy_native_overwrites_destination_and_preserves_source() {
    let tree = Tree::new();
    tree.put("source.bin", b"source bytes");
    tree.put("destination.bin", b"old bytes");
    slr_io::file::reset(&tree.0);
    slr_io::file::with_files(|host| {
        host.set_string(1, b"source.bin");
        host.set_string(2, b"destination.bin");
    });
    let mut vm = VmHost::new();
    slr_io::file::install(&mut vm);
    let sig = "(Ljava.lang.String;Ljava.lang.String;)I";
    let addr = vm
        .natives
        .resolve(FILE_CLASS, "copy", sig)
        .expect("copy installed");
    assert_eq!(addr, 0x4860f0);
    let args = [
        slr_vm::values::Value::reference(2),
        slr_vm::values::Value::reference(1),
        slr_vm::values::Value::reference(OBJ),
    ];
    let result = vm.native_impls[&addr](
        &mut vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "copy",
            sig,
        },
    );
    assert_eq!(result, Some(0));
    assert_eq!(
        std::fs::read(tree.0.join("source.bin")).unwrap(),
        b"source bytes"
    );
    assert_eq!(
        std::fs::read(tree.0.join("destination.bin")).unwrap(),
        b"source bytes"
    );
}

fn snapshot_move_tree(
    root: &std::path::Path,
    dir: &std::path::Path,
) -> (std::collections::BTreeMap<String, String>, Vec<String>) {
    let mut files = std::collections::BTreeMap::new();
    let mut dirs = Vec::new();
    for entry in std::fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        if path.is_dir() {
            dirs.push(name);
            let (children, subdirs) = snapshot_move_tree(root, &path);
            files.extend(children);
            dirs.extend(subdirs);
        } else {
            files.insert(
                name,
                std::fs::read(path)
                    .unwrap()
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect(),
            );
        }
    }
    dirs.sort();
    (files, dirs)
}

/// Data-free regression: protecting an existing destination is part of the
/// public File.move contract even when local executable vectors are absent.
#[test]
fn file_move_native_never_replaces_an_existing_destination() {
    let tree = Tree::new();
    tree.put("source.bin", b"source bytes");
    tree.put("destination.bin", b"destination bytes");
    slr_io::file::reset(&tree.0);
    slr_io::file::with_files(|host| {
        host.set_string(1, b"source.bin");
        host.set_string(2, b"destination.bin");
    });
    let mut vm = VmHost::new();
    slr_io::file::install(&mut vm);
    let sig = "(Ljava.lang.String;Ljava.lang.String;)I";
    let address = vm
        .natives
        .resolve(FILE_CLASS, "move", sig)
        .expect("registered File.move");
    let args = [
        slr_vm::values::Value::reference(2),
        slr_vm::values::Value::reference(1),
        slr_vm::values::Value::reference(OBJ),
    ];
    let result = vm.native_impls[&address](
        &mut vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name: "move",
            sig,
        },
    );
    assert_eq!(result, Some(u32::MAX));
    assert_eq!(
        std::fs::read(tree.0.join("source.bin")).unwrap(),
        b"source bytes"
    );
    assert_eq!(
        std::fs::read(tree.0.join("destination.bin")).unwrap(),
        b"destination bytes"
    );
}

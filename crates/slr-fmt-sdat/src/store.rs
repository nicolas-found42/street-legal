//! The exe's SDAT store path, modelled one write at a time.
//!
//! Everything here mirrors machine code of build 606 and is differentially
//! tested against the Unicorn function oracle (`re/oracle/sdat_vectors.py`,
//! consumer `tests/oracle.rs`). The oracle replaces only the file layer
//! (`0x54cf00` write, `0x54cec0` read, `0x54cca0` close), the JPEG-companion
//! savers (`0x48daa0` / `0x48da30`) and the screenshot prepare call
//! (`0x419860`); the real exe code runs above them. The CRT `fwrite` slot
//! table below the file layer is not modelled.
//!
//! - `File.write(int)` / `write(float)` / `readInt()` / `readFloat()`
//!   natives: [`file_write_int`], [`file_write_float`], [`file_read_int`],
//!   [`file_read_float`] (4 raw little-endian bytes through the file layer).
//! - The SDAT JPEG-slot run writer: [`store_slot_run`].
//! - `File.close()`, which writes the SDAT footer (slot-count word, slot run,
//!   resource-count word, resource table, byte total) before closing:
//!   [`close_file`].
//!
//! Footer layout in file-format major version 2 (the only version the stock
//! build writes; the version byte lives at `0x612c76`):
//! `[slot_count][slot run][resource_count]{[id][len][path NUL]}*[total]`
//! where `total = 4 + slot-run bytes + 4 + sum(8 + len)`: the number of
//! bytes from the slot-count word up to (excluding) the total itself.
//! `slot_count` / `resource_count` are plain words kept by the script side
//! (`[file+0x28]` / `[file+8]`); the exe does not recompute them.

/// One file-layer or cleanup call, in the order the exe issues it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Event {
    /// `0x54cf00(slot, buf, len)`.
    Write { slot: u32, data: Vec<u8> },
    /// `0x54cec0(slot, buf, len)`.
    Read { slot: u32, len: u32 },
    /// `0x54cca0(slot)`.
    Close { slot: u32 },
    /// `0x419860(0x80000000, 1.0f, 0, 0)` on a slot's object, issued once
    /// per slot with an object before any byte is written.
    Prepare,
    /// JPEG-companion save (`<name>.<id>` beside the SDAT): kind 5 =
    /// `0x48daa0`, kind 7 = `0x48da30`.
    Companion { kind: u32, name: String },
    /// `0x45fa70` on the list object at `file + offset` (`0x2c`, then `0xc`).
    ListFree { offset: u32 },
    /// `0x54f5b0` free of the native file struct.
    Free,
    /// `0x42a9e0(this, handle field, 0)`: the script object's handle field
    /// is cleared.
    ClearHandleField,
}

/// What the `File` object's handle field resolves to (`0x42ab50`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Handle {
    /// Field lookup failed or the field is not an `I` field: the exe logs
    /// and the native does nothing / returns 0.
    Missing,
    /// Native struct present but its slot word is 0 (not open).
    Closed,
    /// Open file slot (1..31 are valid; the file layer rejects others).
    Open(u32),
}

/// The file layer below the natives (`0x54cf00` / `0x54cec0`).
pub trait FileIo {
    /// Returns what `0x54cf00` returns (the write native passes it through).
    fn write(&mut self, slot: u32, data: &[u8]) -> u32;
    /// Fills `buf`, returns the number of bytes delivered.
    fn read(&mut self, slot: u32, buf: &mut [u8]) -> u32;
}

/// `File.write(int)`: 4 little-endian bytes; returns the file layer's
/// return value, or 0 if the handle is missing/closed.
///
/// exe: FUN_00485b20
pub fn file_write_int<I: FileIo>(io: &mut I, h: Handle, v: u32) -> u32 {
    match h {
        Handle::Open(slot) => io.write(slot, &v.to_le_bytes()),
        _ => 0,
    }
}

/// `File.write(float)`: the float's 4 raw bytes (no conversion; a signalling
/// NaN keeps its payload).
///
/// exe: FUN_00485bc0
pub fn file_write_float<I: FileIo>(io: &mut I, h: Handle, v: f32) -> u32 {
    match h {
        Handle::Open(slot) => io.write(slot, &v.to_bits().to_le_bytes()),
        _ => 0,
    }
}

fn read4<I: FileIo>(io: &mut I, h: Handle) -> Option<[u8; 4]> {
    let Handle::Open(slot) = h else {
        return None;
    };
    let mut buf = [0u8; 4];
    (io.read(slot, &mut buf) == 4).then_some(buf)
}

/// `File.readInt()`. `Some(0)` for a missing handle field (the exe returns
/// the constant 0). `None` when the exe's result is undefined: a closed
/// handle or a short read returns an uninitialised stack word.
///
/// exe: FUN_00485b70
pub fn file_read_int<I: FileIo>(io: &mut I, h: Handle) -> Option<u32> {
    if h == Handle::Missing {
        return Some(0);
    }
    read4(io, h).map(u32::from_le_bytes)
}

/// `File.readFloat()`. `Some(0.0)` for a missing handle field (constant
/// `DAT_005e73cc`, 0.0f); `None` when undefined, as for [`file_read_int`].
///
/// exe: FUN_00485c10
pub fn file_read_float<I: FileIo>(io: &mut I, h: Handle) -> Option<f32> {
    if h == Handle::Missing {
        return Some(0.0);
    }
    read4(io, h).map(|b| f32::from_bits(u32::from_le_bytes(b)))
}

/// A slot's attached object: the exe reads `[+0x58]` (blob pointer) and
/// `[+0x5c]` (blob length).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotObject {
    pub blob: Option<Vec<u8>>,
}

/// One JPEG-slot record of the slot list (without the list sentinel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotNode {
    pub id: u32,
    /// 5 or 7 write extra data; any other value writes only `(id, kind)`.
    pub kind: u32,
    pub object: Option<SlotObject>,
}

/// One resource-table entry of the resource list (without the sentinel).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceNode {
    pub id: u32,
    pub path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StoreError {
    /// A kind-7 blob did not start with `sourcefile <name>`: the exe's
    /// `sscanf` leaves its buffer uninitialised, so behaviour is undefined.
    UnscannableSource { id: u32 },
}

/// Output of [`store_slot_run`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlotRun {
    pub events: Vec<Event>,
    /// The exe's return value: bytes written by the run.
    pub bytes: u32,
}

/// `sscanf(blob, "sourcefile %5s", out)`: the first up-to-5 non-blank bytes
/// after the literal and any whitespace; `None` if the pattern fails.
fn scan_source(blob: &[u8]) -> Option<Vec<u8>> {
    let end = blob.iter().position(|&b| b == 0).unwrap_or(blob.len());
    let s = blob[..end].strip_prefix(b"sourcefile")?;
    let is_ws = |b: &u8| matches!(*b, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r');
    let s = &s[s.iter().position(|b| !is_ws(b)).unwrap_or(s.len())..];
    let tok: Vec<u8> = s
        .iter()
        .take(5)
        .take_while(|b| !is_ws(b))
        .copied()
        .collect();
    (!tok.is_empty()).then_some(tok)
}

fn w32(slot: u32, v: u32) -> Event {
    Event::Write {
        slot,
        data: v.to_le_bytes().to_vec(),
    }
}

/// The SDAT JPEG-slot run writer: per slot `(id, kind)` plus
/// - kind 5: a zero word and the type-5 companion `<name>.<id>`;
/// - kind 7: if the object's blob scans as `sourcefile <tok>` with `<tok>`
///   (5 chars at most) other than `save\` / `save/`, the blob inline
///   (`len`, bytes); otherwise (no object, no blob, or a `save\`/`save/`
///   source) a zero word and the type-7 companion;
/// - any other kind: nothing more.
///
/// Slots with an object get a [`Event::Prepare`] first. `nodes` excludes
/// the list sentinel.
///
/// exe: FUN_00484fa0
pub fn store_slot_run(slot: u32, name: &str, nodes: &[SlotNode]) -> Result<SlotRun, StoreError> {
    let mut events = Vec::new();
    let mut bytes: u32 = 0;
    for n in nodes {
        if n.object.is_some() {
            events.push(Event::Prepare);
        }
    }
    for n in nodes {
        events.push(w32(slot, n.id));
        events.push(w32(slot, n.kind));
        bytes = bytes.wrapping_add(8);
        let companion = |kind| Event::Companion {
            kind,
            name: format!("{name}.{}", n.id as i32),
        };
        match n.kind {
            5 => {
                events.push(w32(slot, 0));
                bytes = bytes.wrapping_add(4);
                events.push(companion(5));
            }
            7 => {
                let blob = n.object.as_ref().and_then(|o| o.blob.as_ref());
                let inline = match blob {
                    Some(b) => {
                        let tok =
                            scan_source(b).ok_or(StoreError::UnscannableSource { id: n.id })?;
                        (tok != b"save\\" && tok != b"save/").then_some(b)
                    }
                    None => None,
                };
                if let Some(b) = inline {
                    events.push(w32(slot, b.len() as u32));
                    events.push(Event::Write {
                        slot,
                        data: b.clone(),
                    });
                    bytes = bytes.wrapping_add(4 + b.len() as u32);
                } else {
                    events.push(w32(slot, 0));
                    bytes = bytes.wrapping_add(4);
                    events.push(companion(7));
                }
            }
            _ => {}
        }
    }
    Ok(SlotRun { events, bytes })
}

/// The native file struct as `File.close()` sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileState {
    /// `[file+0]`: 0 = no open file.
    pub slot: u32,
    /// `[file+0x48]`: 0 = read, 1 = write, other = leave the slot alone.
    pub mode: u32,
    /// File-format major version (`0x612c76`): 1 = legacy footer, 2 = slot
    /// run footer; any other value writes no footer.
    pub version: u8,
    /// `[file+0x4c]`: the file name, base of the companion names.
    pub name: String,
    /// `[file+0x28]`.
    pub slot_count_word: u32,
    /// `[file+8]`.
    pub resource_count_word: u32,
    pub slots: Vec<SlotNode>,
    pub resources: Vec<ResourceNode>,
}

fn resource_entries(slot: u32, resources: &[ResourceNode], events: &mut Vec<Event>) -> u32 {
    let mut sum: u32 = 0;
    for r in resources {
        let len = r.path.len() as u32 + 1; // strlen + NUL
        events.push(w32(slot, r.id));
        events.push(w32(slot, len));
        let mut data = r.path.as_bytes().to_vec();
        data.push(0);
        events.push(Event::Write { slot, data });
        sum = sum.wrapping_add(8).wrapping_add(len);
    }
    sum
}

/// `File.close()`: in write mode, writes the version-dependent footer and
/// closes the slot; then releases the native struct. Returns the ordered
/// events.
///
/// - slot 0: only the cleanup;
/// - mode 0: close; mode 1: footer (version 1: resource entries, their byte
///   sum, the resource-count word; version 2: see the module docs; other
///   versions: none) then close; any other mode: no close;
/// - cleanup: `ListFree{0x2c}`, `ListFree{0xc}`, `Free`,
///   `ClearHandleField`.
///
/// The exe's `total < 8` diagnostic ("Invalid datafile saved") is
/// unreachable in version 2 (the total is at least 8) and not modelled.
///
/// exe: 0x00485870
pub fn close_file(f: &FileState) -> Result<Vec<Event>, StoreError> {
    let mut ev = Vec::new();
    let s = f.slot;
    if s != 0 {
        match f.mode {
            0 => ev.push(Event::Close { slot: s }),
            1 => {
                match f.version {
                    1 => {
                        let sum = resource_entries(s, &f.resources, &mut ev);
                        ev.push(w32(s, sum));
                        ev.push(w32(s, f.resource_count_word));
                    }
                    2 => {
                        ev.push(w32(s, f.slot_count_word));
                        let run = store_slot_run(s, &f.name, &f.slots)?;
                        ev.extend(run.events);
                        ev.push(w32(s, f.resource_count_word));
                        let table = resource_entries(s, &f.resources, &mut ev);
                        let total = 4u32
                            .wrapping_add(run.bytes)
                            .wrapping_add(4)
                            .wrapping_add(table);
                        ev.push(w32(s, total));
                    }
                    _ => {}
                }
                ev.push(Event::Close { slot: s });
            }
            _ => {}
        }
    }
    ev.push(Event::ListFree { offset: 0x2c });
    ev.push(Event::ListFree { offset: 0xc });
    ev.push(Event::Free);
    ev.push(Event::ClearHandleField);
    Ok(ev)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(ev: &[Event]) -> Vec<u8> {
        ev.iter()
            .filter_map(|e| match e {
                Event::Write { data, .. } => Some(data.clone()),
                _ => None,
            })
            .flatten()
            .collect()
    }

    fn companion_only(id: u32) -> SlotNode {
        SlotNode {
            id,
            kind: 7,
            object: None,
        }
    }

    #[test]
    fn scan_source_takes_five_chars_after_whitespace() {
        assert_eq!(
            scan_source(b"sourcefile save\\ x"),
            Some(b"save\\".to_vec())
        );
        assert_eq!(
            scan_source(b"sourcefile\t\n save/zzz"),
            Some(b"save/".to_vec())
        );
        assert_eq!(scan_source(b"sourcefile ab cd"), Some(b"ab".to_vec()));
        assert_eq!(scan_source(b"sourcefile"), None);
        assert_eq!(scan_source(b"JFIF"), None);
    }

    // Hand-derived literals (not recomputed by the code under test): a
    // single companion-only slot writes 12 bytes: id, kind, zero length.
    #[test]
    fn companion_slot_writes_id_kind_zero() {
        let run = store_slot_run(2, "save\\ROC_X_1", &[companion_only(1)]).unwrap();
        assert_eq!(run.bytes, 12);
        assert_eq!(
            data(&run.events),
            [1, 0, 0, 0, 7, 0, 0, 0, 0, 0, 0, 0].to_vec()
        );
        assert_eq!(
            run.events.last(),
            Some(&Event::Companion {
                kind: 7,
                name: "save\\ROC_X_1.1".into()
            })
        );
    }

    #[test]
    fn inline_blob_is_length_prefixed() {
        let blob = b"sourcefile data/x.jpg \xff\xd8".to_vec();
        let n = SlotNode {
            id: 2,
            kind: 7,
            object: Some(SlotObject {
                blob: Some(blob.clone()),
            }),
        };
        let run = store_slot_run(1, "n", &[n]).unwrap();
        assert_eq!(run.bytes, 8 + 4 + blob.len() as u32);
        assert_eq!(run.events[0], Event::Prepare);
        let mut want = vec![2, 0, 0, 0, 7, 0, 0, 0, blob.len() as u8, 0, 0, 0];
        want.extend_from_slice(&blob);
        assert_eq!(data(&run.events), want);
    }

    #[test]
    fn unscannable_blob_is_an_error() {
        let n = SlotNode {
            id: 9,
            kind: 7,
            object: Some(SlotObject {
                blob: Some(b"\xff\xd8\xff\xe0".to_vec()),
            }),
        };
        assert_eq!(
            store_slot_run(1, "n", &[n]),
            Err(StoreError::UnscannableSource { id: 9 })
        );
    }

    #[test]
    fn version2_footer_total_counts_from_slot_count_word() {
        // 13 companion slots + 1 resource "a": 4 + 13*12 + 4 + (8 + 2) = 174
        let f = FileState {
            slot: 3,
            mode: 1,
            version: 2,
            name: "n".into(),
            slot_count_word: 13,
            resource_count_word: 1,
            slots: (1..=13).map(companion_only).collect(),
            resources: vec![ResourceNode {
                id: 1,
                path: "a".into(),
            }],
        };
        let ev = close_file(&f).unwrap();
        let bytes = data(&ev);
        assert_eq!(bytes.len(), 174 + 4);
        assert_eq!(&bytes[bytes.len() - 4..], &174u32.to_le_bytes());
        assert_eq!(&bytes[..4], &13u32.to_le_bytes());
        assert_eq!(
            &ev[ev.len() - 5..],
            &[
                Event::Close { slot: 3 },
                Event::ListFree { offset: 0x2c },
                Event::ListFree { offset: 0xc },
                Event::Free,
                Event::ClearHandleField
            ]
        );
    }

    #[test]
    fn read_only_handles_are_unspecified_not_zero() {
        struct Never;
        impl FileIo for Never {
            fn write(&mut self, _: u32, _: &[u8]) -> u32 {
                panic!("no write")
            }
            fn read(&mut self, _: u32, _: &mut [u8]) -> u32 {
                panic!("no read")
            }
        }
        assert_eq!(file_read_int(&mut Never, Handle::Closed), None);
        assert_eq!(file_read_int(&mut Never, Handle::Missing), Some(0));
        assert_eq!(file_write_int(&mut Never, Handle::Closed, 1), 0);
    }
}

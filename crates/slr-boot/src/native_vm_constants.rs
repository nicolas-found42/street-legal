//! Source407660: raw CONS allocation/deserialization from serialized bytes.
//!
//! This memory projection preserves allocation identity, untouched padding and
//! source store order. It does not claim native scratch, x86 registers, FPU
//! status, or loader chronology; those remain requirements of the ABI bridge.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<StoreObserver>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]
pub(super) fn set_test_store_observer(observer: Option<fn(u32, u32, u32)>) {
    OBSERVER.with(|o| o.set(observer));
}
#[cfg(test)]
fn notify_observer(p: u32, n: u32, v: u32) {
    OBSERVER.with(|o| {
        if let Some(f) = o.get() {
            f(p, n, v)
        }
    });
}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    notify_observer(p, 4, v);
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 4, v)));
    m.w32(p, v);
}
fn byte(m: &mut Space, p: u32, v: u8) {
    #[cfg(test)]
    notify_observer(p, 1, v as u32);
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 1, v as u32)));
    m.w8(p, v);
}
fn checked_end(source: u32, bytes: u32) -> Result<u32, String> {
    source
        .checked_add(bytes)
        .filter(|_| source != 0 && bytes >= 4)
        .ok_or_else(|| "NativeVmConstants: absent/overflowing serialized CONS span".into())
}
fn need(p: u32, n: u32, end: u32) -> Result<(), String> {
    p.checked_add(n)
        .filter(|q| *q <= end)
        .map(|_| ())
        .ok_or_else(|| "NativeVmConstants: truncated serialized CONS entry".into())
}
/// Validate the entire explicit source span before mutating the pool. Trailing
/// bytes and unsupported kinds remain explicit boundaries, never default cells.
pub(super) fn validate(m: &Space, source: u32, bytes: u32) -> Result<(), String> {
    let end = checked_end(source, bytes)?;
    let count = m.r32(source);
    if count > (bytes - 4) / 8 {
        return Err("NativeVmConstants: impossible CONS count".into());
    }
    let mut p = source + 4;
    for _ in 0..count {
        need(p, 4, end)?;
        let kind = m.r32(p);
        p += 4;
        let n = match kind {
            0 => {
                need(p, 4, end)?;
                m.r32(p)
                    .checked_add(5)
                    .ok_or("NativeVmConstants: string span overflow")?
            }
            1 | 2 | 4 => 4,
            3 | 5 | 7 => 8,
            _ => {
                return Err(format!(
                    "NativeVmConstants: unsupported original CONS kind {kind}"
                ))
            }
        };
        need(p, n, end)?;
        if kind == 2 {
            let bits = m.r32(p);
            if bits & 0x7f800000 == 0x7f800000 && bits & 0x007fffff != 0 && bits & 0x00400000 == 0 {
                return Err(
                    "NativeVmConstants: signaling-NaN FLD/FST oracle trust unresolved".into(),
                );
            }
        }
        p += n;
    }
    if p != end {
        return Err("NativeVmConstants: trailing bytes outside declared CONS entries".into());
    }
    Ok(())
}
/// The caller supplies the original already-constructed pool and exact raw CONS
/// span. The return is the consumed source end, not a reconstructed answer map.
/// exe: 0x407660
#[allow(dead_code)]
pub fn deserialize_memory(
    m: &mut Space,
    this: u32,
    source: u32,
    bytes: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    validate(m, source, bytes)?;
    if this == 0
        || m.r32(this) != 0x5e735c
        || m.r32(this + 4) != 0x5e736c
        || m.r32(this + 16) > m.r32(this + 8)
    {
        return Err("NativeVmConstants: missing original ConstantPool/container identity".into());
    }
    let count = m.r32(source);
    let mut p = source + 4;
    for _ in 0..count {
        let kind = m.r32(p);
        p += 4;
        let (size, vt) = match kind {
            0 => (16, 0x5e738c),
            1 => (12, 0x5e739c),
            2 => (12, 0x5e73bc),
            3 => (16, 0x5e73ac),
            4 => (16, 0x5e73f0),
            5 => (20, 0x5e73e0),
            7 => (16, 0x5e73d0),
            _ => unreachable!(),
        };
        let record = a.allocate(m, size)?;
        if record == 0 {
            return Err("NativeVmConstants: original record allocation failed".into());
        }
        if matches!(kind, 1 | 2) {
            write(m, record, 0x5e7400)
        }
        write(m, record, vt);
        match kind {
            0 => {
                let n = m.r32(p);
                write(m, record + 4, 0);
                write(m, record + 12, n);
                let buffer = a.allocate(m, n + 1)?;
                if buffer == 0 {
                    return Err("NativeVmConstants: original string allocation failed".into());
                }
                write(m, record + 8, buffer);
                //551120 copies until its first NUL, without padding the rest.
                let mut i = 0;
                while i < n && m.r8(p + 4 + i) != 0 {
                    byte(m, buffer + i, m.r8(p + 4 + i));
                    i += 1;
                }
                byte(m, buffer + i, 0);
                byte(m, buffer + n, 0);
                p += n + 5;
            }
            1 | 4 => {
                write(m, record + 4, kind);
                write(m, record + 8, m.r32(p));
                p += 4;
            }
            2 => {
                let bits = m.r32(p);
                // Trusted FLD32/FST32 identity; SNaNs fail before mutation.
                let stored = bits;
                write(m, record + 8, stored);
                write(m, record + 4, 2);
                p += 4;
            }
            3 => {
                write(m, record + 8, m.r32(p + 4));
                write(m, record + 4, 3);
                write(m, record + 12, m.r32(p));
                p += 8;
            }
            5 => {
                write(m, record + 12, m.r32(p + 4));
                write(m, record + 4, 5);
                write(m, record + 8, m.r32(p));
                write(m, record + 16, u32::MAX);
                p += 8;
            }
            7 => {
                write(m, record + 12, m.r32(p + 4));
                write(m, record + 4, 7);
                write(m, record + 8, m.r32(p));
                p += 8;
            }
            _ => unreachable!(),
        }
        let handed = m.r32(this + 16);
        let cap = m.r32(this + 8);
        if handed + 1 > cap {
            let new_cap = cap
                .checked_add(handed + 1)
                .ok_or("NativeVmConstants: container capacity overflow")?;
            let table = a.reallocate(m, m.r32(this + 12), cap * 4, new_cap * 4)?;
            write(m, this + 12, table);
            if new_cap < handed {
                write(m, this + 16, new_cap)
            }
            write(m, this + 8, new_cap);
        }
        write(m, m.r32(this + 12) + m.r32(this + 16) * 4, record);
        write(m, this + 16, m.r32(this + 16) + 1);
    }
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    struct Bump;
    impl Allocator for Bump {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            Ok(m.alloc(n.max(1)))
        }
        fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
            let q = m.alloc(n.max(1));
            for i in 0..old.min(n) {
                m.w8(q + i, m.r8(p + i));
            }
            Ok(q)
        }
    }
    fn word(v: &Value) -> u32 {
        u32::try_from(v.as_u64().expect("u32")).unwrap()
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .expect("bytes")
            .iter()
            .map(|v| u8::try_from(word(v)).unwrap())
            .collect()
    }
    #[test]
    fn original_cons_complete_heap_and_ordered_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value =
            serde_json::from_slice(
                &std::fs::read(root.join(
                    "re/out/checkpoints/abi-reconstruction-20261023/constants-original-v3.json",
                ))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(v["schema"], 3);
        assert_eq!(v["fpu_cw"], 127);
        assert_eq!(
            v["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        for name in [
            "native_vm_constants_probe.py",
            "oracle.py",
            "thread_vectors.py",
        ] {
            assert_eq!(
                v["producer_sources"][name],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                )
            );
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 14);
        let inventory = rows
            .iter()
            .map(|r| {
                (
                    r["label"].as_str().unwrap(),
                    word(&r["seed"]),
                    word(&r["record_count"]),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        let mut expected = [
            ("empty", 0),
            ("all_kinds", 7),
            ("float_bits", 9),
            ("signaling_nan_boundary", 2),
            ("raw_strings", 6),
            ("growth", 70),
        ]
        .into_iter()
        .flat_map(|(n, c)| [606, 20261023].map(move |seed| (n, seed, c)))
        .collect::<std::collections::HashSet<_>>();
        let mut stock_inventory = std::collections::HashMap::new();
        for (label, path) in [
            ("stock_Osd", "system/scripts/render/Osd.class"),
            ("stock_Input", "system/scripts/io/Input.class"),
        ] {
            let raw = std::fs::read(root.join("stock").join(path)).unwrap();
            let mut p = 0;
            let mut cons = None;
            while p < raw.len() {
                let n = u32::from_le_bytes(raw[p + 4..p + 8].try_into().unwrap()) as usize;
                let start = p + 8;
                assert!(start + n <= raw.len());
                if &raw[p..p + 4] == b"CONS" {
                    assert!(cons.is_none());
                    cons = Some((
                        start,
                        n,
                        u32::from_le_bytes(raw[start..start + 4].try_into().unwrap()),
                    ));
                }
                p = start + n;
            }
            let (start, n, count) = cons.unwrap();
            expected.insert((label, 606, count));
            stock_inventory.insert(label, (path, start, n, raw));
        }
        for row in rows {
            let label = row["label"].as_str().unwrap();
            assert_eq!(row["unresolved_snan"], label == "signaling_nan_boundary");
            if let Some((path, start, n, raw)) = stock_inventory.get(label) {
                let stock = &row["stock"];
                assert_eq!(stock["path"], *path);
                assert_eq!(stock["module"], 0);
                assert_eq!(word(&stock["offset"]) as usize, *start);
                assert_eq!(word(&stock["bytes"]) as usize, *n);
                assert_eq!(stock["sha256"], format!("{:x}", Sha256::digest(raw)));
                assert_eq!(word(&row["source_bytes"]) as usize, *n);
                let heap = bytes(&row["before"]["bytes"]);
                let offset = (word(&row["source"]) - 0x20000000) as usize;
                assert_eq!(&heap[offset..offset + n], &raw[*start..start + n]);
            } else {
                assert!(row["stock"].is_null());
            }
        }
        assert_eq!(inventory, expected);
        for row in rows {
            let mut m = Space::new();
            let before = bytes(&row["before"]["bytes"]);
            assert_eq!(
                before.len(),
                (word(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b)
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            STORES.with(|s| s.borrow_mut().clear());
            if row["unresolved_snan"] == true {
                let hp = m.heap_ptr();
                assert!(deserialize_memory(
                    &mut m,
                    word(&row["pool"]),
                    word(&row["source"]),
                    word(&row["source_bytes"]),
                    &mut Bump
                )
                .unwrap_err()
                .contains("signaling-NaN"));
                assert_eq!(m.heap_ptr(), hp);
                assert_eq!(STORES.with(|s| s.borrow().len()), 0);
                continue;
            }
            assert_eq!(
                deserialize_memory(
                    &mut m,
                    word(&row["pool"]),
                    word(&row["source"]),
                    word(&row["source_bytes"]),
                    &mut Bump
                )
                .unwrap(),
                word(&row["source"]) + word(&row["source_bytes"])
            );
            assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]));
            let expected = bytes(&row["after"]["bytes"]);
            assert_eq!(expected.len(), (m.heap_ptr() - 0x20000000) as usize);
            let actual = (0..expected.len())
                .map(|i| m.r8(0x20000000 + i as u32))
                .collect::<Vec<_>>();
            assert_eq!(
                actual.iter().zip(&expected).position(|(a, b)| a != b),
                None,
                "complete heap {} {}",
                row["label"],
                row["seed"]
            );
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (word(&s["address"]), word(&s["size"]), word(&s["value"])))
                .collect::<Vec<_>>();
            STORES.with(|s| {
                let actual = s.borrow();
                assert_eq!(actual.len(), expected.len(), "store count {}", row["label"]);
                assert_eq!(
                    actual.iter().zip(&expected).position(|(a, b)| a != b),
                    None,
                    "ordered stores {}",
                    row["label"]
                )
            });
        }
    }
    #[test]
    fn malformed_cons_rejected_before_mutation() {
        let mut m = Space::new();
        let p = m.alloc(20);
        let src = m.alloc(12);
        m.w32(src, 1);
        m.w32(src + 4, 6);
        let hp = m.heap_ptr();
        assert!(deserialize_memory(&mut m, p, src, 12, &mut Bump).is_err());
        assert_eq!(m.heap_ptr(), hp);
        assert_eq!(m.r32(p), 0);
        m.w32(src + 4, 0);
        m.w32(src + 8, u32::MAX);
        assert!(deserialize_memory(&mut m, p, src, 12, &mut Bump).is_err());
        assert_eq!(m.heap_ptr(), hp);
    }
}

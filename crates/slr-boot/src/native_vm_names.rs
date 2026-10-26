//! Raw canonical name storage used by JVM descriptors and variable names.
//!
//! This is the memory projection of41e4d0/41e610 and the real collision/pool
//! helpers. It does not claim their x86 stack/register history. Its returned
//! pointers come from source allocation/interning, never semantic-ID arithmetic
//! or value-payload matching. The native-entry guard remains required.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<fn(u32,u32,u32)>>=const{std::cell::Cell::new(None)};}
#[cfg(test)]
pub(super) fn set_test_store_observer(observer: Option<fn(u32, u32, u32)>) {
    OBSERVER.with(|o| o.set(observer));
}
#[cfg(test)]
fn observe(p: u32, n: u32, v: u32) {
    OBSERVER.with(|o| {
        if let Some(f) = o.get() {
            f(p, n, v)
        }
    });
}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    observe(p, 4, v);
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 4, v)));
    m.w32(p, v);
}
fn byte(m: &mut Space, p: u32, v: u8) {
    #[cfg(test)]
    observe(p, 1, v as u32);
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 1, v as u32)));
    m.w8(p, v);
}
/// Initialize the original table holder. The source caller owns the holder
/// allocation; malloc supplies its actual0x7f40-byte table block.
#[allow(dead_code)]
pub fn construct(m: &mut Space, holder: u32, a: &mut impl Allocator) -> Result<(), String> {
    if holder == 0 {
        return Err("NativeVmNames: missing holder identity".into());
    }
    write(m, holder, 0x5f0984);
    let table = a.allocate(m, 0x7f40)?;
    if table == 0 {
        return Err("NativeVmNames: table allocation failure".into());
    }
    for off in (0x7f14..=0x7f38).step_by(4) {
        write(m, table + off, 0);
    }
    for bucket in 0..0xa97 {
        write(m, table + bucket * 12 + 4, 0);
    }
    write(m, holder + 4, table);
    Ok(())
}
fn pool(m: &mut Space, header: u32, stride: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let free = m.r32(header + 16);
    if free != 0 {
        write(m, header + 16, m.r32(free));
        return Ok(free);
    }
    let handed = m.r32(header + 8);
    if handed == m.r32(header + 12) {
        let blocks = m.r32(header + 4);
        write(m, header + 12, m.r32(header + 12) + 1024);
        write(m, header + 4, blocks + 1);
        let table = a.reallocate(m, m.r32(header), blocks * 4, (blocks + 1) * 4)?;
        write(m, header, table);
        let block = a.allocate(m, 1024 * stride)?;
        write(m, table + blocks * 4, block);
    }
    write(m, header + 8, handed + 1);
    Ok(m.r32(m.r32(header) + (handed / 1024) * 4) + (handed % 1024) * stride)
}
/// Canonicalize source descriptor/name bytes. The original byte-buffer address
/// is returned; equal names share that address because41e610 itself interns.
/// Inputs must arrive in original loader construction order, with all names
/// (not merely those a later native happens to request). This bounded seam
/// rejects embedded NUL/negative lengths and malformed pool/header contexts.
#[allow(dead_code)]
pub fn intern(
    m: &mut Space,
    holder: u32,
    source: u32,
    len: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let table = m.r32(holder.wrapping_add(4));
    if holder == 0
        || source == 0
        || table == 0
        || len > 0x100000
        || m.r8(source + len) != 0
        || (0..len).any(|i| m.r8(source + i) == 0)
        || [table + 0x7f14, table + 0x7f28]
            .iter()
            .any(|p| m.r32(p + 8) > m.r32(p + 12) || m.r32(p + 12) > i32::MAX as u32 - 1024)
    {
        return Err("NativeVmNames: missing/malformed source name/table context".into());
    }
    let contents = (0..len).map(|i| m.r8(source + i)).collect::<Vec<_>>();
    let hash = slr_vm::strings::Strings::hash(&contents);
    let bucket = table + (hash % 0xa97) * 12;
    let mut node = bucket;
    if m.r32(bucket + 4) != 0 {
        let mut seen = std::collections::HashSet::new();
        while node != 0 {
            if !seen.insert(node) {
                return Err("NativeVmNames: cyclic source name table".into());
            }
            if m.r32(node) == hash {
                write(m, table + 0x7f3c, node);
                let slot = m.r32(node + 4);
                let record = m.r32(slot);
                if slot == 0 || record == 0 {
                    return Err("NativeVmNames: null source name record".into());
                }
                let buffer = m.r32(record + 4);
                if m.r32(record + 8) == len
                    && (0..=len).all(|i| m.r8(buffer + i) == m.r8(source + i))
                {
                    write(m, record + 12, m.r32(record + 12).wrapping_add(1));
                    return Ok(buffer);
                }
            }
            node = m.r32(node + 8);
        }
    }
    let record = a.allocate(m, 20)?;
    if record == 0 {
        return Err("NativeVmNames: record allocation failure".into());
    }
    for off in [4, 8, 12, 16] {
        write(m, record + off, 0);
    }
    write(m, record, 0x5f0980);
    let occupied = m.r32(bucket + 4) != 0;
    let node = if occupied {
        pool(m, table + 0x7f14, 12, a)?
    } else {
        bucket
    };
    write(m, node, hash);
    let slot = pool(m, table + 0x7f28, 4, a)?;
    write(m, slot, record);
    write(m, node + 4, slot);
    if occupied {
        write(m, node + 8, m.r32(bucket + 8));
        write(m, bucket + 8, node);
    } else {
        write(m, bucket + 8, 0);
    }
    let buffer = a.allocate(m, len + 1)?;
    write(m, record + 4, buffer);
    for (i, b) in contents.into_iter().enumerate() {
        byte(m, buffer + i as u32, b);
    }
    byte(m, buffer + len, 0);
    byte(m, buffer + len, 0);
    write(m, record + 8, len);
    write(m, record + 12, 1);
    write(m, record + 16, 0);
    Ok(buffer)
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
        fn reallocate(
            &mut self,
            m: &mut Space,
            old: u32,
            old_n: u32,
            n: u32,
        ) -> Result<u32, String> {
            let p = m.alloc(n.max(1));
            for i in 0..old_n.min(n) {
                m.w8(p + i, m.r8(old + i));
            }
            Ok(p)
        }
    }
    fn word(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(word(v)).unwrap())
            .collect()
    }
    #[test]
    fn original_canonical_names_match_complete_heap_and_all_collision_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-names-original.json"))
                .expect("run native_vm_names_probe.py"),
        )
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["fpu_cw"], 0x7f);
        assert_eq!(
            v["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        let sources = v["producer_sources"].as_object().unwrap();
        assert_eq!(
            sources
                .keys()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            ["native_vm_names_probe.py", "oracle.py", "thread_vectors.py"]
                .into_iter()
                .collect()
        );
        for (name, digest) in sources {
            assert_eq!(
                digest,
                &Value::String(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                ))
            );
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 5);
        let inventory = rows
            .iter()
            .map(|r| {
                (
                    r["name"].as_str().unwrap(),
                    r["inputs"].as_array().unwrap().len(),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inventory,
            [
                ("primitive_and_repeat", 7),
                ("same_hash_same_length", 6),
                ("same_hash_different_length", 4),
                ("stock_names", 6),
                ("both_pool_growth", 1030)
            ]
            .into_iter()
            .collect()
        );
        for row in rows {
            let mut m = Space::new();
            let before = bytes(&row["before"]["bytes"]);
            assert_eq!(
                before.len(),
                (word(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b);
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            STORES.with(|s| s.borrow_mut().clear());
            construct(&mut m, word(&row["holder"]), &mut Bump).unwrap();
            for input in row["inputs"].as_array().unwrap() {
                assert_eq!(
                    intern(
                        &mut m,
                        word(&row["holder"]),
                        word(&input["source"]),
                        word(&input["length"]),
                        &mut Bump
                    )
                    .unwrap(),
                    word(&input["result"]),
                    "{} {}",
                    row["name"],
                    input["text"]
                );
            }
            let expected = bytes(&row["after"]["bytes"]);
            assert_eq!(
                expected.len(),
                (word(&row["after"]["heap_ptr"]) - 0x20000000) as usize
            );
            assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]));
            let actual = (0..expected.len())
                .map(|i| m.r8(0x20000000 + i as u32))
                .collect::<Vec<_>>();
            assert_eq!(actual, expected, "complete heap {}", row["name"]);
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (word(&s["address"]), word(&s["size"]), word(&s["value"])))
                .collect::<Vec<_>>();
            STORES.with(|s| {
                let actual = s.borrow();
                assert_eq!(
                    actual.len(),
                    expected.len(),
                    "ordered store count {}",
                    row["name"]
                );
                let diffs = actual
                    .iter()
                    .zip(&expected)
                    .enumerate()
                    .filter(|(_, (a, b))| a != b)
                    .take(8)
                    .collect::<Vec<_>>();
                assert!(
                    diffs.is_empty(),
                    "all ordered stores {} {diffs:x?}",
                    row["name"]
                );
            });
        }
    }
}

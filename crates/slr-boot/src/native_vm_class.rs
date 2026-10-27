//! Physical class403a90 memory, from actual module/CONS identity to JVM cache.
//! Native caller scratch/register history and original loader chronology remain
//! separate requirements; this module never substitutes semantic class IDs.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
fn observe(p: u32, n: u32, v: u32) {
    STORES.with(|s| s.borrow_mut().push((p, n, v)));
}
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
    observe(p, 4, v);
    m.w32(p, v)
}
fn pool(m: &mut Space, h: u32, stride: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let free = m.r32(h + 16);
    if free != 0 {
        write(m, h + 16, m.r32(free));
        return Ok(free);
    }
    let handed = m.r32(h + 8);
    if handed == m.r32(h + 12) {
        let blocks = m.r32(h + 4);
        write(m, h + 12, m.r32(h + 12) + 64);
        write(m, h + 4, blocks + 1);
        let table = a.reallocate(m, m.r32(h), blocks * 4, (blocks + 1) * 4)?;
        write(m, h, table);
        let block = a.allocate(m, 64 * stride)?;
        write(m, table + blocks * 4, block);
    }
    write(m, h + 8, handed + 1);
    Ok(m.r32(m.r32(h) + (handed / 64) * 4) + (handed % 64) * stride)
}
fn class_name(m: &Space, module: u32) -> Result<Vec<u8>, String> {
    let constants = m.r32(module.wrapping_add(0x24));
    let class = m.r32(module.wrapping_add(0x3c));
    if module == 0
        || constants == 0
        || m.r32(constants) != 0x5e735c
        || class >= m.r32(constants + 16)
    {
        return Err("NativeVmClass: missing module/CONS Class identity".into());
    }
    let record = m.r32(m.r32(constants + 12) + class * 4);
    if record == 0 || m.r32(record + 4) != 4 {
        return Err("NativeVmClass: module class index is not raw CONS Class".into());
    }
    let string = m.r32(record + 8);
    if string >= m.r32(constants + 16) {
        return Err("NativeVmClass: raw class-name string index out of range".into());
    }
    let record = m.r32(m.r32(constants + 12) + string * 4);
    if record == 0
        || m.r32(record + 4) != 0
        || m.r32(record + 8) == 0
        || m.r32(record + 12) > 0x100000
    {
        return Err("NativeVmClass: class-name record is not raw CONS Str".into());
    }
    let source = m.r32(record + 8);
    let n = m.r32(record + 12);
    let mut name = Vec::new();
    for i in 0..=n {
        let b = m.r8(source + i);
        if b == 0 {
            return Ok(name);
        }
        name.push(b)
    }
    Err("NativeVmClass: unterminated raw class name".into())
}
/// exe: 0x403a90
/// `scratch` is the caller's actual512-byte local buffer identity. Allocation
/// and interning consume the source graph; no name/result lookup table is used.
#[allow(dead_code)]
pub fn register_memory(
    m: &mut Space,
    this: u32,
    jvm: u32,
    module: u32,
    scratch: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let name = class_name(m, module)?;
    let cache = m.r32(jvm.wrapping_add(8));
    let holder = m.r32(jvm.wrapping_add(0x68));
    let table = m.r32(holder.wrapping_add(4));
    if this == 0
        || jvm == 0
        || cache == 0
        || holder == 0
        || table == 0
        || scratch == 0
        || scratch.checked_add(512).is_none()
        || name.len() > 200
    {
        return Err("NativeVmClass: missing class/JVM/cache/name/local-buffer identity or unsupported long descriptor".into());
    }
    for h in [cache + 0x1800, cache + 0x1814] {
        if m.r32(h + 8) > m.r32(h + 12)
            || m.r32(h + 12) > i32::MAX as u32 - 64
            || m.r32(h + 12) != m.r32(h + 4) * 64
        {
            return Err("NativeVmClass: malformed original class cache pool".into());
        }
    }
    for off in [0x19c, 0x1b0] {
        for word in 0..5 {
            write(m, this + off + word * 4, 0)
        }
    }
    for slot in 0..32 {
        write(m, this + 0x20 + slot * 12, 0)
    }
    for (off, value) in [
        (0, 0x5e7340),
        (4, module),
        (0x14, m.r32(module + 0x24)),
        (0x18, m.r32(module + 0x60)),
        (0x10, m.r32(module + 0x38)),
        (0xc, jvm),
        (0x1d4, 0),
    ] {
        write(m, this + off, value)
    }
    let arrays = name.iter().take_while(|b| **b == b'[').count();
    let mut descriptor = name[..arrays].to_vec();
    let tail = &name[arrays..];
    if arrays > 0 && matches!(tail.first(), Some(b'I' | b'F')) {
        descriptor.extend_from_slice(tail)
    } else {
        descriptor.push(b'L');
        descriptor.extend_from_slice(tail);
        descriptor.push(b';')
    }
    for (i, b) in descriptor
        .iter()
        .copied()
        .chain(std::iter::once(0))
        .enumerate()
    {
        m.w8(scratch + i as u32, b)
    }
    let descriptor_cell = a.allocate(m, 4)?;
    if descriptor_cell == 0 {
        return Err("NativeVmClass: descriptor cell allocation failed".into());
    }
    let canonical = super::names::intern(m, holder, scratch, descriptor.len() as u32, a)?;
    write(m, descriptor_cell, canonical);
    write(m, this + 8, descriptor_cell);
    write(m, this + 0x1d0, 0);
    write(m, this + 0x1cc, 0);
    let hash = name
        .iter()
        .fold(0u32, |sum, b| sum.wrapping_add(*b as i8 as i32 as u32))
        .wrapping_mul(37)
        .wrapping_add((name.len() as u32) * 7);
    let bucket = cache + (hash & 511) * 12;
    if m.r32(bucket + 4) == 0 {
        write(m, bucket, hash);
        let slot = pool(m, cache + 0x1814, 4, a)?;
        write(m, slot, this);
        write(m, bucket + 4, slot);
        write(m, bucket + 8, 0);
    } else {
        let node = pool(m, cache + 0x1800, 12, a)?;
        write(m, node, hash);
        let slot = pool(m, cache + 0x1814, 4, a)?;
        write(m, slot, this);
        write(m, node + 4, slot);
        write(m, node + 8, m.r32(bucket + 8));
        write(m, bucket + 8, node);
    }
    Ok(this)
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
                m.w8(q + i, m.r8(p + i))
            }
            Ok(q)
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
    struct Observer;
    impl Drop for Observer {
        fn drop(&mut self) {
            super::super::names::set_test_store_observer(None)
        }
    }
    fn declared_names(label: &str) -> Vec<String> {
        match label {
            "simple_duplicate" => ["Probe", "Other", "Probe", ""]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            "duplicate_records" => vec!["Probe".into(); 12],
            "array_descriptors" => [
                "[I",
                "[[F",
                "[java.game.Player",
                "[[java.game.Car",
                "java.render.Osd",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            "cache_growth" => {
                let mut bytes = *b"aabbccdd";
                let mut names = Vec::new();
                loop {
                    names.push(format!("Probe.{}", std::str::from_utf8(&bytes).unwrap()));
                    if names.len() == 70 {
                        break;
                    }
                    let i = (0..bytes.len() - 1)
                        .rev()
                        .find(|i| bytes[*i] < bytes[*i + 1])
                        .unwrap();
                    let j = (i + 1..bytes.len())
                        .rev()
                        .find(|j| bytes[*j] > bytes[i])
                        .unwrap();
                    bytes.swap(i, j);
                    bytes[i + 1..].reverse();
                }
                let hashes = names
                    .iter()
                    .map(|n| n.bytes().map(u32::from).sum::<u32>() * 37 + n.len() as u32 * 7)
                    .collect::<std::collections::HashSet<_>>();
                assert_eq!(hashes.len(), 1);
                names
            }
            _ => panic!("undeclared class sequence {label}"),
        }
    }
    #[test]
    fn original_physical_class_complete_heap_and_ordered_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) = crate::oracle_fixture::file(
            &root.join("re/out/checkpoints/abi-reconstruction-20261023/class-original-v2.json"),
        ) else {
            return;
        };
        let v: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        assert_eq!(v["schema"], 2);
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
        for name in ["native_vm_class_probe.py", "oracle.py", "thread_vectors.py"] {
            assert_eq!(
                v["producer_sources"][name],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                )
            )
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        let inventory = rows
            .iter()
            .map(|r| {
                (
                    r["label"].as_str().unwrap(),
                    word(&r["seed"]),
                    r["inputs"].as_array().unwrap().len(),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inventory,
            [
                ("simple_duplicate", 4),
                ("cache_growth", 70),
                ("duplicate_records", 12),
                ("array_descriptors", 5)
            ]
            .into_iter()
            .flat_map(|(n, c)| [606, 20261023].map(move |seed| (n, seed, c)))
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
                m.w8(0x20000000 + i as u32, b)
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            STORES.with(|s| s.borrow_mut().clear());
            super::super::names::set_test_store_observer(Some(observe));
            let _observer = Observer;
            let declared = declared_names(row["label"].as_str().unwrap());
            assert_eq!(
                row["inputs"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|i| i["name"].as_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
                declared
            );
            for (input, name) in row["inputs"].as_array().unwrap().iter().zip(&declared) {
                assert_eq!(
                    class_name(&m, word(&input["module"])).unwrap(),
                    name.as_bytes()
                );
                assert_eq!(
                    register_memory(
                        &mut m,
                        word(&input["this"]),
                        word(&row["jvm"]),
                        word(&input["module"]),
                        word(&input["descriptor_scratch"]),
                        &mut Bump
                    )
                    .unwrap(),
                    word(&input["this"])
                );
            }
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
                let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
                assert!(
                    first.is_none(),
                    "ordered stores {} first {:?} actual {:?} expected {:?}",
                    row["label"],
                    first,
                    first.map(|i| actual[i]),
                    first.map(|i| expected[i])
                );
            });
        }
    }
    #[test]
    fn wrong_class_metadata_rejects_before_writes() {
        let mut m = Space::new();
        let this = m.alloc(512);
        let jvm = m.alloc(256);
        let module = m.alloc(128);
        let hp = m.heap_ptr();
        STORES.with(|s| s.borrow_mut().clear());
        assert!(register_memory(&mut m, this, jvm, module, 0x30000000, &mut Bump).is_err());
        assert_eq!(m.heap_ptr(), hp);
        assert_eq!(STORES.with(|s| s.borrow().len()), 0)
    }
}

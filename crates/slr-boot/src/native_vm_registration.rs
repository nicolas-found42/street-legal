//! Source416b00 native registry and41dc30 lazy signature ownership.
//! Memory projection only: source caller scratch/GPR execution remains open.
//! Registration inserts duplicate native records; it does not deduplicate them.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[path = "native_vm_registration_context.rs"]
mod context;
pub use context::{begin_catalog, register, resources, secondary_catalog, RegistryBootstrap};
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! {static OBSERVER:std::cell::Cell<Option<StoreObserver>>=const {std::cell::Cell::new(None)};}
#[cfg(test)]
pub(crate) fn set_test_store_observer(observer: Option<StoreObserver>) {
    OBSERVER.with(|slot| slot.set(observer));
}
#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    note(p, 4, v);
    m.w32(p, v)
}
#[cfg(test)]
fn note(p: u32, n: u32, v: u32) {
    STORES.with(|s| s.borrow_mut().push((p, n, v)));
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(p, n, v);
        }
    });
}
fn bytes(m: &Space, p: u32) -> Result<Vec<u8>, String> {
    if p == 0 {
        return Err("NativeVmRegistration: missing source bytes".into());
    }
    let mut out = Vec::new();
    for i in 0..0x100000 {
        let b = m.r8(p + i);
        if b == 0 {
            return Ok(out);
        }
        out.push(b)
    }
    Err("NativeVmRegistration: unterminated source bytes".into())
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
        if table == 0 {
            return Err("NativeVmRegistration: table allocation failure".into());
        }
        write(m, h, table);
        let block = a.allocate(m, 64 * stride)?;
        if block == 0 {
            return Err("NativeVmRegistration: block allocation failure".into());
        }
        write(m, table + blocks * 4, block);
    }
    write(m, h + 8, handed + 1);
    Ok(m.r32(m.r32(h) + (handed / 64) * 4) + (handed % 64) * stride)
}
/// The JVM/name-holder/registry and three byte-string pointers must originate
/// in the actual caller. `implementation` is retained data, never invoked here.
#[allow(dead_code)]
pub fn register_memory(
    m: &mut Space,
    jvm: u32,
    class: u32,
    method: u32,
    signature: u32,
    implementation: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let class_bytes = bytes(m, class)?;
    let method_bytes = bytes(m, method)?;
    let signature_bytes = bytes(m, signature)?;
    let registry = m.r32(jvm.wrapping_add(12));
    let holder = m.r32(jvm.wrapping_add(0x68));
    if jvm == 0 || registry == 0 || holder == 0 || m.r32(holder + 4) == 0 {
        return Err("NativeVmRegistration: missing original JVM registry/name holder".into());
    }
    for h in [registry + 0x600, registry + 0x614] {
        if m.r32(h + 8) > m.r32(h + 12)
            || m.r32(h + 12) > i32::MAX as u32 - 64
            || m.r32(h + 12) != m.r32(h + 4) * 64
        {
            return Err("NativeVmRegistration: malformed original registry pool".into());
        }
    }
    let record = a.allocate(m, 24)?;
    if record == 0 {
        return Err("NativeVmRegistration: record allocation failure".into());
    }
    write(m, record, 0x5f08fc);
    write(m, record + 4, jvm);
    let canonical = super::names::intern(m, holder, class, class_bytes.len() as u32, a)?;
    write(m, record + 12, canonical);
    let canonical = super::names::intern(m, holder, method, method_bytes.len() as u32, a)?;
    write(m, record + 16, canonical);
    let descriptor = a.allocate(m, 12)?;
    if descriptor == 0 {
        return Err("NativeVmRegistration: signature allocation failure".into());
    }
    write(m, descriptor + 8, 0);
    write(m, descriptor, jvm);
    let canonical = super::names::intern(m, holder, signature, signature_bytes.len() as u32, a)?;
    write(m, descriptor + 4, canonical);
    write(m, record + 20, descriptor);
    write(m, record + 8, implementation);
    let sum = |data: &[u8]| {
        data.iter()
            .fold(0u32, |n, b| n.wrapping_add(*b as i8 as i32 as u32))
    };
    let hash = sum(&method_bytes)
        .wrapping_mul(73)
        .wrapping_add(sum(&class_bytes));
    let bucket = registry + (hash & 127) * 12;
    if m.r32(bucket + 4) == 0 {
        write(m, bucket, hash);
        let slot = pool(m, registry + 0x614, 4, a)?;
        write(m, slot, record);
        write(m, bucket + 4, slot);
        write(m, bucket + 8, 0);
    } else {
        let node = pool(m, registry + 0x600, 12, a)?;
        write(m, node, hash);
        let slot = pool(m, registry + 0x614, 4, a)?;
        write(m, slot, record);
        write(m, node + 4, slot);
        write(m, node + 8, m.r32(bucket + 8));
        write(m, bucket + 8, node);
    }
    Ok(1)
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
        v.as_u64().unwrap().try_into().unwrap()
    }
    fn data(v: &Value) -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| word(v).try_into().unwrap())
            .collect()
    }
    #[test]
    fn original_native_registration_preserves_duplicate_records_collisions_growth_and_signed_names()
    {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) = crate::oracle_fixture::file(
            &root.join("re/out/boot/native-vm-registration-original-attempt1.json"),
        ) else {
            return;
        };
        let v: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
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
        assert_eq!(sources.len(), 3);
        for name in [
            "native_vm_registration_probe.py",
            "oracle.py",
            "thread_vectors.py",
        ] {
            assert_eq!(
                sources[name],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                )
            )
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 6);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(
                row["label"],
                ["basic_duplicate", "collision_growth", "empty_signed"][i / 2]
            );
            assert_eq!(row["seed"], [606, 20261023][i % 2]);
            let inputs = row["inputs"].as_array().unwrap();
            assert_eq!(inputs.len(), [3, 70, 3][i / 2]);
            // Independently pin bounded cases; captured inputs cannot silently
            // delete duplicate/growth/signed-byte obligations.
            for (n, item) in inputs.iter().enumerate() {
                assert_eq!(word(&item["implementation"]), 0x470000 + n as u32 * 16);
                if i / 2 == 0 {
                    assert_eq!(
                        data(&item["class_bytes"]),
                        [
                            b"java.io.Input".as_slice(),
                            b"java.render.Osd",
                            b"java.io.Input"
                        ][n]
                    );
                    assert_eq!(
                        data(&item["method_bytes"]),
                        [b"lastKey".as_slice(), b"animate", b"lastKey"][n]
                    );
                    assert_eq!(
                        data(&item["signature_bytes"]),
                        [b"()I".as_slice(), b"()V", b"()I"][n]
                    );
                }
                if i / 2 == 1 {
                    let name = data(&item["class_bytes"]);
                    assert_eq!(&name[..6], b"Probe.");
                    let mut tail = name[6..].to_vec();
                    tail.sort();
                    assert_eq!(tail, b"aabbccdd");
                    assert_eq!(data(&item["method_bytes"]), b"m");
                    assert_eq!(data(&item["signature_bytes"]), b"(IFLjava.lang.String;)[I");
                }
                if i / 2 == 2 {
                    assert_eq!(
                        data(&item["class_bytes"]),
                        [b"".as_slice(), b"\x80\xff", b"x"][n]
                    );
                    assert_eq!(
                        data(&item["method_bytes"]),
                        [b"".as_slice(), b"\xfe", b"\x80"][n]
                    );
                    assert_eq!(
                        data(&item["signature_bytes"]),
                        [b"()V".as_slice(), b"()I", b"()F"][n]
                    );
                }
            }
            if i / 2 == 1 {
                let mut tail = *b"aabbccdd";
                for (n, item) in inputs.iter().enumerate() {
                    assert_eq!(
                        data(&item["class_bytes"]),
                        [b"Probe.".as_slice(), &tail].concat()
                    );
                    if n + 1 < inputs.len() {
                        let pivot = (0..7).rev().find(|j| tail[*j] < tail[*j + 1]).unwrap();
                        let swap = (pivot + 1..8)
                            .rev()
                            .find(|j| tail[*j] > tail[pivot])
                            .unwrap();
                        tail.swap(pivot, swap);
                        tail[pivot + 1..].reverse();
                    }
                }
            }
            let mut m = Space::new();
            let before = data(&row["before"]["bytes"]);
            let base = word(&row["before"]["heap_ptr"]) - before.len() as u32;
            assert_eq!(
                base, 0x20000000,
                "entire original arena must remain compared"
            );
            let after = data(&row["after"]["bytes"]);
            let end = word(&row["after"]["heap_ptr"]);
            assert_eq!(end - base, after.len() as u32);
            for (n, b) in before.iter().enumerate() {
                m.w8(base + n as u32, *b)
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            STORES.with(|s| s.borrow_mut().clear());
            super::super::names::set_test_store_observer(Some(note));
            struct ObserverGuard;
            impl Drop for ObserverGuard {
                fn drop(&mut self) {
                    super::super::names::set_test_store_observer(None);
                }
            }
            let _observer = ObserverGuard;
            for item in inputs {
                let ptrs = item["pointers"].as_array().unwrap();
                assert_eq!(ptrs.len(), 3);
                for (p, name) in ptrs
                    .iter()
                    .zip(["class_bytes", "method_bytes", "signature_bytes"])
                {
                    let expected = data(&item[name]);
                    for (n, b) in expected.iter().enumerate() {
                        assert_eq!(m.r8(word(p) + n as u32), *b)
                    }
                    assert_eq!(m.r8(word(p) + expected.len() as u32), 0);
                }
                assert_eq!(
                    register_memory(
                        &mut m,
                        word(&row["jvm"]),
                        word(&ptrs[0]),
                        word(&ptrs[1]),
                        word(&ptrs[2]),
                        word(&item["implementation"]),
                        &mut Bump
                    )
                    .unwrap(),
                    word(&item["result"])
                );
            }
            super::super::names::set_test_store_observer(None);
            assert_eq!(m.heap_ptr(), end, "heap cursor row{i}");
            let actual = (base..end).map(|p| m.r8(p)).collect::<Vec<_>>();
            assert_eq!(actual, after, "complete heap row{i}");
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| (word(&v[0]), word(&v[1]), word(&v[2])))
                .collect::<Vec<_>>();
            STORES.with(|s| assert_eq!(*s.borrow(), expected, "ordered raw heap stores row{i}"));
        }
    }
}

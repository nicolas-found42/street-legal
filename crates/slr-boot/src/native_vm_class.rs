//! Original403a90 class registration memory from an actual decoded module.
//!
//! The module/CONS record graph must be produced by its source loader. No
//! record, descriptor or body pointer is inferred from a matching class name.
//! This bounded memory projection deliberately does not claim403a90 or its
//! called interner's complete machine scratch/register history.
use super::names;
use crate::native_stack as stack;
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

/// Read the class-name indirection actually consumed by406f60/406140.
/// The module's head word is a Class entry whose payload selects a Str entry.
#[allow(dead_code)]
pub fn source_class_name(m: &Space, module: u32) -> Result<u32, String> {
    let pool = m.r32(module.wrapping_add(0x24));
    let count = m.r32(pool.wrapping_add(8));
    let class_index = m.r32(module.wrapping_add(0x3c));
    if module == 0 || pool == 0 || class_index >= count || class_index > i32::MAX as u32 {
        return Err("NativeVmClass: missing source module/class-name index".into());
    }
    let data = m.r32(pool + 12);
    let record = m.r32(data + class_index * 4);
    if data == 0 || record == 0 || m.r32(record + 4) != 4 {
        return Err("NativeVmClass: class-name source is not a Class record".into());
    }
    let name_index = m.r32(record + 8);
    if name_index >= count || name_index > i32::MAX as u32 {
        return Err("NativeVmClass: missing source class-name Str index".into());
    }
    let record = m.r32(data + name_index * 4);
    if record == 0 || m.r32(record + 4) != 0 || m.r32(record + 8) == 0 {
        return Err("NativeVmClass: class-name source is not a Str record".into());
    }
    Ok(m.r32(record + 8))
}

/// Source class object allocation is owned by the loader caller. This ports
/// normal (non-array) named class registration, including duplicate names and
/// original insert-after-head cache links, at its actual source frame geometry.
/// Static initialization and404e20 instance allocation are later operations.
#[allow(dead_code)]
pub fn register_memory(
    m: &mut Space,
    this: u32,
    jvm: u32,
    module: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let source = source_class_name(m, module)?;
    let length = (0..254)
        .find(|i| m.r8(source + i) == 0)
        .ok_or("NativeVmClass: unsupported overlong source class name")?;
    let cache = m.r32(jvm.wrapping_add(8));
    let holder = m.r32(jvm.wrapping_add(0x68));
    if this == 0
        || jvm == 0
        || cache == 0
        || holder == 0
        || m.r32(holder + 4) == 0
        || m.r8(source) == b'['
    {
        return Err(
            "NativeVmClass: unresolved source class/JVM/cache/name context or array-class branch"
                .into(),
        );
    }
    stack::function(m, &[jvm, module], Some(this), |m| {
        stack::alloc(m, 0x204);
        for r in [
            stack::Reg::Ebx,
            stack::Reg::Ebp,
            stack::Reg::Esi,
            stack::Reg::Edi,
        ] {
            stack::push_reg(m, r);
        }
        for container in [this + 0x19c, this + 0x1b0] {
            for off in [0, 4, 8, 12, 16] {
                m.w32(container + off, 0);
            }
        }
        for i in 0..32 {
            m.w32(this + 0x20 + i * 12, 0);
        }
        for (off, value) in [
            (0, 0x5e7340),
            (4, module),
            (0x14, m.r32(module + 0x24)),
            (0x18, m.r32(module + 0x60)),
            (0x10, m.r32(module + 0x38)),
            (12, jvm),
            (0x1d4, 0),
        ] {
            m.w32(this + off, value);
        }
        //403b6b ordinary-name path forms this bounded descriptor on its own
        //real local frame. The interner reads these live bytes, not a fixture.
        let scratch = stack::sp(m) + 0x14;
        m.w8(scratch, b'L');
        for i in 0..length {
            m.w8(scratch + 1 + i, m.r8(source + i));
        }
        m.w8(scratch + 1 + length, b';');
        m.w8(scratch + 2 + length, 0);
        let descriptor = a.allocate(m, 4)?;
        let name = names::intern(m, holder, scratch, length + 2, a)?;
        m.w32(descriptor, name);
        m.w32(this + 8, descriptor);
        m.w32(this + 0x1d0, 0);
        m.w32(this + 0x1cc, 0);
        let hash = (0..length)
            .fold(0u32, |sum, i| {
                sum.wrapping_add(m.r8(source + i) as i8 as i32 as u32)
            })
            .wrapping_mul(37)
            .wrapping_add(length.wrapping_mul(7));
        let bucket = cache + (hash & 511) * 12;
        let occupied = m.r32(bucket + 4) != 0;
        let node = if occupied {
            names::pool(m, cache + 0x1800, 12, 64, a)?
        } else {
            bucket
        };
        m.w32(node, hash);
        let slot = names::pool(m, cache + 0x1814, 4, 64, a)?;
        m.w32(slot, this);
        m.w32(node + 4, slot);
        if occupied {
            m.w32(node + 8, m.r32(bucket + 8));
            m.w32(bucket + 8, node);
        } else {
            m.w32(bucket + 8, 0);
        }
        Ok(this)
    })
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
    fn original_class_registration_preserves_physical_records_and_complete_heap() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-class-original.json"))
                .expect("run native_vm_class_probe.py"),
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
            ["native_vm_class_probe.py", "oracle.py", "thread_vectors.py"]
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
        assert_eq!(rows.len(), 6);
        assert_eq!(
            rows.iter()
                .map(|r| (
                    r["label"].as_str().unwrap(),
                    r["inputs"].as_array().unwrap().len(),
                    word(&r["seed"])
                ))
                .collect::<std::collections::HashSet<_>>(),
            [
                ("simple_and_duplicate", 4),
                ("cache_growth", 70),
                ("duplicate_records", 12)
            ]
            .into_iter()
            .flat_map(|(label, count)| [606, 20261022].map(move |seed| (label, count, seed)))
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
            for input in row["inputs"].as_array().unwrap() {
                assert_eq!(
                    register_memory(
                        &mut m,
                        word(&input["this"]),
                        word(&row["jvm"]),
                        word(&input["module"]),
                        &mut Bump
                    )
                    .unwrap(),
                    word(&input["this"])
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
            let diffs = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .map(|(i, (a, b))| (0x20000000 + i as u32, *a, *b))
                .collect::<Vec<_>>();
            assert!(
                diffs.is_empty(),
                "all class/cache/descriptor/cell/padding heap bytes {} seed{} {diffs:x?}",
                row["label"],
                row["seed"]
            );
        }
    }
}

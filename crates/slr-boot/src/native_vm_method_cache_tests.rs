use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
        .collect()
}
struct NoAllocation;
impl crate::native_vm_pool::Allocator for NoAllocation {
    fn allocate(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("cached lookup must not allocate".into())
    }
    fn reallocate(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<u32, String> {
        Err("cached lookup must not reallocate".into())
    }
}
impl crate::native_vm_context::Environment for NoAllocation {}
impl crate::native_vm_dispatch::Environment for NoAllocation {
    fn free(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
        Err("cached lookup must not free".into())
    }
}
#[test]
fn original_cached_method_name_and_signature_collisions_match_complete_state() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(path) =
        crate::oracle_fixture::file(&root.join("re/out/boot/native-vm-method-cache-original.json"))
    else {
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(
        data["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    for name in [
        "native_vm_method_cache_probe.py",
        "oracle.py",
        "thread_vectors.py",
    ] {
        assert_eq!(
            data["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 6);
    let regs = [
        (Reg::Eax, "eax"),
        (Reg::Ebx, "ebx"),
        (Reg::Ecx, "ecx"),
        (Reg::Edx, "edx"),
        (Reg::Esi, "esi"),
        (Reg::Edi, "edi"),
        (Reg::Ebp, "ebp"),
    ];
    for row in rows {
        let mut m = Space::new();
        let parent = stack::enter(&mut m, &[], None);
        for (key, base) in [
            ("heap", 0x20000000),
            ("stack", 0x7feff000),
            ("globals", 0x62ddf0),
        ] {
            for (i, b) in bytes(&row["before"][key]).into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
        }
        m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
        for (reg, name) in regs {
            stack::set_reg(&mut m, reg, word(&row["before"]["registers"][name]));
        }
        stack::set_sp(&mut m, word(&row["before"]["registers"]["esp"]) + 12);
        stack::begin_call(
            &mut m,
            word(&row["return_pc"]),
            &[word(&row["name"]), word(&row["descriptor"])],
            Some(word(&row["cl"])),
        );
        let result = method_by_name(
            &mut m,
            word(&row["cl"]),
            word(&row["name"]),
            word(&row["descriptor"]),
            &mut NoAllocation,
        )
        .unwrap();
        assert_eq!(result, word(&row["result"]));
        for (key, base) in [
            ("heap", 0x20000000),
            ("stack", 0x7feff000),
            ("globals", 0x62ddf0),
        ] {
            let diff = bytes(&row["after"][key])
                .into_iter()
                .enumerate()
                .filter_map(|(i, want)| {
                    let p = base + i as u32;
                    let got = m.r8(p);
                    (got != want).then_some((p, got, want))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(
                diff.is_empty(),
                "selected {} {key} {diff:x?}",
                row["selected"]
            );
        }
        for (reg, name) in regs {
            assert_eq!(
                stack::reg(&m, reg),
                word(&row["after"]["registers"][name]),
                "selected {} {name}",
                row["selected"]
            );
        }
        assert_eq!(stack::sp(&m), word(&row["after"]["registers"]["esp"]));
        assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]));
        stack::leave(&mut m, parent);
    }
}

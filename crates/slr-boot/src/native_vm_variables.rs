//! Original4036b0/4038c0 variable owner storage. Entries retain actual pooled
//! variable addresses. Names are compared as source bytes, without semantic IDs.
//! This memory seam does not claim native caller scratch/register history.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 4, v)));
    m.w32(p, v)
}
/// exe:0x4036b0. `this` comes from its actual caller allocation.
#[allow(dead_code)]
pub fn construct_memory(
    m: &mut Space,
    this: u32,
    jvm: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    if this == 0 || jvm == 0 {
        return Err("NativeVmVariables: absent owner/JVM allocation identity".into());
    }
    write(m, this + 8, 0);
    write(m, this + 4, 0);
    write(m, this, 0x5e7338);
    let c = a.allocate(m, 12)?;
    if c != 0 {
        write(m, c, 0);
        write(m, c + 8, 0);
        write(m, c + 4, 0)
    }
    write(m, this + 12, c);
    write(m, this + 16, jvm);
    Ok(this)
}
fn equal(m: &Space, mut a: u32, mut b: u32) -> Result<bool, String> {
    if a == 0 || b == 0 {
        return Err("NativeVmVariables: absent raw variable name identity".into());
    }
    for _ in 0..0x100000 {
        let x = m.r8(a);
        let y = m.r8(b);
        if x != y {
            return Ok(false);
        }
        if x == 0 {
            return Ok(true);
        }
        a = a
            .checked_add(1)
            .ok_or("NativeVmVariables: name address overflow")?;
        b = b
            .checked_add(1)
            .ok_or("NativeVmVariables: name address overflow")?;
    }
    Err("NativeVmVariables: unterminated variable name outside bounded span".into())
}
/// exe:0x4038c0. Full returned word is retained: successful insertion stores
/// AL=1 after EAX held the prior length; callers usually consume only AL.
#[allow(dead_code)]
pub fn append_memory(
    m: &mut Space,
    this: u32,
    variable: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    if this == 0 || variable == 0 || m.r32(this) != 0x5e7338 || m.r32(variable) != 0x5e7344 {
        return Err("NativeVmVariables: missing source owner/variable identity".into());
    }
    let c = m.r32(this + 12);
    let cap = m.r32(c);
    let len = m.r32(c + 8);
    let table = m.r32(c + 4);
    if c == 0 || len > cap || cap > i32::MAX as u32 / 4 || (cap != 0 && table == 0) {
        return Err("NativeVmVariables: malformed original variable vector".into());
    }
    for i in (0..len).rev() {
        let old = m.r32(table + i * 4);
        if old == 0 || m.r32(old) != 0x5e7344 {
            return Err("NativeVmVariables: vector entry is not actual variable owner".into());
        }
        if equal(m, m.r32(old + 12), m.r32(variable + 12))? {
            return Ok(0);
        }
    }
    let wanted = len
        .checked_add(1)
        .ok_or("NativeVmVariables: vector length overflow")?;
    if wanted > cap {
        let nc = cap
            .checked_add(wanted)
            .filter(|n| *n <= i32::MAX as u32 / 4)
            .ok_or("NativeVmVariables: vector capacity overflow")?;
        let table = a.reallocate(m, table, cap * 4, nc * 4)?;
        write(m, c + 4, table);
        if nc < len {
            write(m, c + 8, nc)
        }
        write(m, c, nc);
    }
    let old_len = m.r32(c + 8);
    write(m, m.r32(c + 4) + old_len * 4, variable);
    write(m, c + 8, old_len + 1);
    Ok((old_len & !255) | 1)
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
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(w(v)).unwrap())
            .collect()
    }
    fn names(label: &str) -> Vec<String> {
        match label {
            "empty" => vec![],
            "content_duplicates" => [
                "same", "other", "same", "", "", "other", "prefix", "prefix.x", "prefix",
            ]
            .into_iter()
            .map(str::to_owned)
            .collect(),
            "case_and_odd_even" => ["a", "A", "ab", "abc", "abcd", "ab", "abc", "A"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            "growth_full_eax" => (0..258)
                .map(|i| format!("field{i:03}"))
                .chain(["field000".into(), "field257".into()])
                .collect(),
            _ => panic!("undeclared sequence {label}"),
        }
    }
    #[test]
    fn original_variable_owner_vector_complete_heap_and_all_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value =
            serde_json::from_slice(
                &std::fs::read(root.join(
                    "re/out/checkpoints/abi-reconstruction-20261023/variables-original-v1.json",
                ))
                .unwrap(),
            )
            .unwrap();
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
        let sources = v["producer_sources"].as_object().unwrap();
        assert_eq!(sources.len(), 3);
        for name in [
            "native_vm_variables_probe.py",
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
        assert_eq!(rows.len(), 8);
        let inv = rows
            .iter()
            .map(|r| (r["label"].as_str().unwrap(), w(&r["seed"])))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inv,
            [
                "empty",
                "content_duplicates",
                "case_and_odd_even",
                "growth_full_eax"
            ]
            .into_iter()
            .flat_map(|l| [606, 20261023].map(move |s| (l, s)))
            .collect()
        );
        for row in rows {
            let mut m = Space::new();
            let initial = bytes(&row["before"]["bytes"]);
            assert_eq!(
                initial.len(),
                (w(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in initial.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b)
            }
            m.set_heap_ptr(w(&row["before"]["heap_ptr"]));
            let declared = names(row["label"].as_str().unwrap());
            let inputs = row["inputs"].as_array().unwrap();
            assert_eq!(inputs.len(), declared.len());
            for (item, name) in inputs.iter().zip(&declared) {
                assert_eq!(item["name"], name.as_str());
                let variable = w(&item["variable"]);
                assert_eq!(m.r32(variable), 0x5e7344);
                assert_eq!(m.r32(variable + 12), w(&item["name_pointer"]));
                for (i, b) in name.bytes().chain(std::iter::once(0)).enumerate() {
                    assert_eq!(m.r8(w(&item["name_pointer"]) + i as u32), b)
                }
            }
            STORES.with(|s| s.borrow_mut().clear());
            assert_eq!(
                construct_memory(&mut m, w(&row["owner"]), w(&row["jvm"]), &mut Bump).unwrap(),
                w(&row["constructor_result"])
            );
            for item in inputs {
                assert_eq!(
                    append_memory(&mut m, w(&row["owner"]), w(&item["variable"]), &mut Bump)
                        .unwrap(),
                    w(&item["result"]),
                    "{} {}",
                    row["label"],
                    item["name"]
                )
            }
            if row["label"] == "growth_full_eax" {
                assert_eq!(inputs[256]["result"], 257);
                assert_eq!(inputs[257]["result"], 257);
                assert_eq!(inputs[258]["result"], 0);
                assert_eq!(inputs[259]["result"], 0)
            }
            assert_eq!(m.heap_ptr(), w(&row["after"]["heap_ptr"]));
            let expected = bytes(&row["after"]["bytes"]);
            assert_eq!(expected.len(), (m.heap_ptr() - 0x20000000) as usize);
            let first = expected
                .iter()
                .enumerate()
                .find(|(i, b)| m.r8(0x20000000 + *i as u32) != **b);
            assert!(first.is_none(), "whole heap {} {:?}", row["label"], first);
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|a| (w(&a[0]), w(&a[1]), w(&a[2])))
                .collect::<Vec<_>>();
            STORES.with(|s| assert_eq!(*s.borrow(), expected, "all stores {}", row["label"]));
        }
    }
    #[test]
    fn missing_owner_or_wrong_cell_fails_closed_before_stores() {
        let mut m = Space::new();
        let owner = m.alloc(20);
        let variable = m.alloc(16);
        STORES.with(|s| s.borrow_mut().clear());
        let hp = m.heap_ptr();
        assert!(append_memory(&mut m, owner, variable, &mut Bump).is_err());
        assert_eq!(m.heap_ptr(), hp);
        assert_eq!(STORES.with(|s| s.borrow().len()), 0)
    }
}

/// Source403e80's already-installed branch only. A first initialization still
/// requires dependency resolution, temporaryThread lifetime and initializer
/// execution; this entry rejects it before changing a caller's current frame.
/// exe:0x403e80
#[allow(dead_code)]
pub fn initialized_class_reentry(m: &mut Space, class: u32) -> Result<(), String> {
    use crate::native_stack::{self as stack, Reg};
    let owner = m.r32(class.wrapping_add(0x1d0));
    if class == 0 || m.r32(class) != 0x5e7340 || owner == 0 || m.r32(owner) != 0x5e7338 {
        return Err("NativeVmVariables: first403e80 class initialization requires real dependency/thread/initializer lifecycle".into());
    }
    stack::function(m, &[], Some(class), |m| {
        stack::alloc(m, 0x4c);
        stack::push_reg(m, Reg::Ebp);
        stack::push_reg(m, Reg::Edi);
        stack::set_reg(m, Reg::Edi, class);
        stack::set_reg(m, Reg::Eax, owner);
        stack::set_reg(m, Reg::Ebp, 0);
        stack::set_reg(m, Reg::Edi, m.r32(stack::sp(m)));
        stack::add_sp(m, 4);
        stack::set_reg(m, Reg::Ebp, m.r32(stack::sp(m)));
        stack::add_sp(m, 4);
        stack::add_sp(m, 0x4c);
    });
    Ok(())
}

#[cfg(test)]
mod reentry_tests {
    use super::*;
    use crate::native_stack::{self as stack, Reg};
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(w(v)).unwrap())
            .collect()
    }
    #[test]
    fn original_initialized_class_reentry_matches_complete_heap_stack_and_gprs() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value =
            serde_json::from_slice(
                &std::fs::read(root.join(
                    "re/out/checkpoints/abi-reconstruction-20261023/variables-original-v1.json",
                ))
                .unwrap(),
            )
            .unwrap();
        assert_eq!(v["schema"], 2);
        assert_eq!(v["fpu_cw"], 127);
        for name in [
            "native_vm_variables_probe.py",
            "oracle.py",
            "thread_vectors.py",
        ] {
            assert_eq!(
                v["producer_sources"][name],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                )
            )
        }
        let rows = v["reentries"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        let inventory = rows
            .iter()
            .map(|r| (w(&r["seed"]), w(&r["fill"])))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inventory,
            [(606, 0), (606, 165), (20261023, 0), (20261023, 165)]
                .into_iter()
                .collect()
        );
        for row in rows {
            let mut m = Space::new();
            let before = bytes(&row["before"]["heap"]);
            assert_eq!(
                before.len(),
                (w(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b)
            }
            m.set_heap_ptr(w(&row["before"]["heap_ptr"]));
            let stack_image = bytes(&row["before"]["stack"]);
            assert_eq!(stack_image.len(), 0x400);
            for (i, b) in stack_image.into_iter().enumerate() {
                m.w8(w(&row["stack_base"]) + i as u32, b)
            }
            let regs = [
                (Reg::Eax, "eax"),
                (Reg::Ebx, "ebx"),
                (Reg::Ecx, "ecx"),
                (Reg::Edx, "edx"),
                (Reg::Esi, "esi"),
                (Reg::Edi, "edi"),
                (Reg::Ebp, "ebp"),
            ];
            for (r, n) in regs {
                stack::set_reg(&mut m, r, w(&row["register_inputs"][n]))
            }
            let esp = w(&row["entry_esp"]);
            m.w32(esp, 0x7fff0000);
            stack::set_sp(&mut m, esp);
            m.w32(0x7ffdf004, 1);
            assert_eq!(m.r32(w(&row["class"]) + 0x1d0), w(&row["owner"]));
            initialized_class_reentry(&mut m, w(&row["class"])).unwrap();
            assert_eq!(m.heap_ptr(), w(&row["after"]["heap_ptr"]));
            let expected = bytes(&row["after"]["heap"]);
            assert_eq!(expected.len(), (m.heap_ptr() - 0x20000000) as usize);
            assert!(expected
                .iter()
                .enumerate()
                .all(|(i, b)| m.r8(0x20000000 + i as u32) == *b));
            let expected = bytes(&row["after"]["stack"]);
            assert_eq!(expected.len(), 0x400);
            assert_eq!(
                (0..0x400)
                    .map(|i| m.r8(w(&row["stack_base"]) + i))
                    .collect::<Vec<_>>(),
                expected
            );
            for (r, n) in regs {
                assert_eq!(stack::reg(&m, r), w(&row["after"]["registers"][n]), "{n}")
            }
            assert_eq!(stack::sp(&m), w(&row["after"]["registers"]["esp"]));
        }
    }
    #[test]
    fn first_class_initialization_is_explicit_without_erasing_parent_context() {
        let mut m = Space::new();
        let class = m.alloc(0x1d8);
        m.w32(class, 0x5e7340);
        stack::set_sp(&mut m, 0x7feffe00);
        stack::set_reg(&mut m, Reg::Eax, 0x12345678);
        assert!(initialized_class_reentry(&mut m, class)
            .unwrap_err()
            .contains("first403e80"));
        assert_eq!(stack::sp(&m), 0x7feffe00);
        assert_eq!(stack::reg(&m, Reg::Eax), 0x12345678);
        assert_eq!(m.r32(class + 0x1d0), 0)
    }
}

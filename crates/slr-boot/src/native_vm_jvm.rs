//! Original40fb50 JVM construction with real table/list/name constructors.
//! Environment supplies allocator/logger leaves. Allocation failures remain
//! explicit boundaries; there is no captured pointer or native-result input.
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;
#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
type StoreObserver = fn(u32, u32, u32);
#[cfg(test)]
thread_local! { static OBSERVER: std::cell::Cell<Option<StoreObserver>> = const {std::cell::Cell::new(None)}; }
#[cfg(test)]
pub(crate) fn set_test_store_observer(observer: Option<StoreObserver>) {
    OBSERVER.with(|slot| slot.set(observer));
}
fn observe(p: u32, n: u32, v: u32) {
    #[cfg(test)]
    OBSERVER.with(|slot| {
        if let Some(observer) = slot.get() {
            observer(p, n, v);
        }
    });
    #[cfg(not(test))]
    let _ = (p, n, v);
}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 4, v)));
    observe(p, 4, v);
    m.w32(p, v);
}
fn byte(m: &mut Space, p: u32, v: u8) {
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((p, 1, v as u32)));
    observe(p, 1, u32::from(v));
    m.w8(p, v);
}
fn set(m: &mut Space, r: Reg, v: u32) {
    stack::set_reg(m, r, v);
}
pub trait Environment {
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String>;
    fn logger(&mut self, m: &mut Space, logger: u32, message: u32) -> Result<u32, String>;
}
fn allocation_leaf(m: &mut Space, n: u32, e: &mut impl Environment) -> Result<u32, String> {
    let pointer = e.allocate(m, n)?;
    if pointer == 0 {
        return Err("NativeVmJvm: allocation failure outside ordinary-success constructor".into());
    }
    Ok(pointer)
}
fn allocate(m: &mut Space, pc: u32, n: u32, e: &mut impl Environment) -> Result<u32, String> {
    stack::push(m, n);
    let p = stack::external(m, pc, &[], None, |m| allocation_leaf(m, n, e))?;
    set(m, Reg::Eax, p);
    stack::add_sp(m, 4);
    if p == 0 {
        return Err("NativeVmJvm: allocation failure outside ordinary-success constructor".into());
    }
    Ok(p)
}
fn list(m: &mut Space, p: u32, objects: bool) {
    let node = if objects { 0x5f087c } else { 0x5e7418 };
    set(m, Reg::Ecx, p + 4);
    write(m, p + 4, node);
    write(m, p + 12, 0);
    write(m, p + 8, 0);
    set(m, Reg::Edx, p + 16);
    write(m, p + 16, node);
    write(m, p + 24, 0);
    write(m, p + 20, 0);
    write(m, p, if objects { 0x5f0878 } else { 0x5f0874 });
    write(m, p + 8, p + 16);
    write(m, p + 24, p + 4);
}
fn gc(m: &mut Space, p: u32) {
    stack::function(m, &[], Some(p), |m| {
        set(m, Reg::Eax, p);
        set(m, Reg::Ecx, p + 4);
        write(m, p + 4, 0x5f0888);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, 0);
        write(m, p + 12, 0);
        write(m, p + 8, 0);
        set(m, Reg::Edx, p + 16);
        write(m, p + 24, 0);
        write(m, p + 20, 0);
        write(m, p + 16, 0x5f0888);
        write(m, p + 8, p + 16);
        write(m, p + 24, p + 4);
        write(m, p, 0x5f0884);
        stack::add_sp(m, 4);
    });
}
fn table(m: &mut Space, p: u32, count: u32) {
    stack::function(m, &[], Some(p), |m| {
        set(m, Reg::Eax, p);
        set(m, Reg::Edx, 0);
        for off in [0, 4, 8, 12, 16] {
            write(m, p + count * 12 + off, 0)
        }
        stack::push_reg(m, Reg::Esi);
        for off in [20, 24, 28, 32, 36] {
            write(m, p + count * 12 + off, 0)
        }
        set(m, Reg::Ecx, p + 4);
        set(m, Reg::Esi, count);
        for i in 0..count {
            write(m, p + 4 + i * 12, 0);
            set(m, Reg::Ecx, p + 4 + (i + 1) * 12);
            set(m, Reg::Esi, count - i - 1);
        }
        stack::add_sp(m, 4);
    });
}
fn names(m: &mut Space, p: u32, e: &mut impl Environment) -> Result<u32, String> {
    stack::function(m, &[], Some(p), |m| {
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, p);
        stack::push(m, 0x7f40);
        write(m, p, 0x5f0984);
        let table = stack::external(m, 0x41e4e3, &[], None, |m| allocation_leaf(m, 0x7f40, e))?;
        set(m, Reg::Eax, table);
        set(m, Reg::Edx, 0);
        stack::add_sp(m, 4);
        if table == 0 {
            return Err("NativeVmJvm: name table allocation failure".into());
        }
        for off in [0x7f14, 0x7f18, 0x7f1c, 0x7f20, 0x7f24] {
            write(m, table + off, 0)
        }
        stack::push_reg(m, Reg::Esi);
        for off in [0x7f28, 0x7f2c, 0x7f30, 0x7f34, 0x7f38] {
            write(m, table + off, 0)
        }
        set(m, Reg::Ecx, table + 4);
        set(m, Reg::Esi, 0xa97);
        for i in 0..0xa97 {
            write(m, table + 4 + i * 12, 0);
            set(m, Reg::Ecx, table + 4 + (i + 1) * 12);
            set(m, Reg::Esi, 0xa97 - i - 1);
        }
        write(m, p + 4, table);
        stack::add_sp(m, 4);
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        Ok(p)
    })
}
/// Ordinary-success native JVM constructor with retained actual CALL frames.
/// exe: 0x40fb50
pub fn construct(m: &mut Space, this: u32, e: &mut impl Environment) -> Result<u32, String> {
    if this == 0 {
        return Err("NativeVmJvm: missing caller JVM allocation".into());
    }
    stack::function(m, &[], Some(this), |m| {
        stack::push_reg(m, Reg::Esi);
        stack::push_reg(m, Reg::Edi);
        set(m, Reg::Edi, this);
        set(m, Reg::Ecx, this + 0x4c);
        stack::call(m, 0x40fb5c, &[], Some(this + 0x4c), |m| gc(m, this + 0x4c));
        stack::push(m, 0x60ab34);
        set(m, Reg::Ecx, 0x62f018);
        write(m, this, 0x5f0880);
        let r = stack::external(m, 0x40fb71, &[], None, |m| e.logger(m, 0x62f018, 0x60ab34))?;
        set(m, Reg::Eax, r);
        stack::add_sp(m, 4);
        stack::push(m, 0x60ab44);
        set(m, Reg::Ecx, 0x62def8);
        let r = stack::external(m, 0x40fb80, &[], None, |m| e.logger(m, 0x62def8, 0x60ab44))?;
        set(m, Reg::Eax, r);
        stack::add_sp(m, 4);
        for (i, (pc, g)) in [
            (0x40fb87, 0x62ddf0),
            (0x40fbb2, 0x62f130),
            (0x40fbdb, 0x62f134),
        ]
        .into_iter()
        .enumerate()
        {
            let p = allocate(m, pc, 20, e)?;
            if i == 0 {
                set(m, Reg::Esi, 0)
            }
            for off in [0, 4, 8, 12, 16] {
                write(m, p + off, 0)
            }
            write(m, g, p);
        }
        let owner = allocate(m, 0x40fc04, 4, e)?;
        stack::call(m, 0x40fc13, &[this], Some(owner), |m| {
            stack::function(m, &[this], Some(owner), |m| {
                set(m, Reg::Eax, owner);
                set(m, Reg::Ecx, this);
                write(m, owner, this)
            })
        });
        stack::push_reg(m, Reg::Ebx);
        stack::push(m, 0x62c);
        write(m, this + 4, owner);
        let native = stack::external(m, 0x40fc25, &[], None, |m| allocation_leaf(m, 0x62c, e))?;
        set(m, Reg::Eax, native);
        set(m, Reg::Ebx, native);
        stack::add_sp(m, 4);
        if native == 0 {
            return Err("NativeVmJvm: native table allocation failure".into());
        }
        stack::call(m, 0x40fc35, &[], Some(native), |m| table(m, native, 0x80));
        stack::push(m, 0x182c);
        write(m, this + 12, native);
        let classes = stack::external(m, 0x40fc46, &[], None, |m| allocation_leaf(m, 0x182c, e))?;
        set(m, Reg::Eax, classes);
        set(m, Reg::Ebx, classes);
        stack::add_sp(m, 4);
        if classes == 0 {
            return Err("NativeVmJvm: class table allocation failure".into());
        }
        stack::call(m, 0x40fc56, &[], Some(classes), |m| {
            table(m, classes, 0x200)
        });
        stack::push(m, 28);
        write(m, this + 8, classes);
        let p = stack::external(m, 0x40fc64, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        set(m, Reg::Ebx, stack::load(m, 0));
        stack::add_sp(m, 4);
        if p == 0 {
            return Err("NativeVmJvm: list allocation failure".into());
        }
        list(m, p, true);
        stack::push(m, 28);
        write(m, this + 0x18, p);
        let p = stack::external(m, 0x40fca4, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, true);
        stack::push(m, 28);
        write(m, this + 0x1c, p);
        let p = stack::external(m, 0x40fce3, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, false);
        stack::push(m, 28);
        write(m, this + 0x10, p);
        let p = stack::external(m, 0x40fd22, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, false);
        stack::push(m, 8);
        write(m, this + 0x14, p);
        let p = stack::external(m, 0x40fd61, &[], None, |m| allocation_leaf(m, 8, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        let holder = stack::call(m, 0x40fd6f, &[], Some(p), |m| names(m, p, e))?;
        stack::push(m, 28);
        write(m, this + 0x68, holder);
        write(m, this + 0x38, 1000);
        let p = stack::external(m, 0x40fd84, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, false);
        stack::push(m, 28);
        write(m, this + 0x44, p);
        write(m, this + 0x3c, 4);
        for off in [0x28, 0x24, 0x78, 0x74, 0x70, 0x20] {
            write(m, this + off, 0)
        }
        let p = stack::external(m, 0x40fddc, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, false);
        stack::push(m, 28);
        write(m, this + 0x2c, p);
        let p = stack::external(m, 0x40fe1b, &[], None, |m| allocation_leaf(m, 28, e))?;
        set(m, Reg::Eax, p);
        stack::add_sp(m, 4);
        list(m, p, false);
        write(m, this + 0x30, p);
        write(m, this + 0x34, 0);
        stack::call(m, 0x40fe58, &[], None, |m| {
            stack::function(m, &[], None, |m| {
                set(m, Reg::Eax, 0);
                write(m, 0x62de04, 0);
                write(m, 0x62de08, 0);
                for (p, v) in [
                    (0x62dea2, 8),
                    (0x62deb4, 9),
                    (0x62deae, 10),
                    (0x62dea6, 12),
                    (0x62deb2, 13),
                    (0x62de70, 0),
                    (0x62de62, 34),
                    (0x62de67, 39),
                    (0x62de9c, 92),
                ] {
                    byte(m, p, v)
                }
            })
        });
        set(m, Reg::Eax, this);
        stack::add_sp(m, 8);
        Ok(this)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    fn word(v: &Value) -> u32 {
        v.as_u64().expect("word") as u32
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .expect("bytes")
            .iter()
            .map(|x| u8::try_from(word(x)).unwrap())
            .collect()
    }
    fn regs() -> [(Reg, &'static str); 7] {
        [
            (Reg::Eax, "eax"),
            (Reg::Ebx, "ebx"),
            (Reg::Ecx, "ecx"),
            (Reg::Edx, "edx"),
            (Reg::Esi, "esi"),
            (Reg::Edi, "edi"),
            (Reg::Ebp, "ebp"),
        ]
    }
    fn exact_snapshot_inventory(v: &Value) {
        use std::collections::BTreeSet;
        let keys = |field: &str| {
            v[field]
                .as_object()
                .expect(field)
                .keys()
                .cloned()
                .collect::<BTreeSet<_>>()
        };
        assert_eq!(
            keys("globals"),
            [0x62ddf0u32, 0x62f130, 0x62f134, 0x62de04, 0x62de08]
                .map(|p| p.to_string())
                .into_iter()
                .collect()
        );
        assert_eq!(
            keys("global_bytes"),
            [
                0x62dea2u32,
                0x62deb4,
                0x62deae,
                0x62dea6,
                0x62deb2,
                0x62de70,
                0x62de62,
                0x62de67,
                0x62de9c
            ]
            .map(|p| p.to_string())
            .into_iter()
            .collect()
        );
        assert_eq!(
            keys("registers"),
            ["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp", "esp"]
                .map(str::to_owned)
                .into_iter()
                .collect()
        );
        assert_eq!(bytes(&v["heap"]).len(), 0xc000);
        assert_eq!(bytes(&v["stack"]).len(), 0x500);
    }
    fn compare(m: &Space, v: &Value, label: &str) {
        exact_snapshot_inventory(v);
        for (name, p, n) in [("heap", 0x20000000, 0xc000), ("stack", 0x7feffb00, 0x500)] {
            let expected = bytes(&v[name]);
            assert_eq!(expected.len(), n);
            let diff = expected
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    (m.r8(p + i as u32) != *b).then_some((p + i as u32, m.r8(p + i as u32), *b))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(diff.is_empty(), "{label} {name}: {diff:x?}");
        }
        assert_eq!(m.heap_ptr(), word(&v["heap_ptr"]), "{label} heap cursor");
        for (r, n) in regs() {
            assert_eq!(stack::reg(m, r), word(&v["registers"][n]), "{label} {n}");
        }
        assert_eq!(stack::sp(m), word(&v["registers"]["esp"]), "{label} esp");
        for (p, v) in v["globals"].as_object().unwrap() {
            assert_eq!(m.r32(p.parse().unwrap()), word(v), "{label} global {p}");
        }
        for (p, v) in v["global_bytes"].as_object().unwrap() {
            assert_eq!(m.r8(p.parse().unwrap()) as u32, word(v), "{label} byte {p}");
        }
    }
    struct Env<'a> {
        events: &'a [Value],
        index: usize,
        logs: u32,
    }
    impl Environment for Env<'_> {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            let e = &self.events[self.index];
            assert_eq!(e["kind"], "allocate");
            assert_eq!(word(&e["bytes"]), n);
            compare(m, &e["state"], &format!("allocate{}", self.index));
            self.index += 1;
            Ok(m.alloc(n))
        }
        fn logger(&mut self, m: &mut Space, logger: u32, message: u32) -> Result<u32, String> {
            let e = &self.events[self.index];
            assert_eq!(e["kind"], "logger");
            assert_eq!(word(&e["this"]), logger);
            assert_eq!(word(&e["message"]), message);
            compare(m, &e["state"], &format!("logger{}", self.index));
            self.index += 1;
            self.logs += 1;
            Ok(0xaabb0000 + self.logs)
        }
    }
    #[test]
    fn original_jvm_constructor_complete_heap_stack_registers_collaborators_and_ordered_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) =
            crate::oracle_fixture::file(&root.join("re/out/boot/native-vm-jvm-original.json"))
        else {
            return;
        };
        let v: Value = serde_json::from_slice(
            &std::fs::read(fixture_path).expect("run native_vm_jvm_probe.py"),
        )
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["cw"], 127);
        assert_eq!(v["unicorn"], "2.1.4");
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
        assert_eq!(sources.len(), 2);
        for n in ["native_vm_jvm_probe.py", "oracle.py"] {
            assert_eq!(
                sources[n],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(n)).unwrap())
                )
            );
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 8);
        for (index, row) in rows.iter().enumerate() {
            exact_snapshot_inventory(&row["before"]);
            assert_eq!(
                row["input"],
                serde_json::json!({"fill":([0,0xa5][index/4]),"seed":([606,20261023][(index/2)%2]),"gap":([0,48][index%2])})
            );
            let mut m = Space::new();
            for (name, p, n) in [("heap", 0x20000000, 0xc000), ("stack", 0x7feffb00, 0x500)] {
                let b = bytes(&row["before"][name]);
                assert_eq!(b.len(), n);
                for (i, b) in b.into_iter().enumerate() {
                    m.w8(p + i as u32, b)
                }
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            for (p, v) in row["before"]["globals"].as_object().unwrap() {
                m.w32(p.parse().unwrap(), word(v));
            }
            for (p, v) in row["before"]["global_bytes"].as_object().unwrap() {
                m.w8(p.parse().unwrap(), word(v) as u8);
            }
            for (r, n) in regs() {
                set(&mut m, r, word(&row["register_inputs"][n]));
            }
            stack::set_sp(&mut m, 0x7fef_feec);
            m.w32(0x7fef_feec, 0x7fff0000);
            m.w32(0x7ffd_f004, 1);
            STORES.with(|s| s.borrow_mut().clear());
            let mut env = Env {
                events: row["events"].as_array().unwrap(),
                index: 0,
                logs: 0,
            };
            let result = construct(&mut m, word(&row["this"]), &mut env).unwrap();
            assert_eq!(result, word(&row["return_value"]));
            assert_eq!(env.index, env.events.len());
            compare(&m, &row["after"], &format!("final{index}"));
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (word(&s[0]), word(&s[1]), word(&s[2])))
                .collect::<Vec<_>>();
            let actual = STORES.with(|s| s.borrow().clone());
            assert_eq!(actual.len(), expected.len(), "storecount{index}");
            let difference = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .find(|(_, (a, b))| a != b);
            assert!(difference.is_none(), "storeorder{index}: {difference:x?}");
        }
    }
    #[test]
    fn unresolved_allocation_failure_stops_at_each_leaf_without_null_stores() {
        struct Fail {
            at: usize,
            count: usize,
        }
        impl Environment for Fail {
            fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
                let index = self.count;
                self.count += 1;
                Ok(if index == self.at { 0 } else { m.alloc(n) })
            }
            fn logger(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
                Ok(0)
            }
        }
        for at in 0..15 {
            let mut m = Space::new();
            let this = m.alloc(128);
            let mut env = Fail { at, count: 0 };
            let error = construct(&mut m, this, &mut env).unwrap_err();
            assert!(error.contains("allocation failure"), "{at}: {error}");
            assert_eq!(env.count, at + 1);
            assert_eq!(m.r32(0), 0, "no fabricated null constructor {at}");
        }
        let mut m = Space::new();
        let mut env = Fail { at: 0, count: 0 };
        assert!(construct(&mut m, 0, &mut env)
            .unwrap_err()
            .contains("missing caller"));
        assert_eq!(env.count, 0);
    }
}

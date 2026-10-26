use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
const REGS: [(Reg, &str); 7] = [
    (Reg::Eax, "eax"),
    (Reg::Ebx, "ebx"),
    (Reg::Ecx, "ecx"),
    (Reg::Edx, "edx"),
    (Reg::Esi, "esi"),
    (Reg::Edi, "edi"),
    (Reg::Ebp, "ebp"),
];
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}
fn put(m: &mut Space, base: u32, bytes: &Value) {
    for (i, b) in bytes.as_array().unwrap().iter().enumerate() {
        m.w8(base + i as u32, u8::try_from(word(b)).unwrap());
    }
}
fn compare(m: &Space, row: &Value) {
    for (field, base, len) in [("heap", 0x20000000, 0x8000), ("stack", 0x7feff800, 0x800)] {
        let bytes = row[field].as_array().unwrap();
        assert_eq!(bytes.len(), len);
        let differences = bytes
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let actual = m.r8(base + i as u32);
                (actual as u32 != word(b)).then_some((base + i as u32, actual, word(b)))
            })
            .take(8)
            .collect::<Vec<_>>();
        assert!(differences.is_empty(), "{field}: {differences:x?}");
    }
    for (address, value) in row["globals"].as_object().unwrap() {
        assert_eq!(m.r32(address.parse().unwrap()), word(value));
    }
    for (reg, name) in REGS {
        assert_eq!(stack::reg(m, reg), word(&row["registers"][name]), "{name}");
    }
    assert_eq!(stack::sp(m), word(&row["registers"]["esp"]));
    assert_eq!(m.heap_ptr(), word(&row["heap_ptr"]));
}
#[test]
fn cached_dispatch_entry_matches_original_complete_memory_and_registers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("re/out/boot/native-vm-dispatch-prefix-original.json");
    let Some(path) = crate::oracle_fixture::file(&path) else {
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["cw"], 0x7f);
    assert_eq!(
        data["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(
        data["exe_sha256"],
        Value::String(format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        ))
    );
    let sources = data["producer_sources"].as_object().unwrap();
    assert_eq!(
        sources
            .keys()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>(),
        ["oracle.py", "native_vm_dispatch_prefix_probe.py"]
            .into_iter()
            .collect()
    );
    for (name, digest) in sources {
        assert_eq!(
            *digest,
            Value::String(format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            ))
        );
    }
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 80);
    let mut inventory = std::collections::HashSet::new();
    for row in rows {
        assert!(inventory.insert((
            word(&row["seed"]),
            word(&row["fill"]),
            row["static"].as_bool().unwrap(),
            row["native"].as_bool().unwrap(),
            row["lookup"].as_bool().unwrap(),
            row["collision"].as_bool().unwrap(),
            row["prepared"].as_bool().unwrap(),
            row["entry"].as_bool().unwrap(),
            row["complete"].as_bool().unwrap()
        )));
        let mut m = Space::new();
        let parent = stack::enter(&mut m, &[], None);
        put(&mut m, 0x20000000, &row["before"]["heap"]);
        put(&mut m, 0x7feff800, &row["before"]["stack"]);
        m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
        for (address, value) in row["before"]["globals"].as_object().unwrap() {
            m.w32(address.parse().unwrap(), word(value));
        }
        m.w8(0x60ab50, b'I');
        m.w8(0x60ab5c, b'L');
        for (reg, name) in REGS {
            stack::set_reg(&mut m, reg, word(&row["before"]["registers"][name]));
        }
        let entry = word(&row["before"]["registers"]["esp"]);
        stack::set_sp(&mut m, entry + 16);
        let args = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(word)
            .collect::<Vec<_>>();
        stack::begin_call(
            &mut m,
            word(&row["return_pc"]),
            &args,
            Some(word(&row["thread"])),
        );
        compare(&m, &row["before"]);
        let mut env = Runtime { row, index: 0 };
        let mut pending = begin_cached(&mut m).unwrap();
        assert_eq!(pending.pc, 0x41fc31);
        if row["lookup"].as_bool().unwrap() {
            pending = select_native(&mut m, pending).unwrap();
            assert_eq!(pending.pc, 0x41fe04);
        }
        if row["prepared"].as_bool().unwrap() {
            pending = prepare_arguments(&mut m, pending, &mut env).unwrap();
            assert_eq!(pending.pc, 0x420025);
        }
        if row["complete"].as_bool().unwrap() {
            let result = crate::native_vm_abi::call_prepared(&mut m, |m, target, info, kind| {
                assert_eq!(target, 0x47cc10);
                assert_eq!(kind, crate::native_vm_abi::ReturnKind::Integer);
                crate::native_keyboard::last_key(m, info, &mut env)
            })
            .unwrap()
            .unwrap();
            finish_cached(
                &mut m,
                pending,
                crate::native_vm_abi::ReturnKind::Integer,
                result,
                &mut env,
            )
            .unwrap();
            compare(&m, &row["after"]);
        } else {
            if row["entry"].as_bool().unwrap() {
                crate::native_vm_abi::call_prepared(&mut m, |m, target, info, kind| {
                    assert_eq!(target, 0x47cc10);
                    assert_eq!(kind, crate::native_vm_abi::ReturnKind::Integer);
                    stack::function_cdecl(m, &[info], |m| compare(m, &row["after"]));
                })
                .unwrap();
            } else {
                compare(&m, &row["after"]);
            }
            stack::leave(&mut m, pending.frame);
        }
        assert_eq!(env.index, row["events"].as_array().unwrap().len());
        stack::leave(&mut m, parent);
    }
    let mut expected = std::collections::HashSet::new();
    for seed in [606, 20261024] {
        for fill in [0, 0xa5] {
            for static_ in [false, true] {
                for native in [false, true] {
                    expected.insert((
                        seed, fill, static_, native, false, false, false, false, false,
                    ));
                }
                for collision in [false, true] {
                    for (prepared, entry, complete) in [
                        (false, false, false),
                        (true, false, false),
                        (true, true, false),
                        (true, true, true),
                    ] {
                        expected.insert((
                            seed, fill, static_, true, true, collision, prepared, entry, complete,
                        ));
                    }
                }
            }
        }
    }
    // Prefix rows have no lookup/collision flags; normalize the inventory's
    // stage tuple to the same explicit nine fields as the producer.
    assert_eq!(inventory, expected);
}
#[test]
fn missing_or_uncached_context_does_not_consume_the_pending_caller() {
    let mut m = Space::new();
    assert!(begin_cached(&mut m).is_err());
    let parent = stack::enter(&mut m, &[], None);
    stack::begin_call(&mut m, 0x12345678, &[0, 0, 0], Some(0));
    let sp = stack::sp(&m);
    let heap = m.heap_ptr();
    assert!(begin_cached(&mut m).is_err());
    assert!(stack::has_pending_call(&m));
    assert_eq!(stack::sp(&m), sp);
    assert_eq!(m.heap_ptr(), heap);
    stack::function(&mut m, &[0, 0, 0], None, |_| ());
    stack::leave(&mut m, parent);
}

struct Runtime<'a> {
    row: &'a Value,
    index: usize,
}
impl Runtime<'_> {
    fn observe(&mut self, m: &Space, kind: &str, args: &[u32]) {
        let event = &self.row["events"][self.index];
        self.index += 1;
        assert_eq!(event["kind"], kind, "leaf {}", self.index);
        assert_eq!(event["args"], serde_json::json!(args));
        compare(m, &event["state"]);
    }
}
impl crate::native_vm_pool::Allocator for Runtime<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.observe(m, "allocate", &[n]);
        Ok(m.alloc(n))
    }
    fn reallocate(&mut self, m: &mut Space, old: u32, old_n: u32, n: u32) -> Result<u32, String> {
        self.observe(m, "reallocate", &[old, n]);
        let p = m.alloc(n);
        for i in 0..old_n.min(n) {
            m.w8(p + i, m.r8(old + i));
        }
        Ok(p)
    }
}
impl crate::native_vm_context::Environment for Runtime<'_> {}
impl Environment for Runtime<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        self.observe(m, "free", &[p]);
        Ok(())
    }
}
impl crate::native_keyboard::Environment for Runtime<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        leaf: crate::native_keyboard::Leaf,
        target: u32,
        args: &[u32],
    ) -> Result<u32, String> {
        use crate::native_keyboard::Leaf;
        match leaf {
            Leaf::Acquire => {
                self.observe(m, "acquire", args);
                assert_eq!(target, 0x41b860);
            }
            Leaf::GetDeviceData => {
                self.observe(m, "data", args);
                assert_eq!(target, 0x41b870);
                m.w32(args[3], 0);
            }
            _ => panic!("outside declared quiescent profile"),
        }
        Ok(0)
    }
}

fn hex_bytes(value: &Value) -> Vec<u8> {
    let text = value.as_str().unwrap();
    assert_eq!(text.len() % 2, 0);
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
        .collect()
}
struct Genuine<'a> {
    data: &'a Value,
    index: usize,
}
impl Genuine<'_> {
    fn compare(&self, m: &Space, state: &Value) {
        for page in self.data["pages"].as_array().unwrap() {
            let address = word(&page["address"]);
            let initial = page["bytes"].as_str().unwrap();
            let text = state["changed"]
                .get(address.to_string())
                .map_or(initial, |v| v.as_str().unwrap());
            let bytes = hex_bytes(&Value::String(text.to_owned()));
            assert_eq!(bytes.len(), 4096);
            let differences = bytes
                .into_iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let p = address + i as u32;
                    let a = m.r8(p);
                    (a != b).then_some((p, a, b))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(
                differences.is_empty(),
                "genuine leaf {} memory {differences:x?}",
                self.index
            );
        }
        for (reg, name) in REGS {
            assert_eq!(
                stack::reg(m, reg),
                word(&state["registers"][name]),
                "genuine leaf {} {name}",
                self.index
            );
        }
        assert_eq!(
            stack::sp(m),
            word(&state["registers"]["esp"]),
            "genuine leaf {} esp",
            self.index
        );
        assert_eq!(m.heap_ptr(), word(&state["heap_ptr"]));
    }
    fn observe(&mut self, m: &Space, target: u32, args: &[u32]) {
        let event = &self.data["events"][self.index];
        assert_eq!(
            word(&event["target"]),
            target,
            "genuine leaf {} target",
            self.index
        );
        assert_eq!(event["args"], serde_json::json!(args));
        self.compare(m, &event["state"]);
        self.index += 1;
    }
}
impl crate::native_vm_pool::Allocator for Genuine<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.observe(m, 0x54f560, &[n]);
        Ok(m.alloc(n))
    }
    fn reallocate(&mut self, m: &mut Space, old: u32, old_n: u32, n: u32) -> Result<u32, String> {
        self.observe(m, 0x54f580, &[old, n]);
        let p = m.alloc(n);
        for i in 0..old_n.min(n) {
            m.w8(p + i, m.r8(old + i));
        }
        Ok(p)
    }
}
impl crate::native_vm_context::Environment for Genuine<'_> {}
impl Environment for Genuine<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        self.observe(m, 0x54f5b0, &[p]);
        Ok(())
    }
}
impl crate::native_keyboard::Environment for Genuine<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        leaf: crate::native_keyboard::Leaf,
        target: u32,
        args: &[u32],
    ) -> Result<u32, String> {
        use crate::native_keyboard::Leaf;
        self.observe(m, target, args);
        match leaf {
            Leaf::Acquire => {}
            Leaf::GetDeviceData => m.w32(args[3], 0),
            _ => panic!("outside original quiescent keyboard profile"),
        }
        Ok(0)
    }
}
#[test]
fn genuine_stock_osd_last_key_dispatch_matches_every_observed_page_and_leaf() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(path) = crate::oracle_fixture::file(
        &root.join("re/out/boot/native-vm-dispatch-genuine-original.json"),
    ) else {
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["fpu_cw"], 0x3f);
    assert_eq!(
        data["root_identity"]["class_descriptor"],
        "Ljava.render.Osd;"
    );
    assert_eq!(data["root_identity"]["method"], "animate");
    assert_eq!(
        data["comparison_policy"]["retained_word_exclusions"],
        serde_json::json!([])
    );
    assert_eq!(
        data["execution_profile"]["vm_script_native_result_hooks"],
        serde_json::json!([])
    );
    assert_eq!(
        data["snapshot_provenance"]["exe_sha256"],
        Value::String(format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        ))
    );
    let producers = data["producer_sources"].as_object().unwrap();
    assert_eq!(producers.len(), 5);
    for name in [
        "native_vm_dispatch_genuine_probe.py",
        "native_osd_first_animate_probe.py",
        "snapshot_replay.py",
        "headless_snapshot.py",
        "oracle.py",
    ] {
        assert_eq!(
            producers[name],
            Value::String(format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            ))
        );
    }
    let mut m = Space::new();
    let parent = stack::enter(&mut m, &[], None);
    assert_eq!(data["pages"].as_array().unwrap().len(), 18);
    let mut addresses = std::collections::HashSet::new();
    for page in data["pages"].as_array().unwrap() {
        let address = word(&page["address"]);
        assert_eq!(address & 4095, 0);
        assert!(addresses.insert(address));
        let bytes = hex_bytes(&page["bytes"]);
        assert_eq!(bytes.len(), 4096);
        for (i, b) in bytes.into_iter().enumerate() {
            m.w8(address + i as u32, b);
        }
    }
    m.set_heap_ptr(word(&data["before"]["heap_ptr"]));
    for (reg, name) in REGS {
        stack::set_reg(&mut m, reg, word(&data["before"]["registers"][name]));
    }
    let args = data["args"]
        .as_array()
        .unwrap()
        .iter()
        .map(word)
        .collect::<Vec<_>>();
    assert_eq!(args.len(), 3);
    stack::set_sp(&mut m, word(&data["entry_sp"]) + 16);
    stack::begin_call(
        &mut m,
        word(&data["return_pc"]),
        &args,
        Some(word(&data["thread"])),
    );
    let mut env = Genuine {
        data: &data,
        index: 0,
    };
    env.compare(&m, &data["before"]);
    let pending = begin_cached(&mut m).unwrap();
    let pending = select_native(&mut m, pending).unwrap();
    let pending = prepare_arguments(&mut m, pending, &mut env).unwrap();
    let result = crate::native_vm_abi::call_prepared(&mut m, |m, target, info, kind| {
        assert_eq!(target, 0x47cc10);
        assert_eq!(kind, crate::native_vm_abi::ReturnKind::Integer);
        crate::native_keyboard::last_key(m, info, &mut env)
    })
    .unwrap()
    .unwrap();
    finish_cached(
        &mut m,
        pending,
        crate::native_vm_abi::ReturnKind::Integer,
        result,
        &mut env,
    )
    .unwrap();
    assert_eq!(env.index, data["events"].as_array().unwrap().len());
    env.compare(&m, &data["after"]);
    stack::leave(&mut m, parent);
}

#[test]
fn genuine_stock_osd_animate_materializes_signature_from_original_names() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    for (file, endpoint) in [
        (
            "native-vm-dispatch-genuine-animate-prefix-original.json",
            "prefix",
        ),
        (
            "native-vm-dispatch-genuine-animate-frame-original.json",
            "script-frame",
        ),
        (
            "native-vm-dispatch-genuine-animate-return-original.json",
            "return",
        ),
    ] {
        let Some(path) = crate::oracle_fixture::file(&root.join("re/out/boot").join(file)) else {
            continue;
        };
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["fpu_cw"], 0x3f);
        assert_eq!(data["method"], "animate");
        assert_eq!(data["endpoint"], endpoint);
        assert_eq!(
            data["root_identity"]["class_descriptor"],
            "Ljava.render.Osd;"
        );
        assert_eq!(
            data["comparison_policy"]["retained_word_exclusions"],
            serde_json::json!([])
        );
        assert_eq!(
            data["execution_profile"]["vm_script_native_result_hooks"],
            serde_json::json!([])
        );
        let producers = data["producer_sources"].as_object().unwrap();
        assert_eq!(producers.len(), 5);
        for name in [
            "native_vm_dispatch_genuine_probe.py",
            "native_osd_first_animate_probe.py",
            "snapshot_replay.py",
            "headless_snapshot.py",
            "oracle.py",
        ] {
            assert_eq!(
                producers[name],
                Value::String(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                ))
            );
        }
        let mut m = Space::new();
        let parent = stack::enter(&mut m, &[], None);
        let mut addresses = std::collections::HashSet::new();
        for page in data["pages"].as_array().unwrap() {
            let address = word(&page["address"]);
            assert_eq!(address & 4095, 0);
            assert!(addresses.insert(address));
            let bytes = hex_bytes(&page["bytes"]);
            assert_eq!(bytes.len(), 4096);
            for (i, b) in bytes.into_iter().enumerate() {
                m.w8(address + i as u32, b);
            }
        }
        m.set_heap_ptr(word(&data["before"]["heap_ptr"]));
        for (reg, name) in REGS {
            stack::set_reg(&mut m, reg, word(&data["before"]["registers"][name]));
        }
        let args = data["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(word)
            .collect::<Vec<_>>();
        assert_eq!(args.len(), 3);
        let method = m.r32(args[0] + 8);
        assert_eq!(m.r32(method + 24), 0, "genuine uncached method entry");
        stack::set_sp(&mut m, word(&data["entry_sp"]) + 16);
        stack::begin_call(
            &mut m,
            word(&data["return_pc"]),
            &args,
            Some(word(&data["thread"])),
        );
        let mut env = Genuine {
            data: &data,
            index: 0,
        };
        env.compare(&m, &data["before"]);
        assert!(begin_cached(&mut m).is_err());
        env.compare(&m, &data["before"]);
        let mut pending = begin_materialize(&mut m, &mut env).unwrap();
        assert_eq!(pending.pc, 0x41fc31);
        if endpoint != "prefix" {
            pending = prepare_script_frame(&mut m, pending, &mut env).unwrap();
            assert_eq!(pending.pc, 0x4202fe);
        }
        if endpoint == "return" {
            finish_script_zero_arguments(&mut m, pending, &mut env).unwrap();
            env.compare(&m, &data["after"]);
        } else {
            env.compare(&m, &data["after"]);
            stack::leave(&mut m, pending.frame);
        }
        assert_eq!(env.index, data["events"].as_array().unwrap().len());
        assert_ne!(m.r32(method + 24), 0);
        stack::leave(&mut m, parent);
    }
}

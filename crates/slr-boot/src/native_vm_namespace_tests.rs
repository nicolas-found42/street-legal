use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
const BASE: u32 = 0x20000000;
const HEAP: usize = 0x2000;
const STACK: u32 = 0x7feffb00;
const TABLES: [u32; 3] = [0x7663d8, 0x7685e0, 0x768660];
thread_local! { static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const {std::cell::RefCell::new(Vec::new())}; }
fn note(p: u32, n: u32, v: u32) {
    STORES.with(|s| s.borrow_mut().push((p, n, v)));
}
struct Observe;
impl Drop for Observe {
    fn drop(&mut self) {
        set_test_store_observer(None);
    }
}
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().expect("word")).unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|v| u8::try_from(word(v)).unwrap())
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
fn compare(m: &Space, state: &Value, label: &str) {
    for (name, base, size) in [("heap", BASE, HEAP), ("stack", STACK, 0x500)] {
        let expected = bytes(&state[name]);
        assert_eq!(expected.len(), size, "{label} {name} size");
        let difference = expected
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let actual = m.r8(base + i as u32);
                (actual != *b).then_some((base + i as u32, actual, *b))
            })
            .take(8)
            .collect::<Vec<_>>();
        assert!(difference.is_empty(), "{label} {name} {difference:x?}");
    }
    assert_eq!(
        m.heap_ptr(),
        word(&state["heap_ptr"]),
        "{label} heap cursor"
    );
    assert_eq!(state["tables"].as_object().unwrap().len(), TABLES.len());
    for base in TABLES {
        let expected = state["tables"][base.to_string()].as_array().unwrap();
        assert_eq!(expected.len(), 32);
        for (i, v) in expected.iter().enumerate() {
            assert_eq!(
                m.r32(base + i as u32 * 4),
                word(v),
                "{label} table{base:x}/{i}"
            );
        }
    }
    assert_eq!(state["registers"].as_object().unwrap().len(), 8);
    for (r, name) in regs() {
        assert_eq!(
            stack::reg(m, r),
            word(&state["registers"][name]),
            "{label} {name}"
        );
    }
    assert_eq!(
        stack::sp(m),
        word(&state["registers"]["esp"]),
        "{label} esp"
    );
}
struct Env<'a> {
    events: &'a [Value],
    index: usize,
}
impl Env<'_> {
    fn observe(&mut self, m: &Space, kind: &str, args: &[u32]) {
        let event = &self.events[self.index];
        assert_eq!(event["kind"], kind);
        assert_eq!(event["args"], json!(args));
        compare(m, &event["state"], &format!("{kind}{}", self.index));
        self.index += 1;
    }
}
impl Allocator for Env<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.observe(m, "allocate", &[n]);
        Ok(m.alloc(n))
    }
    fn reallocate(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<u32, String> {
        Err("outside constructor/next/close profile".into())
    }
}
impl Environment for Env<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.observe(m, "free", &[p]);
        Ok(0)
    }
    fn find_first(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
        Err("outside raw-record profile".into())
    }
    fn find_next(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
        Err("outside raw-record profile".into())
    }
    fn find_close(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("outside raw-record profile".into())
    }
}
fn inventory() -> Vec<Value> {
    let mut rows = vec![];
    for fill in [0, 0xa5] {
        for parent in [false, true] {
            for name in ["".to_owned(), "java".into(), "n".repeat(255)] {
                rows.push(json!({"kind":"construct","fill":fill,"parent":parent,"name":name}));
            }
        }
    }
    for fill in [0, 0xa5] {
        for handle in [0, 31] {
            for position in ["missing", "first", "last", "exhausted"] {
                rows.push(json!({"kind":"next","fill":fill,"handle":handle,"position":position}));
            }
        }
    }
    for fill in [0, 0xa5] {
        for (handle, occupied) in [
            (0, false),
            (0, true),
            (31, false),
            (31, true),
            (u32::MAX, false),
            (32, false),
        ] {
            rows.push(json!({"kind":"close","fill":fill,"handle":handle,"occupied":occupied}));
        }
    }
    rows
}
#[test]
fn original_root_constructor_and_raw_next_close_preserve_complete_bounded_machine_states() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixture: Value = serde_json::from_slice(
        &std::fs::read(root.join("re/out/boot/native-vm-namespace-original.json"))
            .expect("run native_vm_namespace_probe.py"),
    )
    .unwrap();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["cw"], 127);
    assert_eq!(fixture["unicorn"], "2.1.4");
    assert_eq!(
        fixture["exe_sha256"],
        format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        )
    );
    assert_eq!(fixture["producer_sources"].as_object().unwrap().len(), 2);
    for name in ["native_vm_namespace_probe.py", "oracle.py"] {
        assert_eq!(
            fixture["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let rows = fixture["rows"].as_array().unwrap();
    let expected = inventory();
    assert_eq!(rows.len(), 40);
    assert_eq!(rows.len(), expected.len());
    for (index, (row, input)) in rows.iter().zip(expected).enumerate() {
        assert_eq!(row["input"], input, "row{index} exact inventory");
        let mut m = Space::new();
        for (name, base, n) in [("heap", BASE, HEAP), ("stack", STACK, 0x500)] {
            let b = bytes(&row["before"][name]);
            assert_eq!(b.len(), n);
            for (i, b) in b.into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
        }
        m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
        assert_eq!(row["before"]["tables"].as_object().unwrap().len(), 3);
        for base in TABLES {
            let values = row["before"]["tables"][base.to_string()]
                .as_array()
                .unwrap();
            assert_eq!(values.len(), 32);
            for (i, v) in values.iter().enumerate() {
                m.w32(base + i as u32 * 4, word(v));
            }
        }
        for (r, name) in regs() {
            set(&mut m, r, word(&row["register_inputs"][name]));
        }
        let args = row["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(word)
            .collect::<Vec<_>>();
        match input["kind"].as_str().unwrap() {
            "construct" => {
                assert_eq!(word(&row["target"]), 0x41cc00);
                assert_eq!(args.len(), 3);
                assert_eq!(args[1], 0);
                assert_eq!(args[0] != 0, input["parent"].as_bool().unwrap());
                let name = input["name"].as_str().unwrap().as_bytes();
                for (i, byte) in name.iter().copied().chain(std::iter::once(0)).enumerate() {
                    assert_eq!(
                        m.r8(args[2] + i as u32),
                        byte,
                        "row{index} declared original name bytes"
                    );
                }
            }
            "next" => {
                assert_eq!(word(&row["target"]), 0x54d1c0);
                assert_eq!(args.len(), 3);
                assert_eq!(args[0], word(&input["handle"]));
                let position = input["position"].as_str().unwrap();
                let records = m.r32(TABLES[0] + args[0] * 4);
                assert_eq!(records == 0, position == "missing");
                assert_eq!(
                    m.r32(TABLES[1] + args[0] * 4),
                    match position {
                        "missing" | "first" => 0,
                        "last" => 1,
                        "exhausted" => 2,
                        _ => unreachable!(),
                    }
                );
                assert_eq!(m.r32(TABLES[2] + args[0] * 4), 2);
                if records != 0 {
                    assert_eq!(m.r8(records), 0);
                    assert!((0..255).all(|i| m.r8(records + 260 + i) == b'n'));
                    assert_eq!(m.r8(records + 260 + 255), 0);
                    assert_eq!(m.r32(records + 256), 0x12345678);
                    assert_eq!(m.r32(records + 516), 0x12345679);
                }
            }
            "close" => {
                assert_eq!(word(&row["target"]), 0x54d240);
                assert_eq!(args.len(), 1);
                assert_eq!(args[0], word(&input["handle"]));
                if args[0] < 32 {
                    assert_eq!(
                        m.r32(TABLES[0] + args[0] * 4) != 0,
                        input["occupied"].as_bool().unwrap()
                    );
                    assert_eq!(m.r32(TABLES[1] + args[0] * 4), 3);
                    assert_eq!(m.r32(TABLES[2] + args[0] * 4), 7);
                }
            }
            _ => unreachable!(),
        }
        let sp = ((0x7ff00000u32 - 0x100 - args.len() as u32 * 4 - 4) & !15) - 4;
        stack::set_sp(&mut m, sp);
        m.w32(sp, 0x7fff0000);
        for (i, arg) in args.iter().enumerate() {
            m.w32(sp + 4 + i as u32 * 4, *arg);
        }
        m.w32(0x7ffdf004, 1);
        STORES.with(|s| s.borrow_mut().clear());
        set_test_store_observer(Some(note));
        let _observe = Observe;
        let mut env = Env {
            events: row["events"].as_array().unwrap(),
            index: 0,
        };
        let result = match input["kind"].as_str().unwrap() {
            "construct" => construct(
                &mut m,
                word(&row["this"]),
                args[0],
                args[1],
                args[2],
                &mut env,
            ),
            "next" => raw_find_next(&mut m, args[0], args[1], args[2]),
            "close" => raw_find_close(&mut m, args[0], &mut env),
            _ => unreachable!(),
        }
        .unwrap();
        assert_eq!(result, word(&row["result"]), "row{index} result");
        assert_eq!(env.index, env.events.len());
        compare(&m, &row["after"], &format!("row{index} final"));
        let stores = row["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (word(&v[0]), word(&v[1]), word(&v[2])))
            .collect::<Vec<_>>();
        STORES.with(|actual| assert_eq!(*actual.borrow(), stores, "row{index} ordered stores"));
    }
}

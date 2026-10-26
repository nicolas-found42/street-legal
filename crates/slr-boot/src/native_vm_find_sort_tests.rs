use super::*;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
thread_local! {static WRITES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
fn record(p: u32, n: u32, v: u32) {
    WRITES.with(|w| w.borrow_mut().push((p, n, v)));
}
struct Observe;
impl Drop for Observe {
    fn drop(&mut self) {
        set_test_store_observer(None);
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
fn names() -> Vec<Vec<String>> {
    vec![
        vec![],
        vec!["z".into()],
        vec!["b".into(), "A".into()],
        "hgfedcba".chars().map(|c| c.to_string()).collect(),
        ["a", "B", "a", "b", "A", "C", "c", "b", "A"]
            .map(str::to_owned)
            .to_vec(),
        (0..17).map(|i| format!("n{i:02}")).collect(),
        (0..51).rev().map(|i| format!("n{i:02}")).collect(),
    ]
}
#[test]
fn actual_crt_find_qsort_preserves_every_heap_swap_and_complete_stack_register_state() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let value: Value = serde_json::from_slice(
        &std::fs::read(root.join("re/out/boot/native-vm-find-sort-original.json"))
            .expect("run native_vm_find_sort_probe.py"),
    )
    .unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["unicorn"], "2.1.4");
    assert_eq!(value["cw"], 127);
    assert_eq!(
        value["exe_sha256"],
        format!(
            "{:x}",
            Sha256::digest(
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
            )
        )
    );
    assert_eq!(value["producer_sources"].as_object().unwrap().len(), 2);
    for name in ["native_vm_find_sort_probe.py", "oracle.py"] {
        assert_eq!(
            value["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let inventory = [0u8, 0xa5]
        .into_iter()
        .flat_map(|fill| names().into_iter().map(move |names| (fill, names)))
        .collect::<Vec<_>>();
    let rows = value["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 14);
    assert_eq!(rows.len(), inventory.len());
    for (index, (row, (fill, names))) in rows.iter().zip(inventory).enumerate() {
        assert_eq!(row["input"], json!({"fill":fill,"names":names}));
        let mut m = Space::new();
        for (base, n) in [(0x20000000, 0x8000), (0x7feff000, 0x1000)] {
            for i in 0..n {
                m.w8(base + i, fill);
            }
        }
        let base = 0x20000000 + if fill == 0 { 0 } else { 48 };
        assert_eq!(word(&row["base"]), base);
        for (i, name) in names.iter().enumerate() {
            for (j, b) in name.bytes().chain(std::iter::once(0)).enumerate() {
                m.w8(base + i as u32 * 260 + j as u32, b);
            }
            m.w32(base + i as u32 * 260 + 256, 0x10000000 + i as u32);
        }
        let heap_end = base + (names.len() as u32 * 260).max(1);
        m.set_heap_ptr(heap_end);
        assert_eq!(word(&row["before"]["heap_ptr"]), heap_end);
        for (name, address, n) in [
            ("heap", 0x20000000, 0x8000usize),
            ("stack", 0x7feff000, 0x1000),
        ] {
            let before = bytes(&row["before"][name]);
            assert_eq!(before.len(), n);
            assert!(
                before
                    .iter()
                    .enumerate()
                    .all(|(i, b)| m.r8(address + i as u32) == *b),
                "row{index} declared{name} input"
            );
        }
        for (reg, name) in regs() {
            set(&mut m, reg, word(&row["register_inputs"][name]));
        }
        let args = [base, names.len() as u32, 260, 0x54d1a0];
        let sp = ((0x7ff00000u32 - 0x100 - 20) & !15) - 4;
        stack::set_sp(&mut m, sp);
        m.w32(sp, 0x7fff0000);
        for (i, arg) in args.iter().enumerate() {
            m.w32(sp + 4 + i as u32 * 4, *arg);
        }
        m.w32(0x7ffdf004, 1);
        WRITES.with(|w| w.borrow_mut().clear());
        set_test_store_observer(Some(record));
        let _observe = Observe;
        sort(&mut m, base, names.len() as u32, 260, 0x54d1a0).unwrap();
        for (name, address, n) in [
            ("heap", 0x20000000, 0x8000usize),
            ("stack", 0x7feff000, 0x1000),
        ] {
            let expected = bytes(&row["after"][name]);
            assert_eq!(expected.len(), n);
            let differences = expected
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let actual = m.r8(address + i as u32);
                    (actual != *b).then_some((address + i as u32, actual, *b))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(differences.is_empty(), "row{index} {name} {differences:x?}");
        }
        for (reg, name) in regs() {
            assert_eq!(
                r(&m, reg),
                word(&row["after"]["registers"][name]),
                "row{index} {name}"
            );
        }
        assert_eq!(stack::sp(&m), word(&row["after"]["registers"]["esp"]));
        assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]));
        let expected = row["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| (word(&v[0]), word(&v[1]), word(&v[2])))
            .collect::<Vec<_>>();
        WRITES.with(|w| {
            let actual = w.borrow();
            assert_eq!(actual.len(), expected.len(), "row{index} storecount");
            assert_eq!(
                actual.iter().zip(&expected).position(|(a, b)| a != b),
                None,
                "row{index} orderedrawswapwrites"
            );
        });
    }
}

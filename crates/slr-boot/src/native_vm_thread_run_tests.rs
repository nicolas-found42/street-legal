use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use slr_x87::X87;
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
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|b| u8::from_str_radix(std::str::from_utf8(b).unwrap(), 16).unwrap())
        .collect()
}
fn put(m: &mut Space, p: u32, v: &Value) {
    for (i, b) in bytes(v).into_iter().enumerate() {
        m.w8(p + i as u32, b);
    }
}
fn state(v: &Value) -> native_vm_fpu::State {
    let regs = std::array::from_fn(|i| {
        let mut b = [0; 10];
        b[..8].copy_from_slice(&v["registers"][i][0].as_u64().unwrap().to_le_bytes());
        b[8..].copy_from_slice(
            &u16::try_from(word(&v["registers"][i][1]))
                .unwrap()
                .to_le_bytes(),
        );
        X87::from_bytes(b)
    });
    native_vm_fpu::State::from_saved(
        word(&v["control"]) as u16,
        word(&v["status"]) as u16,
        word(&v["tags"]) as u16,
        regs,
    )
}
fn compare_fpu(fpu: &native_vm_fpu::State, v: &Value, status: u16) {
    assert_eq!(fpu.control(), word(&v["control"]) as u16, "FPCW");
    assert_eq!(
        fpu.status(),
        status,
        "FPSW (separate x87 referee for arithmetic PE)"
    );
    assert_eq!(fpu.tags(), word(&v["tags"]) as u16, "FPTAG");
    assert_eq!(
        fpu.registers().map(|v| v.to_bytes()),
        state(v).registers().map(|v| v.to_bytes()),
        "all physical x87 register bits, including empty retained slots"
    );
}
fn compare(m: &Space, v: &Value) {
    for (field, base, len) in [
        ("heap", 0x20000000, 0x2000),
        ("stack", 0x7feff800, 0x800),
        ("counters", 0x76a280, 24),
    ] {
        let expected = bytes(&v[field]);
        assert_eq!(expected.len(), len);
        let diff = expected
            .iter()
            .enumerate()
            .filter_map(|(i, b)| {
                let got = m.r8(base + i as u32);
                (got != *b).then_some((base + i as u32, got, *b))
            })
            .take(8)
            .collect::<Vec<_>>();
        assert!(diff.is_empty(), "{field}: {diff:x?}");
    }
    for (reg, name) in REGS {
        assert_eq!(r(m, reg), word(&v["registers"][name]), "{name}");
    }
    assert_eq!(stack::sp(m), word(&v["registers"]["esp"]));
    assert_eq!(m.heap_ptr(), word(&v["heap_ptr"]));
}
struct Platform<'a> {
    counter: crate::native_vm_counter::Counter,
    row: &'a Value,
    index: usize,
}
impl Platform<'_> {
    fn query(&mut self, m: &mut Space, destination: u32, kind: &str) -> Result<u32, String> {
        let event = &self.row["events"][self.index];
        self.index += 1;
        assert_eq!(event["kind"], kind);
        assert_eq!(word(&event["args"][0]), destination);
        compare(m, &event["state"]);
        if kind == "frequency" {
            self.counter.frequency(m, destination)
        } else {
            self.counter.counter(m, destination)
        }
    }
}
impl crate::native_vm_pool::Allocator for Platform<'_> {
    fn allocate(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("clock-only profile must not allocate".into())
    }
    fn reallocate(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<u32, String> {
        Err("clock-only profile must not reallocate".into())
    }
}
impl crate::native_vm_dispatch::Environment for Platform<'_> {
    fn free(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
        Err("clock-only profile must not free".into())
    }
}
impl crate::native_vm_lookup::Environment for Platform<'_> {}
impl Environment for Platform<'_> {
    fn query_frequency(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "frequency")
    }
    fn query_counter(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "counter")
    }
    fn observe(&mut self, m: &Space, fpu: &native_vm_fpu::State, pc: u32) {
        assert_eq!(pc, 0x421061);
        compare(m, &self.row["before_clock"]);
        compare_fpu(
            fpu,
            &self.row["before_clock"]["fpu"],
            word(&self.row["before_clock"]["fpu"]["status"]) as u16,
        );
    }
}
#[test]
fn original_run_entry_clock_and_first_opcode_match_complete_memory_and_registers() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join("re/out/boot/native-vm-thread-run-original.json");
    let Some(path) = crate::oracle_fixture::file(&path) else {
        return;
    };
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(v["schema"], 1);
    assert_eq!(
        v["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(v["producer_sources"].as_object().unwrap().len(), 3);
    for name in [
        "native_vm_thread_run_probe.py",
        "oracle.py",
        "thread_run_fpu_referee.c",
    ] {
        assert_eq!(
            v["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let rows = v["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 48);
    let mut inventory = std::collections::HashSet::new();
    for row in rows {
        assert!(inventory.insert((
            word(&row["seed"]),
            word(&row["fill"]),
            word(&row["budget"]),
            word(&row["cw"]),
            word(&row["depth"])
        )));
        let mut m = Space::new();
        let parent = stack::enter(&mut m, &[], None);
        for (field, base) in [
            ("heap", 0x20000000),
            ("stack", 0x7feff800),
            ("counters", 0x76a280),
        ] {
            put(&mut m, base, &row["before"][field]);
        }
        m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
        m.w32(0x5e73cc, 0);
        m.w32(0x5f0910, 1000f32.to_bits());
        for (reg, name) in REGS {
            s(&mut m, reg, word(&row["before"]["registers"][name]));
        }
        let entry = word(&row["before"]["registers"]["esp"]);
        stack::set_sp(&mut m, entry + 8);
        stack::begin_call(
            &mut m,
            word(&row["return_pc"]),
            &[word(&row["budget"])],
            Some(word(&row["thread"])),
        );
        compare(&m, &row["before"]);
        let mut fpu = state(&row["before"]["fpu"]);
        let mut env = Platform {
            counter: crate::native_vm_counter::Counter::new(
                &m,
                &fpu,
                slr_vm::clock::ClockSite::ThreadEnter,
                slr_vm::clock::CounterInput {
                    ordinal: 1,
                    frequency: row["frequency"].as_i64().unwrap(),
                    counter: row["counter"].as_i64().unwrap(),
                    baseline: 1000000,
                    source_control: word(&row["cw"]) as u16,
                },
            )
            .unwrap(),
            row,
            index: 0,
        };
        let run = begin(&mut m, &mut fpu, &mut env).unwrap();
        assert_eq!(run.pc, 0x4210c5);
        assert_eq!(env.index, 2);
        compare(&m, &row["after"]);
        env.counter.finish().unwrap();
        let referee = word(&row["referee"]["after"]) as u16;
        let unicorn = word(&row["after"]["fpu"]["status"]) as u16;
        assert!(
            matches!(referee ^ unicorn, 0 | 0x20),
            "only independently observed arithmetic PE disagreement is admitted"
        );
        compare_fpu(&fpu, &row["after"]["fpu"], referee);
        stack::leave(&mut m, run.caller);
        stack::leave(&mut m, parent);
    }
}

struct Genuine<'a> {
    data: &'a Value,
    index: usize,
}
impl Genuine<'_> {
    fn compare(&self, m: &Space, value: &Value) {
        for page in self.data["pages"].as_array().unwrap() {
            let address = word(&page["address"]);
            let expected = value["changed"]
                .get(address.to_string())
                .unwrap_or(&page["bytes"]);
            let bytes = bytes(expected);
            assert_eq!(bytes.len(), 4096);
            let diff = bytes
                .iter()
                .enumerate()
                .filter_map(|(i, b)| {
                    let got = m.r8(address + i as u32);
                    (got != *b).then_some((address + i as u32, got, *b))
                })
                .take(8)
                .collect::<Vec<_>>();
            assert!(
                diff.is_empty(),
                "genuine event {} memory {diff:x?}",
                self.index
            );
        }
        for (reg, name) in REGS {
            assert_eq!(
                r(m, reg),
                word(&value["registers"][name]),
                "genuine event {} {name}",
                self.index
            );
        }
        assert_eq!(stack::sp(m), word(&value["registers"]["esp"]));
        assert_eq!(m.heap_ptr(), word(&value["heap_ptr"]));
    }
    fn query(&mut self, m: &mut Space, destination: u32, kind: &str) -> Result<u32, String> {
        let event = &self.data["events"][self.index];
        assert_eq!(event["kind"], kind);
        assert_eq!(event["args"], serde_json::json!([destination]));
        self.compare(m, &event["state"]);
        self.index += 1;
        // The captured original's Win32 stub executes XOR EAX,EAX; RET4,
        // leaving the persistent output qword unchanged. No clock result hook.
        assert_eq!(m.r32(destination), word(&event["value"][0]));
        assert_eq!(m.r32(destination + 4), word(&event["value"][1]));
        Ok(0)
    }
}
impl crate::native_vm_lookup::Environment for Genuine<'_> {}
impl Environment for Genuine<'_> {
    fn query_frequency(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "frequency")
    }
    fn query_counter(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "counter")
    }
    fn observe(&mut self, m: &Space, fpu: &native_vm_fpu::State, pc: u32) {
        assert_eq!(pc, 0x421061);
        self.compare(m, &self.data["before_clock"]);
        compare_fpu(
            fpu,
            &self.data["before_clock"]["fpu"],
            word(&self.data["before_clock"]["fpu"]["status"]) as u16,
        );
    }
}
fn genuine_case(file: &str, dispatches: u32, expected_pages: usize) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(path) = crate::oracle_fixture::file(&root.join(file)) else {
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["dispatches"], dispatches);
    assert_eq!(data["before"]["fpu"]["control"], 0x3f);
    assert_eq!(data["root_identity"]["method"], "animate");
    assert_eq!(
        data["root_identity"]["class_descriptor"],
        "Ljava.render.Osd;"
    );
    assert_eq!(
        data["execution_profile"]["vm_script_native_result_hooks"],
        serde_json::json!([])
    );
    assert_eq!(
        data["comparison_policy"]["retained_word_exclusions"],
        serde_json::json!([])
    );
    assert_eq!(
        data["snapshot_provenance"]["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(data["producer_sources"].as_object().unwrap().len(), 5);
    for name in [
        "native_vm_thread_run_genuine_probe.py",
        "native_osd_first_animate_probe.py",
        "snapshot_replay.py",
        "headless_snapshot.py",
        "oracle.py",
    ] {
        assert_eq!(
            data["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }
    let mut m = Space::new();
    let parent = stack::enter(&mut m, &[], None);
    let mut pages = std::collections::HashSet::new();
    assert_eq!(data["pages"].as_array().unwrap().len(), expected_pages);
    for page in data["pages"].as_array().unwrap() {
        let address = word(&page["address"]);
        assert_eq!(address & 4095, 0);
        assert!(pages.insert(address));
        assert_eq!(bytes(&page["bytes"]).len(), 4096);
        put(&mut m, address, &page["bytes"]);
    }
    m.set_heap_ptr(word(&data["before"]["heap_ptr"]));
    for (reg, name) in REGS {
        s(&mut m, reg, word(&data["before"]["registers"][name]));
    }
    stack::set_sp(&mut m, word(&data["entry_sp"]) + 8);
    stack::begin_call(
        &mut m,
        word(&data["return_pc"]),
        &[word(&data["budget"])],
        Some(word(&data["thread"])),
    );
    let mut env = Genuine {
        data: &data,
        index: 0,
    };
    env.compare(&m, &data["before"]);
    let mut fpu = state(&data["before"]["fpu"]);
    let mut run = begin(&mut m, &mut fpu, &mut env).unwrap();
    env.compare(&m, &data["opcodes"][0]["state"]);
    for index in 1..dispatches as usize {
        step(&mut m, &mut run, &mut fpu, &mut env)
            .unwrap_or_else(|error| panic!("opcode index {}: {error}", index - 1));
        env.compare(&m, &data["opcodes"][index]["state"]);
    }
    if data["native_entry"] == true {
        step(&mut m, &mut run, &mut fpu, &mut env).unwrap();
    }
    env.compare(&m, &data["after"]);
    assert_eq!(env.index, data["events"].as_array().unwrap().len());
    stack::leave(&mut m, run.caller);
    stack::leave(&mut m, parent);
}

#[test]
fn genuine_stock_osd_run_entry_and_first_opcodes_match_all_observed_pages() {
    genuine_case(
        "re/out/boot/native-vm-run-genuine-step1-original.json",
        2,
        14,
    );
    genuine_case(
        "re/out/boot/native-vm-run-genuine-step2-original.json",
        3,
        20,
    );
}

#[test]
fn genuine_stock_osd_complete_interpreter_context_reaches_first_lastkey_entry() {
    genuine_case(
        "re/out/boot/native-vm-run-genuine-lastkey-original.json",
        63,
        86,
    );
}

impl crate::native_vm_pool::Allocator for Genuine<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        let event = &self.data["events"][self.index];
        assert_eq!(event["kind"], "external");
        assert_eq!(event["target"], 0x54f560);
        assert_eq!(event["args"], serde_json::json!([n]));
        self.compare(m, &event["state"]);
        self.index += 1;
        Ok(m.alloc(n))
    }
    fn reallocate(
        &mut self,
        m: &mut Space,
        old: u32,
        old_bytes: u32,
        n: u32,
    ) -> Result<u32, String> {
        let event = &self.data["events"][self.index];
        assert_eq!(event["kind"], "external");
        assert_eq!(event["target"], 0x54f580);
        assert_eq!(event["args"], serde_json::json!([old, n]));
        self.compare(m, &event["state"]);
        self.index += 1;
        let p = m.alloc(n);
        for i in 0..old_bytes.min(n) {
            m.w8(p + i, m.r8(old + i));
        }
        Ok(p)
    }
    fn deallocate(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        let event = &self.data["events"][self.index];
        assert_eq!(event["kind"], "external");
        assert_eq!(event["target"], 0x54f5b0);
        assert_eq!(event["args"], serde_json::json!([p]));
        self.compare(m, &event["state"]);
        self.index += 1;
        Ok(())
    }
}
impl crate::native_vm_dispatch::Environment for Genuine<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        crate::native_vm_pool::Allocator::deallocate(self, m, p)
    }
}
fn callback_genuine_case(file: &str, dispatches: u32, expected_pages: usize) {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(path) = crate::oracle_fixture::file(&root.join(file)) else {
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 2);
    assert_eq!(data["qualified_clock_flags"], true);
    let clock_errata = data["clock_errata"].as_array().unwrap();
    assert_eq!(clock_errata.len(), usize::from(dispatches != 0));
    for erratum in clock_errata {
        assert_eq!(erratum["pc"], 0x5516f2);
        assert_eq!(
            word(&erratum["native"]["status"]) ^ word(&erratum["unicorn_status"]),
            0x20
        );
    }
    assert_eq!(data["dispatches"], dispatches);
    assert_eq!(data["before"]["fpu"]["control"], 0x3f);
    assert_eq!(data["root_identity"]["method"], "animate");
    assert_eq!(
        data["root_identity"]["class_descriptor"],
        "Ljava.render.Osd;"
    );
    assert_eq!(
        data["execution_profile"]["vm_script_native_result_hooks"],
        serde_json::json!([])
    );
    assert_eq!(
        data["comparison_policy"]["retained_word_exclusions"],
        serde_json::json!([])
    );
    assert_eq!(
        data["snapshot_provenance"]["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(data["producer_sources"].as_object().unwrap().len(), 6);
    for name in [
        "native_vm_callback_genuine_probe.py",
        "callback_clock_fpu_referee.c",
        "native_osd_first_animate_probe.py",
        "snapshot_replay.py",
        "headless_snapshot.py",
        "oracle.py",
    ] {
        assert_eq!(
            data["producer_sources"][name],
            format!(
                "{:x}",
                Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
            )
        );
    }

    let mut m = Space::new();
    let parent = stack::enter(&mut m, &[], None);
    assert_eq!(data["pages"].as_array().unwrap().len(), expected_pages);
    let mut pages = std::collections::HashSet::new();
    for page in data["pages"].as_array().unwrap() {
        let address = word(&page["address"]);
        assert_eq!(address & 4095, 0);
        assert!(pages.insert(address));
        assert_eq!(bytes(&page["bytes"]).len(), 4096);
        put(&mut m, address, &page["bytes"]);
    }
    m.set_heap_ptr(word(&data["before"]["heap_ptr"]));
    for (reg, name) in REGS {
        s(&mut m, reg, word(&data["before"]["registers"][name]));
    }
    let args = data["arguments"]
        .as_array()
        .unwrap()
        .iter()
        .map(word)
        .collect::<Vec<_>>();
    stack::set_sp(&mut m, word(&data["entry_sp"]) + 4 + args.len() as u32 * 4);
    stack::begin_call(&mut m, word(&data["return_pc"]), &args, None);
    let mut env = Genuine {
        data: &data,
        index: 0,
    };
    env.compare(&m, &data["before"]);
    let mut fpu = state(&data["before"]["fpu"]);
    let mut callback = crate::native_vm_callback::begin(&mut m, &mut fpu, &mut env).unwrap();
    if dispatches != 0 {
        let mut run = begin(&mut m, &mut fpu, &mut env).unwrap();
        env.compare(&m, &data["opcodes"][0]["state"]);
        compare_fpu(
            &fpu,
            &data["opcodes"][0]["state"]["fpu"],
            word(&data["opcodes"][0]["state"]["fpu"]["status"]) as u16,
        );
        for index in 1..dispatches as usize {
            step(&mut m, &mut run, &mut fpu, &mut env)
                .unwrap_or_else(|error| panic!("composed opcode {}: {error}", index - 1));
            if (data["native_return"] == true || data["callback_return"] == true)
                && run.native.is_some()
            {
                run.resume_native(
                    &mut m,
                    &mut fpu,
                    &mut env,
                    |m, fpu, env, target, info, kind| match target {
                        0x47cc10 => {
                            assert_eq!(kind, crate::native_vm_abi::ReturnKind::Integer);
                            crate::native_keyboard::last_key(m, info, env)
                        }
                        0x47bfe0 => {
                            assert_eq!(kind, crate::native_vm_abi::ReturnKind::Float);
                            current_time(m, info, fpu, env)
                        }
                        _ => panic!(
                            "source native {target:#x} not implemented in this composed comparison"
                        ),
                    },
                )
                .unwrap();
            }
            env.compare(&m, &data["opcodes"][index]["state"]);
            compare_fpu(
                &fpu,
                &data["opcodes"][index]["state"]["fpu"],
                word(&data["opcodes"][index]["state"]["fpu"]["status"]) as u16,
            );
        }
        if data["callback_return"] == true {
            step(&mut m, &mut run, &mut fpu, &mut env).unwrap();
            callback.finish_setup(&mut m, &run, &mut fpu).unwrap();
            env.compare(&m, &data["after"]);
            compare_fpu(
                &fpu,
                &data["after"]["fpu"],
                word(&data["after"]["fpu"]["status"]) as u16,
            );
        } else if data["native_return"] != true {
            step(&mut m, &mut run, &mut fpu, &mut env).unwrap();
            let entry = run.native_entry(&m).unwrap();
            assert_eq!(entry.target(), 0x47cc10);
            assert_eq!(entry.kind(), crate::native_vm_abi::ReturnKind::Integer);
            assert_eq!(m.r32(entry.info() + 4), callback.thread);
            env.compare(&m, &data["after"]);
            compare_fpu(
                &fpu,
                &data["after"]["fpu"],
                word(&data["after"]["fpu"]["status"]) as u16,
            );
            let child = stack::enter_cdecl(&mut m, &[entry.info()]);
            stack::leave(&mut m, child);
            stack::leave(&mut m, run.native.take().unwrap().dispatch.frame);
        } else {
            env.compare(&m, &data["after"]);
            compare_fpu(
                &fpu,
                &data["after"]["fpu"],
                word(&data["after"]["fpu"]["status"]) as u16,
            );
            assert!(run.native_entry(&m).is_err());
        }
        if data["callback_return"] != true {
            stack::leave(&mut m, run.caller);
        }
    } else {
        env.compare(&m, &data["after"]);
        compare_fpu(
            &fpu,
            &data["after"]["fpu"],
            word(&data["after"]["fpu"]["status"]) as u16,
        );
        let child = stack::enter(&mut m, &[0], Some(callback.thread));
        stack::leave(&mut m, child);
    }
    assert_eq!(env.index, data["events"].as_array().unwrap().len());
    if data["callback_return"] != true {
        stack::leave(&mut m, callback.setup);
    }
    stack::add_sp(&mut m, 20);
    stack::leave(&mut m, callback.wrapper);
    stack::leave(&mut m, parent);
}

#[test]
fn genuine_stock_osd_callback_produces_thread_run_caller() {
    callback_genuine_case(
        "re/out/boot/native-vm-callback-genuine-original.json",
        0,
        62,
    );
}
#[test]
fn genuine_stock_osd_callback_and_interpreter_produce_lastkey_entry() {
    callback_genuine_case(
        "re/out/boot/native-vm-callback-lastkey-genuine-original.json",
        63,
        94,
    );
}

impl crate::native_keyboard::Environment for Genuine<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        leaf: crate::native_keyboard::Leaf,
        target: u32,
        args: &[u32],
    ) -> Result<u32, String> {
        let event = &self.data["events"][self.index];
        assert_eq!(event["kind"], "external");
        assert_eq!(word(&event["target"]), target);
        assert_eq!(event["args"], serde_json::json!(args));
        self.compare(m, &event["state"]);
        self.index += 1;
        match leaf {
            crate::native_keyboard::Leaf::Acquire => {}
            crate::native_keyboard::Leaf::GetDeviceData => m.w32(args[3], 0),
            _ => panic!("outside declared original quiescent DirectInput profile"),
        }
        Ok(0)
    }
}
#[test]
fn genuine_stock_osd_callback_interpreter_and_lastkey_return_match() {
    callback_genuine_case(
        "re/out/boot/native-vm-callback-lastkey-return-genuine-original.json",
        64,
        96,
    );
}

#[test]
fn genuine_stock_osd_complete_callback_returns_through_original_scripts() {
    callback_genuine_case(
        "re/out/boot/native-vm-callback-return-genuine-original.json",
        128,
        107,
    );
}

#[derive(Default)]
struct LiteralObserver {
    events: Vec<(&'static str, u32, u32, u32)>,
}
impl crate::native_vm_pool::Allocator for LiteralObserver {
    fn allocate(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("literal regression must use its existing pool/vector storage".into())
    }
    fn reallocate(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<u32, String> {
        Err("literal regression must use its existing pool/vector storage".into())
    }
    fn observe_value(
        &mut self,
        m: &Space,
        observation: crate::native_vm_values::Observation,
    ) -> Result<(), String> {
        use crate::native_vm_values::Observation;
        match observation {
            Observation::Allocated { address, .. } => {
                self.events.push(("allocate", address, 0, 0));
            }
            Observation::Constructed { address } => {
                assert_eq!(m.r32(address), 0x5e734c);
                self.events
                    .push(("construct", address, m.r32(address + 4), m.r32(address + 8)));
            }
            _ => return Err("literal regression must not destroy cells".into()),
        }
        Ok(())
    }
}
impl crate::native_vm_dispatch::Environment for LiteralObserver {
    fn free(&mut self, _: &mut Space, _: u32) -> Result<(), String> {
        Err("literal regression must not free storage".into())
    }
}
impl crate::native_vm_lookup::Environment for LiteralObserver {
    fn observe_owner(
        &mut self,
        m: &Space,
        observation: crate::native_vm_context::Observation,
    ) -> Result<(), String> {
        let crate::native_vm_context::Observation::OperandPush {
            frame, index, cell, ..
        } = observation
        else {
            return Err("unexpected literal owner observation".into());
        };
        let vector = m.r32(frame + 0x28);
        assert_eq!(m.r32(vector + 8) as usize, index + 1);
        assert_eq!(m.r32(m.r32(vector + 4) + index as u32 * 4), cell);
        self.events.push(("push", cell, index as u32, 0));
        Ok(())
    }
}
impl Environment for LiteralObserver {
    fn query_frequency(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("literal regression must not read clocks".into())
    }
    fn query_counter(&mut self, _: &mut Space, _: u32) -> Result<u32, String> {
        Err("literal regression must not read clocks".into())
    }
}
#[test]
fn inline_literals_issue_complete_constructor_before_operand_append() {
    for (tag, payload, descriptor_global, descriptor_tag) in [
        (0x100b, 0xdeadbeef, 0x62e00c, b'I'),
        (0x200a, 1.25f32.to_bits(), 0x62deec, b'F'),
        (0xc, 0, 0x62e000, b'L'),
    ] {
        let mut m = Space::new();
        let (thread, frame, vector, data, pool, cell, descriptor, name, pc) = (
            0x20000000, 0x20000100, 0x20000200, 0x20000300, 0x20000400, 0x20000500, 0x20000600,
            0x20000700, 0x20000800,
        );
        m.w32(thread + 0x20, frame);
        m.w32(thread + 0x24, pc);
        m.w32(frame + 0x28, vector);
        m.w32(vector, 4);
        m.w32(vector + 4, data);
        m.w32(vector + 8, 1);
        m.w32(pool + 16, cell);
        m.w32(0x62f130, pool);
        m.w32(descriptor_global, descriptor);
        m.w32(descriptor, name);
        m.w8(name, descriptor_tag);
        m.w32(pc + 8, tag);
        m.w32(pc + 12, payload);
        let caller = stack::enter(&mut m, &[], None);
        stack::alloc(&mut m, 0x460);
        let run = Run {
            caller,
            thread,
            base: stack::sp(&m),
            pc: 0x42108b,
            native: None,
            returned: false,
            budget: 0,
            instruction: None,
        };
        let mut fpu =
            native_vm_fpu::State::from_saved(0x7f, 0, 0xffff, [X87::from_bytes([0; 10]); 8]);
        let mut observer = LiteralObserver::default();
        inline_literal(&mut m, &run, &mut fpu, &mut observer).unwrap();
        assert_eq!(
            observer.events,
            [
                ("allocate", cell, 0, 0),
                ("construct", cell, name, payload),
                ("push", cell, 1, 0),
            ]
        );
        assert_eq!(m.r32(0x62f138), 1);
        assert_eq!(m.r32(thread + 0x24), pc + 8);
        stack::leave(&mut m, caller);
    }
}

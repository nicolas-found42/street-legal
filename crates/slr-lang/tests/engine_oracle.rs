//! Real-native vectors for the required engine boundary.
use slr_lang::langobj::{
    engine::{self, Call, Engine},
    gametype, system,
};
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value,
};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[test]
fn object_system_and_gametype_cover_the_executable_registry() {
    use slr_lang::{langobj::object, seam, thread};
    use std::collections::BTreeSet;
    let path =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/native_api.tsv");
    let Ok(text) = std::fs::read_to_string(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: object_system_and_gametype_cover_the_executable_registry");
        return;
    };
    let in_scope = |class: &str| {
        matches!(
            class,
            "java.lang.Object" | "java.lang.System" | "java.lang.GameType"
        )
    };
    let expected: BTreeSet<_> = text
        .lines()
        .filter_map(|line| {
            let columns: Vec<_> = line.split('\t').collect();
            if columns.len() != 4 || !in_scope(columns[0]) {
                return None;
            }
            let address = u32::from_str_radix(&columns[3][columns[3].len() - 8..], 16).unwrap();
            Some((
                columns[0].to_owned(),
                columns[1].to_owned(),
                columns[2].to_owned(),
                address,
            ))
        })
        .collect();
    assert_eq!(
        expected.len(),
        40,
        "all registered Object/System/GameType natives"
    );
    let declared: BTreeSet<_> = seam::known_rows()
        .filter(|row| in_scope(row.class))
        .map(|row| {
            (
                row.class.to_owned(),
                row.name.to_owned(),
                row.sig.to_owned(),
                row.impl_addr,
            )
        })
        .collect();
    assert_eq!(
        declared, expected,
        "exact executable registration triples and addresses"
    );
    let mut table = seam::NativeTable::new();
    object::install(&mut table);
    system::install(&mut table);
    gametype::install(&mut table);
    let mut vm = VmHost::new();
    seam::install(&mut vm, &table);
    thread::install(&mut vm);
    for (class, name, sig, address) in expected {
        assert_eq!(vm.natives.resolve(&class, &name, &sig), Some(address));
        assert!(
            vm.native_impls.contains_key(&address),
            "{class}.{name}{sig} has a port"
        );
    }
    assert!(
        !text
            .lines()
            .any(|line| line.starts_with("java.lang.Class\t")),
        "Class has no registered native in this executable"
    );
}
#[derive(Default)]
struct State {
    memory: BTreeMap<u32, u32>,
    calls: Vec<Call>,
    replies: Vec<u32>,
    scratch: u32,
    effects: Vec<BTreeMap<u32, u32>>,
}
struct Backend(Rc<RefCell<State>>);
impl Engine for Backend {
    fn read(&self, a: u32) -> u32 {
        *self.0.borrow().memory.get(&a).unwrap_or(&0)
    }
    fn write(&mut self, a: u32, v: u32) {
        self.0.borrow_mut().memory.insert(a, v);
    }
    fn scratch(&mut self, n: usize) -> u32 {
        let mut s = self.0.borrow_mut();
        if s.scratch == 0 {
            s.scratch = 0x21000000
        };
        let a = s.scratch;
        s.scratch += n as u32 * 4;
        for i in 0..n {
            s.memory.insert(a + i as u32 * 4, 0);
        }
        a
    }
    fn call(&mut self, c: Call) -> u32 {
        let mut s = self.0.borrow_mut();
        s.calls.push(c);
        if !s.effects.is_empty() {
            let effect = s.effects.remove(0);
            s.memory.extend(effect);
        }
        if s.replies.is_empty() {
            0
        } else {
            s.replies.remove(0)
        }
    }
}
#[test]
fn engine_natives_match_oracle() {
    let p = std::env::var_os("SLR_ENGINE_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/langobj/engine.jsonl")
        });
    let Ok(text) = std::fs::read_to_string(p) else {
        eprintln!("SLR_ORACLE_SKIPPED: engine_natives_match_oracle");
        return;
    };
    assert!(
        text.lines().count() >= 66,
        "engine oracle corpus is incomplete"
    );
    for line in text.lines() {
        let r: serde_json::Value = serde_json::from_str(line).unwrap();
        let s = Rc::new(RefCell::new(State::default()));
        if r["family"] == "lifecycle" {
            lifecycle_row(&r);
            continue;
        }
        if r["family"] == "array" {
            array_row(&r);
            continue;
        }
        for (a, v) in r["memory"].as_object().unwrap() {
            s.borrow_mut()
                .memory
                .insert(a.parse().unwrap(), v.as_u64().unwrap() as u32);
        }
        s.borrow_mut().replies = r["replies"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_u64().unwrap() as u32)
            .collect();
        if let Some(effects) = r["effects"].as_array() {
            s.borrow_mut().effects = effects
                .iter()
                .map(|v| {
                    v.as_object()
                        .unwrap()
                        .iter()
                        .map(|(a, v)| (a.parse().unwrap(), v.as_u64().unwrap() as u32))
                        .collect()
                })
                .collect();
        }
        slr_lang::seam::with_lang_host(|h| {
            h.sim_time = r["sim_time"].as_f64().unwrap_or(0.0) as f32
        });
        engine::install(Box::new(Backend(s.clone())));
        let args: Vec<_> = r["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| Value::int(v.as_u64().unwrap() as i32))
            .collect();
        let info = CallInfo {
            jvm: r["jvm"].as_u64().unwrap() as u32,
            thread: 0,
            args: &args,
            class: if r["family"] == "gametype" {
                "java.lang.GameType"
            } else {
                "java.lang.System"
            },
            name: r["name"].as_str().unwrap(),
            sig: r["sig"].as_str().unwrap(),
        };
        let mut vm = VmHost::new();
        let mut table = slr_lang::seam::NativeTable::new();
        system::install(&mut table);
        gametype::install(&mut table);
        slr_lang::seam::install(&mut vm, &table);
        assert_eq!(
            vm.natives.resolve(info.class, info.name, info.sig),
            Some(r["exe_addr"].as_u64().unwrap() as u32),
            "{} binding",
            r["id"]
        );
        let ret = slr_lang::seam::bridge(&mut vm, &info);
        if info.sig.ends_with('I') {
            assert_eq!(
                ret,
                Some(r["return"].as_u64().unwrap() as u32),
                "{} return",
                r["id"]
            );
        }
        let calls: Vec<_> = s
            .borrow()
            .calls
            .iter()
            .map(|c| serde_json::json!([c.address, c.this, c.args]))
            .collect();
        assert_eq!(serde_json::json!(calls), r["calls"], "{} calls", r["id"]);
        for (a, v) in r["after"].as_object().unwrap() {
            assert_eq!(
                s.borrow()
                    .memory
                    .get(&a.parse::<u32>().unwrap())
                    .copied()
                    .unwrap_or(0),
                v.as_u64().unwrap() as u32,
                "{} memory {a}",
                r["id"]
            );
        }
        engine::reset();
    }
}

fn array_row(r: &serde_json::Value) {
    let mut vm = VmHost::new();
    for (field, types) in [("src", "src_types"), ("dst", "dst_types")] {
        vm.objects.push(slr_vm::host::Object {
            class: slr_vm::vmclass::ClassRef::Array(
                slr_vm::vmclass::ArrayDescriptor::new("[I").unwrap(),
            ),
            fields: r[field]
                .as_array()
                .unwrap()
                .iter()
                .zip(r[types].as_array().expect("input cell types"))
                .map(|(v, ty)| {
                    let payload = v.as_u64().unwrap() as u32;
                    match ty.as_str().unwrap() {
                        "I" => Value::int(payload as i32),
                        "F" => Value::float(f32::from_bits(payload)),
                        "L" => Value::reference(payload),
                        other => panic!("unexpected oracle cell type {other}"),
                    }
                })
                .collect(),
        });
    }
    let dst = if r["same"].as_bool().unwrap() { 0 } else { 1 };
    let s = Rc::new(RefCell::new(State::default()));
    engine::install(Box::new(Backend(s.clone())));
    let args = [
        Value::int(r["count"].as_i64().unwrap() as i32),
        Value::int(r["dstpos"].as_i64().unwrap() as i32),
        Value::reference(0x30000000 + dst),
        Value::int(r["srcpos"].as_i64().unwrap() as i32),
        Value::reference(0x30000000),
        Value::reference(0),
    ];
    let info = CallInfo {
        jvm: 0,
        thread: 0,
        args: &args,
        class: "java.lang.System",
        name: "arraycopy",
        sig: r["sig"].as_str().unwrap(),
    };
    assert_eq!(system::arraycopy(&mut vm, &info), None);
    let payloads = |i: usize| {
        serde_json::json!(vm.objects[i]
            .fields
            .iter()
            .map(|v| v.payload)
            .collect::<Vec<_>>())
    };
    assert_eq!(payloads(0), r["after_src"], "{} src", r["id"]);
    assert_eq!(payloads(dst as usize), r["after_dst"], "{} dst", r["id"]);
    let types = |index: usize| {
        serde_json::json!(vm.objects[index]
            .fields
            .iter()
            .map(|v| match v.tag {
                slr_vm::values::TypeTag::Int => "I",
                slr_vm::values::TypeTag::Float => "F",
                slr_vm::values::TypeTag::Ref(_) => "L",
                other => panic!("unexpected port cell type {other:?}"),
            })
            .collect::<Vec<_>>())
    };
    assert_eq!(
        types(0),
        r["after_src_types"],
        "{} source cell type words",
        r["id"]
    );
    assert_eq!(
        types(dst as usize),
        r["after_dst_types"],
        "{} destination cell type words",
        r["id"]
    );
    let calls: Vec<_> = s
        .borrow()
        .calls
        .iter()
        .map(|c| serde_json::json!([c.address, c.this, c.args]))
        .collect();
    assert_eq!(
        serde_json::json!(calls),
        r["calls"],
        "{} diagnostics",
        r["id"]
    );
    engine::reset();
}

thread_local! {static RUNS:RefCell<Vec<serde_json::Value>>=const {RefCell::new(Vec::new())};static SUCCESS:RefCell<bool>=const {RefCell::new(true)};}
fn lifecycle_row(r: &serde_json::Value) {
    use slr_lang::thread::{self, with_threads};
    thread::reset();
    RUNS.with(|v| v.borrow_mut().clear());
    SUCCESS.with(|v| *v.borrow_mut() = r["id"] != "start-fail");
    with_threads(|h| {
        h.queues_mut().spawn(0, 0, "t");
        h.set_name_source(|_| "oracle-worker".into());
        h.set_target_source(|_| 0x1234);
        h.set_run_source(|target, _| {
            RUNS.with(|v| v.borrow_mut().push(serde_json::json!([target, "run"])));
            SUCCESS.with(|v| *v.borrow())
        });
    });
    let receiver = 0x22001000;
    let mut host = slr_lang::LangHost::new();
    let mut id = None;
    for step in r["steps"].as_array().unwrap() {
        let args = if step["op"] == "init" {
            vec![Value::reference(123), Value::reference(receiver)]
        } else {
            vec![Value::reference(receiver)]
        };
        match step["op"].as_str().unwrap() {
            "init" => {
                thread::thread_init(&mut host, &args);
                id = with_threads(|h| h.object(receiver).handle);
            }
            "start" => {
                thread::thread_start(&mut host, &args);
            }
            "stop" => {
                thread::thread_stop(&mut host, &args);
            }
            x => panic!("{x}"),
        }
        with_threads(|h| {
            let state = h.queues().get(id.unwrap());
            assert_eq!(
                state.flags,
                step["flags"].as_u64().unwrap() as u32,
                "{} flags",
                r["id"]
            );
            assert_eq!(
                state.priority,
                step["priority"].as_i64().unwrap() as i32,
                "{} priority",
                r["id"]
            );
            assert_eq!(
                h.script_object(id.unwrap()).unwrap_or(0),
                step["script"].as_u64().unwrap() as u32,
                "{} script",
                r["id"]
            );
            assert_eq!(
                h.object(receiver).handle.is_some(),
                step["handle"].as_bool().unwrap(),
                "{} handle",
                r["id"]
            );
            assert_eq!(
                state.name,
                step["name"].as_str().unwrap(),
                "{} name",
                r["id"]
            );
            assert_eq!(
                h.queues().run_order().len(),
                step["queue_count"].as_u64().unwrap() as usize,
                "{} queue",
                r["id"]
            );
        });
        RUNS.with(|v| {
            assert_eq!(
                serde_json::json!(*v.borrow()),
                step["runs"],
                "{} runs",
                r["id"]
            )
        });
    }
    thread::reset();
}

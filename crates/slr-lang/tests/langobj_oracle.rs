//! Differential test for the `java.lang.Object`/`System.exit`/`System.analyze`
//! natives (M1-08 rows O1..O5, O8, O9 and O10).
//!
//! Replays `re/out/langobj/objects.jsonl`, produced by
//! `re/oracle/langobj_vectors.py gen`: the REAL exe natives executed under
//! Unicorn on a hand-built `CallInfo` (no JVM boot, no game data). Override
//! the directory with `SLR_LANGOBJ_VECTORS`. Skips with a note when the file
//! is absent so `cargo test --workspace` stays green without the exe/venv.
//!
//! The seven suites line up with the port one row at a time:
//!
//! - `hash`: `eax` is the receiver word, so each row is one `hashCode` call.
//! - `tostr`: the text the exe hands to its interner. Object words are
//!   arena-dependent, so the row's `%0x` rendering is re-derived from the
//!   port's own word; the *format rule* and the class name are what compare.
//! - `gc`: the six-step move sequence's link/flag state, compared by fixture
//!   identity (`o1`, `o2`, `hn`, `hd`) rather than raw words.
//! - `exit`: the exit request word before and after each `exit` call; only
//!   `exit()V` moves it, the `String` overload marshals and drops.
//! - `analyze`: the word the exe's marshaller left in the native's own
//!   argument slot. The well-formed rows compare the word the port records —
//!   for `(F)V` the raw f32 bit pattern — and the malformed rows compare the
//!   port's zero (the exe's slot keeps the `info` pointer there; that
//!   divergence is stated in `system::analyze_object`).
//! - `info`: the text the exe hands to its interner, over the row's own
//!   `readings` loaded into the port's [`SystemInfo`] seam. The formatted
//!   rows compare the port's text with the exe's (the exe's `%d` — including
//!   its `INT_MIN` quirk — is `system::decimal`); the `formatted: false` rows
//!   are the id>5 path, whose exe-side text is stack residue, so those
//!   compare the port's documented empty string instead.
//! - `sysgc`: one row is one *scenario* — an initial `[jvm+0x20]`/`[jvm+0x28]`
//!   state, a pass sequence (`gc`/`runFinalization`), and on the `info(4)`
//!   row a `System.info(4)` read before and after it. Each step's `eax`,
//!   `[jvm+0x20]`, `[jvm+0x28]` and the JVM-object offsets it wrote are the
//!   exe's, so the port replays the sequence and compares all three — the
//!   `gc` alternation (`-1` then the collected count) and the mark-size word
//!   the drain pass publishes, which `info(4)` then reports. `[jvm+0x34]` (the
//!   heap byte counter the drain pass also touches) is recorded but not
//!   modelled; the row only requires it to be one of the three named offsets
//!   and to appear only on a draining pass.

#[path = "support/string_vm.rs"]
mod string_vm;

use std::path::PathBuf;

use slr_lang::langobj::object::{
    class_name, disable_gc, enable_gc, hash_code, object_text, to_string, with_gc_lists, GcLists,
};
use slr_lang::langobj::system::{
    analyze_float, analyze_int, analyze_object, build_number, current_time, exit, exit_string, gc,
    info, is_loading, run_finalization, set_ld_priority, set_measure, sim_time, sync_game_time,
    time_warp, ANALYZE_FLOAT, ANALYZE_INT, ANALYZE_OBJECT, EXIT, EXIT_STRING, GC_COLLECTED_NONE,
    GC_INITIALIZED, INFO, IS_LOADING, RUN_FINALIZATION_ZERO, SET_LD_PRIORITY, SET_MEASURE,
    SYNC_GAME_TIME, TIME_WARP,
};
use slr_lang::seam::{self, with_analyze, with_lang_host, AnalyzeEvent, AnalyzeKind, SystemInfo};
use slr_lang::thread::ThreadHost;
use slr_vm::host::{CallInfo, Object, VmHost};
use slr_vm::values::Value;

fn vectors_dir() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_LANGOBJ_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/langobj")
}

fn load_rows(dir: &std::path::Path) -> Option<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(dir.join("objects.jsonl")).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}")));
    }
    Some(rows)
}

/// The `java.lang.Object` rows' own info (the `Object` natives read only
/// their receiver, so the class word is fixed here).
fn object_info<'a>(args: &'a [Value], sig: &'a str, name: &'a str) -> CallInfo<'a> {
    CallInfo {
        jvm: 0,
        thread: 0,
        args,
        class: "java.lang.Object",
        name,
        sig,
    }
}

/// A host whose object heap holds one `class` object; returns its word.
fn host_with(class: &str) -> (VmHost, u32) {
    let mut vm = string_vm::vm();
    let id = vm.loader.lookup(class).unwrap_or_else(|| {
        vm.loader.insert_test_class(slr_vm::vmclass::Class {
            kind: 0,
            name: class.into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        })
    });
    let word = 0x1000 + vm.objects.len() as u32;
    vm.objects.push(Object {
        class: slr_vm::vmclass::ClassRef::Loaded(id),
        fields: Vec::new(),
    });
    (vm, word)
}

/// Fixture identity of a port link value, spelled like the oracle row names
/// its fixture words.
fn rel(value: u32, o1: u32, o2: u32, hn: u32, hd: u32) -> String {
    if value == o1 {
        "o1".to_owned()
    } else if value == o2 {
        "o2".to_owned()
    } else if value == hn {
        "hn".to_owned()
    } else if value == hd {
        "hd".to_owned()
    } else {
        format!("{value:#x}")
    }
}

/// The port's list state in the oracle's row shape.
fn snapshot(lists: &GcLists, class: &str, o1: u32, o2: u32) -> Option<serde_json::Value> {
    let g = lists.class(class)?;
    let (hn, hd) = (g.normal.sentinel, g.disabled.sentinel);
    let four = |cell: u32, node: u32| {
        serde_json::json!({
            "head_cell": rel(cell, o1, o2, hn, hd),
            "node": rel(node, o1, o2, hn, hd),
            "node_prev": rel(g.node(node).prev, o1, o2, hn, hd),
            "node_next": rel(g.node(node).next, o1, o2, hn, hd),
        })
    };
    let obj = |w: u32| {
        serde_json::json!({
            "next": rel(g.node(w).next, o1, o2, hn, hd),
            "prev": rel(g.node(w).prev, o1, o2, hn, hd),
            "flags": g.node(w).flags,
        })
    };
    Some(serde_json::json!({
        "normal": four(g.normal.head, hn),
        "disabled": four(g.disabled.head, hd),
        "o1": obj(o1),
        "o2": obj(o2),
    }))
}

/// The oracle row's `state` object, re-keyed into the same shape as
/// [`snapshot`] so the two compare field by field.
fn row_state(row: &serde_json::Value) -> serde_json::Value {
    let s = &row["state"];
    let four = |cell: &str, node_key: &str, node: &serde_json::Value| {
        serde_json::json!({
            "head_cell": s[cell].as_str().expect("head cell"),
            "node": node_key,
            "node_prev": node["prev"].as_str().expect("node prev"),
            "node_next": node["next"].as_str().expect("node next"),
        })
    };
    serde_json::json!({
        "normal": four("normal_head_cell", "hn", &s["normal_sentinel"]),
        "disabled": four("disabled_head_cell", "hd", &s["disabled_sentinel"]),
        "o1": {
            "next": s["o1"]["next"].as_str().expect("o1 next"),
            "prev": s["o1"]["prev"].as_str().expect("o1 prev"),
            "flags": s["o1"]["flags"].as_u64().expect("o1 flags"),
        },
        "o2": {
            "next": s["o2"]["next"].as_str().expect("o2 next"),
            "prev": s["o2"]["prev"].as_str().expect("o2 prev"),
            "flags": s["o2"]["flags"].as_u64().expect("o2 flags"),
        },
    })
}

#[test]
fn object_natives_match_oracle_vectors() {
    let dir = vectors_dir();
    let Some(rows) = load_rows(&dir) else {
        eprintln!("SLR_ORACLE_SKIPPED: object_natives_match_oracle_vectors");
        eprintln!("SKIP: no langobj vectors (run re/oracle/langobj_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());

    let mut hashed = 0;
    let mut stringed = 0;
    let mut gc_steps = 0;
    let mut exits = 0;
    let mut analyzed = 0;
    let mut informed = 0;
    let mut sysgc_rows = 0;
    let mut string_rows = 0;
    let mut system_rows = 0;
    let mut thread_rows = 0;

    for row in &rows {
        let suite = row["suite"].as_str().expect("suite");
        match suite {
            "hash" => {
                let word = row["receiver_word"].as_u64().expect("word") as u32;
                let args = [Value::int(word as i32)];
                let mut vm = string_vm::vm();
                assert_eq!(
                    hash_code(&mut vm, &object_info(&args, "()I", "hashCode")),
                    Some(row["eax"].as_u64().expect("eax") as u32),
                    "{}",
                    row["id"]
                );
                hashed += 1;
            }
            "tostr" => {
                let class = row["class"].as_str().expect("class");
                let (mut vm, word) = host_with(class);
                let args = [Value::reference(word)];
                assert_eq!(class_name(&vm, word), Some(class));
                let ret = to_string(
                    &mut vm,
                    &object_info(&args, "()Ljava.lang.String;", "toString"),
                )
                .expect("String return");
                // This function probe supplies the CCD's name pointer directly,
                // with arbitrary bare text; it does not execute class loading.
                // Preserve that oracle input when checking the formatter. Real
                // loaded CCDs carry L...; descriptors (also checked by the stock
                // allocation/String-content prefix regression).
                let text = object_text(&format!("L{class};"), word);
                let row_word = row["object_word"].as_u64().expect("object word") as u32;
                assert_eq!(
                    row["text"].as_str().expect("text"),
                    object_text(class, row_word),
                    "{} (exe word)",
                    row["id"]
                );
                assert_eq!(text, format!("L{class};<{word:x}>"), "{}", row["id"]);
                assert!(
                    vm.strings.lookup(text.as_bytes()).is_some(),
                    "{} interns the text",
                    row["id"]
                );
                let index = (ret - 0x1000) as usize;
                assert_eq!(vm.objects[index].class_name(&vm.loader), "java.lang.String");
                stringed += 1;
            }
            "gc" => {
                // One shared fixture per file: the rows are a sequence.
                gc_fixture_step(row);
                gc_steps += 1;
            }
            "exit" => {
                // The exit request is process state, so every row starts from
                // its own fresh runtime and (for the primed rows) reproduces
                // the `exit()` call the oracle made first.
                seam::reset();
                let mut vm = string_vm::vm();
                let receiver = row["receiver"].as_u64().expect("receiver") as u32;
                let mut args: Vec<Value> = row["params"]
                    .as_array()
                    .expect("params")
                    .iter()
                    .map(|w| Value::reference(w.as_u64().expect("param") as u32))
                    .collect();
                args.push(Value::reference(receiver));
                let sig = row["sig"].as_str().expect("sig");
                let call = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.System",
                    name: "exit",
                    sig,
                };
                if row["prime_exit"].as_bool().expect("prime_exit") {
                    let none: [Value; 0] = [];
                    let prime = CallInfo {
                        jvm: 0,
                        thread: 0,
                        args: &none,
                        class: "java.lang.System",
                        name: "exit",
                        sig: "()V",
                    };
                    assert_eq!(exit(&mut vm, &prime), None, "{} primes", row["id"]);
                }
                assert_eq!(
                    u64::from(with_lang_host(|host| host.exit_flag)),
                    row["flag_before"].as_u64().expect("flag_before"),
                    "{} before",
                    row["id"]
                );
                let addr = row["exe_addr"].as_u64().expect("exe_addr") as u32;
                let ret = match addr {
                    EXIT => exit(&mut vm, &call),
                    EXIT_STRING => exit_string(&mut vm, &call),
                    other => panic!("unexpected exit native {other:#x}"),
                };
                assert_eq!(ret, None, "{} is a V native", row["id"]);
                assert_eq!(
                    u64::from(with_lang_host(|host| host.exit_flag)),
                    row["flag_after"].as_u64().expect("flag_after"),
                    "{} after",
                    row["id"]
                );
                exits += 1;
            }
            "analyze" => {
                // The analysed word is process-free state: every row gets
                // its own runtime so the sink holds exactly one event.
                seam::reset();
                let mut vm = string_vm::vm();
                let addr = row["exe_addr"].as_u64().expect("exe_addr") as u32;
                let receiver = row["receiver"].as_u64().expect("receiver") as u32;
                let params: Vec<Value> = row["params"]
                    .as_array()
                    .expect("params")
                    .iter()
                    .map(|w| Value::int(w.as_u64().expect("param") as u32 as i32))
                    .collect();
                let mut args = params;
                args.push(Value::int(receiver as i32));
                let sig = row["sig"].as_str().expect("sig");
                let info = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.System",
                    name: "analyze",
                    sig,
                };
                let (ret, kind) = match addr {
                    ANALYZE_OBJECT => (analyze_object(&mut vm, &info), AnalyzeKind::Object),
                    ANALYZE_INT => (analyze_int(&mut vm, &info), AnalyzeKind::Int),
                    ANALYZE_FLOAT => (analyze_float(&mut vm, &info), AnalyzeKind::Float),
                    other => panic!("unexpected analyze native {other:#x}"),
                };
                assert_eq!(ret, None, "{} is a V native", row["id"]);
                assert_eq!(kind.signature(), sig, "{}", row["id"]);
                // A malformed container stops the marshaller before the port
                // writes its parameter word, so the port records 0 where the
                // exe's slot still holds the `info` pointer.
                let value = if row["slot_source"].as_str().expect("slot_source") == "info" {
                    assert!(
                        row["params"].as_array().expect("params").is_empty(),
                        "{} is the malformed shape",
                        row["id"]
                    );
                    0
                } else {
                    row["params"]
                        .as_array()
                        .expect("params")
                        .last()
                        .expect("a parameter")
                        .as_u64()
                        .expect("param") as u32
                };
                assert_eq!(
                    with_analyze(|sink| sink.events().to_vec()),
                    vec![AnalyzeEvent { kind, value }],
                    "{}",
                    row["id"]
                );
                assert_eq!(
                    with_analyze(|sink| sink.take()),
                    vec![AnalyzeEvent { kind, value }]
                );
                assert!(!with_lang_host(|host| host.exit_requested()));
                analyzed += 1;
            }
            "info" => {
                // The row carries the JVM object's readings, so the port
                // starts from them; the id comes from the row's container
                // through the port's own marshalling.
                seam::reset();
                let mut vm = string_vm::vm();
                let readings = &row["readings"];
                let word = |key: &str| {
                    readings[key]
                        .as_u64()
                        .unwrap_or_else(|| panic!("{}: readings.{key}", row["id"]))
                        as u32
                };
                with_lang_host(|host| {
                    host.info = SystemInfo {
                        list_18: word("list_18"),
                        list_1c: word("list_1c"),
                        list_10: word("list_10"),
                        word_24: word("word_24"),
                        word_28: word("word_28"),
                        live_cells: word("live_cells"),
                    };
                });
                let mut args: Vec<Value> = row["params"]
                    .as_array()
                    .expect("params")
                    .iter()
                    .map(|w| Value::int(w.as_u64().expect("param") as u32 as i32))
                    .collect();
                args.push(Value::int(
                    row["receiver"].as_u64().expect("receiver") as i32
                ));
                let sig = row["sig"].as_str().expect("sig");
                let call = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.System",
                    name: "info",
                    sig,
                };
                assert_eq!(
                    row["exe_addr"].as_u64().expect("exe_addr") as u32,
                    INFO,
                    "{}",
                    row["id"]
                );
                let ret = info(&mut vm, &call).expect("the row returns a String");
                let index = (ret - 0x1000) as usize;
                assert_eq!(
                    vm.objects[index].class_name(&vm.loader),
                    "java.lang.String",
                    "{}",
                    row["id"]
                );
                // The exe's own text for the formatted ids (the `%d` rule,
                // `INT_MIN` quirk included); the id>5 path's exe text is
                // frame residue, which the port replaces with its empty
                // buffer (see `system::info`).
                let exe_text = row["text"].as_str().expect("text");
                let want = if row["formatted"].as_bool().expect("formatted") {
                    exe_text
                } else {
                    match row["residue"].as_str().expect("residue") {
                        // A fresh frame leaves the `%d` target zeroed.
                        "fresh" => {
                            assert_eq!(exe_text, "", "{}: fresh frame", row["id"]);
                            ""
                        }
                        // A primed frame leaves the earlier call's text: the
                        // exe's residue is not the port's to reproduce.
                        "primed" => {
                            assert!(!exe_text.is_empty(), "{}: primed frame", row["id"]);
                            ""
                        }
                        other => panic!("unknown residue {other}"),
                    }
                };
                assert_eq!(
                    vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
                    want.len() as i32,
                    "{}",
                    row["id"]
                );
                assert!(
                    vm.strings.lookup(want.as_bytes()).is_some(),
                    "{} interns {want:?}",
                    row["id"]
                );
                assert!(!with_lang_host(|host| host.exit_requested()));
                informed += 1;
            }
            "sysgc" => {
                // One row is one scenario over process state, so it starts
                // from its own runtime, seeded with the JVM words the oracle
                // started from, and primes the exit request where the oracle
                // did (the `exit-string-after-exit` shape).
                seam::reset();
                let mut vm = string_vm::vm();
                with_lang_host(|host| {
                    host.gc_context = row["ctx_before"].as_bool().expect("ctx_before");
                    host.info.word_28 = row["mark_bytes_before"]
                        .as_u64()
                        .expect("mark_bytes_before") as u32;
                });
                if row["prime_exit"].as_bool().expect("prime_exit") {
                    let none: [Value; 0] = [];
                    let prime = CallInfo {
                        jvm: 0,
                        thread: 0,
                        args: &none,
                        class: "java.lang.System",
                        name: "exit",
                        sig: "()V",
                    };
                    assert_eq!(exit(&mut vm, &prime), None, "{} primes", row["id"]);
                }
                assert_eq!(
                    u64::from(with_lang_host(|host| host.exit_flag)),
                    row["flag_before"].as_u64().expect("flag_before"),
                    "{} before",
                    row["id"]
                );
                // `System.info(4)` on this row's JVM: the port reads the seam's
                // `word_28` and interns it, so the row's own text is what the
                // port must produce at each of the two points.
                let info_text = |vm: &mut VmHost, id: u64, text: &str| {
                    let args = [Value::int(id as u32 as i32), Value::int(0)];
                    let call = CallInfo {
                        jvm: 0,
                        thread: 0,
                        args: &args,
                        class: "java.lang.System",
                        name: "info",
                        sig: "(I)Ljava.lang.String;",
                    };
                    let ret = info(vm, &call).expect("the row returns a String");
                    let index = (ret - 0x1000) as usize;
                    assert_eq!(
                        vm.objects[index].class_name(&vm.loader),
                        "java.lang.String",
                        "{}",
                        row["id"]
                    );
                    assert_eq!(
                        vm.string_bytes(ret).expect("actual String.ptr").len() as i32,
                        text.len() as i32,
                        "{}",
                        row["id"]
                    );
                    assert!(
                        vm.strings.lookup(text.as_bytes()).is_some(),
                        "{} interns {text:?}",
                        row["id"]
                    );
                };
                if let Some(id) = row["info_id"].as_u64() {
                    let before = row["info_text_before"].as_str().expect("info_text_before");
                    info_text(&mut vm, id, before);
                }
                for step in row["per_step"].as_array().expect("per_step") {
                    let pass = step["call"].as_str().expect("call");
                    let eax = step["eax"].as_u64().expect("eax") as u32;
                    let offsets: Vec<u64> = step["jvm_write_offsets"]
                        .as_array()
                        .expect("jvm_write_offsets")
                        .iter()
                        .map(|o| o.as_u64().expect("offset"))
                        .collect();
                    // The exe's writes into the JVM object on a collector
                    // pass: `[jvm+0x20]` always, `[jvm+0x28]` (the mark size)
                    // and `[jvm+0x34]` (the heap byte counter) only when the
                    // pass drains a context. Only the first two are modelled.
                    let draining = offsets.contains(&0x28);
                    for off in &offsets {
                        assert!(
                            *off == 0x20 || *off == 0x28 || *off == 0x34,
                            "{}: unmodelled JVM write {off:#x}",
                            row["id"]
                        );
                    }
                    assert_eq!(
                        draining,
                        offsets.contains(&0x34),
                        "{}: the heap byte counter moves with the mark size",
                        row["id"]
                    );
                    match pass {
                        "gc" => {
                            assert_eq!(offsets.first(), Some(&0x20), "{}", row["id"]);
                            assert_eq!(
                                eax,
                                if draining {
                                    GC_COLLECTED_NONE
                                } else {
                                    GC_INITIALIZED
                                },
                                "{}: the pass discriminator",
                                row["id"]
                            );
                            assert_eq!(
                                with_lang_host(|host| gc(host, &[])),
                                Some(eax),
                                "{}",
                                row["id"]
                            );
                        }
                        "runFinalization" => {
                            assert_eq!(step["writes_total"].as_u64(), Some(0), "{}", row["id"]);
                            assert_eq!(step["icount"].as_u64(), Some(2), "{}", row["id"]);
                            assert!(offsets.is_empty(), "{}", row["id"]);
                            assert_eq!(
                                with_lang_host(|host| run_finalization(host, &[])),
                                Some(RUN_FINALIZATION_ZERO),
                                "{}",
                                row["id"]
                            );
                        }
                        other => panic!("unexpected sysgc pass {other}"),
                    }
                    assert_eq!(step["st0_empty"].as_bool(), Some(true), "{}", row["id"]);
                    assert_eq!(step["logged"].as_bool(), Some(false), "{}", row["id"]);
                    assert_eq!(
                        with_lang_host(|host| host.gc_context),
                        step["ctx"].as_bool().expect("ctx"),
                        "{}: [jvm+0x20]",
                        row["id"]
                    );
                    assert_eq!(
                        u64::from(with_lang_host(|host| host.info.word_28)),
                        step["mark_bytes"].as_u64().expect("mark_bytes"),
                        "{}: [jvm+0x28]",
                        row["id"]
                    );
                }
                assert_eq!(
                    u64::from(with_lang_host(|host| host.exit_flag)),
                    row["flag_after"].as_u64().expect("flag_after"),
                    "{} after",
                    row["id"]
                );
                if let Some(id) = row["info_id"].as_u64() {
                    let after = row["info_text_after"].as_str().expect("info_text_after");
                    info_text(&mut vm, id, after);
                }
                sysgc_rows += 1;
            }
            "thread" => {
                // The two Thread field-write natives. The exe writes the
                // thread object's `+0x28` (priority) or `+0x2c` (flags, bit
                // 0x100 = daemon); the port writes the same fields on its
                // queue entry, so the comparison is field-for-field.
                let name = row["name"].as_str().expect("name");
                let addr = u32::from_str_radix(
                    row["exe_addr"]
                        .as_str()
                        .expect("exe_addr")
                        .trim_start_matches("0x"),
                    16,
                )
                .expect("exe_addr hex");
                let arg = row["args"].get(0).and_then(|v| v.as_u64()).unwrap_or(0) as u32;
                let mut runtime = ThreadHost::new();
                const RECEIVER: u32 = 0x5100;
                let id = runtime.init(RECEIVER, 0);
                let before = &row["before"];
                // priority natives read/write +0x28; setDaemon alone owns the
                // flag word at +0x2c
                let field = if name.starts_with("setDaemon") {
                    "0x2c"
                } else {
                    "0x28"
                };
                let seeded = before[field].as_u64().unwrap_or(0) as u32;
                {
                    let entry = runtime.queues_mut().get_mut(id);
                    if field == "0x28" {
                        entry.priority = seeded as i32;
                    } else {
                        entry.flags = seeded;
                    }
                }
                let want = row["after"][field].as_u64().expect("after") as u32;
                if name == "isAlive_zero" || name == "isAlive_live_thread" {
                    // build 606 answers 0 even for a live thread: record the
                    // quirk, do not repair it.
                    assert_eq!(addr, 0x47C730, "{name}");
                    let exe_alive = row["eax"].as_u64().expect("eax") != 0;
                    assert_eq!(runtime.is_alive(RECEIVER), exe_alive);
                    assert_eq!(row["eax"].as_u64(), Some(0), "{name}: exe says dead");
                    assert_eq!(runtime.queues().get(id).priority as u32, seeded);
                } else if name.starts_with("getPriority") {
                    assert_eq!(addr, 0x47C760, "{name}");
                    assert_eq!(runtime.get_priority(RECEIVER) as u32, want, "{name}");
                    assert_eq!(row["eax"].as_u64(), Some(u64::from(want)), "{name}: eax");
                } else if name.starts_with("setPriority") {
                    runtime.set_priority(RECEIVER, arg as i32);
                    assert_eq!(
                        runtime.queues().get(id).priority as u32,
                        want,
                        "{name}: +0x28"
                    );
                } else {
                    runtime.set_daemon(RECEIVER, arg != 0);
                    assert_eq!(runtime.queues().get(id).flags, want, "{name}: +0x2c");
                }
                thread_rows += 1;
            }
            "system" => {
                // The ten non-timer System rows (issue #35). Each row records
                // the exe's globals before and after its call, so the port is
                // compared on the words the exe actually wrote, not on the
                // return value alone.
                seam::reset();
                let mut vm = string_vm::vm();
                let name = row["name"].as_str().expect("name");
                let sig = row["sig"].as_str().expect("sig");
                let before = &row["before"];
                // Seed the seam with the globals the oracle started from.
                let bits = |j: &serde_json::Value, k: &str| -> u32 {
                    j[k].as_u64().unwrap_or_else(|| panic!("{k}")) as u32
                };
                with_lang_host(|host| {
                    host.ld_priority = (
                        f32::from_bits(bits(before, "ld_scale")),
                        bits(before, "ld_flag"),
                    );
                    host.time_warp = f32::from_bits(bits(before, "time_warp"));
                    host.time_warp_rate = f32::from_bits(bits(before, "time_warp_rate"));
                    host.game_time_of_day = before["game_time"].as_i64().expect("game_time") as i32;
                    host.daylight = bits(before, "daylight") != 0;
                    host.measure = (
                        f32::from_bits(bits(before, "measure")),
                        f32::from_bits(bits(before, "measure_scaled")),
                        f32::from_bits(bits(before, "measure_recip")),
                    );
                    // the physics world's time word (`simTime`) and the clock
                    // reading (`currentTime`) both come from these
                    host.sim_time = f32::from_bits(bits(before, "sim_time"));
                    host.clock = 500.0;
                });
                let args: Vec<Value> = row["args"]
                    .as_array()
                    .expect("args")
                    .iter()
                    .map(|w| Value::int(w.as_u64().expect("arg") as u32 as i32))
                    .collect();
                // The exe's container holds the declared parameters *before*
                // the receiver/return slot (`[count-1]`), so the replay needs
                // the trailing receiver cell the oracle's `call_info` adds.
                let mut args = args;
                args.push(Value::int(0));
                let call = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.System",
                    name,
                    sig,
                };
                let addr = u32::from_str_radix(
                    row["exe_addr"]
                        .as_str()
                        .expect("exe_addr")
                        .trim_start_matches("0x"),
                    16,
                )
                .expect("exe_addr hex");
                // `timeWarp`'s derived rate reads the clock seam, so the port
                // must see the same 0.5 s reading the oracle seeded.
                if name.starts_with("timeWarp") {
                    with_lang_host(|host| host.clock = 500.0);
                }
                if name == "isLoading" {
                    with_lang_host(|host| {
                        host.loading = row["loader_word"].as_u64().unwrap() as u32
                    });
                }
                let got = match name {
                    n if n.starts_with("timeWarp") => time_warp(&mut vm, &call),
                    n if n.starts_with("setLdPriority") => set_ld_priority(&mut vm, &call),
                    n if n.starts_with("syncGameTime") => sync_game_time(&mut vm, &call),
                    n if n.starts_with("setMeasure") => set_measure(&mut vm, &call),
                    "isLoading" => is_loading(&mut vm, &call),
                    // the three scalar rows read the seam's host directly
                    "buildNumber" => with_lang_host(|host| build_number(host, &args)),
                    "currentTime" => with_lang_host(|host| current_time(host, &args)),
                    "simTime" => with_lang_host(|host| sim_time(host, &args)),
                    other => panic!("unhandled system row {other}"),
                };
                assert_eq!(
                    addr,
                    match name {
                        n if n.starts_with("timeWarp") => TIME_WARP,
                        n if n.starts_with("setLdPriority") => SET_LD_PRIORITY,
                        n if n.starts_with("syncGameTime") => SYNC_GAME_TIME,
                        n if n.starts_with("setMeasure") => SET_MEASURE,
                        "buildNumber" | "currentTime" | "simTime" => addr,
                        _ => IS_LOADING,
                    },
                    "{name}"
                );
                let after = &row["after"];
                match sig {
                    // `timeWarp` returns the word loaded *before* the store.
                    "(F)F" => assert_eq!(
                        got,
                        Some(bits(before, "time_warp")),
                        "{name}: the pre-call scale"
                    ),
                    "(F)V" | "(I)V" => assert_eq!(got, None, "{name}: V return"),
                    "()I" => assert_eq!(
                        got,
                        Some(
                            match row.get("loader_word").and_then(serde_json::Value::as_u64) {
                                // buildNumber carries no loader word: its answer is
                                // the build constant itself
                                None => {
                                    assert_eq!(name, "buildNumber", "{name}: no loader_word");
                                    606
                                }
                                Some(w) => w as u32,
                            }
                        ),
                        "{name}: I return"
                    ),
                    // a float return rides ST0; the row stores the f64 view
                    "()F" => {
                        let want = row["eax"].as_f64().expect("float return");
                        let got = got.expect("float return");
                        assert_eq!(f32::from_bits(got) as f64, want, "{name}: F return");
                    }
                    other => panic!("{name}: unhandled sig {other}"),
                }
                with_lang_host(|host| {
                    let (scale, flag) = host.ld_priority;
                    assert_eq!(scale.to_bits(), bits(after, "ld_scale"), "{name}: ld scale");
                    assert_eq!(flag, bits(after, "ld_flag"), "{name}: ld flag");
                    assert_eq!(
                        host.time_warp.to_bits(),
                        bits(after, "time_warp"),
                        "{name}: time warp"
                    );
                    assert_eq!(
                        host.time_warp_rate.to_bits(),
                        bits(after, "time_warp_rate"),
                        "{name}: time warp rate"
                    );
                    assert_eq!(
                        host.game_time_of_day,
                        after["game_time"].as_i64().expect("game_time") as i32,
                        "{name}: game time"
                    );
                    assert_eq!(
                        host.daylight,
                        bits(after, "daylight") != 0,
                        "{name}: daylight"
                    );
                    let (m, scaled, recip) = host.measure;
                    assert_eq!(m.to_bits(), bits(after, "measure"), "{name}: measure");
                    assert_eq!(
                        scaled.to_bits(),
                        bits(after, "measure_scaled"),
                        "{name}: measure scaled"
                    );
                    assert_eq!(
                        recip.to_bits(),
                        bits(after, "measure_recip"),
                        "{name}: measure reciprocal"
                    );
                });
                system_rows += 1;
            }
            "string" => {
                let name = row["name"].as_str().expect("name");
                let sig = row["sig"].as_str().expect("sig");
                let receiver = row["receiver"].as_str().expect("receiver");
                let mut vm = string_vm::vm();
                let receiver_word = slr_lang::seam::intern_string(&mut vm, receiver.as_bytes());
                let frame_args: Vec<Value> = match name {
                    "intValue" | "floatValue" | "length" | "finalize" => {
                        vec![Value::reference(receiver_word)]
                    }
                    "append" => vec![
                        Value::int(row["arg"].as_i64().expect("arg") as i32),
                        Value::reference(receiver_word),
                    ],
                    "token" => {
                        let charset = row["charset"].as_str().expect("charset");
                        let seps = slr_lang::seam::intern_string(&mut vm, charset.as_bytes());
                        // Match real VM calls: reversed parameters, then receiver.
                        vec![
                            Value::reference(seps),
                            Value::int(row["index"].as_i64().expect("index") as i32),
                            Value::reference(receiver_word),
                        ]
                    }
                    other => panic!("unexpected String native {other}"),
                };
                let args = frame_args;
                let call = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.String",
                    name,
                    sig,
                };
                let want = row["eax"].as_u64().expect("eax") as u32;
                match name {
                    "intValue" => {
                        assert_eq!(
                            slr_lang::string::int_value(&mut vm, &call),
                            Some(want),
                            "{}",
                            row["id"]
                        );
                    }
                    "floatValue" => {
                        assert_eq!(
                            slr_lang::string::float_value(&mut vm, &call),
                            Some(row["st0_bits"].as_u64().expect("st0") as u32),
                            "{}",
                            row["id"]
                        );
                    }
                    "length" => {
                        assert_eq!(
                            slr_lang::string::length(&mut vm, &call),
                            Some(want),
                            "{}",
                            row["id"]
                        );
                    }
                    "append" => {
                        let ret = slr_lang::string::append(&mut vm, &call).expect("word");
                        let want_text = row["interned"][0].as_str().expect("interned");
                        assert_eq!(
                            slr_lang::seam::string_bytes(&vm, ret).as_deref(),
                            Some(want_text.as_bytes()),
                            "{}",
                            row["id"]
                        );
                    }
                    "finalize" => {
                        assert_eq!(
                            slr_lang::string::finalize(&mut vm, &call),
                            None,
                            "{}",
                            row["id"]
                        );
                        assert!(
                            vm.strings.lookup(receiver.as_bytes()).is_none(),
                            "{} released the bytes",
                            row["id"]
                        );
                    }
                    "token" => {
                        let ret = slr_lang::string::token(&mut vm, &call).expect("result word");
                        if want == 0 {
                            assert_eq!(ret, 0, "{}", row["id"]);
                        } else {
                            let want_text = row["interned"][0].as_str().expect("interned");
                            assert_eq!(
                                slr_lang::seam::string_bytes(&vm, ret).as_deref(),
                                Some(want_text.as_bytes()),
                                "{}",
                                row["id"]
                            );
                        }
                    }
                    other => panic!("unexpected String native {other}"),
                }
                string_rows += 1;
            }
            other => panic!("unknown suite {other}"),
        }
    }

    assert!(hashed >= 4, "hash rows");
    assert!(stringed >= 4, "toString rows");
    assert_eq!(gc_steps, 6, "gc sequence rows");
    assert_eq!(exits, 3, "exit rows");
    assert_eq!(analyzed, 11, "analyze rows");
    assert_eq!(informed, 16, "info rows");
    assert_eq!(sysgc_rows, 7, "sysgc rows");
    assert!(string_rows >= 19, "String rows");
    assert_eq!(system_rows, 24, "System rows (#35 non-timer)");
    assert_eq!(thread_rows, 15, "Thread native rows (#35)");
    assert!(sysgc_rows > 0, "sysgc rows");
    let _ = system_rows;
}

/// The six `gc` rows replay on one persistent port state, in file order:
/// the fixture words are the port's own, so the row's identity names
/// (`o1`/`o2`/`hn`/`hd`) line up row by row.
fn gc_fixture_step(row: &serde_json::Value) {
    use std::sync::{LazyLock, Mutex};

    struct Fixture {
        vm: VmHost,
        class: String,
        o1: u32,
        o2: u32,
    }

    static SEQ: LazyLock<Mutex<Option<Fixture>>> = LazyLock::new(|| Mutex::new(None));
    let mut guard = SEQ.lock().expect("sequence lock");
    let fixture = guard.get_or_insert_with(|| {
        with_gc_lists(|l| *l = GcLists::new());
        let (mut vm, o1) = host_with("java.game.Car");
        let o2 = 0x1000 + vm.objects.len() as u32;
        let car_id = vm.loader.lookup("java.game.Car").unwrap();
        vm.objects.push(Object {
            class: slr_vm::vmclass::ClassRef::Loaded(car_id),
            fields: Vec::new(),
        });
        Fixture {
            vm,
            class: "java.game.Car".to_owned(),
            o1,
            o2,
        }
    });
    let word = if row["target"].as_str() == Some("o1") {
        fixture.o1
    } else {
        fixture.o2
    };
    let args = [Value::reference(word)];
    let call = object_info(&args, "()V", row["name"].as_str().expect("name"));
    match call.name {
        "disableGC" => assert_eq!(disable_gc(&mut fixture.vm, &call), None),
        "enableGC" => assert_eq!(enable_gc(&mut fixture.vm, &call), None),
        other => panic!("unexpected gc native {other}"),
    }
    let got = with_gc_lists(|l| snapshot(l, &fixture.class, fixture.o1, fixture.o2))
        .expect("class context");
    assert_eq!(got, row_state(row), "{}", row["id"]);
}

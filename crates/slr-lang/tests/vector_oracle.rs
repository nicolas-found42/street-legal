//! Differential replay of the M1-07 `Vector3`/`Ypr` oracle vectors
//! (ticket M1-07, issue #34).
//!
//! Reads `re/out/langvec/langvec.jsonl` (git-ignored; produced locally by
//! `re/oracle/langvec_vectors.py gen`, which runs the **real exe code** under
//! Unicorn at `cw 0x007f`, 24-bit x87 precision). Override the path with
//! `SLR_LANGVEC_VECTORS`. Skips with a note when the file is absent, so
//! `cargo test --workspace` stays green without the exe/venv.
//!
//! Every row carries its `f32` inputs and outputs as 8-hex-digit bit
//! patterns. Rows whose arithmetic is only multiply/add (`length`,
//! `distance`, `normalize`, `mat3_mul`) are compared **bit for bit**; rows
//! that go through `fsin`/`fcos`/`fpatan` ([`slr_x87::trans`] is APPROX, see
//! `docs/x87.md`) are compared within a relative tolerance. The natives are
//! driven through their `record+8` entries, so `receiver_and_arg`'s
//! `out[0]`/`out[1]` container shape is replayed too.

use serde_json::Value as Json;
use slr_lang::vec::{Mat3, Vec3, Ypr};
use slr_vm::host::{CallInfo, Object, VmHost};
use slr_vm::values::Value;
use slr_vm::vmclass::{Class, Field};
use std::path::PathBuf;

const V3: &str = "java.lang.Vector3";
const YPR: &str = "java.lang.Ypr";
const VEC_FIELDS: [&str; 3] = ["x", "y", "z"];
const YPR_FIELDS: [&str; 3] = ["y", "p", "r"];

/// Relative tolerance for the APPROX transcendental outputs; the measured
/// `fsin`/`fcos` divergence is a few `f32` ulps (`docs/x87.md`), and the
/// matrix bridges accumulate a handful more. Every one of the 707 recorded
/// rows still passes at `1e-6`, so this leaves a 10x margin against a future
/// `libm`/`slr-x87` shift while staying far tighter than a blanket `1e-3`.
const TRIG_TOL: f32 = 1e-5;

fn vectors_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_LANGVEC_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/langvec/langvec.jsonl")
}

fn load_rows() -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(vectors_path()).ok()?;
    Some(
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("langvec row"))
            .collect(),
    )
}

// ---- reading the recorded bit patterns ------------------------------------

fn hex_word(s: &str) -> u32 {
    u32::from_str_radix(s.trim(), 16).unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

fn word_f(s: &str) -> f32 {
    f32::from_bits(hex_word(s))
}

fn triple(v: &Json) -> [f32; 3] {
    let a = v.as_array().expect("triple");
    [
        word_f(a[0].as_str().unwrap()),
        word_f(a[1].as_str().unwrap()),
        word_f(a[2].as_str().unwrap()),
    ]
}

fn words(v: &Json) -> Vec<f32> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| word_f(x.as_str().unwrap()))
        .collect()
}

fn parts(v: &Json) -> Vec<[f32; 3]> {
    v.as_array().unwrap().iter().map(triple).collect()
}

// ---- synthetic hosts ------------------------------------------------------

fn class(name: &str, fields: &[&str]) -> Class {
    Class {
        kind: 0,
        name: name.to_owned(),
        base: None,
        base_id: None,
        interfaces: Vec::new(),
        methods: Vec::new(),
        fields: fields
            .iter()
            .map(|f| Field {
                class: name.to_owned(),
                name: (*f).to_owned(),
                ty: "F".to_owned(),
                is_static: false,
            })
            .collect(),
        bodies: Vec::new(),
    }
}

/// A host with both synthetic classes and one object per `spec` entry
/// (`V3`/`YPR`, three `f32` values). Returns the object words in order.
fn host(spec: &[(&str, [f32; 3])]) -> (VmHost, Vec<u32>) {
    let mut vm = VmHost::new();
    vm.loader.insert_test_class(class(V3, &VEC_FIELDS));
    vm.loader.insert_test_class(class(YPR, &YPR_FIELDS));
    let words = spec
        .iter()
        .map(|(cls, vals)| {
            let word = 0x1000 + vm.objects.len() as u32;
            let id = vm.loader.lookup(cls).unwrap();
            vm.objects.push(Object {
                class: slr_vm::vmclass::ClassRef::Loaded(id),
                fields: vals.iter().copied().map(Value::float).collect(),
            });
            word
        })
        .collect();
    (vm, words)
}

fn field_bits(vm: &VmHost, word: u32, name: &str) -> u32 {
    let object = &vm.objects[(word - 0x1000) as usize];
    let class_id = match &object.class {
        slr_vm::vmclass::ClassRef::Loaded(id) => *id,
        slr_vm::vmclass::ClassRef::Array(_) => panic!("vector object is not an array"),
    };
    let class = vm.loader.find_by_id(class_id).expect("class");
    let slot = class
        .fields
        .iter()
        .position(|f| f.name == name)
        .expect("field");
    object.fields[slot].as_f32().to_bits()
}

fn info<'a>(args: &'a [Value], cls: &'a str, name: &'a str, sig: &'a str) -> CallInfo<'a> {
    CallInfo {
        jvm: 0,
        thread: 0,
        args,
        class: cls,
        name,
        sig,
    }
}

// ---- comparison -----------------------------------------------------------

fn same(a: u32, b: u32) -> bool {
    a == b || (f32::from_bits(a).is_nan() && f32::from_bits(b).is_nan())
}

fn close(a: u32, b: u32) -> bool {
    if same(a, b) {
        return true;
    }
    let (x, y) = (f32::from_bits(a), f32::from_bits(b));
    if x.is_nan() || y.is_nan() {
        return false;
    }
    (x - y).abs() <= TRIG_TOL * y.abs().max(1.0)
}

/// Compare one object's written fields (`result.writes`) against the port.
fn check_writes(
    vm: &VmHost,
    word: u32,
    fields: &[&str],
    result: &Json,
    exact: bool,
    id: u64,
    name: &str,
) {
    let writes = result["writes"].as_object().expect("writes");
    for f in fields {
        let want = hex_word(writes[*f].as_str().unwrap());
        let got = field_bits(vm, word, f);
        let ok = if exact {
            same(got, want)
        } else {
            close(got, want)
        };
        assert!(
            ok,
            "row {id} {name}: field {f} got {got:08x} want {want:08x}"
        );
    }
}

fn check_triple(got: [f32; 3], want: &Json, exact: bool, id: u64, name: &str) {
    let w = triple(want);
    for (i, (g, x)) in got.iter().zip(w.iter()).enumerate() {
        let ok = if exact {
            same(g.to_bits(), x.to_bits())
        } else {
            close(g.to_bits(), x.to_bits())
        };
        assert!(
            ok,
            "row {id} {name}: entry {i} got {:08x} want {:08x}",
            g.to_bits(),
            x.to_bits()
        );
    }
}

fn check_matrix(got: &Mat3, want: &Json, exact: bool, id: u64, name: &str) {
    let w = words(want);
    for (i, x) in w.iter().enumerate() {
        let g = got.m[i];
        let ok = if exact {
            same(g.to_bits(), x.to_bits())
        } else {
            close(g.to_bits(), x.to_bits())
        };
        assert!(
            ok,
            "row {id} {name}: m{i} got {:08x} want {:08x}",
            g.to_bits(),
            x.to_bits()
        );
    }
}

// ---- the replay -----------------------------------------------------------

#[test]
fn vector_natives_match_oracle_vectors() {
    let Some(rows) = load_rows() else {
        eprintln!("SLR_ORACLE_SKIPPED: vector_natives_match_oracle_vectors");
        eprintln!(
            "SKIP vector_oracle: {} is absent (run re/oracle/langvec_vectors.py gen)",
            vectors_path().display()
        );
        return;
    };
    assert!(!rows.is_empty(), "langvec.jsonl is empty");

    for row in &rows {
        let id = row["id"].as_u64().unwrap();
        let name = row["name"].as_str().unwrap();
        let result = &row["result"];
        match name {
            // ---- natives --------------------------------------------------
            "length" | "distance" => {
                let this = triple(&row["this"]);
                let spec: Vec<(&str, [f32; 3])> = if name == "length" {
                    vec![(V3, this)]
                } else {
                    vec![(V3, this), (V3, parts(&row["args"])[0])]
                };
                let (mut vm, w) = host(&spec);
                let got = if name == "length" {
                    let args = [Value::reference(w[0])];
                    slr_lang::vec::length(&mut vm, &info(&args, V3, "length", "()F"))
                } else {
                    let args = [Value::reference(w[1]), Value::reference(w[0])];
                    slr_lang::vec::distance(
                        &mut vm,
                        &info(&args, V3, "distance", "(Ljava.lang.Vector3;)F"),
                    )
                };
                let want = hex_word(result["st0"].as_str().unwrap());
                let got = got.expect("st0");
                assert!(
                    same(got, want),
                    "row {id} {name}: got {got:08x} want {want:08x}"
                );
            }
            "normalize" => {
                let this = triple(&row["this"]);
                let (mut vm, w) = host(&[(V3, this)]);
                let args = [Value::reference(w[0])];
                assert_eq!(
                    slr_lang::vec::normalize(&mut vm, &info(&args, V3, "normalize", "()V")),
                    None
                );
                check_writes(&vm, w[0], &VEC_FIELDS, result, true, id, name);
            }
            "rotate_ypr" => {
                let this = triple(&row["this"]);
                let angles = parts(&row["args"])[0];
                let (mut vm, w) = host(&[(V3, this), (YPR, angles)]);
                let args = [Value::reference(w[1]), Value::reference(w[0])];
                let got = slr_lang::vec::rotate(
                    &mut vm,
                    &info(&args, V3, "rotate", "(Ljava.lang.Ypr;)Ljava.lang.Vector3;"),
                );
                assert_eq!(got, Some(w[0]), "row {id}: the L return is the receiver");
                assert!(
                    row["eax_is_receiver"].as_bool().unwrap(),
                    "row {id}: exe EAX is the receiver"
                );
                check_writes(&vm, w[0], &VEC_FIELDS, result, false, id, name);
            }
            "rotate_axis" => {
                let this = triple(&row["this"]);
                let args_json = row["args"].as_array().unwrap();
                let axis = triple(&args_json[0]);
                let angle = word_f(args_json[1].as_str().unwrap());
                let (mut vm, w) = host(&[(V3, this), (V3, axis)]);
                let args = [
                    Value::float(angle),
                    Value::reference(w[1]),
                    Value::reference(w[0]),
                ];
                let got = slr_lang::vec::rotate_axis(
                    &mut vm,
                    &info(
                        &args,
                        V3,
                        "rotate",
                        "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
                    ),
                );
                assert_eq!(got, Some(w[0]));
                assert!(row["eax_is_receiver"].as_bool().unwrap());
                check_writes(&vm, w[0], &VEC_FIELDS, result, false, id, name);
            }
            "vec_init" => {
                let angles = parts(&row["args"])[0];
                let (mut vm, w) = host(&[(V3, [0.0; 3]), (YPR, angles)]);
                let args = [Value::reference(w[1]), Value::reference(w[0])];
                assert_eq!(
                    slr_lang::vec::init_from_ypr(
                        &mut vm,
                        &info(&args, V3, "<init>", "(Ljava.lang.Ypr;)")
                    ),
                    None
                );
                check_writes(&vm, w[0], &VEC_FIELDS, result, false, id, name);
            }
            "ypr_init" => {
                let v = parts(&row["args"])[0];
                let (mut vm, w) = host(&[(YPR, [0.0; 3]), (V3, v)]);
                let args = [Value::reference(w[1]), Value::reference(w[0])];
                assert_eq!(
                    slr_lang::vec::ypr_init_from_vec3(
                        &mut vm,
                        &info(&args, YPR, "<init>", "(Ljava.lang.Vector3;)"),
                    ),
                    None
                );
                check_writes(&vm, w[0], &YPR_FIELDS, result, false, id, name);
            }
            "ypr_mul" => {
                let this = triple(&row["this"]);
                let other = parts(&row["args"])[0];
                let (mut vm, w) = host(&[(YPR, this), (YPR, other)]);
                let args = [Value::reference(w[1]), Value::reference(w[0])];
                assert_eq!(
                    slr_lang::vec::ypr_mul(&mut vm, &info(&args, YPR, "mul", "(Ljava.lang.Ypr;)V")),
                    None
                );
                check_writes(&vm, w[0], &YPR_FIELDS, result, false, id, name);
            }
            // ---- shared helpers ------------------------------------------
            "mat3_from_ypr" => {
                let angles = triple(&row["args"][0]);
                let m = Mat3::from_ypr(Ypr::new(angles[0], angles[1], angles[2]));
                check_matrix(&m, &result["m"], false, id, name);
            }
            "mat3_mul" => {
                let a = row["args"].as_array().unwrap();
                let left = words(&a[0]);
                let self_m = words(&a[1]);
                let mut m = Mat3 {
                    m: [
                        self_m[0], self_m[1], self_m[2], self_m[3], self_m[4], self_m[5],
                        self_m[6], self_m[7], self_m[8],
                    ],
                };
                m.premul(Mat3 {
                    m: [
                        left[0], left[1], left[2], left[3], left[4], left[5], left[6], left[7],
                        left[8],
                    ],
                });
                check_matrix(&m, &result["m"], true, id, name);
            }
            "mat3_to_ypr" => {
                let m = words(&row["args"][0]);
                let mat = Mat3 {
                    m: [m[0], m[1], m[2], m[3], m[4], m[5], m[6], m[7], m[8]],
                };
                let ypr = mat.to_ypr();
                check_triple([ypr.y, ypr.p, ypr.r], &result["triple"], false, id, name);
            }
            "vec_to_ypr" => {
                let v = triple(&row["args"][0]);
                let ypr = Vec3::new(v[0], v[1], v[2]).to_ypr();
                check_triple([ypr.y, ypr.p, ypr.r], &result["triple"], false, id, name);
            }
            "mat3_from_axis" => {
                let args = row["args"].as_array().unwrap();
                let axis = triple(&args[0]);
                let angle = word_f(args[1].as_str().unwrap());
                let m = Mat3::from_axis_angle(Vec3::new(axis[0], axis[1], axis[2]), angle);
                check_matrix(&m, &result["m"], false, id, name);
            }
            other => panic!("row {id}: unknown name {other}"),
        }
    }
    eprintln!("vector_oracle: {} rows replayed", rows.len());
}

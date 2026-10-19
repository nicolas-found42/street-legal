//! Differential replay of the M1-06 `java.lang.Math` / `Integer` / `Float`
//! oracle vectors (ticket M1-06, issue #33).
//!
//! Reads `re/out/langmath/langmath.jsonl` (git-ignored; produced locally by
//! `re/oracle/langmath_vectors.py gen`, which runs the **real exe code**
//! under Unicorn at `cw 0x007f`, 24-bit x87 precision). Override the path with
//! `SLR_LA_MATH_VECTORS`. Skips with a note when the file is absent, so
//! `cargo test --workspace` stays green without the exe/venv.
//!
//! Seven suites, each replayed through the port's own entry point:
//!
//! - `setrandseed` / `randomize`: the CRT `rand` word the exe left behind.
//!   `randomize` seeds from `__ftol(clock())`, so the row names the clock
//!   reading and the port's [`LangHost::read_clock`] returns it.
//! - `random`: a seeded draw sequence, compared as `f32` bit patterns.
//! - `sqrt`: `FSQRT` at PC=24, compared bit for bit (including the
//!   `0xffc0_0000` indefinite for negatives and the quieted NaNs).
//! - `int_to_string` / `int_to_hex`: the bytes the exe handed its interner,
//!   including the signed-`IDIV` quirk for `0x8000_0000`.
//! - `float_to_string`: `sprintf` of a script format, compared as text.

#[path = "support/string_vm.rs"]
mod string_vm;

use serde_json::Value as Json;
use slr_lang::math::{random, randomize, setrandseed, sqrt};
use slr_lang::number::{float_to_string, to_hex_string, to_string};
use slr_lang::seam::{self, string_bytes, LangHost};
use slr_vm::host::CallInfo;
use slr_vm::values::{TypeTag, Value};
use std::path::PathBuf;

fn vectors_path() -> PathBuf {
    if let Some(p) = std::env::var_os("SLR_LA_MATH_VECTORS") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/langmath/langmath.jsonl")
}

fn load_rows() -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(vectors_path()).ok()?;
    Some(
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("langmath row"))
            .collect(),
    )
}

fn hex_word(s: &str) -> u32 {
    u32::from_str_radix(s.trim().trim_start_matches("0x"), 16)
        .unwrap_or_else(|e| panic!("{s:?}: {e}"))
}

fn word_f(s: &str) -> f32 {
    f32::from_bits(hex_word(s))
}

/// The `(I)V` container `setrandseed` reads: parameter first, receiver last.
fn seed_args(seed: u32) -> [Value; 2] {
    [Value::int(seed as i32), Value::int(0)]
}

/// The row's clock reading preloaded into the host word. With no
/// `clock_source` installed, [`LangHost::read_clock`] returns the stored
/// sample, which is exactly the exe's `clock()` value for that row.
fn host_with_clock(clock: f32) -> LangHost {
    let mut host = LangHost::new();
    host.clock = clock;
    host
}

#[test]
fn math_natives_match_oracle_vectors() {
    let Some(rows) = load_rows() else {
        eprintln!("SLR_ORACLE_SKIPPED: math_natives_match_oracle_vectors");
        eprintln!("SKIP: no langmath vectors (run re/oracle/langmath_vectors.py gen)");
        return;
    };
    assert!(!rows.is_empty());

    let mut counts: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for row in &rows {
        let name = row["name"].as_str().expect("name");
        *counts.entry(name).or_default() += 1;
        match name {
            "setrandseed" => {
                let mut host = LangHost::new();
                let seed = hex_word(row["seed"].as_str().expect("seed"));
                let args = seed_args(seed);
                assert_eq!(setrandseed(&mut host, &args), None, "{}", row["id"]);
                assert_eq!(
                    host.rng.state(),
                    hex_word(row["result"]["rand_state"].as_str().expect("state")),
                    "{} seed {seed:08x}",
                    row["id"]
                );
            }
            "randomize" => {
                let clock = word_f(row["clock"].as_str().expect("clock"));
                let mut host = host_with_clock(clock);
                assert_eq!(randomize(&mut host, &[]), None, "{}", row["id"]);
                assert_eq!(
                    host.rng.state(),
                    hex_word(row["result"]["rand_state"].as_str().expect("state")),
                    "{} clock {clock:?}",
                    row["id"]
                );
            }
            "random" => {
                let mut host = LangHost::new();
                let seed = hex_word(row["seed"].as_str().expect("seed"));
                assert_eq!(setrandseed(&mut host, &seed_args(seed)), None);
                for (i, want) in row["draws"].as_array().expect("draws").iter().enumerate() {
                    let got = random(&mut host, &[]).expect("an F result");
                    assert_eq!(
                        got,
                        hex_word(want.as_str().expect("draw")),
                        "{} draw {i}",
                        row["id"]
                    );
                }
                assert_eq!(
                    host.rng.state(),
                    hex_word(row["result"]["rand_state"].as_str().expect("state")),
                    "{}",
                    row["id"]
                );
            }
            "sqrt" => {
                let mut host = LangHost::new();
                let arg = hex_word(row["arg"].as_str().expect("arg"));
                let args = [Value::int(arg as i32), Value::int(0)];
                let got = sqrt(&mut host, &args).expect("an F result");
                assert_eq!(
                    got,
                    hex_word(row["result"]["st0"].as_str().expect("st0")),
                    "{} arg {arg:08x}",
                    row["id"]
                );
            }
            "int_to_string" | "int_to_hex" => {
                seam::reset();
                let mut vm = string_vm::vm();
                let arg = hex_word(row["arg"].as_str().expect("arg"));
                let args = [Value::int(arg as i32), Value::int(0)];
                let info = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.Integer",
                    name: if name == "int_to_hex" {
                        "toHexString"
                    } else {
                        "toString"
                    },
                    sig: "(I)Ljava.lang.String;",
                };
                let ret = if name == "int_to_hex" {
                    to_hex_string(&mut vm, &info).expect("a String result")
                } else {
                    to_string(&mut vm, &info).expect("a String result")
                };
                let want: Vec<u8> = row["interned"][0]
                    .as_str()
                    .expect("interned")
                    .bytes()
                    .collect();
                assert_eq!(
                    string_bytes(&vm, ret).as_deref(),
                    Some(want.as_slice()),
                    "{} arg {arg:08x}",
                    row["id"]
                );
                seam::reset();
            }
            "float_to_string" => {
                seam::reset();
                let mut vm = string_vm::vm();
                let arg = hex_word(row["arg"].as_str().expect("arg"));
                let format: Vec<u8> = row["format"].as_str().expect("format").bytes().collect();
                let format_word = seam::intern_string(&mut vm, &format);
                let args = [
                    Value::reference_named(format_word, TypeTag::STRING),
                    Value::int(arg as i32),
                    Value::int(0),
                ];
                let info = CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.lang.Float",
                    name: "toString",
                    sig: "(FLjava.lang.String;)Ljava.lang.String;",
                };
                let ret = float_to_string(&mut vm, &info).expect("a String result");
                let want: Vec<u8> = row["interned"][0]
                    .as_str()
                    .expect("interned")
                    .bytes()
                    .collect();
                assert_eq!(
                    string_bytes(&vm, ret).as_deref(),
                    Some(want.as_slice()),
                    "{} arg {arg:08x} format {:?}",
                    row["id"],
                    String::from_utf8_lossy(&format)
                );
                seam::reset();
            }
            other => panic!("unknown suite {other}"),
        }
    }

    assert_eq!(counts.get("setrandseed"), Some(&7), "setrandseed rows");
    assert_eq!(counts.get("randomize"), Some(&6), "randomize rows");
    assert_eq!(counts.get("random"), Some(&10), "random rows");
    assert_eq!(counts.get("sqrt"), Some(&40), "sqrt rows");
    assert_eq!(counts.get("int_to_string"), Some(&17), "int_to_string rows");
    assert_eq!(counts.get("int_to_hex"), Some(&16), "int_to_hex rows");
    assert_eq!(
        counts.get("float_to_string"),
        Some(&52),
        "float_to_string rows"
    );
}

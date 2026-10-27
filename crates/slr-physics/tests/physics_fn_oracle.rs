//! Differential test: the vehicle physics leaf functions against the exe's own code under Unicorn
//! (`re/oracle/physics_fn_vectors.py gen`), word for word and bit for bit.
//!
//! Vectors are git-ignored derived data (`SLR_PHYS_FN_VECTORS` overrides the directory).
//! Unicorn and x87 models can propagate different NaN payloads; slip and Pacejka compare NaN
//! class only, while all other outputs are compared bit for bit. Transcendental operations remain
//! approximate and these vectors do not establish hardware parity.

use slr_physics::tyre::{pacejka_force, slip, torque_curve, torque_curve_obj, SlipConsts};
use slr_x87::X87;
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_PHYS_FN_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/physics_fn")
        })
}

fn rows(name: &str) -> Option<Vec<serde_json::Value>> {
    let text = std::fs::read_to_string(vectors_dir().join(name)).ok()?;
    Some(
        text.lines()
            .map(|l| serde_json::from_str(l).expect("jsonl"))
            .collect(),
    )
}

fn skip(test: &str) {
    eprintln!("SLR_ORACLE_SKIPPED: {test}");
    eprintln!("SKIP: no oracle vectors (run re/oracle/physics_fn_vectors.py gen)");
}

fn word(v: &serde_json::Value) -> u32 {
    v.as_u64().expect("u32") as u32
}

fn words(v: &serde_json::Value) -> Vec<u32> {
    v.as_array().expect("array").iter().map(word).collect()
}

fn same_f32_or_nan(a: u32, b: u32) -> bool {
    a == b || (f32::from_bits(a).is_nan() && f32::from_bits(b).is_nan())
}

fn same_ext_or_nan(got: X87, want: &str) -> bool {
    if st0_hex(got) == want {
        return true;
    }
    let Ok(bytes) = (0..10)
        .map(|i| u8::from_str_radix(&want[i * 2..i * 2 + 2], 16))
        .collect::<Result<Vec<_>, _>>()
    else {
        return false;
    };
    got.is_nan() && X87::from_bytes(bytes.try_into().unwrap()).is_nan()
}

fn st0_hex(value: X87) -> String {
    value
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn torque_curves_match_unicorn_vectors() {
    let Some(all) = rows("torque.jsonl") else {
        return skip("torque_curves_match_unicorn_vectors");
    };
    assert!(!all.is_empty(), "empty oracle vectors");
    let mut bad = Vec::new();
    for (index, row) in all.iter().enumerate() {
        let flat = words(&row["pairs"]);
        let pairs: Vec<[u32; 2]> = flat.chunks(2).map(|c| [c[0], c[1]]).collect();
        let count = row["count"].as_i64().unwrap() as i32;
        let x = word(&row["x"]);
        let got = match row["fn"].as_str().unwrap() {
            "torque_curve" => torque_curve(x, &pairs, count),
            "torque_curve_obj" => torque_curve_obj(x, &pairs, count),
            other => panic!("unknown fn {other}"),
        };
        assert_eq!(row["depth"].as_u64(), Some(1));
        let want = row["st0"].as_str().unwrap();
        if st0_hex(got) != want {
            bad.push(format!(
                "row {index} {}: got {} want {want}",
                row["fn"],
                st0_hex(got)
            ));
        }
    }
    eprintln!(
        "physics_fn_oracle: torque rows={} bad={}",
        all.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(4)]
    );
}

#[test]
fn slip_matches_unicorn_vectors() {
    let Some(all) = rows("slip.jsonl") else {
        return skip("slip_matches_unicorn_vectors");
    };
    assert!(!all.is_empty(), "empty oracle vectors");
    let mut bad = Vec::new();
    let mut nan_payload_differences = 0;
    for (index, row) in all.iter().enumerate() {
        let a = words(&row["args"]);
        let c = words(&row["consts"]);
        let consts = SlipConsts {
            max_long: c[0],
            max_lat: c[1],
            long_weight: c[2],
            lat_weight: c[3],
            blend_gain: c[4],
            threshold: c[5],
            long_len: c[6],
            lat_len: c[7],
        };
        let before = words(&row["out0"]);
        let (x, y) = slip([a[0], a[1], a[2], a[3]], &consts, before[0], before[1]);
        let want = words(&row["out"]);
        nan_payload_differences += usize::from(x != want[0] && same_f32_or_nan(x, want[0]));
        nan_payload_differences += usize::from(y != want[1] && same_f32_or_nan(y, want[1]));
        if !same_f32_or_nan(x, want[0]) || !same_f32_or_nan(y, want[1]) {
            bad.push(format!(
                "row {index}: got [{x:#x},{y:#x}] want {want:x?} args {a:x?}"
            ));
        }
    }
    eprintln!(
        "physics_fn_oracle: slip rows={} bad={} NaN payload differences={nan_payload_differences}",
        all.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} mismatches, first: {:?}",
        bad.len(),
        &bad[..bad.len().min(4)]
    );
}

#[test]
fn pacejka_matches_unicorn_vectors_except_nan_payloads() {
    let Some(all) = rows("pacejka.jsonl") else {
        return skip("pacejka_matches_unicorn_vectors_up_to_the_g11_ceiling");
    };
    assert!(!all.is_empty(), "empty oracle vectors");
    let mut bad = 0usize;
    let mut first = Vec::new();
    let mut nan_payload_differences = 0;
    for (index, row) in all.iter().enumerate() {
        let coef: [u32; 11] = words(&row["coef"]).try_into().unwrap();
        let clamped = row["fn"].as_str().unwrap() == "pacejka_clamped";
        let got = pacejka_force(&coef, word(&row["slip"]), word(&row["fz"]), clamped);
        let want = row["st0"].as_str().unwrap();
        nan_payload_differences += usize::from(st0_hex(got) != want && same_ext_or_nan(got, want));
        if !same_ext_or_nan(got, want) {
            bad += 1;
            if first.len() < 4 {
                first.push(format!(
                    "row {index} {}: got {} want {want}",
                    row["fn"],
                    st0_hex(got)
                ));
            }
        }
    }
    eprintln!(
        "physics_fn_oracle: pacejka rows={} mismatch={bad} NaN payload differences={nan_payload_differences} first={first:?}",
        all.len()
    );
    assert!(bad == 0, "{bad} of {} rows differ: {first:?}", all.len());
}

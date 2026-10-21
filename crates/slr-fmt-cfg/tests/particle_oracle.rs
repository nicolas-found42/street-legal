//! Differential test: the Rust particle / skidmark cfg parsers against the exe's own
//! parsers, executed in Unicorn (`re/oracle/particle_vectors.py gen`).
//!
//! Oracle: **Unicorn function oracle** (FUN_0051ac20 and FUN_00521e10 run as exe code with
//! only file I/O, the allocator and the texture-slot setter hooked; the exe's own strtok,
//! sscanf and stricmp run unmodified). Unicorn ignores x87 precision control (docs/x87.md),
//! so the oracle computes at PC=64: the port is therefore run at `Precision::F32` here and
//! at `Precision::F32` (the game's configuration) in production. Vectors live in
//! git-ignored `re/out/particle/`; the test SKIPs when they are absent.

use serde_json::{json, Value};
use slr_fmt_cfg::particle::{Action, ActionKind, Domain, ParticleCfg};
use slr_fmt_cfg::skid::SkidCfg;
use slr_x87::Precision;
use std::path::PathBuf;

fn vectors_dir() -> PathBuf {
    std::env::var_os("SLR_PARTICLE_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../re/out/particle"))
}

fn load_rows(name: &str) -> Option<Vec<Value>> {
    let path = vectors_dir().join(name);
    let text = std::fs::read_to_string(&path).ok()?;
    let rows: Vec<Value> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(|l| serde_json::from_str(l).expect("vector row is JSON"))
        .collect();
    Some(rows)
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

fn bits(f: f32) -> u32 {
    f.to_bits()
}

fn domain_json(d: &Domain) -> Value {
    json!({ "k": d.kind(), "v": d.words() })
}

fn opt_domain_json(d: &Option<Domain>) -> Value {
    domain_json(d.as_ref().expect("vector inputs always yield a domain"))
}

fn action_json(a: &Action) -> Value {
    let (v, d): (Vec<u32>, Vec<Value>) = match &a.kind {
        ActionKind::Bounce {
            friction,
            resilience,
            cutoff,
            domain,
        } => (
            vec![bits(*friction), bits(*resilience), bits(*cutoff)],
            vec![opt_domain_json(domain)],
        ),
        ActionKind::Damping {
            damping,
            vlow,
            vhigh,
        } => (
            vec![
                bits(damping[0]),
                bits(damping[1]),
                bits(damping[2]),
                bits(*vlow),
                bits(*vhigh),
            ],
            vec![],
        ),
        ActionKind::Explosion {
            center,
            velocity,
            magnitude,
            stdev,
            epsilon,
            lifetime,
        } => (
            vec![
                bits(center[0]),
                bits(center[1]),
                bits(center[2]),
                bits(*velocity),
                bits(*magnitude),
                bits(*stdev),
                bits(*epsilon),
                bits(*lifetime),
            ],
            vec![],
        ),
        ActionKind::Gravity { gravity } => (gravity.iter().map(|f| bits(*f)).collect(), vec![]),
        ActionKind::Move { flags } => (vec![*flags], vec![]),
        ActionKind::Source {
            rate,
            position,
            velocity,
        }
        | ActionKind::DirectSource {
            rate,
            position,
            velocity,
        } => (
            vec![bits(*rate)],
            vec![opt_domain_json(position), opt_domain_json(velocity)],
        ),
        ActionKind::Counter { count, domain } => {
            (vec![0, 0, *count as u32], vec![opt_domain_json(domain)])
        }
    };
    json!({ "t": a.kind.type_id(), "name": a.label, "v": v, "d": d })
}

fn particle_json(c: &ParticleCfg) -> Value {
    let s = &c.state;
    let doms = [
        ("source_size", &s.source_size),
        ("dest_size", &s.dest_size),
        ("source_color", &s.source_color),
        ("dest_color", &s.dest_color),
        ("source_alpha", &s.source_alpha),
        ("dest_alpha", &s.dest_alpha),
        ("source_angle", &s.source_angle),
        ("dest_angle", &s.dest_angle),
        ("texture_offset", &s.texture_offset),
        ("angular_velocity", &s.angular_velocity),
        ("lifetime", &s.lifetime),
    ];
    let mut domains = serde_json::Map::new();
    for (n, d) in doms {
        domains.insert(n.to_string(), domain_json(d));
    }
    json!({
        "limit": c.header.limit, "count": c.header.count,
        "scale": bits(c.header.rate_scale), "tail": c.header.tail,
        "state": {
            "p0": bits(s.param0), "velscale": bits(s.velscale), "kind": s.kind,
            "flags": s.flags, "dim": [s.dimension[0] as u32, s.dimension[1] as u32],
            "tex_origin": [bits(s.tex_origin[0]), bits(s.tex_origin[1])],
            "tex_step": [bits(s.tex_step[0]), bits(s.tex_step[1])],
            "init_tex_origin": [bits(c.initial.tex_origin[0]), bits(c.initial.tex_origin[1])],
            "init_tex_step": [bits(c.initial.tex_step[0]), bits(c.initial.tex_step[1])],
            "init_flags": c.initial.flags,
            "domains": domains,
        },
        "actions": c.actions.iter().map(action_json).collect::<Vec<_>>(),
    })
}

fn skid_json(c: &SkidCfg) -> Value {
    json!({
        "maxcount": c.maxcount as u32,
        "width": [bits(c.width[0]), bits(c.width[1])],
        "height": bits(c.height),
        "uv_normal": c.uv_normal.iter().map(|f| bits(*f)).collect::<Vec<_>>(),
        "uv_end": c.uv_end.iter().map(|f| bits(*f)).collect::<Vec<_>>(),
        "fade": bits(c.fade),
        "one": bits(1.0),
        "tail": [0, 0, 0, 0],
        "textures": c.textures,
    })
}

fn first_diffs(
    rust: &Value,
    oracle: &Value,
    path: String,
    excluded: &[String],
    out: &mut Vec<String>,
) {
    if excluded.contains(&path) {
        return;
    }
    if out.len() >= 6 {
        return;
    }
    match (rust, oracle) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => first_diffs(v, w, format!("{path}.{k}"), excluded, out),
                    None => out.push(format!("{path}.{k} only in rust")),
                }
            }
            for k in b.keys().filter(|k| !a.contains_key(*k)) {
                out.push(format!("{path}.{k} only in oracle"));
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                first_diffs(v, w, format!("{path}[{i}]"), excluded, out);
            }
        }
        _ if rust != oracle => out.push(format!("{path}: rust {rust} oracle {oracle}")),
        _ => {}
    }
}

/// Narrow ADR 0007 exception: original execution confirms the two synthetic
/// state inputs and the stock source action below read a leftover argument slot
/// as the absent radius. Keep the raw oracle result intact; assert both the
/// original bug and the corrected output before excluding only radius words.
fn sphere_exception(bytes: &[u8], rust: &Value, oracle: &Value) -> Result<Vec<String>, String> {
    let (pointer, path, supplied_radius) = match bytes {
        b"[STATE]\r\nsource_size sphere 1 2 3\r\n" => (
            "/state/domains/source_size".to_owned(),
            ".state.domains.source_size".to_owned(),
            0.0f32,
        ),
        b"[STATE]\r\nsource_size sphere 1 2 3 2\r\n" => (
            "/state/domains/source_size".to_owned(),
            ".state.domains.source_size".to_owned(),
            2.0f32,
        ),
        _ => {
            // The complete measured action form, not any NaN, any sphere,
            // any file name, or any missing numeric argument.
            let mut in_action = false;
            let matches = bytes
                .split(|b| *b == b'\n')
                .filter(|line| {
                    if line.windows(8).any(|word| word == b"[ACTION]") {
                        in_action = true;
                        return false;
                    }
                    if line.windows(7).any(|word| word == b"[STATE]") {
                        in_action = false;
                        return false;
                    }
                    if !in_action {
                        return false;
                    }
                    let fields: Vec<_> = line
                        .split(|b| b" \t\r".contains(b))
                        .filter(|field| !field.is_empty())
                        .collect();
                    fields.len() == 11
                        && fields[0] == b"source"
                        && fields[1] == b"s0"
                        && [b"20".as_slice(), b"40".as_slice()].contains(&fields[2])
                        && fields[3..]
                            == [
                                b"point".as_slice(),
                                b"0.00",
                                b"0.00",
                                b"0.00",
                                b"sphere",
                                b"1.00",
                                b"1.00",
                                b"1.00",
                            ]
                })
                .count();
            if matches != 1 {
                return Ok(vec![]);
            }
            let actions = oracle["actions"]
                .as_array()
                .ok_or("missing original actions")?;
            let selected: Vec<_> = actions
                .iter()
                .enumerate()
                .filter(|(_, action)| action["t"] == 10 && action["name"] == "s0")
                .collect();
            if selected.len() != 1 {
                return Err("missing-radius trigger has ambiguous original source action".into());
            }
            let index = selected[0].0;
            (
                format!("/actions/{index}/d/1"),
                format!(".actions[{index}].d[1]"),
                0.0f32,
            )
        }
    };
    let original = oracle.pointer(&pointer).ok_or("missing original sphere")?;
    let corrected = rust.pointer(&pointer).ok_or("missing corrected sphere")?;
    if original["k"] != "sphere" || corrected["k"] != "sphere" {
        return Err("missing-radius trigger did not produce spheres".into());
    }
    let words = original["v"]
        .as_array()
        .ok_or("missing original sphere words")?;
    let fixed = corrected["v"]
        .as_array()
        .ok_or("missing corrected sphere words")?;
    if words.len() != 7 || fixed.len() != 7 {
        return Err("unexpected sphere layout".into());
    }
    let raw = words[3]
        .as_u64()
        .filter(|word| *word <= u32::MAX as u64)
        .ok_or("original radius is not a u32")? as u32;
    if !f32::from_bits(raw).is_nan()
        || words[5] != words[3]
        || words[4] != json!(supplied_radius.to_bits())
        || words[6] != json!((supplied_radius * supplied_radius).to_bits())
    {
        return Err("original missing-radius witness no longer shows the confirmed bug".into());
    }
    let expected = [
        supplied_radius.to_bits(),
        0,
        (supplied_radius * supplied_radius).to_bits(),
        0,
    ];
    if fixed[3..] != expected.map(|word| json!(word)) {
        return Err("corrected sphere did not default only missing radii to zero".into());
    }
    Ok((3..7).map(|index| format!("{path}.v[{index}]")).collect())
}

fn run_rows(
    test: &str,
    file: &str,
    check: impl Fn(&[u8]) -> Value,
) -> Option<(usize, usize, usize)> {
    let Some(rows) = load_rows(file) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}");
        eprintln!(
            "SKIP: no oracle vectors in {} (run re/oracle/particle_vectors.py gen)",
            vectors_dir().display()
        );
        return None;
    };
    assert!(!rows.is_empty(), "{file} has no rows");
    let (mut bad, mut corpus, mut syn) = (0usize, 0usize, 0usize);
    let (mut shown, mut exceptions) = (0, 0);
    for row in &rows {
        assert_eq!(
            row["schema_version"], 2,
            "{file}: regenerate raw original vectors; legacy sphere results were normalized"
        );
        assert_eq!(
            row["result_encoding"], "raw-original-words",
            "{file}: original result encoding is not raw"
        );
        let src = row["src"].as_str().unwrap();
        if src.starts_with("syn:") {
            syn += 1;
        } else {
            corpus += 1;
        }
        let bytes = unhex(row["cfg_hex"].as_str().unwrap());
        let got = check(&bytes);
        let excluded = if file == "particle_parse.jsonl" {
            sphere_exception(&bytes, &got, &row["result"])
                .unwrap_or_else(|error| panic!("{src}: {error}"))
        } else {
            vec![]
        };
        if !excluded.is_empty() {
            exceptions += 1;
            eprintln!("CONFIRMED_DIVERGENCE {src}: missing sphere radii; raw original retained, corrected words checked separately");
        }
        let mut diffs = Vec::new();
        first_diffs(&got, &row["result"], String::new(), &excluded, &mut diffs);
        if !diffs.is_empty() {
            bad += 1;
            if shown < 8 {
                shown += 1;
                eprintln!("MISMATCH {src}: {}", diffs.join("; "));
            }
        }
    }
    eprintln!("{file}: {corpus} asset cfgs + {syn} synthetic; {exceptions} separately checked intentional divergences");
    Some((rows.len(), bad, corpus))
}

#[test]
fn missing_sphere_exception_rejects_unrelated_differences() {
    let bytes = b"[STATE]\r\nsource_size sphere 1 2 3\r\n";
    let rust = particle_json(&ParticleCfg::parse_with(bytes, Precision::F32));
    let mut oracle = rust.clone();
    // Synthetic test data tests the acceptance guard; original bug evidence
    // comes from the separately replayed original vectors below.
    oracle["state"]["domains"]["source_size"]["v"][3] = json!(0x7fc0_1234u32);
    oracle["state"]["domains"]["source_size"]["v"][5] = json!(0x7fc0_1234u32);
    let frozen = oracle.clone();
    let excluded = sphere_exception(bytes, &rust, &oracle).unwrap();
    let mut differences = vec![];
    first_diffs(&rust, &oracle, String::new(), &excluded, &mut differences);
    assert!(differences.is_empty());
    assert_eq!(oracle, frozen, "raw original results must remain untouched");

    let mut wrong = rust.clone();
    wrong["state"]["domains"]["source_size"]["v"][0] = json!(0);
    first_diffs(&wrong, &oracle, String::new(), &excluded, &mut differences);
    assert!(!differences.is_empty(), "center differences must fail");
    wrong = rust.clone();
    wrong["state"]["domains"]["source_size"]["v"][3] = json!(1);
    assert!(sphere_exception(bytes, &wrong, &oracle).is_err());
    assert!(
        sphere_exception(bytes, &rust, &rust).is_err(),
        "normalized original must fail"
    );
    assert!(sphere_exception(
        b"[STATE]\r\nsource_size sphere 1 2 3 2 4\r\n",
        &rust,
        &oracle
    )
    .unwrap()
    .is_empty());

    assert!(
        sphere_exception(
            b"[STATE]\nsource s0 20 point 0.00 0.00 0.00 sphere 1.00 1.00 1.00\n",
            &rust,
            &oracle
        )
        .unwrap()
        .is_empty(),
        "inactive action text is not a trigger"
    );
    let mut extra = oracle.clone();
    extra["unexpected_original_field"] = json!(1);
    differences.clear();
    first_diffs(&rust, &extra, String::new(), &excluded, &mut differences);
    assert!(!differences.is_empty(), "omitted original fields must fail");
}

#[test]
fn missing_sphere_radius_original_witnesses_and_corrected_results() {
    let Some(rows) = load_rows("particle_parse.jsonl") else {
        eprintln!(
            "SLR_ORACLE_SKIPPED: missing_sphere_radius_original_witnesses_and_corrected_results"
        );
        return;
    };
    for name in [
        "syn:sphere_missing_both_radii",
        "syn:sphere_missing_second_radius",
    ] {
        let selected: Vec<_> = rows.iter().filter(|row| row["src"] == name).collect();
        assert_eq!(selected.len(), 1, "regenerate raw vectors including {name}");
        let row = selected[0];
        assert_eq!(row["schema_version"], 2);
        assert_eq!(row["result_encoding"], "raw-original-words");
        let bytes = unhex(row["cfg_hex"].as_str().unwrap());
        let rust = particle_json(&ParticleCfg::parse_with(&bytes, Precision::F32));
        let excluded = sphere_exception(&bytes, &rust, &row["result"]).unwrap();
        assert_eq!(excluded.len(), 4);
        assert_ne!(
            rust, row["result"],
            "witness must demonstrate the original bug"
        );
        let mut diffs = vec![];
        first_diffs(&rust, &row["result"], String::new(), &excluded, &mut diffs);
        assert!(diffs.is_empty(), "non-radius state must match: {diffs:?}");
    }
}

#[test]
fn particle_scripts_match_unicorn_vectors() {
    let Some((n, bad, corpus)) = run_rows(
        "particle_scripts_match_unicorn_vectors",
        "particle_parse.jsonl",
        |bytes| particle_json(&ParticleCfg::parse_with(bytes, Precision::F32)),
    ) else {
        return;
    };
    eprintln!("particle_oracle: rows={n} (asset cfgs={corpus}) mismatches={bad}");
    // The vectors must actually exercise every action type and every domain shape.
    let rows = load_rows("particle_parse.jsonl").unwrap();
    let mut kinds = std::collections::BTreeSet::new();
    let mut types = std::collections::BTreeSet::new();
    let mut note = |d: &Value| {
        kinds.insert(d["k"].as_str().unwrap().to_string());
    };
    for r in &rows {
        for d in r["result"]["state"]["domains"]
            .as_object()
            .unwrap()
            .values()
        {
            note(d);
        }
        for a in r["result"]["actions"].as_array().unwrap() {
            types.insert(a["t"].as_u64().unwrap());
            for d in a["d"].as_array().unwrap() {
                note(d);
            }
        }
    }
    assert_eq!(kinds.len(), 7, "domain kinds covered: {kinds:?}");
    assert_eq!(types.len(), 8, "action types covered: {types:?}");
    assert_eq!(
        bad, 0,
        "{bad} of {n} particle rows differ from FUN_0051ac20"
    );
}

#[test]
fn skid_scripts_match_unicorn_vectors() {
    let Some((n, bad, corpus)) = run_rows(
        "skid_scripts_match_unicorn_vectors",
        "skid_parse.jsonl",
        |bytes| skid_json(&SkidCfg::parse_with(bytes, Precision::F32)),
    ) else {
        return;
    };
    eprintln!("skid_oracle: rows={n} (asset cfgs={corpus}) mismatches={bad}");
    assert_eq!(bad, 0, "{bad} of {n} skid rows differ from FUN_00521e10");
}

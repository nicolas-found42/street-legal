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

fn first_diffs(rust: &Value, oracle: &Value, path: String, out: &mut Vec<String>) {
    if out.len() >= 6 {
        return;
    }
    match (rust, oracle) {
        (Value::Object(a), Value::Object(b)) => {
            for (k, v) in a {
                match b.get(k) {
                    Some(w) => first_diffs(v, w, format!("{path}.{k}"), out),
                    None => out.push(format!("{path}.{k} only in rust")),
                }
            }
        }
        (Value::Array(a), Value::Array(b)) if a.len() == b.len() => {
            for (i, (v, w)) in a.iter().zip(b).enumerate() {
                first_diffs(v, w, format!("{path}[{i}]"), out);
            }
        }
        _ if rust != oracle => out.push(format!("{path}: rust {rust} oracle {oracle}")),
        _ => {}
    }
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
    let mut shown = 0;
    for row in &rows {
        let src = row["src"].as_str().unwrap();
        if src.starts_with("syn:") {
            syn += 1;
        } else {
            corpus += 1;
        }
        let bytes = unhex(row["cfg_hex"].as_str().unwrap());
        let got = check(&bytes);
        if got != row["result"] {
            bad += 1;
            if shown < 8 {
                shown += 1;
                let mut diffs = Vec::new();
                first_diffs(&got, &row["result"], String::new(), &mut diffs);
                eprintln!("MISMATCH {src}: {}", diffs.join("; "));
            }
        }
    }
    eprintln!("{file}: {corpus} asset cfgs + {syn} synthetic");
    Some((rows.len(), bad, corpus))
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

//! Original-confirmed malformed-input bug fixes, separate from parity equality.
//! Generate/replay with re/oracle/particle_exception_probe.py. Raw original
//! pointers and words remain in ignored evidence; none are substituted into the
//! parser. These tests cover only the named trigger inputs and declared shims.
use serde_json::{json, Value};
use slr_fmt_cfg::particle::{ActionKind, Domain, ParticleCfg};
use slr_fmt_cfg::skid::SkidCfg;
use std::path::PathBuf;

fn evidence(test: &str) -> Option<Value> {
    let path = std::env::var_os("SLR_PARTICLE_EXCEPTION_VECTORS")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/particle-exceptions/original.json")
        });
    let Ok(bytes) = std::fs::read(path) else {
        eprintln!("SLR_ORACLE_SKIPPED: {test}; run re/oracle/particle_exception_probe.py");
        return None;
    };
    let result: Value = serde_json::from_slice(&bytes).expect("original exception JSON");
    assert_eq!(result["schema_version"], 1);
    assert_eq!(result["oracle"], "build606-unicorn-original-functions");
    assert_eq!(
        result["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(result["profile"]["cw"], 0x007f);
    assert_eq!(result["rows"].as_array().unwrap().len(), 9);
    Some(result)
}

fn row<'a>(evidence: &'a Value, name: &str, input: &[u8]) -> &'a Value {
    let selected: Vec<_> = evidence["rows"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|row| row["case"] == name)
        .collect();
    assert_eq!(
        selected.len(),
        1,
        "original case {name} must run exactly once"
    );
    let row = selected[0];
    let hex: String = input.iter().map(|byte| format!("{byte:02x}")).collect();
    assert_eq!(
        row["cfg_hex"], hex,
        "original and port inputs must be identical"
    );
    assert!(row["instructions"].as_u64().unwrap() > 0);
    row
}

#[test]
fn original_stack_domains_are_replaced_by_safe_state_or_absence() {
    let Some(evidence) = evidence("original_stack_domains_are_replaced_by_safe_state_or_absence")
    else {
        return;
    };
    let stack_top = evidence["profile"]["stack_top"].as_u64().unwrap();
    let stack_start = stack_top - evidence["profile"]["stack_size"].as_u64().unwrap();
    let cases: [(&str, &[u8]); 4] = [
        (
            "unknown_state_default",
            b"[STATE]\nsource_size blob 1 2 3\n",
        ),
        (
            "unknown_state_after_point",
            b"[STATE]\nsource_size point 7 8 9\nsource_size blob 1 2 3\n",
        ),
        ("truncated_state_domain", b"[STATE]\nsource_size\n"),
        (
            "truncated_source_velocity",
            b"[ACTION]\nsource s0 5 point 0 0 0\n",
        ),
    ];
    for (name, input) in cases {
        let row = row(&evidence, name, input);
        let original = &row["original"];
        let pointer = original["domain_pointer"].as_u64().unwrap();
        assert!(
            (stack_start..stack_top).contains(&pointer),
            "original publishes a dangling stack domain"
        );
        assert_ne!(
            original["domain_first_word"], 0x5f30d4u32,
            "not a valid point domain"
        );
        let corrected = ParticleCfg::parse(input);
        if name == "truncated_source_velocity" {
            assert_eq!(corrected.actions.len(), 1);
            assert_eq!(
                corrected.actions[0].kind,
                ActionKind::Source {
                    rate: 5.,
                    position: Some(Domain::Point { p: [0.; 3] }),
                    velocity: None,
                }
            );
        } else {
            assert_eq!(original["state_flags"], 4);
            assert_eq!(corrected.state.flags, 4);
            let expected = if name == "unknown_state_after_point" {
                Domain::Point { p: [7., 8., 9.] }
            } else {
                ParticleCfg::parse(b"[STATE]\n").state.source_size
            };
            assert_eq!(corrected.state.source_size, expected);
            assert!(
                original["freed"].as_array().unwrap().len() >= 2,
                "original frees a domain and its input buffer"
            );
        }
    }
}

#[test]
fn original_failed_numeric_scans_retain_prior_values_port_defaults_to_zero() {
    let Some(evidence) =
        evidence("original_failed_numeric_scans_retain_prior_values_port_defaults_to_zero")
    else {
        return;
    };
    for token in ["abc", "+3", ".5"] {
        let input =
            format!("[ACTION]\ngravity first 1 2 3\ngravity second {token} {token} {token}\n");
        let name = format!("invalid_gravity_{token}");
        let row = row(&evidence, &name, input.as_bytes());
        let actions = row["original"]["actions"].as_array().unwrap();
        assert_eq!(actions.len(), 2);
        for action in actions {
            assert_eq!(
                action["v"],
                json!([1f32.to_bits(), 2f32.to_bits(), 3f32.to_bits()])
            );
        }
        let corrected = ParticleCfg::parse(input.as_bytes());
        assert_eq!(corrected.actions.len(), 2);
        assert_eq!(
            corrected.actions[0].kind,
            ActionKind::Gravity {
                gravity: [1., 2., 3.]
            }
        );
        assert_eq!(
            corrected.actions[1].kind,
            ActionKind::Gravity { gravity: [0.; 3] }
        );
    }
}

#[test]
fn original_missing_skid_maxcount_depends_on_heap_port_defaults_to_zero() {
    let Some(evidence) =
        evidence("original_missing_skid_maxcount_depends_on_heap_port_defaults_to_zero")
    else {
        return;
    };
    let input = b"width 2 4\nheight 1\n";
    for (name, fill) in [
        ("skid_missing_maxcount_zero", 0u8),
        ("skid_missing_maxcount_poison", 0xa5),
    ] {
        let row = row(&evidence, name, input);
        assert_eq!(row["allocator_fill"], fill);
        assert_eq!(row["original"]["maxcount"], u32::from_le_bytes([fill; 4]));
        assert_eq!(
            row["original"]["width"],
            json!([1f32.to_bits(), 2f32.to_bits()])
        );
        assert_eq!(row["original"]["height"], 1f32.to_bits());
        let corrected = SkidCfg::parse(input);
        assert_eq!(corrected.maxcount, 0);
        assert_eq!(corrected.width, [1., 2.]);
        assert_eq!(corrected.height, 1.);
    }
}

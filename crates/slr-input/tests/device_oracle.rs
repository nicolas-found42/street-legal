//! Differential test: the ported DirectInput device layer against the
//! original's scripted scenarios (`re/oracle/input_device_vectors.py`).
//!
//! Vectors are original-derived local evidence (`re/out/boot/input-device.jsonl`,
//! git-ignored). Without them the test skips unless
//! `SLR_REQUIRE_ORACLE_FIXTURES` is set; `SLR_INPUT_DEVICE_VECTORS` overrides
//! the path.
use serde_json::Value;
use slr_input::device::{DeviceLayer, Image};
use slr_input::scripted::{hex, ScriptedPlatform};

fn vectors() -> Option<String> {
    let path = std::env::var_os("SLR_INPUT_DEVICE_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/boot/input-device.jsonl")
        });
    match std::fs::read_to_string(&path) {
        Ok(text) => Some(text),
        Err(error)
            if error.kind() == std::io::ErrorKind::NotFound
                && std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none() =>
        {
            eprintln!(
                "SLR_ORACLE_SKIPPED: device_layer_matches_the_original_scenarios; local input-device oracle vectors unavailable at {}; set SLR_REQUIRE_ORACLE_FIXTURES=1 to require them",
                path.display()
            );
            None
        }
        Err(error) => panic!("required input-device vectors {}: {error}", path.display()),
    }
}

fn arg(step: &Value, index: usize) -> u32 {
    step["args"][index].as_u64().unwrap() as u32
}

fn validate_rows(id: &str, steps: &[Value], rows: &[Value]) {
    assert_eq!(
        steps.len(),
        rows.len(),
        "{id}: capture must cover every declared step"
    );
    for (index, (step, row)) in steps.iter().zip(rows).enumerate() {
        assert_eq!(
            row["op"], step["op"],
            "{id} step {index}: captured operation"
        );
        let args = step.get("args").cloned().unwrap_or(serde_json::json!([]));
        assert_eq!(row["args"], args, "{id} step {index}: captured arguments");
    }
}

#[test]
fn capture_shape_rejects_dropped_extra_and_mispaired_steps() {
    let steps = vec![serde_json::json!({"op": "axis", "args": [2, 20]})];
    validate_rows("complete", &steps, &steps);
    for rows in [
        vec![],
        vec![steps[0].clone(), steps[0].clone()],
        vec![serde_json::json!({"op": "poll", "args": [2, 20]})],
        vec![serde_json::json!({"op": "axis", "args": [2, 21]})],
    ] {
        assert!(std::panic::catch_unwind(|| validate_rows("invalid", &steps, &rows)).is_err());
    }
}

#[test]
fn missing_capture_emits_the_collectors_skip_marker() {
    let suffix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let missing = std::env::temp_dir().join(format!("slr-input-missing-{suffix}.jsonl"));
    assert!(!missing.exists());
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "device_layer_matches_the_original_scenarios",
            "--nocapture",
        ])
        .env("SLR_INPUT_DEVICE_VECTORS", missing)
        .env_remove("SLR_REQUIRE_ORACLE_FIXTURES")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr)
        .contains("SLR_ORACLE_SKIPPED: device_layer_matches_the_original_scenarios"));
}

#[test]
fn device_layer_matches_the_original_scenarios() {
    let Some(text) = vectors() else { return };
    let mut steps_checked = 0;
    let mut scenarios = 0;
    for line in text.lines() {
        let row: Value = serde_json::from_str(line).unwrap();
        let scenario = &row["scenario"];
        let id = scenario["id"].as_str().unwrap();
        validate_rows(
            id,
            scenario["steps"].as_array().unwrap(),
            row["rows"].as_array().unwrap(),
        );
        let env = &scenario["env"];
        let mut platform = ScriptedPlatform::new(scenario);
        let mut layer = DeviceLayer::new(
            env["hwnd"].as_u64().unwrap() as u32,
            env["ff"].as_u64().unwrap() as u32,
        );
        let mut previous: Image = layer.image.clone();
        for (index, (step, expected)) in scenario["steps"]
            .as_array()
            .unwrap()
            .iter()
            .zip(row["rows"].as_array().unwrap())
            .enumerate()
        {
            let at = format!("{id} step {index} ({})", step["op"]);
            platform.feed(&step["feed"]);
            let mut ret: Option<Value> = expected["ret"].clone().into();
            let mut float: Option<u32> = None;
            match step["op"].as_str().unwrap() {
                "init" => ret = Some(layer.init(&mut platform).unwrap().into()),
                "poll" => float = Some(layer.poll(&mut platform).to_bits()),
                "lastkey" => ret = Some(layer.last_key(&mut platform).into()),
                "isdown" => {
                    ret = Some(
                        layer
                            .is_down(arg(step, 0), arg(step, 1), arg(step, 2))
                            .into(),
                    );
                }
                "axis" => float = Some(layer.axis_value(arg(step, 0), arg(step, 1)).to_bits()),
                "isaxis" => ret = Some(layer.is_axis(arg(step, 0), arg(step, 1)).into()),
                "axisname" => {
                    let mut buffer = [0xccu8; 0x100];
                    let status =
                        layer.axis_name(arg(step, 0) as i32, arg(step, 1) as i32, &mut buffer);
                    ret = Some(serde_json::json!([status, hex(&buffer)]));
                }
                "activeaxis" => ret = Some(layer.active_axis(arg(step, 0) as i32).into()),
                "devname" => {
                    ret = Some(match layer.device_name(arg(step, 0) as i32) {
                        Some(name) => Value::String(hex(&name)),
                        None => Value::Null,
                    });
                }
                "devkind" => ret = Some(layer.device_kind(arg(step, 0) as i32).into()),
                "shutdown" => ret = Some(layer.shutdown(&mut platform).into()),
                other => panic!("unknown op {other}"),
            }
            assert_eq!(
                platform.take_calls(),
                expected["calls"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|c| c.as_str().unwrap().to_owned())
                    .collect::<Vec<_>>(),
                "{at}: DirectInput call stream"
            );
            if let Some(bits) = float {
                assert_eq!(
                    u64::from(bits),
                    expected["st0_f32"].as_u64().unwrap(),
                    "{at}: result bits"
                );
            } else {
                assert_eq!(ret.as_ref().unwrap(), &expected["ret"], "{at}: result");
            }
            let diff: Vec<(u32, u32)> = layer.image.diff(&previous);
            let want: Vec<(u32, u32)> = expected["diff"]
                .as_array()
                .unwrap()
                .iter()
                .map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32))
                .collect();
            assert_eq!(diff, want, "{at}: state words");
            previous = layer.image.clone();
            steps_checked += 1;
        }
        scenarios += 1;
    }
    assert!(
        scenarios >= 10 && steps_checked > 500,
        "{scenarios} scenarios / {steps_checked} steps"
    );
}

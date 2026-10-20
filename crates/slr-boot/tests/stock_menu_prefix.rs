//! Integrated runtime regression, not original-oracle parity acceptance.
use std::process::Command;
#[path = "support/gc_prefix.rs"]
mod gc_prefix;
#[path = "support/object_prefix.rs"]
mod object_prefix;

#[test]
fn unchanged_stock_builds_menu_and_continues_through_frame_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let output = root.join(format!(
        "re/out/boot/test-runs/stock-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let result = Command::new(env!("CARGO_BIN_EXE_slr-boot"))
        .current_dir(&root)
        .args([
            "stock",
            "--config-instance-id",
            "0xffff0080",
            "--game-init-instance-id",
            "0xffff007f",
            "--frames",
            "600",
        ])
        .args(["--output", output.to_str().unwrap()])
        .env("SLR_TRACE_STEPS", "1")
        .env("SLR_TRACE_CALLS", "1")
        .output()
        .expect("run local stock boot");
    // Runtime success is separate from the original native-log comparison.
    assert!(result.status.success());
    object_prefix::assert_original_object_prefix(&root, &output);
    gc_prefix::assert_original_first_sweep(&root, &output);
    let stdout = String::from_utf8(result.stdout).unwrap();
    let trace = String::from_utf8(result.stderr).unwrap();
    assert!(stdout.contains("BOOT scheduler frames=600 ticks=600"));
    let stops: Vec<_> = trace.lines().filter(|l| l.starts_with("STOP ")).collect();
    assert!(stops.is_empty());
    assert!(stdout.contains("BOOT MainMenu dialog complete at frame "));
    let idle = stdout
        .split("idle frames=")
        .nth(1)
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert!(idle >= 300);
    assert!(trace.contains("CALL java.io.Controller.setcontrol()V"));
    for method in [
        "java.game.MainMenuDialog.show()V",
        "java.game.MainMenu.enter(",
    ] {
        assert!(
            trace.lines().any(|line| line.starts_with("STEP ")
                && line.contains(method)
                && line.contains(" op=002b ")),
            "stock menu method did not finish: {method}"
        );
    }
    let log = std::fs::read(output.join("runtime-all.calllog")).unwrap();
    assert_eq!(
        log.windows(b"displayModeName".len())
            .filter(|bytes| *bytes == b"displayModeName")
            .count(),
        6,
        "driver-owned String mode results must still be captured"
    );

    // Primitive resource IDs remain part of acceptance. Read the preserved
    // original, rather than pinning the port allocator to a desired answer.
    let read_log = |path| {
        let mut reader = slr_calllog::LogReader::open(std::io::BufReader::new(
            std::fs::File::open(path).unwrap(),
        ))
        .unwrap();
        let mut records = Vec::new();
        while let Some(record) = reader.next_record().unwrap() {
            records.push(record.clone());
        }
        records
    };
    let expected =
        read_log(root.join("re/out/headless/oracle-33d8aa8/observed/script-natives.slrlog"));
    let actual = read_log(output.join("runtime.calllog"));
    for ordinal in 167..=169 {
        assert_eq!(
            actual[ordinal], expected[ordinal],
            "native resource ID record #{ordinal}"
        );
    }
    // Package-manager indices are primitive results too. Compare the entire
    // stock discovery pass, not just its first previously divergent return.
    let library_indices = |records: &[slr_calllog::LogRecord]| {
        records
            .iter()
            .filter_map(|record| match record {
                slr_calllog::LogRecord::NativeCall {
                    class,
                    name,
                    result,
                    ..
                } if class == "java.lang.System" && name == "openLib" => Some(result.clone()),
                _ => None,
            })
            .collect::<Vec<_>>()
    };
    let expected_libraries = library_indices(&expected);
    assert!(!expected_libraries.is_empty());
    assert_eq!(library_indices(&actual), expected_libraries);
    let packages: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("re/out/headless/package-investigation/full-observed/original-facts.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let packages = &packages["package_history"];
    assert_eq!(packages["truncated"], false);
    assert_eq!(packages["pending"], 0);
    let expected_packages: String = packages["creations"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|entry| entry["index"].as_u64().unwrap() != 0xffff)
        .map(|entry| {
            format!(
                "{}\t{}\n",
                entry["index"].as_u64().unwrap(),
                entry["name"]
                    .as_str()
                    .unwrap()
                    .replace('\\', "/")
                    .to_ascii_lowercase()
            )
        })
        .collect();
    assert_eq!(
        std::fs::read_to_string(output.join("package-indices.tsv")).unwrap(),
        expected_packages
    );
    // Compare the created resource graph too: an allocator-only adjustment
    // could satisfy the ID assertions while omitting ownership/type effects.
    let history: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(
            root.join("re/out/headless/allocation-history/long-observed/original-facts.json"),
        )
        .unwrap(),
    )
    .unwrap();
    let history = &history["resource_history"];
    assert_eq!(history["truncated"], false);
    assert_eq!(history["pending"], 0);
    let expected_graph: Vec<_> = history["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|event| {
            event["operation"] == "register"
                && !matches!(event["descriptor"][0].as_u64(), Some(1 | 8))
        })
        .map(|event| {
            (
                event["id"].as_u64().unwrap(),
                event["descriptor"][0].as_u64().unwrap(),
                event["owner_id"].as_u64().unwrap(),
            )
        })
        .collect();
    let actual_graph: Vec<_> = std::fs::read_to_string(output.join("resource-identities.tsv"))
        .unwrap()
        .lines()
        .map(|line| {
            let fields: Vec<u64> = line.split('\t').map(|v| v.parse().unwrap()).collect();
            (fields[0], fields[1], fields[2])
        })
        .take(expected_graph.len())
        .collect();
    assert!(!expected_graph.is_empty());
    assert_eq!(actual_graph, expected_graph);

    let short = Command::new(env!("CARGO_BIN_EXE_slr-boot"))
        .current_dir(&root)
        .args([
            "stock",
            "--config-instance-id",
            "0xffff0080",
            "--game-init-instance-id",
            "0xffff007f",
            "--frames",
            "200",
        ])
        .output()
        .unwrap();
    assert_eq!(short.status.code(), Some(1));
    assert!(String::from_utf8(short.stderr).unwrap().contains(
        "MainMenuBoundary: frame budget exhausted; observed idle frames=20, require 300"
    ));
}

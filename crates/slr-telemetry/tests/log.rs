//! Structured events reach the session journal as durable `log` rows, filtered
//! by `SLR_LOG`, while stderr text stays exactly what the caller wrote.
use std::path::Path;
use std::process::Command;

const CHILD: &str = "SLR_TELEMETRY_LOG_CHILD";

#[test]
fn log_events_are_journaled_leveled_and_stderr_text_is_unchanged() {
    if let Some(root) = std::env::var_os(CHILD) {
        slr_telemetry::journal::configure(Path::new(&root), "log-run").unwrap();
        slr_telemetry::log::eprintln(
            slr_telemetry::log::Level::Error,
            "boot.stop",
            "STOP animate frame 3: NativeFrameBoundary: player state missing",
        );
        slr_telemetry::log::event(
            slr_telemetry::log::Level::Info,
            "window",
            "presented",
            &[("frame", "8")],
        );
        slr_telemetry::log::event(slr_telemetry::log::Level::Debug, "window", "noisy", &[]);
        return;
    }
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "log-{}-{}",
        std::process::id(),
        slr_telemetry::monotonic_ns()
    ));
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "log_events_are_journaled_leveled_and_stderr_text_is_unchanged",
            "--nocapture",
        ])
        .env(CHILD, &root)
        .env("SLR_LOG", "info")
        .output()
        .expect("spawn child");
    assert!(output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr
            .lines()
            .any(|line| line == "STOP animate frame 3: NativeFrameBoundary: player state missing"),
        "the stderr line is byte-identical: {stderr}"
    );
    let rows: Vec<serde_json::Value> = std::fs::read_to_string(root.join("session-00000.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 2, "debug is below the threshold: {rows:?}");
    assert!(rows.iter().all(|row| row["kind"] == "log"));
    assert_eq!(rows[0]["data"]["level"], "error");
    assert_eq!(rows[0]["data"]["target"], "boot.stop");
    assert_eq!(
        rows[0]["data"]["message"],
        "STOP animate frame 3: NativeFrameBoundary: player state missing"
    );
    assert_eq!(
        rows[1]["data"]["fields"],
        serde_json::json!([["frame", "8"]])
    );
    assert_eq!(rows[1]["sequence"], 2);
}

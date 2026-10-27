//! A panic must leave `failure.json` (message, location, backtrace, last events)
//! in the active bundle, flush the journal, and keep the stderr panic text.
//! Verified by re-executing this test binary so the panic never reaches the
//! test harness.
use std::path::{Path, PathBuf};
use std::process::Command;

const CHILD: &str = "SLR_TELEMETRY_PANIC_CHILD";
const DIRECTORY: &str = "SLR_TELEMETRY_PANIC_DIR";
const JOURNAL: &str = "SLR_TELEMETRY_PANIC_JOURNAL";

#[test]
fn panic_publishes_failure_json_and_flushes_the_journal() {
    if std::env::var_os(CHILD).is_some() {
        let directory = PathBuf::from(std::env::var_os(DIRECTORY).unwrap());
        let journal = PathBuf::from(std::env::var_os(JOURNAL).unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        slr_telemetry::configure(&directory, "panic-run", 16, false, false, false).unwrap();
        slr_telemetry::journal::configure(&journal, "panic-run").unwrap();
        slr_telemetry::install_panic_hook(slr_telemetry::crash_context);
        slr_telemetry::detail("before-panic", vec![("n".into(), "1".into())]);
        slr_telemetry::detail("last-words", vec![("n".into(), "2".into())]);
        panic!("diagnostic panic marker");
    }
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "panic-{}-{}",
        std::process::id(),
        slr_telemetry::monotonic_ns()
    ));
    let (directory, journal) = (root.join("bundle"), root.join("journal"));
    let output = Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "panic_publishes_failure_json_and_flushes_the_journal",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(DIRECTORY, &directory)
        .env(JOURNAL, &journal)
        .output()
        .expect("spawn child");
    assert!(!output.status.success(), "the child must die by panic");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("diagnostic panic marker") && stderr.contains("panicked at"),
        "the previous hook's stderr text is kept: {stderr}"
    );
    let failure: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("failure.json")).unwrap())
            .unwrap();
    assert!(failure["message"]
        .as_str()
        .unwrap()
        .contains("diagnostic panic marker"));
    assert!(failure["location"].as_str().unwrap().contains("panic.rs"));
    assert_eq!(failure["phase"], "panic");
    assert!(failure["backtrace"]
        .as_str()
        .is_some_and(|text| !text.is_empty()));
    assert_eq!(failure["run_id"], "panic-run");
    let events = failure["recent_events"].as_array().expect("recent events");
    let categories: Vec<&str> = events
        .iter()
        .filter_map(|event| event["kind"]["category"].as_str())
        .collect();
    assert!(
        categories.ends_with(&["before-panic", "last-words"]),
        "the last events precede the panic: {categories:?}"
    );
    assert!(failure["diagnostics"]["events_total"].as_u64().unwrap() >= 2);
    let end: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(directory.join("capture-end.json")).unwrap())
            .unwrap();
    assert_eq!(end["status"], "failed");
    assert!(directory.join("crash.json").exists());
    let rows: Vec<serde_json::Value> = std::fs::read_to_string(journal.join("session-00000.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let last = rows.last().expect("journal row");
    assert_eq!(last["kind"], "panic");
    assert!(last["data"]["message"]
        .as_str()
        .unwrap()
        .contains("diagnostic panic marker"));
}

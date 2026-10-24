//! Exercise actual process boundaries, including termination before teardown.
use std::{
    fs,
    io::BufReader,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};

struct Running(Child);
impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn output(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!(
        "../../re/out/boot/test-runs/{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ))
}
fn command(output: &std::path::Path, frames: &str) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_slr-boot"));
    cmd.current_dir(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../.."))
        .args([
            "stock",
            "--config-instance-id",
            "0xffff0080",
            "--game-init-instance-id",
            "0xffff007f",
            "--frames",
            frames,
            "--run-id",
            "capture-test",
            "--event-ring",
            "32",
            "--output",
            output.to_str().unwrap(),
        ]);
    cmd
}

fn stock_available() -> bool {
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock")
        });
    if !stock.join(slr_io::stock::MANIFEST_NAME).is_file() {
        eprintln!("SKIP: no verified stock install at {}", stock.display());
        return false;
    }
    true
}

#[test]
fn failed_run_has_correlated_state_and_durable_end_marker() {
    if !stock_available() {
        return;
    }
    let output = output("failure");
    let result = command(&output, "1").output().unwrap();
    assert_eq!(
        result.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let end: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("capture-end.json")).unwrap()).unwrap();
    assert_eq!(end["status"], "failed");
    assert!(end["all_records"].as_u64().unwrap() > 0);
    let failure: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("failure.json")).unwrap()).unwrap();
    assert!(failure["failure"]
        .as_str()
        .unwrap()
        .contains("MainMenuBoundary"));
    assert_eq!(failure["provenance"]["run_id"], "capture-test");
    assert!(!failure["queues"]["ready"].as_array().unwrap().is_empty());
    assert!(!failure["threads"].as_array().unwrap().is_empty());
    assert!(!failure["objects"].as_array().unwrap().is_empty());
    assert!(failure["diagnostics"]["ring_evicted"].as_u64().unwrap() > 0);
    assert_eq!(failure["recent_events"].as_array().unwrap().len(), 32);
    let lines = fs::read_to_string(output.join("events.jsonl")).unwrap();
    let events: Vec<serde_json::Value> = lines
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert!(events.iter().all(|e| e["run_id"] == "capture-test"));
    assert!(!events.iter().any(|e| e["kind"]["event"] == "step"));
    assert!(
        events.iter().any(|e| e["kind"]["category"] == "gc"),
        "collector phases must be part of the shared durable timeline"
    );
    assert!(events
        .iter()
        .any(|e| e["kind"]["category"] == "native-call"
            && e["context"]["native"]["call_id"].is_number()));
    let native_id = events
        .iter()
        .find(|e| e["kind"]["category"] == "native-call")
        .unwrap()["context"]["native"]["call_id"]
        .as_u64()
        .unwrap();
    let targeted = output.with_extension("targeted");
    let replay = command(&targeted, "1")
        .args(["--snapshot-native", &native_id.to_string()])
        .output()
        .unwrap();
    assert_eq!(
        replay.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&replay.stderr)
    );
    let target: serde_json::Value =
        serde_json::from_slice(&fs::read(targeted.join("divergence.json")).unwrap()).unwrap();
    assert_eq!(target["native_call_id"], native_id);
    assert!(!target["current_thread"]["frames"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        fs::read(output.join("runtime-all.calllog")).unwrap(),
        fs::read(targeted.join("runtime-all.calllog")).unwrap(),
        "targeted observation must preserve the complete native history"
    );
    let before = fs::read(output.join("runtime-all.calllog")).unwrap();
    let second = command(&output, "1").output().unwrap();
    assert_eq!(second.status.code(), Some(1));
    assert_eq!(
        fs::read(output.join("runtime-all.calllog")).unwrap(),
        before,
        "existing evidence must never be overwritten"
    );
}

#[test]
fn killing_process_preserves_prior_native_records_without_completed_marker() {
    if !stock_available() {
        return;
    }
    let output = output("killed");
    let mut process = Running(
        command(&output, "1000000")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        if fs::read_to_string(output.join("events.jsonl"))
            .is_ok_and(|text| text.contains("native-call"))
        {
            break;
        }
        assert!(
            process.0.try_wait().unwrap().is_none(),
            "process exited before live capture"
        );
        assert!(
            Instant::now() < deadline,
            "live native history did not appear"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    process.0.kill().unwrap();
    process.0.wait().unwrap();
    assert!(!output.join("capture-end.json").exists());
    let mut reader = slr_calllog::LogReader::open(BufReader::new(
        fs::File::open(output.join("runtime-all.calllog")).unwrap(),
    ))
    .unwrap();
    assert!(
        reader.next_record().unwrap().is_some(),
        "native record must survive without normal shutdown"
    );
    let capture: serde_json::Value =
        serde_json::from_slice(&fs::read(output.join("capture.json")).unwrap()).unwrap();
    assert_eq!(
        capture["status"], "running",
        "absence of end marker exposes interrupted capture"
    );
}

//! The fatal-signal handler must leave a marker behind and keep the exit
//! status a signal death. Verified by re-executing this test binary in a child
//! process, so the abort never reaches the test harness.
use std::path::{Path, PathBuf};
use std::process::Command;

/// Set in the child so `main` raises instead of running tests.
const CHILD: &str = "SLR_TELEMETRY_FATAL_CHILD";

fn child_binary() -> PathBuf {
    std::env::current_exe().expect("test binary path")
}

#[test]
fn fatal_signal_writes_a_marker_before_dying() {
    if std::env::var_os(CHILD).is_some() {
        // Child: arm the handler for a capture directory and die.
        let directory = PathBuf::from(std::env::var_os("SLR_TELEMETRY_FATAL_DIR").unwrap());
        std::fs::create_dir_all(&directory).unwrap();
        slr_telemetry::configure(&directory, "fatal", 8, false, false, false).unwrap();
        slr_telemetry::install_fatal_handler(&directory.join("capture.json"));
        // SAFETY: raising a signal with the handler installed is the behaviour
        // under test; the handler re-raises with the default action.
        unsafe { libc::raise(libc::SIGSEGV) };
        unreachable!("the handler must not return");
    }
    let directory = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!(
        "fatal-{}-{}",
        std::process::id(),
        slr_telemetry::monotonic_ns()
    ));
    let output = Command::new(child_binary())
        .args(["--exact", "fatal_signal_writes_a_marker_before_dying"])
        .env(CHILD, "1")
        .env("SLR_TELEMETRY_FATAL_DIR", &directory)
        .output()
        .expect("spawn child");
    assert!(
        !output.status.success(),
        "the child must die from the signal"
    );
    let marker = directory.join("crash.json");
    assert!(
        marker.exists(),
        "no crash marker: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let text = std::fs::read_to_string(&marker).unwrap();
    assert!(text.contains("\"reason\":\"signal\""), "{text}");
    assert!(text.contains("\"signal\":11"), "SIGSEGV is 11: {text}");
    assert!(
        text.ends_with("\n"),
        "the marker is one complete line: {text}"
    );
}

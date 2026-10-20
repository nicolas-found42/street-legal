//! Integrated runtime regression, not original-oracle parity acceptance.
use std::process::Command;

#[test]
fn unchanged_stock_builds_menu_and_continues_through_frame_budget() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
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
        .env("SLR_TRACE_STEPS", "1")
        .env("SLR_TRACE_CALLS", "1")
        .output()
        .expect("run local stock boot");
    // Runtime success is separate from the original native-log comparison.
    assert!(result.status.success());
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
    let log = std::fs::read(root.join("re/out/boot/runtime-all.calllog")).unwrap();
    assert_eq!(
        log.windows(b"displayModeName".len())
            .filter(|bytes| *bytes == b"displayModeName")
            .count(),
        6,
        "driver-owned String mode results must still be captured"
    );

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

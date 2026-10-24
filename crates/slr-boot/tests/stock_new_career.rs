//! Drive the shipped New Career dialogs into the original Garage state.
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};
#[test]
fn original_new_career_enters_garage_and_remains_idle() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stock = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("stock"));
    if !stock.join(slr_io::stock::MANIFEST_NAME).exists() {
        eprintln!("SKIP: verified stock unavailable");
        return;
    }
    let output = root.join(format!(
        "re/out/boot/test-runs/career-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&output).unwrap();
    let mut events = Vec::new();
    for (tick, x, y) in [
        (5, 0.35, 0.2),
        (50, 0.4, 0.8),
        (100, 0.5, 0.8),
        (180, 0.5, 0.8),
    ] {
        events.extend([
            json!({"tick":tick,"event":{"type":"pointer","x":x,"y":y}}),
            json!({"tick":tick+1,"event":{"type":"button","pressed":true}}),
            json!({"tick":tick+2,"event":{"type":"button","pressed":false}}),
        ]);
    }
    events.push(json!({"tick":20,"event":{"type":"text","text":"DIAG"}}));
    let tape = output.join("events.json");
    std::fs::write(&tape, serde_json::to_vec(&events).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_slr-boot"))
        .current_dir(&root)
        .arg(stock)
        .arg("--input-script")
        .arg(tape)
        .args(["--frames", "800", "--output"])
        .arg(output.join("runtime"))
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let evidence: Value =
        serde_json::from_slice(&std::fs::read(output.join("runtime/input-state.json")).unwrap())
            .unwrap();
    let history = evidence["history"].as_array().unwrap();
    let stable: Vec<_> = history
        .iter()
        .filter(|row| {
            row["state"]["game_state"] == "java.game.Garage"
                && row["state"]["pending_state_call"] == false
                && !row["state"]["labels"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|label| label == "WELCOME!")
        })
        .collect();
    assert!(
        stable.len() >= 5,
        "garage did not complete entry and remain active: {history:?}"
    );
    let last = &history.last().unwrap()["state"];
    assert_eq!(last["game_state"], "java.game.Garage");
    assert_eq!(last["pending_state_call"], false);
    let labels = last["labels"].as_array().unwrap();
    for label in ["DIAG racing in Green Slip/20", "$20000", "DAY ", "1"] {
        assert!(
            labels.iter().any(|s| s == label),
            "missing original garage HUD {label}"
        );
    }
    assert!(!labels
        .iter()
        .any(|s| s == "MAIN MENU" || s == "Choose character" || s == "WELCOME!"));
    let presentation: Value =
        serde_json::from_slice(&std::fs::read(output.join("runtime/presentation.json")).unwrap())
            .unwrap();
    assert!(
        presentation["draws"].as_u64().unwrap() > labels.len() as u64,
        "native toolbar geometry missing"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(
            &std::fs::read(output.join("runtime/capture-end.json")).unwrap()
        )
        .unwrap()["status"],
        "completed"
    );
}

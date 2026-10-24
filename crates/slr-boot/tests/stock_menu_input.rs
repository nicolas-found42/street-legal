//! Exercise platform events through stock Hotkey/Group callbacks, not host-side
//! menu commands. No licensed assets or oracle captures are committed.
use serde_json::{json, Value};
use std::{path::PathBuf, process::Command};
fn key(events: &mut Vec<Value>, tick: u64, code: u32) {
    events.push(json!({"tick":tick,"event":{"type":"key","code":code,"pressed":true}}));
    events.push(json!({"tick":tick+1,"event":{"type":"key","code":code,"pressed":false}}));
}
fn pointer(events: &mut Vec<Value>, tick: u64, x: f32, y: f32) {
    events.push(json!({"tick":tick,"event":{"type":"pointer","x":x,"y":y}}));
}
fn button(events: &mut Vec<Value>, tick: u64, pressed: bool) {
    events.push(json!({"tick":tick,"event":{"type":"button","pressed":pressed}}));
}
fn has(state: &Value, label: &str) -> bool {
    state["labels"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v == label)
}
#[test]
fn mouse_keyboard_and_text_drive_unchanged_stock_menu_scripts() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stock = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("stock"));
    if !stock.join(slr_io::stock::MANIFEST_NAME).is_file() {
        eprintln!("SKIP: verified stock install unavailable");
        return;
    }
    let output = root.join(format!(
        "re/out/boot/test-runs/input-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&output).unwrap();
    let mut events = Vec::new();
    key(&mut events, 5, 39);
    for tick in [20, 35, 50] {
        key(&mut events, tick, 40);
    }
    key(&mut events, 65, 13);
    key(&mut events, 80, 27);
    pointer(&mut events, 95, 0.35, 0.56);
    button(&mut events, 96, true);
    button(&mut events, 97, false);
    pointer(&mut events, 110, 0.65, 0.41);
    button(&mut events, 111, true);
    pointer(&mut events, 112, 0.99, 0.99);
    button(&mut events, 113, false);
    pointer(&mut events, 125, 0.65, 0.41);
    button(&mut events, 126, true);
    events.push(json!({"tick":127,"event":{"type":"focus_lost"}}));
    button(&mut events, 128, false);
    pointer(&mut events, 140, 0.65, 0.41);
    button(&mut events, 141, true);
    button(&mut events, 142, false);
    key(&mut events, 155, 27);
    events.push(json!({"tick":170,"event":{"type":"key","code":13,"pressed":true}}));
    events.push(json!({"tick":171,"event":{"type":"focus_lost"}}));
    events.push(json!({"tick":172,"event":{"type":"key","code":13,"pressed":false}}));
    events.push(json!({"tick":180,"event":{"type":"key","code":13,"pressed":false}}));
    pointer(&mut events, 195, 0.35, 0.20);
    button(&mut events, 196, true);
    button(&mut events, 197, false);
    pointer(&mut events, 210, 0.65, 0.41);
    button(&mut events, 211, true);
    button(&mut events, 212, false);
    events.push(json!({"tick":225,"event":{"type":"text","text":"AABCD"}}));
    key(&mut events, 245, 8);
    pointer(&mut events, 260, 0.61, 0.80);
    button(&mut events, 261, true);
    button(&mut events, 262, false);
    key(&mut events, 280, 27);
    key(&mut events, 295, 27);
    key(&mut events, 310, 27);
    key(&mut events, 325, 37);
    key(&mut events, 340, 13);
    let script = output.join("events.json");
    std::fs::write(&script, serde_json::to_vec_pretty(&events).unwrap()).unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_slr-boot"))
        .current_dir(&root)
        .arg(stock)
        .arg("--input-script")
        .arg(&script)
        .arg("--output")
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
    // Durable input evidence must retain every event, including events that
    // share a VM frame, and pair each application with its callback outcome.
    let journal: Vec<Value> = std::fs::read_to_string(output.join("runtime/session-00000.jsonl"))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let applied: Vec<_> = journal
        .iter()
        .filter(|row| row["kind"] == "input-apply")
        .collect();
    assert_eq!(applied.len(), events.len());
    for row in applied {
        assert!(row["context"]["frame"].is_number());
        assert!(journal.iter().any(|result| result["kind"] == "input-result"
            && result["data"]["input_id"] == row["sequence"]
            && result["data"]["error"].is_null()));
    }
    let history = evidence["history"].as_array().unwrap();
    let at = |tick: u64| -> &Value {
        &history
            .iter()
            .find(|h| h["tick"] == tick)
            .unwrap_or_else(|| panic!("missing input tick {tick}"))["state"]
    };
    assert_ne!(
        at(0)["selected"],
        at(6)["selected"],
        "arrow key did not change original selection"
    );
    assert!(
        has(at(67), "VIDEO OPTIONS"),
        "keyboard select did not open Options"
    );
    for tick in [82, 98, 114, 129, 157, 173, 181, 263, 297] {
        assert!(
            !has(at(tick), "VIDEO OPTIONS")
                && !has(at(tick), "Enter player name")
                && !has(at(tick), "Do you really want to Exit?"),
            "unexpected action or stuck dialog at {tick}"
        );
    }
    assert!(
        has(at(143), "VIDEO OPTIONS"),
        "mouse click did not open Options"
    );
    assert!(has(at(198), "Enter player name"));
    assert_eq!(
        at(198)["focus"],
        at(213)["focus"],
        "click passed through the modal"
    );
    assert!(
        has(at(247), "AABC_"),
        "original text editing did not handle repeated characters/backspace"
    );
    assert!(has(at(282), "Do you really want to Exit?"));
    assert_eq!(
        history.last().unwrap()["state"]["exit"],
        true,
        "original exit script did not request shutdown"
    );
}

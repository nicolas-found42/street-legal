//! Real-stock render smoke plus a differential guard against presentation
//! changing the VM/native sequence. Assets and captures remain uncommitted.
use std::{path::PathBuf, process::Command};

#[test]
fn stock_menu_renders_without_changing_native_execution() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let stock = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("stock"));
    if !stock.join(slr_io::stock::MANIFEST_NAME).is_file() {
        eprintln!("SKIP: verified stock install unavailable");
        return;
    }
    let output = root.join(format!(
        "re/out/boot/test-runs/presentation-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&output).unwrap();
    for visible in [false, true] {
        let dir = output.join(if visible { "presented" } else { "headless" });
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_slr-boot"));
        cmd.current_dir(&root)
            .arg(&stock)
            .args([
                "--config-instance-id",
                "0xffff0080",
                "--game-init-instance-id",
                "0xffff007f",
                "--frames",
                "600",
                "--output",
            ])
            .arg(&dir);
        if visible {
            cmd.arg("--menu-frame").arg(output.join("menu.png"));
        }
        let result = cmd.output().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
    }
    assert_eq!(
        std::fs::read(output.join("headless/runtime-all.calllog")).unwrap(),
        std::fs::read(output.join("presented/runtime-all.calllog")).unwrap(),
        "presentation changed original script/native execution"
    );
    let manifest: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output.join("presented/presentation.json")).unwrap())
            .unwrap();
    let labels = manifest["labels"].as_array().unwrap();
    for label in [
        "MAIN MENU",
        "NEW CAREER",
        "LOAD CAREER",
        "OPTIONS",
        "EXIT GAME",
    ] {
        assert!(
            labels.iter().any(|v| v == label),
            "missing stock label {label}"
        );
    }
    assert!(
        !labels.iter().any(|v| v == "VIDEO OPTIONS"),
        "hidden options panel rendered"
    );
    let bounds = &manifest["text_bounds"];
    assert!(
        bounds["MAIN MENU"][0].as_f64().unwrap() >= -1.,
        "menu heading clipped"
    );
    assert!(
        bounds["NEW CAREER"][2].as_f64().unwrap() < bounds["QUICKRACE"][0].as_f64().unwrap(),
        "menu columns overlap"
    );
    let (_, image) =
        slr_fmt_texture::pngdec::decode(&std::fs::read(output.join("menu.png")).unwrap()).unwrap();
    assert_eq!((image.width, image.height), (800, 600));
    let colors: std::collections::HashSet<_> = image
        .pixels
        .as_chunks::<4>()
        .0
        .iter()
        .map(|p| [p[0], p[1], p[2]])
        .collect();
    assert!(
        colors.len() > 256,
        "menu framebuffer is blank or lacks decoded stock assets"
    );
}

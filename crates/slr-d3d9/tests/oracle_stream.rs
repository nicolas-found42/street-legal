//! Canonicalizes the headless original's real D3D9 stream (menu scenario) and
//! checks reproducibility invariants. The log is a local, ignored capture made
//! by `re/headless/d3d9_capture.py`; override with `SLR_D3D9_ORACLE_LOG`.
//! Skips with a note when absent unless `SLR_REQUIRE_ORACLE_FIXTURES` is set.
use slr_d3d9::stream::{
    canon::Canonicalizer,
    compare::{compare, Options},
    inventory,
};
use std::{fs::File, io::BufReader, path::PathBuf};

fn oracle_log() -> Option<PathBuf> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    if let Some(path) = std::env::var_os("SLR_D3D9_ORACLE_LOG").map(PathBuf::from) {
        return path.is_file().then_some(path);
    }
    let candidates = [
        root.join("re/out/headless/d3d9-stream/fresh-menu1800M/run.slrlog"),
        root.join("re/out/headless/clock-investigation/menu1800M/run.slrlog"),
    ];
    candidates.into_iter().find(|p| p.is_file())
}

fn open(path: &PathBuf) -> Canonicalizer<BufReader<File>> {
    Canonicalizer::new(BufReader::new(File::open(path).unwrap())).unwrap()
}

#[test]
fn the_original_menu_stream_canonicalizes_and_is_self_equal() {
    let Some(path) = oracle_log() else {
        assert!(
            std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none(),
            "required oracle D3D9 log is missing"
        );
        eprintln!(
            "SKIP: no local oracle D3D9 log; set SLR_REQUIRE_ORACLE_FIXTURES=1 to require it"
        );
        return;
    };
    let mut c = open(&path);
    let (mut frames, mut draws, mut presented) = (0u64, 0u64, 0u64);
    while let Some(f) = c.next_frame().expect("no unknown or unsupported method") {
        frames += 1;
        presented += u64::from(f.presented);
        draws += f.events.iter().filter(|e| e["k"] == "draw").count() as u64;
    }
    assert!(
        presented >= 300,
        "expected the 1.8G-instruction menu run, got {presented} frames"
    );
    assert!(frames >= presented);
    assert!(draws > 10_000);
    let report = compare(&mut open(&path), &mut open(&path), &Options::default()).unwrap();
    assert_eq!(report["equal"], true);
    assert_eq!(report["frames_equal"].as_u64().unwrap(), frames);
    let inv = inventory::scan(BufReader::new(File::open(&path).unwrap())).unwrap();
    assert_eq!(inv["programmable_pipeline"]["fixed_function_only"], true);
    assert_eq!(inv["presents"].as_u64().unwrap(), presented);
}

#[test]
fn missing_explicit_capture_cannot_fall_back_to_an_archived_log() {
    let missing = std::env::temp_dir()
        .join(format!("slr-no-d3d9-capture-{}", std::process::id()))
        .join("missing.slrlog");
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "the_original_menu_stream_canonicalizes_and_is_self_equal",
            "--exact",
            "--nocapture",
        ])
        .env("SLR_D3D9_ORACLE_LOG", &missing)
        .env("SLR_REQUIRE_ORACLE_FIXTURES", "1")
        .output()
        .unwrap();
    assert!(!output.status.success(), "missing explicit capture passed");
    assert!(String::from_utf8_lossy(&output.stderr).contains("required oracle D3D9 log is missing"));
}

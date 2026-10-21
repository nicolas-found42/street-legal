//! Replay the recovered original-executable selection oracle (5 path,
//! 6 directory and 2 direct-file rows). Controlled package cache, successful
//! compiler check and exact stock-byte reads are explicit oracle seams;
//! original OS enumeration, compilation and boot are not established.
use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use slr_vm::vmclass::{ClassId, FileReader, Loader, RequestEvent};

fn stock_root() -> Option<PathBuf> {
    let root = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    root.is_dir().then_some(root)
}

fn rows(name: &str) -> Option<Vec<Value>> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/vmclass110")
        .join(format!("slr-vm110-class-selection-{name}-regen.jsonl"));
    let text = std::fs::read_to_string(path).ok()?;
    Some(
        text.lines()
            .filter(|s| !s.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("oracle JSON"))
            .collect(),
    )
}

const PACKAGE: &str = "java.render.osd.dialog";
const DIRECTORY: &str = "system\\Scripts\\render\\osd\\dialog";

fn controlled(root: &Path, directories: Vec<(String, Vec<String>)>) -> Loader {
    let mut loader = Loader::new();
    loader.boot();
    loader.set_reader(FileReader::stock(root.to_owned()));
    loader.set_package_cache(PACKAGE, directories);
    loader
}

fn dialog_loader(root: &Path) -> Loader {
    let mut names: Vec<_> = std::fs::read_dir(root.join("system/Scripts/render/osd/dialog"))
        .expect("stock dialog directory")
        .map(Result::unwrap)
        .filter(|e| e.path().extension().is_some_and(|ext| ext == "class"))
        .map(|e| e.path().file_stem().unwrap().to_str().unwrap().to_owned())
        .collect();
    names.sort();
    controlled(root, vec![(DIRECTORY.to_owned(), names)])
}

fn events(loader: &mut Loader) -> Vec<Value> {
    loader
        .take_request_events()
        .into_iter()
        .map(|event| match event {
            RequestEvent::CompileCheck {
                directory,
                basename,
            } => json!({"compile_check": [directory, basename]}),
            RequestEvent::ReadRequest(path) => json!({"read_request": path}),
        })
        .collect()
}

fn replay_request(loader: &mut Loader, observation: &Value) {
    let requested = observation["requested"].as_str().expect("requested");
    let before = loader.len();
    let reads = loader.read_count();
    let selected = loader.request(requested).expect("valid stock file");
    assert_eq!(
        loader.len() - before,
        observation["new_class_registrations"].as_u64().unwrap() as usize,
        "{requested}: registrations"
    );
    let expected_events: Vec<_> = observation["events"]
        .as_array()
        .unwrap()
        .iter()
        .map(|event| {
            if event.get("compile_check").is_some() {
                json!({"compile_check": event["compile_check"]})
            } else {
                json!({"read_request": event["read_request"]})
            }
        })
        .collect();
    assert_eq!(
        events(loader),
        expected_events,
        "{requested}: boundary calls"
    );
    assert_eq!(
        loader.read_count() - reads,
        expected_events
            .iter()
            .filter(|event| event.get("read_request").is_some())
            .count(),
        "{requested}: actual reads"
    );
    let diagnostics: Vec<_> = observation["diagnostics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().to_owned())
        .collect();
    assert_eq!(loader.take_diagnostics(), diagnostics, "{requested}: logs");
    if observation["selected"].is_null() {
        assert_eq!(selected, None, "{requested}: null");
    } else {
        let registration = observation["selected"]["registration"].as_u64().unwrap();
        assert_eq!(
            selected.map(ClassId::get),
            Some(registration as u32 + 1),
            "{requested}: selected identity"
        );
        let bare = requested
            .strip_prefix('L')
            .and_then(|s| s.strip_suffix(';'))
            .unwrap_or(requested);
        assert_eq!(selected.and_then(|id| loader.class_name(id)), Some(bare));
    }
}

#[test]
fn request_paths_replay_path_rows() {
    let (Some(root), Some(rows)) = (stock_root(), rows("paths")) else {
        eprintln!("SLR_ORACLE_SKIPPED: request_paths_replay_path_rows");
        return;
    };
    assert_eq!(rows.len(), 5);
    for row in &rows {
        let mut loader = dialog_loader(&root);
        for observation in row["observations"].as_array().unwrap() {
            replay_request(&mut loader, observation);
        }
    }
    eprintln!("original-executable selection replay: paths=5");
}

#[test]
fn request_directories_replay_directory_rows() {
    let (Some(root), Some(rows)) = (stock_root(), rows("directories")) else {
        eprintln!("SLR_ORACLE_SKIPPED: request_directories_replay_directory_rows");
        return;
    };
    assert_eq!(rows.len(), 6);
    let bytes =
        std::fs::read(root.join("system/Scripts/render/osd/dialog/YesNoDialog.class")).unwrap();
    for (i, row) in rows.iter().enumerate() {
        let mut loader = if i < 3 {
            controlled(
                &root,
                vec![
                    (
                        "first\\dialogs".to_owned(),
                        if i == 1 {
                            vec![]
                        } else {
                            vec!["YesNoDialog".to_owned()]
                        },
                    ),
                    // Deliberately deeper: package entry order, never depth, selects.
                    ("second\\dialogs".to_owned(), vec!["YesNoDialog".to_owned()]),
                ],
            )
        } else {
            dialog_loader(&root)
        };
        let mut reader = FileReader::default();
        if i != 2 {
            reader.serve("first\\dialogs\\YesNoDialog.class", bytes.clone());
        }
        // Stale first must not fall back even though the second read WOULD succeed.
        reader.serve("second\\dialogs\\YesNoDialog.class", bytes.clone());
        loader.set_reader(reader);
        replay_request(&mut loader, &row["result"]);
    }
    eprintln!("original-executable selection replay: directories=6");
}

#[test]
fn request_files_replay_file_rows() {
    let (Some(root), Some(rows)) = (stock_root(), rows("files")) else {
        eprintln!("SLR_ORACLE_SKIPPED: request_files_replay_file_rows");
        return;
    };
    assert_eq!(rows.len(), 2);
    for row in &rows {
        let mut loader = dialog_loader(&root);
        // These original rows invoke the parser directly, bypassing getClass:
        // even a cached name's second file registers fresh module identities.
        for receipt in row["receipts"].as_array().unwrap() {
            let loaded = &receipt["loaded"];
            let bytes = std::fs::read(root.join(loaded["file"].as_str().unwrap())).unwrap();
            let modules = loader.load_file(&bytes).unwrap();
            assert_eq!(
                modules.len(),
                loaded["registered_count"].as_u64().unwrap() as usize
            );
            assert_eq!(
                modules[0].0.get(),
                loaded["returned"]["registration"].as_u64().unwrap() as u32 + 1
            );
            let lookup = &receipt["lookup"];
            assert_eq!(
                loader
                    .lookup(lookup["name"].as_str().unwrap())
                    .map(ClassId::get),
                Some(lookup["selected"]["registration"].as_u64().unwrap() as u32 + 1)
            );
        }
        assert_eq!(loader.len(), 12);
        for entry in row["final"].as_array().unwrap() {
            assert_eq!(
                loader
                    .lookup(entry["name"].as_str().unwrap())
                    .map(ClassId::get),
                Some(entry["selected"]["registration"].as_u64().unwrap() as u32 + 1)
            );
        }
    }
    eprintln!("original-executable selection replay: files=2");
}

#[test]
fn stock_package_walk_keeps_package_nodes_separate_and_hits_have_no_side_effects() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: stock_package_walk");
        return;
    };
    let mut loader = Loader::new();
    loader.boot();
    loader.set_reader(FileReader::stock(root.clone()));
    loader.build_package_cache(&root);
    assert_eq!(
        loader
            .request_filename("java.render.osd.dialog.Dialog")
            .as_deref(),
        Some("system\\scripts\\render\\osd\\dialog\\Dialog.class")
    );
    // A basename in a different package must never be selected globally.
    assert_eq!(loader.request_filename("java.lang.YesNoDialog"), None);
    assert_eq!(
        loader.request_filename("java.Render.osd.dialog.YesNoDialog"),
        None
    );
    assert_eq!(
        loader.request_filename("java.render.osd.dialog.yesnodialog"),
        None
    );
    loader
        .request("java.render.osd.dialog.Dialog")
        .unwrap()
        .unwrap();
    loader.take_request_events();
    let reads = loader.read_count();
    let registrations = loader.len();
    loader.set_reader(FileReader::default());
    for name in [
        "java.render.osd.dialog.Dialog",
        "Ljava.render.osd.dialog.Dialog;",
        "java.render.osd.dialog.YesNoDialog",
    ] {
        assert!(loader.request(name).unwrap().is_some());
        assert_eq!(loader.len(), registrations);
        assert_eq!(loader.read_count(), 0);
        assert!(loader.take_request_events().is_empty());
        assert!(loader.take_diagnostics().is_empty());
    }
    assert_eq!(reads, 1);
}

#[test]
fn direct_resource_script_registers_its_actual_package_directory() {
    let Some(root) = stock_root() else {
        eprintln!("SLR_ORACLE_SKIPPED: direct resource script dependencies");
        return;
    };
    let mut loader = Loader::new();
    loader.boot();
    loader.set_reader(FileReader::stock(root.clone()));
    loader.build_package_cache(&root);
    assert_eq!(loader.request_filename("java.game.cars.Prime_models"), None);
    let car = loader
        .request_path("cars\\racers\\Prime_data\\scripts", "Prime_DLH_500")
        .unwrap()
        .unwrap();
    assert_eq!(loader.class_name(car), Some("java.game.cars.Prime_DLH_500"));
    assert_eq!(
        loader
            .request_filename("java.game.cars.Prime_models")
            .as_deref(),
        Some("cars\\racers\\Prime_data\\scripts\\Prime_models.class")
    );
    assert!(loader.lookup("java.game.cars.Prime_models").is_none());
    let base = loader
        .request("java.game.cars.Prime_models")
        .unwrap()
        .unwrap();
    assert_eq!(loader.class_name(base), Some("java.game.cars.Prime_models"));
    assert_eq!(
        loader.request_filename("java.game.parts.Prime_models"),
        None
    );
}

//! Fresh original-native replay through VM-native dispatch and unchanged stock
//! Object/Native/String/File class metadata. OS helper boundaries are controlled
//! in the oracle; Rust uses its existing root-confined FileHost filesystem.
use slr_io::file::{FILE_CLASS, FILE_DELETE, FILE_EXISTS};
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

struct Tree(PathBuf);
impl Tree {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/io")
            .join(format!(
                "path-native-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
        std::fs::create_dir_all(&root).unwrap();
        Self(root)
    }
}
impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[test]
fn stock_file_path_natives_replay_original_native_rows() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let vectors = root.join("re/out/io/path_native.jsonl");
    let Ok(text) = std::fs::read_to_string(vectors) else {
        eprintln!("SLR_ORACLE_SKIPPED: stock_file_path_natives_replay_original_native_rows");
        return;
    };
    let mut vm = VmHost::new();
    for file in ["lang/Object", "lang/Native", "lang/String", "io/File"] {
        let Ok(bytes) = std::fs::read(root.join(format!("stock/system/Scripts/{file}.class")))
        else {
            eprintln!("SLR_ORACLE_SKIPPED: stock_file_path_natives_replay_original_native_rows");
            return;
        };
        vm.loader.load_file(&bytes).unwrap();
    }
    slr_io::install(&mut vm);
    let receiver = vm.alloc_object(FILE_CLASS).unwrap();
    assert!(
        vm.read_field(receiver, "ptr").is_ok(),
        "stock File inherits Native.ptr"
    );
    let rows: Vec<serde_json::Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(rows.len(), 7);
    for row in &rows {
        let name = row["name"].as_str().unwrap();
        let path = row["path"].as_str().unwrap();
        let tree = Tree::new();
        let physical = tree
            .0
            .join(path.split('\0').next().unwrap().replace('\\', "/"));
        if row["present"].as_bool().unwrap() {
            std::fs::create_dir_all(physical.parent().unwrap()).unwrap();
            std::fs::write(&physical, b"unchanged input").unwrap();
        }
        slr_io::reset(tree.0.clone());
        let word = vm.intern_string(Some(path.as_bytes())).unwrap();
        // FileHost has no injected string: the native must read the real VM
        // String object's inherited ptr, not treat its heap ID as char bytes.
        assert!(slr_io::with_files(|files| files.string(word).is_none()));
        let sig = "(Ljava.lang.String;)I";
        let address = vm.natives.resolve(FILE_CLASS, name, sig).unwrap();
        assert_eq!(
            address,
            if name == "exists" {
                FILE_EXISTS
            } else {
                FILE_DELETE
            }
        );
        let args = [Value::reference(word), Value::reference(receiver)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: FILE_CLASS,
            name,
            sig,
        };
        let native = vm.native_impls[&address];
        assert_eq!(
            native(&mut vm, &info),
            Some(row["eax"].as_u64().unwrap() as u32),
            "{name} {path:?}"
        );
        if name == "delete" {
            assert!(!physical.exists(), "delete reaches filesystem");
        } else {
            assert_eq!(physical.exists(), row["present"].as_bool().unwrap());
        }
    }
}

//! Test-only source/corpus binding for the original ray captures.
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::path::PathBuf;
pub fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}
pub fn word(v: &Value) -> u32 {
    v.as_u64().unwrap().try_into().unwrap()
}
pub fn words(v: &Value) -> Vec<u32> {
    v.as_array().unwrap().iter().map(word).collect()
}
pub fn digest(path: impl AsRef<std::path::Path>) -> String {
    format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
}
pub fn load(variable: &str, filename: &str, kind: &str, producers: &[&str]) -> Option<Value> {
    let root = root();
    let path = std::env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/boot").join(filename));
    let Some(path) = crate::oracle_fixture::file(&path) else {
        eprintln!("SLR_ORACLE_SKIPPED: {kind}; run its re/oracle producer");
        return None;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(data["schema"], 1);
    assert_eq!(data["kernel"], kind);
    assert_eq!(data["control_word"], 0x7f);
    assert_eq!(
        data["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(
        data["exe_sha256"],
        digest(root.join("reference/raw/StreetLegal_Redline.bak.exe"))
    );
    let sources = data["sources"].as_object().unwrap();
    let mut expected = producers.to_vec();
    expected.sort();
    assert_eq!(
        sources.keys().map(String::as_str).collect::<Vec<_>>(),
        expected
    );
    for source in producers {
        assert_eq!(
            sources[*source],
            digest(root.join(source)),
            "producer {source}"
        );
    }
    Some(data)
}
pub fn kernel_rows(data: &Value, count: usize, width: usize, expected_sha: &str) {
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), count);
    let mut hash = Sha256::new();
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(word(&row["id"]), index as u32);
        let input = words(&row["words"]);
        assert_eq!(input.len(), width);
        assert_eq!(row["input_unchanged"], true);
        assert_eq!(row["st0"].as_str().unwrap().len(), 20);
        for w in input {
            hash.update(w.to_le_bytes());
        }
    }
    let actual = format!("{:x}", hash.finalize());
    assert_eq!(data["input_sha256"], actual);
    assert_eq!(actual, expected_sha);
}
pub fn negative_kernel_controls(data: &Value, count: usize, width: usize, expected_sha: &str) {
    let mut missing = data.clone();
    missing["rows"].as_array_mut().unwrap().pop();
    assert!(
        std::panic::catch_unwind(|| kernel_rows(&missing, count, width, expected_sha)).is_err()
    );
    let mut duplicate = data.clone();
    duplicate["rows"][0] = duplicate["rows"][count - 1].clone();
    duplicate["rows"][0]["id"] = 0.into();
    assert!(
        std::panic::catch_unwind(|| kernel_rows(&duplicate, count, width, expected_sha)).is_err()
    );
    let mut altered = data.clone();
    altered["rows"][0]["words"][0] = (word(&altered["rows"][0]["words"][0]) ^ 1).into();
    let mut hash = Sha256::new();
    for row in altered["rows"].as_array().unwrap() {
        for w in words(&row["words"]) {
            hash.update(w.to_le_bytes());
        }
    }
    altered["input_sha256"] = format!("{:x}", hash.finalize()).into();
    assert!(
        std::panic::catch_unwind(|| kernel_rows(&altered, count, width, expected_sha)).is_err()
    );
    let mut changed = data.clone();
    changed["rows"][0]["input_unchanged"] = false.into();
    assert!(
        std::panic::catch_unwind(|| kernel_rows(&changed, count, width, expected_sha)).is_err()
    );
}
pub fn returned(row: &Value, got: slr_x87::X87) {
    let hex = got
        .to_bytes()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    assert_eq!(hex, row["st0"].as_str().unwrap(), "case {}", row["id"]);
}

fn feed(hash: &mut Sha256, words: impl IntoIterator<Item = u32>) {
    for word in words {
        hash.update(word.to_le_bytes());
    }
}
pub fn state_digest(data: &Value, tree: bool) -> String {
    let mut hash = Sha256::new();
    for row in data["rows"].as_array().unwrap() {
        feed(
            &mut hash,
            [
                word(&row["world"]),
                word(&row[if tree { "root" } else { "entity" }]),
            ],
        );
        if tree {
            feed(
                &mut hash,
                [u32::from(row["original_triangles"].as_bool().unwrap())],
            );
        }
        for key in ["heap", "globals"] {
            let input = words(&row["before"][key]);
            feed(&mut hash, [input.len().try_into().unwrap()]);
            feed(&mut hash, input);
        }
        if tree {
            for key in ["mapping", "responses"] {
                let mut pairs = row[key]
                    .as_object()
                    .unwrap()
                    .iter()
                    .map(|(k, v)| (k.parse::<u32>().unwrap(), word(v)))
                    .collect::<Vec<_>>();
                pairs.sort_unstable_by_key(|(k, _)| *k);
                feed(&mut hash, [pairs.len().try_into().unwrap()]);
                for (k, v) in pairs {
                    feed(&mut hash, [k, v]);
                }
            }
        }
    }
    format!("{:x}", hash.finalize())
}
pub fn state_rows(data: &Value, count: usize, tree: bool, expected_sha: &str) {
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), count);
    for (index, row) in rows.iter().enumerate() {
        assert_eq!(word(&row["id"]), index as u32);
        assert!(!row["before"]["heap"].as_array().unwrap().is_empty());
        assert_eq!(
            row["before"]["heap"].as_array().unwrap().len(),
            row["after"]["heap"].as_array().unwrap().len()
        );
        assert_eq!(row["before"]["globals"].as_array().unwrap().len(), 33);
        assert_eq!(row["after"]["globals"].as_array().unwrap().len(), 33);
    }
    let actual = state_digest(data, tree);
    assert_eq!(data["input_sha256"], actual);
    assert_eq!(actual, expected_sha);
}
pub fn negative_state_controls(data: &Value, count: usize, tree: bool, expected_sha: &str) {
    let mut missing = data.clone();
    missing["rows"].as_array_mut().unwrap().pop();
    assert!(std::panic::catch_unwind(|| state_rows(&missing, count, tree, expected_sha)).is_err());
    let mut duplicate = data.clone();
    duplicate["rows"][0] = duplicate["rows"][count - 1].clone();
    duplicate["rows"][0]["id"] = 0.into();
    assert!(
        std::panic::catch_unwind(|| state_rows(&duplicate, count, tree, expected_sha)).is_err()
    );
    let mut changed = data.clone();
    changed["rows"][0]["before"]["globals"][0] =
        (word(&changed["rows"][0]["before"]["globals"][0]) ^ 1).into();
    changed["input_sha256"] = state_digest(&changed, tree).into();
    assert!(std::panic::catch_unwind(|| state_rows(&changed, count, tree, expected_sha)).is_err());
    if tree {
        let mut changed = data.clone();
        changed["rows"][count - 1]["original_triangles"] = false.into();
        changed["input_sha256"] = state_digest(&changed, tree).into();
        assert!(
            std::panic::catch_unwind(|| state_rows(&changed, count, tree, expected_sha)).is_err()
        );
    }
}

#[derive(Default)]
pub struct Observations {
    pub queries: Vec<Value>,
    pub trees: Vec<Value>,
}
thread_local! {static OBSERVATIONS:std::cell::RefCell<Option<Observations>>=const{std::cell::RefCell::new(None)};}
/// Passively observe the production path, never supplying results or memory.
pub fn observe<R>(run: impl FnOnce() -> R) -> (R, Observations) {
    struct Restore(Option<Observations>);
    impl Drop for Restore {
        fn drop(&mut self) {
            OBSERVATIONS.with(|slot| *slot.borrow_mut() = self.0.take());
        }
    }
    let _restore = Restore(OBSERVATIONS.with(|slot| slot.replace(Some(Observations::default()))));
    let result = run();
    let observations = OBSERVATIONS.with(|slot| slot.borrow_mut().take().unwrap());
    (result, observations)
}
pub fn record_query(world: u32, ray: [u32; 6], selection: u32, limit_before: u32) {
    OBSERVATIONS.with(|slot|{if let Some(events)=slot.borrow_mut().as_mut(){events.queries.push(serde_json::json!({"world":world,"ray":ray,"selection":selection,"limit_before":limit_before}));}});
}
pub fn record_tree(m: &slr_physics::space::Space, resource: u32) {
    OBSERVATIONS.with(|slot|{if let Some(events)=slot.borrow_mut().as_mut(){events.trees.push(serde_json::json!({"raw":resource,"kind":m.r32(resource+0x4c),"id":m.r32(resource+0x50)}));}});
}

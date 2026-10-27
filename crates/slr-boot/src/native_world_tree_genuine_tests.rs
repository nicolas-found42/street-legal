//! The production picking/loading adapter on original caller pages only.
use crate::native_ray_test_support as proof;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use slr_physics::space::Space;
use std::collections::{BTreeMap, BTreeSet};

fn verify_provenance(data: &Value, root: &std::path::Path) -> Value {
    assert_eq!(data["schema"], 1);
    assert_eq!(data["snapshot"], "gui-cursor-pick-nonempty");
    let required = [
        "re/oracle/headless_snapshot.py",
        "re/oracle/native_cursor_pick_nonempty_probe.py",
        "re/oracle/oracle.py",
        "re/oracle/snapshot_replay.py",
    ];
    assert_eq!(
        data["sources"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        required
    );
    for source in required {
        assert_eq!(data["sources"][source], proof::digest(root.join(source)));
    }
    let snapshot = root.join("re/out/snapshots/gui-cursor-pick-nonempty.json");
    assert_eq!(data["snapshot_metadata_sha256"], proof::digest(&snapshot));
    let meta: Value = serde_json::from_slice(&std::fs::read(snapshot).unwrap()).unwrap();
    assert_eq!(meta["schema_version"], 2);
    assert_eq!(meta["function"], 0x48d450);
    assert_eq!(meta["native_calls"], 7001);
    assert_eq!(data["args"], meta["stack_args"]);
    assert_eq!(data["args"].as_array().unwrap().len(), 10);
    assert_eq!(data["registers"], meta["registers"]);
    assert_eq!(data["entry_sp"], meta["registers"]["esp"]);
    assert_eq!(data["return_pc"], meta["return_address"]);
    assert_eq!(data["heap_ptr"], meta["heap_ptr"]);
    assert_eq!(meta["fpu_cw"], meta["fpu"]["control"]);
    assert_eq!(proof::word(&meta["fpu_cw"]) & 0xf00, 0);
    assert_eq!(data["provenance"], meta["provenance"]);
    assert_eq!(
        meta["provenance"]["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(
        meta["provenance"]["exe_sha256"],
        proof::digest(root.join("reference/raw/StreetLegal_Redline.bak.exe"))
    );
    let mut sources = vec![
        "re/oracle/headless_snapshot.py".to_owned(),
        "re/oracle/oracle.py".to_owned(),
        "acceptance/validators/core.py".to_owned(),
    ];
    for entry in std::fs::read_dir(root.join("re/headless")).unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "py") {
            sources.push(p.strip_prefix(root).unwrap().to_str().unwrap().to_owned());
        }
    }
    sources.sort();
    assert_eq!(
        meta["provenance"]["sources"]
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<Vec<_>>(),
        sources
    );
    for source in sources {
        assert_eq!(
            meta["provenance"]["sources"][&source],
            proof::digest(root.join(source))
        );
    }
    let profiles=["re/out/headless/boot/graphics-profile.json","re/out/headless/boot/platform-profile.json","re/out/headless/hypothetical-cd-toc.json","re/out/headless/window-transcript-native/hypothetical-seven-notifications-declared-handle-profile.json"];
    assert_eq!(
        meta["provenance"]["profiles"]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        profiles
    );
    for profile in profiles {
        assert_eq!(
            meta["provenance"]["profiles"][profile],
            proof::digest(root.join(profile))
        );
    }
    assert_eq!(meta["provenance"]["stock"]["files"], 7858);
    assert_eq!(
        meta["provenance"]["stock"]["manifest_sha256"],
        proof::digest(root.join("stock/.slr-manifest"))
    );
    assert_eq!(data["comparison_policy"], meta["comparison_policy"]);
    assert_eq!(data["comparison_policy"]["id"], "retained-native-memory-v1");
    assert_eq!(
        data["comparison_policy"]["retained_word_exclusions"],
        json!([])
    );
    assert_eq!(
        data["comparison_policy"]["native_stack_bookkeeping"],
        json!([0x7ffdf000u32, 0x7ffe0000u32])
    );
    meta
}

#[test]
fn genuine_nonempty_gui_pick_matches_the_production_adapter_and_retained_state() {
    let root = proof::root();
    let path = std::env::var_os("SLR_CURSOR_PICK_NONEMPTY_VECTORS")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root.join("re/out/boot/native-cursor-pick-nonempty-original.json"));
    let Some(path) = crate::oracle_fixture::file(&path) else {
        eprintln!("SLR_ORACLE_SKIPPED: genuine_nonempty_gui_pick");
        return;
    };
    let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let meta = verify_provenance(&data, &root);
    assert_eq!(data["eax"], 1);
    assert_eq!(data["queries"].as_array().unwrap().len(), 2);
    assert_eq!(data["trees"].as_array().unwrap().len(), 19);
    assert_eq!(data["bodies"].as_array().unwrap().len(), 1);
    assert_eq!(data["bodies"][0]["shape"], 5);
    assert_eq!(data["triangle_calls"], json!([]));
    assert_eq!(data["reached_hooks"], json!([]));
    assert_eq!(
        data["trees"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == 1)
            .count(),
        18
    );
    assert_eq!(
        data["trees"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["kind"] == 2)
            .count(),
        1
    );
    let raw_pages =
        std::fs::read(root.join("re/out/snapshots/gui-cursor-pick-nonempty.pages")).unwrap();
    assert_eq!(raw_pages.len() % 4100, 0);
    assert_eq!(
        data["snapshot_pages_sha256"],
        proof::digest(root.join("re/out/snapshots/gui-cursor-pick-nonempty.pages"))
    );
    assert_eq!(data["snapshot_pages_sha256"], meta["pages_sha256"]);
    let mut original = BTreeMap::new();
    for record in raw_pages.as_chunks::<4100>().0 {
        let a = u32::from_le_bytes(record[..4].try_into().unwrap());
        assert_eq!(a & 4095, 0);
        assert!(original.insert(a, &record[4..]).is_none());
    }
    let mut m = Space::new();
    let _parent = crate::native_stack::enter(&mut m, &[], None);
    let mut before = BTreeMap::new();
    let mut input_hash = Sha256::new();
    assert_eq!(data["pages"].as_array().unwrap().len(), 57);
    for page in data["pages"].as_array().unwrap() {
        let a = proof::word(&page["address"]);
        assert_eq!(a & 4095, 0);
        input_hash.update(a.to_le_bytes());
        let words = proof::words(&page["words"]);
        assert_eq!(words.len(), 1024);
        for (i, w) in words.into_iter().enumerate() {
            let expected = original.get(&a).map_or(0, |bytes| {
                u32::from_le_bytes(bytes[i * 4..i * 4 + 4].try_into().unwrap())
            });
            assert_eq!(w, expected, "incoming original page {a:#x} word {i}");
            input_hash.update(w.to_le_bytes());
            let address = a + i as u32 * 4;
            assert!(before.insert(address, w).is_none());
            m.w32(address, w);
        }
    }
    let input_hash = format!("{:x}", input_hash.finalize());
    assert_eq!(data["input_sha256"], input_hash);
    assert_eq!(
        input_hash,
        "1a39f8bf98447e7a182cbe23efedfe26ac0fd6d73e51387dd35a7833602aa3a7"
    );
    let mut expected = BTreeMap::new();
    let mut diff_hash = Sha256::new();
    assert_eq!(data["diff"].as_array().unwrap().len(), 87);
    for row in data["diff"].as_array().unwrap() {
        let words = proof::words(row);
        assert_eq!(words.len(), 3);
        assert_eq!(before[&words[0]], words[1]);
        assert!(expected.insert(words[0], words[2]).is_none());
        for word in words {
            diff_hash.update(word.to_le_bytes());
        }
    }
    let diff_hash = format!("{:x}", diff_hash.finalize());
    assert_eq!(data["diff_sha256"], diff_hash);
    assert_eq!(
        diff_hash,
        "463e4d956029810663b9f4d5e062f205419d78476d22d45e34240b0a700defa9"
    );
    struct RestoreHost(Option<slr_resource::node::ResHost>);
    impl Drop for RestoreHost {
        fn drop(&mut self) {
            slr_resource::node::with_res(|host| *host = self.0.take().unwrap());
        }
    }
    let _host = RestoreHost(Some(slr_resource::node::with_res(|host| {
        std::mem::replace(host, slr_resource::node::ResHost::new())
    })));
    let mut seen = BTreeSet::new();
    let mut bindings = Vec::new();
    for tree in data["trees"].as_array().unwrap() {
        let raw = proof::word(&tree["raw"]);
        assert_eq!(m.r32(raw + 0x4c), proof::word(&tree["kind"]));
        assert_eq!(m.r32(raw + 0x50), proof::word(&tree["id"]));
        if seen.insert(raw) {
            let mut resource =
                slr_resource::node::Container::new(m.r32(raw + 0x4c) as i32, m.r32(raw + 0x50));
            resource.flags = m.r32(raw + 0x54);
            resource.load_state = m.r32(raw + 0x64);
            let logical = slr_resource::node::with_res(|host| host.add_container(resource));
            bindings.push((logical, raw));
        }
    }
    m.set_heap_ptr(proof::word(&data["heap_ptr"]));
    let args = proof::words(&data["args"]);
    let sp = proof::word(&data["entry_sp"]);
    crate::native_stack::set_sp(&mut m, sp + 44);
    crate::native_stack::begin_call(&mut m, proof::word(&data["return_pc"]), &args, None);
    m.begin_journal();
    let (got, observations) = crate::native_body::with_test_raw_resources(bindings.clone(), || {
        proof::observe(|| {
            crate::native_cursor_runtime::pick_memory(&mut m, args.try_into().unwrap())
        })
    });
    let written = m.end_journal();
    assert_eq!(got.unwrap(), proof::word(&data["eax"]));
    assert_eq!(json!(observations.queries), data["queries"]);
    assert_eq!(json!(observations.trees), data["trees"]);
    for address in expected
        .keys()
        .copied()
        .chain(written.iter().map(|r| r.0))
        .collect::<BTreeSet<_>>()
    {
        if (0x7ffdf000..0x7ffe0000).contains(&address) || (0x7fe00000..sp + 4).contains(&address) {
            continue;
        }
        let want = expected
            .get(&address)
            .or_else(|| before.get(&address))
            .expect("unexpected write outside original observed pages");
        assert_eq!(m.r32(address), *want, "retained word {address:#x}");
    }
    slr_resource::node::with_res(|host| {
        for (logical, raw) in bindings {
            let resource = host.container(logical).unwrap();
            assert_eq!(resource.flags, m.r32(raw + 0x54));
            assert_eq!(resource.load_state, m.r32(raw + 0x64));
        }
    });
}

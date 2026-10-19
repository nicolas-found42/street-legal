//! Differential test for the `java.util.resource.ResourceRef` rows (M1-10).
//!
//! Replays `re/out/res/resnode.jsonl`, produced by
//! `re/oracle/resnode_vectors.py gen`: the REAL exe natives executed under
//! Unicorn on a hand-built `CallInfo` (no JVM boot, no game data), with the
//! `obj` field hooked to answer a probe-allocated node. Override the directory
//! with `SLR_RESNODE_VECTORS`. Skips with a note when the file is absent.
//!
//! # What is compared
//!
//! The facts a ported row is responsible for: the answer word, and the
//! node/container words the row leaves behind (`id`, `container`, the handle
//! list's head, the flags word). The probe's *event* list is not compared
//! here because half of it is probe bookkeeping (its `set_field` hook, the
//! link word each getter helper writes); the port's own event shapes are
//! asserted by the unit tests in `src/node.rs`.
//!
//! # Rows that are not replayed
//!
//! A row whose probe entry carries an `error` is skipped: the exe reached an
//! engine tail the harness does not model. Those rows are listed here with
//! the reason, and `src/node.rs`'s module docs say which tail each one needs:
//!
//! - `load_*` — `0x5447d0`'s result drives `X->vt[0x0c](1.0f)` on a container
//!   whose vtable the probe has no object for.
//! - `cache_*`/`precache_*` — `0x48a920` on an unbound node dereferences the
//!   null container (`0xc`).
//! - `duplicate_other`/`scale_mesh_*` — the same null-container dereference.
//! - `parent_unbound`/`wt_root_unbound`/`first_child_unbound`/
//!   `next_child_unbound` — the getters call their helper with a null node.

mod support;
use serde_json::Value as Json;
use slr_resource::node as n;
use slr_vm::host::{CallInfo, VmHost};
use slr_vm::values::Value;

const RECEIVER: u32 = 0x0050_0000;

fn vectors_dir() -> std::path::PathBuf {
    std::env::var_os("SLR_RESNODE_VECTORS").map_or_else(
        || {
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../re/out/res")
                .to_path_buf()
        },
        std::path::PathBuf::from,
    )
}

fn load(name: &str) -> Option<Vec<Json>> {
    let text = std::fs::read_to_string(vectors_dir().join(name)).ok()?;
    let mut rows = Vec::new();
    for (i, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        rows.push(serde_json::from_str(line).unwrap_or_else(|e| panic!("row {i}: {e}")));
    }
    Some(rows)
}

fn params(row: &Json) -> Vec<(u8, u32)> {
    row["args"]["params"]
        .as_array()
        .expect("params")
        .iter()
        .map(|p| {
            let pair = p.as_array().expect("pair");
            let ty = pair[0].as_str().expect("type");
            (ty.as_bytes()[0], pair[1].as_u64().expect("word") as u32)
        })
        .collect()
}

fn call(vm: &mut VmHost, row: &n::ResBinding, words: &[(u8, u32)]) -> Option<u32> {
    let mut args: Vec<Value> = words
        .iter()
        .rev()
        .map(|(ty, word)| match ty {
            b'F' => Value::float(f32::from_bits(*word)),
            b'L' | b'[' => Value::reference(*word),
            _ => Value::int(*word as i32),
        })
        .collect();
    args.push(Value::reference(RECEIVER));
    n::bridge(
        vm,
        &CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: row.class,
            name: row.name,
            sig: row.sig,
        },
    )
}

#[test]
fn the_resource_ref_rows_replay() {
    let Some(rows) = load("resnode.jsonl") else {
        eprintln!("SKIP: no re/out/res/resnode.jsonl (run re/oracle/resnode_vectors.py gen)");
        return;
    };
    let mut checked = 0;
    let mut skipped = 0;
    for row in &rows {
        let id = row["id"].as_str().expect("id");
        let name = row["name"].as_str().expect("name");
        let sig = row["sig"].as_str().expect("sig");
        let want = &row["result"];
        if want["error"].as_str().is_some() {
            skipped += 1;
            continue;
        }

        let binding = n::RESOURCE_REF_BINDINGS
            .iter()
            .chain(n::GAME_REF_BINDINGS)
            .find(|b| b.name == name && b.sig == sig)
            .unwrap_or_else(|| panic!("{id}: no binding for {name}{sig}"));

        n::reset_res();
        // plant the row's world: a node (or none), a container with its type
        // and id, and the object binding
        let world = &row["world"];
        let has_node = world["node"].as_bool().unwrap_or(true);
        let node_id = world["node_id"].as_u64().unwrap_or(0) as u32;
        let container_type = world["container"].as_u64().map(|v| v as i32);
        let ground_precache = world["ground_precache"].as_i64().unwrap_or(0) as i32;
        let (node_word, container_word) = n::with_res(|h| {
            h.set_ground_precache(ground_precache);
            let container = container_type.map(|ty| {
                let mut c = n::Container::new(ty, node_id);
                c.parent = world["parent"].as_u64().unwrap_or(0) as u32;
                c.wt_root = world["wt_root"].as_u64().unwrap_or(0) as u32;
                c.first_child = world["first_child"].as_u64().unwrap_or(0) as u32;
                h.add_container(c)
            });
            let node = has_node.then(|| h.add_node_for_test(node_id, container.unwrap_or(0)));
            if let Some(node) = node {
                h.bind_object(RECEIVER, node);
            }
            (node.unwrap_or(0), container.unwrap_or(0))
        });

        let mut vm = VmHost::new();
        support::resource_classes(&mut vm);
        n::install(&mut vm);
        let got = call(&mut vm, binding, &params(row));

        // A `void` row leaves whatever the marshaller left in `eax`; the
        // native result is nothing, so only the effect is compared.
        if n::return_type(sig) == b'V' {
            assert_eq!(got, None, "{id}: void");
        } else if n::return_type(sig) == b'F' {
            // a float row answers on ST0; the probe records it there
            let want_bits = want["st0"].as_f64().map(|v| (v as f32).to_bits());
            assert_eq!(got, want_bits, "{id}: float result");
        } else if matches!(
            name,
            "getParent" | "getWTRoot" | "getFirstChild" | "getNextChild"
        ) {
            // the wrapper word is each side's own arena, so compare whether
            // the row handed a wrapper back, not the word itself
            let want_word = want["eax"].as_u64().unwrap_or(0);
            assert_eq!(
                got.unwrap_or(0) != 0,
                want_word != 0,
                "{id}: wrapper returned"
            );
        } else {
            let want_eax = want["eax"].as_u64().map(|w| w as u32);
            assert_eq!(got, want_eax, "{id}: result");
        }

        // The node's own words, for the rows that leave it alive. The
        // probe's `free` is a no-op, so after `deleteNative` its memory is
        // still readable; the port drops the node instead, which is the
        // honest behaviour, so those rows compare only the answer word.
        if node_word != 0 && name != "deleteNative" {
            let want_node = &want["node"];
            n::with_res(|h| {
                if let Some(node) = h.node(node_word) {
                    assert_eq!(
                        node.id,
                        want_node["id"].as_u64().unwrap_or(0) as u32,
                        "{id}: node id"
                    );
                    // the words are each side's own arena, so compare
                    // whether the node is bound, not the word itself
                    assert_eq!(
                        node.container != 0,
                        want_node["container"].as_u64().unwrap_or(0) != 0,
                        "{id}: node bound"
                    );
                }
            });
        }
        // The flag rows' observable side effect is the container's flags.
        if matches!(name, "setFlags" | "clearFlags" | "setActiveCollision") && container_word != 0 {
            let want_flags = want["container"]["flags"].as_u64().unwrap_or(0) as u32;
            n::with_res(|h| {
                assert_eq!(
                    h.container(container_word).map_or(0, |c| c.flags),
                    want_flags,
                    "{id}: flags"
                );
            });
        }
        // `load`'s observable side effect is the precache flag.
        if name == "load" && container_word != 0 {
            let want_flags = want["container"]["flags"].as_u64().unwrap_or(0) as u32;
            n::with_res(|h| {
                assert_eq!(
                    h.container(container_word).map_or(0, |c| c.flags),
                    want_flags,
                    "{id}: flags"
                );
            });
        }
        checked += 1;
    }
    eprintln!("replayed {checked} rows, {skipped} skipped");
    assert!(
        checked >= 30,
        "replayed only {checked} rows ({skipped} skipped)"
    );
    assert!(skipped >= 10, "only {skipped} rows were skipped");
}

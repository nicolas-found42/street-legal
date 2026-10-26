//! Full GroundRef environment native bridges against original instructions.
mod support;

use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use slr_physics::{cfg::PHYSICS_WORLD as WORLD, space::Space};
use slr_resource::{environment as e, node as n};
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value as Cell,
    vmclass::Class,
};
use std::cell::RefCell;

thread_local! {
    static MEMORY: RefCell<Space> = RefCell::new(Space::new());
}

fn water_consumer(_: &mut VmHost, command: e::WaterCommand) {
    MEMORY.with(|memory| e::apply_water(&mut memory.borrow_mut(), WORLD, command));
}

fn fog_consumer(_: &mut VmHost, node: u32, fog: e::Fog) {
    e::with_environment(|state| state.fog_messages.push((node, fog)));
}

fn seed_memory(state: &e::Environment) {
    MEMORY.with(|slot| {
        let mut memory = Space::new();
        for (offset, values) in [
            (0x1b8, state.position),
            (0x1c4, state.normal),
            (0x1d8, state.extra),
        ] {
            for (index, word) in values.into_iter().enumerate() {
                memory.w32(WORLD + offset + index as u32 * 4, word);
            }
        }
        memory.w32(WORLD + 0x80, state.enabled);
        memory.w32(WORLD + 0x1e4, state.sizes[0]);
        memory.w32(WORLD + 0x1e8, state.sizes[1]);
        let buffer = memory.alloc(state.limits.len() as u32 * 24);
        for (index, word) in state.limits.iter().flatten().enumerate() {
            memory.w32(buffer + index as u32 * 4, *word);
        }
        memory.w32(WORLD + 0x1d0, buffer);
        memory.w32(WORLD + 0x1d4, state.limits.len() as u32);
        *slot.borrow_mut() = memory;
    });
}

fn memory_projection() -> Value {
    MEMORY.with(|slot| {
        let memory = slot.borrow();
        let vector =
            |offset| std::array::from_fn::<_, 3, _>(|i| memory.r32(WORLD + offset + i as u32 * 4));
        let buffer = memory.r32(WORLD + 0x1d0);
        let limits: Vec<_> = (0..memory.r32(WORLD + 0x1d4))
            .map(|index| {
                std::array::from_fn::<_, 6, _>(|i| memory.r32(buffer + index * 24 + i as u32 * 4))
            })
            .collect();
        json!({"enabled":memory.r32(WORLD + 0x80), "position":vector(0x1b8),
            "normal":vector(0x1c4),"extra":vector(0x1d8),
            "sizes":[memory.r32(WORLD+0x1e4),memory.r32(WORLD+0x1e8)],"limits":limits})
    })
}

fn words<const N: usize>(value: &Value) -> [u32; N] {
    std::array::from_fn(|index| value[index].as_u64().unwrap() as u32)
}

fn state(value: &Value) -> e::Environment {
    e::Environment {
        enabled: value["enabled"].as_u64().unwrap() as u32,
        position: words(&value["position"]),
        normal: words(&value["normal"]),
        extra: words(&value["extra"]),
        sizes: words(&value["sizes"]),
        limits: value["limits"]
            .as_array()
            .unwrap()
            .iter()
            .map(words)
            .collect(),
        fog_messages: vec![],
    }
}

fn projection(state: &e::Environment) -> Value {
    json!({"enabled":state.enabled,"position":state.position,"normal":state.normal,
        "extra":state.extra,"sizes":state.sizes,"limits":state.limits})
}

fn vm() -> VmHost {
    let mut vm = VmHost::new();
    support::resource_classes(&mut vm);
    support::vector_classes(&mut vm);
    vm.loader.insert_test_class(Class {
        name: "java.util.resource.GroundRef".into(),
        base: Some(n::GAME_REF_CLASS.into()),
        kind: 0,
        interfaces: vec![],
        methods: vec![],
        bodies: vec![],
        fields: vec![],
    });
    e::reset();
    n::reset_res();
    e::install(&mut vm, water_consumer);
    e::install_fog(&mut vm, fog_consumer);
    vm
}

fn vector(vm: &mut VmHost, words: [u32; 3]) -> Cell {
    let word = vm.alloc_object("java.lang.Vector3").unwrap();
    for (name, word_value) in ["x", "y", "z"].into_iter().zip(words) {
        vm.write_field(word, name, Cell::float(f32::from_bits(word_value)))
            .unwrap();
    }
    Cell::reference(word)
}

#[test]
fn ground_environment_natives_match_original_vectors() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../re/out/ground_environment.json");
    let Ok(text) = std::fs::read_to_string(path) else {
        assert!(
            std::env::var_os("SLR_REQUIRE_ORACLE").is_none(),
            "required fixture missing: run re/oracle/ground_environment_probe.py"
        );
        eprintln!("SLR_ORACLE_SKIPPED: ground_environment_natives_match_original_vectors");
        return;
    };
    let data: Value = serde_json::from_str(&text).unwrap();
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let digest = |path: &str| {
        format!(
            "{:x}",
            Sha256::digest(std::fs::read(root.join(path)).unwrap())
        )
    };
    assert_eq!(
        data["schema_version"], 2,
        "regenerate stale environment vectors"
    );
    assert_eq!(
        data["producer_sha256"],
        digest("re/oracle/ground_environment_probe.py")
    );
    assert_eq!(
        data["exe_sha256"],
        digest("reference/raw/StreetLegal_Redline.bak.exe")
    );
    assert_eq!(
        data["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    assert_eq!(data["fpu_cw"], 0x007f);
    assert_eq!(
        data["addresses"],
        json!([
            e::SET_WATER,
            e::SET_WATER_PLANE,
            e::ADD_WATER_LIMIT,
            e::SET_FOG,
            0x49b440,
            0x49b530
        ])
    );
    let rows = data["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 576);
    assert_eq!(data["row_count"], rows.len());
    for (index, row) in rows.iter().enumerate() {
        let mut vm = vm();
        let receiver = vm.alloc_object("java.util.resource.GroundRef").unwrap();
        let bound = row["bound"].as_bool().unwrap();
        let node = n::with_res(|host| host.add_node_for_test(1, if bound { 99 } else { 0 }));
        vm.write_field(receiver, "ptr", Cell::int(node as i32))
            .unwrap();
        e::with_environment(|host| *host = state(&row["before"]));
        seed_memory(&state(&row["before"]));
        let value = &row["values"];
        let scalars: Vec<_> = value["scalars"]
            .as_array()
            .unwrap()
            .iter()
            .map(|word| Cell::float(f32::from_bits(word.as_u64().unwrap() as u32)))
            .collect();
        let (address, mut args) = match row["kind"].as_str().unwrap() {
            "simple" => (e::SET_WATER, scalars),
            "plane" => {
                let mut args = vec![
                    vector(&mut vm, words(&value["vectors"][0])),
                    vector(&mut vm, words(&value["vectors"][1])),
                ];
                args.extend(scalars);
                (e::SET_WATER_PLANE, args)
            }
            "limit" => (
                e::ADD_WATER_LIMIT,
                vec![
                    vector(&mut vm, words(&value["vectors"][0])),
                    vector(&mut vm, words(&value["vectors"][1])),
                ],
            ),
            "fog" => {
                let mut args = vec![Cell::int(value["color"].as_u64().unwrap() as i32)];
                args.extend(scalars);
                (e::SET_FOG, args)
            }
            _ => unreachable!(),
        };
        args.reverse();
        args.push(Cell::reference(receiver));
        let &(_, name, sig) = e::BINDINGS.iter().find(|row| row.0 == address).unwrap();
        let implementation = vm.native_impls[&address];
        assert_eq!(
            implementation(
                &mut vm,
                &CallInfo {
                    jvm: 0,
                    thread: 0,
                    args: &args,
                    class: "java.util.resource.GroundRef",
                    name,
                    sig
                }
            ),
            None
        );
        e::with_environment(|host| {
            assert_eq!(memory_projection(), row["after"], "row {index}: {row}");
            let messages: Vec<_> = host
                .fog_messages
                .iter()
                .map(|(target, fog)| {
                    assert_eq!(*target, node);
                    json!({"code":0x4a,"payload":fog.words()})
                })
                .collect();
            assert_eq!(json!(messages), row["messages"], "fog row {index}");
        });
    }
    let helpers = data["helpers"].as_array().unwrap();
    assert_eq!(helpers.len(), 64);
    assert_eq!(data["helper_count"], helpers.len());
    for (index, row) in helpers.iter().enumerate() {
        let mut host = state(&row["before"]);
        let mask = row["mask"].as_u64().unwrap();
        let optional = |bit: usize| (mask & (1 << bit) != 0).then(|| words(&row["inputs"][bit]));
        host.replace_water(words(&row["sizes"]), optional(0), optional(1), optional(2));
        assert_eq!(projection(&host), row["after"], "helper {index}: {row}");
    }
}

#[test]
fn water_replacement_clears_limits_and_preserves_absent_optional_vectors() {
    let mut state = e::Environment {
        extra: [1, 2, 3],
        ..e::Environment::default()
    };
    assert_eq!(state.add_water_limit([0; 3], [1; 3]), 0);
    assert_eq!(state.add_water_limit([2; 3], [3; 3]), 1);
    state.replace_water([1.0f32.to_bits(); 2], None, None, None);
    assert!(state.limits.is_empty());
    assert_eq!(state.extra, [1, 2, 3]);
    assert_eq!(state.normal, [0; 3]);
}

#[test]
fn disabled_water_keeps_the_previous_plane_and_limits() {
    let mut state = e::Environment {
        sizes: [10, 20],
        normal: [30, 40, 50],
        limits: vec![[1; 6]],
        enabled: 1,
        ..e::Environment::default()
    };
    let before = state.clone();
    state.set_water(false, [1.0f32.to_bits(); 2], [0; 3], [0; 3]);
    assert_eq!(state, before);
    state.set_water(true, [0, 1.0f32.to_bits()], [0; 3], [0; 3]);
    assert_eq!(state.enabled, 0);
    state.enabled = before.enabled;
    assert_eq!(state, before);
}

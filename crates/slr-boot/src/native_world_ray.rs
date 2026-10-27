//! Original49e3d0 ray setup, hit-global ownership and hit-point conversion.
//! The49e710 tree traversal is a live collaborator; an unimplemented nonempty
//! tree must return an error rather than manufacture a no-hit observation.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn value(word: u32) -> X87 {
    X87::from_f32(f32::from_bits(word))
}
fn store(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}

pub trait Host {
    fn traverse(&mut self, m: &mut Space, world: u32, tree: u32) -> Result<u32, String>;
}

/// The caller supplies the actual world's selected tree owner. The optional
/// limit corresponds to the original nullable third argument.
/// exe: FUN_0049e3d0
pub fn query(
    m: &mut Space,
    world: u32,
    ray: [u32; 6],
    selection: u32,
    limit: Option<&mut u32>,
    host: &mut dyn Host,
) -> Result<u32, String> {
    for (i, word) in ray.into_iter().enumerate() {
        m.w32(0x643538 + i as u32 * 4, word);
    }
    let delta = [0, 1, 2].map(|i| value(ray[i + 3]).sub(value(ray[i]), PC, RC));
    // Original49e433..49e441 adds Y² and Z² before X²; reassociation
    // changes stored normal finite results under the game's precision.
    let length = delta[1]
        .mul(delta[1], PC, RC)
        .add(delta[2].mul(delta[2], PC, RC), PC, RC)
        .add(delta[0].mul(delta[0], PC, RC), PC, RC)
        .sqrt(PC, RC);
    m.w32(0x64355c, store(length));
    let inverse = value(0x3f800000).div(value(m.r32(0x64355c)), PC, RC);
    for i in 0..3 {
        m.w32(
            0x643550 + i as u32 * 4,
            store(inverse.mul(delta[i], PC, RC)),
        );
        m.w32(
            0x643564 + i as u32 * 4,
            store(
                delta[i]
                    .mul(value(0x3f000000), PC, RC)
                    .add(value(ray[i]), PC, RC),
            ),
        );
        m.w32(
            0x643570 + i as u32 * 4,
            store(delta[i].mul(value(0x3f000000), PC, RC)) & 0x7fffffff,
        );
    }
    m.w32(0x64357c, u32::from(limit.is_some()));
    m.w32(0x643560, 1);
    m.w32(0x643580, 0x3f800000);
    for address in (0x643584..=0x643598).step_by(4) {
        m.w32(address, 0);
    }
    if m.r32(0x6435b8) == 0 {
        m.w32(0x6435b4, 0);
    } else {
        crate::native_install::release(m, 0x6435ac);
    }
    crate::native_car_command::unlink_entity(m, 0x6435a4);
    m.w32(0x64359c, u32::MAX);
    m.w32(0x6435a0, u32::MAX);
    let tree = m.r32(selection + 12);
    if tree == 0 {
        return Ok(0);
    }
    let found = host.traverse(m, world, tree)?;
    let Some(limit) = limit else {
        return Ok(found);
    };
    let distance = value(m.r32(0x643580));
    let condition = distance.fcom(value(0x3f800000)).0;
    if !condition.c0 || condition.c2 {
        return Ok(0);
    }
    for i in 0..3 {
        let source = value(m.r32(0x643538 + i * 4));
        let target = value(m.r32(0x643544 + i * 4));
        m.w32(
            0x643584 + i * 4,
            store(
                source
                    .sub(target, PC, RC)
                    .mul(distance, PC, RC)
                    .add(target, PC, RC),
            ),
        );
    }
    let body = m.r32(0x6435a4);
    if body != 0 {
        let block = body + m.r32(body + 0x20c) * 0xd4;
        for i in 0..3 {
            m.w32(
                0x643584 + i * 4,
                store(value(m.r32(0x643584 + i * 4)).sub(
                    value(m.r32(block + 0x64 + i * 4)),
                    PC,
                    RC,
                )),
            );
        }
        crate::native_car_command::transpose_vector(m, 0x643584, block + 0xa4, 0x643584);
    }
    *limit = m.r32(0x643580);
    Ok(1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::path::PathBuf;
    const BASE: u32 = 0x20000000;
    const GLOBAL: u32 = 0x643538;
    const COUNT: usize = 33;
    fn word(v: &Value) -> u32 {
        v.as_u64().unwrap().try_into().unwrap()
    }
    fn words(v: &Value) -> Vec<u32> {
        v.as_array().unwrap().iter().map(word).collect()
    }
    fn read(m: &Space, start: u32, count: usize) -> Vec<u32> {
        (0..count).map(|i| m.r32(start + i as u32 * 4)).collect()
    }
    struct Tree<'a> {
        row: &'a Value,
        events: Vec<Value>,
    }
    impl Host for Tree<'_> {
        fn traverse(&mut self, m: &mut Space, world: u32, tree: u32) -> Result<u32, String> {
            let count = self.row["heap_before"].as_array().unwrap().len();
            self.events.push(json!({"this":world,"args":[tree],"globals":read(m,GLOBAL,COUNT),"heap":read(m,BASE,count)}));
            let mode = self.row["mode"].as_str().unwrap();
            m.w32(
                0x643580,
                if mode.starts_with("hit") {
                    0x3e800000
                } else {
                    0x3f800000
                },
            );
            if mode == "hit-body" {
                let body = word(&self.row["body"]);
                m.w32(0x6435a4, body);
                m.w32(body, 0x6435a4);
                m.w32(body + 4, 1);
            }
            Ok(u32::from(mode == "any-hit"))
        }
    }
    #[test]
    fn original_ray_setup_ownership_and_hit_coordinates_match() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = std::env::var_os("SLR_WORLD_RAY_VECTORS")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("re/out/boot/native-world-ray-original.json"));
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_ray_setup_ownership_and_hit_coordinates_match");
            assert!(
                std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none(),
                "required world-ray vectors missing"
            );
            return;
        };
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(
            data["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        assert_eq!(
            data["producer_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/oracle/native_world_ray_probe.py")).unwrap()
                )
            )
        );
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 50);
        for (index, row) in rows.iter().enumerate() {
            assert_eq!(word(&row["id"]), index as u32);
            let before = words(&row["heap_before"]);
            let after = words(&row["heap_after"]);
            assert_eq!(before.len(), after.len());
            let globals = words(&row["globals_before"]);
            assert_eq!(globals.len(), COUNT);
            assert_eq!(row["globals_after"].as_array().unwrap().len(), COUNT);
            let mut m = Space::new();
            for (i, v) in before.iter().enumerate() {
                m.w32(BASE + i as u32 * 4, *v);
            }
            for (i, v) in globals.iter().enumerate() {
                m.w32(GLOBAL + i as u32 * 4, *v);
            }
            m.set_heap_ptr(word(&row["heap_end"]));
            let mut limit = m.r32(word(&row["limit_address"]));
            let supplied = row["limit_supplied"].as_bool().unwrap();
            let mut host = Tree {
                row,
                events: Vec::new(),
            };
            let got = query(
                &mut m,
                word(&row["world"]),
                words(&row["ray"]).try_into().unwrap(),
                word(&row["selection"]),
                supplied.then_some(&mut limit),
                &mut host,
            )
            .unwrap();
            if supplied {
                m.w32(word(&row["limit_address"]), limit);
            }
            assert_eq!(got, word(&row["eax"]), "row {index} return");
            assert_eq!(
                host.events,
                *row["events"].as_array().unwrap(),
                "row {index} callbacks"
            );
            assert_eq!(read(&m, BASE, before.len()), after, "row {index} heap");
            assert_eq!(
                read(&m, GLOBAL, COUNT),
                words(&row["globals_after"]),
                "row {index} globals"
            );
        }
    }
}

#[cfg(test)]
mod genuine_tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;
    fn w(v: &Value) -> u32 {
        v.as_u64().unwrap().try_into().unwrap()
    }
    struct TreeBoundary;
    impl Host for TreeBoundary {
        fn traverse(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
            Err("nonempty49e710 must remain explicit".into())
        }
    }
    #[test]
    fn missing_nonempty_tree_implementation_is_an_error() {
        let mut m = Space::new();
        let selection = 0x20000100;
        m.w32(selection + 12, 0x20000200);
        let mut limit = 0x3f400000;
        let error = query(
            &mut m,
            0x20000000,
            [0, 0, 0, 0x3f800000, 0, 0],
            selection,
            Some(&mut limit),
            &mut TreeBoundary,
        )
        .unwrap_err();
        assert!(error.contains("49e710"));
        assert_eq!(limit, 0x3f400000);
    }
    struct Ray {
        events: Vec<Value>,
    }
    impl crate::native_cursor_pick::Host for Ray {
        fn ray(
            &mut self,
            m: &mut Space,
            ray: [u32; 6],
            selection: u32,
            limit: &mut u32,
        ) -> Result<u32, String> {
            let world = m.r32(0x617650);
            self.events
                .push(json!({"world":world,"ray":ray,"selection":selection,"limit_before":*limit}));
            query(m, world, ray, selection, Some(limit), &mut TreeBoundary)
        }
    }
    #[test]
    fn genuine_cursor_pick_and_world_ray_match_retained_original_state() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
        let path = std::env::var_os("SLR_CURSOR_PICK_GENUINE_VECTORS")
            .map(PathBuf::from)
            .unwrap_or_else(|| root.join("re/out/boot/native-cursor-pick-genuine-original.json"));
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: genuine_cursor_pick_and_world_ray_match_retained_original_state");
            assert!(
                std::env::var_os("SLR_REQUIRE_ORACLE_FIXTURES").is_none(),
                "required genuine cursor vectors missing"
            );
            return;
        };
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(
            data["provenance"]["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        let required = [
            "re/oracle/headless_snapshot.py",
            "re/oracle/native_cursor_pick_genuine_probe.py",
            "re/oracle/oracle.py",
            "re/oracle/snapshot_replay.py",
        ];
        let sources = data["sources"].as_object().unwrap();
        assert_eq!(
            sources.keys().map(String::as_str).collect::<Vec<_>>(),
            required
        );
        for source in required {
            assert_eq!(
                sources[source],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(source)).unwrap())
                )
            );
        }
        for (field, suffix) in [
            ("snapshot_metadata_sha256", "json"),
            ("snapshot_pages_sha256", "pages"),
        ] {
            let path = root.join(format!("re/out/snapshots/first-cursor-pick.{suffix}"));
            assert_eq!(
                data[field],
                format!("{:x}", Sha256::digest(std::fs::read(path).unwrap()))
            );
        }
        let mut m = Space::new();
        let parent = crate::native_stack::enter(&mut m, &[], None);
        let mut before = BTreeMap::new();
        for page in data["pages"].as_array().unwrap() {
            let address = w(&page["address"]);
            assert_eq!(address & 4095, 0);
            let words = page["words"].as_array().unwrap();
            assert_eq!(words.len(), 1024);
            for (i, word) in words.iter().enumerate() {
                let a = address + i as u32 * 4;
                assert!(before.insert(a, w(word)).is_none());
                m.w32(a, w(word));
            }
        }
        m.set_heap_ptr(w(&data["heap_ptr"]));
        let args: Vec<_> = data["args"].as_array().unwrap().iter().map(w).collect();
        crate::native_stack::set_sp(&mut m, w(&data["entry_sp"]) + 44);
        crate::native_stack::begin_call(&mut m, w(&data["return_pc"]), &args, None);
        m.begin_journal();
        let mut host = Ray { events: Vec::new() };
        let got =
            crate::native_cursor_pick::pick(&mut m, args.try_into().unwrap(), &mut host).unwrap();
        let written = m.end_journal();
        assert_eq!(got, w(&data["eax"]));
        assert_eq!(json!(host.events), data["queries"]);
        let expected: BTreeMap<_, _> = data["diff"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| (w(&r[0]), w(&r[2])))
            .collect();
        let addresses: BTreeSet<_> = expected
            .keys()
            .copied()
            .chain(written.iter().map(|r| r.0))
            .collect();
        assert_eq!(
            data["comparison_policy"]["retained_word_exclusions"],
            json!([])
        );
        assert_eq!(
            data["comparison_policy"]["native_stack_bookkeeping"],
            json!([0x7ffdf000u32, 0x7ffe0000u32])
        );
        for address in addresses {
            if (0x7ffdf000..0x7ffe0000).contains(&address)
                || (0x7fe00000..w(&data["entry_sp"]) + 4).contains(&address)
            {
                continue;
            }
            let want = expected
                .get(&address)
                .or_else(|| before.get(&address))
                .expect("unexpected write outside original observed pages");
            assert_eq!(m.r32(address), *want, "retained word {address:#x}");
        }
        crate::native_stack::leave(&mut m, parent);
    }
}

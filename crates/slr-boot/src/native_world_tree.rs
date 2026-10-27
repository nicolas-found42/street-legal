//! Original49e710 recursive resource tree and49deb0 body ray selection.
//! Resource loading remains a live collaborator; all supported shape queries
//! execute the ported arithmetic, never a supplied hit result in production.
use crate::native_ray_kernels::{mesh_triangle, segment_sphere};
use crate::native_resource_load::Access;
use crate::native_world_ray::segment_box;
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn value(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn store(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}
fn ray_lt(a: X87, b: X87) -> bool {
    a.fcom(b).0.c0
}
fn ray_le(a: X87, b: X87) -> bool {
    let c = a.fcom(b).0;
    c.c0 || c.c3
}
/// Original49deb0 sphere/box/mesh body ray branches.
pub fn body_query(m: &mut Space, _world: u32, entity: u32) -> Result<u32, String> {
    if m.r32(entity + 0x58) & 0x4020 != 0 {
        return Ok(0);
    }
    let body = m.r32(entity + 0x5c);
    if body == 0 {
        return Ok(0);
    }
    let definition = m.r32(body + 0x78);
    if definition == 0 || m.r32(definition + 8) == 0 {
        return Ok(0);
    }
    let frame = entity + m.r32(entity + 0x20c) * 0xd4;
    let fraction = segment_sphere(
        [0, 1, 2].map(|i| m.r32(0x643538 + i * 4)),
        [0, 1, 2].map(|i| m.r32(0x643550 + i * 4)),
        m.r32(0x64355c),
        [0, 1, 2].map(|i| m.r32(frame + 0x64 + i * 4)),
        m.r32(definition + 12),
    );
    let fraction_word = store(fraction);
    if ray_lt(fraction, value(0)) || ray_lt(value(m.r32(0x643580)), value(fraction_word)) {
        return Ok(0);
    }
    let mut child = m.r32(entity + 0x24);
    while child != 0 {
        if body_query(m, _world, child)? != 0 {
            return Ok(1);
        }
        child = m.r32(child + 0x28);
    }
    let definition = m.r32(m.r32(entity + 0x5c) + 0x78);
    match m.r32(definition + 8) {
        1 => {
            if ray_le(value(m.r32(0x643580)), value(fraction_word)) {
                return Ok(0);
            }
            m.w32(0x643580, fraction_word);
        }
        5 => {
            let rotation = entity + m.r32(entity + 0x20c) * 0xd4 + 0xa4;
            let local = |m: &Space, ray: u32| {
                let frame = entity + m.r32(entity + 0x20c) * 0xd4;
                let delta = [0, 1, 2].map(|i| {
                    value(store(value(m.r32(ray + i * 4)).sub(
                        value(m.r32(frame + 0x64 + i * 4)),
                        PC,
                        RC,
                    )))
                });
                [
                    [(0x20, 2), (0x10, 1), (0, 0)],
                    [(0x24, 2), (4, 0), (0x14, 1)],
                    [(0x28, 2), (8, 0), (0x18, 1)],
                ]
                .map(|terms| {
                    let [(a, i), (b, j), (c, k)] = terms;
                    store(
                        value(m.r32(rotation + a))
                            .mul(delta[i], PC, RC)
                            .add(value(m.r32(rotation + b)).mul(delta[j], PC, RC), PC, RC)
                            .add(value(m.r32(rotation + c)).mul(delta[k], PC, RC), PC, RC),
                    )
                })
            };
            let hit = segment_box(
                local(m, 0x643538),
                local(m, 0x643544),
                [0, 1, 2].map(|i| m.r32(definition + 0x9c + i * 4)),
            );
            if ray_le(hit, value(0)) || ray_le(value(m.r32(0x643580)), hit) {
                return Ok(0);
            }
            m.w32(0x643580, store(hit));
        }
        6 | 7 => {
            let rotation = entity + m.r32(entity + 0x20c) * 0xd4 + 0xa4;
            let local = |m: &Space, ray: u32| {
                let frame = entity + m.r32(entity + 0x20c) * 0xd4;
                let delta = [0, 1, 2].map(|i| {
                    value(store(value(m.r32(ray + i * 4)).sub(
                        value(m.r32(frame + 0x64 + i * 4)),
                        PC,
                        RC,
                    )))
                });
                [
                    [(0x20, 2), (0x10, 1), (0, 0)],
                    [(0x24, 2), (4, 0), (0x14, 1)],
                    [(0x28, 2), (8, 0), (0x18, 1)],
                ]
                .map(|terms| {
                    let [(a, i), (b, j), (c, k)] = terms;
                    store(
                        value(m.r32(rotation + a))
                            .mul(delta[i], PC, RC)
                            .add(value(m.r32(rotation + b)).mul(delta[j], PC, RC), PC, RC)
                            .add(value(m.r32(rotation + c)).mul(delta[k], PC, RC), PC, RC),
                    )
                })
            };
            let start = local(m, 0x643538);
            let end = local(m, 0x643544);
            let ray = [start[0], start[1], start[2], end[0], end[1], end[2]];
            let mut triangle = m.r32(definition + 0xa8) + 0x14;
            let mut index = 0i32;
            while index < m.r32(definition + 0xa4) as i32 {
                let vertices = [0, 1, 2].map(|i| {
                    let vertex = m.r32(definition + 0xa0) + m.r32(triangle - 16 + i * 4) * 12;
                    [0, 1, 2].map(|j| m.r32(vertex + j * 4))
                });
                let normal = [0, 1, 2].map(|i| m.r32(triangle + i * 4));
                let fraction = mesh_triangle(vertices, normal, ray, m.r32(0x643580));
                if !ray_le(fraction, value(0)) {
                    if m.r32(0x64357c) == 0 {
                        return Ok(1);
                    }
                    if ray_lt(fraction, value(m.r32(0x643580))) {
                        m.w32(0x643580, store(fraction));
                        crate::native_install::bind(m, 0x6435ac, m.r32(entity + 0x44));
                        crate::native_car_command::unlink_entity(m, 0x6435a4);
                        m.w32(0x6435a4, entity);
                        m.w32(0x6435a8, m.r32(entity));
                        m.w32(entity, 0x6435a4);
                        m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
                        for axis in 0..3 {
                            m.w32(0x643590 + axis * 4, m.r32(triangle + axis * 4));
                        }
                        m.w32(0x6435a0, 0);
                        m.w32(0x64359c, index as u32);
                    }
                }
                triangle += 32;
                index += 1;
            }
            return Ok(0);
        }
        _ => return Ok(0),
    }
    crate::native_install::bind(m, 0x6435ac, m.r32(entity + 0x44));
    crate::native_car_command::unlink_entity(m, 0x6435a4);
    m.w32(0x6435a4, entity);
    m.w32(0x6435a8, m.r32(entity));
    m.w32(entity, 0x6435a4);
    m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
    for address in [0x643590, 0x643594, 0x643598, 0x64359c, 0x6435a0] {
        m.w32(address, 0);
    }
    Ok(0)
}
pub trait Host {
    fn data(&mut self, m: &mut Space, resource: u32, args: Access) -> Result<u32, String>;
    fn triangle(
        &mut self,
        m: &mut Space,
        _world: u32,
        vertices: u32,
        triangle: u32,
    ) -> Result<X87, String> {
        Ok(crate::native_ray_kernels::game_resource_triangle(
            m, vertices, triangle, 0x643538,
        ))
    }
}
/// Original49e710 resource-tree walk; loading and triangle kernel are collaborators.
pub fn tree_query(
    m: &mut Space,
    world: u32,
    resource: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    #[cfg(test)]
    crate::native_ray_test_support::record_tree(m, resource);
    match m.r32(resource + 0x4c) {
        1 => {
            let payload = host.data(
                m,
                resource,
                Access {
                    flags: 0xa0000000,
                    quality: 0x3f800000,
                    priority: 0,
                    time: 0x41200000,
                },
            )?;
            if payload != 0 {
                let count = m.r32(payload + 0x18) as i32;
                for index in 0..count {
                    let triangle = m.r32(payload + 0x1c) + index as u32 * 32;
                    let result = host.triangle(m, world, m.r32(payload + 0x14), triangle)?;
                    if !ray_le(result, value(0)) {
                        if m.r32(0x64357c) == 0 {
                            return Ok(1);
                        }
                        let distance = value(0x3f800000).sub(result, PC, RC);
                        if !ray_le(value(m.r32(0x643580)), distance) {
                            m.w32(0x643580, store(distance));
                            for axis in 0..3 {
                                m.w32(0x643590 + axis * 4, m.r32(triangle + 0x10 + axis * 4));
                            }
                            crate::native_install::bind(m, 0x6435ac, resource);
                            crate::native_car_command::unlink_entity(m, 0x6435a4);
                            m.w32(0x64359c, index as u32);
                            m.w32(0x6435a0, m.r32(triangle));
                        }
                    }
                }
            }
            if m.r32(resource + 0x54) & 0x40 != 0
                || crate::native_install::empty(m, resource + 0x18)
            {
                return Ok(0);
            }
            let mut current = crate::native_install::first(m, resource + 0x18);
            while current != 0 {
                let following = crate::native_install::next(m, current);
                let kind = m.r32(current + 0x4c) as i32;
                if (1..=2).contains(&kind) {
                    let mode = m.r32(current + 0x80);
                    let center = [0, 1, 2].map(|i| m.r32(current + 0x84 + i * 4));
                    let half = [0, 1, 2].map(|i| m.r32(current + 0x90 + i * 4));
                    let bounds = [0, 1, 2, 3, 4, 5, 6].map(|i| m.r32(0x643560 + i * 4));
                    let classification = if bounds[0] == 2 || mode == 2 {
                        14
                    } else {
                        match mode {
                            0 => crate::native_spatial::classify_sphere(bounds, center, half[0]),
                            1 => crate::native_spatial::classify_box(bounds, center, half),
                            _ => 0,
                        }
                    };
                    let pass = if classification & 1 != 0 {
                        false
                    } else if mode == 2 {
                        true
                    } else {
                        let a = [0, 1, 2].map(|i| {
                            store(value(m.r32(0x643538 + i * 4)).sub(
                                value(center[i as usize]),
                                PC,
                                RC,
                            ))
                        });
                        let b = [0, 1, 2].map(|i| {
                            store(value(m.r32(0x643544 + i * 4)).sub(
                                value(center[i as usize]),
                                PC,
                                RC,
                            ))
                        });
                        let fraction = segment_box(a, b, half);
                        !ray_lt(fraction, value(0)) && !ray_le(value(m.r32(0x643580)), fraction)
                    };
                    if pass && tree_query(m, world, current, host)? != 0 && m.r32(0x64357c) == 0 {
                        return Ok(1);
                    }
                }
                current = following;
            }
        }
        2 => {
            let payload = host.data(
                m,
                resource,
                Access {
                    flags: 0xa0000000,
                    quality: 0x3f800000,
                    priority: 0,
                    time: 0x41200000,
                },
            )?;
            if payload != 0 {
                let entity = m.r32(payload + 0x84);
                if entity != 0 && m.r32(entity + 0x58) & 0x4020 == 0 {
                    let body = m.r32(entity + 0x5c);
                    if body != 0 {
                        let definition = m.r32(body + 0x78);
                        if definition != 0
                            && m.r32(definition + 8) != 0
                            && body_query(m, world, entity)? != 0
                        {
                            return Ok(1);
                        }
                    }
                }
            }
        }
        _ => {}
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_ray_test_support as proof;
    use serde_json::{json, Value};
    fn load(row: &Value) -> Space {
        let mut m = Space::new();
        for (key, base) in [("heap", 0x20000000), ("globals", 0x643538)] {
            for (i, w) in proof::words(&row["before"][key]).into_iter().enumerate() {
                m.w32(base + i as u32 * 4, w);
            }
        }
        m.set_heap_ptr(0x20000000 + row["before"]["heap"].as_array().unwrap().len() as u32 * 4);
        m
    }
    fn state(m: &Space, row: &Value) -> Value {
        json!({"heap":(0..row["before"]["heap"].as_array().unwrap().len()).map(|i|m.r32(0x20000000+i as u32*4)).collect::<Vec<_>>(),"globals":(0..33).map(|i|m.r32(0x643538+i*4)).collect::<Vec<_>>()})
    }
    #[test]
    fn original_body_queries_preserve_all_caller_heap_and_globals() {
        let Some(data) = proof::load(
            "SLR_NATIVE_BODY_RAY_VECTORS",
            "native-body-ray-original.json",
            "body",
            &["re/oracle/native_body_ray_probe.py", "re/oracle/oracle.py"],
        ) else {
            return;
        };
        let sha = "06befe514392f2f787b3b6956b63676fda1726ccd83df2270324c85dd4420d4b";
        proof::state_rows(&data, 65, false, sha);
        proof::negative_state_controls(&data, 65, false, sha);
        for row in data["rows"].as_array().unwrap() {
            let mut m = load(row);
            let got = body_query(
                &mut m,
                proof::word(&row["world"]),
                proof::word(&row["entity"]),
            )
            .unwrap();
            assert_eq!(got, proof::word(&row["result"]), "case {}", row["id"]);
            assert_eq!(state(&m, row), row["after"], "case {}", row["id"]);
        }
    }
    struct Calls<'a> {
        row: &'a Value,
        index: usize,
    }
    impl Host for Calls<'_> {
        fn data(&mut self, m: &mut Space, resource: u32, args: Access) -> Result<u32, String> {
            let event = &self.row["events"][self.index];
            self.index += 1;
            assert_eq!(event["name"], "data");
            assert_eq!(event["resource"], resource);
            assert_eq!(
                event["args"],
                json!([args.flags, args.quality, args.priority, args.time])
            );
            assert_eq!(event["state"], state(m, self.row));
            Ok(proof::word(&self.row["mapping"][resource.to_string()]))
        }
        fn triangle(
            &mut self,
            m: &mut Space,
            world: u32,
            vertices: u32,
            triangle: u32,
        ) -> Result<X87, String> {
            let event = &self.row["events"][self.index];
            self.index += 1;
            assert_eq!(event["name"], "triangle");
            assert_eq!(event["world"], world);
            assert_eq!(event["args"], json!([0, vertices, triangle, 0, 0x643538]));
            assert_eq!(event["state"], state(m, self.row));
            if self.row["original_triangles"] == true {
                Ok(crate::native_ray_kernels::game_resource_triangle(
                    m, vertices, triangle, 0x643538,
                ))
            } else {
                Ok(value(proof::word(
                    &self.row["responses"][triangle.to_string()],
                )))
            }
        }
    }
    #[test]
    fn original_tree_walk_and_composed_triangles_preserve_state_and_all_calls() {
        let Some(data) = proof::load(
            "SLR_NATIVE_WORLD_TREE_VECTORS",
            "native-world-tree-original.json",
            "world-tree",
            &[
                "re/oracle/native_world_tree_probe.py",
                "re/oracle/oracle.py",
            ],
        ) else {
            return;
        };
        let sha = "e3ac56f5804849789e6d8573add0a22b6bb3fde9d1fc50e51f19c8c8dd6a809b";
        proof::state_rows(&data, 46, true, sha);
        proof::negative_state_controls(&data, 46, true, sha);
        assert_eq!(
            data["rows"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|row| row["original_triangles"] == true)
                .count(),
            6
        );
        for row in data["rows"].as_array().unwrap() {
            let mut m = load(row);
            let mut host = Calls { row, index: 0 };
            let got = tree_query(
                &mut m,
                proof::word(&row["world"]),
                proof::word(&row["root"]),
                &mut host,
            )
            .unwrap();
            assert_eq!(got, proof::word(&row["result"]), "case {}", row["id"]);
            assert_eq!(
                host.index,
                row["events"].as_array().unwrap().len(),
                "all seam calls consumed"
            );
            assert_eq!(state(&m, row), row["after"], "case {}", row["id"]);
        }
    }
    #[test]
    fn loading_and_triangle_errors_are_not_reported_as_no_hit() {
        struct Fail {
            triangle: bool,
        }
        impl Host for Fail {
            fn data(&mut self, _: &mut Space, _: u32, _: Access) -> Result<u32, String> {
                if self.triangle {
                    Ok(0x22000000)
                } else {
                    Err("loading failed".into())
                }
            }
            fn triangle(&mut self, _: &mut Space, _: u32, _: u32, _: u32) -> Result<X87, String> {
                Err("triangle failed".into())
            }
        }
        let mut m = Space::new();
        m.w32(0x2000004c, 1);
        m.w32(0x22000018, 1);
        m.w32(0x2200001c, 0x23000000);
        assert_eq!(
            tree_query(&mut m, 0x610000, 0x20000000, &mut Fail { triangle: false }).unwrap_err(),
            "loading failed"
        );
        assert_eq!(
            tree_query(&mut m, 0x610000, 0x20000000, &mut Fail { triangle: true }).unwrap_err(),
            "triangle failed"
        );
    }
}

#[cfg(test)]
#[path = "native_world_tree_genuine_tests.rs"]
mod genuine_tests;

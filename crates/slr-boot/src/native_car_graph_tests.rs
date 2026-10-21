mod graph_tests {
    use super::*;
    use crate::native_part::PhysicalHost;
    struct DeclaredResources {
        calls: Vec<serde_json::Value>,
        render_calls: Vec<serde_json::Value>,
        attachment_calls: Vec<serde_json::Value>,
        owner_payload: u32,
    }
    impl PhysicalHost for DeclaredResources {
        fn create_physics(
            &mut self,
            m: &mut Space,
            owner: u32,
            entity: u32,
            name: &str,
        ) -> Result<u32, String> {
            let index = self.calls.len() as u32;
            let resource = 0x30020000 + index * 0x1000;
            let payload = resource + 0x100;
            m.w32(resource, 0x30000000);
            m.w32(resource + 0x4c, 1);
            m.w32(resource + 0x50, 0xffff0100 + index);
            m.w32(resource + 0x54, 0x200);
            m.w32(payload + 0x84, entity);
            m.w32(payload + 0x88, m.r32(entity));
            m.w32(entity, payload + 0x84);
            m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
            self.calls.push(
                serde_json::json!({"owner":owner,"entity":entity,"name":name,"resource":resource}),
            );
            Ok(resource)
        }
        fn wake(&mut self, m: &mut Space, entity: u32) -> Result<(), String> {
            crate::native_world::sleep(m, entity)
        }
        fn or_flags(&mut self, m: &mut Space, object: u32, flags: u32) -> Result<(), String> {
            m.w32(object + 0x70, m.r32(object + 0x70) | flags);
            Ok(())
        }
        fn or_resource_flags(
            &mut self,
            m: &mut Space,
            object: u32,
            flags: u32,
        ) -> Result<(), String> {
            let resource = m.r32(object + 0xc);
            m.w32(resource + 0x54, m.r32(resource + 0x54) | flags);
            Ok(())
        }
    }
    impl slr_physics::cfg::Host for DeclaredResources {
        fn call(&mut self, _m: &mut Space, _c: &slr_physics::cfg::ExtCall) -> u32 {
            panic!("unexpected cfg call")
        }
    }
    impl CarHost for DeclaredResources {
        fn refresh_render(
            &mut self,
            _m: &mut Space,
            _t: u32,
            _o: u32,
            lod: u32,
            wheels: bool,
        ) -> Result<(), String> {
            self.render_calls.push(serde_json::json!([wheels, lod]));
            Ok(())
        }
        fn or_native_payload_flags(
            &mut self,
            m: &mut Space,
            _r: u32,
            flags: u32,
        ) -> Result<(), String> {
            let p = self.owner_payload;
            m.w32(p + 0xc, m.r32(p + 0xc) | flags);
            Ok(())
        }
        fn physics_entity(&mut self, m: &mut Space, r: u32) -> Result<u32, String> {
            Ok(m.r32(r + 0x184))
        }
        fn activate(&mut self, m: &mut Space, entity: u32) -> Result<(), String> {
            crate::native_world::activate(m, entity)
        }
        fn attach_parameter(
            &mut self,
            m: &mut Space,
            object: u32,
            destination: u32,
            record: u32,
        ) -> Result<(), String> {
            self.attachment_calls.push(serde_json::json!({"destination":destination,"source":object+0x2c,"position":record,"rotation":record+12,"fov":m.r32(record+24),"internal":1}));
            Ok(())
        }
    }
    #[test]
    fn original_composed_car_physics_graph_matches() {
        check_composed_graph("native-car-graph-original.json", false);
    }
    #[test]
    fn original_full_car_graph_with_declared_render_callbacks_matches() {
        check_composed_graph("native-car-full-graph-original.json", true);
    }
    #[test]
    fn original_parameter_camera_attachment_callbacks_match() {
        check_composed_graph("native-car-attachments-original.json", false);
    }
    fn check_composed_graph(fixture: &str, full: bool) {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot")
            .join(fixture);
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_car_graph_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            for pair in row["input_words"].as_array().unwrap() {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            let mut host = DeclaredResources {
                calls: vec![],
                render_calls: vec![],
                attachment_calls: vec![],
                owner_payload: row["object"].as_u64().unwrap() as u32,
            };
            let k = row["k"].as_u64().unwrap() as u32;
            let object = row["object"].as_u64().unwrap() as u32;
            let stage = begin_runtime(
                &mut m,
                k,
                row["parent"].as_u64().unwrap() as u32,
                "0,0,0,0,0,0,7",
                object,
                slr_physics::cfg::PHYSICS_WORLD,
                &mut host,
            )
            .unwrap();
            if full {
                finish_runtime(
                    &mut m,
                    k,
                    object,
                    slr_physics::cfg::PHYSICS_WORLD,
                    &stage,
                    &mut host,
                )
                .unwrap();
            } else {
                finish_physics(
                    &mut m,
                    k,
                    object,
                    slr_physics::cfg::PHYSICS_WORLD,
                    &stage,
                    &mut host,
                )
                .unwrap();
                leave_runtime_stack(&mut m, &stage);
            }
            if full {
                assert_eq!(serde_json::json!(host.render_calls), row["render_calls"]);
            }
            assert_eq!(serde_json::json!(host.calls), row["calls"]);
            if row.get("attachment_calls").is_some() {
                assert_eq!(
                    serde_json::json!(host.attachment_calls),
                    row["attachment_calls"]
                );
            }
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            let mut ranges = vec![
                (
                    slr_physics::space::HEAP_BASE,
                    m.heap_ptr() - slr_physics::space::HEAP_BASE,
                ),
                (slr_physics::cfg::PHYSICS_WORLD, 0x210),
                (0x30010000, 0x100),
                (0x30001000, 0x4000),
            ];
            for call in host.calls {
                ranges.push((call["resource"].as_u64().unwrap() as u32, 0x200));
            }
            let mut actual = std::collections::BTreeMap::new();
            for (address, bytes) in ranges {
                for p in (address..address + bytes).step_by(4) {
                    let value = m.r32(p);
                    if value != 0 {
                        actual.insert(p, value);
                    }
                }
            }
            let expected = row["output_words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|p| (p[0].as_u64().unwrap() as u32, p[1].as_u64().unwrap() as u32))
                .collect::<std::collections::BTreeMap<_, _>>();
            for address in actual.keys().chain(expected.keys()) {
                assert_eq!(
                    actual.get(address).copied().unwrap_or(0),
                    expected.get(address).copied().unwrap_or(0),
                    "count{} address{address:#x}",
                    row["count"]
                );
            }
        }
    }
}

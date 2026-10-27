// Included in the bootstrap tests to reuse their complete leaf comparisons.
impl crate::native_vm_metadata::registration::resources::Environment for FullEnv<'_> {
    fn package_id(
        &mut self,
        m: &mut Space,
        manager: u32,
        package: u32,
        id: u32,
    ) -> Result<u32, String> {
        assert_eq!(manager, m.r32(0x618d48));
        let result = word(&self.base.events[self.base.index]["result"]);
        self.check(m, "package-id", json!([package, id]));
        Ok(result)
    }
    fn resolve(
        &mut self,
        m: &mut Space,
        manager: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<u32, String> {
        assert_eq!(manager, m.r32(0x618d48));
        let result = word(&self.base.events[self.base.index]["result"]);
        self.check(m, "resolve-resource", json!([id, kind, flags]));
        assert_ne!(result, 0);
        Ok(result)
    }
}
#[test]
fn source_root_retains_world_caller_through_jvm_bootstrap() {
    root_caller_profile(false, false, false, false);
}
#[test]
fn source_root_retains_world_caller_through_native_registry_return() {
    root_caller_profile(true, false, false, false);
}
#[test]
fn source_root_registers_all_353_natives_with_retained_startup_caller() {
    root_caller_profile(true, true, false, false);
}
#[test]
fn source_root_complete_caller_returns_with_explicit_world_and_config_collaborators() {
    root_caller_profile(true, true, true, false);
}
#[test]
fn source_root_complete_world_caller_returns_with_declared_body_entity_collaborators() {
    root_caller_profile(true, true, true, true);
}
fn root_caller_profile(registry: bool, remaining: bool, suffix: bool, world: bool) {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let Some(path) = crate::oracle_fixture::file(&root.join(if world {
        "re/out/boot/native-vm-root-world-original.json"
    } else if suffix {
        "re/out/boot/native-vm-root-suffix-original.json"
    } else if remaining {
        "re/out/boot/native-vm-root-catalogs-fullheap-original.json"
    } else if registry {
        "re/out/boot/native-vm-root-registry-original.json"
    } else {
        "re/out/boot/native-vm-root-bootstrap-original.json"
    })) else {
        return;
    };
    let _profile = SOURCE_PROFILE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let value: Json = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(value["schema"], 1);
    assert_eq!(value["cw"], 127);
    assert_eq!(value["unicorn"], "2.1.4");
    assert_eq!(value["world"], "636338");
    assert_eq!(
        value["endpoint"],
        if suffix {
            "30000028"
        } else if remaining {
            "427a0c"
        } else if registry {
            "427a02"
        } else {
            "4279fd"
        }
    );
    assert_eq!(
        value["exe_sha256"],
        digest(&root.join("reference/raw/StreetLegal_Redline.bak.exe"))
    );
    for (name, hash) in value["producer_sources"].as_object().unwrap() {
        assert_eq!(*hash, digest(&root.join("re/oracle").join(name)));
    }
    let rows = value["rows"].as_array().unwrap();
    assert_eq!(rows.len(), 8);
    for (profile, row) in rows.iter().enumerate() {
        assert_eq!(
            row["input"],
            json!({"fill":([0,0xa5][profile/4]),"seed":([606,20261023][profile/2%2]),"gap":([0,48][profile%2])})
        );
        let events = row["events"].as_array().unwrap();
        let mut snapshots = vec![&row["before"]];
        snapshots.extend(events.iter().map(|e| &e["state"]));
        snapshots.push(&row["after"]);
        let snapshots = states(&snapshots);
        let mut m = Space::new();
        let pages = row["input_pages"].as_array().unwrap();
        let bytes = decode(&pages.iter().map(|p| &p["data"]).collect::<Vec<_>>());
        for (page, data) in pages.iter().zip(bytes) {
            let base = word(&page["base"]);
            assert!((0x400000..0x7c0000).contains(&base) && base.is_multiple_of(4096));
            assert_eq!(data.len(), 4096);
            for (i, b) in data.into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
        }
        let before = &snapshots[0];
        for (base, data) in [(0x20000000, &before.heap), (0x7fefd000, &before.stack)]
            .into_iter()
            .chain(before.image.iter().map(|(p, b)| (*p, b)))
        {
            for (i, b) in data.iter().enumerate() {
                m.w8(base + i as u32, *b);
            }
        }
        m.set_heap_ptr(before.heap_ptr);
        let _parent = stack::enter_cdecl(&mut m, &[]);
        for (register, want) in [
            Reg::Eax,
            Reg::Ebx,
            Reg::Ecx,
            Reg::Edx,
            Reg::Esi,
            Reg::Edi,
            Reg::Ebp,
        ]
        .into_iter()
        .zip(before.registers)
        {
            set(&mut m, register, want);
        }
        stack::set_sp(&mut m, before.registers[7] + 4);
        stack::begin_call(&mut m, 0x30000028, &[], None);
        compare(&m, before, "root entry");
        let saved = &row["before"]["fpu"];
        let registers = std::array::from_fn(|i| {
            let mut bytes = [0; 10];
            bytes[..8].copy_from_slice(&saved["registers"][i][0].as_u64().unwrap().to_le_bytes());
            bytes[8..].copy_from_slice(&(word(&saved["registers"][i][1]) as u16).to_le_bytes());
            slr_x87::X87::from_bytes(bytes)
        });
        let fpu = crate::native_vm_fpu::State::from_saved(
            word(&saved["control"]) as u16,
            word(&saved["status"]) as u16,
            word(&saved["tags"]) as u16,
            registers,
        );
        let mut env = FullEnv {
            base: Env {
                events,
                states: &snapshots[1..1 + events.len()],
                index: 0,
            },
            enumerations: row["stock_enumerations"].as_array().unwrap(),
            next_enumeration: 0,
            platform: slr_boot::native_vm_platform::Files::new(root.join("stock"), 0x3000).unwrap(),
            logging: 0,
        };
        STORES.with(|stores| stores.borrow_mut().clear());
        set_test_store_observer(Some(observe));
        crate::native_vm_test_store::set(Some(observe));
        crate::native_vm_namespace::set_test_store_observer(Some(observe));
        crate::native_vm_class_context::set_test_store_observer(Some(observe));
        crate::native_vm_signature::set_test_store_observer(Some(observe));
        crate::native_vm_jvm::set_test_store_observer(Some(observe));
        crate::native_vm_lookup::set_test_store_observer(Some(observe));
        crate::native_vm_metadata::registration::set_test_store_observer(Some(observe));
        let mut startup = crate::native_vm_root::Startup::begin(&mut m, fpu, &mut env).unwrap();
        // The actual constructor has returned; the startup continuation owns
        // the following publication store and bootstrap CALL.
        env.check(&m, "constructor-return", json!([]));
        startup.bootstrap(&mut m, &mut env).unwrap();
        env.check(&m, "bootstrap-return", json!([]));
        assert_eq!(startup.pc(), 0x4279fd);
        let _retained_caller = startup.caller();
        if registry {
            startup.register(&mut m, &mut env).unwrap();
            env.check(&m, "registry-return", json!([]));
            assert_eq!(startup.pc(), 0x427a02);
            if remaining {
                startup.register_remaining(&mut m, &mut env).unwrap();
                env.check(&m, "secondary-return", json!([]));
                assert_eq!(startup.pc(), 0x427a0c);
                let registry = m.r32(startup.runtime().jvm() + 12);
                let mut nodes = std::collections::HashSet::new();
                let mut implementations = Vec::new();
                for bucket in 0..128 {
                    let mut node = registry + bucket * 12;
                    while m.r32(node + 4) != 0 {
                        assert!(nodes.insert(node), "registry node loop");
                        let record = m.r32(m.r32(node + 4));
                        implementations.push(m.r32(record + 8));
                        node = m.r32(node + 8);
                        if node == 0 {
                            break;
                        }
                    }
                }
                assert_eq!(implementations.len(), 353);
                assert!(implementations.contains(&0x47cc10));
            }
        }
        if suffix {
            startup.finish(&mut m, &mut env).unwrap();
            assert_eq!(startup.pc(), 0x30000028);
            assert!(!stack::has_pending_call(&m));
            assert!(startup.finish(&mut m, &mut env).is_err());
        }
        assert_eq!(m.r32(0x63641c), startup.runtime().jvm());
        assert_eq!(startup.runtime().files().len(), 2);
        compare(&m, snapshots.last().unwrap(), "root bootstrap endpoint");
        assert_eq!(env.base.index, events.len());
        set_test_store_observer(None);
        crate::native_vm_test_store::set(None);
        crate::native_vm_namespace::set_test_store_observer(None);
        crate::native_vm_class_context::set_test_store_observer(None);
        crate::native_vm_signature::set_test_store_observer(None);
        crate::native_vm_jvm::set_test_store_observer(None);
        crate::native_vm_lookup::set_test_store_observer(None);
        crate::native_vm_metadata::registration::set_test_store_observer(None);
        let expected_stores = row["stores"]
            .as_array()
            .unwrap()
            .iter()
            .map(|store| (word(&store[0]), word(&store[1]), word(&store[2])))
            .collect::<Vec<_>>();
        let actual_stores = STORES.with(|source| source.borrow().clone());
        assert_eq!(
            actual_stores.len(),
            expected_stores.len(),
            "ordered store count"
        );
        let difference = actual_stores
            .iter()
            .zip(&expected_stores)
            .enumerate()
            .find(|(_, (actual, expected))| actual != expected);
        assert!(
            difference.is_none(),
            "ordered startup stores {difference:x?}"
        );
        let fpu = startup.runtime().fpu();
        let expected = &row["after"]["fpu"];
        assert_eq!(u32::from(fpu.control()), word(&expected["control"]));
        assert_eq!(u32::from(fpu.status()), word(&expected["status"]));
        assert_eq!(u32::from(fpu.tags()), word(&expected["tags"]));
        for (value, expected) in fpu
            .registers()
            .iter()
            .zip(expected["registers"].as_array().unwrap())
        {
            let bytes = value.to_bytes();
            assert_eq!(
                u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                expected[0].as_u64().unwrap()
            );
            assert_eq!(
                u16::from_le_bytes(bytes[8..].try_into().unwrap()) as u32,
                word(&expected[1])
            );
        }
        if suffix {
            let jvm = m.r32(0x63641c);
            assert_eq!(startup.into_runtime().unwrap().jvm(), jvm);
        } else {
            assert!(startup.into_runtime().is_err());
        }
    }
}

impl crate::native_vm_root::ContinuationEnvironment for FullEnv<'_> {
    fn initialize_world(&mut self, m: &mut Space) -> Result<(), String> {
        if self.base.events[self.base.index]["kind"] == "world-body" {
            crate::native_vm_root::world::initialize(m, self)
        } else {
            self.root_domain(m, "initialize-world", 0, false)
        }
    }
    fn refresh_game_root(&mut self, m: &mut Space) -> Result<(), String> {
        assert_eq!(stack::argument(m, 0), 0x636460);
        self.root_domain(m, "refresh-game-root", 1, false)
    }
    fn create_config(
        &mut self,
        m: &mut Space,
        runtime: &mut crate::native_vm_live::Runtime,
    ) -> Result<(), String> {
        assert_eq!(m.r32(0x63641c), runtime.jvm());
        assert_eq!(stack::argument(m, 0), 0x636460);
        assert_eq!(stack::argument(m, 2), 0x63c55c);
        assert_eq!(stack::argument(m, 3), 0x60cb40);
        self.root_domain(m, "create-config", 4, true)
    }
    fn apply_settings(
        &mut self,
        m: &mut Space,
        runtime: &mut crate::native_vm_live::Runtime,
    ) -> Result<(), String> {
        assert_eq!(m.r32(0x63641c), runtime.jvm());
        assert_eq!(get(m, Reg::Ecx), 0x636338);
        self.root_domain(m, "apply-settings", 0, false)
    }
}
impl FullEnv<'_> {
    fn root_domain(
        &mut self,
        m: &mut Space,
        kind: &str,
        count: usize,
        cdecl: bool,
    ) -> Result<(), String> {
        assert!(stack::active(m) && stack::has_pending_call(m));
        let args = (0..count)
            .map(|i| stack::argument(m, i))
            .collect::<Vec<_>>();
        let result = word(&self.base.events[self.base.index]["result"]);
        self.check(m, kind, json!(args));
        let frame = if cdecl {
            stack::enter_cdecl(m, &args)
        } else {
            stack::enter(m, &args, None)
        };
        set(m, Reg::Eax, result);
        stack::leave(m, frame);
        Ok(())
    }
}

impl crate::native_vm_root::world::Environment for FullEnv<'_> {
    fn body(&mut self, m: &mut Space) -> Result<(), String> {
        assert_eq!(get(m, Reg::Ecx), m.r32(0x617650));
        for i in 0..6 {
            assert_eq!(stack::argument(m, i), 0);
        }
        self.root_domain(m, "world-body", 6, false)
    }
    fn entity(&mut self, m: &mut Space) -> Result<(), String> {
        assert_eq!(get(m, Reg::Ecx), m.r32(0x617650));
        self.root_domain(m, "world-entity", 3, false)
    }
}

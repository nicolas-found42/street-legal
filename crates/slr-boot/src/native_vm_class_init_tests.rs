use super::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
fn word(v: &Value) -> u32 {
    u32::try_from(v.as_u64().unwrap()).unwrap()
}
fn bytes(v: &Value) -> Vec<u8> {
    v.as_str()
        .unwrap()
        .as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|c| u8::from_str_radix(std::str::from_utf8(c).unwrap(), 16).unwrap())
        .collect()
}
const REGS: [(Reg, &str); 7] = [
    (Reg::Eax, "eax"),
    (Reg::Ebx, "ebx"),
    (Reg::Ecx, "ecx"),
    (Reg::Edx, "edx"),
    (Reg::Esi, "esi"),
    (Reg::Edi, "edi"),
    (Reg::Ebp, "ebp"),
];
const AREAS: [(&str, u32); 17] = [
    ("heap", 0x20000000),
    ("stack", 0x7fefc000),
    ("globals", 0x62ddf0),
    ("constants", 0x607540),
    ("timer", 0x76a280),
    ("format", 0x607500),
    ("instance_constants", 0x607900),
    ("signature_constants", 0x60c060),
    ("constructor_constants", 0x60c650),
    ("resource_globals", 0x618d48),
    ("callback_constants", 0x60c960),
    ("callback_names", 0x60cae0),
    ("keyboard_globals", 0x76f9b0),
    ("clock_scale", 0x5f10a4),
    ("resource_epoch", 0x6200a4),
    ("quality_kinds", 0x619dc0),
    ("callback_globals", 0x636418),
];
type Memory = std::collections::HashMap<&'static str, Vec<u8>>;
fn compare(m: &Space, v: &Value, before: &Memory) {
    for (key, base) in AREAS {
        let mut expected = v.get(key).map(bytes).unwrap_or_else(|| before[key].clone());
        if let Some(patches) = v["patches"][key].as_array() {
            for patch in patches {
                let offset = word(&patch[0]) as usize;
                assert_eq!(offset % 4096, 0);
                let data = bytes(&patch[1]);
                assert!(!data.is_empty() && data.len() <= 4096);
                expected[offset..offset + data.len()].copy_from_slice(&data);
            }
        }
        let diff = m.first_difference(base, &expected);
        assert!(
            diff.is_none(),
            "{key}: {diff:x?}, return PC {:#x}",
            m.r32(stack::sp(m))
        );
    }
    for (r, name) in REGS {
        assert_eq!(
            stack::reg(m, r),
            word(&v["registers"][name]),
            "{name}, return PC {:#x}",
            m.r32(stack::sp(m))
        );
    }
    assert_eq!(stack::sp(m), word(&v["registers"]["esp"]));
    assert_eq!(m.heap_ptr(), word(&v["heap_ptr"]));
}
struct Host<'a> {
    row: &'a Value,
    before: &'a Memory,
    event: usize,
    frame_fpu: Option<crate::native_vm_fpu::State>,
}
impl Host<'_> {
    fn leaf(
        &mut self,
        m: &mut Space,
        kind: &str,
        args: &[u32],
        old_bytes: u32,
    ) -> Result<u32, String> {
        let e = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(e["kind"], kind);
        assert_eq!(e["args"], serde_json::json!(args));
        compare(m, &e["state"], self.before);
        let p = match kind {
            "allocate" => m.alloc(args[0]),
            "reallocate" => {
                let p = m.alloc(args[1].max(1));
                if args[0] != 0 {
                    for i in 0..old_bytes.min(args[1]) {
                        m.w8(p + i, m.r8(args[0] + i));
                    }
                }
                p
            }
            "free" => 0,
            _ => unreachable!(),
        };
        assert_eq!(p, word(&e["result"]));
        Ok(p)
    }
}
impl crate::native_vm_lookup::Environment for Host<'_> {}
impl crate::native_vm_thread_run::Environment for Host<'_> {
    fn resolve_resource(
        &mut self,
        m: &mut Space,
        manager: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<u32, String> {
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], "resolve-resource");
        assert_eq!(event["args"], serde_json::json!([id, kind, flags]));
        assert_ne!(manager, 0);
        assert_eq!(stack::reg(m, Reg::Ecx), manager);
        compare(m, &event["state"], self.before);
        Ok(word(&event["result"]))
    }
    fn native(
        &mut self,
        m: &mut Space,
        fpu: &mut crate::native_vm_fpu::State,
        target: u32,
        info: u32,
        kind: crate::native_vm_abi::ReturnKind,
    ) -> Result<u32, String> {
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], "native-entry");
        assert_eq!(event["target"], target);
        assert_eq!(event["args"], serde_json::json!([info]));
        compare(m, &event["state"], self.before);
        let expected = initial_fpu(&serde_json::json!({"before": event["state"]}));
        assert_eq!(fpu.control(), expected.control());
        assert_eq!(fpu.status(), expected.status());
        assert_eq!(fpu.tags(), expected.tags());
        assert_eq!(
            fpu.registers().map(|v| v.to_bytes()),
            expected.registers().map(|v| v.to_bytes())
        );
        match target {
            0x47cc10 => {
                assert_eq!(kind, crate::native_vm_abi::ReturnKind::Integer);
                crate::native_keyboard::last_key(m, info, self)
            }
            0x47bfe0 => {
                assert_eq!(kind, crate::native_vm_abi::ReturnKind::Float);
                crate::native_vm_thread_run::current_time(m, info, fpu, self)
            }
            0x47cea0 => {
                assert_eq!(kind, crate::native_vm_abi::ReturnKind::Void);
                crate::native_vm_resource::new_native(m, info, self)
            }
            0x47cf80 => crate::native_vm_resource::set_id(m, info, self),
            _ => Err(format!(
                "source native {target:#x} continuation unavailable"
            )),
        }
    }
    fn resource(&mut self, m: &mut Space, package: u32, id: u32) -> Result<u32, String> {
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], "resource");
        assert_eq!(event["args"], serde_json::json!([package, id]));
        compare(m, &event["state"], self.before);
        Ok(word(&event["result"]))
    }
    fn query_frequency(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "frequency")
    }
    fn query_counter(&mut self, m: &mut Space, p: u32) -> Result<u32, String> {
        self.query(m, p, "counter")
    }
}
impl Host<'_> {
    fn query(&mut self, m: &mut Space, p: u32, kind: &str) -> Result<u32, String> {
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], kind);
        assert_eq!(event["args"], serde_json::json!([p]));
        compare(m, &event["state"], self.before);
        let value = self.row["platform"][kind].as_u64().unwrap();
        m.w32(p, value as u32);
        m.w32(p + 4, (value >> 32) as u32);
        Ok(word(&event["result"]))
    }
}
impl crate::native_vm_dispatch::Environment for Host<'_> {
    fn free(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        self.deallocate(m, p)
    }
}
impl Allocator for Host<'_> {
    fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
        self.leaf(m, "allocate", &[n], 0)
    }
    fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
        self.leaf(m, "reallocate", &[p, n], old)
    }
    fn deallocate(&mut self, m: &mut Space, p: u32) -> Result<(), String> {
        self.leaf(m, "free", &[p], 0)?;
        stack::set_reg(m, Reg::Eax, 0);
        Ok(())
    }
}
#[derive(Clone, Copy)]
enum Profile {
    Dependencies,
    Owner,
    Complete,
    Cold,
    Fields,
    Initializers,
    StockInput,
    Constructor,
    ArrayPrefix,
    ArrayClass,
    ArrayPrimitive,
    ArrayObject,
    PrimitiveArrayObject,
    CachedArrayClass,
    InstanceEmpty,
    InstanceFields,
    InstanceInitializers,
    InstanceDefault,
    InstanceNew,
    FloatLiterals,
    ResourceLiteral,
    ResourceNullPackage,
    ResourceWrongKind,
    ResourceBadIndex,
    CallConstructor,
    ConstructorArgument,
    StockVector,
    StockResource,
    StockResourceBound,
    StockOsd,
    ParentCallback,
}
#[test]
fn original_class_dependency_prefix_matches_complete_memory_and_registers() {
    run(Profile::Dependencies)
}
#[test]
fn original_empty_static_owner_matches_complete_memory_and_registers() {
    run(Profile::Owner)
}
#[test]
fn original_empty_class_initialization_returns_with_complete_memory_and_registers() {
    run(Profile::Complete)
}
#[test]
fn original_recursive_empty_class_initialization_matches_complete_memory_and_registers() {
    run(Profile::Cold)
}
#[test]
fn original_static_fields_and_thread_lifecycle_match_complete_memory_and_registers() {
    run(Profile::Fields)
}
#[test]
fn original_static_initializer_scripts_match_complete_memory_and_registers() {
    run(Profile::Initializers)
}
#[test]
fn original_stock_input_class_initializers_match_complete_memory_and_registers() {
    run(Profile::StockInput)
}
#[test]
fn original_class_constructor_matches_complete_memory_and_registers() {
    run(Profile::Constructor)
}
#[test]
fn original_array_metadata_prefix_matches_complete_memory_and_registers() {
    run(Profile::ArrayPrefix)
}
#[test]
fn original_array_class_construction_matches_complete_memory_and_registers() {
    run(Profile::ArrayClass)
}
#[test]
fn original_primitive_array_class_construction_matches_complete_memory_and_registers() {
    run(Profile::ArrayPrimitive)
}
#[test]
fn original_array_instance_allocation_matches_complete_memory_and_registers() {
    run(Profile::ArrayObject)
}
#[test]
fn original_primitive_array_instance_allocation_matches_complete_memory_and_registers() {
    run(Profile::PrimitiveArrayObject)
}
#[test]
fn original_cached_array_class_return_matches_complete_memory_and_registers() {
    run(Profile::CachedArrayClass)
}
#[test]
fn original_ordinary_instance_empty_matches_complete_memory_and_registers() {
    run(Profile::InstanceEmpty)
}
#[test]
fn original_ordinary_instance_fields_match_complete_memory_and_registers() {
    run(Profile::InstanceFields)
}
#[test]
fn original_ordinary_instance_initializers_match_complete_memory_registers_and_fpu() {
    run(Profile::InstanceInitializers)
}
#[test]
fn original_default_instance_initializers_match_complete_memory_registers_and_fpu() {
    run(Profile::InstanceDefault)
}
#[test]
fn original_instance_initializer_new_object_matches_complete_memory_registers_and_fpu() {
    run(Profile::InstanceNew)
}
#[test]
fn original_float_literal_initializer_matches_complete_memory_registers_and_fpu() {
    run(Profile::FloatLiterals)
}
#[test]
fn original_resource_literal_initializer_matches_complete_memory_registers_and_fpu() {
    run(Profile::ResourceLiteral)
}
#[test]
fn original_null_package_resource_literal_matches_complete_memory_registers_and_fpu() {
    run(Profile::ResourceNullPackage)
}
#[test]
fn original_wrong_kind_resource_literal_matches_complete_memory_registers_and_fpu() {
    run(Profile::ResourceWrongKind)
}
#[test]
fn original_bad_index_resource_literal_matches_complete_memory_registers_and_fpu() {
    run(Profile::ResourceBadIndex)
}
#[test]
fn original_instance_initializer_script_constructor_matches_complete_memory_registers_and_fpu() {
    run(Profile::CallConstructor)
}
#[test]
fn original_instance_constructor_argument_matches_complete_memory_registers_and_fpu() {
    run(Profile::ConstructorArgument)
}
#[test]
fn original_stock_vector_constructor_matches_complete_memory_registers_and_fpu() {
    run(Profile::StockVector)
}
#[test]
fn original_stock_resource_constructor_matches_complete_memory_registers_and_fpu() {
    run(Profile::StockResource)
}
#[test]
fn original_stock_bound_resource_constructor_matches_complete_memory_registers_and_fpu() {
    run(Profile::StockResourceBound)
}
#[test]
fn original_stock_osd_instance_matches_complete_memory_registers_and_fpu() {
    run(Profile::StockOsd)
}
#[test]
fn original_cold_osd_parent_callback_produces_and_returns_through_thread_run() {
    run(Profile::ParentCallback)
}
fn initial_fpu(row: &Value) -> crate::native_vm_fpu::State {
    let before = &row["before"]["fpu"];
    let registers = std::array::from_fn(|i| {
        let mut bytes = [0; 10];
        bytes[..8].copy_from_slice(&before["registers"][i][0].as_u64().unwrap().to_le_bytes());
        bytes[8..].copy_from_slice(&(word(&before["registers"][i][1]) as u16).to_le_bytes());
        slr_x87::X87::from_bytes(bytes)
    });
    crate::native_vm_fpu::State::from_saved(
        word(&before["control"]) as u16,
        word(&before["status"]) as u16,
        word(&before["tags"]) as u16,
        registers,
    )
}
fn run(profile: Profile) {
    let instance = matches!(
        profile,
        Profile::CallConstructor
            | Profile::ConstructorArgument
            | Profile::StockVector
            | Profile::StockResource
            | Profile::StockResourceBound
            | Profile::StockOsd
            | Profile::ParentCallback
            | Profile::InstanceNew
            | Profile::InstanceEmpty
            | Profile::InstanceFields
            | Profile::InstanceInitializers
            | Profile::InstanceDefault
    );
    let array_object = matches!(
        profile,
        Profile::ArrayObject | Profile::PrimitiveArrayObject
    );
    let primitive_object = matches!(profile, Profile::PrimitiveArrayObject);
    let array_class = matches!(
        profile,
        Profile::ArrayClass | Profile::ArrayPrimitive | Profile::CachedArrayClass
    );
    let cached_array = matches!(profile, Profile::CachedArrayClass);
    let primitive_array = matches!(profile, Profile::ArrayPrimitive);
    let (static_owner, complete, cold, fields, initializers, stock, constructor, array_prefix) =
        match profile {
            Profile::CallConstructor
            | Profile::ConstructorArgument
            | Profile::StockVector
            | Profile::StockResource
            | Profile::StockResourceBound
            | Profile::InstanceNew
            | Profile::InstanceDefault
            | Profile::InstanceInitializers => (true, true, true, true, true, false, false, false),
            Profile::InstanceEmpty => (true, true, true, false, false, false, false, false),
            Profile::InstanceFields => (true, true, true, true, false, false, false, false),
            Profile::ResourceLiteral
            | Profile::ResourceNullPackage
            | Profile::ResourceWrongKind
            | Profile::ResourceBadIndex
            | Profile::FloatLiterals => (true, true, false, true, true, false, false, false),
            Profile::Dependencies => (false, false, false, false, false, false, false, false),
            Profile::Owner => (true, false, false, false, false, false, false, false),
            Profile::Complete => (true, true, false, false, false, false, false, false),
            Profile::Cold => (true, true, true, false, false, false, false, false),
            Profile::Fields => (true, true, false, true, false, false, false, false),
            Profile::Initializers => (true, true, false, true, true, false, false, false),
            Profile::StockOsd | Profile::ParentCallback => {
                (true, true, true, true, true, true, false, false)
            }
            Profile::StockInput => (true, true, true, true, true, true, false, false),
            Profile::Constructor => (false, true, false, false, false, false, true, false),
            Profile::ArrayPrefix => (true, false, false, true, false, true, false, true),
            Profile::ArrayClass
            | Profile::ArrayPrimitive
            | Profile::ArrayObject
            | Profile::PrimitiveArrayObject
            | Profile::CachedArrayClass => (true, true, false, true, false, true, false, true),
        };
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let path = root.join(if matches!(profile, Profile::ParentCallback) {
        "re/out/boot/native-vm-parent-callback-original.json"
    } else if matches!(profile, Profile::StockOsd) {
        "re/out/boot/native-vm-instance-stock-osd-original.json"
    } else if matches!(profile, Profile::StockResourceBound) {
        "re/out/boot/native-vm-instance-stock-resource-bound-original.json"
    } else if matches!(profile, Profile::StockResource) {
        "re/out/boot/native-vm-instance-stock-resource-original.json"
    } else if matches!(profile, Profile::StockVector) {
        "re/out/boot/native-vm-instance-stock-vector-original.json"
    } else if matches!(profile, Profile::ConstructorArgument) {
        "re/out/boot/native-vm-instance-constructor-argument-original.json"
    } else if matches!(profile, Profile::CallConstructor) {
        "re/out/boot/native-vm-instance-script-constructor-original.json"
    } else if matches!(profile, Profile::ResourceLiteral) {
        "re/out/boot/native-vm-class-resource-literal-original.json"
    } else if matches!(profile, Profile::ResourceNullPackage) {
        "re/out/boot/native-vm-class-resource-null-original.json"
    } else if matches!(profile, Profile::ResourceWrongKind) {
        "re/out/boot/native-vm-class-resource-wrong-original.json"
    } else if matches!(profile, Profile::ResourceBadIndex) {
        "re/out/boot/native-vm-class-resource-index-original.json"
    } else if matches!(profile, Profile::FloatLiterals) {
        "re/out/boot/native-vm-class-float-literals-original.json"
    } else if matches!(profile, Profile::InstanceNew) {
        "re/out/boot/native-vm-instance-new-original.json"
    } else if matches!(profile, Profile::InstanceDefault) {
        "re/out/boot/native-vm-instance-default-original.json"
    } else if matches!(profile, Profile::InstanceInitializers) {
        "re/out/boot/native-vm-instance-initializers-original.json"
    } else if matches!(profile, Profile::InstanceEmpty) {
        "re/out/boot/native-vm-instance-empty-original.json"
    } else if matches!(profile, Profile::InstanceFields) {
        "re/out/boot/native-vm-instance-fields-original.json"
    } else if cached_array {
        "re/out/boot/native-vm-array-cached-class-original.json"
    } else if primitive_object {
        "re/out/boot/native-vm-array-primitive-object-original.json"
    } else if array_object {
        "re/out/boot/native-vm-array-object-original.json"
    } else if primitive_array {
        "re/out/boot/native-vm-array-primitive-class-original.json"
    } else if array_class {
        "re/out/boot/native-vm-array-class-original.json"
    } else if array_prefix {
        "re/out/boot/native-vm-array-class-prefix-original.json"
    } else if constructor {
        "re/out/boot/native-vm-class-constructor-original.json"
    } else if stock {
        "re/out/boot/native-vm-class-input-original.json"
    } else if initializers {
        "re/out/boot/native-vm-class-initializers-original.json"
    } else if fields {
        "re/out/boot/native-vm-class-init-fields-original.json"
    } else if cold {
        "re/out/boot/native-vm-class-init-cold-original.json"
    } else if complete {
        "re/out/boot/native-vm-class-init-complete-original.json"
    } else if static_owner {
        "re/out/boot/native-vm-class-static-owner-original.json"
    } else {
        "re/out/boot/native-vm-class-init-original.json"
    });
    let Some(path) = crate::oracle_fixture::file(&path) else {
        return;
    };
    let v: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    assert_eq!(v["schema"], 3);
    assert_eq!(
        v["exe_sha256"],
        "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
    );
    for name in [
        "native_vm_class_init_probe.py",
        "oracle.py",
        "thread_vectors.py",
        "class_init_fpu_referee.c",
    ] {
        let data = std::fs::read(root.join("re/oracle").join(name)).unwrap();
        assert_eq!(
            v["producer_sources"][name],
            format!("{:x}", Sha256::digest(data))
        );
    }
    assert_eq!(
        v["rows"].as_array().unwrap().len(),
        if matches!(
            profile,
            Profile::StockResource
                | Profile::StockResourceBound
                | Profile::StockOsd
                | Profile::ParentCallback
        ) {
            2
        } else if instance {
            12
        } else if constructor {
            8
        } else if fields {
            2
        } else {
            12
        }
    );
    for row in v["rows"].as_array().unwrap() {
        let memory: Memory = AREAS
            .into_iter()
            .map(|(key, _)| (key, bytes(&row["before"][key])))
            .collect();
        let mut m = Space::new();
        if stock
            || matches!(
                profile,
                Profile::StockVector
                    | Profile::StockResource
                    | Profile::StockResourceBound
                    | Profile::StockOsd
                    | Profile::ParentCallback
            )
        {
            let exe =
                std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap();
            assert_eq!(
                format!("{:x}", Sha256::digest(&exe)),
                v["exe_sha256"].as_str().unwrap()
            );
            crate::native_image::ImageData::parse(&exe)
                .unwrap()
                .initialize(&mut m);
            let files = row["stock"].as_array().unwrap();
            assert_eq!(
                files.len(),
                if matches!(profile, Profile::StockVector) {
                    2
                } else if matches!(
                    profile,
                    Profile::StockResource
                        | Profile::StockResourceBound
                        | Profile::StockOsd
                        | Profile::ParentCallback
                ) {
                    if matches!(profile, Profile::StockOsd | Profile::ParentCallback) {
                        if matches!(profile, Profile::ParentCallback) {
                            13
                        } else {
                            9
                        }
                    } else {
                        4
                    }
                } else {
                    3
                }
            );
            for file in files {
                let data =
                    std::fs::read(root.join("stock").join(file["path"].as_str().unwrap())).unwrap();
                assert_eq!(
                    format!("{:x}", Sha256::digest(data)),
                    file["sha256"].as_str().unwrap()
                );
            }
        }
        let parent = stack::enter(&mut m, &[], None);
        for (key, base) in AREAS {
            for (i, b) in bytes(&row["before"][key]).into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
        }
        m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
        for (reg, name) in REGS {
            stack::set_reg(&mut m, reg, word(&row["before"]["registers"][name]));
        }
        let args = if constructor {
            vec![word(&row["jvm"]), word(&row["module"])]
        } else if instance && !matches!(profile, Profile::InstanceDefault) {
            vec![0]
        } else if array_prefix {
            vec![word(&row["entry_name"])]
        } else {
            vec![]
        };
        stack::set_sp(
            &mut m,
            word(&row["before"]["registers"]["esp"]) + 4 + args.len() as u32 * 4,
        );
        stack::begin_call(
            &mut m,
            word(&row["return_pc"]),
            &args,
            Some(word(
                &row[if array_object {
                    "entry_this"
                } else if array_prefix {
                    "jvm"
                } else {
                    "cl"
                }],
            )),
        );
        compare(&m, &row["before"], &memory);
        let mut host = Host {
            row,
            before: &memory,
            event: 0,
            frame_fpu: None,
        };
        let mut object_fpu = initial_fpu(row);
        if instance {
            m.w32(0x5f0910, 1000f32.to_bits());
            if matches!(profile, Profile::InstanceDefault) {
                crate::native_vm_array_object::create_instance(
                    &mut m,
                    word(&row["cl"]),
                    &mut object_fpu,
                    &mut host,
                )
                .unwrap();
            } else {
                crate::native_vm_array_object::create(
                    &mut m,
                    word(&row["cl"]),
                    0,
                    &mut object_fpu,
                    &mut host,
                )
                .unwrap();
            }
            if matches!(profile, Profile::ParentCallback) {
                let event = &row["events"][host.event];
                host.event += 1;
                assert_eq!(event["kind"], "constructed-osd");
                compare(&m, &event["state"], &memory);
                let envelope = word(&row["parent_envelope"]);
                m.w32(envelope + 0x50, stack::reg(&m, Reg::Eax));
                let context = word(&row["parent_context"]);
                stack::set_reg(&mut m, Reg::Ecx, context);
                stack::push(&mut m, 0x3c888889);
                stack::begin_call(&mut m, word(&row["parent_return"]), &[], Some(context));
                let event = &row["events"][host.event];
                host.event += 1;
                assert_eq!(event["kind"], "parent-entry");
                compare(&m, &event["state"], &memory);
                host.frame_fpu = Some(object_fpu);
                assert!(
                    crate::native_frame::animate(&mut m, context, 0x3c888889, &mut host)
                        .unwrap()
                        .is_empty()
                );
                object_fpu = host.frame_fpu.take().unwrap();
            }
        } else if array_object {
            crate::native_vm_array_object::create(
                &mut m,
                word(&row["entry_this"]),
                word(&row["entry_name"]),
                &mut object_fpu,
                &mut host,
            )
            .unwrap();
        } else if array_class {
            let class = crate::native_vm_array_class::get(&mut m, &mut host).unwrap();
            assert_eq!(class, stack::reg(&m, Reg::Eax));
        } else if array_prefix {
            let prefix = crate::native_vm_array_class::begin(&mut m, &mut host).unwrap();
            assert_eq!(prefix.pc, 0x410bdd);
            assert_eq!(prefix.jvm, word(&row["jvm"]));
            assert_eq!(prefix.module, stack::reg(&m, Reg::Esi));
            compare(&m, &row["after"], &memory);
            stack::leave(&mut m, prefix.frame);
        } else if constructor {
            crate::native_vm_class_context::construct(
                &mut m,
                word(&row["cl"]),
                word(&row["jvm"]),
                word(&row["module"]),
                &mut host,
            )
            .unwrap();
        } else if initializers {
            m.w32(0x5f0910, 1000f32.to_bits());
            let before = &row["before"]["fpu"];
            let registers = std::array::from_fn(|i| {
                let mut bytes = [0; 10];
                bytes[..8]
                    .copy_from_slice(&before["registers"][i][0].as_u64().unwrap().to_le_bytes());
                bytes[8..]
                    .copy_from_slice(&(word(&before["registers"][i][1]) as u16).to_le_bytes());
                slr_x87::X87::from_bytes(bytes)
            });
            let mut fpu = crate::native_vm_fpu::State::from_saved(
                word(&before["control"]) as u16,
                word(&before["status"]) as u16,
                word(&before["tags"]) as u16,
                registers,
            );
            initialize_with_scripts(&mut m, word(&row["cl"]), &mut fpu, &mut host).unwrap();
            assert_eq!(row["qualified"], true);
            let after = &row["after"]["fpu"];
            assert_eq!(fpu.control(), word(&after["control"]) as u16);
            assert_eq!(fpu.status(), word(&after["status"]) as u16);
            assert_eq!(fpu.tags(), word(&after["tags"]) as u16);
            for (i, register) in fpu.registers().iter().enumerate() {
                let bytes = register.to_bytes();
                assert_eq!(
                    u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                    after["registers"][i][0].as_u64().unwrap(),
                    "physical x87 register {i}"
                );
                assert_eq!(
                    u16::from_le_bytes(bytes[8..].try_into().unwrap()),
                    word(&after["registers"][i][1]) as u16
                );
            }
            let errata = row["errata"].as_array().unwrap();
            assert_eq!(
                errata.len(),
                usize::from(!stock && word(&row["seed"]) == 20261025)
            );
            for erratum in errata {
                assert_eq!(word(&erratum["pc"]), 0x417e35);
                assert_eq!(
                    word(&erratum["native_status"]) ^ word(&erratum["unicorn_status"]),
                    0x20
                );
                assert_eq!(erratum["conversion"]["value"].as_i64().unwrap(), -20261025);
                assert_eq!(word(&erratum["conversion"]["status"]), 0x20);
            }
        } else if cold || fields {
            initialize_empty(&mut m, word(&row["cl"]), &mut host).unwrap();
        } else {
            let mut init = begin(&mut m, &mut host).unwrap();
            if static_owner {
                init.install_empty_owner(&mut m, &mut host).unwrap();
            }
            if complete {
                init.finish_empty_owner(&mut m, &mut host).unwrap();
            }
            assert_eq!(
                init.pc,
                if complete {
                    word(&row["return_pc"])
                } else if static_owner {
                    0x404377
                } else {
                    0x403fd9
                }
            );
            assert_eq!(init.class, word(&row["cl"]));
            if !complete {
                compare(&m, &row["after"], &memory);
                stack::leave(&mut m, init.frame);
            }
        }
        if instance {
            let after = &row["after"]["fpu"];
            assert_eq!(object_fpu.control(), word(&after["control"]) as u16);
            assert_eq!(object_fpu.status(), word(&after["status"]) as u16);
            assert_eq!(object_fpu.tags(), word(&after["tags"]) as u16);
            for (i, register) in object_fpu.registers().iter().enumerate() {
                let bytes = register.to_bytes();
                assert_eq!(
                    u64::from_le_bytes(bytes[..8].try_into().unwrap()),
                    after["registers"][i][0].as_u64().unwrap(),
                    "physical x87 {i}"
                );
                assert_eq!(
                    u16::from_le_bytes(bytes[8..].try_into().unwrap()),
                    word(&after["registers"][i][1]) as u16
                );
            }
            assert_eq!(
                row["errata"].as_array().unwrap().len(),
                usize::from(
                    initializers
                        && !matches!(profile, Profile::StockOsd | Profile::ParentCallback)
                        && word(&row["seed"]) == 20261025
                )
            );
        }
        assert_eq!(host.event, row["events"].as_array().unwrap().len());
        if complete {
            compare(&m, &row["after"], &memory);
        }
        stack::leave(&mut m, parent);
    }
}

impl crate::native_keyboard::Environment for Host<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        leaf: crate::native_keyboard::Leaf,
        _target: u32,
        args: &[u32],
    ) -> Result<u32, String> {
        let event = &self.row["events"][self.event];
        self.event += 1;
        let kind = match leaf {
            crate::native_keyboard::Leaf::Acquire => "keyboard-acquire",
            crate::native_keyboard::Leaf::GetDeviceData => "keyboard-data",
            _ => panic!("outside quiescent device contract"),
        };
        assert_eq!(event["kind"], kind);
        assert_eq!(event["args"], serde_json::json!(args));
        compare(m, &event["state"], self.before);
        if leaf == crate::native_keyboard::Leaf::GetDeviceData {
            m.w32(args[3], 0);
        }
        Ok(0)
    }
}
impl crate::native_frame::Host for Host<'_> {
    fn minimum(&mut self, _m: &mut Space, _resource: u32) -> Result<(), String> {
        Err("unexpected minimum".into())
    }
    fn prepare(&mut self, _m: &mut Space, _resource: u32, _flags: u32) -> Result<u32, String> {
        Err("source prepare entry required".into())
    }
    fn payload(&mut self, _m: &mut Space, resource: u32) -> Result<u32, String> {
        Ok(resource + 0x80)
    }
    fn native_drive(
        &mut self,
        _m: &mut Space,
        _class: u32,
        _body: u32,
        _dt: u32,
    ) -> Result<(), String> {
        Err("unexpected native drive".into())
    }
    fn script_drive(&mut self, _m: &mut Space, _envelope: u32) -> Result<(), String> {
        Err("unexpected script drive".into())
    }
}
impl crate::native_frame::CallbackHost for Host<'_> {
    fn native_callback(
        &mut self,
        _m: &mut Space,
        _class: u32,
        _body: u32,
        _dt: u32,
        _kind: crate::native_frame::Callback,
    ) -> Result<(), String> {
        Err("unexpected native callback".into())
    }
    fn script_callback(
        &mut self,
        _m: &mut Space,
        _envelope: u32,
        _dt: u32,
        _kind: crate::native_frame::Callback,
    ) -> Result<(), String> {
        Err("pending source callback required".into())
    }
    fn prepare_callback(
        &mut self,
        m: &mut Space,
        resource: u32,
        flags: u32,
    ) -> Result<u32, String> {
        let mut fpu = self.frame_fpu.take().ok_or("missing parent FPU owner")?;
        let result =
            crate::native_vm_callback::prepare_loaded(m, resource, flags, &mut fpu).map(|()| 0);
        self.frame_fpu = Some(fpu);
        result
    }
    fn script_callback_entry(
        &mut self,
        m: &mut Space,
        _envelope: u32,
        _dt: u32,
        kind: crate::native_frame::Callback,
    ) -> Result<(), String> {
        assert_eq!(kind, crate::native_frame::Callback::Animate);
        assert!(stack::has_pending_call(m));
        assert_eq!(m.r32(stack::sp(m)), 0x427111);
        let mut fpu = self.frame_fpu.take().ok_or("missing callback FPU owner")?;
        let mut callback = crate::native_vm_callback::Context::begin(m, &mut fpu, self)?;
        while !callback.advance(m, &mut fpu, self)? {
            let entry = callback.native_entry(m)?;
            assert_eq!(m.r32(entry.info() + 4), callback.thread());
            callback.resume_native(m, &mut fpu, self)?;
        }
        assert!(callback.native_entry(m).is_err());
        callback.finish_setup(m, &mut fpu)?;
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], "callback-setup-return");
        compare(m, &event["state"], self.before);
        callback.finish_wrapper(m)?;
        let event = &self.row["events"][self.event];
        self.event += 1;
        assert_eq!(event["kind"], "callback-wrapper-return");
        compare(m, &event["state"], self.before);
        self.frame_fpu = Some(fpu);
        Ok(())
    }
}

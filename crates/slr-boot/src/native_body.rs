//! Configured native car/part state. VM wrappers address the original raw
//! template/runtime layouts; engine containers remain the ownership identity.
use slr_physics::space::Space;
use slr_vm::host::VmHost;
use std::{cell::RefCell, collections::HashMap};

pub struct NativeBody {
    pub memory: Space,
    /// Raw K address (configured native class, not its resource ID).
    pub template: u32,
    /// Raw runtime engine object address.
    pub body: u32,
    pub container: u32,
    pub resource_bindings: HashMap<u32, crate::factory::CfgBinding>,
}
impl NativeBody {
    pub fn acquire(container: u32, template: u32) -> Result<Self, String> {
        let memory = MEMORY
            .with(|memory| memory.borrow_mut().take())
            .ok_or("NativeBodyBoundary: shared memory is already borrowed")?;
        let mut memory = memory;
        synchronize_flags(&mut memory, true);
        synchronize_generation(&mut memory);
        Ok(Self {
            memory,
            template,
            body: 0,
            container,
            resource_bindings: HashMap::new(),
        })
    }
    pub fn bind_resource(
        &mut self,
        slot: u32,
        id: u32,
        kind: i32,
        flag: u32,
    ) -> Result<(), String> {
        let binding =
            crate::factory::bind_cfg(self.memory.r32(self.template + 0xc), id, kind, flag)?;
        if let Some(old) = self.resource_bindings.insert(slot, binding) {
            crate::factory::release_cfg(old);
        }
        let raw = raw_resource(&mut self.memory, binding.container)?;
        crate::native_part::bind_reference(&mut self.memory, slot, raw);
        Ok(())
    }
    pub fn resource(&mut self, slot: u32) -> crate::factory::CfgBinding {
        let binding = self.resource_bindings.get(&slot).copied().or_else(|| {
            TEMPLATES.with(|templates| {
                templates
                    .borrow()
                    .values()
                    .find(|template| template.body == self.template)
                    .and_then(|template| template.bindings.get(&slot).copied())
            })
        });
        let mut binding = binding.unwrap_or_else(|| crate::factory::CfgBinding {
            id: self.memory.r32(slot + 8),
            container: logical_resource(self.memory.r32(slot + 12)).unwrap_or(0),
            link: 0,
        });
        let live = slr_resource::node::with_res(|host| {
            if binding.link != 0 {
                host.node(binding.link)
                    .map(|node| (node.id, node.container))
            } else {
                host.container(binding.container)
                    .map(|resource| (resource.id, binding.container))
            }
        })
        .unwrap_or_default();
        binding.id = live.0;
        binding.container = live.1;
        if live.1 == 0 {
            crate::native_part::bind_reference(&mut self.memory, slot, 0);
        } else {
            self.memory.w32(slot + 8, live.0);
        }
        if let Some(owned) = self.resource_bindings.get_mut(&slot) {
            *owned = binding;
        }
        binding
    }
}
impl Drop for NativeBody {
    fn drop(&mut self) {
        let _ = MEMORY.try_with(|memory| {
            for (_, binding) in self.resource_bindings.drain() {
                crate::factory::release_cfg(binding);
            }
            if memory.borrow().is_none() {
                *memory.borrow_mut() = Some(std::mem::replace(&mut self.memory, Space::new()));
            }
        });
    }
}
struct NativeTemplate {
    body: u32,
    bindings: HashMap<u32, crate::factory::CfgBinding>,
}
thread_local! {
    static BODIES: RefCell<HashMap<u32, NativeBody>> = RefCell::new(HashMap::new());
    static TEMPLATES: RefCell<HashMap<u32, NativeTemplate>> = RefCell::new(HashMap::new());
    static MEMORY: RefCell<Option<Space>> = RefCell::new(Some(Space::new()));
    static RAW_RESOURCES: RefCell<HashMap<u32,u32>> = RefCell::new(HashMap::new());
}

/// Resource identities use their own compact host arena. Native layouts must
/// have separate storage: a type2 container is0x164 bytes, exceeding the host
/// arena's0x100 spacing. Only original resource-class sizes are admitted.
pub fn raw_resource(memory: &mut Space, container: u32) -> Result<u32, String> {
    raw_resource_named(memory, container, None)
}
/// An explicit descriptor-name override retains null versus empty-string names
/// when constructing a clone; the host identity stores text only.
pub fn raw_resource_named(
    memory: &mut Space,
    container: u32,
    descriptor_name: Option<Option<&[u8]>>,
) -> Result<u32, String> {
    if container == 0 {
        return Ok(0);
    }
    if let Some(raw) = RAW_RESOURCES.with(|map| map.borrow().get(&container).copied()) {
        return Ok(raw);
    }
    let resource = slr_resource::node::with_res(|host| host.container(container).cloned())
        .ok_or("NativeResourceBoundary: missing live resource identity")?;
    let kind = resource.type_id as u32;
    let size = crate::native_resource_base::size(kind)
        .ok_or_else(|| format!("NativeResourceBoundary: unported container layout type{kind}"))?;
    let raw = memory.alloc(size);
    RAW_RESOURCES.with(|map| map.borrow_mut().insert(container, raw));
    let (offset, length) = if matches!(kind, 5 | 0xe) && resource.id >> 16 != 0xffff {
        let (_, offset, length) = crate::native_assets::descriptor_location(resource.id)?;
        (offset, length)
    } else {
        (0, 0)
    };
    let descriptor = [kind, resource.id, resource.flags, offset, length];
    let name = crate::factory::resource_name(container);
    let name = descriptor_name.unwrap_or_else(|| name.as_ref().map(|name| name.as_bytes()));
    crate::native_resource_base::construct(memory, raw, descriptor, name, None)?;
    memory.w32(raw + 0x64, resource.load_state);
    slr_resource::node::with_res(|host| {
        host.container_mut(container)
            .expect("raw resource has live identity")
            .flags = memory.r32(raw + 0x54)
    });
    let parent = raw_resource(memory, resource.parent)?;
    if parent != 0 {
        //536a10 appends the newly constructed node to its parent's second
        //intrusive child list; raw identities remain owned by the live factory.
        let previous = memory.r32(parent + 0x38);
        memory.w32(raw + 0x14, parent);
        memory.w32(previous + 4, raw);
        memory.w32(raw + 4, parent + 0x30);
        memory.w32(raw + 8, previous);
        memory.w32(parent + 0x38, raw);
        if matches!(kind, 1..=3) {
            crate::native_resource_base::inherit_spatial_root(memory, raw, parent);
        }
    }
    Ok(raw)
}
fn synchronize_generation(memory: &mut Space) {
    memory.w32(
        0x6200a4,
        slr_render_natives::with_render(|render| render.frame()),
    );
}
fn synchronize_flags(memory: &mut Space, from_host: bool) {
    let resources = RAW_RESOURCES.with(|map| {
        map.borrow()
            .iter()
            .map(|(&logical, &raw)| (logical, raw))
            .collect::<Vec<_>>()
    });
    slr_resource::node::with_res(|host| {
        for (logical, raw) in resources {
            if let Some(resource) = host.container_mut(logical) {
                if from_host {
                    memory.w32(raw + 0x54, resource.flags);
                } else {
                    resource.flags = memory.r32(raw + 0x54);
                }
            }
        }
    });
}

pub fn logical_resource(raw: u32) -> Option<u32> {
    RAW_RESOURCES.with(|map| {
        map.borrow()
            .iter()
            .find_map(|(&logical, &address)| (address == raw).then_some(logical))
    })
}
pub fn with_body<R>(container: u32, f: impl FnOnce(&mut NativeBody) -> R) -> Option<R> {
    BODIES.with(|bodies| {
        let mut bodies = bodies.borrow_mut();
        let body = bodies.get_mut(&container)?;
        body.memory = MEMORY
            .with(|memory| memory.borrow_mut().take())
            .expect("nested native body memory borrow");
        struct Restore<'a>(&'a mut NativeBody);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                MEMORY.with(|memory| {
                    *memory.borrow_mut() = Some(std::mem::replace(&mut self.0.memory, Space::new()))
                });
            }
        }
        let restore = Restore(body);
        synchronize_flags(&mut restore.0.memory, true);
        synchronize_generation(&mut restore.0.memory);
        let result = f(restore.0);
        synchronize_flags(&mut restore.0.memory, false);
        Some(result)
    })
}
pub fn clear() {
    crate::native_assets::clear();
    BODIES.with(|bodies| bodies.borrow_mut().clear());
    let templates = TEMPLATES.with(|templates| std::mem::take(&mut *templates.borrow_mut()));
    for (_, template) in templates {
        for (_, binding) in template.bindings {
            crate::factory::release_cfg(binding);
        }
    }
    MEMORY.with(|memory| *memory.borrow_mut() = Some(Space::new()));
    RAW_RESOURCES.with(|map| map.borrow_mut().clear());
}

pub fn insert(mut body: NativeBody) {
    synchronize_flags(&mut body.memory, false);
    MEMORY.with(|memory| {
        *memory.borrow_mut() = Some(std::mem::replace(&mut body.memory, Space::new()))
    });
    BODIES.with(|bodies| {
        bodies.borrow_mut().insert(body.container, body);
    });
}

pub fn has_loaded_template(container: u32) -> bool {
    TEMPLATES.with(|templates| templates.borrow().contains_key(&container))
}

/// Raw configured-template payload (`K`) retained for `container` by
/// `prepare_template`. Query routing uses the original template vtable slot.
pub fn template_payload(container: u32) -> Option<u32> {
    TEMPLATES.with(|templates| templates.borrow().get(&container).map(|t| t.body))
}

pub fn with_world_memory<R>(f: impl FnOnce(&mut Space) -> R) -> Result<R, String> {
    struct Lease(Option<Space>);
    impl Drop for Lease {
        fn drop(&mut self) {
            MEMORY.with(|memory| *memory.borrow_mut() = self.0.take());
        }
    }
    let memory = MEMORY
        .with(|memory| memory.borrow_mut().take())
        .ok_or("NativeBodyBoundary: shared memory is already borrowed")?;
    let mut lease = Lease(Some(memory));
    synchronize_generation(lease.0.as_mut().expect("active shared memory lease"));
    Ok(f(lease.0.as_mut().expect("active shared memory lease")))
}

/// 442500/45dcf0: fresh native template storage and embedded list sentinels.
/// Camera words come from the settings lookup performed by the constructor.
pub fn new_car_template(
    memory: &mut Space,
    resource_id: u32,
    container: u32,
    camera: [u32; 6],
) -> u32 {
    let k = memory.alloc(0x2e98);
    memory.w32(k, 0x5f0f18);
    memory.w32(k + 0xc, resource_id);
    memory.w32(k + 0x10, container);
    memory.w32(k + 0xfb4, 1f32.to_bits());
    // DAT_611358, the default part damage scale used by442500.
    memory.w32(k + 0x11d4, 1f32.to_bits());
    for (offset, vtable) in [(0x1154, 0x5f0fac), (0x1234, 0x5f0fa8)] {
        let list = k + offset;
        memory.w32(list, vtable);
        memory.w32(list + 4, 0x5f09ac);
        memory.w32(list + 8, list + 0x10);
        memory.w32(list + 0x10, 0x5f09ac);
        memory.w32(list + 0x18, list + 4);
    }
    // 42bcb0 constructs the click body record and sixteen camera records.
    memory.w32(k + 0x1170 + 0x44, 1f32.to_bits());
    for i in 0..16 {
        memory.w32(k + 0x12fc + i * 0x48 + 0x44, 1f32.to_bits());
    }
    // 45e070's seventeen sound records retain the default unit gain.
    for offset in (0x19d4..=0x1e54).step_by(0x48) {
        memory.w32(k + offset + 0x3c, 1f32.to_bits());
    }
    for (offset, value) in [0x126c, 0x1270, 0x1274, 0x1278, 0x127c, 0x1294]
        .into_iter()
        .zip(camera)
    {
        memory.w32(k + offset, value);
    }
    memory.w32(k + 0x1ec8, 0x3f000000);
    k
}

/// `FUN_0046b330`: fresh native **part** template storage (`0x126c`) and the
/// sentinel lists its constructor builds, before `FUN_0046e570` runs the
/// preparse defaults, the part cfg table and the postparse stage.
///
/// Unlike the car template this has no parameter block, camera records, sound
/// records or wheel fields; `K+0x128` holds 32 body records, `K+0x1170` one
/// click record, `K+0x1234/0x1250` the two list heads and `K+0x11e8` the skinned
/// block.
pub fn new_part_template(memory: &mut Space, resource_id: u32, container: u32) -> u32 {
    let k = memory.alloc(0x126c);
    // 429910: base configured-object header.
    memory.w32(k, 0x5f09e0);
    for offset in [4, 8, 0xc, 0x10] {
        memory.w32(k + offset, 0);
    }
    memory.w32(k + 0x14, 0);
    // 32 body records through 42bcf0.
    for i in 0..32 {
        for offset in (0..=0x3c).step_by(4) {
            memory.w32(k + 0x128 + i * 0x74 + offset, 0);
        }
        memory.w32(k + 0x128 + i * 0x74 + 0x44, 1f32.to_bits());
    }
    memory.w32(k + 0xfac, 0);
    memory.w32(k + 0xfb0, 0);
    memory.w32(k + 0xfb4, 1f32.to_bits());
    memory.w32(k + 0x1040, 0);
    memory.w32(k + 0x1044, 0);
    // 1048 click record through 428fc0; then its trailing words.
    for offset in (0..=0xc).step_by(4) {
        memory.w32(k + 0x1048 + offset, 0);
    }
    for offset in [0x1058, 0x105c, 0x1060, 0x1064, 0x1068, 0x106c, 0x1070] {
        memory.w32(k + offset, 0);
    }
    // 1074 shadow record through 42bcf0.
    for offset in (0..=0x3c).step_by(4) {
        memory.w32(k + 0x1074 + offset, 0);
    }
    for offset in (0x40..=0x70).step_by(4) {
        memory.w32(k + 0x1074 + offset, 0);
    }
    // Six embedded Ref nodes at 10e8/f8/108/118/128/138.
    for base in [0x10e8, 0x10f8, 0x1108, 0x1118, 0x1128, 0x1138] {
        for offset in (0..=0xc).step_by(4) {
            memory.w32(k + base + offset, 0);
        }
    }
    // 4771a0 and 4771e0 lists.
    for (base, vtable) in [(0x1154, 0x5f0fac), (0x1234, 0x5f0fa8)] {
        crate::native_car::list(memory, k + base, vtable);
    }
    // 401860 with 42bcb0: one 0x48-byte click record at 1170.
    for offset in (0..0x48).step_by(4) {
        memory.w32(k + 0x1170 + offset, 0);
    }
    // 45dce0 writes one word at 11e8; 4771e0 builds the 1234 list; 42cec0
    // clears the 1250 Ref.
    memory.w32(k + 0x11e8, 0);
    for offset in (0..=0xc).step_by(4) {
        memory.w32(k + 0x1250 + offset, 0);
    }
    memory.w32(k, 0x5f0f60);
    memory.w32(k + 0x120, 0);
    // 46b461: the descriptor argument carries the resource; +4/+8/+c/+10 are
    // its weak link and id (id at +0xc from the resource's +0x50).
    memory.w32(k + 0xc, resource_id);
    memory.w32(k + 0x10, container);
    // 46b4d1's `%s %d %d` scan destinations.
    for offset in (0x18..0x118).step_by(4) {
        memory.w32(k + offset, 0);
    }
    memory.w32(k + 0x118, 0);
    memory.w32(k + 0x11c, 0);
    memory.w32(k + 0x124, 0);
    k
}

/// The original in-memory cfg dispatcher (4b6480 -> 4b6340). Lines are
/// bounded independently, comments terminate the parser's local copy, and
/// unknown keywords do not invoke a handler.
pub fn parse_cfg(
    memory: &mut Space,
    template: u32,
    host: &mut dyn slr_physics::cfg::Host,
    text: &[u8],
) {
    parse_cfg_with(memory, template, host, text, slr_physics::cfg::table());
}

/// Same dispatcher with an explicit keyword table: the car loader (`4472ef`)
/// uses `0x60e0f8`, the part loader (`46e570`) uses `0x611368`.
pub fn parse_cfg_with(
    memory: &mut Space,
    template: u32,
    host: &mut dyn slr_physics::cfg::Host,
    text: &[u8],
    table: Vec<(&'static str, slr_physics::cfg::Handler)>,
) {
    for line in text.split(|byte| *byte == b'\n') {
        let line = &line[..line.len().min(255)];
        let end = line
            .iter()
            .position(|byte| matches!(byte, b';' | b'#' | b'/' | 0))
            .unwrap_or(line.len());
        let line = &line[..end];
        let whitespace = |byte: &u8| matches!(byte, b' ' | b'\r' | b'\n' | b'\t');
        let Some(start) = line.iter().position(|byte| !whitespace(byte)) else {
            continue;
        };
        let end = line[start..]
            .iter()
            .position(whitespace)
            .map_or(line.len(), |end| start + end);
        let Some((_, handler)) = table
            .iter()
            .find(|(name, _)| name.as_bytes().eq_ignore_ascii_case(&line[start..end]))
        else {
            continue;
        };
        let mut buffer = line.to_vec();
        if end == buffer.len() {
            buffer.push(0);
        } else {
            buffer[end] = 0;
        }
        buffer.extend_from_slice(&[0; 4]);
        handler(
            &mut slr_physics::cfg::Ctx {
                m: memory,
                host,
                k: template,
            },
            &buffer,
        );
    }
}

/// Resource/geometry boundaries used by the original cfg handlers. A failed
/// collaborator is retained as a typed failure; partial raw state is never
/// published as a configured native instance.
pub struct ConfigHost {
    pub template_id: u32,
    pub error: Option<String>,
    bindings: HashMap<u32, crate::factory::CfgBinding>,
    local_bindings: Vec<crate::factory::CfgBinding>,
    definitions: HashMap<u32, u32>,
}
impl ConfigHost {
    pub fn take_bindings(&mut self) -> HashMap<u32, crate::factory::CfgBinding> {
        std::mem::take(&mut self.bindings)
    }
    pub fn new(template_id: u32) -> Self {
        Self {
            template_id,
            error: None,
            bindings: HashMap::new(),
            local_bindings: Vec::new(),
            definitions: HashMap::new(),
        }
    }
    pub fn finish(&mut self) -> Result<(), String> {
        for binding in self.local_bindings.drain(..) {
            crate::factory::release_cfg(binding);
        }
        self.error.take().map_or(Ok(()), Err)
    }
    fn invoke(
        &mut self,
        memory: &mut Space,
        call: &slr_physics::cfg::ExtCall,
    ) -> Result<u32, String> {
        use slr_physics::cfg::*;
        let word = |i: usize| match call.a.get(i) {
            Some(Arg::U(word)) => *word,
            _ => 0,
        };
        let text = |i: usize| match call.a.get(i) {
            Some(Arg::S(text)) => text.as_str(),
            _ => "",
        };
        let address = match call.this {
            This::Addr(address) => address,
            _ => 0,
        };
        match call.f {
            EXT_RESOLVE => crate::factory::resolve_cfg(self.template_id, word(0)),
            EXT_LOOKUP => crate::factory::lookup_cfg(text(0), word(1)),
            EXT_BIND => {
                let binding =
                    crate::factory::bind_cfg(self.template_id, word(0), word(1) as i32, word(2))?;
                if matches!(call.this, This::Local) {
                    self.local_bindings.push(binding);
                } else {
                    if let Some(old) = self.bindings.insert(address, binding) {
                        crate::factory::release_cfg(old);
                    }
                    let raw = raw_resource(memory, binding.container)?;
                    crate::native_part::bind_reference(memory, address, raw);
                }
                Ok(binding.container)
            }
            EXT_EULER_TO_MATRIX => {
                let matrix = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
                    f32::from_bits(word(0)),
                    f32::from_bits(word(1)),
                    f32::from_bits(word(2)),
                ));
                for (i, value) in matrix.m.into_iter().enumerate() {
                    memory.wf(address + (i / 3 * 16 + i % 3 * 4) as u32, value);
                }
                Ok(0)
            }
            EXT_MATRIX_TO_EULER => {
                let source = word(0);
                let mut matrix = slr_lang::vec::Mat3 { m: [0.; 9] };
                for (i, value) in matrix.m.iter_mut().enumerate() {
                    *value = memory.rf(source + (i / 3 * 16 + i % 3 * 4) as u32);
                }
                let angles = matrix.to_ypr();
                for (i, value) in [angles.y, angles.p, angles.r].into_iter().enumerate() {
                    memory.wf(address + i as u32 * 4, value);
                }
                Ok(0)
            }
            EXT_LOG => {
                slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
                    category: "native-cfg-log".into(),
                    fields: vec![
                        ("template".into(), format!("{:#010x}", self.template_id)),
                        ("message".into(), format!("{:?}", call.a)),
                    ],
                });
                Ok(0)
            }
            EXT_RESNAME => {
                let name = crate::factory::package_for_resource(word(0))?;
                let descriptor = memory.alloc(12 + name.len() as u32 + 1);
                memory.w32(descriptor + 8, word(0) >> 16);
                for (i, byte) in name.bytes().enumerate() {
                    memory.w8(descriptor + 12 + i as u32, byte);
                }
                Ok(descriptor)
            }
            EXT_BODY => {
                let bytes = match call.a.get(5) {
                    Some(Arg::S(shape)) => Some(crate::factory::read_cfg_file(shape)?),
                    _ => None,
                };
                crate::native_geometry::body(
                    memory,
                    word(0),
                    word(1),
                    [word(2), word(3), word(4)],
                    bytes.as_deref(),
                )
            }
            EXT_BODYDEF => {
                // The cfg handlers pass their four-word local ResourceRef as
                // values after the mass argument; +8 is its canonical ID.
                let id = word(3);
                if id == 0 {
                    return Ok(0);
                }
                let definition = if let Some(definition) = self.definitions.get(&id) {
                    *definition
                } else {
                    let (kind, _, descriptor) = crate::factory::cfg_descriptor(id)?;
                    if kind != 9 {
                        return Err(format!(
                            "NativeGeometryBoundary: click definition {id:#010x} has type {kind}"
                        ));
                    }
                    let config = crate::native_definition::Definition::parse(&descriptor)?;
                    let bytes = crate::factory::read_cfg_file(&config.shape)?;
                    let (definition, _) = config.materialize(memory, &bytes)?;
                    self.definitions.insert(id, definition);
                    definition
                };
                crate::native_geometry::body_from_definition(memory, word(0), definition)
            }
            _ => Err(format!(
                "NativeCfgBoundary: unsupported collaborator {:#08x}",
                call.f
            )),
        }
    }
}
impl slr_physics::cfg::Host for ConfigHost {
    fn call(&mut self, memory: &mut Space, call: &slr_physics::cfg::ExtCall) -> u32 {
        if self.error.is_some() {
            return 0;
        }
        match self.invoke(memory, call) {
            Ok(value) => value,
            Err(error) => {
                self.error = Some(error);
                0
            }
        }
    }
}

impl Drop for ConfigHost {
    fn drop(&mut self) {
        for (_, binding) in self.bindings.drain() {
            crate::factory::release_cfg(binding);
        }
        for binding in self.local_bindings.drain(..) {
            crate::factory::release_cfg(binding);
        }
    }
}

fn camera_settings(vm: &mut VmHost) -> Result<[u32; 6], String> {
    let config = crate::gc_runtime::config_script()
        .ok_or("NativeConfigBoundary: Config anchor63643c unavailable")?;
    let mut read = |name| {
        vm.read_field(config, name)
            .map(|value| value.payload)
            .map_err(|error| format!("NativeConfigBoundary: Config.{name}: {error:?}"))
    };
    let distance = slr_x87::X87::from_f32(f32::from_bits(read("camera_distance")?))
        .mul(
            slr_x87::X87::from_f32(0.2),
            slr_x87::Precision::F32,
            slr_x87::Rounding::NearestEven,
        )
        .to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven)
        .to_bits();
    let mut values = [
        read("camera_look_ahead")?,
        read("camera_go_behind")?,
        read("camera_go_above")?,
        distance,
        0,
        read("target_angle_weight")?,
    ];
    values[4] = vm
        .read_field(config, "camera_FOV")
        .map_or(0, |value| value.payload);
    Ok(values)
}

/// Load a car template through its original constructor, preparse defaults,
/// cfg handlers and postparse stage. Keep ownership local until the runtime
/// construction succeeds; failed configurations never publish a live body.
pub fn prepare_template(
    vm: &mut VmHost,
    id: u32,
    container: u32,
    class: &str,
    path: &str,
) -> Result<NativeBody, String> {
    if !matches!(class, "car" | "part") {
        return Err(format!(
            "NativeTemplateBoundary: {class} template construction is not integrated"
        ));
    }
    if let Some(template) = TEMPLATES.with(|templates| {
        templates
            .borrow()
            .get(&container)
            .map(|template| template.body)
    }) {
        return NativeBody::acquire(container, template);
    }
    let bytes = crate::factory::read_cfg_file(path)?;
    let mut state = NativeBody::acquire(container, 0)?;
    let raw = raw_resource(&mut state.memory, container)?;
    let mut host = ConfigHost::new(id);
    if class == "part" {
        // 46b330 part constructor, 46e570 preparse defaults + 0x611368 table,
        // then 46e400 slot-frame transform and the 0x120 increment.
        let k = new_part_template(&mut state.memory, id, raw);
        state.template = k;
        state.memory.w32(k + 0x124, 0);
        if path.len() >= 256 {
            return Err(
                "NativeTemplateBoundary: cfg path exceeds original filename storage".into(),
            );
        }
        // 46b4d1 writes the descriptor text at K+0x18; 46e570 reads the same
        // fixed path when no in-memory cfg buffer is installed at K+0x118.
        for (i, byte) in path.bytes().enumerate() {
            state.memory.w8(k + 0x18 + i as u32, byte);
        }
        crate::native_defaults::base_defaults(&mut state.memory, k, &mut host);
        host.finish()?;
        crate::native_part::bind_template_sound(&mut state.memory, k, &mut host);
        host.finish()?;
        parse_cfg_with(
            &mut state.memory,
            k,
            &mut host,
            &bytes,
            slr_physics::cfg::part_table(),
        );
        host.finish()?;
        crate::native_postparse::configure_finish_part(&mut state.memory, k, &mut host)?;
        host.finish()?;
        state.memory.w32(raw + 0x8c, k);
        crate::native_resource_load::mark_loaded(&mut state.memory, raw, 1)?;
        slr_resource::node::with_res(|resources| {
            let resource = resources
                .container_mut(container)
                .expect("loaded template retains its resource identity");
            resource.load_state = state.memory.r32(raw + 0x64);
        });
        TEMPLATES.with(|templates| {
            templates.borrow_mut().insert(
                container,
                NativeTemplate {
                    body: k,
                    bindings: host.take_bindings(),
                },
            )
        });
        return Ok(state);
    }
    let camera = camera_settings(vm)?;
    let k = new_car_template(&mut state.memory, id, raw, camera);
    state.template = k;
    //442500's fixed filename storage is the original file-load fallback when
    // no in-memory cfg buffer has been installed at+118.
    if path.len() >= 256 {
        return Err("NativeTemplateBoundary: cfg path exceeds original filename storage".into());
    }
    for (i, byte) in path.bytes().enumerate() {
        state.memory.w8(k + 0x18 + i as u32, byte);
    }
    crate::native_defaults::car_defaults(&mut state.memory, k, &mut host);
    host.finish()?;
    parse_cfg(&mut state.memory, k, &mut host, &bytes);
    host.finish()?;
    crate::native_postparse::configure_finish(&mut state.memory, k, &mut host)?;
    host.finish()?;
    state.memory.w32(raw + 0x8c, k);
    crate::native_resource_load::mark_loaded(&mut state.memory, raw, 1)?;
    slr_resource::node::with_res(|resources| {
        let resource = resources
            .container_mut(container)
            .expect("loaded template retains its resource identity");
        resource.load_state = state.memory.r32(raw + 0x64);
    });
    // The loaded type8 template owns its native class and cfg references.
    // Runtime instances reuse that class rather than running its loader again.
    TEMPLATES.with(|templates| {
        templates.borrow_mut().insert(
            container,
            NativeTemplate {
                body: k,
                bindings: host.take_bindings(),
            },
        )
    });
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn loaded_template_reuses_class_and_owns_cfg_links_across_instances() {
        clear();
        slr_resource::node::reset_res();
        let resource = slr_resource::node::with_res(|host| {
            let container = host.add_container(slr_resource::node::Container::new(7, 0xffff0100));
            host.bind_id(0xffff0100, container);
            container
        });
        let owner = 0x30000100;
        let mut state = NativeBody::acquire(owner, 0).unwrap();
        let template = new_car_template(&mut state.memory, 0xffff0001, 0, [0; 6]);
        state.template = template;
        let slot = template + 0x10e8;
        state.bind_resource(slot, 0xffff0100, 7, 0).unwrap();
        let binding = state.resource(slot);
        assert_eq!(binding.container, resource);
        let heap = state.memory.heap_ptr();
        TEMPLATES.with(|templates| {
            templates.borrow_mut().insert(
                owner,
                NativeTemplate {
                    body: template,
                    bindings: std::mem::take(&mut state.resource_bindings),
                },
            )
        });
        drop(state);
        let mut vm = VmHost::new();
        for _ in 0..2 {
            let mut instance = prepare_template(
                &mut vm,
                0xffff0001,
                owner,
                "car",
                "absent-cfg-must-not-be-read",
            )
            .unwrap();
            assert_eq!(instance.template, template);
            assert_eq!(instance.memory.heap_ptr(), heap);
            assert_eq!(instance.resource(slot), binding);
            drop(instance);
            assert!(slr_resource::node::with_res(|host| host
                .node(binding.link)
                .is_some()));
        }
        clear();
        assert!(slr_resource::node::with_res(|host| host
            .node(binding.link)
            .is_none()));
    }

    #[test]
    fn original_car_constructor_heap() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-defaults-original.json");
        let Ok(text) = std::fs::read_to_string(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: native-defaults-original");
            return;
        };
        let row: serde_json::Value = serde_json::from_str(&text).unwrap();
        let mut memory = Space::new();
        let cfg = memory.alloc(16);
        for (i, byte) in b"eof\n".iter().enumerate() {
            memory.w8(cfg + i as u32, *byte);
        }
        let k = new_car_template(
            &mut memory,
            0,
            0,
            [
                1f32.to_bits(),
                1f32.to_bits(),
                1f32.to_bits(),
                0.2f32.to_bits(),
                0,
                1f32.to_bits(),
            ],
        );
        memory.w32(k + 0x18, b'x' as u32);
        memory.w32(k + 0x118, cfg);
        memory.w32(k + 0x11c, 4);
        assert_eq!(k as u64, row["k"].as_u64().unwrap());
        let actual: Vec<_> = (slr_physics::space::HEAP_BASE..memory.heap_ptr())
            .step_by(4)
            .filter_map(|a| {
                let v = memory.r32(a);
                (v != 0).then_some(serde_json::json!([a, v]))
            })
            .collect();
        assert_eq!(serde_json::json!(actual), row["before"]["words"]);
        assert_eq!(
            memory.heap_ptr() as u64,
            row["before"]["heap"].as_u64().unwrap()
        );
    }
    #[test]
    fn cfg_dispatcher_comments_case_and_line_bound() {
        struct NoHost;
        impl slr_physics::cfg::Host for NoHost {
            fn call(&mut self, _: &mut Space, _: &slr_physics::cfg::ExtCall) -> u32 {
                panic!("scalar handler has no external calls")
            }
        }
        let mut memory = Space::new();
        let k = memory.alloc(0x2e98);
        parse_cfg(
            &mut memory,
            k,
            &mut NoHost,
            b"unknown ignored\n  CATEGORY 17 #comment\ncategory 99/ ignored\ncategory 42;comment\n",
        );
        assert_eq!(memory.r32(k + 0x114c), 42);
        let mut long = b"category ".to_vec();
        long.extend_from_slice(&[b' '; 246]);
        long.extend_from_slice(b"88\n");
        parse_cfg(&mut memory, k, &mut NoHost, &long);
        assert_eq!(memory.r32(k + 0x114c), 42);
    }
}

//! Configured native car/part state. VM wrappers address the original raw
//! template/runtime layouts; engine containers remain the ownership identity.
use slr_physics::space::Space;
use slr_vm::host::VmHost;
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
};

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
    static IMAGE: RefCell<Option<crate::native_image::ImageData>> = const { RefCell::new(None) };
    static BODIES: RefCell<HashMap<u32, NativeBody>> = RefCell::new(HashMap::new());
    static TEMPLATES: RefCell<HashMap<u32, NativeTemplate>> = RefCell::new(HashMap::new());
    static MEMORY: RefCell<Option<Space>> = RefCell::new(Some(new_world_memory()));
    static RAW_RESOURCES: RefCell<HashMap<u32,u32>> = RefCell::new(HashMap::new());
    static CONSTRUCTING: RefCell<HashSet<u32>> = RefCell::new(HashSet::new());
}

/// Called after the stock checksum gate and before native static constructors.
pub fn configure_image(path: &std::path::Path) -> Result<(), String> {
    let bytes = std::fs::read(path).map_err(|e| format!("NativeImageBoundary: {e}"))?;
    let image = crate::native_image::ImageData::parse(&bytes)
        .map_err(|e| format!("NativeImageBoundary: {e}"))?;
    IMAGE.with(|slot| *slot.borrow_mut() = Some(image));
    Ok(())
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
    if !CONSTRUCTING.with(|set| set.borrow_mut().insert(container)) {
        return Err("NativeResourceBoundary: cyclic resource materialization".into());
    }
    struct Construction(u32);
    impl Drop for Construction {
        fn drop(&mut self) {
            CONSTRUCTING.with(|set| set.borrow_mut().remove(&self.0));
        }
    }
    let _construction = Construction(container);
    let resource = slr_resource::node::with_res(|host| host.container(container).cloned())
        .ok_or("NativeResourceBoundary: missing live resource identity")?;
    let kind = resource.type_id as u32;
    let size = crate::native_resource_base::size(kind)
        .ok_or_else(|| format!("NativeResourceBoundary: unported container layout type{kind}"))?;
    let raw = memory.alloc(size);
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
    // Publish only a fully constructed, linked resource. A failed descriptor
    // access must remain an error when retried, never a cached raw identity.
    RAW_RESOURCES.with(|map| map.borrow_mut().insert(container, raw));
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

/// Publish native parent changes in raw sibling order before the VM observes
/// them. Unmaterialized host children remain in their existing order.
pub fn synchronize_topology(memory: &Space) -> Result<(), String> {
    let resources = RAW_RESOURCES.with(|map| map.borrow().clone());
    let inverse: HashMap<_, _> = resources
        .iter()
        .map(|(&logical, &raw)| (raw, logical))
        .collect();
    let mut moved = HashMap::new();
    slr_resource::node::with_res(|host| {
        for (&logical, &raw) in &resources {
            let Some(resource) = host.container(logical) else {
                continue;
            };
            let parent_raw = memory.r32(raw + 0x14);
            let parent = if parent_raw == 0 {
                0
            } else {
                *inverse
                    .get(&parent_raw)
                    .ok_or("native parent lacks host identity")?
            };
            if resource.parent != parent {
                moved.insert(raw, (logical, parent));
            }
        }
        // A resource move appends. Read the final native list to order moves
        // that happened within the same native graph before any callback.
        let parents: std::collections::BTreeSet<_> =
            moved.keys().map(|raw| memory.r32(raw + 0x14)).collect();
        for parent_raw in parents {
            if parent_raw == 0 {
                continue;
            }
            let mut child = memory.r32(parent_raw + 0x20);
            let mut seen = std::collections::HashSet::new();
            while child != 0 && memory.r32(child + 4) != 0 {
                if !seen.insert(child) {
                    return Err("cyclic native resource child list".into());
                }
                if let Some((logical, parent)) = moved.remove(&child) {
                    host.synchronize_parent(logical, parent)?;
                }
                child = memory.r32(child + 4);
            }
        }
        for (_, (logical, parent)) in moved {
            if parent != 0 {
                return Err("moved native resource absent from parent's list".into());
            }
            host.synchronize_parent(logical, parent)?;
        }
        Ok(())
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
    MEMORY.with(|memory| *memory.borrow_mut() = Some(new_world_memory()));
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

pub fn has_native_body(container: u32) -> bool {
    BODIES.with(|bodies| bodies.borrow().contains_key(&container))
}

pub fn body_container(body: u32) -> Option<u32> {
    BODIES.with(|bodies| {
        bodies
            .borrow()
            .iter()
            .find_map(|(&container, state)| (state.body == body).then_some(container))
    })
}

pub fn has_loaded_template(container: u32) -> bool {
    TEMPLATES.with(|templates| templates.borrow().contains_key(&container))
}

/// Raw configured-template payload (`K`) retained for `container` by
/// `prepare_template`. Query routing uses the original template vtable slot.
pub fn template_payload(container: u32) -> Option<u32> {
    TEMPLATES.with(|templates| templates.borrow().get(&container).map(|t| t.body))
}

/// Lazy native template loads may acquire references while constructing the
/// first instance. Their ownership belongs to the template, across instances.
pub fn retain_template_bindings(
    container: u32,
    bindings: HashMap<u32, crate::factory::CfgBinding>,
) -> Result<(), String> {
    TEMPLATES.with(|templates| {
        let mut templates = templates.borrow_mut();
        let Some(template) = templates.get_mut(&container) else {
            for (_, binding) in bindings {
                crate::factory::release_cfg(binding);
            }
            return Err("NativeTemplateBoundary: loaded reference owner missing".into());
        };
        for (slot, binding) in bindings {
            if let Some(old) = template.bindings.insert(slot, binding) {
                crate::factory::release_cfg(old);
            }
        }
        Ok(())
    })
}

/// Read the current ABI state without taking the shared graph's lease. During
/// a VM callback that graph is back in MEMORY, including the parent's live
/// native scratch. A new script-native call needs its own recovered CALL
/// context; treating it as an independent oracle-driver entry corrupts that
/// parent or panics inside native_stack::enter.
pub fn has_active_native_frame() -> bool {
    MEMORY.with(|memory| {
        memory
            .borrow()
            .as_ref()
            .is_some_and(crate::native_stack::active)
    })
}

/// Read the shared native memory without leasing or synchronizing it.
/// `None` while another caller holds the lease.
pub fn inspect_world_memory<R>(f: impl FnOnce(&Space) -> R) -> Option<R> {
    MEMORY.with(|memory| memory.borrow().as_ref().map(f))
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
    let memory = lease.0.as_mut().expect("active shared memory lease");
    synchronize_flags(memory, true);
    synchronize_generation(memory);
    let result = f(memory);
    synchronize_flags(memory, false);
    Ok(result)
}

/// Return an active native graph's memory to the runtime while a VM callback
/// executes. A callback can enter another native, construct a resource or
/// destroy one; all of those operations must see the same live memory. Moving
/// ownership avoids holding a RefCell borrow or aliasing a mutable Space.
pub fn during_vm_callback<R>(
    memory: &mut Space,
    f: impl FnOnce() -> Result<R, String>,
) -> Result<R, String> {
    if MEMORY.with(|slot| slot.borrow().is_some()) {
        return Err("NativeBodyBoundary: callback requires an active memory lease".into());
    }
    synchronize_topology(memory)?;
    synchronize_flags(memory, false);
    MEMORY.with(|slot| *slot.borrow_mut() = Some(std::mem::replace(memory, Space::new())));
    struct Restore<'a>(&'a mut Space);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            *self.0 = MEMORY
                .with(|slot| slot.borrow_mut().take())
                .expect("VM callback returned shared memory");
            synchronize_flags(self.0, true);
        }
    }
    let _restore = Restore(memory);
    f()
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
                // The CFG port captures the three words behind the original
                // angle pointer. Recreate that input in the native scratch
                // frame; keep the caller's ESP, including on explicit gaps.
                let saved = crate::native_stack::sp(memory);
                let angles = crate::native_install::local_reference(memory);
                for i in 0..3 {
                    memory.w32(angles + i as u32 * 4, word(i));
                }
                let frame = crate::native_stack::sp(memory).wrapping_sub(8);
                let result =
                    crate::native_matrix::from_euler_memory(memory, address, angles, frame);
                crate::native_stack::set_sp(memory, saved);
                result?;
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
    if !matches!(class, "car" | "part" | "cursor" | "player") {
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
    if class == "player" {
        let mut state = NativeBody::acquire(container, 0)?;
        let raw = raw_resource(&mut state.memory, container)?;
        let mut host = crate::native_player_runtime::Runtime::new(vm, id);
        state.template = crate::native_player::template(&mut state.memory, 0, raw, &mut host)?;
        state.memory.w32(raw + 0x8c, state.template);
        crate::native_resource_load::mark_loaded(&mut state.memory, raw, 1)?;
        crate::native_assets::synchronize_loaded(&state.memory, raw)?;
        TEMPLATES.with(|templates| {
            templates.borrow_mut().insert(
                container,
                NativeTemplate {
                    body: state.template,
                    bindings: host.take_bindings(),
                },
            );
        });
        return Ok(state);
    }
    if class == "cursor" {
        let mut state = NativeBody::acquire(container, 0)?;
        let raw = raw_resource(&mut state.memory, container)?;
        let mut host = crate::native_cursor_runtime::Runtime {
            vm,
            template_id: id,
        };
        state.template = crate::native_cursor::template(&mut state.memory, raw, &mut host)?;
        state.memory.w32(raw + 0x8c, state.template);
        crate::native_resource_load::mark_loaded(&mut state.memory, raw, 1)?;
        crate::native_assets::synchronize_loaded(&state.memory, raw)?;
        TEMPLATES.with(|templates| {
            templates.borrow_mut().insert(
                container,
                NativeTemplate {
                    body: state.template,
                    bindings: Default::default(),
                },
            );
        });
        return Ok(state);
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

fn new_world_memory() -> Space {
    let mut m = Space::new();
    IMAGE.with(|image| {
        if let Some(image) = image.borrow().as_ref() {
            image.initialize(&mut m);
        }
    });
    crate::native_notifications::initialize_retired(&mut m);
    crate::native_callbacks::initialize(&mut m, crate::native_callbacks::CONTEXT);
    crate::native_message::initialize_release_queue(&mut m);
    m
}

/// Original545f70 invalidates the raw weak links before the resource is freed.
/// This runs inside the factory's host borrow and does not synchronize the host.
pub fn invalidate_raw_resource(container: u32) -> Result<(), String> {
    let raw = RAW_RESOURCES.with(|map| map.borrow().get(&container).copied());
    let Some(raw) = raw else {
        return Ok(());
    };
    MEMORY.with(|memory| {
        let mut slot = memory.borrow_mut();
        let m = slot
            .as_mut()
            .ok_or("raw resource destruction while native memory is leased")?;
        let mut reference = m.r32(raw + 0x48);
        while reference != 0 {
            let next = m.r32(reference + 4);
            for offset in [0, 4, 8, 12] {
                m.w32(reference + offset, 0);
            }
            reference = next;
        }
        m.w32(raw + 0x48, 0);
        Ok(())
    })
}

/// Final5517f0 tree/list unlink, after recursive child destruction. Return
/// retained host links for release after the factory's host borrow ends.
pub fn retire_raw_resource(container: u32) -> Result<Vec<crate::factory::CfgBinding>, String> {
    let raw = RAW_RESOURCES.with(|map| map.borrow().get(&container).copied());
    if let Some(raw) = raw {
        MEMORY.with(|memory| {
            let mut slot = memory.borrow_mut();
            let m = slot
                .as_mut()
                .ok_or("raw resource retirement while native memory is leased")?;
            for next_offset in [4, 12] {
                let previous = m.r32(raw + next_offset + 4);
                let next = m.r32(raw + next_offset);
                if previous != 0 && next != 0 {
                    m.w32(next + next_offset + 4, previous);
                    m.w32(previous + next_offset, next);
                }
                m.w32(raw + next_offset, 0);
                m.w32(raw + next_offset + 4, 0);
            }
            m.w32(raw + 0x14, 0);
            Ok::<_, String>(())
        })?;
        RAW_RESOURCES.with(|map| map.borrow_mut().remove(&container));
    }
    let mut bindings = vec![];
    if let Some(mut body) = BODIES.with(|map| map.borrow_mut().remove(&container)) {
        bindings.extend(body.resource_bindings.drain().map(|(_, binding)| binding));
    }
    if let Some(template) = TEMPLATES.with(|map| map.borrow_mut().remove(&container)) {
        bindings.extend(template.bindings.into_values());
    }
    Ok(bindings)
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

#[cfg(test)]
mod live_graph_tests {
    use super::*;
    use slr_resource::node::with_res;

    #[test]
    fn native_reparent_is_visible_to_callbacks_and_preserves_sibling_destruction() {
        crate::factory::configure(None, None);
        let root = crate::factory::game_root().unwrap();
        let make = |parent, name| {
            crate::factory::create_resource(crate::factory::ResourceDescriptor {
                kind: 1,
                flags: 0,
                parent: Some(parent),
                template: None,
                name,
            })
            .unwrap()
        };
        let old = make(root, "old");
        let new = make(root, "new");
        let a = make(old, "a");
        let b = make(old, "b");
        let c = make(new, "c");
        with_world_memory(|m| {
            let ar = raw_resource(m, a).unwrap();
            let br = raw_resource(m, b).unwrap();
            let nr = raw_resource(m, new).unwrap();
            raw_resource(m, c).unwrap();
            let body = m.alloc(16);
            let link = m.alloc(16);
            crate::native_part::bind_reference(m, body, ar);
            crate::native_part::bind_reference(m, link, nr);
            crate::native_render_graph::reparent_body(m, body, link);
            crate::native_part::bind_reference(m, body, br);
            crate::native_render_graph::reparent_body(m, body, link);
            during_vm_callback(m, || {
                with_res(|host| {
                    assert_eq!(host.container(a).unwrap().parent, new);
                    assert_eq!(host.child_containers(old), Vec::<u32>::new());
                    assert_eq!(host.child_containers(new), vec![c, a, b]);
                });
                with_world_memory(|shared| shared.w32(0x3010_0020, 7))?;
                Ok(())
            })
            .unwrap();
            assert_eq!(m.r32(0x3010_0020), 7);
            crate::native_part::bind_reference(m, body, 0);
            crate::native_part::bind_reference(m, link, 0);
        })
        .unwrap();
        crate::factory::destroy_resource(a).unwrap();
        with_res(|host| {
            assert_eq!(host.child_containers(new), vec![c, b]);
            assert_eq!(host.container(b).unwrap().previous_sibling, c);
        });
    }

    #[test]
    fn native_memory_is_returned_after_nested_callback_error() {
        clear();
        with_world_memory(|m| {
            m.w32(0x3010_0010, 12);
            let result: Result<(), String> = during_vm_callback(m, || {
                with_world_memory(|shared| {
                    assert_eq!(shared.r32(0x3010_0010), 12);
                    shared.w32(0x3010_0010, 13);
                    during_vm_callback(shared, || {
                        with_world_memory(|inner| inner.w32(0x3010_0014, 14))?;
                        Err::<(), _>("script failure".into())
                    })
                })?
            });
            assert_eq!(result.unwrap_err(), "script failure");
            assert_eq!(m.r32(0x3010_0010), 13);
            assert_eq!(m.r32(0x3010_0014), 14);
        })
        .unwrap();
        assert_eq!(with_world_memory(|m| m.r32(0x3010_0014)).unwrap(), 14);
    }
}

#[cfg(test)]
mod independent_lifecycle_review {
    use super::*;
    #[test]
    fn verifier_deleted_raw_resources_are_not_discoverable() {
        crate::factory::configure(None, None);
        let parent = crate::factory::game_root().unwrap();
        let child = crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind: 1,
            flags: 0,
            parent: Some(parent),
            template: None,
            name: "review-child",
        })
        .unwrap();
        let (parent_raw, child_raw, id) = with_world_memory(|m| {
            let raw = raw_resource(m, child).unwrap();
            let parent_raw = raw_resource(m, parent).unwrap();
            (parent_raw, raw, m.r32(raw + 0x50))
        })
        .unwrap();
        crate::factory::destroy_resource(child).unwrap();
        assert!(slr_resource::node::with_res(|host| host
            .container(child)
            .is_none()));
        with_world_memory(|m| {
            let found = crate::native_resource_index::find(m, parent_raw, id);
            assert_eq!(
                found, 0,
                "destroyed id {id:#x} raw={child_raw:#x} remains in original resource tree"
            );
        })
        .unwrap();
    }
}

#[cfg(test)]
mod independent_followup_review {
    use super::*;
    fn create(parent: u32, kind: i32, name: &str) -> u32 {
        crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind,
            flags: 0,
            parent: Some(parent),
            template: None,
            name,
        })
        .unwrap()
    }
    #[test]
    fn verifier_retired_id_reuse_only_finds_new_raw_identity() {
        crate::factory::configure(None, None);
        let parent = crate::factory::game_root().unwrap();
        let victim = create(parent, 1, "victim");
        let (parent_raw, old_raw, old_id) = with_world_memory(|m| {
            let raw = raw_resource(m, victim).unwrap();
            (raw_resource(m, parent).unwrap(), raw, m.r32(raw + 0x50))
        })
        .unwrap();
        crate::factory::destroy_resource(victim).unwrap();
        assert_eq!(logical_resource(old_raw), None);
        let replacement = create(parent, 1, "replacement");
        let new_id = slr_resource::node::with_res(|h| h.container(replacement).unwrap().id);
        assert_eq!(new_id, old_id, "test requires actual recycled identity");
        with_world_memory(|m| {
            let raw = raw_resource(m, replacement).unwrap();
            assert_ne!(raw, old_raw);
            assert_eq!(
                crate::native_resource_index::find(m, parent_raw, new_id),
                raw
            );
        })
        .unwrap();
    }
    #[test]
    fn verifier_failed_raw_materialization_cannot_become_cached_success() {
        crate::factory::configure(None, None);
        crate::RES.with(|r| *r.borrow_mut() = crate::Resources::default());
        let resource = slr_resource::node::with_res(|h| {
            h.add_container(slr_resource::node::Container::new(5, 0x90012))
        });
        with_world_memory(|m| {
            let first = raw_resource(m, resource);
            assert!(first.is_err(), "missing descriptor should fail");
            let second = raw_resource(m, resource);
            assert!(
                second.is_err(),
                "failed materialization became cached success: {second:?}"
            );
        })
        .unwrap();
    }
    #[test]
    fn verifier_forced_script_unload_cannot_hide_payload_destructor_boundary() {
        crate::factory::configure(None, None);
        let root = crate::factory::game_root().unwrap();
        let parent = create(root, 1, "parent");
        let texture = create(root, 13, "texture");
        let asset = create(parent, 14, "loaded-asset");
        slr_resource::node::with_res(|h| h.container_mut(parent).unwrap().load_state = 1);
        with_world_memory(|m| {
            let ar = raw_resource(m, asset).unwrap();
            let tr = raw_resource(m, texture).unwrap();
            crate::native_part::bind_reference(m, ar + 0x8c, tr);
            crate::native_assets::synchronize_bindings(m, ar).unwrap();
        })
        .unwrap();
        assert!(crate::native_assets::owns_payload(asset));
        let result = crate::factory::unload_resource(parent);
        assert!(result.is_err(),"forced unload silently retired payload needing original destructor; owns_payload={} host_resource_alive={}", crate::native_assets::owns_payload(asset),slr_resource::node::with_res(|h|h.container(asset).is_some()));
    }
}

#[cfg(test)]
mod independent_flag_projection_review {
    use super::*;
    #[test]
    fn verifier_host_native_flags_reach_next_raw_native_operation() {
        crate::factory::configure(None, None);
        let parent = crate::factory::game_root().unwrap();
        let child = crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind: 1,
            flags: 0,
            parent: Some(parent),
            template: None,
            name: "flags-child",
        })
        .unwrap();
        let raw = with_world_memory(|m| raw_resource(m, child).unwrap()).unwrap();
        let flag = slr_resource::node::FLAG_ACTIVE_COLLISION;
        slr_resource::node::with_res(|h| {
            let id = h.container(child).unwrap().id;
            let link = h.add_bound_node(id, child);
            h.bind_object(0x7777, link);
            let row = slr_resource::node::GAME_REF_BINDINGS
                .iter()
                .find(|r| r.impl_addr == slr_resource::node::SET_FLAGS)
                .unwrap();
            h.call(row, &[0x7777, flag]);
            assert_ne!(h.container(child).unwrap().flags & flag, 0);
        });
        with_world_memory(|m| {
            assert_ne!(
                m.r32(raw + 0x54) & flag,
                0,
                "VM GameRef.setFlags changes never reached shared native memory"
            )
        })
        .unwrap();
    }
}

#[cfg(test)]
mod independent_nested_publication_review {
    use super::*;
    fn make() -> (u32, u32) {
        crate::factory::configure(None, None);
        let parent = crate::factory::game_root().unwrap();
        let child = crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind: 1,
            flags: 0,
            parent: Some(parent),
            template: None,
            name: "nested-flags",
        })
        .unwrap();
        let raw = with_world_memory(|m| raw_resource(m, child).unwrap()).unwrap();
        (child, raw)
    }
    fn change_flags(child: u32, operation: u32, bits: u32) {
        slr_resource::node::with_res(|h| {
            let id = h.container(child).unwrap().id;
            let link = h.add_bound_node(id, child);
            h.bind_object(0x7777, link);
            let row = slr_resource::node::GAME_REF_BINDINGS
                .iter()
                .find(|r| r.impl_addr == operation)
                .unwrap();
            h.call(row, &[0x7777, bits]);
        });
    }
    fn host_flags(child: u32) -> u32 {
        slr_resource::node::with_res(|h| h.container(child).unwrap().flags)
    }
    #[test]
    fn verifier_two_way_flags_survive_nested_callback_error() {
        let (child, raw) = make();
        with_world_memory(|m| {
            m.w32(raw + 0x54, 0x1000);
            let status = during_vm_callback(m, || {
                assert_eq!(host_flags(child), 0x1000);
                change_flags(child, slr_resource::node::SET_FLAGS, 0x2000);
                with_world_memory(|inner| {
                    assert_eq!(inner.r32(raw + 0x54), 0x3000);
                    inner.w32(raw + 0x54, 0x4000);
                    during_vm_callback(inner, || {
                        assert_eq!(host_flags(child), 0x4000);
                        change_flags(child, slr_resource::node::CLEAR_FLAGS, 0x4000);
                        change_flags(child, slr_resource::node::SET_FLAGS, 0x8000);
                        Err::<(), String>("expected script stop".into())
                    })
                })?
            });
            assert_eq!(status.unwrap_err(), "expected script stop");
            assert_eq!(m.r32(raw + 0x54), 0x8000);
        })
        .unwrap();
        assert_eq!(host_flags(child), 0x8000);
    }
    #[test]
    fn verifier_native_flag_mutation_without_callback_reaches_host() {
        let (child, raw) = make();
        with_world_memory(|m| m.w32(raw + 0x54, 0x400000)).unwrap();
        assert_eq!(host_flags(child), 0x400000);
        change_flags(child, slr_resource::node::CLEAR_FLAGS, 0x400000);
        with_world_memory(|m| assert_eq!(m.r32(raw + 0x54), 0)).unwrap();
    }
    #[test]
    fn verifier_callback_destruction_retires_raw_graph_without_host_reborrow() {
        let (child, raw) = make();
        with_world_memory(|m| {
            let link = m.alloc(16);
            crate::native_part::bind_reference(m, link, raw);
            during_vm_callback(m, || crate::factory::destroy_resource(child)).unwrap();
            assert_eq!(m.r32(link + 12), 0);
            assert_eq!(logical_resource(raw), None);
        })
        .unwrap();
        assert!(slr_resource::node::with_res(|h| h
            .container(child)
            .is_none()));
    }

    #[test]
    fn raw_callback_weak_refs_do_not_follow_recycled_resource_ids() {
        let (child, raw) = make();
        let id = slr_resource::node::with_res(|h| h.container(child).unwrap().id);
        with_world_memory(|m| {
            let context = crate::native_callbacks::CONTEXT;
            crate::native_callbacks::register(m, context, raw, 9);
            let old = crate::native_install::first(m, context + 0x1c);
            let replacement = during_vm_callback(m, || {
                crate::factory::destroy_resource(child)?;
                let parent = crate::factory::game_root()?;
                crate::factory::create_resource(crate::factory::ResourceDescriptor {
                    kind: 1,
                    flags: 0,
                    parent: Some(parent),
                    template: None,
                    name: "recycled-callback-target",
                })
            })
            .unwrap();
            assert_eq!(m.r32(old + 20), 0);
            assert_eq!(m.r32(old + 24), 0);
            assert_eq!(
                slr_resource::node::with_res(|h| h.container(replacement).unwrap().id),
                id
            );
            let new_raw = raw_resource(m, replacement).unwrap();
            assert_ne!(new_raw, raw);
            crate::native_callbacks::register(m, context, new_raw, 9);
            let mut observed = vec![];
            let retired = crate::native_callbacks::walk(m, context, 9, |m, reference| {
                observed.push(m.r32(reference + 12));
                Ok(())
            })
            .unwrap();
            assert_eq!(observed, vec![new_raw]);
            assert_eq!(retired, vec![old]);
            assert_eq!(m.r32(context + 0xc8), 1);
        })
        .unwrap();
    }
}

#[cfg(test)]
mod native_destructor_boundary_tests {
    use super::*;
    #[test]
    fn configured_runtime_destroy_requires_original_native_destructor_before_retirement() {
        crate::factory::configure(None, None);
        let root = crate::factory::game_root().unwrap();
        let resource = crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind: 1,
            flags: 0,
            parent: Some(root),
            template: None,
            name: "native-destructor-boundary",
        })
        .unwrap();
        let mut state = NativeBody::acquire(resource, 0).unwrap();
        state.body = state.memory.alloc(0x11c);
        insert(state);
        let result = crate::factory::destroy_resource(resource);
        assert!(result.unwrap_err().contains("native class destructor"));
        assert!(has_native_body(resource));
        assert!(slr_resource::node::with_res(|host| host
            .container(resource)
            .is_some()));
    }
}

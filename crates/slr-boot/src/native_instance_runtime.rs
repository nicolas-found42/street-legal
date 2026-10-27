//! Live collaborators for53a2a0. The manager's preallocated logical identity
//! remains an explicit host boundary; internal caller/constructor frames run
//! through their source kernels. Upper481a70/VM caller scratch is unclaimed.
use crate::{factory, native_body, native_instance, native_stack as s};
use slr_physics::space::Space;
use slr_vm::host::VmHost;
use std::collections::HashMap;

struct Runtime<'a> {
    vm: &'a mut VmHost,
    container: u32,
    raw: u32,
    template: u32,
    id: u32,
    allocated: bool,
    created: bool,
    bindings: HashMap<u32, factory::CfgBinding>,
}
impl Drop for Runtime<'_> {
    fn drop(&mut self) {
        for (_, binding) in self.bindings.drain() {
            factory::release_cfg(binding);
        }
    }
}
impl native_instance::Host for Runtime<'_> {
    fn call(
        &mut self,
        m: &mut Space,
        address: u32,
        args: &[u32],
        this: Option<u32>,
    ) -> Result<u32, String> {
        if address == 0x4797d0 {
            let text = if args[1] == 0 {
                String::new()
            } else {
                crate::engine::read_event_text(m, args[1])
                    .ok_or("NativeInstanceBoundary: unreadable constructor parameters")?
            };
            let mut host =
                crate::native_player_runtime::Runtime::new(self.vm, m.r32(self.template + 0xc));
            let result =
                crate::native_player::construct(m, self.template, args[0], &text, &mut host);
            self.bindings.extend(host.take_bindings());
            return result;
        }
        if address == 0x45fdf0 {
            let mut host = crate::native_cursor_runtime::Runtime {
                vm: self.vm,
                template_id: m.r32(self.template + 0xc),
            };
            return crate::native_cursor::construct(m, self.template, args[0], &mut host);
        }
        if address == 0x427210 {
            crate::native_query_frame::register_runtime(m, self.vm, this.unwrap(), args[0])?;
            return Ok(s::reg(m, s::Reg::Eax));
        }
        s::function(m, args, this, |m| {
            let object = this.unwrap_or(0);
            match address {
                0x536ed0 => {
                    if self.allocated {
                        return Err(
                            "NativeInstanceBoundary: repeated preallocated ID request".into()
                        );
                    }
                    self.allocated = true;
                    Ok(self.id)
                }
                0x536890 | 0x536900 => {
                    if !self.allocated || self.created || args[0] != self.id {
                        return Err(
                            "NativeInstanceBoundary: preallocated manager transition differs"
                                .into(),
                        );
                    }
                    let descriptor = args[2];
                    if m.r32(descriptor) != 1
                        || m.r32(descriptor + 4) != self.id
                        || m.r32(descriptor + 8) != 0x21
                    {
                        return Err("NativeInstanceBoundary: type1 descriptor differs".into());
                    }
                    if address == 0x536900 && m.r32(self.raw + 0x14) != args[3] {
                        return Err("NativeInstanceBoundary: preallocated parent differs".into());
                    }
                    self.created = true;
                    Ok(self.raw)
                }
                0x54f560 => Ok(m.malloc(args[0])),
                0x54f5b0 => Ok(0),
                0x544b90 => {
                    crate::native_resource_load::mark_loaded(m, object, args[0])?;
                    crate::native_assets::synchronize_loaded(m, object)?;
                    Ok(0)
                }
                0x419860 => crate::native_assets::access(
                    m,
                    object,
                    crate::native_resource_load::Access {
                        flags: args[0],
                        quality: args[1],
                        priority: args[2],
                        time: args[3],
                    },
                ),
                0x5447d0 => crate::native_assets::prepare(
                    m,
                    object,
                    crate::native_resource_load::Access {
                        flags: args[0],
                        quality: 0,
                        priority: args[1],
                        time: args[2],
                    },
                ),
                0x4988a0 => {
                    crate::native_spatial::aggregate_bounds(m, args[0])?;
                    Ok(0)
                }
                0x498b10 => Ok(u32::from(crate::native_spatial::relocate(
                    m,
                    args[0],
                    &mut Spatial,
                )?)),
                0x48ae40 => {
                    let raw = m.r32(object + 12);
                    if raw == 0 {
                        return Ok(0);
                    }
                    let data = crate::native_assets::access(
                        m,
                        raw,
                        crate::native_resource_load::Access {
                            flags: 0xa0000000,
                            quality: 0,
                            priority: 0,
                            time: 0,
                        },
                    )?;
                    Ok(
                        if data != 0 && m.r32(data + 0x40) == 0x35 && m.r32(data + 0x4c) != 0 {
                            m.r32(data + 0x4c) + 0xb4
                        } else {
                            0
                        },
                    )
                }
                //429f80 is the original default postcreate: RET4.
                0x429f80 => Ok(s::reg(m, s::Reg::Eax)),
                _ => {
                    let table = m.r32(object);
                    if object != 0 && address == m.r32(table + 0xc) {
                        return crate::native_assets::data(m, object);
                    }
                    if object != 0 && address == m.r32(table + 0x14) {
                        crate::native_resource_load::AccessHost::set_minimum(
                            &mut crate::native_assets::AccessAdapter,
                            m,
                            object,
                            args[0],
                        )?;
                        return Ok(s::reg(m, s::Reg::Eax));
                    }
                    if object != 0 && address == m.r32(table + 0x18) && m.r32(object + 0x4c) == 1 {
                        let frame = m.r32(0x6200a4);
                        let value = slr_x87::X87::from_f32(f32::from_bits(args[0]))
                            .mul(
                                slr_x87::X87::from_f32(m.rf(object + 0xbc)),
                                slr_x87::Precision::F32,
                                slr_x87::Rounding::NearestEven,
                            )
                            .add(
                                slr_x87::X87::from_f32(m.rf(object + 0xb8)),
                                slr_x87::Precision::F32,
                                slr_x87::Rounding::NearestEven,
                            )
                            .to_f32(slr_x87::Precision::F32, slr_x87::Rounding::NearestEven);
                        if m.r32(object + 0x7c) < frame || value > m.rf(object + 0x70) {
                            m.wf(object + 0x70, value);
                        }
                        m.w32(object + 0x7c, frame);
                        return Ok(s::reg(m, s::Reg::Eax));
                    }
                    Err(format!("NativeInstanceBoundary: unported caller collaborator{address:#x} this{object:#x} container{:#x}",self.container))
                }
            }
        })
    }
}
struct Spatial;
impl crate::native_spatial::IndexHost for Spatial {
    fn access(&mut self, m: &mut Space, raw: u32, flags: u32) -> Result<u32, String> {
        crate::native_assets::prepare(
            m,
            raw,
            crate::native_resource_load::Access {
                flags,
                quality: 0,
                priority: 0,
                time: 0,
            },
        )
    }
}
fn text(m: &mut Space, text: &str) -> u32 {
    let p = m.alloc(text.len() as u32 + 1);
    for (i, b) in text.bytes().chain([0]).enumerate() {
        m.w8(p + i as u32, b);
    }
    p
}

/// Top-level host adapter. Arguments are declared live driver inputs; the
/// actual temporary Ref and constructor pending CALL belong exclusively to
///53a2a0. Active callers must use create_in with their prepared CALL instead.
pub fn begin(
    vm: &mut VmHost,
    state: native_body::NativeBody,
    container: u32,
    parameters: &str,
    name: &str,
) -> Result<(), String> {
    begin_driver(vm, state, container, parameters, name, false)
}
/// Native-only53a5e0 route. A nonzero raw script class remains explicit until
/// the VM bridge supplies actual class/object/pool ownership.
pub fn begin_new(
    vm: &mut VmHost,
    state: native_body::NativeBody,
    container: u32,
    parameters: &str,
    name: &str,
) -> Result<(), String> {
    begin_driver(vm, state, container, parameters, name, true)
}
fn begin_driver(
    vm: &mut VmHost,
    mut state: native_body::NativeBody,
    container: u32,
    parameters: &str,
    name: &str,
    new: bool,
) -> Result<(), String> {
    if s::active(&state.memory) {
        return Err(
            "NativeInstanceBoundary: active caller must supply original pending53a2a0 CALL".into(),
        );
    }
    state.container = container;
    let template_container = factory::instance_template_container(container)
        .ok_or("NativeInstanceBoundary: missing template identity")?;
    let raw = native_body::raw_resource(&mut state.memory, container)?;
    let secondary = native_body::raw_resource(&mut state.memory, template_container)?;
    if new && state.memory.r32(secondary + 0x90) != 0 {
        return Err("NativeInstanceNewBoundary: nonzero script class requires original VM metadata ownership".into());
    }
    let owner = state.memory.r32(raw + 0x14);
    let owner_reference = state.memory.alloc(16);
    let request = state.memory.alloc(16);
    crate::native_part::bind_reference(&mut state.memory, owner_reference, owner);
    crate::native_part::bind_reference(&mut state.memory, request, secondary);
    let parameters = text(&mut state.memory, parameters);
    let name = text(&mut state.memory, name);
    let script = slr_resource::node::with_res(|h| {
        h.container(container)
            .and_then(|c| c.instance.as_ref())
            .map(|i| i.script)
    })
    .ok_or("NativeInstanceBoundary: missing script envelope")?;
    let result = if new {
        create_new_in(
            &mut state.memory,
            vm,
            state.template,
            container,
            [owner_reference, request, parameters, name],
        )
    } else {
        create_in(
            &mut state.memory,
            vm,
            state.template,
            container,
            [owner_reference, request, script, parameters, name],
        )
    };
    crate::native_part::bind_reference(&mut state.memory, request, 0);
    crate::native_part::bind_reference(&mut state.memory, owner_reference, 0);
    let bindings = result?;
    state.body = state.memory.r32(raw + 0x124);
    let (template_bindings, body_bindings) =
        bindings
            .into_iter()
            .partition::<HashMap<_, _>, _>(|(slot, _)| {
                *slot >= state.template && *slot < state.template + 0x50
            });
    state.resource_bindings.extend(body_bindings);
    native_body::retain_template_bindings(template_container, template_bindings)?;
    if state.memory.r32(state.template) == 0x5f11e4 {
        for offset in [0x2c, 0x3c] {
            let slot = state.body + offset;
            let id = state.memory.r32(slot + 8);
            state.bind_resource(slot, id, 0, 0)?;
        }
    }
    let player = state.memory.r32(state.template) == crate::native_player::VTABLE;
    native_body::insert(state);
    if player {
        crate::engine::player_created(container);
    }
    Ok(())
}

pub fn create_in(
    m: &mut Space,
    vm: &mut VmHost,
    template: u32,
    container: u32,
    arguments: [u32; 5],
) -> Result<HashMap<u32, factory::CfgBinding>, String> {
    let raw = native_body::raw_resource(m, container)?;
    let id = m.r32(raw + 0x50);
    let mut host = Runtime {
        vm,
        container,
        raw,
        template,
        id,
        allocated: false,
        created: false,
        bindings: HashMap::new(),
    };
    let result = native_instance::create(m, arguments, &mut host);
    if result.as_ref().is_ok_and(|value| *value != raw) {
        return Err(
            "NativeInstanceBoundary: source caller returned unexpected raw identity".into(),
        );
    }
    result?;
    native_body::synchronize_topology(m)?;
    Ok(std::mem::take(&mut host.bindings))
}

pub fn create_new_in(
    m: &mut Space,
    vm: &mut VmHost,
    template: u32,
    container: u32,
    arguments: [u32; 4],
) -> Result<HashMap<u32, factory::CfgBinding>, String> {
    let raw = native_body::raw_resource(m, container)?;
    let secondary = m.r32(arguments[1] + 12);
    if secondary == 0 || m.r32(secondary + 0x90) != 0 {
        return Err(
            "NativeInstanceNewBoundary: live native-only template needs zero original script class"
                .into(),
        );
    }
    let id = m.r32(raw + 0x50);
    let mut host = Runtime {
        vm,
        container,
        raw,
        template,
        id,
        allocated: false,
        created: false,
        bindings: HashMap::new(),
    };
    let result = crate::native_instance_new::create(m, arguments, &mut host)?;
    if result != raw {
        return Err(
            "NativeInstanceNewBoundary: source caller returned unexpected raw identity".into(),
        );
    }
    native_body::synchronize_topology(m)?;
    Ok(std::mem::take(&mut host.bindings))
}

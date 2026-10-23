//! Production collaborators for the recovered part/car installation graph.
//! VM object identities and fields cross an explicit host boundary; native
//! resources and bodies continue to use the shared original memory layouts.
use crate::{engine, native_assets, native_body, native_install, native_part, native_query};
use slr_physics::space::Space;
use slr_resource::node::with_res;
use slr_vm::{host::VmHost, values::Value, vmclass::ClassId};
use std::collections::HashMap;

pub struct Runtime<'a> {
    vm: &'a mut VmHost,
    objects: HashMap<u32, u32>,
    fields: Vec<(u32, u32, String)>,
}
impl<'a> Runtime<'a> {
    pub(crate) fn new(vm: &'a mut VmHost) -> Self {
        Self {
            vm,
            objects: HashMap::new(),
            fields: vec![],
        }
    }
    fn callback<R>(
        &mut self,
        m: &mut Space,
        f: impl FnOnce(&mut VmHost) -> Result<R, String>,
    ) -> Result<R, String> {
        native_body::during_vm_callback(m, || f(self.vm))
    }
    pub(crate) fn script_target(m: &mut Space, envelope: u32) -> Result<Option<u32>, String> {
        crate::native_script_access::target(m, envelope, |m, secondary| {
            native_assets::access(
                m,
                secondary,
                crate::native_resource_load::Access {
                    flags: 0x8000_0000,
                    quality: 1f32.to_bits(),
                    priority: 0,
                    time: 0,
                },
            )
        })
    }
    fn synchronize_fields(&mut self, m: &Space) -> Result<(), String> {
        for (object, cell, name) in &self.fields {
            if name != "ptr" {
                return Err(format!("NativeVmBoundary: field synchronization {name}"));
            }
            let raw_link = m.r32(*cell + 8);
            let raw = if raw_link == 0 {
                0
            } else {
                m.r32(raw_link + 12)
            };
            let logical = if raw == 0 {
                0
            } else {
                native_body::logical_resource(raw)
                    .ok_or("NativeVmBoundary: field refers to unknown resource")?
            };
            let node = with_res(|host| {
                let existing = host.object_node(*object);
                if existing != 0 {
                    host.bind_created(existing, logical)?;
                    return Ok::<_, String>(existing);
                }
                let id = host.container(logical).map_or(0, |r| r.id);
                let node = host.add_bound_node(id, logical);
                host.bind_object(*object, node);
                Ok(node)
            })?;
            self.vm
                .write_field(*object, name, Value::int(node as i32))
                .map_err(|e| format!("NativeVmBoundary: {name}: {e:?}"))?;
        }
        Ok(())
    }
}
impl native_install::Host for Runtime<'_> {
    fn vm_reference_native(
        &mut self,
        _m: &mut Space,
        object: u32,
        name: &str,
        descriptor: &str,
    ) -> Result<Option<u32>, String> {
        use slr_vm::vmclass::ClassRef;
        let Some(ClassRef::Loaded(class)) = object
            .checked_sub(0x1000)
            .and_then(|i| self.vm.objects.get(i as usize))
            .map(|o| o.class.clone())
        else {
            return Ok(None);
        };
        let fields = self
            .vm
            .instance_fields_id(class)
            .map_err(|e| format!("NativeVmBoundary: field metadata {e:?}"))?;
        let Some(field) = fields.iter().find(|f| f.name == name) else {
            return Ok(None);
        };
        let Some(ty) = field.ty.strip_prefix('L').and_then(|s| s.strip_suffix(';')) else {
            return Ok(None);
        };
        if !self
            .vm
            .loader
            .is_inherited_from(ty, descriptor)
            .unwrap_or(false)
        {
            return Ok(None);
        }
        let value = self
            .vm
            .read_field(object, name)
            .map_err(|e| format!("NativeVmBoundary: reference field {e:?}"))?;
        let Some(ClassRef::Loaded(target)) = value
            .payload
            .checked_sub(0x1000)
            .and_then(|i| self.vm.objects.get(i as usize))
            .map(|o| o.class.clone())
        else {
            return Ok(None);
        };
        let Some(name) = self.vm.loader.class_name(target) else {
            return Ok(None);
        };
        if !self
            .vm
            .loader
            .is_inherited_from(name, "java.lang.Native")
            .unwrap_or(false)
        {
            return Ok(None);
        }
        self.vm
            .read_field(value.payload, "ptr")
            .map(|v| Some(v.payload))
            .map_err(|e| format!("NativeVmBoundary: Native.ptr {e:?}"))
    }
    fn rand(&mut self, _m: &mut Space) -> Result<u32, String> {
        Ok(slr_lang::seam::with_lang_host(|host| {
            u32::from(host.rng.next_u15())
        }))
    }
    fn minimum(&mut self, m: &mut Space, resource: u32, quality: u32) -> Result<(), String> {
        crate::native_resource_load::AccessHost::set_minimum(
            &mut native_assets::AccessAdapter,
            m,
            resource,
            quality,
        )
    }
    fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
        native_assets::prepare(
            m,
            resource,
            crate::native_resource_load::Access {
                flags,
                quality: 0,
                priority: 0,
                time: 0,
            },
        )
    }
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
        native_assets::data(m, resource)
    }
    fn resolve(&mut self, m: &mut Space, id: u32, kind: u32, flags: u32) -> Result<u32, String> {
        let binding = crate::factory::bind_cfg(0, id, kind as i32, flags)?;
        let result = native_body::raw_resource(m, binding.container);
        crate::factory::release_cfg(binding);
        result
    }
    fn query(
        &mut self,
        m: &mut Space,
        reference: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        native_query::query(
            &mut native_install::QueryRouter { m, host: self },
            reference,
            message,
            argument,
        )
    }
    fn script_query(&mut self, m: &mut Space, envelope: u32, message: u32) -> Result<u32, String> {
        let Some(script) = Self::script_target(m, envelope)? else {
            return Ok(0);
        };
        let container = native_body::logical_resource(envelope - 0xd8)
            .ok_or("NativeVmBoundary: script query envelope missing identity")?;
        self.callback(m, |vm| engine::query_script(vm, container, script, message))
    }
    fn command(
        &mut self,
        m: &mut Space,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        name: u32,
    ) -> Result<(), String> {
        native_install::route_command(m, self, reference, source, event, argument, name)
    }
    fn deliver(
        &mut self,
        m: &mut Space,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        envelope: u32,
        name: u32,
    ) -> Result<(), String> {
        let script = m.r32(envelope + 0x50);
        if script == 0 {
            return Ok(());
        }
        if event == 0x0010_0000 {
            //425e16 uses argument as an object, ignoring reference/source/name.
            // Native-created objects retain their original address in this map;
            // events entering from the VM already carry a validated host word.
            let object = self.objects.get(&argument).copied().unwrap_or(argument);
            self.synchronize_fields(m)?;
            return self.callback(m, |vm| {
                engine::deliver_script_to(
                    vm,
                    script,
                    0,
                    None,
                    event,
                    &engine::EventArg::Int(object as i32),
                    None,
                )
            });
        }

        let container = native_body::logical_resource(m.r32(reference + 12))
            .ok_or("NativeVmBoundary: event reference lacks identity")?;
        let source = if source == 0 {
            None
        } else {
            Some(
                native_body::logical_resource(m.r32(source + 12))
                    .ok_or("NativeVmBoundary: source lacks identity")?,
            )
        };
        let arg = if event & 0x0fff_ffff == 0x80 {
            engine::EventArg::Int(argument as i32)
        } else {
            engine::EventArg::Str(engine::read_event_text(m, argument))
        };
        let name = engine::read_event_text(m, name);
        self.callback(m, |vm| {
            engine::deliver_script_to(vm, script, container, source, event, &arg, name.as_deref())
        })
    }
    fn script(
        &mut self,
        m: &mut Space,
        envelope: u32,
        method: &str,
        words: &[u32],
    ) -> Result<(), String> {
        self.synchronize_fields(m)?;
        let Some(script) = Self::script_target(m, envelope)? else {
            return Ok(());
        };
        let args = match words {
            [0x4f, 0x4f] => vec![],
            [0, 4, object, 0x4f] => {
                let object = *self
                    .objects
                    .get(object)
                    .ok_or("NativeVmBoundary: unknown callback object")?;
                vec![Value::reference_named(
                    object,
                    "Ljava.util.resource.GameRef;",
                )]
            }
            _ => {
                return Err(format!(
                    "NativeVmBoundary: callback argument types {words:x?}"
                ))
            }
        };
        self.callback(m, |vm| engine::call_script(vm, script, method, &args))
    }
    fn vm_class(&mut self, _m: &mut Space, name: &str) -> Result<u32, String> {
        self.vm
            .loader
            .request(name)
            .map_err(|e| format!("NativeVmBoundary: class lookup {e:?}"))?
            .map(ClassId::get)
            .ok_or_else(|| format!("NativeVmBoundary: missing class {name}"))
    }
    fn vm_object(&mut self, m: &mut Space, object: u32, class: u32) -> Result<(), String> {
        let class = ClassId::from_raw(class).ok_or("NativeVmBoundary: invalid class identity")?;
        let word = self.callback(m, |vm| {
            vm.alloc_object_id(class)
                .map_err(|e| format!("NativeVmBoundary: object construction {e:?}"))
        })?;
        self.objects.insert(object, word);
        Ok(())
    }
    fn vm_field(&mut self, m: &mut Space, object: u32, name: &str) -> Result<u32, String> {
        let object = *self
            .objects
            .get(&object)
            .ok_or("NativeVmBoundary: unknown object field")?;
        self.vm
            .read_field(object, name)
            .map_err(|e| format!("NativeVmBoundary: field lookup {e:?}"))?;
        // VM cells live in VmHost, outside the native allocator. This temporary
        // address carries the original cell store across the host boundary.
        let cell = native_install::local_reference(m);
        self.fields.push((object, cell, name.into()));
        Ok(cell)
    }
}

/// Constructor-time426470 uses the same live VM and raw-memory callback
/// protocol as installation, before the new script object exists.
pub(crate) fn query_reference(
    m: &mut Space,
    vm: &mut VmHost,
    reference: u32,
    message: u32,
    argument: u32,
) -> Result<u32, String> {
    let mut host = Runtime::new(vm);
    native_query::query(
        &mut native_install::QueryRouter { m, host: &mut host },
        reference,
        message,
        argument,
    )
}

pub fn dispatch(
    vm: &mut VmHost,
    container: u32,
    source: Option<u32>,
    event: u32,
    arg: &engine::EventArg,
    name: Option<&str>,
) -> Result<(), String> {
    if (event == 0x0010_0000 || event & 0x0fff_ffff == 0x80)
        && matches!(arg, engine::EventArg::Str(Some(_)))
    {
        return Err(
            "NativeVmBoundary: non-text event cannot use an allocated text address as its argument"
                .into(),
        );
    }
    native_body::with_world_memory(|m| {
        let raw = native_body::raw_resource(m, container)?;
        let reference = native_install::local_reference(m);
        native_part::bind_reference(m, reference, raw);
        let source_ref = native_install::local_reference(m);
        if let Some(source) = source {
            let source = native_body::raw_resource(m, source)?;
            native_part::bind_reference(m, source_ref, source);
        }
        let argument = match arg {
            engine::EventArg::Int(value) => *value as u32,
            engine::EventArg::Str(Some(text)) => native_install::local_text(m, text),
            engine::EventArg::Str(None) => 0,
        };
        let name = name.map_or(0, |s| native_install::local_text(m, s));
        let result = native_install::route_command(
            m,
            &mut Runtime::new(vm),
            reference,
            if source.is_some() { source_ref } else { 0 },
            event,
            argument,
            name,
        );
        let topology = native_body::synchronize_topology(m);
        native_part::bind_reference(m, reference, 0);
        native_part::bind_reference(m, source_ref, 0);
        result.and(topology)
    })?
}

#[cfg(test)]
mod object_event_tests {
    use super::*;
    use crate::native_install::Host;

    #[test]
    fn object_event_translates_proxy_and_bypasses_irrelevant_native_text_and_references() {
        for use_proxy in [false, true] {
            let (mut vm, script, _) = crate::engine::event_argument_tests::setup(true);
            let object = vm.alloc_object("java.lang.Object").unwrap();
            let before = vm.objects.len();
            native_body::with_world_memory(|m| {
                let envelope = m.alloc(0x100);
                m.w32(envelope + 0x50, script);
                let mut runtime = Runtime::new(&mut vm);
                let argument = if use_proxy {
                    runtime.objects.insert(0x300e0000, object);
                    0x300e0000
                } else {
                    object
                };
                runtime
                    .deliver(
                        m, 0xdeadbeef, 0xcafebabe, 0x100000, argument, envelope, 0xfefefefe,
                    )
                    .unwrap();
            })
            .unwrap();
            let calls = crate::engine::event_argument_tests::take_calls();
            assert_eq!(
                calls,
                vec![(
                    "handleEvent".into(),
                    "(Ljava.lang.Object;)V".into(),
                    vec![script, object]
                )]
            );
            assert_eq!(vm.objects.len(), before);
        }
    }

    #[test]
    fn object_event_unknown_identity_fails_without_string_conversion() {
        let (mut vm, script, _) = crate::engine::event_argument_tests::setup(false);
        let before = vm.objects.len();
        native_body::with_world_memory(|m| {
            let envelope = m.alloc(0x100);
            m.w32(envelope + 0x50, script);
            let mut runtime = Runtime::new(&mut vm);
            assert!(runtime
                .deliver(m, 0, 0, 0x100000, 0x300e0000, envelope, 0)
                .unwrap_err()
                .contains("unknown VM identity"));
        })
        .unwrap();
        assert!(crate::engine::event_argument_tests::take_calls().is_empty());
        assert_eq!(vm.objects.len(), before);
    }
}

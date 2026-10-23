//! Live resource/Config/registry collaborators for the original player class.
use crate::{factory, native_body, native_part, native_player};
use slr_physics::space::Space;
use slr_vm::{host::VmHost, values::TypeTag};
use std::collections::HashMap;

pub struct Runtime<'a> {
    vm: &'a mut VmHost,
    template_id: u32,
    bindings: HashMap<u32, factory::CfgBinding>,
}
impl<'a> Runtime<'a> {
    pub fn new(vm: &'a mut VmHost, template_id: u32) -> Self {
        Self {
            vm,
            template_id,
            bindings: HashMap::new(),
        }
    }
    pub fn take_bindings(&mut self) -> HashMap<u32, factory::CfgBinding> {
        std::mem::take(&mut self.bindings)
    }
    fn text(m: &Space, pointer: u32) -> Result<String, String> {
        crate::engine::read_event_text(m, pointer)
            .ok_or_else(|| "NativePlayerBoundary: missing native metadata string".into())
    }
    fn setting(&mut self, m: &Space, key: u32, tag: TypeTag) -> Result<u32, String> {
        let name = Self::text(m, key)?;
        let config = crate::gc_runtime::config_script()
            .ok_or("NativePlayerBoundary: Config anchor63643c unavailable")?;
        let value = self
            .vm
            .read_field(config, &name)
            .map_err(|error| format!("NativePlayerBoundary: Config.{name}: {error:?}"))?;
        if value.tag != tag {
            return Err(format!(
                "NativePlayerBoundary: Config.{name} expected {tag:?}, found {:?}",
                value.tag
            ));
        }
        Ok(value.payload)
    }
}
impl Drop for Runtime<'_> {
    fn drop(&mut self) {
        for (_, binding) in self.bindings.drain() {
            factory::release_cfg(binding);
        }
    }
}
impl native_player::Host for Runtime<'_> {
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
        Ok(m.malloc(bytes))
    }
    fn name(&mut self, m: &mut Space, id: u32) -> Result<u32, String> {
        crate::native_cursor::Host::name(
            &mut crate::native_cursor_runtime::Runtime {
                vm: self.vm,
                template_id: self.template_id,
            },
            m,
            id,
        )
    }
    fn lookup(&mut self, m: &mut Space, name: u32, entry: u32) -> Result<u32, String> {
        factory::lookup_cfg(&Self::text(m, name)?, entry)
    }
    fn bind(
        &mut self,
        m: &mut Space,
        reference: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<(), String> {
        let binding = factory::bind_cfg(self.template_id, id, kind as i32, flags)?;
        let raw = match native_body::raw_resource(m, binding.container) {
            Ok(raw) => raw,
            Err(error) => {
                factory::release_cfg(binding);
                return Err(error);
            }
        };
        native_part::bind_reference(m, reference, raw);
        if let Some(old) = self.bindings.insert(reference, binding) {
            factory::release_cfg(old);
        }
        Ok(())
    }
    fn register(
        &mut self,
        m: &mut Space,
        class: u32,
        method: u32,
        signature: u32,
        address: u32,
    ) -> Result<(), String> {
        let (class, method, signature) = (
            Self::text(m, class)?,
            Self::text(m, method)?,
            Self::text(m, signature)?,
        );
        if !self.vm.native_impls.contains_key(&address) {
            return Err(format!("NativePlayerBoundary: missing native implementation {class}.{method}{signature} at{address:#x}"));
        }
        self.vm
            .natives
            .register(&class, &method, &signature, address);
        Ok(())
    }
    fn integer(&mut self, m: &mut Space, key: u32) -> Result<u32, String> {
        self.setting(m, key, TypeTag::Int)
    }
    fn float(&mut self, m: &mut Space, key: u32) -> Result<u32, String> {
        self.setting(m, key, TypeTag::Float)
    }
    fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String> {
        let raw = m.r32(reference + 12);
        if raw != 0 {
            let envelope = crate::native_factory::loaded_game_payload(m, raw)?;
            m.w32(envelope + 0x70, m.r32(envelope + 0x70) | mask);
        }
        Ok(())
    }
}

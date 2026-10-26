//! Live VM/resource collaborators for the stock cursor native class.
use crate::{factory, native_body, native_cursor, native_install, native_install_runtime};
use slr_physics::space::Space;
use slr_vm::host::VmHost;

pub struct Runtime<'a> {
    pub vm: &'a mut VmHost,
    pub template_id: u32,
}
impl native_cursor::Host for Runtime<'_> {
    fn cursor_backend(&mut self, m: &mut Space, operation: &str, value: u32) -> Result<(), String> {
        native_cursor::system_command(m, operation, value, &mut HeadlessDevice)
    }
    fn name(&mut self, m: &mut Space, id: u32) -> Result<u32, String> {
        let manager = m.r32(0x618d48);
        if manager == 0 {
            return Err("NativeCursorBoundary: original5383f0 resource manager missing".into());
        }
        if id >> 16 == 0xffff {
            return Ok(manager + 0xffe24);
        }
        let container = slr_resource::node::with_res(|h| h.container_for_id(id))
            .ok_or("NativeCursorBoundary: original5383f0 package resource missing")?;
        let raw = native_body::raw_resource(m, container)?;
        crate::native_resource_manager::Host::package(
            &mut crate::native_assets::AccessAdapter,
            m,
            raw,
        )
    }
    fn lookup(&mut self, _m: &mut Space, name: &str, kind: u32) -> Result<u32, String> {
        factory::lookup_cfg(name, kind)
    }
    fn resolve(&mut self, m: &mut Space, id: u32) -> Result<u32, String> {
        if id == 0 {
            return Ok(0);
        }
        let binding = factory::bind_cfg(self.template_id, id, 0, 0)?;
        let result = native_body::raw_resource(m, binding.container);
        factory::release_cfg(binding);
        result
    }
    fn sensitivity(&mut self, _m: &mut Space) -> Result<u32, String> {
        let object = crate::gc_runtime::config_script()
            .ok_or("NativeCursorBoundary: Config anchor63643c unavailable")?;
        let value = self
            .vm
            .read_field(object, "mouseSensitivity")
            .map_err(|e| format!("NativeCursorBoundary: Config.mouseSensitivity: {e:?}"))?;
        if value.tag != slr_vm::values::TypeTag::Float {
            return Err("NativeCursorBoundary: Config.mouseSensitivity is not float".into());
        }
        Ok(value.payload)
    }
    fn system_cursor(&mut self, _m: &mut Space) -> Result<Option<u32>, String> {
        let object = crate::gc_runtime::config_script()
            .ok_or("NativeCursorBoundary: Config anchor63643c unavailable")?;
        match self.vm.read_field(object, "SysCursor") {
            Err(slr_vm::host::HostError::MissingMember) => Ok(None),
            Err(e) => Err(format!("NativeCursorBoundary: Config.SysCursor: {e:?}")),
            Ok(value) if value.tag == slr_vm::values::TypeTag::Int => Ok(Some(value.payload)),
            Ok(_) => Err("NativeCursorBoundary: Config.SysCursor is not integer".into()),
        }
    }
    fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String> {
        let raw = m.r32(reference + 12);
        if raw != 0 {
            let data = crate::native_factory::loaded_game_payload(m, raw)?;
            m.w32(data + 0x70, m.r32(data + 0x70) | mask);
        }
        Ok(())
    }
    fn query(
        &mut self,
        m: &mut Space,
        reference: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        native_install_runtime::query_reference(m, self.vm, reference, message, argument)
    }
    fn pick(&mut self, m: &mut Space, arguments: [u32; 10]) -> Result<u32, String> {
        crate::native_cursor_pick::pick(m, arguments, &mut WorldRay)
    }
    fn place(
        &mut self,
        m: &mut Space,
        reference: u32,
        translation: u32,
        words: [u32; 9],
        _name: u32,
    ) -> Result<(), String> {
        let rotation = crate::native_stack::sp(m) + 0x290;
        for (i, word) in words.into_iter().enumerate() {
            m.w32(rotation + (i / 3) as u32 * 16 + (i % 3) as u32 * 4, word);
        }
        let mut host = native_install_runtime::Runtime::new(self.vm);
        crate::native_render_graph::named_bone_transform(
            &mut native_install::Install { m, host: &mut host },
            reference,
            "bone00",
            0,
            translation,
            rotation,
        )
    }
    fn command(
        &mut self,
        m: &mut Space,
        reference: u32,
        sender: u32,
        text: &str,
    ) -> Result<(), String> {
        let saved = crate::native_stack::sp(m);
        let pointer = native_install::local_text(m, text);
        let result = native_install::route_command(
            m,
            &mut native_install_runtime::Runtime::new(self.vm),
            reference,
            sender,
            0x10000,
            pointer,
            0,
        );
        crate::native_stack::set_sp(m, saved);
        result
    }
}

/// Picking traverses the actual retained renderer/camera lists before asking
/// the world for a ray hit. The missing49e3d0 body cannot be replaced by an
/// invented no-hit result, even when the cursor is only driving a menu.
struct WorldRay;
impl crate::native_cursor_pick::Host for WorldRay {
    fn ray(
        &mut self,
        _m: &mut Space,
        _ray: [u32; 6],
        _selection: u32,
        _limit: &mut u32,
    ) -> Result<u32, String> {
        Err(
            "NativeCursorPickBoundary: original49e3d0 world ray traversal is not implemented"
                .into(),
        )
    }
}

/// Consume the pending native virtual CALL in the dispatcher's live memory
/// lease, preserving its scratch stack through nested callbacks.
pub fn frame(
    m: &mut Space,
    vm: &mut VmHost,
    k: u32,
    b: u32,
    dt: u32,
    animate: bool,
) -> Result<(), String> {
    let mut host = Runtime {
        vm,
        template_id: m.r32(k + 0xc),
    };
    if animate {
        native_cursor::animate(m, k, b, dt, &mut host)
    } else {
        native_cursor::control(m, k, b, dt, &mut host)
    }
}

/// Collaborators of461d20 available from the generic4265c0 dispatcher.
/// Device cursor operations and Config refresh retain their explicit boundary
/// until that host exposes their actual native path.
struct CommandHost<'a>(&'a mut dyn native_install::Host);
impl native_cursor::Host for CommandHost<'_> {
    fn cursor_backend(&mut self, m: &mut Space, operation: &str, value: u32) -> Result<(), String> {
        native_cursor::system_command(m, operation, value, &mut HeadlessDevice)
    }
    fn resolve_kind(&mut self, m: &mut Space, id: u32, kind: u32) -> Result<u32, String> {
        self.0.resolve(m, id, kind, 0)
    }
    fn place(
        &mut self,
        m: &mut Space,
        reference: u32,
        translation: u32,
        words: [u32; 9],
        _name: u32,
    ) -> Result<(), String> {
        let saved = crate::native_stack::sp(m);
        let rotation = saved.wrapping_sub(0x30);
        for (i, word) in words.into_iter().enumerate() {
            m.w32(rotation + (i / 3) as u32 * 16 + (i % 3) as u32 * 4, word);
        }
        crate::native_stack::set_sp(m, rotation);
        let result = crate::native_render_graph::named_bone_transform(
            &mut native_install::Install { m, host: self.0 },
            reference,
            "bone00",
            0,
            translation,
            rotation,
        );
        crate::native_stack::set_sp(m, saved);
        result
    }
    fn color(&mut self, m: &mut Space, reference: u32, color: u32) -> Result<(), String> {
        let raw = m.r32(reference + 12);
        if raw != 0 {
            let payload = native_install::Install { m, host: self.0 }.data(
                raw,
                0x80000001,
                1f32.to_bits(),
            )?;
            if payload != 0 {
                m.w32(payload + 0xcc, color);
            }
        }
        Ok(())
    }
    fn release_render(&mut self, m: &mut Space, reference: u32) -> Result<(), String> {
        if m.r32(reference + 8) != 0 && m.r32(reference + 12) != 0 {
            crate::native_message::defer_destroy(m, reference)?;
        }
        Ok(())
    }
}

/// The boot binary's declared logging backend. These requests are observable
/// native platform call-stream evidence only; no macOS window/device effect
/// is asserted. A physical pointing observation cannot be supplied by a log.
struct HeadlessDevice;
fn record_device(api: &str, arguments: String) {
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: "native-cursor-platform".into(),
        fields: vec![
            ("backend".into(), "headless-call-stream".into()),
            ("api".into(), api.into()),
            ("arguments".into(), arguments),
            ("physical_device_observed".into(), "false".into()),
        ],
    });
}
impl native_cursor::Device for HeadlessDevice {
    fn set_cursor(&mut self, handle: u32) -> Result<(), String> {
        record_device("SetCursor", format!("{handle:#010x}"));
        Ok(())
    }
    fn show_cursor(&mut self, visible: bool) -> Result<(), String> {
        record_device("ShowCursor", u32::from(visible).to_string());
        Ok(())
    }
    fn clip_cursor(&mut self, rectangle: Option<[i32; 4]>) -> Result<(), String> {
        record_device("ClipCursor", format!("{rectangle:?}"));
        Ok(())
    }
    fn cursor_position(&mut self) -> Result<[i32; 2], String> {
        Err(
            "NativeCursorBoundary: GetCursorPos requires declared platform pointer observation"
                .into(),
        )
    }
}

pub fn native_command(
    m: &mut Space,
    host: &mut dyn native_install::Host,
    body: u32,
    event: u32,
    argument: u32,
) -> Result<u32, String> {
    let mut text = Vec::new();
    let mut p = argument;
    if event == 0x10 {
        if p == 0 {
            return Err("NativeCursorBoundary: original461d20 null text dereference".into());
        }
        while m.r8(p) != 0 {
            text.push(m.r8(p));
            p = p.wrapping_add(1);
        }
    }
    native_cursor::event(m, body, event, &text, &mut CommandHost(host))
}

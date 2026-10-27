//! Stock cursor template and frame dispatch. Collaborators stay explicit: this
//! module does not invent a picked object, render output or input-device value.
//! The original's callbacks may mutate the live Space between instructions.
use slr_physics::space::Space;
use slr_vm::format::{sprintf, FormatArg};
use slr_x87::{Precision, Rounding, X87};

const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;

/// Platform operations requested by the original cursor wrappers. Their
/// return values are discarded by those wrappers; pointer position is the
/// exception and must come from the declared device observation.
pub trait Device {
    fn set_cursor(&mut self, handle: u32) -> Result<(), String>;
    fn show_cursor(&mut self, visible: bool) -> Result<(), String>;
    fn clip_cursor(&mut self, rectangle: Option<[i32; 4]>) -> Result<(), String>;
    fn cursor_position(&mut self) -> Result<[i32; 2], String>;
}

///4b7bd0/4b7c50/4b7c90/4b7cb0/4b7cc0, through their actual device calls.
/// exe: FUN_004b7bd0
/// exe: FUN_004b7c50
/// exe: FUN_004b7c90
/// exe: FUN_004b7cb0
/// exe: FUN_004b7cc0
pub fn system_command(
    m: &mut Space,
    operation: &str,
    value: u32,
    device: &mut dyn Device,
) -> Result<(), String> {
    match operation {
        "mode" => {
            let mode = if (value as i32) < 0 || value >= 10 {
                0
            } else {
                value
            };
            let handle = m.r32(0x6495b4 + mode * 4);
            m.w32(0x6495ac, handle);
            device.set_cursor(if m.r32(0x6495a4) != 0 { handle } else { 0 })?;
        }
        "show" => device.show_cursor(true)?,
        "hide" => device.show_cursor(false)?,
        "lock" => {
            let [x, y] = device.cursor_position()?;
            // The original requests a degenerate clip rectangle. Its API
            // result is ignored, then the game sets its own lock word.
            device.clip_cursor(Some([x, y, x, y]))?;
            m.w32(0x6495b0, 1);
        }
        "unlock" => {
            m.w32(0x6495b0, 0);
            device.clip_cursor(None)?;
        }
        _ => {
            return Err(format!(
                "NativeCursorBoundary: unknown system cursor operation {operation}"
            ))
        }
    }
    Ok(())
}

/// The native boundaries invoked by45fd00,45fdf0,460140 and461c40.
/// Default errors deliberately keep an unported collaborator observable.
pub trait Host {
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
        Ok(m.malloc(bytes))
    }
    fn name(&mut self, _m: &mut Space, _id: u32) -> Result<u32, String> {
        Err("NativeCursorBoundary: original5383f0 resource name".into())
    }
    fn lookup(&mut self, _m: &mut Space, _name: &str, _kind: u32) -> Result<u32, String> {
        Err("NativeCursorBoundary: original538440 resource lookup".into())
    }
    fn resolve(&mut self, _m: &mut Space, _id: u32) -> Result<u32, String> {
        Err("NativeCursorBoundary: original545fc0 resource resolution".into())
    }
    fn resolve_kind(&mut self, m: &mut Space, id: u32, kind: u32) -> Result<u32, String> {
        let raw = self.resolve(m, id)?;
        Ok(if raw != 0 && kind != 0 && m.r32(raw + 0x4c) != kind {
            0
        } else {
            raw
        })
    }
    fn cursor_backend(
        &mut self,
        _m: &mut Space,
        _operation: &str,
        _value: u32,
    ) -> Result<(), String> {
        Err("NativeCursorBoundary: original4b7bd0/4b7c50/4b7cb0 system cursor backend".into())
    }
    fn release_render(&mut self, _m: &mut Space, _reference: u32) -> Result<(), String> {
        Err("NativeCursorBoundary: original48a8d0 render release".into())
    }
    fn color(&mut self, _m: &mut Space, _reference: u32, _color: u32) -> Result<(), String> {
        Err("NativeCursorBoundary: original48c8c0 render color".into())
    }
    fn sensitivity(&mut self, _m: &mut Space) -> Result<u32, String> {
        Err("NativeCursorBoundary: mouseSensitivity setting".into())
    }
    fn system_cursor(&mut self, _m: &mut Space) -> Result<Option<u32>, String> {
        Err("NativeCursorBoundary: SysCursor setting".into())
    }
    fn system_cursor_integer(&mut self, _m: &mut Space, value: u32) -> Result<u32, String> {
        Ok(value)
    }
    fn subscribe(&mut self, _m: &mut Space, _reference: u32, _mask: u32) -> Result<(), String> {
        Err("NativeCursorBoundary: original48d3d0 event mask".into())
    }
    fn query(
        &mut self,
        _m: &mut Space,
        _reference: u32,
        _message: u32,
        _argument: u32,
    ) -> Result<u32, String> {
        Err("NativeCursorBoundary: original426470 input query".into())
    }
    ///48bc40/48c0f0, bone00, mode0, identity rotation and supplied translation.
    fn place(
        &mut self,
        _m: &mut Space,
        _reference: u32,
        _translation: u32,
        _rotation: [u32; 9],
        _name: u32,
    ) -> Result<(), String> {
        Err("NativeCursorBoundary: original48bc40/48c0f0 cursor placement".into())
    }
    ///48d450's ten machine arguments. Outputs include intrusive resource and
    /// entity links, so a result boolean alone cannot stand in for this call.
    fn pick(&mut self, _m: &mut Space, _arguments: [u32; 10]) -> Result<u32, String> {
        Err("NativeCursorBoundary: original48d450 picking graph".into())
    }
    ///426800(reference, sender, 0x10000, text, 0).
    fn command(
        &mut self,
        _m: &mut Space,
        _reference: u32,
        _sender: u32,
        _text: &str,
    ) -> Result<(), String> {
        Err("NativeCursorBoundary: original426800 event dispatch".into())
    }
    fn debug_frame(&mut self, _m: &mut Space, _body: u32) -> Result<(), String> {
        Err("NativeCursorBoundary: original46058f debug geometry".into())
    }
    ///461c98:48c4b0, keeping all arguments in their original storage.
    fn render_cursor(&mut self, _m: &mut Space, _body: u32) -> Result<(), String> {
        Err("NativeCursorBoundary: original48c4b0 cursor rendering".into())
    }
    ///461cec:48c670, or461d11:48a8d0 when the viewport link is null.
    fn render_debug(&mut self, _m: &mut Space, _body: u32, _visible: bool) -> Result<(), String> {
        Err("NativeCursorBoundary: original48c670 debug rendering".into())
    }
    /// The conditional46187f..461979 coordinate conversion. This is called
    /// only with a non-null counted entity and must perform its actual query,
    /// ownership test and matrices; it is not an approximate coordinate cast.
    fn release_coordinates(
        &mut self,
        _m: &mut Space,
        _body: u32,
        _point: [u32; 3],
    ) -> Result<[u32; 3], String> {
        Err("NativeCursorBoundary: original46187f release coordinate graph".into())
    }
}

fn f(word: u32) -> X87 {
    X87::from_f32(f32::from_bits(word))
}
fn store(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}
fn vector(m: &Space, p: u32) -> [u32; 3] {
    [m.r32(p), m.r32(p + 4), m.r32(p + 8)]
}
fn copy_vector(m: &mut Space, to: u32, from: u32) {
    for i in 0..3 {
        m.w32(to + i * 4, m.r32(from + i * 4));
    }
}
fn camera(m: &Space) -> u32 {
    m.r32(0x610a54)
}
fn entity_id(m: &Space, link: u32) -> u32 {
    let entity = m.r32(link);
    if entity == 0 {
        0
    } else {
        m.r32(entity + 0x50)
    }
}
fn link_entity(m: &mut Space, link: u32, entity: u32) {
    crate::native_car_command::unlink_entity(m, link);
    m.w32(link, entity);
    if entity == 0 {
        m.w32(link + 4, 0);
    } else {
        m.w32(link + 4, m.r32(entity));
        m.w32(entity, link);
        m.w32(entity + 4, m.r32(entity + 4).wrapping_add(1));
    }
}

/// exe: FUN_0045fd00
pub fn template(m: &mut Space, owner: u32, host: &mut dyn Host) -> Result<u32, String> {
    let k = host.allocate(m, 0x28)?;
    if k == 0 {
        return Err("NativeCursorBoundary: original45fd00 null allocation dereference".into());
    }
    for offset in (4..=0x14).step_by(4) {
        m.w32(k + offset, 0);
    }
    m.w32(k, 0x5f11e4);
    crate::native_part::bind_reference(m, k + 4, owner);
    let name = host.name(m, m.r32(k + 0xc))?;
    m.w32(k + 0x1c, name);
    unload(m, k);
    m.w32(k + 0x20, 0x3ca3d70a);
    m.w32(k + 0x24, 0x3c23d70a);
    Ok(k)
}

/// exe: FUN_0045fde0
pub fn unload(m: &mut Space, k: u32) {
    m.w32(k + 0x18, 0);
}

/// exe: FUN_0045fdf0
pub fn construct(
    m: &mut Space,
    k: u32,
    reference: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[reference, 0], Some(k), |m| {
        s::set_reg(m, Reg::Eax, m.r32(k + 0x18));
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        s::set_reg(m, Reg::Edi, 0);
        if m.r32(k + 0x18) == 0 {
            s::set_reg(m, Reg::Eax, m.r32(k));
            s::call(m, 0x45fe00, &[], Some(k), |m| {
                s::function(m, &[], Some(k), |m| {
                    m.w32(k + 0x18, m.r32(k + 0x18) + 1);
                })
            });
        }
        let b = s::external(m, 0x45fe0a, &[0x1bc], None, |m| host.allocate(m, 0x1bc))?;
        s::set_reg(m, Reg::Eax, b);
        s::set_reg(m, Reg::Esi, b);
        if b == 0 {
            return Err("NativeCursorBoundary: original45fdf0 null allocation dereference".into());
        }
        s::call(m, 0x45fe1e, &[], Some(b), |m| {
            s::function(m, &[], Some(b), |m| {
                s::set_reg(m, Reg::Eax, b);
                s::set_reg(m, Reg::Ecx, 0);
                for off in [0, 4, 8, 12, 0x14, 0x18, 0x10] {
                    m.w32(b + off, 0);
                }
            })
        });
        crate::native_player::empty_ref(m, b + 0x1c, 0x45fe26, false);
        for (off, pc) in [
            (0x2c, 0x45fe2e),
            (0x3c, 0x45fe36),
            (0x54, 0x45fe3e),
            (0x64, 0x45fe46),
            (0x74, 0x45fe4e),
            (0x84, 0x45fe59),
            (0x94, 0x45fe64),
        ] {
            crate::native_player::empty_ref(m, b + off, pc, true);
        }
        cursor_array(m, b + 0xb0, 12, 3, 0x45dc70, 0x45fe79);
        cursor_array(m, b + 0xec, 16, 2, 0x42cec0, 0x45fe8e);
        cursor_array(m, b + 0x10c, 8, 2, 0x4627e0, 0x45fea3);
        cursor_array(m, b + 0x11c, 12, 2, 0x45dc70, 0x45feb8);
        crate::native_player::empty_ref(m, b + 0x13c, 0x45fec3, true);
        for off in [0x14c, 0x150] {
            m.w32(b + off, 0);
        }
        crate::native_player::empty_ref(m, b + 0x180, 0x45feda, true);
        for off in [0x1a0, 0x1a4] {
            m.w32(b + off, 0);
        }
        s::set_reg(m, Reg::Edx, reference);
        s::set_reg(m, Reg::Eax, m.r32(reference + 12));
        let owner = m.r32(reference + 12);
        s::call(m, 0x45fef9, &[owner], Some(b), |m| {
            crate::native_instance::bind(m, b, owner)
        });
        for (off, key, lookup_pc, bind_pc) in [
            (0x2c, 0x610a64, 0x45ff0b, 0x45ff14),
            (0x3c, 0x610a74, 0x45ff26, 0x45ff2f),
        ] {
            let manager = m.r32(0x618d48);
            let id = s::external(m, lookup_pc, &[key, 0x23], Some(manager), |m| {
                host.lookup(m, "frontend.rpk", 0x23)
            })?;
            s::set_reg(m, Reg::Eax, id);
            s::external(m, bind_pc, &[id], Some(b + off), |m| {
                let raw = host.resolve(m, id)?;
                crate::native_part::bind_reference(m, b + off, raw);
                Ok::<_, String>(())
            })?;
        }
        m.w32(b + 0x4c, u32::MAX);
        s::push(m, 0x610a84);
        let sensitivity = s::call(m, 0x45ff40, &[], None, |m| {
            s::function_cdecl(m, &[], |m| host.sensitivity(m))
        })?;
        let sensitivity = store(f(sensitivity).mul(f(0x447a0000), PC, RC));
        s::push(m, 0x100);
        s::push(m, 0x610a98);
        s::set_reg(m, Reg::Ecx, b + 0x1ac);
        s::push(m, b + 0x1ac);
        m.w32(0x610a5c, sensitivity);
        m.w32(0x610a58, sensitivity);
        s::call(m, 0x45ff68, &[], None, |m| {
            crate::native_instance::copy_name(m, b + 0x1ac, 0x610a98, 0x100)
        });
        for off in (0xa4..=0xe8).step_by(4) {
            m.w32(b + off, 0);
        }
        s::push(m, 0);
        s::push(m, 0x10);
        s::push(m, b);
        m.w32(b + 0x190, 0x40400000);
        for off in (0x164..=0x170).step_by(4) {
            m.w32(b + off, 0);
        }
        m.w32(b + 0x1a8, 1);
        m.w32(b + 0x50, 3);
        s::call(m, 0x460010, &[], None, |m| {
            s::function_cdecl(m, &[], |m| host.subscribe(m, b, 0x10))
        })?;
        s::push(m, 0);
        s::push(m, 0x610a9c);
        let system = s::call(m, 0x46001b, &[], None, |m| {
            s::function_cdecl(m, &[], |m| host.system_cursor(m))
        })?;
        s::set_reg(m, Reg::Eax, u32::from(system.is_some()));
        s::add_sp(m, 0x24);
        if let Some(value) = system {
            let value = s::external(m, 0x46002c, &[0x610aa8], None, |m| {
                host.system_cursor_integer(m, value)
            })?;
            s::set_reg(m, Reg::Eax, value);
            m.w32(0x6495a4, value);
        }
        s::set_reg(m, Reg::Eax, b);
        Ok(b)
    })
}
fn cursor_array(m: &mut Space, p: u32, size: u32, count: u32, ctor: u32, pc: u32) {
    use crate::native_stack::{self as s, Reg};
    s::call(m, pc, &[p, size, count, ctor], None, |m| {
        s::function(m, &[p, size, count, ctor], None, |m| {
            s::set_reg(m, Reg::Eax, count - 1);
            for (r, v) in [
                (Reg::Ebx, ctor),
                (Reg::Ebp, size),
                (Reg::Esi, p),
                (Reg::Edi, count),
            ] {
                s::push_reg(m, r);
                s::set_reg(m, r, v);
            }
            for i in 0..count {
                s::call(m, 0x40187e, &[], Some(p + i * size), |m| {
                    s::function(m, &[], Some(p + i * size), |m| {
                        if ctor == 0x42cec0 {
                            s::set_reg(m, Reg::Eax, p + i * size);
                            s::set_reg(m, Reg::Ecx, 0);
                            for off in (0..16).step_by(4) {
                                m.w32(p + i * size + off, 0);
                            }
                        }
                        if ctor == 0x4627e0 {
                            s::set_reg(m, Reg::Eax, p + i * size);
                            for off in [0, 4] {
                                m.w32(p + i * size + off, 0);
                            }
                        }
                    })
                });
                s::set_reg(m, Reg::Esi, p + (i + 1) * size);
                s::set_reg(m, Reg::Edi, count - i - 1);
            }
        })
    });
}

/// Original4625f0 query frame. Registration's8/9/28 paths retain the two
/// saved-register stores even though their scalar results are constants.
/// exe: FUN_004625f0
pub fn get_info(m: &mut Space, k: u32, b: u32, message: u32, arg: u32) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[b, message, arg], Some(k), |m| {
        let selector = message.wrapping_sub(2);
        s::set_reg(m, Reg::Eax, selector);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        if selector <= 0x39 {
            s::set_reg(
                m,
                Reg::Ecx,
                match message {
                    2 => 0,
                    28 => 1,
                    42 => 2,
                    44 => 3,
                    59 => 4,
                    _ => 5,
                },
            );
        }
        let value = match message {
            28 => 1,
            42 => {
                s::set_reg(m, Reg::Edx, b);
                m.r32(b + 0x1a8)
            }
            2 => {
                s::set_reg(m, Reg::Eax, arg);
                if arg != 0 {
                    s::set_reg(m, Reg::Ecx, b);
                    s::set_reg(m, Reg::Edx, b + 0xa4);
                    s::set_reg(m, Reg::Esi, arg);
                    for off in [0, 4, 8] {
                        m.w32(arg + off, m.r32(b + 0xa4 + off));
                    }
                    s::set_reg(m, Reg::Edx, m.r32(b + 0xac));
                    s::set_reg(m, Reg::Ecx, m.r32(b + 0x170));
                    m.w32(arg + 8, m.r32(b + 0x170));
                }
                1
            }
            59 => {
                s::set_reg(m, Reg::Eax, arg);
                s::set_reg(m, Reg::Ecx, b);
                if arg != 0 {
                    for off in [0, 4, 8] {
                        m.w32(arg + off, m.r32(b + 0x164 + off));
                    }
                    s::set_reg(m, Reg::Edx, m.r32(b + 0x16c));
                }
                m.r32(b + 0x144)
            }
            44 => {
                return Err(
                    "NativeCursorBoundary: original4625f0 retained hit-reference query frame"
                        .into(),
                )
            }
            _ => 0,
        };
        s::set_reg(m, Reg::Eax, value);
        Ok(value)
    })
}

fn bind_id(
    m: &mut Space,
    reference: u32,
    id: u32,
    kind: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    //545fc0/546070 compare IDs before releasing or resolving.
    if m.r32(reference + 8) == id {
        return Ok(());
    }
    if m.r32(reference + 12) != 0 {
        crate::native_part::bind_reference(m, reference, 0);
    }
    if id != 0 {
        let raw = host.resolve_kind(m, id, kind)?;
        if raw != 0 {
            crate::native_part::bind_reference(m, reference, raw);
            m.w32(reference + 8, id);
        }
    }
    Ok(())
}

/// exe: FUN_00461d20
pub fn event(
    m: &mut Space,
    b: u32,
    kind: u32,
    text: &[u8],
    host: &mut dyn Host,
) -> Result<u32, String> {
    use slr_lang::scan::{sscanf, ScanArg};
    if kind != 0x10 {
        return Ok(1);
    }
    let verb = text
        .split(|c| c.is_ascii_whitespace() || *c == 0)
        .find(|s| !s.is_empty())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let verb = verb.as_slice();
    match verb {
        b"cursor" => {
            let (mut viewport, mut control, mut owner) = (0, 0, 0);
            let _ = sscanf(
                text,
                b"%*s %d %d %d",
                &mut [
                    ScanArg::Word(&mut viewport),
                    ScanArg::Word(&mut control),
                    ScanArg::Word(&mut owner),
                ],
            );
            bind_id(m, b + 0x64, viewport, 0x12, host)?;
            bind_id(m, b + 0x54, control, 0, host)?;
            bind_id(m, b + 0x74, owner, 1, host)?;
        }
        b"activate" => {
            let (mut control, mut listener) = (0, 0);
            let _ = sscanf(
                text,
                b"%*s %d %d",
                &mut [ScanArg::Word(&mut control), ScanArg::Word(&mut listener)],
            );
            bind_id(m, b + 0x1c, control, 1, host)?;
            bind_id(m, b + 0x180, listener, 1, host)?;
            m.w32(b + 0x10, m.r32(b + 0x10) | 1);
        }
        b"deactivate" => {
            crate::native_install::release(m, b + 0x1c);
            m.w32(b + 0x10, m.r32(b + 0x10) & !1);
        }
        b"enable" => {
            if m.r32(0x6495a4) != 0 && m.r8(b + 0x50) & 1 == 0 {
                host.cursor_backend(m, "show", 0)?;
            }
            m.w32(b + 0x50, m.r32(b + 0x50) | 1);
        }
        b"disable" | b"standby" => {
            if m.r32(0x6495a4) != 0 && m.r8(b + 0x50) & 1 != 0 {
                host.cursor_backend(m, "hide", 0)?;
            }
            let status = m.r32(b + 0x50);
            m.w32(
                b + 0x50,
                if verb == b"disable" {
                    status & 0xfffffffa
                } else {
                    (status & !1) | 4
                },
            );
            crate::native_install::release(m, b + 0x13c);
            crate::native_car_command::unlink_entity(m, b + 0x14c);
            for off in [0x160, 0x154, 0x158, 0x15c] {
                m.w32(b + off, 0);
            }
        }
        b"lock" | b"unlock" => {
            if m.r32(0x6495a4) != 0 {
                host.cursor_backend(m, if verb == b"lock" { "lock" } else { "unlock" }, 0)?;
            }
        }
        b"sens" => {
            let sensitivity = f(host.sensitivity(m)?).mul(f(0x447a0000), PC, RC);
            m.w32(0x610a5c, store(sensitivity));
            m.w32(0x610a58, store(sensitivity));
        }
        b"color" => {
            let mut color = m.r32(b + 0x4c);
            let _ = sscanf(text, b"%*s %d", &mut [ScanArg::Word(&mut color)]);
            m.w32(b + 0x4c, color);
            host.color(m, b + 0x84, color)?;
        }
        b"mode" => {
            let mut mode = m.r32(b + 0x1a8);
            let mut glyph = vec![];
            let mut resource = m.r32(b + 0x34);
            let _ = sscanf(
                text,
                b"%*s %d %1s %d",
                &mut [
                    ScanArg::Word(&mut mode),
                    ScanArg::Str(&mut glyph),
                    ScanArg::Word(&mut resource),
                ],
            );
            m.w32(b + 0x1a8, mode);
            if !glyph.is_empty() {
                m.w8(b + 0x1ac, glyph[0]);
                m.w8(b + 0x1ad, 0);
            }
            if resource != 0 {
                bind_id(m, b + 0x2c, resource, 0xe, host)?;
            }
            host.release_render(m, b + 0x84)?;
            let mode = match m.r8(b + 0x1ac) {
                b'1' | b'G' => 1,
                b'2' | b'M' => 2,
                b'3' | b'W' => 3,
                b'4' | b'A' => 4,
                b'5' | b'D' => 5,
                b'6' => 6,
                b'7' => 7,
                b'8' => 8,
                b'9' => 9,
                _ => 0,
            };
            host.cursor_backend(m, "mode", mode)?;
        }
        b"move" | b"relmove" => {
            // Original scan destinations are uninitialized stack slots when
            // either float is absent. Preserve that as an explicit boundary.
            let (mut x, mut y) = (0., 0.);
            if sscanf(
                text,
                b"%*s %f,%f",
                &mut [ScanArg::Float(&mut x), ScanArg::Float(&mut y)],
            )
            .map_err(|e| format!("NativeCursorBoundary: move scan {e:?}"))?
            .matches
                != 3
            {
                return Err(
                    "NativeCursorBoundary: original cursor move has uninitialized scan destination"
                        .into(),
                );
            }
            if verb == b"relmove" {
                x = f(x.to_bits())
                    .add(f(m.r32(b + 0xa4)), PC, RC)
                    .to_f32(PC, RC);
                y = f(y.to_bits())
                    .add(f(m.r32(b + 0xa8)), PC, RC)
                    .to_f32(PC, RC);
            }
            m.w32(b + 0xa4, x.to_bits());
            m.w32(b + 0xa8, y.to_bits());
            clamp(m, b + 0xa4);
            clamp(m, b + 0xa8);
            host.place(
                m,
                b + 0x84,
                b + 0xa4,
                [0; 9],
                if verb == b"relmove" {
                    0x610d94
                } else {
                    0x610db0
                },
            )?;
        }
        _ => {}
    }
    Ok(1)
}

fn clamp(m: &mut Space, p: u32) {
    let codes = f(m.r32(p)).fcom(f(0x3f800000)).0;
    if !(codes.c0 || codes.c3) {
        m.w32(p, 0x3f800000);
    }
    if f(m.r32(p)).fcom(f(0xbf800000)).0.c0 {
        m.w32(p, 0xbf800000);
    }
}

fn emit(
    host: &mut dyn Host,
    m: &mut Space,
    reference: u32,
    sender: u32,
    words: &[u32],
    points: &[[u32; 3]],
) -> Result<String, String> {
    let mut fmt = vec!["%d"; words.len()].join(" ");
    let mut args: Vec<FormatArg<'_>> = words.iter().copied().map(FormatArg::Word).collect();
    for point in points {
        fmt.push_str(" %.3f %.3f %.3f");
        args.extend(
            point
                .iter()
                .map(|word| FormatArg::Double(f32::from_bits(*word) as f64)),
        );
    }
    let bytes = sprintf(fmt.as_bytes(), &args)
        .map_err(|e| format!("NativeCursorBoundary: event format {e:?}"))?;
    let text = String::from_utf8(bytes).map_err(|e| e.to_string())?;
    host.command(m, reference, sender, &text)?;
    Ok(text)
}

/// exe: FUN_00461c20
pub fn control(m: &mut Space, k: u32, b: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    crate::native_stack::function(m, &[b, dt], Some(k), |m| {
        if m.r8(b + 0x10) & 1 != 0 {
            crate::native_stack::call(m, 0x461c35, &[b, dt], None, |m| frame(m, k, b, dt, host))?;
        }
        Ok(())
    })
}
/// exe: FUN_00461c40
pub fn animate(m: &mut Space, k: u32, b: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    crate::native_stack::function(m, &[b, dt], Some(k), |m| {
        crate::native_stack::alloc(m, 0xc);
        crate::native_stack::push_reg(m, crate::native_stack::Reg::Esi);
        crate::native_stack::call(m, 0x461c53, &[b, dt], None, |m| frame(m, k, b, dt, host))?;
        let visible = m.r32(b + 0x7c) != 0;
        if visible && m.r32(0x6495a4) == 0 && m.r32(b + 0x8c) == 0 {
            host.render_cursor(m, b)?;
        }
        if m.r32(0x63c83c) != 0 && m.r32(b + 0x9c) == 0 {
            host.render_debug(m, b, visible)?;
        }
        Ok(())
    })
}

/// exe: FUN_00460140
pub fn frame(m: &mut Space, k: u32, b: u32, dt: u32, host: &mut dyn Host) -> Result<(), String> {
    crate::native_stack::function(m, &[b, dt], Some(k), |m| {
        crate::native_stack::push_reg(m, crate::native_stack::Reg::Ebp);
        crate::native_stack::set_sp(m, crate::native_stack::sp(m) & !7);
        crate::native_stack::alloc(m, 0x2b4);
        for r in [
            crate::native_stack::Reg::Ebx,
            crate::native_stack::Reg::Esi,
            crate::native_stack::Reg::Edi,
        ] {
            crate::native_stack::push_reg(m, r);
        }
        let scratch = crate::native_stack::sp(m);
        m.w32(scratch + 0x60, k);
        m.w32(scratch + 0x74, 0);
        frame_body(m, k, b, dt, scratch, host)
    })
}
fn frame_body(
    m: &mut Space,
    k: u32,
    b: u32,
    dt: u32,
    scratch: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    if m.r32(b + 0x24) == 0 {
        return Ok(());
    }
    let mut moved = 0;
    if m.r32(0x6495a4) == 0 {
        let px = host.query(m, b + 0x1c, 0x43, 0x2a)?;
        let dx = store(
            f(m.r32(px))
                .mul(f(m.r32(0x610a58)), PC, RC)
                .mul(f(dt), PC, RC),
        );
        let py = host.query(m, b + 0x1c, 0x43, 0x2b)?;
        let dy_register = f(m.r32(py))
            .mul(f(m.r32(0x610a5c)), PC, RC)
            .mul(f(dt), PC, RC);
        let dy = store(dy_register);
        let size = dy_register
            .mul(f(dy), PC, RC)
            .add(f(dx).mul(f(dx), PC, RC), PC, RC);
        let cc = size.fcom(f(0x358637be)).0;
        if !(cc.c0 || cc.c3) {
            moved = 1;
            if m.r32(b + 0x50) & 4 != 0 {
                m.w32(b + 0x50, m.r32(b + 0x50) | 1);
            }
        }
        for (offset, delta) in [(0xa4, dx), (0xa8, dy), (0xac, 0)] {
            m.w32(
                b + offset,
                store(f(delta).add(f(m.r32(b + offset)), PC, RC)),
            );
        }
    } else {
        let dx = f(m.r32(b + 0xa4)).sub(f(m.r32(0x64959c)), PC, RC);
        let dy = f(m.r32(b + 0xa8)).sub(f(m.r32(0x649598)), PC, RC);
        let size = dy.mul(dy, PC, RC).add(dx.mul(dx, PC, RC), PC, RC);
        let cc = size.fcom(f(0x358637be)).0;
        if !(cc.c0 || cc.c3) {
            moved = 1;
            if m.r32(b + 0x50) & 4 != 0 {
                m.w32(b + 0x50, m.r32(b + 0x50) | 1);
            }
        }
        m.w32(b + 0xa4, m.r32(0x64959c));
        m.w32(b + 0xa8, m.r32(0x649598));
    }
    clamp(m, b + 0xa4);
    clamp(m, b + 0xa8);
    host.place(
        m,
        b + 0x84,
        b + 0xa4,
        [0x3f800000, 0, 0, 0, 0x3f800000, 0, 0, 0, 0x3f800000],
        0x610ab4,
    )?;
    if m.r8(b + 0x50) & 1 == 0 {
        return Ok(());
    }
    let c = camera(m);
    let arguments = [
        b + 0xa4,
        c + 0x74,
        c + 0x6c,
        c + 0x4c,
        c + 0x58,
        c + 0x64,
        c + 0x48,
        c,
        c + 0xc,
        c + 0x68,
    ];
    let hit =
        crate::native_stack::call(m, 0x46034e, &arguments, None, |m| host.pick(m, arguments))?;
    for i in 0..3 {
        m.w32(
            b + 0x164 + i * 4,
            store(f(m.r32(camera(m) + i * 4)).sub(f(m.r32(camera(m) + 0xc + i * 4)), PC, RC)),
        );
    }
    let scale = f(m.r32(camera(m) + 0x48));
    for i in 0..3 {
        m.w32(
            b + 0x164 + i * 4,
            store(scale.mul(f(m.r32(b + 0x164 + i * 4)), PC, RC)),
        );
    }
    for i in 0..3 {
        m.w32(
            b + 0x164 + i * 4,
            store(f(m.r32(camera(m) + 0xc + i * 4)).add(f(m.r32(b + 0x164 + i * 4)), PC, RC)),
        );
    }
    m.w32(b + 0x170, m.r32(camera(m) + 0x48));
    if m.r32(b + 0x144) != m.r32(camera(m) + 0x7c) || m.r32(b + 0x14c) != m.r32(camera(m) + 0x6c) {
        let selected = m.r32(b + 0x144);
        if selected != 0 {
            let text = emit(
                host,
                m,
                b + 0x13c,
                0,
                &[0x10, m.r32(b + 8), selected, entity_id(m, b + 0x14c), moved],
                &[],
            )?;
            host.command(m, b, 0, &text)?;
        }
        crate::native_part::bind_reference(m, b + 0x13c, m.r32(camera(m) + 0x80));
        link_entity(m, b + 0x14c, m.r32(camera(m) + 0x6c));
        if m.r32(b + 0x144) != 0 {
            let text = emit(
                host,
                m,
                b + 0x13c,
                0,
                &[
                    0xf,
                    m.r32(b + 8),
                    m.r32(b + 0x144),
                    entity_id(m, b + 0x14c),
                    moved,
                ],
                &[],
            )?;
            host.command(m, b, 0, &text)?;
        }
    }
    m.w32(b + 0x160, m.r32(camera(m) + 0x64));
    copy_vector(m, b + 0x154, camera(m) + 0x4c);
    if m.r32(0x63c83c) != 0 {
        host.debug_frame(m, b)?;
    }
    button(m, k, b, hit, false, scratch, host)?;
    button(m, k, b, hit, true, scratch, host)
}

fn button(
    m: &mut Space,
    k: u32,
    b: u32,
    hit: u32,
    right: bool,
    scratch: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let pressed = host.query(m, b + 0x1c, 0x19, if right { 0x2e } else { 0x2d })?;
    let cc = X87::from_i32(pressed as i32)
        .mul(f(0x3c010204), PC, RC)
        .fcom(f(0x3e4ccccd))
        .0;
    let (down, drag, ref_at, entity_at, start, point) = if right {
        (0xd8, 0xe4, 0xfc, 0x114, 0xbc, 0x128)
    } else {
        (0xd4, 0xe0, 0xec, 0x10c, 0xb0, 0x11c)
    };
    if !(cc.c0 || cc.c3) {
        if m.r32(b + down) == 0 {
            copy_vector(m, b + start, b + 0xa4);
            m.w32(b + down, 1);
            m.w32(b + drag, 0);
            if hit != 0 {
                crate::native_part::bind_reference(m, b + ref_at, m.r32(camera(m) + 0x80));
                link_entity(m, b + entity_at, m.r32(camera(m) + 0x6c));
                copy_vector(m, b + point, camera(m) + 0x4c);
                if right {
                    let mode = host.query(m, camera(m) + 0x74, 7, 0)?;
                    m.w32(b + 0x138, mode);
                }
            } else {
                crate::native_install::release(m, b + ref_at);
                crate::native_car_command::unlink_entity(m, b + entity_at);
            }
            let mut words = vec![
                if right { 3 } else { 1 },
                m.r32(b + 8),
                m.r32(b + ref_at + 8),
                entity_id(m, b + entity_at),
            ];
            let points = if right {
                vec![]
            } else {
                vec![vector(m, b + point)]
            };
            if right {
                words.truncate(4);
            }
            let text = if m.r32(b + ref_at + 8) != 0 {
                emit(host, m, b + ref_at, 0, &words, &points)?
            } else {
                // Same formatter without dispatch to a null selected resource.
                event_text(&words, &points)?
            };
            host.command(m, b, 0, &text)?;
        } else if m.r32(b + drag) == 0 {
            let dx = f(m.r32(b + start)).sub(f(m.r32(b + 0xa4)), PC, RC);
            let dy = f(m.r32(b + start + 4)).sub(f(m.r32(b + 0xa8)), PC, RC);
            let dz = f(m.r32(b + start + 8)).sub(f(m.r32(b + 0xac)), PC, RC);
            let radius = f(m.r32(k + 0x20));
            let length =
                dx.mul(dx, PC, RC)
                    .add(dy.mul(dy, PC, RC), PC, RC)
                    .add(dz.mul(dz, PC, RC), PC, RC);
            let cc = radius.mul(radius, PC, RC).fcom(length).0;
            if cc.c0 || cc.c3 {
                m.w32(b + drag, 1);
                if right {
                    m.w32(b + 0xe0, 1);
                }
                let text = emit(
                    host,
                    m,
                    b,
                    0,
                    &[
                        if right { 0xc } else { 9 },
                        m.r32(b + 8),
                        entity_id(m, b + entity_at),
                    ],
                    &[],
                )?;
                if m.r32(b + ref_at + 8) != 0 {
                    host.command(m, b + ref_at, 0, &text)?;
                }
            }
        }
        return Ok(());
    }
    if m.r32(b + down) == 0 {
        return Ok(());
    }
    m.w32(b + down, 0);
    let text = event_text(
        &[
            if right { 4 } else { 2 },
            m.r32(b + 8),
            m.r32(b + ref_at + 8),
            entity_id(m, b + entity_at),
        ],
        &[],
    )?;
    if !right {
        host.command(m, b, 0, &text)?;
    }
    if m.r32(b + ref_at + 8) != 0 {
        host.command(m, b + ref_at, 0, &text)?;
    }
    if right {
        host.command(m, b, 0, &text)?;
    }
    if m.r32(b + drag) == 0 {
        if m.r32(b + ref_at + 8) != 0 {
            let point_words = vector(m, b + point);
            emit(
                host,
                m,
                b + ref_at,
                0,
                &[
                    if right { 6 } else { 5 },
                    m.r32(b + 8),
                    entity_id(m, b + entity_at),
                ],
                &[point_words],
            )?;
            let adjusted = if right && m.r32(b + entity_at) != 0 {
                let reference = scratch + 0x64;
                for i in 0..4 {
                    m.w32(reference + i * 4, 0);
                }
                let payload = host.query(m, b + ref_at, 1, reference)?;
                let adjusted = if payload != 0 {
                    host.release_coordinates(m, b, point_words)?
                } else {
                    point_words
                };
                crate::native_install::release(m, reference);
                adjusted
            } else {
                point_words
            };
            emit(
                host,
                m,
                b,
                b + ref_at,
                &[
                    if right { 6 } else { 5 },
                    m.r32(b + 8),
                    m.r32(b + ref_at + 8),
                ],
                &[adjusted, vector(m, b + 0x164)],
            )?;
        }
    } else {
        let text = emit(
            host,
            m,
            b,
            0,
            &[
                if right { 0xd } else { 0xa },
                m.r32(b + 8),
                entity_id(m, b + entity_at),
            ],
            &[],
        )?;
        if m.r32(b + ref_at + 8) != 0 {
            host.command(m, b + ref_at, 0, &text)?;
        }
        if hit != 0 {
            emit(
                host,
                m,
                camera(m) + 0x74,
                0,
                &[
                    if right { 0xe } else { 0xb },
                    m.r32(b + 8),
                    entity_id(m, camera(m) + 0x6c),
                    m.r32(b + ref_at + 8),
                    entity_id(m, b + entity_at),
                ],
                &[],
            )?;
            let mut words = vec![
                if right { 0xe } else { 0xb },
                m.r32(b + 8),
                m.r32(camera(m) + 0x7c),
                m.r32(b + ref_at + 8),
            ];
            let mut args: Vec<FormatArg<'_>> = words.drain(..).map(FormatArg::Word).collect();
            args.extend(vector(m, b + 0x164).map(|v| FormatArg::Double(f32::from_bits(v) as f64)));
            let fmt = if right {
                b"%d %d %d %d %.3f %.3f %.3f".as_slice()
            } else {
                args.push(FormatArg::Word(entity_id(m, b + entity_at)));
                b"%d %d %d %d %.3f %.3f %.3f %d".as_slice()
            };
            let bytes = sprintf(fmt, &args)
                .map_err(|e| format!("NativeCursorBoundary: release format {e:?}"))?;
            host.command(
                m,
                b,
                camera(m) + 0x74,
                &String::from_utf8(bytes).map_err(|e| e.to_string())?,
            )?;
        }
    }
    crate::native_install::release(m, b + ref_at);
    crate::native_car_command::unlink_entity(m, b + entity_at);
    Ok(())
}

fn event_text(words: &[u32], points: &[[u32; 3]]) -> Result<String, String> {
    let mut fmt = vec!["%d"; words.len()].join(" ");
    let mut args: Vec<FormatArg<'_>> = words.iter().copied().map(FormatArg::Word).collect();
    for point in points {
        fmt.push_str(" %.3f %.3f %.3f");
        args.extend(
            point
                .iter()
                .map(|v| FormatArg::Double(f32::from_bits(*v) as f64)),
        );
    }
    String::from_utf8(
        sprintf(fmt.as_bytes(), &args)
            .map_err(|e| format!("NativeCursorBoundary: event format {e:?}"))?,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    fn u(v: &Value) -> u32 {
        v.as_u64().unwrap() as u32
    }
    fn image(m: &mut Space, v: &Value) {
        for (i, word) in v.as_array().unwrap().iter().enumerate() {
            m.w32(0x20000000 + i as u32 * 4, u(word));
        }
        m.set_heap_ptr(0x20000000 + v.as_array().unwrap().len() as u32 * 4);
    }
    fn globals(m: &mut Space, v: &Value) {
        check_globals(v, false).unwrap();
        for (address, word) in v.as_object().unwrap() {
            m.w32(address.parse().unwrap(), u(word));
        }
    }
    fn compare(m: &Space, v: &Value, label: &str) {
        assert_eq!(
            (m.heap_ptr() - 0x20000000).div_ceil(4) as usize,
            v.as_array().unwrap().len(),
            "{label} heap length"
        );
        for (i, word) in v.as_array().unwrap().iter().enumerate() {
            assert_eq!(
                m.r32(0x20000000 + i as u32 * 4),
                u(word),
                "{label} heap+{:x}",
                i * 4
            );
        }
    }
    fn compare_globals(m: &Space, v: &Value, label: &str) {
        check_globals(v, false).unwrap();
        for (address, word) in v.as_object().unwrap() {
            assert_eq!(
                m.r32(address.parse().unwrap()),
                u(word),
                "{label} global {address}"
            );
        }
    }
    fn check_globals(v: &Value, device: bool) -> Result<(), String> {
        let expected = if device {
            vec![
                0x6495a4u32,
                0x6495ac,
                0x6495b0,
                0x6495b4,
                0x6495b8,
                0x6495bc,
                0x6495c0,
                0x6495c4,
                0x6495c8,
                0x6495cc,
                0x6495d0,
                0x6495d4,
                0x6495d8,
            ]
        } else {
            vec![
                0x610a54, 0x610a58, 0x610a5c, 0x649598, 0x64959c, 0x6495a4, 0x63c83c,
            ]
        };
        let fields = v.as_object().ok_or("globals must be an object")?;
        if fields.len() != expected.len()
            || expected
                .iter()
                .any(|p| fields.get(&p.to_string()).and_then(Value::as_u64).is_none())
        {
            return Err("incomplete original global inventory".into());
        }
        Ok(())
    }
    fn check_shapes(v: &Value) -> Result<(), String> {
        for (suite, count, words) in [
            ("constructors", 32, 260),
            ("frames", 30, 438),
            ("commands", 22, 660),
        ] {
            let rows = v[suite].as_array().ok_or("missing suite")?;
            if rows.len() != count {
                return Err(format!("{suite} scenario count"));
            }
            for (i, row) in rows.iter().enumerate() {
                check_globals(&row["before_globals"], false)?;
                if row["before"].as_array().map(Vec::len) != Some(words) {
                    return Err(format!("{suite} initial heap length"));
                }
                if suite == "constructors" {
                    check_globals(&row["globals"], false)?;
                    continue;
                }
                let expected_steps = if suite == "frames" {
                    if i < 17 {
                        5
                    } else if i == 17 {
                        3
                    } else {
                        1
                    }
                } else {
                    [2, 2, 2, 2, 5, 3, 2, 4, 6, 3, 1][i % 11]
                };
                let steps = row["steps"].as_array().ok_or("missing step sequence")?;
                if steps.len() != expected_steps {
                    return Err(format!("{suite} truncated step sequence"));
                }
                for step in steps {
                    check_globals(&step["globals"], false)?;
                    if step["after"].as_array().map(Vec::len) != Some(words) {
                        return Err(format!("{suite} step heap length"));
                    }
                    if step["events"].as_array().is_none() {
                        return Err("missing callback event list".into());
                    }
                }
            }
        }
        Ok(())
    }
    fn vectors() -> Value {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-cursor-original.json")).expect(
                "run native_cursor_probe.py --output re/out/boot/native-cursor-original.json",
            ),
        )
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["fpu_cw"], 0x7f);
        assert_eq!(
            v["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_cursor_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
            ("stock_exe_sha256", "stock/StreetLegal_Redline.exe"),
        ] {
            assert_eq!(
                v[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                ),
                "{field}"
            );
        }
        let profile = serde_json::to_vec(&v["profile"]).unwrap();
        assert_eq!(
            v["profile_sha256"],
            format!("{:x}", Sha256::digest(profile))
        );
        assert_eq!(v["profile"]["name"], "cursor-controlled-collaborators-v1");
        assert_eq!(v["tools"]["unicorn"], "2.1.4");
        check_shapes(&v).unwrap();
        v
    }
    struct Controlled<'a> {
        row: &'a Value,
        input: &'a Value,
        events: Vec<Value>,
    }
    impl Host for Controlled<'_> {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            let p = m.malloc(n);
            for i in 0..n {
                m.w8(p + i, u(&self.row["fill"]) as u8);
            }
            self.events.push(json!(["alloc", n, p]));
            Ok(p)
        }
        fn name(&mut self, _m: &mut Space, id: u32) -> Result<u32, String> {
            self.events.push(json!(["name", id]));
            Ok(0x610a64)
        }
        fn lookup(&mut self, _m: &mut Space, name: &str, kind: u32) -> Result<u32, String> {
            self.events.push(json!(["lookup", name, kind]));
            Ok(0x10023)
        }
        fn resolve(&mut self, _m: &mut Space, id: u32) -> Result<u32, String> {
            self.events.push(json!(["resolve", id, 0, 0]));
            Ok(u(&self.row["visual"]))
        }
        fn resolve_kind(&mut self, m: &mut Space, id: u32, kind: u32) -> Result<u32, String> {
            self.events.push(json!(["resolve_kind", id, kind, 0]));
            let raw = self.row["resources"][id.to_string()].as_u64().unwrap_or(0) as u32;
            Ok(if raw != 0 && (kind == 0 || m.r32(raw + 0x4c) == kind) {
                raw
            } else {
                0
            })
        }
        fn sensitivity(&mut self, _m: &mut Space) -> Result<u32, String> {
            self.events.push(json!(["sensitivity", "mouseSensitivity"]));
            Ok(self.row["setting"]
                .as_u64()
                .map(|x| x as u32)
                .unwrap_or(0.625f32.to_bits()))
        }
        fn cursor_backend(
            &mut self,
            _m: &mut Space,
            operation: &str,
            value: u32,
        ) -> Result<(), String> {
            self.events.push(json!(["backend", operation, value]));
            Ok(())
        }
        fn release_render(&mut self, _m: &mut Space, reference: u32) -> Result<(), String> {
            self.events.push(json!(["release_render", reference]));
            Ok(())
        }
        fn color(&mut self, _m: &mut Space, reference: u32, color: u32) -> Result<(), String> {
            self.events.push(json!(["color", reference, color]));
            Ok(())
        }
        fn system_cursor(&mut self, _m: &mut Space) -> Result<Option<u32>, String> {
            self.events.push(json!(["option", "SysCursor", 0]));
            if u(&self.row["exists"]) != 0 {
                self.events.push(json!(["integer", "SysCursor"]));
                Ok(Some(u(&self.row["system"])))
            } else {
                Ok(None)
            }
        }
        fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String> {
            let resource = m.r32(reference + 12);
            if resource != 0 {
                let p = resource + 0xd8 + 0x70;
                m.w32(p, m.r32(p) | mask);
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
            self.events
                .push(json!(["query", reference, message, argument]));
            match message {
                0x19 => Ok(u(&self.input["buttons"][(argument - 0x2d) as usize])),
                0x43 => {
                    let p = 0x30000800 + (argument - 0x2a) * 4;
                    let delta = self.input["delta"][(argument - 0x2a) as usize]
                        .as_f64()
                        .unwrap_or(0.);
                    m.w32(p, (delta as f32).to_bits());
                    Ok(p)
                }
                7 => Ok(0xb),
                1 => Ok(0),
                _ => Err(format!("unexpected query {message:#x}")),
            }
        }
        fn place(
            &mut self,
            m: &mut Space,
            reference: u32,
            translation: u32,
            rotation: [u32; 9],
            name: u32,
        ) -> Result<(), String> {
            self.events.push(json!([
                "place",
                reference,
                name,
                0,
                vector(m, translation),
                rotation
            ]));
            self.events.push(json!(["placed", reference, 0x30000c00]));
            Ok(())
        }
        fn pick(&mut self, m: &mut Space, a: [u32; 10]) -> Result<u32, String> {
            let mut event = vec![json!("pick")];
            event.extend(a.map(|v| json!(v)));
            self.events.push(Value::Array(event));
            let hit = u(&self.input["hit"]);
            crate::native_part::bind_reference(
                m,
                a[1],
                if hit != 0 { u(&self.row["hit"]) } else { 0 },
            );
            link_entity(
                m,
                a[2],
                if self.input["entity"].as_u64().unwrap_or(0) != 0 {
                    u(&self.row["entity"])
                } else {
                    0
                },
            );
            for p in [a[3], a[4]] {
                for (i, word) in [0.5f32.to_bits(), 0.25f32.to_bits(), (-0.125f32).to_bits()]
                    .iter()
                    .enumerate()
                {
                    m.w32(p + i as u32 * 4, *word);
                }
            }
            m.w32(a[5], 0x45);
            m.w32(a[6], 0.375f32.to_bits());
            m.w32(a[9], 0);
            Ok(hit)
        }
        fn command(
            &mut self,
            m: &mut Space,
            reference: u32,
            sender: u32,
            text: &str,
        ) -> Result<(), String> {
            self.events
                .push(json!(["command", reference, sender, 0x10000, text, 0]));
            if self.input["mutate"].as_bool().unwrap_or(false)
                && self.events.iter().filter(|e| e[0] == "command").count() == 1
            {
                m.w32(u(&self.row["body"]) + 0x160, 0x23456789);
            }
            Ok(())
        }
        fn render_cursor(&mut self, m: &mut Space, b: u32) -> Result<(), String> {
            self.events.push(json!([
                "render",
                b + 0x84,
                b + 0x74,
                b + 0x1ac,
                b + 0xa4,
                b + 0x2c,
                m.r32(b + 0x4c),
                2,
                0x3f800000
            ]));
            Ok(())
        }
    }
    #[test]
    fn constructors_match_original_complete_heap_and_calls() {
        let data = vectors();
        let rows = data["constructors"].as_array().unwrap();
        assert_eq!(rows.len(), 32);
        for (i, row) in rows.iter().enumerate() {
            let mut m = Space::new();
            let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
            crate::native_image::ImageData::parse(
                &std::fs::read(root.join("stock/StreetLegal_Redline.bak.exe")).unwrap(),
            )
            .unwrap()
            .initialize(&mut m);
            image(&mut m, &row["before"]);
            globals(&mut m, &row["before_globals"]);
            let mut h = Controlled {
                row,
                input: &Value::Null,
                events: vec![],
            };
            let k = template(
                &mut m,
                if u(&row["owner_bound"]) != 0 {
                    u(&row["owner"])
                } else {
                    0
                },
                &mut h,
            )
            .unwrap();
            assert_eq!(k, u(&row["template"]));
            assert_eq!(
                h.events,
                *row["template_events"].as_array().unwrap(),
                "constructor{i} template events"
            );
            compare(
                &m,
                &row["template_after"],
                &format!("constructor{i} template"),
            );
            h.events.clear();
            let b = construct(&mut m, k, u(&row["reference"]), &mut h).unwrap();
            assert_eq!(b, u(&row["body"]));
            assert_eq!(
                h.events,
                *row["events"].as_array().unwrap(),
                "constructor{i} calls"
            );
            compare(&m, &row["after"], &format!("constructor{i}"));
            compare_globals(&m, &row["globals"], &format!("constructor{i}"));
        }
    }
    #[test]
    fn frame_transitions_match_original_complete_heap_and_ordered_calls() {
        let data = vectors();
        let rows = data["frames"].as_array().unwrap();
        assert_eq!(rows.len(), 30);
        for row in rows {
            let mut m = Space::new();
            image(&mut m, &row["before"]);
            globals(&mut m, &row["before_globals"]);
            for (step_index, step) in row["steps"].as_array().unwrap().iter().enumerate() {
                let input = &step["input"];
                let label = format!("{} step{step_index}", row["label"]);
                for (i, p) in [0x64959c, 0x649598].into_iter().enumerate() {
                    m.w32(
                        p,
                        if input["xy_bits"].is_array() {
                            u(&input["xy_bits"][i])
                        } else {
                            (input["xy"][i].as_f64().unwrap_or(0.) as f32).to_bits()
                        },
                    );
                }
                m.w32(0x6495a4, input["absolute"].as_u64().unwrap_or(1) as u32);
                let mut h = Controlled {
                    row,
                    input,
                    events: vec![],
                };
                let dt = (input["dt"].as_f64().unwrap_or(1. / 60.) as f32).to_bits();
                if input["animate"].as_bool().unwrap_or(false) {
                    animate(&mut m, u(&row["template"]), u(&row["body"]), dt, &mut h).unwrap();
                } else {
                    control(&mut m, u(&row["template"]), u(&row["body"]), dt, &mut h).unwrap();
                }
                assert_eq!(
                    h.events,
                    *step["events"].as_array().unwrap(),
                    "{label} ordered calls"
                );
                compare(&m, &step["after"], &label);
                compare_globals(&m, &step["globals"], &label);
                assert_eq!(step["fpu_depth"], 0);
            }
        }
    }
    #[test]
    fn command_sequences_match_original_complete_heap_and_calls() {
        let data = vectors();
        let rows = data["commands"].as_array().unwrap();
        assert_eq!(rows.len(), 22);
        for (i, row) in rows.iter().enumerate() {
            let mut m = Space::new();
            image(&mut m, &row["before"]);
            globals(&mut m, &row["before_globals"]);
            let mut h = Controlled {
                row,
                input: &Value::Null,
                events: vec![],
            };
            for step in row["steps"].as_array().unwrap() {
                h.events.clear();
                let text = step["text"].as_str().unwrap();
                let label = format!("command{i}: {text}");
                assert_eq!(
                    event(
                        &mut m,
                        u(&row["body"]),
                        u(&step["event"]),
                        text.as_bytes(),
                        &mut h
                    )
                    .unwrap(),
                    u(&step["result"])
                );
                assert_eq!(
                    h.events,
                    *step["events"].as_array().unwrap(),
                    "{label} calls"
                );
                compare(&m, &step["after"], &label);
                compare_globals(&m, &step["globals"], &label);
            }
        }
    }

    #[test]
    fn system_cursor_wrappers_match_original_global_writes_and_api_order() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-cursor-device-original.json"))
                .expect("run native_cursor_device_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(data["fpu_cw"], 0x7f);
        assert_eq!(data["unicorn"], "2.1.4");
        for (field, path) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_cursor_device_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
            ("stock_exe_sha256", "stock/StreetLegal_Redline.exe"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        assert_eq!(
            data["profile_sha256"],
            format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&data["profile"]).unwrap())
            )
        );
        assert_eq!(data["profile"]["physical_device_observed"], false);
        struct ControlledDevice {
            point: [i32; 2],
            events: Vec<Value>,
        }
        impl Device for ControlledDevice {
            fn set_cursor(&mut self, handle: u32) -> Result<(), String> {
                self.events.push(json!(["SetCursor", handle]));
                Ok(())
            }
            fn show_cursor(&mut self, visible: bool) -> Result<(), String> {
                self.events.push(json!(["ShowCursor", u32::from(visible)]));
                Ok(())
            }
            fn clip_cursor(&mut self, rect: Option<[i32; 4]>) -> Result<(), String> {
                self.events.push(json!(["ClipCursor", rect]));
                Ok(())
            }
            fn cursor_position(&mut self) -> Result<[i32; 2], String> {
                self.events.push(json!(["GetCursorPos"]));
                Ok(self.point)
            }
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 4);
        for (i, row) in rows.iter().enumerate() {
            let mut m = Space::new();
            image(&mut m, &row["before"]);
            check_globals(&row["before_globals"], true).unwrap();
            for (address, word) in row["before_globals"].as_object().unwrap() {
                m.w32(address.parse().unwrap(), u(word));
            }
            let mut device = ControlledDevice {
                point: [
                    row["point"][0].as_i64().unwrap() as i32,
                    row["point"][1].as_i64().unwrap() as i32,
                ],
                events: vec![],
            };
            assert_eq!(row["steps"].as_array().unwrap().len(), 18);
            for step in row["steps"].as_array().unwrap() {
                device.events.clear();
                let op = step["operation"].as_str().unwrap();
                let label = format!("device{i} {op}");
                system_command(&mut m, op, u(&step["value"]), &mut device).unwrap();
                assert_eq!(
                    device.events,
                    *step["events"].as_array().unwrap(),
                    "{label}"
                );
                compare(&m, &step["after"], &label);
                check_globals(&step["globals"], true).unwrap();
                for (address, word) in step["globals"].as_object().unwrap() {
                    assert_eq!(
                        m.r32(address.parse().unwrap()),
                        u(word),
                        "{label} global {address}"
                    );
                }
            }
        }
    }
    #[test]
    fn missing_globals_and_truncated_steps_are_rejected() {
        let v = vectors();
        for suite in ["constructors", "frames", "commands"] {
            let mut bad = v.clone();
            bad[suite][0]["before_globals"]
                .as_object_mut()
                .unwrap()
                .clear();
            assert!(check_shapes(&bad).is_err());
        }
        let mut bad = v.clone();
        bad["frames"][0]["steps"].as_array_mut().unwrap().pop();
        assert!(check_shapes(&bad).is_err());
        let mut bad = v.clone();
        bad["commands"][0]["steps"][0]["after"]
            .as_array_mut()
            .unwrap()
            .pop();
        assert!(check_shapes(&bad).is_err());
    }
}

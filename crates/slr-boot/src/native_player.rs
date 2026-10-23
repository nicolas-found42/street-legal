//! Build606 native player class and boot-empty runtime. The real player body
//! is0x2510 bytes (4797d0), distinct from the larger436370 controller body.
//! Resource/configuration/registration collaborators stay explicit and live.
use slr_physics::space::Space;

pub const VTABLE: u32 = 0x5f12f0;
pub const BODY_SIZE: u32 = 0x2510;

pub trait Host {
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String>;
    fn name(&mut self, m: &mut Space, id: u32) -> Result<u32, String>;
    fn lookup(&mut self, m: &mut Space, name: u32, entry: u32) -> Result<u32, String>;
    fn bind(
        &mut self,
        m: &mut Space,
        reference: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<(), String>;
    fn register(
        &mut self,
        m: &mut Space,
        class: u32,
        method: u32,
        signature: u32,
        address: u32,
    ) -> Result<(), String>;
    fn integer(&mut self, m: &mut Space, key: u32) -> Result<u32, String>;
    fn float(&mut self, m: &mut Space, key: u32) -> Result<u32, String>;
    fn subscribe(&mut self, m: &mut Space, reference: u32, mask: u32) -> Result<(), String>;
}

/// Original477bc0. The descriptor's unused first argument is retained in the
/// ABI; the owner is a raw resource container, never a host container identity.
/// exe: FUN_00477bc0
pub fn template(
    m: &mut Space,
    descriptor: u32,
    owner: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    let result = s::function(m, &[descriptor, owner], None, |m| {
        s::push_reg(m, Reg::Ebx);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        let k = s::external(m, 0x477bca, &[0x50], None, |m| host.allocate(m, 0x50))?;
        if k == 0 {
            return Err("NativePlayerBoundary: original477bc0 null allocation dereference".into());
        }
        for offset in (4..=0x14).step_by(4) {
            m.w32(k + offset, 0);
        }
        for offset in (0x20..=0x4c).step_by(4) {
            m.w32(k + offset, 0);
        }
        m.w32(k, VTABLE);
        crate::native_part::bind_reference(m, k + 4, owner);
        s::set_reg(m, Reg::Esi, k);
        s::set_reg(m, Reg::Ebx, owner);
        s::set_reg(m, Reg::Edi, 0);
        let id = m.r32(k + 12);
        let manager = m.r32(0x618d48);
        let name = s::external(m, 0x477c9a, &[id], Some(manager), |m| host.name(m, id))?;
        m.w32(k + 0x18, 0);
        m.w32(k + 0x1c, name);
        Ok(k)
    });
    //477bc0 is cdecl: its caller owns the two incoming argument words.
    s::set_sp(m, s::sp(m).wrapping_sub(8));
    result
}

/// Original477cb0. Loading acquires both stock sounds and registers the seven
/// Controller natives once per process; each class load increments K+18.
/// exe: FUN_00477cb0
pub fn load(m: &mut Space, k: u32, host: &mut dyn Host) -> Result<(), String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[], Some(k), |m| {
        s::push_reg(m, Reg::Esi);
        s::set_reg(m, Reg::Esi, k);
        for (name, lookup_pc, offset, bind_pc) in [
            (0x612624, 0x477cc9, 0x20, 0x477cd2),
            (0x612630, 0x477ce8, 0x30, 0x477cf1),
        ] {
            let manager = m.r32(0x618d48);
            s::push(m, 0);
            s::push(m, 6);
            let id = s::external(m, lookup_pc, &[name, 9], Some(manager), |m| {
                host.lookup(m, name, 9)
            })?;
            s::set_reg(m, Reg::Eax, id);
            s::external(m, bind_pc, &[id], Some(k + offset), |m| {
                host.bind(m, k + offset, id, 6, 0)
            })?;
            s::add_sp(m, 8);
        }
        if m.r32(0x63c86c) == 0 {
            m.w32(0x63c86c, 1);
            for (class, method, sig, address, pc) in [
                (0x612654, 0x612648, 0x61263c, 0x477740, 0x477d27),
                (0x61267c, 0x612670, 0x612668, 0x477810, 0x477d46),
                (0x6126b0, 0x61269c, 0x612690, 0x4778b0, 0x477d65),
                (0x6126e0, 0x6126cc, 0x6126c4, 0x477970, 0x477d84),
                (0x612704, 0x6126f8, 0x6126f4, 0x477a00, 0x477da3),
                (0x612730, 0x612720, 0x612718, 0x477a80, 0x477dc2),
                (0x612760, 0x61274c, 0x612744, 0x477b20, 0x477de1),
            ] {
                let vm = m.r32(0x63641c);
                s::external(m, pc, &[class, method, sig, address], Some(vm), |m| {
                    host.register(m, class, method, sig, address)
                })?;
            }
        }
        m.w32(k + 0x18, m.r32(k + 0x18).wrapping_add(1));
        Ok(())
    })
}

/// Original54d580. Unwritten bytes, including padding in mapping records,
/// retain allocator contents; never zero an entire body to imitate a capture.
fn input_defaults(m: &mut Space, p: u32) {
    for i in 0..0x4c {
        let row = p + 4 + i * 0x30;
        for (offset, value) in [
            (0, 0),
            (4, 0),
            (8, 0),
            (12, 0x3dcccccd),
            (16, 0x3f800000),
            (20, 0x3f800000),
            (24, 0x3f800000),
            (28, 0x3f800000),
            (32, 0x3f800000),
            (36, 0),
            (40, 0),
            (44, 0),
        ] {
            m.w32(row + offset, value);
        }
    }
    for i in 0..0x98 {
        m.w32(p + 0xe44 + i * 0x20, 0);
    }
    m.w32(p + 0x2144, 0);
    m.w32(p, 0);
}

fn activate_input(m: &mut Space, p: u32) {
    m.w32(p + 0x2144, m.r32(0x7686f8));
    m.w32(0x7686f0, m.r32(0x7686f0).wrapping_add(1));
    m.w32(0x7686f8, p);
}

/// Call a cdecl collaborator whose argument words were already pushed by the
/// original caller. Their delayed cleanup and actual CALL frame are retained.
fn prepared<R>(m: &mut Space, pc: u32, f: impl FnOnce(&mut Space) -> R) -> R {
    crate::native_stack::call(m, pc, &[], None, |m| {
        crate::native_stack::function(m, &[], None, f)
    })
}

/// The decimal-only551220 ->553d70 ->554170 stack path used by "Player%d".
/// Its integer buffer ends at554199's [ESP+37]. Preserve the original ECX
/// terminator pointer returned by55425d..55426c; subsequent cdecl settings
/// callbacks observe it. The address follows actual frames, not a captured SP.
fn player_name_frame(m: &mut Space, destination: u32, index: u32) {
    use crate::native_stack::{self as s, Reg};
    s::call(m, 0x47999d, &[], None, |m| {
        s::function_cdecl(m, &[], |m| {
            let arguments = s::sp(m) + 12;
            s::set_reg(m, Reg::Ecx, 0x612854);
            s::set_reg(m, Reg::Edx, destination);
            s::set_reg(m, Reg::Eax, arguments);
            let args = [destination, 0x612854, arguments];
            s::call(m, 0x551234, &args, None, |m| {
                s::function_cdecl(m, &args, |m| {
                    s::alloc(m, 0x40);
                    s::push_reg(m, Reg::Ebp);
                    s::set_reg(m, Reg::Ebp, destination);
                    s::push_reg(m, Reg::Edi);
                    s::set_reg(m, Reg::Edi, 0x612855);
                    s::set_reg(m, Reg::Ecx, arguments);
                    s::set_reg(m, Reg::Edx, arguments - 8);
                    s::push_reg(m, Reg::Ebx);
                    s::store(m, 0x58, arguments - 8);
                    s::push_reg(m, Reg::Esi);
                    let format_sp = s::sp(m);
                    s::set_reg(m, Reg::Edx, arguments - 4);
                    // Six literal bytes followed by a widthless signed decimal.
                    s::set_reg(m, Reg::Ebp, destination + 6);
                    s::set_reg(m, Reg::Edi, 0x61285c);
                    s::set_reg(m, Reg::Ebx, 0);
                    s::set_reg(m, Reg::Esi, u32::MAX);
                    m.w32(format_sp + 0x58, 0x61285c);
                    m.w8(format_sp + 0x54, 0);
                    m.w8(format_sp + 0x10, 0x20);
                    s::set_reg(m, Reg::Ecx, m.r32(format_sp + 0x10));
                    s::set_reg(m, Reg::Eax, index);
                    s::set_reg(m, Reg::Edx, arguments);
                    s::set_reg(m, Reg::Esi, arguments - 4);
                    m.w32(format_sp + 0x54, arguments);
                    m.w32(format_sp + 0x5c, arguments - 4);
                    let decimal = [destination + 6, index, 0, m.r32(format_sp + 0x10)];
                    s::call(m, 0x553eea, &decimal, None, |m| {
                        s::function_cdecl(m, &decimal, |m| {
                            s::set_reg(m, Reg::Ecx, (index as i32).unsigned_abs());
                            s::alloc(m, 0x28);
                            s::store(m, 0x30, if (index as i32) < 0 { u32::MAX } else { 1 });
                            for reg in [Reg::Ebx, Reg::Ebp, Reg::Esi] {
                                s::push_reg(m, reg);
                            }
                            s::set_reg(m, Reg::Esi, destination + 6);
                            s::set_reg(m, Reg::Ebx, 0);
                            s::push_reg(m, Reg::Edi);
                            let terminator = s::sp(m) + 0x37;
                            m.w8(terminator, 0);
                            let digits = (index as i32).unsigned_abs().to_string();
                            for (i, byte) in digits.bytes().rev().enumerate() {
                                m.w8(terminator - 1 - i as u32, byte);
                            }
                            s::store(m, 0x14, digits.len() as u32);
                            s::store(m, 0x10, 0x1e - digits.len() as u32);
                            s::set_reg(m, Reg::Edx, 0);
                            s::set_reg(m, Reg::Ecx, terminator);
                            s::set_reg(
                                m,
                                Reg::Eax,
                                destination
                                    + 6
                                    + digits.len() as u32
                                    + u32::from((index as i32) < 0),
                            );
                        });
                    });
                    s::set_reg(m, Reg::Edx, arguments);
                    s::set_reg(
                        m,
                        Reg::Ebp,
                        destination + 6 + (index as i32).to_string().len() as u32,
                    );
                    s::set_reg(m, Reg::Eax, 0);
                });
            });
        });
    });
}

pub(crate) fn empty_ref(m: &mut Space, p: u32, pc: u32, cross: bool) {
    use crate::native_stack::{self as s, Reg};
    s::call(m, pc, &[], Some(p), |m| {
        s::function(m, &[], Some(p), |m| {
            s::set_reg(m, Reg::Eax, if cross { p } else { 0 });
            if cross {
                s::set_reg(m, Reg::Ecx, 0);
            }
            for i in 0..4 {
                m.w32(p + i * 4, 0);
            }
        })
    });
}
fn reference_array(m: &mut Space, p: u32, count: u32, pc: u32) {
    use crate::native_stack::{self as s, Reg};
    s::call(m, pc, &[p, 0x10, count, 0x42cec0], None, |m| {
        s::function(m, &[p, 0x10, count, 0x42cec0], None, |m| {
            s::set_reg(m, Reg::Eax, count - 1);
            for (r, value) in [
                (Reg::Ebx, 0x42cec0),
                (Reg::Ebp, 0x10),
                (Reg::Esi, p),
                (Reg::Edi, count),
            ] {
                s::push_reg(m, r);
                s::set_reg(m, r, value);
            }
            for i in 0..count {
                empty_ref(m, p + i * 0x10, 0x40187e, true);
                s::set_reg(m, Reg::Esi, p + (i + 1) * 0x10);
                s::set_reg(m, Reg::Edi, count - i - 1);
            }
        })
    });
}

/// Source551240/553890 decimal scanner path, including its real varargs
/// frame. Only the fixed "%d %d %d" format reached by4797d0 is represented.
fn scan_player_parameters(m: &mut Space, k: u32, parameters: &str) -> [u32; 3] {
    use crate::native_stack::{self as s, Reg};
    let base = s::sp(m);
    let pointer = s::load(m, 0x20);
    s::set_reg(m, Reg::Eax, base + 12);
    s::push(m, base + 12);
    s::set_reg(m, Reg::Eax, pointer);
    s::set_reg(m, Reg::Esi, k);
    s::set_reg(m, Reg::Ecx, base + 20);
    s::push(m, base + 20);
    s::set_reg(m, Reg::Edx, base + 16);
    s::push(m, base + 16);
    s::set_reg(m, Reg::Ebx, 0);
    s::push(m, 0x612838);
    s::push(m, pointer);
    for offset in [12, 16, 20] {
        m.w32(base + offset, 0);
    }
    s::call(m, 0x479804, &[], None, |m| {
        s::function_cdecl(m, &[], |m| {
            let destinations = s::sp(m) + 12;
            s::set_reg(m, Reg::Ecx, 0x612838);
            s::set_reg(m, Reg::Edx, pointer);
            s::set_reg(m, Reg::Eax, destinations);
            s::call(m, 0x551254, &[pointer, 0x612838, destinations], None, |m| {
                s::function_cdecl(m, &[], |m| {
                    s::set_reg(m, Reg::Eax, pointer);
                    s::alloc(m, 0x18);
                    for r in [Reg::Ebx, Reg::Ebp, Reg::Esi, Reg::Edi] {
                        s::push_reg(m, r);
                    }
                    let bp = s::sp(m);
                    let mut position = 0;
                    let mut count = 0;
                    let bytes = parameters.as_bytes();
                    m.w32(bp + 0x14, 0);
                    m.w32(bp + 0x1c, pointer);
                    m.w8(bp + 0x13, 0);
                    for field in 0..3 {
                        //5538d9 and553aa0 advance the source word in-place.
                        while bytes
                            .get(position)
                            .is_some_and(|b| matches!(b, 9..=13 | 32))
                        {
                            position += 1;
                            m.w32(bp + 0x2c, pointer + position as u32);
                        }
                        m.w8(bp + 0x12, 0);
                        m.w32(bp + 0x30, 0x61283a + field * 3);
                        m.w32(bp + 0x34, destinations + (field + 1) * 4);
                        let destination = m.r32(destinations + field * 4);
                        s::set_reg(m, Reg::Ecx, 0);
                        let negative = bytes.get(position) == Some(&b'-');
                        let mut cursor = position + usize::from(negative);
                        let start = cursor;
                        let mut value = 0u32;
                        while let Some(byte @ b'0'..=b'9') = bytes.get(cursor) {
                            s::set_reg(m, Reg::Edx, value.wrapping_mul(5));
                            value = value.wrapping_mul(10).wrapping_add((byte - b'0') as u32);
                            cursor += 1;
                        }
                        if cursor == start {
                            break;
                        }
                        if negative {
                            value = value.wrapping_neg();
                        }
                        m.w32(destination, value);
                        position = cursor;
                        count += 1;
                        m.w32(bp + 0x2c, pointer + position as u32);
                        m.w32(bp + 0x14, count);
                        s::set_reg(m, Reg::Ecx, (0x61283a + field * 3) & !0xff);
                    }
                    s::set_reg(m, Reg::Eax, count);
                });
            });
        });
    });
    s::add_sp(m, 20);
    s::set_reg(m, Reg::Eax, m.r32(k + 0x18));
    [m.r32(base + 12), m.r32(base + 16), m.r32(base + 20)]
}

/// Original4797d0. Parameters use the original integer scanner. Host callbacks
/// must perform actual current settings/resource work, not cached answers.
/// exe: FUN_004797d0
pub fn construct(
    m: &mut Space,
    k: u32,
    reference: u32,
    parameters: &str,
    host: &mut dyn Host,
) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[reference, 0], Some(k), |m| {
        s::alloc(m, 0xc);
        s::push_reg(m, Reg::Ebx);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        let values = scan_player_parameters(m, k, parameters);
        if m.r32(k + 0x18) == 0 {
            s::set_reg(m, Reg::Edx, m.r32(k));
            s::call(m, 0x479815, &[], Some(k), |m| load(m, k, host))?;
        }
        let b = s::external(m, 0x47981f, &[BODY_SIZE], None, |m| {
            host.allocate(m, BODY_SIZE)
        })?;
        s::set_reg(m, Reg::Eax, b);
        if b == 0 {
            return Err("NativePlayerBoundary: original4797d0 null allocation dereference".into());
        }
        s::set_reg(m, Reg::Esi, b);
        empty_ref(m, b, 0x479833, false);
        for off in [0x14, 0x18, 0x10] {
            m.w32(b + off, 0);
        }
        s::call(m, 0x479844, &[], Some(b + 0x1c), |m| {
            s::function(m, &[], Some(b + 0x1c), |m| {
                s::push_reg(m, Reg::Ebx);
                s::push_reg(m, Reg::Esi);
                s::set_reg(m, Reg::Eax, b + 0x1c);
                s::push_reg(m, Reg::Edi);
                input_defaults(m, b + 0x1c);
                s::set_reg(m, Reg::Edx, b + 0x1c + 0x2144);
            })
        });
        for (off, pc) in [
            (0x2164, 0x47984f),
            (0x2174, 0x47985a),
            (0x2184, 0x479865),
            (0x2194, 0x479870),
            (0x21a4, 0x47987b),
        ] {
            empty_ref(m, b + off, pc, false);
        }
        empty_ref(m, b + 0x21c8, 0x479886, true);
        reference_array(m, b + 0x2310, 16, 0x47989b);
        empty_ref(m, b + 0x2450, 0x4798a6, true);
        reference_array(m, b + 0x2460, 8, 0x4798bb);
        empty_ref(m, b + 0x24e4, 0x4798c6, true);
        empty_ref(m, b + 0x24f4, 0x4798d1, true);
        let owner = m.r32(reference + 12);
        crate::native_part::bind_reference(m, b, owner);
        s::set_reg(m, Reg::Eax, owner);
        s::set_reg(
            m,
            Reg::Edx,
            if owner == 0 { 0 } else { m.r32(owner + 0x50) },
        );
        s::call(m, 0x47995e, &[], Some(b + 0x1c), |m| {
            s::function(m, &[], Some(b + 0x1c), |m| {
                activate_input(m, b + 0x1c);
                s::set_reg(m, Reg::Eax, 1);
            })
        });
        s::push(m, 0x612844);
        let display = prepared(m, 0x479968, |m| host.integer(m, 0x612844))?;
        s::set_reg(m, Reg::Eax, display);
        m.w32(b + 0x250c, display);
        m.w32(b + 0x2410, 0);
        m.w32(b + 0x230c, 0);
        m.w32(b + 0x2428, values[0]);
        s::add_sp(m, 4);
        let name = format!("Player{}", values[0] as i32);
        for (i, byte) in name.bytes().chain([0]).enumerate() {
            m.w8(b + 0x2430 + i as u32, byte);
        }
        //479998 leaves sprintf's destination/format/index arguments below
        //the three subsequent setting lookups until4799f7 cleans24 bytes.
        for value in [values[0], 0x612854, b + 0x2430] {
            s::push(m, value);
        }
        player_name_frame(m, b + 0x2430, values[0]);
        for offset in [0x2414, 0x2418, 0x241c, 0x2420, 0x2424, 0x242c] {
            m.w32(b + offset, 0);
        }
        for (key, pc, offset) in [
            (0x612860, 0x4799cb, 0x21c4),
            (0x61287c, 0x4799db, 0x2504),
            (0x612894, 0x4799eb, 0x2508),
        ] {
            s::push(m, key);
            let value = prepared(m, pc, |m| host.float(m, key))?;
            m.w32(b + offset, value);
        }
        s::add_sp(m, 0x18);
        let manager = m.r32(0x618d48);
        s::push(m, 0);
        s::push(m, 0xe);
        let resource = s::external(m, 0x479a09, &[0x6128ac, 0x23], Some(manager), |m| {
            host.lookup(m, 0x6128ac, 0x23)
        })?;
        s::set_reg(m, Reg::Eax, resource);
        s::external(m, 0x479a15, &[resource], Some(b + 0x2450), |m| {
            host.bind(m, b + 0x2450, resource, 0xe, 0)
        })?;
        s::add_sp(m, 8);
        m.w32(b + 0x24e0, 0);
        m.w32(b + 0x21c0, 0x40400000);
        m.w32(b + 0x21d8, values[1]);
        for i in 0..0x4c {
            m.w32(b + 0x21dc + i * 4, 0);
        }
        s::set_reg(m, Reg::Edi, b + 0x230c);
        s::set_reg(m, Reg::Ecx, 0);
        s::set_reg(m, Reg::Eax, 0);
        m.w32(b + 0x10, m.r32(b + 0x10) | 1);
        s::external(m, 0x479a52, &[values[2]], Some(b + 0x21c8), |m| {
            host.bind(m, b + 0x21c8, values[2], 0, 0)
        })?;
        s::external(m, 0x479a5b, &[b, 0x10, 0], None, |m| {
            host.subscribe(m, b, 0x10)
        })?;
        Ok(b)
    })
}

/// exe: FUN_0047a5f0
pub fn control(m: &mut Space, k: u32, b: u32, dt: u32) -> Result<(), String> {
    crate::native_stack::function(m, &[b, dt], Some(k), |m| {
        if (m.r32(b + 0x230c).wrapping_sub(1) as i32) >= 0 {
            return Err(
                "NativePlayerBoundary: nonempty control target traversal is not ported".into(),
            );
        }
        Ok(())
    })
}

/// exe: FUN_0047a780
pub fn animate(m: &mut Space, k: u32, b: u32, dt: u32) -> Result<(), String> {
    crate::native_stack::function(m, &[b, dt], Some(k), |m| {
        m.w32(0x63c874, dt);
        if m.r32(b + 0x21ac) != 0 {
            return Err("NativePlayerBoundary: active player animation is not ported".into());
        }
        for i in 0..8 {
            if m.r32(b + 0x2468 + i * 16) != 0 {
                return Err(format!(
                    "NativePlayerBoundary: render-link animation slot{i} is not ported"
                ));
            }
        }
        Ok(())
    })
}

/// exe: FUN_0047b220
pub fn get_info(m: &mut Space, k: u32, b: u32, message: u32, arg: u32) -> Result<u32, String> {
    use crate::native_stack::{self as s, Reg};
    s::function(m, &[b, message, arg], Some(k), |m| {
        let selector = message.wrapping_sub(8);
        s::set_reg(m, Reg::Eax, selector);
        s::push_reg(m, Reg::Esi);
        s::push_reg(m, Reg::Edi);
        if selector <= 0x3c {
            s::set_reg(
                m,
                Reg::Ecx,
                match message {
                    8 | 28 => 0,
                    21 => 1,
                    25 => 2,
                    26 => 3,
                    27 => 4,
                    67 => 5,
                    68 => 6,
                    _ => 7,
                },
            );
        }
        let result = match message {
            8 | 0x1c => 1,
            21 => {
                s::set_reg(m, Reg::Edx, b);
                m.r32(b + 0x242c)
            }
            26 => {
                if arg != 0 {
                    crate::native_part::bind_reference(m, arg, m.r32(b + 0x21b0));
                }
                m.r32(b + 0x21ac)
            }
            27 => b + 0x1c,
            25 if arg == 0 || arg >= 0x4c => 0,
            67 if arg == 0 => 0x63c870,
            67 if arg >= 0x4c => {
                m.w32(0x63c870, 0);
                0x63c870
            }
            68 if arg == 0 || m.r32(b + 0x10) & 2 != 0 => 1,
            25 | 67 | 68 => {
                return Err(format!(
                    "NativePlayerBoundary: getInfo{message} live input mapping is not ported"
                ))
            }
            _ => 0,
        };
        s::set_reg(m, Reg::Eax, result);
        Ok(result)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::native_stack as stack;
    use serde_json::{json, Value};
    use sha2::{Digest, Sha256};
    const GLOBALS: [u32; 6] = [0x63c86c, 0x7686f0, 0x7686f8, 0x63c870, 0x63c874, 0x6439c0];
    fn w(v: &Value) -> u32 {
        u32::try_from(v.as_u64().expect("unsigned machine word")).expect("32-bit machine word")
    }
    fn globals(m: &Space) -> Value {
        json!(GLOBALS
            .into_iter()
            .map(|a| (a.to_string(), m.r32(a)))
            .collect::<std::collections::BTreeMap<_, _>>())
    }
    fn heap(m: &Space, words: usize) -> Value {
        json!((0..words)
            .map(|i| m.r32(0x20000000 + i as u32 * 4))
            .collect::<Vec<_>>())
    }
    fn expected_sp(arguments: u32, cleanup: u32) -> u32 {
        (((0x7ff00000u32 - 0x100 - arguments * 4 - 4) & !15) - 4) + 4 + cleanup
    }
    fn setup(case: &Value, phase: &Value) -> Space {
        validate_phase(phase).unwrap();
        let mut m = Space::new();
        for (i, value) in phase["before"].as_array().unwrap().iter().enumerate() {
            m.w32(0x20000000 + i as u32 * 4, w(value));
        }
        for a in GLOBALS {
            m.w32(a, w(&phase["globals_before"][a.to_string()]));
        }
        m.w32(0x618d48, 0x30080000);
        m.w32(0x63641c, 0x30090000);
        assert_eq!(
            phase["before"].as_array().unwrap().len(),
            w(&case["layout"]["words"]) as usize
        );
        m
    }
    fn validate_phase(phase: &Value) -> Result<(), &'static str> {
        const WORDS: usize = 0x2b00 / 4;
        for field in ["before", "after"] {
            if phase[field].as_array().map(Vec::len) != Some(WORDS)
                || phase[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v.as_u64().is_none_or(|n| n > u32::MAX as u64))
            {
                return Err("incomplete caller heap");
            }
        }
        for field in ["globals_before", "globals_after"] {
            if phase[field].as_object().map(serde_json::Map::len) != Some(GLOBALS.len())
                || GLOBALS
                    .iter()
                    .any(|a| phase[field][a.to_string()].as_u64().is_none())
            {
                return Err("incomplete caller globals");
            }
        }
        for event in phase["events"]
            .as_array()
            .ok_or("missing callback observations")?
        {
            if event["heap"].as_array().map(Vec::len) != Some(WORDS)
                || event["esp"].as_u64().is_none()
                || event["return_pc"].as_u64().is_none()
                || event["args"].as_array().is_none()
            {
                return Err("incomplete callback heap or CALL identity");
            }
        }
        Ok(())
    }
    struct Controlled<'a> {
        case: &'a Value,
        events: Vec<Value>,
    }
    impl Controlled<'_> {
        fn record(&mut self, m: &Space, name: &str, n: usize) {
            self.events
                .push(json!({"name":name,"this":stack::reg(m,stack::Reg::Ecx),
                "args":(0..n).map(|i|m.r32(stack::sp(m)+4+i as u32*4)).collect::<Vec<_>>(),
                "esp":stack::sp(m),"return_pc":m.r32(stack::sp(m)),
                "heap":heap(m,w(&self.case["layout"]["words"]) as usize),"globals":globals(m)}));
        }
    }
    impl Host for Controlled<'_> {
        fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
            self.record(m, "allocate", 1);
            Ok(w(
                &self.case["layout"][if bytes == 0x50 { "k" } else { "body" }]
            ))
        }
        fn name(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
            self.record(m, "name", 1);
            Ok(0x30070000)
        }
        fn lookup(&mut self, m: &mut Space, name: u32, _: u32) -> Result<u32, String> {
            self.record(m, "lookup", 2);
            Ok(match name {
                0x612624 => 0xffff2001,
                0x612630 => 0xffff2002,
                0x6128ac => 0xffff2003,
                _ => panic!("unexpected lookup"),
            })
        }
        fn bind(
            &mut self,
            m: &mut Space,
            reference: u32,
            id: u32,
            kind: u32,
            _: u32,
        ) -> Result<(), String> {
            self.record(m, "bind", if kind == 0 { 1 } else { 3 });
            let raw = std::iter::once(&self.case["layout"]["owner"])
                .chain(self.case["layout"]["resources"].as_array().unwrap())
                .map(w)
                .find(|raw| m.r32(raw + 0x50) == id)
                .unwrap_or(0);
            crate::native_part::bind_reference(m, reference, raw);
            Ok(())
        }
        fn register(
            &mut self,
            m: &mut Space,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            self.record(m, "register", 4);
            Ok(())
        }
        fn integer(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
            self.record(m, "integer", 1);
            Ok(0x12345678)
        }
        fn float(&mut self, m: &mut Space, key: u32) -> Result<u32, String> {
            self.record(m, "float", 1);
            Ok(match key {
                0x612860 => 1.5f32,
                0x61287c => 0.25,
                0x612894 => -2.,
                _ => panic!("unexpected setting"),
            }
            .to_bits())
        }
        fn subscribe(&mut self, m: &mut Space, _: u32, _: u32) -> Result<(), String> {
            self.record(m, "subscribe", 3);
            Ok(())
        }
    }
    #[test]
    fn original_player_constructors_empty_frames_and_queries_match_complete_state() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-player-original.json"))
                .expect("run native_player_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        for (field, path) in [
            ("exe_sha256", "stock/StreetLegal_Redline.bak.exe"),
            ("producer_sha256", "re/oracle/native_player_probe.py"),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            assert_eq!(
                data[field],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(path)).unwrap())
                )
            );
        }
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 32);
        let actual = rows
            .iter()
            .map(|r| {
                (
                    w(&r["fill"]),
                    w(&r["loaded"]),
                    w(&r["registered"]),
                    r["parameters"].as_str().unwrap(),
                )
            })
            .collect::<std::collections::BTreeSet<_>>();
        let mut expected = std::collections::BTreeSet::new();
        for fill in [0, 0xa5] {
            for loaded in [0, 3] {
                for registered in [0, 1] {
                    for parameters in ["0 0 0", "7 -3 -57341", "-12 19 0", "4 junk"] {
                        expected.insert((fill, loaded, registered, parameters));
                    }
                }
            }
        }
        assert_eq!(actual, expected);
        let complete = &rows[0]["constructor"];
        let mut truncated = complete.clone();
        truncated["after"].as_array_mut().unwrap().pop();
        assert!(validate_phase(&truncated).is_err());
        let mut overflow = complete.clone();
        overflow["before"][0] = json!(0x100000000u64);
        assert!(validate_phase(&overflow).is_err());
        let mut missing_call = complete.clone();
        missing_call["events"][0]
            .as_object_mut()
            .unwrap()
            .remove("return_pc");
        assert!(validate_phase(&missing_call).is_err());
        for (index, case) in rows.iter().enumerate() {
            for name in ["template", "constructor"] {
                let phase = &case[name];
                let mut m = setup(case, phase);
                let mut host = Controlled {
                    case,
                    events: vec![],
                };
                let k = w(&case["layout"]["k"]);
                let body = w(&case["layout"]["body"]);
                let result = if name == "template" {
                    template(&mut m, 0x1a000006, w(&case["layout"]["owner"]), &mut host)
                } else {
                    construct(
                        &mut m,
                        k,
                        w(&case["layout"]["reference"]),
                        case["parameters"].as_str().unwrap(),
                        &mut host,
                    )
                }
                .unwrap_or_else(|e| panic!("case{index} {name}: {e}"));
                assert_eq!(result, w(&phase["result"]), "case{index} {name} return");
                let expected = phase["events"].as_array().unwrap();
                assert_eq!(
                    host.events.len(),
                    expected.len(),
                    "case{index} {name} callback count"
                );
                for (call, (actual, expected)) in host.events.iter().zip(expected).enumerate() {
                    for field in [
                        "name",
                        "this",
                        "args",
                        "esp",
                        "return_pc",
                        "heap",
                        "globals",
                    ] {
                        assert_eq!(
                            actual[field], expected[field],
                            "case{index} {name} call{call} {field}"
                        );
                    }
                }
                assert_eq!(
                    heap(&m, w(&case["layout"]["words"]) as usize),
                    phase["after"],
                    "case{index} {name} complete heap"
                );
                assert_eq!(
                    globals(&m),
                    phase["globals_after"],
                    "case{index} {name} globals"
                );
                let cleanup = if name == "template" { 0 } else { 8 };
                assert_eq!(phase["esp_delta"], cleanup);
                assert_eq!(stack::sp(&m), expected_sp(2, cleanup));
                assert_eq!(phase["fpu_cw"], 0x7f);
                if name == "constructor" {
                    assert_eq!(result, body);
                    assert_eq!(phase["fpu_depth"], 0);
                    for (name, reg) in [
                        ("ebx", stack::Reg::Ebx),
                        ("esi", stack::Reg::Esi),
                        ("edi", stack::Reg::Edi),
                        ("ebp", stack::Reg::Ebp),
                    ] {
                        assert_eq!(stack::reg(&m, reg), w(&phase["nonvolatile"][name]));
                    }
                }
            }
            let k = w(&case["layout"]["k"]);
            let b = w(&case["layout"]["body"]);
            let frames = case["frames"].as_array().unwrap();
            assert_eq!(frames.len(), 8);
            let frame_inventory = frames
                .iter()
                .map(|f| (w(&f["entry"]), w(&f["dt"])))
                .collect::<std::collections::BTreeSet<_>>();
            let expected_frames = [0x47a5f0, 0x47a780]
                .into_iter()
                .flat_map(|e| [0, 0x3c888889, 0x80000000, 0x7fc01234].map(|d| (e, d)))
                .collect();
            assert_eq!(frame_inventory, expected_frames);
            for frame in frames {
                let mut m = setup(case, frame);
                if frame["entry"] == 0x47a5f0 {
                    control(&mut m, k, b, w(&frame["dt"])).unwrap();
                } else if frame["entry"] == 0x47a780 {
                    animate(&mut m, k, b, w(&frame["dt"])).unwrap();
                } else {
                    panic!("unknown player frame entry");
                }
                assert_eq!(
                    heap(&m, w(&case["layout"]["words"]) as usize),
                    frame["after"],
                    "case{index} empty frame heap"
                );
                assert_eq!(
                    globals(&m),
                    frame["globals_after"],
                    "case{index} empty frame globals"
                );
                assert_eq!(frame["events"], json!([]));
                assert_eq!(frame["esp_delta"], 8);
                assert_eq!(stack::sp(&m), expected_sp(2, 8));
                assert_eq!(frame["fpu_cw"], 0x7f);
            }
            let queries = case["queries"].as_array().unwrap();
            assert_eq!(queries.len(), 75);
            let query_inventory = queries
                .iter()
                .map(|q| (w(&q["message"]), w(&q["arg"])))
                .collect::<std::collections::BTreeSet<_>>();
            let expected_queries = (0..70)
                .map(|message| (message, 0))
                .chain([
                    (25, 76),
                    (25, u32::MAX),
                    (67, 76),
                    (67, u32::MAX),
                    (26, w(&case["layout"]["output"])),
                ])
                .collect();
            assert_eq!(query_inventory, expected_queries);
            for q in queries {
                let mut m = setup(case, q);
                let result = get_info(&mut m, k, b, w(&q["message"]), w(&q["arg"])).unwrap();
                assert_eq!(result, w(&q["result"]), "case{index} query{}", q["message"]);
                assert_eq!(heap(&m, w(&case["layout"]["words"]) as usize), q["after"]);
                assert_eq!(globals(&m), q["globals_after"]);
                assert_eq!(q["events"], json!([]));
                assert_eq!(q["esp_delta"], 12);
                assert_eq!(stack::sp(&m), expected_sp(3, 12));
                assert_eq!(q["fpu_cw"], 0x7f);
            }
        }
    }
    #[test]
    fn unrecovered_player_branches_fail_at_their_real_boundary() {
        let mut m = Space::new();
        let b = m.alloc(BODY_SIZE);
        let k = m.alloc(0x50);
        m.w32(b + 0x230c, 1);
        assert!(control(&mut m, k, b, 0)
            .unwrap_err()
            .contains("target traversal"));
        m.w32(b + 0x21ac, 1);
        assert!(animate(&mut m, k, b, 0x80000000)
            .unwrap_err()
            .contains("active player"));
        assert_eq!(m.r32(0x63c874), 0x80000000);
        m.w32(b + 0x21ac, 0);
        m.w32(b + 0x2468 + 7 * 16, 1);
        assert!(animate(&mut m, k, b, 0).unwrap_err().contains("slot7"));
        for message in [25, 67, 68] {
            assert!(get_info(&mut m, k, b, message, 1)
                .unwrap_err()
                .contains("live input mapping"));
        }
    }
}

//! Original458c00: the configured car template's event handler (template
//! vtable 5f0f18 slot +38). Event 0x10 carries a text command; this module
//! ports the attachment command family and the scalar control commands. Known
//! driving/AI collaborators still fail explicitly until their own ports land.
use crate::native_install::{bind, first, next, unlink, Install};
use slr_physics::space::Space;

/// Counted entity link `{target, next}`: the target's +0 heads its links and
/// +4 counts them (the inline sequences around 45fb80).
fn link_entity(m: &mut Space, link: u32, target: u32) {
    if m.r32(link) != 0 {
        unlink_entity(m, link);
    }
    m.w32(link, target);
    if target == 0 {
        m.w32(link + 4, 0);
    } else {
        m.w32(link + 4, m.r32(target));
        m.w32(target, link);
        m.w32(target + 4, m.r32(target + 4) + 1);
    }
}
/// Original45fb80 (and its inline copies): remove a counted entity link.
pub fn unlink_entity(m: &mut Space, link: u32) {
    let target = m.r32(link);
    if target == 0 {
        return;
    }
    let mut previous = 0;
    let mut current = m.r32(target);
    while current != 0 {
        if current == link {
            let following = m.r32(current + 4);
            if previous == 0 {
                m.w32(target, following);
            } else {
                m.w32(previous + 4, following);
            }
            m.w32(target + 4, m.r32(target + 4).wrapping_sub(1));
            m.w32(link, 0);
            m.w32(link + 4, 0);
            return;
        }
        previous = current;
        current = m.r32(current + 4);
    }
}
/// Detach a list node {vtable, next, previous} (429150's tail).
fn detach_node(m: &mut Space, node: u32) {
    m.w32(node, 0x5f09ac);
    let previous = m.r32(node + 8);
    let following = m.r32(node + 4);
    if previous != 0 && following != 0 {
        m.w32(following + 8, previous);
        m.w32(previous + 4, following);
    }
    m.w32(node + 4, 0);
    m.w32(node + 8, 0);
}
/// Release a weak reference embedded at +c (45f8b0 / 45f9d0 head).
fn release_embedded(m: &mut Space, node: u32) {
    let link = node + 0xc;
    if m.r32(link + 12) == 0 {
        m.w32(link + 8, 0);
    } else {
        unlink(m, link);
    }
}
/// Virtual destructor `vtable[0](1)` of the car's attachment list nodes.
/// The bump allocator never reuses the freed block.
pub fn destroy_node(m: &mut Space, node: u32) -> Result<(), String> {
    match m.r32(m.r32(node)) {
        // 45f8b0: light / registration node with an embedded reference.
        0x45f8b0 => {
            release_embedded(m, node);
            detach_node(m, node);
        }
        // 45f9d0: linked render node: two counted links, then the reference.
        0x45f9d0 => {
            unlink_entity(m, node + 0x1c);
            unlink_entity(m, node + 0x24);
            release_embedded(m, node);
            detach_node(m, node);
        }
        // 45e560: wing node with one counted link at +10.
        0x45e560 => {
            unlink_entity(m, node + 0x10);
            detach_node(m, node);
        }
        // 429150: plain list node (mount slot records).
        0x429150 => detach_node(m, node),
        other => {
            return Err(format!(
                "NativeMessageBoundary: car node destructor {other:#x}"
            ))
        }
    }
    Ok(())
}

fn text(m: &Space, mut p: u32) -> Vec<u8> {
    let mut out = vec![];
    while m.r8(p) != 0 {
        out.push(m.r8(p));
        p += 1;
    }
    out
}
fn scan_words<const N: usize>(input: &[u8], format: &[u8], defaults: [u32; N]) -> [u32; N] {
    let mut words = defaults;
    {
        let mut args: Vec<slr_lang::scan::ScanArg<'_>> = words
            .iter_mut()
            .map(slr_lang::scan::ScanArg::Word)
            .collect();
        let _ = slr_lang::scan::sscanf(input, format, &mut args);
    }
    words
}

impl Install<'_> {
    /// Original458c00 (part attachment command family of event 0x10).
    /// exe: FUN_00458c00
    pub fn car_command(
        &mut self,
        k: u32,
        body: u32,
        event: u32,
        argument: u32,
    ) -> Result<u32, String> {
        if event != 0x10 {
            return match event {
                0x80 if argument == 0 => Ok(1),
                0x80 if argument > 3 => Ok(0),
                0x200 if argument == 0 => Ok(0),
                0x20 if (self.m.r32(k + 0x1f40) as i32) < 1 => Ok(0),
                1 | 4 | 0x20 | 0x80 | 0x200 | 0x40_0000 => {
                    Err(format!("NativeMessageBoundary: car458c00 event{event:#x}"))
                }
                _ => Ok(0),
            };
        }
        let saved = crate::native_stack::sp(self.m);
        let result = self.car_text_command(k, body, argument);
        crate::native_stack::set_sp(self.m, saved);
        result
    }

    fn car_text_command(&mut self, k: u32, body: u32, argument: u32) -> Result<u32, String> {
        let input = text(self.m, argument);
        let mut verb = vec![];
        let _ = slr_lang::scan::sscanf(
            &input,
            b"%s",
            &mut [slr_lang::scan::ScanArg::Str(&mut verb)],
        );
        match verb.as_slice() {
            b"add_part" => {
                let [part] = scan_words(&input, b"%*s %d", [0]);
                self.car_add_part(k, body, part)?;
                let record = self.car_part_dynamics(k, body, part)?;
                if record != 0 {
                    self.m.w32(record + 0x88, self.m.r32(record + 0x88) | 1);
                }
                Ok(1)
            }
            b"rem_part" => {
                let [part] = scan_words(&input, b"%*s %d", [0]);
                self.car_remove_dynamics(k, body, part);
                self.car_remove_part(k, body, part)?;
                Ok(1)
            }
            b"updatevariables" => {
                self.m.w32(body + 0x70, self.m.r32(body + 0x70) | 0x80_0000);
                Ok(1)
            }
            b"add_light" => {
                let [render, flags] = scan_words(&input, b"%*s %d %d", [0, 0]);
                self.car_add_light(body, render, flags)
            }
            b"rem_light" => {
                let [id, flags] = scan_words(&input, b"%*s %d %d", [0, 0]);
                self.car_remove_light(body, id, flags)?;
                Ok(1)
            }
            b"add_wing" => {
                let [wing, entity] = scan_words(&input, b"%*s %d %d", [0, 0]);
                let m = &mut *self.m;
                let node = m.malloc(0x1c);
                if node != 0 {
                    m.w32(node + 8, 0);
                    m.w32(node + 4, 0);
                    m.w32(node + 0x10, 0);
                    m.w32(node + 0x14, 0);
                    m.w32(node, 0x5f0fc8);
                    m.w32(node + 0xc, wing);
                    link_entity(m, node + 0x10, entity);
                    m.w32(node + 0x18, 0);
                }
                append(m, body + 0x2088, node);
                Ok(1)
            }
            b"rem_wing" => {
                let [wing, entity] = scan_words(&input, b"%*s %d %d", [0, 0]);
                let m = &mut *self.m;
                let mut node = first(m, body + 0x2078);
                while node != 0 {
                    if m.r32(node + 0xc) == wing && m.r32(node + 0x10) == entity {
                        destroy_node(m, node)?;
                        break;
                    }
                    node = next(m, node);
                }
                Ok(1)
            }
            b"add_rollbar" | b"rem_rollbar" => {
                // Integer cells start at -1; the remaining cells are floats.
                let (mut a, mut b) = (u32::MAX, u32::MAX);
                let (mut c, mut d) = (10000f32, 500f32);
                let _ = slr_lang::scan::sscanf(
                    &input,
                    b"%*s %d %d %f %f",
                    &mut [
                        slr_lang::scan::ScanArg::Word(&mut a),
                        slr_lang::scan::ScanArg::Word(&mut b),
                        slr_lang::scan::ScanArg::Float(&mut c),
                        slr_lang::scan::ScanArg::Float(&mut d),
                    ],
                );
                let values = [a, b, c.to_bits(), d.to_bits()];
                if verb == b"add_rollbar" {
                    self.car_add_rollbar(body, values)
                } else {
                    self.car_remove_rollbar(body, values);
                    Ok(1)
                }
            }
            b"add_linked" => {
                let values = scan_words(&input, b"%*s %d %d %d %d", [0, u32::MAX, 0, 0]);
                self.car_add_linked(body, values)
            }
            b"rem_linked" => {
                let [id, wheel, _, _] = scan_words(&input, b"%*s %d %d %d %d", [0, u32::MAX, 0, 0]);
                self.car_remove_linked(body, id, wheel)?;
                Ok(1)
            }
            b"add_mslot" => {
                let [slot, descriptor] = scan_words(&input, b"%*s %d %d", [0, 0]);
                let m = &mut *self.m;
                let record = m.malloc(0x54);
                if record != 0 {
                    m.w32(record + 4, 0);
                    m.w32(record + 8, 0);
                    m.w32(record, 0x5f1184);
                    m.w32(record + 0xc, slot);
                    m.w32(record + 0x10, descriptor);
                    m.w32(record + 0x14, 0);
                    for i in 0..3 {
                        m.w32(record + 0x18 + i * 4, m.r32(slot + 0x2c + i * 4));
                    }
                    for i in 0..12 {
                        m.w32(record + 0x24 + i * 4, m.r32(slot + 0x38 + i * 4));
                    }
                }
                append(m, body + 0x20a4, record);
                Ok(1)
            }
            b"rem_mslot" => {
                let [slot, descriptor] = scan_words(&input, b"%*s %d %d", [0, 0]);
                let m = &mut *self.m;
                let mut node = first(m, body + 0x2094);
                while node != 0 {
                    if m.r32(node + 0xc) == slot && m.r32(node + 0x10) == descriptor {
                        destroy_node(m, node)?;
                        break;
                    }
                    node = next(m, node);
                }
                Ok(1)
            }
            b"transmission" => {
                let [mode] = scan_words(&input, b"%*s %d", [0]);
                let mut flags = self.m.r32(body + 0x70) & !0x30;
                flags = if mode & 2 == 0 {
                    flags | 0x40
                } else {
                    flags & !0x40
                };
                if mode & 1 != 0 {
                    flags |= if mode & 4 == 0 { 0x10 } else { 0x20 };
                }
                self.m.w32(body + 0x70, flags);
                self.m.w32(body + 0x1f44, self.m.r32(body + 0x1dbc));
                Ok(1)
            }
            b"damage_multiplier" | b"difflock" | b"steerhelp" | b"abs" | b"asr" | b"esp" => {
                let mut value = if verb == b"damage_multiplier" {
                    1.0
                } else {
                    0.0
                };
                let _ = slr_lang::scan::sscanf(
                    &input,
                    b"%*s %f",
                    &mut [slr_lang::scan::ScanArg::Float(&mut value)],
                );
                self.car_scalar_control(k, body, &verb, value.to_bits());
                Ok(1)
            }
            b"team" | b"sethorn" => {
                let [value] = scan_words(&input, b"%*s %d", [0]);
                let (offset, value) = if verb == b"team" {
                    (0x1edc, value)
                } else {
                    (0x1dcc, ((value as i32) % 4) as u32)
                };
                self.m.w32(body + offset, value);
                Ok(1)
            }
            b"brake" => {
                self.m.w32(body + 0x70, self.m.r32(body + 0x70) | 0x40000);
                Ok(1)
            }
            b"idle" => {
                let data = self.m.r32(body + 0x1fbc);
                self.m.w32(body + 0x1e18, self.m.r32(data + 0xec));
                Ok(1)
            }
            b"autopilot_on" | b"autopilot_off" => {
                for i in 0..(self.m.r32(k + 0x177c) as i32).max(0) as u32 {
                    self.m
                        .w32(body + 0x180 + i * 0x4a8, u32::from(verb == b"autopilot_on"));
                }
                Ok(1)
            }
            other
                if [
                    b"render".as_slice(),
                    b"hide",
                    b"activate",
                    b"deactivate",
                    b"next",
                    b"prev",
                    b"corpse",
                    b"osd",
                    b"cruise",
                    b"pickup",
                    b"stop",
                    b"start",
                    b"suspend",
                    b"wakeup",
                    b"reset",
                    b"reload",
                    b"setsteer",
                    b"setpitch",
                    b"addroll",
                    b"setvel",
                    b"install",
                    b"remove",
                    b"cancel",
                    b"repair",
                    b"texture",
                    b"mesh",
                    b"bigdecal",
                    b"decal",
                    b"paint",
                    b"filter",
                    b"ai_cheat",
                ]
                .contains(&other) =>
            {
                Err(format!(
                    "NativeMessageBoundary: car458c00 command {}",
                    String::from_utf8_lossy(other)
                ))
            }
            _ => Ok(0),
        }
    }

    fn car_scalar_control(&mut self, k: u32, body: u32, verb: &[u8], value: u32) {
        let m = &mut *self.m;
        let data = m.r32(body + 0x1fbc);
        if verb == b"damage_multiplier" {
            m.w32(body + 0x1ef0, value);
            return;
        }
        let f = f32::from_bits(value);
        // The executable's <= branch differs from ESP's ordered < branch
        // for NaNs; preserve the individual comparisons.
        match verb {
            b"abs" | b"asr" => {
                let (bit, offset) = if verb == b"abs" {
                    (0x80, 0xa28)
                } else {
                    (0x100, 0xa2c)
                };
                if f <= m.rf(0x5e73cc) {
                    m.w32(body + 0x70, m.r32(body + 0x70) & !bit);
                } else {
                    m.w32(body + 0x70, m.r32(body + 0x70) | bit);
                    let mut fp = crate::native_fpu::Fpu::new();
                    fp.fld(m, 0x5f08f0);
                    fp.push(slr_x87::X87::from_f32(f));
                    fp.arith_st0(crate::native_fpu::Op::DivR, 1);
                    m.w32(data + offset, fp.fstp_bits());
                }
            }
            b"esp" => {
                if m.rf(0x5e73cc) < f {
                    m.w32(body + 0x70, m.r32(body + 0x70) | 0x4000000);
                    m.w32(data + 0xa30, value);
                } else {
                    m.w32(body + 0x70, m.r32(body + 0x70) & !0x4000000);
                    for i in 0..(m.r32(k + 0x1f40) as i32).max(0) as u32 {
                        m.w32(m.r32(body + 0x13e4) + i * 0x2b4 + 0xfc, 0);
                    }
                }
            }
            b"difflock" => m.w32(
                data + 0xa24,
                if f <= m.rf(0x5e73cc) {
                    0x3dcccccd
                } else {
                    value
                },
            ),
            b"steerhelp" => {
                let flags = m.r32(body + 0x70);
                if f <= m.rf(0x5e73cc) {
                    m.w32(body + 0x70, flags | 0x200);
                    m.w32(data + 0xa20, 0);
                } else {
                    m.w32(body + 0x70, flags & !0x200);
                    m.w32(data + 0xa20, value);
                }
            }
            _ => unreachable!(),
        }
    }

    /// Original48bc40/48bd30: create/find a named skeleton id, then remove
    /// that bone, retaining both ordered resource accesses and allocation.
    fn remove_named_bone(&mut self, link: u32, name: &str) -> Result<(), String> {
        let id = self.bone_access(link, Some(name), 0)?;
        self.bone_access(link, None, id)?;
        Ok(())
    }

    /// exe: FUN_0048bc40
    /// exe: FUN_0048bd30
    fn bone_access(&mut self, link: u32, name: Option<&str>, id: u32) -> Result<u32, String> {
        let resource = self.m.r32(link + 0xc);
        let local = crate::native_install::local_reference(self.m);
        if resource != 0 {
            bind(self.m, local, resource);
        }
        // The original rereads the caller reference after pinning it.
        let resource = self.m.r32(link + 0xc);
        let result = if resource == 0 {
            0
        } else {
            let data = self.data(resource, 0x8000_0000, 1f32.to_bits())?;
            if data == 0 {
                0
            } else if let Some(name) = name {
                crate::native_camera::bone(self.m, data, name)?.0
            } else {
                let mut bone = self.m.r32(data + 0x12c);
                while bone != 0 && self.m.r32(bone + 4) != 0 {
                    let found = self.m.r32(bone + 0x34);
                    if found == id {
                        self.m.w32(data + 0xbc, self.m.r32(data + 0xbc) | 0x1800);
                        self.destroy_bone(bone);
                        break;
                    }
                    if (found.wrapping_sub(id) as i32) >= 0 {
                        break;
                    }
                    bone = self.m.r32(bone + 4);
                }
                0
            }
        };
        crate::native_install::release(self.m, local);
        Ok(result)
    }

    /// Original543ab0: release the resource controller and both intrusive
    /// list memberships. Freed storage remains observable in the bump model.
    /// exe: FUN_00543ab0
    fn destroy_bone(&mut self, bone: u32) {
        let m = &mut *self.m;
        m.w32(bone, 0x5f37dc);
        crate::native_install::release(m, bone + 0x44);
        m.w32(bone, 0x5f2af8);
        for off in [4, 0xc] {
            let previous = m.r32(bone + off + 4);
            let following = m.r32(bone + off);
            if previous != 0 && following != 0 {
                m.w32(following + off + 4, previous);
                m.w32(previous + off, following);
            }
            m.w32(bone + off, 0);
            m.w32(bone + off + 4, 0);
        }
    }

    fn car_add_linked(&mut self, body: u32, values: [u32; 4]) -> Result<u32, String> {
        let [id, wheel, first_body, second_body] = values;
        if first_body == 0 || second_body == 0 {
            return Ok(1);
        }
        let first_data = crate::native_render_graph::instance(self, first_body + 0x18)?;
        if first_data == 0 {
            return Ok(1);
        }
        let second_data = crate::native_render_graph::instance(self, second_body + 0x18)?;
        if second_data == 0 {
            return Ok(1);
        }
        let node = self.m.malloc(0x40);
        for off in (4..=0x28).step_by(4) {
            self.m.w32(node + off, 0);
        }
        self.m.w32(node, 0x5f1188);
        if id != self.m.r32(node + 0x14) {
            let resource = self.host.resolve(self.m, id, 0, 0)?;
            if resource != 0 {
                bind(self.m, node + 0xc, resource);
            }
        }
        self.remove_named_bone(node + 0xc, "bone00")?;
        self.remove_named_bone(node + 0xc, "bone01")?;
        link_entity(self.m, node + 0x1c, self.m.r32(first_data + 0x84));
        link_entity(self.m, node + 0x24, self.m.r32(second_data + 0x84));
        for (offset, value) in [
            (0x2c, first_body),
            (0x30, second_body),
            (0x34, 0),
            (0x38, 0),
            (0x3c, 0x3f800000),
        ] {
            self.m.w32(node + offset, value);
        }
        let sentinel = if (wheel as i32) < 0 {
            body + 0x20c0
        } else {
            self.m.r32(body + 0x13e4) + wheel.wrapping_mul(0x2b4) + 0x2a4
        };
        append(self.m, sentinel, node);
        Ok(1)
    }

    fn car_remove_linked(&mut self, body: u32, id: u32, wheel: u32) -> Result<(), String> {
        let header = if (wheel as i32) < 0 {
            body + 0x20b0
        } else {
            self.m.r32(body + 0x13e4) + wheel.wrapping_mul(0x2b4) + 0x294
        };
        let mut node = first(self.m, header);
        while node != 0 {
            if self.m.r32(node + 0x14) == id {
                self.remove_named_bone(node + 0xc, "bone00")?;
                self.remove_named_bone(node + 0xc, "bone01")?;
                destroy_node(self.m, node)?;
                break;
            }
            node = next(self.m, node);
        }
        Ok(())
    }

    /// Original43f490: copy a dynamics record, rebinding its counted links.
    /// exe: FUN_0043f490
    fn copy_dynamics(&mut self, dst: u32, src: u32) {
        let m = &mut *self.m;
        for offset in (0..0x30).step_by(4) {
            m.w32(dst + offset, m.r32(src + offset));
        }
        for offset in [0x30, 0x38] {
            unlink_entity(m, dst + offset);
            let target = m.r32(src + offset);
            link_entity(m, dst + offset, target);
        }
        for offset in (0x40..0x8c).step_by(4) {
            m.w32(dst + offset, m.r32(src + offset));
        }
    }

    /// Original43f710: remove the selected record and move the final record
    /// into its slot, preserving intrusive references and slot backpointers.
    /// exe: FUN_0043f710
    pub fn car_remove_dynamics(&mut self, k: u32, body: u32, part: u32) {
        let count = self.m.r32(body + 0x14e8) as i32;
        if count <= 0 {
            return;
        }
        let mut record = body + 0x14ec;
        let mut selected = false;
        for _ in 0..count {
            let entity = self.m.r32(record + 0x38);
            if entity == 0 || self.m.r32(entity + 0x40) == self.m.r32(part + 8) {
                selected = true;
                break;
            }
            record += 0x8c;
        }
        if !selected {
            return;
        }
        let last = body + 0x1460 + count as u32 * 0x8c;
        self.copy_dynamics(record, last);
        unlink_entity(self.m, last + 0x38);
        unlink_entity(self.m, last + 0x30);
        self.m.w32(last, 0);
        for i in 0..(self.m.r32(k + 0x1150) as i32).max(0) as u32 {
            let field = self.m.r32(body + 0x78) + i * 0x68 + 0x28;
            let value = self.m.r32(field);
            if value == last {
                self.m.w32(field, record);
            } else if value == record {
                self.m.w32(field, 0);
            }
        }
        self.m
            .w32(body + 0x14e8, self.m.r32(body + 0x14e8).wrapping_sub(1));
    }

    /// Original448030: remove the attached weak reference and engine/wheel
    /// registrations, then call the stock script's rempart callback.
    /// exe: FUN_00448030
    pub fn car_remove_part(&mut self, k: u32, body: u32, part: u32) -> Result<(), String> {
        let id = self.m.r32(part + 8);
        let mut node = first(self.m, body + 0xd4);
        while node != 0 {
            if self.m.r32(node + 0x14) == id {
                // The caller detaches before invoking the virtual destructor.
                let previous = self.m.r32(node + 8);
                let following = self.m.r32(node + 4);
                if previous != 0 && following != 0 {
                    self.m.w32(following + 8, previous);
                    self.m.w32(previous + 4, following);
                }
                self.m.w32(node + 4, 0);
                self.m.w32(node + 8, 0);
                destroy_node(self.m, node)?;
                break;
            }
            node = next(self.m, node);
        }
        if self.m.r32(body + 0x200c) == id {
            crate::native_install::release(self.m, body + 0x2004);
            unlink_entity(self.m, body + 0x2014);
            self.m.w32(body + 0x70, self.m.r32(body + 0x70) & !0x4000);
            self.m.w32(self.m.r32(body + 0x1fbc) + 0xc18, 0);
        }
        if self.m.r32(body + 0x2034) == id {
            crate::native_install::release(self.m, body + 0x202c);
        }
        let resource = self.m.r32(body + 0xc);
        if resource != 0 {
            let data = self.data(resource, 0xa000_0000, 0)?;
            if data != 0 && self.m.r32(data + 0x50) != 0 {
                let object = self.m.malloc(0x1c);
                let class = self.host.vm_class(self.m, "java.util.resource.GameRef")?;
                self.m.w32(object + 8, 0);
                self.m.w32(object + 4, 0);
                self.m.w32(object, 0x5e7354);
                self.host.vm_object(self.m, object, class)?;
                let link = self.m.malloc(0x10);
                for offset in [0, 4, 8, 12] {
                    self.m.w32(link + offset, 0);
                }
                bind(self.m, link, self.m.r32(part + 0xc));
                let cell = self.host.vm_field(self.m, object, "ptr")?;
                self.m.w32(cell + 8, link);
                self.host
                    .script(self.m, data, "rempart", &[0, 4, object, 0x4f])?;
            }
        }
        let mut node = first(self.m, body + 0x1ddc);
        while node != 0 {
            if self.m.r32(node + 0xc) == id {
                destroy_node(self.m, node)?;
            }
            // Original reads following after destruction (the base destructor
            // zeroes it); do not substitute a prefetched next pointer.
            node = next(self.m, node);
        }
        self.car_remove_wheel(k, body, part)
    }

    fn car_remove_wheel(&mut self, k: u32, body: u32, part: u32) -> Result<(), String> {
        let resource = self.m.r32(part + 0xc);
        if resource == 0 {
            return Ok(());
        }
        let data = self.data(resource, 0xa000_0000, 1f32.to_bits())?;
        if data == 0 {
            return Ok(());
        }
        let part_body = self.m.r32(data + 0x4c);
        let mount = self.m.r32(part_body + 0xd0) as i32;
        if mount > 100 && mount < 0x191 {
            let wheel = self.m.r32(body + 0x13e4) + ((mount - 0x65) % 10) as u32 * 0x2b4;
            if self.m.r32(wheel + 0x2b0) != 0 && self.m.r32(data + 0x44) != 0 {
                let template = self.data(self.m.r32(data + 0x44), 0xa0000000, 1f32.to_bits())?;
                if template == 0 {
                    return Ok(());
                }
                let count = self.m.r32(self.m.r32(template + 0xc) + 0x1150) as i32;
                let mut slot = self.m.r32(part_body + 0x78);
                for _ in 0..count.max(0) {
                    if slot == self.m.r32(wheel + 0x2b0) {
                        let config = k + 0x1f44 + ((mount - 0x65) % 10) as u32 * 200;
                        self.m.w32(wheel + 0x100, self.m.r32(wheel + 0x100) & !0x20);
                        self.m.w32(wheel + 0x2b0, 0);
                        wheel_euler_matrix(self.m, wheel + 0x90, config + 0xc)?;
                        for i in 0..12 {
                            self.m
                                .w32(wheel + 0x110 + i * 4, self.m.r32(wheel + 0x90 + i * 4));
                        }
                        let point = crate::native_install::local_reference(self.m);
                        self.m.w32(point, self.m.r32(config));
                        self.m.w32(point + 8, self.m.r32(config + 8));
                        let mut fp = crate::native_fpu::Fpu::new();
                        fp.fld(self.m, config + 0x3c);
                        fp.fadd(self.m, config + 0x38);
                        fp.fadd(self.m, config + 4);
                        fp.fstp(self.m, point + 4);
                        for i in 0..3 {
                            self.m.w32(wheel + 0x78 + i * 4, self.m.r32(point + i * 4));
                        }
                        self.wheel_world_point(body, point, false)?;
                        wheel_transform(self.m, self.m.r32(wheel + 0x30), point, wheel + 0x110);
                        return Ok(());
                    }
                    slot += 0x68;
                }
            }
        }
        Ok(())
    }

    /// Original447b30 (cdecl car, part body): register an attached part,
    /// run the car's `addpart` script and bind dyno/wheel data.
    /// exe: FUN_00447b30
    pub fn car_add_part(&mut self, k: u32, body: u32, part: u32) -> Result<(), String> {
        let m = &mut *self.m;
        let node = m.malloc(0x1c);
        if node != 0 {
            m.w32(node + 8, 0);
            m.w32(node + 4, 0);
            for offset in [0xc, 0x10, 0x14, 0x18] {
                m.w32(node + offset, 0);
            }
            m.w32(node, 0x5f09b4);
            bind(m, node + 0xc, m.r32(part + 0xc));
        }
        // 45faf0 on body+d4: append to the attached-part list.
        let last = m.r32(body + 0xd4 + 0x18);
        m.w32(last + 4, node);
        m.w32(node + 4, body + 0xd4 + 0x10);
        m.w32(node + 8, last);
        m.w32(body + 0xd4 + 0x18, node);
        let resource = m.r32(body + 0xc);
        if resource != 0 {
            let data = self.data(resource, 0xa000_0000, 0)?;
            if data != 0 && self.m.r32(data + 0x50) != 0 {
                let m = &mut *self.m;
                let object = m.malloc(0x1c);
                if object != 0 {
                    let class = self.host.vm_class(self.m, "java.util.resource.GameRef")?;
                    let m = &mut *self.m;
                    m.w32(object + 8, 0);
                    m.w32(object + 4, 0);
                    m.w32(object, 0x5e7354);
                    self.host.vm_object(self.m, object, class)?;
                }
                let m = &mut *self.m;
                let link = m.malloc(0x10);
                if link != 0 {
                    for offset in [0, 4, 8, 12] {
                        m.w32(link + offset, 0);
                    }
                    bind(m, link, m.r32(part + 0xc));
                }
                let cell = self.host.vm_field(self.m, object, "ptr")?;
                self.m.w32(cell + 8, link);
                self.host
                    .script(self.m, data, "addpart", &[0, 4, object, 0x4f])?;
            }
        }
        let wheel_record = self.slot_record(k, body, 0x191);
        if wheel_record == 0 {
            let resource = self.m.r32(body + 0xc);
            if resource != 0 {
                let data = self.data(resource, 0xa000_0000, 0)?;
                if data != 0 && self.m.r32(data + 0x50) != 0 {
                    if let Some(native) = self.host.vm_reference_native(
                        self.m,
                        self.m.r32(data + 0x50),
                        "dynodata",
                        "java.game.parts.DynoData",
                    )? {
                        self.m.w32(body + 0x70, self.m.r32(body + 0x70) | 0x4000);
                        self.m.w32(self.m.r32(body + 0x1fbc) + 0xc18, native);
                    }
                }
            }
        } else if self.m.r32(wheel_record + 8) == self.m.r32(part + 8) {
            bind(self.m, body + 0x2004, self.m.r32(part + 0xc));
            self.m.w32(body + 0x70, self.m.r32(body + 0x70) | 0x4000);
            let link = crate::native_install::local_reference(self.m);
            if self.host.query(self.m, body + 0x2004, 1, link)? != 0 {
                let data = crate::native_render_graph::instance(self, link)?;
                if data != 0 {
                    let entity = self.m.r32(data + 0x84);
                    link_entity(self.m, body + 0x2014, entity);
                    let relative = self.m.r32(entity + 0x1c);
                    for i in 0..4 {
                        self.m
                            .w32(body + 0x201c + i * 4, self.m.r32(relative + 0x10 + i * 4));
                    }
                }
            }
            let resource = self.m.r32(body + 0x2010);
            if resource != 0 {
                let data = self.data(resource, 0xa000_0000, 0)?;
                if data != 0 && self.m.r32(data + 0x50) != 0 {
                    if let Some(native) = self.host.vm_reference_native(
                        self.m,
                        self.m.r32(data + 0x50),
                        "dynodata",
                        "java.game.parts.DynoData",
                    )? {
                        self.m.w32(self.m.r32(body + 0x1fbc) + 0xc18, native);
                    }
                }
            }
            crate::native_install::release(self.m, link);
        }
        let resource = self.m.r32(part + 0xc);
        if resource != 0 {
            let data = self.data(resource, 0xa000_0000, 0x3f80_0000)?;
            if data != 0 {
                let part_body = self.m.r32(data + 0x4c);
                let mount = self.m.r32(part_body + 0xd0) as i32;
                if mount > 100 && mount < 0x191 {
                    let offset = ((mount - 0x65) % 10) as u32 * 0x2b4;
                    let wheels = self.m.r32(body + 0x13e4);
                    if self.m.r32(wheels + offset + 0x2b0) == 0 && self.m.r32(data + 0x44) != 0 {
                        let template =
                            self.data(self.m.r32(data + 0x44), 0xa0000000, 1f32.to_bits())?;
                        if template != 0 {
                            let mut node = first(self.m, self.m.r32(template + 0xc) + 0x1154);
                            let mut slot = self.m.r32(part_body + 0x78);
                            while node != 0 {
                                if self.m.r32(node + 0x4c) & 0x100000 != 0 {
                                    self.attach_wheel_slot(body, wheels + offset, slot)?;
                                    break;
                                }
                                node = next(self.m, node);
                                slot += 0x68;
                            }
                        }
                    }
                }
            }
        }
        Ok(())
    }

    fn attach_wheel_slot(&mut self, body: u32, wheel: u32, slot: u32) -> Result<(), String> {
        let physics = self.m.r32(wheel + 0x30);
        let relative = self.m.r32(self.m.r32(physics + 0x7c) + 0x1c);
        self.m.w32(wheel + 0x2b0, slot);
        self.m.w32(wheel + 0x100, self.m.r32(wheel + 0x100) | 0x20);
        let mut fp = crate::native_fpu::Fpu::new();
        for i in 0..3 {
            fp.fld(self.m, slot + 0x2c + i * 4);
            fp.fadd(self.m, relative + 4 + i * 4);
            fp.fstp(self.m, relative + 4 + i * 4);
        }
        for i in 0..3 {
            fp.fld(self.m, slot + 0x2c + i * 4);
            fp.fadd(self.m, wheel + 0x78 + i * 4);
            fp.fstp(self.m, wheel + 0x78 + i * 4);
        }
        for i in 0..12 {
            self.m
                .w32(wheel + 0x110 + i * 4, self.m.r32(wheel + 0x90 + i * 4));
        }
        let point = crate::native_install::local_reference(self.m);
        for i in 0..3 {
            self.m.w32(point + i * 4, self.m.r32(wheel + 0x78 + i * 4));
        }
        self.wheel_world_point(body, point, true)?;
        wheel_transform(self.m, physics, point, wheel + 0x110);
        Ok(())
    }

    fn wheel_world_point(
        &mut self,
        body: u32,
        point: u32,
        chain_per_axis: bool,
    ) -> Result<(), String> {
        let data = crate::native_render_graph::instance(self, body + 0x2c)?;
        if data == 0 {
            return Ok(());
        }
        let entity = self.m.r32(data + 0x84);
        let first = self.m.r32(entity + 0x1c);
        let second = self.m.r32(self.m.r32(entity + 0x20) + 0x1c);
        let mut fp = crate::native_fpu::Fpu::new();
        if chain_per_axis {
            for i in 0..3 {
                fp.fld(self.m, point + i * 4);
                fp.fadd(self.m, first + 4 + i * 4);
                fp.fadd(self.m, second + 4 + i * 4);
                fp.fstp(self.m, point + i * 4);
            }
        } else {
            for relative in [first, second] {
                for i in 0..3 {
                    fp.fld(self.m, point + i * 4);
                    fp.fadd(self.m, relative + 4 + i * 4);
                    fp.fstp(self.m, point + i * 4);
                }
            }
        }
        Ok(())
    }

    /// Original43f2e0: start a part-dynamics record for a slot whose
    /// template node carries dynamics (+ac).
    /// exe: FUN_0043f2e0
    pub fn car_part_dynamics(&mut self, k: u32, body: u32, part: u32) -> Result<u32, String> {
        let mut node = if crate::native_install::empty(self.m, k + 0x1154) {
            0
        } else {
            self.m.r32(k + 0x115c)
        };
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0i32;
        while i < self.m.r32(k + 0x1150) as i32 {
            if self.m.r32(record + 8) == self.m.r32(part + 8) {
                if self.m.r32(node + 0xac) == 0 {
                    return Ok(0);
                }
                let dynamics = self.m.r32(node + 0xac);
                let result = body + 0x14ec + self.m.r32(body + 0x14e8) * 0x8c;
                self.m
                    .w32(body + 0x14e8, self.m.r32(body + 0x14e8).wrapping_add(1));
                self.m.w32(result, dynamics);
                self.m.w32(result + 0x88, 0);
                self.m.w32(result + 0x28, 0x3f800000);
                for off in [0x1c, 0x20, 0x24] {
                    self.m.w32(result + off, 0);
                }
                for (dst, src) in [(0x7c, 0x2c), (0x84, 0x38), (0x80, 0x34)] {
                    self.m.w32(result + dst, self.m.r32(dynamics + src));
                }
                self.m.w32(record + 0x28, result);
                let resource = self.m.r32(record + 0xc);
                if resource != 0 {
                    let data = self.data(resource, 0xa0000000, 0)?;
                    if data != 0 {
                        let part_body = self.m.r32(data + 0x4c);
                        let data = crate::native_render_graph::instance(self, part_body + 0x1c)?;
                        if data != 0 {
                            link_entity(self.m, result + 0x38, self.m.r32(data + 0x84));
                            for i in 0..3 {
                                self.m.w32(result + 4 + i * 4, self.m.r32(dynamics + i * 4));
                            }
                            let data = crate::native_render_graph::instance(self, body + 0x2c)?;
                            if data != 0 {
                                link_entity(self.m, result + 0x30, self.m.r32(data + 0x84));
                                refresh_dynamics(self.m, result);
                            }
                        }
                    }
                }
                return Ok(result);
            }
            node = next(self.m, node);
            record += 0x68;
            i += 1;
        }
        Ok(0)
    }

    /// 45ae9a's add_light tail (0x2c-byte light node, flashing phase).
    fn car_add_light(&mut self, body: u32, render: u32, flags: u32) -> Result<u32, String> {
        let m = &mut *self.m;
        if render == 0 || m.r32(render + 8) == 0 {
            return Ok(0);
        }
        let node = m.malloc(0x2c);
        if node != 0 {
            m.w32(node + 8, 0);
            m.w32(node + 4, 0);
            for offset in [0xc, 0x10, 0x14, 0x18] {
                m.w32(node + offset, 0);
            }
            m.w32(node, 0x5f1190);
            bind(m, node + 0xc, m.r32(render + 0xc));
            m.w32(node + 0x1c, flags);
            if flags & 0x38 != 0 {
                // FILD qword of (id & 15) zero-extended, * 0.01 + 0.4.
                let (pc, rc) = (slr_x87::Precision::F32, slr_x87::Rounding::NearestEven);
                let phase = if flags & 8 != 0 {
                    slr_x87::X87::from_i32((self.host.rand(m)? & 0x7fff) as i32)
                        .mul(slr_x87::X87::from_f32(m.rf(0x5f0dbc)), pc, rc)
                        .mul(slr_x87::X87::from_f32(m.rf(0x5f0d50)), pc, rc)
                        .add(slr_x87::X87::from_f32(m.rf(0x5f0cd4)), pc, rc)
                        .to_f32(pc, rc)
                } else {
                    slr_x87::X87::from_i64((m.r32(body + 8) & 0xf) as i64)
                        .mul(slr_x87::X87::from_f32(m.rf(0x5f0c20)), pc, rc)
                        .add(slr_x87::X87::from_f32(m.rf(0x5f0d50)), pc, rc)
                        .to_f32(pc, rc)
                };
                m.wf(node + 0x20, phase);
                m.w32(node + 0x24, 0);
                let half = slr_x87::X87::from_f32(phase)
                    .mul(slr_x87::X87::from_f32(m.rf(0x5f09d0)), pc, rc)
                    .to_f32(pc, rc);
                m.wf(node + 0x28, half);
            }
        }
        append(m, body + 0x206c, node);
        if m.r32(node + 0x1c) & 0x40 == 0 {
            let resource = m.r32(render + 0xc);
            if resource != 0 {
                m.w32(resource + 0x54, m.r32(resource + 0x54) | 0x40_0000);
            }
        }
        Ok(1)
    }

    /// 45b40a: rem_light drops flags and destroys an emptied light node.
    fn car_remove_light(&mut self, body: u32, id: u32, flags: u32) -> Result<(), String> {
        let m = &mut *self.m;
        let mut node = first(m, body + 0x205c);
        while node != 0 && m.r32(node + 0x14) != id {
            node = next(m, node);
        }
        if node == 0 {
            return Ok(());
        }
        let remaining = m.r32(node + 0x1c) & !flags;
        m.w32(node + 0x1c, remaining);
        if remaining != 0 {
            return Ok(());
        }
        let resource = m.r32(node + 0x18);
        if resource != 0 {
            let value = if flags & 0x40 == 0 {
                m.r32(resource + 0x54) | 0x40_0000
            } else {
                m.r32(resource + 0x54) & !0x40_0000
            };
            m.w32(resource + 0x54, value);
        }
        destroy_node(m, node)
    }

    fn car_add_rollbar(&mut self, body: u32, values: [u32; 4]) -> Result<u32, String> {
        let m = &mut *self.m;
        let bars = m.r32(body + 0x1fbc);
        let count = m.r32(bars + 0x44) as i32;
        if count > 7 {
            return Ok(0);
        }
        let entry = bars + 0x48 + count as u32 * 0x10;
        m.w32(bars + 0x44, (count + 1) as u32);
        for (i, value) in values.into_iter().enumerate() {
            m.w32(entry + i as u32 * 4, value);
        }
        Ok(1)
    }

    fn car_remove_rollbar(&mut self, body: u32, values: [u32; 4]) {
        let m = &mut *self.m;
        let bars = m.r32(body + 0x1fbc);
        let count = m.r32(bars + 0x44) as i32;
        let original_equal = |a: u32, b: u32| {
            let (codes, _) = slr_x87::X87::from_f32(f32::from_bits(a))
                .fcom(slr_x87::X87::from_f32(f32::from_bits(b)));
            codes.c3
        };
        for i in 0..count.max(0) as u32 {
            let entry = bars + 0x48 + i * 0x10;
            if m.r32(entry) == values[0]
                && m.r32(entry + 4) == values[1]
                && original_equal(m.r32(entry + 8), values[2])
                && original_equal(m.r32(entry + 12), values[3])
            {
                m.w32(bars + 0x44, (count - 1) as u32);
                let last = bars + 0x48 + (count - 1) as u32 * 0x10;
                for j in 0..4 {
                    m.w32(entry + j * 4, m.r32(last + j * 4));
                }
                return;
            }
        }
    }
}

/// Original492030: wheel-frame position, matrix and derived quaternion.
/// exe: FUN_00492030
fn wheel_transform(m: &mut Space, physics: u32, point: u32, rotation: u32) {
    let relative = m.r32(m.r32(physics + 0x7c) + 0x1c);
    let mut fp = crate::native_fpu::Fpu::new();
    fp.fld(m, physics + 0x30);
    fp.fmul(m, 0x5f0cf8);
    let mut displacement = [0; 3];
    for i in 0..3 {
        fp.fld_st(0);
        fp.fmul(m, relative + 0x24 + i * 0x10);
        displacement[i as usize] = fp.fstp_bits();
    }
    fp.pop();
    for i in 0..3 {
        fp.fld_bits(displacement[i as usize]);
        fp.fadd(m, point + i * 4);
        fp.fstp(m, relative + 4 + i * 4);
    }
    for i in 0..12 {
        m.w32(relative + 0x20 + i * 4, m.r32(rotation + i * 4));
    }
    crate::native_matrix::to_quaternion(
        m,
        relative + 0x10,
        rotation,
        crate::native_stack::sp(m).wrapping_sub(4),
    );
}

/// Original54ecd0 through the matrix module's exact recovered trig domains.
/// Ordinary nonzero angles keep its explicit unresolved-math boundary.
fn wheel_euler_matrix(m: &mut Space, out: u32, angles: u32) -> Result<(), String> {
    let sp = crate::native_stack::sp(m);
    let frame = if sp == 0 {
        0x7fef_fefc
    } else {
        sp.wrapping_sub(4)
    };
    crate::native_matrix::from_euler_memory(m, out, angles, frame)
}

/// Original43ee70: initialize the live constraint geometry from both counted
/// entities. Arithmetic follows x87 instruction order, including the stores
/// between projection, residual length, normalization and basis construction.
/// exe: FUN_0043ee70
pub fn refresh_dynamics(m: &mut Space, record: u32) {
    use crate::native_fpu::{Fpu, Op};
    let a = m.r32(record + 0x30);
    let b = m.r32(record + 0x38);
    if a == 0 || b == 0 {
        return;
    }
    let definition = m.r32(record);
    let saved = crate::native_stack::sp(m);
    if saved == 0 {
        crate::native_stack::set_sp(m, 0x7fef_ff00);
    }
    crate::native_stack::alloc(m, 0xb0);
    let s = crate::native_stack::sp(m);
    let state = b + m.r32(b + 0x20c) * 0xd4;
    for i in 0..3 {
        m.w32(record + 0x10 + i * 4, m.r32(state + 0x64 + i * 4));
        m.w32(record + 0x1c + i * 4, m.r32(state + 0x70 + i * 4));
    }
    crate::native_car_query::world_point(m, a, record + 4, s + 0x2c);
    let rotation = a + 0xa4 + m.r32(a + 0x20c) * 0xd4;
    let mut fp = Fpu::new();
    let mut dot = |m: &mut Space, dst: u32, terms: [(u32, u32); 3]| {
        fp.fld(m, terms[0].0);
        fp.fmul(m, terms[0].1);
        fp.fld(m, terms[1].0);
        fp.fmul(m, terms[1].1);
        fp.faddp(1);
        fp.fld(m, terms[2].0);
        fp.fmul(m, terms[2].1);
        fp.faddp(1);
        fp.fstp(m, dst);
    };
    for (dst, terms) in [
        (0x1c, [(4, 0x10), (8, 0x14), (0, 0xc)]),
        (0x20, [(0x10, 0xc), (0x14, 0x10), (0x18, 0x14)]),
        (0x24, [(0x20, 0xc), (0x24, 0x10), (0x28, 0x14)]),
        (0x44, [(4, 0x1c), (8, 0x20), (0, 0x18)]),
        (0x48, [(0x14, 0x1c), (0x10, 0x18), (0x18, 0x20)]),
        (0x4c, [(0x24, 0x1c), (0x20, 0x18), (0x28, 0x20)]),
    ] {
        dot(
            m,
            s + dst,
            terms.map(|(x, y)| (rotation + x, definition + y)),
        );
    }
    for i in 0..3 {
        m.w32(s + 0x58 + i * 4, m.r32(s + 0x1c + i * 4));
    }
    fp.fld(m, record + 0x10);
    fp.fsub(m, s + 0x2c);
    fp.fld(m, record + 0x14);
    fp.fsub(m, s + 0x30);
    fp.fstp(m, s + 0x14);
    fp.fld(m, record + 0x18);
    fp.fsub(m, s + 0x34);
    fp.fst(m, s + 0x18);
    fp.fmul(m, s + 0x24);
    fp.fld(m, s + 0x14);
    fp.fmul(m, s + 0x20);
    fp.faddp(1);
    fp.fld_st(1);
    fp.fmul(m, s + 0x1c);
    fp.faddp(1);
    for i in 0..3 {
        fp.fld_st(0);
        fp.fmul(m, definition + 0xc + i * 4);
        fp.fadd(m, record + 4 + i * 4);
        fp.fstp(m, record + 4 + i * 4);
    }
    fp.fld(m, s + 0x1c);
    fp.arith_st0(Op::Mul, 1);
    fp.fst(m, s + 0x28);
    fp.fadd(m, s + 0x2c);
    fp.fstp(m, s + 0x2c);
    fp.fld(m, s + 0x20);
    fp.arith_st0(Op::Mul, 1);
    fp.fst(m, s + 0x54);
    fp.fadd(m, s + 0x30);
    fp.fstp(m, s + 0x30);
    fp.fld(m, s + 0x24);
    fp.arith_st0(Op::Mul, 1);
    fp.fstp(m, s + 0x50);
    fp.fstp_st(0);
    fp.fld(m, s + 0x50);
    fp.fadd(m, s + 0x34);
    fp.fstp(m, s + 0x34);
    fp.fsub(m, s + 0x28);
    fp.fstp(m, s + 0x10);
    fp.fld(m, s + 0x14);
    fp.fsub(m, s + 0x54);
    fp.fstp(m, s + 0x14);
    fp.fld(m, s + 0x18);
    fp.fsub(m, s + 0x50);
    fp.fst(m, s + 0x18);
    fp.fld(m, s + 0x14);
    fp.fld(m, s + 0x10);
    fp.fmul(m, s + 0x10);
    fp.fld_st(1);
    fp.arith_st0(Op::Mul, 2);
    fp.faddp(1);
    fp.fld_st(2);
    fp.arith_st0(Op::Mul, 3);
    fp.faddp(1);
    fp.fsqrt();
    fp.fstp_st(2);
    fp.fstp_st(0);
    fp.fld_st(0);
    fp.fstp(m, record + 0x28);
    fp.fld_st(0);
    fp.fmul(m, s + 0x44);
    fp.fadd(m, s + 0x2c);
    fp.fstp(m, s + 0x1c);
    fp.fld_st(0);
    fp.fmul(m, s + 0x48);
    fp.fadd(m, s + 0x30);
    fp.fstp(m, s + 0x20);
    fp.fmul(m, s + 0x4c);
    fp.fadd(m, s + 0x34);
    fp.fstp(m, s + 0x24);
    fp.fld(m, s + 0x10);
    fp.fadd(m, s + 0x2c);
    fp.fld(m, s + 0x14);
    fp.fadd(m, s + 0x30);
    fp.fstp(m, s + 0x3c);
    fp.fld(m, s + 0x18);
    fp.fadd(m, s + 0x34);
    fp.fstp(m, s + 0x40);
    fp.fsub(m, s + 0x1c);
    fp.fstp(m, record + 0x70);
    fp.fld(m, s + 0x3c);
    fp.fsub(m, s + 0x20);
    fp.fstp(m, record + 0x74);
    fp.fld(m, s + 0x40);
    fp.fsub(m, s + 0x24);
    fp.fstp(m, record + 0x78);
    let rotation = b + 0xa4 + m.r32(b + 0x20c) * 0xd4;
    for (dst, terms) in [
        (0x38, [(0x20, 0x78), (0x10, 0x74), (0, 0x70)]),
        (0x3c, [(4, 0x70), (0x24, 0x78), (0x14, 0x74)]),
        (0x40, [(8, 0x70), (0x28, 0x78), (0x18, 0x74)]),
    ] {
        let mut fp = Fpu::new();
        fp.fld(m, rotation + terms[0].0);
        fp.fmul(m, record + terms[0].1);
        fp.fld(m, rotation + terms[1].0);
        fp.fmul(m, record + terms[1].1);
        fp.faddp(1);
        fp.fld(m, rotation + terms[2].0);
        fp.fmul(m, record + terms[2].1);
        fp.faddp(1);
        fp.fstp(m, s + dst);
    }
    for i in 0..3 {
        m.w32(record + 0x70 + i * 4, m.r32(s + 0x38 + i * 4));
        m.w32(record + 0x10 + i * 4, m.r32(s + 0x1c + i * 4));
    }
    let parent = m.r32(b + 0x20);
    let rotation = parent + 0xa4 + m.r32(parent + 0x20c) * 0xd4;
    transpose_vector(m, s + 0x58, rotation, s + 0x58);
    for i in 0..3 {
        m.w32(s + 0x10 + i * 4, m.r32(s + 0x44 + i * 4));
    }
    let mut fp = Fpu::new();
    fp.fld(m, s + 0x18);
    fp.fld(m, s + 0x14);
    fp.fld(m, s + 0x10);
    fp.fld_st(0);
    fp.arith_st0(Op::Mul, 1);
    fp.fld_st(2);
    fp.arith_st0(Op::Mul, 3);
    fp.faddp(1);
    fp.fld_st(3);
    fp.arith_st0(Op::Mul, 4);
    fp.faddp(1);
    fp.fsqrt();
    fp.fstp_st(3);
    fp.fstp_st(0);
    fp.fstp_st(0);
    if !fp.fcom(m, 0x5e73cc).test(0x40) {
        fp.fld(m, 0x5f08f0);
        fp.arith_st0(Op::Div, 1);
        let scale = fp.fstp_bits();
        fp.pop();
        for i in 0..3 {
            fp.fld_bits(scale);
            fp.fmul(m, s + 0x10 + i * 4);
            fp.fstp(m, s + 0x10 + i * 4);
        }
    } else {
        fp.pop();
    }
    transpose_vector(m, s + 0x10, rotation, s + 0x10);
    let mut cross = [0; 3];
    for (i, terms) in [
        [(0x18, 0x5c), (0x60, 0x14)],
        [(0x60, 0x10), (0x58, 0x18)],
        [(0x58, 0x14), (0x10, 0x5c)],
    ]
    .into_iter()
    .enumerate()
    {
        fp.fld(m, s + terms[0].0);
        fp.fmul(m, s + terms[0].1);
        fp.fld(m, s + terms[1].0);
        fp.fmul(m, s + terms[1].1);
        fp.fsubp(1);
        cross[i] = fp.fstp_bits();
    }
    for i in 0..3 {
        m.w32(s + 0x68 + i * 4, cross[i as usize]);
    }
    for (column, vector) in [(0, 0x68), (1, 0x58), (2, 0x10)] {
        for row in 0..3 {
            m.w32(
                s + 0x74 + row * 0x10 + column * 4,
                m.r32(s + vector + row * 4),
            );
        }
    }
    crate::native_matrix::transpose(m, record + 0x40, s + 0x74);
    crate::native_stack::set_sp(m, saved);
}

/// Original45dbc0: transpose-rotation vector product, including its distinct
/// operand order and temporary outputs when input and output alias.
/// exe: FUN_0045dbc0
fn transpose_vector(m: &mut Space, out: u32, rotation: u32, vector: u32) {
    let mut fp = crate::native_fpu::Fpu::new();
    let mut result = [0; 3];
    for (i, terms) in [
        [(0x20, 8), (0x10, 4), (0, 0)],
        [(0x24, 8), (4, 0), (0x14, 4)],
        [(0x28, 8), (8, 0), (0x18, 4)],
    ]
    .into_iter()
    .enumerate()
    {
        fp.fld(m, rotation + terms[0].0);
        fp.fmul(m, vector + terms[0].1);
        fp.fld(m, rotation + terms[1].0);
        fp.fmul(m, vector + terms[1].1);
        fp.faddp(1);
        fp.fld(m, rotation + terms[2].0);
        fp.fmul(m, vector + terms[2].1);
        fp.faddp(1);
        result[i] = fp.fstp_bits();
    }
    for i in 0..3 {
        m.w32(out + i * 4, result[i as usize]);
    }
}

/// Append to a list whose trailing sentinel is at `sentinel` (+8 = last).
fn append(m: &mut Space, sentinel: u32, node: u32) {
    let last = m.r32(sentinel + 8);
    m.w32(last + 4, node);
    m.w32(node + 4, sentinel);
    m.w32(node + 8, last);
    m.w32(sentinel + 8, node);
}

/// Construction-time entry (native part slot dispatch sends mount-slot
/// records directly to the car object before any instance routing exists).
pub fn command(
    m: &mut Space,
    template: u32,
    object: u32,
    direct: bool,
    _sender: u32,
    message: u32,
    text: &str,
) -> Result<(), String> {
    if !direct {
        return Err(
            "NativeMessageBoundary: indirect car command requires original4265c0 routing".into(),
        );
    }
    let saved = crate::native_stack::sp(m);
    let buffer = crate::native_install::local_text(m, text);
    let result = Install {
        m,
        host: &mut crate::native_install::Detached,
    }
    .car_command(template, object, message, buffer);
    crate::native_stack::set_sp(m, saved);
    result.map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct CommandHost<'a> {
        row: &'a Value,
        events: Vec<Value>,
        cells: Vec<u32>,
        rng: u32,
    }
    impl crate::native_install::Host for CommandHost<'_> {
        fn minimum(&mut self, _: &mut Space, resource: u32, quality: u32) -> Result<(), String> {
            self.events.push(json!(["minimum", resource, quality]));
            Ok(())
        }
        fn prepare(&mut self, _: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
            self.events.push(json!(["prepare", resource, flags]));
            Ok(0)
        }
        fn payload(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
            self.events.push(json!(["payload", resource]));
            Ok(self.row["payloads"][resource.to_string()]
                .as_u64()
                .unwrap_or(0) as u32)
        }
        fn resolve(
            &mut self,
            _: &mut Space,
            id: u32,
            kind: u32,
            flags: u32,
        ) -> Result<u32, String> {
            self.events.push(json!(["resolve", id, kind, flags]));
            Ok(self.row["ids"][id.to_string()].as_u64().unwrap_or(0) as u32)
        }
        fn query(
            &mut self,
            m: &mut Space,
            reference: u32,
            message: u32,
            argument: u32,
        ) -> Result<u32, String> {
            self.events.push(json!(["query", reference, message]));
            let resource = self.row["query_resource"].as_u64().unwrap() as u32;
            if resource != 0 {
                bind(m, argument, resource);
            }
            Ok(u32::from(resource != 0))
        }
        fn script_query(&mut self, _: &mut Space, _: u32, _: u32) -> Result<u32, String> {
            Err("unexpected script query".into())
        }
        fn command(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            Err("unexpected routed command".into())
        }
        fn deliver(
            &mut self,
            _: &mut Space,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
            _: u32,
        ) -> Result<(), String> {
            Err("unexpected script event".into())
        }
        fn script(
            &mut self,
            _: &mut Space,
            envelope: u32,
            method: &str,
            words: &[u32],
        ) -> Result<(), String> {
            self.events.push(json!(["script", envelope, method, words]));
            Ok(())
        }
        fn vm_class(&mut self, _: &mut Space, name: &str) -> Result<u32, String> {
            self.events.push(json!(["vm_class", name]));
            Ok(0x30000100)
        }
        fn vm_object(&mut self, _: &mut Space, object: u32, class: u32) -> Result<(), String> {
            self.events.push(json!(["vm_object", object, class, 0]));
            Ok(())
        }
        fn vm_field(&mut self, _: &mut Space, object: u32, name: &str) -> Result<u32, String> {
            let cell = 0x30000800 + self.cells.len() as u32 * 16;
            self.cells.push(cell);
            self.events.push(json!(["vm_field", object, name, cell]));
            Ok(cell)
        }
        fn vm_reference_native(
            &mut self,
            _: &mut Space,
            object: u32,
            name: &str,
            descriptor: &str,
        ) -> Result<Option<u32>, String> {
            self.events
                .push(json!(["vm_reference_native", object, name, descriptor]));
            Ok(self.row["dyno"].as_u64().map(|n| n as u32))
        }
        fn rand(&mut self, _: &mut Space) -> Result<u32, String> {
            self.rng = self.rng.wrapping_mul(0x343fd).wrapping_add(0x269ec3);
            Ok((self.rng >> 16) & 0x7fff)
        }
    }

    #[test]
    fn original_attachment_commands_match_full_lifecycles() {
        let path = std::env::var("SLR_CAR_COMMAND_VECTORS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../re/out/boot/native-car-command-attachments-v1.json")
            });
        let bytes = std::fs::read(path)
            .expect("generate native_car_attachment_vectors.py gen before original-backed tests");
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], "slr-car-attachments-v1");
        assert_eq!(
            data["original_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["fpu_cw"], 0x7f);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 42);
        let labels: std::collections::HashSet<_> =
            rows.iter().map(|r| r["label"].as_str().unwrap()).collect();
        assert_eq!(labels.len(), rows.len());
        for row in rows {
            let label = row["label"].as_str().unwrap();
            assert!(!row["commands"].as_array().unwrap().is_empty());
            assert_eq!(
                row["commands"].as_array().unwrap().len(),
                row["results"].as_array().unwrap().len()
            );
            assert_eq!(
                row["before"].as_array().unwrap().len() as u64,
                (row["heap_before"].as_u64().unwrap() - 0x20000000).div_ceil(4)
            );
            assert_eq!(
                row["after"].as_array().unwrap().len() as u64,
                (row["heap_after"].as_u64().unwrap() - 0x20000000).div_ceil(4)
            );
            let mut m = Space::new();
            m.set_heap_ptr(row["heap_before"].as_u64().unwrap() as u32);
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            for pair in data["image_words"]
                .as_array()
                .unwrap()
                .iter()
                .chain(row["external_words"].as_array().unwrap())
            {
                m.w32(
                    pair[0].as_u64().unwrap() as u32,
                    pair[1].as_u64().unwrap() as u32,
                );
            }
            let mut host = CommandHost {
                row,
                events: vec![],
                cells: vec![],
                rng: row["rng_before"].as_u64().unwrap() as u32,
            };
            for (i, command) in row["commands"].as_array().unwrap().iter().enumerate() {
                let command = command.as_str().unwrap();
                for (j, byte) in command.bytes().chain(std::iter::once(0)).enumerate() {
                    m.w8(0x30010000 + j as u32, byte);
                }
                let got = Install {
                    m: &mut m,
                    host: &mut host,
                }
                .car_command(
                    row["k"].as_u64().unwrap() as u32,
                    row["body"].as_u64().unwrap() as u32,
                    0x10,
                    0x30010000,
                )
                .unwrap_or_else(|e| panic!("{label} command{i} {command}: {e}"));
                assert_eq!(
                    got,
                    row["results"][i].as_u64().unwrap() as u32,
                    "{label} command{i}"
                );
            }
            let native_events = row["native_events"].as_array().unwrap();
            assert_eq!(
                native_events.len(),
                row["event_results"].as_array().unwrap().len()
            );
            for (i, event) in native_events.iter().enumerate() {
                let got = Install {
                    m: &mut m,
                    host: &mut host,
                }
                .car_command(
                    row["k"].as_u64().unwrap() as u32,
                    row["body"].as_u64().unwrap() as u32,
                    event[0].as_u64().unwrap() as u32,
                    event[1].as_u64().unwrap() as u32,
                )
                .unwrap();
                assert_eq!(
                    got,
                    row["event_results"][i].as_u64().unwrap() as u32,
                    "{label} event{i}"
                );
            }
            assert_eq!(
                m.heap_ptr(),
                row["heap_after"].as_u64().unwrap() as u32,
                "{label} allocator"
            );
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "{label} heap word{i}"
                );
            }
            for pair in row["cells"].as_array().unwrap() {
                assert_eq!(
                    m.r32(pair[0].as_u64().unwrap() as u32 + 8),
                    pair[1].as_u64().unwrap() as u32,
                    "{label} VM cell"
                );
            }
            assert_eq!(
                json!(host.events),
                row["events"],
                "{label} ordered collaborators"
            );
            assert_eq!(
                host.rng,
                row["rng_after"].as_u64().unwrap() as u32,
                "{label} shared CRT state"
            );
        }
    }
    #[test]
    fn original_constraint_refresh_matches_complete_buffers() {
        let path = std::env::var("SLR_CAR_DYNAMICS_VECTORS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../re/out/boot/native-car-dynamics-original.json")
            });
        let bytes = std::fs::read(path)
            .expect("generate native_car_dynamics_vectors.py gen before the original-backed suite");
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], "slr-car-dynamics-v1");
        assert_eq!(
            data["original_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["fpu_cw"], 0x7f);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 42);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["case"], i);
            assert_eq!(
                row["before"].as_array().unwrap().len(),
                row["after"].as_array().unwrap().len()
            );
            assert_eq!(row["before"].as_array().unwrap().len(), 0x790 / 4);
            let mut m = Space::new();
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            // Original global operands used by normalization.
            m.w32(0x5e73cc, 0);
            m.w32(0x5f08f0, 0x3f800000);
            refresh_dynamics(&mut m, row["record"].as_u64().unwrap() as u32);
            for (j, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + j as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "case{i} word{j}"
                );
            }
        }
    }
    #[test]
    fn original_add_mslot_preserves_full_records_and_intrusive_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-car-command-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_car_command_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let k = m.alloc(0x2e98);
            let object = m.alloc(0x212c);
            let slots = m.alloc(0x68 * 3);
            let descriptors = m.alloc(0x100 * 3);
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            for i in 0..row["count"].as_u64().unwrap() as u32 {
                let command = format!("add_mslot {} {}", slots + i * 0x68, descriptors + i * 0x100);
                // Caller-owned command storage is outside the constructor
                // heap in the oracle. Allocating it here shifts every node.
                let text = 0x300f_f000 + i * 96;
                for (j, byte) in command.bytes().chain(std::iter::once(0)).enumerate() {
                    m.w8(text + j as u32, byte);
                }
                let mut host = crate::native_install::snapshot_tests::RawHost::new();
                Install {
                    m: &mut m,
                    host: &mut host,
                }
                .car_command(k, object, 0x10, text)
                .unwrap();
            }
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                let address = 0x20000000 + i as u32 * 4;
                assert_eq!(
                    m.r32(address),
                    word.as_u64().unwrap() as u32,
                    "count={} word{i}",
                    row["count"]
                );
            }
        }
    }
}

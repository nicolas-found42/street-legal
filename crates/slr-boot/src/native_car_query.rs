//! Original45c510: the configured car template's instance query switch
//! (template vtable 5f0f18 slot +3c). `k` is the car template payload and
//! `body` the runtime car object; `argument` is message specific.
use crate::native_fpu::{Fpu, Op};
use crate::native_install::{bind, first, next, unlink, Install};
use slr_physics::space::Space;
use slr_x87::{Rounding, X87};

/// 5d6750 (`__ftol`): truncate ST(0) to a 64-bit integer, return EAX.
fn ftol(v: X87) -> u32 {
    v.to_i64(Rounding::Zero) as u32
}
fn state(m: &Space, entity: u32) -> u32 {
    entity + 0x64 + m.r32(entity + 0x20c) * 0xd4
}
/// 429060 on a nullable reference argument.
fn bind_out(m: &mut Space, out: u32, resource: u32) {
    if out != 0 {
        bind(m, out, resource);
    }
}
/// |gravity| from the physics world (x87 order of 45c662..45c684).
fn gravity_magnitude(fpu: &mut Fpu, m: &Space) {
    let world = m.r32(0x617650);
    fpu.fld(m, world + 0x10c);
    fpu.fld(m, world + 0x108);
    fpu.fld(m, world + 0x104);
    fpu.fld_st(0);
    fpu.arith_st0(Op::Mul, 1);
    fpu.fld_st(2);
    fpu.arith_st0(Op::Mul, 3);
    fpu.faddp(1);
    fpu.fld_st(3);
    fpu.arith_st0(Op::Mul, 4);
    fpu.faddp(1);
    fpu.fsqrt();
    fpu.fstp_st(3);
    fpu.fstp_st(0);
    fpu.fstp_st(0);
}

impl Install<'_> {
    /// Original45c510. Stack temporaries live for the call.
    pub fn car_query(
        &mut self,
        k: u32,
        body: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        let saved = crate::native_stack::sp(self.m);
        let result = self.car_query_inner(k, body, message, argument);
        crate::native_stack::set_sp(self.m, saved);
        result
    }
    fn car_query_inner(
        &mut self,
        k: u32,
        body: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        let m = &mut *self.m;
        let entity = m.r32(body + 0x13bc);
        Ok(match message {
            7 => 5,
            8 | 9 | 0x1c => 1,
            0x3e => u32::from(m.r32(body + 0x1fc0) == 0),
            0x45 => (m.r32(k + 0x2e4c) >> 1) & 1,
            0xa => {
                let mut fpu = Fpu::new();
                fpu.fld(m, body + 0xb4);
                if fpu.fcomp(m, 0x5e73cc).test(0x41) {
                    return Ok(0);
                }
                let count = m.r32(k + 0x177c);
                if count as i32 <= 0 {
                    return Ok(count);
                }
                let mut result = count;
                for i in 0..count {
                    let record = body + 0x180 + i * 0x4a8;
                    if m.r32(record - 0x58) != 0 || m.r32(record) != 0 {
                        result = result.wrapping_sub(1);
                    }
                }
                result
            }
            1 => {
                bind_out(m, argument, m.r32(body + 0x38));
                m.r32(body + 0x34)
            }
            6 => {
                bind_out(m, argument, m.r32(k + 0x10));
                m.r32(k + 0xc)
            }
            0x2b => {
                bind_out(m, argument, m.r32(body + 0xcc));
                m.r32(body + 0xc8)
            }
            0x2e => {
                bind_out(m, argument, m.r32(body + 0xc));
                m.r32(body + 8)
            }
            0x18 => {
                bind_out(m, argument, m.r32(body + 0x1eec));
                m.r32(body + 0x1ee8)
            }
            0x22 => {
                if m.r32(m.r32(k + 0x2e48) + 0xa54) == 0 {
                    return Ok(0);
                }
                bind_out(m, argument, m.r32(body + 0x13f4));
                m.r32(body + 0x13f0)
            }
            0x30 => m.r32(m.r32(k + 0x2e48) + 0xa54),
            0x3f => m.r32(body + 0xac),
            0x15 => m.r32(body + 0x1edc),
            0x37 => m.r32(k + 0x114c),
            0x27 => m.r32(k + 0x1150).wrapping_sub(m.r32(body + 0x74)),
            0x42 => m.r32(body + 0x70) & 0x0400_01f0,
            0xf => {
                let mut fpu = Fpu::new();
                fpu.fld(m, body + 0xb4);
                fpu.fmul(m, 0x5f09c8);
                ftol(fpu.pop())
            }
            0xb => {
                let definition = m.r32(entity + 0x5c);
                let inertia = m.r32(definition + 0x78);
                let mut fpu = Fpu::new();
                fpu.fld(m, inertia + 0xc);
                if argument != 0 {
                    fpu.fst(m, argument);
                }
                fpu.fmul(m, 0x5f11d8);
                1u32.wrapping_sub(ftol(fpu.pop()))
            }
            2 => {
                if argument != 0 {
                    let instance = crate::native_render_graph::instance(self, body + 0x2c)?;
                    if instance != 0 {
                        let m = &mut *self.m;
                        world_point(m, m.r32(instance + 0x84), 0, argument);
                        if m.r32(k + 0x1f40) as i32 >= 4 {
                            self.ground_offset(k, body, argument);
                        }
                    }
                }
                1
            }
            3 => {
                if argument != 0 {
                    let velocity = state(m, entity) + 0xc;
                    for i in 0..3 {
                        m.w32(argument + i * 4, m.r32(velocity + i * 4));
                    }
                }
                1
            }
            0x32 => {
                if argument != 0 {
                    let rotation = state(m, entity) + 0x40;
                    for i in 0..12 {
                        m.w32(argument + i * 4, m.r32(rotation + i * 4));
                    }
                }
                1
            }
            4 => self.car_heading(entity, argument)?,
            0x33 => {
                if argument != 0 {
                    let b = state(m, entity);
                    let mut fpu = Fpu::new();
                    let rows = [
                        [(0x60, 0x78), (0x50, 0x74), (0x70, 0x40)],
                        [(0x44, 0x70), (0x64, 0x78), (0x54, 0x74)],
                        [(0x48, 0x70), (0x68, 0x78), (0x58, 0x74)],
                    ];
                    let mut out = [0u32; 3];
                    for (i, terms) in rows.iter().enumerate() {
                        fpu.fld(m, b + terms[0].0);
                        fpu.fmul(m, b + terms[0].1);
                        fpu.fld(m, b + terms[1].0);
                        fpu.fmul(m, b + terms[1].1);
                        fpu.faddp(1);
                        fpu.fld(m, b + terms[2].0);
                        fpu.fmul(m, b + terms[2].1);
                        fpu.faddp(1);
                        out[i] = fpu.fstp_bits();
                    }
                    for (i, word) in out.into_iter().enumerate() {
                        m.w32(argument + i as u32 * 4, word);
                    }
                }
                1
            }
            0x34 => {
                let count = m.r32(k + 0x1f40) as i32;
                if count <= 0 {
                    return Ok(0);
                }
                let wheels = m.r32(body + 0x13e4);
                let (mut contact, mut driven) = (0i32, 0i32);
                for i in 0..count as u32 {
                    let record = wheels + 0xc8 + i * 0x2b4;
                    let mut fpu = Fpu::new();
                    fpu.fld(m, record + 4);
                    if fpu.fcomp(m, 0x5e73cc).test(0x40) {
                        continue;
                    }
                    contact += 1;
                    fpu.fld(m, record);
                    if !fpu.fcomp(m, 0x5e73cc).test(0x40) {
                        driven += 1;
                    }
                }
                if contact == 0 {
                    0
                } else if contact == count {
                    1
                } else if driven == contact {
                    2
                } else {
                    3 + u32::from(driven != 0)
                }
            }
            0x35 => {
                if argument == 0 {
                    return Ok(0);
                }
                let count = m.r32(k + 0x1f40) as i32;
                if count <= 0 {
                    return Ok(0);
                }
                let mut fpu = Fpu::new();
                gravity_magnitude(&mut fpu, m);
                let definition = m.r32(entity + 0x5c);
                fpu.fdivr(m, definition + 0x14);
                m.w32(argument, if count < 4 { count as u32 } else { 4 });
                let mut wheel = m.r32(body + 0x13e4) + 0xc0;
                let mut out = argument + 4;
                let mut i = 0;
                loop {
                    let limit = (m.r32(k + 0x1f40) as i32).min(4);
                    if i >= limit {
                        break;
                    }
                    let contact = m.r32(wheel - 0x90);
                    m.w32(out, m.r32(wheel));
                    for j in 0..3 {
                        m.w32(out + 0xc + j * 4, m.r32(wheel + 0x80 + j * 4));
                    }
                    fpu.fld(m, contact + 0xd8);
                    if fpu.fcomp(m, contact + 0x38).test(1) {
                        fpu.fld(m, contact + 0xdc);
                        fpu.arith_st0(Op::Mul, 1);
                        fpu.fstp(m, out + 4);
                        let material = m.r32(contact + 0x94);
                        if material != 0 {
                            fpu.fld(m, wheel - 0x54);
                            fpu.fmul(m, material + 0x50);
                            fpu.fstp(m, out + 8);
                        } else {
                            m.w32(out + 8, m.r32(wheel - 0x54));
                        }
                    } else {
                        m.w32(out + 4, 0);
                        m.w32(out + 8, 0);
                    }
                    wheel += 0x2b4;
                    out += 0x18;
                    i += 1;
                }
                1
            }
            0x36 => {
                if argument != 0 {
                    let gear = m.r32(body + 0x1df8);
                    m.w32(argument, m.r32(m.r32(body + 0x1fbc) + gear * 4 + 0x20));
                }
                m.r32(body + 0x1df8)
            }
            0x40 | 0x46 => {
                if argument == 0 {
                    return Ok(0);
                }
                if m.r32(k + 0x1f40) == 0 {
                    m.w32(argument, 0x4000_0000);
                    return Ok(0);
                }
                let mut fpu = Fpu::new();
                fpu.fld(m, k + 0x1f4c);
                fpu.fadd(m, k + 0x2014);
                fpu.fld(m, k + 0x20dc);
                fpu.fadd(m, k + 0x21a4);
                if message == 0x40 {
                    fpu.arith_st0(Op::Sub, 1);
                    fpu.fmul(m, 0x5f09d0);
                } else {
                    fpu.fld_st(0);
                    fpu.arith_st0(Op::Sub, 2);
                    fpu.arith_st0(Op::DivR, 1);
                }
                fpu.fstp(m, argument);
                1
            }
            0x19 => {
                let field = match argument {
                    28 => 0x1db4,
                    29 => 0x1db8,
                    48 => 0x1dc0,
                    49 => 0x1dbc,
                    31 => 0x1dc4,
                    1 => 0x1dac,
                    _ => return Ok(0),
                };
                let mut fpu = Fpu::new();
                fpu.fld(m, body + field);
                fpu.fmul(m, 0x5f0c2c);
                ftol(fpu.pop())
            }
            0x28 => self.car_free_slot(k, body, argument)?,
            0x29 => self.installed_root(k, body)?,
            0x2f => self.car_slot_frame(k, body, argument)?,
            0x31 => {
                if argument == 0 {
                    return Ok(0);
                }
                attached_part(m, body, argument)
            }
            0x3c => {
                if argument != 0 {
                    m.w32(body + 0x58, argument);
                }
                crate::native_message::part_variant(self.m, k, body, 0)?;
                let mut node = first(self.m, body + 0xd4);
                while node != 0 {
                    self.host.query(self.m, node + 0xc, 0x3c, argument)?;
                    node = next(self.m, node);
                }
                self.m.r32(body + 0x58)
            }
            0x38 => self.car_driver(k, body, argument)?,
            0x41 => {
                if argument == 0 || entity == 0 {
                    return Ok(0);
                }
                bounds(m, k, entity, argument);
                1
            }
            _ => 0,
        })
    }

    /// 45c638..45c6df: average suspension offset added to the point's y.
    fn ground_offset(&mut self, k: u32, body: u32, out: u32) {
        let m = &mut *self.m;
        let count = m.r32(k + 0x1f40);
        let mut fpu = Fpu::new();
        gravity_magnitude(&mut fpu, m);
        let entity = m.r32(body + 0x13bc);
        let definition = m.r32(entity + 0x5c);
        fpu.fld1();
        fpu.fdiv(m, definition + 0x14);
        fpu.fmulp(1);
        // FSTP into the dead argument word, then reload as a float.
        let weight = fpu.fstp_bits();
        fpu.fld(m, 0x5e73cc);
        fpu.fild(count as i32);
        fpu.fld_bits(weight);
        fpu.arith_st0(Op::Div, 1);
        let mut wheel = m.r32(body + 0x13e4) + 0x60;
        for _ in 0..4 {
            fpu.fld(m, wheel - 0x10);
            wheel += 0x2b4;
            fpu.fadd(m, wheel - 0x2b4);
            fpu.fld_st(1);
            fpu.fdiv(m, wheel - 0x2d0);
            fpu.fsubp(1);
            fpu.fsubr(m, wheel - 0x298);
            fpu.faddp(3);
        }
        fpu.fstp_st(0);
        fpu.fxch(1);
        fpu.arith_st0(Op::Div, 1);
        fpu.fadd(m, out + 4);
        fpu.fstp(m, out + 4);
    }

    /// 45c774: optional forward vector (negated third column), then the
    /// yaw from 551c90 in whole degrees, folded into 1..360.
    fn car_heading(&mut self, entity: u32, out: u32) -> Result<u32, String> {
        let m = &mut *self.m;
        let rotation = state(m, entity) + 0x40;
        if out != 0 {
            let mut fpu = Fpu::new();
            for (i, offset) in [8u32, 0x18, 0x28].into_iter().enumerate() {
                fpu.fld(m, rotation + offset);
                fpu.fmul(m, 0x5f0c70);
                let word = fpu.fstp_bits();
                m.w32(out + i as u32 * 4, word);
            }
        }
        let matrix = slr_lang::vec::Mat3 {
            m: std::array::from_fn(|i| m.rf(rotation + (i as u32 / 3) * 0x10 + (i as u32 % 3) * 4)),
        };
        let yaw = matrix.to_ypr().y;
        let degrees = X87::from_f32(yaw).mul(
            X87::from_f32(m.rf(0x5f11dc)),
            slr_x87::Precision::F32,
            Rounding::NearestEven,
        );
        let mut remainder = (ftol(degrees) as i32) % 360;
        if remainder <= 0 {
            remainder += 360;
        }
        Ok(remainder as u32)
    }

    /// 45ca86: `"%d %d"` = (slot id, target slot) on a stack reference.
    fn car_free_slot(&mut self, k: u32, body: u32, text: u32) -> Result<u32, String> {
        let bytes = c_string(self.m, text);
        let (mut target, mut slot) = (0u32, 0u32);
        let _ = slr_lang::scan::sscanf(
            &bytes,
            b"%d %d",
            &mut [
                slr_lang::scan::ScanArg::Word(&mut target),
                slr_lang::scan::ScanArg::Word(&mut slot),
            ],
        );
        let link = crate::native_install::local_reference(self.m);
        self.bind_reference_id(link, target)?;
        let result = self.free_target_slot(k, body, link, slot, 0)?;
        if self.m.r32(link + 12) != 0 {
            unlink(self.m, link);
        }
        Ok(result)
    }

    /// 545fc0: bind by id through 536820 with no kind filter.
    pub fn bind_reference_id(&mut self, link: u32, id: u32) -> Result<(), String> {
        if self.m.r32(link + 8) == id {
            return Ok(());
        }
        if self.m.r32(link + 12) != 0 {
            unlink(self.m, link);
        }
        if id == 0 {
            return Ok(());
        }
        let resource = self.host.resolve(self.m, id, 0, 0)?;
        if resource == 0 {
            return Ok(());
        }
        let m = &mut *self.m;
        m.w32(link + 8, id);
        let head = m.r32(resource + 0x48);
        if head != 0 {
            m.w32(head, link);
        }
        m.w32(link, 0);
        m.w32(link + 4, head);
        m.w32(resource + 0x48, link);
        m.w32(link + 12, resource);
        Ok(())
    }

    /// Original472370: id attached through a locking slot, -1 when locked.
    pub fn installed_root(&mut self, k: u32, body: u32) -> Result<u32, String> {
        let mut node = {
            let head = self.m.r32(k + 0x115c);
            if self.m.r32(head + 4) != 0 {
                head
            } else {
                0
            }
        };
        let mut record = self.m.r32(body + 0x78);
        if self.locked(k, body)? != 0 {
            return Ok(u32::MAX);
        }
        let count = self.m.r32(k + 0x1150) as i32;
        for _ in 0..count.max(0) {
            let m = &*self.m;
            let flags = m.r32(record + 0x14);
            if flags & 0x11 == 0x11 {
                return Ok(u32::MAX);
            }
            if m.r32(record + 8) != 0 && flags & 2 != 0 {
                return Ok(m.r32(record + 8));
            }
            node = m.r32(node + 4);
            record += 0x68;
            if node == 0 || m.r32(node + 4) == 0 {
                node = 0;
            }
        }
        Ok(0)
    }

    /// 45c928: slot frame for an install request record `{id, ref, ...}`.
    fn car_slot_frame(&mut self, k: u32, body: u32, request: u32) -> Result<u32, String> {
        if request == 0 {
            return Ok(0);
        }
        let id = self.m.r32(request);
        let slot = self.slot_record(k, body, id);
        let node = self.slot_node(k, id);
        let m = &mut *self.m;
        bind(m, request + 4, m.r32(slot + 0x24));
        let kind = m.r32(node + 0x4c) & 0xffff;
        if m.r32(slot + 0x20) != m.r32(body + 0x34) && kind > 0x64 && kind <= 0x190 {
            bind(m, request + 0x14, m.r32(body + 0x38));
            let index = (kind - 1) % 100;
            let wheel = m.r32(body + 0x13e4) + index * 0x2b4;
            // Locals of 45c510's 0x60-byte frame: point at +14, matrix at +30.
            let frame = crate::native_stack::sp(m).wrapping_sub(0x70);
            let point = frame + 0x14;
            let matrix = frame + 0x30;
            crate::native_matrix::identity(m, matrix);
            for i in 0..3 {
                m.w32(point + i * 4, m.r32(wheel + 0x78 + i * 4));
            }
            let mut fpu = Fpu::new();
            fpu.fld(m, point);
            fpu.fsub(m, k + 0x1eac);
            fpu.fld(m, point + 4);
            fpu.fsub(m, k + 0x1eb0);
            fpu.fstp(m, point + 4);
            fpu.fld(m, point + 8);
            fpu.fsub(m, k + 0x1eb4);
            fpu.fstp(m, point + 8);
            fpu.fadd(m, wheel + 0x84);
            fpu.fstp(m, point);
            for i in 1..3 {
                fpu.fld(m, point + i * 4);
                fpu.fadd(m, wheel + 0x84 + i * 4);
                fpu.fstp(m, point + i * 4);
            }
            let product = crate::native_entity::matrix_inplace_product(
                crate::native_entity::read_matrix(m, matrix),
                crate::native_entity::read_matrix(m, wheel + 0x90),
            );
            crate::native_entity::write_matrix(m, matrix, product);
            for i in 0..3 {
                m.w32(request + 0x24 + i * 4, m.r32(point + i * 4));
            }
            for i in 0..12 {
                m.w32(request + 0x30 + i * 4, m.r32(matrix + i * 4));
            }
        }
        Ok(self.m.r32(request + 0xc))
    }

    /// 45cea9: assign or clear the driver (`*argument` is the new driver
    /// object, zero when the driver leaves).
    fn car_driver(&mut self, k: u32, body: u32, argument: u32) -> Result<u32, String> {
        if argument == 0 {
            return Ok(self.m.r32(body + 0x1fc0));
        }
        let current = self.m.r32(body + 0x1fc0);
        let new = self.m.r32(argument);
        if current == new {
            return Ok(current);
        }
        if new == 0 {
            self.driver_leaves(body, current)?;
        } else if current == 0 {
            self.driver_enters(k, body, new)?;
        } else {
            follow_driver_link(self.m, body, new);
        }
        let m = &mut *self.m;
        m.w32(body + 0x1fc0, m.r32(argument));
        Ok(m.r32(body + 0x1fc0))
    }

    /// 45ced3: the car becomes an unattended obstacle again.
    fn driver_leaves(&mut self, body: u32, driver: u32) -> Result<(), String> {
        let m = &mut *self.m;
        flags_clear(m, body, 0x100);
        let root = crate::native_install::local_reference(m);
        let root_id = wt_root(m, body, root);
        let _ = root_id;
        if m.r32(root + 8) != 0 {
            let text = format!("obs_add {}", m.r32(body + 8) as i32);
            let buffer = crate::native_install::local_text(m, &text);
            self.host.command(self.m, root, 0, 0x10, buffer, 0)?;
        }
        let m = &mut *self.m;
        let target = m.r32(driver + 0x150);
        let link = body + 0x1fc4;
        if m.r32(body + 0x1fd0) != target {
            if m.r32(body + 0x1fd0) != 0 {
                unlink(m, link);
            }
            if target != 0 {
                bind(m, link, target);
            } else {
                for offset in [0xc, 8, 0, 4] {
                    m.w32(link + offset, 0);
                }
            }
        }
        m.w32(body + 0x58, m.r32(driver + 0x154));
        m.w32(body + 0x1e18, m.r32(m.r32(body + 0x1fbc) + 0xec));
        register_callback(m, body, 8);
        register_callback(m, body, 9);
        if m.r32(root + 12) != 0 {
            unlink(m, root);
        }
        Ok(())
    }

    /// 45cfd9: a driver takes the car: settle the physics root, reset its
    /// velocities, release wheel effect references and stop callbacks.
    fn driver_enters(&mut self, k: u32, body: u32, driver: u32) -> Result<(), String> {
        let m = &mut *self.m;
        let entity = m.r32(body + 0x13bc);
        if m.r32(entity + 0x58) & 0x80 == 0 {
            let mut root = entity;
            while m.r32(root + 0x58) & 4 != 0 {
                root = m.r32(root + 0x20);
            }
            crate::native_world::sleep(m, root)?;
        }
        let entity = m.r32(body + 0x13bc);
        let rotation = state(m, entity) + 0x40;
        let mut fpu = Fpu::new();
        fpu.fld(m, driver + 0xb0);
        fpu.fchs();
        let mut velocity = [0u32; 3];
        for (i, offset) in [8u32, 0x18, 0x28].into_iter().enumerate() {
            fpu.fld_st(0);
            fpu.fmul(m, rotation + offset);
            velocity[i] = fpu.fstp_bits();
        }
        let mut root = entity;
        while m.r32(root + 0x58) & 4 != 0 {
            root = m.r32(root + 0x20);
        }
        for buffer in [m.r32(root + 0x20c), m.r32(root + 0x20c) ^ 1] {
            let b = root + 0x64 + buffer * 0xd4;
            for i in 0..3 {
                m.w32(b + 0xc + i * 4, velocity[i as usize]);
            }
        }
        for buffer in [m.r32(root + 0x20c), m.r32(root + 0x20c) ^ 1] {
            let b = root + 0x64 + buffer * 0xd4;
            for i in 0..3 {
                m.w32(b + 0x70 + i * 4, 0);
            }
        }
        flags_set(m, body, 0x100);
        m.w32(body + 0x1dcc, 0);
        m.w32(body + 0x1e18, m.r32(m.r32(body + 0x1fbc) + 0xec));
        let count = self.m.r32(k + 0x1f40) as i32;
        for i in 0..count.max(0) as u32 {
            let wheel = self.m.r32(body + 0x13e4) + i * 0x2b4;
            for (link, flag) in [
                (wheel + 0x194, Effect::Skid),
                (wheel + 0x1b4, Effect::Smoke),
            ] {
                if self.m.r32(link + 8) != 0 {
                    self.effect_release(link, flag)?;
                    let m = &mut *self.m;
                    if m.r32(link + 12) != 0 {
                        unlink(m, link);
                    } else {
                        m.w32(link + 8, 0);
                    }
                    m.w32(link + 0x10, 0);
                }
            }
        }
        let m = &mut *self.m;
        unregister_callback(m, body, 8);
        unregister_callback(m, body, 9);
        follow_driver_link(m, body, driver);
        Ok(())
    }

    /// 48a610 / 48a800: flag a released wheel effect resource.
    fn effect_release(&mut self, link: u32, effect: Effect) -> Result<(), String> {
        let resource = self.m.r32(link + 12);
        if resource == 0 {
            return Ok(());
        }
        let data = self.data(resource, 0xa000_0001, 0x3f80_0000)?;
        if data == 0 {
            return Ok(());
        }
        let m = &mut *self.m;
        match effect {
            Effect::Skid => {
                let target = m.r32(data + 0x18);
                if target != 0 {
                    m.w32(target + 0x6c, m.r32(target + 0x6c) | 0x4000_0000);
                }
            }
            Effect::Smoke => {
                let target = m.r32(data + 0x1c);
                if target != 0 {
                    m.w32(target + 0xe4, m.r32(target + 0xe4) | 0x8000_0000);
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy)]
enum Effect {
    Skid,
    Smoke,
}

/// 48d210 / 48d1f0 on the body's resource flags.
fn flags_clear(m: &mut Space, body: u32, bits: u32) -> u32 {
    let resource = m.r32(body + 0xc);
    if resource == 0 {
        return 0;
    }
    let value = m.r32(resource + 0x54) & !bits;
    m.w32(resource + 0x54, value);
    value
}
fn flags_set(m: &mut Space, body: u32, bits: u32) -> u32 {
    let resource = m.r32(body + 0xc);
    if resource == 0 {
        return 0;
    }
    let value = m.r32(resource + 0x54) | bits;
    m.w32(resource + 0x54, value);
    value
}

/// Original48aa70 (body thiscall): bind `out` to the WT root of the body's
/// resource; with no output, return the root id.
pub fn wt_root(m: &mut Space, body: u32, out: u32) -> u32 {
    let resource = m.r32(body + 0xc);
    if m.r32(body + 8) == 0 || resource == 0 {
        return 0;
    }
    if out == 0 {
        return m.r32(resource + 0xc8);
    }
    let root = m.r32(resource + 0xcc);
    if m.r32(out + 12) != root {
        if m.r32(out + 12) != 0 {
            unlink(m, out);
        }
        if root != 0 {
            bind(m, out, root);
            return m.r32(out + 8);
        }
        for offset in [0xc, 8, 0, 4] {
            m.w32(out + offset, 0);
        }
    }
    m.r32(out + 8)
}

/// 45d238..45d2df / 45d2e1..45d35a: the driver's +144 reference follows the
/// car's +1fc4 target; the driver inherits the car's color scheme.
fn follow_driver_link(m: &mut Space, body: u32, driver: u32) {
    if m.r32(body + 0x1fcc) == 0 {
        return;
    }
    let target = m.r32(body + 0x1fd0);
    let link = driver + 0x144;
    if m.r32(driver + 0x150) != target {
        if m.r32(driver + 0x150) != 0 {
            unlink(m, link);
        }
        if target != 0 {
            bind(m, link, target);
        } else {
            for offset in [0xc, 8, 0, 4] {
                m.w32(link + offset, 0);
            }
        }
    }
    m.w32(driver + 0x154, m.r32(body + 0x58));
}

/// Original427370 on GameLogic (636338): append a callback registration.
pub fn register_callback(m: &mut Space, body: u32, kind: u32) {
    let context = 0x636338;
    let (tail, list, count) = match kind {
        8 => (0x18, 0x10, 0xc4),
        9 => (0x34, 0x2c, 0xc8),
        0x1c => (0x50, 0x48, 0xcc),
        _ => return,
    };
    let node = m.malloc(0x1c);
    if node != 0 {
        m.w32(node + 4, 0);
        m.w32(node + 8, 0);
        m.w32(node, 0x5f09bc);
        if kind == 0x1c {
            for offset in [0xc, 0x10, 0x14, 0x18] {
                m.w32(node + offset, 0);
            }
            m.w32(node, 0x5f09b4);
        } else {
            m.w32(node, 0x5f09b8);
            for offset in [0xc, 0x10, 0x14, 0x18] {
                m.w32(node + offset, 0);
            }
            m.w32(node, 0x5f09b4);
        }
        bind(m, node + 0xc, m.r32(body + 0xc));
    }
    let last = m.r32(context + tail);
    m.w32(last + 4, node);
    m.w32(node + 8, last);
    m.w32(node + 4, context + list);
    m.w32(context + tail, node);
    m.w32(context + count, m.r32(context + count) + 1);
}

/// Original4274e0: release the registration matching the body's id.
pub fn unregister_callback(m: &mut Space, body: u32, kind: u32) {
    let context = 0x636338;
    let head = match kind {
        8 => 8,
        9 => 0x24,
        0x1c => 0x40,
        _ => return,
    };
    let mut node = m.r32(context + head);
    if m.r32(node + 4) == 0 || node == 0 {
        return;
    }
    while m.r32(node + 0x14) != m.r32(body + 8) {
        node = m.r32(node + 4);
        if node == 0 || m.r32(node + 4) == 0 {
            return;
        }
    }
    let link = node + 0xc;
    if kind == 0x1c {
        if m.r32(node + 0x18) == 0 {
            m.w32(node + 0x14, 0);
            return;
        }
        unlink(m, link);
        return;
    }
    if m.r32(node + 0x18) != 0 {
        unlink(m, link);
    } else {
        m.w32(link + 8, 0);
    }
}

/// 45cb62 (and part message 49): next attached part after `out`'s id.
pub fn attached_part(m: &mut Space, body: u32, out: u32) -> u32 {
    let old = m.r32(out + 8);
    if m.r32(out + 12) != 0 {
        unlink(m, out);
    } else {
        m.w32(out + 8, 0);
    }
    let mut node = {
        let head = m.r32(body + 0xdc);
        if m.r32(head + 4) != 0 {
            head
        } else {
            0
        }
    };
    if node == 0 {
        return m.r32(out + 8);
    }
    if old == 0 {
        bind(m, out, m.r32(node + 0x18));
        return m.r32(out + 8);
    }
    while node != 0 {
        let raw = m.r32(node + 4);
        if old == m.r32(node + 0x14) {
            if raw != 0 && m.r32(raw + 4) != 0 {
                bind(m, out, m.r32(raw + 0x18));
            }
            break;
        }
        node = if raw == 0 || m.r32(raw + 4) == 0 {
            0
        } else {
            raw
        };
    }
    m.r32(out + 8)
}

/// Original4a6f30 (entity thiscall): world point of an optional local offset.
pub fn world_point(m: &mut Space, entity: u32, local: u32, out: u32) {
    let b = state(m, entity);
    if local == 0 {
        for i in 0..3 {
            m.w32(out + i * 4, m.r32(b + i * 4));
        }
        return;
    }
    let mut fpu = Fpu::new();
    let mut v = [0u32; 3];
    let rows = [
        [(0x40, 0), (0x48, 8), (0x44, 4)],
        [(0x58, 8), (0x54, 4), (0x50, 0)],
        [(0x68, 8), (0x64, 4), (0x60, 0)],
    ];
    for (i, terms) in rows.iter().enumerate() {
        fpu.fld(m, b + terms[0].0);
        fpu.fmul(m, local + terms[0].1);
        fpu.fld(m, b + terms[1].0);
        fpu.fmul(m, local + terms[1].1);
        fpu.faddp(1);
        fpu.fld(m, b + terms[2].0);
        fpu.fmul(m, local + terms[2].1);
        fpu.faddp(1);
        v[i] = fpu.fstp_bits();
    }
    for i in 0..3u32 {
        fpu.fld(m, b + i * 4);
        fpu.fld_bits(v[i as usize]);
        fpu.faddp(1);
        let word = fpu.fstp_bits();
        m.w32(out + i * 4, word);
    }
}

/// 45d472: box of the entity mesh's points around the template offset.
/// The original's maximum tracking compares against the just-updated
/// minima, so x/y maxima are the last point and z follows the stack.
fn bounds(m: &mut Space, k: u32, entity: u32, out: u32) {
    let mut fpu = Fpu::new();
    let local = crate::native_stack::sp(m).wrapping_sub(0x70);
    let v = local + 0x14;
    for i in 0..3 {
        m.w32(v + i * 4, m.r32(k + 0x1eac + i * 4));
    }
    fpu.fld(m, v);
    fpu.fsub(m, k + 0x1eb8);
    fpu.fst(m, v);
    fpu.fld(m, v + 4);
    fpu.fsub(m, k + 0x1ebc);
    fpu.fstp(m, v + 4);
    fpu.fld(m, v + 8);
    fpu.fsub(m, k + 0x1ec0);
    fpu.fstp(m, v + 8);
    // Compound members start at the entity itself, so the parent walk at
    // 45d4e3 never iterates: x stays where FST left it.
    fpu.fstp(m, v);
    let words = [m.r32(v), m.r32(v + 4), m.r32(v + 8)];
    for (i, word) in words.into_iter().enumerate() {
        m.w32(out + i as u32 * 4, word);
        m.w32(out + 0xc + i as u32 * 4, word);
    }
    fpu.fld(m, 0x5e73cc);
    let mesh = m.r32(m.r32(entity + 0x5c) + 0x78);
    let count = m.r32(mesh + 0x20) as i32;
    let extent = local + 0x14;
    for i in 0..5 {
        m.w32(extent + i * 4, 0);
    }
    if count > 0 {
        let mut point = m.r32(mesh + 0x24);
        fpu.fstp_st(0);
        for n in 0..count {
            if n > 0 {
                fpu.fstp_st(0);
            }
            for i in 0..3 {
                fpu.fld(m, point + i * 4);
                if fpu.fcomp(m, extent + i * 4).test(0x41) {
                    fpu.fld(m, point + i * 4);
                    fpu.fstp(m, extent + i * 4);
                }
            }
            for i in 0..2 {
                fpu.fld(m, point + i * 4);
                if fpu.fcomp(m, extent + i * 4).test(1) {
                    m.w32(extent + 0xc + i * 4, m.r32(extent + i * 4));
                } else {
                    fpu.fld(m, point + i * 4);
                    fpu.fstp(m, extent + 0xc + i * 4);
                }
            }
            fpu.fld(m, point + 8);
            if fpu.fcomp(m, extent + 8).test(1) {
                fpu.fld(m, extent + 8);
            } else {
                fpu.fld(m, point + 8);
            }
            point += 0xc;
        }
    }
    for i in 0..3 {
        fpu.fld(m, extent + i * 4);
        fpu.fadd(m, out + i * 4);
        fpu.fstp(m, out + i * 4);
    }
    for i in 0..2 {
        fpu.fld(m, extent + 0xc + i * 4);
        fpu.fadd(m, out + 0xc + i * 4);
        fpu.fstp(m, out + 0xc + i * 4);
    }
    fpu.fadd(m, out + 0x14);
    fpu.fstp(m, out + 0x14);
}

fn c_string(m: &Space, mut p: u32) -> Vec<u8> {
    let mut out = vec![];
    while m.r8(p) != 0 {
        out.push(m.r8(p));
        p += 1;
    }
    out
}

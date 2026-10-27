//! Source546070 and487f20's retained resource suffix. Package lookup538440
//! and manager resolution536820 are explicit domain collaborators; neither is
//! a script/native-result answer. Their raw resource owners must already exist.
use super::{pop, r, s, stack, write, Reg, RegistryBootstrap, Space};

pub trait Environment {
    fn package_id(
        &mut self,
        m: &mut Space,
        manager: u32,
        package: u32,
        id: u32,
    ) -> Result<u32, String>;
    fn resolve(
        &mut self,
        m: &mut Space,
        manager: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<u32, String>;
}

/// Consume the actual pending546070 CALL, including its three argument words.
/// This preserves the original unlink/list insertion stores and volatile GPRs.
pub fn bind(m: &mut Space, e: &mut impl Environment) -> Result<(), String> {
    if !stack::active(m) || !stack::has_pending_call(m) {
        return Err("NativeVmRegistryResources: actual546070 CALL required".into());
    }
    let node = r(m, Reg::Ecx);
    let args = std::array::from_fn::<_, 3, _>(|i| stack::argument(m, i));
    let frame = stack::enter(m, &args, Some(node));
    stack::push_reg(m, Reg::Esi);
    s(m, Reg::Esi, node);
    s(m, Reg::Eax, m.r32(node + 8));
    stack::push_reg(m, Reg::Edi);
    s(m, Reg::Edi, args[0]);
    if r(m, Reg::Eax) != args[0] {
        s(m, Reg::Ecx, m.r32(node + 12));
        if r(m, Reg::Ecx) != 0 {
            s(m, Reg::Eax, m.r32(node));
            if r(m, Reg::Eax) != 0 {
                s(m, Reg::Ecx, m.r32(node + 4));
                write(m, r(m, Reg::Eax) + 4, r(m, Reg::Ecx));
            } else {
                s(m, Reg::Edx, m.r32(node + 4));
                write(m, r(m, Reg::Ecx) + 0x48, r(m, Reg::Edx));
            }
            s(m, Reg::Eax, m.r32(node + 4));
            if r(m, Reg::Eax) != 0 {
                s(m, Reg::Ecx, m.r32(node));
                write(m, r(m, Reg::Eax), r(m, Reg::Ecx));
            }
            for offset in [12, 8, 0, 4] {
                write(m, node + offset, 0);
            }
        }
        if args[0] != 0 {
            s(m, Reg::Edx, args[2]);
            s(m, Reg::Eax, args[1]);
            let manager = m.r32(0x618d48);
            s(m, Reg::Ecx, manager);
            let resource = stack::external(m, 0x5460de, &args, Some(manager), |m| {
                e.resolve(m, manager, args[0], args[1], args[2])
            })?;
            s(m, Reg::Eax, resource);
            if resource != 0 {
                let list = resource.wrapping_add(0x44);
                s(m, Reg::Eax, list);
                write(m, node + 8, args[0]);
                s(m, Reg::Ecx, m.r32(list.wrapping_add(4)));
                if r(m, Reg::Ecx) != 0 {
                    write(m, r(m, Reg::Ecx), node);
                }
                write(m, node, 0);
                s(m, Reg::Ecx, m.r32(list.wrapping_add(4)));
                write(m, node + 4, r(m, Reg::Ecx));
                write(m, list.wrapping_add(4), node);
                s(m, Reg::Eax, if list == 0 { 0 } else { resource });
                if list != 0 {
                    pop(m, Reg::Edi);
                    write(m, node + 12, resource);
                    pop(m, Reg::Esi);
                    stack::leave(m, frame);
                    return Ok(());
                }
                write(m, node + 12, 0);
            }
        }
    }
    pop(m, Reg::Edi);
    pop(m, Reg::Esi);
    stack::leave(m, frame);
    Ok(())
}

impl RegistryBootstrap {
    /// Complete this retained487f20 caller. FPU state is untouched by every
    /// reached source instruction; the collaborators must preserve its owner.
    pub fn finish(self, m: &mut Space, e: &mut impl Environment) -> Result<(), String> {
        if self.pc != 0x489e20 || !stack::active(m) || stack::has_pending_call(m) {
            return Err("NativeVmRegistryResources: retained487f20 suffix required".into());
        }
        stack::push(m, 0);
        stack::push(m, 7);
        stack::push(m, 8);
        let manager = m.r32(0x618d48);
        s(m, Reg::Ecx, manager);
        stack::push(m, 0x61757c);
        let id = stack::external(m, 0x489e36, &[], Some(manager), |m| {
            e.package_id(m, manager, 0x61757c, 8)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Eax, id);
        stack::push(m, id);
        s(m, Reg::Ecx, 0x6408f8);
        stack::call(m, 0x489e41, &[], None, |m| bind(m, e))?;
        stack::add_sp(m, 12);
        let manager = m.r32(0x618d48);
        s(m, Reg::Ecx, manager);
        stack::push(m, 0);
        stack::push(m, 5);
        stack::push(m, 7);
        stack::push(m, 0x617588);
        let id = stack::external(m, 0x489e57, &[], Some(manager), |m| {
            e.package_id(m, manager, 0x617588, 7)
        })?;
        stack::add_sp(m, 8);
        s(m, Reg::Eax, id);
        stack::push(m, id);
        s(m, Reg::Ecx, 0x6408c0);
        stack::call(m, 0x489e62, &[], None, |m| bind(m, e))?;
        stack::add_sp(m, 12);
        stack::leave(m, self.caller);
        Ok(())
    }
}

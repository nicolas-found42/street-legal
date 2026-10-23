//! Original Part command/installation graph over raw template and body
//! layouts: original475ec0 and the slot search, attachment, render and
//! physics-tree operations it reaches.
//!
//! `K` is the configured part template (the native payload; vtable 5f0f60)
//! and `body` its runtime instance. Resource virtual calls, id lookup,
//! instance routing (426470/426800) and script execution are host calls
//! because each can run code that mutates memory the next instruction reads.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};

const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
/// 1e8 as stored by the slot searches' initial best distance.
const FAR: u32 = 0x4cbe_bc20;
const QUALITY_ONE: u32 = 0x3f80_0000;

/// One word of 425a90's variadic list (type tags and values as pushed).
pub type ScriptWords<'a> = &'a [u32];

pub trait Host {
    /// Resource vtable+14 (set requested minimum quality).
    fn minimum(&mut self, m: &mut Space, resource: u32, quality: u32) -> Result<(), String>;
    /// Original5447d0 with zero priority and time.
    fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String>;
    /// Resource vtable+0c with quality one.
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    /// Original546070's resolution through536820: the raw resource for an id
    /// of the requested kind, or zero.
    fn resolve(&mut self, m: &mut Space, id: u32, kind: u32, flags: u32) -> Result<u32, String>;
    /// Original426470 on its global context.
    fn query(
        &mut self,
        m: &mut Space,
        reference: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String>;
    /// Original425a90 → 4255e0 query of an envelope's script (getInfo).
    fn script_query(&mut self, m: &mut Space, envelope: u32, message: u32) -> Result<u32, String>;
    /// Original426800.
    fn command(
        &mut self,
        m: &mut Space,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        name: u32,
    ) -> Result<(), String>;
    /// Original425c60: deliver an event to the envelope's script handler.
    #[allow(clippy::too_many_arguments)]
    fn deliver(
        &mut self,
        m: &mut Space,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        envelope: u32,
        name: u32,
    ) -> Result<(), String>;
    /// Original425a90: invoke `method` on the envelope's script instance.
    fn script(
        &mut self,
        m: &mut Space,
        envelope: u32,
        method: &str,
        words: ScriptWords<'_>,
    ) -> Result<(), String>;
    /// Original411600 class lookup on the VM class table.
    fn vm_class(&mut self, m: &mut Space, name: &str) -> Result<u32, String>;
    /// Original407da0 object construction in already allocated storage.
    fn vm_object(&mut self, m: &mut Space, object: u32, class: u32) -> Result<(), String>;
    /// Original408800 named field cell of a VM object.
    fn vm_field(&mut self, m: &mut Space, object: u32, name: &str) -> Result<u32, String>;
    fn vm_reference_native(
        &mut self,
        _m: &mut Space,
        _object: u32,
        name: &str,
        _descriptor: &str,
    ) -> Result<Option<u32>, String> {
        Err(format!("NativeVmBoundary: native reference field {name}"))
    }
    fn rand(&mut self, _m: &mut Space) -> Result<u32, String> {
        Err("NativeVmBoundary: shared CRT rand".into())
    }
}

fn f(m: &Space, a: u32) -> X87 {
    X87::from_f32(m.rf(a))
}
fn st(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}
fn add(a: X87, b: X87) -> X87 {
    a.add(b, PC, RC)
}
fn sub(a: X87, b: X87) -> X87 {
    a.sub(b, PC, RC)
}
fn mul(a: X87, b: X87) -> X87 {
    a.mul(b, PC, RC)
}
/// `FCOMP` of `left` against `right` followed by `TEST AH,41; JNE`: true
/// when `left > right` (neither less, equal nor unordered).
fn greater(left: X87, right: X87) -> bool {
    let (codes, _) = left.fcom(right);
    !(codes.c0 || codes.c3)
}

/// Original4293a0: first node of an intrusive list header, or zero.
pub fn first(m: &Space, list: u32) -> u32 {
    let node = m.r32(list + 8);
    if m.r32(node + 4) != 0 {
        node
    } else {
        0
    }
}
/// Original429390.
pub fn empty(m: &Space, list: u32) -> bool {
    m.r32(m.r32(list + 8) + 4) == 0
}
/// Original40cfc0.
pub fn next(m: &Space, node: u32) -> u32 {
    let next = m.r32(node + 4);
    if next == 0 || m.r32(next + 4) == 0 {
        0
    } else {
        next
    }
}
/// Original429060: bind a weak reference to a raw resource.
pub fn bind(m: &mut Space, link: u32, resource: u32) {
    crate::native_part::bind_reference(m, link, resource);
}
/// Original429010 on `resource+44`: unlink and clear.
pub fn unlink(m: &mut Space, link: u32) {
    crate::native_part::bind_reference(m, link, 0);
}
/// Original45e520: release; an unbound reference only clears its id.
pub fn release(m: &mut Space, link: u32) {
    if m.r32(link + 12) != 0 {
        unlink(m, link);
    } else {
        m.w32(link + 8, 0);
    }
}
/// Fresh zeroed weak reference (428fc0/42cec0) in the native frame region.
fn local(m: &mut Space, bytes: u32) -> u32 {
    use crate::native_stack as stack;
    if stack::sp(m) == 0 {
        stack::set_sp(m, 0x7fef_ff00);
    }
    stack::alloc(m, bytes);
    let base = stack::sp(m);
    for offset in (0..bytes).step_by(4) {
        m.w32(base + offset, 0);
    }
    base
}

/// A zeroed 16-byte weak reference in the current native frame.
pub fn local_reference(m: &mut Space) -> u32 {
    local(m, 16)
}
/// A NUL-terminated copy of `text` in the current native frame.
pub fn local_text(m: &mut Space, text: &str) -> u32 {
    let base = local(m, (text.len() as u32 + 4) & !3);
    for (i, byte) in text.bytes().chain(std::iter::once(0)).enumerate() {
        m.w8(base + i as u32, byte);
    }
    base
}

pub struct Install<'a> {
    pub m: &'a mut Space,
    pub host: &'a mut dyn Host,
}

impl Install<'_> {
    /// Frame scope: locals allocated inside are released on return.
    fn frame<R>(&mut self, f: impl FnOnce(&mut Self) -> R) -> R {
        let saved = crate::native_stack::sp(self.m);
        let result = f(self);
        crate::native_stack::set_sp(self.m, saved);
        result
    }

    /// The inline419860 sequence: vtable+14 unless resident, 5447d0, then
    /// vtable+0c. `minimum` is the quality word the call site passes.
    pub fn data(&mut self, resource: u32, flags: u32, minimum: u32) -> Result<u32, String> {
        if self.m.r32(resource + 0x4c) != 1 {
            self.host.minimum(self.m, resource, minimum)?;
        }
        if self.host.prepare(self.m, resource, flags)? & 0x8000_0000 != 0 {
            return Ok(0);
        }
        self.host.payload(self.m, resource)
    }
    /// 419860 on a weak reference's resource, or zero when unbound.
    fn referenced(&mut self, link: u32, flags: u32, minimum: u32) -> Result<u32, String> {
        let resource = self.m.r32(link + 12);
        if resource == 0 {
            return Ok(0);
        }
        self.data(resource, flags, minimum)
    }
    /// Original546070: rebind `link` to the resource with `id`.
    fn bind_id(&mut self, link: u32, id: u32, kind: u32, flags: u32) -> Result<(), String> {
        if self.m.r32(link + 8) == id {
            return Ok(());
        }
        let m = &mut *self.m;
        if m.r32(link + 12) != 0 {
            unlink(m, link);
        }
        if id == 0 {
            return Ok(());
        }
        let resource = self.host.resolve(self.m, id, kind, flags)?;
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

    /// Original4707d0: the body slot record whose template slot has `id`.
    pub fn slot_record(&self, k: u32, body: u32, id: u32) -> u32 {
        let m = &*self.m;
        let mut node = first(m, k + 0x1154);
        let mut slot = m.r32(body + 0x78);
        while node != 0 {
            if m.r32(node + 0x48) == id {
                return slot;
            }
            node = next(m, node);
            slot += 0x68;
        }
        0
    }

    /// Original470720: template slot node with `id`, or zero.
    pub fn slot_node(&self, k: u32, id: u32) -> u32 {
        let m = &*self.m;
        let mut node = first(m, k + 0x1154);
        while node != 0 && m.r32(node + 0x48) != id {
            node = next(m, node);
        }
        node
    }

    /// Original471cf0: whether an installed descendant locks this body.
    pub fn locked(&mut self, k: u32, body: u32) -> Result<u32, String> {
        let count = self.m.r32(k + 0x1150) as i32;
        let mut flags = self.m.r32(body + 0x78) + 0x14;
        for _ in 0..count.max(0) {
            let word = self.m.r32(flags);
            if word & 1 != 0 {
                if word & 0x50 == 0x50 {
                    return Ok(1);
                }
                let resource = self.m.r32(flags - 8);
                if self.m.r32(flags - 0xc) != 0 && resource != 0 {
                    let data = self.data(resource, 0xa000_0000, QUALITY_ONE)?;
                    if data != 0 {
                        let secondary = self.m.r32(data + 0x44);
                        let child = self.m.r32(data + 0x4c);
                        if secondary == 0 {
                            return Ok(0);
                        }
                        let template = self.data(secondary, 0xa000_0000, QUALITY_ONE)?;
                        if template == 0 {
                            return Ok(0);
                        }
                        let k2 = self.m.r32(template + 0xc);
                        return self.locked(k2, child);
                    }
                }
            }
            flags += 0x68;
        }
        Ok(0)
    }

    /// Original470910 (cdecl): whether two template slots are linked or
    /// share an exclusion pair. `a`/`b` are weak references to templates.
    pub fn conflicting(&mut self, a: u32, a_slot: u32, b: u32, b_slot: u32) -> Result<u32, String> {
        let m = &*self.m;
        if m.r32(a + 8) == m.r32(b + 8) && a_slot == b_slot {
            return Ok(0);
        }
        let find = |this: &mut Self, link: u32, id: u32| -> Result<u32, String> {
            let data = this.referenced(link, 0xa000_0000, QUALITY_ONE)?;
            if data == 0 {
                return Ok(0);
            }
            let k = this.m.r32(data + 0xc);
            if k == 0 {
                return Ok(0);
            }
            let m = &*this.m;
            let mut node = first(m, k + 0x1154);
            while node != 0 && m.r32(node + 0x48) != id {
                node = next(m, node);
            }
            Ok(node)
        };
        let na = find(self, a, a_slot)?;
        let nb = find(self, b, b_slot)?;
        let m = &*self.m;
        let raw_first = |list: u32| if empty(m, list) { 0 } else { m.r32(list + 8) };
        let mut node = raw_first(na + 0x74);
        while node != 0 {
            if m.r32(node + 0xc) == m.r32(b + 8) && m.r32(node + 0x10) == b_slot {
                return Ok(1);
            }
            node = next(m, node);
        }
        let mut node = raw_first(nb + 0x74);
        while node != 0 {
            if m.r32(node + 0xc) == m.r32(a + 8) && m.r32(node + 0x10) == a_slot {
                return Ok(1);
            }
            node = next(m, node);
        }
        let mut outer = raw_first(na + 0x74);
        while outer != 0 {
            let mut inner = raw_first(nb + 0x90);
            while inner != 0 {
                if m.r32(outer + 0xc) == m.r32(inner + 0xc)
                    && m.r32(outer + 0x10) == m.r32(inner + 0x10)
                {
                    return Ok(1);
                }
                inner = next(m, inner);
            }
            outer = next(m, outer);
        }
        let mut outer = first(m, nb + 0x74);
        while outer != 0 {
            let mut inner = first(m, na + 0x90);
            while inner != 0 {
                if m.r32(outer + 0xc) == m.r32(inner + 0xc)
                    && m.r32(outer + 0x10) == m.r32(inner + 0x10)
                {
                    return Ok(1);
                }
                inner = next(m, inner);
            }
            outer = next(m, outer);
        }
        Ok(0)
    }

    /// The shared distance step of 4720a0/473790/473b10: world position of a
    /// slot record through its render instance (436880 rotation plus the
    /// instance origin) and the squared distance to `position`, each in the
    /// call site's own x87 order. None when the slot has no render instance.
    fn slot_distance(
        &mut self,
        slot: u32,
        position: u32,
        order: Order,
    ) -> Result<Option<X87>, String> {
        let instance = crate::native_render_graph::instance(self, slot + 0x18)?;
        if instance == 0 {
            return Ok(None);
        }
        let m = &*self.m;
        let state = m.r32(instance + 0x84);
        let index = m.r32(state + 0x20c);
        let v = transform(m, state + 0xa4 + index * 0xd4, slot + 0x2c);
        let origin = state + 0x64 + index * 0xd4;
        let rotated = |i: usize| X87::from_f32(f32::from_bits(v[i]));
        let p = |i: u32| f(m, position + i * 4);
        let o = |i: u32| f(m, origin + i * 4);
        let reload = |w: u32| X87::from_f32(f32::from_bits(w));
        Ok(Some(match order {
            // 477560 stores all three sums; 473a5f reloads z, y, x.
            Order::Separate => {
                let w = [0, 1, 2].map(|i| st(add(o(i), rotated(i as usize))));
                let (dx, dy, dz) = (
                    sub(reload(w[0]), p(0)),
                    sub(reload(w[1]), p(1)),
                    sub(reload(w[2]), p(2)),
                );
                add(add(mul(dz, dz), mul(dx, dx)), mul(dy, dy))
            }
            // x and y are stored; z stays on the stack (FST) for its FSUB.
            Order::Xyz | Order::Yzx => {
                let wx = st(add(rotated(0), o(0)));
                let wy = st(add(rotated(1), o(1)));
                let dz = sub(add(rotated(2), o(2)), p(2));
                let dy = sub(reload(wy), p(1));
                let dx = sub(reload(wx), p(0));
                if order == Order::Xyz {
                    add(add(mul(dx, dx), mul(dy, dy)), mul(dz, dz))
                } else {
                    add(add(mul(dy, dy), mul(dz, dz)), mul(dx, dx))
                }
            }
        }))
    }

    /// Original4720a0: best free slot of the target template that `link`
    /// (a template reference, slot `id`) may attach to.
    pub fn free_target_slot(
        &mut self,
        target_k: u32,
        target_body: u32,
        link: u32,
        id: u32,
        position: u32,
    ) -> Result<u32, String> {
        let mut node = {
            let head = self.m.r32(target_k + 0x115c);
            if self.m.r32(head + 4) != 0 {
                head
            } else {
                0
            }
        };
        let mut flags = self.m.r32(target_body + 0x78) + 0x14;
        let mut best = X87::from_f32(f32::from_bits(FAR));
        let mut result = 0;
        if self.locked(target_k, target_body)? != 0 {
            return Ok(0);
        }
        while node != 0 {
            if self.m.r32(flags - 0xc) == 0
                && self.m.r32(flags) & 0x10 == 0
                && self.conflicting(target_k + 4, self.m.r32(node + 0x48), link, id)? != 0
            {
                if position == 0 {
                    return Ok(self.m.r32(node + 0x48));
                }
                let slot = self.slot_record(target_k, target_body, self.m.r32(node + 0x48));
                if let Some(distance) = self.slot_distance(slot, position, Order::Yzx)? {
                    if greater(best, distance) {
                        best = X87::from_f32(f32::from_bits(st(distance)));
                        result = self.m.r32(node + 0x48);
                    }
                }
            }
            node = next(self.m, node);
            flags += 0x68;
        }
        Ok(result)
    }
    /// Original473790: a free slot of the target referenced by `link` for
    /// this template's slot `*out` (zero: any), optionally nearest `position`.
    pub fn target_slot(
        &mut self,
        k: u32,
        body: u32,
        link: u32,
        out: u32,
        position: u32,
    ) -> Result<u32, String> {
        if (self.host.query(self.m, link, 0x27, 0)? as i32) <= 0 {
            return Ok(0);
        }
        if self.m.r32(link + 8) == self.m.r32(body + 8) {
            return Ok(0);
        }
        let want = self.m.r32(out);
        let mut result = 0;
        let mut best = X87::from_f32(f32::from_bits(FAR));
        let data = self.referenced(link, 0xa000_0000, QUALITY_ONE)?;
        if data == 0 {
            return Ok(0);
        }
        let target_body = self.m.r32(data + 0x4c);
        let template_link = data + 0x38;
        let secondary = self.m.r32(data + 0x44);
        if secondary == 0 {
            return Ok(0);
        }
        let template = self.data(secondary, 0xa000_0000, QUALITY_ONE)?;
        if template == 0 {
            return Ok(0);
        }
        let target_k = self.m.r32(template + 0xc);
        if want != 0 {
            let mut node = first(self.m, target_k + 0x1154);
            let mut flags = self.m.r32(target_body + 0x78) + 0x14;
            if self.locked(target_k, target_body)? != 0 {
                return Ok(0);
            }
            while node != 0 {
                if self.m.r32(node + 0x48) == want
                    && self.m.r32(flags - 0xc) == 0
                    && self.m.r32(flags) & 0x10 == 0
                {
                    let mut own = if empty(self.m, k + 0x1154) {
                        0
                    } else {
                        self.m.r32(k + 0x115c)
                    };
                    let count = self.m.r32(k + 0x1150) as i32;
                    let mut slot = 0;
                    for _ in 0..count.max(0) {
                        let record = self.m.r32(body + 0x78) + slot;
                        if self.m.r32(record + 8) == 0
                            && self.m.r32(record + 0x14) & 0x10 == 0
                            && self.conflicting(
                                k + 4,
                                self.m.r32(own + 0x48),
                                template_link,
                                want,
                            )? != 0
                        {
                            return Ok(self.m.r32(own + 0x48));
                        }
                        own = next(self.m, own);
                        slot += 0x68;
                    }
                    return Ok(result);
                }
                node = next(self.m, node);
                flags += 0x68;
            }
            return Ok(result);
        }
        let mut node = {
            let head = self.m.r32(k + 0x115c);
            if self.m.r32(head + 4) != 0 {
                head
            } else {
                0
            }
        };
        let count = self.m.r32(k + 0x1150) as i32;
        let mut slot = 0;
        for _ in 0..count.max(0) {
            let record = self.m.r32(body + 0x78) + slot;
            let attached = self.m.r32(record + 8);
            let flags = self.m.r32(record + 0x14);
            let eligible = (attached == 0 || flags & 1 != 0) && flags & 0x10 == 0;
            if eligible {
                let found = self.free_target_slot(
                    target_k,
                    target_body,
                    k + 4,
                    self.m.r32(node + 0x48),
                    position,
                )?;
                if found != 0 {
                    if position == 0 {
                        self.m.w32(out, found);
                        return Ok(self.m.r32(node + 0x48));
                    }
                    let target_slot = self.slot_record(target_k, target_body, found);
                    if let Some(distance) =
                        self.slot_distance(target_slot, position, Order::Separate)?
                    {
                        if greater(best, distance) {
                            best = X87::from_f32(f32::from_bits(st(distance)));
                            self.m.w32(out, found);
                            result = self.m.r32(node + 0x48);
                        }
                    }
                }
            }
            node = next(self.m, node);
            slot += 0x68;
        }
        Ok(result)
    }

    /// Original473b10: choose the slot pair for an install command.
    /// `link` names the requested target; the body's +60 reference supplies
    /// a hint. Returns this template's slot id and stores the target's.
    pub fn choose_slot(
        &mut self,
        k: u32,
        body: u32,
        link: u32,
        out: u32,
        position: u32,
    ) -> Result<u32, String> {
        self.frame(|this| this.choose_slot_inner(k, body, link, out, position))
    }
    fn choose_slot_inner(
        &mut self,
        k: u32,
        body: u32,
        link: u32,
        out: u32,
        position: u32,
    ) -> Result<u32, String> {
        let l40 = local(self.m, 16);
        let l30 = local(self.m, 16);
        let l10 = local(self.m, 4);
        let hint = self.m.r32(body + 0x6c);
        bind(self.m, l30, hint);
        self.m.w32(l10, self.m.r32(out));
        let mut chosen = 0;
        let mut best = X87::from_f32(f32::from_bits(FAR));
        if self.m.r32(link + 8) != 0 {
            let r = self.target_slot(k, body, link, l10, 0)?;
            if r != 0 {
                self.m.w32(out, self.m.r32(l10));
                release(self.m, l30);
                release(self.m, l40);
                return Ok(r);
            }
        }
        let kind = self.host.query(self.m, l30, 7, 0)?;
        if kind == 9 || kind == 5 {
            let r = self.target_slot(k, body, l30, l10, position)?;
            if r != 0 {
                if position == 0 {
                    bind(self.m, link, self.m.r32(l30 + 12));
                    self.m.w32(out, self.m.r32(l10));
                    if self.m.r32(l30 + 12) != 0 {
                        unlink(self.m, l30);
                    } else {
                        self.m.w32(l30 + 8, 0);
                    }
                    if self.m.r32(l40 + 12) != 0 {
                        unlink(self.m, l40);
                    }
                    return Ok(r);
                }
                if let Some(distance) = self.candidate_distance(l30, l10, position, Order::Xyz)? {
                    // 473cd2 compares against the constant at5f12a0.
                    if greater(f(self.m, 0x5f12a0), distance) {
                        best = X87::from_f32(f32::from_bits(st(distance)));
                        bind(self.m, link, self.m.r32(l30 + 12));
                        self.m.w32(out, self.m.r32(l10));
                        chosen = r;
                    }
                }
            }
            if kind == 5 {
                let l50 = local(self.m, 16);
                while self.host.query(self.m, l30, 0x31, l50)? != 0 {
                    self.m.w32(l10, 0);
                    let r = self.target_slot(k, body, l50, l10, position)?;
                    if r != 0 {
                        if position == 0 {
                            bind(self.m, link, self.m.r32(l50 + 12));
                            self.m.w32(out, self.m.r32(l10));
                            if self.m.r32(l50 + 12) != 0 {
                                unlink(self.m, l50);
                            } else {
                                self.m.w32(l50 + 8, 0);
                            }
                            if self.m.r32(l30 + 12) != 0 {
                                unlink(self.m, l30);
                            } else {
                                self.m.w32(l30 + 8, 0);
                            }
                            if self.m.r32(l40 + 12) != 0 {
                                unlink(self.m, l40);
                            }
                            return Ok(r);
                        }
                        if let Some(distance) =
                            self.candidate_distance(l50, l10, position, Order::Yzx)?
                        {
                            if greater(best, distance) {
                                best = X87::from_f32(f32::from_bits(st(distance)));
                                bind(self.m, link, self.m.r32(l50 + 12));
                                self.m.w32(out, self.m.r32(l10));
                                chosen = r;
                            }
                        }
                    }
                }
                if self.m.r32(l50 + 12) != 0 {
                    unlink(self.m, l50);
                }
            }
        }
        if self.m.r32(l30 + 12) != 0 {
            unlink(self.m, l30);
        }
        if self.m.r32(l40 + 12) != 0 {
            unlink(self.m, l40);
        }
        Ok(chosen)
    }
    /// 473bf1/473da1: distance from the referenced target's chosen slot.
    fn candidate_distance(
        &mut self,
        link: u32,
        slot_id: u32,
        position: u32,
        order: Order,
    ) -> Result<Option<X87>, String> {
        let data = self.referenced(link, 0xa000_0000, QUALITY_ONE)?;
        if data == 0 {
            return Ok(None);
        }
        let target_body = self.m.r32(data + 0x4c);
        let secondary = self.m.r32(data + 0x44);
        if secondary == 0 {
            return Ok(None);
        }
        let template = self.data(secondary, 0xa000_0000, QUALITY_ONE)?;
        if template == 0 {
            return Ok(None);
        }
        let target_k = self.m.r32(template + 0xc);
        let slot = self.slot_record(target_k, target_body, self.m.r32(slot_id));
        self.slot_distance(slot, position, order)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Order {
    /// 473790: stored world sums, (z*z + x*x) + y*y.
    Separate,
    /// 473b10's direct target: (x*x + y*y) + z*z.
    Xyz,
    /// 4720a0 and 473b10's attached targets: (y*y + z*z) + x*x.
    Yzx,
}

impl Install<'_> {
    /// Original474600: attach the body's slot `own` to `target_slot` of the
    /// part referenced by `target`.
    pub fn install(
        &mut self,
        k: u32,
        body: u32,
        own: u32,
        target: u32,
        target_slot: u32,
    ) -> Result<(), String> {
        self.frame(|this| this.install_inner(k, body, own, target, target_slot))
    }
    fn install_inner(
        &mut self,
        k: u32,
        body: u32,
        own: u32,
        target: u32,
        target_slot: u32,
    ) -> Result<(), String> {
        if self.m.r32(body + 0x68) != 0 {
            crate::native_render_graph::set_parent(self, body, body + 0x60)?;
        }
        let mut i = 0;
        while (i as i32) < self.m.r32(k + 0xff8) as i32 {
            let render = self.m.r32(body + 0x4c) + i * 0x10;
            crate::native_render_graph::set_parent(self, render, body)?;
            i += 1;
        }
        self.m.w32(body + 0x5c, 1);
        // EBP frame: position at -20, rotation at -b0 (twelve words).
        let position = local(self.m, 0xc);
        let rotation = local(self.m, 0x30);
        let result = self.attach_frame(k, body, own, target, target_slot, position, rotation)?;
        let instance = crate::native_render_graph::instance(self, body + 0x2c)?;
        let link = if instance != 0 {
            body + 0x2c
        } else {
            body + 0x1c
        };
        crate::native_render_graph::set_transform(self, link, position, rotation)?;
        let record = self.slot_record(k, body, own);
        let m = &mut *self.m;
        m.w32(record + 0x14, m.r32(record + 0x14) | 4);
        bind(m, record, m.r32(target + 12));
        m.w32(record + 0x10, target_slot);
        let target_resource = self.m.r32(target + 12);
        if target_resource != 0 {
            let data = self.data(target_resource, 0xa000_0000, QUALITY_ONE)?;
            if data != 0 && self.m.r32(data + 0x4c) != 0 {
                let target_body = self.m.r32(data + 0x4c);
                let template_link = local(self.m, 16);
                if self.host.query(self.m, target, 6, template_link)? != 0 {
                    if self.m.r32(template_link + 12) != 0 {
                        let template = self.referenced(template_link, 0xa000_0000, QUALITY_ONE)?;
                        if template != 0 {
                            let target_k = self.m.r32(template + 0xc);
                            let other = self.slot_record(target_k, target_body, target_slot);
                            let m = &mut *self.m;
                            m.w32(other + 0x14, m.r32(other + 0x14) | 4);
                            bind(m, other, m.r32(body + 12));
                            m.w32(other + 0x10, own);
                        }
                        unlink(self.m, template_link);
                    }
                } else if self.m.r32(template_link + 12) != 0 {
                    unlink(self.m, template_link);
                }
            }
        }
        if self.m.r32(k + 0x11b8) != 0 {
            let physics = crate::native_render_graph::instance(self, body + 0x1c)?;
            if physics != 0 {
                return Err("NativeRenderBoundary: part bone attachment 48bc40/48c0f0".into());
            }
        }
        if result == 0 {
            self.complete_install(k, body)?;
        }
        Ok(())
    }

    /// Original472e80: world placement for attaching `own` to the target's
    /// `target_slot`, and the summed slot weights (+54) of both sides.
    #[allow(clippy::too_many_arguments)]
    pub fn attach_frame(
        &mut self,
        k: u32,
        body: u32,
        own: u32,
        target: u32,
        target_slot: u32,
        position_out: u32,
        rotation_out: u32,
    ) -> Result<u32, String> {
        self.frame(|this| {
            this.attach_frame_inner(
                k,
                body,
                own,
                target,
                target_slot,
                position_out,
                rotation_out,
            )
        })
    }
    #[allow(clippy::too_many_arguments)]
    fn attach_frame_inner(
        &mut self,
        k: u32,
        body: u32,
        own: u32,
        target: u32,
        target_slot: u32,
        position_out: u32,
        rotation_out: u32,
    ) -> Result<u32, String> {
        let position = local(self.m, 0xc);
        let rotation = local(self.m, 0x30);
        let offset = local(self.m, 0xc);
        let mut weight = 0u32;
        let target_resource = self.m.r32(target + 12);
        if target_resource != 0 {
            let data = self.data(target_resource, 0xa000_0000, QUALITY_ONE)?;
            let target_body = if data == 0 {
                0
            } else {
                self.m.r32(data + 0x4c)
            };
            if target_body != 0 {
                let unused = local(self.m, 16);
                let request = local(self.m, 0x60);
                self.m.w32(request, target_slot);
                if self.host.query(self.m, target, 0x2f, request)? != 0 {
                    let instance = crate::native_render_graph::instance(self, request + 4)?;
                    if instance != 0 {
                        let m = &mut *self.m;
                        let entity = m.r32(instance + 0x84);
                        let state = entity + 0x64 + m.r32(entity + 0x20c) * 0xd4;
                        for i in 0..3 {
                            m.w32(position + i * 4, m.r32(state + i * 4));
                        }
                        for i in 0..12 {
                            m.w32(rotation + i * 4, m.r32(state + 0x40 + i * 4));
                        }
                    }
                }
                let template_link = local(self.m, 16);
                if self.host.query(self.m, target, 6, template_link)? != 0
                    && self.m.r32(template_link + 12) != 0
                {
                    let template = self.referenced(template_link, 0xa000_0000, QUALITY_ONE)?;
                    let target_k = if template == 0 {
                        0
                    } else {
                        self.m.r32(template + 0xc)
                    };
                    if target_k != 0 {
                        let slot = self.slot_record(target_k, target_body, target_slot);
                        let node = self.slot_node(target_k, target_slot);
                        let m = &mut *self.m;
                        let v = rotate_rows(m, rotation, slot + 0x2c);
                        let mut fpu = crate::native_fpu::Fpu::new();
                        for i in 0..3 {
                            fpu.fld_bits(v[i as usize]);
                            fpu.fadd(m, position + i * 4);
                            fpu.fstp(m, position + i * 4);
                        }
                        let frame = crate::native_stack::sp(m).wrapping_sub(8);
                        crate::native_matrix::multiply_in_place(m, rotation, slot + 0x38, frame);
                        weight = m.r32(node + 0x54);
                    }
                }
                for link in [template_link, request + 0x14, request + 4, unused] {
                    release(self.m, link);
                }
            }
        }
        let record = self.slot_record(k, body, own);
        let node = self.slot_node(k, own);
        let m = &mut *self.m;
        let transposed = local(m, 0x30);
        crate::native_matrix::transpose(m, transposed, record + 0x38);
        let frame = crate::native_stack::sp(m).wrapping_sub(8);
        crate::native_matrix::multiply_in_place(m, rotation, transposed, frame);
        crate::native_matrix::rotate(m, offset, rotation, record + 0x2c);
        let mut fpu = crate::native_fpu::Fpu::new();
        for i in 0..3 {
            fpu.fld(m, position + i * 4);
            fpu.fsub(m, offset + i * 4);
            fpu.fstp(m, position + i * 4);
        }
        let weight = weight.wrapping_add(m.r32(node + 0x54));
        if position_out != 0 {
            for i in 0..3 {
                m.w32(position_out + i * 4, m.r32(position + i * 4));
            }
        }
        if rotation_out != 0 {
            for i in 0..12 {
                m.w32(rotation_out + i * 4, m.r32(rotation + i * 4));
            }
        }
        Ok(weight)
    }

    /// Original474170: mark pending attachments installed, propagate render
    /// type flags, join the physics compound, notify the owner and run the
    /// part's `install` script.
    pub fn complete_install(&mut self, k: u32, body: u32) -> Result<(), String> {
        use crate::native_stack::{self as stack, Reg};
        let frame = stack::enter(self.m, &[body], Some(k));
        stack::alloc(self.m, 0x90);
        for r in [Reg::Ebx, Reg::Esi, Reg::Edi] {
            stack::push_reg(self.m, r);
        }
        stack::set_reg(self.m, Reg::Edi, k);
        stack::store(self.m, 0xc, k);
        stack::push_reg(self.m, Reg::Ebp);
        stack::set_reg(self.m, Reg::Ebp, body);
        stack::set_reg(self.m, Reg::Ebx, 0);
        stack::set_reg(self.m, Reg::Esi, self.m.r32(body + 0x78));
        for offset in [0x14, 0x18, 0x1c, 0x20, 0x24, 0x28, 0x2c, 0x34] {
            stack::store(self.m, offset, 0);
        }
        let result = self.complete_install_inner(k, body);
        stack::leave(self.m, frame);
        result
    }
    fn complete_install_inner(&mut self, k: u32, body: u32) -> Result<(), String> {
        let frame_base = crate::native_stack::sp(self.m);
        let owner_link = frame_base + 0x18;
        let mut first_record = 0;
        let mut first_node = 0;
        let mut target_node = 0;
        let mut node = if empty(self.m, k + 0x1154) {
            0
        } else {
            self.m.r32(k + 0x115c)
        };
        let mut record = self.m.r32(body + 0x78);
        // [esp+14] persists across iterations like the original local.
        let mut target_body = 0;
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &mut *self.m;
            if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 4 != 0 {
                m.w32(record + 0x14, (m.r32(record + 0x14) & !4) | 1);
                m.w32(body + 0x74, m.r32(body + 0x74) + 1);
                let resource = m.r32(record + 0xc);
                if resource != 0 {
                    let data = self.data(resource, 0xa000_0000, 0)?;
                    if data != 0 {
                        target_body = self.m.r32(data + 0x4c);
                        let secondary = self.m.r32(data + 0x44);
                        if secondary != 0 {
                            let template = self.data(secondary, 0xa000_0000, 0)?;
                            if template != 0 {
                                let target_k = self.m.r32(template + 0xc);
                                let slot = self.m.r32(record + 0x10);
                                target_node = self.slot_node(target_k, slot);
                                let other = self.slot_record(target_k, target_body, slot);
                                let m = &mut *self.m;
                                m.w32(other + 0x14, (m.r32(other + 0x14) & !4) | 2);
                                m.w32(target_body + 0x74, m.r32(target_body + 0x74) + 1);
                                self.mass(target_k, target_body, 1)?;
                            }
                        }
                    }
                }
                let m = &mut *self.m;
                let flags = m.r32(target_node + 0x50);
                let old = m.r32(body + 0xb0);
                let mut value = m.r32(target_body + 0xb0);
                m.w32(body + 0xb0, value);
                if flags & 0xffff != 0xffff {
                    value = (value & 0xffff_0000) | (flags & 0xffff);
                    m.w32(body + 0xb0, value);
                }
                if flags & 0xffff_0000 != 0xffff_0000 {
                    value = (m.r32(body + 0xb0) & 0xffff) | (flags & 0xffff_0000);
                    m.w32(body + 0xb0, value);
                }
                if m.r32(body + 0xb0) != old {
                    self.render_type(k, body)?;
                }
                if first_record == 0 {
                    bind(self.m, owner_link, self.m.r32(record + 0xc));
                    first_record = record;
                    first_node = target_node;
                }
            }
            node = next(self.m, node);
            record += 0x68;
            i += 1;
        }
        let _ = (node, first_record);
        let physics = crate::native_render_graph::instance(self, body + 0x1c)?;
        if physics != 0 {
            let entity = self.m.r32(physics + 0x84);
            let request = frame_base + 0x40;
            for offset in (4..=0x20).step_by(4) {
                self.m.w32(request + offset, 0);
            }
            self.m.w32(request, self.m.r32(first_node + 0x48));
            if self.host.query(self.m, owner_link, 0x2f, request)? != 0 {
                let target = crate::native_render_graph::instance(self, request + 4)?;
                if target != 0 {
                    crate::native_stack::set_sp(self.m, frame_base);
                    self.join_compound(entity, self.m.r32(target + 0x84))?;
                }
            }
            release(self.m, request + 0x14);
            if self.m.r32(request + 4 + 12) != 0 {
                unlink(self.m, request + 4);
            }
        }
        self.propagate_owner(k, body)?;
        if self.m.r32(body + 0xc8) != 0 {
            self.remove_from_owner(k, body, 0)?;
        }
        self.host.query(self.m, owner_link, 0x2b, body + 0xc0)?;
        if self.m.r32(body + 0xc8) != 0 {
            let flags = self.render_flags(k, body, 0)?;
            self.m.w32(body + 0xd0, flags);
            self.add_to_owner(k, body, 0)?;
        }
        let resource = self.m.r32(body + 0xc);
        if resource != 0 {
            let data = self.data(resource, 0xa000_0000, 0)?;
            if data != 0 && self.m.r32(data + 0x50) != 0 {
                self.host.script(self.m, data, "install", &[0x4f, 0x4f])?;
            }
        }
        let m = &mut *self.m;
        if m.r32(body + 0x6c) != 0 {
            unlink(m, body + 0x60);
        } else {
            m.w32(body + 0x68, 0);
        }
        if m.r32(owner_link + 12) != 0 {
            unlink(m, owner_link);
        }
        Ok(())
    }

    /// 4743e8..47449e: the part's physics entity joins its new owner's
    /// compound (activating sleeping roots, then sleeping it again).
    fn join_compound(&mut self, entity: u32, target: u32) -> Result<(), String> {
        use crate::native_stack::{self as stack, Reg};
        let m = &mut *self.m;
        let mut parent = target;
        if m.r32(parent + 0x58) & 0x8000 == 0
            && m.r32(parent + 0x20) != 0
            && m.r32(parent + 0x1c) != 0
        {
            parent = m.r32(parent + 0x20);
        }
        let definition = m.r32(entity + 0x5c);
        m.w32(definition + 0x9c, m.r32(definition + 0x9c) & !4);
        let parent_flags = m.r32(parent + 0x58);
        let flags = m.r32(entity + 0x58) | 1;
        m.w32(entity + 0x58, flags);
        let root = |m: &Space, mut e: u32| {
            while m.r32(e + 0x58) & 4 != 0 {
                e = m.r32(e + 0x20);
            }
            e
        };
        stack::set_reg(m, Reg::Edi, parent);
        stack::set_reg(m, Reg::Esi, entity);
        stack::set_reg(m, Reg::Ebx, parent_flags);
        if flags & 0x80 != 0 {
            let r = root(m, entity);
            stack::external(m, 0x47443e, &[], Some(r), |m| {
                crate::native_world::activate(m, r)
            })?;
        }
        if m.r32(parent + 0x58) & 0x80 != 0 {
            let r = root(m, parent);
            stack::external(m, 0x474459, &[], Some(r), |m| {
                crate::native_world::activate(m, r)
            })?;
        }
        stack::call(
            m,
            0x474465,
            &[entity],
            Some(slr_physics::cfg::PHYSICS_WORLD),
            |m| crate::native_world::detach(m, entity),
        )?;
        stack::call(
            m,
            0x474472,
            &[entity, parent],
            Some(slr_physics::cfg::PHYSICS_WORLD),
            |m| crate::native_world::attach(m, entity, parent),
        )?;
        stack::call(
            m,
            0x47447e,
            &[parent],
            Some(slr_physics::cfg::PHYSICS_WORLD),
            |m| crate::native_entity::compound(m, parent),
        )?;
        if parent_flags & 0x80 != 0 && m.r32(parent + 0x58) & 0x80 == 0 {
            let r = root(m, parent);
            stack::call(m, 0x47449e, &[], Some(r), |m| {
                crate::native_world::sleep(m, r)
            })?;
        }
        Ok(())
    }

    /// Original46a250: aggregate attached weights (+f0) through the slot
    /// tree; `upward` follows the part this one is installed on.
    pub fn mass(&mut self, k: u32, body: u32, upward: u32) -> Result<(), String> {
        let mut scale = self.m.r32(k + 0x11e0);
        let mut sum = 0u32;
        let mut parent_record = 0;
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &*self.m;
            if m.r32(record + 8) != 0 {
                let flags = m.r32(record + 0x14);
                if flags & 1 != 0 {
                    parent_record = record;
                } else if flags & 2 != 0 && m.r32(record + 0xc) != 0 {
                    let resource = m.r32(record + 0xc);
                    let data = self.data(resource, 0xa000_0000, QUALITY_ONE)?;
                    let secondary = if data == 0 {
                        0
                    } else {
                        self.m.r32(data + 0x44)
                    };
                    if secondary != 0 {
                        let template = self.data(secondary, 0x2000_0000, QUALITY_ONE)?;
                        if template != 0 {
                            let child = self.m.r32(data + 0x4c);
                            let child_k = self.m.r32(template + 0xc);
                            if upward == 0 {
                                self.mass(child_k, child, 0)?;
                            }
                            let m = &*self.m;
                            let mut fpu = crate::native_fpu::Fpu::new();
                            fpu.fld(m, 0x5f08f0);
                            fpu.fsub(m, child_k + 0x11e4);
                            fpu.arith_m_bits(crate::native_fpu::Op::Mul, scale);
                            scale = fpu.fstp_bits();
                            fpu.fld_bits(sum);
                            fpu.fadd(m, child + 0xf0);
                            sum = fpu.fstp_bits();
                        }
                    }
                }
            }
            record += 0x68;
            i += 1;
        }
        let mut fpu = crate::native_fpu::Fpu::new();
        fpu.fld_bits(sum);
        fpu.arith_m_bits(crate::native_fpu::Op::Add, scale);
        let total = fpu.fstp_bits();
        self.m.w32(body + 0xf0, total);
        if parent_record != 0 {
            let resource = self.m.r32(parent_record + 0xc);
            if resource != 0 {
                let data = self.data(resource, 0xa000_0000, QUALITY_ONE)?;
                if data != 0 {
                    let secondary = self.m.r32(data + 0x44);
                    if secondary != 0 {
                        let template = self.data(secondary, 0x2000_0000, QUALITY_ONE)?;
                        if template != 0 {
                            let parent_k = self.m.r32(template + 0xc);
                            let parent_body = self.m.r32(data + 0x4c);
                            self.mass(parent_k, parent_body, 1)?;
                        }
                    }
                }
            }
        }
        Ok(())
    }

    /// Original474000: propagate the render-type word (+b0) to attached
    /// children through each slot's mask (+50).
    pub fn render_type(&mut self, k: u32, body: u32) -> Result<(), String> {
        let mut node = first(self.m, k + 0x1154);
        let mut record = self.m.r32(body + 0x78);
        while node != 0 {
            let m = &*self.m;
            if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 2 != 0 && m.r32(record + 0xc) != 0 {
                let data = self.data(m.r32(record + 0xc), 0xa000_0000, 0)?;
                if data != 0 {
                    let m = &mut *self.m;
                    let mask = m.r32(node + 0x50);
                    let child = m.r32(data + 0x4c);
                    let old = m.r32(child + 0xb0);
                    let value = m.r32(body + 0xb0);
                    m.w32(child + 0xb0, value);
                    if mask & 0xffff != 0xffff {
                        m.w32(child + 0xb0, (value & 0xffff_0000) | (mask & 0xffff));
                    }
                    if mask & 0xffff_0000 != 0xffff_0000 {
                        m.w32(
                            child + 0xb0,
                            (m.r32(child + 0xb0) & 0xffff) | (mask & 0xffff_0000),
                        );
                    }
                    let secondary = m.r32(data + 0x44);
                    if m.r32(child + 0xb0) != old && secondary != 0 {
                        let template = self.data(secondary, 0xa000_0000, 0)?;
                        if template != 0 && self.m.r32(template + 0xc) != 0 {
                            let child_k = self.m.r32(template + 0xc);
                            self.render_type(child_k, child)?;
                        }
                    }
                }
            }
            node = next(self.m, node);
            record += 0x68;
        }
        Ok(())
    }
}

impl Install<'_> {
    /// The two-step template access shared by many slot walks: payload of a
    /// slot record's resource, its body (+4c) and its template K (+44 → +c).
    fn slot_target(
        &mut self,
        record: u32,
        minimum: u32,
        flags: u32,
    ) -> Result<Option<(u32, u32)>, String> {
        let resource = self.m.r32(record + 0xc);
        if resource == 0 {
            return Ok(None);
        }
        let data = self.data(resource, 0xa000_0000, minimum)?;
        if data == 0 {
            return Ok(None);
        }
        let secondary = self.m.r32(data + 0x44);
        let child = self.m.r32(data + 0x4c);
        if secondary == 0 {
            return Ok(Some((child, 0)));
        }
        let template = self.data(secondary, flags, minimum)?;
        if template == 0 {
            return Ok(Some((child, 0)));
        }
        Ok(Some((child, self.m.r32(template + 0xc))))
    }

    /// Original470e20: re-home attached children under this part's
    /// resource parent and re-place them (473230), recursively.
    pub fn propagate_owner(&mut self, k: u32, body: u32) -> Result<(), String> {
        self.frame(|this| {
            let parent = local(this.m, 16);
            crate::native_render_graph::parent(this.m, body, parent);
            let mut record = this.m.r32(body + 0x78);
            let mut i = 0;
            while i < this.m.r32(k + 0x1150) as i32 {
                let m = &*this.m;
                if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 2 != 0 {
                    if let Some((child, child_k)) =
                        this.slot_target(record, QUALITY_ONE, 0xa000_0000)?
                    {
                        if child_k != 0 {
                            this.replace_children(child_k, child)?;
                            let m = &mut *this.m;
                            bind(m, child + 0x60, m.r32(body + 0x6c));
                            crate::native_render_graph::set_parent(this, record, parent)?;
                            let m = &mut *this.m;
                            m.w32(child + 0x5c, m.r32(body + 0x5c));
                            this.propagate_owner(child_k, child)?;
                            let m = &mut *this.m;
                            if m.r32(child + 0x6c) != 0 {
                                unlink(m, child + 0x60);
                            } else {
                                m.w32(child + 0x68, 0);
                            }
                        }
                    }
                }
                record += 0x68;
                i += 1;
            }
            if this.m.r32(parent + 12) != 0 {
                unlink(this.m, parent);
            }
            Ok(())
        })
    }

    /// Original473230: re-place a part relative to what it is installed on.
    pub fn replace_children(&mut self, k: u32, body: u32) -> Result<(), String> {
        self.frame(|this| {
            let mut node = if empty(this.m, k + 0x1154) {
                0
            } else {
                this.m.r32(k + 0x115c)
            };
            let mut record = this.m.r32(body + 0x78);
            let position = local(this.m, 0xc);
            let rotation = local(this.m, 0x30);
            while node != 0 {
                let m = &*this.m;
                if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 1 != 0 {
                    let target_slot = m.r32(record + 0x10);
                    let own = m.r32(node + 0x48);
                    this.attach_frame(k, body, own, record, target_slot, position, rotation)?;
                    let physics = crate::native_render_graph::instance(this, body + 0x1c)?;
                    if physics != 0 {
                        let entity = this.m.r32(physics + 0x84);
                        let parent = this.m.r32(entity + 0x20);
                        crate::native_world::detach(this.m, entity)?;
                        crate::native_render_graph::set_transform(
                            this,
                            body + 0x2c,
                            position,
                            rotation,
                        )?;
                        crate::native_world::attach(this.m, entity, parent)?;
                        crate::native_entity::compound(this.m, parent)?;
                        crate::native_render_graph::relative_tree(this.m, entity);
                        if this.m.r32(k + 0x11b8) != 0 {
                            return Err(
                                "NativeRenderBoundary: part bone attachment 48bc40/48c0f0".into()
                            );
                        }
                    }
                }
                node = next(this.m, node);
                record += 0x68;
            }
            Ok(())
        })
    }

    /// Original471de0: slot flags of the root mount reached through the
    /// installed-on records.
    pub fn render_flags(&mut self, k: u32, body: u32, slot: u32) -> Result<u32, String> {
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &*self.m;
            if m.r32(record + 0x14) & 1 != 0 && m.r32(record + 8) != 0 && m.r32(record + 0xc) != 0 {
                let data = self.data(self.m.r32(record + 0xc), 0xa000_0000, QUALITY_ONE)?;
                if data != 0 {
                    let secondary = self.m.r32(data + 0x44);
                    let parent = self.m.r32(data + 0x4c);
                    if secondary == 0 {
                        return Ok(0);
                    }
                    let template = self.data(secondary, 0xa000_0000, QUALITY_ONE)?;
                    if template == 0 {
                        return Ok(0);
                    }
                    let parent_k = self.m.r32(template + 0xc);
                    let target = self.m.r32(record + 0x10);
                    return self.render_flags(parent_k, parent, target);
                }
            }
            record += 0x68;
            i += 1;
        }
        let node = self.slot_node(k, slot);
        Ok(if node == 0 {
            0
        } else {
            self.m.r32(node + 0x4c) & 0xffff
        })
    }

    /// Original471f10: slot record with `slot` on this part or, failing
    /// that, on the part it is installed on.
    pub fn linked_record(&mut self, k: u32, body: u32, slot: u32) -> Result<u32, String> {
        if slot == 0 {
            return Ok(0);
        }
        let mut node = {
            let head = self.m.r32(k + 0x115c);
            if self.m.r32(head + 4) != 0 {
                head
            } else {
                0
            }
        };
        let count = self.m.r32(k + 0x1150) as i32;
        let mut record = self.m.r32(body + 0x78);
        for _ in 0..count.max(0) {
            if self.m.r32(node + 0x48) == slot {
                return Ok(record);
            }
            node = next(self.m, node);
            record += 0x68;
        }
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &*self.m;
            if m.r32(record + 0x14) & 1 != 0 && m.r32(record + 8) != 0 && m.r32(record + 0xc) != 0 {
                let data = self.data(self.m.r32(record + 0xc), 0xa000_0000, QUALITY_ONE)?;
                if data != 0 {
                    let secondary = self.m.r32(data + 0x44);
                    let parent = self.m.r32(data + 0x4c);
                    if secondary == 0 {
                        return Ok(0);
                    }
                    let template = self.data(secondary, 0xa000_0000, QUALITY_ONE)?;
                    if template == 0 {
                        return Ok(0);
                    }
                    let parent_k = self.m.r32(template + 0xc);
                    return self.linked_record(parent_k, parent, slot);
                }
            }
            record += 0x68;
            i += 1;
        }
        Ok(0)
    }

    fn command_text(&mut self, link: u32, text: &str) -> Result<(), String> {
        self.frame(|this| {
            let buffer = local_text(this.m, text);
            this.host.command(this.m, link, 0, 0x10, buffer, 0)
        })
    }

    /// Original470ff0: announce this part (and, on a car owner, its lights,
    /// wing, rollbars, mount slots and linked render parts) to the owner
    /// referenced at +c0, then recurse into attached children.
    pub fn add_to_owner(&mut self, k: u32, body: u32, nested: u32) -> Result<(), String> {
        let owner = body + 0xc0;
        if self.m.r32(body + 0xc8) != self.m.r32(body + 8) {
            self.command_text(owner, &format!("add_part {}", body as i32))?;
            if nested == 0 {
                let group = self.host.query(self.m, owner, 0x3f, 0)?;
                self.m.w32(body + 0xac, group);
            }
            let mut i = 0;
            while (i as i32) < self.m.r32(body + 0x54) as i32 {
                let render = self.m.r32(body + 0x4c) + i * 0x10;
                let resource = self.m.r32(render + 0xc);
                if resource != 0 {
                    let data = self.data(resource, 0xa000_0001, QUALITY_ONE)?;
                    if data != 0 {
                        let group = self.m.r32(body + 0xac);
                        self.m.w32(data + 0xec, group);
                        self.m.w32(data + 0xf0, group);
                    }
                }
                i += 1;
            }
        }
        if self.host.query(self.m, owner, 7, 0)? == 5 {
            let mut i = 0;
            while (i as i32) < self.m.r32(body + 0x54) as i32 {
                let descriptor = k + 0x128 + self.m.r32(self.m.r32(body + 0x50) + i * 4) * 0x74;
                let render = self.m.r32(body + 0x4c) + i * 0x10;
                let flags = self.m.r32(descriptor + 0x58);
                if flags & 0x13f != 0 {
                    render_flags_clear(self.m, render, 0x40_0000);
                    let flags = self.m.r32(descriptor + 0x58);
                    self.command_text(
                        owner,
                        &format!("add_light {} {}", render as i32, flags as i32),
                    )?;
                }
                i += 1;
            }
            let wing = k + 0x11e8;
            if self.m.r32(wing) != 0 {
                let instance = crate::native_render_graph::instance(self, body + 0x2c)?;
                if instance != 0 {
                    let entity = self.m.r32(instance + 0x84);
                    self.command_text(
                        owner,
                        &format!("add_wing {} {}", wing as i32, entity as i32),
                    )?;
                }
            }
            self.rollbars(k, owner, "add_rollbar")?;
            let mut node = first(self.m, k + 0x1154);
            let mut record = self.m.r32(body + 0x78);
            let mut i = 0;
            while i < self.m.r32(k + 0x1150) as i32 {
                if self.m.r32(node + 0xc4) != 0 {
                    self.command_text(
                        owner,
                        &format!("add_mslot {} {}", record as i32, node as i32),
                    )?;
                }
                node = next(self.m, node);
                record += 0x68;
                i += 1;
            }
        }
        self.add_linked(k, body, owner)?;
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &*self.m;
            if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 2 != 0 && m.r32(record + 0xc) != 0 {
                if let Some((child, child_k)) =
                    self.slot_target(record, QUALITY_ONE, 0xa000_0000)?
                {
                    if child_k != 0 {
                        let m = &mut *self.m;
                        let target = m.r32(body + 0xcc);
                        let link = child + 0xc0;
                        if m.r32(child + 0xcc) != target {
                            if m.r32(child + 0xcc) != 0 {
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
                        m.w32(child + 0xd0, m.r32(body + 0xd0));
                        m.w32(child + 0xac, m.r32(body + 0xac));
                        self.add_to_owner(child_k, child, 1)?;
                    }
                }
            }
            record += 0x68;
            i += 1;
        }
        if nested == 0 && self.m.r32(body + 0xc8) != self.m.r32(body + 8) {
            self.host.command(self.m, owner, 0, 0x10, 0x6123cc, 0)?;
        }
        Ok(())
    }

    /// 4711e1 / 4717db: "%s %d %d %f %f" for each template rollbar.
    fn rollbars(&mut self, k: u32, owner: u32, verb: &str) -> Result<(), String> {
        let mut j = 0;
        while (j as i32) < self.m.r32(k + 0x1210) as i32 {
            let bar = k + 0x121c + j * 0x10;
            let m = &*self.m;
            let text = slr_vm::format::sprintf(
                format!("{verb} %d %d %f %f").as_bytes(),
                &[
                    slr_vm::format::FormatArg::Word(m.r32(bar - 8)),
                    slr_vm::format::FormatArg::Word(m.r32(bar - 4)),
                    slr_vm::format::FormatArg::Double(m.rf(bar) as f64),
                    slr_vm::format::FormatArg::Double(m.rf(bar + 4) as f64),
                ],
            )
            .map_err(|e| format!("rollbar text: {e:?}"))?;
            let text = String::from_utf8_lossy(&text).into_owned();
            self.command_text(owner, &text)?;
            j += 1;
        }
        Ok(())
    }

    /// 4712d6..471463: linked render parts of the slot this part is
    /// installed through ("add_linked" or the bone00 placement).
    fn add_linked(&mut self, k: u32, body: u32, owner: u32) -> Result<(), String> {
        let mut record = self.m.r32(body + 0x78);
        let mut found = 0;
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            if self.m.r32(record + 0x14) & 1 != 0 {
                found = record;
                break;
            }
            record += 0x68;
            i += 1;
        }
        if found == 0 {
            return Ok(());
        }
        let mut i = 0;
        while (i as i32) < self.m.r32(body + 0x54) as i32 {
            let descriptor = k + 0x128 + self.m.r32(self.m.r32(body + 0x50) + i * 4) * 0x74;
            if self.m.r32(descriptor + 0x58) & 0x4000 != 0 {
                let render = self.m.r32(body + 0x4c) + i * 0x10;
                let group = mount_group(self.m.r32(body + 0xd0));
                let count = self.m.r32(descriptor + 0x6c) as i32;
                let mut link = self.m.r32(descriptor + 0x70);
                for _ in 0..count.max(0) {
                    let wanted = self.m.r32(link + 8);
                    if wanted == 0 || self.m.r32(found + 0x10) == wanted {
                        let offset = if group < 0 { 0 } else { group as u32 };
                        let first_slot = self.m.r32(link).wrapping_add(offset);
                        let second = self.m.r32(link + 4);
                        let a = self.linked_record(k, body, first_slot)?;
                        if second != 0 {
                            let b = self.linked_record(k, body, offset.wrapping_add(second))?;
                            let id = self.m.r32(render + 8);
                            self.command_text(
                                owner,
                                &format!(
                                    "add_linked {} {} {} {}",
                                    id as i32, group, a as i32, b as i32
                                ),
                            )?;
                        } else if a != 0 {
                            crate::native_render_graph::named_bone_transform(
                                self,
                                render,
                                "bone00",
                                a + 0x18,
                                a + 0x2c,
                                a + 0x38,
                            )?;
                        }
                        break;
                    }
                    link += 0xc;
                }
            }
            i += 1;
        }
        Ok(())
    }

    /// Original471620: withdraw this part (and its car-owner extras and
    /// children) from the owner at +c0, then clear that reference.
    pub fn remove_from_owner(&mut self, k: u32, body: u32, nested: u32) -> Result<(), String> {
        let owner = body + 0xc0;
        if self.m.r32(body + 0xc8) != self.m.r32(body + 8) {
            self.command_text(owner, &format!("rem_part {}", body as i32))?;
            let mut i = 0;
            while (i as i32) < self.m.r32(body + 0x54) as i32 {
                let resource = self.m.r32(self.m.r32(body + 0x4c) + i * 0x10 + 0xc);
                if resource != 0 {
                    let data = self.data(resource, 0xa000_0001, QUALITY_ONE)?;
                    if data != 0 {
                        let group = self.m.r32(body + 0xac);
                        self.m.w32(data + 0xec, group);
                        self.m.w32(data + 0xf0, group);
                    }
                }
                i += 1;
            }
        }
        if self.host.query(self.m, owner, 7, 0)? == 5 {
            let mut i = 0;
            while (i as i32) < self.m.r32(body + 0x54) as i32 {
                let descriptor = k + 0x128 + self.m.r32(self.m.r32(body + 0x50) + i * 4) * 0x74;
                let render = self.m.r32(body + 0x4c) + i * 0x10;
                if self.m.r32(descriptor + 0x58) & 0x13f != 0 {
                    render_flags_set(self.m, render, 0x40_0000);
                    let id = self.m.r32(render + 8);
                    let flags = self.m.r32(descriptor + 0x58);
                    self.command_text(owner, &format!("rem_light {} {}", id as i32, flags as i32))?;
                }
                i += 1;
            }
            let wing = k + 0x11e8;
            if self.m.r32(wing) != 0 {
                let instance = crate::native_render_graph::instance(self, body + 0x2c)?;
                if instance != 0 {
                    let entity = self.m.r32(instance + 0x84);
                    self.command_text(
                        owner,
                        &format!("rem_wing {} {}", wing as i32, entity as i32),
                    )?;
                }
            }
            self.rollbars(k, owner, "rem_rollbar")?;
            let mut node = first(self.m, k + 0x1154);
            let mut record = self.m.r32(body + 0x78);
            let mut i = 0;
            while i < self.m.r32(k + 0x1150) as i32 {
                if self.m.r32(node + 0xc4) != 0 {
                    self.command_text(
                        owner,
                        &format!("rem_mslot {} {}", record as i32, node as i32),
                    )?;
                }
                node = next(self.m, node);
                record += 0x68;
                i += 1;
            }
            let mut i = 0;
            while (i as i32) < self.m.r32(body + 0x54) as i32 {
                let descriptor = k + 0x128 + self.m.r32(self.m.r32(body + 0x50) + i * 4) * 0x74;
                let render = self.m.r32(body + 0x4c) + i * 0x10;
                if self.m.r32(descriptor + 0x58) & 0x4000 != 0
                    && self.m.r32(descriptor + 0x6c) as i32 > 0
                {
                    let group = mount_group(self.m.r32(body + 0xd0));
                    if self.m.r32(self.m.r32(descriptor + 0x70) + 4) != 0 {
                        let id = self.m.r32(render + 8);
                        self.command_text(owner, &format!("rem_linked {} {}", id as i32, group))?;
                    } else {
                        return Err(
                            "NativeRenderBoundary: unlinked render bone00 48bc40/48c0f0".into()
                        );
                    }
                }
                i += 1;
            }
        }
        let mut record = self.m.r32(body + 0x78);
        let mut i = 0;
        while i < self.m.r32(k + 0x1150) as i32 {
            let m = &*self.m;
            if m.r32(record + 8) != 0 && m.r32(record + 0x14) & 2 != 0 && m.r32(record + 0xc) != 0 {
                if let Some((child, child_k)) =
                    self.slot_target(record, QUALITY_ONE, 0xa000_0000)?
                {
                    if child_k != 0 {
                        self.remove_from_owner(child_k, child, 1)?;
                    }
                }
            }
            record += 0x68;
            i += 1;
        }
        if nested == 0 && self.m.r32(body + 0xc8) != self.m.r32(body + 8) {
            self.host.command(self.m, owner, 0, 0x10, 0x61244c, 0)?;
        }
        let m = &mut *self.m;
        if m.r32(body + 0xcc) == 0 {
            m.w32(body + 0xc8, 0);
            m.w32(body + 0xd0, 0);
            return Ok(());
        }
        unlink(m, owner);
        m.w32(body + 0xd0, 0);
        Ok(())
    }
}

/// 471366 / 471930: slot group of the owner mount (101..400 → 0..9).
fn mount_group(value: u32) -> i32 {
    let v = value as i32;
    if v <= 0x64 || v > 0x190 {
        -1
    } else {
        (v - 0x65) % 10
    }
}
/// 48d210 / 48d1f0 on a render reference's resource flags.
fn render_flags_clear(m: &mut Space, link: u32, bits: u32) {
    let resource = m.r32(link + 0xc);
    if resource != 0 {
        m.w32(resource + 0x54, m.r32(resource + 0x54) & !bits);
    }
}
fn render_flags_set(m: &mut Space, link: u32, bits: u32) {
    let resource = m.r32(link + 0xc);
    if resource != 0 {
        m.w32(resource + 0x54, m.r32(resource + 0x54) | bits);
    }
}

impl Install<'_> {
    /// Original475ec0 (part template vtable+38).
    pub fn part_command(
        &mut self,
        k: u32,
        body: u32,
        event: u32,
        argument: u32,
    ) -> Result<u32, String> {
        match event {
            0x10000 => Ok(0),
            0x10 => self.frame(|this| this.part_text_command(k, body, argument)),
            1 | 4 => Err(format!(
                "NativeMessageBoundary: part damage event{event:#x} (vtable+44 474db0)"
            )),
            0x80 => Err("NativeMessageBoundary: part timer event 0x80 (48b750/427620)".into()),
            _ => Ok(0),
        }
    }

    fn part_text_command(&mut self, k: u32, body: u32, argument: u32) -> Result<u32, String> {
        let input = {
            let m = &*self.m;
            let mut bytes = vec![];
            let mut p = argument;
            while m.r8(p) != 0 {
                bytes.push(m.r8(p));
                p += 1;
            }
            bytes
        };
        let mut verb = vec![];
        let _ = slr_lang::scan::sscanf(
            &input,
            b"%s",
            &mut [slr_lang::scan::ScanArg::Str(&mut verb)],
        );
        let words = |format: &[u8], count: usize| -> (i32, Vec<u32>) {
            let mut cells = vec![0u32; count];
            let outcome = {
                let mut args: Vec<_> = cells
                    .iter_mut()
                    .map(slr_lang::scan::ScanArg::Word)
                    .collect();
                slr_lang::scan::sscanf(&input, format, &mut args)
            };
            (outcome.map(|o| o.returned).unwrap_or(0), cells)
        };
        match verb.as_slice() {
            b"install" => {
                let mut cells = [0u32; 5];
                let mut floats = [0f32; 3];
                let converted = {
                    let [c0, c1, c2, c3, c4] = &mut cells;
                    let [f0, f1, f2] = &mut floats;
                    slr_lang::scan::sscanf(
                        &input,
                        b"%*s %d %d %d %d %d %f %f %f",
                        &mut [
                            slr_lang::scan::ScanArg::Word(c0),
                            slr_lang::scan::ScanArg::Word(c1),
                            slr_lang::scan::ScanArg::Word(c2),
                            slr_lang::scan::ScanArg::Word(c3),
                            slr_lang::scan::ScanArg::Word(c4),
                            slr_lang::scan::ScanArg::Float(f0),
                            slr_lang::scan::ScanArg::Float(f1),
                            slr_lang::scan::ScanArg::Float(f2),
                        ],
                    )
                    .map(|o| o.returned)
                    .unwrap_or(0)
                };
                let [_, hint, _, target_id, own] = cells;
                self.bind_id(body + 0x60, hint, 1, 0)?;
                let target = local(self.m, 16);
                self.bind_id(target, target_id, 1, 0)?;
                let slot = local(self.m, 4);
                self.m.w32(slot, own);
                let position = if converted < 8 {
                    0
                } else {
                    let p = local(self.m, 0xc);
                    for (i, value) in floats.into_iter().enumerate() {
                        self.m.wf(p + i as u32 * 4, value);
                    }
                    p
                };
                let chosen = self.choose_slot(k, body, target, slot, position)?;
                if chosen == 0 {
                    if self.m.r32(target + 12) != 0 {
                        unlink(self.m, target);
                    }
                    return Ok(0);
                }
                let target_slot = self.m.r32(slot);
                self.install(k, body, chosen, target, target_slot)?;
                if self.m.r32(target + 12) != 0 {
                    unlink(self.m, target);
                }
                Ok(1)
            }
            b"cancel" => Ok(1),
            b"colorscheme" => {
                let (_, cells) = words(b"%*s %d", 1);
                self.m.w32(body + 0x58, cells[0]);
                crate::native_message::part_variant(self.m, k, body, 0)?;
                Ok(1)
            }
            b"add_part" => {
                let (_, cells) = words(b"%*s %d", 1);
                self.part_add_part(body, cells[0])?;
                Ok(1)
            }
            b"rem_part" => {
                let (_, cells) = words(b"%*s %d", 1);
                self.part_remove_part(body, cells[0])?;
                Ok(1)
            }
            other => Err(format!(
                "NativeMessageBoundary: part475ec0 command {}",
                String::from_utf8_lossy(other)
            )),
        }
    }

    /// Original470b60: register a sub-part on this part and run `addpart`.
    pub fn part_add_part(&mut self, body: u32, part: u32) -> Result<(), String> {
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
        let last = m.r32(body + 0xec);
        m.w32(last + 4, node);
        m.w32(node + 4, body + 0xe4);
        m.w32(node + 8, last);
        m.w32(body + 0xec, node);
        self.part_script(body, part, "addpart")
    }

    /// Original470cc0: drop a sub-part registration and run `rempart`.
    pub fn part_remove_part(&mut self, body: u32, part: u32) -> Result<(), String> {
        let m = &mut *self.m;
        let mut node = first(m, body + 0xd4);
        while node != 0 {
            if m.r32(part + 8) == m.r32(node + 0x14) {
                let previous = m.r32(node + 8);
                let following = m.r32(node + 4);
                if previous != 0 && following != 0 {
                    m.w32(following + 8, previous);
                    m.w32(previous + 4, following);
                }
                m.w32(node + 4, 0);
                m.w32(node + 8, 0);
                crate::native_car_command::destroy_node(m, node)?;
                break;
            }
            node = next(m, node);
        }
        self.part_script(body, part, "rempart")
    }

    /// The GameRef construction and script call shared by 470b60/470cc0.
    fn part_script(&mut self, body: u32, part: u32, method: &str) -> Result<(), String> {
        let resource = self.m.r32(body + 0xc);
        if resource == 0 {
            return Ok(());
        }
        let data = self.data(resource, 0xa000_0000, 0)?;
        if data == 0 || self.m.r32(data + 0x50) == 0 {
            return Ok(());
        }
        let object = self.m.malloc(0x1c);
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
            let target = m.r32(part + 0xc);
            if target != 0 {
                bind(m, link, target);
            }
        }
        let cell = self.host.vm_field(self.m, object, "ptr")?;
        self.m.w32(cell + 8, link);
        self.host
            .script(self.m, data, method, &[0, 4, object, 0x4f])
    }
}

/// 473030..4730a4: rotation rows times a vector with 472e80's operand order.
fn rotate_rows(m: &Space, rotation: u32, v: u32) -> [u32; 3] {
    let mut fpu = crate::native_fpu::Fpu::new();
    std::array::from_fn(|r| {
        let row = rotation + r as u32 * 0x10;
        fpu.fld(m, row + 4);
        fpu.fmul(m, v + 4);
        fpu.fld(m, row);
        fpu.fmul(m, v);
        fpu.faddp(1);
        fpu.fld(m, row + 8);
        fpu.fmul(m, v + 8);
        fpu.faddp(1);
        fpu.fstp_bits()
    })
}

/// Original436880: 3x4 row-major rotation times vector, x87 order retained.
pub fn transform(m: &Space, matrix: u32, vector: u32) -> [u32; 3] {
    let r = |row: u32, col: u32| f(m, matrix + row * 0x10 + col * 4);
    let v = |i: u32| f(m, vector + i * 4);
    [
        st(add(
            add(mul(r(0, 2), v(2)), mul(r(0, 1), v(1))),
            mul(r(0, 0), v(0)),
        )),
        st(add(
            add(mul(r(1, 2), v(2)), mul(r(1, 0), v(0))),
            mul(r(1, 1), v(1)),
        )),
        st(add(
            add(mul(r(2, 2), v(2)), mul(r(2, 0), v(0))),
            mul(r(2, 1), v(1)),
        )),
    ]
}

/// A host for code paths that must not reach any collaborator.
pub struct Detached;
impl Host for Detached {
    fn minimum(&mut self, _: &mut Space, resource: u32, _: u32) -> Result<(), String> {
        Err(format!(
            "detached native call: resource minimum {resource:#x}"
        ))
    }
    fn prepare(&mut self, _: &mut Space, resource: u32, _: u32) -> Result<u32, String> {
        Err(format!(
            "detached native call: resource prepare {resource:#x}"
        ))
    }
    fn payload(&mut self, _: &mut Space, resource: u32) -> Result<u32, String> {
        Err(format!(
            "detached native call: resource payload {resource:#x}"
        ))
    }
    fn resolve(&mut self, _: &mut Space, id: u32, _: u32, _: u32) -> Result<u32, String> {
        Err(format!("detached native call: resolve {id:#x}"))
    }
    fn query(&mut self, _: &mut Space, _: u32, message: u32, _: u32) -> Result<u32, String> {
        Err(format!("detached native call: query{message}"))
    }
    fn script_query(&mut self, _: &mut Space, _: u32, message: u32) -> Result<u32, String> {
        Err(format!("detached native call: script query{message}"))
    }
    fn command(
        &mut self,
        _: &mut Space,
        _: u32,
        _: u32,
        event: u32,
        _: u32,
        _: u32,
    ) -> Result<(), String> {
        Err(format!("detached native call: command{event:#x}"))
    }
    fn script(
        &mut self,
        _: &mut Space,
        _: u32,
        method: &str,
        _: ScriptWords<'_>,
    ) -> Result<(), String> {
        Err(format!("detached native call: script {method}"))
    }
    fn deliver(
        &mut self,
        _: &mut Space,
        _: u32,
        _: u32,
        event: u32,
        _: u32,
        _: u32,
        _: u32,
    ) -> Result<(), String> {
        Err(format!("detached native call: deliver{event:#x}"))
    }
    fn vm_class(&mut self, _: &mut Space, name: &str) -> Result<u32, String> {
        Err(format!("detached native call: class {name}"))
    }
    fn vm_object(&mut self, _: &mut Space, _: u32, _: u32) -> Result<(), String> {
        Err("detached native call: object".into())
    }
    fn vm_field(&mut self, _: &mut Space, _: u32, name: &str) -> Result<u32, String> {
        Err(format!("detached native call: field {name}"))
    }
}

/// Original426470 over raw memory: resource access through the recovered
/// virtual methods and native class handlers; a script query is delegated.
pub struct QueryRouter<'a, 'b> {
    pub m: &'a mut Space,
    pub host: &'b mut dyn Host,
}
impl crate::native_query::Host for QueryRouter<'_, '_> {
    fn read(&mut self, address: u32) -> Result<u32, String> {
        Ok(self.m.r32(address))
    }
    fn minimum(&mut self, resource: u32) -> Result<(), String> {
        crate::native_resource_load::AccessHost::set_minimum(
            &mut crate::native_assets::AccessAdapter,
            self.m,
            resource,
            QUALITY_ONE,
        )
    }
    fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String> {
        crate::native_resource_load::prepare(
            self.m,
            resource,
            crate::native_resource_load::Access {
                flags,
                quality: QUALITY_ONE,
                priority: 0,
                time: 0,
            },
            &mut crate::native_assets::AccessAdapter,
        )
    }
    fn data(&mut self, resource: u32) -> Result<u32, String> {
        crate::native_resource_load::AccessHost::payload(
            &mut crate::native_assets::AccessAdapter,
            self.m,
            resource,
            QUALITY_ONE,
        )
    }
    fn access(&mut self, resource: u32) -> Result<u32, String> {
        crate::native_resource_load::access(
            self.m,
            resource,
            crate::native_resource_load::Access {
                flags: 0x8000_0000,
                quality: QUALITY_ONE,
                priority: 0,
                time: 0,
            },
            &mut crate::native_assets::AccessAdapter,
        )
    }
    fn script(&mut self, envelope: u32, message: u32) -> Result<u32, String> {
        self.host.script_query(self.m, envelope, message)
    }
    fn native(
        &mut self,
        payload: u32,
        body: u32,
        message: u32,
        argument: u32,
    ) -> Result<u32, String> {
        native_query(self.m, self.host, payload, body, message, argument)
    }
}

/// Template vtable+38 dispatch (the native half of 4265c0).
pub fn native_command(
    m: &mut Space,
    host: &mut dyn Host,
    payload: u32,
    body: u32,
    source: u32,
    event: u32,
    argument: u32,
) -> Result<u32, String> {
    let _ = source;
    match m.r32(payload) {
        0x5f0f60 => Install { m, host }.part_command(payload, body, event, argument),
        0x5f0f18 => Install { m, host }.car_command(payload, body, event, argument),
        0x5f11e4 => crate::native_cursor_runtime::native_command(m, host, body, event, argument),
        vtable => Err(format!(
            "NativeMessageBoundary: command for template vtable {vtable:#x}"
        )),
    }
}

/// Original426800 → 4265c0 over raw memory with the given collaborators.
pub fn route_command(
    m: &mut Space,
    host: &mut dyn Host,
    reference: u32,
    source: u32,
    event: u32,
    argument: u32,
    name: u32,
) -> Result<(), String> {
    let saved = crate::native_stack::sp(m);
    let result = slr_boot::native_event_route::dispatch(
        &mut EventRouter { m, host },
        reference,
        source,
        event,
        argument,
        name,
    );
    crate::native_stack::set_sp(m, saved);
    result.map(|_| ())
}

struct EventRouter<'a, 'b> {
    m: &'a mut Space,
    host: &'b mut dyn Host,
}
impl slr_boot::native_event_route::Host for EventRouter<'_, '_> {
    fn read(&mut self, address: u32) -> Result<u32, String> {
        Ok(self.m.r32(address))
    }
    fn write(&mut self, address: u32, value: u32) -> Result<(), String> {
        self.m.w32(address, value);
        Ok(())
    }
    fn scratch_reference(&mut self) -> Result<u32, String> {
        Ok(local(self.m, 16))
    }
    fn release_reference(&mut self, _reference: u32) -> Result<(), String> {
        Ok(())
    }
    fn minimum(&mut self, resource: u32) -> Result<(), String> {
        self.host.minimum(self.m, resource, QUALITY_ONE)
    }
    fn prepare(&mut self, resource: u32, flags: u32) -> Result<u32, String> {
        self.host.prepare(self.m, resource, flags)
    }
    fn data(&mut self, resource: u32) -> Result<u32, String> {
        self.host.payload(self.m, resource)
    }
    fn native(
        &mut self,
        payload: u32,
        body: u32,
        source: u32,
        event: u32,
        argument: u32,
    ) -> Result<u32, String> {
        native_command(self.m, self.host, payload, body, source, event, argument)
    }
    fn script(
        &mut self,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        envelope: u32,
        name: u32,
    ) -> Result<(), String> {
        self.host
            .deliver(self.m, reference, source, event, argument, envelope, name)
    }
    fn destroy_notification(&mut self, node: u32) -> Result<(), String> {
        crate::native_notifications::destroy(
            self.m,
            node,
            1,
            &mut crate::native_notifications::Runtime,
        );
        Ok(())
    }
}

/// Template vtable+3c dispatch (the native half of 426470).
pub fn native_query(
    m: &mut Space,
    host: &mut dyn Host,
    payload: u32,
    body: u32,
    message: u32,
    argument: u32,
) -> Result<u32, String> {
    match m.r32(payload) {
        0x5f0f18 => Install { m, host }.car_query(payload, body, message, argument),
        _ => crate::native_message::query(m, payload, body, true, message, argument),
    }
}

#[cfg(test)]
pub(crate) mod snapshot_tests {
    use super::*;
    use crate::snapshot_harness::{report, Snapshot};
    use serde_json::{json, Value};

    /// Raw-memory collaborators for genuine snapshot state. VM boundaries
    /// are recorded exactly as the oracle replay's stubs record them.
    pub struct RawHost {
        pub events: Vec<Value>,
        cells: u32,
    }
    impl RawHost {
        pub fn new() -> Self {
            Self {
                events: vec![],
                cells: 0,
            }
        }
    }
    impl Host for RawHost {
        fn minimum(&mut self, m: &mut Space, resource: u32, quality: u32) -> Result<(), String> {
            crate::native_resource_load::AccessHost::set_minimum(
                &mut crate::native_assets::AccessAdapter,
                m,
                resource,
                quality,
            )
        }
        fn prepare(&mut self, m: &mut Space, resource: u32, flags: u32) -> Result<u32, String> {
            crate::native_resource_load::prepare(
                m,
                resource,
                crate::native_resource_load::Access {
                    flags,
                    quality: 0,
                    priority: 0,
                    time: 0,
                },
                &mut crate::native_assets::AccessAdapter,
            )
        }
        fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String> {
            crate::native_resource_load::AccessHost::payload(
                &mut crate::native_assets::AccessAdapter,
                m,
                resource,
                QUALITY_ONE,
            )
        }
        fn resolve(
            &mut self,
            m: &mut Space,
            id: u32,
            kind: u32,
            flags: u32,
        ) -> Result<u32, String> {
            crate::native_resource_index::lookup(m, id, kind, flags)
        }
        fn query(
            &mut self,
            m: &mut Space,
            reference: u32,
            message: u32,
            argument: u32,
        ) -> Result<u32, String> {
            crate::native_query::query(
                &mut QueryRouter { m, host: self },
                reference,
                message,
                argument,
            )
        }
        fn script_query(
            &mut self,
            _m: &mut Space,
            envelope: u32,
            message: u32,
        ) -> Result<u32, String> {
            Err(format!(
                "snapshot query reached script envelope {envelope:#x} message{message}"
            ))
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
            route_command(m, self, reference, source, event, argument, name)
        }
        #[allow(clippy::too_many_arguments)]
        fn deliver(
            &mut self,
            _m: &mut Space,
            reference: u32,
            source: u32,
            event: u32,
            argument: u32,
            envelope: u32,
            name: u32,
        ) -> Result<(), String> {
            self.events
                .push(json!({"kind":"event","reference":reference,"source":source,
                "event":event,"argument":argument,"envelope":envelope,"name":name}));
            Ok(())
        }
        fn script(
            &mut self,
            m: &mut Space,
            envelope: u32,
            method: &str,
            words: ScriptWords<'_>,
        ) -> Result<(), String> {
            let cells: serde_json::Map<String, Value> = (0..self.cells)
                .map(|i| {
                    let cell = 0x3000_0800 + 16 * i;
                    (format!("{cell:#x}"), json!(m.r32(cell + 8)))
                })
                .collect();
            self.events
                .push(json!({"kind":"script","instance":envelope,"method":method,
                "args":words,"cells":cells}));
            Ok(())
        }
        fn vm_class(&mut self, m: &mut Space, name: &str) -> Result<u32, String> {
            self.events
                .push(json!({"kind":"vm_class","table":m.r32(0x63641c),"name":name}));
            Ok(0x3000_0100)
        }
        fn vm_object(&mut self, _m: &mut Space, object: u32, class: u32) -> Result<(), String> {
            self.events
                .push(json!({"kind":"vm_object","object":object,"vm_class":class,"flags":0}));
            Ok(())
        }
        fn vm_field(&mut self, _m: &mut Space, object: u32, name: &str) -> Result<u32, String> {
            let cell = 0x3000_0800 + 16 * self.cells;
            self.cells += 1;
            self.events
                .push(json!({"kind":"vm_field","object":object,"name":name,"cell":cell}));
            Ok(cell)
        }
    }

    pub fn snapshot() -> Option<Snapshot> {
        let snapshot = Snapshot::load("install-part-queue", "install");
        if snapshot.is_none() {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/headless_snapshot.py install-part-queue --function 0x426800 --after-native 908 --args 5 and part_install_replay.py --checkpoints");
        }
        snapshot
    }

    /// Run `f` from every traced entry of `function` and compare; `None`
    /// from `f` marks a void original whose EAX is not a result.
    pub fn check(
        function: u32,
        f: impl Fn(&mut Install<'_>, &[u32], u32) -> Result<Option<u32>, String>,
    ) {
        let Some(snapshot) = snapshot() else { return };
        let entries = snapshot.entries(function);
        assert!(!entries.is_empty(), "original{function:06x} was not traced");
        for index in entries {
            let mut entry = snapshot.enter(index);
            let mut host = RawHost::new();
            entry.memory.begin_journal();
            let stack = entry.stack.clone();
            let this = entry.this;
            let result = {
                let mut install = Install {
                    m: &mut entry.memory,
                    host: &mut host,
                };
                f(&mut install, &stack, this)
            };
            let written = entry.memory.end_journal();
            let result =
                result.unwrap_or_else(|e| panic!("original{function:06x} entry {index}: {e}"));
            if let Some(result) = result {
                assert_eq!(
                    result,
                    snapshot.result(&entry),
                    "original{function:06x} entry {index} result"
                );
            }
            let mismatches = snapshot.compare(&entry, &entry.memory, &written);
            assert!(
                mismatches.is_empty(),
                "original{function:06x} entry {index}:\n{}",
                report(&mismatches)
            );
            assert_eq!(
                entry.memory.heap_ptr(),
                snapshot.expected_heap_ptr(&entry),
                "heap"
            );
        }
    }

    /// The whole captured 426800 install graph: every VM boundary event in
    /// order, and every retained memory word.
    #[test]
    fn original426800_part_install_graph_on_genuine_boot_state() {
        let Some(snapshot) = snapshot() else { return };
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/snapshots/install-part-queue.install.json");
        let replay: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let index = snapshot.entries(0x426800)[0];
        let mut entry = snapshot.enter(index);
        let mut host = RawHost::new();
        entry.memory.begin_journal();
        let args = entry.stack.clone();
        route_command(
            &mut entry.memory,
            &mut host,
            args[0],
            args[1],
            args[2],
            args[3],
            args[4],
        )
        .unwrap_or_else(|e| panic!("install graph: {e}"));
        let written = entry.memory.end_journal();
        assert_eq!(json!(host.events), replay["events"], "VM boundary events");
        let mismatches = snapshot.compare_final(&entry.memory, &written, &replay["diff"]);
        assert!(
            mismatches.is_empty(),
            "install graph:\n{}",
            report(&mismatches)
        );
        assert_eq!(
            entry.memory.heap_ptr() as u64,
            replay["heap_ptr"].as_u64().unwrap(),
            "heap"
        );
    }
    #[test]
    fn original474600_install_on_genuine_state() {
        check(0x474600, |install, args, this| {
            install
                .install(this, args[0], args[1], args[2], args[3])
                .map(|()| None)
        });
    }
    #[test]
    fn original472e80_attach_frame_on_genuine_state() {
        check(0x472e80, |install, args, this| {
            install
                .attach_frame(this, args[0], args[1], args[2], args[3], args[4], args[5])
                .map(Some)
        });
    }
    #[test]
    fn original474170_complete_install_on_genuine_state() {
        check(0x474170, |install, args, this| {
            install.complete_install(this, args[0]).map(|()| None)
        });
    }
    #[test]
    fn original46a250_mass_on_genuine_state() {
        check(0x46a250, |install, args, this| {
            install.mass(this, args[0], args[1]).map(|()| None)
        });
    }
    #[test]
    fn original470ff0_add_to_owner_on_genuine_state() {
        check(0x470ff0, |install, args, this| {
            install.add_to_owner(this, args[0], args[1]).map(|()| None)
        });
    }
    #[test]
    fn original470e20_propagate_owner_on_genuine_state() {
        check(0x470e20, |install, args, this| {
            install.propagate_owner(this, args[0]).map(|()| None)
        });
    }
    #[test]
    fn original471de0_render_flags_on_genuine_state() {
        check(0x471de0, |install, args, this| {
            install.render_flags(this, args[0], args[1]).map(Some)
        });
    }
    #[test]
    fn original48af20_set_transform_on_genuine_state() {
        check(0x48af20, |install, args, this| {
            crate::native_render_graph::set_transform(install, this, args[0], args[1]).map(Some)
        });
    }
    #[test]
    fn original48aba0_set_parent_on_genuine_state() {
        check(0x48aba0, |install, args, this| {
            crate::native_render_graph::set_parent(install, this, args[0]).map(Some)
        });
    }
    #[test]
    fn original458c00_car_command_on_genuine_state() {
        check(0x458c00, |install, args, this| {
            install
                .car_command(this, args[0], args[2], args[3])
                .map(Some)
        });
    }
    #[test]
    fn original49a760_detach_on_genuine_state() {
        check(0x49a760, |install, args, _| {
            crate::native_world::detach(install.m, args[0]).map(|()| None)
        });
    }
    #[test]
    fn original49a810_attach_on_genuine_state() {
        check(0x49a810, |install, args, _| {
            crate::native_world::attach(install.m, args[0], args[1]).map(|()| None)
        });
    }
    #[test]
    fn original49a050_compound_on_genuine_state() {
        check(0x49a050, |install, args, _| {
            crate::native_entity::compound(install.m, args[0]).map(|()| None)
        });
    }
    #[test]
    fn original4a7990_activate_on_genuine_state() {
        check(0x4a7990, |install, _, this| {
            crate::native_world::activate(install.m, this).map(|()| None)
        });
    }
    #[test]
    fn original4a7a80_sleep_on_genuine_state() {
        check(0x4a7a80, |install, _, this| {
            crate::native_world::sleep(install.m, this).map(|()| None)
        });
    }
    #[test]
    fn original447b30_car_add_part_on_genuine_state() {
        check(0x447b30, |install, args, this| {
            install.car_add_part(this, args[0], args[1]).map(|()| None)
        });
    }

    #[test]
    fn original473b10_slot_choice_on_genuine_install_state() {
        check(0x473b10, |install, args, this| {
            install
                .choose_slot(this, args[0], args[1], args[2], args[3])
                .map(Some)
        });
    }
    #[test]
    fn original473790_target_slot_on_genuine_install_state() {
        check(0x473790, |install, args, this| {
            install
                .target_slot(this, args[0], args[1], args[2], args[3])
                .map(Some)
        });
    }
    #[test]
    fn original4720a0_free_target_slot_on_genuine_install_state() {
        check(0x4720a0, |install, args, this| {
            install
                .free_target_slot(this, args[0], args[1], args[2], args[3])
                .map(Some)
        });
    }
    #[test]
    fn original470910_slot_conflict_on_genuine_install_state() {
        check(0x470910, |install, args, _| {
            install
                .conflicting(args[0], args[1], args[2], args[3])
                .map(Some)
        });
    }
    #[test]
    fn original471cf0_lock_on_genuine_install_state() {
        check(0x471cf0, |install, args, this| {
            install.locked(this, args[0]).map(Some)
        });
    }
}

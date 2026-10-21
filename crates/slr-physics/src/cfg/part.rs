//! Keywords that build the body / part / slot graph: body records at `K+0x128+i*0x74`
//! (count at `K+0xfa8`, at most 32), the click record at `K+0x1000`, slot nodes appended to
//! the `K+0x1164` head (last node at `K+0x116c`, count at `K+0x1150`, and the `K+0x115c`
//! object's `+4` word gating whether any slot is open) and the scalar fields they touch.

use super::*;

const ONE: u32 = 0x3f80_0000;

/// Base of body record `i`: `K + 0x128 + i*0x74`.
fn brec(c: &Ctx, i: u32) -> u32 {
    c.k.wrapping_add(0x128).wrapping_add(i.wrapping_mul(0x74))
}

/// Base of the current (last) body record. `K+0xfa8` is the count, so the last
/// record is index `nbody - 1`. The exe's own arithmetic for every `+0x58`/`+0x3c`
/// keyword is `lea esi, [K + nbody*0x74 + 0xb4]` (`n*8 - n`, then `n + 7n*4`), which is
/// the same address as `brec(nbody - 1)`: `0x128 + (n-1)*0x74 = 0x74*n + 0xb4`.
pub(crate) fn cur(c: &Ctx) -> u32 {
    brec(c, c.kr(0xfa8).wrapping_sub(1))
}

/// The tail the resource keywords share: `resolve(id)` then `bind(slot, handle, kind, flag)`
/// as the exe's `FUN_00546070` call. The resolve always goes through the resource manager
/// `*(K+0x124)` (see [`Ctx::resolve`] and its audit table): the exe sets `ECX = *(K+0x124)`
/// at every `FUN_00544590` site.
fn bind_res(c: &mut Ctx, slot: u32, id: u32, kind: u32, flag: u32) {
    let h = c.resolve(id);
    c.bind(slot, h, kind, flag);
}

/// `click shape x y z a b c` (shape a resource id, default 0): the click record at `K+0x1000`
/// — the position at `+0x1000..0x1008`, the euler matrix at `+0x100c`, the body at `+0x1040`
/// and the flag `2` at `+0x103c` — built through the physics world either from a click
/// definition (`EXT_BODYDEF`, when the shape parses as a resource id) or as a primitive
/// (`EXT_BODY`). A live body ends with `0x20` set and `1` cleared in its `+0x9c`.
///
/// exe: FUN_0046b950
pub fn click(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%s %f %f %f %f %f %f");
    let shape = s.s(0).map(|v| v.to_vec()).unwrap_or_default();
    let v: Vec<u32> = (1..7).map(|i| s.fb(i).unwrap_or(0)).collect();
    let rec = c.k.wrapping_add(0x1000);
    // The six floats land in locals the exe zeroes before the scan (0x46b98e..0x46b99a), so a
    // missing operand leaves `0` behind.
    c.w(rec, v[0]);
    c.w(rec + 4, v[1]);
    c.w(rec + 8, v[2]);
    c.call(
        EXT_EULER_TO_MATRIX,
        This::Addr(rec + 0xc),
        vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
    );
    // The second scan runs on the `%s` buffer itself (not on the argument text `scan` skips
    // to), and its `%d` destination is another pre-zeroed local: a failed conversion leaves
    // the id at 0 and takes the primitive path.
    let s2 = slr_fmt_cfg::scan::sscanf(&shape, b"%d", slr_fmt_cfg::Precision::F32);
    let id = s2.db(0).unwrap_or(0);
    let body = if id == 0 {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(0),
                Arg::U(7),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::S(String::from_utf8_lossy(&shape).into_owned()),
            ],
        )
    } else {
        // `resolve` on the resource manager, bound into a zeroed 4-word stack temporary that
        // `EXT_BODYDEF` reads: `FUN_00546070` leaves the handle in `tmp[2]`.
        let h = c.resolve(id);
        let (t2, t3) = c.bind_local(h, 9, 0);
        let tmp = [0u32, 0, t2, t3];
        if tmp[2] == 0 {
            // The slot stayed unbound: the exe reports it (`FUN_005383f0` names the id, the
            // message prints the name from `name + 0xc`) and builds from the definition anyway.
            let name = c.call(EXT_RESNAME, This::None, vec![Arg::U(c.kr(0xc))]);
            c.call(
                EXT_LOG,
                This::None,
                vec![
                    Arg::S("Invalid clickdef (%08x) in %08x (%s)".to_string()),
                    Arg::U(id),
                    Arg::U(c.kr(0xc)),
                    Arg::U(name + 0xc),
                ],
            );
        }
        // `FUN_00429010(&tmp, tmp[3]+0x44)`, the unlink-and-clear of the temporary at
        // 0x46bae3..0x46baf0, is not modelled: it takes the temporary's stack address (which
        // the port keeps as the four values below, as the replay requires) and is unreachable
        // anyway, since `FUN_00546070` only ever writes `tmp[2]`.
        c.call(
            EXT_BODYDEF,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(0),
                Arg::U(tmp[0]),
                Arg::U(tmp[1]),
                Arg::U(tmp[2]),
                Arg::U(tmp[3]),
            ],
        )
    };
    attach_body(c.m, rec + 0x40, body);
    let o = c.r(rec + 0x40);
    if o != 0 {
        c.w(rec + 0x3c, 2);
        let f = c.r(o + 0x9c) | 0x20;
        c.w(o + 0x9c, f);
        let f = c.r(o + 0x9c) & !1;
        c.w(o + 0x9c, f);
    }
}

/// `body x y z a b c mass shape d e f` (shape `ball`, `sphere`, `box`, `rodx`/`rody`/`rodz`
/// or a click-definition resource): appends a body record (at most 32) and creates its body
/// through the physics world. A live body stores `1` at `rec+0x3c`, its position, the euler
/// matrix at `rec+0x4..0x28` (`EXT_EULER_TO_MATRIX`), clears `1` in the body `+0x9c` and sets
/// `4` when the body mass word is zero; the body index lands at `K+0xffc`.
///
/// exe: FUN_0046bb80
#[allow(clippy::too_many_lines)]
pub fn body(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let n = c.nbody();
    if n > 0x1f {
        return;
    }
    if (c.kr(0xff0) as i32) < 0 {
        c.kw(0xff0, n as u32);
    }
    c.kw(0xfa8, (n as u32).wrapping_add(1));
    let r = brec(c, n as u32);
    let s = scan(line, "%f %f %f %f %f %f %f %s %f %f %f");
    let v: Vec<u32> = (0..7).map(|i| s.fb(i).unwrap_or(0)).collect();
    let shape = s.s(7).map(|x| x.to_vec()).unwrap_or_default();
    let (d, e, f) = (
        s.fb(8).unwrap_or(0),
        s.fb(9).unwrap_or(0),
        s.fb(10).unwrap_or(0),
    );
    let s2 = scan(&shape, "%d");
    let body = if let Some(id) = s2.db(0) {
        let h = c.resolve(id);
        let _ = c.bind_local(h, 9, 0);
        c.call(
            EXT_BODYDEF,
            This::Addr(PHYSICS_WORLD),
            vec![Arg::U(v[6]), Arg::U(0)],
        )
    } else if stricmp_eq(&shape, b"ball") || stricmp_eq(&shape, b"sphere") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(1),
                Arg::U(d),
                Arg::U(0),
                Arg::U(0),
                Arg::U(0),
            ],
        )
    } else if stricmp_eq(&shape, b"box") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(5),
                Arg::U(d),
                Arg::U(e),
                Arg::U(f),
                Arg::U(0),
            ],
        )
    } else if stricmp_eq(&shape, b"rodx") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(2),
                Arg::U(d),
                Arg::U(e),
                Arg::U(0),
                Arg::U(0),
            ],
        )
    } else if stricmp_eq(&shape, b"rody") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(3),
                Arg::U(d),
                Arg::U(e),
                Arg::U(0),
                Arg::U(0),
            ],
        )
    } else if stricmp_eq(&shape, b"rodz") {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(4),
                Arg::U(d),
                Arg::U(e),
                Arg::U(0),
                Arg::U(0),
            ],
        )
    } else {
        c.call(
            EXT_BODY,
            This::Addr(PHYSICS_WORLD),
            vec![
                Arg::U(v[6]),
                Arg::U(6),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::U(ONE),
                Arg::S(String::from_utf8_lossy(&shape).into_owned()),
            ],
        )
    };
    let slot = r + 0x40;
    attach_body(c.m, slot, body);
    let o = c.r(slot);
    if o != 0 {
        c.w(r + 0x3c, 1);
        c.w(r, v[0]);
        c.w(r + 4, v[1]);
        c.w(r + 8, v[2]);
        c.call(
            EXT_EULER_TO_MATRIX,
            This::Addr(r + 12),
            vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
        );
        let f = c.r(o + 0x9c) & !1;
        c.w(o + 0x9c, f);
        if c.rf(o + 0x14) == 0.0 {
            let f = c.r(o + 0x9c) | 4;
            c.w(o + 0x9c, f);
        }
        c.kw(0xffc, (n as u32).wrapping_add(1).wrapping_sub(1));
    } else {
        c.kw(0xfa8, (n as u32).wrapping_add(1).wrapping_sub(1));
    }
}

/// `bone`: marks the current body record boned (`0x400` in `rec+0x58`) and steps the
/// free-body cursor `K+0xff0` back one.
///
/// exe: FUN_0046c280
pub fn bone(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let f = c.r(r + 0x58) | 0x400;
    c.w(r + 0x58, f);
    c.kw(0xff0, (c.nbody() as u32).wrapping_sub(1));
}

/// `noclick`: marks the current body record click-through (`0x200` in `rec+0x58`).
///
/// exe: FUN_0046c2d0
pub fn noclick(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let f = c.r(r + 0x58) | 0x200;
    c.w(r + 0x58, f);
}

/// `nocollision`: marks the current body record non-colliding (`0x100` in `rec+0x58`) and
/// its body unknown-to-collide (`8` in body `+0x9c`).
///
/// exe: FUN_0046c310
pub fn nocollision(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let f = c.r(r + 0x58) | 0x100;
    c.w(r + 0x58, f);
    let o = c.r(r + 0x40);
    let f = c.r(o + 0x9c) | 8;
    c.w(o + 0x9c, f);
}

/// `buoy`: marks the current body buoyant (`0x800` in body `+0x9c`).
///
/// exe: FUN_0046c360
pub fn buoy(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let o = c.r(cur(c) + 0x40);
    let f = c.r(o + 0x9c) | 0x800;
    c.w(o + 0x9c, f);
}

/// `deformable id` (default 0): appends a deformable-body record (body count and the
/// `K+0xff8` cursor each below 32) and binds its resource slot (`0xd` for `light`, else
/// `0xe` with flag 1). The `K+0x1108` owner-reference chain then unlinks the previously
/// linked owner and links this record's `+0x54` ref in its place, and the position triple
/// and euler matrix close the record.
///
/// exe: FUN_0046bfa0
pub fn deformable(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let n = c.nbody();
    if !(n < 0x20 && (c.kr(0xff8) as i32) < 0x20) {
        return;
    }
    let r = brec(c, n as u32);
    c.kw(0xfa8, (n as u32).wrapping_add(1));
    c.w(r + 0x3c, 3);
    c.kw(0xff8, c.kr(0xff8).wrapping_add(1));
    let s = scan(line, "%d %f %f %f %f %f %f");
    let id = s.db(0).unwrap_or(0);
    let v: Vec<u32> = (1..7).map(|i| s.fb(i).unwrap_or(0)).collect();
    if stricmp_eq(line, b"light") {
        bind_res(c, r + 0x48, id, 0xd, 0);
    } else {
        bind_res(c, r + 0x48, id, 0xe, 1);
        if c.r(r + 0x50) == 0 {
            bind_res(c, r + 0x48, id, 0xd, 0);
        }
    }
    ref_chain(c, r);
    vec3_set(c.m, r, v[0], v[1], v[2]);
    c.call(
        EXT_EULER_TO_MATRIX,
        This::Addr(r + 12),
        vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
    );
}

/// The owner-reference list `FUN_0046bfa0` keeps at `K+0x1108` (disassembly `0x46c0dc`..`0x46c16b`):
/// `[K+0x1108]` is the first link, `[K+0x110c]` the last, `[K+0x1114]` the linked owner ref and
/// `[K+0x1110]` its `+0x50` word. With no owner registered, the previous ref (if any and not this
/// record's) is unlinked, then the record's own `+0x54` ref is linked in with its `+0x48` list
/// link. A pending owner (`K+0x1110 != 0`) skips the whole dance.
fn ref_chain(c: &mut Ctx, rec: u32) {
    if c.kr(0x1110) != 0 {
        return;
    }
    let prev = c.kr(0x1114);
    let this_ref = c.r(rec + 0x54);
    if prev == this_ref {
        return;
    }
    if prev != 0 {
        if c.kr(0x1108) == 0 {
            // No first node: the previous owner's link word takes the list's last node.
            let last = c.kr(0x110c);
            c.w(prev + 0x48, last);
        } else {
            let first = c.kr(0x1108);
            let last = c.kr(0x110c);
            c.w(first + 4, last);
        }
        let last = c.kr(0x110c);
        if last != 0 {
            let first = c.kr(0x1108);
            c.w(last, first);
        }
        c.kw(0x1114, 0);
        c.kw(0x1110, 0);
        c.kw(0x1108, 0);
        c.kw(0x110c, 0);
    }
    if this_ref == 0 {
        c.kw(0x1114, 0);
        c.kw(0x1110, 0);
        c.kw(0x1108, 0);
        c.kw(0x110c, 0);
        return;
    }
    let link = c.r(this_ref + 0x48);
    if link != 0 {
        c.w(link, c.k + 0x1108);
    }
    c.kw(0x1108, 0);
    c.kw(0x110c, link);
    c.w(this_ref + 0x48, c.k + 0x1108);
    c.kw(0x1114, this_ref);
    let owner = c.r(this_ref + 0x50);
    c.kw(0x1110, owner);
}

/// `shadow id x y z a b c`: shadow record at `K+0x1074..0x107c` (kind 3 at `0x10b0`,
/// clear at `0x10cc`), euler matrix at `K+0x107c` and its resource slot bound (`0xe`).
///
/// exe: FUN_0046c1a0
pub fn shadow(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d %f %f %f %f %f %f");
    let id = s.db(0).unwrap_or(0);
    let v: Vec<u32> = (1..7).map(|i| s.fb(i).unwrap_or(0)).collect();
    // `esi = K+0x1074`: vec3 at `esi`, matrix at `esi+0xc`, `+0x3c`/`+0x58` flags and the
    // bound resource at `esi+0x48`.
    vec3_zero(c.m, c.k + 0x1074);
    c.w(c.k + 0x1074, v[0]);
    c.w(c.k + 0x1078, v[1]);
    c.w(c.k + 0x107c, v[2]);
    c.w(c.k + 0x10b0, 3);
    c.w(c.k + 0x10cc, 0);
    c.call(
        EXT_EULER_TO_MATRIX,
        This::Addr(c.k + 0x1080),
        vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
    );
    bind_res(c, c.k + 0x10bc, id, 0xe, 0);
}

/// `flags a`: ORs `a & 0x1ff` into the current body record's `+0x58`.
///
/// Record base: the exe computes it from the *incremented* body count exactly as
/// `lea esi, [ecx + eax*4 + 0xb4]` with `eax = K+0xfa8*0x74`, i.e. the `+0x58` word lives at
/// `K + 0x74*nbody + 0x10c` (== `cur(c) + 0x58` == `brec(nbody - 1) + 0x58`). `nbody > 0` is
/// the exe's `jle` test on `K+0xfa8`, so `nbody - 1` is never negative.
///
/// exe: FUN_0046d0c0
pub fn flags(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let s = scan(line, "%d");
    let a = s.db(0).unwrap_or(0);
    let f = c.r(r + 0x58) | (a & 0x1ff);
    c.w(r + 0x58, f);
}

/// `colors n c1..cn` (`%x` words): count at `rec+0x5c`, a fresh `n`-word table at
/// `rec+0x60` (the old one freed), each entry walked with `%#%d` so parsing resumes
/// after every number.
///
/// exe: FUN_0046d330
pub fn colors(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let mut text = rest(line);
    let s = slr_fmt_cfg::scan::sscanf(text, b"%#%d", slr_fmt_cfg::Precision::F32);
    let n = s.db(0).unwrap_or(0);
    text = &text[(s.ret.max(0) as usize).min(text.len())..];
    let t = c.m.malloc(n.wrapping_mul(4));
    c.w(r + 0x60, t);
    c.w(r + 0x5c, n);
    for i in 0..(n as i32).max(0) as u32 {
        let s = slr_fmt_cfg::scan::sscanf(text, b"%#%d", slr_fmt_cfg::Precision::F32);
        c.put(t.wrapping_add(i.wrapping_mul(4)), s.db(0));
        text = &text[(s.ret.max(0) as usize).min(text.len())..];
    }
}

/// `category a`: `K+0x114c`.
///
/// exe: FUN_0046ced0
pub fn category(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    c.put(c.k + 0x114c, s.db(0));
}

/// `slot x y z a b c id`: appends a 0xc8-byte slot node to the list whose embedded head
/// sentinel is `K+0x1164` (`K+0x116c` is its `prev`, i.e. the last node; count at
/// `K+0x1150`), position at `+0..0x8`, euler matrix at `+0xc..0x30`
/// (`EXT_EULER_TO_MATRIX`), `-1` at `+0x50`, id at `+0x48` when the line has one.
///
/// exe: FUN_0046c3a0
pub fn slot(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let node = c.m.alloc(0xc8);
    list_node_ctor(c.m, node);
    list_head_ctor(c.m, node + 0x58);
    c.w(node + 0x58, 0x005f_128c);
    list_head_ctor(c.m, node + 0x74);
    c.w(node + 0x74, 0x005f_1288);
    list_head_ctor(c.m, node + 0x90);
    c.w(node + 0x90, 0x005f_1288);
    c.w(node, 0x005f_1284);
    c.w(node + 0xac, 0);
    c.w(node + 0xb0, ONE);
    c.w(node + 0xc4, 0);
    // Append at the tail: `t` is the current last node, `K+0x1164` the head sentinel the
    // new node's `next` points back at.
    let t = c.kr(0x116c);
    c.w(t + 4, node);
    c.w(node + 8, t);
    c.w(node + 4, c.k + 0x1164);
    c.kw(0x116c, node);
    c.kw(0x1150, c.kr(0x1150).wrapping_add(1));
    c.w(node + 0x50, 0xffff_ffff);
    c.w(node + 0x4c, 0);
    c.w(node + 0x54, 0);
    let s = scan(line, "%f %f %f %f %f %f %d");
    let v: Vec<u32> = (0..6).map(|i| s.fb(i).unwrap_or(0)).collect();
    c.w(node + 0xc, v[0]);
    c.w(node + 0x10, v[1]);
    c.w(node + 0x14, v[2]);
    c.call(
        EXT_EULER_TO_MATRIX,
        This::Addr(node + 0x18),
        vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
    );
    // The exe passes `&node[0x48]` as the `%d` destination, so a line without one leaves
    // the fresh allocation untouched.
    c.put(node + 0x48, s.db(6));
}

/// `bolt x y z a b c id`: appends a 0x58-byte bolt node to the current slot's bolt
/// chain (`slot+0x68`, count at `slot+0x54`), same position/euler layout as [`slot`];
/// when `id` is nonzero its resource slot is bound (`0xe`), otherwise the bolt links
/// into the `K+0x1134` owner chain.
///
/// exe: FUN_0046c4e0
pub fn bolt(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let node = c.m.alloc(0x58);
    list_node_ctor(c.m, node);
    c.w(node + 0x48, 0);
    c.w(node + 0x4c, 0);
    c.w(node + 0x50, 0);
    c.w(node + 0x54, 0);
    c.w(node, 0x005f_1294);
    let tail = c.r(slot + 0x70);
    c.w(tail + 4, node);
    c.w(node + 8, tail);
    c.w(node + 4, slot + 0x68);
    c.w(slot + 0x70, node);
    c.w(slot + 0x54, c.r(slot + 0x54).wrapping_add(1));
    let s = scan(line, "%f %f %f %f %f %f %d");
    let v: Vec<u32> = (0..6).map(|i| s.fb(i).unwrap_or(0)).collect();
    c.w(node + 0xc, v[0]);
    c.w(node + 0x10, v[1]);
    c.w(node + 0x14, v[2]);
    c.call(
        EXT_EULER_TO_MATRIX,
        This::Addr(node + 0x18),
        vec![Arg::U(v[3]), Arg::U(v[4]), Arg::U(v[5])],
    );
    let id = s.db(6).unwrap_or(0);
    if id != 0 {
        bind_res(c, node + 0x48, id, 0xe, 0);
    }
}

/// The body [`attach`] and [`compatible`] share: `malloc(0x14)`, the 0x14-byte node the exe
/// builds inline, the append at `chain`, then the two `%d`s.
///
/// The exe does **not** call the node ctor `FUN_00429130` here: it clears `+0x8`, `+0x4`,
/// `+0xc`, `+0x10` in that order and writes the vtable `0x5f1298` **last**
/// (0x46c6ed..0x46c6ff / 0x46c7bd..0x46c7cf), so the ctor's `0x5f09bc` never lands on the
/// node. `chain` is `slot+0x84` ([`attach`]) or `slot+0xa0` ([`compatible`]); `chain+8` is
/// the tail word the exe reads back (`slot+0x8c` / `slot+0xa8`). Both `%d`s go into zeroed
/// stack locals first, so a missing operand leaves its field `0` and still calls
/// `resolve(0)`; the resolve is `FUN_00544590` with `ECX = *(K+0x124)`, the resource manager.
///
/// This is the body of `FUN_0046c6a0` ([`attach`], `chain = slot+0x84`, `%d %d` at
/// `0x611f34`) and of `FUN_0046c770` ([`compatible`], `chain = slot+0xa0`, fmt `0x611f3c`);
/// the ledger annotations for both live on those two handlers.
fn attach_node(c: &mut Ctx, line: &[u8], chain: u32) {
    let node = c.m.alloc(0x14);
    c.w(node + 8, 0);
    c.w(node + 4, 0);
    c.w(node + 0xc, 0);
    c.w(node + 0x10, 0);
    c.w(node, 0x005f_1298);
    let tail = c.r(chain + 8);
    c.w(tail + 4, node);
    c.w(node + 8, tail);
    c.w(node + 4, chain);
    c.w(chain + 8, node);
    let s = scan(line, "%d %d");
    let h = c.resolve(s.db(0).unwrap_or(0));
    c.w(node + 0xc, h);
    c.w(node + 0x10, s.db(1).unwrap_or(0));
}

/// `attach id n`: appends a 0x14-byte attach node to the current slot's attach chain
/// (`slot+0x84`), `resolve(id)` at `+0xc` and `n` at `+0x10`.
///
/// exe: FUN_0046c6a0
pub fn attach(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    attach_node(c, line, slot + 0x84);
}

/// `compatible id n`: the same 0x14-byte node as [`attach`] on the current slot's
/// compatible chain (`slot+0xa0`).
///
/// exe: FUN_0046c770
pub fn compatible(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    attach_node(c, line, slot + 0xa0);
}

/// `slotdmgmode a` (default -1): current slot `+0x50`.
///
/// exe: FUN_0046c840
pub fn slotdmgmode(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let s = scan(line, "%d");
    c.put(slot + 0x50, s.db(0).or(Some(0xffff_ffff)));
}

/// `slotdeform a`: current slot `+0xb0`.
///
/// exe: FUN_0046c8a0
pub fn slotdeform(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let s = scan(line, "%f");
    c.put(slot + 0xb0, s.fb(0));
}

/// `slottype a` / `slotflags a` (the exe's table maps both to this handler): the current
/// slot's `+0x4c`, and nothing else.
///
/// There is no early return: the `%d` is scanned in every case (into a zeroed local), and the
/// slot word is only loaded when the `K+0x115c` list has a first node — `if (*(K+0x115c)+4)
/// slot = *(K+0x116c)`. With no such node the exe stores through the null `ESI` it started
/// from; no cfg state reaches that, so the port simply has no destination.
///
/// exe: FUN_0046cb30
pub fn slottype(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    let a = s.db(0).unwrap_or(0);
    if c.r(c.kr(0x115c) + 4) != 0 {
        let slot = c.kr(0x116c);
        c.w(slot + 0x4c, a);
    }
}

/// `slotmove id c0 c1 c2 c3` (defaults `0x40490fdb, 1.0, 0.0, 0.0`): current slot
/// `+0xc0` (float bits) / `+0xc4` (id) and the normalized direction at `+0xb4..0xbc`;
/// sets `0x400000` in the slot flags.
///
/// exe: FUN_0046ccd0
pub fn slotmove(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let s = scan(line, "%d %f %f %f %f");
    let id = s.db(0).unwrap_or(0);
    let b = s.fb(1).unwrap_or(0x4049_0fdb);
    let (x, y, z) = (
        s.f(2).unwrap_or(1.0),
        s.f(3).unwrap_or(0.0),
        s.f(4).unwrap_or(0.0),
    );
    c.w(slot + 0xc0, b);
    c.w(slot + 0xc4, id);
    let len = (x * x + z * z + y * y).sqrt();
    if len != 0.0 {
        let inv = 1.0 / len;
        c.wf(slot + 0xb4, inv * x);
        c.wf(slot + 0xb8, inv * y);
        c.wf(slot + 0xbc, z * inv);
    }
    let f = c.r(slot + 0x4c) | 0x400000;
    c.w(slot + 0x4c, f);
}

/// `deform a`: `K+0x1148`.
///
/// exe: FUN_0046cf10
pub fn deform(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f");
    c.put(c.k + 0x1148, s.fb(0));
}

/// `texture id`: bind the texture slot at `K+0x10e8` (`7`).
///
/// exe: FUN_0046cdf0
pub fn texture(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    let id = s.db(0).unwrap_or(0);
    bind_res(c, c.k + 0x10e8, id, 7, 0);
}

/// `mesh id`: bind the mesh slot at `K+0x10f8` (`5`).
///
/// exe: FUN_0046ce60
pub fn mesh(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    let id = s.db(0).unwrap_or(0);
    bind_res(c, c.k + 0x10f8, id, 5, 0);
}

/// `lod a b`: packs two nibbles into the current body record's `+0x58`
/// (`a` at bits 16..19, `b` at bits 20..23).
///
/// exe: FUN_0046d180
pub fn lod(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let s = scan(line, "%d %d");
    let (a, b) = (s.db(0).unwrap_or(0), s.db(1).unwrap_or(0));
    let mut f = c.r(r + 0x58);
    f = ((a & 0xf) << 0x10) | f & 0xfff0_ffff;
    f = ((b & 0xf) << 0x14) | f & 0xff0f_ffff;
    c.w(r + 0x58, f);
}

/// `lods n d1..dn` (`%#%f` walk like [`colors`]): count at `K+0xfac`, distances at
/// `K+0xfb0..`; a trailing distance below 1.0 is replaced by 1.0, else the count is
/// stepped back one.
///
/// exe: FUN_0046d290
pub fn lods(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let mut text = rest(line);
    let s = slr_fmt_cfg::scan::sscanf(text, b"%#%d", slr_fmt_cfg::Precision::F32);
    let n = s.db(0).unwrap_or(0);
    text = &text[(s.ret.max(0) as usize).min(text.len())..];
    c.kw(0xfac, n);
    for i in 0..(n as i32).max(0) as u32 {
        let s = slr_fmt_cfg::scan::sscanf(text, b"%#%f", slr_fmt_cfg::Precision::F32);
        c.put(
            c.k.wrapping_add(0xfb0).wrapping_add(i.wrapping_mul(4)),
            s.fb(0),
        );
        text = &text[(s.ret.max(0) as usize).min(text.len())..];
    }
    let last = c.kr(0xfac);
    if c.rf(c.k.wrapping_add(0xfac).wrapping_add(last.wrapping_mul(4))) < 1.0 {
        c.kw(0xfb0 + last.wrapping_mul(4), ONE);
    } else {
        c.kw(0xfac, last.wrapping_sub(1));
    }
}

/// `logo id`: bind the logo slot at `K+0x1238` (`7`).
///
/// exe: FUN_0046d400
pub fn logo(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    let id = s.db(0).unwrap_or(0);
    bind_res(c, c.k + 0x1118, id, 7, 0);
}

/// `flap x y z a b c d e f g h i j k l` (15 floats): appends a 0x5c-byte flap node to the
/// current slot (`slot+0xac`). The `%f`s land in the arguments the exe pushes at 0x46c963-0x46c9b3,
/// i.e. the fifteen consecutive words `+0x00..+0x38`; a short argument list ends the conversion
/// chain, so the defaults `+0x2c = 0`, `+0x34 = 0`, `+0x30 = 0x3f99999a` (1.2) and
/// `+0x38 = 0x3f000000` (0.5) survive. `+0x3c..+0x58` is zeroed and the two directions at
/// `+0xc..0x14` / `+0x18..0x20` are then normalised in place (length-normalise block 0x46c9c1).
///
/// exe: FUN_0046c8f0
pub fn flap(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let node = c.m.alloc(0x5c);
    c.w(node + 0x3c, 0);
    c.w(node + 0x40, 0);
    c.w(node + 0x44, 0);
    c.w(node + 0x48, 0);
    c.w(node + 0x4c, 0);
    c.w(node + 0x50, 0);
    c.w(node + 0x54, 0);
    c.w(node + 0x58, 0);
    c.w(slot + 0xac, node);
    c.w(node + 0x2c, 0);
    c.w(node + 0x34, 0);
    c.w(node + 0x30, 0x3f99_999a);
    c.w(node + 0x38, 0x3f00_0000);
    let s = scan(line, "%f %f %f %f %f %f %f %f %f %f %f %f %f %f %f");
    for i in 0..15 {
        c.put(node + 4 * i as u32, s.fb(i));
    }
    let (x, y, z) = (c.rf(node + 0xc), c.rf(node + 0x10), c.rf(node + 0x14));
    let len = (x * x + y * y + z * z).sqrt();
    if len != 0.0 {
        let inv = 1.0 / len;
        c.wf(node + 0xc, x * inv);
        c.wf(node + 0x10, y * inv);
        c.wf(node + 0x14, z * inv);
    }
    let (x, y, z) = (c.rf(node + 0x18), c.rf(node + 0x1c), c.rf(node + 0x20));
    let len = (x * x + y * y + z * z).sqrt();
    if len != 0.0 {
        let inv = 1.0 / len;
        c.wf(node + 0x18, x * inv);
        c.wf(node + 0x1c, y * inv);
        c.wf(node + 0x20, z * inv);
    }
}

/// `flapsfx a b`: binds two flap sound slots (`6`) once the current slot has a flap node.
///
/// exe: FUN_0046ca70
pub fn flapsfx(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let flap = c.r(c.kr(0x116c) + 0xac);
    let s = scan(line, "%d %d");
    let (a, b) = (s.db(0).unwrap_or(0), s.db(1).unwrap_or(0));
    bind_res(c, flap + 0x3c, a, 6, 0);
    bind_res(c, flap + 0x4c, b, 6, 0);
}

/// `linked a b c`: appends one 12-byte `(a, b, c)` entry to the current body record's
/// linked table (`rec+0x70`, count at `rec+0x6c`, grown with realloc-style copy) and sets
/// `0x4000` in `rec+0x58`.
///
/// exe: FUN_0046d8a0
pub fn linked(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    let n = c.r(r + 0x6c);
    let old = c.r(r + 0x70);
    let t = c.m.malloc(n.wrapping_add(1).wrapping_mul(0xc));
    if old != 0 {
        c.m.copy(t, old, n.wrapping_mul(0xc));
    }
    c.w(r + 0x70, t);
    let e = t.wrapping_add(n.wrapping_mul(0xc));
    c.w(r + 0x6c, n.wrapping_add(1));
    c.w(e + 4, 0);
    c.w(e + 8, 0);
    c.w(e, 0);
    let s = scan(line, "%d %d %d");
    c.put(e, s.db(0));
    c.put(e + 4, s.db(1));
    c.put(e + 8, s.db(2));
    let f = c.r(r + 0x58) | 0x4000;
    c.w(r + 0x58, f);
}

/// `stockpart id`: appends a 0x2c-byte stock-part node to the `K+0x124c` chain (the new
/// node's `next` points back at the `K+0x1244` tail sentinel) and binds its resource slot
/// (`8`) on the node's own reference field at `+0xc`.
///
/// exe: FUN_0046d970
pub fn stockpart(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d");
    let id = s.db(0).unwrap_or(0);
    if id == 0 {
        return;
    }
    let node = c.m.alloc(0x2c);
    c.w(node + 8, 0);
    c.w(node + 4, 0);
    c.w(node + 0xc, 0);
    c.w(node + 0x10, 0);
    c.w(node + 0x14, 0);
    c.w(node + 0x18, 0);
    c.w(node, 0x005f_129c);
    // The exe resolves through the resource manager (`ECX = *(K+0x124)`, `FUN_00544590`)
    // and binds the handle into the node's own reference slot (`this = node+0xc`).
    bind_res(c, node + 0xc, id, 8, 0);
    // Append at the tail: `t` is the current last node, `K+0x1244` the sentinel address the
    // new node's `next` points at.
    let t = c.kr(0x124c);
    c.w(t + 4, node);
    c.w(node + 8, t);
    c.w(node + 4, c.k + 0x1244);
    c.kw(0x124c, node);
}

/// `trigger` (`FUN_0046d130`), the sibling of [`flags`] in the part table: when the current
/// body record's `+0x3c` is 1, advances it to 2 and marks its body deformable-twice (`0x20`
/// in body `+0x9c`). No `sscanf` and no `_` token use: the whole body is the guard, the
/// `K+0xfa8 > 0` count test and the `cur(c)+0x3c == 1` fixup.
///
/// The exe binds this address to the `trigger` keyword of the part keyword table
/// (`0x611368`, used by `FUN_0046e570`), directly after `flags` (`0x46d0c0`); it is *not* a
/// `deform2` keyword — no such keyword exists in any exe table.
///
/// exe: FUN_0046d130
pub fn trigger(c: &mut Ctx, _line: &[u8]) {
    if !(c.guard() && c.nbody() > 0) {
        return;
    }
    let r = cur(c);
    if c.r(r + 0x3c) == 1 {
        c.w(r + 0x3c, 2);
        let o = c.r(r + 0x40);
        let f = c.r(o + 0x9c) | 0x20;
        c.w(o + 0x9c, f);
    }
}

/// `slotmove2` tail (`FUN_0046cb90`): like [`slotmove`] with an extra flag word; sets
/// `0x200000` in the slot flags, plus `0x80000` when the flag is nonzero.
///
/// exe: FUN_0046cb90
pub fn slotmove2(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    if !(c.r(c.kr(0x115c) + 4) != 0 && c.kr(0x116c) != 0) {
        return;
    }
    let slot = c.kr(0x116c);
    let s = scan(line, "%d %f %f %f %f %d");
    let id = s.db(0).unwrap_or(0);
    let b = s.fb(1).unwrap_or(ONE);
    let (x, y, z) = (
        s.f(2).unwrap_or(1.0),
        s.f(3).unwrap_or(0.0),
        s.f(4).unwrap_or(0.0),
    );
    let flag = s.db(5).unwrap_or(0);
    c.w(slot + 0xc0, b);
    c.w(slot + 0xc4, id);
    let len = (x * x + z * z + y * y).sqrt();
    if len != 0.0 {
        let inv = 1.0 / len;
        c.wf(slot + 0xb4, inv * x);
        c.wf(slot + 0xb8, inv * y);
        c.wf(slot + 0xbc, z * inv);
    }
    let f = c.r(slot + 0x4c) | 0x200000;
    c.w(slot + 0x4c, f);
    if flag != 0 {
        let f = c.r(slot + 0x4c) | 0x280000;
        c.w(slot + 0x4c, f);
    }
}

/// `texture2` tail (`FUN_0046cfb0`): `K+0x11dc`.
///
/// exe: FUN_0046cfb0
pub fn texture2(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f");
    c.put(c.k + 0x11dc, s.fb(0));
}

/// `material2` tail (`FUN_0046cff0`): `system.rpk:3` lookup plus three binds (`0xe`,
/// `6`, `6`); the float lands at `K+0x11b4`.
///
/// exe: FUN_0046cff0
pub fn material2(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let lookup = c.call(
        EXT_LOOKUP,
        This::None,
        vec![Arg::S("system.rpk".to_string()), Arg::U(3)],
    );
    let s = scan(line, "%d %d %d %f");
    let id = s.db(0).unwrap_or(lookup);
    let a = s.db(1).unwrap_or(0);
    let b = s.db(2).unwrap_or(0);
    c.put(c.k + 0x11b4, s.fb(3));
    // `esi = K+0x1170`: material at `esi+0x20`, the two maps at `esi` and `esi+0x30`.
    bind_res(c, c.k + 0x1190, id, 0xe, 0);
    bind_res(c, c.k + 0x1170, a, 6, 0);
    bind_res(c, c.k + 0x11a0, b, 6, 0);
}

/// `mesh2` tail (`FUN_0046d470`): `K+0x11e8..0x120c` (id, position, normalized direction,
/// signed/unsigned magnitudes); the position is re-based through the free-body cursor's
/// matrix when a body is live.
///
/// exe: FUN_0046d470
pub fn mesh2(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    // `%d %f` x9 (`0x61201c`).
    let s = scan(line, "%d %f %f %f %f %f %f %f %f %f");
    let id = s.db(0).unwrap_or(0);
    let v: Vec<f32> = (1..7).map(|i| s.f(i).unwrap_or(0.0)).collect();
    let (a, b, d) = (
        s.f(7).unwrap_or(0.0),
        s.f(8).unwrap_or(0.0),
        s.fb(9).unwrap_or(ONE),
    );
    c.kw(0x11e8, id);
    // The exe re-bases the position through the enclosing record's transposed matrix
    // (`0x54e4a0` copies `src` transposed into `dst`) and subtracts the record origin.
    // `0x46d531..0x46d553`: a live body record wins; otherwise the click record at
    // `K+0x1000` is used (its `+0x40` body pointer is then zero and no matrix is live).
    let n = c.kr(0xff0) as i32;
    let rec = if n < 0 {
        c.k + 0x1000
    } else {
        c.k + 0x128 + (n as u32).wrapping_mul(0x74)
    };
    // `0x46d559` tests the record's body pointer, not the index sign.
    let edi = if c.r(rec + 0x40) != 0 {
        rec
    } else {
        c.k + 0x1000
    };
    let src = edi + 0xc;
    // `src` holds the 3x3 rotation as three rows of stride 0x10 (`m[r][c] = src + r*0x10 + c*4`).
    let mut m = [0.0f32; 9];
    for r in 0..3 {
        for col in 0..3 {
            m[r * 3 + col] = c.rf(src + (r as u32) * 0x10 + (col as u32) * 4);
        }
    }
    let mut p = [c.rf(edi), c.rf(edi + 4), c.rf(edi + 8)];
    if c.r(edi + 0x40) != 0 && c.rf(edi + 0x14) != 0.0 {
        let e = c.r(edi + 0x78) + 0x40;
        p = [p[0] + c.rf(e), p[1] + c.rf(e + 4), p[2] + c.rf(e + 8)];
    }
    // Two x87 passes, exactly as `0x46d5d2..0x46d696`:
    //   1. `b = v - Mt * (pos + body_offset)`      (`fchs` the origin, then the first
    //      matrix product, then `fadd v`);
    //   2. `q = M * b`, where `M` is the matrix as stored (row `r` at `src + r*0x10`).
    // Every partial product/sum rounds to f32 (control word 0x007f).
    let a3 = [v[0], v[1], v[2]];
    let mut bias = [0.0f32; 3];
    // Column `i` of the stored matrix dotted with `-pos`, top row last.
    for (i, out) in bias.iter_mut().enumerate() {
        let mut acc = 0.0f32;
        for j in (0..3).rev() {
            acc += m[j * 3 + i] * (-p[j]);
        }
        *out = a3[i] + acc;
    }
    let mut q = [0.0f32; 3];
    for (i, out) in q.iter_mut().enumerate() {
        *out = m[i * 3 + 2] * bias[2] + m[i * 3 + 1] * bias[1] + m[i * 3] * bias[0];
    }
    for (i, x) in q.iter().enumerate() {
        c.wf(c.k + 0x11ec + (i as u32) * 4, *x);
    }
    let len = (v[3] * v[3] + v[4] * v[4] + v[5] * v[5]).sqrt();
    let dir = [v[3], v[4], v[5]];
    if len != 0.0 {
        // `0x4369b0` normalises the transformed direction in place before it is stored.
        let mut t = [0.0f32; 3];
        for (i, out) in t.iter_mut().enumerate() {
            let mut acc = 0.0f32;
            for j in (0..3).rev() {
                acc += m[j * 3 + i] * dir[j];
            }
            *out = acc;
        }
        let tl = ((t[0] as f64 * t[0] as f64)
            + (t[1] as f64 * t[1] as f64)
            + (t[2] as f64 * t[2] as f64))
            .sqrt() as f32;
        if tl != 0.0 {
            let inv = 1.0 / tl;
            for (i, x) in t.iter().enumerate() {
                c.wf(c.k + 0x11f8 + (i as u32) * 4, *x * inv);
            }
        }
    }
    c.wf(c.k + 0x1204, -a.abs());
    c.w(c.k + 0x120c, d);
    c.wf(c.k + 0x1208, b.abs());
}

/// `lods2` tail (`FUN_0046d7a0`): appends a 16-byte `(a, b, c, d)` entry at
/// `K+0x1214+n*0x10` (at most two; defaults `100.0, 500.0`).
///
/// exe: FUN_0046d7a0
pub fn lods2(c: &mut Ctx, line: &[u8]) {
    if !(c.guard() && (c.kr(0x1210) as i32) < 2) {
        return;
    }
    let s = scan(line, "%d %d %f %f");
    let (a, b) = (s.db(0).unwrap_or(0), s.db(1).unwrap_or(0));
    let (x, y) = (
        s.fb(2).unwrap_or(0x461c_4000),
        s.fb(3).unwrap_or(0x43fa_0000),
    );
    let e =
        c.k.wrapping_add(0x1214)
            .wrapping_add(c.kr(0x1210).wrapping_mul(0x10));
    c.kw(0x1210, c.kr(0x1210).wrapping_add(1));
    c.w(e, a);
    c.w(e + 4, b);
    c.w(e + 8, x);
    c.w(e + 12, y);
}

/// `skinned a b c`: three floats into `K+0x11bc/0x11c0/0x11c4` and the flag at
/// `K+0x11b8 = 1`.
///
/// exe: FUN_0046d840
pub fn skinned(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%f %f %f");
    c.w(c.k + 0x11bc, s.fb(0).unwrap_or(0));
    c.w(c.k + 0x11c0, s.fb(1).unwrap_or(0));
    c.w(c.k + 0x11c4, s.fb(2).unwrap_or(0));
    c.kw(0x11b8, 1);
}

/// `stockpart2` tail (`FUN_0046da30`): binds two `6` slots and stores the float at
/// `K+0x1260`.
///
/// exe: FUN_0046da30
pub fn stockpart2(c: &mut Ctx, line: &[u8]) {
    if !c.guard() {
        return;
    }
    let s = scan(line, "%d %f");
    let (a, b) = (s.db(0).unwrap_or(0), s.fb(1).unwrap_or(0));
    // `esi = K+0x1250`: bound resource at `esi`, speed at `esi+0x10`.
    bind_res(c, c.k + 0x1250, a, 6, 0);
    c.w(c.k + 0x1260, b);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Host that fails the test if a handler calls out: `flags`/`trigger` are pure memory.
    struct NoCalls;

    impl Host for NoCalls {
        fn call(&mut self, _m: &mut Space, c: &ExtCall) -> u32 {
            panic!("unexpected host call 0x{:x}", c.f);
        }
    }

    fn car(m: &mut Space) -> u32 {
        let k = 0x2000_0010;
        let p = 0x2000_0100;
        m.w32(k + 0x2e48, p);
        m.w32(p + 4, 0);
        k
    }

    /// Run one handler over `m` with no host calls.
    fn run(m: &mut Space, k: u32, h: Handler, line: &[u8]) {
        let mut host = NoCalls;
        let mut c = Ctx::new(m, &mut host, k);
        h(&mut c, line);
    }

    /// `K + 0xfa8` counts body records of `0x74` bytes starting at `K+0x128`; the "current
    /// record" the exe's `lea [ecx + n*0x74 + 0xb4]` addresses is `brec(n-1)`.
    #[test]
    fn cur_is_brec_of_the_last_body_record() {
        let mut m = Space::new();
        let k = car(&mut m);
        for n in 1..=32u32 {
            m.w32(k + 0xfa8, n);
            let mut host = NoCalls;
            let c = Ctx::new(&mut m, &mut host, k);
            assert_eq!(cur(&c), k + 0x74 * n + 0xb4);
            assert_eq!(cur(&c), brec(&c, n - 1));
            assert_eq!(cur(&c) + 0x58, k + 0x74 * n + 0x10c);
        }
    }

    /// `flags 16`: ORs `0x10 & 0x1ff` into the current record's `+0x58`, and leaves every
    /// other body record alone.
    #[test]
    fn flags_ors_into_the_current_record_only() {
        let mut m = Space::new();
        let k = car(&mut m);
        m.w32(k + 0xfa8, 2);
        run(&mut m, k, flags, b"flags\0 16");
        assert_eq!(m.r32(k + 0x74 * 2 + 0x10c), 0x10);
        assert_eq!(m.r32(k + 0x74 + 0x10c), 0);
        assert_eq!(m.r32(k + 0xfa8), 2);
    }

    /// The `K+0x1108` owner-ref dance (`FUN_0046bfa0` `0x46c0dc`..`0x46c16b`), against the words
    /// the exe writes for the same crafted state (Unicorn run of `0x46bfa0`, cw 0x007F). No cfg
    /// line in the corpus reaches this path, so it is pinned here instead of by the differential
    /// test. `ref_chain` is pure memory and runs under `NoCalls`.
    #[test]
    fn ref_chain_links_and_unlinks_like_the_exe() {
        const HEAD_SENT: u32 = 0x1111_1111;
        const TAIL_SENT: u32 = 0x2222_2222;
        const L1_SENT: u32 = 0x7777_7777;
        const L2_SENT: u32 = 0x8888_8888;
        // `(K, rec, prev, this, l1, l2)`: the two owner objects (`+0x48` their list link, `+0x50`
        // their owner word) and the link words they point at; `rec+0x54` holds `this`.
        fn chain(m: &mut Space) -> (u32, u32, u32, u32, u32, u32) {
            let k = car(m);
            let rec = k + 0x128;
            let prev = m.alloc(0x60);
            let this = m.alloc(0x60);
            let l1 = m.alloc(4);
            let l2 = m.alloc(4);
            m.w32(l1, L1_SENT);
            m.w32(l2, L2_SENT);
            m.w32(prev + 0x48, l1);
            m.w32(prev + 0x50, 0x1234);
            m.w32(this + 0x48, l2);
            m.w32(this + 0x50, 0x5678);
            m.w32(rec + 0x54, this);
            (k, rec, prev, this, l1, l2)
        }
        fn dance(m: &mut Space, k: u32, rec: u32) {
            let mut host = NoCalls;
            let mut c = Ctx::new(m, &mut host, k);
            ref_chain(&mut c, rec);
        }
        /// The link `this` makes: `[l2] = &head`, `this+0x48 = &head`, `K+0x110c = l2`,
        /// `K+0x1110 = this+0x50` and `K+0x1114 = this`, with `K+0x1108` left zero.
        fn assert_linked(m: &Space, k: u32, this: u32, l2: u32) {
            assert_eq!(m.r32(k + 0x1108), 0);
            assert_eq!(m.r32(k + 0x110c), l2);
            assert_eq!(m.r32(k + 0x1110), 0x5678);
            assert_eq!(m.r32(k + 0x1114), this);
            assert_eq!(m.r32(l2), k + 0x1108);
            assert_eq!(m.r32(this + 0x48), k + 0x1108);
        }

        // Previous owner linked and no head node: `prev` is unlinked through its own `+0x48`
        // word (set to the list's last node, here zero) and `this` is linked in.
        let mut m = Space::new();
        let (k, rec, prev, this, l1, l2) = chain(&mut m);
        m.w32(k + 0x1114, prev);
        dance(&mut m, k, rec);
        assert_eq!(m.r32(prev + 0x48), 0);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_linked(&m, k, this, l2);

        // Same, with a head and a last node: the unlink goes through `[head+4]` and `[last]`,
        // and `prev`'s own link word is left alone.
        let mut m = Space::new();
        let (k, rec, prev, this, l1, l2) = chain(&mut m);
        let (h, t) = (m.alloc(8), m.alloc(8));
        m.w32(h, HEAD_SENT);
        m.w32(t, TAIL_SENT);
        m.w32(k + 0x1108, h);
        m.w32(k + 0x110c, t);
        m.w32(k + 0x1114, prev);
        dance(&mut m, k, rec);
        assert_eq!(m.r32(h), HEAD_SENT);
        assert_eq!(m.r32(h + 4), t);
        assert_eq!(m.r32(t), h);
        assert_eq!(m.r32(t + 4), 0);
        assert_eq!(m.r32(prev + 0x48), l1);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_linked(&m, k, this, l2);

        // A zero `rec+0x54`: only the unlink and the head/last zeroing happen.
        let mut m = Space::new();
        let (k, rec, prev, this, l1, l2) = chain(&mut m);
        m.w32(rec + 0x54, 0);
        m.w32(k + 0x1114, prev);
        dance(&mut m, k, rec);
        for off in [0x1108, 0x110c, 0x1110, 0x1114] {
            assert_eq!(m.r32(k + off), 0);
        }
        assert_eq!(m.r32(prev + 0x48), 0);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_eq!(m.r32(l2), L2_SENT);
        assert_eq!(m.r32(this + 0x48), l2);

        // Nothing linked before: the link happens without any unlink.
        let mut m = Space::new();
        let (k, rec, _prev, this, l1, l2) = chain(&mut m);
        dance(&mut m, k, rec);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_linked(&m, k, this, l2);

        // A pending owner (`K+0x1110 != 0`) skips the dance entirely.
        let mut m = Space::new();
        let (k, rec, prev, this, l1, l2) = chain(&mut m);
        m.w32(k + 0x1110, 0x99);
        m.w32(k + 0x1114, prev);
        dance(&mut m, k, rec);
        assert_eq!(m.r32(k + 0x1108), 0);
        assert_eq!(m.r32(k + 0x110c), 0);
        assert_eq!(m.r32(k + 0x1110), 0x99);
        assert_eq!(m.r32(k + 0x1114), prev);
        assert_eq!(m.r32(prev + 0x48), l1);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_eq!(m.r32(l2), L2_SENT);
        assert_eq!(m.r32(this + 0x48), l2);

        // The record already owns the link: nothing changes.
        let mut m = Space::new();
        let (k, rec, _prev, this, l1, l2) = chain(&mut m);
        m.w32(k + 0x1114, this);
        dance(&mut m, k, rec);
        assert_eq!(m.r32(k + 0x1108), 0);
        assert_eq!(m.r32(k + 0x110c), 0);
        assert_eq!(m.r32(k + 0x1110), 0);
        assert_eq!(m.r32(k + 0x1114), this);
        assert_eq!(m.r32(l1), L1_SENT);
        assert_eq!(m.r32(l2), L2_SENT);
        assert_eq!(m.r32(this + 0x48), l2);
    }

    /// `trigger`: only a record whose `+0x3c` is `1` advances to `2` and marks its body
    /// (`[rec+0x40]+0x9c`) with `0x20`; a `2` or `3` (deformable) record is left untouched.
    #[test]
    fn trigger_finalises_a_body_record_once() {
        let mut m = Space::new();
        let k = car(&mut m);
        m.w32(k + 0xfa8, 1);
        let rec = k + 0x74 + 0xb4;
        let body = m.alloc(0xa0);
        m.w32(rec + 0x40, body);
        m.w32(rec + 0x3c, 1);
        run(&mut m, k, trigger, b"trigger\0");
        assert_eq!(m.r32(rec + 0x3c), 2);
        assert_eq!(m.r32(body + 0x9c), 0x20);
        // Second run: `+0x3c` is now 2, so nothing changes.
        run(&mut m, k, trigger, b"trigger\0");
        assert_eq!(m.r32(body + 0x9c), 0x20);
        m.w32(rec + 0x3c, 3);
        run(&mut m, k, trigger, b"trigger\0");
        assert_eq!(m.r32(rec + 0x3c), 3);
    }

    /// Both handlers obey the shared guards: `K+0x120 != 0` (already configured) and
    /// `K+0xfa8 <= 0` (no body record) leave memory untouched.
    #[test]
    fn flags_and_trigger_respect_the_guards() {
        let mut m = Space::new();
        let k = car(&mut m);
        let rec = k + 0x74 + 0xb4;
        m.w32(k + 0x120, 1);
        m.w32(k + 0xfa8, 1);
        run(&mut m, k, flags, b"flags\0 16");
        run(&mut m, k, trigger, b"trigger\0");
        assert_eq!(m.r32(rec + 0x58), 0);
        assert_eq!(m.r32(rec + 0x3c), 0);
        m.w32(k + 0x120, 0);
        m.w32(k + 0xfa8, 0);
        run(&mut m, k, flags, b"flags\0 16");
        run(&mut m, k, trigger, b"trigger\0");
        assert_eq!(m.r32(rec + 0x58), 0);
        assert_eq!(m.r32(rec + 0x3c), 0);
    }
}

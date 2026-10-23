//! Original 3x3 rotation and vector helpers over 3x4 row-major storage
//! (rows 0x10 apart; the fourth word of each row is padding). Every element
//! keeps the executable's x87 operand order under the D3D9 control word.
use crate::native_fpu::{Fpu, Op};
use slr_physics::space::Space;

const ONE: u32 = 0x5f08f0;
const HALF: u32 = 0x5f09d0;
const ZERO: u32 = 0x5e73cc;
type EulerTrig = fn(u32) -> Result<(slr_x87::X87, slr_x87::X87, bool), String>;

/// Exact FSIN/FCOS results for recovered special input domains. Normal finite
/// angles outside the tiny range stay unsupported; no approximate provider is
/// admitted. All arguments here originate in FLD m32, not arbitrary extF80.
fn euler_trig(bits: u32) -> Result<(slr_x87::X87, slr_x87::X87, bool), String> {
    use slr_x87::X87;
    let absolute = bits & 0x7fff_ffff;
    let value = X87::from_f32(f32::from_bits(bits));
    if absolute <= 0x2f00_0000 {
        // |x| <= 2^-33: |sin(x)-x| < |x|^3/6 and 1-cos(x) < x^2/2
        // are strictly below half an extF80 ulp, including the binade edges.
        return Ok((value, X87::from_i32(1), false));
    }
    if absolute > 0x5f00_0000 && absolute < 0x7f80_0000 {
        // Both instructions leave ST0 unchanged with C2 for |x|>2^63 in
        // the observed original. Exactly2^63 disagrees with the expected
        // range model on the Unicorn leg and remains unsupported.
        return Ok((value, value, true));
    }
    Err(format!(
        "NativeMatrixBoundary: original54ecd0 exact x87 sin/cos for angle word {bits:#010x}"
    ))
}

/// Original54ecd0 with exact recovered trig domains. Input words are loaded
/// before stores, preserving the original's aliased matrix/angle inputs.
/// `frame` is the original callee entry ESP; output padding is untouched.
/// exe: FUN_0054ecd0
pub fn from_euler(m: &mut Space, out: u32, angles: u32, frame: u32) -> Result<(), String> {
    from_euler_using(m, out, angles, frame, euler_trig)
}

/// Memory-only CFG/wheel consumers can also use a certified enclosure: every
/// original store must have one possible m32 result. This does not supply or
/// claim a scalar FSIN/FCOS result or certify their CPU status flags.
pub fn from_euler_memory(m: &mut Space, out: u32, angles: u32, frame: u32) -> Result<(), String> {
    match from_euler(m, out, angles, frame) {
        Ok(()) => Ok(()),
        Err(scalar) => crate::native_matrix_interval::certify(m, out, angles, frame)
            .map_err(|proof| format!("{scalar}; {proof}")),
    }
}

fn from_euler_using(
    m: &mut Space,
    out: u32,
    angles: u32,
    frame: u32,
    trig: EulerTrig,
) -> Result<(), String> {
    let inputs: [u32; 3] = std::array::from_fn(|i| m.r32(angles + i as u32 * 4));
    let [x, y, z] = inputs.map(trig);
    let (sx, cx, _) = x?;
    let (sy, cy, _) = y?;
    let (sz, cz, _) = z?;
    let s = frame.wrapping_sub(0x14);
    let mut fp = Fpu::new();
    fp.push(sx);
    fp.push(cx);
    fp.fstp(m, s + 0x18);
    fp.push(sy);
    fp.push(cy);
    fp.fstp(m, s + 8);
    fp.push(sz);
    fp.fstp(m, s + 4);
    fp.push(cz);
    fp.fstp(m, s);
    fp.fld(m, s + 4);
    fp.arith_st0(Op::Mul, 1);
    fp.fst(m, s + 0xc);
    fp.arith_st0(Op::Mul, 2);
    fp.fld(m, s);
    fp.fmul(m, s + 0x18);
    fp.faddp(1);
    fp.fstp(m, out);
    fp.fld(m, s);
    fp.arith_st0(Op::Mul, 1);
    fp.fst(m, s + 0x10);
    fp.arith_st0(Op::Mul, 2);
    fp.fld(m, s + 4);
    fp.fmul(m, s + 0x18);
    fp.fsubp(1);
    fp.fstp(m, out + 4);
    fp.fld(m, s + 8);
    fp.arith_st0(Op::Mul, 2);
    fp.fstp(m, out + 8);
    fp.fld(m, s + 4);
    fp.fmul(m, s + 8);
    fp.fstp(m, out + 0x10);
    fp.fld(m, s);
    fp.fmul(m, s + 8);
    fp.fstp(m, out + 0x14);
    fp.fchs();
    fp.fstp(m, out + 0x18);
    fp.fld(m, s + 0xc);
    fp.fmul(m, s + 0x18);
    fp.fld(m, s);
    fp.arith_st0(Op::Mul, 2);
    fp.fsubp(1);
    fp.fstp(m, out + 0x20);
    fp.fld(m, s + 0x10);
    fp.fmul(m, s + 0x18);
    fp.fld(m, s + 4);
    fp.arith_st0(Op::Mul, 2);
    fp.faddp(1);
    fp.fstp(m, out + 0x24);
    fp.pop();
    fp.fld(m, s + 8);
    fp.fmul(m, s + 0x18);
    fp.fstp(m, out + 0x28);
    Ok(())
}

/// Element address of row `r`, column `c`.
fn at(p: u32, r: u32, c: u32) -> u32 {
    p + r * 0x10 + c * 4
}

/// Original54e1d0: identity rotation; padding words are not written.
pub fn identity(m: &mut Space, p: u32) {
    for r in 0..3 {
        for c in 0..3 {
            m.w32(at(p, r, c), if r == c { 0x3f80_0000 } else { 0 });
        }
    }
}

/// Original54e4a0: `dst` = transpose(`src`). The in-place path moves the
/// three upper elements through the FPU and the lower ones as integers.
pub fn transpose(m: &mut Space, dst: u32, src: u32) {
    if src == dst {
        let mut fpu = Fpu::new();
        fpu.fld(m, dst + 4);
        let a = m.r32(dst + 0x10);
        let b = m.r32(dst + 0x20);
        fpu.fstp(m, dst + 0x10);
        fpu.fld(m, dst + 8);
        m.w32(dst + 4, a);
        let c = m.r32(dst + 0x24);
        fpu.fstp(m, dst + 0x20);
        fpu.fld(m, dst + 0x18);
        m.w32(dst + 8, b);
        fpu.fstp(m, dst + 0x24);
        m.w32(dst + 0x18, c);
        return;
    }
    for r in 0..3 {
        for c in 0..3 {
            m.w32(at(dst, r, c), m.r32(at(src, c, r)));
        }
    }
}

/// Original54e9c0: `dst` = `a` * `b` (thiscall dst; padding untouched).
pub fn product(m: &mut Space, dst: u32, a: u32, b: u32) {
    let ra = crate::native_entity::read_matrix(m, a);
    let rb = crate::native_entity::read_matrix(m, b);
    let out = crate::native_entity::matrix_product(ra, rb);
    for (i, word) in out.into_iter().enumerate() {
        m.w32(at(dst, i as u32 / 3, i as u32 % 3), word);
    }
}

/// Original54ebc0: `a` = `a` * `b`, formed in a 0x30-byte stack temporary
/// and copied back with `REP MOVSD` (twelve words: the temporary's three
/// unwritten padding words land in `a`'s padding). `frame` is the entry ESP
/// of the call (the return-address slot).
pub fn multiply_in_place(m: &mut Space, a: u32, b: u32, frame: u32) {
    let temp = frame.wrapping_sub(0x30);
    let mut fpu = Fpu::new();
    // (address of left factor, right factor) per product term, in order.
    let element = |m: &mut Space, fpu: &mut Fpu, terms: [(u32, u32); 3], out: u32| {
        fpu.fld(m, terms[0].0);
        fpu.fmul(m, terms[0].1);
        fpu.fld(m, terms[1].0);
        fpu.fmul(m, terms[1].1);
        fpu.faddp(1);
        fpu.fld(m, terms[2].0);
        fpu.fmul(m, terms[2].1);
        fpu.faddp(1);
        fpu.fstp(m, out);
    };
    let (ae, be) = (|r, c| at(a, r, c), |r, c| at(b, r, c));
    element(
        m,
        &mut fpu,
        [
            (be(1, 0), ae(0, 1)),
            (be(0, 0), ae(0, 0)),
            (ae(0, 2), be(2, 0)),
        ],
        temp,
    );
    element(
        m,
        &mut fpu,
        [
            (be(2, 1), ae(0, 2)),
            (be(0, 1), ae(0, 0)),
            (be(1, 1), ae(0, 1)),
        ],
        temp + 4,
    );
    element(
        m,
        &mut fpu,
        [
            (be(1, 2), ae(0, 1)),
            (be(2, 2), ae(0, 2)),
            (ae(0, 0), be(0, 2)),
        ],
        temp + 8,
    );
    element(
        m,
        &mut fpu,
        [
            (ae(1, 2), be(2, 0)),
            (be(1, 0), ae(1, 1)),
            (ae(1, 0), be(0, 0)),
        ],
        temp + 0x10,
    );
    element(
        m,
        &mut fpu,
        [
            (be(2, 1), ae(1, 2)),
            (be(1, 1), ae(1, 1)),
            (be(0, 1), ae(1, 0)),
        ],
        temp + 0x14,
    );
    element(
        m,
        &mut fpu,
        [
            (ae(1, 0), be(0, 2)),
            (be(2, 2), ae(1, 2)),
            (be(1, 2), ae(1, 1)),
        ],
        temp + 0x18,
    );
    element(
        m,
        &mut fpu,
        [
            (ae(2, 2), be(2, 0)),
            (be(1, 0), ae(2, 1)),
            (ae(2, 0), be(0, 0)),
        ],
        temp + 0x20,
    );
    element(
        m,
        &mut fpu,
        [
            (be(2, 1), ae(2, 2)),
            (ae(2, 1), be(1, 1)),
            (ae(2, 0), be(0, 1)),
        ],
        temp + 0x24,
    );
    element(
        m,
        &mut fpu,
        [
            (ae(2, 0), be(0, 2)),
            (be(2, 2), ae(2, 2)),
            (be(1, 2), ae(2, 1)),
        ],
        temp + 0x28,
    );
    // The saved ESI/EDI pushes sit below the temporary and are not copied.
    for word in 0..12 {
        m.w32(a + word * 4, m.r32(temp + word * 4));
    }
}

/// Original54e2d0: squared Frobenius distance of two rotations, returned in
/// ST(0). Elements are subtracted in storage order and summed from the last.
pub fn distance(m: &mut Space, a: u32, b: u32, frame: u32) -> slr_x87::X87 {
    let mut fpu = Fpu::new();
    let local = frame.wrapping_sub(8);
    for (r, c) in [(0, 0), (0, 1), (0, 2), (1, 0), (1, 1), (1, 2), (2, 0)] {
        fpu.fld(m, at(a, r, c));
        fpu.fsub(m, at(b, r, c));
    }
    fpu.fstp(m, local + 4);
    fpu.fld(m, at(a, 2, 1));
    fpu.fsub(m, at(b, 2, 1));
    fpu.fstp(m, local);
    fpu.fld(m, at(a, 2, 2));
    fpu.fsub(m, at(b, 2, 2));
    // FST [esp+0c] stores into the argument slot above the return address.
    fpu.fst(m, frame + 4);
    fpu.fmul(m, frame + 4);
    fpu.fld(m, local);
    fpu.fmul(m, local);
    fpu.faddp(1);
    fpu.fld(m, local + 4);
    fpu.fmul(m, local + 4);
    fpu.faddp(1);
    for i in 1..=6 {
        fpu.fld_st(i);
        fpu.arith_st0(Op::Mul, i + 1);
        fpu.faddp(1);
    }
    fpu.fstp_st(6);
    for _ in 0..5 {
        fpu.fstp_st(0);
    }
    fpu.pop()
}

/// Original54eda0: rotation from a quaternion (x, y, z, w at `q`).
pub fn from_quaternion(m: &mut Space, dst: u32, q: u32, frame: u32) {
    let mut fpu = Fpu::new();
    let s = frame.wrapping_sub(0x1c);
    let arg = frame + 4;
    fpu.fld(m, q);
    fpu.arith_st0(Op::Add, 0);
    fpu.fld(m, q + 4);
    fpu.arith_st0(Op::Add, 0);
    fpu.fstp(m, s);
    fpu.fld(m, q + 8);
    fpu.arith_st0(Op::Add, 0);
    fpu.fstp(m, arg);
    fpu.fld_st(0);
    fpu.fmul(m, q);
    fpu.fstp(m, s + 0x18);
    fpu.fld(m, s);
    fpu.fmul(m, q);
    fpu.fstp(m, s + 4);
    fpu.fld(m, arg);
    fpu.fmul(m, q);
    fpu.fstp(m, s + 0xc);
    fpu.fld(m, s);
    fpu.fmul(m, q + 4);
    fpu.fstp(m, s + 0x14);
    fpu.fld(m, arg);
    fpu.fmul(m, q + 4);
    fpu.fstp(m, s + 0x10);
    fpu.fld(m, arg);
    fpu.fmul(m, q + 8);
    fpu.fstp(m, s + 8);
    fpu.fmul(m, q + 0xc);
    fpu.fld(m, s);
    fpu.fmul(m, q + 0xc);
    fpu.fld(m, arg);
    fpu.fmul(m, q + 0xc);
    fpu.fld(m, s + 8);
    fpu.fadd(m, s + 0x14);
    fpu.fsubr(m, ONE);
    fpu.fstp(m, at(dst, 0, 0));
    fpu.fld(m, s + 4);
    fpu.arith_st0(Op::Sub, 1);
    fpu.fstp(m, at(dst, 0, 1));
    fpu.fld_st(1);
    fpu.fadd(m, s + 0xc);
    fpu.fstp(m, at(dst, 0, 2));
    fpu.fadd(m, s + 4);
    fpu.fstp(m, at(dst, 1, 0));
    fpu.fld(m, s + 8);
    fpu.fadd(m, s + 0x18);
    fpu.fsubr(m, ONE);
    fpu.fstp(m, at(dst, 1, 1));
    fpu.fld(m, s + 0x10);
    fpu.arith_st0(Op::Sub, 2);
    fpu.fstp(m, at(dst, 1, 2));
    fpu.fld(m, s + 0xc);
    fpu.arith_st0(Op::Sub, 1);
    fpu.fstp(m, at(dst, 2, 0));
    fpu.fstp_st(0);
    fpu.fadd(m, s + 0x10);
    fpu.fstp(m, at(dst, 2, 1));
    fpu.fld(m, s + 0x14);
    fpu.fadd(m, s + 0x18);
    fpu.fsubr(m, ONE);
    fpu.fstp(m, at(dst, 2, 2));
}

/// Original54ff20: quaternion (x, y, z, w) of a rotation. `frame` is the
/// entry ESP; the axis-order table and results pass through its locals.
pub fn to_quaternion(m: &mut Space, q: u32, r: u32, frame: u32) {
    let mut fpu = Fpu::new();
    // sub esp,14; push esi: locals at frame-0x14.., esi saved at frame-0x18.
    let l = frame.wrapping_sub(0x14);
    fpu.fld(m, at(r, 1, 1));
    fpu.fadd(m, at(r, 0, 0));
    m.w32(l, q);
    m.w32(l + 4, 1);
    fpu.fadd(m, at(r, 2, 2));
    m.w32(l + 8, 2);
    m.w32(l + 0xc, 0);
    if !fpu.fcom(m, ZERO).test(0x41) {
        fpu.fadd(m, ONE);
        fpu.fsqrt();
        fpu.fld_st(0);
        fpu.fmul(m, HALF);
        fpu.fstp(m, q + 0xc);
        fpu.fdivr(m, HALF);
        for (out, (p, n)) in [
            (0, ((2, 1), (1, 2))),
            (4, ((0, 2), (2, 0))),
            (8, ((1, 0), (0, 1))),
        ] {
            fpu.fld(m, at(r, p.0, p.1));
            fpu.fsub(m, at(r, n.0, n.1));
            fpu.arith_st0(Op::Mul, 1);
            fpu.fstp(m, q + out);
        }
        fpu.fstp_st(0);
        return;
    }
    fpu.fstp_st(0);
    let diagonal = |i: u32| at(r, i, i);
    let mut i = 0u32;
    fpu.fld(m, diagonal(1));
    if !fpu.fcomp(m, diagonal(0)).test(0x41) {
        i = 1;
    }
    fpu.fld(m, diagonal(2));
    if !fpu.fcomp(m, diagonal(i)).test(0x41) {
        i = 2;
    }
    // After three more pushes the table is at esp+0x14 = frame-0x10+4*index.
    let table = l + 4;
    let j = m.r32(table + i * 4);
    let k = m.r32(table + j * 4);
    let e = |row: u32, col: u32| r + row * 0x10 + col * 4;
    fpu.fld(m, e(k, k));
    fpu.fadd(m, e(j, j));
    fpu.fsubr(m, e(i, i));
    fpu.fadd(m, ONE);
    fpu.fsqrt();
    fpu.fld_st(0);
    fpu.fmul(m, HALF);
    fpu.fstp(m, table + i * 4);
    if !fpu.fcom(m, ZERO).test(0x40) {
        fpu.fdivr(m, HALF);
    }
    fpu.fld(m, e(k, j));
    fpu.fsub(m, e(j, k));
    fpu.arith_st0(Op::Mul, 1);
    fpu.fstp(m, table + 0xc);
    fpu.fld(m, e(j, i));
    fpu.fadd(m, e(i, j));
    fpu.arith_st0(Op::Mul, 1);
    fpu.fstp(m, table + j * 4);
    fpu.fld(m, e(k, i));
    fpu.fadd(m, e(i, k));
    fpu.arith_st0(Op::Mul, 1);
    fpu.fstp(m, table + k * 4);
    fpu.fstp_st(0);
    for word in 0..4 {
        m.w32(q + word * 4, m.r32(table + word * 4));
    }
}

/// Original436880: `dst` = rotation * vector, x87 order retained.
pub fn rotate(m: &mut Space, dst: u32, matrix: u32, vector: u32) {
    let v = crate::native_install::transform(m, matrix, vector);
    for (i, word) in v.into_iter().enumerate() {
        m.w32(dst + i as u32 * 4, word);
    }
}

/// Original477560: `dst` += `v` (FLD v; FADD dst).
pub fn add_vector(m: &mut Space, dst: u32, v: u32) {
    let mut fpu = Fpu::new();
    for i in 0..3 {
        fpu.fld(m, v + i * 4);
        fpu.fadd(m, dst + i * 4);
        fpu.fstp(m, dst + i * 4);
    }
}

/// Original54e510 with its 54e3b0 adjugate: `dst` = inverse(`src`). The
/// determinant is stored over the caller's argument slot (`frame`+4); the
/// adjugate is built in 54e3b0's 0x30-byte temporary and copied with its
/// three padding words, then scaled by 1/det while transposing.
pub fn inverse(m: &mut Space, dst: u32, src: u32, frame: u32) {
    let s = |r: u32, c: u32| at(src, r, c);
    let mut fpu = Fpu::new();
    let triple = |fpu: &mut Fpu, m: &Space, a: u32, b: u32, c: u32| {
        fpu.fld(m, a);
        fpu.fmul(m, b);
        fpu.fmul(m, c);
    };
    triple(&mut fpu, m, s(2, 0), s(0, 1), s(1, 2));
    triple(&mut fpu, m, s(0, 2), s(1, 0), s(2, 1));
    fpu.faddp(1);
    triple(&mut fpu, m, s(2, 2), s(0, 0), s(1, 1));
    fpu.faddp(1);
    triple(&mut fpu, m, s(0, 0), s(2, 1), s(1, 2));
    fpu.fsubp(1);
    triple(&mut fpu, m, s(2, 2), s(0, 1), s(1, 0));
    fpu.fsubp(1);
    triple(&mut fpu, m, s(2, 0), s(0, 2), s(1, 1));
    fpu.fsubp(1);
    fpu.fstp(m, frame + 4);
    // 54e3b0 entered below `push esi; push eax` and its return address.
    let temp = frame.wrapping_sub(0xc).wrapping_sub(0x30);
    let terms = [
        ((1, 1), (2, 2), (2, 1), (1, 2), 0x00),
        ((2, 0), (1, 2), (1, 0), (2, 2), 0x04),
        ((2, 1), (1, 0), (2, 0), (1, 1), 0x08),
        ((2, 1), (0, 2), (0, 1), (2, 2), 0x10),
        ((0, 0), (2, 2), (2, 0), (0, 2), 0x14),
        ((2, 0), (0, 1), (2, 1), (0, 0), 0x18),
        ((0, 1), (1, 2), (0, 2), (1, 1), 0x20),
        ((1, 0), (0, 2), (0, 0), (1, 2), 0x24),
        ((1, 1), (0, 0), (1, 0), (0, 1), 0x28),
    ];
    for (a, b, c, d, out) in terms {
        fpu.fld(m, s(a.0, a.1));
        fpu.fmul(m, s(b.0, b.1));
        fpu.fld(m, s(c.0, c.1));
        fpu.fmul(m, s(d.0, d.1));
        fpu.fsubp(1);
        fpu.fstp(m, temp + out);
    }
    for word in 0..12 {
        m.w32(dst + word * 4, m.r32(temp + word * 4));
    }
    fpu.fld(m, ONE);
    fpu.fdiv(m, frame + 4);
    for (r, c) in [(0, 0), (1, 1), (2, 2)] {
        fpu.fld_st(0);
        fpu.fmul(m, at(dst, r, c));
        fpu.fstp(m, at(dst, r, c));
    }
    for (a, b) in [((0, 1), (1, 0)), ((0, 2), (2, 0))] {
        fpu.fld_st(0);
        fpu.fmul(m, at(dst, a.0, a.1));
        fpu.fld_st(1);
        fpu.fmul(m, at(dst, b.0, b.1));
        fpu.fstp(m, at(dst, a.0, a.1));
        fpu.fstp(m, at(dst, b.0, b.1));
    }
    fpu.fld_st(0);
    fpu.fmul(m, at(dst, 1, 2));
    fpu.fxch(1);
    fpu.fmul(m, at(dst, 2, 1));
    fpu.fstp(m, at(dst, 1, 2));
    fpu.fstp(m, at(dst, 2, 1));
}

#[cfg(test)]
mod euler_tests {
    use super::*;
    use serde_json::Value;

    #[cfg(feature = "experimental-intel-trig")]
    #[test]
    fn intel_euler_candidate_complete_consumer_diagnostic() {
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let fixture =
            root.join("re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json");
        let bytes = std::fs::read(&fixture).expect("generate original Euler vectors");
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["rows"].as_array().unwrap().len(), 160);
        assert_eq!(
            data["original_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        let mut rows = vec![];
        let (mut unsupported, mut mismatched, mut raw_mismatches) = (0, 0, 0);
        for (index, row) in data["rows"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let result = from_euler_using(
                &mut m,
                row["out"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                0x7fef0000,
                |bits| slr_x87::intel_trig::candidate_f32(bits).map_err(str::to_owned),
            );
            let actual = (0..0x90 / 4)
                .map(|i| m.r32(0x20000000 + i * 4))
                .collect::<Vec<_>>();
            let differences = actual
                .iter()
                .enumerate()
                .filter_map(|(i, word)| {
                    (*word as u64 != row["after"][i].as_u64().unwrap()).then_some(i)
                })
                .collect::<Vec<_>>();
            let mut raw = vec![];
            for observation in row["trig"].as_array().unwrap() {
                let bits = observation["input"].as_u64().unwrap() as u32;
                if let Ok((sin, cos, _)) = slr_x87::intel_trig::candidate_f32(bits) {
                    let value = if observation["op"] == "sin" { sin } else { cos };
                    let image = value
                        .to_bytes()
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>();
                    let equal = observation["st0"] == image;
                    raw_mismatches += usize::from(!equal);
                    raw.push(
                        serde_json::json!({"input":bits, "op":observation["op"], "candidate":image,
                        "original":observation["st0"], "equal":equal}),
                    );
                }
            }
            match &result {
                Err(_) => unsupported += 1,
                Ok(()) if !differences.is_empty() => mismatched += 1,
                _ => {}
            }
            rows.push(serde_json::json!({"index":index, "error":result.err(),
                "actual_heap":actual, "differing_words":differences, "raw_trig":raw}));
        }
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let out = root.join(format!("re/out/boot/intel-euler-candidate-{stamp}.json"));
        let source_hashes = [
            "crates/slr-boot/src/native_matrix.rs",
            "crates/slr-boot/src/native_fpu.rs",
            "crates/slr-x87/src/intel_trig.rs",
            "crates/slr-x87/src/ffi.rs",
        ]
        .into_iter()
        .map(|name| {
            (
                name,
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join(name)).unwrap())
                ),
            )
        })
        .collect::<std::collections::BTreeMap<_, _>>();
        std::fs::write(&out, serde_json::to_vec_pretty(&serde_json::json!({
            "scope":"diagnostic: every caller heap word and raw trig observation; no exclusions; no production/float acceptance",
            "fixture_sha256":format!("{:x}",Sha256::digest(bytes)), "source_hashes":source_hashes,
            "cases":160, "unsupported":unsupported, "mismatched_heaps":mismatched,
            "raw_mismatches":raw_mismatches, "rows":rows})).unwrap()).unwrap();
        eprintln!("candidate diagnostic: {unsupported} unsupported, {mismatched} heap mismatches, {raw_mismatches} raw mismatches; {}", out.display());
        assert_eq!((unsupported, mismatched, raw_mismatches), (0,0,0),
            "experimental candidate has unresolved original observations; production remains bounded");
    }

    #[test]
    fn original_euler_special_domains_match_complete_buffers_and_raw_trig() {
        let path = std::env::var("SLR_EULER_VECTORS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    "../../re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json",
                )
            });
        let bytes = std::fs::read(path)
            .expect("generate native_car_euler_vectors.py gen before original-backed tests");
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(data["schema"], "slr-original-euler-v2");
        assert_eq!(
            data["original_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["fpu_cw"], 0x7f);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 160);
        let (mut supported, mut unresolved) = (0, 0);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["index"], i);
            assert_eq!(row["before"].as_array().unwrap().len(), 0x90 / 4);
            assert_eq!(row["after"].as_array().unwrap().len(), 0x90 / 4);
            assert_eq!(row["trig"].as_array().unwrap().len(), 6);
            let mut m = Space::new();
            for (j, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + j as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let result = from_euler(
                &mut m,
                row["out"].as_u64().unwrap() as u32,
                row["source"].as_u64().unwrap() as u32,
                0x7fef0000,
            );
            let expected_unsupported = row["domains"]
                .as_array()
                .unwrap()
                .iter()
                .any(|d| d != "tiny-exact" && d != "range-c2");
            if expected_unsupported {
                assert!(result.unwrap_err().contains("exact x87 sin/cos"));
                for (j, word) in row["before"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(0x20000000 + j as u32 * 4),
                        word.as_u64().unwrap() as u32,
                        "unsupported{i} word{j}"
                    );
                }
                unresolved += 1;
                continue;
            }
            result.unwrap();
            for (j, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + j as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "case{i} word{j}"
                );
            }
            for trig in row["trig"].as_array().unwrap() {
                let bits = trig["input"].as_u64().unwrap() as u32;
                let (sin, cos, c2) = euler_trig(bits).unwrap();
                let actual = if trig["op"] == "sin" { sin } else { cos };
                let encoded = actual
                    .to_bytes()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect::<String>();
                assert_eq!(
                    encoded,
                    trig["st0"].as_str().unwrap(),
                    "case{i} raw {}",
                    trig["op"]
                );
                assert_eq!(
                    c2,
                    trig["fpsw"].as_u64().unwrap() & 0x400 != 0,
                    "case{i} C2"
                );
            }
            supported += 1;
        }
        assert_eq!((supported, unresolved), (50, 110));
    }

    #[test]
    fn config_host_euler_uses_captured_words_and_restores_native_scratch() {
        use slr_physics::cfg::{Arg, ExtCall, Host, This, EXT_EULER_TO_MATRIX};
        let path = std::env::var("SLR_EULER_VECTORS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    "../../re/out/checkpoints/20261021-campaign/car-command/euler-specials-v2.json",
                )
            });
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 160);
        let (mut parity, mut unresolved) = (0, 0);
        for (i, row) in rows.iter().enumerate() {
            for saved_sp in [0, 0x7fef0000] {
                let mut m = Space::new();
                crate::native_stack::set_sp(&mut m, saved_sp);
                for (j, word) in row["before"].as_array().unwrap().iter().enumerate() {
                    m.w32(0x20000000 + j as u32 * 4, word.as_u64().unwrap() as u32);
                }
                let call = ExtCall {
                    f: EXT_EULER_TO_MATRIX,
                    this: This::Addr(row["out"].as_u64().unwrap() as u32),
                    a: row["angles"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|word| Arg::U(word.as_u64().unwrap() as u32))
                        .collect(),
                };
                let mut host = crate::native_body::ConfigHost::new(0);
                assert_eq!(host.call(&mut m, &call), 0);
                assert_eq!(crate::native_stack::sp(&m), saved_sp, "case{i} ESP");
                let exact_scalar_domain = row["domains"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|d| d == "tiny-exact" || d == "range-c2");
                let certified_memory_domain = row["angles"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|word| word.as_u64().unwrap() as u32 & 0x7fff_ffff <= 0x4100_0000);
                let unsupported = !exact_scalar_domain && !certified_memory_domain;
                let expected = if unsupported {
                    assert!(host.finish().unwrap_err().contains("exact x87 sin/cos"));
                    unresolved += 1;
                    &row["before"]
                } else {
                    host.finish().unwrap();
                    parity += 1;
                    &row["after"]
                };
                for (j, word) in expected.as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(0x20000000 + j as u32 * 4),
                        word.as_u64().unwrap() as u32,
                        "ConfigHost case{i} word{j}"
                    );
                }
            }
        }
        assert_eq!((parity, unresolved), (256, 64));
    }

    #[test]
    fn original_cfg_click_consumes_exact_euler_with_null_body_collaborator() {
        use slr_physics::cfg::{Arg, Ctx, ExtCall, Host, This, EXT_BODY, EXT_EULER_TO_MATRIX};
        struct ClickHost {
            inner: crate::native_body::ConfigHost,
            euler: Vec<Value>,
            bodies: Vec<Value>,
        }
        impl Host for ClickHost {
            fn call(&mut self, m: &mut Space, call: &ExtCall) -> u32 {
                let This::Addr(this) = call.this else {
                    panic!("unexpected CFG this")
                };
                match call.f {
                    EXT_EULER_TO_MATRIX => {
                        let a: Vec<u32> = call
                            .a
                            .iter()
                            .map(|arg| match arg {
                                Arg::U(word) => *word,
                                _ => panic!("unexpected Euler arg"),
                            })
                            .collect();
                        self.euler.push(serde_json::json!({"this":this,"a":a}));
                        self.inner.call(m, call)
                    }
                    EXT_BODY => {
                        let a: Vec<Value> = call
                            .a
                            .iter()
                            .map(|arg| match arg {
                                Arg::U(word) => serde_json::json!(word),
                                Arg::S(text) => serde_json::json!(text),
                            })
                            .collect();
                        self.bodies.push(serde_json::json!({"this":this,"a":a}));
                        0
                    }
                    _ => panic!("unexpected collaborator {:#x}", call.f),
                }
            }
        }
        let path = std::env::var("SLR_CFG_EULER_VECTORS")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| {
                std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(
                    "../../re/out/checkpoints/20261021-campaign/car-command/cfg-euler-v2.json",
                )
            });
        let data: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(data["schema"], "slr-original-cfg-euler-v1");
        assert_eq!(
            data["original_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        assert_eq!(data["fpu_cw"], 0x7f);
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 24);
        let (mut parity, mut unresolved) = (0, 0);
        for (i, row) in rows.iter().enumerate() {
            assert_eq!(row["index"], i);
            let k = row["k"].as_u64().unwrap() as u32;
            let mut m = Space::new();
            for (j, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(k + j as u32 * 4, word.as_u64().unwrap() as u32);
            }
            let mut host = ClickHost {
                inner: crate::native_body::ConfigHost::new(0),
                euler: vec![],
                bodies: vec![],
            };
            let click = slr_physics::cfg::table()
                .into_iter()
                .find(|(name, _)| *name == "click")
                .unwrap()
                .1;
            click(
                &mut Ctx::new(&mut m, &mut host, k),
                row["line"].as_str().unwrap().as_bytes(),
            );
            assert_eq!(
                serde_json::json!(host.euler),
                row["euler"],
                "case{i} Euler calls"
            );
            assert_eq!(
                serde_json::json!(host.bodies),
                row["body_calls"],
                "case{i} body calls"
            );
            let exact_scalar_domain = row["domains"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d == "tiny-exact" || d == "range-c2");
            let certified_memory_domain = row["euler"]
                .as_array()
                .unwrap()
                .iter()
                .flat_map(|call| call["a"].as_array().unwrap())
                .all(|word| word.as_u64().unwrap() as u32 & 0x7fff_ffff <= 0x4100_0000);
            let unsupported = !exact_scalar_domain && !certified_memory_domain;
            if unsupported {
                assert!(host
                    .inner
                    .finish()
                    .unwrap_err()
                    .contains("exact x87 sin/cos"));
                unresolved += 1;
            } else {
                host.inner.finish().unwrap();
                assert_eq!(row["after"].as_array().unwrap().len(), 0x2e98 / 4);
                for (j, word) in row["after"].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(k + j as u32 * 4),
                        word.as_u64().unwrap() as u32,
                        "CFG click case{i} word{j}"
                    );
                }
                parity += 1;
            }
        }
        assert_eq!((parity, unresolved), (18, 6));
    }
}

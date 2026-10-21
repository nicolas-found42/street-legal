//! Original 4a9710/4a9850/4a9c80/4a9fe0/4aa330 mesh mass integrals.
//! Every arithmetic operation uses the configured 24-bit x87 precision;
//! double globals retain their original constants and store boundaries.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
use std::ops::{Add, Div, Mul, Neg, Sub};
#[derive(Clone, Copy)]
struct F(X87);
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
impl F {
    fn bits(v: u32) -> Self {
        Self(X87::from_f32(f32::from_bits(v)))
    }
    fn d(v: f64) -> Self {
        Self(X87::from_f64(v))
    }
    fn word(self) -> u32 {
        self.0.to_f32(PC, RC).to_bits()
    }
    fn single(self) -> Self {
        Self::bits(self.word())
    }
    fn double(self) -> Self {
        Self::d(self.0.to_f64(PC, RC))
    }
}
impl Add for F {
    type Output = Self;
    fn add(self, r: Self) -> Self {
        Self(self.0.add(r.0, PC, RC))
    }
}
impl Sub for F {
    type Output = Self;
    fn sub(self, r: Self) -> Self {
        Self(self.0.sub(r.0, PC, RC))
    }
}
impl Mul for F {
    type Output = Self;
    fn mul(self, r: Self) -> Self {
        Self(self.0.mul(r.0, PC, RC))
    }
}
impl Div for F {
    type Output = Self;
    fn div(self, r: Self) -> Self {
        Self(self.0.div(r.0, PC, RC))
    }
}
impl Neg for F {
    type Output = Self;
    fn neg(self) -> Self {
        let mut bits = self.0.to_bytes();
        bits[9] ^= 0x80;
        Self(X87::from_bytes(bits))
    }
}
#[derive(Clone, Copy)]
struct Projection {
    p1: F,
    pa: F,
    paa: F,
    paaa: F,
    pb: F,
    pbb: F,
    pbbb: F,
    pab: F,
    paab: F,
    pabb: F,
}
fn project(vertices: &[[F; 3]], ids: [usize; 3], a: usize, b: usize) -> Projection {
    let zero = F::d(0.);
    let mut p = Projection {
        p1: zero,
        pa: zero,
        paa: zero,
        paaa: zero,
        pb: zero,
        pbb: zero,
        pbbb: zero,
        pab: zero,
        paab: zero,
        pabb: zero,
    };
    for edge in 0..3 {
        let [a0, b0] = [vertices[ids[edge]][a], vertices[ids[edge]][b]];
        let [a1, b1] = [
            vertices[ids[(edge + 1) % 3]][a],
            vertices[ids[(edge + 1) % 3]][b],
        ];
        let da = a1 - a0;
        let db = b1 - b0;
        let a1sq = a1 * a1;
        let a0sq = a0 * a0;
        let a0cube = a0sq * a0;
        let b1cube = b1 * b1 * b1;
        let b0sq = b0 * b0;
        let b0cube = b0sq * b0;
        let ca = (a1 + a0) * a1 + a0sq;
        let caa = ca * a1 + a0cube;
        let twice = a1 * a0 + a1 * a0;
        let cab1 = a1sq * F::d(3.) + twice + a0sq;
        let cab0 = a0sq * F::d(3.) + twice + a1sq;
        let cb = (b1 + b0) * b1 + b0sq;
        let cbb = cb * b1 + b0cube;
        p.p1 = ((a1 + a0) * db + p.p1).double();
        p.pa = (ca * db + p.pa).double();
        p.paa = (caa * db + p.paa).double();
        p.paaa = ((caa * a1 + a0cube * a0) * db + p.paaa).double();
        p.pb = (cb * da + p.pb).double();
        p.pbb = (cbb * da + p.pbb).double();
        p.pbbb = ((cbb * b1 + b0cube * b0) * da + p.pbbb).double();
        p.pab = ((cab0 * b0 + cab1 * b1) * db + p.pab).double();
        p.paab = (((a0cube * F::d(4.) + cab0 * a1) * b0 + (cab1 * a0 + a1sq * a1 * F::d(4.)) * b1)
            * db
            + p.paab)
            .double();
        let b10sq = b1 * b0sq;
        let b1sq0 = b1 * b1 * b0;
        p.pabb = ((((b1sq0 + b1sq0 + b10sq * F::d(3.) + b0cube * F::d(4.) + b1cube) * a0)
            + ((b1sq0 * F::d(3.) + (b10sq + b10sq) + b1cube * F::d(4.) + b0cube) * a1))
            * da
            + p.pabb)
            .double();
    }
    p.p1 = (p.p1 * F::d(0.5)).double();
    p.pa = (p.pa * F::d(1. / 6.)).double();
    p.paa = (p.paa * F::d(1. / 12.)).double();
    p.paaa = (p.paaa * F::d(1. / 20.)).double();
    p.pb = (p.pb * F::d(-1. / 6.)).double();
    p.pbb = (p.pbb * F::d(-1. / 12.)).double();
    p.pbbb = (p.pbbb * F::d(-1. / 20.)).double();
    p.pab = (p.pab * F::d(1. / 24.)).double();
    p.paab = (p.paab * F::d(1. / 60.)).double();
    p.pabb = (p.pabb * F::d(-1. / 60.)).double();
    p
}
struct FaceIntegrals {
    first: [F; 3],
    second: [F; 3],
    third: [F; 3],
    product: [F; 3],
}
fn face(
    vertices: &[[F; 3]],
    ids: [usize; 3],
    n: [F; 3],
    w: F,
    a: usize,
    b: usize,
    c: usize,
) -> FaceIntegrals {
    let p = project(vertices, ids, a, b);
    let k = F::d(1.) / n[c];
    let k2 = k * k;
    let k3 = k2 * k;
    let na = n[a];
    let nb = n[b];
    let first = [
        (p.pa * k).double(),
        (p.pb * k).double(),
        -((nb * p.pb + na * p.pa + w * p.p1) * k2),
    ];
    let mixed = nb * p.pb + na * p.pa;
    let twice = nb * na + nb * na;
    let second_c = ((((mixed + mixed + w * p.p1) * w + nb * nb * p.pbb) + na * na * p.paa)
        + twice * p.pab)
        * k3;
    let second = [p.paa * k, p.pbb * k, second_c];
    let third_base = (na * na * nb * F::d(3.) * p.paab + na * na * na * p.paaa).double();
    let third_base =
        (nb * nb * na * F::d(3.) * p.pabb + nb * nb * nb * p.pbbb + third_base).double();
    let third_base = ((nb * nb * p.pbb + na * na * p.paa + twice * p.pab) * (w * F::d(3.))
        + third_base)
        .double();
    let third_base =
        (((nb * p.pb + na * p.pa) * F::d(3.) + w * p.p1) * (w * w) + third_base).double();
    let third = [p.paaa * k, p.pbbb * k, -(k3 * k * third_base)];
    let mixed = nb * p.pab + na * p.paa;
    let product = [
        p.paab * k,
        -((na * p.pabb + nb * p.pbbb + w * p.pbb) * k2),
        ((nb * nb * p.pabb + na * na * p.paaa + twice * p.paab).double()
            + (mixed + mixed + w * p.pa) * w)
            * k3,
    ];
    FaceIntegrals {
        first: first.map(F::double),
        second: second.map(F::double),
        third: third.map(F::double),
        product: product.map(F::double),
    }
}
/// Original54e510: cofactors are stored as float before scaling/transposition.
fn inverse(values: [F; 9]) -> [F; 9] {
    let [a, b, c, d, e, f, g, h, i] = values;
    // The six determinant terms follow the x87 instruction order, including the
    // initial g*b*f term preceding c*d*h and i*a*e.
    let det = (((g * b * f + c * d * h) + i * a * e) - a * h * f) - i * b * d - g * c * e;
    let cof = [
        e * i - f * h,
        f * g - d * i,
        d * h - e * g,
        c * h - b * i,
        a * i - c * g,
        b * g - a * h,
        b * f - c * e,
        c * d - a * f,
        a * e - b * d,
    ]
    .map(F::single);
    let inv = F::d(1.) / det.single();
    std::array::from_fn(|k| (inv * cof[(k % 3) * 3 + k / 3]).single())
}
/// Shift vertices to their computed centre of mass and write inverse inertia
/// per unit mass at body+18. Temporary geometry allocations follow49a9710.
pub fn compute(m: &mut Space, body: u32, definition: u32) -> Result<(), String> {
    let vc = m.r32(definition + 0x9c) as usize;
    let fc = m.r32(definition + 0xa4) as usize;
    let vp = m.r32(definition + 0xa0);
    let fp = m.r32(definition + 0xa8);
    // Original4a9710 copies its working arrays through the allocator. The raw
    // pointers are observable in the same physics memory arena after this call.
    let temp_v = m.alloc(vc as u32 * 12);
    let temp_f = m.alloc(fc as u32 * 36);
    let vertices: Vec<[F; 3]> = (0..vc)
        .map(|v| std::array::from_fn(|a| F::bits(m.r32(vp + v as u32 * 12 + a as u32 * 4))))
        .collect();
    for (v, point) in vertices.iter().enumerate() {
        for (a, value) in point.iter().enumerate() {
            m.w32(temp_v + v as u32 * 12 + a as u32 * 4, value.word());
        }
    }
    let zero = F::d(0.);
    let mut volume = zero;
    let mut t1 = [zero; 3];
    let mut t2 = [zero; 3];
    let mut tp = [zero; 3];
    let mut last_plane = 0;
    let mut last_abs_z_low = 0;
    for j in 0..fc {
        let src = fp + j as u32 * 32;
        let dst = temp_f + j as u32 * 36;
        let ids: [usize; 3] = std::array::from_fn(|a| m.r32(src + 4 + a as u32 * 4) as usize);
        if ids.iter().any(|&i| i >= vc) {
            return Err("NativeMeshInertiaBoundary: triangle outside vertex array".into());
        }
        let n: [F; 3] = std::array::from_fn(|a| F::bits(m.r32(src + 20 + a as u32 * 4)));
        last_abs_z_low = (f32::from_bits(n[2].word()).abs() as f64).to_bits() as u32;
        let v = vertices[ids[0]];
        let w = (-n[0] * v[0] - v[1] * n[1] - v[2] * n[2]).single();
        last_plane = w.word();
        m.w32(dst, 3);
        for a in 0..3 {
            m.w32(dst + 4 + a as u32 * 4, n[a].word());
            m.w32(dst + 20 + a as u32 * 4, ids[a] as u32);
        }
        m.w32(dst + 16, w.word());
        let abs = n.map(|v| f32::from_bits(v.word()).abs());
        let c = if abs[0] > abs[1] && abs[0] > abs[2] {
            0
        } else if abs[1] > abs[2] {
            1
        } else {
            2
        };
        let a = (c + 1) % 3;
        let b = (a + 1) % 3;
        let f = face(&vertices, ids, n, w, a, b, c);
        let first = if a == 0 {
            f.first[0]
        } else if b == 0 {
            f.first[1]
        } else {
            f.first[2]
        };
        volume = (n[0] * first + volume).double();
        for (axis, slot) in [a, b, c].into_iter().zip(0..3) {
            t1[axis] = (n[axis] * f.second[slot] + t1[axis]).double();
            t2[axis] = (n[axis] * f.third[slot] + t2[axis]).double();
            tp[axis] = (n[axis] * f.product[slot] + tp[axis]).double();
        }
    }
    t1 = t1.map(|v| (v * F::d(0.5)).double());
    t2 = t2.map(|v| (v * F::d(1. / 3.)).double());
    tp = tp.map(|v| (v * F::d(0.5)).double());
    let centre = t1.map(|v| v / volume);
    let [x, y, z] = centre;
    let density = (F::d(1.) / volume).double();
    let zz = (z * z).double();
    let yy = (y * y).double();
    let xx = ((t2[2] + t2[1]) * density - (y * y + zz)).double();
    let iy = ((t2[0] + t2[2]) * density).double() - (x * x + zz);
    let iz = ((t2[0] + t2[1]) * density).double() - (x * x + yy);
    let xy = y * x + (-(tp[0] * density)).double();
    let yz = z * y + (-(tp[1] * density)).double();
    let xz = z.double() * x + (-(tp[2] * density)).double();
    let tensor = [xx, xy, xz, xy, iy, yz, xz, yz, iz].map(F::single);
    let inv = inverse(tensor);
    // 54e3b0 copies all twelve matrix words, including its three stack gaps.
    // At the first inverse these slots retain the last 4a9c80 plane store,
    // 4a9fe0 absolute-normal double low word, and completed face byte offset.
    // At the second inverse, 4aa330's tensor stores occupy the same locations.
    for (offset, value) in [
        (0x24, last_plane),
        (0x34, last_abs_z_low),
        (0x44, fc as u32 * 36),
        (0x54, tensor[2].word()),
        (0x64, tensor[5].word()),
        (0x74, tensor[8].word()),
    ] {
        m.w32(body + offset, value);
    }
    for r in 0..3 {
        for c in 0..3 {
            m.w32(
                body + 0x18 + r * 16 + c * 4,
                inv[(r * 3 + c) as usize].word(),
            );
        }
    }
    for (v, point) in vertices.iter().enumerate() {
        for a in 0..3 {
            m.w32(
                vp + v as u32 * 12 + a as u32 * 4,
                (point[a] - centre[a].single()).word(),
            );
        }
    }
    for (a, value) in centre.iter().enumerate() {
        m.w32(definition + 0x40 + a as u32 * 4, value.word());
    }
    m.w32(definition + 0x4c, volume.word());
    Ok(())
}
/// Original4a6010 tail: scale inverse unit inertia by inverse mass, then invert.
pub fn scale_mass(m: &mut Space, body: u32) {
    let mass = F::bits(m.r32(body + 0x14));
    let values: [F; 9] = std::array::from_fn(|i| {
        (F::bits(m.r32(body + 0x18 + (i / 3) as u32 * 16 + (i % 3) as u32 * 4)) * mass).single()
    });
    let inv = inverse(values);
    for r in 0..3 {
        for c in 0..3 {
            m.w32(
                body + 0x18 + r * 16 + c * 4,
                values[(r * 3 + c) as usize].word(),
            );
            m.w32(
                body + 0x48 + r * 16 + c * 4,
                inv[(r * 3 + c) as usize].word(),
            );
        }
    }
}

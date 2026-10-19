//! `java.lang.Vector3` and `java.lang.Ypr`: the value model behind the eight
//! vector/yaw-pitch-roll natives (M1-07, issue #34).
//!
//! Both script classes are plain triples of `f32` fields, read and written by
//! name through the exe's field helpers (`0x42a560` get-float, `0x42a040`
//! set-float). The names are the interned cstrings the natives pass, so the
//! constants below are the only spelling accepted by [`Vec3::field`] and
//! [`Ypr::field`]:
//!
//! | class | fields | exe cstrings (natives pass these) |
//! |---|---|---|
//! | `java.lang.Vector3` | [`X`], [`Y`], [`Z`] | `0x6133b8`, `0x6133ac`, `0x6133c0` |
//! | `java.lang.Ypr` | [`Y`], [`P`], [`R`] | `0x6133c4`, `0x6133c8`, `0x6133cc` |
//!
//! The exe's lookup matches the name cstring, so no field order is observable
//! from the natives; [`Vec3::FIELD_NAMES`]/[`Ypr::FIELD_NAMES`] are only the
//! order this crate reads and writes them in.
//!
//! The eight registered natives are all ported here: the five readers
//! ([`Vec3::length`], [`Vec3::distance`], [`Vec3::normalize`],
//! [`Vec3::rotate`], [`Vec3::rotate_axis`]), the vector constructor
//! [`Vec3::from_ypr`] (`Vector3.<init>(Ypr)`), and the two yaw-pitch-roll
//! entry points [`Vec3::to_ypr`] (`Ypr.<init>(Vector3)`) and
//! [`Ypr::mul`]'s [`Mat3::premul`]/[`Mat3::to_ypr`] composition.
//! [`NATIVES`] is the registry source of truth and [`install`] registers all
//! eight impl addresses.
//!
//! Their input is a
//! `java.lang.Vector3`/`java.lang.Ypr` **object**, so the port needs the
//! object model rather than the per-thread language state: the ABI entries
//! [`length`], [`distance`], [`normalize`], [`rotate`], [`rotate_axis`],
//! [`init_from_ypr`], [`ypr_init_from_vec3`] and [`ypr_mul`] read the
//! receiver's (and the declared argument's) fields by
//! name out of
//! [`VmHost::objects`] (the exe's `get_float_field`, `0x42a560`; the writing
//! natives go back through `set_float_field`, `0x42a040`) and [`install`]
//! registers them at their exe addresses, the `record+8` slots
//! `VmHost::invoke_native` reads.
//!
//! `rotate` is the first port that needs the shared 3×3 helper the exe keeps
//! outside the natives ([`Mat3::from_ypr`], `0x54ecd0`); the axis-angle
//! `rotate` adds the second one ([`Mat3::from_axis_angle`], `0x54ee80`).
//! `Ypr.<init>(Vector3)` adds the vec→ypr inverse
//! ([`Vec3::to_ypr`], `0x551d20`) and `Ypr.mul` the matrix multiply
//! ([`Mat3::premul`], `0x54eab0`) plus the mat3→ypr inverse
//! ([`Mat3::to_ypr`], `0x551c90`). `Vector3.<init>(Ypr)` reuses `0x54ecd0`,
//! reading its third column and negating it.

use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};
use slr_vm::values::Value;
use slr_x87::trans;
use slr_x87::{Precision, Rounding, X87};

use crate::seam::{NativeBinding, NativeResult};

/// Field name `"x"` (`0x6133b8`, `0x6133dc`, `0x613400`, ...).
pub const X: &str = "x";
/// Field name `"y"` (`0x6133ac`, `0x6133c4`, `0x6133e0`, ...).
pub const Y: &str = "y";
/// Field name `"z"` (`0x6133c0`, `0x6133d0`, `0x6133e4`, ...).
pub const Z: &str = "z";
/// Field name `"p"` (`0x6133b0`, `0x6133c8`, `0x613458`, `0x613464`, ...).
pub const P: &str = "p";
/// Field name `"r"` (`0x6133b4`, `0x6133cc`, `0x61345c`, `0x613468`, ...).
pub const R: &str = "r";

/// Script class name used by the native registry.
pub const VECTOR3: &str = "java.lang.Vector3";
/// Script class name used by the native registry.
pub const YPR: &str = "java.lang.Ypr";

/// `java.lang.Vector3`: a 3-component `f32` vector.
///
/// Field order is the class's own name set (`x`, `y`, `z`); the exe's field
/// lookup walks the container by name, so container order is not observable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec3 {
    /// `x` component.
    pub x: f32,
    /// `y` component.
    pub y: f32,
    /// `z` component.
    pub z: f32,
}

/// `java.lang.Ypr`: yaw/pitch/roll angles in radians.
///
/// Field order is the class's own name set (`y`, `p`, `r`); the exe's field
/// lookup walks the container by name, so container order is not observable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ypr {
    /// `y` (yaw) angle.
    pub y: f32,
    /// `p` (pitch) angle.
    pub p: f32,
    /// `r` (roll) angle.
    pub r: f32,
}

impl Vec3 {
    /// The field names this crate reads and writes, in the order used by
    /// [`Self::field`] and [`Self::set_field`].
    pub const FIELD_NAMES: [&'static str; 3] = [X, Y, Z];

    /// A vector with the given components.
    #[must_use]
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// The zero vector.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Position of `name` in [`Self::FIELD_NAMES`], or `None` for a name the
    /// exe's `get_float_field` would miss (it logs and returns `+0.0`).
    #[must_use]
    pub fn field_index(name: &str) -> Option<usize> {
        Self::FIELD_NAMES.iter().position(|f| *f == name)
    }

    /// Read one field by its exe cstring name, or `None` on a miss.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<f32> {
        Self::field_index(name).map(|i| match i {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        })
    }

    /// Write one field by its exe cstring name. Returns `false` and leaves the
    /// vector untouched on a miss (the exe's `set_float_field` logs instead).
    pub fn set_field(&mut self, name: &str, value: f32) -> bool {
        match Self::field_index(name) {
            Some(0) => self.x = value,
            Some(1) => self.y = value,
            Some(2) => self.z = value,
            _ => return false,
        }
        true
    }

    /// The components in declaration order.
    #[must_use]
    pub fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    /// `java.lang.Vector3.length()F`: the native at `0x004823e0`, which
    /// returns `sqrt((z*z + y*y) + x*x)` evaluated in this exact x87 order:
    ///
    /// ```text
    /// 4823f6  push 0x61340c ; "x"
    /// 482401  fstp [esp+0x14]   ; x -> the frame's x slot
    /// 482409  push 0x613410 ; "y"
    /// 482414  fstp [esp+0x18]   ; y -> the frame's y slot
    /// 48241c  push 0x613414 ; "z"   (each via call 0x42a560)
    /// 482427  fld  st(0)        ; z is still st0 from the third read: st1 = z
    /// 482429  fmulp st(1)       ; st1 = z*z, pop -> st0 = z*z
    /// 48242b  fld  [esp+0x20]   ; y
    /// 48242f  fmul [esp+0x20]   ; st0 = y*y
    /// 482433  faddp st(1)       ; st0 = z*z + y*y
    /// 482435  fld  [esp+0x24]   ; x
    /// 482439  fmul [esp+0x24]   ; st0 = x*x
    /// 48243d  faddp st(1)       ; st0 = (z*z + y*y) + x*x
    /// 48243f  fsqrt             ; st0 = sqrt(...)
    /// ```
    ///
    /// Every step rounds to the game's 24-bit precision
    /// ([`Precision::F32`], nearest-even), so `x*x` is added **last** — the
    /// partial sum `z*z + y*y` is rounded before the third term lands. That
    /// is observable and pinned by
    /// [`tests::length_adds_x_squared_last_at_24_bit_precision`]; term order
    /// in the first add is not observable (IEEE addition is commutative).
    #[must_use]
    pub fn length(self) -> f32 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // `fld st(0); fmulp st(1)` — `z` against itself.
        let z = X87::from_f32(self.z);
        let mut sum = z.mul(z, pc, rc);
        // `fld [y]; fmul [y]; faddp st(1)`.
        let y = X87::from_f32(self.y);
        sum = sum.add(y.mul(y, pc, rc), pc, rc);
        // `fld [x]; fmul [x]; faddp st(1)`.
        let x = X87::from_f32(self.x);
        sum = sum.add(x.mul(x, pc, rc), pc, rc);
        // `fsqrt`.
        sum.sqrt(pc, rc).to_f32(pc, rc)
    }

    /// `java.lang.Vector3.distance(Ljava.lang.Vector3;)F`: the native at
    /// `0x00482550`, which returns `sqrt(dz*dz + (dx*dx + dy*dy))` evaluated
    /// in this exact x87 order:
    ///
    /// ```text
    /// 482567  mov eax,[esp+0x2c]   ; the binder's argument-area word
    /// 48256b  push 0x613430 ; "x"  (then 0x613434 "y", 0x613438 "z"):
    /// 482576  fstp [esp+0x18]      ; arg.x
    /// 482589  fstp [esp+0x24]      ; arg.y
    /// 48259c  fstp [esp+0x30]      ; arg.z
    /// 4825a0  mov eax,[esp+0x24]   ; the binder's out[0], the receiver
    /// 4825a4  push 0x61343c ; "x"  (then 0x613440 "y", 0x613444 "z"):
    /// 4825af  fstp [esp+0x3c]      ; this.x
    /// 4825c2  fstp [esp+0x48]      ; this.y
    /// 4825d5  fsub <arg.z slot>    ; st0 = this.z - arg.z      (6th read)
    /// 4825d9  fld  <this.y slot>
    /// 4825dd  fsub <arg.y slot>    ; st0 = this.y - arg.y
    /// 4825e1  fld  <this.x slot>
    /// 4825e5  fsub <arg.x slot>    ; st0 = dx (st1 = dy, st2 = dz)
    /// 4825e9  fld  st(0)           ; duplicate dx
    /// 4825eb  fmul st(1)           ; st0 = dx*dx
    /// 4825ed  fld  st(2)           ; dy
    /// 4825ef  fmul st(3)           ; st0 = dy*dy
    /// 4825f1  faddp st(1)          ; st0 = dy*dy + dx*dx
    /// 4825f3  fld  st(3)           ; dz
    /// 4825f5  fmul st(4)           ; st0 = dz*dz
    /// 4825f7  faddp st(1)          ; st0 = dz*dz + (dx*dx + dy*dy)
    /// 4825f9  fsqrt
    /// 4825fb  fstp st(3) ; 4825fd fstp st(0) ; 4825ff fstp st(0)
    /// ```
    ///
    /// The operand text in the listing is `[esp+...]`-style and shifts by 4
    /// per push — these natives never pop their own argument words until the
    /// closing `add esp,0x58` — so an instruction that reads back a slot an
    /// earlier instruction stored carries the sum of both depths, and its raw
    /// text names neither slot. Each operand above is annotated with the
    /// *value* it resolves to instead.
    ///
    /// Differences: the **argument** is read first (`0x613430`/`34`/`38`,
    /// out of the binder's argument-area word) and the **receiver** second
    /// (`0x61343c`/`40`/`44`, out of `out[0]`), and each difference is
    /// `receiver - argument` (probe-verified read order; the handoff table's
    /// "distance this"/"distance arg" labels are swapped). Which of the two
    /// is subtracted from which is not observable after squaring, nor is the
    /// order inside an add (IEEE addition is commutative).
    ///
    /// What *is* observable is the accumulation: every step rounds to the
    /// game's 24-bit precision ([`Precision::F32`], nearest-even), so the
    /// partial sum `dx*dx + dy*dy` is rounded before `dz*dz` lands — the
    /// opposite of [`Self::length`], where `x*x` is the term added last. The
    /// triple `(2^-20, 1.25*2^-20, 2^-8)` shows it: the exe's `length`
    /// returns `0x3b800001` and its `distance` from the origin
    /// `0x3b800000`, and neither function's bits come out of the other's
    /// order ([`tests::distance_adds_z_squared_last_at_24_bit_precision`]).
    #[must_use]
    pub fn distance(self, other: Vec3) -> f32 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // Three `fsub`s, in the native's own operand order.
        let dx = X87::from_f32(self.x).sub(X87::from_f32(other.x), pc, rc);
        let dy = X87::from_f32(self.y).sub(X87::from_f32(other.y), pc, rc);
        let dz = X87::from_f32(self.z).sub(X87::from_f32(other.z), pc, rc);
        // `fld/fmul` pairs plus the first `faddp`: dx*dx + dy*dy.
        let sum = dx.mul(dx, pc, rc).add(dy.mul(dy, pc, rc), pc, rc);
        // The second `faddp`: dz*dz added to the rounded partial sum.
        sum.add(dz.mul(dz, pc, rc), pc, rc)
            .sqrt(pc, rc)
            .to_f32(pc, rc)
    }

    /// `java.lang.Vector3.normalize()V`: the native at `0x00482450`, which
    /// turns the receiver into the unit vector of its own components and
    /// returns the triple the native leaves behind (the receiver's
    /// components unchanged when the guard skips).
    ///
    /// Field reads, in the native's own order — `z` at `0x613418`, `y` at
    /// `0x61341c`, `x` at `0x613420`, all through `get_float_field`
    /// (`0x42a560`):
    ///
    /// ```text
    /// 482453  mov ecx,[esp+0x18]  ; the CallInfo word (0x48245d call 0x45d910)
    /// 482462  mov edx,[esp+0x20]  ; the binder's out[0], the receiver
    /// 482466  push 0x613418 ; "z"   (0x48246c call 0x42a560)
    /// 482471  fstp [esp+0x14]       ; z -> the frame's z slot
    /// 482479  push 0x61341c ; "y"
    /// 482484  fstp [esp+0x18]       ; y -> the frame's y slot
    /// 48248c  push 0x613420 ; "x"
    /// 482497  fstp [esp+0x28]       ; x -> the frame's x slot
    /// ```
    ///
    /// The length, in this exact x87 order — `y*y` first, then `z*z`, then
    /// `x*x` (unlike [`Self::length`], which squares `z` first):
    ///
    /// ```text
    /// 48249b  fld  <z slot>
    /// 4824a3  fld  <y slot>
    /// 4824a7  fld  st(0)
    /// 4824a9  fmul st(1)     ; st0 = y*y
    /// 4824af  fld  st(2)     ; z
    /// 4824b5  fmul st(3)     ; st0 = z*z
    /// 4824be  faddp st(1)    ; st0 = y*y + z*z
    /// 4824c0  fld  <x slot>
    /// 4824c4  fmul <x slot>  ; st0 = x*x
    /// 4824c8  faddp st(1)    ; st0 = (y*y + z*z) + x*x
    /// 4824ca  fsqrt          ; st0 = len
    /// ```
    ///
    /// The two first addends are swapped against [`Self::length`], which is
    /// the only difference in the sum: IEEE addition is commutative, so the
    /// extended `len` is bit-identical to that native's, and the shared test
    /// triple `(2^-20, 1.25*2^-20, 2^-8)` gives both `0x3b800001`.
    ///
    /// The guard then tests **the extended `len`** — `fcom [0x5e73cc]`
    /// (`0.0f`) + `test ah,0x40`, i.e. C3 — so `0x482501` is taken for
    /// `+0.0`, `-0.0` **and** an unordered (NaN) compare, and the three
    /// original components are written back unscaled. Otherwise the divisor
    /// is that same extended value (`0x4824dd fdivr [0x5f08f0]`, `1.0f`), not
    /// its f32 rounding, and each component is scaled with `fmul` at 24-bit
    /// precision:
    ///
    /// ```text
    /// 4824d0  fcom [0x5e73cc]     ; 0.0f
    /// 4824d6  fnstsw ax
    /// 4824d8  test ah,0x40        ; C3 set = equal or unordered
    /// 4824db  jne 0x482501        ; -> skip: 482501 fstp st(0), write as read
    /// 4824dd  fdivr [0x5f08f0]    ; st0 = 1.0 / len
    /// 4824e3  fld st(0) ; 4824e5 fmul <x> ; 4824e9 fstp <x out>
    /// 4824ed  fld st(0) ; 4824ef fmul <y> ; 4824f3 fstp <y out>
    /// 4824f7  fmul <z>            ; 4824fb fstp <z out>
    /// 482503  set_float_field(receiver, "x" 0x613424, <x out>)
    /// 482517  set_float_field(receiver, "y" 0x613428, <y out>)
    /// 48252b  set_float_field(receiver, "z" 0x61342c, <z out>)
    /// ```
    ///
    /// Two consequences of the extended divisor are observable and pinned by
    /// [`tests::normalize_divides_by_the_extended_length`]: a length that
    /// overflows `f32` still divides (`(3e38, 3e38, 3e38)` becomes
    /// `0x3f13cd3a` per component, not `0`, which is what dividing by
    /// `f32::INFINITY` gives), and a component far below the `f32` normal
    /// range (`2^-149`) still normalizes exactly to `(1, 0, 0)` because the
    /// reciprocal is taken on the 80-bit value — the same reciprocal in `f32`
    /// is `+inf`.
    #[must_use]
    pub fn normalize(self) -> Vec3 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // `fld z; fld y; fld st(0); fmul st(1)` — `y` squared first.
        let y = X87::from_f32(self.y);
        let mut len = y.mul(y, pc, rc);
        // `fld st(2); fmul st(3); faddp st(1)` — `z*z` added to `y*y`.
        let z = X87::from_f32(self.z);
        len = len.add(z.mul(z, pc, rc), pc, rc);
        // `fld x; fmul x; faddp st(1)`.
        let x = X87::from_f32(self.x);
        len = len.add(x.mul(x, pc, rc), pc, rc);
        // `fsqrt`.
        let len = len.sqrt(pc, rc);
        // `fcom [0x5e73cc]; fnstsw ax; test ah,0x40; jne` — C3 is set for an
        // equal **or unordered** compare, and that arm writes the components
        // back exactly as they were read.
        if len.fcom(X87::from_f32(0.0)).0.c3 {
            return self;
        }
        // `fdivr [0x5f08f0]` — `1.0 / len`, on the 80-bit value.
        let inv = X87::from_f32(1.0).div(len, pc, rc);
        // `fmul <component>; fstp dword` — x, then y, then z.
        let scale = |c: f32| X87::from_f32(c).mul(inv, pc, rc).to_f32(pc, rc);
        Vec3::new(scale(self.x), scale(self.y), scale(self.z))
    }

    /// `java.lang.Vector3.rotate(Ljava.lang.Ypr;)Ljava.lang.Vector3;`: the
    /// native at `0x00482110`, which turns the receiver into the receiver
    /// rotated by the argument angles and hands that same object back.
    ///
    /// The matrix is the exe's own ypr→3×3 helper ([`Mat3::from_ypr`],
    /// `0x54ecd0`), fed the argument triple through `0x551c70` in the read
    /// order `y` (`0x6133c4`), `p` (`0x6133c8`), `r` (`0x6133cc`). The three
    /// outputs are that matrix's rows dotted with the receiver triple, each
    /// row with **its own accumulation order** (`0x4821cb`-`0x482231`): row 0
    /// accumulates `x`, then `y`, then `z`, while rows 1 and 2 multiply `x`
    /// **last**. The listing below names the entries of
    ///
    /// ```text
    ///        | m0 m1 m2 |
    /// M  =   | m3 m4 m5 |
    ///        | m6 m7 m8 |
    /// ```
    ///
    /// in the order the FPU actually touches them:
    ///
    /// ```text
    /// 4821cb  fld m0 ; fmul <x>    ; st0 = m0*x
    /// 4821d3  fld m1 ; fmul <y>    ; 4821db faddp st(1)  ; m0*x + m1*y
    /// 4821dd  fld m2 ; fmul <z>    ; 4821e5 faddp st(1)  ; + m2*z   -> [x out]
    /// 4821ef  fld m4 ; fmul <y>    ; st0 = m4*y
    /// 4821fd  fld m5 ; fmul <z>    ; 482205 faddp st(1)  ; m4*y + m5*z
    /// 482207  fld m3 ; fmul <x>    ; 48220f faddp st(1)  ; + m3*x   -> [y out]
    /// 482215  fld m7 ; fmul <y>    ; st0 = m7*y
    /// 48221d  fld m8 ; fmul <z>    ; 482225 faddp st(1)  ; m7*y + m8*z
    /// 482227  fld m6 ; fmul <x>    ; 48222f faddp st(1)  ; + m6*x   -> [z out]
    /// ```
    ///
    /// which is `M * v` with `v = (x, y, z)`: rows 1 and 2 add the `m*x`
    /// term **last** and row 0 first. Every multiply and every add rounds to
    /// the game's 24-bit precision ([`Precision::F32`], nearest-even), so the
    /// grouping is observable; the order *within* one add is not (IEEE
    /// addition is commutative), and each term is a single `fmul`, so no
    /// term's grouping is in doubt.
    ///
    /// The result is the receiver object with `x` (`0x6133dc`), `y`
    /// (`0x6133e0`), `z` (`0x6133e4`) written back in that order through
    /// `set_float_field` (`0x42a040`), and the native returns that same
    /// object word (`[esp+0x6c]`, the `out[0]` payload) in `EAX` — `rotate`
    /// is in place, the receiver cell *is* the return cell.
    ///
    /// `fsin`/`fcos` inside [`Mat3::from_ypr`] are **APPROX**
    /// (`docs/x87.md`), so no bit-equality claim is made for a nonzero angle;
    /// the tolerance is the one [`slr_x87::trans`] documents.
    #[must_use]
    pub fn rotate(self, ypr: Ypr) -> Vec3 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let m = Mat3::from_ypr(ypr);
        // `fld dword [esp+...]` — the receiver's components are read by name
        // and held as `f32` locals before the dot products (the exe reads
        // them `z`, `y`, `x`; all three reads are independent, so only the
        // values matter here).
        let (x, y, z) = (
            X87::from_f32(self.x),
            X87::from_f32(self.y),
            X87::from_f32(self.z),
        );
        let entry = |i: usize| X87::from_f32(m.m[i]);
        // `fld m0; fmul x; fld m1; fmul y; faddp; fld m2; fmul z; faddp`.
        let row0 = entry(0)
            .mul(x, pc, rc)
            .add(entry(1).mul(y, pc, rc), pc, rc)
            .add(entry(2).mul(z, pc, rc), pc, rc);
        // `fld m4; fmul y; fld m5; fmul z; faddp; fld m3; fmul x; faddp`.
        let row1 = entry(4)
            .mul(y, pc, rc)
            .add(entry(5).mul(z, pc, rc), pc, rc)
            .add(entry(3).mul(x, pc, rc), pc, rc);
        // `fld m7; fmul y; fld m8; fmul z; faddp; fld m6; fmul x; faddp`.
        let row2 = entry(7)
            .mul(y, pc, rc)
            .add(entry(8).mul(z, pc, rc), pc, rc)
            .add(entry(6).mul(x, pc, rc), pc, rc);
        // `fstp dword [esp+...]` — one rounding per component, as the stores
        // do before each `set_float_field` call.
        Vec3::new(
            row0.to_f32(pc, rc),
            row1.to_f32(pc, rc),
            row2.to_f32(pc, rc),
        )
    }

    /// `java.lang.Vector3.rotate(Ljava.lang.Vector3;F)Ljava.lang.Vector3;`:
    /// the native at `0x00482280`, which turns the receiver into the receiver
    /// rotated by `angle` radians about the declared axis vector, and hands
    /// that same object back.
    ///
    /// The native reads the **receiver's** `z` (`0x6133e8`), `y`
    /// (`0x6133ec`), `x` (`0x6133f0`) out of `container[count-1]` first, and
    /// the declared **axis** object's `z` (`0x6133f4`), `y` (`0x6133f8`),
    /// `x` (`0x6133fc`) out of `container[count-2]` second (those six reads
    /// are at `0x4822a6`-`0x482308`); the `F` payload word
    /// (`container[count-3]`) is the angle. Each read is an independent
    /// `get_float_field` by name, so the port's three `x`, `y`, `z` reads
    /// out of each object see exactly those twelve fields
    /// ([`components`]); the read *order* is only observable through the
    /// exe's miss diagnostics, which the port does not model.
    ///
    /// The three fields the axis reads leave in `x`, `y`, `z` order go into
    /// [`Mat3::from_axis_angle`] (`0x54ee80`, the Rodrigues helper, which
    /// normalises the axis when `|len² - 1| > 1e-6f`) with the angle, and the
    /// receiver triple is dotted with that matrix's rows (`0x482331`-
    /// `0x482392`), each row in **its own** accumulation order — the same
    /// structure as [`Vec3::rotate`], with one difference in row 0:
    ///
    /// ```text
    /// 482331  fld m1 ; fmul y    ; st0 = m1*y
    /// 482339  fld m0 ; fmul x    ; 482341 faddp st(1)  ; m1*y + m0*x
    /// 482343  fld m2 ; fmul z    ; 48234b faddp st(1)  ; + m2*z   -> [x out]
    /// 482355  fld m4 ; fmul y    ; 482366 faddp st(1)  ; m4*y + m5*z
    /// 482368  fld m3 ; fmul x    ; 482370 faddp st(1)  ; + m3*x   -> [y out]
    /// 482376  fld m7 ; fmul y    ; 482386 faddp st(1)  ; m7*y + m8*z
    /// 482388  fld m6 ; fmul x    ; 482390 faddp st(1)  ; + m6*x   -> [z out]
    /// ```
    ///
    /// `rotate(Ypr)` pushes `m0*x` before `m1*y` in its row 0 where this
    /// native pushes `m1*y` first; IEEE addition is commutative, so the two
    /// round to the same sum (only a NaN payload could differ) and the
    /// operand order is kept as the exe has it. Every multiply and every add
    /// rounds to the game's 24-bit precision ([`Precision::F32`],
    /// nearest-even), and the `fld m1`-first grouping of row 0 is
    /// load-bearing: the three rows do not share an accumulation order.
    ///
    /// The result is the receiver object with `x` (`0x613400`), `y`
    /// (`0x613404`), `z` (`0x613408`) written back in that order through
    /// `set_float_field` (`0x4823a0`/`0x4823b4`/`0x4823c8`), and the native
    /// returns that same object word (its `out[0]` payload, `EAX` at
    /// `0x4823cd`) — like `rotate`, it is in place: the receiver cell *is*
    /// the return cell.
    ///
    /// `fsin`/`fcos` inside [`Mat3::from_axis_angle`] are **APPROX**
    /// (`docs/x87.md`), so no bit-equality claim is made for a nonzero angle;
    /// the tolerance is the one [`slr_x87::trans`] documents.
    #[must_use]
    pub fn rotate_axis(self, axis: Vec3, angle: f32) -> Vec3 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let m = Mat3::from_axis_angle(axis, angle);
        // `fld dword [esp+...]` — the receiver's components, read by name
        // (the exe reads them `z`, `y`, `x`; all three reads are
        // independent, so only the values matter here).
        let (x, y, z) = (
            X87::from_f32(self.x),
            X87::from_f32(self.y),
            X87::from_f32(self.z),
        );
        let entry = |i: usize| X87::from_f32(m.m[i]);
        // `fld m1; fmul y; fld m0; fmul x; faddp; fld m2; fmul z; faddp`.
        let row0 = entry(1)
            .mul(y, pc, rc)
            .add(entry(0).mul(x, pc, rc), pc, rc)
            .add(entry(2).mul(z, pc, rc), pc, rc);
        // `fld m4; fmul y; fld m5; fmul z; faddp; fld m3; fmul x; faddp`.
        let row1 = entry(4)
            .mul(y, pc, rc)
            .add(entry(5).mul(z, pc, rc), pc, rc)
            .add(entry(3).mul(x, pc, rc), pc, rc);
        // `fld m7; fmul y; fld m8; fmul z; faddp; fld m6; fmul x; faddp`.
        let row2 = entry(7)
            .mul(y, pc, rc)
            .add(entry(8).mul(z, pc, rc), pc, rc)
            .add(entry(6).mul(x, pc, rc), pc, rc);
        // `fstp dword [esp+...]` — one rounding per component, as the stores
        // do before each `set_float_field` call.
        Vec3::new(
            row0.to_f32(pc, rc),
            row1.to_f32(pc, rc),
            row2.to_f32(pc, rc),
        )
    }

    /// `java.lang.Vector3.<init>(Ljava.lang.Ypr;)`: the native at
    /// `0x00482020`, whose effect is the direction vector the `Ypr` argument
    /// names.
    ///
    /// The native reads the **argument's** `y` (`0x6133ac`), `p`
    /// (`0x6133b0`), `r` (`0x6133b4`) out of `container[count-2]` — three
    /// `get_float_field` calls at `0x482041`/`0x482054`/`0x482067`, each
    /// re-reading the same word — copies the triple into a local with the
    /// inlined `0x551c70` (`0x482086`; the port keeps it as one [`Ypr`]), and
    /// hands it to [`Mat3::from_ypr`] (`0x54ecd0`, `0x482094`). It then loads
    /// the matrix's **third column**, `fmul`s each entry by `DAT_005f0c70` =
    /// `-1.0f` (`0xbf800000`), `fstp dword`s it to a local and writes the
    /// triple into `this`:
    ///
    /// ```text
    /// 482099  fld [esp+0x24]     ; matrix+8    = m2 = cp*sy   (0x54ed3a)
    /// 48209d  fmul [0x5f0c70]    ; * -1.0f
    /// 4820a7  fstp [esp+0x10]
    /// 4820d2  set_float_field(this, "x"/0x6133b8, -m2)
    /// 4820af  fld [esp+0x34]     ; matrix+0x18 = m5 = -sp     (0x54ed55)
    /// 4820b4  fmul [0x5f0c70]
    /// 4820c0  fstp [esp+0x14]
    /// 4820e6  set_float_field(this, "y"/0x6133bc, -m5)
    /// 4820c4  fld [esp+0x50]     ; matrix+0x28 = m8 = cp*cy   (0x54ed88)
    /// 4820c8  fmul [0x5f0c70]
    /// 4820ce  fstp [esp+0x18]
    /// 4820fa  set_float_field(this, "z"/0x6133c0, -m8)
    /// ```
    ///
    /// so the result is `x = -cos(p)*sin(y)`, `y = sin(p)` (the matrix's `m5`
    /// is already `-sin(p)` before the negation) and `z = -cos(p)*cos(y)`.
    /// The roll angle reaches only `m0`, `m1`, `m3`, `m4`, `m6` and `m7` —
    /// the first two columns — so it cannot change this vector.
    ///
    /// Every matrix entry was `fstp dword`ed by `0x54ecd0`, and the negation
    /// reloads those `f32` slots (`fld dword`), so the only arithmetic this
    /// native adds is one `fmul` by `-1.0f` and one `fstp dword` per
    /// component: a plain sign flip, exact for every finite component.
    ///
    /// `fsin`/`fcos` inside [`Mat3::from_ypr`] are **APPROX**
    /// (`docs/x87.md`), so no bit-equality claim is made for a nonzero angle;
    /// the zero-angle row is bit-exact.
    #[must_use]
    pub fn from_ypr(ypr: Ypr) -> Vec3 {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let m = Mat3::from_ypr(ypr);
        // `fld dword [matrix+n]; fmul [0x5f0c70]; fstp dword [esp+n]`.
        let negated = |entry: f32| {
            X87::from_f32(entry)
                .mul(X87::from_f32(-1.0), pc, rc)
                .to_f32(pc, rc)
        };
        Vec3::new(negated(m.m[2]), negated(m.m[5]), negated(m.m[8]))
    }

    /// The vec→ypr helper `0x551d20`, the inverse of the direction
    /// [`Vec3::from_ypr`] names: the `(y, p, r)` triple
    /// `Ypr.<init>(Vector3)` (`0x482610`) writes.
    ///
    /// Its one stack argument is a pointer to the triple it reads
    /// (`fld [edx]`, `[edx+4]`, `[edx+8]` = `x`, `y`, `z`) and its `ecx` is
    /// the destination (three `f32`s at `[ecx]`, `[ecx+4]`, `[ecx+8]` =
    /// `y`, `p`, `r`). The exe first checks the degenerate `x = z = 0` case:
    ///
    /// ```text
    /// 551d24  fld [edx+8] z ; fcomp [0x5f0c18] (0.0) ; test ah,0x40 ; je general
    /// 551d34  fld [edx]   x ; fcomp [0x5f0c18] (0.0) ; test ah,0x40 ; je general
    /// 551d43  [ecx]   = 0                      ; y = 0
    /// 551d49  fld [edx+4] y ; fcomp [0x5e73cc] (0.0f)
    /// 551d54  test ah,0x41 ; jne -pi/2 ; else +pi/2     ; p, then [ecx+8] = 0
    /// ```
    ///
    /// so only a strictly positive `y` takes `+pi/2`; `y <= 0` **and NaN**
    /// take `-pi/2`. The general path computes `atan2(-x, -z)` (`fld x; fchs;
    /// fld z; fchs; fpatan`, `0x551d7f`-`0x551d8a`), then
    /// `atan2(y, sqrt(x*x + z*z))` — the sum added `x*x` first, then `z*z`, and
    /// `fsqrt` last (`0x551d8c`-`0x551db3`) — and always writes `r = 0`.
    ///
    /// `fpatan` is APPROX ([`slr_x87::trans`]), so no bit-equality claim is
    /// made beyond the degenerate branch (which writes constants).
    ///
    /// exe: FUN_00551d20
    #[must_use]
    pub fn to_ypr(self) -> Ypr {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // `fcomp qword [0x5f0c18]` is an equality test against `±0.0`; a NaN
        // falls through to the general path.
        if self.z == 0.0 && self.x == 0.0 {
            // `test ah,0x41` (C0|C3) jumps to `-pi/2` for `y <= 0` or NaN.
            let p = if self.y > 0.0 { HALF_PI } else { NEG_HALF_PI };
            return Ypr::new(0.0, p, 0.0);
        }
        // `fld x; fchs; fld z; fchs; fpatan` -> atan2(-x, -z).
        let y = trans::approx_patan(fchs(X87::from_f32(self.x)), fchs(X87::from_f32(self.z)));
        // `fld st(1)` (x) `fmul st(2)` (x) ; `fld st(3)` (z) `fmul st(4)` (z);
        // `faddp` -> x*x + z*z ; `fsqrt`.
        let x = X87::from_f32(self.x);
        let z = X87::from_f32(self.z);
        let len = x.mul(x, pc, rc).add(z.mul(z, pc, rc), pc, rc).sqrt(pc, rc);
        // `fpatan` on (y, len) -> atan2(y, len).
        let p = trans::approx_patan(X87::from_f32(self.y), len);
        Ypr::new(y.value.to_f32(pc, rc), p.value.to_f32(pc, rc), 0.0)
    }
}

// ---- the ypr -> 3x3 helper (`0x54ecd0`) ------------------------------

/// `FCHS` on an 80-bit image: flip the sign bit, payloads included.
///
/// The exe negates `sin(p)` in the register (the matrix's `m5`), which is not
/// the same as `0.0 - sin(p)`: `FCHS` turns `+0.0` into `-0.0` while the
/// subtraction gives `+0.0`. `slr-x87` has no `fchs`, so the image is edited
/// directly (bit 15 of the sign/exponent word, the layout `to_bytes`
/// documents).
fn fchs(v: X87) -> X87 {
    let mut bytes = v.to_bytes();
    bytes[9] ^= 0x80;
    X87::from_bytes(bytes)
}

/// `FABS` on an 80-bit image: clear the sign bit, payloads included.
///
/// Like [`fchs`], `slr-x87` has no `fabs`, so the image is edited directly.
/// `FABS` also clears the sign of a NaN, which clearing bit 15 does.
fn fabs(v: X87) -> X87 {
    let mut bytes = v.to_bytes();
    bytes[9] &= 0x7f;
    X87::from_bytes(bytes)
}

/// `DAT_005f09c0` = `1e-6f` (`0x358637bd`): the epsilon both guards of the
/// axis-angle helper (`0x54eea3`, `0x54eec0`) compare against.
const AXIS_EPSILON: f32 = 1e-6;

/// `DAT_005f0cc4` = `+pi/2` (`0x3fc90fdb`): the pitch the degenerate `x = z =
/// 0` branch of the vec→ypr helper (`0x551d20`) writes for a positive `y`.
///
/// Only [`Vec3::to_ypr`] uses this: [`Mat3::to_ypr`]'s own degenerate branch
/// writes the different immediate [`PI`] (`0x40490fdb`) instead.
const HALF_PI: f32 = f32::from_bits(0x3fc9_0fdb);

/// `DAT_005f3ab0` = `-pi/2` (`0xbfc90fdb`): the pitch the same branch writes
/// for a non-positive (or unordered) `y`.
const NEG_HALF_PI: f32 = f32::from_bits(0xbfc9_0fdb);

/// The immediate `0x40490fdb` the degenerate `m8 = m2 = 0` branch of the
/// mat3→ypr helper writes into its `p` slot (`mov [ecx+4], 0x40490fdb` at
/// `0x551cbb`).
///
/// `0x40490fdb` is **`pi`**, not `pi/2` (that would be `0x3fc90fdb`): the
/// exe writes the full turn here, unlike [`Vec3::to_ypr`]'s own degenerate
/// branch, which loads `DAT_005f0cc4` = `+pi/2`. The oracle row
/// `re/oracle/langvec_vectors.py` (`mat3_to_ypr`) pins it.
const PI: f32 = f32::from_bits(0x4049_0fdb);

/// The 3×3 matrix the exe's `0x54ecd0` builds from a `(y, p, r)` triple.
///
/// In memory the exe keeps the rows `0x10` bytes apart — `m0..m2`, a pad
/// float, `m3..m5`, a pad float, `m6..m8` — and both callers index them by
/// those offsets (`0x54ed19` writes `[ecx]`, `0x54ed31` `[ecx+4]`,
/// `0x54ed3a` `[ecx+8]`, `0x54ed45` `[ecx+0x10]`, ...). Nothing reads a pad
/// word, so the port carries the nine entries row-major and the callers
/// index `3*r + c`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Mat3 {
    /// Row-major entries: row `r`, column `c` is `m[3 * r + c]`.
    pub m: [f32; 9],
}

impl Mat3 {
    /// Build the matrix from a `java.lang.Ypr` triple, exactly as `0x54ecd0`
    /// does.
    ///
    /// The helper's one stack argument is the `(y, p, r)` triple it reads in
    /// order (`fld [eax]`, `[eax+4]`, `[eax+8]`), and its `ecx` is the
    /// destination. Per angle the exe computes `fsin` **before** `fcos`:
    ///
    /// ```text
    /// 54ecd7  fld y ; 54ecd9 fsin          ; sin(y) stays in st1 for the rest
    /// 54ecdb  fld y ; 54ecdd fcos ; fstp [esp+0x18]   ; cos(y) -> local
    /// 54ece3  fld p ; fsin ; 54ece8 fld p ; fcos ; fstp [esp+8]
    /// 54ecf1  fld r ; fsin ; fstp [esp+4]             ; sin(r) -> local
    /// 54ecfa  fld r ; fcos ; fstp [esp]               ; cos(r) -> local
    /// ```
    ///
    /// so `sin(y)` and `sin(p)` live in the register stack while `cos(y)`,
    /// `cos(p)`, `sin(r)` and `cos(r)` pass through `fstp dword` locals —
    /// the port models that with [`X87::from_f32`] around the stored value.
    /// The nine entries then come out as
    ///
    /// ```text
    /// m0 = (sr*sp)*sy + cr*cy          m3 = sr*cp     m6 = (sr*sp)*cy - cr*sy
    /// m1 = (cr*sp)*sy - sr*cy          m4 = cr*cp     m7 = (cr*sp)*cy + sr*sy
    /// m2 = cp*sy                       m5 = -sp       m8 = cp*cy
    /// ```
    ///
    /// with `(sr*sp)` and `(cr*sp)` each rounded once by the `fst dword` to
    /// the `[esp+0xc]`/`[esp+0x10]` locals (`0x54ed09`, `0x54ed21`) and
    /// reloaded (`0x54ed58`, `0x54ed6b`) for the `m6`/`m7` rows, `m5` negated
    /// in the register ([`fchs`]), and every `fmul`/`fadd`/`fsub` rounded to
    /// 24-bit precision.
    ///
    /// `fsin`/`fcos` are APPROX ([`slr_x87::trans`]); the rest is exact.
    ///
    /// exe: FUN_0054ecd0
    #[must_use]
    pub fn from_ypr(ypr: Ypr) -> Self {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // `fstp dword` — the value an `fld dword` reads back.
        let stored = |v: X87| X87::from_f32(v.to_f32(pc, rc));
        // `fld y; fsin` (0x54ecd7) stays live in st1.
        let sy = trans::approx_sin(X87::from_f32(ypr.y)).value;
        // `fld y; fcos; fstp [esp+0x18]` (0x54ecdb).
        let cy = stored(trans::approx_cos(X87::from_f32(ypr.y)).value);
        // `fld p; fsin` (0x54ece3) stays live in st0, under it `sin(y)`.
        let sp = trans::approx_sin(X87::from_f32(ypr.p)).value;
        // `fld p; fcos; fstp [esp+8]` (0x54ece8).
        let cp = stored(trans::approx_cos(X87::from_f32(ypr.p)).value);
        // `fld r; fsin; fstp [esp+4]` (0x54ecf1).
        let sr = stored(trans::approx_sin(X87::from_f32(ypr.r)).value);
        // `fld r; fcos; fstp [esp]` (0x54ecfa).
        let cr = stored(trans::approx_cos(X87::from_f32(ypr.r)).value);
        // 0x54ed03-0x54ed19: `fld [esp+4]` sr; `fmul st(1)` sp; `fst [esp+0xc]`;
        // `fmul st(2)` sy; `fld [esp]` cr; `fmul [esp+0x18]` cy; `faddp`. The
        // `fst` keeps st0, so the `sy` product uses the **live** `sr*sp`, not
        // the stored copy (the two differ only below the `f32` normal range).
        let t1 = sr.mul(sp, pc, rc);
        let t1_mem = stored(t1);
        let m0 = t1.mul(sy, pc, rc).add(cr.mul(cy, pc, rc), pc, rc);
        // 0x54ed1b-0x54ed31: the same shape with cr*sp (`fst [esp+0x10]`) and
        // `fsubp` against sr*cy.
        let t4 = cr.mul(sp, pc, rc);
        let t4_mem = stored(t4);
        let m1 = t4.mul(sy, pc, rc).sub(sr.mul(cy, pc, rc), pc, rc);
        // 0x54ed34-0x54ed3a: `fld [esp+8]` cp; `fmul st(2)` sy.
        let m2 = cp.mul(sy, pc, rc);
        // 0x54ed3d-0x54ed45: `fld [esp+4]` sr; `fmul [esp+8]` cp.
        let m3 = sr.mul(cp, pc, rc);
        // 0x54ed48-0x54ed50: `fld [esp]` cr; `fmul [esp+8]` cp.
        let m4 = cr.mul(cp, pc, rc);
        // 0x54ed53-0x54ed55: `fchs` on the `sin(p)` still in st0.
        let m5 = fchs(sp);
        // 0x54ed58-0x54ed68: `fld [esp+0xc]` (the stored sr*sp); `fmul [esp+0x18]`
        // cy; `fld [esp]` cr; `fmul st(2)` sy; `fsubp`.
        let m6 = t1_mem.mul(cy, pc, rc).sub(cr.mul(sy, pc, rc), pc, rc);
        // 0x54ed6b-0x54ed7b: `fld [esp+0x10]` (the stored cr*sp); `fmul
        // [esp+0x18]` cy; `fld [esp+4]` sr; `fmul st(2)` sy; `faddp`.
        let m7 = t4_mem.mul(cy, pc, rc).add(sr.mul(sy, pc, rc), pc, rc);
        // 0x54ed7e pops the live sin(y); 0x54ed80-0x54ed88: `fld [esp+8]` cp;
        // `fmul [esp+0x18]` cy.
        let m8 = cp.mul(cy, pc, rc);
        // Every entry is `fstp dword`.
        Mat3 {
            m: [m0, m1, m2, m3, m4, m5, m6, m7, m8].map(|v| v.to_f32(pc, rc)),
        }
    }

    /// The Rodrigues axis-angle matrix the exe's `0x54ee80` builds, the
    /// helper behind `Vector3.rotate(Ljava.lang.Vector3;F)`.
    ///
    /// Its `ecx` is the destination and it takes four stack words, which the
    /// caller pushes in the order the native read them off the argument
    /// object: `[esp+0x14]` = the axis `x` (the *third* field read,
    /// `0x6133fc`), `[esp+0x18]` = `y` (`0x6133f8`), `[esp+0x1c]` = `z`
    /// (`0x6133f4`), `[esp+0x20]` = the angle (the `F` payload word). The
    /// destination is a 3×3 with the rows `0x10` bytes apart, exactly the
    /// layout [`Mat3`] carries row-major (see [`Mat3::from_ypr`]).
    ///
    /// ```text
    /// 54ee83  fld x ; fmul x ; fld y ; fmul y ; faddp
    /// 54ee95  fld z ; fmul z ; faddp        ; len2 = x² + y² + z², in order
    /// 54ee9f  fst [esp]                     ; the f32 image, kept in st0
    /// 54eea3  fcomp [1e-6f] ; test ah,1 ; jne 54efab   ; len2 < eps -> exit
    /// 54eeb4  fld [esp] ; fsub [1.0f] ; fabs ; fcomp [1e-6f]
    /// 54eec8  test ah,0x41 ; jne 54eef5     ; |len2-1| < eps or NaN -> skip
    /// 54eecd  fld [esp] ; fsqrt ; fdivr [1.0f]         ; inv = 1/|k|
    /// 54eed9  fmul x -> [esp+0x14] ; fmul y -> [esp+0x18] ; fmul z -> [...]
    /// 54eef5  fld angle ; fcos              ; cos first, stays under the sin
    /// 54eefb  fld angle ; fsin
    /// 54ef01  fld [1.0f] ; fsub st(2)       ; t = 1 - cos, live in st0
    /// ```
    ///
    /// The first guard is `len² < 1e-6f` (C0 after the `fcomp`; an unordered
    /// compare falls through into the body) and it jumps straight to the
    /// epilogue: **the helper writes nothing**. The exe's destination is the
    /// caller's own stack, so the three dot rows that follow read whatever
    /// the frame held there before (stale, unspecified); this port is handed
    /// a zeroed matrix, which is the value a first call on a clean stack
    /// sees, and returns it. The second guard normalises only when
    /// `|len² - 1|` is at or above the epsilon **and** ordered, and the
    /// normalised components replace the argument words the entries below
    /// read.
    ///
    /// `t = 1 - cos` is `fst dword`ed to `[esp+0x20]` but stays live, and the
    /// two rows that reload it (`0x54ef5e`, `0x54ef98`) get the same value,
    /// so one local serves both. The helper reuses its own scratch liberally
    /// — `[esp]` holds `len²` until `s*z` (0x54ef2b) overwrites it, `[esp+4]`
    /// and `[esp+8]` hold `t*x*y` and `t*x*z`, `[esp+0xc]` holds `s*y`, and
    /// `t*y*z` (0x54ef75) is stored **over the `y` argument**, which is dead
    /// by then. The entries come out as
    ///
    /// ```text
    /// m0 = c + (t*x)*x       m3 = (t*x)*y + s*z     m6 = (t*x)*z - s*y
    /// m1 = (t*x)*y - s*z     m4 = c + (t*y)*y       m7 = (t*y)*z + s*x
    /// m2 = (t*x)*z + s*y     m5 = (t*y)*z - s*x     m8 = c + (t*z)*z
    /// ```
    ///
    /// with the shared products `t*x*y`, `t*x*z` and `t*y*z` each rounded
    /// once by the `fstp dword` to their scratch slot and reloaded by the
    /// second row that uses them (so `m3`/`m6` use the copies while `m1`/`m2`
    /// use the live products for the first row: the two are the same value).
    /// Every product is left-to-right (`(t*x)*y`, not `t*(x*y)`), every entry
    /// is `fstp dword`, and every operation rounds to 24-bit precision
    /// ([`Precision::F32`], nearest-even).
    ///
    /// `fsin`/`fcos` are APPROX ([`slr_x87::trans`]): the entries that carry
    /// them make no bit-equality claim, the rest is exact.
    ///
    /// exe: FUN_0054ee80
    #[must_use]
    pub fn from_axis_angle(axis: Vec3, angle: f32) -> Self {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        // `fstp dword` — the value an `fld dword` reads back.
        let stored = |v: X87| X87::from_f32(v.to_f32(pc, rc));
        // `fld [esp+0x14]; fmul [esp+0x14]` — the three argument words are
        // `fld dword`s, so the register copies start as their f32 images.
        let (x, y, z) = (
            X87::from_f32(axis.x),
            X87::from_f32(axis.y),
            X87::from_f32(axis.z),
        );
        // 0x54ee83-0x54ee9d: `x² + y² + z²`, added left to right.
        let len2 = x
            .mul(x, pc, rc)
            .add(y.mul(y, pc, rc), pc, rc)
            .add(z.mul(z, pc, rc), pc, rc);
        // 0x54eea3-0x54eeae: `fcomp [1e-6f]` + `test ah,1` (C0), so a
        // squared length **below** the epsilon (or an unordered compare,
        // which leaves C0 clear) takes the exit that writes nothing.
        if len2.fcom(X87::from_f32(AXIS_EPSILON)).0.c0 {
            return Self { m: [0.0; 9] };
        }
        // 0x54eeb4-0x54eecb: `fabs(len² - 1)` vs the same epsilon, skipping
        // the normalisation on C0 (`<`) or C2 (unordered).
        let delta = fabs(len2.sub(X87::from_f32(1.0), pc, rc));
        let (cc, _) = delta.fcom(X87::from_f32(AXIS_EPSILON));
        let (x, y, z) = if cc.c0 || cc.c2 {
            (x, y, z)
        } else {
            // 0x54eecd-0x54eef1: `fsqrt` on the register copy of `len²`, then
            // `fdivr [1.0f]` (`1.0 / |k|`); each component is multiplied by
            // that one register copy and `fstp dword`ed back over its own
            // argument word, which is what the entries below read.
            let inv = X87::from_f32(1.0).div(len2.sqrt(pc, rc), pc, rc);
            (
                stored(x.mul(inv, pc, rc)),
                stored(y.mul(inv, pc, rc)),
                stored(z.mul(inv, pc, rc)),
            )
        };
        // 0x54eef5-0x54eef9: `fld angle; fcos` — the cosine is computed
        // **before** the sine (`0x54eeff`), and it stays in the register
        // stack under it.
        let c = trans::approx_cos(X87::from_f32(angle)).value;
        let s = trans::approx_sin(X87::from_f32(angle)).value;
        // 0x54ef01-0x54ef09: `fld [1.0f]; fsub st(2)`, i.e. `1.0 - cos` with
        // the sine and cosine still live; `fst [esp+0x20]` stores it but
        // keeps it.
        let t = X87::from_f32(1.0).sub(c, pc, rc);
        let t_mem = stored(t);
        // 0x54ef0d-0x54ef19: `fmul x` on the live `t`, then `fld st(0);
        // fmul x; fadd st(3)` — the square against the **live** cosine.
        let tx = t.mul(x, pc, rc);
        let m0 = tx.mul(x, pc, rc).add(c, pc, rc);
        // 0x54ef1b-0x54ef21: `fld st(0); fmul y; fstp [esp+4]` — the shared
        // `(t*x)*y`, stored and reloaded by the `m3` row.
        let kxky = tx.mul(y, pc, rc);
        // 0x54ef25-0x54ef2b: `fld st(1)` duplicates the live sine;
        // `fmul z; fstp [esp]` (over `len²`).
        let sz = s.mul(z, pc, rc);
        let sz_mem = stored(sz);
        let m1 = stored(kxky).sub(sz_mem, pc, rc);
        // 0x54ef3a-0x54ef3e: `fmul z` on the live `t*x`; `fstp [esp+8]`.
        let kxkz = tx.mul(z, pc, rc);
        let kxkz_mem = stored(kxkz);
        // 0x54ef42-0x54ef48: `fld st(0)` (the live sine again);
        // `fmul y; fst [esp+0xc]` keeps it live for the `m7` row.
        let sy = s.mul(y, pc, rc);
        let sy_mem = stored(sy);
        let m2 = sy_mem.add(kxkz_mem, pc, rc);
        let m3 = sz_mem.add(stored(kxky), pc, rc);
        // 0x54ef5e-0x54ef6e: `fld [esp+0x20]` — the **stored** `t`, then
        // `fmul y; fld st(0); fmul y; fadd st(3)` against the live cosine.
        let ty = t_mem.mul(y, pc, rc);
        let m4 = ty.mul(y, pc, rc).add(c, pc, rc);
        // 0x54ef71-0x54ef75: `fmul z` on the live `t*y`; the exe stores this
        // over the `y` argument word (dead from here on).
        let kykz = ty.mul(z, pc, rc);
        let kykz_mem = stored(kykz);
        // 0x54ef79-0x54ef83: `fmul x` on the live sine, then
        // `fld [esp+0x18]; fsub st(1)`.
        let sx = s.mul(x, pc, rc);
        let m5 = kykz_mem.sub(sx, pc, rc);
        // 0x54ef86-0x54ef8e: `fld [esp+8]` (the stored `(t*x)*z`);
        // `fsub [esp+0xc]` (the stored `s*y`).
        let m6 = kxkz_mem.sub(sy_mem, pc, rc);
        // 0x54ef91-0x54ef95: `fadd [esp+0x18]` adds the stored `(t*y)*z`
        // to the live `s*x` left in st0.
        let m7 = sx.add(kykz_mem, pc, rc);
        // 0x54ef98-0x54efa6: `fld [esp+0x20]` (the stored `t`);
        // `fmul z; fmul z; fadd st(1)` against the live cosine.
        let m8 = t_mem.mul(z, pc, rc).mul(z, pc, rc).add(c, pc, rc);
        // Every entry is `fstp dword`; 0x54efa9 pops the leftover cosine.
        Mat3 {
            m: [m0, m1, m2, m3, m4, m5, m6, m7, m8].map(|v| v.to_f32(pc, rc)),
        }
    }

    /// The 3×3 multiply helper `0x54eab0`, `self = left * self` in place —
    /// the composition `Ypr.mul` (`0x4826c0`) applies between its two
    /// [`Mat3::from_ypr`] matrices.
    ///
    /// `ecx` is the destination (`self`) and the one stack argument is
    /// `left`; the exe computes `result[i][j] = sum_k left[i][k] *
    /// self[k][j]` and copies the nine `f32` results back over `self`
    /// (`rep movsd 0xc` at `0x54eba8` moves the twelve-dword local, pads
    /// included, back to the destination — the three pad words are never
    /// read, so the port keeps the nine row-major entries). Every product is
    /// `fmul` and every partial sum `faddp`, both rounded to 24-bit precision
    /// ([`Precision::F32`], nearest-even), and the per-entry sum is
    /// accumulated in the exe's own order (the two `faddp`s, not a
    /// left-to-right tree), which the port reproduces entry by entry:
    ///
    /// ```text
    /// out[i][j] = (left[3i+1]*self[3+3j] + left[3i+0]*self[0+3j]) + left[3i+2]*self[6+3j]
    /// ```
    ///
    /// (the exe pushes the second operand of each `faddp` under `st(1)`
    /// first, so the pairing is `(a + b) + c` with the middle product first).
    ///
    /// exe: FUN_0054eab0
    pub fn premul(&mut self, left: Mat3) {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let l = left.m.map(X87::from_f32);
        let s = self.m.map(X87::from_f32);
        // `l[r*3+c] * s[k*3+c]` for the entry `(r, c)`.
        let p = |lr: X87, sc: X87| lr.mul(sc, pc, rc);
        let out = [
            // row 0: (m3*l1 + m0*l0) + m6*l2, then m7*l2 + m4*l1 + m1*l0,
            // then m8*l2 + m5*l1 + m2*l0.
            p(l[1], s[3])
                .add(p(l[0], s[0]), pc, rc)
                .add(p(l[2], s[6]), pc, rc),
            p(s[7], l[2])
                .add(p(l[1], s[4]), pc, rc)
                .add(p(s[1], l[0]), pc, rc),
            p(s[8], l[2])
                .add(p(s[5], l[1]), pc, rc)
                .add(p(l[0], s[2]), pc, rc),
            // row 1.
            p(l[3], s[0])
                .add(p(l[5], s[6]), pc, rc)
                .add(p(l[4], s[3]), pc, rc),
            p(l[5], s[7])
                .add(p(l[4], s[4]), pc, rc)
                .add(p(l[3], s[1]), pc, rc),
            p(l[3], s[2])
                .add(p(s[8], l[5]), pc, rc)
                .add(p(s[5], l[4]), pc, rc),
            // row 2.
            p(l[6], s[0])
                .add(p(l[8], s[6]), pc, rc)
                .add(p(l[7], s[3]), pc, rc),
            p(l[8], s[7])
                .add(p(l[7], s[4]), pc, rc)
                .add(p(l[6], s[1]), pc, rc),
            p(l[6], s[2])
                .add(p(l[8], s[8]), pc, rc)
                .add(p(s[5], l[7]), pc, rc),
        ];
        self.m = out.map(|v| v.to_f32(pc, rc));
    }

    /// The mat3→ypr helper `0x551c90`, the inverse [`Mat3::from_ypr`] names:
    /// the `(y, p, r)` triple `Ypr.mul` (`0x4826c0`) writes.
    ///
    /// `ecx` is the destination (`[ecx]`, `[ecx+4]`, `[ecx+8]` = `y`, `p`,
    /// `r`) and the one stack argument points at the matrix (the rows `0x10`
    /// bytes apart, so `m0..m8` at `0`, `4`, `8`, `0x10`, `0x14`, `0x18`,
    /// `0x20`, `0x24`, `0x28`). The exe first checks the degenerate
    /// `m8 = m2 = 0` case (`0x551c94`, `0x551ca4`), the same shape as
    /// [`Vec3::to_ypr`]:
    ///
    /// ```text
    /// 551cb4  fld [edx]   m0 ; fld [edx+4] m1 ; fchs ; fpatan
    ///         -> y = atan2(m0, -m1) ; [ecx+4] = 0x40490fdb (pi) ; [ecx+8] = 0
    /// ```
    ///
    /// The general path is `y = atan2(m2, m8)` (`0x551cd0`) and
    /// `r = atan2(m3, m4)` (`0x551cda`). The roll is `fst dword`ed to
    /// `[ecx+8]` but stays live, and `fcos` (0x551ce5) then `fmul m5`
    /// (`0x551ced`) form `prod = cos(r) * m5`. The `fcom` against `0.0f`
    /// (`0x551ce7`) sets the flags the `test ah,0x41` (`0x551cf2`) reads —
    /// Unicorn preserves them across the intervening `fmul` (the toy probe
    /// `re/out/probe_toypr.py` pins this), and the port branches the same
    /// way, so a **positive** cosine takes `atan2(-prod, m4)` (`fchs; fld
    /// m4; fpatan`) and `cos <= 0` **or NaN** takes `atan2(prod, -m4)` (`fld
    /// m4; fchs; fpatan`):
    ///
    /// ```text
    /// 551cf7  fchs ; fld [edx+0x14] m4 ; fpatan   ; cos(r) > 0
    /// 551d04  fld [edx+0x14] m4 ; fchs ; fpatan   ; cos(r) <= 0 or NaN
    /// ```
    ///
    /// `fpatan`/`fcos` are APPROX ([`slr_x87::trans`]); the port keeps the
    /// **wide** (unrounded) angle for `fcos`, exactly as the exe does (the
    /// `fst` stores but does not pop).
    ///
    /// exe: FUN_00551c90
    #[must_use]
    pub fn to_ypr(&self) -> Ypr {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let f = |i: usize| X87::from_f32(self.m[i]);
        // `fcomp qword [0x5f0c18]` on `m8` then `m2`: equality against `±0.0`
        // (NaN falls through to the general path).
        if self.m[8] == 0.0 && self.m[2] == 0.0 {
            // `fld m0; fld m1; fchs; fpatan` -> atan2(m0, -m1); the immediate
            // `0x40490fdb` is `pi`.
            let y = trans::approx_patan(f(0), fchs(f(1)));
            return Ypr::new(y.value.to_f32(pc, rc), PI, 0.0);
        }
        // `fld m2; fld m8; fpatan` -> atan2(m2, m8).
        let y = trans::approx_patan(f(2), f(8));
        // `fld m3; fld m4; fpatan` -> atan2(m3, m4); `fst [ecx+8]` stores the
        // rounded roll but keeps the wide one for `fcos`.
        let r = trans::approx_patan(f(3), f(4)).value;
        let r_out = r.to_f32(pc, rc);
        let c = trans::approx_cos(r).value;
        // `fcom [0x5f0c18]; fmul [edx+0x18]` — the flags are the `fcom`'s.
        let (cc, _) = c.fcom(X87::from_f32(0.0));
        let prod = c.mul(f(5), pc, rc);
        let p = if cc.c0 || cc.c3 {
            // `fld m4; fchs; fpatan` -> atan2(prod, -m4).
            trans::approx_patan(prod, fchs(f(4)))
        } else {
            // `fchs; fld m4; fpatan` -> atan2(-prod, m4).
            trans::approx_patan(fchs(prod), f(4))
        };
        Ypr::new(y.value.to_f32(pc, rc), p.value.to_f32(pc, rc), r_out)
    }
}

impl Ypr {
    /// The field names in declaration order (`y`, `p`, `r`).
    pub const FIELD_NAMES: [&'static str; 3] = [Y, P, R];

    /// Angles in declaration order: yaw, pitch, roll.
    #[must_use]
    pub fn new(y: f32, p: f32, r: f32) -> Self {
        Self { y, p, r }
    }

    /// All angles zero.
    #[must_use]
    pub fn zero() -> Self {
        Self::default()
    }

    /// Position of `name` in [`Self::FIELD_NAMES`], or `None` on a miss.
    #[must_use]
    pub fn field_index(name: &str) -> Option<usize> {
        Self::FIELD_NAMES.iter().position(|f| *f == name)
    }

    /// Read one field by its exe cstring name, or `None` on a miss.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<f32> {
        Self::field_index(name).map(|i| match i {
            0 => self.y,
            1 => self.p,
            _ => self.r,
        })
    }

    /// Write one field by its exe cstring name. Returns `false` and leaves the
    /// angles untouched on a miss.
    pub fn set_field(&mut self, name: &str, value: f32) -> bool {
        match Self::field_index(name) {
            Some(0) => self.y = value,
            Some(1) => self.p = value,
            Some(2) => self.r = value,
            _ => return false,
        }
        true
    }

    /// The angles in declaration order.
    #[must_use]
    pub fn to_array(self) -> [f32; 3] {
        [self.y, self.p, self.r]
    }
}

// ---- the native entry -------------------------------------------------

/// The receiver/return slot word the native reads: `container[count-1]`
/// after `FUN_0045d910`, i.e. `out[0]`.
///
/// A marshalling failure leaves the exe's out word at whatever the caller's
/// stack held; the port reads 0 there, which is the only value it can state
/// truthfully (a null receiver is a fault in the exe).
fn receiver_word(info: &CallInfo<'_>) -> u32 {
    let mut out = [0u32; 1];
    match marshal_args(info, &mut out) {
        Ok(_) => out[0],
        Err(_) => 0,
    }
}

/// The receiver and declared-argument slot words: `marshal_args` with the
/// `(Ljava.lang.Vector3;)F` shape, i.e. `out[0]` = `container[count-1]` (the
/// receiver) and `out[1]` = `container[count-2]` (the declared argument).
///
/// A marshalling failure leaves every word it did not write at 0: the exe
/// instead reads whatever its stack held there (for `distance` its own
/// argument-area pointer), which the port cannot state truthfully.
fn receiver_and_arg(info: &CallInfo<'_>) -> (u32, u32) {
    let mut out = [0u32; 2];
    match marshal_args(info, &mut out) {
        Ok(_) | Err(_) => (out[0], out[1]),
    }
}

/// The receiver, axis-object and angle words: `marshal_args` with the
/// `(Ljava.lang.Vector3;F)Ljava.lang.Vector3;` shape, i.e. `out[0]` =
/// `container[count-1]` (the receiver), `out[1]` = `container[count-2]` (the
/// declared axis object) and `out[2]` = `container[count-3]` (the `F` slot's
/// payload word, an `f32` bit pattern — `F`/`I` arguments are popped as raw
/// payload words).
///
/// A marshalling failure leaves every word it did not write at 0, exactly as
/// in [`receiver_and_arg`].
fn receiver_axis_and_angle(info: &CallInfo<'_>) -> (u32, u32, u32) {
    let mut out = [0u32; 3];
    match marshal_args(info, &mut out) {
        Ok(_) | Err(_) => (out[0], out[1], out[2]),
    }
}

/// Decode a port object word to its index in [`VmHost::objects`].
///
/// [`VmHost::alloc_object`] hands out `0x1000 + index` words; a
/// `java.lang.Vector3` receiver is always one of those (strings are
/// `0x2000_0000 +`, arrays `0x3000_0000 +`). `None` is a word the port
/// cannot follow.
fn object_index(word: u32) -> Option<usize> {
    usize::try_from(word.checked_sub(0x1000)?).ok()
}

/// One `F` field of object `index`, read by name — the port's
/// `get_float_field` (`0x42a560`).
///
/// The exe's lookup walks the object's field cells **from the last to the
/// first** comparing the name cstring, then requires the field's type byte
/// to be `F`; every miss (no object, no loaded class, no such instance
/// field, another declared type) logs and yields `+0.0f` (`fld [0x5e73cc]`).
/// All four miss paths are reproduced here.
///
/// Script and native lookup share the VM's original static-first name
/// resolution, inherited instance layout and authoritative typed cells.
fn float_field(vm: &mut VmHost, index: usize, name: &str) -> f32 {
    match vm.read_field(0x1000 + index as u32, name) {
        Ok(value) if value.tag == slr_vm::values::TypeTag::Float => value.as_f32(),
        Err(slr_vm::host::HostError::UnsupportedInitializer(error)) => {
            panic!("unsupported native float field initializer: {error}")
        }
        _ => 0.0,
    }
}

/// Writes the same class-static or instance cell selected by the original
/// `0x408800` name lookup, preserving a non-F field unchanged.
fn set_float_field(vm: &mut VmHost, index: usize, name: &str, value: f32) {
    let word = 0x1000 + index as u32;
    match vm.read_field(word, name) {
        Ok(current) if current.tag == slr_vm::values::TypeTag::Float => {
            let _ = vm.write_field(word, name, Value::float(value));
        }
        Err(slr_vm::host::HostError::UnsupportedInitializer(error)) => {
            panic!("unsupported native float field initializer: {error}")
        }
        _ => {}
    }
}

/// The `x`, `y`, `z` components of a `java.lang.Vector3` object word, read
/// in the native's own field order (`x` at `0x61340c`, `y` at `0x613410`,
/// `z` at `0x613414`).
#[must_use]
fn components(vm: &mut VmHost, word: u32) -> Vec3 {
    match object_index(word) {
        Some(index) => Vec3::new(
            float_field(vm, index, X),
            float_field(vm, index, Y),
            float_field(vm, index, Z),
        ),
        None => Vec3::zero(),
    }
}

/// The `y`, `p`, `r` angles of a `java.lang.Ypr` object word, read in the
/// native's own field order (`y` at `0x6133c4`, `p` at `0x6133c8`, `r` at
/// `0x6133cc` — the order `rotate` reads them in).
///
/// A word the port cannot follow reads as the zero angle triple, exactly as
/// three `get_float_field` misses would.
#[must_use]
fn ypr_fields(vm: &mut VmHost, word: u32) -> Ypr {
    match object_index(word) {
        Some(index) => Ypr::new(
            float_field(vm, index, Y),
            float_field(vm, index, P),
            float_field(vm, index, R),
        ),
        None => Ypr::zero(),
    }
}

/// `java.lang.Vector3.length()F` — the exe native's `record+8` entry.
///
/// The `F` return is the `st0` payload word, so this is the `f32` bits of
/// [`Vec3::length`] (the caller takes them per the `0x42012f` return-cell
/// rule).
///
/// exe: LAB_004823e0
pub fn length(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let receiver = receiver_word(info);
    Some(components(vm, receiver).length().to_bits())
}

/// `java.lang.Vector3.distance(Ljava.lang.Vector3;)F` — the exe native's
/// `record+8` entry.
///
/// The `F` return is the `st0` payload word, so this is the `f32` bits of
/// [`Vec3::distance`]. The exe reads the declared argument's triple first and
/// the receiver's second; both are `java.lang.Vector3` objects, so both go
/// through [`components`] (the exe's `get_float_field`, `0x42a560`).
///
/// exe: LAB_00482550
pub fn distance(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, argument) = receiver_and_arg(info);
    Some(
        components(vm, receiver)
            .distance(components(vm, argument))
            .to_bits(),
    )
}

/// `java.lang.Vector3.normalize()V` — the exe native's `record+8` entry.
///
/// A `V` native, so there is no return word (`None`); the effect is the three
/// `set_float_field` (`0x42a040`) writes at `0x482503`-`0x48253a` of the
/// triple [`Vec3::normalize`] computes. The exe writes all three fields on
/// **both** arms of the guard (`0x482501` jumps into the same write block),
/// so the receiver's components are read through [`components`] (the exe's
/// `get_float_field`, `0x42a560`) and written back through
/// [`set_float_field`] in the native's own order `x`, `y`, `z`
/// (`0x613424`/`0x613428`/`0x61342c`), scaled or not.
///
/// exe: LAB_00482450
pub fn normalize(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let receiver = receiver_word(info);
    let scaled = components(vm, receiver).normalize();
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, X, scaled.x);
        set_float_field(vm, index, Y, scaled.y);
        set_float_field(vm, index, Z, scaled.z);
    }
    None
}

/// `java.lang.Vector3.rotate(Ljava.lang.Ypr;)Ljava.lang.Vector3;` — the exe
/// native's `record+8` entry.
///
/// `this` is the receiver slot `container[count-1]` (`out[0]`) and the
/// declared argument `container[count-2]` (`out[1]`); the exe's own reads
/// (`y`,`p`,`r` out of the *argument* at `0x482136`/`482149`/`48215c`, then
/// `z`,`y`,`x` out of the *receiver* at `0x482172`/`482188`/`48219f`) put
/// them in that order. The rotated triple goes back into **the receiver**
/// through [`set_float_field`] in the order `x` (`0x6133dc`), `y`
/// (`0x6133e0`), `z` (`0x6133e4`) — the native's `[esp+0x6c]` object is the
/// same `out[0]` word it read, so `rotate` mutates and returns the receiver —
/// and that word is the `L` return (`EAX`).
///
/// A marshalling failure leaves the unwritten slot at 0 (see
/// [`receiver_and_arg`]); the exe would read its own stale stack word there.
///
/// exe: LAB_00482110
pub fn rotate(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, argument) = receiver_and_arg(info);
    let ypr = ypr_fields(vm, argument);
    let rotated = components(vm, receiver).rotate(ypr);
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, X, rotated.x);
        set_float_field(vm, index, Y, rotated.y);
        set_float_field(vm, index, Z, rotated.z);
    }
    Some(receiver)
}

/// `java.lang.Vector3.rotate(Ljava.lang.Vector3;F)Ljava.lang.Vector3;` — the
/// exe native's `record+8` entry, the axis-angle rotation in place.
///
/// `this` is the receiver slot `container[count-1]` (`out[0]`), the declared
/// axis object `container[count-2]` (`out[1]`) and the angle the `F` payload
/// word of `container[count-3]` (`out[2]`). The exe reads the receiver's
/// `z`,`y`,`x` first (`0x4822a6`-`0x4822cf`) and the axis' `z`,`y`,`x` second
/// (`0x4822e2`-`0x482308`) — the opposite order from `rotate(Ypr)`, which
/// reads its argument first — then the axis triple and the angle go through
/// [`Mat3::from_axis_angle`] (`0x54ee80`). The rotated triple is written back
/// into **the receiver** through [`set_float_field`] in the order `x`
/// (`0x613400`), `y` (`0x613404`), `z` (`0x613408`), and that word is the `L`
/// return (`EAX` at `0x4823cd`).
///
/// A marshalling failure leaves the unwritten slots at 0 (see
/// [`receiver_axis_and_angle`]); the exe would read its own stale stack words
/// there.
///
/// exe: LAB_00482280
pub fn rotate_axis(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, axis, angle) = receiver_axis_and_angle(info);
    let rotated = components(vm, receiver).rotate_axis(components(vm, axis), f32::from_bits(angle));
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, X, rotated.x);
        set_float_field(vm, index, Y, rotated.y);
        set_float_field(vm, index, Z, rotated.z);
    }
    Some(receiver)
}

/// `java.lang.Vector3.<init>(Ljava.lang.Ypr;)` — the exe native's `record+8`
/// entry, the constructor a script's `new Vector3(ypr)` lands on.
///
/// `this` is the receiver slot `container[count-1]` (`out[0]`) — an `<init>`
/// returns no object, so the caller pushes no result cell and the last stack
/// word is the freshly allocated receiver — and the declared argument is
/// `container[count-2]` (`out[1]`), the word the exe reads once per field
/// (`[esp+0x5c]`, `[esp+0x64]`, `[esp+0x6c]` at `0x482037`/`0x48204a`/
/// `0x48205d`). The triple [`Vec3::from_ypr`] computes goes into **the
/// receiver** through [`set_float_field`] in the order `x` (`0x6133b8`),
/// `y` (`0x6133bc`), `z` (`0x6133c0`), which is the same `out[0]` word, so
/// the constructor is in place. The signature declares no return type and the
/// native returns no word (`ret` at `0x482102`; the `EAX` left behind is a
/// dead reload of `this` at `0x4820ef`).
///
/// A marshalling failure leaves the unwritten slot at 0 (see
/// [`receiver_and_arg`]); the exe would read its own stale stack word there,
/// so a container with only the receiver cell reads the zero `Ypr` and writes
/// `(-0.0, 0.0, -1.0)` where the exe writes the direction of stale angles.
///
/// exe: LAB_00482020
pub fn init_from_ypr(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, argument) = receiver_and_arg(info);
    let direction = Vec3::from_ypr(ypr_fields(vm, argument));
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, X, direction.x);
        set_float_field(vm, index, Y, direction.y);
        set_float_field(vm, index, Z, direction.z);
    }
    None
}

/// `java.lang.Ypr.<init>(Ljava.lang.Vector3;)` — the exe native's `record+8`
/// entry, the constructor a script's `new Ypr(vector)` lands on.
///
/// `this` is the receiver slot `container[count-1]` (`out[0]`) — an `<init>`
/// returns no object, so the caller pushes no result cell and the last stack
/// word is the freshly allocated receiver — and the declared argument is
/// `container[count-2]` (`out[1]`), the word the exe reads once per field
/// (`[esp+0x2c]` at `0x482627`/`0x48263a`/`0x48264d`). It reads the
/// **argument's** `x` (`0x613448`), `y` (`0x61344c`) and `z` (`0x613450`)
/// with three [`get_float_field`]-equivalent calls (`0x482631`/`0x482644`/
/// `0x482657`), hands the triple to the vec→ypr helper `0x551d20`
/// ([`Vec3::to_ypr`], `0x48266c`) and writes the resulting `y`
/// (`0x613454`), `p` (`0x613458`) and `r` (`0x61345c`) into **the receiver**
/// through [`set_float_field`] (`0x482680`/`0x482694`/`0x4826a8`), which is
/// the same `out[0]` word, so the constructor is in place. The signature
/// declares no return type and the native returns no word (`ret` at
/// `0x4826b0`).
///
/// A marshalling failure leaves the unwritten slot at 0 (see
/// [`receiver_and_arg`]); the exe would read its own stale stack word there,
/// so a container with only the receiver cell reads the zero `Vector3`
/// (`x = z = 0`, `y = 0`) and writes `(0, -pi/2, 0)`.
///
/// The `fpatan` inside [`Vec3::to_ypr`] is APPROX ([`slr_x87::trans`]), so
/// only the degenerate-branch rows are bit-exact.
///
/// exe: LAB_00482610
pub fn ypr_init_from_vec3(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, argument) = receiver_and_arg(info);
    let ypr = components(vm, argument).to_ypr();
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, Y, ypr.y);
        set_float_field(vm, index, P, ypr.p);
        set_float_field(vm, index, R, ypr.r);
    }
    None
}

/// `java.lang.Ypr.mul(Ljava.lang.Ypr;)V` — the exe native's `record+8` entry,
/// the in-place angle composition a script's `ypr.mul(other)` lands on.
///
/// `this` is the receiver slot `container[count-1]` (`out[0]`) and the
/// declared argument `container[count-2]` (`out[1]`). The native reads six
/// fields — the receiver's `y`(`0x613460`)/`p`(`0x613464`)/`r`(`0x613468`)
/// and the argument's `y`(`0x61346c`)/`p`(`0x613470`)/`r`(`0x613474`) — then
/// builds two matrices with `0x54ecd0` ([`Mat3::from_ypr`]) and calls
/// `0x54eab0` ([`Mat3::premul`]) with `ecx` = the receiver's matrix and the
/// stack argument = the argument's matrix, so the product is
/// `argument_matrix * receiver_matrix`. `0x551c90` ([`Mat3::to_ypr`]) turns
/// that back into `y`, `p`, `r`, which go into **the receiver** through
/// [`set_float_field`] in the order `y` (`0x613478`), `p` (`0x61347c`),
/// `r` (`0x613480`) — the same `out[0]` word. The signature declares no
/// return type and the native returns no word (`ret` at `0x4827cc`).
///
/// A marshalling failure leaves the unwritten slot at 0 (see
/// [`receiver_and_arg`]); the exe would read its own stale stack word there,
/// so a container with only the receiver cell reads `y = p = r = 0` for the
/// argument.
///
/// `fsin`/`fcos`/`fpatan` in the helpers are APPROX
/// ([`slr_x87::trans`]): a zero-angle row is bit-exact, nonzero rows are
/// compared within the transcendental tolerance.
///
/// exe: LAB_004826c0
pub fn ypr_mul(vm: &mut VmHost, info: &CallInfo<'_>) -> NativeResult {
    let (receiver, argument) = receiver_and_arg(info);
    let mut product = Mat3::from_ypr(ypr_fields(vm, receiver));
    product.premul(Mat3::from_ypr(ypr_fields(vm, argument)));
    let ypr = product.to_ypr();
    if let Some(index) = object_index(receiver) {
        set_float_field(vm, index, Y, ypr.y);
        set_float_field(vm, index, P, ypr.p);
        set_float_field(vm, index, R, ypr.r);
    }
    None
}

/// Register and install the `java.lang.Vector3`/`java.lang.Ypr` ports that
/// have landed.
///
/// Two slots are filled per row, both idempotent (the exe's registrar
/// `FUN_00416b00` is a plain overwrite): the `(class, name, sig)` record in
/// [`slr_vm::natives::Natives`], and the implementation the VM calls by
/// address in [`VmHost::native_impls`]. All eight [`NATIVES`] rows are
/// registered here.
pub fn install(vm: &mut VmHost) {
    vm.natives.register(
        VECTOR3_ROTATE_YPR.class,
        VECTOR3_ROTATE_YPR.name,
        VECTOR3_ROTATE_YPR.sig,
        VECTOR3_ROTATE_YPR.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_ROTATE_YPR.impl_addr, rotate as NativeFn);
    vm.natives.register(
        VECTOR3_LENGTH.class,
        VECTOR3_LENGTH.name,
        VECTOR3_LENGTH.sig,
        VECTOR3_LENGTH.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_LENGTH.impl_addr, length as NativeFn);
    vm.natives.register(
        VECTOR3_DISTANCE.class,
        VECTOR3_DISTANCE.name,
        VECTOR3_DISTANCE.sig,
        VECTOR3_DISTANCE.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_DISTANCE.impl_addr, distance as NativeFn);
    vm.natives.register(
        VECTOR3_NORMALIZE.class,
        VECTOR3_NORMALIZE.name,
        VECTOR3_NORMALIZE.sig,
        VECTOR3_NORMALIZE.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_NORMALIZE.impl_addr, normalize as NativeFn);
    vm.natives.register(
        VECTOR3_ROTATE_AXIS.class,
        VECTOR3_ROTATE_AXIS.name,
        VECTOR3_ROTATE_AXIS.sig,
        VECTOR3_ROTATE_AXIS.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_ROTATE_AXIS.impl_addr, rotate_axis as NativeFn);
    vm.natives.register(
        VECTOR3_INIT.class,
        VECTOR3_INIT.name,
        VECTOR3_INIT.sig,
        VECTOR3_INIT.impl_addr,
    );
    vm.native_impls
        .insert(VECTOR3_INIT.impl_addr, init_from_ypr as NativeFn);
    vm.natives.register(
        YPR_INIT.class,
        YPR_INIT.name,
        YPR_INIT.sig,
        YPR_INIT.impl_addr,
    );
    vm.native_impls
        .insert(YPR_INIT.impl_addr, ypr_init_from_vec3 as NativeFn);
    vm.natives
        .register(YPR_MUL.class, YPR_MUL.name, YPR_MUL.sig, YPR_MUL.impl_addr);
    vm.native_impls
        .insert(YPR_MUL.impl_addr, ypr_mul as NativeFn);
}

/// `Vector3.rotate(Ypr)` — one registry row, one named item so the ledger
/// scanner can attach the exe symbol.
///
/// exe: LAB_00482110
pub const VECTOR3_ROTATE_YPR: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "rotate",
    sig: "(Ljava.lang.Ypr;)Ljava.lang.Vector3;",
    impl_addr: 0x0048_2110,
};

/// `Vector3.length()F`.
///
/// exe: LAB_004823e0
pub const VECTOR3_LENGTH: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "length",
    sig: "()F",
    impl_addr: 0x0048_23e0,
};

/// `Vector3.distance(Vector3)F`.
///
/// exe: LAB_00482550
pub const VECTOR3_DISTANCE: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "distance",
    sig: "(Ljava.lang.Vector3;)F",
    impl_addr: 0x0048_2550,
};

/// `Vector3.normalize()V`.
///
/// exe: LAB_00482450
pub const VECTOR3_NORMALIZE: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "normalize",
    sig: "()V",
    impl_addr: 0x0048_2450,
};

/// `Vector3.rotate(Vector3,F)Vector3` (axis-angle).
///
/// exe: LAB_00482280
pub const VECTOR3_ROTATE_AXIS: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "rotate",
    sig: "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
    impl_addr: 0x0048_2280,
};

/// `Vector3.<init>(Ypr)`.
///
/// exe: LAB_00482020
pub const VECTOR3_INIT: NativeBinding = NativeBinding {
    class: VECTOR3,
    name: "<init>",
    sig: "(Ljava.lang.Ypr;)",
    impl_addr: 0x0048_2020,
};

/// `Ypr.<init>(Vector3)`.
///
/// exe: LAB_00482610
pub const YPR_INIT: NativeBinding = NativeBinding {
    class: YPR,
    name: "<init>",
    sig: "(Ljava.lang.Vector3;)",
    impl_addr: 0x0048_2610,
};

/// `Ypr.mul(Ypr)V`.
///
/// exe: LAB_004826c0
pub const YPR_MUL: NativeBinding = NativeBinding {
    class: YPR,
    name: "mul",
    sig: "(Ljava.lang.Ypr;)V",
    impl_addr: 0x0048_26c0,
};

/// The eight `Vector3`/`Ypr` natives registered by the exe
/// (`re/out/native_api.tsv` rows 130-137), in registration order.
pub const NATIVES: [NativeBinding; 8] = [
    VECTOR3_ROTATE_YPR,
    VECTOR3_LENGTH,
    VECTOR3_DISTANCE,
    VECTOR3_NORMALIZE,
    VECTOR3_ROTATE_AXIS,
    VECTOR3_INIT,
    YPR_INIT,
    YPR_MUL,
];

#[cfg(test)]
mod tests {
    use super::*;
    use slr_vm::host::Object;
    use slr_vm::values::Value;
    use slr_vm::vmclass::{Class, Field};

    #[test]
    fn constructors_keep_component_order() {
        let v = Vec3::new(1.5, 2.5, 3.5);
        assert_eq!((v.x, v.y, v.z), (1.5, 2.5, 3.5));
        assert_eq!(v.to_array(), [1.5, 2.5, 3.5]);

        // Ypr takes yaw, pitch, roll in that order; note the shared `y` name.
        let a = Ypr::new(0.25, -0.5, 1.75);
        assert_eq!((a.y, a.p, a.r), (0.25, -0.5, 1.75));
        assert_eq!(a.to_array(), [0.25, -0.5, 1.75]);
    }

    #[test]
    fn zero_constructors_are_all_zero() {
        let v = Vec3::zero();
        assert_eq!(v, Vec3::new(0.0, 0.0, 0.0));
        assert_eq!(v.to_array(), [0.0, 0.0, 0.0]);
        let a = Ypr::zero();
        assert_eq!(a, Ypr::new(0.0, 0.0, 0.0));
        assert_eq!(a.to_array(), [0.0, 0.0, 0.0]);
        assert!(v.to_array().iter().all(|c| c.to_bits() == 0));
        assert!(a.to_array().iter().all(|c| c.to_bits() == 0));
    }

    #[test]
    fn vec3_field_names_map_to_declaration_order() {
        assert_eq!(Vec3::FIELD_NAMES, ["x", "y", "z"]);
        assert_eq!(Vec3::field_index(X), Some(0));
        assert_eq!(Vec3::field_index(Y), Some(1));
        assert_eq!(Vec3::field_index(Z), Some(2));

        let mut v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.field(X), Some(1.0));
        assert_eq!(v.field(Y), Some(2.0));
        assert_eq!(v.field(Z), Some(3.0));

        assert!(v.set_field(Y, 7.25));
        assert_eq!(v, Vec3::new(1.0, 7.25, 3.0));
        assert_eq!(v.field(Y).map(f32::to_bits), Some(7.25f32.to_bits()));
    }

    #[test]
    fn ypr_field_names_map_to_declaration_order() {
        // The Ypr class declares `y,p,r` (no `x`/`z`), unlike Vector3.
        assert_eq!(Ypr::FIELD_NAMES, ["y", "p", "r"]);
        assert_eq!(Ypr::field_index(Y), Some(0));
        assert_eq!(Ypr::field_index(P), Some(1));
        assert_eq!(Ypr::field_index(R), Some(2));
        assert_eq!(Ypr::field_index(X), None);
        assert_eq!(Ypr::field_index(Z), None);

        let mut a = Ypr::new(0.0, 0.0, 0.0);
        assert!(a.set_field(P, -1.5));
        assert!(a.set_field(R, 2.5));
        assert_eq!(a, Ypr::new(0.0, -1.5, 2.5));
        assert_eq!(a.field(P), Some(-1.5));
    }

    #[test]
    fn unknown_field_names_miss_without_writing() {
        let mut v = Vec3::new(1.0, 2.0, 3.0);
        assert_eq!(v.field("w"), None);
        assert_eq!(
            v.field("X"),
            None,
            "field names are case-sensitive cstrings"
        );
        assert!(!v.set_field("w", 9.0));
        assert_eq!(v, Vec3::new(1.0, 2.0, 3.0));

        let mut a = Ypr::new(4.0, 5.0, 6.0);
        assert_eq!(a.field("x"), None);
        assert!(!a.set_field("x", 9.0));
        assert_eq!(a, Ypr::new(4.0, 5.0, 6.0));
    }

    #[test]
    fn natives_table_pins_all_eight_rows() {
        assert_eq!(NATIVES.len(), 8);
        let want = [
            (
                VECTOR3,
                "rotate",
                "(Ljava.lang.Ypr;)Ljava.lang.Vector3;",
                0x0048_2110u32,
            ),
            (VECTOR3, "length", "()F", 0x0048_23e0),
            (VECTOR3, "distance", "(Ljava.lang.Vector3;)F", 0x0048_2550),
            (VECTOR3, "normalize", "()V", 0x0048_2450),
            (
                VECTOR3,
                "rotate",
                "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
                0x0048_2280,
            ),
            (VECTOR3, "<init>", "(Ljava.lang.Ypr;)", 0x0048_2020),
            (YPR, "<init>", "(Ljava.lang.Vector3;)", 0x0048_2610),
            (YPR, "mul", "(Ljava.lang.Ypr;)V", 0x0048_26c0),
        ];
        for (row, (class, name, sig, addr)) in NATIVES.iter().zip(want) {
            assert_eq!(
                (row.class, row.name, row.sig, row.impl_addr),
                (class, name, sig, addr)
            );
        }
    }

    #[test]
    fn natives_table_addresses_are_unique() {
        let mut addrs: Vec<u32> = NATIVES.iter().map(|n| n.impl_addr).collect();
        addrs.sort_unstable();
        addrs.dedup();
        assert_eq!(addrs.len(), NATIVES.len());
        assert!(NATIVES.iter().all(|n| n.impl_addr >= 0x0048_0000));
    }

    // ---- Vector3.length (0x004823e0) ---------------------------------

    /// Exactly representable inputs: these are the values the three squares
    /// and their sum represent with no rounding at all, so `fsqrt` is the
    /// only rounded step (the exe's own probe cases).
    #[test]
    fn length_is_exact_on_representable_vectors() {
        let cases: [(f32, f32, f32, f32); 7] = [
            (3.0, 4.0, 0.0, 5.0),
            (4.0, 3.0, 0.0, 5.0),
            (0.0, 3.0, 4.0, 5.0),
            (-3.0, -4.0, 12.0, 13.0),
            (2.0, 2.0, 1.0, 3.0),
            (1.0, 0.0, 0.0, 1.0),
            (0.0, 0.0, 0.0, 0.0),
        ];
        for (x, y, z, want) in cases {
            assert_eq!(
                Vec3::new(x, y, z).length().to_bits(),
                want.to_bits(),
                "({x}, {y}, {z})"
            );
        }
        // `+0.0`, not `-0.0`: the sum has no negative terms.
        assert_eq!(Vec3::new(-0.0, -0.0, -0.0).length().to_bits(), 0);
    }

    /// The exe's own results for two inputs where the **add order is
    /// observable**: `(z*z + y*y) + x*x` (the order at `0x482427-0x48243f`)
    /// rounds the first partial sum at 24-bit precision before `x*x` lands,
    /// so putting another term last gives a different bit pattern. Both
    /// triples are exactly representable decimals — `2^-20`,
    /// `1.25 * 2^-20`, `2^-8` and the relabelled second row — and the
    /// expected bits are what the exe returned under Unicorn at `cw 0x7f`.
    #[test]
    fn length_adds_x_squared_last_at_24_bit_precision() {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let sq = |v: f32| X87::from_f32(v).mul(X87::from_f32(v), pc, rc);
        for (x, y, z, want) in [
            (9.536_743e-7, 1.192_092_9e-6, 0.003_906_25, 0x3b80_0001u32),
            (1.430_511_5e-6, 9.536_743e-7, 0.004_882_812_5, 0x3ba0_0000),
        ] {
            assert_eq!(Vec3::new(x, y, z).length().to_bits(), want, "({x},{y},{z})");
            // The same x87 sequence with another term added last; the first
            // add stays put because IEEE addition is commutative. Each of
            // these disagrees with the exe on both rows.
            let y_last = sq(z)
                .add(sq(x), pc, rc)
                .add(sq(y), pc, rc)
                .sqrt(pc, rc)
                .to_f32(pc, rc);
            let z_last = sq(y)
                .add(sq(x), pc, rc)
                .add(sq(z), pc, rc)
                .sqrt(pc, rc)
                .to_f32(pc, rc);
            assert_ne!(y_last.to_bits(), want, "({x},{y},{z}) with y last");
            assert_ne!(z_last.to_bits(), want, "({x},{y},{z}) with z last");
        }
        // One rounded-anchor case where the sum itself is not exact: the
        // exe's bits for `(1.5, 2.5, 3.5)` at `cw 0x7f`.
        assert_eq!(Vec3::new(1.5, 2.5, 3.5).length().to_bits(), 0x4091_c456);
        // A square far below the f32 normal range: the extended-precision
        // square is a normal number (the exponent keeps its 15-bit range),
        // so the only rounding left is the final conversion (`1e-30` squared
        // rounds to `0xda24260`).
        assert_eq!(Vec3::new(1e-30, 0.0, 0.0).length().to_bits(), 0x0da2_4260);
    }

    /// `fsqrt`/`fmul` special values, as the exe's probe returned them.
    #[test]
    fn length_special_values_follow_the_x87_ops() {
        // `sqrt(+inf)` and `+inf` propagation through the sums.
        assert_eq!(
            Vec3::new(0.0, f32::INFINITY, 0.0).length().to_bits(),
            0x7f80_0000
        );
        // A NaN component survives the multiplies, the adds and `fsqrt` (the
        // exe's raw was `f32::NAN`'s pattern, `0x7fc00000`).
        assert_eq!(
            Vec3::new(f32::NAN, 1.0, 2.0).length().to_bits(),
            0x7fc0_0000
        );
        // The squares do *not* overflow to `+inf` in the extended format
        // (`3e38 * 3e38` is a normal 24-bit-significand extended number), so
        // the infinity comes from the final `to_f32` conversion.
        assert_eq!(
            Vec3::new(3.0e38, 3.0e38, 3.0e38).length().to_bits(),
            0x7f80_0000
        );
    }

    // ---- Vector3.distance (0x00482550) -------------------------------

    /// Exactly representable rows: every difference, square and partial sum
    /// is exact, so `fsqrt` is the only rounded step. Expected bits are the
    /// exe's own (`cw 0x7f`).
    #[test]
    fn distance_is_exact_on_representable_vectors() {
        // `(receiver, argument, exe result)`.
        type Case = ((f32, f32, f32), (f32, f32, f32), f32);
        let cases: [Case; 6] = [
            ((3.0, 4.0, 0.0), (0.0, 0.0, 0.0), 5.0),
            ((0.0, 0.0, 0.0), (3.0, 4.0, 0.0), 5.0),
            ((-3.0, -4.0, 12.0), (0.0, 0.0, 0.0), 13.0),
            ((2.0, 2.0, 1.0), (0.0, 0.0, 0.0), 3.0),
            ((1.0, 2.0, 2.0), (1.0, 2.0, 2.0), 0.0),
            ((1.5, 2.5, 3.5), (0.5, 1.0, 1.5), 2.692_582_4),
        ];
        for (this, arg, want) in cases {
            assert_eq!(
                Vec3::new(this.0, this.1, this.2)
                    .distance(Vec3::new(arg.0, arg.1, arg.2))
                    .to_bits(),
                want.to_bits(),
                "{this:?} - {arg:?}"
            );
        }
        // `(1.5, 2.5, 3.5) - (0.5, 1.0, 1.5)` is the exe's `0x402c5345`.
        assert_eq!(
            Vec3::new(1.5, 2.5, 3.5)
                .distance(Vec3::new(0.5, 1.0, 1.5))
                .to_bits(),
            0x402c_5345
        );
        // `+0.0`, not `-0.0`: every square is non-negative.
        assert_eq!(
            Vec3::new(-0.0, -0.0, -0.0).distance(Vec3::zero()).to_bits(),
            0
        );
    }

    /// The exe's own results for inputs where the **accumulation order is
    /// observable**: `dz*dz + (dx*dx + dy*dy)` (the order at
    /// `0x4825e9-0x4825f7`) rounds the `dx*dx + dy*dy` partial sum at 24-bit
    /// precision before `dz*dz` lands, so switching which term is added last
    /// changes the bits. The first row is the triple the `length` test above
    /// uses, where the exe's `length` — which adds `x*x` last — returns
    /// `0x3b800001` while `distance` from the origin returns `0x3b800000`.
    #[test]
    fn distance_adds_z_squared_last_at_24_bit_precision() {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let sq = |v: f32| X87::from_f32(v).mul(X87::from_f32(v), pc, rc);
        // The `length` order: (dz*dz + dy*dy) + dx*dx.
        let length_order = |v: Vec3| {
            sq(v.z)
                .add(sq(v.y), pc, rc)
                .add(sq(v.x), pc, rc)
                .sqrt(pc, rc)
                .to_f32(pc, rc)
        };
        for (this, arg, want) in [
            (
                (9.536_743e-7, 1.192_092_9e-6, 0.003_906_25),
                (0.0, 0.0, 0.0),
                0x3b80_0000u32,
            ),
            (
                (-598.247_3, 788.957_6, -251.483_5),
                (407.040_62, -591.953_6, -768.467_2),
                0x44df_1327,
            ),
            (
                (502.716_3, -244.165_16, 635.916_75),
                (584.998_84, -786.317_57, -107.388_12),
                0x4466_ec1c,
            ),
        ] {
            let this = Vec3::new(this.0, this.1, this.2);
            let arg = Vec3::new(arg.0, arg.1, arg.2);
            assert_eq!(this.distance(arg).to_bits(), want, "{this:?} - {arg:?}");
            // The same squared terms with the other term added last; this
            // disagrees with the exe on all three rows.
            let d = Vec3::new(this.x - arg.x, this.y - arg.y, this.z - arg.z);
            assert_ne!(
                length_order(d).to_bits(),
                want,
                "{this:?} - {arg:?} with the length order"
            );
        }
        // The first row is exactly the `length` test's triple: the two
        // natives disagree, so neither one's order explains the other.
        let v = Vec3::new(9.536_743e-7, 1.192_092_9e-6, 0.003_906_25);
        assert_eq!(v.length().to_bits(), 0x3b80_0001);
        assert_eq!(v.distance(Vec3::zero()).to_bits(), 0x3b80_0000);
    }

    /// `fsub`/`fmul`/`fsqrt` special values and the extended square range, as
    /// the exe's probe returned them (`cw 0x7f`).
    #[test]
    fn distance_special_values_follow_the_x87_ops() {
        let zero = Vec3::zero();
        // `+inf` propagates through the difference, its square and the sums.
        assert_eq!(
            Vec3::new(0.0, f32::INFINITY, 0.0).distance(zero).to_bits(),
            0x7f80_0000
        );
        // A NaN component survives the difference, the multiplies, the adds
        // and `fsqrt` (`0x7fc00000`).
        assert_eq!(
            Vec3::new(f32::NAN, 1.0, 2.0).distance(zero).to_bits(),
            0x7fc0_0000
        );
        // `inf - inf` is the 80-bit indefinite QNaN, whose sign bit is SET:
        // `0xffc00000` once converted to f32 (the exe's probe returned that
        // raw `(0xc000000000000000, 0xffff)` register pair).
        assert_eq!(
            Vec3::new(f32::INFINITY, 1.0, 2.0)
                .distance(Vec3::new(f32::INFINITY, 1.0, 2.0))
                .to_bits(),
            0xffc0_0000
        );
        // The squares do not overflow to `+inf` in the extended format, so
        // this infinity comes from the final `to_f32` conversion (the
        // extended sum is `5.1961525446e38`).
        assert_eq!(
            Vec3::new(3.0e38, 3.0e38, 3.0e38).distance(zero).to_bits(),
            0x7f80_0000
        );
        // Squares far below the f32 normal range stay normal in the extended
        // format, so the only rounding left is the final conversion.
        assert_eq!(
            Vec3::new(1e-30, 0.0, 0.0).distance(zero).to_bits(),
            0x0da2_4260
        );
        assert_eq!(
            Vec3::new(1e-20, 1e-20, 1e-20).distance(zero).to_bits(),
            0x1ea3_966b
        );
        assert_eq!(
            Vec3::new(0.0, 0.0, 3e-20).distance(zero).to_bits(),
            0x1f0d_abc6
        );
    }

    // ---- Vector3.normalize (0x00482450) ------------------------------

    /// Exactly representable rows: every square, sum and quotient lands on an
    /// f32 value with no rounding, so these pin the guard and the two x87
    /// orders rather than the rounding. Expected bits are the exe's own
    /// written payload words (`re/out/probe_normalize.py`, `cw 0x7f`).
    #[test]
    fn normalize_is_exact_on_representable_vectors() {
        type Case = ((f32, f32, f32), (u32, u32, u32));
        let cases: [Case; 6] = [
            ((0.0, 0.0, 0.0), (0x0000_0000, 0x0000_0000, 0x0000_0000)),
            ((1.0, 0.0, 0.0), (0x3f80_0000, 0x0000_0000, 0x0000_0000)),
            ((3.0, 4.0, 0.0), (0x3f19_999a, 0x3f4c_cccd, 0x0000_0000)),
            ((0.0, 3.0, 4.0), (0x0000_0000, 0x3f19_999a, 0x3f4c_cccd)),
            ((-3.0, -4.0, 12.0), (0xbe6c_4ec6, 0xbe9d_89d9, 0x3f6c_4ec6)),
            ((2.0, 2.0, 1.0), (0x3f2a_aaab, 0x3f2a_aaab, 0x3eaa_aaab)),
        ];
        for (v, want) in cases {
            let got = Vec3::new(v.0, v.1, v.2).normalize();
            assert_eq!(
                (got.x.to_bits(), got.y.to_bits(), got.z.to_bits()),
                want,
                "{v:?}"
            );
        }
    }

    /// Every row of the probe where at least one step rounds, against the
    /// exe's own written payload words. The probe logged the three
    /// `set_float_field` arguments with the get/set helpers hooked and then
    /// repeated every row with the real helpers in place (only the CRT log
    /// sinks muted), reading the fields back through the real `0x42a560`:
    /// the two paths agreed on all 13 finite rows.
    #[test]
    fn normalize_matches_the_exe_payload_words() {
        type Case = ((f32, f32, f32), (u32, u32, u32));
        let cases: [Case; 7] = [
            ((1.5, 2.5, 3.5), (0x3ea8_9910, 0x3f0c_7f8e, 0x3f44_b293)),
            (
                (9.536_743e-7, 1.192_092_9e-6, 0.003_906_25),
                (0x397f_fffe, 0x399f_ffff, 0x3f7f_fffe),
            ),
            ((1.0e-30, 0.0, 0.0), (0x3f80_0000, 0x0000_0000, 0x0000_0000)),
            (
                (1.0e-20, 1.0e-20, 1.0e-20),
                (0x3f13_cd3a, 0x3f13_cd3a, 0x3f13_cd3a),
            ),
            (
                (-598.247_3, 788.957_6, -251.483_5),
                (0xbf15_eb09, 0x3f45_b591, 0xbe7c_1524),
            ),
            (
                (0.0, f32::INFINITY, 0.0),
                (0x0000_0000, 0xffc0_0000, 0x0000_0000),
            ),
            (
                (f32::NAN, 1.0, 2.0),
                (0x7fc0_0000, 0x3f80_0000, 0x4000_0000),
            ),
        ];
        for (v, want) in cases {
            let got = Vec3::new(v.0, v.1, v.2).normalize();
            assert_eq!(
                (got.x.to_bits(), got.y.to_bits(), got.z.to_bits()),
                want,
                "{v:?}"
            );
        }
    }

    /// The divisor is the **extended** `fsqrt` result (`0x4824dd fdivr`), not
    /// its f32 rounding. Where the two disagree the written bits differ, so
    /// this is observable: `f32(|v|)` overflows for `(3e38, 3e38, 3e38)`,
    /// which would make every component `3e38 * 0 = +0`, while the exe's
    /// extended divisor is `5.1961524e38` and every component comes out
    /// `1/sqrt(3)` (the probe's payload word `0x3f13cd3a`). On the rows where
    /// the f32 rounding is below the quotient's own rounding the two variants
    /// write the same words — the divisor choice is only visible when it
    /// changes the result.
    #[test]
    fn normalize_divides_by_the_extended_length() {
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let v = Vec3::new(3.0e38, 3.0e38, 3.0e38);
        assert_eq!(
            v.length().to_bits(),
            0x7f80_0000,
            "the f32 length overflows"
        );
        let f32_divisor = |c: f32| {
            let inv = X87::from_f32(1.0).div(X87::from_f32(v.length()), pc, rc);
            X87::from_f32(c).mul(inv, pc, rc).to_f32(pc, rc)
        };
        assert_eq!(f32_divisor(3.0e38).to_bits(), 0, "an f32 divisor gives 0");
        assert_eq!(
            v.normalize().to_array().map(f32::to_bits),
            [0x3f13_cd3a, 0x3f13_cd3a, 0x3f13_cd3a]
        );
        // A component far below the f32 normal range: `1/(2^-149)` on the
        // 80-bit value is a plain number, so the unit vector is exact (the
        // probe's row). Done in f32 the same reciprocal is `+inf`, which
        // would write `inf * 2^-149 = inf` rather than 1.0.
        let tiny = Vec3::new(f32::from_bits(1), 0.0, 0.0);
        assert_eq!(
            (1.0f32 / f32::from_bits(1)).to_bits(),
            0x7f80_0000,
            "the f32 reciprocal overflows"
        );
        assert_eq!(
            tiny.normalize().to_array().map(f32::to_bits),
            [0x3f80_0000, 0x0000_0000, 0x0000_0000]
        );
        // The sum's first add is `y*y + z*z` here against `z*z + y*y` in
        // `length`: the two extended values are identical (IEEE addition is
        // commutative), so the divisor is exactly that native's length.
        let v = Vec3::new(9.536_743e-7, 1.192_092_9e-6, 0.003_906_25);
        let sq = |c: f32| X87::from_f32(c).mul(X87::from_f32(c), pc, rc);
        let normalize_order = sq(v.y).add(sq(v.z), pc, rc).add(sq(v.x), pc, rc);
        let length_order = sq(v.z).add(sq(v.y), pc, rc).add(sq(v.x), pc, rc);
        assert_eq!(normalize_order, length_order);
        assert_eq!(v.length().to_bits(), 0x3b80_0001);
    }

    /// The guard at `0x4824d0` is `fcom [0x5e73cc]` (`0.0f`) + `test ah,0x40`
    /// (C3), so it skips on **equal or unordered**: a zero length, a `-0.0`
    /// length (which compares equal to `+0.0`), and an unordered NaN compare
    /// all leave the receiver's components exactly as they were read — under
    /// a plain `len != 0.0` test the NaN row would scale.
    #[test]
    fn normalize_guard_skips_on_zero_and_unordered() {
        let bits = |v: Vec3| v.to_array().map(f32::to_bits);
        // `(z*z + y*y) + x*x` with every term `+0` is `+0.0`: equal.
        assert_eq!(bits(Vec3::zero().normalize()), [0, 0, 0]);
        // `-0.0` components: the sum is still `+0.0` (`fadd` of two `+0`s),
        // and the write-back stores the read words unmodified.
        assert_eq!(
            bits(Vec3::new(-0.0, -0.0, -0.0).normalize()),
            [0x8000_0000, 0x8000_0000, 0x8000_0000]
        );
        // A NaN component makes the length NaN: unordered -> C3 -> skip, so
        // the other two components are *not* scaled (the probe's row writes
        // 0x7fc00000, 0x3f800000, 0x40000000 — scaling would write NaN to
        // all three).
        assert_eq!(
            bits(Vec3::new(f32::NAN, 1.0, 2.0).normalize()),
            [0x7fc0_0000, 0x3f80_0000, 0x4000_0000]
        );
        // An infinite length is **not** zero or unordered: the guard scales
        // with `inv = 1/inf = +0`, so `inf * 0` is the x87 indefinite NaN
        // (sign set: `0xffc00000`) while the finite components become `+0`.
        assert_eq!(
            bits(Vec3::new(0.0, f32::INFINITY, 0.0).normalize()),
            [0x0000_0000, 0xffc0_0000, 0x0000_0000]
        );
    }

    // ---- the native entry (`0x004823e0` in `VmHost::native_impls`) ----

    /// `(field name, type descriptor, static)` for a synthetic class.
    type FieldSpec<'a> = (&'a str, &'a str, bool);

    /// Push one object of the host's `java.lang.Vector3` class whose slots
    /// hold `values` (declaration order) and return its word.
    fn vec3_object(vm: &mut VmHost, values: &[f32]) -> u32 {
        let Some(metadata) = vm.loader.find(VECTOR3).map(|class| class.fields.clone()) else {
            // Explicit malformed-metadata fixture for native miss cases.
            let word = 0x1000 + vm.objects.len() as u32;
            vm.objects.push(Object {
                class: VECTOR3.to_owned(),
                fields: values.iter().copied().map(Value::float).collect(),
            });
            return word;
        };
        let word = vm.alloc_object(VECTOR3).unwrap();
        let fields = &mut vm.objects[(word - 0x1000) as usize].fields;
        let mut slot = 0;
        for (index, field) in metadata.iter().enumerate() {
            if field.is_static {
                continue;
            }
            if let Some(value) = values.get(index) {
                fields[slot] = Value {
                    tag: slr_vm::values::TypeTag::intern(&field.ty),
                    payload: value.to_bits(),
                };
            }
            slot += 1;
        }
        word
    }

    /// A host with one synthetic `java.lang.Vector3` class and one object of
    /// it whose slots hold `values` (declaration order). Returns the word.
    fn vec3_host(fields: &[FieldSpec<'_>], values: &[f32]) -> (VmHost, u32) {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: VECTOR3.to_owned(),
            base: None,
            interfaces: Vec::new(),
            methods: Vec::new(),
            fields: fields
                .iter()
                .map(|(name, ty, is_static)| Field {
                    class: VECTOR3.to_owned(),
                    name: (*name).to_owned(),
                    ty: (*ty).to_owned(),
                    is_static: *is_static,
                })
                .collect(),
            bodies: Vec::new(),
        });
        let word = vec3_object(&mut vm, values);
        (vm, word)
    }

    fn length_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "length",
            sig: "()F",
        }
    }

    fn distance_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "distance",
            sig: "(Ljava.lang.Vector3;)F",
        }
    }

    fn normalize_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "normalize",
            sig: "()V",
        }
    }

    /// Push one object of the host's `java.lang.Ypr` class whose slots hold
    /// `values` (declaration order) and return its word.
    fn ypr_object(vm: &mut VmHost, values: &[f32]) -> u32 {
        let Some(metadata) = vm.loader.find(YPR).map(|class| class.fields.clone()) else {
            // Explicit malformed-metadata fixture for native miss cases.
            let word = 0x1000 + vm.objects.len() as u32;
            vm.objects.push(Object {
                class: YPR.to_owned(),
                fields: values.iter().copied().map(Value::float).collect(),
            });
            return word;
        };
        let word = vm.alloc_object(YPR).unwrap();
        let fields = &mut vm.objects[(word - 0x1000) as usize].fields;
        let mut slot = 0;
        for (index, field) in metadata.iter().enumerate() {
            if field.is_static {
                continue;
            }
            if let Some(value) = values.get(index) {
                fields[slot] = Value {
                    tag: slr_vm::values::TypeTag::intern(&field.ty),
                    payload: value.to_bits(),
                };
            }
            slot += 1;
        }
        word
    }

    /// A host with a synthetic `java.lang.Vector3` and a synthetic
    /// `java.lang.Ypr` class, one Ypr holding `angles` and one Vector3
    /// holding `components`, each declared in the given field order.
    fn rotate_host_with(
        vec_fields: &[FieldSpec<'_>],
        vec_values: &[f32],
        ypr_fields: &[FieldSpec<'_>],
        ypr_values: &[f32],
    ) -> (VmHost, u32, u32) {
        let mut vm = VmHost::new();
        for (class, fields) in [(VECTOR3, vec_fields), (YPR, ypr_fields)] {
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: class.to_owned(),
                base: None,
                interfaces: Vec::new(),
                methods: Vec::new(),
                fields: fields
                    .iter()
                    .map(|(name, ty, is_static)| Field {
                        class: class.to_owned(),
                        name: (*name).to_owned(),
                        ty: (*ty).to_owned(),
                        is_static: *is_static,
                    })
                    .collect(),
                bodies: Vec::new(),
            });
        }
        let receiver = vec3_object(&mut vm, vec_values);
        let ypr = ypr_object(&mut vm, ypr_values);
        (vm, receiver, ypr)
    }

    /// [`rotate_host_with`] with both classes declared in their canonical
    /// order (`x, y, z` and `y, p, r`).
    fn rotate_host(components: &[f32], angles: &[f32]) -> (VmHost, u32, u32) {
        rotate_host_with(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            components,
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            angles,
        )
    }

    fn rotate_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "rotate",
            sig: "(Ljava.lang.Ypr;)Ljava.lang.Vector3;",
        }
    }

    /// The receiver is read by field **name** (the exe's cstring compare),
    /// walking the field list backwards, so declaration order is irrelevant
    /// and a duplicated name resolves to its last occurrence.
    #[test]
    fn the_native_reads_the_receiver_components_by_name() {
        // Declared `z, x, y` against slots `0, 3, 4`: by name that is
        // x = 3, y = 4, z = 0.
        let (mut vm, word) = vec3_host(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(5.0f32.to_bits()));

        // A duplicated name: the backwards walk takes the last `x` (slot 2),
        // so the length is 3, not the 1 or 2 of the earlier slots.
        let (mut vm, word) = vec3_host(
            &[
                (X, "F", false),
                (X, "F", false),
                (X, "F", false),
                (Y, "F", false),
                (Z, "F", false),
            ],
            &[1.0, 2.0, 3.0, 0.0, 0.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(3.0f32.to_bits()));
    }

    /// Every `get_float_field` miss path answers `+0.0f` (`fld [0x5e73cc]`):
    /// a field of another declared type, a field the class does not declare,
    /// an unloaded class, an unallocated word, and an empty
    /// container (the exe's out word is then whatever the caller's stack
    /// held; the port reads 0).
    #[test]
    fn static_components_use_the_same_class_owned_cells_as_vm_access() {
        let (mut vm, word) = vec3_host(
            &[(X, "F", true), (Y, "F", false), (Z, "F", false)],
            &[0., 4., 0.],
        );
        vm.write_field(word, X, Value::float(3.)).unwrap();
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(5_f32.to_bits()));
        normalize(&mut vm, &normalize_info(&args));
        assert_eq!(vm.read_field(word, X).unwrap(), Value::float(0.6));
        assert_eq!(vm.objects[(word - 0x1000) as usize].fields.len(), 2);
    }

    #[test]
    fn component_misses_read_zero_like_get_float_field() {
        // `x` is declared `I`, so the float read misses even though slot 0
        // holds 999.0: only `y`/`z` count, and 3-4-5 comes out exactly.
        let (mut vm, word) = vec3_host(
            &[(X, "I", false), (Y, "F", false), (Z, "F", false)],
            &[999.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(5.0f32.to_bits()));

        // Static x lives in the separate class container and defaults to zero.
        let (mut vm, word) = vec3_host(
            &[(X, "F", true), (Y, "F", false), (Z, "F", false)],
            &[999.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(5.0f32.to_bits()));

        // The class name is unknown to the loader: every read misses.
        let (mut vm, word) = vec3_host(&[(X, "F", false)], &[3.0]);
        vm.objects[0].class = YPR.to_owned();
        let args = [Value::reference(word)];
        assert_eq!(length(&mut vm, &length_info(&args)), Some(0.0f32.to_bits()));

        // An object word outside the heap, and a container with no slot.
        let (mut vm, _) = vec3_host(&[(X, "F", false)], &[3.0]);
        let stray = [Value::reference(0x7fff_0000)];
        assert_eq!(
            length(&mut vm, &length_info(&stray)),
            Some(0.0f32.to_bits())
        );
        assert_eq!(length(&mut vm, &length_info(&[])), Some(0.0f32.to_bits()));
    }

    /// `install` fills both exe slots for each landed row and is idempotent,
    /// and the installed `record+8` entries run the ports end to end.
    #[test]
    fn install_registers_the_landed_addresses_and_their_impls() {
        let mut vm = VmHost::new();
        install(&mut vm);
        assert_eq!(
            vm.natives.resolve(VECTOR3, "length", "()F"),
            Some(VECTOR3_LENGTH.impl_addr)
        );
        assert_eq!(
            vm.natives
                .resolve(VECTOR3, "distance", "(Ljava.lang.Vector3;)F"),
            Some(VECTOR3_DISTANCE.impl_addr)
        );
        let port = *vm
            .native_impls
            .get(&0x0048_23e0)
            .expect("the length address is installed");
        let distance_port = *vm
            .native_impls
            .get(&0x0048_2550)
            .expect("the distance address is installed");
        assert_eq!(
            vm.natives.resolve(VECTOR3, "normalize", "()V"),
            Some(VECTOR3_NORMALIZE.impl_addr)
        );
        let normalize_port = *vm
            .native_impls
            .get(&0x0048_2450)
            .expect("the normalize address is installed");
        assert_eq!(
            vm.natives
                .resolve(VECTOR3, "rotate", "(Ljava.lang.Ypr;)Ljava.lang.Vector3;"),
            Some(VECTOR3_ROTATE_YPR.impl_addr)
        );
        let rotate_port = *vm
            .native_impls
            .get(&0x0048_2110)
            .expect("the rotate address is installed");
        assert_eq!(
            vm.natives.resolve(
                VECTOR3,
                "rotate",
                "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;"
            ),
            Some(VECTOR3_ROTATE_AXIS.impl_addr)
        );
        let axis_port = *vm
            .native_impls
            .get(&0x0048_2280)
            .expect("the axis-angle rotate address is installed");

        let (mut host, word) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(port(&mut host, &length_info(&args)), Some(5.0f32.to_bits()));

        let origin = vec3_object(&mut host, &[0.0, 0.0, 0.0]);
        let args = [Value::reference(origin), Value::reference(word)];
        assert_eq!(
            distance_port(&mut host, &distance_info(&args)),
            Some(5.0f32.to_bits())
        );

        let args = [Value::reference(word)];
        assert_eq!(normalize_port(&mut host, &normalize_info(&args)), None);
        assert_eq!(float_field(&mut host, 0, X).to_bits(), 0x3f19_999a);
        assert_eq!(float_field(&mut host, 0, Y).to_bits(), 0x3f4c_cccd);
        assert_eq!(float_field(&mut host, 0, Z).to_bits(), 0x0000_0000);

        // The zero-angle row goes through the installed `record+8` entry:
        // the receiver comes back unchanged and the same word is returned.
        let (mut host, receiver) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
        );
        let ypr = ypr_object(&mut host, &[0.0, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(rotate_port(&mut host, &rotate_info(&args)), Some(receiver));
        assert_eq!(float_field(&mut host, 0, X).to_bits(), 3.0f32.to_bits());
        assert_eq!(float_field(&mut host, 0, Y).to_bits(), 4.0f32.to_bits());
        assert_eq!(float_field(&mut host, 0, Z).to_bits(), 0.0f32.to_bits());

        // The axis-angle row through the same `record+8` slot: a zero axis
        // takes the helper's guard arm, so the port's zero matrix zeroes the
        // receiver, and the `L` return is still the receiver word.
        let (mut host, receiver, _, args) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
            &[0.0, 0.0, 0.0],
            1.0,
        );
        assert_eq!(
            axis_port(&mut host, &rotate_axis_info(&args)),
            Some(receiver)
        );
        assert_eq!(float_field(&mut host, 0, X).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut host, 0, Y).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut host, 0, Z).to_bits(), 0.0f32.to_bits());

        // The constructor row through the same `record+8` slot: the `Ypr`
        // cell is the declared argument and the vector the receiver, and the
        // zero angle triple lands as the `(-0.0, +0.0, -1.0)` direction.
        assert_eq!(
            vm.natives.resolve(VECTOR3, "<init>", "(Ljava.lang.Ypr;)"),
            Some(VECTOR3_INIT.impl_addr)
        );
        let init_port = *vm
            .native_impls
            .get(&0x0048_2020)
            .expect("the constructor address is installed");
        let (mut host, receiver, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(init_port(&mut host, &init_info(&args)), None);
        assert_eq!(float_field(&mut host, 0, X).to_bits(), 0x8000_0000);
        assert_eq!(float_field(&mut host, 0, Y).to_bits(), 0x0000_0000);
        assert_eq!(float_field(&mut host, 0, Z).to_bits(), (-1.0f32).to_bits());

        // The two `Ypr` rows resolve and install at their own addresses.
        assert_eq!(
            vm.natives.resolve(YPR, "<init>", "(Ljava.lang.Vector3;)"),
            Some(YPR_INIT.impl_addr)
        );
        let ypr_init_port = *vm
            .native_impls
            .get(&0x0048_2610)
            .expect("the Ypr constructor address is installed");
        assert_eq!(
            vm.natives.resolve(YPR, "mul", "(Ljava.lang.Ypr;)V"),
            Some(YPR_MUL.impl_addr)
        );
        let ypr_mul_port = *vm
            .native_impls
            .get(&0x0048_26c0)
            .expect("the Ypr.mul address is installed");

        // `Ypr.<init>(Vector3)` through its `record+8` slot: the `Vector3`
        // is the spilled argument (`out[1]`) and the `Ypr` the receiver
        // (`out[0]`), so the triple lands as the receiver's `y`/`p`/`r` and
        // the native returns no word. `Vec3::to_ypr` always writes `r = 0`.
        let (mut host, vector, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[1.5, 2.5, -3.5]);
        let args = [Value::reference(vector), Value::reference(ypr)];
        assert_eq!(ypr_init_port(&mut host, &ypr_init_info(&args)), None);
        assert_eq!(float_field(&mut host, 1, R).to_bits(), 0.0f32.to_bits());

        // `Ypr.mul(Ypr)` through the same slot: the receiver's `y` slot is
        // overwritten by the composed triple.
        let (mut host, lhs, rhs) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0],
        );
        let args = [Value::reference(rhs), Value::reference(lhs)];
        assert_eq!(ypr_mul_port(&mut host, &ypr_mul_info(&args)), None);
        assert_eq!(float_field(&mut host, 0, Y).to_bits(), 0.0f32.to_bits());

        install(&mut vm);
        assert_eq!(vm.native_impls.len(), 8, "all eight rows landed");
    }

    // ---- the `distance` entry (`0x00482550`) --------------------------

    /// Both operands are read by field **name** out of two different objects:
    /// the receiver is `container[count-1]` (`out[0]`) and the declared
    /// argument `container[count-2]`, which is the exe's own stack order
    /// (`data[count-1]` above `data[count-2]`). Swapping the two cells
    /// therefore swaps the two vectors.
    #[test]
    fn the_distance_native_reads_both_operands_by_name() {
        // Declared `z, x, y` against slots `0, 3, 4` on both objects: by name
        // the receiver is (3, 4, 0) and the argument the origin.
        let (mut vm, receiver) = vec3_host(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 3.0, 4.0],
        );
        let arg = vec3_object(&mut vm, &[0.0, 0.0, 0.0]);
        let args = [Value::reference(arg), Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&args)),
            Some(5.0f32.to_bits())
        );

        // The argument's own fields are the second triple: give it z = 12 too
        // and the distance becomes 13.
        let (mut vm, receiver) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
        );
        let arg = vec3_object(&mut vm, &[0.0, 0.0, 12.0]);
        let args = [Value::reference(arg), Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&args)),
            Some(13.0f32.to_bits())
        );

        // Swapping the cells swaps the roles: the receiver is now the origin.
        let (mut vm, receiver) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[0.0, 0.0, 0.0],
        );
        let arg = vec3_object(&mut vm, &[3.0, 4.0, 0.0]);
        let args = [Value::reference(arg), Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&args)),
            Some(5.0f32.to_bits())
        );
    }

    /// Every `get_float_field` miss path answers `+0.0f` on either operand,
    /// and a short container leaves the argument word at 0 — the exe reads
    /// whatever its stack held there (its argument-area pointer), which the
    /// port cannot state truthfully.
    #[test]
    fn distance_operand_misses_read_zero_like_get_float_field() {
        // An argument word outside the heap: its three reads all miss, so the
        // distance is the receiver's own length.
        let (mut vm, receiver) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
        );
        let args = [Value::reference(0x7fff_0000), Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&args)),
            Some(5.0f32.to_bits())
        );

        // A class the loader does not know: both operands read as zero, so a
        // distance between two non-zero objects collapses to zero.
        let (mut vm, receiver) = vec3_host(&[(X, "F", false)], &[3.0]);
        let arg = vec3_object(&mut vm, &[4.0]);
        vm.objects[0].class = YPR.to_owned();
        vm.objects[1].class = YPR.to_owned();
        let args = [Value::reference(arg), Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&args)),
            Some(0.0f32.to_bits())
        );

        // A container with only the receiver cell (and none at all): the
        // unwritten argument word stays 0, so the missing operand is the
        // origin. The exe would read its stale argument-area pointer here.
        let (mut vm, receiver) = vec3_host(&[(X, "F", false)], &[3.0]);
        let short = [Value::reference(receiver)];
        assert_eq!(
            distance(&mut vm, &distance_info(&short)),
            Some(3.0f32.to_bits())
        );
        assert_eq!(
            distance(&mut vm, &distance_info(&[])),
            Some(0.0f32.to_bits())
        );
    }

    // ---- the `normalize` entry (`0x00482450`) -------------------------

    /// A `V` native: no return word, and the scaled triple goes back into the
    /// receiver's own slots through the name lookup. The class here declares
    /// `z, x, y` against slots `0, 3, 4`, so by name the receiver is
    /// x = 3, y = 4, z = 0 before the call and 0.6 / 0.8 / 0 after it — in
    /// those same three slots, in the exe's write order `x`, `y`, `z`.
    #[test]
    fn the_normalize_native_writes_the_receiver_back_by_name() {
        let (mut vm, word) = vec3_host(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(normalize(&mut vm, &normalize_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), 0x3f19_999a);
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0x3f4c_cccd);
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), 0x0000_0000);
        // slot order is `z, x, y` on this class.
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots, [0x0000_0000, 0x3f19_999a, 0x3f4c_cccd]);

        // A duplicated name resolves to its last occurrence for the write
        // exactly as it does for the read: the last `x` (slot 2) holds 3.
        let (mut vm, word) = vec3_host(
            &[
                (X, "F", false),
                (X, "F", false),
                (X, "F", false),
                (Y, "F", false),
                (Z, "F", false),
            ],
            &[1.0, 2.0, 3.0, 4.0, 0.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(normalize(&mut vm, &normalize_info(&args)), None);
        // The triple is (3, 4, 0) -> (0.6, 0.8, 0); the earlier `x` slots
        // (0, 1) are untouched and only the last one is written.
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(
            slots,
            [
                1.0f32.to_bits(),
                2.0f32.to_bits(),
                0x3f19_999a,
                0x3f4c_cccd,
                0x0000_0000
            ]
        );
    }

    /// The write-back misses exactly where the read misses (`0x408800` is the
    /// same lookup for both), so a component that read as `+0.0` leaves its
    /// slot alone instead of being zeroed:
    ///
    /// - `x` declared `I` reads `+0.0` and its slot keeps 999.0 while `y`/`z`
    ///   scale against each other (3-4-5 -> 0.6/0.8),
    /// - the static `x` fixture has its class-owned default zero cell; the
    ///   instance contains only `y`/`z`,
    /// - an unknown class misses every read (length 0, the guard skips) and
    ///   every write, so the slots keep their values,
    /// - an unallocated receiver word and an empty container write nothing
    ///   and do not panic (the exe reads its stale out word / argument-area
    ///   pointer there).
    #[test]
    fn normalize_write_back_misses_leave_slots_alone() {
        let (mut vm, word) = vec3_host(
            &[(X, "I", false), (Y, "F", false), (Z, "F", false)],
            &[999.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(normalize(&mut vm, &normalize_info(&args)), None);
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots, [999.0f32.to_bits(), 0x3f19_999a, 0x3f4c_cccd]);

        let (mut vm, word) = vec3_host(
            &[(X, "F", true), (Y, "F", false), (Z, "F", false)],
            &[999.0, 3.0, 4.0],
        );
        let args = [Value::reference(word)];
        normalize(&mut vm, &normalize_info(&args));
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots, [0x3f19_999a, 0x3f4c_cccd]);

        let (mut vm, word) = vec3_host(&[(X, "F", false)], &[3.0]);
        vm.objects[0].class = YPR.to_owned();
        let args = [Value::reference(word)];
        normalize(&mut vm, &normalize_info(&args));
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());

        let (mut vm, _) = vec3_host(&[(X, "F", false)], &[3.0]);
        let stray = [Value::reference(0x7fff_0000)];
        assert_eq!(normalize(&mut vm, &normalize_info(&stray)), None);
        assert_eq!(normalize(&mut vm, &normalize_info(&[])), None);
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());
    }

    /// The zero vector goes through the guard's skip arm, which still writes
    /// all three fields: the read words (`+0.0`) land back in their slots, so
    /// the native is a no-op on the object rather than a partial write.
    #[test]
    fn normalize_on_the_zero_vector_writes_the_read_words_back() {
        let (mut vm, word) = vec3_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[0.0, 0.0, 0.0],
        );
        let args = [Value::reference(word)];
        assert_eq!(normalize(&mut vm, &normalize_info(&args)), None);
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots, [0, 0, 0]);
    }

    // ---- the `rotate(Ypr)` entry (`0x00482110`) -----------------------

    /// The ypr→matrix helper at zero angles is the identity — with one
    /// observable exception that pins `FCHS`: `m5` is `-sin(p)`, negated in
    /// the register, so it is `-0.0` where `0.0 - sin(p)` would be `+0.0`.
    /// The other entries are `+0.0`, the result of subtracting two `+0.0`
    /// products (`+0.0 - +0.0` and `+0.0 + +0.0` are both `+0.0`).
    #[test]
    fn the_ypr_matrix_is_the_identity_at_zero_angles() {
        let m = Mat3::from_ypr(Ypr::zero());
        assert_eq!(
            m.m.map(f32::to_bits),
            [
                1.0f32.to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                1.0f32.to_bits(),
                (-0.0f32).to_bits(),
                0.0f32.to_bits(),
                0.0f32.to_bits(),
                1.0f32.to_bits(),
            ]
        );
    }

    /// At zero angles the trig is exact (`sin 0 = ±0`, `cos 0 = 1`), so the
    /// whole native is bit-exact: the matrix is the identity, every dot
    /// product is `m4*v - 0*v` and the receiver comes back with the same
    /// three payload words. The two non-finite arms below pin what the
    /// remaining `+0.0` matrix entries do to an infinite or NaN component.
    #[test]
    fn rotate_by_zero_angles_returns_the_same_payload_words() {
        let rotated = Vec3::new(3.0, 4.0, 0.0).rotate(Ypr::zero());
        assert_eq!(
            rotated.to_array().map(f32::to_bits),
            [3.0f32.to_bits(), 4.0f32.to_bits(), 0.0f32.to_bits()]
        );

        // A non-finite component does **not** ride through: with the identity
        // matrix each row still multiplies the other two components by the
        // `+0.0` entries, and `0 * inf` is the indefinite NaN — so an
        // infinite `x` makes `y` and `z` NaN (the exe's rows do the same
        // `fmul` against a `+0.0` entry; probe-verified).
        let rotated = Vec3::new(f32::INFINITY, -1.0, 2.5).rotate(Ypr::zero());
        assert_eq!(
            rotated.to_array().map(f32::to_bits),
            [f32::INFINITY.to_bits(), 0xffc0_0000, 0xffc0_0000,]
        );

        // A NaN operand instead propagates into every component: each row
        // multiplies it by a `+0.0` entry somewhere, and `0 * NaN` is NaN.
        let rotated = Vec3::new(-1.0, f32::NAN, 2.5).rotate(Ypr::zero());
        assert_eq!(
            rotated.to_array().map(f32::to_bits),
            [0x7fc0_0000, 0x7fc0_0000, 0x7fc0_0000]
        );
    }

    /// The two quarter-turn axes, to a tolerance: `fsin`/`fcos` are APPROX
    /// (`slr_x87::trans`, `docs/x87.md`), so the port is compared against the
    /// geometric answer rather than against bits. `p = +π/2` takes `+z` to
    /// `-y` and `y = +π/2` takes `+x` to `-z`; the residual is the `cos` of
    /// the `f32` quarter turn (~4.4e-8), not a port error.
    #[test]
    fn rotate_by_a_quarter_turn_takes_an_axis_to_the_neighbouring_one() {
        let pitch = Ypr::new(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        let rotated = Vec3::new(0.0, 0.0, 1.0).rotate(pitch);
        assert_eq!(rotated.x.to_bits(), 0.0f32.to_bits());
        assert_eq!(rotated.y, -1.0);
        // cos(f32 π/2) rounded to f32, the same entry `m8` holds.
        assert_eq!(rotated.z.to_bits(), Mat3::from_ypr(pitch).m[8].to_bits());
        assert_eq!((-4.371_139e-8f32).to_bits(), rotated.z.to_bits());

        let yaw = Ypr::new(std::f32::consts::FRAC_PI_2, 0.0, 0.0);
        let rotated = Vec3::new(1.0, 0.0, 0.0).rotate(yaw);
        assert_eq!(rotated.z, -1.0);
        assert_eq!((-4.371_139e-8f32).to_bits(), rotated.x.to_bits());
    }

    /// One nonzero-angle row pinned to the **exe's own** payload words: the
    /// probe (`re/out/probe_rotate.py`) ran `0x482110` on `(1, 2, 3)` with
    /// ypr `(0.25, 0.5, 0.75)` and the port reproduced all three words; the
    /// same probe pinned the dot-row order bit-exactly over 300 rows by
    /// feeding the exe's own `0x54ecd0` matrix into the model (`mine`
    /// 300/300, the `x, y, z`-ordered accumulation 148/300, a right-nested
    /// sum 206/300). A change in op order or rounding moves these bits.
    #[test]
    fn rotate_reproduces_the_probed_exe_payload_words() {
        let rotated = Vec3::new(1.0, 2.0, 3.0).rotate(Ypr::new(0.25, 0.5, 0.75));
        assert_eq!(
            rotated.to_array().map(f32::to_bits),
            [0x3e96_6fe0, 0x3ee3_67f2, 0x406d_0737]
        );
    }

    /// The native reads the **argument**'s `y`, `p`, `r` out of
    /// `container[count-2]` and the **receiver**'s `x`, `y`, `z` out of
    /// `container[count-1]` (the exe's own stack order, `0x482136`/`482172`),
    /// writes the rotated triple back into the receiver in the order `x`,
    /// `y`, `z`, and returns the receiver word.
    #[test]
    fn the_rotate_native_reads_both_operands_by_name_and_returns_the_receiver() {
        // Declared `r, p, y` against slots `0, 1, 2`: by name the angles are
        // y = 0, p = 0, r = 0, so the receiver is unchanged; `z, x, y` on the
        // vector side reads (0, 3, 4) as x = 0, y = 4, z = 3.
        let (mut vm, receiver, ypr) = rotate_host_with(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 0.0, 4.0],
            &[(R, "F", false), (P, "F", false), (Y, "F", false)],
            &[0.0, 0.0, 0.0],
        );
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(rotate(&mut vm, &rotate_info(&args)), Some(receiver));
        assert_eq!(
            vm.objects[0]
                .fields
                .iter()
                .map(|v| v.as_f32())
                .collect::<Vec<_>>(),
            [0.0, 0.0, 4.0]
        );

        // The roles follow the container, not the classes: a nonzero yaw in
        // the *argument* cell rotates the vector in the *receiver* cell, and
        // those receiver payload words are the only ones that change. Yaw
        // `+π/2` takes `+x` to `-z`, so (1, 2, 3) becomes (≈3, 2, ≈-1) — the
        // `y` component is untouched by a yaw (`m5*z` is `-0.0 * 3`).
        let quarter = std::f32::consts::FRAC_PI_2;
        let (mut vm, receiver, ypr) = rotate_host(&[1.0, 2.0, 3.0], &[quarter, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(rotate(&mut vm, &rotate_info(&args)), Some(receiver));
        assert_eq!(
            vm.objects[1]
                .fields
                .iter()
                .map(|v| v.as_f32())
                .collect::<Vec<_>>(),
            [quarter, 0.0, 0.0]
        );
        let slots: Vec<f32> = vm.objects[0].fields.iter().map(|v| v.as_f32()).collect();
        assert!(
            (slots[0] - 3.0).abs() < 1e-6,
            "x' = {:#x}",
            slots[0].to_bits()
        );
        assert_eq!(slots[1].to_bits(), 2.0f32.to_bits());
        assert!(
            (slots[2] + 1.0).abs() < 1e-6,
            "z' = {:#x}",
            slots[2].to_bits()
        );
    }

    /// The receiver is the object the writes land in, in the exe's write
    /// order (`x`, `y`, `z`), and the return word is that same word: the
    /// receiver cell *is* the return cell.
    #[test]
    fn the_rotate_native_writes_the_receiver_in_place() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let (mut vm, receiver, ypr) = rotate_host(&[0.0, 0.0, 1.0], &[0.0, quarter, 0.0]);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(rotate(&mut vm, &rotate_info(&args)), Some(receiver));
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots[0], 0.0f32.to_bits());
        assert_eq!(slots[1], (-1.0f32).to_bits());
        assert_eq!(slots[2], (-4.371_139e-8f32).to_bits());
    }

    /// Every `get_float_field` miss path answers `+0.0f` on either operand,
    /// and a short container leaves the argument word at 0 — so a missing
    /// argument is the zero angle triple (the identity) and a missing
    /// receiver reads as the zero vector, which the native still returns.
    #[test]
    fn rotate_operand_misses_read_zero_like_get_float_field() {
        // The angle object word is outside the heap: three misses, zero
        // angles, so the receiver's payload words come back untouched.
        let (mut vm, receiver, _) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        let args = [Value::reference(0x7fff_0000), Value::reference(receiver)];
        assert_eq!(rotate(&mut vm, &rotate_info(&args)), Some(receiver));
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(
            slots,
            [3.0f32.to_bits(), 4.0f32.to_bits(), 0.0f32.to_bits()]
        );

        // A container with only the receiver cell: the unwritten argument
        // word stays 0, so the rotation is the identity again.
        let (mut vm, receiver, _) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        let short = [Value::reference(receiver)];
        assert_eq!(rotate(&mut vm, &rotate_info(&short)), Some(receiver));
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(
            slots,
            [3.0f32.to_bits(), 4.0f32.to_bits(), 0.0f32.to_bits()]
        );

        // An unallocated receiver word: the reads are zero, the writes miss
        // (the same name lookup) and the word is still the `L` return.
        let (mut vm, _, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(0x7fff_0000)];
        assert_eq!(rotate(&mut vm, &rotate_info(&args)), Some(0x7fff_0000));
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());

        // An empty container has no receiver and no argument word: the return
        // word is the port's unwritten 0 (the exe reads its stale stack word).
        let (mut vm, _, _) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        assert_eq!(rotate(&mut vm, &rotate_info(&[])), Some(0));
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());
    }

    // ---- the `rotate(Vector3, F)` entry (`0x00482280`) ----------------

    /// The Rodrigues helper at a zero angle is the identity, and bit-exactly
    /// so: `fsin 0` is `+0` and `fcos 0` is `1`, so `t = 0` and every
    /// off-diagonal entry is a sum of `+0` products. There is no `FCHS` in
    /// this helper (unlike `0x54ecd0`'s `-sin(p)`), so `m5` here is `+0.0`,
    /// where the ypr matrix's `m5` is `-0.0`.
    #[test]
    fn the_axis_angle_matrix_is_the_identity_at_zero_angle() {
        let m = Mat3::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), 0.0);
        assert_eq!(
            m.m.map(f32::to_bits),
            [0x3f80_0000, 0, 0, 0, 0x3f80_0000, 0, 0, 0, 0x3f80_0000,]
        );
        assert_eq!(m.m.map(f32::to_bits)[5], 0, "no FCHS: m5 is +0.0 here");
        assert_eq!(
            Mat3::from_ypr(Ypr::zero()).m.map(f32::to_bits)[5],
            (-0.0f32).to_bits()
        );
        // The axis length is irrelevant at a zero angle, and a normalised or
        // un-normalised axis therefore agree entry for entry.
        assert_eq!(
            Mat3::from_axis_angle(Vec3::new(3.0, 4.0, 12.0), 0.0),
            Mat3::from_axis_angle(Vec3::new(0.6, 0.8, 0.0), 0.0)
        );
    }

    /// The first guard (`len² < 1e-6f`) makes `0x54ee80` return without
    /// writing anything at all. In the exe the three dot rows then read the
    /// caller's own frame, which the probe shows still holds the **previous**
    /// call's matrix (`re/out/probe_rotate_axis.py`: a real rotation followed
    /// by a zero axis returns the first rotated triple again). No port can
    /// reproduce a stale frame, so [`Mat3::from_axis_angle`] hands the
    /// helper the zeroed matrix it would have on a clean stack; this test
    /// pins that choice plus the boundary (a squared length of exactly
    /// `1e-6f` is **not** below the epsilon, so it writes).
    #[test]
    fn the_axis_angle_helper_gives_up_on_a_near_zero_axis() {
        assert_eq!(Mat3::from_axis_angle(Vec3::zero(), 1.0).m, [0.0; 9]);
        assert_eq!(
            Mat3::from_axis_angle(Vec3::new(1e-4, 0.0, 0.0), 1.0).m,
            [0.0; 9]
        );
        // `1e-3f32 * 1e-3f32` rounds to `0x358637be`, one ulp **above** the
        // epsilon (`0x358637bd`), so the helper writes.
        let sq =
            X87::from_f32(1e-3).mul(X87::from_f32(1e-3), Precision::F32, Rounding::NearestEven);
        assert_eq!(
            sq.to_f32(Precision::F32, Rounding::NearestEven).to_bits(),
            0x3586_37be
        );
        assert_ne!(
            Mat3::from_axis_angle(Vec3::new(1e-3, 0.0, 0.0), 1.0).m,
            [0.0; 9]
        );
    }

    /// The second guard normalises the axis only when `|len² - 1| ≥ 1e-6f`
    /// (and the compare is ordered), so an axis that is already unit is used
    /// as it was read, and a scaled one is scaled by `1/|k|` before the
    /// products. `(2, 0, 0)` and `(1, 0, 0)` therefore agree exactly —
    /// `inv = 1/2` is exact — while a rotated copy of a unit axis with a
    /// f32-inexact inverse does not have to.
    #[test]
    fn the_axis_angle_helper_normalises_only_a_non_unit_axis() {
        let angle = 1.0;
        assert_eq!(
            Mat3::from_axis_angle(Vec3::new(2.0, 0.0, 0.0), angle),
            Mat3::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), angle)
        );
        // `len² - 1` below the epsilon leaves the axis alone: `1.0000001f32`
        // squares to a length whose `|len² - 1|` is ~2.4e-7, so its matrix
        // keeps the un-normalised `x` and differs from the unit axis's.
        let unit = Mat3::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), angle);
        let near = Mat3::from_axis_angle(Vec3::new(1.0 + 1e-7, 0.0, 0.0), angle);
        assert_ne!(unit, near);
        assert_eq!(
            near.m[3].to_bits(),
            Mat3::from_axis_angle(Vec3::new(1.0 + 1e-7, 0.0, 0.0), angle).m[3].to_bits()
        );
        // `(3, 4, 0)` normalises to `1/5` and `3/5`, `4/5` — the same three
        // `f32` words `(0.6, 0.8, 0)` reads from the object, so the two
        // matrices agree bit for bit.
        assert_eq!(
            Mat3::from_axis_angle(Vec3::new(3.0, 4.0, 0.0), angle),
            Mat3::from_axis_angle(Vec3::new(0.6, 0.8, 0.0), angle)
        );
    }

    /// One nonzero-angle row pinned to the **exe's own** payload words: the
    /// probe (`re/out/probe_rotate_axis.py`) ran `0x54ee80` on `(0.6, 0.8, 0)`
    /// with angle `-1.25` and the port reproduced all nine words. The row
    /// also discriminates the trig **width**: `0x54ee80` never stores
    /// `sin`/`cos` (only `t = 1 - cos` goes to `[esp+0x20]`), so their low
    /// bits reach the products and a model that rounds them to their `f32`
    /// images first lands on `0x3ea84431` for `m1`/`m3` instead of the exe's
    /// `0x3ea84433` (and on 92/200 random native rows instead of 200/200).
    #[test]
    fn the_axis_angle_matrix_reproduces_the_probed_exe_words() {
        let m = Mat3::from_axis_angle(Vec3::new(0.6, 0.8, 0.0), -1.25);
        assert_eq!(
            m.m.map(f32::to_bits),
            [
                0x3f0f_d28a,
                0x3ea8_4433,
                0xbf42_5a20,
                0x3ea8_4433,
                0x3f40_e66e,
                0x3f11_c398,
                0x3f42_5a20,
                0xbf11_c398,
                0x3ea1_71ef,
            ]
        );
    }

    /// The whole native on probed rows: `0x482280` reads the receiver's
    /// `z`,`y`,`x` **first** (the opposite order from `rotate(Ypr)`) and the
    /// axis' `z`,`y`,`x` second, dots the receiver with the Rodrigues rows in
    /// the probed accumulation order and writes `x`,`y`,`z` back into the
    /// receiver. The probe pinned 12/13 case rows and 200/200 random rows to
    /// the port's f32 model; these are four of them.
    #[test]
    fn rotate_about_an_axis_reproduces_the_probed_exe_payload_words() {
        let quarter = std::f32::consts::FRAC_PI_2;
        // The zero-angle identity through the whole native: 3-4-0 unchanged.
        assert_eq!(
            Vec3::new(3.0, 4.0, 0.0)
                .rotate_axis(Vec3::new(0.0, 0.0, 1.0), 0.0)
                .to_array()
                .map(f32::to_bits),
            [3.0f32.to_bits(), 4.0f32.to_bits(), 0.0f32.to_bits()]
        );
        // A quarter turn about +z: (1, 2, 3) -> (-2, ~1, ~3). The `c` entry
        // is `cos(f32 π/2)`, so `y'` and `z'` are one f32 ulp short of 1 and
        // 3 rather than exactly those values.
        assert_eq!(
            Vec3::new(1.0, 2.0, 3.0)
                .rotate_axis(Vec3::new(0.0, 0.0, 1.0), quarter)
                .to_array()
                .map(f32::to_bits),
            [0xc000_0000, 0x3f7f_ffff, 0x403f_ffff]
        );
        // The +x axis turns (1, 2, 3) about x: y' = -3, z' = 2 to the same
        // one-ulp accuracy.
        assert_eq!(
            Vec3::new(1.0, 2.0, 3.0)
                .rotate_axis(Vec3::new(1.0, 0.0, 0.0), quarter)
                .to_array()
                .map(f32::to_bits),
            [0x3f7f_ffff, 0xc040_0000, 0x3fff_ffff]
        );
        // A random-angle row off the probe's table.
        assert_eq!(
            Vec3::new(1.0, 2.0, 3.0)
                .rotate_axis(Vec3::new(0.6, 0.8, 0.0), 0.75)
                .to_array()
                .map(f32::to_bits),
            [0x402e_31da, 0x3f35_6a73, 0x401d_ef28]
        );
    }

    /// The five non-finite rows the probe ran. `0 * inf` is the x87
    /// indefinite (sign set), a NaN component propagates through every row,
    /// and an infinite axis or angle makes the matrix's own arithmetic NaN.
    #[test]
    fn rotate_about_an_axis_follows_x87_on_the_special_values() {
        let bits = |v: Vec3| v.to_array().map(f32::to_bits);
        // A zero vector comes back as the `0 * component` rows: `0 * 2.5` is
        // `+0`, and `0 * -1` is `-0`, which the row's addition of `+0`
        // products turns back into `+0`.
        assert_eq!(
            bits(Vec3::zero().rotate_axis(Vec3::new(0.0, 1.0, 0.0), 1.0)),
            [0, 0, 0]
        );
        // An infinite component through the **guard arm**: the port's zero
        // matrix still multiplies it, and `0 * inf` is the x87 indefinite
        // (`0xffc00000`, sign set) in every row that touches the infinity —
        // so all three components come out as that NaN. (The exe instead
        // dots against the frame the previous call left behind.)
        assert_eq!(
            bits(Vec3::new(f32::INFINITY, -1.0, 2.5).rotate_axis(Vec3::zero(), 0.0)),
            [0xffc0_0000, 0xffc0_0000, 0xffc0_0000]
        );
        assert_eq!(
            bits(Vec3::new(f32::NAN, 1.0, 2.0).rotate_axis(Vec3::new(1.0, 0.0, 0.0), 0.0)),
            [0x7fc0_0000, 0x7fc0_0000, 0x7fc0_0000]
        );
        // An infinite axis normalises: `1/|k|` is `+0`, so `inf * 0` is the
        // x87 indefinite (`0xffc00000`, the default NaN with the sign set)
        // and every matrix entry that touches it is that NaN.
        assert_eq!(
            bits(Vec3::new(1.0, 2.0, 3.0).rotate_axis(Vec3::new(f32::INFINITY, 0.0, 0.0), 1.0)),
            [0xffc0_0000, 0xffc0_0000, 0xffc0_0000]
        );
        assert_eq!(
            bits(Vec3::new(1.0, 2.0, 3.0).rotate_axis(Vec3::new(0.0, 0.0, 1.0), f32::INFINITY)),
            [0x7fc0_0000, 0x7fc0_0000, 0x7fc0_0000]
        );
    }

    /// A host with one synthetic `java.lang.Vector3` class and two objects of
    /// it — the receiver and the axis — plus the `F` cell the angle rides in,
    /// in the container order `marshal_args` walks (`data[count-1]` is the
    /// receiver, `data[count-2]` the axis, `data[count-3]` the angle).
    fn axis_host(
        fields: &[FieldSpec<'_>],
        components: &[f32],
        axis: &[f32],
        angle: f32,
    ) -> (VmHost, u32, u32, [Value; 3]) {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: VECTOR3.to_owned(),
            base: None,
            interfaces: Vec::new(),
            methods: Vec::new(),
            fields: fields
                .iter()
                .map(|(name, ty, is_static)| Field {
                    class: VECTOR3.to_owned(),
                    name: (*name).to_owned(),
                    ty: (*ty).to_owned(),
                    is_static: *is_static,
                })
                .collect(),
            bodies: Vec::new(),
        });
        let receiver = vec3_object(&mut vm, components);
        let axis = vec3_object(&mut vm, axis);
        let args = [
            Value::float(angle),
            Value::reference(axis),
            Value::reference(receiver),
        ];
        (vm, receiver, axis, args)
    }

    fn rotate_axis_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "rotate",
            sig: "(Ljava.lang.Vector3;F)Ljava.lang.Vector3;",
        }
    }

    /// The native reads both objects by field **name**: the receiver is
    /// `container[count-1]` (`out[0]`), the axis `container[count-2]`
    /// (`out[1]`) and the angle the `F` payload of `container[count-3]`
    /// (`out[2]`), the container order `marshal_args` walks. The receiver
    /// comes back as the `L` word and is the only object written, and the
    /// roles follow the two cells rather than the two objects.
    #[test]
    fn the_axis_rotate_native_reads_both_operands_by_name() {
        // Declared `z, x, y` against slots `0, 3, 4`: by name the receiver is
        // x = 0, y = 1, z = 0 and the axis x = 1, y = 0, z = 0. A quarter
        // turn about +x sends the receiver's y to z and its z to -y.
        let quarter = std::f32::consts::FRAC_PI_2;
        let (mut vm, receiver, axis, args) = axis_host(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 0.0, 1.0],
            &[0.0, 1.0, 0.0],
            quarter,
        );
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&args)),
            Some(receiver)
        );
        // `marshal_args` order: the receiver is the last cell, the axis the
        // one below it, and the angle's payload word the one below that.
        assert_eq!(args[0], Value::float(quarter));
        assert_eq!(args[1], Value::reference(axis));
        assert_eq!(args[2], Value::reference(receiver));
        // The receiver is (0, 1, 0) and the axis (1, 0, 0) **by name**, in
        // slots `z, x, y`. A quarter turn about +x sends `+y` to `+z`, so the
        // three slots hold `z' = m7`, `x' = m1` and `y' = m4` — the row-0
        // order `m1*y + m0*x + m2*z` read off `0x482331`.
        let m = Mat3::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), quarter);
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots[0], m.m[7].to_bits(), "z' = s");
        assert_eq!(slots[1], 0, "x' = m1, and `m1` is a zero product here");
        assert_eq!(slots[2], m.m[4].to_bits(), "y' = cos(f32 pi/2)");
        assert_eq!(slots[2], (-4.371_139e-8f32).to_bits());
        // The axis object is read, never written.
        assert_eq!(
            vm.objects[1]
                .fields
                .iter()
                .map(|v| v.as_f32())
                .collect::<Vec<_>>(),
            [0.0, 1.0, 0.0]
        );

        // Swapping the two cells swaps the roles: the object that held
        // `(1, 0, 0)` is now the receiver and `(0, 1, 0)` the axis, so a
        // quarter turn about +y sends `+x` to `-z`, and the other object is
        // untouched.
        let (mut vm, receiver, axis, _) = axis_host(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 0.0, 1.0],
            &[0.0, 1.0, 0.0],
            quarter,
        );
        let swapped = [
            Value::float(quarter),
            Value::reference(receiver),
            Value::reference(axis),
        ];
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&swapped)),
            Some(axis)
        );
        let m = Mat3::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), quarter);
        let turned: Vec<u32> = vm.objects[1]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(turned[0], m.m[6].to_bits(), "z' = -s");
        assert_eq!(turned[0], (-1.0f32).to_bits());
        assert_eq!(turned[1], m.m[0].to_bits(), "x' = cos(f32 pi/2)");
        assert_eq!(turned[2], 0, "y' = m3, a zero product here");
        assert_eq!(
            vm.objects[0]
                .fields
                .iter()
                .map(|v| v.as_f32())
                .collect::<Vec<_>>(),
            [0.0, 0.0, 1.0]
        );
    }

    /// The receiver is the object the writes land in, in the exe's write
    /// order (`x`, `y`, `z`), and the return word is that same word: like
    /// `rotate(Ypr)`, the receiver cell *is* the return cell.
    #[test]
    fn the_axis_rotate_native_writes_the_receiver_in_place() {
        let quarter = std::f32::consts::FRAC_PI_2;
        let (mut vm, receiver, _, args) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[0.0, 0.0, 4.0],
            &[1.0, 0.0, 0.0],
            quarter,
        );
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&args)),
            Some(receiver)
        );
        // A quarter turn about +x sends `+z` to `-y`: the rows give
        // `x' = m2*z = +0`, `y' = m5*z = -4` and `z' = m8*z = cos*z`.
        let m = Mat3::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), quarter);
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|v| v.as_f32().to_bits())
            .collect();
        assert_eq!(slots[0], 0, "x' = m2*z, and `m2` is a zero product");
        assert_eq!(slots[1], (-4.0f32).to_bits(), "y' = m5*z");
        // The row multiplies `m8` by the receiver's `z = 4`, which is exact.
        assert_eq!(slots[2], (m.m[8] * 4.0).to_bits(), "z' = m8*z");
        assert_eq!(slots[2], 0xb43b_bd2e);
    }

    /// The miss paths, which differ from `rotate(Ypr)`'s: the axis word is a
    /// third container slot, so a short container leaves **both** the axis
    /// and the angle at 0 — a zero axis takes the helper's guard arm, and the
    /// port then applies its zero matrix, so every finite component of the
    /// receiver collapses to `+0`. The exe instead dots against the frame the
    /// previous call left behind (probe-verified), which the port cannot
    /// state; there is no "identity" fallback here.
    #[test]
    fn axis_rotate_misses_read_zero_like_get_float_field() {
        // The axis object word is outside the heap: three misses read the
        // zero vector, so the guard trips and the receiver is zeroed.
        let (mut vm, receiver, _, _) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
            &[0.0, 0.0, 1.0],
            0.0,
        );
        let args = [
            Value::float(0.0),
            Value::reference(0x7fff_0000),
            Value::reference(receiver),
        ];
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&args)),
            Some(receiver)
        );
        assert_eq!(
            vm.objects[0]
                .fields
                .iter()
                .map(|v| v.as_f32().to_bits())
                .collect::<Vec<_>>(),
            [0, 0, 0]
        );

        // A container with only the receiver cell: the axis and angle words
        // stay 0, which is the zero axis and the zero angle — the guard still
        // wins, so this is not the identity rotation `rotate(Ypr)` gives.
        let (mut vm, receiver, _, _) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
            &[0.0, 0.0, 1.0],
            0.0,
        );
        let short = [Value::reference(receiver)];
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&short)),
            Some(receiver)
        );
        assert_eq!(
            vm.objects[0]
                .fields
                .iter()
                .map(|v| v.as_f32().to_bits())
                .collect::<Vec<_>>(),
            [0, 0, 0]
        );

        // An unallocated receiver word: the reads are zero, the writes miss
        // (the same name lookup) and the word is still the `L` return.
        let (mut vm, _, axis, _) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
            &[0.0, 0.0, 1.0],
            0.0,
        );
        let args = [
            Value::float(0.0),
            Value::reference(axis),
            Value::reference(0x7fff_0000),
        ];
        assert_eq!(
            rotate_axis(&mut vm, &rotate_axis_info(&args)),
            Some(0x7fff_0000)
        );
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());

        // An empty container has no receiver, no axis and no angle word: the
        // return word is the port's unwritten 0 (the exe reads its stale
        // stack word).
        let (mut vm, _, _, _) = axis_host(
            &[(X, "F", false), (Y, "F", false), (Z, "F", false)],
            &[3.0, 4.0, 0.0],
            &[0.0, 0.0, 1.0],
            0.0,
        );
        assert_eq!(rotate_axis(&mut vm, &rotate_axis_info(&[])), Some(0));
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());
    }

    // ---- the `Vector3.<init>(Ypr)` entry (`0x00482020`) ----------------

    fn init_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: VECTOR3,
            name: "<init>",
            sig: "(Ljava.lang.Ypr;)",
        }
    }

    /// The direction of a zero `Ypr` is `(-0.0, +0.0, -1.0)`: the matrix's
    /// third column is `(cp*sy, -sp, cp*cy) = (+0.0, -0.0, 1.0)` and each
    /// entry is negated by an `fmul` with `-1.0f`, which turns the matrix's
    /// `-0.0` into `+0.0` (a `0.0 - v` model would leave `+0.0` at `+0.0`
    /// and the matrix's `+0.0` at `-0.0`, i.e. the other way round here).
    #[test]
    fn from_ypr_is_the_negated_third_column_of_the_ypr_matrix() {
        let v = Vec3::from_ypr(Ypr::zero());
        assert_eq!(v.x.to_bits(), 0x8000_0000, "x = -(1 * +0.0)");
        assert_eq!(v.y.to_bits(), 0x0000_0000, "y = -(-sin(0))");
        assert_eq!(v.z.to_bits(), (-1.0f32).to_bits(), "z = -(1 * cos(0))");

        // With `p = 0` the matrix's `cp` is exactly `1.0`, so its third
        // column is the `fsin`/`fcos` result itself and only the sign flip
        // is left: x = -sin(y), y = +0.0, z = -cos(y).
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let sy = trans::approx_sin(X87::from_f32(0.75)).value.to_f32(pc, rc);
        let cy = trans::approx_cos(X87::from_f32(0.75)).value.to_f32(pc, rc);
        let v = Vec3::from_ypr(Ypr::new(0.75, 0.0, -2.5));
        assert_eq!(v.x.to_bits(), (-sy).to_bits());
        assert_eq!(v.y.to_bits(), 0.0f32.to_bits());
        assert_eq!(v.z.to_bits(), (-cy).to_bits());

        // `m2`, `m5` and `m8` are the only entries the native loads, so the
        // roll angle (which reaches only the first two columns) cannot
        // change the vector: every `r` gives the same three words.
        let base = Vec3::from_ypr(Ypr::new(0.25, -0.5, 0.0));
        for r in [-2.5f32, 1.0, 7.75] {
            assert_eq!(Vec3::from_ypr(Ypr::new(0.25, -0.5, r)), base);
        }
        assert_eq!(base.to_array().map(f32::to_bits), {
            let v = Vec3::from_ypr(Ypr::new(0.25, -0.5, 3.0));
            v.to_array().map(f32::to_bits)
        });
    }

    /// `this` is the container's top cell (`out[0]`) and the declared `Ypr`
    /// the one below it (`out[1]`), the exe's own stack order; the direction
    /// lands in the receiver's fields and the native returns no word (its
    /// descriptor declares no return type).
    #[test]
    fn the_init_native_writes_the_direction_into_the_receiver() {
        let (mut vm, receiver, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.0, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), 0x8000_0000);
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0x0000_0000);
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), (-1.0f32).to_bits());

        // The receiver is `container[count-1]`: swapping the two cells makes
        // the Ypr the receiver and the vector the source. The vector's `y`
        // (`4.0`) is then the yaw and its `p`/`r` reads miss (`+0.0`), so
        // the direction's `y` (`sin 0`) overwrites the Ypr's own `y` slot
        // (`1.5`) while the vector itself is left untouched.
        let (mut vm, receiver, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[1.5, 0.25, -3.0]);
        let args = [Value::reference(receiver), Value::reference(ypr)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&args)), None);
        assert_eq!(float_field(&mut vm, 1, Y).to_bits(), 0x0000_0000);
        assert_eq!(float_field(&mut vm, 1, P).to_bits(), 0.25f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), 3.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 4.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), 0.0f32.to_bits());
    }

    /// Both objects are read by field **name** (the exe's cstring compare),
    /// so the declared order is irrelevant: the angles come out of the `Ypr`
    /// argument's `y`, `p`, `r` cells wherever they sit in the container,
    /// and the three results go back to the receiver's `x`, `y`, `z` slots.
    #[test]
    fn init_reads_the_argument_angles_by_name() {
        // Declared `p, r, y` against slots `0.0, 5.0, 0.5`: by name the
        // argument is y = 0.5, p = 0, r = 5, and with p = 0 exactly the
        // direction is (-sin(0.5), +0.0, -cos(0.5)). The receiver is
        // declared `z, x, y` against slots `0.0, 3.0, 4.0`, so its writes
        // land at slots 2 (`x`), 3 (`y`) and 1 (`z`).
        let (mut vm, receiver, ypr) = rotate_host_with(
            &[(Z, "F", false), (X, "F", false), (Y, "F", false)],
            &[0.0, 3.0, 4.0],
            &[(P, "F", false), (R, "F", false), (Y, "F", false)],
            &[0.0, 5.0, 0.5],
        );
        let (pc, rc) = (Precision::F32, Rounding::NearestEven);
        let s = trans::approx_sin(X87::from_f32(0.5)).value.to_f32(pc, rc);
        let c = trans::approx_cos(X87::from_f32(0.5)).value.to_f32(pc, rc);
        let args = [Value::reference(ypr), Value::reference(receiver)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), (-s).to_bits());
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), (-c).to_bits());
        let slots: Vec<u32> = vm.objects[0]
            .fields
            .iter()
            .map(|value| value.as_f32().to_bits())
            .collect();
        assert_eq!(slots, [(-c).to_bits(), (-s).to_bits(), 0x0000_0000]);
    }

    /// Every `get_float_field` miss answers `+0.0f` and a `set_float_field`
    /// miss writes nothing, so a word the port cannot follow leaves the host
    /// alone; a container that leaves the argument word at 0 reads the zero
    /// `Ypr`. The exe would read its own stale stack word for a short
    /// container, which the port cannot state truthfully.
    #[test]
    fn init_operand_misses_follow_get_float_field() {
        // A receiver word outside the heap and an argument that hits: the
        // three writes all miss, nothing changes.
        let (mut vm, _receiver, ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.5, 0.0, 0.0]);
        let args = [Value::reference(ypr), Value::reference(0x7fff_0000)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&args)), None);
        assert_eq!(vm.objects[0].fields[0].as_f32().to_bits(), 3.0f32.to_bits());
        assert_eq!(vm.objects[0].fields[1].as_f32().to_bits(), 4.0f32.to_bits());

        // Only the receiver cell: the unwritten argument word stays 0, so
        // the port reads the zero angle triple.
        let (mut vm, receiver, _ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.5, 0.0, 0.0]);
        let short = [Value::reference(receiver)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&short)), None);
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), 0x8000_0000);
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), (-1.0f32).to_bits());

        // A `Ypr` word outside the heap: all three angle reads miss (+0.0),
        // which is the zero triple again.
        let (mut vm, receiver, _ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.5, 0.0, 0.0]);
        let args = [Value::reference(0x7fff_0000), Value::reference(receiver)];
        assert_eq!(init_from_ypr(&mut vm, &init_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), (-1.0f32).to_bits());

        // An empty container has no receiver either, so nothing is written.
        let (mut vm, _receiver, _ypr) = rotate_host(&[3.0, 4.0, 0.0], &[0.5, 0.0, 0.0]);
        assert_eq!(init_from_ypr(&mut vm, &init_info(&[])), None);
        assert_eq!(vm.objects[0].fields[2].as_f32().to_bits(), 0.0f32.to_bits());
    }

    // ---- the `Ypr.<init>(Vector3)` / `Ypr.mul` rows (0x482610/0x4826c0) --

    fn ypr_init_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: YPR,
            name: "<init>",
            sig: "(Ljava.lang.Vector3;)",
        }
    }

    fn ypr_mul_info(args: &[Value]) -> CallInfo<'_> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: YPR,
            name: "mul",
            sig: "(Ljava.lang.Ypr;)V",
        }
    }

    /// A host with one synthetic `java.lang.Ypr` class and two objects of it
    /// (`receiver` first, so its word is the lower one) holding the two
    /// triples in `fields` order.
    fn ypr_pair(
        fields: &[FieldSpec<'_>],
        receiver: &[f32],
        argument: &[f32],
    ) -> (VmHost, u32, u32) {
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: YPR.to_owned(),
            base: None,
            interfaces: Vec::new(),
            methods: Vec::new(),
            fields: fields
                .iter()
                .map(|(name, ty, is_static)| Field {
                    class: YPR.to_owned(),
                    name: (*name).to_owned(),
                    ty: (*ty).to_owned(),
                    is_static: *is_static,
                })
                .collect(),
            bodies: Vec::new(),
        });
        let receiver = ypr_object(&mut vm, receiver);
        let argument = ypr_object(&mut vm, argument);
        (vm, receiver, argument)
    }

    /// Absolute `f32` tolerance for the APPROX transcendental outputs
    /// (`fsin`/`fcos`/`fpatan`, [`slr_x87::trans`]); NaN matches NaN.
    fn close(got: f32, want: f32, tol: f32) -> bool {
        (got.is_nan() && want.is_nan()) || (got - want).abs() <= tol
    }

    /// The degenerate `x = z = 0` branch writes constants (`0`, `±pi/2`, `0`):
    /// only a strictly positive `y` takes `+pi/2`; `y == 0`, `y < 0` and an
    /// unordered `y` all take `-pi/2` (`test ah,0x41` = C0|C3).
    #[test]
    fn to_ypr_degenerate_branch_writes_the_pitch_constants() {
        let bits = |v: Vec3| v.to_ypr().to_array().map(f32::to_bits);
        assert_eq!(
            bits(Vec3::new(0.0, 1.0, 0.0)),
            [0x0000_0000, HALF_PI.to_bits(), 0x0000_0000]
        );
        assert_eq!(
            bits(Vec3::new(0.0, -1.0, 0.0)),
            [0x0000_0000, NEG_HALF_PI.to_bits(), 0x0000_0000]
        );
        assert_eq!(
            Vec3::new(0.0, 0.0, 0.0).to_ypr().p.to_bits(),
            NEG_HALF_PI.to_bits()
        );
        assert_eq!(
            Vec3::new(0.0, f32::NAN, 0.0).to_ypr().p.to_bits(),
            NEG_HALF_PI.to_bits()
        );
        // A `-0.0` axis compares equal to `0.0` (`fcomp`), so it is still the
        // degenerate branch.
        assert_eq!(
            Vec3::new(-0.0, 2.0, -0.0).to_ypr().p.to_bits(),
            HALF_PI.to_bits()
        );
    }

    /// The general path is `y = atan2(-x, -z)`, `p = atan2(y, sqrt(x*x +
    /// z*z))` and always `r = 0`; `x` is added before `z` under the
    /// `fsqrt`.
    #[test]
    fn to_ypr_general_path_is_atan2_of_the_negated_axes() {
        // `(0, 2, -2)`: yaw = atan2(-0, +2) = -0.0, pitch = atan2(2, 2).
        let ypr = Vec3::new(0.0, 2.0, -2.0).to_ypr();
        assert_eq!(ypr.y.to_bits(), (-0.0f32).to_bits());
        assert!(
            close(ypr.p, std::f32::consts::FRAC_PI_4, 2.0e-6),
            "{}",
            ypr.p
        );
        assert_eq!(ypr.r.to_bits(), 0.0f32.to_bits());
        // `(3, 4, 0)`: `z == 0` but `x != 0`, so the general path still runs:
        // yaw = atan2(-3, -0) = -pi/2, pitch = atan2(4, 3).
        let ypr = Vec3::new(3.0, 4.0, 0.0).to_ypr();
        assert!(
            close(ypr.y, -std::f32::consts::FRAC_PI_2, 2.0e-6),
            "{}",
            ypr.y
        );
        assert!(close(ypr.p, (4.0f32).atan2(3.0), 2.0e-6), "{}", ypr.p);
        assert_eq!(ypr.r.to_bits(), 0.0f32.to_bits());
    }

    /// `m8 = m2 = 0` writes the `0x40490fdb` immediate (`pi`, **not** `pi/2`)
    /// for the pitch, `0` for the roll, and `y = atan2(m0, -m1)`.
    #[test]
    fn mat3_to_ypr_degenerate_branch_writes_the_pi_immediate() {
        let m = Mat3 {
            m: [0.5, -0.25, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        };
        let ypr = m.to_ypr();
        assert_eq!(ypr.p.to_bits(), PI.to_bits());
        assert_ne!(ypr.p.to_bits(), HALF_PI.to_bits(), "the immediate is pi");
        assert_eq!(ypr.r.to_bits(), 0.0f32.to_bits());
        assert!(close(ypr.y, (0.5f32).atan2(0.25), 2.0e-6), "{}", ypr.y);
        // `m8 = -0.0` also compares equal to `0.0`.
        let m = Mat3 {
            m: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, -0.0],
        };
        assert_eq!(m.to_ypr().p.to_bits(), PI.to_bits());
    }

    /// The `fcom` against `0.0` sets the flags the branch reads **before**
    /// the `fmul m5`: a negative `m4` makes `cos(r) <= 0`, so the exe takes
    /// `atan2(prod, -m4)` — pinned against the exe by
    /// `re/out/probe_toypr.py` (which returned `-0.4636` for this matrix, not
    /// the `+2.6779` the other arm would give).
    #[test]
    fn mat3_to_ypr_takes_the_negative_cosine_arm_for_a_negative_m4() {
        let m = Mat3 {
            m: [1.0, 0.0, 0.0, 0.0, -1.0, 0.5, 0.0, 0.0, 1.0],
        };
        let ypr = m.to_ypr();
        assert!(close(ypr.p, (-0.5f32).atan2(1.0), 2.0e-6), "{}", ypr.p);
        assert!(close(ypr.r, std::f32::consts::PI, 2.0e-6), "{}", ypr.r);
    }

    /// `0x54eab0` computes `self = left * self` (the left operand's rows
    /// against `self`'s columns), not a commutative product: checked against
    /// an independent `f64` matrix multiply.
    #[test]
    fn premul_is_left_times_self() {
        let a = Mat3 {
            m: [0.6, -0.8, 0.0, 0.8, 0.6, 0.0, 0.0, 0.0, 1.0],
        };
        let b = Mat3 {
            m: [1.0, 0.0, 0.0, 0.0, 0.5, -0.866_025_4, 0.0, 0.866_025_4, 0.5],
        };
        let mut m = a;
        m.premul(b);
        for i in 0..3 {
            for j in 0..3 {
                let want: f64 = (0..3)
                    .map(|k| f64::from(b.m[3 * i + k]) * f64::from(a.m[3 * k + j]))
                    .sum();
                assert!(
                    (f64::from(m.m[3 * i + j]) - want).abs() < 1.0e-5,
                    "({i},{j}) = {} want {want}",
                    m.m[3 * i + j]
                );
            }
        }
    }

    /// `this` is the container's top cell (`out[0]`, the `Ypr`) and the
    /// declared `Vector3` the one below it (`out[1]`): the triple comes out
    /// of the **vector** and lands in the receiver's `y`/`p`/`r`.
    #[test]
    fn the_ypr_init_native_writes_the_triple_into_the_receiver() {
        let (mut vm, vector, ypr) = rotate_host(&[0.0, 2.0, -2.0], &[9.0, 9.0, 9.0]);
        let args = [Value::reference(vector), Value::reference(ypr)];
        assert_eq!(ypr_init_from_vec3(&mut vm, &ypr_init_info(&args)), None);
        assert_eq!(float_field(&mut vm, 1, Y).to_bits(), (-0.0f32).to_bits());
        assert!(close(
            float_field(&mut vm, 1, P),
            std::f32::consts::FRAC_PI_4,
            2.0e-6
        ));
        assert_eq!(float_field(&mut vm, 1, R).to_bits(), 0.0f32.to_bits());
        // The vector argument is read only.
        assert_eq!(float_field(&mut vm, 0, X).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, Z).to_bits(), (-2.0f32).to_bits());
    }

    /// The composition is `argument_matrix * receiver_matrix`: two pure-roll
    /// triples add their angle (the planar product is commutative), and
    /// swapping the two cells changes the result for a non-commuting pair.
    #[test]
    fn the_ypr_mul_native_composes_the_two_triples() {
        // Zero angles: both sides are the identity, so the triple is zero.
        let (mut vm, receiver, argument) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0],
        );
        let args = [Value::reference(argument), Value::reference(receiver)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, R).to_bits(), 0.0f32.to_bits());

        // Pure rolls add: `r = 0.3` (receiver) and `r = 0.4` (argument).
        let (mut vm, receiver, argument) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.0, 0.0, 0.3],
            &[0.0, 0.0, 0.4],
        );
        let args = [Value::reference(argument), Value::reference(receiver)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        assert!(close(float_field(&mut vm, 0, R), 0.7, 2.0e-6));
        assert!(close(float_field(&mut vm, 0, Y), 0.0, 2.0e-6));

        // A yaw on the argument and a roll on the receiver do not commute:
        // swapping the two cells must change the receiver's `y`.
        let (mut vm, a, b) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.25, 0.0, 0.4],
            &[0.7, 0.0, 0.0],
        );
        let args = [Value::reference(b), Value::reference(a)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        let forward = [
            float_field(&mut vm, 0, Y),
            float_field(&mut vm, 0, P),
            float_field(&mut vm, 0, R),
        ];
        let (mut vm, a, b) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.25, 0.0, 0.4],
            &[0.7, 0.0, 0.0],
        );
        let args = [Value::reference(a), Value::reference(b)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        let swapped = [
            float_field(&mut vm, 0, Y),
            float_field(&mut vm, 0, P),
            float_field(&mut vm, 0, R),
        ];
        assert_ne!(
            forward.map(f32::to_bits),
            swapped.map(f32::to_bits),
            "the argument is the left-hand matrix, so order matters"
        );
    }

    /// Both triples are read by field **name**; a word the port cannot follow
    /// reads as the zero triple, exactly as three `get_float_field` misses.
    #[test]
    fn ypr_mul_reads_both_triples_by_name() {
        // Declared `p, r, y` against slots `0.0, 0.4, 0.0` for the argument:
        // by name that is `y = 0, p = 0, r = 0.4`, a pure roll. The receiver
        // is the zero triple, so the composed roll is exactly the argument's
        // `r` (a pure-roll planar product) and `y` stays zero.
        let (mut vm, receiver, argument) = ypr_pair(
            &[(P, "F", false), (R, "F", false), (Y, "F", false)],
            &[0.0, 0.0, 0.0],
            &[0.0, 0.4, 0.0],
        );
        let args = [Value::reference(argument), Value::reference(receiver)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0.0f32.to_bits());
        assert!(close(float_field(&mut vm, 0, R), 0.4, 2.0e-6));

        // A word outside the heap reads the zero triple; the receiver is then
        // composed with the identity and stays at zero.
        let (mut vm, receiver, _argument) = ypr_pair(
            &[(Y, "F", false), (P, "F", false), (R, "F", false)],
            &[0.0, 0.0, 0.0],
            &[0.0, 0.0, 0.0],
        );
        let args = [Value::reference(0x7fff_0000), Value::reference(receiver)];
        assert_eq!(ypr_mul(&mut vm, &ypr_mul_info(&args)), None);
        assert_eq!(float_field(&mut vm, 0, Y).to_bits(), 0.0f32.to_bits());
        assert_eq!(float_field(&mut vm, 0, R).to_bits(), 0.0f32.to_bits());
    }
}

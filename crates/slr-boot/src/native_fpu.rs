//! x87 register stack for instruction-ordered ports of floating-point code.
//!
//! Native code runs with the D3D9 control word (24-bit precision, round to
//! nearest), so every arithmetic result rounds its significand to 24 bits
//! while keeping the extended exponent range. Memory operands are dword
//! floats unless a method says otherwise. Comparisons return the status-word
//! condition bits exactly as `FNSTSW AX` exposes them in AH.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};

const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;

pub struct Fpu {
    st: Vec<X87>,
}

/// AH after `FNSTSW AX`: C0 = 0x01, C2 = 0x04, C3 = 0x40.
#[derive(Clone, Copy)]
pub struct Status(pub u8);
impl Status {
    /// `TEST AH, mask` nonzero.
    pub fn test(self, mask: u8) -> bool {
        self.0 & mask != 0
    }
}

impl Default for Fpu {
    fn default() -> Self {
        Self::new()
    }
}

// Retain the recovered instruction vocabulary as coherent source. Operations
// not yet reached by the current graph have no parity credit in the ledger.
#[expect(
    dead_code,
    reason = "instruction vocabulary extends the currently integrated graph"
)]
impl Fpu {
    pub fn new() -> Self {
        Self {
            st: Vec::with_capacity(8),
        }
    }
    pub fn depth(&self) -> usize {
        self.st.len()
    }
    fn top(&mut self, i: usize) -> &mut X87 {
        let n = self.st.len();
        &mut self.st[n - 1 - i]
    }
    pub fn get(&self, i: usize) -> X87 {
        self.st[self.st.len() - 1 - i]
    }
    pub fn push(&mut self, v: X87) {
        assert!(self.st.len() < 8, "x87 stack overflow");
        self.st.push(v);
    }
    pub fn pop(&mut self) -> X87 {
        self.st.pop().expect("x87 stack underflow")
    }
    pub fn fld(&mut self, m: &Space, a: u32) {
        self.push(X87::from_f32(m.rf(a)));
    }
    pub fn fld_bits(&mut self, bits: u32) {
        self.push(X87::from_f32(f32::from_bits(bits)));
    }
    pub fn fld_f64(&mut self, v: f64) {
        self.push(X87::from_f64(v));
    }
    pub fn fild(&mut self, v: i32) {
        self.push(X87::from_i32(v));
    }
    pub fn fld_st(&mut self, i: usize) {
        let v = self.get(i);
        self.push(v);
    }
    pub fn fld1(&mut self) {
        self.fld_bits(0x3f80_0000);
    }
    pub fn fldz(&mut self) {
        self.fld_bits(0);
    }
    /// FST m32 (no pop): the stored bits.
    pub fn fst(&mut self, m: &mut Space, a: u32) -> u32 {
        let bits = self.get(0).to_f32(PC, RC).to_bits();
        m.w32(a, bits);
        bits
    }
    pub fn fstp(&mut self, m: &mut Space, a: u32) -> u32 {
        let bits = self.fst(m, a);
        self.pop();
        bits
    }
    /// FSTP m32 into a value instead of memory.
    pub fn fstp_bits(&mut self) -> u32 {
        self.pop().to_f32(PC, RC).to_bits()
    }
    pub fn fst_bits(&self) -> u32 {
        self.get(0).to_f32(PC, RC).to_bits()
    }
    /// FSTP ST(i).
    pub fn fstp_st(&mut self, i: usize) {
        let v = self.get(0);
        *self.top(i) = v;
        self.pop();
    }
    pub fn fxch(&mut self, i: usize) {
        let n = self.st.len();
        self.st.swap(n - 1, n - 1 - i);
    }
    pub fn fchs(&mut self) {
        let mut bytes = self.get(0).to_bytes();
        bytes[9] ^= 0x80;
        *self.top(0) = X87::from_bytes(bytes);
    }
    pub fn fabs(&mut self) {
        let mut bytes = self.get(0).to_bytes();
        bytes[9] &= 0x7f;
        *self.top(0) = X87::from_bytes(bytes);
    }
    pub fn fsqrt(&mut self) {
        let v = self.get(0).sqrt(PC, RC);
        *self.top(0) = v;
    }

    fn op(a: X87, b: X87, op: Op) -> X87 {
        match op {
            Op::Add => a.add(b, PC, RC),
            Op::Sub => a.sub(b, PC, RC),
            Op::SubR => b.sub(a, PC, RC),
            Op::Mul => a.mul(b, PC, RC),
            Op::Div => a.div(b, PC, RC),
            Op::DivR => b.div(a, PC, RC),
        }
    }
    /// `F<op> m32`: ST(0) = ST(0) op m.
    pub fn arith_m(&mut self, op: Op, m: &Space, a: u32) {
        let v = Self::op(self.get(0), X87::from_f32(m.rf(a)), op);
        *self.top(0) = v;
    }
    /// `F<op> m32` with the operand's bits supplied directly.
    pub fn arith_m_bits(&mut self, op: Op, bits: u32) {
        let v = Self::op(self.get(0), X87::from_f32(f32::from_bits(bits)), op);
        *self.top(0) = v;
    }
    /// `F<op> m64`.
    pub fn arith_f64(&mut self, op: Op, v: f64) {
        let r = Self::op(self.get(0), X87::from_f64(v), op);
        *self.top(0) = r;
    }
    /// `F<op> ST(0), ST(i)`.
    pub fn arith_st0(&mut self, op: Op, i: usize) {
        let v = Self::op(self.get(0), self.get(i), op);
        *self.top(0) = v;
    }
    /// `F<op> ST(i), ST(0)`: ST(i) = ST(i) op ST(0).
    pub fn arith_sti(&mut self, op: Op, i: usize) {
        let v = Self::op(self.get(i), self.get(0), op);
        *self.top(i) = v;
    }
    /// `F<op>P ST(i), ST(0)`.
    pub fn arith_p(&mut self, op: Op, i: usize) {
        self.arith_sti(op, i);
        self.pop();
    }
    pub fn fadd(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::Add, m, a)
    }
    pub fn fsub(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::Sub, m, a)
    }
    pub fn fsubr(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::SubR, m, a)
    }
    pub fn fmul(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::Mul, m, a)
    }
    pub fn fdiv(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::Div, m, a)
    }
    pub fn fdivr(&mut self, m: &Space, a: u32) {
        self.arith_m(Op::DivR, m, a)
    }
    pub fn faddp(&mut self, i: usize) {
        self.arith_p(Op::Add, i)
    }
    pub fn fsubp(&mut self, i: usize) {
        self.arith_p(Op::Sub, i)
    }
    pub fn fsubrp(&mut self, i: usize) {
        self.arith_p(Op::SubR, i)
    }
    pub fn fmulp(&mut self, i: usize) {
        self.arith_p(Op::Mul, i)
    }
    pub fn fdivp(&mut self, i: usize) {
        self.arith_p(Op::Div, i)
    }
    pub fn fdivrp(&mut self, i: usize) {
        self.arith_p(Op::DivR, i)
    }

    fn status(a: X87, b: X87) -> Status {
        let (codes, _) = a.fcom(b);
        Status(u8::from(codes.c0) | (u8::from(codes.c2) << 2) | (u8::from(codes.c3) << 6))
    }
    /// FCOM m32 (no pop).
    pub fn fcom(&self, m: &Space, a: u32) -> Status {
        Self::status(self.get(0), X87::from_f32(m.rf(a)))
    }
    pub fn fcomp(&mut self, m: &Space, a: u32) -> Status {
        let s = self.fcom(m, a);
        self.pop();
        s
    }
    pub fn fcom_bits(&self, bits: u32) -> Status {
        Self::status(self.get(0), X87::from_f32(f32::from_bits(bits)))
    }
    pub fn fcomp_bits(&mut self, bits: u32) -> Status {
        let s = self.fcom_bits(bits);
        self.pop();
        s
    }
    pub fn fcom_f64(&self, v: f64) -> Status {
        Self::status(self.get(0), X87::from_f64(v))
    }
    pub fn fcomp_f64(&mut self, v: f64) -> Status {
        let s = self.fcom_f64(v);
        self.pop();
        s
    }
    /// FCOM ST(i) / FCOMP ST(i).
    pub fn fcom_st(&self, i: usize) -> Status {
        Self::status(self.get(0), self.get(i))
    }
    pub fn fcomp_st(&mut self, i: usize) -> Status {
        let s = self.fcom_st(i);
        self.pop();
        s
    }
    pub fn fcompp(&mut self) -> Status {
        let s = self.fcom_st(1);
        self.pop();
        self.pop();
        s
    }
    /// FTST.
    pub fn ftst(&self) -> Status {
        Self::status(self.get(0), X87::from_f32(0.0))
    }
}

#[derive(Clone, Copy)]
pub enum Op {
    Add,
    Sub,
    SubR,
    Mul,
    Div,
    DivR,
}

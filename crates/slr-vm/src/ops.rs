//! The TUFA operator and assignment helpers (the `Thread::run` sub-op
//! targets of opwords `0x1007` / `0x1008`, and the `0x002d` assignment).
//!
//! Each helper is a port of one exe function and works on the value-cell
//! model of [`crate::values`]: a cell is a [`Value`] (type tag + payload
//! word) and a helper mutates the cells it is given exactly like the exe
//! mutates the 12-byte cells it is passed. Operand order is the order the
//! exe functions take their parameters (`docs/tufa-executor.md`, "Operand
//! order"): binary helpers get `(LEFT, RIGHT)` and write the result into
//! LEFT; the two shifts get `(count, value)` and write into `value`.
//!
//! What is deliberately *not* here (it needs the class loader, ticket
//! #31): a [`Value`] carries only the first byte of its type name, so the
//! helpers cannot tell a `java.lang.String` from any other class. The
//! exe's `+` checks exactly that (`FUN_00417a70`) and concatenates through
//! the string pool; [`Alu::add`] treats every reference operand as "some
//! other class" (the exe's `unsupported class type` log, dst unchanged),
//! and the interpreter offers reference operands to its host first
//! (`Host::operator_on_refs`). Comparisons and plain stores of references
//! only read payload words and are ported.
//!
//! Error model. The exe has two kinds of failure paths:
//! * *log and continue* (`unsupported type`, `non integral operands`,
//!   `cannot promote`, `division by zero` for floats, ...): recorded in
//!   [`Alu::diagnostics`] as a [`Diagnostic`] naming the helper and the
//!   exe's message ([`LogKind`]); execution continues exactly as in the
//!   exe. The conditions were read off the pseudocode of every helper, not
//!   oracle-verified (the Unicorn log sinks are stubbed);
//! * *trap*: integer `idiv` of zero or `INT_MIN / -1` raises `#DE` after the
//!   log line. The port returns [`OpError::IntegerTrap`] and the caller
//!   (the interpreter) stops the thread. The original's behaviour after the
//!   trap (a Windows SEH exception) is not reproduced.
//!
//! Float arithmetic runs through [`slr_x87::X87`] at the configured
//! precision control (PC24 on the device thread, ADR 0007): the exe does
//! `fld dword; fadd|fmul|fdiv dword; fstp dword`, which is exactly
//! `X87::from_f32` / op / `to_f32`. Comparisons and sign flips are exact in
//! plain `f32`; the x87 `fcomp` + `test ah,0x40` equality treats an
//! unordered result (a NaN operand) as *equal* (docs: "NaN compares").

use slr_x87::{Precision, Rounding, X87};

use crate::values::{
    assign_convert, boolean_convert, loader_convert, numeric_promotion, ConvertError,
    LoaderConvert, PromoteError, TypeTag, Value,
};

/// Which message the exe logged (the strings live in the exe; the port
/// names them).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LogKind {
    /// `unsupported type` (arithmetic, unary minus, `++`/`--` on a type
    /// that is neither `I` nor `F`).
    UnsupportedType,
    /// `unsupported class type` (`+` on a reference that is not a
    /// `java.lang.String`).
    UnsupportedClassType,
    /// `non integral operands` (shifts, bitwise, `%`, `||`, `&&`, compound
    /// bitwise assignment).
    NonIntegralOperands,
    /// `non integral operand` (`~`, `!`).
    NonIntegralOperand,
    /// `division by zero` (integer: then `#DE`; float: then `+-inf`/NaN).
    DivisionByZero,
    /// `unsupported type (==)` in the equality family.
    UnsupportedEq,
    /// `unsupported type (>)` in the ordering family.
    UnsupportedGt,
    /// `unsupported type (assign)` in the assignment store.
    UnsupportedAssign,
    /// `unsupported assignmenttype` (assignment dispatcher `default`).
    UnsupportedAssignmentType,
}

/// A "log and continue" path the exe took (the message goes to the diag log
/// in the original). Collected so tests and the interpreter can observe it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Diagnostic {
    /// `JVM::numericPromotion`: `cannot promote` (dst `F`/`I` with a source
    /// that is neither, dst `L` with a source that is not `L`).
    CannotPromote(PromoteError),
    /// `JVM::assignmentConversion` type check failed (the store still
    /// happens with the unconverted payload).
    AssignConversion(ConvertError),
    /// A helper logged one of its own messages.
    Logged {
        /// Exe address of the helper that logged.
        helper: u32,
        /// Which message.
        kind: LogKind,
        /// First byte of the dst (or only) operand's type name, or the
        /// assignment kind byte for `UnsupportedAssignmentType`.
        tag: u8,
    },
}

/// Failure of an operator helper.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OpError {
    /// Integer `idiv` raised `#DE` (zero divisor or `INT_MIN / -1`).
    IntegerTrap {
        /// Exe address of the helper.
        helper: u32,
    },
}

/// Operator state: the x87 precision the float helpers run under, the
/// diagnostics the exe would have logged, and the reference stores the
/// exe's write barrier (`FUN_004198b0`) would have seen.
#[derive(Clone, Debug)]
pub struct Alu {
    /// x87 precision control (PC24 on the device thread, PC53 on workers).
    pub precision: Precision,
    /// "Log and continue" events, oldest first.
    pub diagnostics: Vec<Diagnostic>,
    /// Reference values stored through [`Alu::assign`]. The exe calls the
    /// GC write barrier `FUN_004198b0(src)` for each when the JVM's GC
    /// worklist (`jvm+0x20`) is set up; that needs the heap (ticket #31),
    /// so the port records every store and leaves the decision to the host.
    pub ref_stores: Vec<Value>,
}

impl Default for Alu {
    fn default() -> Self {
        Self::new(Precision::F32)
    }
}

const RC: Rounding = Rounding::NearestEven;

fn is_f_or_i(v: &Value) -> bool {
    matches!(v.tag, TypeTag::Float | TypeTag::Int)
}

impl Alu {
    /// New operator state at the given x87 precision.
    #[must_use]
    pub fn new(precision: Precision) -> Self {
        Self {
            precision,
            diagnostics: Vec::new(),
            ref_stores: Vec::new(),
        }
    }

    fn fop(&self, a: u32, b: u32, op: fn(X87, X87, Precision, Rounding) -> X87) -> u32 {
        let pc = self.precision;
        op(
            X87::from_f32(f32::from_bits(a)),
            X87::from_f32(f32::from_bits(b)),
            pc,
            RC,
        )
        .to_f32(pc, RC)
        .to_bits()
    }

    fn log(&mut self, helper: u32, kind: LogKind, tag: u8) {
        self.diagnostics
            .push(Diagnostic::Logged { helper, kind, tag });
    }

    /// `JVM::numericPromotion` (`FUN_00417570`) as the helpers call it.
    ///
    /// `dst_is_var` is the exe's "slot 0 answers 2" probe on the *first*
    /// parameter: when it is a variable slot the other operand converts
    /// toward the slot's declared type and the slot keeps its type.
    /// Otherwise a failed promotion logs `cannot promote` for an `F`, `I`
    /// or `L` destination (an `[` or unknown destination returns silently).
    fn promote(&mut self, dst: &mut Value, src: &mut Value, dst_is_var: bool) {
        if dst_is_var {
            // Result ignored by the exe.
            let _ = assign_convert(src, dst.tag);
            return;
        }
        let (dt, st) = (dst.tag, src.tag);
        let logs = match dt {
            TypeTag::Float | TypeTag::Int => !is_f_or_i(src),
            // `L` destination: anything but another `L` logs (an `[` source
            // included); `[` and unknown destinations are silent.
            TypeTag::Ref(_) => !st.is_ref() || matches!(st, TypeTag::Array(_)),
            TypeTag::Array(_) | TypeTag::Other(_) => false,
        };
        let _ = numeric_promotion(dst, src);
        if logs {
            self.diagnostics
                .push(Diagnostic::CannotPromote(PromoteError {
                    dst: dt.byte(),
                    src: st.byte(),
                }));
        }
    }

    /// `*` (`FUN_0041b860`, sub-op `0x13`).
    ///
    /// exe: FUN_0041b860
    pub fn mul(&mut self, l: &mut Value, r: &mut Value) -> Result<(), OpError> {
        self.mul_impl(l, r, false)
    }

    fn mul_impl(&mut self, l: &mut Value, r: &mut Value, l_var: bool) -> Result<(), OpError> {
        self.promote(l, r, l_var);
        match l.tag {
            TypeTag::Float => l.payload = self.fop(r.payload, l.payload, X87::mul),
            TypeTag::Int => l.payload = (r.as_i32().wrapping_mul(l.as_i32())) as u32,
            t => self.log(0x41b860, LogKind::UnsupportedType, t.byte()),
        }
        Ok(())
    }

    /// `+` (`FUN_0041b4d0`, sub-op `0x10`). Integers and floats; a
    /// reference operand is logged as `unsupported class type` (the exe's
    /// result for every class except `java.lang.String`, see the module
    /// docs).
    ///
    /// exe: FUN_0041b4d0
    pub fn add(&mut self, l: &mut Value, r: &mut Value) -> Result<(), OpError> {
        self.add_impl(l, r, false)
    }

    fn add_impl(&mut self, l: &mut Value, r: &mut Value, l_var: bool) -> Result<(), OpError> {
        self.promote(l, r, l_var);
        match l.tag {
            TypeTag::Float => l.payload = self.fop(r.payload, l.payload, X87::add),
            TypeTag::Int => l.payload = l.as_i32().wrapping_add(r.as_i32()) as u32,
            TypeTag::Ref(_) => self.log(0x41b4d0, LogKind::UnsupportedClassType, b'L'),
            t => self.log(0x41b4d0, LogKind::UnsupportedType, t.byte()),
        }
        Ok(())
    }

    /// The negation `-` applies to its right operand before adding
    /// (`FUN_0041b760`'s first half): `F`/`I` negate, anything else logs
    /// `unsupported type` and is left alone. Exposed so the interpreter can
    /// offer a reference `-` to its host after the same preparation.
    pub fn negate_for_sub(&mut self, r: &mut Value) {
        match r.tag {
            TypeTag::Float | TypeTag::Int => self.negate(r),
            t => self.log(0x41b760, LogKind::UnsupportedType, t.byte()),
        }
    }

    /// `-` (`FUN_0041b760`, sub-op `0x0f`): negate the right cell in place,
    /// then `+`.
    ///
    /// exe: FUN_0041b760
    pub fn sub(&mut self, l: &mut Value, r: &mut Value) -> Result<(), OpError> {
        self.sub_impl(l, r, false)
    }

    fn sub_impl(&mut self, l: &mut Value, r: &mut Value, l_var: bool) -> Result<(), OpError> {
        self.negate_for_sub(r);
        self.add_impl(l, r, l_var)
    }

    /// `/` (`FUN_0041b970`, sub-op `0x12`).
    ///
    /// exe: FUN_0041b970
    pub fn div(&mut self, l: &mut Value, r: &mut Value) -> Result<(), OpError> {
        self.div_impl(l, r, false)
    }

    fn div_impl(&mut self, l: &mut Value, r: &mut Value, l_var: bool) -> Result<(), OpError> {
        self.promote(l, r, l_var);
        match l.tag {
            TypeTag::Float => {
                // `== 0.0` through fcomp + C3: a NaN divisor counts as zero.
                if r.as_f32() == 0.0 || r.as_f32().is_nan() {
                    self.log(0x41b970, LogKind::DivisionByZero, b'F');
                }
                l.payload = self.fop(l.payload, r.payload, X87::div);
                Ok(())
            }
            TypeTag::Int => {
                if r.as_i32() == 0 {
                    self.log(0x41b970, LogKind::DivisionByZero, b'I');
                    return Err(OpError::IntegerTrap { helper: 0x41b970 });
                }
                match l.as_i32().checked_div(r.as_i32()) {
                    Some(q) => {
                        l.payload = q as u32;
                        Ok(())
                    }
                    None => Err(OpError::IntegerTrap { helper: 0x41b970 }),
                }
            }
            t => {
                self.log(0x41b970, LogKind::UnsupportedType, t.byte());
                Ok(())
            }
        }
    }

    /// `%` (`FUN_0041bbf0`, sub-op `0x11`): integers only.
    ///
    /// exe: FUN_0041bbf0
    pub fn rem(&mut self, l: &mut Value, r: &mut Value) -> Result<(), OpError> {
        self.rem_impl(l, r, false)
    }

    fn rem_impl(&mut self, l: &mut Value, r: &mut Value, l_var: bool) -> Result<(), OpError> {
        self.promote(l, r, l_var);
        if l.tag != TypeTag::Int {
            self.log(0x41bbf0, LogKind::NonIntegralOperands, l.tag.byte());
            return Ok(());
        }
        if r.as_i32() == 0 {
            self.log(0x41bbf0, LogKind::DivisionByZero, b'I');
            return Err(OpError::IntegerTrap { helper: 0x41bbf0 });
        }
        match l.as_i32().checked_rem(r.as_i32()) {
            Some(m) => {
                l.payload = m as u32;
                Ok(())
            }
            None => Err(OpError::IntegerTrap { helper: 0x41bbf0 }),
        }
    }

    /// `||` (`FUN_0041bfb0`, sub-op `1`): both cells are boolean-converted
    /// in place, the result lands in LEFT.
    ///
    /// exe: FUN_0041bfb0
    pub fn logical_or(&mut self, l: &mut Value, r: &mut Value) {
        boolean_convert(l);
        boolean_convert(r);
        if l.tag != TypeTag::Int {
            self.log(0x41bfb0, LogKind::NonIntegralOperands, l.tag.byte());
            return;
        }
        l.payload = u32::from(!(l.payload == 0 && r.payload == 0));
    }

    /// `&&` (`FUN_0041bea0`, sub-op `2`).
    ///
    /// exe: FUN_0041bea0
    pub fn logical_and(&mut self, l: &mut Value, r: &mut Value) {
        boolean_convert(l);
        boolean_convert(r);
        if l.tag != TypeTag::Int {
            self.log(0x41bea0, LogKind::NonIntegralOperands, l.tag.byte());
            return;
        }
        l.payload = u32::from(l.payload != 0 && r.payload != 0);
    }

    /// `|` (`FUN_0041c290`, sub-op `3`).
    ///
    /// exe: FUN_0041c290
    pub fn bit_or(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        if l.tag == TypeTag::Int {
            l.payload |= r.payload;
        } else {
            self.log(0x41c290, LogKind::NonIntegralOperands, l.tag.byte());
        }
    }

    /// `^` (`FUN_0041c380`, sub-op `4`).
    ///
    /// exe: FUN_0041c380
    pub fn bit_xor(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        if l.tag == TypeTag::Int {
            l.payload ^= r.payload;
        } else {
            self.log(0x41c380, LogKind::NonIntegralOperands, l.tag.byte());
        }
    }

    /// `&` (`FUN_0041c1a0`, sub-op `5`).
    ///
    /// exe: FUN_0041c1a0
    pub fn bit_and(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        if l.tag == TypeTag::Int {
            l.payload &= r.payload;
        } else {
            self.log(0x41c1a0, LogKind::NonIntegralOperands, l.tag.byte());
        }
    }

    /// `>>` (`FUN_0041aff0`, sub-op `0xd`). `count` is the stack top, the
    /// result (arithmetic shift by `count & 31`) lands in `value`.
    ///
    /// exe: FUN_0041aff0
    pub fn shr(&mut self, count: &mut Value, value: &mut Value) {
        self.promote(count, value, false);
        if count.tag != TypeTag::Int {
            self.log(0x41aff0, LogKind::NonIntegralOperands, count.tag.byte());
            return;
        }
        value.payload = (value.as_i32() >> (count.payload & 0x1f)) as u32;
    }

    /// `<<` (`FUN_0041b0e0`, sub-op `0xe`). Same shape as [`Alu::shr`].
    ///
    /// exe: FUN_0041b0e0
    pub fn shl(&mut self, count: &mut Value, value: &mut Value) {
        self.promote(count, value, false);
        if count.tag != TypeTag::Int {
            self.log(0x41b0e0, LogKind::NonIntegralOperands, count.tag.byte());
            return;
        }
        value.payload = value.as_i32().wrapping_shl(count.payload & 0x1f) as u32;
    }

    /// Equality as the exe's compare helpers see it: `F` through
    /// `fcomp` + C3 (unordered counts as equal), `I`/`L`/`[` by payload
    /// word, anything else never equal.
    fn is_equal(l: &Value, r: &Value) -> bool {
        match l.tag {
            TypeTag::Float => {
                l.as_f32() == r.as_f32() || l.as_f32().is_nan() || r.as_f32().is_nan()
            }
            TypeTag::Int | TypeTag::Ref(_) | TypeTag::Array(_) => l.payload == r.payload,
            TypeTag::Other(_) => false,
        }
    }

    /// `R < L` for the types that have an ordering (`F`, `I`), else false.
    fn is_greater(l: &Value, r: &Value) -> bool {
        match l.tag {
            TypeTag::Float => r.as_f32() < l.as_f32(),
            TypeTag::Int => r.as_i32() < l.as_i32(),
            _ => false,
        }
    }

    /// The equality switch's `default` log (`unsupported type (==)`).
    fn eq_log(&mut self, helper: u32, l: &Value) {
        if matches!(l.tag, TypeTag::Other(_)) {
            self.log(helper, LogKind::UnsupportedEq, l.tag.byte());
        }
    }

    /// The ordering tail's log (`unsupported type (>)`): neither `F` nor `I`.
    fn gt_log(&mut self, helper: u32, l: &Value) {
        if !is_f_or_i(l) {
            self.log(helper, LogKind::UnsupportedGt, l.tag.byte());
        }
    }

    fn set_bool(l: &mut Value, b: bool) {
        // The exe stores the payload and points the type at `*DAT_0062e00c`
        // (the `I` descriptor).
        l.tag = TypeTag::Int;
        l.payload = u32::from(b);
    }

    /// `==` (`FUN_0041c5c0`, sub-op `7`).
    ///
    /// exe: FUN_0041c5c0
    pub fn eq(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.eq_log(0x41c5c0, l);
        let b = Self::is_equal(l, r);
        Self::set_bool(l, b);
    }

    /// `!=` (`FUN_0041c6c0`, sub-op `6`): the negation of `==` lives in
    /// the helper.
    ///
    /// exe: FUN_0041c6c0
    pub fn ne(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.eq_log(0x41c6c0, l);
        let b = !Self::is_equal(l, r);
        Self::set_bool(l, b);
    }

    /// `>=` (`FUN_0041c860`, sub-op `8`): equal or greater.
    ///
    /// exe: FUN_0041c860
    pub fn ge(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.eq_log(0x41c860, l);
        let eq = Self::is_equal(l, r);
        if !eq {
            self.gt_log(0x41c860, l);
        }
        let b = eq || Self::is_greater(l, r);
        Self::set_bool(l, b);
    }

    /// `<=` (`FUN_0041cb60`, sub-op `9`): not greater.
    ///
    /// exe: FUN_0041cb60
    pub fn le(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.gt_log(0x41cb60, l);
        let b = !Self::is_greater(l, r);
        Self::set_bool(l, b);
    }

    /// `>` (`FUN_0041c790`, sub-op `0xa`).
    ///
    /// exe: FUN_0041c790
    pub fn gt(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.gt_log(0x41c790, l);
        let b = Self::is_greater(l, r);
        Self::set_bool(l, b);
    }

    /// `<` (`FUN_0041c9e0`, sub-op `0xb`): not (equal or greater).
    ///
    /// exe: FUN_0041c9e0
    pub fn lt(&mut self, l: &mut Value, r: &mut Value) {
        self.promote(l, r, false);
        self.eq_log(0x41c9e0, l);
        let eq = Self::is_equal(l, r);
        if !eq {
            self.gt_log(0x41c9e0, l);
        }
        let b = !(eq || Self::is_greater(l, r));
        Self::set_bool(l, b);
    }

    /// `~` (`FUN_0041c0c0`, sub-op `0x14`): integers only.
    ///
    /// exe: FUN_0041c0c0
    pub fn bit_not(&mut self, v: &mut Value) {
        if v.tag == TypeTag::Int {
            v.payload = !v.payload;
        } else {
            self.log(0x41c0c0, LogKind::NonIntegralOperand, v.tag.byte());
        }
    }

    /// `!` (`FUN_0041bdb0`, sub-op `0x15`).
    ///
    /// exe: FUN_0041bdb0
    pub fn logical_not(&mut self, v: &mut Value) {
        boolean_convert(v);
        if v.tag == TypeTag::Int {
            v.payload = u32::from(v.payload == 0);
        } else {
            self.log(0x41bdb0, LogKind::NonIntegralOperand, v.tag.byte());
        }
    }

    /// Unary `-` (`FUN_0041b1d0`, sub-op `0x16`): `fchs` / `neg`.
    ///
    /// exe: FUN_0041b1d0
    pub fn negate(&mut self, v: &mut Value) {
        match v.tag {
            TypeTag::Float => v.payload ^= 0x8000_0000,
            TypeTag::Int => v.payload = v.as_i32().wrapping_neg() as u32,
            t => self.log(0x41b1d0, LogKind::UnsupportedType, t.byte()),
        }
    }

    /// In-place `+ 1` (`FUN_0041b2d0`): the increment half of `++`. The
    /// caller (the run loop) owns the lvalue check and the copy it pushes.
    ///
    /// exe: FUN_0041b2d0
    pub fn increment(&mut self, v: &mut Value) {
        match v.tag {
            TypeTag::Float => v.payload = self.fop(v.payload, 1.0f32.to_bits(), X87::add),
            TypeTag::Int => v.payload = v.as_i32().wrapping_add(1) as u32,
            t => self.log(0x41b2d0, LogKind::UnsupportedType, t.byte()),
        }
    }

    /// In-place `- 1` (`FUN_0041b3d0`): the decrement half of `--`.
    ///
    /// exe: FUN_0041b3d0
    pub fn decrement(&mut self, v: &mut Value) {
        match v.tag {
            TypeTag::Float => v.payload = self.fop(v.payload, 1.0f32.to_bits(), X87::sub),
            TypeTag::Int => v.payload = v.as_i32().wrapping_sub(1) as u32,
            t => self.log(0x41b3d0, LogKind::UnsupportedType, t.byte()),
        }
    }

    /// Assignment store (`FUN_0041ab60`): convert `src` toward the
    /// destination's type (`JVM::assignmentConversion`, `FUN_004178f0`,
    /// which logs and carries on when the conversion fails), then copy the
    /// payload word. A reference store also reaches the GC write barrier;
    /// an unknown destination type logs `unsupported type (assign)`.
    ///
    /// The `L`-target class-loader paths (`I`/`F` sources, non-null
    /// cross-class refs) convert through `loader` when given
    /// ([`loader_convert`], the `FUN_00417da0` static port); with `None`
    /// the port keeps the historical behavior (log + raw-word copy).
    ///
    /// exe: FUN_0041ab60
    pub fn store(&mut self, dst: &mut Value, src: &mut Value) {
        self.store_with(dst, src, None);
    }

    /// [`Alu::store`] with an optional class loader for the `L`-target
    /// conversion paths.
    pub fn store_with(
        &mut self,
        dst: &mut Value,
        src: &mut Value,
        loader: Option<&mut dyn LoaderConvert>,
    ) {
        if dst.tag != src.tag {
            let target_name = dst.tag.name();
            let is_l_target = dst.tag.is_ref();
            let converted = match (loader, is_l_target) {
                (Some(l), true) => Some(loader_convert(l, src, target_name)),
                _ => None,
            };
            match converted {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    self.diagnostics.push(Diagnostic::AssignConversion(e));
                }
                None => {
                    if let Err(e) = assign_convert(src, dst.tag) {
                        self.diagnostics.push(Diagnostic::AssignConversion(e));
                    }
                }
            }
        }
        match dst.tag {
            TypeTag::Float | TypeTag::Int => dst.payload = src.payload,
            TypeTag::Ref(_) | TypeTag::Array(_) => {
                dst.payload = src.payload;
                self.ref_stores.push(*src);
            }
            TypeTag::Other(t) => self.log(0x41ab60, LogKind::UnsupportedAssign, t),
        }
    }

    /// The assignment dispatcher `FUN_0041acb0(dst, src, kind)`: `0x23`
    /// plain, `0x24` `*=`, `0x25` `/=`, `0x26` `%=`, `0x27` `+=`, `0x28`
    /// `-=`, `0x2c` `&=`, `0x2d` `^=`, `0x2e` `|=`. Any other kind logs
    /// `unsupported assignmenttype` and does nothing. `dst_is_var` says
    /// whether `dst` is a variable slot (the lvalue the run loop passes) or
    /// a plain cell (the oracle vectors): it only changes how mixed `I`/`F`
    /// operands promote.
    ///
    /// exe: FUN_0041acb0
    pub fn assign(
        &mut self,
        dst: &mut Value,
        dst_is_var: bool,
        src: &mut Value,
        kind: u32,
    ) -> Result<(), OpError> {
        match kind {
            0x23 => self.store(dst, src),
            0x24 => {
                let mut tmp = *dst;
                self.mul_impl(&mut tmp, src, false)?;
                self.store(dst, &mut tmp);
            }
            0x25 => {
                let mut tmp = *dst;
                self.div_impl(&mut tmp, src, false)?;
                self.store(dst, &mut tmp);
            }
            0x26 => self.rem_impl(dst, src, dst_is_var)?,
            0x27 => self.add_impl(dst, src, dst_is_var)?,
            0x28 => {
                self.negate(src);
                self.add_impl(dst, src, dst_is_var)?;
            }
            0x2c..=0x2e => {
                self.promote(dst, src, dst_is_var);
                if dst.tag == TypeTag::Int {
                    match kind {
                        0x2c => dst.payload &= src.payload,
                        0x2d => dst.payload ^= src.payload,
                        _ => dst.payload |= src.payload,
                    }
                } else {
                    self.log(0x41acb0, LogKind::NonIntegralOperands, dst.tag.byte());
                }
            }
            _ => self.log(0x41acb0, LogKind::UnsupportedAssignmentType, dst.tag.byte()),
        }
        Ok(())
    }
}

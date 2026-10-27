//! Retained x87 state for source VM continuations. Ordinary finite arithmetic
//! is admitted; unsupported stack/exception paths fail instead of guessing SW.
//! Empty physical registers retain their bits after a pop, just as x87 does.
use slr_x87::{Arithmetic, Precision, Rounding, X87};

#[derive(Clone)]
pub struct State {
    control: u16,
    status: u16,
    tags: u16,
    registers: [X87; 8],
}
impl State {
    /// FINIT state. Physical register contents are retained from the actual
    /// caller; FINIT does not overwrite them.
    pub fn finit(registers: [X87; 8]) -> Self {
        Self {
            control: 0x37f,
            status: 0,
            tags: 0xffff,
            registers,
        }
    }
    /// Restore an actual saved environment, used by original input comparisons.
    /// Production never loads this state from an oracle capture.
    pub fn from_saved(control: u16, status: u16, tags: u16, registers: [X87; 8]) -> Self {
        Self {
            control,
            status,
            tags,
            registers,
        }
    }
    pub fn control(&self) -> u16 {
        self.control
    }
    pub fn status(&self) -> u16 {
        self.status
    }
    pub fn tags(&self) -> u16 {
        self.tags
    }
    pub fn registers(&self) -> [X87; 8] {
        self.registers
    }
    pub fn load_control(&mut self, control: u16) {
        self.control = control;
    }
    fn controls(&self) -> Result<(Precision, Rounding), String> {
        if self.control & 0x3f != 0x3f || self.status & 0x80 != 0 {
            return Err(
                "NativeVmFpu: unmasked/pending exception requires its source handler".into(),
            );
        }
        let pc = match (self.control >> 8) & 3 {
            0 => Precision::F32,
            2 => Precision::F64,
            3 => Precision::Ext,
            _ => return Err("NativeVmFpu: reserved precision".into()),
        };
        let rc = match (self.control >> 10) & 3 {
            0 => Rounding::NearestEven,
            1 => Rounding::Down,
            2 => Rounding::Up,
            _ => Rounding::Zero,
        };
        Ok((pc, rc))
    }
    fn top(&self) -> usize {
        usize::from((self.status >> 11) & 7)
    }
    fn set_top(&mut self, top: usize) {
        self.status = (self.status & !0x3800) | ((top as u16 & 7) << 11);
    }
    fn tag(&self, slot: usize) -> u16 {
        (self.tags >> (slot * 2)) & 3
    }
    fn set(&mut self, slot: usize, value: X87) {
        let tag = if value.is_zero() {
            1
        } else if value.is_inf() || value.is_nan() {
            2
        } else {
            0
        };
        self.tags = (self.tags & !(3 << (slot * 2))) | (tag << (slot * 2));
        self.registers[slot] = value;
    }
    fn finite(value: X87) -> Result<(), String> {
        if value.is_inf() || value.is_nan() {
            return Err(
                "NativeVmFpu: non-finite operand requires its source exception path".into(),
            );
        }
        Ok(())
    }
    pub fn st(&self, index: usize) -> Result<X87, String> {
        let slot = (self.top() + index) & 7;
        if index > 7 || self.tag(slot) == 3 {
            return Err("NativeVmFpu: empty stack operand".into());
        }
        Ok(self.registers[slot])
    }
    pub fn push(&mut self, value: X87) -> Result<(), String> {
        self.controls()?;
        Self::finite(value)?;
        let top = (self.top() + 7) & 7;
        if self.tag(top) != 3 {
            return Err("NativeVmFpu: stack overflow".into());
        }
        self.status &= !0x200;
        self.set_top(top);
        self.set(top, value);
        Ok(())
    }
    pub fn pop(&mut self) -> Result<X87, String> {
        let value = self.st(0)?;
        let top = self.top();
        self.tags |= 3 << (top * 2);
        self.set_top((top + 1) & 7);
        Ok(value)
    }
    fn operand_f32(&mut self, bits: u32) -> Result<X87, String> {
        let value = X87::from_f32(f32::from_bits(bits));
        Self::finite(value)?;
        if bits & 0x7f80_0000 == 0 && bits & 0x7f_ffff != 0 {
            self.status |= 2;
        }
        Ok(value)
    }
    pub fn fld_f32(&mut self, bits: u32) -> Result<(), String> {
        let value = self.operand_f32(bits)?;
        self.push(value)
    }
    pub fn fild_i64(&mut self, value: i64) -> Result<(), String> {
        self.push(X87::from_i64(value))
    }
    pub fn fcomp_f32(&mut self, bits: u32) -> Result<(), String> {
        self.controls()?;
        let left = self.st(0)?;
        Self::finite(left)?;
        let right = self.operand_f32(bits)?;
        let (codes, invalid) = left.fcom(right);
        if invalid {
            return Err("NativeVmFpu: unsupported invalid comparison".into());
        }
        self.status = (self.status & !0x4700)
            | (u16::from(codes.c0) << 8)
            | (u16::from(codes.c2) << 10)
            | (u16::from(codes.c3) << 14);
        self.pop()?;
        Ok(())
    }
    fn exceptions(&mut self, flags: u8, rounded_up: bool) {
        // SoftFloat: inexact1,underflow2,overflow4,infinite8,invalid16.
        let sw = ((flags & 1) << 5)
            | ((flags & 2) << 3)
            | ((flags & 4) << 1)
            | ((flags & 8) >> 1)
            | ((flags & 16) >> 4);
        self.status = (self.status & !0x200) | (u16::from(rounded_up) << 9) | u16::from(sw);
    }
    fn result(&mut self, left: X87, right: X87, op: Arithmetic) -> Result<X87, String> {
        let (pc, rc) = self.controls()?;
        Self::finite(left)?;
        Self::finite(right)?;
        let (result, flags) = left.arithmetic_with_exceptions(right, op, pc, rc);
        Self::finite(result)?;
        let truncated = left
            .arithmetic_with_exceptions(right, op, pc, Rounding::Zero)
            .0;
        self.exceptions(flags, result != truncated);
        Ok(result)
    }
    pub fn arithmetic_pop(&mut self, op: Arithmetic) -> Result<(), String> {
        let value = self.result(self.st(1)?, self.st(0)?, op)?;
        self.set((self.top() + 1) & 7, value);
        self.pop()?;
        Ok(())
    }
    pub fn arithmetic_f32(&mut self, op: Arithmetic, bits: u32) -> Result<(), String> {
        let right = self.operand_f32(bits)?;
        let value = self.result(self.st(0)?, right, op)?;
        self.set(self.top(), value);
        Ok(())
    }
    pub fn reverse_subtract_f32(&mut self, bits: u32) -> Result<(), String> {
        let left = self.operand_f32(bits)?;
        let value = self.result(left, self.st(0)?, Arithmetic::Subtract)?;
        self.set(self.top(), value);
        Ok(())
    }
    pub fn fstp_f32(&mut self) -> Result<u32, String> {
        let value = self.st(0)?;
        let (pc, rc) = self.controls()?;
        let (result, flags) = value.to_f32_with_exceptions(pc, rc);
        if !result.is_finite() {
            return Err("NativeVmFpu: non-finite store".into());
        }
        let truncated = value.to_f32_with_exceptions(pc, Rounding::Zero).0;
        self.exceptions(flags, result.to_bits() != truncated.to_bits());
        self.pop()?;
        Ok(result.to_bits())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn finit_and_control_load_preserve_physical_registers_without_live_stack_values() {
        let physical = std::array::from_fn(|i| X87::from_f64(i as f64 + 0.25));
        let mut state = State::finit(physical);
        assert_eq!(
            (state.control(), state.status(), state.tags()),
            (0x37f, 0, 0xffff)
        );
        assert_eq!(
            state.registers().map(X87::to_bytes),
            physical.map(X87::to_bytes)
        );
        state.load_control(0x7f);
        assert_eq!(
            (state.control(), state.status(), state.tags()),
            (0x7f, 0, 0xffff)
        );
        assert_eq!(
            state.registers().map(X87::to_bytes),
            physical.map(X87::to_bytes)
        );
    }
}

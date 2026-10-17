//! TUFA value cells and the small helpers that move them around.
//!
//! Machine model (see `docs/tufa-executor.md`; all claims there are marked
//! `[oracle]` or `[static]`):
//!
//! - A value is a 12-byte ref-counted cell: `+0` vtable pointer (value cells
//!   `0x5e734c`), `+4` type-name pointer, `+8` payload word. Type identity is
//!   the first byte of the type name: `I` int, `F` float, `L`/`[` reference,
//!   `V` void. Float payloads are IEEE-754 `f32` bits.
//! - A variable slot (lvalue) is a 16-byte cell with the `0x5e7344` vtable:
//!   the value shape above at `+0`, real payload at `+8`, plus the interned
//!   variable name at `+0xc`. Slot 0 of the vtable answers the probe
//!   "is this a variable slot" (value cells return 1, varslots return 2);
//!   slot 1 releases the cell (value cells decrement the live-value counter
//!   and return to the value pool; varslots return to the variable pool).
//! - The value pool behind [`ValuePool`] is `DAT_0062f130` (see
//!   [`ValuePool::alloc`] for the exact freelist/growth rule); the variable
//!   pool `DAT_0062f134` has the same shape with 84,000-cell blocks and
//!   16-byte cells but is not ported here (open: no oracle vectors yet).
//!
//! The ported helpers each carry a strict `/// exe:` line so `slr-ledger`
//! can attribute them. Anything needing a booted JVM (string interning,
//! class loading, GC write barriers) is deliberately absent.

use core::fmt;

/// First byte of the type name: the exe's whole type identity for values.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeTag {
    /// `I`: 32-bit integer (payload is the word itself).
    Int,
    /// `F`: 32-bit float (payload is `f32` bits).
    Float,
    /// `L...;` / `V`: reference or void marker carried as a word.
    /// The exe treats `L` and `[` identically in promotion/conversion reads
    /// (`cVar1 == 'L' || cVar1 == '['`); void `V` never reaches a helper on a
    /// live path, so it shares the word representation here.
    Ref,
    /// `[`: array reference. Same word representation as [`TypeTag::Ref`];
    /// kept distinct so callers can preserve the spelling they observed.
    Array,
    /// Any other leading byte. Helpers only ever read the first byte, so an
    /// unexpected tag fails closed at the same points the exe logs.
    Other(u8),
}

impl TypeTag {
    /// Read the tag the way the exe does: first byte of the type name.
    #[must_use]
    pub fn of(first_byte: u8) -> Self {
        match first_byte {
            b'I' => Self::Int,
            b'F' => Self::Float,
            b'L' | b'V' => Self::Ref,
            b'[' => Self::Array,
            b => Self::Other(b),
        }
    }

    /// The first byte, for writes back into a cell.
    #[must_use]
    pub fn byte(self) -> u8 {
        match self {
            Self::Int => b'I',
            Self::Float => b'F',
            Self::Ref => b'L',
            Self::Array => b'[',
            Self::Other(b) => b,
        }
    }

    /// `true` for the `L`/`[` reference family (exe: both arms together).
    #[must_use]
    pub fn is_ref(self) -> bool {
        matches!(self, Self::Ref | Self::Array)
    }
}

/// A decoded value cell: `(type tag, payload word)`.
///
/// This is the `(type, payload)` pair the oracle vectors record per cell;
/// addresses are arena-dependent and never compared.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Value {
    /// Type tag (first byte of the type name).
    pub tag: TypeTag,
    /// Payload word: int bits, `f32` bits, or reference word.
    pub payload: u32,
}

impl Value {
    /// Integer cell.
    #[must_use]
    pub fn int(v: i32) -> Self {
        Self {
            tag: TypeTag::Int,
            payload: v as u32,
        }
    }

    /// Float cell (payload stored as `f32` bits, exactly like the exe).
    #[must_use]
    pub fn float(v: f32) -> Self {
        Self {
            tag: TypeTag::Float,
            payload: v.to_bits(),
        }
    }

    /// Reference cell (object/array word, 0 = null).
    #[must_use]
    pub fn reference(word: u32) -> Self {
        Self {
            tag: TypeTag::Ref,
            payload: word,
        }
    }

    /// Payload as signed int (meaningful for [`TypeTag::Int`]).
    #[must_use]
    pub fn as_i32(self) -> i32 {
        self.payload as i32
    }

    /// Payload as float (meaningful for [`TypeTag::Float`]).
    #[must_use]
    pub fn as_f32(self) -> f32 {
        f32::from_bits(self.payload)
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.tag {
            TypeTag::Int => write!(f, "I({})", self.as_i32()),
            TypeTag::Float => write!(f, "F({})", self.as_f32()),
            TypeTag::Ref | TypeTag::Array => write!(f, "R({:#x})", self.payload),
            TypeTag::Other(b) => write!(f, "?{b:#x}({:#x})", self.payload),
        }
    }
}

/// What [`ValuePool::alloc`] did: fresh pool slot vs. freelist reuse.
///
/// The exe returns only the pointer either way; the test oracle records
/// this discriminant through memory-write capture (pooled growth writes
/// the pool headers; freelist reuse does not).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AllocOrigin {
    /// Reused head of the freelist (`param_1[4]` chain).
    Freelist,
    /// Next slot of the current 512-cell block, possibly after growing the
    /// block-pointer table and `malloc`ing a fresh `0x1800`-byte block.
    Pooled,
}

/// Value-cell pool: the `DAT_0062f130` allocator state.
///
/// Layout (five words, observed at `FUN_00423c00` and its siblings):
/// `[0]` block-pointer table, `[1]` block count, `[2]` cells handed out,
/// `[3]` cells allocated (capacity), `[4]` freelist head (each free cell's
/// first word chains to the next). Cells are 12 bytes; blocks hold 512.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ValuePool {
    blocks: Vec<[Value; 512]>,
    next: u32,
    free: Vec<Value>,
    /// Live-value counter: `DAT_0062f138` (`copy` +1, `release` -1).
    pub live: i32,
}

impl ValuePool {
    /// Empty pool (matches a zeroed `DAT_0062f130` + zeroed counter).
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Number of block-table entries (backs `[1]`, the exe's growth input).
    #[must_use]
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Cells handed out so far (backs `[2]`, the pooled index).
    #[must_use]
    pub fn handed_out(&self) -> u32 {
        self.next
    }

    /// Cells allocated = `blocks * 512` (backs `[3]`, the capacity word).
    #[must_use]
    pub fn capacity(&self) -> u32 {
        self.blocks.len() as u32 * 512
    }

    /// Freelist depth (backs `[4] == null` vs non-null).
    #[must_use]
    pub fn free_len(&self) -> usize {
        self.free.len()
    }

    /// Allocate one value cell.
    ///
    /// exe: FUN_00423c00
    pub fn alloc(&mut self) -> (Value, AllocOrigin) {
        if let Some(v) = self.free.pop() {
            return (v, AllocOrigin::Freelist);
        }
        if self.next == self.capacity() {
            self.blocks.push([Value::int(0); 512]);
        }
        let idx = self.next;
        self.next += 1;
        let cell = self.blocks[(idx / 512) as usize][(idx % 512) as usize];
        (cell, AllocOrigin::Pooled)
    }

    /// Release one value cell: slot-1 release with ownership transfer.
    ///
    /// The exe splits this in two: `FUN_00423e90` (thiscall, stamps the
    /// value vtable and decrements `DAT_0062f138`) and the slot-1 wrapper
    /// (`0x405000`: on flag `& 1`, chains the cell onto the freelist
    /// `DAT_0062f130[4]`). Every `run` call site passes flag 1, so the two
    /// always run together on live paths; this models their composition.
    ///
    /// exe: FUN_00423e90
    pub fn release(&mut self, v: Value) {
        self.live -= 1;
        self.free.push(v);
    }

    /// Test hook: append one zeroed 512-cell block (models a prebuilt pool
    /// block the oracle scenario constructed in emulator memory). Public so
    /// the oracle replay test can stage pool states.
    pub fn blocks_push_zeroed(&mut self) {
        self.blocks.push([Value::int(0); 512]);
    }

    /// Test hook: set the handed-out index (models pool `[2]`).
    pub fn set_handed(&mut self, handed: u32) {
        self.next = handed;
    }

    /// Test hook: reverse the freelist (models head-first chaining order).
    pub fn reverse_free(&mut self) {
        self.free.reverse();
    }
}

/// Copy one value cell's `(type, payload)` words into `dst`.
///
/// The exe writes the value vtable, both words, and bumps `DAT_0062f138`;
/// the caller supplies `dst` (usually fresh from [`ValuePool::alloc`]).
/// Returns the live-counter delta (+1) so the caller can apply it to the
/// pool owning the counter.
///
/// exe: FUN_00423e30
#[must_use]
pub fn copy_into(dst: &mut Value, src: Value) -> i32 {
    *dst = src;
    1
}

/// Push a value word onto the operand-stack container.
///
/// The exe container is `[+0] capacity, [+4] data, [+8] count`; growth is
/// `new = cap + count + 1` (`realloc`, or `free` + null when that is 0) and
/// then the count slot is re-clamped (`if (new < count) count = new`) before
/// the store. Returns the new `(capacity, count)`.
///
/// exe: FUN_00423ba0
pub fn push_slot(cap: u32, count: u32) -> (u32, u32) {
    if count + 1 > cap {
        let new = cap + count + 1;
        let count = if new < count { new } else { count };
        return (new, count + 1);
    }
    (cap, count + 1)
}

/// Pop a value word off the operand-stack container.
///
/// Returns `None` on underflow (the exe logs `T_Container__underflow` and
/// returns 0); the popped slot is zeroed.
///
/// exe: FUN_00423d40
pub fn pop_slot(stack: &mut Vec<u32>) -> Option<u32> {
    stack.pop()
}

/// Operand-stack container with the exe's exact growth rule, for tests.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OperandStack {
    /// Backs `data`: the stored value words.
    pub words: Vec<u32>,
    /// Backs `[+0] capacity` (may exceed `words.len()` after growth).
    pub cap: u32,
}

impl OperandStack {
    /// Empty container.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Push one word; returns the new `(capacity, count)`.
    pub fn push(&mut self, word: u32) -> (u32, u32) {
        let (cap, _count) = push_slot(self.cap, self.words.len() as u32);
        self.cap = cap;
        self.words.push(word);
        (self.cap, self.words.len() as u32)
    }

    /// Pop one word; `None` on underflow (logs in the exe).
    pub fn pop(&mut self) -> Option<u32> {
        pop_slot(&mut self.words)
    }
}

/// A variable slot (lvalue): value words plus the interned name word.
///
/// `+0..+8` are the value shape (`0x5e7344` vtable, type, payload); `+0xc`
/// is the variable-name pointer. Only the payload/type words participate in
/// the helpers below; the name word is carried untouched.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct VarSlot {
    /// Current value words.
    pub value: Value,
    /// Interned variable-name pointer (`+0xc`).
    pub name: u32,
}

/// Pop with variable-slot dereferencing.
///
/// A plain value pops as-is; a variable slot pops a *copy* of its current
/// value words into a fresh cell (the exe `alloc`s the copy from the value
/// pool and `copy`s into it). Underflow (empty stack) logs and yields
/// `None`; a null pool (allocation failure) also yields `None`.
///
/// exe: FUN_0041f7d0
pub fn deref_pop(stack: &mut Vec<StackCell>, pool: &mut ValuePool) -> Option<Value> {
    let cell = stack.pop()?;
    match cell {
        StackCell::Value(v) => Some(v),
        StackCell::Var(slot) => {
            let (mut fresh, _) = pool.alloc();
            pool.live += copy_into(&mut fresh, slot.value);
            Some(fresh)
        }
    }
}

/// A stack word: either a plain value or a variable slot (lvalue).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum StackCell {
    /// Plain value (vtable `0x5e734c`, slot 0 answers 1).
    Value(Value),
    /// Variable slot (vtable `0x5e7344`, slot 0 answers 2).
    Var(VarSlot),
}

/// Vtable slot-0 probe: 2 means "variable slot" (lvalue), else plain value.
/// Value cells answer 1, varslots answer 2 (machine-code verified).
///
/// exe: 0x515610
/// exe: 0x405030
#[must_use]
pub fn slot_kind(cell: &StackCell) -> u32 {
    match cell {
        StackCell::Value(_) => 1,
        StackCell::Var(_) => 2,
    }
}

/// Numeric promotion between two operand cells.
///
/// `F`+`F` and `I`+`I` stay; mixed `I`/`F` converts the `I` side to `F`
/// (`I`→`F` is `fild`, exact for small ints; `F`→`I` truncates toward zero,
/// matching the exe's `__ftol` truncating `fistp` sequence). `L`+`L` (any
/// `L`/`[` pair) passes; a variable slot on either side converts the *other*
/// side toward the slot's declared type instead. Anything else is a
/// `cannot promote` log in the exe; here it is [`PromoteError`].
///
/// exe: FUN_00417570
pub fn numeric_promotion(dst: &mut Value, src: &mut Value) -> Result<(), PromoteError> {
    promote_pair(
        &mut dst.tag,
        &mut dst.payload,
        &mut src.tag,
        &mut src.payload,
    )
}

/// Promotion failure: the exe logs `cannot promote` and continues with the
/// operands unconverted (the operator then takes its fatal path).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PromoteError {
    /// First byte of the destination type name.
    pub dst: u8,
    /// First byte of the source type name.
    pub src: u8,
}

fn promote_pair(
    dst_tag: &mut TypeTag,
    dst_pay: &mut u32,
    src_tag: &mut TypeTag,
    src_pay: &mut u32,
) -> Result<(), PromoteError> {
    let err = PromoteError {
        dst: dst_tag.byte(),
        src: src_tag.byte(),
    };
    match (*dst_tag, *src_tag) {
        (TypeTag::Float, TypeTag::Float)
        | (TypeTag::Int, TypeTag::Int)
        | (TypeTag::Ref, TypeTag::Ref)
        | (TypeTag::Ref, TypeTag::Array)
        | (TypeTag::Array, TypeTag::Ref)
        | (TypeTag::Array, TypeTag::Array) => Ok(()),
        (TypeTag::Float, TypeTag::Int) => {
            *src_pay = (*src_pay as i32 as f32).to_bits();
            *src_tag = TypeTag::Float;
            Ok(())
        }
        (TypeTag::Int, TypeTag::Float) => {
            *dst_pay = (*dst_pay as i32 as f32).to_bits();
            *dst_tag = TypeTag::Float;
            Ok(())
        }
        _ => Err(err),
    }
}

/// Assignment/numeric conversion of one value toward a target type.
///
/// `I`→`F` is exact (`fild`); `F`→`I` truncates toward zero and keeps the
/// low 32 bits of the 64-bit result (the exe's `FUN_005d6750`
/// truncating-`fistp` to a qword; NaN/out-of-range yields `#IA` indefinite
/// whose low word is `0x0`).
/// `L`/`[`→`L`/`[` of a null reference passes; anything else fails closed.
///
/// exe: FUN_00417da0
pub fn assign_convert(value: &mut Value, target: TypeTag) -> Result<(), ConvertError> {
    let err = ConvertError {
        from: value.tag.byte(),
        to: target.byte(),
    };
    match (target, value.tag) {
        (a, b) if a == b => Ok(()),
        (TypeTag::Float, TypeTag::Int) => {
            value.payload = (value.payload as i32 as f32).to_bits();
            value.tag = TypeTag::Float;
            Ok(())
        }
        (TypeTag::Int, TypeTag::Float) => {
            value.payload = trunc_f32_to_i32_bits(value.payload);
            value.tag = TypeTag::Int;
            Ok(())
        }
        (TypeTag::Ref | TypeTag::Array, TypeTag::Ref | TypeTag::Array) if value.payload == 0 => {
            value.tag = target;
            Ok(())
        }
        _ => Err(err),
    }
}

/// Conversion failure: the exe logs and the caller takes its fatal path.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ConvertError {
    /// First byte of the value's type name.
    pub from: u8,
    /// First byte of the target type name.
    pub to: u8,
}

/// Truncate `f32` bits toward zero, matching the exe's `FUN_005d6750`.
///
/// That helper sets a truncating control word, `fistp`s to a *qword*, and
/// returns the full 64-bit result in `edx:eax`; the caller keeps the low
/// 32 bits. Out-of-range/NaN inputs raise `#IA` and yield the integer
/// indefinite value `0x8000000000000000`, whose low word is `0x0`
/// (oracle-verified: NaN, `1e20`, `9e18` all yield `0x0`; `5e9` yields the
/// low word of `5000000000`).
fn trunc_f32_to_i32_bits(bits: u32) -> u32 {
    let v = f32::from_bits(bits);
    if v.is_nan() || v >= 9_223_372_036_854_775_808.0 || v < -9_223_372_036_854_775_808.0 {
        return 0x0000_0000;
    }
    (v.trunc() as i64 & 0xffff_ffff) as u32
}

/// Boolean conversion in place: `F` compares against `+0.0` (any nonzero,
/// including `-0.0`... note `-0.0 == +0.0`, so `-0.0` converts to 0, exactly
/// like the exe's `fcomp`+C3 test); `L`/`[` tests the payload against null;
/// `I` is already a truth value and passes through. The result type becomes
/// `I` with payload 0/1, except `I` inputs keep their payload.
///
/// exe: FUN_00417840
pub fn boolean_convert(value: &mut Value) {
    match value.tag {
        TypeTag::Float => {
            let is_true = value.as_f32() != 0.0;
            value.tag = TypeTag::Int;
            value.payload = u32::from(is_true);
        }
        TypeTag::Ref | TypeTag::Array => {
            value.tag = TypeTag::Int;
            value.payload = u32::from(value.payload != 0);
        }
        TypeTag::Int | TypeTag::Other(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_round_trips_first_byte() {
        assert_eq!(TypeTag::of(b'I'), TypeTag::Int);
        assert_eq!(TypeTag::of(b'F'), TypeTag::Float);
        assert_eq!(TypeTag::of(b'L'), TypeTag::Ref);
        assert_eq!(TypeTag::of(b'['), TypeTag::Array);
        assert_eq!(TypeTag::of(b'V'), TypeTag::Ref);
        assert_eq!(TypeTag::Int.byte(), b'I');
    }

    #[test]
    fn push_growth_matches_exe_rule() {
        // (cap, count) -> (new_cap, new_count) per FUN_00423ba0.
        assert_eq!(push_slot(0, 0), (1, 1));
        assert_eq!(push_slot(1, 1), (3, 2));
        assert_eq!(push_slot(3, 2), (3, 3));
        assert_eq!(push_slot(3, 3), (7, 4));
    }

    #[test]
    fn pop_underflow_is_none() {
        let mut s: Vec<u32> = vec![];
        assert_eq!(pop_slot(&mut s), None);
        let mut s = vec![7u32];
        assert_eq!(pop_slot(&mut s), Some(7));
        assert!(s.is_empty());
    }

    #[test]
    fn slot_kind_matches_vtable_probes() {
        // Value slot 0 (0x515610) answers 1, varslot slot 0 answers 2.
        assert_eq!(slot_kind(&StackCell::Value(Value::int(0))), 1);
        assert_eq!(
            slot_kind(&StackCell::Var(VarSlot {
                value: Value::int(0),
                name: 0,
            })),
            2
        );
    }

    #[test]
    fn assign_convert_matches_static_semantics() {
        // Same-type passes through (pointer identity in the exe).
        let mut v = Value::int(-5);
        assert!(assign_convert(&mut v, TypeTag::Int).is_ok());
        assert_eq!(v, Value::int(-5));
        // I -> F exact for small ints (fild).
        let mut v = Value::int(6);
        assert!(assign_convert(&mut v, TypeTag::Float).is_ok());
        assert_eq!(v.payload, 6f32.to_bits());
        // F -> I truncates toward zero, keeping the low 32 bits of the
        // 64-bit result (oracle-verified against FUN_00417da0).
        let mut v = Value::float(2.75);
        assert!(assign_convert(&mut v, TypeTag::Int).is_ok());
        assert_eq!(v, Value::int(2));
        let mut v = Value::float(-2.75);
        assert!(assign_convert(&mut v, TypeTag::Int).is_ok());
        assert_eq!(v, Value::int(-2));
        // Low word of 5000000000 (5e9 needs 64 bits).
        let mut v = Value::float(5e9);
        assert!(assign_convert(&mut v, TypeTag::Int).is_ok());
        assert_eq!(v.payload, 0x2a05_f200);
        // NaN yields #IA indefinite: low word 0x0.
        let mut v = Value {
            tag: TypeTag::Float,
            payload: f32::NAN.to_bits(),
        };
        assert!(assign_convert(&mut v, TypeTag::Int).is_ok());
        assert_eq!(v.payload, 0x0);
        // Null references pass across L/[ spellings.
        let mut v = Value::reference(0);
        assert!(assign_convert(&mut v, TypeTag::Array).is_ok());
        // Non-null across refs and I<->L both fail closed (log path).
        let mut v = Value::reference(0x2000);
        assert!(assign_convert(&mut v, TypeTag::Array).is_err());
        let mut v = Value::int(1);
        assert!(assign_convert(&mut v, TypeTag::Ref).is_err());
    }
}

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
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
    sync::atomic::{AtomicU64, Ordering},
};

/// Identity of one allocation lifetime. The slot is reused by the original
/// freelist; the generation distinguishes successive owners. These are opaque
/// arena identities, never executable/native memory addresses.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct CellId {
    pub pool: u64,
    pub slot: u32,
    pub generation: u32,
}

/// Unique ownership of a semantic value cell. Moving or re-pushing preserves
/// the identity; copying its Value explicitly creates a different cell.
/// Dropping on an error path does not synthesize an original release.
#[derive(Debug)]
pub struct OwnedCell {
    id: CellId,
    value: Value,
    pool: Weak<RefCell<OwnedPool>>,
    released: bool,
}
impl PartialEq for OwnedCell {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id && self.value == other.value
    }
}
impl Eq for OwnedCell {}
impl OwnedCell {
    pub fn id(&self) -> CellId {
        self.id
    }
    pub fn value(&self) -> Value {
        self.value
    }
    /// A complete, explicit semantic store. Native projections still execute
    /// their source helper for any internal instruction/allocator chronology.
    pub fn edit<R>(&mut self, edit: impl FnOnce(&mut Value) -> R) -> R {
        let before = self.value;
        let result = edit(&mut self.value);
        if let Some(pool) = self.pool.upgrade() {
            pool.borrow().trace.machine.note(crate::machine::Event::CellStore {
                identity: self.id, before, after: self.value,
            });
        }
        result
    }
}
impl PartialEq<Value> for OwnedCell {
    fn eq(&self, other: &Value) -> bool {
        self.value == *other
    }
}
impl Drop for OwnedCell {
    fn drop(&mut self) {
        if self.released {
            return;
        }
        // Original errors can abandon a popped pointer without freeing it.
        // Preserve an immutable view of that still-live cell; never invent a
        // destructor/release or make the address available for reuse.
        if let Some(pool) = self.pool.upgrade() {
            let mut pool = pool.borrow_mut();
            let entry = &mut pool.entries[self.id.slot as usize];
            if entry.leased && entry.generation == self.id.generation {
                entry.abandoned = Some(self.value);
                pool.trace.machine.note(crate::machine::Event::CellAbandon {
                    identity: self.id, value: self.value,
                });
            }
        }
    }
}
impl std::ops::Deref for OwnedCell {
    type Target = Value;
    fn deref(&self) -> &Value {
        &self.value
    }
}
impl std::ops::DerefMut for OwnedCell {
    fn deref_mut(&mut self) -> &mut Value {
        if let Some(pool) = self.pool.upgrade() {
            pool.borrow().trace.machine.note(crate::machine::Event::CellWriteAccess {
                identity: self.id, before: self.value,
            });
        }
        &mut self.value
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValuePoolSnapshot {
    pub handed: u32,
    pub capacity: u32,
    /// Head first, as in the original intrusive freelist.
    pub free: Vec<u32>,
    pub live: i32,
    /// Still-allocated cells abandoned by an original error/stop path.
    pub abandoned: Vec<(CellId, Value)>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CellEvent {
    Allocate { id: CellId, origin: AllocOrigin },
    Release { id: CellId },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PoolKind {
    Value,
    Variable,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PoolEvent {
    Allocate {
        kind: PoolKind,
        slot: u32,
        origin: AllocOrigin,
        cell: PoolCellView,
    },
    Release {
        kind: PoolKind,
        slot: u32,
        cell: PoolCellView,
    },
}
/// Immutable observation of a precise allocation lifetime. The name word is
/// present only for a variable cell; it is a semantic name identity, not a
/// claimed original pointer. Raw observers must resolve it explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PoolCellView {
    pub kind: PoolKind,
    pub pool: u64,
    pub slot: u32,
    pub generation: u32,
    pub value: Value,
    pub name: Option<u32>,
}
impl OwnedCell {
    pub fn view(&self) -> PoolCellView {
        PoolCellView {
            kind: PoolKind::Value,
            pool: self.id.pool,
            slot: self.id.slot,
            generation: self.id.generation,
            value: self.value,
            name: None,
        }
    }
}
/// Shared, opt-in immutable event observations across both global pools.
#[derive(Clone, Debug, Default)]
pub struct PoolTrace {
    events: Rc<RefCell<Option<Vec<PoolEvent>>>>,
    machine: crate::machine::Trace,
}
impl PoolTrace {
    pub fn observe(&self, enabled: bool) {
        *self.events.borrow_mut() = enabled.then(Vec::new);
    }
    pub fn events(&self) -> Vec<PoolEvent> {
        self.events.borrow().clone().unwrap_or_default()
    }
    pub(crate) fn note(&self, event: PoolEvent) {
        self.machine.note(crate::machine::Event::Pool(event.clone()));
        if let Some(events) = &mut *self.events.borrow_mut() {
            events.push(event);
        }
    }
    pub(crate) fn machine(&self) -> crate::machine::Trace { self.machine.clone() }
}
#[derive(Debug)]
struct OwnedEntry {
    generation: u32,
    leased: bool,
    stale: Value,
    abandoned: Option<Value>,
}
#[derive(Debug)]
struct OwnedPool {
    id: u64,
    entries: Vec<OwnedEntry>,
    free: Vec<u32>,
    live: i32,
    events: Option<Vec<CellEvent>>,
    trace: PoolTrace,
}
/// Explicit shared owner of the original global value pool. Cloning this
/// handle shares the pool, not any OwnedCell. Standalone threads create their
/// own pool; VmHost-created threads receive the same handle.
#[derive(Clone, Debug)]
pub struct SharedValuePool(Rc<RefCell<OwnedPool>>);
impl Default for SharedValuePool {
    fn default() -> Self {
        Self::with_trace(PoolTrace::default())
    }
}
impl SharedValuePool {
    pub(crate) fn with_trace(trace: PoolTrace) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(Rc::new(RefCell::new(OwnedPool {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            entries: vec![],
            free: vec![],
            live: 0,
            events: None,
            trace,
        })))
    }
    pub fn allocate(&self, value: Value) -> OwnedCell {
        let mut pool = self.0.borrow_mut();
        let (slot, origin) = if let Some(slot) = pool.free.pop() {
            (slot, AllocOrigin::Freelist)
        } else {
            let slot = pool.entries.len() as u32;
            pool.entries.push(OwnedEntry {
                generation: 0,
                leased: false,
                stale: Value::int(0),
                abandoned: None,
            });
            (slot, AllocOrigin::Pooled)
        };
        let pool_id = pool.id;
        let entry = &mut pool.entries[slot as usize];
        assert!(!entry.leased, "value pool returned an already-owned cell");
        entry.leased = true;
        entry.abandoned = None;
        let id = CellId {
            pool: pool_id,
            slot,
            generation: entry.generation,
        };
        pool.live += 1;
        if let Some(events) = &mut pool.events {
            events.push(CellEvent::Allocate { id, origin });
        }
        pool.trace.note(PoolEvent::Allocate {
            kind: PoolKind::Value,
            slot,
            origin,
            cell: PoolCellView {
                kind: PoolKind::Value,
                pool: id.pool,
                slot,
                generation: id.generation,
                value,
                name: None,
            },
        });
        OwnedCell {
            id,
            value,
            pool: Rc::downgrade(&self.0),
            released: false,
        }
    }
    pub fn release(&self, mut cell: OwnedCell) {
        let mut pool = self.0.borrow_mut();
        assert_eq!(cell.id.pool, pool.id, "release into a different value pool");
        let entry = &mut pool.entries[cell.id.slot as usize];
        assert!(
            entry.leased && entry.generation == cell.id.generation,
            "stale or duplicated value ownership"
        );
        cell.released = true;
        entry.leased = false;
        entry.stale = cell.value;
        entry.generation = entry.generation.wrapping_add(1);
        pool.free.push(cell.id.slot);
        pool.live -= 1;
        if let Some(events) = &mut pool.events {
            events.push(CellEvent::Release { id: cell.id });
        }
        pool.trace.note(PoolEvent::Release {
            kind: PoolKind::Value,
            slot: cell.id.slot,
            cell: cell.view(),
        });
    }
    pub fn owns(&self, cell: &OwnedCell) -> bool {
        self.0.borrow().id == cell.id.pool
    }
    pub fn live(&self) -> i32 {
        self.0.borrow().live
    }
    pub fn snapshot(&self) -> ValuePoolSnapshot {
        let pool = self.0.borrow();
        let handed = pool.entries.len() as u32;
        ValuePoolSnapshot {
            handed,
            capacity: handed.div_ceil(512) * 512,
            free: pool.free.iter().rev().copied().collect(),
            live: pool.live,
            abandoned: pool
                .entries
                .iter()
                .enumerate()
                .filter_map(|(slot, entry)| {
                    entry.abandoned.map(|value| {
                        (
                            CellId {
                                pool: pool.id,
                                slot: slot as u32,
                                generation: entry.generation,
                            },
                            value,
                        )
                    })
                })
                .collect(),
        }
    }
    /// Opt-in bounded investigation trace; normal execution retains no log.
    pub fn observe(&self, enabled: bool) {
        self.0.borrow_mut().events = enabled.then(Vec::new);
    }
    pub fn events(&self) -> Vec<CellEvent> {
        self.0.borrow().events.clone().unwrap_or_default()
    }
}

/// A type name: the exe's full type identity for values.
///
/// The exe stores a value cell's type as a *pointer* to an interned type
/// name (`cell+4`): `I`, `F`, `V`, `L;`, `Ljava.lang.String;`, `[I`, ... All
/// helper dispatch reads only the first byte (`cVar1 == 'F'`, `== 'I'`,
/// `== 'L' || == '['`), but the string check (`FUN_00417a70`, exact
/// `Ljava.lang.String;` compare) and the conversion/assignability walkers
/// (`FUN_00417da0`, `FUN_00405400`) read the whole name. So a tag carries
/// the full name; [`TypeTag::of`] keeps the exe's first-byte dispatch.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TypeTag {
    /// `I`: 32-bit integer (payload is the word itself).
    Int,
    /// `F`: 32-bit float (payload is `f32` bits).
    Float,
    /// Any `L...;` class or `V` void marker. Carries the full name so the
    /// string check and the class-loader conversion can read it; the exe
    /// treats `L` and `[` identically in promotion/conversion reads
    /// (`cVar1 == 'L' || cVar1 == '['`). Native explicit-V returns retain a
    /// V:0 cell; casts validate its descriptor separately from references.
    Ref(&'static str),
    /// Any `[...` array spelling. Same word representation as [`TypeTag::Ref`];
    /// kept distinct so callers can preserve the spelling they observed.
    Array(&'static str),
    /// Any other leading byte. Helpers only ever read the first byte, so an
    /// unexpected tag fails closed at the same points the exe logs.
    Other(u8),
}

impl TypeTag {
    /// Read the tag the way the exe dispatches: first byte of the type
    /// name. `L`/`[` keep the full `name` for the string check and class
    /// resolution; `I`/`F`/others discard it (the exe never reads past
    /// byte 0 for them).
    #[must_use]
    pub fn of(first_byte: u8) -> Self {
        Self::named(first_byte, "")
    }

    /// Bind a loaded descriptor to a process-stable interned spelling, like
    /// the executable's JVM type-name interner. Equal names share storage;
    /// Copy value cells can safely retain a tag beyond a loader borrow.
    pub fn intern(name: &str) -> Self {
        use std::collections::HashSet;
        use std::sync::{Mutex, OnceLock};
        static NAMES: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
        let mut names = NAMES
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
            .unwrap();
        let name = if let Some(existing) = names.get(name) {
            *existing
        } else {
            let name: &'static str = Box::leak(name.to_owned().into_boxed_str());
            names.insert(name);
            name
        };
        Self::named(name.as_bytes().first().copied().unwrap_or(0), name)
    }

    /// Read the tag with its full interned type name.
    #[must_use]
    pub fn named(first_byte: u8, name: &'static str) -> Self {
        match first_byte {
            b'I' => Self::Int,
            b'F' => Self::Float,
            b'L' | b'V' => Self::Ref(name),
            b'[' => Self::Array(name),
            b => Self::Other(b),
        }
    }

    /// The full type name when the tag carries one (`L`/`[`), else the
    /// single-byte spelling.
    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            Self::Ref(n) | Self::Array(n) => n,
            Self::Int => "I",
            Self::Float => "F",
            Self::Other(_) => "?",
        }
    }

    /// The first byte, for writes back into a cell.
    #[must_use]
    pub fn byte(self) -> u8 {
        match self {
            Self::Int => b'I',
            Self::Float => b'F',
            Self::Ref(_) => b'L',
            Self::Array(_) => b'[',
            Self::Other(b) => b,
        }
    }

    /// `true` for the `L`/`[` reference family (exe: both arms together).
    #[must_use]
    pub fn is_ref(self) -> bool {
        matches!(self, Self::Ref(_) | Self::Array(_))
    }

    /// The exe's string-descriptor spelling for the `+` check.
    pub const STRING: &'static str = "Ljava.lang.String;";
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

    /// Reference cell (object/array word, 0 = null). The name defaults to
    /// the bare `L;` descriptor; use [`Value::reference_named`] for a full name.
    #[must_use]
    pub fn reference(word: u32) -> Self {
        Self {
            tag: TypeTag::Ref("L;"),
            payload: word,
        }
    }

    /// Reference cell with a full interned type name.
    ///
    /// The word plus descriptor binding `FUN_00423e10` performs — it interns
    /// the `String` at `arg0+0x68` (`FUN_0041e800`) and stores the resulting
    /// word into the cell whose descriptor the caller supplies.
    ///
    /// exe: FUN_00423e10
    #[must_use]
    pub fn reference_named(word: u32, name: &'static str) -> Self {
        Self {
            tag: TypeTag::named(name.as_bytes()[0], name),
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
            TypeTag::Ref(n) | TypeTag::Array(n) => {
                if n.is_empty() || n == "L;" || n == "[" {
                    write!(f, "R({:#x})", self.payload)
                } else {
                    write!(f, "R({} {:#x})", n, self.payload)
                }
            }
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
        | (TypeTag::Ref(_), TypeTag::Ref(_))
        | (TypeTag::Ref(_), TypeTag::Array(_))
        | (TypeTag::Array(_), TypeTag::Ref(_))
        | (TypeTag::Array(_), TypeTag::Array(_)) => Ok(()),
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
/// The class-loader paths (an `I`/`F` value into an `L` target, or a
/// non-null `L`→`L` across classes) need the loader and live string objects;
/// they are [`LoaderConvert`] (static port, same function).
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
        (TypeTag::Ref(_) | TypeTag::Array(_), TypeTag::Ref(_) | TypeTag::Array(_))
            if value.payload == 0 =>
        {
            value.tag = target;
            Ok(())
        }
        _ => Err(err),
    }
}

/// What the class-loader half of `FUN_00417da0` needs: load the target
/// class, test assignability, and format/lookup strings. The interpreter's
/// [`crate::interp::Host`] provides it; tests use a fake.
///
/// Identity model (#110 steps 1-5): the interface carries
/// [`crate::vmclass::ClassRef`] instead of bare names where the record
/// identity matters. `load_class` resolves the descriptor once at the
/// boundary and the returned reference is retained by the caller;
/// `reference_class` returns the object's authoritative runtime reference
/// (loaded record or array spelling). Descriptor text stays an argument
/// for the canonical-cell checks and logging, never a re-resolution key.
/// Production duplicate-name registration stays rejected, so the selected
/// record and its diagnostic name still agree one-to-one.
pub trait LoaderConvert {
    /// Load-or-get the class named by a bare (`java.lang.String`) or
    /// descriptor (`Ljava.lang.String;`) spelling (`FUN_00410890`).
    /// Returns the opaque class reference for assignability checks.
    /// Arrays keep their explicit spelling and own no record.
    fn load_class(&mut self, descriptor: &str) -> Option<crate::vmclass::ClassRef>;
    /// Assignability of a loaded reference into the target (`FUN_00405400`).
    /// Array references never convert; loaded records use the ID-carrying
    /// cost against the target's selected record.
    fn is_assignable(&mut self, from: &crate::vmclass::ClassRef, to: &str) -> bool;
    /// Numeric conversion uses Class::isInheritedFrom with target -> String,
    /// independently of405400's reference-conversion predicate.
    fn string_target(
        &mut self,
        target: &crate::vmclass::ClassRef,
        string: &crate::vmclass::ClassRef,
    ) -> bool;
    /// Runtime class of a non-null reference's payload (`instance+0xc`),
    /// which can differ from the cell's declared descriptor. Standalone
    /// loader backends may use the descriptor when they have no object heap.
    fn reference_class(&mut self, value: Value) -> Option<crate::vmclass::ClassRef> {
        self.load_class(value.tag.name())
    }
    /// Format an `I`/`F` payload the way the exe's `sprintf` does
    /// (`%d` / `%.3f`) and intern it as a `String` object, returning the
    /// object word (`FUN_004044e0` + `FUN_004174a0`).
    fn make_string(&mut self, value: Value) -> u32;
    /// The full type name of the target descriptor (for the converted
    /// cell's tag).
    fn target_name(&self, descriptor: &str) -> &'static str;
}

/// Class-loader conversion: the `L`/`[`-target half of `FUN_00417da0`.
///
/// - Same descriptor: pass (pointer identity in the exe).
/// - `I`/`F` source: load the target and `java.lang.String`, require the target
///   to inherit String
///   (`FUN_00411610` + `FUN_004044e0`), format (`%d`/`%.3f`) and intern
///   (`FUN_004174a0`); the cell becomes a `String` reference.
/// - Non-null `L`/`[` source: load both classes, check assignability
///   (`FUN_00412670` + `FUN_00405400`); on success the tag becomes the
///   target descriptor.
/// - Null source: passes with the target tag (covered by
///   [`assign_convert`]; repeated here so callers have one entry point).
///
/// Anything else fails closed (the exe logs and the caller takes its
/// fatal path). Static port: the `sprintf` spellings (`%d`, `%.3f`) are
/// decoded from exe bytes; no differential test (needs a booted JVM).
///
/// exe: FUN_00417da0
pub fn loader_convert(
    loader: &mut dyn LoaderConvert,
    value: &mut Value,
    target: &str,
) -> Result<(), ConvertError> {
    loader_convert_ref(loader, value, target)
}

/// ID-carrying conversion entry: same semantics as [`loader_convert`]; the
/// `Ref` suffix names the [`crate::vmclass::ClassRef`] identity the
/// [`LoaderConvert`] boundary now carries (loaded record vs array spelling).
/// exe: FUN_00417da0
pub fn loader_convert_ref(
    loader: &mut dyn LoaderConvert,
    value: &mut Value,
    target: &str,
) -> Result<(), ConvertError> {
    let err = || ConvertError {
        from: value.tag.byte(),
        to: target.as_bytes()[0],
    };
    let tfirst = target.as_bytes()[0];
    if tfirst != b'L' && tfirst != b'[' {
        return Err(err());
    }
    if value.tag.name() == target {
        return Ok(());
    }
    match value.tag {
        TypeTag::Int | TypeTag::Float => {
            let target_ref = loader.load_class(target).ok_or_else(err)?;
            let string_ref = loader.load_class("java.lang.String").ok_or_else(err)?;
            if !loader.string_target(&target_ref, &string_ref) {
                // The target must inherit String; the original permits a
                // subclass tag even though4174a0 allocates an actual String.
                return Err(err());
            }
            let word = loader.make_string(*value);
            let name = loader.target_name(target);
            value.tag = TypeTag::named(tfirst, name);
            value.payload = word;
            Ok(())
        }
        TypeTag::Ref(_) | TypeTag::Array(_) => {
            if value.payload == 0 {
                let name = loader.target_name(target);
                value.tag = TypeTag::named(tfirst, name);
                return Ok(());
            }
            let from = loader.reference_class(*value).ok_or_else(err)?;
            let _to = loader.load_class(target).ok_or_else(err)?;
            if loader.is_assignable(&from, target) {
                let name = loader.target_name(target);
                value.tag = TypeTag::named(tfirst, name);
                Ok(())
            } else {
                Err(err())
            }
        }
        _ => Err(err()),
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

/// Boolean conversion in place: `F` compares against `+0.0` through the
/// exe's `fcomp` + C3 test, so `-0.0` and **NaN** (unordered sets C3, like
/// "equal") both convert to 0; `L`/`[` tests the payload against null; `I`
/// is already a truth value and passes through. The result type becomes
/// `I` with payload 0/1, except `I` inputs keep their payload. (The NaN
/// case was found by the `Thread::run` differential vectors, M1-03: the
/// `vmval` vectors of M1-02 did not cover it.)
///
/// exe: FUN_00417840
pub fn boolean_convert(value: &mut Value) {
    match value.tag {
        TypeTag::Float => {
            let f = value.as_f32();
            let is_true = !(f == 0.0 || f.is_nan());
            value.tag = TypeTag::Int;
            value.payload = u32::from(is_true);
        }
        TypeTag::Ref(_) | TypeTag::Array(_) => {
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
        assert_eq!(TypeTag::of(b'L'), TypeTag::Ref(""));
        assert_eq!(TypeTag::of(b'['), TypeTag::Array(""));
        assert_eq!(TypeTag::of(b'V'), TypeTag::Ref(""));
        assert_eq!(
            TypeTag::named(b'L', "Ljava.lang.String;"),
            TypeTag::Ref("Ljava.lang.String;")
        );
        assert_eq!(TypeTag::Ref("L;").byte(), b'L');
        assert_eq!(TypeTag::Ref(TypeTag::STRING).name(), TypeTag::STRING);
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
    fn boolean_convert_treats_nan_and_negative_zero_as_false() {
        // Oracle-verified through `Thread::run` (`NaN && 1` is 0).
        for (bits, want) in [
            (f32::NAN.to_bits(), 0),
            ((-0.0f32).to_bits(), 0),
            (0.0f32.to_bits(), 0),
            (0.5f32.to_bits(), 1),
            (f32::INFINITY.to_bits(), 1),
        ] {
            let mut v = Value {
                tag: TypeTag::Float,
                payload: bits,
            };
            boolean_convert(&mut v);
            assert_eq!(v, Value::int(want), "bits {bits:#x}");
        }
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
        assert!(assign_convert(&mut v, TypeTag::Array("")).is_ok());
        // Non-null across refs and I<->L both fail closed (log path).
        let mut v = Value::reference(0x2000);
        assert!(assign_convert(&mut v, TypeTag::Array("")).is_err());
        let mut v = Value::int(1);
        assert!(assign_convert(&mut v, TypeTag::Ref("")).is_err());
    }

    struct FakeLoader {
        classes: Vec<String>,
        next_word: u32,
        made: Vec<Value>,
    }

    impl FakeLoader {
        fn with(classes: &[&str]) -> Self {
            Self {
                classes: classes.iter().map(|s| s.to_string()).collect(),
                next_word: 0x2000,
                made: Vec::new(),
            }
        }
    }

    impl LoaderConvert for FakeLoader {
        fn load_class(&mut self, descriptor: &str) -> Option<crate::vmclass::ClassRef> {
            use crate::vmclass::{ArrayDescriptor, ClassId, ClassRef};
            if let Some(array) = ArrayDescriptor::new(descriptor) {
                return Some(ClassRef::Array(array));
            }
            let bare = descriptor
                .strip_prefix('L')
                .and_then(|s| s.strip_suffix(';'))
                .unwrap_or(descriptor);
            self.classes
                .iter()
                .position(|c| c.as_str() == bare)
                .map(|index| ClassRef::Loaded(ClassId::from_raw(index as u32 + 1).unwrap()))
        }

        fn is_assignable(&mut self, from: &crate::vmclass::ClassRef, to: &str) -> bool {
            use crate::vmclass::ClassRef;
            let bare = to
                .strip_prefix('L')
                .and_then(|s| s.strip_suffix(';'))
                .unwrap_or(to);
            match from {
                ClassRef::Loaded(id) => {
                    let index = id.get() as usize - 1;
                    self.classes.get(index).is_some_and(|name| name == bare)
                }
                ClassRef::Array(_) => false,
            }
        }

        fn string_target(
            &mut self,
            target: &crate::vmclass::ClassRef,
            _string: &crate::vmclass::ClassRef,
        ) -> bool {
            use crate::vmclass::ClassRef;
            matches!(target, ClassRef::Loaded(id) if self.classes.get(id.get() as usize - 1).is_some_and(|name| name == "java.lang.String"))
        }

        fn make_string(&mut self, value: Value) -> u32 {
            // The exe formats %d / %.3f; the fake records the input and
            // hands out object words.
            self.made.push(value);
            let w = self.next_word;
            self.next_word += 1;
            w
        }

        fn target_name(&self, descriptor: &str) -> &'static str {
            if descriptor == "Ljava.lang.String;" {
                TypeTag::STRING
            } else if descriptor == "L;" {
                "L;"
            } else {
                ""
            }
        }
    }

    #[test]
    fn loader_convert_int_to_string_formats_like_sprintf() {
        // FUN_00417da0 L-target I/F path (static port): load String,
        // check assignability, format, intern. 42 -> "42" (%d).
        let mut l = FakeLoader::with(&["java.lang.String"]);
        let mut v = Value::int(42);
        assert!(loader_convert(&mut l, &mut v, "Ljava.lang.String;").is_ok());
        assert_eq!(v.tag, TypeTag::Ref(TypeTag::STRING));
        assert_eq!(v.payload, 0x2000);
        assert_eq!(l.made, vec![Value::int(42)]);
        // Float goes through the same path (%.3f in the exe).
        let mut v = Value::float(1.5);
        assert!(loader_convert(&mut l, &mut v, "Ljava.lang.String;").is_ok());
        assert_eq!(v.payload, 0x2001);
    }

    #[test]
    fn loader_convert_ref_paths() {
        let mut l = FakeLoader::with(&["java.lang.String", "game.Widget"]);
        // Null passes with the target tag.
        let mut v = Value::reference(0);
        assert!(loader_convert(&mut l, &mut v, "Ljava.lang.String;").is_ok());
        assert_eq!(v.tag, TypeTag::Ref(TypeTag::STRING));
        // Same descriptor passes untouched.
        let mut v = Value::reference_named(7, "Lgame.Widget;");
        assert!(loader_convert(&mut l, &mut v, "Lgame.Widget;").is_ok());
        assert_eq!(v.payload, 7);
        // Unrelated classes fail closed (cannot cast).
        let mut v = Value::reference_named(7, "Lgame.Widget;");
        assert!(loader_convert(&mut l, &mut v, "Ljava.lang.String;").is_err());
        // Missing class fails closed (cannot parse class).
        let mut v = Value::reference_named(7, "Lgame.Missing;");
        assert!(loader_convert(&mut l, &mut v, "Ljava.lang.String;").is_err());
        // Non-L target fails closed.
        let mut v = Value::int(1);
        assert!(loader_convert(&mut l, &mut v, "I").is_err());
    }
}

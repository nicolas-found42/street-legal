//! The TUFA interpreter loop `Thread::run` (exe `FUN_00420ff0`), frames and
//! the script-method call protocol.
//!
//! Machine model (all from `docs/tufa-executor.md`, now backed by the
//! Unicorn `Thread::run` vectors in `re/out/thread/`):
//!
//! - a thread owns a stack of [`Frame`]s (the exe keeps them in a doubly
//!   linked list at `thread+0xc`; the current frame is `thread+0x20`) and a
//!   program counter [`Pc`] (an 8-byte node pointer in the exe: `thread+0x24`);
//! - a frame holds the saved pc (`frame+0x2c`), receiver (`+0x30`), class
//!   context (`+0x34`), signature (`+0x38`), the locals container (`+0x18`,
//!   variable slots) and the operand stack (`+0x28`);
//! - the operand stack holds [`Slot`]s: a plain value cell, a pointer to a
//!   variable slot (an lvalue) or null. Ownership of value cells is tracked
//!   by the live-cell counter `DAT_0062f138` ([`Thread::live`]): every cell
//!   the exe creates (literal, copy, variable slot) is `+1`, every cell it
//!   releases through vtable slot 1 is `-1`; the differential test compares
//!   the counter after every program, which pins the exact release
//!   discipline of every opcode.
//!
//! Everything that needs the class loader, the constant pool or the string
//! pool (name resolution, fields, method calls, `new`, casts, string
//! literals) is delegated to the [`Host`] trait (ticket #31, M1-04); the
//! loop itself, jumps, literals, locals, assignments, the operator
//! sub-ops and returns are ported and oracle-verified here.
//!
//! Error model. The exe reports script errors through `FUN_004190d0`
//! ("fatal script error") which logs and then continues with garbage; the
//! port stops the thread with [`VmError`] instead (a deliberate
//! simplification: behaviour after an error is undefined in the original).

use slr_fmt_tufa::Node;
use slr_x87::{Precision, Rounding, X87};

use crate::ops::{Alu, OpError};
use crate::values::{boolean_convert, TypeTag, Value};

/// Thread flag bits (`thread+0x2c`, docs: "Thread flags").
pub mod flags {
    /// The thread is inside `run`.
    pub const RUNNING: u32 = 0x04;
    /// Waiting on a monitor.
    pub const WAITING: u32 = 0x10;
    /// Notified.
    pub const NOTIFIED: u32 = 0x20;
    /// Stopped (`0x0010` op, or a return into a null pc).
    pub const STOPPED: u32 = 0x40;
    /// Dead.
    pub const DEAD: u32 = 0x80;
    /// `run` leaves its loop when any of these is set (`& 0x78`).
    pub const LOOP_ABORT: u32 = 0x78;
    /// The scheduler check `FUN_00423b90` fails when any of these is set.
    pub const SCHED_BLOCK: u32 = 0x82;
}

/// Identity of a node array (a method body). The port's stand-in for the
/// exe's raw node pointers; the host maps it to a `&[Node]`.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct BodyId(pub u32);

/// A program counter: node `idx` of body `body` (`thread+0x24`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pc {
    /// Which node array.
    pub body: BodyId,
    /// Node index within it.
    pub idx: usize,
}

/// Handle of a variable slot (an lvalue; locals and `this`). Carries the
/// slot's generation so a stale handle to a recycled slot is detected
/// (the exe would silently alias the new occupant).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct VarId {
    index: u32,
    generation: u32,
}

/// A variable slot: current value words plus the interned name word
/// (`+0xc`, carried untouched).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Var {
    /// Current value.
    pub value: Value,
    /// Interned variable-name word.
    pub name: u32,
}

#[derive(Clone, Debug)]
struct VarEntry {
    generation: u32,
    var: Option<Var>,
}

/// Arena of variable slots (the exe's variable pool `DAT_0062f134`): freed
/// slots go on a free list and are reused, like the exe's pool freelist.
#[derive(Clone, Debug, Default)]
pub struct Vars {
    slots: Vec<VarEntry>,
    free: Vec<u32>,
}

impl Vars {
    /// Allocate a slot, reusing a freed one when available.
    pub fn alloc(&mut self, value: Value, name: u32) -> VarId {
        let var = Some(Var { value, name });
        if let Some(index) = self.free.pop() {
            let e = &mut self.slots[index as usize];
            e.var = var;
            return VarId {
                index,
                generation: e.generation,
            };
        }
        self.slots.push(VarEntry { generation: 0, var });
        VarId {
            index: self.slots.len() as u32 - 1,
            generation: 0,
        }
    }

    /// Number of slots ever created (live + free); the arena's footprint.
    #[must_use]
    pub fn slot_count(&self) -> usize {
        self.slots.len()
    }

    /// Read a slot, `None` if it was released (stale handle).
    #[must_use]
    pub fn try_get(&self, id: VarId) -> Option<&Var> {
        let e = self.slots.get(id.index as usize)?;
        if e.generation == id.generation {
            e.var.as_ref()
        } else {
            None
        }
    }

    /// Read a slot. Panics on a released slot (a dangling lvalue; undefined
    /// behaviour in the exe).
    #[must_use]
    pub fn get(&self, id: VarId) -> &Var {
        self.try_get(id).expect("dangling variable slot")
    }

    fn checked(&mut self, id: VarId) -> Result<&mut Var, VmError> {
        self.slots
            .get_mut(id.index as usize)
            .filter(|e| e.generation == id.generation)
            .and_then(|e| e.var.as_mut())
            .ok_or(VmError::DanglingVar)
    }

    fn free(&mut self, id: VarId) {
        if let Some(e) = self.slots.get_mut(id.index as usize) {
            if e.generation == id.generation && e.var.take().is_some() {
                e.generation = e.generation.wrapping_add(1);
                self.free.push(id.index);
            }
        }
    }
}

/// Address of an authoritative host-owned variable cell. Fetching its
/// lvalue borrows the cell; dereferencing creates a temporary value copy.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum HeapSlot {
    /// An object's instance field (arrays use their own element address).
    Object { word: u32, slot: usize },
    /// A class-owned static field.
    Static { context: u32, slot: usize },
    /// A typed array element.
    Array { word: u32, index: usize },
}

#[derive(Clone, Copy)]
enum Lvalue {
    Local(VarId),
    Heap(HeapSlot),
}

/// An operand-stack word.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slot {
    /// Null pointer (underflow pop, out-of-range local push).
    Null,
    /// A plain value cell (vtable `0x5e734c`).
    Value(Value),
    /// A pointer to a variable slot (vtable `0x5e7344`): an lvalue.
    Var(VarId),
    /// Borrowed variable cell owned by the object model.
    Heap(HeapSlot),
}

/// One activation frame (exe: the `FUN_00407b10` object).
#[derive(Clone, Debug)]
pub struct Frame {
    /// `frame+0x2c`: where execution resumes (entry node for a fresh
    /// frame, the call node for a suspended caller). `None` is a null pc.
    pub saved_pc: Option<Pc>,
    /// `frame+0x30`: receiver word (0 for static methods).
    pub receiver: u32,
    /// `frame+0x34`: class context word (opaque to the loop).
    pub class_ctx: u32,
    /// `frame+0x38`: method signature word (opaque to the loop).
    pub signature: u32,
    /// `frame+0x18`: locals (variable slots), declaration order.
    pub locals: Vec<VarId>,
    /// `frame+0x28`: operand stack, bottom first.
    pub stack: Vec<Slot>,
}

impl Frame {
    /// A fresh frame (exe: `FUN_00407b10`).
    #[must_use]
    pub fn new(saved_pc: Option<Pc>, receiver: u32) -> Self {
        Self {
            saved_pc,
            receiver,
            class_ctx: 0,
            signature: 0,
            locals: Vec::new(),
            stack: Vec::new(),
        }
    }
}

/// Fatal array-access branches distinguished by the original interpreter.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ArrayError {
    IndexType,
    NegativeIndex,
    NotArray,
    NullArray,
}

/// Why the thread stopped abnormally.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum VmError {
    /// A diagnosed original array-access error, before operand release.
    ArrayAccess(ArrayError),
    /// `FUN_004190d0` reached: explicit fatal opwords (`0x0000`, `0x0013`,
    /// `0x0042`, `0x0048`, `0x004a`, `0x004d`).
    FatalOpcode(u16),
    /// `stack access: value on top (instead of variable)`: an lvalue was
    /// required (`++`/`--`, assignments).
    NotLvalue,
    /// `Thread::run: unhandled literal type`.
    UnhandledLiteral,
    /// Integer `idiv` raised `#DE` (the exe's SEH exception).
    IntegerTrap {
        /// Exe address of the helper that trapped.
        helper: u32,
    },
    /// Popped a null word and then used it (the exe dereferences null).
    NullSlot,
    /// An lvalue whose variable slot was already released.
    DanglingVar,
    /// The operation needs the (absent) current frame.
    NoFrame,
    /// A jump or step left the node array.
    PcOutOfRange,
    /// `Thread::run: empty thread` (pc is null on entry).
    EmptyThread,
    /// `Thread::run: already running`.
    AlreadyRunning,
    /// `Thread::callMethod: instance method called without a receiver`.
    InstanceMethodWithoutReceiver,
    /// The host does not implement this operation.
    Unsupported(u16),
}

impl VmError {
    /// `true` for errors the exe reports through its fatal-script-error
    /// path (as opposed to hardware traps and port-level failures).
    #[must_use]
    pub fn is_script_fatal(&self) -> bool {
        matches!(
            self,
            Self::FatalOpcode(_)
                | Self::NotLvalue
                | Self::UnhandledLiteral
                | Self::EmptyThread
                | Self::AlreadyRunning
                | Self::InstanceMethodWithoutReceiver
        )
    }
}

impl From<OpError> for VmError {
    fn from(e: OpError) -> Self {
        match e {
            OpError::IntegerTrap { helper } => Self::IntegerTrap { helper },
        }
    }
}

/// Operations the loop delegates to the object model (ticket #31).
///
/// Conventions for the host implementing them: the node at `pc` is the op;
/// operands are popped with [`Thread::pop_deref`] / [`Thread::pop_raw`] in
/// the exe's order (first pop = stack top); results pushed with
/// [`Thread::push_new`]; a host that consumes inline name nodes sets
/// `thread.pc` to the last node it consumed (the loop then steps past it).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ExternKind {
    /// `0x1002` declare a local (`FUN_004050b0`/`FUN_00408d60`): type from
    /// the pool entry `imm`, optional `0x402e` initialiser node, optional
    /// `0x002c` follower that also pushes the new lvalue.
    Declare,
    /// `0x1011` resolve an inline name (`FUN_00420d20`).
    ResolveName,
    /// `0x101b` field reference (static / instance).
    FieldRef,
    /// `0x101e` pack `imm` values into an argument array.
    ArgArray,
    /// `0x401c` member access on an object.
    MemberAccess,
    /// `0x001d` field by name.
    FieldByName,
    /// `0x0020` array element access.
    ArrayElement,
    /// `0x4025` instance method call through `callMethod`.
    CallInstance,
    /// `0x4026` static / super method call by name (`FUN_004207c0`).
    CallStatic,
    /// `0x0021` `<init>` call.
    CallInit,
    /// `0x0022` current defining-class constructor (`this`).
    CallThisInit,
    /// `0x0023` superclass constructor.
    CallSuperInit,
    /// `0x0024` general call through a resolved name (`FUN_004208e0`).
    CallResolved,
    /// `0x1007/0x1e` and `0x1008/0x1e` `new`.
    New,
    /// `0x1007/0x1f` and `0x1008/0x1f` `new T[]`.
    NewArray,
    /// `0x1007/0x20` `instanceof`.
    InstanceOf,
    /// `0x1007/0x21` cast.
    Cast,
    /// `0x1007/0x1c` followed by a resource Member literal (`0x100e`).
    ResourceLiteral,
    /// `0x1007/0x1c` followed by a String literal (`0x4009`).
    StringLiteral,
}

/// One delegated operation: what it is, where, and whether it came from the
/// statement form `0x1008` (result discarded).
#[derive(Clone, Copy, Debug)]
pub struct ExternOp {
    /// Which operation.
    pub kind: ExternKind,
    /// Position of the op's node.
    pub pc: Pc,
    /// The op's node.
    pub node: Node,
    /// `true` for the `0x1008` statement form.
    pub discard: bool,
}

/// How the loop proceeds after a delegated operation.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Flow {
    /// Step to the next node (`pc += 8`).
    Advance,
    /// Stay on `thread.pc` (a frame was entered: the callee's entry).
    Stay,
}

/// Which exe path reached the string-aware `+` (`FUN_0041b4d0`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RefOperator {
    /// Expression `a + b` (sub-op `0x10`), and `a - b` (sub-op `0xf`) after
    /// the negation of `b`: both values are plain cells, the result lands
    /// in `left`.
    Add,
    /// Compound `a += b` (`0x1007/0x1008` kind `0x27`) and `a -= b` (kind
    /// `0x28`, after negating `b`): `left` is the variable being assigned.
    AddAssign,
}

/// The object-model side of the interpreter.
pub trait Host {
    /// The node array of a body.
    fn body(&self, id: BodyId) -> &[Node];

    /// Runtime descriptor of a script receiver. A numeric-only host has no
    /// class metadata; object-model hosts must validate the actual heap word.
    fn receiver_value(&mut self, word: u32) -> Result<Value, VmError> {
        Ok(Value::reference(word))
    }

    /// Read a host-owned variable cell without creating a value temporary.
    fn read_cell(&mut self, _slot: HeapSlot) -> Result<Value, VmError> {
        Err(VmError::Unsupported(0x401c))
    }

    /// Store into the same authoritative cell returned by field lookup.
    fn write_cell(&mut self, _slot: HeapSlot, _value: Value) -> Result<(), VmError> {
        Err(VmError::Unsupported(0x401c))
    }

    /// Human label of a body for diagnostics (`class.method(sig)`).
    fn body_label(&self, body: BodyId) -> String {
        format!("body{}", body.0)
    }

    /// A delegated operation (see [`ExternKind`]). The default stops the
    /// thread with [`VmError::Unsupported`].
    fn extern_op(&mut self, _thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        Err(VmError::Unsupported(op.node.opword))
    }

    /// A string-aware `+` ([`RefOperator`]): called when an operand of `+`,
    /// `-`, `+=` or `-=` is a reference. The exe checks for
    /// `java.lang.String` and concatenates (`FUN_00417a70`/`FUN_0041b4d0`).
    /// The loop has already negated the right operand for the `-` forms
    /// (the exe negates before adding). Return `None` to fall back to the
    /// plain helper (which logs `unsupported class type`). For
    /// [`RefOperator::AddAssign`] `left` is a copy of the variable's value;
    /// whatever the host leaves in it is written back to the variable.
    fn operator_on_refs(
        &mut self,
        _op: RefOperator,
        _left: &mut Value,
        _right: &mut Value,
    ) -> Option<Result<(), VmError>> {
        None
    }

    /// The concatenating arm of `FUN_0041b4d0` for two `String` cells, tried
    /// after [`Self::operator_on_refs`] declines. `None` falls back to the
    /// plain helper.
    fn concatenate_strings(
        &mut self,
        _left: &mut Value,
        _right: &mut Value,
    ) -> Option<Result<(), VmError>> {
        None
    }

    /// Typed plain assignment. Object-model hosts provide loaded-class
    /// conversion; numeric-only hosts retain the standalone helper behavior.
    fn store_value(&mut self, alu: &mut Alu, destination: &mut Value, source: &mut Value) {
        alu.store(destination, source);
    }

    /// A reference value was stored into a variable (the exe's GC write
    /// barrier `FUN_004198b0`).
    fn on_reference_store(&mut self, _value: Value) {}
}

/// A thread: frames, pc, flags and the cell accounting.
#[derive(Clone, Debug)]
pub struct Thread {
    /// Frame stack, bottom first; the last is the current frame.
    pub frames: Vec<Frame>,
    /// `thread+0x24`.
    pub pc: Option<Pc>,
    /// `thread+0x2c` (see [`flags`]).
    pub flags: u32,
    /// Variable slots.
    pub vars: Vars,
    /// Live-cell counter `DAT_0062f138`.
    pub live: i32,
    /// Operator state (x87 precision, diagnostics).
    pub alu: Alu,
    /// Line number of the node being executed (`jvm+0x70`, error reports).
    pub line: u16,
}

impl Default for Thread {
    fn default() -> Self {
        Self::new()
    }
}

thread_local! {
    static STEP_TRACE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Print every executed node to stderr (diagnostic).
pub fn set_step_trace(on: bool) {
    STEP_TRACE.with(|t| t.set(on));
}

/// What a step decided.
enum Next {
    Advance,
    Stay,
    Exit,
}

const RC: Rounding = Rounding::NearestEven;

impl Thread {
    /// An empty thread (no frames). Add frames with `frames.push`.
    #[must_use]
    pub fn new() -> Self {
        Self {
            frames: Vec::new(),
            pc: None,
            flags: 0,
            vars: Vars::default(),
            live: 0,
            alu: Alu::default(),
            line: 0,
        }
    }

    // ---- operand stack primitives -------------------------------------

    fn frame(&mut self) -> Result<&mut Frame, VmError> {
        self.frames.last_mut().ok_or(VmError::NoFrame)
    }

    /// Push a word on the current frame's stack (`FUN_00423ba0`).
    pub fn push(&mut self, slot: Slot) -> Result<(), VmError> {
        self.frame()?.stack.push(slot);
        Ok(())
    }

    /// Push a freshly created value cell (`+1` live).
    pub fn push_new(&mut self, v: Value) -> Result<(), VmError> {
        self.live += 1;
        self.push(Slot::Value(v))
    }

    /// Raw pop (`FUN_00423d40`): no dereference, underflow yields null.
    pub fn pop_raw(&mut self) -> Result<Slot, VmError> {
        Ok(self.frame()?.stack.pop().unwrap_or(Slot::Null))
    }

    /// Dereferencing pop (`FUN_0041f7d0`): a variable slot yields a fresh
    /// copy of its value (`+1` live); a plain value comes back as is.
    pub fn pop_deref(&mut self) -> Result<Value, VmError> {
        let slot = self.pop_raw()?;
        self.deref_local(slot)
    }

    /// Dereference either a local or host-owned lvalue, copying the value
    /// exactly once (the original variable remains owned by its container).
    pub fn pop_deref_with<H: Host>(&mut self, host: &mut H) -> Result<Value, VmError> {
        match self.pop_raw()? {
            Slot::Heap(slot) => {
                let value = host.read_cell(slot)?;
                self.live += 1;
                Ok(value)
            }
            slot => self.deref_local(slot),
        }
    }

    fn deref_local(&mut self, slot: Slot) -> Result<Value, VmError> {
        match slot {
            Slot::Null => Err(VmError::NullSlot),
            Slot::Heap(_) => Err(VmError::Unsupported(0x401c)),
            Slot::Value(v) => Ok(v),
            Slot::Var(id) => {
                let v = self.vars.checked(id)?.value;
                self.live += 1;
                Ok(v)
            }
        }
    }

    /// Release one value cell (vtable slot 1: `-1` live).
    pub fn release(&mut self) {
        self.live -= 1;
    }

    /// Create a variable slot (`+1` live) and return its handle; the caller
    /// links it into a frame's locals.
    pub fn new_var(&mut self, value: Value, name: u32) -> VarId {
        self.live += 1;
        self.vars.alloc(value, name)
    }

    fn require_lvalue(&mut self) -> Result<Lvalue, VmError> {
        match self.pop_raw()? {
            Slot::Var(id) => Ok(Lvalue::Local(id)),
            Slot::Heap(slot) => Ok(Lvalue::Heap(slot)),
            Slot::Null => Err(VmError::NullSlot),
            Slot::Value(_) => Err(VmError::NotLvalue),
        }
    }

    fn read_lvalue<H: Host>(&mut self, host: &mut H, lv: Lvalue) -> Result<Value, VmError> {
        match lv {
            Lvalue::Local(id) => Ok(self.vars.checked(id)?.value),
            Lvalue::Heap(slot) => host.read_cell(slot),
        }
    }

    fn write_lvalue<H: Host>(
        &mut self,
        host: &mut H,
        lv: Lvalue,
        value: Value,
    ) -> Result<(), VmError> {
        match lv {
            Lvalue::Local(id) => {
                self.vars.checked(id)?.value = value;
                Ok(())
            }
            Lvalue::Heap(slot) => host.write_cell(slot, value),
        }
    }

    // ---- frames -------------------------------------------------------

    /// Destroy the current frame (the deleting destructor `FUN_00407d10`
    /// over `FUN_00407ba0`): value cells left on its stack are released
    /// (lvalue words are not), all locals are released and freed.
    ///
    /// exe: FUN_00407d10
    /// exe: FUN_00407ba0
    pub fn pop_frame(&mut self) -> Result<(), VmError> {
        let f = self.frames.pop().ok_or(VmError::NoFrame)?;
        for s in f.stack.iter().rev() {
            if let Slot::Value(_) = s {
                self.live -= 1;
            }
        }
        for id in f.locals {
            self.live -= 1;
            self.vars.free(id);
        }
        Ok(())
    }

    /// Enter a script method: the script path of `Thread::callMethod`
    /// (`FUN_0041fbc0`). Native methods (`flag 0x40`) and the call-record /
    /// name resolution that precede this are the host's (ticket #31).
    ///
    /// Compatibility entry for local-only hosts. The caller
    /// frame's pc is saved, a frame is created (`FUN_00407b10`) and linked,
    /// an instance method gets its `this` slot, then the arguments are
    /// popped from the caller's stack **last parameter first**, each copied
    /// (dereferenced) into a typed variable slot of the callee through the
    /// assignment store `FUN_0041ab60`, and the thread resumes at the
    /// callee's entry. The caller must treat the call as
    /// [`Flow::Stay`]. An instance method called with receiver 0 is a
    /// fatal script error ([`VmError::InstanceMethodWithoutReceiver`])
    /// raised before anything is touched, like the exe's first check.
    ///
    /// exe: FUN_0041fbc0
    pub fn call_script_method(&mut self, m: &ScriptMethod, receiver: u32) -> Result<(), VmError> {
        struct LocalHost;
        impl Host for LocalHost {
            fn body(&self, _id: BodyId) -> &[Node] {
                &[]
            }
        }
        self.call_script_method_with(&mut LocalHost, m, receiver)
    }

    /// Enter a script frame with runtime receiver typing, heap-lvalue argument
    /// reads, and loaded-class parameter assignment supplied by the host.
    /// Public execution replay covers real frame construction/return with
    /// controlled loaded metadata (`vmexec_frames` and `vmexec_calls`).
    pub fn call_script_method_with<H: Host>(
        &mut self,
        host: &mut H,
        m: &ScriptMethod,
        receiver: u32,
    ) -> Result<(), VmError> {
        // First statement of the exe function: an instance method needs a
        // receiver (fatal script error, nothing touched).
        if !m.is_static && receiver == 0 {
            return Err(VmError::InstanceMethodWithoutReceiver);
        }
        let this = if m.is_static {
            None
        } else {
            Some(host.receiver_value(receiver)?)
        };
        let pc = self.pc;
        self.frame()?.saved_pc = pc;
        let mut callee = Frame::new(
            Some(Pc {
                body: m.body,
                idx: 0,
            }),
            receiver,
        );
        callee.class_ctx = m.class_ctx;
        callee.signature = m.signature;
        if let Some(value) = this {
            let this = self.new_var(value, 0);
            callee.locals.push(this);
        }
        for ty in m.params.iter().rev() {
            let mut arg = self.pop_deref_with(host)?;
            let mut slot = Value {
                tag: *ty,
                payload: 0,
            };
            host.store_value(&mut self.alu, &mut slot, &mut arg);
            let id = self.new_var(slot, 0);
            callee.locals.push(id);
            self.release();
        }
        self.pc = callee.saved_pc;
        self.frames.push(callee);
        Ok(())
    }

    // ---- the loop -----------------------------------------------------

    /// Run the thread until it stops, blocks, or `budget` runs out
    /// (`Thread::run(float budget)`, `FUN_00420ff0`).
    ///
    /// `clock` is the exe's high-resolution clock `FUN_005516c0`; it is read
    /// once at entry, once more every 256 steps when `budget != 0` and once
    /// at exit. Returns the x87 leftover time the exe returns in `st0`:
    /// `(float)(t_start + budget) - t_end` under the thread's precision
    /// control.
    ///
    /// **What the annotation covers.** `FUN_00420ff0` is ported for the
    /// opwords the loop executes itself, verified by the original loop suite
    /// (`tests/thread_oracle.rs`, Unicorn running the exe's `Thread::run`):
    /// `0x0004 0x0005 0x0010 0x0018 0x0028 0x0029 0x002a 0x002b 0x002d
    /// 0x1001 0x1003 0x1007 0x1008 0x1014 0x1015 0x1016 0x1017 0x101f
    /// 0x1027`, the sub-ops `0..=0x1d` and `0x23..=0x2e` of `0x1007`/`0x1008`
    /// (`0x1007/0x1c` for `I`, `F` and null literals, `0x1008/0x20` and
    /// `0x1008/0x21` as statements), the fatal opwords and the silent
    /// no-ops. The separate field/execution suites cover bounded host paths.
    /// **Delegated to [`Host`] and not verified by this loop-only suite:** `0x001d`
    /// `0x0020` `0x0021..=0x0024` `0x1002` `0x1011` `0x101b` `0x101e`
    /// `0x401c` `0x4025` `0x4026`, `0x1007/0x1c` with resource Member (`0x100e`) or
    /// String (`0x4009`) literals, `0x1007/0x1e` `0x1007/0x1f`
    /// `0x1007/0x20` `0x1007/0x21` and `0x1008/0x1e` `0x1008/0x1f`.
    ///
    /// exe: FUN_00420ff0
    pub fn run<H: Host>(
        &mut self,
        host: &mut H,
        budget: f32,
        clock: &mut dyn FnMut() -> X87,
    ) -> Result<X87, VmError> {
        if self.pc.is_none() {
            return Err(VmError::EmptyThread);
        }
        if self.flags & flags::RUNNING != 0 {
            return Err(VmError::AlreadyRunning);
        }
        let pc24 = self.alu.precision;
        let timed = budget != 0.0;
        let t0 = clock();
        let deadline = t0.add(X87::from_f32(budget), pc24, RC).to_f32(pc24, RC);
        self.pc = self.frame()?.saved_pc;
        self.flags |= flags::RUNNING;
        let mut counter: u8 = 0;
        loop {
            let pc = self.pc.ok_or(VmError::PcOutOfRange)?;
            let node = *host
                .body(pc.body)
                .get(pc.idx)
                .ok_or(VmError::PcOutOfRange)?;
            self.line = node.operand;
            if STEP_TRACE.with(std::cell::Cell::get) {
                eprintln!(
                    "STEP {} idx={} op={:04x} imm={} line={}",
                    host.body_label(pc.body),
                    pc.idx,
                    node.opword,
                    node.imm,
                    node.operand
                );
            }
            let next = match self.step(host, pc, node) {
                Ok(n) => n,
                Err(e) => {
                    // Stopped by a script error: leave the running state so
                    // the caller can inspect (and not re-run) the thread.
                    self.flags &= !flags::RUNNING;
                    return Err(e);
                }
            };
            match next {
                Next::Exit => break,
                Next::Advance => {
                    if let Some(p) = self.pc.as_mut() {
                        p.idx += 1;
                    }
                }
                Next::Stay => {}
            }
            // Loop tail (`LAB_004238f1`): time slice every 256 steps, then
            // the status flags and the scheduler check.
            if timed {
                let first = counter == 0;
                counter = counter.wrapping_add(1);
                if first {
                    let now = clock();
                    let (cc, _) = X87::from_f32(deadline).fcom(now);
                    if !cc.c2 && (cc.c0 || cc.c3) {
                        break;
                    }
                }
            }
            if self.flags & (flags::LOOP_ABORT | flags::SCHED_BLOCK) != 0 {
                break;
            }
        }
        // Exit path (`LAB_00423936`).
        self.flags &= !flags::RUNNING;
        let pc = self.pc;
        if let Some(f) = self.frames.last_mut() {
            f.saved_pc = pc;
        }
        let t_end = clock();
        Ok(X87::from_f32(deadline).sub(t_end, pc24, RC))
    }

    fn stop(&mut self) -> Next {
        self.flags |= flags::STOPPED;
        Next::Exit
    }

    fn jump(&mut self, pc: Pc, delta: u32) -> Result<Next, VmError> {
        let idx =
            i64::try_from(pc.idx).map_err(|_| VmError::PcOutOfRange)? + i64::from(delta as i32);
        let idx = usize::try_from(idx).map_err(|_| VmError::PcOutOfRange)?;
        self.pc = Some(Pc { body: pc.body, idx });
        Ok(Next::Stay)
    }

    fn ext<H: Host>(
        &mut self,
        host: &mut H,
        kind: ExternKind,
        pc: Pc,
        node: Node,
        discard: bool,
    ) -> Result<Next, VmError> {
        let flow = host.extern_op(
            self,
            ExternOp {
                kind,
                pc,
                node,
                discard,
            },
        )?;
        Ok(match flow {
            Flow::Advance => Next::Advance,
            Flow::Stay => Next::Stay,
        })
    }

    fn step<H: Host>(&mut self, host: &mut H, pc: Pc, node: Node) -> Result<Next, VmError> {
        match node.opword {
            0x0000 | 0x0013 | 0x0042 | 0x0048 | 0x004a | 0x004d => {
                Err(VmError::FatalOpcode(node.opword))
            }
            0x0004 | 0x0005 => {
                // Short circuit: re-push the (dereferenced) top as a
                // boolean and skip the next node on true (`||`) / false
                // (`&&`).
                let mut v = self.pop_deref_with(host)?;
                boolean_convert(&mut v);
                self.push(Slot::Value(v))?;
                let truthy = v.payload != 0; // FUN_0041bd90
                if truthy == (node.opword == 0x0004) {
                    if let Some(p) = self.pc.as_mut() {
                        p.idx += 1;
                    }
                }
                Ok(Next::Advance)
            }
            0x0010 => Ok(self.stop()),
            0x0018 => {
                self.pop_deref_with(host)?;
                self.release();
                Ok(Next::Advance)
            }
            0x001d => self.ext(host, ExternKind::FieldByName, pc, node, false),
            0x0020 => self.ext(host, ExternKind::ArrayElement, pc, node, false),
            0x0021 => self.ext(host, ExternKind::CallInit, pc, node, false),
            0x0022 => self.ext(host, ExternKind::CallThisInit, pc, node, false),
            0x0023 => self.ext(host, ExternKind::CallSuperInit, pc, node, false),
            0x0024 => self.ext(host, ExternKind::CallResolved, pc, node, false),
            0x0028 => {
                let v = self.pop_deref_with(host)?;
                self.push(Slot::Value(v))?;
                Ok(Next::Advance)
            }
            0x0029 => {
                self.push_new(Value::int(0))?;
                Ok(Next::Advance)
            }
            0x002a => self.ret(host, false),
            0x002b => self.ret(host, true),
            0x002d => {
                let mut value = self.pop_deref_with(host)?;
                let lv = self.require_lvalue()?;
                self.assign(host, lv, &mut value, 0x23)?;
                self.release();
                Ok(Next::Advance)
            }
            0x1001 => {
                let slot = self
                    .frame()?
                    .locals
                    .get(node.imm as usize)
                    .map_or(Slot::Null, |id| Slot::Var(*id));
                self.push(slot)?;
                Ok(Next::Advance)
            }
            0x1002 => self.ext(host, ExternKind::Declare, pc, node, false),
            0x1003 => {
                if let Some(id) = self.frame()?.locals.pop() {
                    self.live -= 1;
                    self.vars.free(id);
                }
                Ok(Next::Advance)
            }
            0x1007 => self.sub_op(host, pc, node, false),
            0x1008 => self.sub_op(host, pc, node, true),
            0x1011 => self.ext(host, ExternKind::ResolveName, pc, node, false),
            0x1014 => self.jump(pc, node.imm),
            0x1015 | 0x1016 => {
                let mut c = self.pop_deref_with(host)?;
                if STEP_TRACE.with(std::cell::Cell::get) {
                    eprintln!("  COND value tag={:?} payload={:#x}", c.tag, c.payload);
                }
                boolean_convert(&mut c);
                self.release();
                if (c.payload == 0) == (node.opword == 0x1015) {
                    self.jump(pc, node.imm)
                } else {
                    Ok(Next::Advance)
                }
            }
            0x1017 => {
                // `==` on (case, switch): jump on match, else keep the
                // switch value for the next case.
                let mut case = self.pop_deref_with(host)?;
                let mut switch = self.pop_deref_with(host)?;
                self.alu.eq(&mut case, &mut switch);
                if case.payload != 0 {
                    self.release();
                    self.release();
                    self.jump(pc, node.imm)
                } else {
                    self.release();
                    self.push(Slot::Value(switch))?;
                    Ok(Next::Advance)
                }
            }
            0x101b => self.ext(host, ExternKind::FieldRef, pc, node, false),
            0x101e => self.ext(host, ExternKind::ArgArray, pc, node, false),
            0x101f => {
                for _ in 0..node.imm {
                    self.push_new(Value::int(0))?;
                }
                Ok(Next::Advance)
            }
            0x1027 => {
                self.push_new(Value::int(node.imm as i32))?;
                Ok(Next::Advance)
            }
            0x401c => self.ext(host, ExternKind::MemberAccess, pc, node, false),
            0x4025 => self.ext(host, ExternKind::CallInstance, pc, node, false),
            0x4026 => self.ext(host, ExternKind::CallStatic, pc, node, false),
            // Structural nodes and anything the dispatch has no case for:
            // a silent advance.
            _ => Ok(Next::Advance),
        }
    }

    /// `0x002a` / `0x002b`: pop the frame, resume the caller.
    fn ret<H: Host>(&mut self, host: &mut H, with_value: bool) -> Result<Next, VmError> {
        let value = if with_value {
            Some(self.pop_deref_with(host)?)
        } else {
            None
        };
        if !self.frames.is_empty() {
            self.pop_frame()?;
        }
        // FUN_0041f7b0: restore pc from the (new) current frame.
        self.pc = self.frame()?.saved_pc;
        if let Some(v) = value {
            self.push(Slot::Value(v))?;
        }
        if self.pc.is_none() {
            return Ok(self.stop());
        }
        Ok(Next::Advance)
    }

    /// Store through an lvalue: `FUN_0041acb0(var, value, kind)`. Kinds
    /// `0x27` (`+=`) and `0x28` (`-=`, the exe negates the value first) run
    /// the string-aware `+` helper, so a reference operand is offered to
    /// the host (the other kinds never reach `FUN_0041b4d0`).
    fn assign<H: Host>(
        &mut self,
        host: &mut H,
        lv: Lvalue,
        value: &mut Value,
        kind: u32,
    ) -> Result<(), VmError> {
        let mut kind = kind;
        if kind == 0x28 {
            self.alu.negate(value);
            kind = 0x27;
        }
        if kind == 0x27 {
            let cur = self.read_lvalue(host, lv)?;
            if cur.tag.is_ref() || value.tag.is_ref() {
                let mut left = cur;
                if let Some(r) = host.operator_on_refs(RefOperator::AddAssign, &mut left, value) {
                    r?;
                    self.write_lvalue(host, lv, left)?;
                    return Ok(());
                }
                if let Some(r) = host.concatenate_strings(&mut left, value) {
                    r?;
                    self.write_lvalue(host, lv, left)?;
                    return Ok(());
                }
            }
        }
        // `*=` and `/=` work on a temporary copy of the variable
        // (`FUN_00423e30`, +1 live) that is released after the store; a
        // trap in the multiplication/division leaves it live.
        let temp = matches!(kind, 0x24 | 0x25);
        if temp {
            self.live += 1;
        }
        let mut current = self.read_lvalue(host, lv)?;
        if kind == 0x23 {
            host.store_value(&mut self.alu, &mut current, value);
        } else {
            self.alu.assign(&mut current, true, value, kind)?;
        }
        self.write_lvalue(host, lv, current)?;
        if temp {
            self.live -= 1;
        }
        for v in self.alu.ref_stores.drain(..) {
            host.on_reference_store(v);
        }
        Ok(())
    }

    /// The `0x1007` (push the result) / `0x1008` (statement: drop it)
    /// sub-op dispatcher.
    fn sub_op<H: Host>(
        &mut self,
        host: &mut H,
        pc: Pc,
        node: Node,
        discard: bool,
    ) -> Result<Next, VmError> {
        let sub = node.imm;
        match sub {
            0 => {
                // Ternary select: [cond][a][b].
                let top = self.pop_deref_with(host)?;
                let second = self.pop_deref_with(host)?;
                let mut cond = self.pop_deref_with(host)?;
                boolean_convert(&mut cond);
                self.release();
                let keep = if cond.payload == 0 { top } else { second };
                self.release();
                if discard {
                    self.release();
                } else {
                    self.push(Slot::Value(keep))?;
                }
                Ok(Next::Advance)
            }
            0x1..=0xb | 0xd..=0x13 => {
                if discard {
                    for _ in 0..2 {
                        self.pop_deref_with(host)?;
                        self.release();
                    }
                    return Ok(Next::Advance);
                }
                let mut first = self.pop_deref_with(host)?; // stack top
                let mut second = self.pop_deref_with(host)?;
                match sub {
                    0xd => {
                        self.alu.shr(&mut first, &mut second);
                        self.release();
                        self.push(Slot::Value(second))?;
                        return Ok(Next::Advance);
                    }
                    0xe => {
                        self.alu.shl(&mut first, &mut second);
                        self.release();
                        self.push(Slot::Value(second))?;
                        return Ok(Next::Advance);
                    }
                    _ => {}
                }
                let (left, right) = (&mut second, &mut first);
                let mut done = false;
                if matches!(sub, 0xf | 0x10) {
                    if sub == 0xf {
                        // `-` negates its right operand, then adds.
                        self.alu.negate_for_sub(right);
                    }
                    if left.tag.is_ref() || right.tag.is_ref() {
                        if let Some(r) = host.operator_on_refs(RefOperator::Add, left, right) {
                            r?;
                            done = true;
                        } else if let Some(r) = host.concatenate_strings(left, right) {
                            r?;
                            done = true;
                        }
                    }
                    if !done {
                        self.alu.add(left, right)?;
                        done = true;
                    }
                }
                if !done {
                    self.binary(sub, left, right)?;
                }
                self.release();
                self.push(Slot::Value(second))?;
                Ok(Next::Advance)
            }
            0x14..=0x16 => {
                let mut v = self.pop_deref_with(host)?;
                if discard {
                    self.release();
                    return Ok(Next::Advance);
                }
                match sub {
                    0x14 => self.alu.bit_not(&mut v),
                    0x15 => self.alu.logical_not(&mut v),
                    _ => self.alu.negate(&mut v),
                }
                self.push(Slot::Value(v))?;
                Ok(Next::Advance)
            }
            0x17..=0x1a => {
                let lv = self.require_lvalue()?;
                let inc = matches!(sub, 0x17 | 0x19);
                let copy_first = matches!(sub, 0x17 | 0x18);
                let old = self.read_lvalue(host, lv)?;
                let mut new = old;
                if inc {
                    self.alu.increment(&mut new);
                } else {
                    self.alu.decrement(&mut new);
                }
                self.write_lvalue(host, lv, new)?;
                if !discard {
                    // 0x17/0x18 push the copy taken before the change,
                    // 0x19/0x1a the one taken after it.
                    self.push_new(if copy_first { old } else { new })?;
                }
                Ok(Next::Advance)
            }
            0x1c if !discard => self.literal(host, pc),
            0x1c => {
                // Statement-position literal: skip its node, push nothing.
                if let Some(p) = self.pc.as_mut() {
                    p.idx += 1;
                }
                Ok(Next::Advance)
            }
            0x1e => self.ext(host, ExternKind::New, pc, node, discard),
            0x1f => self.ext(host, ExternKind::NewArray, pc, node, discard),
            0x20 | 0x21 if discard => {
                if let Some(p) = self.pc.as_mut() {
                    p.idx += 1;
                }
                self.pop_deref_with(host)?;
                self.release();
                Ok(Next::Advance)
            }
            0x20 => self.ext(host, ExternKind::InstanceOf, pc, node, false),
            0x21 => self.ext(host, ExternKind::Cast, pc, node, false),
            0x23..=0x2e => {
                // [value][lvalue] -> store; 0x1007 pushes the value back.
                let lv = self.require_lvalue()?;
                let mut value = self.pop_deref_with(host)?;
                self.assign(host, lv, &mut value, sub)?;
                if discard {
                    self.release();
                } else {
                    self.push(Slot::Value(value))?;
                }
                Ok(Next::Advance)
            }
            // 0xc (`>>>`), 0x1b, 0x1d, 0x22 and everything else: no case.
            _ => {
                if discard && sub == 0x1b {
                    self.pop_deref_with(host)?;
                    self.release();
                }
                Ok(Next::Advance)
            }
        }
    }

    fn binary(&mut self, sub: u32, l: &mut Value, r: &mut Value) -> Result<(), VmError> {
        match sub {
            0x1 => self.alu.logical_or(l, r),
            0x2 => self.alu.logical_and(l, r),
            0x3 => self.alu.bit_or(l, r),
            0x4 => self.alu.bit_xor(l, r),
            0x5 => self.alu.bit_and(l, r),
            0x6 => self.alu.ne(l, r),
            0x7 => self.alu.eq(l, r),
            0x8 => self.alu.ge(l, r),
            0x9 => self.alu.le(l, r),
            0xa => self.alu.gt(l, r),
            0xb => self.alu.lt(l, r),
            0xf => self.alu.sub(l, r)?,
            0x10 => self.alu.add(l, r)?,
            0x11 => self.alu.rem(l, r)?,
            0x12 => self.alu.div(l, r)?,
            0x13 => self.alu.mul(l, r)?,
            _ => {}
        }
        Ok(())
    }

    /// `0x1007/0x1c`: push the literal in the next node.
    fn literal<H: Host>(&mut self, host: &mut H, pc: Pc) -> Result<Next, VmError> {
        let lit_pc = Pc {
            body: pc.body,
            idx: pc.idx + 1,
        };
        let lit = *host
            .body(pc.body)
            .get(lit_pc.idx)
            .ok_or(VmError::PcOutOfRange)?;
        self.pc = Some(lit_pc);
        match lit.opword {
            0x100b | 0x100d => self.push_new(Value::int(lit.imm as i32))?,
            0x000c => self.push_new(Value::reference(0))?,
            0x200a => self.push_new(Value {
                tag: TypeTag::Float,
                payload: lit.imm,
            })?,
            0x100e => return self.ext(host, ExternKind::ResourceLiteral, lit_pc, lit, false),
            0x4009 => return self.ext(host, ExternKind::StringLiteral, lit_pc, lit, false),
            _ => return Err(VmError::UnhandledLiteral),
        }
        Ok(Next::Advance)
    }
}

/// What the call protocol needs to know about a script method (the host
/// derives it from the class record; see [`Thread::call_script_method`]).
#[derive(Clone, Debug)]
pub struct ScriptMethod {
    /// The method body.
    pub body: BodyId,
    /// Class context word stored in the frame.
    pub class_ctx: u32,
    /// Signature word stored in the frame.
    pub signature: u32,
    /// Static methods get no `this` local.
    pub is_static: bool,
    /// Parameter types in declaration order (return type excluded).
    pub params: Vec<TypeTag>,
}

/// The x87 precision the port runs scripts under (ADR 0007: device thread).
pub const SCRIPT_PRECISION: Precision = Precision::F32;

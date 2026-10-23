//! Lossless chronological inputs for the native-memory projection.
//!
//! Identities here name semantic owners, never x86 addresses. The embedding
//! resolves them through original metadata/allocator producers. This stream is
//! separate from diagnostics: no ring, filtering, truncation or clock reads.
//! Events remain pending until the consumer explicitly acknowledges them.
use crate::{interp::{HeapSlot, Pc, VarId}, values::{CellId, PoolEvent, Value}};
use std::{cell::RefCell, collections::VecDeque, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operand {
    Null,
    Value(CellId),
    Variable(VarId),
    Heap(HeapSlot),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OperandAction { Push, Pop, Drop }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FramePhase { Create, Enter, Return, Destroy }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameView {
    pub identity: u64,
    pub receiver: u32,
    pub context: u32,
    pub signature: u32,
    pub saved_pc: Option<Pc>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Pool(PoolEvent),
    ThreadCreate { identity: u64 },
    /// Supplied by the actual creator, distinct from a later thread rename.
    ThreadOrigin { identity: u64, priority: u32, flags: u32, name: String },
    Frame { thread: u64, phase: FramePhase, frame: FrameView, caller: Option<u64> },
    Operand { thread: u64, frame: u64, action: OperandAction, slot: Operand },
    CellStore { identity: CellId, before: Value, after: Value },
    VariableStore { identity: VarId, before: Value, after: Value },
    HeapStore { slot: HeapSlot, before: Value, after: Value },
    /// An unpaired mutable borrow cannot prove the time/value of its store.
    /// Producers must reject this boundary until its source path emits a store.
    CellWriteAccess { identity: CellId, before: Value },
    CellAbandon { identity: CellId, value: Value },
    Run { thread: u64, enter: bool, budget_bits: u32, pc: Option<Pc>, flags: u32, error: Option<String> },
    Opcode { thread: u64, enter: bool, pc: Pc, opcode: u16, operand: u16, immediate: u32, next_pc: Option<Pc>, flags: u32, error: Option<String> },
    ObjectAllocate { word: u32, class_context: u32, array_descriptor: Option<String>, array_length: Option<usize> },
    NativeEnter { thread: u64, defining_class: u32, method_index: Option<usize>, implementation: u32, descriptor: String, receiver: u32, cells: Vec<crate::values::PoolCellView> },
    NativeReturn { thread: u64, implementation: u32, executed: bool, result: Option<u32>, error: Option<String> },
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry { pub sequence: u64, pub event: Event }
#[derive(Debug, Default)]
struct State { enabled: bool, seen_activity: bool, next_sequence: u64, next_identity: u64, pending: VecDeque<Entry> }
#[derive(Clone, Debug, Default)]
pub struct Trace(Rc<RefCell<State>>);
impl Trace {
    /// Recording begins only before any identities have been issued. Resuming
    /// a partially observed heap would reconstruct history from current values.
    pub fn start(&self) -> Result<(), &'static str> {
        let mut state=self.0.borrow_mut();
        if state.enabled || state.seen_activity || state.next_identity!=0 || !state.pending.is_empty() { return Err("machine recording must start before owner creation"); }
        state.enabled=true; Ok(())
    }
    pub fn enabled(&self)->bool { self.0.borrow().enabled }
    pub fn identity(&self)->u64 { let mut state=self.0.borrow_mut(); state.next_identity=state.next_identity.checked_add(1).expect("machine identity exhausted");state.next_identity }
    pub fn note(&self,event:Event){ let mut state=self.0.borrow_mut();state.seen_activity=true;if !state.enabled{return} let sequence=state.next_sequence;state.next_sequence=sequence.checked_add(1).expect("machine event sequence exhausted");state.pending.push_back(Entry{sequence,event}); }
    pub fn pending(&self,limit:usize)->Vec<Entry>{self.0.borrow().pending.iter().take(limit).cloned().collect()}
    pub fn acknowledge(&self,through:u64)->Result<(), &'static str>{
        let mut state=self.0.borrow_mut();
        if !state.pending.iter().any(|entry|entry.sequence==through){return Err("acknowledgement is not a pending machine event")}
        while state.pending.front().is_some_and(|entry|entry.sequence<=through){state.pending.pop_front();}Ok(())
    }
}

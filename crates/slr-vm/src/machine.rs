//! Chronological live inputs to the native memory projection.
//!
//! These are semantic owner identities, not x86 pointers. Consumers resolve
//! them through source-derived allocation/metadata code. Recording must begin
//! before owner creation. No ring, filtering, clock reads or inferred history.
use crate::{
    interp::{HeapSlot, Pc, VarId},
    values::{CellId, PoolCellView, PoolEvent, Value},
};
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    rc::Rc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Operand {
    Null,
    Value(CellId),
    Variable(VarId),
    Heap(HeapSlot),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum OperandAction {
    Push,
    Pop,
    Drop,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub enum FramePhase {
    Create,
    Enter,
    SavePc,
    Return,
    Destroy,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct FrameView {
    pub identity: u64,
    pub receiver: u32,
    pub context: u32,
    pub signature: u32,
    pub saved_pc: Option<Pc>,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub enum Event {
    Pool(PoolEvent),
    ClockInput {
        site: crate::clock::ClockSite,
        input: crate::clock::CounterInput,
        thread: Option<crate::clock::ThreadContext>,
        driver_cursor: Option<usize>,
    },
    ThreadCreate {
        identity: u64,
    },
    ThreadOrigin {
        identity: u64,
        priority: u32,
        flags: u32,
        name: String,
    },
    ThreadRetire {
        identity: u64,
        flags: u32,
    },
    Frame {
        thread: u64,
        phase: FramePhase,
        frame: FrameView,
        caller: Option<u64>,
    },
    CurrentFrame {
        thread: u64,
        frame: u64,
    },
    Local {
        thread: u64,
        frame: u64,
        append: bool,
        index: usize,
        variable: VarId,
    },
    Operand {
        thread: u64,
        frame: u64,
        action: OperandAction,
        slot: Operand,
    },
    CellStore {
        identity: CellId,
        before: Value,
        after: Value,
    },
    VariableStore {
        identity: VarId,
        before: Value,
        after: Value,
    },
    HeapStore {
        slot: HeapSlot,
        before: Value,
        after: Value,
    },
    HeapConstruct {
        slot: HeapSlot,
        value: Value,
    },
    /// Finalizer cleanup destroys the array's owned element cells and clears
    /// its container. The complete prior values identify those actual owners.
    ArrayClear {
        word: u32,
        before: Vec<Value>,
    },
    /// The producer must reject these paths until source stores are paired.
    UnresolvedMutation {
        owner: String,
    },
    CellWriteAccess {
        identity: CellId,
        before: Value,
    },
    CellAbandon {
        identity: CellId,
        value: Value,
    },
    Run {
        thread: u64,
        enter: bool,
        budget_bits: u32,
        pc: Option<Pc>,
        flags: u32,
        error: Option<String>,
    },
    Opcode {
        thread: u64,
        enter: bool,
        pc: Pc,
        opcode: u16,
        operand: u16,
        immediate: u32,
        next_pc: Option<Pc>,
        flags: u32,
        error: Option<String>,
    },
    ObjectAllocate {
        word: u32,
        class_context: u32,
        array_descriptor: Option<String>,
        array_length: Option<usize>,
    },
    FileRead {
        identity: u64,
        bytes: Vec<u8>,
    },
    ModuleRead {
        identity: u64,
        file: u64,
        version: u32,
        chunks: Vec<([u8; 4], usize, usize)>,
    },
    ClassRegister {
        identity: u32,
        module: Option<u64>,
        bodies: Vec<crate::interp::BodyId>,
    },
    /// Exact resolved dependency records before recursive initialization.
    /// The raw producer must allocate the dependency owner at this phase;
    /// waiting for ClassInstall would reorder it after its dependencies.
    ClassDependencies {
        identity: u32,
        base: Option<u32>,
        interfaces: Vec<u32>,
    },
    /// The class has installed its static owner after resolving and
    /// initializing these exact dependency records. Empty owners need this
    /// event too: no HeapConstruct exists from which to recover installation.
    ClassInstall {
        identity: u32,
        base: Option<u32>,
        interfaces: Vec<u32>,
    },
    MethodSelect {
        thread: u64,
        frame: Option<u64>,
        pc: Option<Pc>,
        defining_class: u32,
        method_index: usize,
        receiver: u32,
    },
    NativeEnter {
        thread: u64,
        frame: Option<u64>,
        pc: Option<Pc>,
        defining_class: u32,
        method_index: Option<usize>,
        implementation: u32,
        descriptor: String,
        receiver: u32,
        cells: Vec<PoolCellView>,
    },
    NativeReturn {
        thread: u64,
        implementation: u32,
        executed: bool,
        result: Option<u32>,
        error: Option<String>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Entry {
    pub sequence: u64,
    pub event: Event,
}
#[derive(Debug, Default)]
struct State {
    enabled: bool,
    seen_activity: bool,
    next_sequence: u64,
    next_identity: u64,
    pending: VecDeque<Entry>,
    primary_next: u64,
    next_reader: u64,
    readers: BTreeMap<u64, u64>,
}
#[derive(Clone, Debug, Default)]
pub struct Trace(Rc<RefCell<State>>);
impl Trace {
    pub fn start(&self) -> Result<(), &'static str> {
        let mut state = self.0.borrow_mut();
        if state.enabled
            || state.seen_activity
            || state.next_identity != 0
            || !state.pending.is_empty()
        {
            return Err("machine recording must start before owner creation");
        }
        state.enabled = true;
        Ok(())
    }
    pub fn enabled(&self) -> bool {
        self.0.borrow().enabled
    }
    pub fn same_owner(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn identity(&self) -> u64 {
        let mut state = self.0.borrow_mut();
        state.next_identity = state
            .next_identity
            .checked_add(1)
            .expect("machine identity exhausted");
        state.next_identity
    }
    pub fn note(&self, event: Event) {
        let mut state = self.0.borrow_mut();
        state.seen_activity = true;
        if !state.enabled {
            return;
        }
        let sequence = state.next_sequence;
        state.next_sequence = sequence
            .checked_add(1)
            .expect("machine event sequence exhausted");
        state.pending.push_back(Entry { sequence, event });
    }
    /// Attach a second lossless consumer before the first event/owner. A
    /// recorder and a source producer acknowledge independently; a fast sink
    /// cannot discard the other sink's unprocessed history. Dropping a reader
    /// retains its unacknowledged prefix until the complete trace is dropped.
    pub fn reader(&self) -> Result<Reader, &'static str> {
        let mut state = self.0.borrow_mut();
        if !state.enabled || state.next_sequence != 0 || state.next_identity != 0 {
            return Err("machine reader must attach before owner creation");
        }
        let identity = state
            .next_reader
            .checked_add(1)
            .ok_or("machine reader identity exhausted")?;
        state.next_reader = identity;
        state.readers.insert(identity, 0);
        Ok(Reader {
            trace: self.clone(),
            identity,
        })
    }
    pub fn pending(&self, limit: usize) -> Vec<Entry> {
        let state = self.0.borrow();
        pending(&state, state.primary_next, limit)
    }
    pub fn acknowledge(&self, through: u64) -> Result<(), &'static str> {
        let mut state = self.0.borrow_mut();
        validate_acknowledgement(&state, state.primary_next, through)?;
        state.primary_next = through + 1;
        compact(&mut state);
        Ok(())
    }
}
/// One independent consumer of a run's causal stream. It cannot attach late,
/// rewind, acknowledge an invented entry or discard another reader's prefix.
#[derive(Debug)]
pub struct Reader {
    trace: Trace,
    identity: u64,
}
impl Reader {
    pub fn pending(&self, limit: usize) -> Vec<Entry> {
        let state = self.trace.0.borrow();
        pending(&state, state.readers[&self.identity], limit)
    }
    pub fn acknowledge(&self, through: u64) -> Result<(), &'static str> {
        let mut state = self.trace.0.borrow_mut();
        validate_acknowledgement(&state, state.readers[&self.identity], through)?;
        state.readers.insert(self.identity, through + 1);
        compact(&mut state);
        Ok(())
    }
}
fn pending(state: &State, next: u64, limit: usize) -> Vec<Entry> {
    state
        .pending
        .iter()
        .skip_while(|entry| entry.sequence < next)
        .take(limit)
        .cloned()
        .collect()
}
fn validate_acknowledgement(state: &State, next: u64, through: u64) -> Result<(), &'static str> {
    if through < next || !state.pending.iter().any(|entry| entry.sequence == through) {
        return Err("acknowledgement is not a pending machine event");
    }
    Ok(())
}
fn compact(state: &mut State) {
    let before = state
        .readers
        .values()
        .copied()
        .fold(state.primary_next, u64::min);
    while state
        .pending
        .front()
        .is_some_and(|entry| entry.sequence < before)
    {
        state.pending.pop_front();
    }
}

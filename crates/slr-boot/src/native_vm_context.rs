//! Explicit source VM collaborators and observations of issued owners.
//! Observations carry actual source records; they never supply machine results.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
/// Immutable value words read at an actual source operation. Descriptor bytes
/// retain the canonical pointer's contents without reconstructing a value.
#[derive(Clone, Debug)]
pub struct ValueSnapshot {
    pub descriptor: u32,
    pub name: Vec<u8>,
    pub payload: u32,
}
impl ValueSnapshot {
    pub fn read(m: &Space, address: u32) -> Result<Self, String> {
        if address == 0 || !matches!(m.r32(address), 0x5e7344 | 0x5e734c) {
            return Err("NativeVmLive: source value snapshot type differs".into());
        }
        let descriptor = m.r32(address + 4);
        if descriptor == 0 {
            return Err("NativeVmLive: source value snapshot descriptor absent".into());
        }
        let mut name = Vec::new();
        for index in 0..0x10000 {
            let byte = m.r8(descriptor + index);
            if byte == 0 {
                return Ok(Self {
                    descriptor,
                    name,
                    payload: m.r32(address + 8),
                });
            }
            name.push(byte);
        }
        Err("NativeVmLive: source value snapshot descriptor unterminated".into())
    }
}
#[derive(Clone, Debug)]
pub enum Observation {
    /// A method parameter transfer: the caller's operand cell converts into the
    /// callee's freshly appended typed local.
    ParameterAssignment {
        thread: u32,
        frame: u32,
        source: u32,
        destination: u32,
        source_before: ValueSnapshot,
        source_after: ValueSnapshot,
        destination_before: ValueSnapshot,
        destination_after: ValueSnapshot,
    },
    FrameRetiring {
        address: u32,
    },
    LocalRetiring {
        thread: u32,
        frame: u32,
        index: usize,
        variable: u32,
    },
    OperandDrop {
        thread: u32,
        frame: u32,
        index: usize,
        cell: u32,
    },
    NativeEnter {
        thread: u32,
        class: u32,
        method: u32,
        signature: u32,
        receiver: u32,
        implementation: u32,
        info: u32,
    },
    NativeReturn {
        thread: u32,
        implementation: u32,
        result: Option<u32>,
    },
    CurrentFrame {
        thread: u32,
        frame: u32,
    },
    VariableAllocated {
        address: u32,
        pool: u32,
        freelist: bool,
    },
    VariableConstructed {
        address: u32,
    },
    VariableReleasing {
        address: u32,
    },
    VariableReleased {
        address: u32,
    },
    Local {
        thread: u32,
        frame: u32,
        append: bool,
        index: usize,
        variable: u32,
    },
    VariableStore {
        address: u32,
        before: ValueSnapshot,
        after: ValueSnapshot,
    },
    MethodSelect {
        thread: u32,
        class: u32,
        method: u32,
        receiver: u32,
    },
    MethodFrame {
        address: u32,
        caller: u32,
        method: u32,
        signature: u32,
        enter: bool,
    },
    ArraySupportBegin {
        thread: u32,
        class: u32,
    },
    ArraySupportEnd {
        thread: u32,
        class: u32,
        object: u32,
    },
    OperandPop {
        thread: u32,
        frame: u32,
        index: usize,
        cell: u32,
    },
    InitializerAssignment {
        thread: u32,
        frame: u32,
        source: u32,
        destination: u32,
        source_before: ValueSnapshot,
        destination_before: ValueSnapshot,
    },
    SavedPc {
        thread: u32,
        frame: u32,
        pc: u32,
    },
    OperandPush {
        thread: u32,
        frame: u32,
        index: usize,
        cell: u32,
    },
    Run {
        thread: u32,
        enter: bool,
        budget_bits: u32,
        pc: u32,
        flags: u32,
    },
    Opcode {
        thread: u32,
        enter: bool,
        pc: u32,
        opcode: u16,
        operand: u16,
        immediate: u32,
        next_pc: Option<u32>,
        flags: u32,
    },
    StaticFieldFrame {
        address: u32,
        class: u32,
        index: usize,
        variable: u32,
        record: u32,
    },
    StaticField {
        class: u32,
        index: usize,
        variable: u32,
        record: u32,
    },
    ObjectBegin {
        address: u32,
        class: u32,
        length: u32,
    },
    InstanceField {
        object: u32,
        index: usize,
        variable: u32,
        declaring: u32,
        declared_index: usize,
        record: u32,
    },
    ObjectCommitted {
        address: u32,
    },
    ThreadBegin {
        address: u32,
        input: crate::native_vm_metadata::ThreadInit,
    },
    FrameConstructed {
        address: u32,
        input: crate::native_vm_metadata::FrameInit,
    },
    ThreadCommitted {
        address: u32,
    },
    ThreadRetiring {
        address: u32,
        flags: u32,
    },
    FrameReleased {
        address: u32,
    },
    ThreadDestroyed {
        address: u32,
    },
    Dependencies {
        class: u32,
        base: u32,
        interfaces: Vec<u32>,
    },
    StaticOwner {
        class: u32,
        owner: u32,
    },
}
pub trait Environment: Allocator {
    fn class_lookup(&mut self, m: &mut Space, jvm: u32, name: u32) -> Result<u32, String> {
        crate::native_vm_lookup::get_class(m, jvm, name)
    }
    fn observe_owner(&mut self, _m: &Space, _observation: Observation) -> Result<(), String> {
        Ok(())
    }
}

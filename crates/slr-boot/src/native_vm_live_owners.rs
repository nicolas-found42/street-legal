//! Causal owner receipts issued by source execution. Historical observations
//! bind semantic identities but never reconstruct memory or grant ABI entry.
use super::Metadata;
use crate::native_vm_context::{Observation, ValueSnapshot};
use crate::native_vm_metadata::{FrameInit, ThreadInit};
use slr_physics::space::Space;
use slr_vm::machine::{Entry, Event, FramePhase, FrameView};
use std::collections::{HashMap, VecDeque};
#[path = "native_vm_live_objects.rs"]
mod objects;
#[path = "native_vm_live_statics.rs"]
mod statics;
#[path = "native_vm_live_values.rs"]
mod values;
#[path = "native_vm_live_variables.rs"]
mod variables;
pub use values::ValueHistory;
struct Class {
    raw: u32,
    base: u32,
    interfaces: Vec<u32>,
    owner: Option<u32>,
}
#[derive(Clone, Copy)]
struct Frame {
    token: u64,
    address: u32,
    thread: u64,
    input: FrameInit,
    released: bool,
    saved_pc: u32,
}
#[derive(Clone)]
struct Thread {
    token: u64,
    address: u32,
    input: ThreadInit,
    name: Vec<u8>,
    frame: Option<Frame>,
    committed: bool,
    retiring: Option<u32>,
    destroyed: bool,
}
struct ArrayScope {
    start: usize,
    thread: u64,
    frame: Frame,
    pc: u32,
    class: u32,
    descriptor: Vec<u8>,
}
struct Native {
    thread: u64,
    frame: Frame,
    pc: u32,
    class: u32,
    method: u32,
    descriptor: Vec<u8>,
    receiver: u32,
    implementation: u32,
    info: u32,
    cells: Vec<(u32, u64, ValueSnapshot)>,
    reference_result: bool,
}
struct BoundNative {
    thread: u64,
    source: Native,
}
enum Receipt {
    FrameReturn {
        frame: Frame,
        caller: Frame,
    },
    NativeEnter(Native),
    NativeReturn {
        thread: u64,
        implementation: u32,
        result: Option<u32>,
    },
    ArraySupport {
        thread: u64,
        frame: Frame,
        pc: u32,
        class: u32,
        descriptor: Vec<u8>,
        source: Vec<Receipt>,
    },
    SavedPc {
        thread: u64,
        frame: Frame,
        pc: u32,
    },
    Value(values::Receipt),
    Variable(variables::Receipt),
    Local {
        thread: u64,
        frame: u64,
        append: bool,
        index: usize,
        variable: u32,
        token: u64,
    },
    Current {
        thread: u64,
        frame: Frame,
    },
    VariableStore {
        token: u64,
        before: ValueSnapshot,
        after: ValueSnapshot,
    },
    MethodSelect {
        thread: u64,
        frame: Frame,
        pc: u32,
        class: u32,
        method: u32,
        receiver: u32,
    },
    MethodFrame {
        frame: Frame,
        caller: Frame,
        method: u32,
        signature: u32,
        enter: bool,
    },
    Operand {
        thread: u64,
        frame: u64,
        index: usize,
        cell: u32,
        value_token: Option<u64>,
        action: slr_vm::machine::OperandAction,
    },
    Assignment {
        thread: u64,
        frame: u64,
        source: u64,
        destination: u32,
        source_before: ValueSnapshot,
        source_after: ValueSnapshot,
        destination_before: ValueSnapshot,
        destination_after: ValueSnapshot,
        stage: u8,
    },
    /// Stage 0 awaits the source cell conversion, stage 1 the destination
    /// local store; both must match the source snapshots.
    Parameter {
        thread: u64,
        frame: u64,
        source: u64,
        destination: u64,
        source_before: ValueSnapshot,
        source_after: ValueSnapshot,
        destination_before: ValueSnapshot,
        destination_after: ValueSnapshot,
        stage: u8,
    },
    ScriptRetire {
        frame: Frame,
        caller: Frame,
        stage: u8,
    },
    Run {
        thread: u64,
        frame: Option<u64>,
        enter: bool,
        budget: u32,
        pc: u32,
        flags: u32,
    },
    Opcode {
        thread: u64,
        frame: u64,
        enter: bool,
        pc: u32,
        opcode: u16,
        operand: u16,
        immediate: u32,
        next_pc: Option<u32>,
        flags: u32,
    },
    Object(objects::Receipt),
    Static(statics::Receipt),
    ScriptFrame {
        frame: Frame,
        stage: u8,
        identity: Option<u64>,
    },
    Class(Observation),
    Thread {
        thread: Thread,
        stage: u8,
        identity: Option<u64>,
    },
    Retire {
        thread: Thread,
        stage: u8,
        identity: Option<u64>,
    },
}
struct BoundThread {
    token: u64,
    address: u32,
    frame: Option<u64>,
    current: Option<u64>,
    running: bool,
    opcode: Option<(u32, u16, u16, u32)>,
    retired: bool,
}
struct BoundFrame {
    address: u32,
    token: u64,
    thread: u64,
    phase: FramePhase,
    depth: usize,
    locals: Vec<Option<slr_vm::interp::VarId>>,
    caller: Option<u64>,
}
/// Historical issued bindings, never an executable callback capability.
#[derive(Clone, Debug, serde::Serialize)]
pub struct OwnerHistory {
    pub thread: u64,
    pub frame: u64,
    pub raw_thread: u32,
    pub raw_frame: u32,
    pub thread_generation: u64,
    pub frame_generation: u64,
    pub retired: bool,
}
/// Physical script frames issued by407b10, including pool reuse. These
/// History never grants an executable native entry capability.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ScriptFrameHistory {
    pub generation: u64,
    pub address: u32,
    pub thread_generation: u64,
    pub thread: u32,
    pub context: u32,
    pub pc: u32,
    pub receiver: u32,
    pub locals: u32,
    pub static_field: Option<(usize, u32, u32)>,
    pub signature: Option<u32>,
    pub method: Option<u32>,
    pub released: bool,
}
#[derive(Default)]
pub(super) struct Owners {
    issued: VecDeque<Receipt>,
    array_scope: Option<ArrayScope>,
    classes: HashMap<u32, Class>,
    objects: objects::Objects,
    statics: statics::Statics,
    values: values::Values,
    variables: variables::Variables,
    next: u64,
    physical_threads: HashMap<u32, Thread>,
    physical_frames: HashMap<u32, Frame>,
    script_frames: Vec<ScriptFrameHistory>,
    threads: HashMap<u64, BoundThread>,
    frames: HashMap<u64, BoundFrame>,
    natives: Vec<BoundNative>,
}
fn bytes(m: &Space, pointer: u32) -> Result<Vec<u8>, String> {
    if pointer == 0 {
        return Err("NativeVmLive: source owner name is absent".into());
    }
    let mut bytes = Vec::new();
    for i in 0..0x10000 {
        let byte = m.r8(pointer + i);
        if byte == 0 {
            return Ok(bytes);
        }
        bytes.push(byte);
    }
    Err("NativeVmLive: source owner name is unterminated".into())
}
fn empty_support_thread(created: &Thread, retired: &Thread, name: &[u8]) -> bool {
    let Some(frame) = created.frame else {
        return false;
    };
    let Some(released) = retired.frame else {
        return false;
    };
    created.token == retired.token
        && created.address == retired.address
        && created.committed
        && !created.destroyed
        && created.retiring.is_none()
        && retired.committed
        && retired.destroyed
        && retired.retiring == Some(0x81)
        && created.name == name
        && retired.name == name
        && created.input.priority == 10
        && created.input.flags == 1
        && frame.token == released.token
        && frame.thread == created.token
        && released.thread == retired.token
        && frame.input.context == 0
        && frame.input.pc == 0
        && frame.input.receiver == 0
        && frame.input.locals == 0
        && frame.input.thread == created.address
        && released.input.thread == retired.address
        && released.input.context == 0
        && released.input.pc == 0
        && released.input.receiver == 0
        && released.input.locals == 0
        && created.input.jvm == retired.input.jvm
        && created.input.name == retired.input.name
        && retired.input.priority == 10
        && retired.input.flags == 1
}
fn array_support_object<'a>(
    source: &[&'a Receipt],
    class: u32,
) -> Result<&'a objects::Receipt, String> {
    let rest = match source {
        [Receipt::Class(Observation::Dependencies {
            class: declared,
            interfaces,
            ..
        }), Receipt::Class(Observation::StaticOwner {
            class: installed,
            owner,
        }), Receipt::Thread {
            thread: created,
            stage: 0,
            identity: None,
        }, Receipt::Retire {
            thread: retired,
            stage: 0,
            identity: None,
        }, rest @ ..]
            if *declared == class
                && *installed == class
                && *owner != 0
                && interfaces.is_empty()
                && empty_support_thread(created, retired, b"THRD-CLASSVAR-INI") =>
        {
            rest
        }
        _ => source,
    };
    match rest {
        [Receipt::Object(object @ objects::Receipt::Object { class: actual, .. }), Receipt::Thread {
            thread: created,
            stage: 0,
            identity: None,
        }, Receipt::Retire {
            thread: retired,
            stage: 0,
            identity: None,
        }] if *actual == class && empty_support_thread(created, retired, b"THRD-INSTVAR-INI") => {
            Ok(object)
        }
        _ => Err(
            "NativeVmLive: array support contains unrelated or incomplete source lifecycle".into(),
        ),
    }
}
fn snapshot_matches(
    snapshot: &ValueSnapshot,
    value: slr_vm::values::Value,
    mut object: impl FnMut(u32) -> Result<u32, String>,
) -> Result<bool, String> {
    let payload =
        if matches!(value.tag.name().as_bytes().first(), Some(b'L' | b'[')) && value.payload != 0 {
            object(value.payload)?
        } else {
            value.payload
        };
    Ok(value.tag.name().as_bytes() == snapshot.name && payload == snapshot.payload)
}
fn initial_frame(view: &FrameView, caller: Option<u64>) -> bool {
    // The initial frame has no signature role. Its raw+38 word is unwritten
    // by407b10 and is deliberately neither read nor cleared here.
    view.identity != 0
        && view.receiver == 0
        && view.context == 0
        && view.signature == 0
        && view.saved_pc.is_none()
        && caller.is_none()
}
impl Owners {
    /// The source entry is executable only after the corresponding semantic
    /// NativeEnter has acknowledged these exact argument generations. Entry
    /// validation alone proves its raw CALL, not the live semantic ownership.
    // Pending root (#110): the live callback is not installed in production yet.
    #[allow(dead_code)]
    pub fn validate_native_entry(
        &self,
        m: &Space,
        entry: &crate::native_vm_abi::Entry,
    ) -> Result<(), String> {
        entry.validate(m)?;
        let active = self.natives.last().ok_or(
            "NativeVmLive: produced native CALL has no acknowledged semantic argument owner",
        )?;
        let owner = self
            .threads
            .get(&active.thread)
            .ok_or("NativeVmLive: native argument thread binding absent")?;
        let frame = owner
            .current
            .and_then(|frame| self.frames.get(&frame))
            .ok_or("NativeVmLive: native argument frame binding absent")?;
        let source = &active.source;
        if owner.retired
            || owner.token != source.thread
            || frame.token != source.frame.token
            || frame.phase != FramePhase::Enter
            || entry.info() != source.info
            || entry.target() != source.implementation
            || m.r32(source.info + 4) != owner.address
            || m.r32(owner.address + 0x20) != frame.address
        {
            return Err(
                "NativeVmLive: produced native entry differs from current semantic owner".into(),
            );
        }
        let vector = m.r32(source.info + 8);
        if m.r32(vector + 8) as usize != source.cells.len() {
            return Err("NativeVmLive: native argument owner length changed".into());
        }
        for (index, (address, token, value)) in source.cells.iter().enumerate() {
            let live = ValueSnapshot::read(m, *address)?;
            if self.values.token(*address)? != *token
                || m.r32(m.r32(vector + 4) + index as u32 * 4) != *address
                || live.descriptor != value.descriptor
                || live.name != value.name
                || live.payload != value.payload
            {
                return Err("NativeVmLive: native argument generation/payload changed after acknowledgement".into());
            }
        }
        Ok(())
    }
    pub fn value_history(&self) -> Vec<ValueHistory> {
        self.values.history()
    }
    pub fn observe_value(
        &mut self,
        m: &Space,
        _jvm: u32,
        observation: crate::native_vm_values::Observation,
    ) -> Result<(), String> {
        if let Some(receipt) = self.values.observe(m, observation)? {
            self.issued.push_back(Receipt::Value(receipt));
        }
        Ok(())
    }
    pub fn value(&self, m: &Space, id: slr_vm::values::CellId) -> Result<u32, String> {
        self.values.live(m, id)
    }
    pub fn static_field(
        &self,
        m: &Space,
        context: u32,
        index: usize,
        metadata: &Metadata,
    ) -> Result<u32, String> {
        self.statics.field(m, context, index, metadata)
    }
    pub fn frame(&self, m: &Space, identity: u64) -> Result<u32, String> {
        let binding = self
            .frames
            .get(&identity)
            .ok_or("NativeVmLive: frame has no issued binding")?;
        let physical = self
            .physical_frames
            .get(&binding.address)
            .ok_or("NativeVmLive: physical frame generation absent")?;
        let thread = self
            .threads
            .get(&binding.thread)
            .ok_or("NativeVmLive: frame thread binding absent")?;
        if physical.released
            || physical.token != binding.token
            || thread.retired
            || thread.current != Some(identity)
            || binding.phase != FramePhase::Enter
            || m.r32(binding.address) != 0x5e7410
            || m.r32(thread.address + 0x20) != binding.address
        {
            return Err("NativeVmLive: historical frame is not a live source context".into());
        }
        Ok(binding.address)
    }
    pub fn object(&self, m: &Space, word: u32) -> Result<u32, String> {
        self.objects.object(m, word)
    }
    // Pending root (#110): the live callback is not installed in production yet.
    #[allow(dead_code)]
    pub fn semantic_object(&self, m: &Space, address: u32) -> Result<u32, String> {
        self.objects.semantic(m, address)
    }
    pub fn field(
        &self,
        m: &Space,
        word: u32,
        index: usize,
        metadata: &Metadata,
    ) -> Result<u32, String> {
        self.objects.field(m, word, index, metadata)
    }
    pub fn script_frame_history(&self) -> &[ScriptFrameHistory] {
        &self.script_frames
    }
    pub fn history(&self) -> Vec<OwnerHistory> {
        let mut history = self
            .threads
            .iter()
            .filter_map(|(identity, thread)| {
                let frame_identity = thread.frame?;
                let frame = self.frames.get(&frame_identity)?;
                Some(OwnerHistory {
                    thread: *identity,
                    frame: frame_identity,
                    raw_thread: thread.address,
                    raw_frame: frame.address,
                    thread_generation: thread.token,
                    frame_generation: frame.token,
                    retired: thread.retired && frame.phase == FramePhase::Destroy,
                })
            })
            .collect::<Vec<_>>();
        history.sort_by_key(|owner| owner.thread);
        history
    }
    fn token(&mut self) -> Result<u64, String> {
        self.next = self
            .next
            .checked_add(1)
            .ok_or("NativeVmLive: source owner generation exhausted")?;
        Ok(self.next)
    }
    pub fn observe(&mut self, m: &Space, jvm: u32, observation: Observation) -> Result<(), String> {
        match observation {
            Observation::FrameRetiring { address } => {
                let frame = *self
                    .physical_frames
                    .get(&address)
                    .ok_or("NativeVmLive: retiring frame source generation absent")?;
                if frame.input.context != 0 {
                    let caller = *self
                        .physical_frames
                        .get(&m.r32(address + 4))
                        .ok_or("NativeVmLive: retiring frame source caller absent")?;
                    if frame.released
                        || caller.released
                        || caller.thread != frame.thread
                        || m.r32(frame.input.thread + 0x20) != address
                    {
                        return Err("NativeVmLive: retiring frame source owner differs".into());
                    }
                    self.issued
                        .push_back(Receipt::FrameReturn { frame, caller });
                }
            }
            Observation::LocalRetiring {
                thread,
                frame,
                index,
                variable,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: retiring local source thread absent")?;
                let activation = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: retiring local source frame absent")?;
                let vector = m.r32(frame + 0x18);
                if owner.destroyed
                    || activation.released
                    || owner.token != activation.thread
                    || index >= m.r32(vector + 8) as usize
                    || m.r32(m.r32(vector + 4) + index as u32 * 4) != variable
                {
                    return Err("NativeVmLive: retiring local source owner differs".into());
                }
                self.issued.push_back(Receipt::Local {
                    thread: owner.token,
                    frame: activation.token,
                    append: false,
                    index,
                    variable,
                    token: self.variables.token(variable)?,
                });
            }
            Observation::OperandDrop {
                thread,
                frame,
                index,
                cell,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: dropped operand source thread absent")?;
                let activation = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: dropped operand source frame absent")?;
                let vector = m.r32(frame + 0x28);
                if owner.destroyed
                    || activation.released
                    || owner.token != activation.thread
                    || index >= m.r32(vector + 8) as usize
                    || m.r32(m.r32(vector + 4) + index as u32 * 4) != cell
                {
                    return Err("NativeVmLive: dropped operand source owner differs".into());
                }
                self.issued.push_back(Receipt::Operand {
                    thread: owner.token,
                    frame: activation.token,
                    index,
                    cell,
                    action: slr_vm::machine::OperandAction::Drop,
                    value_token: if m.r32(cell) == 0x5e734c {
                        Some(self.values.token(cell)?)
                    } else {
                        None
                    },
                });
            }

            Observation::NativeEnter {
                thread,
                class,
                method,
                signature,
                receiver,
                implementation,
                info,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: native source thread absent")?;
                let frame = *self
                    .physical_frames
                    .get(&m.r32(thread + 0x20))
                    .ok_or("NativeVmLive: native source frame absent")?;
                let vector = m.r32(info + 8);
                let record = m.r32(info + 12);
                let descriptor = bytes(m, m.r32(signature + 4))?;
                if owner.destroyed
                    || !owner.committed
                    || frame.released
                    || frame.thread != owner.token
                    || m.r32(info) != jvm
                    || m.r32(info + 4) != thread
                    || m.r32(class + 12) != jvm
                    || m.r32(method + 0x18) != signature
                    || m.r32(record + 8) != implementation
                    || m.r32(m.r32(record + 0x14) + 4) != m.r32(signature + 4)
                    || !crate::native_stack::has_pending_call(m)
                    || crate::native_stack::argument(m, 0) != info
                    || m.r32(vector + 8) == 0
                    || m.r32(vector + 8) > 1024
                {
                    return Err("NativeVmLive: native CALL differs from actual source method/argument owner".into());
                }
                let mut cells = Vec::new();
                for index in 0..m.r32(vector + 8) {
                    let cell = m.r32(m.r32(vector + 4) + index * 4);
                    cells.push((
                        cell,
                        self.values.token(cell)?,
                        ValueSnapshot::read(m, cell)?,
                    ));
                }
                if cells.last().unwrap().2.payload != receiver
                    || cells.last().unwrap().2.name != bytes(m, m.r32(m.r32(class + 8)))?
                {
                    return Err(
                        "NativeVmLive: native receiver argument differs from source owner".into(),
                    );
                }
                let result = descriptor
                    .iter()
                    .position(|byte| *byte == b')')
                    .and_then(|index| descriptor.get(index + 1));
                self.issued.push_back(Receipt::NativeEnter(Native {
                    thread: owner.token,
                    frame,
                    pc: m.r32(thread + 0x24),
                    class,
                    method,
                    descriptor: descriptor.clone(),
                    receiver,
                    implementation,
                    info,
                    cells,
                    reference_result: matches!(result, Some(b'L' | b'[')),
                }));
            }
            Observation::NativeReturn {
                thread,
                implementation,
                result,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: returned native source thread absent")?;
                if owner.destroyed || !owner.committed || crate::native_stack::has_pending_call(m) {
                    return Err("NativeVmLive: native return has no consumed source CALL".into());
                }
                self.issued.push_back(Receipt::NativeReturn {
                    thread: owner.token,
                    implementation,
                    result,
                });
            }
            observation @ (Observation::VariableAllocated { .. }
            | Observation::VariableConstructed { .. }
            | Observation::VariableReleasing { .. }
            | Observation::VariableReleased { .. }) => {
                if let Some(receipt) = self.variables.observe(m, observation)? {
                    self.issued.push_back(Receipt::Variable(receipt));
                }
            }
            Observation::CurrentFrame { thread, frame } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: current frame source thread absent")?;
                let activation = *self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: selected frame source generation absent")?;
                if owner.destroyed
                    || !owner.committed
                    || activation.released
                    || activation.thread != owner.token
                    || m.r32(thread + 0x20) != frame
                {
                    return Err("NativeVmLive: current frame source owner differs".into());
                }
                self.issued.push_back(Receipt::Current {
                    thread: owner.token,
                    frame: activation,
                });
            }
            Observation::Local {
                thread,
                frame,
                append,
                index,
                variable,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: local source thread absent")?;
                let activation = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: local source frame absent")?;
                let vector = m.r32(frame + 0x18);
                let count = m.r32(vector + 8) as usize;
                if owner.destroyed
                    || !owner.committed
                    || activation.released
                    || activation.thread != owner.token
                    || m.r32(thread + 0x20) != frame
                    || if append {
                        count != index + 1
                            || m.r32(m.r32(vector + 4) + index as u32 * 4) != variable
                    } else {
                        count != index || m.r32(m.r32(vector + 4) + index as u32 * 4) != 0
                    }
                {
                    return Err("NativeVmLive: local source append/removal differs".into());
                }
                self.issued.push_back(Receipt::Local {
                    thread: owner.token,
                    frame: activation.token,
                    append,
                    index,
                    variable,
                    token: self.variables.token(variable)?,
                });
            }
            Observation::VariableStore {
                address,
                before,
                after,
            } => {
                if ValueSnapshot::read(m, address)?.descriptor != after.descriptor
                    || m.r32(address + 8) != after.payload
                    || before.descriptor != after.descriptor
                    || before.name != after.name
                {
                    return Err("NativeVmLive: local source store differs".into());
                }
                self.issued.push_back(Receipt::VariableStore {
                    token: self.variables.token(address)?,
                    before,
                    after,
                });
            }
            Observation::MethodSelect {
                thread,
                class,
                method,
                receiver,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: method selection source thread absent")?;
                let frame = *self
                    .physical_frames
                    .get(&m.r32(thread + 0x20))
                    .ok_or("NativeVmLive: method selection source frame absent")?;
                if owner.destroyed
                    || !owner.committed
                    || frame.released
                    || frame.thread != owner.token
                    || m.r32(class + 12) != jvm
                    || m.r32(method) != 0x5f08d0
                {
                    return Err("NativeVmLive: method selection source ownership differs".into());
                }
                self.issued.push_back(Receipt::MethodSelect {
                    thread: owner.token,
                    frame,
                    pc: m.r32(thread + 0x24),
                    class,
                    method,
                    receiver,
                });
            }
            Observation::MethodFrame {
                address,
                caller,
                method,
                signature,
                enter,
            } => {
                let frame = *self
                    .physical_frames
                    .get(&address)
                    .ok_or("NativeVmLive: method frame source construction absent")?;
                let caller = *self
                    .physical_frames
                    .get(&caller)
                    .ok_or("NativeVmLive: method frame source caller absent")?;
                let bodies = m.r32(frame.input.context + 0x18);
                let index = m.r32(method + 0x10);
                if frame.released
                    || caller.released
                    || frame.thread != caller.thread
                    || m.r32(method) != 0x5f08d0
                    || signature != m.r32(method + 0x18)
                    || m.r32(address + 0x38) != signature
                    || index >= m.r32(bodies)
                    || m.r32(m.r32(bodies + 4) + index * 4) != frame.input.pc
                    || frame.input.locals != m.r32(method + 0x14)
                    || (enter
                        && (m.r32(frame.input.thread + 0x20) != address
                            || m.r32(frame.input.thread + 0x24) != frame.input.pc))
                {
                    return Err(
                        "NativeVmLive: activated method frame differs from source linkage".into(),
                    );
                }
                let history = self
                    .script_frames
                    .iter_mut()
                    .find(|history| history.generation == frame.token)
                    .ok_or("NativeVmLive: method frame history absent")?;
                if (!enter && history.signature.is_some())
                    || (enter
                        && (history.signature != Some(signature) || history.method != Some(method)))
                {
                    return Err("NativeVmLive: method frame activation phase differs".into());
                }
                if !enter {
                    history.signature = Some(signature);
                    history.method = Some(method);
                }
                self.issued.push_back(Receipt::MethodFrame {
                    frame,
                    caller,
                    method,
                    signature,
                    enter,
                });
            }
            Observation::ArraySupportBegin { thread, class } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: array caller absent")?;
                let frame = *self
                    .physical_frames
                    .get(&m.r32(thread + 0x20))
                    .ok_or("NativeVmLive: array caller frame absent")?;
                let descriptor = bytes(m, m.r32(m.r32(class + 8)))?;
                if self.array_scope.is_some()
                    || owner.destroyed
                    || !owner.committed
                    || frame.released
                    || frame.thread != owner.token
                    || descriptor.first() != Some(&b'[')
                    || m.r32(class) != 0x5e7340
                    || m.r32(class + 12) != jvm
                    || m.r32(m.r32(m.r32(class + 4) + 0x54) + 12) != 0
                {
                    return Err(
                        "NativeVmLive: array support scope differs from actual caller/class".into(),
                    );
                }
                self.array_scope = Some(ArrayScope {
                    start: self.issued.len(),
                    thread: owner.token,
                    frame,
                    pc: m.r32(thread + 0x24),
                    class,
                    descriptor,
                });
            }
            Observation::ArraySupportEnd {
                thread,
                class,
                object,
            } => {
                let scope = self
                    .array_scope
                    .as_ref()
                    .ok_or("NativeVmLive: array support scope absent")?;
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: array completed caller absent")?;
                let current_frame = self
                    .physical_frames
                    .get(&scope.frame.address)
                    .ok_or("NativeVmLive: array completed caller generation absent")?;
                if scope.thread != owner.token
                    || current_frame.token != scope.frame.token
                    || current_frame.released
                    || owner.destroyed
                    || scope.class != class
                    || m.r32(thread + 0x20) != scope.frame.address
                    || m.r32(thread + 0x24) != scope.pc
                    || bytes(m, m.r32(m.r32(class + 8)))? != scope.descriptor
                    || m.r32(object + 12) != class
                    || m.r32(class + 0x1d0) == 0
                {
                    return Err(
                        "NativeVmLive: array support completion caller/class differs".into(),
                    );
                }
                let source = self.issued.iter().skip(scope.start).collect::<Vec<_>>();
                let receipt = array_support_object(&source, class)?;
                if !matches!(receipt, objects::Receipt::Object { address, .. } if *address == object)
                {
                    return Err("NativeVmLive: array support completed another object".into());
                }
                let scope = self.array_scope.take().unwrap();
                let source = self.issued.drain(scope.start..).collect();
                self.issued.push_back(Receipt::ArraySupport {
                    thread: scope.thread,
                    frame: scope.frame,
                    pc: scope.pc,
                    class,
                    descriptor: scope.descriptor,
                    source,
                });
            }
            Observation::StaticField {
                class,
                index,
                variable,
                record,
            } => {
                let receipt = self
                    .statics
                    .observe(m, jvm, class, index, variable, record)?;
                self.issued.push_back(Receipt::Static(receipt));
            }
            observation @ (Observation::ObjectBegin { .. }
            | Observation::InstanceField { .. }
            | Observation::ObjectCommitted { .. }) => {
                if let Some(receipt) = self.objects.observe(m, jvm, observation)? {
                    self.issued.push_back(Receipt::Object(receipt));
                }
            }
            Observation::Dependencies {
                class,
                base,
                interfaces,
            } => {
                if class == 0
                    || m.r32(class) != 0x5e7340
                    || m.r32(class + 12) != jvm
                    || m.r32(class + 0x1c8) != base
                    || (base != 0 && m.r32(base + 12) != jvm)
                    || interfaces.iter().any(|p| *p == 0 || m.r32(*p + 12) != jvm)
                {
                    return Err(
                        "NativeVmLive: issued class dependencies differ from source owners".into(),
                    );
                }
                self.issued
                    .push_back(Receipt::Class(Observation::Dependencies {
                        class,
                        base,
                        interfaces,
                    }));
            }
            Observation::StaticOwner { class, owner } => {
                if class == 0
                    || m.r32(class + 12) != jvm
                    || owner == 0
                    || m.r32(class + 0x1d0) != owner
                    || m.r32(owner) != 0x5e7338
                    || m.r32(owner + 16) != jvm
                {
                    return Err(
                        "NativeVmLive: issued static owner differs from source construction".into(),
                    );
                }
                self.issued
                    .push_back(Receipt::Class(Observation::StaticOwner { class, owner }));
            }
            Observation::ThreadBegin { address, input } => {
                use crate::native_stack as stack;
                if address == 0
                    || input.jvm != jvm
                    || !stack::active(m)
                    || stack::reg(m, stack::Reg::Ecx) != address
                    || [input.jvm, input.priority, input.flags, input.name]
                        .iter()
                        .enumerate()
                        .any(|(i, v)| stack::argument(m, i) != *v)
                    || self
                        .physical_threads
                        .get(&address)
                        .is_some_and(|t| !t.destroyed)
                {
                    return Err(
                        "NativeVmLive: source thread constructor generation/caller differs".into(),
                    );
                }
                let name = bytes(m, input.name)?;
                let token = self.token()?;
                self.physical_threads.insert(
                    address,
                    Thread {
                        token,
                        address,
                        input,
                        name,
                        frame: None,
                        committed: false,
                        retiring: None,
                        destroyed: false,
                    },
                );
            }
            Observation::FrameConstructed { address, input } => {
                let script = input.context != 0;
                if address == 0
                    || (script && (input.pc == 0 || m.r32(input.context + 12) != jvm))
                    || (!script && (input.pc != 0 || input.receiver != 0 || input.locals != 0))
                {
                    return Err("NativeVmLive: frame source context or entry differs".into());
                }
                if script {
                    let module = m.r32(input.context + 4);
                    let tree = m.r32(module + 0x60);
                    if m.r32(input.context) != 0x5e7340
                        || tree == 0
                        || !(0..m.r32(tree + 8))
                            .any(|index| m.r32(m.r32(tree + 4) + index * 4) == input.pc)
                        || (input.receiver != 0 && m.r32(m.r32(input.receiver + 12) + 12) != jvm)
                    {
                        return Err("NativeVmLive: script frame entry has no issued class/TREE/receiver owner".into());
                    }
                }
                let thread = self
                    .physical_threads
                    .get(&input.thread)
                    .ok_or("NativeVmLive: frame precedes source thread construction")?;
                if thread.destroyed
                    || thread.retiring.is_some()
                    || (script && !thread.committed)
                    || (!script && (thread.committed || thread.frame.is_some()))
                    || m.r32(address) != 0x5e7410
                    || m.r32(address + 0x3c) != input.thread
                    || m.r32(address + 0x2c) != input.pc
                    || m.r32(address + 0x30) != input.receiver
                    || m.r32(address + 0x34) != input.context
                    || self
                        .physical_frames
                        .get(&address)
                        .is_some_and(|f| !f.released)
                {
                    return Err(
                        "NativeVmLive: initial frame differs from its source constructor owner"
                            .into(),
                    );
                }
                let thread = thread.token;
                let token = self.token()?;
                let frame = Frame {
                    token,
                    address,
                    thread,
                    input,
                    released: false,
                    saved_pc: input.pc,
                };
                self.physical_frames.insert(address, frame);
                if script {
                    self.script_frames.push(ScriptFrameHistory {
                        generation: token,
                        address,
                        thread_generation: thread,
                        thread: input.thread,
                        context: input.context,
                        pc: input.pc,
                        receiver: input.receiver,
                        locals: input.locals,
                        static_field: None,
                        signature: None,
                        method: None,
                        released: false,
                    });
                } else {
                    self.physical_threads.get_mut(&input.thread).unwrap().frame = Some(frame);
                }
            }
            Observation::SavedPc { thread, frame, pc } => {
                let source = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: saved PC thread absent")?;
                let owner = *self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: saved PC frame absent")?;
                if source.destroyed
                    || !source.committed
                    || owner.released
                    || owner.thread != source.token
                    || m.r32(thread + 0x20) != frame
                    || m.r32(thread + 0x24) != pc
                    || m.r32(frame + 0x2c) != pc
                {
                    return Err("NativeVmLive: saved PC differs from source frame store".into());
                }
                self.physical_frames.get_mut(&frame).unwrap().saved_pc = pc;
                self.issued.push_back(Receipt::SavedPc {
                    thread: source.token,
                    frame: owner,
                    pc,
                });
            }
            Observation::OperandPop {
                thread,
                frame,
                index,
                cell,
            } => {
                let source = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: popped operand thread absent")?;
                let owner = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: popped operand frame absent")?;
                let vector = m.r32(frame + 0x28);
                if source.destroyed
                    || !source.committed
                    || owner.released
                    || owner.thread != source.token
                    || m.r32(thread + 0x20) != frame
                    || m.r32(vector + 8) as usize != index
                    || m.r32(m.r32(vector + 4) + index as u32 * 4) != 0
                    || cell == 0
                    || !matches!(m.r32(cell), 0x5e7344 | 0x5e734c)
                {
                    return Err("NativeVmLive: popped operand differs from source removal".into());
                }
                self.issued.push_back(Receipt::Operand {
                    thread: source.token,
                    frame: owner.token,
                    index,
                    cell,
                    action: slr_vm::machine::OperandAction::Pop,
                    value_token: if m.r32(cell) == 0x5e734c {
                        Some(self.values.token(cell)?)
                    } else {
                        None
                    },
                });
            }
            Observation::ParameterAssignment {
                thread,
                frame,
                source,
                destination,
                source_before,
                source_after,
                destination_before,
                destination_after,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: parameter assignment thread absent")?;
                let activation = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: parameter assignment frame absent")?;
                let vector = m.r32(frame + 0x18);
                let count = m.r32(vector + 8);
                let current_source = ValueSnapshot::read(m, source)?;
                let current_destination = ValueSnapshot::read(m, destination)?;
                if owner.destroyed
                    || !owner.committed
                    || activation.released
                    || activation.thread != owner.token
                    || count == 0
                    || m.r32(m.r32(vector + 4) + (count - 1) * 4) != destination
                    || m.r32(source) != 0x5e734c
                    || m.r32(destination) != 0x5e7344
                    || source_before.descriptor == 0
                    || destination_before.descriptor != destination_after.descriptor
                    || destination_before.name != destination_after.name
                    || current_source.descriptor != source_after.descriptor
                    || current_source.payload != source_after.payload
                    || current_destination.descriptor != destination_after.descriptor
                    || current_destination.payload != destination_after.payload
                {
                    return Err("NativeVmLive: parameter assignment source owners differ".into());
                }
                self.issued.push_back(Receipt::Parameter {
                    thread: owner.token,
                    frame: activation.token,
                    source: self.values.token(source)?,
                    destination: self.variables.token(destination)?,
                    source_before,
                    source_after,
                    destination_before,
                    destination_after,
                    stage: 0,
                });
            }
            Observation::InitializerAssignment {
                thread,
                frame,
                source,
                destination,
                source_before,
                destination_before,
            } => {
                let owner = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: initializer assignment thread absent")?;
                let activation = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: initializer assignment frame absent")?;
                let source_after = ValueSnapshot::read(m, source)?;
                let destination_after = ValueSnapshot::read(m, destination)?;
                if owner.destroyed
                    || !owner.committed
                    || activation.released
                    || activation.thread != owner.token
                    || m.r32(thread + 0x20) != frame
                    || m.r32(m.r32(frame + 0x28) + 8) != 0
                    || m.r32(source) != 0x5e734c
                    || m.r32(destination) != 0x5e7344
                    || source_before.descriptor == 0
                    || destination_before.descriptor == 0
                {
                    return Err("NativeVmLive: initializer assignment source owners differ".into());
                }
                self.issued.push_back(Receipt::Assignment {
                    thread: owner.token,
                    frame: activation.token,
                    source: self.values.token(source)?,
                    destination,
                    source_before,
                    source_after,
                    destination_before,
                    destination_after,
                    stage: 0,
                });
            }
            Observation::OperandPush {
                thread,
                frame,
                index,
                cell,
            } => {
                let source = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: operand thread absent")?;
                let owner = self
                    .physical_frames
                    .get(&frame)
                    .ok_or("NativeVmLive: operand frame absent")?;
                let vector = m.r32(frame + 0x28);
                if source.destroyed
                    || !source.committed
                    || owner.released
                    || owner.thread != source.token
                    || m.r32(thread + 0x20) != frame
                    || m.r32(vector + 8) as usize != index + 1
                    || m.r32(m.r32(vector + 4) + index as u32 * 4) != cell
                    || (cell != 0 && !matches!(m.r32(cell), 0x5e7344 | 0x5e734c))
                {
                    return Err(
                        "NativeVmLive: pushed operand differs from actual source owner".into(),
                    );
                }
                self.issued.push_back(Receipt::Operand {
                    thread: source.token,
                    frame: owner.token,
                    index,
                    cell,
                    action: slr_vm::machine::OperandAction::Push,
                    value_token: if cell != 0 && m.r32(cell) == 0x5e734c {
                        Some(self.values.token(cell)?)
                    } else {
                        None
                    },
                });
            }
            Observation::Run {
                thread,
                enter,
                budget_bits,
                pc,
                flags,
            } => {
                let source = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: run thread absent")?;
                let frame = m.r32(thread + 0x20);
                let owner = self.physical_frames.get(&frame);
                if source.destroyed
                    || !source.committed
                    || m.r32(thread + 0x10) != jvm
                    || m.r32(thread + 0x24) != pc
                    || m.r32(thread + 0x2c) != flags
                    || (frame != 0
                        && owner.is_none_or(|owner| owner.released || owner.thread != source.token))
                {
                    return Err("NativeVmLive: run differs from source thread/frame owner".into());
                }
                self.issued.push_back(Receipt::Run {
                    thread: source.token,
                    frame: owner.map(|frame| frame.token),
                    enter,
                    budget: budget_bits,
                    pc,
                    flags,
                });
            }
            Observation::Opcode {
                thread,
                enter,
                pc,
                opcode,
                operand,
                immediate,
                next_pc,
                flags,
            } => {
                let source = self
                    .physical_threads
                    .get(&thread)
                    .ok_or("NativeVmLive: opcode thread absent")?;
                let frame = self
                    .physical_frames
                    .get(&m.r32(thread + 0x20))
                    .ok_or("NativeVmLive: opcode frame absent")?;
                if source.destroyed
                    || !source.committed
                    || frame.released
                    || frame.thread != source.token
                    || m.r32(pc) != u32::from(opcode) | (u32::from(operand) << 16)
                    || m.r32(pc + 4) != immediate
                    || m.r32(thread + 0x2c) != flags
                    || (enter && (next_pc != Some(pc) || m.r32(thread + 0x24) != pc))
                    || (!enter && next_pc != Some(m.r32(thread + 0x24)))
                {
                    return Err("NativeVmLive: opcode differs from source instruction/owner".into());
                }
                self.issued.push_back(Receipt::Opcode {
                    thread: source.token,
                    frame: frame.token,
                    enter,
                    pc,
                    opcode,
                    operand,
                    immediate,
                    next_pc,
                    flags,
                });
            }
            Observation::StaticFieldFrame {
                address,
                class,
                index,
                variable,
                record,
            } => {
                let frame = *self
                    .physical_frames
                    .get(&address)
                    .ok_or("NativeVmLive: activated field frame has no constructor")?;
                let fields = m.r32(m.r32(class + 4) + 0x54);
                let owner = m.r32(class + 0x1d0);
                let variables = m.r32(owner + 12);
                let tree = m.r32(m.r32(class + 4) + 0x60);
                let initializer = m.r32(record + 16);
                if frame.released
                    || frame.input.context != class
                    || frame.input.receiver != 0
                    || frame.input.locals != 0
                    || m.r32(address + 0x38) != 0x6075c0
                    || m.r32(frame.input.thread + 0x20) != address
                    || m.r32(frame.input.thread + 0x24) != frame.input.pc
                    || index >= m.r32(fields + 12) as usize
                    || m.r32(m.r32(fields + 8) + index as u32 * 4) != record
                    || initializer >= m.r32(tree + 8)
                    || m.r32(m.r32(tree + 4) + initializer * 4) != frame.input.pc
                    || index >= m.r32(variables + 8) as usize
                    || m.r32(m.r32(variables + 4) + index as u32 * 4) != variable
                    || m.r32(address + 4) == 0
                    || m.r32(address + 8) == 0
                {
                    return Err(
                        "NativeVmLive: activated static field frame differs from source linkage"
                            .into(),
                    );
                }
                let history = self
                    .script_frames
                    .iter_mut()
                    .rev()
                    .find(|history| history.generation == frame.token)
                    .ok_or("NativeVmLive: field frame history absent")?;
                if history.signature.is_some() {
                    return Err("NativeVmLive: field frame activated twice".into());
                }
                history.signature = Some(m.r32(address + 0x38));
                history.static_field = Some((index, variable, record));
                self.issued.push_back(Receipt::ScriptFrame {
                    frame,
                    stage: 0,
                    identity: None,
                });
            }
            Observation::ThreadCommitted { address } => {
                let thread = self
                    .physical_threads
                    .get_mut(&address)
                    .ok_or("NativeVmLive: source thread commit has no constructor")?;
                let frame = thread
                    .frame
                    .ok_or("NativeVmLive: source thread commit has no initial frame")?;
                if thread.committed
                    || m.r32(address) != 0x5f098c
                    || m.r32(address + 0x10) != jvm
                    || m.r32(address + 0x20) != frame.address
                    || m.r32(address + 0x28) != thread.input.priority
                    || m.r32(address + 0x2c) != thread.input.flags
                    || bytes(m, m.r32(address + 0x1c))? != thread.name
                {
                    return Err(
                        "NativeVmLive: committed thread differs from its actual construction"
                            .into(),
                    );
                }
                thread.committed = true;
                self.issued.push_back(Receipt::Thread {
                    thread: thread.clone(),
                    stage: 0,
                    identity: None,
                });
            }
            Observation::ThreadRetiring { address, flags } => {
                let thread = self
                    .physical_threads
                    .get_mut(&address)
                    .ok_or("NativeVmLive: retirement has no source thread")?;
                let frame = thread
                    .frame
                    .ok_or("NativeVmLive: retiring thread has no initial frame")?;
                if !thread.committed
                    || thread.retiring.is_some()
                    || thread.destroyed
                    || m.r32(address + 0x2c) != flags
                    || m.r32(address + 0x20) != frame.address
                {
                    return Err("NativeVmLive: source thread retirement generation differs".into());
                }
                // This bounded slice owns exactly the empty constructor frame.
                let list = m.r32(address + 0xc);
                if m.r32(list + 8) != frame.address
                    || m.r32(frame.address + 4) != list + 16
                    || m.r32(frame.address + 8) != list + 4
                    || m.r32(frame.address + 0x14) != 0
                    || m.r32(frame.address + 0x24) != 0
                {
                    return Err(
                        "NativeVmLive: populated thread retirement needs its source continuation"
                            .into(),
                    );
                }
                thread.retiring = Some(flags);
            }
            Observation::FrameReleased { address } => {
                let frame = *self
                    .physical_frames
                    .get(&address)
                    .ok_or("NativeVmLive: released frame has no source generation")?;
                if frame.released || m.r32(m.r32(0x62ddf0) + 16) != address {
                    return Err(
                        "NativeVmLive: frame release differs from source pool handoff".into(),
                    );
                }
                let thread = self
                    .physical_threads
                    .get(&frame.input.thread)
                    .ok_or("NativeVmLive: frame release owner is absent")?;
                if thread.token != frame.thread
                    || !thread.committed
                    || thread.destroyed
                    || (frame.input.context == 0 && thread.retiring.is_none())
                {
                    return Err(
                        "NativeVmLive: frame released outside its source owner lifetime".into(),
                    );
                }
                if frame.input.context != 0 {
                    let history = self
                        .script_frames
                        .iter_mut()
                        .rev()
                        .find(|history| history.generation == frame.token)
                        .ok_or("NativeVmLive: script-frame generation is absent")?;
                    if history.released || m.r32(address + 4) != 0 || m.r32(address + 8) != 0 {
                        return Err(
                            "NativeVmLive: script frame was not unlinked by source release".into(),
                        );
                    }
                    history.released = true;
                    let caller = *self
                        .physical_frames
                        .get(&m.r32(thread.address + 0x20))
                        .ok_or("NativeVmLive: script release caller absent")?;
                    if m.r32(thread.address + 0x20) != caller.address
                        || m.r32(m.r32(address + 0x28) + 8) != 0
                    {
                        return Err(
                            "NativeVmLive: script release did not restore empty caller".into()
                        );
                    }
                    self.issued.push_back(Receipt::ScriptRetire {
                        frame,
                        caller,
                        stage: 1,
                    });
                }
                self.physical_frames.get_mut(&address).unwrap().released = true;
            }
            Observation::ThreadDestroyed { address } => {
                let thread = self
                    .physical_threads
                    .get_mut(&address)
                    .ok_or("NativeVmLive: destroyed thread has no source generation")?;
                let frame = thread
                    .frame
                    .ok_or("NativeVmLive: destroyed thread has no issued frame")?;
                let released = self
                    .physical_frames
                    .get(&frame.address)
                    .ok_or("NativeVmLive: destroyed frame generation disappeared")?;
                if thread.destroyed
                    || thread.retiring.is_none()
                    || released.token != frame.token
                    || !released.released
                    || m.r32(address) != 0x5f087c
                    || m.r32(address + 4) != 0
                    || m.r32(address + 8) != 0
                {
                    return Err("NativeVmLive: source thread destruction is not complete".into());
                }
                thread.destroyed = true;
                self.issued.push_back(Receipt::Retire {
                    thread: thread.clone(),
                    stage: 0,
                    identity: None,
                });
            }
        }
        Ok(())
    }
    pub fn apply(
        &mut self,
        m: &Space,
        entry: &Entry,
        metadata: &mut Metadata,
    ) -> Result<(), String> {
        if entry.sequence != metadata.sequence {
            return Err(format!(
                "NativeVmLive: expected sequence {}, received {}",
                metadata.sequence, entry.sequence
            ));
        }
        let next = metadata
            .sequence
            .checked_add(1)
            .ok_or("NativeVmLive: sequence exhausted")?;
        let raw_dependencies =
            |base: &Option<u32>, interfaces: &[u32]| -> Result<(u32, Vec<u32>), String> {
                let base = base
                    .map(|id| metadata.class(id).map(|r| r.class))
                    .transpose()?
                    .unwrap_or(0);
                Ok((
                    base,
                    interfaces
                        .iter()
                        .map(|id| metadata.class(*id).map(|r| r.class))
                        .collect::<Result<Vec<_>, _>>()?,
                ))
            };
        let complete = match (&entry.event, self.issued.front_mut()) {
            (Event::Frame {thread,phase:FramePhase::Return,frame,caller},Some(Receipt::FrameReturn {frame:source,caller:parent})) => {
                let owner=self.threads.get(thread).ok_or("NativeVmLive: frame return semantic thread absent")?;
                let binding=self.frames.get(&frame.identity).ok_or("NativeVmLive: frame return semantic generation absent")?;
                let parent_binding=caller.and_then(|caller|self.frames.get(&caller)).ok_or("NativeVmLive: frame return semantic caller absent")?;
                let receiver=if frame.receiver == 0 {0} else {self.objects.object(m,frame.receiver)?};
                if owner.retired || owner.token != source.thread || owner.current != Some(frame.identity) || binding.token != source.token || binding.thread != *thread || binding.phase != FramePhase::Enter || binding.caller != *caller || parent_binding.token != parent.token || metadata.class(frame.context)?.class != source.input.context || frame.saved_pc.map(|pc|metadata.pc(m,pc)).transpose()? != Some(source.saved_pc) || receiver != source.input.receiver || frame.signature != 0 {
                    return Err("NativeVmLive: frame return differs from actual source destructor entry".into());
                }
                self.frames.get_mut(&frame.identity).unwrap().phase=FramePhase::Return;
                true
            }

            (
                Event::Frame {
                    thread,
                    phase: FramePhase::SavePc,
                    frame,
                    caller,
                },
                Some(Receipt::SavedPc {
                    thread: raw_thread,
                    frame: raw_frame,
                    pc,
                }),
            ) => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: saved PC has no bound thread")?;
                let binding = self
                    .frames
                    .get(&frame.identity)
                    .ok_or("NativeVmLive: saved PC has no bound frame")?;
                let saved = frame
                    .saved_pc
                    .map(|pc| metadata.pc(m, pc))
                    .transpose()?
                    .unwrap_or(0);
                let context = if frame.context == 0 {
                    0
                } else {
                    metadata.class(frame.context)?.class
                };
                if owner.retired
                    || owner.token != *raw_thread
                    || owner.current != Some(frame.identity)
                    || binding.thread != *thread
                    || binding.token != raw_frame.token
                    || binding.phase != FramePhase::Enter
                    || binding.caller != *caller
                    || saved != *pc
                    || context != raw_frame.input.context
                    || (if frame.receiver == 0 {0} else {self.objects.object(m,frame.receiver)?}) != raw_frame.input.receiver
                    || frame.signature != 0
                {
                    return Err(
                        "NativeVmLive: saved PC event differs from source store/owner".into(),
                    );
                }
                true
            }
            (Event::NativeEnter {thread,frame,pc,defining_class,method_index,implementation,descriptor,receiver,cells}, Some(Receipt::NativeEnter(source))) => {
                let owner=self.threads.get(thread).ok_or("NativeVmLive: native semantic thread binding absent")?;
                let activation=frame.and_then(|frame|self.frames.get(&frame)).ok_or("NativeVmLive: native semantic frame binding absent")?;
                let receiver=if *receiver == 0 {0} else {self.objects.object(m,*receiver)?};
                let method=method_index.ok_or("NativeVmLive: native selected method ordinal absent")?;
                if owner.retired || owner.token != source.thread || owner.current != *frame || activation.token != source.frame.token || activation.phase != FramePhase::Enter || pc.map(|pc|metadata.pc(m,pc)).transpose()?.unwrap_or(0) != source.pc || metadata.class(*defining_class)?.class != source.class || metadata.method(m,*defining_class,method)? != source.method || *implementation != source.implementation || descriptor.as_bytes() != source.descriptor || receiver != source.receiver || cells.len() != source.cells.len() {
                    return Err("NativeVmLive: native entry differs from source CALL/method generation".into());
                }
                let mut identities=std::collections::HashSet::new();
                for (cell,(address,token,value)) in cells.iter().zip(&source.cells) {
                    let id=slr_vm::values::CellId {pool:cell.pool,slot:cell.slot,generation:cell.generation};
                    if cell.kind != slr_vm::values::PoolKind::Value || cell.name.is_some() || !identities.insert(id) || self.values.historical(id)? != (*address,*token) || !snapshot_matches(value,cell.value,|v|self.objects.object(m,v))? {
                        return Err("NativeVmLive: native argument differs from source value generation".into());
                    }
                }
                self.natives.push(BoundNative {thread:*thread,source:Native {thread:source.thread,frame:source.frame,pc:source.pc,class:source.class,method:source.method,descriptor:source.descriptor.clone(),receiver:source.receiver,implementation:source.implementation,info:source.info,cells:source.cells.clone(),reference_result:source.reference_result}});
                true
            }
            (Event::NativeReturn {thread,implementation,executed,result,error}, Some(Receipt::NativeReturn {thread:raw_thread,implementation:raw_implementation,result:raw_result})) => {
                let active=self.natives.last().ok_or("NativeVmLive: native return has no owned entry")?;
                let result=if active.source.reference_result {result.map(|word|if word == 0 {Ok(0)} else {self.objects.object(m,word)}).transpose()?} else {*result};
                if active.thread != *thread || active.source.thread != *raw_thread || active.source.implementation != *implementation || implementation != raw_implementation || !*executed || error.is_some() || result != *raw_result {
                    return Err("NativeVmLive: native return differs from current nested source owner".into());
                }
                self.natives.pop();
                true
            }
            (Event::MethodSelect {thread, frame, pc, defining_class, method_index, receiver}, Some(Receipt::MethodSelect {thread:raw_thread, frame:raw_frame, pc:raw_pc, class, method, receiver:raw_receiver})) => {
                let owner = self.threads.get(thread).ok_or("NativeVmLive: selected method thread binding absent")?;
                let binding = frame.and_then(|frame| self.frames.get(&frame)).ok_or("NativeVmLive: selected method frame binding absent")?;
                let receiver = if *receiver == 0 {0} else {self.objects.object(m,*receiver)?};
                if owner.retired || owner.token != *raw_thread || owner.current != *frame || binding.token != raw_frame.token || binding.thread != *thread || binding.phase != FramePhase::Enter || pc.map(|pc| metadata.pc(m,pc)).transpose()?.unwrap_or(0) != *raw_pc || metadata.class(*defining_class)?.class != *class || metadata.method(m,*defining_class,*method_index)? != *method || receiver != *raw_receiver {
                    return Err("NativeVmLive: selected method differs from source record/caller".into());
                }
                true
            }
            (Event::Frame {thread,phase,frame,caller}, Some(Receipt::MethodFrame {frame:source,caller:parent,method,signature,enter})) => {
                let owner = self.threads.get(thread).ok_or("NativeVmLive: method frame thread binding absent")?;
                let parent_binding = caller.and_then(|caller| self.frames.get(&caller)).ok_or("NativeVmLive: method frame caller binding absent")?;
                let receiver = if frame.receiver == 0 {0} else {self.objects.object(m,frame.receiver)?};
                let context = metadata.class(frame.context)?.class;
                let method_owned = metadata.methods.keys().any(|(class,index)| *class == frame.context && metadata.method(m,*class,*index).is_ok_and(|record| record == *method));
                if owner.retired || owner.token != source.thread || parent_binding.token != parent.token || parent_binding.thread != *thread || parent_binding.phase != FramePhase::Enter || source.input.context != context || frame.saved_pc.map(|pc| metadata.pc(m,pc)).transpose()? != Some(source.input.pc) || receiver != source.input.receiver || frame.signature != 0 || *signature != m.r32(*method+0x18) || !method_owned || frame.identity == 0 {
                    return Err("NativeVmLive: method frame differs from issued source owner".into());
                }
                if !*enter {
                    if *phase != FramePhase::Create || owner.current != *caller || self.frames.contains_key(&frame.identity) {
                        return Err("NativeVmLive: method frame creation phase differs".into());
                    }
                    self.frames.insert(frame.identity,BoundFrame {address:source.address,token:source.token,thread:*thread,phase:FramePhase::Create,depth:0,locals:Vec::new(),caller:*caller});
                } else {
                    let bound = self.frames.get(&frame.identity).ok_or("NativeVmLive: entered method frame binding absent")?;
                    if *phase != FramePhase::Enter || owner.current != Some(frame.identity) || bound.token != source.token || bound.phase != FramePhase::Create || bound.caller != *caller {
                        return Err("NativeVmLive: method frame entry generation differs".into());
                    }
                    self.frames.get_mut(&frame.identity).unwrap().phase=FramePhase::Enter;
                }
                true
            }
            (Event::CurrentFrame {thread,frame},Some(Receipt::Current {thread:raw_thread,frame:source})) => {
                let owner = self.threads.get(thread).ok_or("NativeVmLive: current frame thread binding absent")?;
                let bound = self.frames.get(frame).ok_or("NativeVmLive: current frame binding absent")?;
                if owner.retired || owner.token != *raw_thread || bound.token != source.token || bound.thread != *thread || !matches!(bound.phase,FramePhase::Create|FramePhase::Enter) {
                    return Err("NativeVmLive: current frame differs from source store/generation".into());
                }
                self.threads.get_mut(thread).unwrap().current=Some(*frame);
                true
            }
            (Event::Pool(event),Some(Receipt::Variable(receipt))) => {
                self.variables.apply(event,receipt,|value| {
                    if matches!(value.tag.name().as_bytes().first(),Some(b'L'|b'[')) && value.payload != 0 {self.objects.object(m,value.payload)} else {Ok(value.payload)}
                })?;
                true
            }
            (Event::Local {thread,frame,append,index,variable},Some(Receipt::Local {thread:raw_thread,frame:raw_frame,append:raw_append,index:raw_index,variable:address,token})) => {
                let owner = self.threads.get(thread).ok_or("NativeVmLive: local thread binding absent")?;
                let activation = self.frames.get(frame).ok_or("NativeVmLive: local frame binding absent")?;
                let (raw, generation)=self.variables.historical(*variable)?;
                if owner.retired || owner.token != *raw_thread || owner.current != Some(*frame) || activation.thread != *thread || activation.token != *raw_frame || !matches!(activation.phase,FramePhase::Create|FramePhase::Enter|FramePhase::Return) || append != raw_append || index != raw_index || raw != *address || generation != *token || if *append {activation.locals.len() != *index} else {activation.locals.get(*index) != Some(&Some(*variable))} {
                    return Err("NativeVmLive: local owner differs from source append/removal".into());
                }
                let locals=&mut self.frames.get_mut(frame).unwrap().locals;
                if *append {locals.push(Some(*variable));} else if *index+1 == locals.len() {locals.pop();}
                true
            }
            (Event::VariableStore {identity,before,after},Some(Receipt::VariableStore {token,before:raw_before,after:raw_after})) => {
                if self.variables.historical(*identity)?.1 != *token || !snapshot_matches(raw_before,*before,|v|self.objects.object(m,v))? || !snapshot_matches(raw_after,*after,|v|self.objects.object(m,v))? {
                    return Err("NativeVmLive: local store differs from source generation/value".into());
                }
                true
            }
            (Event::Pool(event), Some(Receipt::Value(receipt))) => {
                self.values.apply(event, receipt, |value| {
                    if matches!(value.tag.name().as_bytes().first(), Some(b'L' | b'['))
                        && value.payload != 0
                    {
                        self.objects.object(m, value.payload)
                    } else {
                        Ok(value.payload)
                    }
                })?;
                true
            }
            (
                Event::Operand {
                    thread,
                    frame,
                    action,
                    slot,
                },
                Some(Receipt::Operand {
                    thread: raw_thread,
                    frame: raw_frame,
                    index,
                    cell,
                    value_token,
                    action: raw_action,
                }),
            ) => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: operand has no thread binding")?;
                let binding = self
                    .frames
                    .get(frame)
                    .ok_or("NativeVmLive: operand has no frame binding")?;
                let (address, token) = match slot {
                    slr_vm::machine::Operand::Null => (0, None),
                    slr_vm::machine::Operand::Value(id) => {
                        let (address, token) = self.values.historical(*id)?;
                        (address, Some(token))
                    }
                    slr_vm::machine::Operand::Variable(id) => {
                        let (address, _) = self.variables.historical(*id)?;
                        (address, None)
                    }
                    slr_vm::machine::Operand::Heap(slr_vm::interp::HeapSlot::Static {
                        context,
                        slot,
                    }) => (self.statics.field(m, *context, *slot, metadata)?, None),
                    slr_vm::machine::Operand::Heap(slr_vm::interp::HeapSlot::Object {
                        word,
                        slot,
                    }) => (self.objects.field(m, *word, *slot, metadata)?, None),
                    _ => {
                        return Err(
                            "NativeVmLive: operand cell/variable source binding unavailable".into(),
                        )
                    }
                };
                if owner.retired
                    || owner.token != *raw_thread
                    || owner.current != Some(*frame)
                    || binding.thread != *thread
                    || binding.token != *raw_frame
                    || !matches!(binding.phase,FramePhase::Enter|FramePhase::Return)
                    || action != raw_action
                    || binding.depth
                        != match action {
                            slr_vm::machine::OperandAction::Push => *index,
                            slr_vm::machine::OperandAction::Pop | slr_vm::machine::OperandAction::Drop => *index + 1,
                        }
                    || address != *cell
                    || token != *value_token
                {
                    return Err("NativeVmLive: operand event differs from source append".into());
                }
                let depth = match action {
                    slr_vm::machine::OperandAction::Push => binding
                        .depth
                        .checked_add(1)
                        .ok_or("NativeVmLive: operand depth overflow")?,
                    slr_vm::machine::OperandAction::Pop | slr_vm::machine::OperandAction::Drop => *index,
                };
                self.frames.get_mut(frame).unwrap().depth = depth;
                true
            }
            (
                Event::CellStore {
                    identity,
                    before,
                    after,
                },
                Some(Receipt::Assignment {
                    thread,
                    frame,
                    source,
                    source_before,
                    source_after,
                    stage,
                    ..
                }),
            ) if *stage == 0 => {
                let (address, token) = self.values.historical(*identity)?;
                let active = self
                    .threads
                    .values()
                    .find(|owner| owner.token == *thread)
                    .ok_or("NativeVmLive: assignment thread binding absent")?;
                let activation = active
                    .current
                    .and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: assignment frame binding absent")?;
                if active.retired
                    || active.running
                    || active.opcode.is_some()
                    || activation.token != *frame
                    || activation.depth != 0
                    || token != *source
                    || address == 0
                    || !snapshot_matches(source_before, *before, |v| self.objects.object(m, v))?
                    || !snapshot_matches(source_after, *after, |v| self.objects.object(m, v))?
                {
                    return Err("NativeVmLive: initializer source conversion differs".into());
                }
                *stage = 1;
                false
            }
            (
                Event::CellStore {
                    identity,
                    before,
                    after,
                },
                Some(Receipt::Parameter {
                    thread,
                    frame,
                    source,
                    source_before,
                    source_after,
                    stage,
                    ..
                }),
            ) if *stage == 0 => {
                let (address, token) = self.values.historical(*identity)?;
                let active = self
                    .threads
                    .values()
                    .find(|owner| owner.token == *thread)
                    .ok_or("NativeVmLive: parameter thread binding absent")?;
                let activation = active
                    .current
                    .and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: parameter frame binding absent")?;
                if active.retired
                    || activation.token != *frame
                    || activation.phase != FramePhase::Create
                    || token != *source
                    || address == 0
                    || !snapshot_matches(source_before, *before, |v| self.objects.object(m, v))?
                    || !snapshot_matches(source_after, *after, |v| self.objects.object(m, v))?
                {
                    return Err("NativeVmLive: parameter source conversion differs".into());
                }
                *stage = 1;
                false
            }
            (
                Event::VariableStore {
                    identity,
                    before,
                    after,
                },
                Some(Receipt::Parameter {
                    thread,
                    frame,
                    destination,
                    destination_before,
                    destination_after,
                    stage,
                    ..
                }),
            ) if *stage == 1 => {
                let active = self
                    .threads
                    .values()
                    .find(|owner| owner.token == *thread)
                    .ok_or("NativeVmLive: parameter thread binding absent")?;
                let activation = active
                    .current
                    .and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: parameter frame binding absent")?;
                if active.retired
                    || activation.token != *frame
                    || activation.locals.last() != Some(&Some(*identity))
                    || self.variables.historical(*identity)?.1 != *destination
                    || !snapshot_matches(destination_before, *before, |v| {
                        self.objects.object(m, v)
                    })?
                    || !snapshot_matches(destination_after, *after, |v| self.objects.object(m, v))?
                {
                    return Err("NativeVmLive: parameter destination store differs".into());
                }
                true
            }
            (
                Event::HeapStore {
                    slot: slr_vm::interp::HeapSlot::Static { context, slot },
                    before,
                    after,
                },
                Some(Receipt::Assignment {
                    destination,
                    destination_before,
                    destination_after,
                    stage,
                    ..
                }),
            ) if *stage == 1 => {
                if self.statics.field(m, *context, *slot, metadata)? != *destination
                    || !snapshot_matches(destination_before, *before, |v| {
                        self.objects.object(m, v)
                    })?
                    || !snapshot_matches(destination_after, *after, |v| self.objects.object(m, v))?
                {
                    return Err("NativeVmLive: initializer destination store differs".into());
                }
                true
            }
            (
                Event::Frame {
                    thread,
                    phase,
                    frame,
                    caller,
                },
                Some(Receipt::ScriptRetire {
                    frame: source,
                    caller: parent,
                    stage,
                }),
            ) if *stage < 2 => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: script return thread absent")?;
                let binding = self
                    .frames
                    .get(&frame.identity)
                    .ok_or("NativeVmLive: script return frame absent")?;
                let parent_binding = caller
                    .and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: script return caller absent")?;
                if owner.retired
                    || owner.token != source.thread
                    || owner.current != Some(frame.identity)
                    || binding.thread != *thread
                    || binding.token != source.token
                    || binding.depth != 0
                    || binding.caller != *caller
                    || parent_binding.token != parent.token
                    || metadata.class(frame.context)?.class != source.input.context
                    || frame.saved_pc.map(|pc| metadata.pc(m, pc)).transpose()?
                        != Some(source.saved_pc)
                    || (if frame.receiver == 0 {0} else {self.objects.object(m,frame.receiver)?}) != source.input.receiver
                    || frame.signature != 0
                    || binding.locals.iter().any(Option::is_some)
                    || (*stage == 0
                        && (*phase != FramePhase::Return || binding.phase != FramePhase::Enter))
                    || (*stage == 1
                        && (*phase != FramePhase::Destroy || binding.phase != FramePhase::Return))
                {
                    return Err(
                        "NativeVmLive: script return/destroy differs from source release".into(),
                    );
                }
                self.frames.get_mut(&frame.identity).unwrap().phase = *phase;
                *stage += 1;
                false
            }
            (
                Event::CurrentFrame { thread, frame },
                Some(Receipt::ScriptRetire {
                    frame: source,
                    caller,
                    stage,
                }),
            ) if *stage == 2 => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: restored current thread absent")?;
                let parent = self
                    .frames
                    .get(frame)
                    .ok_or("NativeVmLive: restored current frame absent")?;
                let previous = owner
                    .current
                    .and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: released current frame absent")?;
                if owner.retired
                    || owner.token != source.thread
                    || parent.thread != *thread
                    || parent.token != caller.token
                    || parent.phase != FramePhase::Enter
                    || previous.token != source.token
                    || previous.phase != FramePhase::Destroy
                {
                    return Err(
                        "NativeVmLive: current frame restoration differs from source unlink".into(),
                    );
                }
                self.threads.get_mut(thread).unwrap().current = Some(*frame);
                true
            }
            (
                Event::Run {
                    thread,
                    enter,
                    budget_bits,
                    pc,
                    flags,
                    error,
                },
                Some(Receipt::Run {
                    thread: raw_thread,
                    frame,
                    enter: raw_enter,
                    budget,
                    pc: raw_pc,
                    flags: raw_flags,
                }),
            ) => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: run has no thread binding")?;
                let source_pc = pc.map(|pc| metadata.pc(m, pc)).transpose()?.unwrap_or(0);
                let current = owner
                    .current
                    .map(|frame| {
                        self.frames
                            .get(&frame)
                            .map(|frame| frame.token)
                            .ok_or("NativeVmLive: run current frame binding absent")
                    })
                    .transpose()?;
                if owner.retired
                    || owner.token != *raw_thread
                    || current != *frame
                    || *enter != *raw_enter
                    || *budget_bits != *budget
                    || source_pc != *raw_pc
                    || *flags != *raw_flags
                    || error.is_some()
                    || owner.running == *enter
                    || owner.opcode.is_some()
                {
                    return Err("NativeVmLive: run event differs from source caller/phase".into());
                }
                self.threads.get_mut(thread).unwrap().running = *enter;
                true
            }
            (
                Event::Opcode {
                    thread,
                    enter,
                    pc,
                    opcode,
                    operand,
                    immediate,
                    next_pc,
                    flags,
                    error,
                },
                Some(Receipt::Opcode {
                    thread: raw_thread,
                    frame,
                    enter: raw_enter,
                    pc: raw_pc,
                    opcode: raw_opcode,
                    operand: raw_operand,
                    immediate: raw_immediate,
                    next_pc: raw_next,
                    flags: raw_flags,
                }),
            ) => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: opcode has no thread binding")?;
                let current = owner
                    .current
                    .and_then(|frame| self.frames.get(&frame))
                    .ok_or("NativeVmLive: opcode current frame binding absent")?;
                let source_pc = metadata.pc(m, *pc)?;
                let next = next_pc.map(|pc| metadata.pc(m, pc)).transpose()?;
                let instruction = (*raw_pc, *raw_opcode, *raw_operand, *raw_immediate);
                if owner.retired
                    || !owner.running
                    || owner.token != *raw_thread
                    || current.token != *frame
                    || *enter != *raw_enter
                    || source_pc != *raw_pc
                    || *opcode != *raw_opcode
                    || *operand != *raw_operand
                    || if slr_fmt_tufa::Node::has_imm(*opcode) {
                        *immediate != *raw_immediate
                    } else {
                        *immediate != 0
                    }
                    || next != *raw_next
                    || *flags != *raw_flags
                    || error.is_some()
                    || (*enter && owner.opcode.is_some())
                    || (!enter && owner.opcode != Some(instruction))
                {
                    return Err(format!(
                        "NativeVmLive: opcode event differs from source instruction/phase at sequence {}: semantic={:?}; raw=(thread={}, frame={}, enter={}, pc={:#x}, opcode={:#x}, operand={:#x}, immediate={:#x}, next={:?}, flags={:#x}); bound=(thread={}, frame={}, running={}, opcode={:?})",
                        entry.sequence, entry.event, raw_thread, frame, raw_enter, raw_pc,
                        raw_opcode, raw_operand, raw_immediate, raw_next, raw_flags,
                        owner.token, current.token, owner.running, owner.opcode,
                    ));
                }
                self.threads.get_mut(thread).unwrap().opcode = enter.then_some(instruction);
                true
            }
            (event, Some(Receipt::Static(receipt))) => {
                self.statics.apply(m, event, metadata, receipt)?;
                true
            }
            (
                event @ Event::ObjectAllocate { array_descriptor: Some(descriptor), .. },
                Some(Receipt::ArraySupport { thread, frame, pc, class, descriptor: raw_descriptor, source }),
            ) => {
                let owner = self.threads.values().find(|owner| owner.token == *thread)
                    .ok_or("NativeVmLive: array allocation caller binding absent")?;
                let activation = owner.current.and_then(|id| self.frames.get(&id))
                    .ok_or("NativeVmLive: array allocation frame binding absent")?;
                if owner.retired || !owner.running || activation.token != frame.token
                    || owner.opcode.is_none_or(|instruction| instruction.0 + 8 != *pc)
                    || descriptor.as_bytes() != raw_descriptor
                {
                    return Err("NativeVmLive: array allocation differs from support caller/descriptor".into());
                }
                let source = source.iter().collect::<Vec<_>>();
                let receipt = array_support_object(&source, *class)?;
                self.objects.apply(m, event, metadata, receipt)?;
                true
            }
            (event, Some(Receipt::Object(receipt))) => {
                self.objects.apply(m, event, metadata, receipt)?;
                true
            }
            (
                Event::ClassDependencies {
                    identity,
                    base,
                    interfaces,
                },
                Some(Receipt::Class(Observation::Dependencies {
                    class,
                    base: raw_base,
                    interfaces: raw_interfaces,
                })),
            ) => {
                let raw = metadata.class(*identity)?.class;
                let (base, interfaces) = raw_dependencies(base, interfaces)?;
                if raw != *class
                    || base != *raw_base
                    || interfaces != *raw_interfaces
                    || self.classes.contains_key(identity)
                {
                    return Err("NativeVmLive: class dependency event differs from next source-issued owner".into());
                }
                self.classes.insert(
                    *identity,
                    Class {
                        raw,
                        base,
                        interfaces,
                        owner: None,
                    },
                );
                true
            }
            (
                Event::ClassInstall {
                    identity,
                    base,
                    interfaces,
                },
                Some(Receipt::Class(Observation::StaticOwner { class, owner })),
            ) => {
                let (base, interfaces) = raw_dependencies(base, interfaces)?;
                let installed = self
                    .classes
                    .get_mut(identity)
                    .ok_or("NativeVmLive: static owner precedes its source dependencies")?;
                if installed.raw != *class
                    || installed.base != base
                    || installed.interfaces != interfaces
                    || installed.owner.is_some()
                {
                    return Err(
                        "NativeVmLive: static install event differs from next source-issued owner"
                            .into(),
                    );
                }
                installed.owner = Some(*owner);
                true
            }
            (
                Event::ThreadCreate { identity },
                Some(Receipt::Thread {
                    thread,
                    stage,
                    identity: bound,
                }),
            ) if *stage == 0 => {
                if *identity == 0 || self.threads.contains_key(identity) {
                    return Err("NativeVmLive: repeated thread identity".into());
                }
                self.threads.insert(
                    *identity,
                    BoundThread {
                        token: thread.token,
                        address: thread.address,
                        frame: None,
                        current: None,
                        running: false,
                        opcode: None,
                        retired: false,
                    },
                );
                *bound = Some(*identity);
                *stage = 1;
                false
            }
            (
                Event::ThreadOrigin {
                    identity,
                    priority,
                    flags,
                    name,
                },
                Some(Receipt::Thread {
                    thread,
                    stage,
                    identity: bound,
                }),
            ) if *stage == 1 => {
                if *bound != Some(*identity)
                    || *priority != thread.input.priority
                    || *flags != thread.input.flags
                    || name.as_bytes() != thread.name
                {
                    return Err(
                        "NativeVmLive: thread origin differs from next source constructor".into(),
                    );
                }
                *stage = 2;
                false
            }
            (
                Event::Frame {
                    thread,
                    phase: FramePhase::Create,
                    frame,
                    caller,
                },
                Some(Receipt::Thread {
                    thread: source,
                    stage,
                    identity,
                }),
            ) if *stage == 2 => {
                if *identity != Some(*thread)
                    || !initial_frame(frame, *caller)
                    || self.frames.contains_key(&frame.identity)
                {
                    return Err(
                        "NativeVmLive: initial frame event differs from source construction".into(),
                    );
                }
                let raw = source
                    .frame
                    .ok_or("NativeVmLive: constructor frame receipt absent")?;
                self.frames.insert(
                    frame.identity,
                    BoundFrame {
                        address: raw.address,
                        token: raw.token,
                        thread: *thread,
                        phase: FramePhase::Create,
                        depth: 0,
                        locals: Vec::new(),
                        caller: None,
                    },
                );
                self.threads.get_mut(thread).unwrap().frame = Some(frame.identity);
                *stage = 3;
                false
            }
            (
                Event::Frame {
                    thread,
                    phase: FramePhase::Enter,
                    frame,
                    caller,
                },
                Some(Receipt::Thread {
                    thread: source,
                    stage,
                    identity,
                }),
            ) if *stage == 3 => {
                let expected = source
                    .frame
                    .ok_or("NativeVmLive: source frame receipt absent")?;
                let binding = self
                    .frames
                    .get_mut(&frame.identity)
                    .ok_or("NativeVmLive: frame entered before its source creation")?;
                if *identity != Some(*thread)
                    || !initial_frame(frame, *caller)
                    || binding.token != expected.token
                    || binding.thread != *thread
                    || binding.phase != FramePhase::Create
                {
                    return Err(
                        "NativeVmLive: initial frame entry differs from its source owner".into(),
                    );
                }
                binding.phase = FramePhase::Enter;
                self.threads.get_mut(thread).unwrap().current = Some(frame.identity);
                true
            }
            (
                Event::Frame {
                    thread,
                    phase,
                    frame,
                    caller,
                },
                Some(Receipt::ScriptFrame {
                    frame: source,
                    stage,
                    identity,
                }),
            ) => {
                let owner = self
                    .threads
                    .get(thread)
                    .ok_or("NativeVmLive: script frame has no bound source thread")?;
                let history = self
                    .script_frames
                    .iter()
                    .find(|history| history.generation == source.token)
                    .ok_or("NativeVmLive: script frame history absent")?;
                let pc = frame
                    .saved_pc
                    .ok_or("NativeVmLive: script frame entry PC absent")?;
                let (index, variable, record) = history
                    .static_field
                    .ok_or("NativeVmLive: script frame caller role absent")?;
                // Field frames have no semantic signature identity. The actual
                // caller's6075c0 word is observed separately, never cleared or
                // supplied by that semantic absence.
                if owner.retired
                    || owner.token != source.thread
                    || *caller != owner.current
                    || metadata.class(frame.context)?.class != source.input.context
                    || metadata.pc(m, pc)? != source.input.pc
                    || frame.receiver != 0
                    || frame.signature != 0
                    || frame.identity == 0
                    || history.signature != Some(0x6075c0)
                    || metadata.field(m, frame.context, true, index)? != record
                    || self.statics.field(m, frame.context, index, metadata)? != variable
                {
                    return Err(
                        "NativeVmLive: script frame differs from its source field caller".into(),
                    );
                }
                match (*stage, phase) {
                    (0, FramePhase::Create) => {
                        if identity.is_some() || self.frames.contains_key(&frame.identity) {
                            return Err("NativeVmLive: repeated script frame identity".into());
                        }
                        self.frames.insert(
                            frame.identity,
                            BoundFrame {
                                address: source.address,
                                token: source.token,
                                thread: *thread,
                                phase: FramePhase::Create,
                                depth: 0,
                            locals: Vec::new(),
                                caller: *caller,
                            },
                        );
                        *identity = Some(frame.identity);
                        *stage = 1;
                        false
                    }
                    (1, FramePhase::Enter) => {
                        let binding = self
                            .frames
                            .get_mut(&frame.identity)
                            .ok_or("NativeVmLive: script frame entered before creation")?;
                        if *identity != Some(frame.identity)
                            || binding.token != source.token
                            || binding.thread != *thread
                            || binding.phase != FramePhase::Create
                        {
                            return Err(
                                "NativeVmLive: script frame entry generation differs".into()
                            );
                        }
                        binding.phase = FramePhase::Enter;
                        self.threads.get_mut(thread).unwrap().current = Some(frame.identity);
                        true
                    }
                    _ => return Err("NativeVmLive: unexpected script frame lifecycle phase".into()),
                }
            }
            (
                Event::ThreadRetire { identity, flags },
                Some(Receipt::Retire {
                    thread,
                    stage,
                    identity: bound,
                }),
            ) if *stage == 0 => {
                let binding = self
                    .threads
                    .get_mut(identity)
                    .ok_or("NativeVmLive: thread retired before source creation")?;
                if binding.retired
                    || binding.token != thread.token
                    || binding.address != thread.address
                    || Some(*flags) != thread.retiring
                {
                    return Err(
                        "NativeVmLive: thread retirement differs from source destruction".into(),
                    );
                }
                binding.retired = true;
                *bound = Some(*identity);
                *stage = 1;
                false
            }
            (
                Event::Frame {
                    thread,
                    phase: FramePhase::Return,
                    frame,
                    caller,
                },
                Some(Receipt::Retire {
                    thread: source,
                    stage,
                    identity,
                }),
            ) if *stage == 1 => {
                let expected = source
                    .frame
                    .ok_or("NativeVmLive: retiring source frame absent")?;
                let binding = self
                    .frames
                    .get_mut(&frame.identity)
                    .ok_or("NativeVmLive: returning frame has no source generation")?;
                if *identity != Some(*thread)
                    || !initial_frame(frame, *caller)
                    || binding.token != expected.token
                    || binding.thread != *thread
                    || binding.phase != FramePhase::Enter
                {
                    return Err("NativeVmLive: frame return differs from source destruction".into());
                }
                binding.phase = FramePhase::Return;
                *stage = 2;
                false
            }
            (
                Event::Frame {
                    thread,
                    phase: FramePhase::Destroy,
                    frame,
                    caller,
                },
                Some(Receipt::Retire {
                    thread: source,
                    stage,
                    identity,
                }),
            ) if *stage == 2 => {
                let expected = source
                    .frame
                    .ok_or("NativeVmLive: retiring source frame absent")?;
                let binding = self
                    .frames
                    .get_mut(&frame.identity)
                    .ok_or("NativeVmLive: destroyed frame has no source generation")?;
                if *identity != Some(*thread)
                    || !initial_frame(frame, *caller)
                    || binding.token != expected.token
                    || binding.thread != *thread
                    || binding.phase != FramePhase::Return
                {
                    return Err(
                        "NativeVmLive: frame destruction differs from source generation".into(),
                    );
                }
                binding.phase = FramePhase::Destroy;
                true
            }
            _ => {
                return Err(format!(
                    "NativeVmLive: source owner continuation unavailable at sequence {}: {:?}; source phase={}, active opcodes={:?}",
                    entry.sequence, entry.event,
                    match self.issued.front() {
                        Some(Receipt::Operand { .. }) => "operand", Some(Receipt::Opcode { .. }) => "opcode",
                        Some(Receipt::Value(_)) => "value", Some(Receipt::Assignment { .. }) => "assignment", Some(Receipt::Parameter { .. }) => "parameter",
                        Some(Receipt::ScriptRetire { .. }) => "script release", Some(_) => "owner", None => "none",
                    },
                    self.threads.iter().filter_map(|(id, thread)| thread.opcode.map(|opcode| (*id, opcode))).collect::<Vec<_>>()
                ))
            }
        };
        if complete {
            self.issued.pop_front();
        }
        metadata.sequence = next;
        Ok(())
    }
}

#[cfg(test)]
#[path = "native_vm_live_owners_tests.rs"]
mod lifecycle_tests;

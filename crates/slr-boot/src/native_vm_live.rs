//! Causal bindings from the lossless VM stream to source-allocated metadata.
//! Semantic identities only select previously issued records. File buffers and
//! loader scratch must come from the actual input/caller owner supplied by Host.
//! This metadata slice does not admit callbacks or bypass the ABI guard.
use crate::native_vm_metadata::loader::{self, LoadedModule};
use slr_physics::space::Space;
use slr_vm::machine::{Entry, Event};
use std::collections::HashMap;
#[path = "native_vm_live_owners.rs"]
mod owners;
pub use owners::{OwnerHistory, ScriptFrameHistory, ValueHistory};

/// Produced by actual file I/O, deserialization and class construction. The
/// file buffer may already be released; these input bytes retain its identity
/// for the subsequent semantic FileRead, never to reconstruct runtime state.
pub struct IssuedFile {
    bytes: Vec<u8>,
    modules: Vec<LoadedModule>,
}

/// The source JVM owns its caller-derived floating-point state. Construction
/// and bootstrap contain no floating-point arithmetic and do not reset it.
/// Object/thread ownership is still required before admitting a live callback.
pub struct Runtime {
    jvm: u32,
    booted: bool,
    fpu: crate::native_vm_fpu::State,
    loader: Loader,
    callback_owner: std::sync::Arc<()>,
    next_callback: u64,
    callbacks: Vec<ActiveCallback>,
}
/// An opaque continuation owned by one live source runtime. A semantic thread
/// identity or captured raw pointer cannot manufacture this capability.
pub struct CallbackId {
    owner: std::sync::Arc<()>,
    generation: u64,
}
struct ActiveCallback {
    generation: u64,
    source: crate::native_vm_callback::Context,
}
struct Loader {
    jvm: u32,
    files: Vec<IssuedFile>,
    next_file: usize,
    metadata: Metadata,
    owners: owners::Owners,
}
impl Runtime {
    pub fn construct<E: crate::native_vm_jvm::Environment>(
        m: &mut Space,
        jvm: u32,
        fpu: crate::native_vm_fpu::State,
        e: &mut E,
    ) -> Result<Self, String> {
        use crate::native_stack as stack;
        if !stack::active(m) || !stack::has_pending_call(m) || stack::reg(m, stack::Reg::Ecx) != jvm
        {
            return Err("NativeVmLive: actual JVM constructor CALL required".into());
        }
        if crate::native_vm_jvm::construct(m, jvm, e)? != jvm {
            return Err("NativeVmLive: constructor returned a different JVM owner".into());
        }
        Ok(Self {
            jvm,
            booted: false,
            fpu,
            callback_owner: std::sync::Arc::new(()),
            next_callback: 0,
            callbacks: Vec::new(),
            loader: Loader {
                jvm,
                files: Vec::new(),
                next_file: 0,
                metadata: Metadata::new(m, jvm)?,
                owners: owners::Owners::default(),
            },
        })
    }
    /// Consume the real pending425a90 CALL only when its script payload is
    /// the object already issued and bound by this runtime. The complete
    /// callback/interpreter state remains owned here across native pauses.
    pub fn begin_callback(
        &mut self,
        m: &mut Space,
        receiver: u32,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_thread_run::Environment),
    ) -> Result<CallbackId, String> {
        use crate::native_stack as stack;
        if !self.booted
            || !stack::active(m)
            || !stack::has_pending_call(m)
            || m.r32(0x63641c) != self.jvm
        {
            return Err("NativeVmLive: booted runtime and actual callback CALL required".into());
        }
        let object = self.object(m, receiver)?;
        let envelope = stack::argument(m, 0);
        if envelope == 0 || m.r32(envelope + 0x50) != object {
            return Err(
                "NativeVmLive: callback receiver differs from its issued source owner".into(),
            );
        }
        let generation = self
            .next_callback
            .checked_add(1)
            .ok_or("NativeVmLive: callback generations exhausted")?;
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        let source = crate::native_vm_callback::Context::begin(m, &mut self.fpu, &mut environment)?;
        self.next_callback = generation;
        self.callbacks.push(ActiveCallback { generation, source });
        Ok(CallbackId {
            owner: self.callback_owner.clone(),
            generation,
        })
    }
    fn callback_index(&self, callback: &CallbackId) -> Result<usize, String> {
        if !std::sync::Arc::ptr_eq(&self.callback_owner, &callback.owner)
            || self
                .callbacks
                .last()
                .is_none_or(|active| active.generation != callback.generation)
        {
            return Err(
                "NativeVmLive: callback is retired, foreign, or not the current nested owner"
                    .into(),
            );
        }
        Ok(self.callbacks.len() - 1)
    }
    pub fn advance_callback(
        &mut self,
        m: &mut Space,
        callback: &CallbackId,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_thread_run::Environment),
    ) -> Result<bool, String> {
        let index = self.callback_index(callback)?;
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        self.callbacks[index]
            .source
            .advance(m, &mut self.fpu, &mut environment)
    }
    pub fn callback_native_entry(
        &self,
        m: &Space,
        callback: &CallbackId,
    ) -> Result<&crate::native_vm_abi::Entry, String> {
        let entry = self.callbacks[self.callback_index(callback)?]
            .source
            .native_entry(m)?;
        self.loader.owners.validate_native_entry(m, entry)?;
        Ok(entry)
    }
    pub fn resume_callback_native(
        &mut self,
        m: &mut Space,
        callback: &CallbackId,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_thread_run::Environment),
    ) -> Result<(), String> {
        self.callback_native_entry(m, callback)?;
        let index = self.callback_index(callback)?;
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        self.callbacks[index]
            .source
            .resume_native(m, &mut self.fpu, &mut environment)
    }
    pub fn finish_callback(&mut self, m: &mut Space, callback: &CallbackId) -> Result<(), String> {
        let index = self.callback_index(callback)?;
        self.callbacks[index]
            .source
            .finish_setup(m, &mut self.fpu)?;
        self.callbacks[index].source.finish_wrapper(m)?;
        self.callbacks.pop();
        Ok(())
    }
    pub fn class_lookup(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_namespace::FileEnvironment,
    ) -> Result<u32, String> {
        if !self.booted {
            return Err("NativeVmLive: source bootstrap is not complete".into());
        }
        self.loader.class_lookup(m, e)
    }
    pub fn initialize_empty_class(
        &mut self,
        m: &mut Space,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_dispatch::Environment),
    ) -> Result<(), String> {
        let class = crate::native_stack::reg(m, crate::native_stack::Reg::Ecx);
        if !self.booted
            || !self
                .loader
                .files
                .iter()
                .any(|f| f.modules.iter().any(|raw| raw.class == class))
        {
            return Err("NativeVmLive: source-issued class initializer owner required".into());
        }
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        crate::native_vm_class_init::initialize_empty(m, class, &mut environment)
    }
    pub fn initialize_class(
        &mut self,
        m: &mut Space,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_thread_run::Environment),
    ) -> Result<(), String> {
        let class = crate::native_stack::reg(m, crate::native_stack::Reg::Ecx);
        if !self.booted
            || !self
                .loader
                .files
                .iter()
                .any(|file| file.modules.iter().any(|raw| raw.class == class))
        {
            return Err("NativeVmLive: source-issued scripted class required".into());
        }
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        crate::native_vm_class_init::initialize_with_scripts(
            m,
            class,
            &mut self.fpu,
            &mut environment,
        )
    }
    pub fn create_instance(
        &mut self,
        m: &mut Space,
        e: &mut (impl crate::native_vm_namespace::FileEnvironment
                  + crate::native_vm_thread_run::Environment),
    ) -> Result<u32, String> {
        let class = crate::native_stack::reg(m, crate::native_stack::Reg::Ecx);
        if !self.booted
            || !self
                .loader
                .files
                .iter()
                .any(|file| file.modules.iter().any(|raw| raw.class == class))
        {
            return Err("NativeVmLive: source-issued instance class required".into());
        }
        let mut environment = WithLoader {
            loader: &mut self.loader,
            environment: e,
        };
        crate::native_vm_array_object::create_instance(m, class, &mut self.fpu, &mut environment)
    }
    pub fn register(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_pool::Allocator,
    ) -> Result<u32, String> {
        if !self.booted || crate::native_stack::reg(m, crate::native_stack::Reg::Ecx) != self.jvm {
            return Err("NativeVmLive: booted source JVM registration owner required".into());
        }
        crate::native_vm_metadata::registration::register(m, e)
    }
    pub fn prepare_registry(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_pool::Allocator,
    ) -> Result<crate::native_vm_metadata::registration::RegistryBootstrap, String> {
        if !self.booted || m.r32(0x63641c) != self.jvm {
            return Err("NativeVmLive: published source JVM registry owner required".into());
        }
        crate::native_vm_metadata::registration::begin_catalog(m, e)
    }
    pub fn bootstrap(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_namespace::FileEnvironment,
    ) -> Result<(), String> {
        use crate::native_stack as stack;
        use crate::native_vm_bootstrap as source;
        if self.booted
            || !stack::active(m)
            || !stack::has_pending_call(m)
            || stack::reg(m, stack::Reg::Ecx) != self.jvm
        {
            return Err("NativeVmLive: fresh actual bootstrap CALL required".into());
        }
        let descriptors = source::prepare_descriptors(m, self.jvm, e)?;
        let namespace = source::prepare_namespace_root(m, descriptors, e)?;
        let call = source::prepare_class_load(m, namespace, e)?;
        let mut first = self.loader.class(m, call, e)?;
        let call = source::prepare_second_class(m, &mut first)?;
        let mut second = self.loader.class(m, call, e)?;
        source::finish_bootstrap(m, &mut second)?;
        self.booted = true;
        Ok(())
    }
    pub fn apply(&mut self, m: &mut Space, entry: &Entry) -> Result<(), String> {
        if !self.booted {
            return Err("NativeVmLive: source bootstrap is not complete".into());
        }
        self.loader.apply(m, entry)
    }
    /// Consume this source projection's independent chronological reader.
    /// A recorder may acknowledge its own stream before or after this call;
    /// only successfully bound source events advance this reader. In
    /// particular an unsupported boundary remains pending for diagnosis.
    pub fn consume(
        &mut self,
        m: &mut Space,
        reader: &slr_vm::machine::Reader,
        limit: usize,
    ) -> Result<usize, String> {
        let mut consumed = 0;
        for entry in reader.pending(limit) {
            self.apply(m, &entry)?;
            reader
                .acknowledge(entry.sequence)
                .map_err(|error| format!("NativeVmLive: source acknowledgement: {error}"))?;
            consumed += 1;
        }
        Ok(consumed)
    }
    pub fn value_history(&self) -> Vec<ValueHistory> {
        self.loader.owners.value_history()
    }
    pub fn value(&self, m: &Space, id: slr_vm::values::CellId) -> Result<u32, String> {
        self.loader.owners.value(m, id)
    }
    pub fn frame(&self, m: &Space, identity: u64) -> Result<u32, String> {
        self.loader.owners.frame(m, identity)
    }
    pub fn static_field(&self, m: &Space, context: u32, index: usize) -> Result<u32, String> {
        self.loader
            .owners
            .static_field(m, context, index, &self.loader.metadata)
    }
    pub fn object(&self, m: &Space, word: u32) -> Result<u32, String> {
        self.loader.owners.object(m, word)
    }
    /// Inverse lookup of a live source-issued script allocation. Only an
    /// already acknowledged semantic ObjectAllocate can establish this owner.
    pub fn semantic_object(&self, m: &Space, address: u32) -> Result<u32, String> {
        self.loader.owners.semantic_object(m, address)
    }
    pub fn object_field(&self, m: &Space, word: u32, index: usize) -> Result<u32, String> {
        self.loader
            .owners
            .field(m, word, index, &self.loader.metadata)
    }
    pub fn script_frame_history(&self) -> &[ScriptFrameHistory] {
        self.loader.owners.script_frame_history()
    }
    pub fn owner_history(&self) -> Vec<OwnerHistory> {
        self.loader.owners.history()
    }
    pub fn metadata(&self) -> &Metadata {
        &self.loader.metadata
    }
    pub fn jvm(&self) -> u32 {
        self.jvm
    }
    pub fn fpu(&self) -> &crate::native_vm_fpu::State {
        &self.fpu
    }
    pub fn files(&self) -> &[IssuedFile] {
        &self.loader.files
    }
}
impl Loader {
    fn class(
        &mut self,
        m: &mut Space,
        call: crate::native_vm_bootstrap::PendingClassLoad,
        e: &mut impl crate::native_vm_namespace::FileEnvironment,
    ) -> Result<crate::native_vm_namespace::PendingClassResolution, String> {
        use crate::native_vm_namespace as source;
        let mut pending = source::begin_class_resolution(m, call, e)?;
        self.finish_class(m, &mut pending, e)?;
        Ok(pending)
    }
    fn finish_class(
        &mut self,
        m: &mut Space,
        pending: &mut crate::native_vm_namespace::PendingClassResolution,
        e: &mut impl crate::native_vm_namespace::FileEnvironment,
    ) -> Result<u32, String> {
        use crate::native_vm_namespace as source;
        source::prepare_class_cache(m, pending, e)?;
        let input = source::read_class_input(m, pending, e)?;
        let bytes = (0..input.bytes).map(|i| m.r8(input.source + i)).collect();
        let mut module = source::begin_module_load(m, self.jvm, &input, e)?;
        loop {
            source::deserialize_constants(m, &mut module, e)?;
            source::deserialize_members(m, &mut module, e)?;
            source::finish_module_tree(m, &mut module, e)?;
            if !module.has_next() {
                break;
            }
            source::begin_next_module(m, &mut module, e)?;
        }
        let class = source::finish_class_load(m, pending, &mut module, e)?;
        self.files.push(IssuedFile {
            bytes,
            modules: module.modules().to_vec(),
        });
        Ok(class)
    }
    /// Source class lookup consumes the caller's pending CALL, including its
    /// actual return PC. Loading extends the same owned JVM and issued inputs.
    pub fn class_lookup(
        &mut self,
        m: &mut Space,
        e: &mut impl crate::native_vm_namespace::FileEnvironment,
    ) -> Result<u32, String> {
        let name = crate::native_stack::argument(m, 0);
        match crate::native_vm_namespace::begin_class_lookup(m, self.jvm, name, e)? {
            crate::native_vm_namespace::ClassLookup::Cached(class) => Ok(class),
            crate::native_vm_namespace::ClassLookup::Loading(mut pending) => {
                self.finish_class(m, &mut pending, e)
            }
        }
    }
    /// Bind semantic metadata only to the next file actually issued by this
    /// source runtime. No parser/allocation body runs a second time.
    pub fn apply(&mut self, m: &mut Space, entry: &Entry) -> Result<(), String> {
        if !matches!(
            entry.event,
            Event::FileRead { .. } | Event::ModuleRead { .. } | Event::ClassRegister { .. }
        ) {
            return self.owners.apply(m, entry, &mut self.metadata);
        }
        let issued = self.files.get(self.next_file);
        let jvm = self.jvm;
        self.metadata
            .apply_loaded(m, entry, |m, _identity, bytes| {
                let issued =
                    issued.ok_or("NativeVmLive: FileRead has no next source-issued file")?;
                if bytes != issued.bytes
                    || issued.modules.is_empty()
                    || issued.modules.iter().any(|raw| {
                        m.r32(raw.module) != 0x5f08a0
                            || m.r32(raw.module + 0xc) != jvm
                            || m.r32(raw.class) != 0x5e7340
                            || m.r32(raw.class + 4) != raw.module
                    })
                {
                    return Err(
                        "NativeVmLive: FileRead differs from its next issued source owner".into(),
                    );
                }
                Ok(issued.modules.clone())
            })?;
        if matches!(entry.event, Event::FileRead { .. }) {
            self.next_file += 1;
        }
        Ok(())
    }
}
struct WithLoader<'a, E> {
    loader: &'a mut Loader,
    environment: &'a mut E,
}
impl<E: crate::native_vm_pool::Allocator> crate::native_vm_pool::Allocator for WithLoader<'_, E> {
    fn observe_context(
        &mut self,
        m: &Space,
        observation: crate::native_vm_context::Observation,
    ) -> Result<(), String> {
        self.loader.owners.observe(m, self.loader.jvm, observation)
    }
    fn observe_value(
        &mut self,
        m: &Space,
        observation: crate::native_vm_values::Observation,
    ) -> Result<(), String> {
        self.loader
            .owners
            .observe_value(m, self.loader.jvm, observation)
    }
    fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
        self.environment.allocate(m, bytes)
    }
    fn reallocate(
        &mut self,
        m: &mut Space,
        old: u32,
        old_bytes: u32,
        bytes: u32,
    ) -> Result<u32, String> {
        self.environment.reallocate(m, old, old_bytes, bytes)
    }
    fn deallocate(&mut self, m: &mut Space, pointer: u32) -> Result<(), String> {
        self.environment.deallocate(m, pointer)
    }
}
impl<E: crate::native_vm_namespace::FileEnvironment> crate::native_vm_lookup::Environment
    for WithLoader<'_, E>
{
    fn observe_owner(
        &mut self,
        m: &Space,
        observation: crate::native_vm_context::Observation,
    ) -> Result<(), String> {
        self.loader.owners.observe(m, self.loader.jvm, observation)
    }
    fn class_lookup(&mut self, m: &mut Space, jvm: u32, name: u32) -> Result<u32, String> {
        if jvm != self.loader.jvm || crate::native_stack::argument(m, 0) != name {
            return Err("NativeVmLive: class lookup owner or source argument differs".into());
        }
        self.loader.class_lookup(m, self.environment)
    }
}
impl<E: crate::native_vm_namespace::FileEnvironment + crate::native_vm_dispatch::Environment>
    crate::native_vm_dispatch::Environment for WithLoader<'_, E>
{
    fn free(&mut self, m: &mut Space, pointer: u32) -> Result<(), String> {
        crate::native_vm_dispatch::Environment::free(self.environment, m, pointer)
    }
}
impl<E: crate::native_vm_namespace::FileEnvironment + crate::native_vm_thread_run::Environment>
    crate::native_vm_thread_run::Environment for WithLoader<'_, E>
{
    fn resource(&mut self, m: &mut Space, package: u32, id: u32) -> Result<u32, String> {
        self.environment.resource(m, package, id)
    }
    fn query_frequency(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
        self.environment.query_frequency(m, destination)
    }
    fn query_counter(&mut self, m: &mut Space, destination: u32) -> Result<u32, String> {
        self.environment.query_counter(m, destination)
    }
    fn resolve_resource(
        &mut self,
        m: &mut Space,
        manager: u32,
        id: u32,
        kind: u32,
        flags: u32,
    ) -> Result<u32, String> {
        self.environment
            .resolve_resource(m, manager, id, kind, flags)
    }
    fn native(
        &mut self,
        m: &mut Space,
        fpu: &mut crate::native_vm_fpu::State,
        target: u32,
        info: u32,
        kind: crate::native_vm_abi::ReturnKind,
    ) -> Result<u32, String> {
        self.environment.native(m, fpu, target, info, kind)
    }
    fn observe(&mut self, m: &Space, fpu: &crate::native_vm_fpu::State, pc: u32) {
        self.environment.observe(m, fpu, pc);
    }
}
impl IssuedFile {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
    pub fn module(&self) -> LoadedModule {
        self.modules[0]
    }
    pub fn modules(&self) -> &[LoadedModule] {
        &self.modules
    }
}

#[derive(Clone, Copy)]
pub struct FileMemory {
    pub source: u32,
    pub bytes: u32,
    pub scratch: u32,
}
pub trait Host: crate::native_vm_pool::Allocator {
    fn file_memory(&mut self, m: &Space, identity: u64, bytes: &[u8])
        -> Result<FileMemory, String>;
}
struct Module {
    version: u32,
    chunks: Vec<([u8; 4], usize, usize)>,
    raw: LoadedModule,
    bodies: Vec<Vec<slr_fmt_tufa::Node>>,
    methods: [Vec<slr_fmt_tufa::MethodRec>; 2],
    fields: [Vec<slr_fmt_tufa::FieldRec>; 2],
}
struct File {
    modules: Vec<Module>,
    next: usize,
}
struct Binding {
    raw: LoadedModule,
    bodies: Vec<Vec<slr_fmt_tufa::Node>>,
    methods: [Vec<slr_fmt_tufa::MethodRec>; 2],
    fields: [Vec<slr_fmt_tufa::FieldRec>; 2],
    registered: bool,
}
struct Body {
    class: u32,
    tree_index: usize,
    address: u32,
    nodes: usize,
}
struct Field {
    address: u32,
    container: u32,
    index: usize,
    record: slr_fmt_tufa::FieldRec,
}
struct Method {
    address: u32,
    container: u32,
    index: usize,
    record: slr_fmt_tufa::MethodRec,
}
pub struct Metadata {
    jvm: u32,
    sequence: u64,
    files: HashMap<u64, File>,
    modules: HashMap<u64, Binding>,
    classes: HashMap<u32, LoadedModule>,
    bodies: HashMap<slr_vm::interp::BodyId, Body>,
    methods: HashMap<(u32, usize), Method>,
    fields: HashMap<(u32, bool, usize), Field>,
}
impl Metadata {
    pub fn new(m: &Space, jvm: u32) -> Result<Self, String> {
        if jvm == 0 || m.r32(jvm + 8) == 0 || m.r32(jvm + 0x68) == 0 {
            return Err("NativeVmLive: original JVM/cache/name owner is missing".into());
        }
        Ok(Self {
            jvm,
            sequence: 0,
            files: HashMap::new(),
            modules: HashMap::new(),
            classes: HashMap::new(),
            bodies: HashMap::new(),
            methods: HashMap::new(),
            fields: HashMap::new(),
        })
    }
    pub fn class(&self, identity: u32) -> Result<LoadedModule, String> {
        self.classes.get(&identity).copied().ok_or_else(|| {
            format!("NativeVmLive: class record {identity} has no causal raw binding")
        })
    }
    /// Resolve an issued TREE identity through its serialized module ordinal.
    /// Neither descriptor spelling nor payload/address arithmetic selects it.
    pub fn pc(&self, m: &Space, pc: slr_vm::interp::Pc) -> Result<u32, String> {
        let body = self
            .bodies
            .get(&pc.body)
            .ok_or("NativeVmLive: body has no causal raw binding")?;
        let class = self.class(body.class)?;
        let tree = m.r32(class.module + 0x60);
        if pc.idx >= body.nodes
            || body.tree_index >= m.r32(tree + 8) as usize
            || m.r32(m.r32(tree + 4) + body.tree_index as u32 * 4) != body.address
        {
            return Err("NativeVmLive: body position or original TREE owner changed".into());
        }
        body.address
            .checked_add(
                u32::try_from(pc.idx)
                    .map_err(|_| "NativeVmLive: node index exceeds u32")?
                    .checked_mul(8)
                    .ok_or("NativeVmLive: node offset overflow")?,
            )
            .ok_or_else(|| "NativeVmLive: node address overflow".into())
    }
    /// NativeContext's selected method ordinal refers to its pinned declaring
    /// record: serialized static methods first, then serialized instance methods.
    /// Cached signatures may change; the declared record and its owner may not.
    pub fn method(&self, m: &Space, class: u32, index: usize) -> Result<u32, String> {
        let method = self
            .methods
            .get(&(class, index))
            .ok_or("NativeVmLive: method has no causal raw binding")?;
        if method.index >= m.r32(method.container + 12) as usize
            || m.r32(m.r32(method.container + 8) + method.index as u32 * 4) != method.address
            || m.r32(method.address) != 0x5f08d0
            || method
                .record
                .iter()
                .enumerate()
                .any(|(i, word)| m.r32(method.address + 4 + i as u32 * 4) != *word)
        {
            return Err("NativeVmLive: selected method record or original owner changed".into());
        }
        Ok(method.address)
    }
    pub fn field(
        &self,
        m: &Space,
        class: u32,
        is_static: bool,
        index: usize,
    ) -> Result<u32, String> {
        let field = self
            .fields
            .get(&(class, is_static, index))
            .ok_or("NativeVmLive: field has no causal raw binding")?;
        if field.index >= m.r32(field.container + 12) as usize
            || m.r32(m.r32(field.container + 8) + field.index as u32 * 4) != field.address
            || m.r32(field.address) != 0x5f0890
            || field
                .record
                .iter()
                .enumerate()
                .any(|(i, word)| m.r32(field.address + 4 + i as u32 * 4) != *word)
        {
            return Err(
                "NativeVmLive: selected declaring field or its source owner changed".into(),
            );
        }
        Ok(field.address)
    }
    fn instance_field_from_raw(&self, m: &Space, class: u32, index: usize) -> Result<u32, String> {
        let mut matches = self
            .classes
            .iter()
            .filter(|(_, record)| record.class == class);
        let identity = *matches
            .next()
            .ok_or("NativeVmLive: declaring raw class has no issued binding")?
            .0;
        if matches.next().is_some() {
            return Err("NativeVmLive: declaring raw class has ambiguous bindings".into());
        }
        self.field(m, identity, false, index)
    }
    /// Apply exactly one chronological metadata event. A rejected event is
    /// never acknowledged or silently skipped; unrelated owners need their
    /// own source producer before this can be wired into production.
    pub fn apply(
        &mut self,
        m: &mut Space,
        entry: &Entry,
        host: &mut impl Host,
    ) -> Result<(), String> {
        let jvm = self.jvm;
        self.apply_loaded(m, entry, |m, identity, bytes| {
            let memory = host.file_memory(m, identity, bytes)?;
            if memory.source == 0
                || memory.scratch == 0
                || memory.bytes as usize != bytes.len()
                || memory.source.checked_add(memory.bytes).is_none()
                || bytes
                    .iter()
                    .enumerate()
                    .any(|(i, b)| m.r8(memory.source + i as u32) != *b)
            {
                return Err(
                    "NativeVmLive: FileRead bytes do not match their actual raw input owner".into(),
                );
            }
            loader::load_memory(m, jvm, memory.source, memory.bytes, memory.scratch, host)
        })
    }
    fn apply_loaded(
        &mut self,
        m: &mut Space,
        entry: &Entry,
        load: impl FnOnce(&mut Space, u64, &[u8]) -> Result<Vec<LoadedModule>, String>,
    ) -> Result<(), String> {
        if entry.sequence != self.sequence {
            return Err(format!(
                "NativeVmLive: expected sequence {}, received {}",
                self.sequence, entry.sequence
            ));
        }
        match &entry.event {
            Event::FileRead { identity, bytes } => {
                if self.files.contains_key(identity) {
                    return Err("NativeVmLive: repeated file identity".into());
                }
                let parsed = slr_fmt_tufa::parse(bytes)
                    .map_err(|e| format!("NativeVmLive: serialized source: {e}"))?;
                let loaded = load(m, *identity, bytes)?;
                if loaded.len() != parsed.len() {
                    return Err("NativeVmLive: raw/typed module count differs".into());
                }
                let modules = parsed
                    .iter()
                    .zip(loaded)
                    .map(|(module, raw)| {
                        Ok(Module {
                            version: module.version,
                            chunks: module
                                .chunks
                                .iter()
                                .map(|c| (c.tag, c.offset, c.size))
                                .collect(),
                            raw,
                            fields: module
                                .fields(bytes)
                                .map_err(|e| format!("NativeVmLive: serialized FILD: {e}"))?
                                .unwrap_or_default(),
                            methods: module
                                .methods(bytes)
                                .map_err(|e| format!("NativeVmLive: serialized MTHD: {e}"))?
                                .unwrap_or_default(),
                            bodies: module
                                .trees(bytes)
                                .map_err(|e| format!("NativeVmLive: serialized TREE: {e}"))?,
                        })
                    })
                    .collect::<Result<Vec<_>, String>>()?;
                self.files.insert(*identity, File { modules, next: 0 });
            }
            Event::ModuleRead {
                identity,
                file,
                version,
                chunks,
            } => {
                if self.modules.contains_key(identity) {
                    return Err("NativeVmLive: repeated module identity".into());
                }
                let source = self
                    .files
                    .get_mut(file)
                    .ok_or("NativeVmLive: ModuleRead precedes its FileRead")?;
                let module = source
                    .modules
                    .get(source.next)
                    .ok_or("NativeVmLive: module exceeds its physical source file")?;
                if module.version != *version || module.chunks != *chunks {
                    return Err(
                        "NativeVmLive: ModuleRead does not identify the next serialized module"
                            .into(),
                    );
                }
                self.modules.insert(
                    *identity,
                    Binding {
                        raw: module.raw,
                        bodies: module.bodies.clone(),
                        methods: module.methods.clone(),
                        fields: module.fields.clone(),
                        registered: false,
                    },
                );
                source.next += 1;
            }
            Event::ClassRegister {
                identity,
                module: Some(module),
                bodies,
            } => {
                if self.classes.contains_key(identity) {
                    return Err("NativeVmLive: repeated class identity".into());
                }
                let binding = self
                    .modules
                    .get_mut(module)
                    .ok_or("NativeVmLive: class registration precedes its physical module")?;
                if binding.registered {
                    return Err("NativeVmLive: physical module registered twice".into());
                }
                let tree = m.r32(binding.raw.module + 0x60);
                if bodies.len() != binding.bodies.len()
                    || tree == 0
                    || m.r32(tree + 8) as usize != bodies.len()
                {
                    return Err(
                        "NativeVmLive: class body inventory differs from serialized TREE".into(),
                    );
                }
                let mut issued = std::collections::HashSet::new();
                let mut pending = Vec::new();
                for (index, (body_identity, nodes)) in
                    bodies.iter().zip(&binding.bodies).enumerate()
                {
                    if self.bodies.contains_key(body_identity) || !issued.insert(*body_identity) {
                        return Err("NativeVmLive: repeated body identity".into());
                    }
                    let address = m.r32(m.r32(tree + 4) + index as u32 * 4);
                    if (address == 0) != nodes.is_empty() {
                        return Err("NativeVmLive: original TREE body allocation differs".into());
                    }
                    for (index, node) in nodes.iter().enumerate() {
                        let p = address + index as u32 * 8;
                        if m.r32(p) != (u32::from(node.opword) | (u32::from(node.operand) << 16))
                            || (slr_fmt_tufa::Node::has_imm(node.opword)
                                && m.r32(p + 4) != node.imm)
                        {
                            return Err(
                                "NativeVmLive: original TREE node differs from serialized source"
                                    .into(),
                            );
                        }
                    }
                    pending.push((
                        *body_identity,
                        Body {
                            class: *identity,
                            tree_index: index,
                            address,
                            nodes: nodes.len(),
                        },
                    ));
                }
                let mut fields = Vec::new();
                for ((offset, is_static), records) in [(0x54, true), (0x4c, false)]
                    .into_iter()
                    .zip(&binding.fields)
                {
                    let container = m.r32(binding.raw.module + offset);
                    if container == 0 || m.r32(container + 12) as usize != records.len() {
                        return Err(
                            "NativeVmLive: original field inventory differs from serialized FILD"
                                .into(),
                        );
                    }
                    for (index, record) in records.iter().enumerate() {
                        let address = m.r32(m.r32(container + 8) + index as u32 * 4);
                        if address == 0
                            || m.r32(address) != 0x5f0890
                            || record
                                .iter()
                                .enumerate()
                                .any(|(i, word)| m.r32(address + 4 + i as u32 * 4) != *word)
                        {
                            return Err(
                                "NativeVmLive: declaring field differs from serialized FILD".into(),
                            );
                        }
                        fields.push((
                            (*identity, is_static, index),
                            Field {
                                address,
                                container,
                                index,
                                record: *record,
                            },
                        ));
                    }
                }
                let mut methods = Vec::new();
                for (offset, records) in [0x58, 0x50].into_iter().zip(&binding.methods) {
                    let container = m.r32(binding.raw.module + offset);
                    if container == 0 || m.r32(container + 12) as usize != records.len() {
                        return Err(
                            "NativeVmLive: method inventory differs from serialized MTHD".into(),
                        );
                    }
                    for (index, record) in records.iter().enumerate() {
                        let address = m.r32(m.r32(container + 8) + index as u32 * 4);
                        if address == 0
                            || m.r32(address) != 0x5f08d0
                            || record
                                .iter()
                                .enumerate()
                                .any(|(i, word)| m.r32(address + 4 + i as u32 * 4) != *word)
                        {
                            return Err(
                                "NativeVmLive: method allocation differs from serialized MTHD"
                                    .into(),
                            );
                        }
                        methods.push(Method {
                            address,
                            container,
                            index,
                            record: *record,
                        });
                    }
                }
                for (key, field) in fields {
                    self.fields.insert(key, field);
                }
                for (index, method) in methods.into_iter().enumerate() {
                    self.methods.insert((*identity, index), method);
                }
                for (identity, body) in pending {
                    self.bodies.insert(identity, body);
                }
                binding.registered = true;
                self.classes.insert(*identity, binding.raw);
            }
            _ => {
                return Err(format!(
                    "NativeVmLive: source owner continuation unavailable at sequence {}",
                    entry.sequence
                ))
            }
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("NativeVmLive: sequence exhausted")?;
        Ok(())
    }
}

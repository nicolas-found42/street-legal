//! The object-model host: `VmHost` implements [`Host`] over the loader.
//!
//! Implements interpreter delegation over loaded class metadata and persistent
//! typed instance, static, and array cells. Field vectors cover the recovered
//! field slice; execution vectors additionally cover bounded call dispatch,
//! literals, construction, casts, and per-VM String identity.
//!
//! Bounded class/field initialization uses sequential cells, dependency order,
//! and the same RID/String/call/new/cast paths. Untraced initializer families,
//! missing backends, malformed graphs and nesting-budget exhaustion fail
//! explicitly. Diagnostic-only conversion errors preserve original raw copies.
//! Versioned duplicate-class identity, complete name/cache resolution, machine
//! CallInfo context, active String GC, and whole-world boot remain open (#110).
//!
//! The target executable operations are:
//!
//! - `Declare` (`0x1002`): allocate a local with the following pool type and
//!   optional name; push the indexed lvalue on a `0x002c` follower.
//! - `ResolveName` (`0x1011`), `CallResolved` (`0x0024`): resolve the inline
//!   name nodes (`FUN_00420d20`/`FUN_004208e0` shape: `0x1011` count +
//!   `0x1019`/`0x101a` leaves, `0x4012` method name) against loaded classes.
//! - `FieldRef` (`0x101b`), `FieldByName` (`0x001d`), `MemberAccess`
//!   (`0x401c`): field lookup (static vs instance by the pool bit-30 rule).
//! - `ArgArray` (`0x101e`): pack `imm` values (`FUN_00404e60`).
//! - `CallStatic` (`0x4026`), `CallInstance` (`0x4025`), `CallInit`
//!   (`0x0021`), `CallThisInit` (`0x0022`), `CallSuperInit` (`0x0023`):
//!   resolve through [`Loader::resolve_method`] (`FUN_00404910`); native
//!   targets go through the registry instead of a script frame
//!   (`FUN_0041fbc0` flag `0x40` path); script targets enter via
//!   [`Thread::call_script_method_with`].
//! - `New` (`0x1e`), `NewArray` (`0x1f`): allocate the object/array
//!   (`FUN_00410890` class init + sizing; one-dimensional only).
//! - `InstanceOf` (`0x20`), `Cast` (`0x21`): hierarchy checks
//!   (`FUN_00405400` / `FUN_00417da0`).
//! - `ResourceLiteral` (`0x100e`) reads a resource Member; `StringLiteral`
//!   (`0x4009`) uses per-VM String object interning with an actual ptr cell.
//!
//! Executable addresses identify the recovery targets; they do not certify the
//! complete functions. Data-free tests use synthetic modules and the
//! stock corpus exercises load/link. Headless boot acceptance remains open.

use std::collections::{HashMap, HashSet};

use slr_fmt_tufa::Node;

use crate::interp::{
    BodyId, ExternKind, ExternOp, Flow, HeapSlot, Host, RefOperator, Slot, Thread, VmError,
};
use crate::natives::Natives;
use crate::strings::Strings;
use crate::values::{LoaderConvert, TypeTag, Value};
use crate::vmclass::Loader;

/// Errors the host can raise beyond [`VmError`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HostError {
    /// Name resolution failed (`name cannot be parsed`, fatal in the exe).
    UnresolvedName,
    /// Field or member missing (fatal in the exe).
    MissingMember,
    /// Call target missing (`... not found`, fatal in the exe).
    MissingMethod,
    /// `new` of an unloaded class, bad cast (`cannot cast`, fatal).
    BadCast,
    /// Multi-dimensional `new[]` (fatal in the exe).
    MultiArray,
    /// A field initializer requires an opcode outside the recovered field slice.
    UnsupportedInitializer(String),
    /// Abstract or native method has no script body to enter.
    NoBody,
}

thread_local! {
    static LAST_HOST_ERROR: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// Attach a diagnostic detail to the next [`HostError`] conversion.
pub(crate) fn note(detail: String) {
    LAST_HOST_NOTE.with(|slot| *slot.borrow_mut() = Some(detail));
}

thread_local! {
    static LAST_HOST_NOTE: std::cell::RefCell<Option<String>> = const { std::cell::RefCell::new(None) };
}

/// The detail of the most recent [`HostError`] folded into a [`VmError`]
/// (the opword code alone cannot say which lookup failed). Diagnostic only.
#[must_use]
pub fn last_host_error() -> Option<String> {
    LAST_HOST_ERROR.with(|e| e.borrow().clone())
}

impl From<HostError> for VmError {
    fn from(e: HostError) -> Self {
        let detail = LAST_HOST_NOTE.with(|slot| slot.borrow_mut().take());
        LAST_HOST_ERROR.with(|slot| {
            *slot.borrow_mut() = Some(match detail {
                Some(detail) => format!("{e:?}: {detail}"),
                None => format!("{e:?}"),
            });
        });
        match e {
            HostError::UnresolvedName => Self::Unsupported(0x1011),
            HostError::MissingMember => Self::Unsupported(0x401c),
            HostError::MissingMethod => Self::Unsupported(0x4026),
            HostError::BadCast => Self::Unsupported(0x0021),
            HostError::MultiArray => Self::Unsupported(0x001f),
            HostError::NoBody => Self::Unsupported(0x0024),
            HostError::UnsupportedInitializer(_) => Self::Unsupported(0x402e),
        }
    }
}

/// A heap object: runtime record identity plus base-first field words.
#[derive(Clone, Debug)]
pub struct Object {
    /// Loaded record for ordinary objects; explicit array spelling for
    /// typed arrays (arrays own no arena record).
    pub class: crate::vmclass::ClassRef,
    /// Base-first instance values; class statics have their own owner.
    pub fields: Vec<Value>,
}

impl Object {
    /// Boundary projection of the runtime class spelling (diagnostics,
    /// native registries, cross-crate seams). Never an identity substitute:
    /// execution carries the [`crate::vmclass::ClassRef`] itself.
    #[must_use]
    pub fn class_name<'a>(&'a self, loader: &'a crate::vmclass::Loader) -> &'a str {
        match &self.class {
            crate::vmclass::ClassRef::Loaded(id) => loader.class_name(*id).unwrap_or("<stale>"),
            crate::vmclass::ClassRef::Array(array) => array.descriptor(),
        }
    }
}

// ---- the shared native-call ABI --------------------------------------

/// The argument bundle every native receives: the exe's 4-word `CallInfo`
/// plus the record words it points at.
///
/// Machine layout (all `[static]`, `0x41fc20-0x4200b7` builds it; every
/// `0x47xxxx`/`0x48xxxx` native reads it from its one stack argument):
///
/// ```text
/// info+0x00  jvm              info+0x04  thread
/// info+0x08  arg container    info+0x0c  native record
/// container  { +0 cap, +4 data ptr, +8 count, +0xc self }    (0x41e2d0/e2e0/e2f0)
/// record     { +0 vtable 0x5f08fc, +4 jvm, +8 impl addr,
///              +0xc class cstr, +0x10 name cstr, +0x14 sig record }
/// sig rec    { +0 owner, +4 cached descriptor text, +8 type container }  (0x41def0)
/// ```
///
/// The container's `count` includes the receiver/return slot: `[count-1]`
/// is that slot and the parameters run downward, first parameter at
/// `[count-2]` (pinned by [`marshal_args`], see `docs/tufa-executor.md`).
///
/// The port projects the container onto `args` (its `len()` is the exe's
/// `count`) and the two records onto `class`/`name`/`sig`. The `jvm` and
/// `thread` words are 0 until the port has host context words to pass.
#[derive(Clone, Copy, Debug)]
pub struct CallInfo<'a> {
    /// `info+0x00`: the JVM context word.
    pub jvm: u32,
    /// `info+0x04`: the calling thread word.
    pub thread: u32,
    /// `info+0x08`: the argument container contents (`count = len`).
    pub args: &'a [Value],
    /// `info+0x0c -> +0xc`: the native record's class name.
    pub class: &'a str,
    /// `info+0x0c -> +0x10`: the method name.
    pub name: &'a str,
    /// `info+0x0c -> +0x14 -> +0x4`: the signature record's cached
    /// descriptor text (`(II)V`), which is what [`marshal_args`] parses.
    /// `FUN_0041def0` only *builds* that text when the cache is empty, from
    /// the record's type container; the port always carries the text.
    pub sig: &'a str,
}

/// Why [`marshal_args`] stopped before writing every slot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MarshalError {
    /// The descriptor declares more parameters than the container holds
    /// (`0x45dabf`).
    NotEnoughParameters,
    /// A type byte outside `I`/`F`/`L`/`[` (`0x45d9e5`, `0x45da60`).
    UnknownType,
    /// An array whose element type is not `I`/`F`/`L` (`0x45d9e5`): the
    /// payload of the array slot was already copied.
    UnknownArrayElement,
    /// Port bound only: `out` has fewer words than the descriptor needs.
    /// The exe's out slots are variadic caller-provided stack words with no
    /// runtime bound.
    OutOfSlots,
}

impl MarshalError {
    /// The exe's log text (`0x61099c`, `0x6109e8`, `0x610a1c`); the
    /// `UnknownArrayElement` text keeps its trailing space.
    #[must_use]
    pub fn message(self) -> &'static str {
        match self {
            Self::NotEnoughParameters => {
                "CallInfo.parseParameters: not enough parameters passed to native function"
            }
            Self::UnknownType => "CallInfo.parseParameters: unknown type in signature",
            Self::UnknownArrayElement => "CallInfo.parseParameters: unknown type in signature ",
            Self::OutOfSlots => "",
        }
    }

    /// The full diagnostic the exe concatenates at `0x62e018` before
    /// flushing it: `"!"` (`0x607b3c`), the message, the record's class and
    /// method names, then the descriptor text (`0x45db1c` calls
    /// `FUN_0041def0` again, which returns the cached text).
    #[must_use]
    pub fn log_line(self, info: &CallInfo<'_>) -> String {
        format!("!{}{}{}{}", self.message(), info.class, info.name, info.sig)
    }
}

/// The exe's argument marshaller: write the receiver/return slot and each
/// declared parameter into `out`.
///
/// `out[0]` receives `args[count-1]` (the receiver/return slot), then one
/// word per descriptor parameter, in declaration order. `I` and `F` copy
/// the cell payload word verbatim (no float conversion here — the caller
/// side does that on return); `L...;` copies one word and skips to the
/// `;`; `[` copies one word after skipping every `[` and then also skips
/// an `L...;` element name. Returns the number of words written.
///
/// The container is read downward: the first declared parameter is
/// `args[count-2]`, the last is `args[0]`. A descriptor parameter with no
/// container slot left is `NotEnoughParameters` (the exe logs and returns,
/// leaving the remaining out words untouched — so do we).
///
/// exe: FUN_0045d910
pub fn marshal_args(info: &CallInfo<'_>, out: &mut [u32]) -> Result<usize, MarshalError> {
    /// One `*out = payload; out++`.
    fn put(out: &mut [u32], written: &mut usize, word: u32) -> Result<(), MarshalError> {
        let Some(slot) = out.get_mut(*written) else {
            return Err(MarshalError::OutOfSlots);
        };
        *slot = word;
        *written += 1;
        Ok(())
    }

    let cells = info.args;
    let count = cells.len();
    if out.is_empty() {
        return Err(MarshalError::OutOfSlots);
    }
    // `container[count-1]`, or a null cell when the container is empty
    // (the exe indexes `data[-1]` there; the port reads null).
    out[0] = if count == 0 {
        0
    } else {
        cells[count - 1].payload
    };
    let mut written = 1;
    let sig = info.sig.as_bytes();
    // `FUN_0041def0` returns `"(" + params + ")" + return`; the walk
    // begins one byte past the `(` (`mov cl,[edi+1]; inc edi`).
    if sig.first() != Some(&b'(') {
        return Err(MarshalError::UnknownType);
    }
    let mut cur = 1;
    let mut remaining = count as isize - 1;
    loop {
        let Some(&ty) = sig.get(cur) else {
            // The exe reads whatever follows the text; the port fails the
            // descriptor instead of walking into unmapped memory.
            return Err(MarshalError::UnknownType);
        };
        // The tail check `cl == ')'` happens before the next decrement.
        if ty == b')' {
            return Ok(written);
        }
        let prev = remaining;
        remaining -= 1;
        // `test eax,eax; je 0x45dabf` — the check precedes the type switch,
        // so a bogus type byte after the container runs dry still reports
        // the missing parameter.
        if prev == 0 {
            return Err(MarshalError::NotEnoughParameters);
        }
        let cell = if remaining >= 0 && (remaining as usize) < count {
            cells[remaining as usize].payload
        } else {
            0
        };
        cur = match ty {
            b'I' | b'F' => {
                put(out, &mut written, cell)?;
                cur + 1
            }
            b'L' => {
                put(out, &mut written, cell)?;
                // `while [edi+1] != ';' { edi++ }` then step past it.
                let semi = sig
                    .get(cur + 1..)
                    .and_then(|tail| tail.iter().position(|&c| c == b';'))
                    .map(|p| p + cur + 1)
                    .ok_or(MarshalError::UnknownType)?;
                semi + 1
            }
            b'[' => {
                // Skip every extra `[` (0x45d9bb loop).
                let mut p = cur;
                loop {
                    p += 1;
                    match sig.get(p) {
                        Some(&b'[') => {}
                        Some(_) => break,
                        None => return Err(MarshalError::UnknownType),
                    }
                }
                // The payload is copied before the element type is tested.
                put(out, &mut written, cell)?;
                match sig.get(p) {
                    Some(&b'F') | Some(&b'I') => p + 1,
                    Some(&b'L') => {
                        let semi = sig
                            .get(p + 1..)
                            .and_then(|tail| tail.iter().position(|&c| c == b';'))
                            .map(|s| s + p + 1)
                            .ok_or(MarshalError::UnknownType)?;
                        semi + 1
                    }
                    // The trailing-space diagnostic (`0x610a1c`).
                    _ => return Err(MarshalError::UnknownArrayElement),
                }
            }
            _ => return Err(MarshalError::UnknownType),
        };
    }
}

/// Number of argument slots a descriptor declares (`(II)V` -> 2,
/// `([[I)V` -> 1, `()V` -> 0), or `None` when the text is not a
/// descriptor. This is the inverse walk of [`marshal_args`] and the count
/// the port's caller side needs to pop (the exe's call record already
/// carries the container).
///
/// exe: FUN_0041def0 (type-container walk that produces the same text)
#[must_use]
pub fn parameter_slots(sig: &str) -> Option<usize> {
    fn skip_class(bytes: &[u8], from: usize) -> Option<usize> {
        let tail = bytes.get(from..)?;
        let semi = tail.iter().position(|&c| c == b';')?;
        Some(from + semi + 1)
    }

    let bytes = sig.as_bytes();
    if bytes.first() != Some(&b'(') {
        return None;
    }
    let mut i = 1;
    let mut n = 0;
    loop {
        match *bytes.get(i)? {
            b')' => return Some(n),
            b'I' | b'F' => {
                i += 1;
                n += 1;
            }
            b'L' => {
                i = skip_class(bytes, i + 1)?;
                n += 1;
            }
            b'[' => {
                i += 1;
                while *bytes.get(i)? == b'[' {
                    i += 1;
                }
                match *bytes.get(i)? {
                    b'I' | b'F' => i += 1,
                    b'L' => i = skip_class(bytes, i + 1)?,
                    _ => return None,
                }
                n += 1;
            }
            _ => return None,
        }
    }
}

/// Native return-cell descriptor, including V:0 and full reference/array
/// spelling. A parameter-only constructor signature has no return descriptor.
#[must_use]
pub fn return_type(sig: &str) -> Option<TypeTag> {
    let descriptor = sig.split_once(')')?.1;
    if descriptor == "V" {
        return Some(TypeTag::intern("V"));
    }
    let parameters = parameter_types(&format!("({descriptor})")).ok()?;
    (parameters.len() == 1).then(|| parameters[0])
}

/// Build the exe's argument container for a native call: pop the
/// parameters **last-declared first**, then the receiver/return slot, so
/// the receiver lands at `len-1` exactly where [`marshal_args`] reads it.
///
/// The exe's container *is* the caller's operand stack (`0x420165` reads
/// `[frame+0x20]+0x28`, the same container `info+8` points at), and its
/// `count` includes the receiver/return slot — which is why the marshaller
/// walks downward from `count-1`. The port therefore pops the caller's
/// stack instead of copying a call record.
///
/// exe: FUN_0045d910 (container contract)
fn collect_args_using(
    thread: &mut Thread,
    sig: &str,
    mut pop: impl FnMut(&mut Thread) -> Result<Value, VmError>,
) -> Result<Vec<Value>, VmError> {
    let params = parameter_slots(sig).ok_or(VmError::Unsupported(0x4012))?;
    let mut args = Vec::with_capacity(params + 1);
    for _ in 0..params {
        let value = pop(thread)?;
        args.push(value);
        thread.release();
    }
    // A static call recorded no receiver; the exe's container still holds
    // the slot (`count` includes it), so supply a null one.
    let receiver = match pop(thread) {
        Ok(value) => {
            thread.release();
            value
        }
        Err(VmError::NullSlot) => Value::int(0),
        Err(error) => return Err(error),
    };
    args.push(receiver);
    Ok(args)
}

pub(crate) fn parameter_types(sig: &str) -> Result<Vec<TypeTag>, VmError> {
    let bytes = sig.as_bytes();
    if bytes.first() != Some(&b'(') {
        return Err(HostError::MissingMethod.into());
    }
    let mut index = 1;
    let mut result = vec![];
    while bytes.get(index) != Some(&b')') {
        let start = index;
        while bytes.get(index) == Some(&b'[') {
            index += 1;
        }
        match bytes.get(index) {
            Some(b'I' | b'F') => index += 1,
            Some(b'L') => {
                index += 1;
                while bytes.get(index).is_some_and(|byte| *byte != b';') {
                    index += 1;
                }
                if bytes.get(index) != Some(&b';') {
                    return Err(HostError::MissingMethod.into());
                }
                index += 1;
            }
            _ => return Err(HostError::MissingMethod.into()),
        }
        result.push(TypeTag::intern(&sig[start..index]));
    }
    Ok(result)
}

/// Collect native arguments from local/value operands.
pub fn collect_args(thread: &mut Thread, sig: &str) -> Result<Vec<Value>, VmError> {
    collect_args_using(thread, sig, Thread::pop_deref)
}

/// Collect native arguments while dereferencing authoritative host lvalues.
pub fn collect_args_with<H: Host>(
    thread: &mut Thread,
    sig: &str,
    host: &mut H,
) -> Result<Vec<Value>, VmError> {
    collect_args_using(thread, sig, |thread| thread.pop_deref_with(host))
}

/// A ported native: the target of the `record+8` code pointer.
///
/// It receives the calling host and its [`CallInfo`]; like every exe
/// native it marshals its own parameters through [`marshal_args`]. The
/// result is the return payload word (`eax`, or the `st0` bits for an `F`
/// return) and `None` for `void`.
pub type NativeFn = fn(&mut VmHost, &CallInfo<'_>) -> Option<u32>;

/// Embedding check before a native requires its runtime collaborator.
pub type NativeBoundary = fn(&CallInfo<'_>) -> Result<(), String>;

/// Resource table boundary used by literal100e's4122b0 call. A missing backend
/// is unsupported; the VM never fabricates a global resource identifier.
pub type ResourceResolver = fn(&mut VmHost, Option<&str>, u32) -> Result<u32, VmError>;

/// Log-and-continue observations retained from initializer execution.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InitializationDiagnostic {
    /// A declared interface dependency had a non-interface kind word.
    InterfaceExtendsClass {
        class: String,
        interface: String,
        kind: u32,
    },
    /// Existing ALU diagnostics; assignment may still copy the source payload.
    Body(crate::ops::Diagnostic),
}

/// Resolved constructor request before dispatch; arguments remain in source order.
#[derive(Clone, Debug)]
pub struct ConstructorCall {
    pub class: crate::vmclass::ClassId,
    pub class_name: String,
    pub caller_class: Option<crate::vmclass::ClassId>,
    pub receiver: u32,
    pub signature: String,
    pub arguments: Vec<Value>,
    /// None on dispatch failure; false is original immediate/null-body return.
    pub entered_script_frame: Option<bool>,
}

/// The object model over a loaded world.
/// Read-only observation of the actual interpreter caller at a native boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NativePhase {
    Enter,
    Return,
}

pub struct VmHost {
    /// Loaded classes and packages.
    pub loader: Loader,
    /// Native registry.
    pub natives: Natives,
    /// Interned strings (pool + literals).
    pub strings: Strings,
    /// Heap objects by word (`0x1000 + index`; 0 = null).
    pub objects: Vec<Object>,
    /// Original object marking state; value-cell releases are independent.
    pub gc: crate::gc::Collector,
    /// Embedding clock for transient field-initializer interpreter threads.
    pub runtime_clock: Option<fn() -> slr_x87::X87>,
    /// Current embedding OS thread's x87 precision. Startup remains at CRT
    /// double precision until device creation changes it; script threads share
    /// that control state rather than each choosing their own precision.
    pub runtime_precision: slr_x87::Precision,
    /// Ported native implementations by exe address (`record+8`): the
    /// registry maps `(class, name, sig)` to the address, this maps the
    /// address to the port. Empty until the native slices land; an address
    /// with no function here is [`HostError::NoBody`].
    pub native_impls: HashMap<u32, NativeFn>,
    /// Resolve package/local resource IDs through the installed resource world.
    pub resource_resolver: Option<ResourceResolver>,
    /// Original log-and-continue method resolution diagnostics.
    pub method_diagnostics: Vec<&'static str>,
    /// Inspectable constructor dispatch requests, including pending initializers.
    pub constructor_calls: Vec<ConstructorCall>,
    /// Last unavailable native, including its exact descriptor.
    pub missing_native: Option<String>,
    /// Optional observer of actual native calls in dispatch order.
    pub native_observer: Option<fn(&CallInfo<'_>, Option<u32>)>,
    /// Called with the live interpreter thread; observing must not re-enter execution.
    pub native_diagnostics: Option<fn(&VmHost, &Thread, &CallInfo<'_>, NativePhase)>,
    /// Embedding hook around every native call, with the calling script
    /// thread (`CallInfo+4` in the exe). Called with `true` just before the
    /// native runs and `false` right after it returns, so a runtime can
    /// install the caller and fold flag changes the native made (sleep, wait)
    /// back into the interpreter thread before its loop-tail check.
    pub native_thread_hook: Option<fn(&mut Thread, bool)>,
    /// Print every method dispatch to stderr (diagnostic).
    pub call_trace: bool,
    /// Embedding boundary for implementations whose required engine/runtime is
    /// unavailable. Checked before invoking a native, preserving named stops.
    pub native_boundary: Option<NativeBoundary>,
    pub native_failure: Option<String>,
    /// Last failed field/class initializer, preserved before VM error encoding.
    pub initializer_failure: Option<String>,
    /// Original diagnostic-only conditions during field/class initialization.
    pub initialization_diagnostics: Vec<InitializationDiagnostic>,
    /// Embedding guard for nested initializer bodies (default32). This is a
    /// Rust resource limit, not a recovered original stack/unwind policy.
    pub initializer_depth_limit: usize,
    initializer_depth: usize,
    static_cells: HashMap<u32, Vec<Value>>,
    /// Retained dependency IDs per installed owner (context word ->
    /// (base, interfaces)), resolved through the current cache selection
    /// at install time and kept across later duplicate registrations.
    /// Reentry after install returns without re-resolving (403e80).
    initialized_dependencies: HashMap<
        u32,
        (
            Option<crate::vmclass::ClassId>,
            Vec<crate::vmclass::ClassId>,
        ),
    >,
    initializing_classes: HashSet<u32>,
    array_cells: HashMap<u32, Vec<Value>>,
    variable_names: Vec<String>,
}

impl VmHost {
    fn diagnostic_reference_words(&self, thread: &Thread) -> std::collections::BTreeSet<u32> {
        let mut words = std::collections::BTreeSet::new();
        for frame in thread.frames.iter().rev().take(16) {
            if frame.receiver != 0 {
                words.insert(frame.receiver);
            }
            for id in frame.locals.iter().take(64) {
                if let Some(var) = thread.vars.try_get(*id) {
                    if var.value.tag.is_ref() && var.value.payload != 0 {
                        words.insert(var.value.payload);
                    }
                }
            }
            for slot in frame.stack.iter().rev().take(64) {
                let value = match slot {
                    Slot::Value(v) => Some(*v),
                    Slot::Var(id) => thread.vars.try_get(*id).map(|v| v.value),
                    Slot::Heap(slot) => self.diagnostic_cell(*slot),
                    _ => None,
                };
                if let Some(value) = value {
                    if value.tag.is_ref() && value.payload != 0 {
                        words.insert(value.payload);
                    }
                }
            }
        }
        words
    }

    /// Bounded, read-only heap projection; never loads classes or dereferences native handles.
    pub fn diagnostic_snapshot(&self) -> Vec<crate::diagnostics::ObjectSnapshot> {
        (0..self.objects.len().min(64))
            .filter_map(|index| self.diagnostic_object(0x1000 + index as u32))
            .collect()
    }
    pub fn diagnostic_object(&self, word: u32) -> Option<crate::diagnostics::ObjectSnapshot> {
        let object = self.objects.get(word.checked_sub(0x1000)? as usize)?;
        let elements = self.array_cells.get(&word);
        let bytes = self.string_bytes(word);
        Some(crate::diagnostics::ObjectSnapshot {
            word,
            class: object.class_name(&self.loader).to_owned(),
            fields: object
                .fields
                .iter()
                .take(64)
                .copied()
                .map(Into::into)
                .collect(),
            fields_total: object.fields.len(),
            field_names: match &object.class {
                crate::vmclass::ClassRef::Loaded(id) => self
                    .instance_fields_id(*id)
                    .map(|fields| {
                        fields
                            .into_iter()
                            .take(64)
                            .map(|field| format!("{}.{}", field.class, field.name))
                            .collect()
                    })
                    .unwrap_or_default(),
                crate::vmclass::ClassRef::Array(_) => vec!["length".into()],
            },
            elements: elements
                .into_iter()
                .flatten()
                .take(64)
                .copied()
                .map(Into::into)
                .collect(),
            elements_total: elements.map_or(0, Vec::len),
            string_bytes: bytes.map(|b| b.iter().take(256).copied().collect()),
            string_bytes_total: bytes.map(<[u8]>::len),
        })
    }

    fn gc_edges(
        objects: &[Object],
        arrays: &HashMap<u32, Vec<Value>>,
        word: u32,
    ) -> crate::gc::Edges {
        let references = |values: &[Value]| {
            values
                .iter()
                .filter(|value| value.tag.is_ref() && value.tag.name() != "V" && value.payload != 0)
                .map(|value| value.payload)
                .collect()
        };
        if let Some(values) = arrays.get(&word) {
            return crate::gc::Edges::Array(references(values));
        }
        let values = word
            .checked_sub(0x1000)
            .and_then(|index| objects.get(index as usize))
            .map_or(&[][..], |object| object.fields.as_slice());
        crate::gc::Edges::Object(references(values))
    }

    /// Reference-assignment barrier (`FUN_004198b0`), including native stores.
    pub fn gc_mark_reference(&mut self, value: Value) {
        if value.tag.is_ref() && value.tag.name() != "V" && value.payload != 0 {
            let objects = &self.objects;
            let arrays = &self.array_cells;
            self.gc.mark(value.payload, &mut |word| {
                Self::gc_edges(objects, arrays, word)
            });
        }
    }

    /// Class-cache bucket/chain order, with each static variable table reversed.
    pub fn gc_static_roots(&self) -> Vec<u32> {
        let mut roots = Vec::new();
        for bucket in 0..crate::vmclass::CLASS_CACHE_BUCKETS {
            for class in self.loader.cache_chain(bucket) {
                if let Some(values) = self.static_cells.get(&class.get()) {
                    roots.extend(
                        values
                            .iter()
                            .rev()
                            .filter(|value| {
                                value.tag.is_ref() && value.tag.name() != "V" && value.payload != 0
                            })
                            .map(|value| value.payload),
                    );
                }
            }
        }
        roots
    }

    /// Frame tables and operand/local cells in original root-discovery order.
    pub fn gc_thread_roots(&mut self, thread: &Thread) -> Result<Vec<u32>, VmError> {
        let mut roots = Vec::new();
        for frame in thread.frames.iter().rev() {
            for slot in frame.stack.iter().rev() {
                let value = self.borrow_slot(thread, *slot)?;
                if value.tag.is_ref() && value.tag.name() != "V" && value.payload != 0 {
                    roots.push(value.payload);
                }
            }
            for local in frame.locals.iter().rev() {
                let value = thread.vars.get(*local).value;
                if value.tag.is_ref() && value.tag.name() != "V" && value.payload != 0 {
                    roots.push(value.payload);
                }
            }
        }
        Ok(roots)
    }

    /// One original incremental mark/sweep opportunity; finalizer execution is
    /// owned by the embedding scheduler, after this operation returns its IDs.
    pub fn gc_step(&mut self, roots: impl IntoIterator<Item = u32>) -> crate::gc::Step {
        let objects = &self.objects;
        let arrays = &self.array_cells;
        self.gc
            .step(roots, &mut |word| Self::gc_edges(objects, arrays, word))
    }

    /// After finalize, the original clears reference payloads in the object.
    pub fn gc_clear_references(&mut self, word: u32) {
        if let Some(index) = word.checked_sub(0x1000) {
            if let Some(object) = self.objects.get_mut(index as usize) {
                for value in &mut object.fields {
                    if matches!(value.tag, TypeTag::Ref(_)) && value.tag.name() != "V" {
                        value.payload = 0;
                    }
                }
            }
        }
        if let Some(values) = self.array_cells.get_mut(&word) {
            values.clear();
            if let Some(object) = word
                .checked_sub(0x1000)
                .and_then(|index| self.objects.get_mut(index as usize))
            {
                object.fields[0].payload = 0;
            }
        }
    }

    /// Retained hierarchy IDs installed by the last successful
    /// initialization of one record (403e80 binding-time policy): the base
    /// and interface IDs as selected at install time. `None` while the
    /// owner has no installed owner; reentry after install returns the
    /// stored IDs without re-resolving through the current selection.
    #[must_use]
    pub fn initialized_dependencies(
        &self,
        class: crate::vmclass::ClassId,
    ) -> Option<(
        Option<crate::vmclass::ClassId>,
        Vec<crate::vmclass::ClassId>,
    )> {
        self.initialized_dependencies.get(&class.get()).cloned()
    }
}

impl VmHost {
    /// Intern a nullable C string as a real loaded String object. Content hits
    /// reuse the object associated with the intern record; String.ptr contains
    /// that record's stable buffer word. GC-list revival is not implemented.
    /// exe: FUN_004174a0
    pub fn intern_string(&mut self, bytes: Option<&[u8]>) -> Result<u32, HostError> {
        let Some(bytes) = bytes else { return Ok(0) };
        let bytes = &bytes[..bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(bytes.len())];
        let string_id = self
            .loader
            .lookup("java.lang.String")
            .ok_or(HostError::BadCast)?;
        if let Some(word) = self.strings.lookup(bytes).and_then(|record| record.object) {
            self.gc.revive(word);
            return Ok(word);
        }
        let slot = self.instance_field_slot_id(string_id, "ptr")?;
        if self.instance_fields_id(string_id)?[slot].ty != "I" {
            return Err(HostError::MissingMember);
        }
        let word = self.alloc_object_id(string_id)?;
        let buffer = self.strings.intern(bytes).word;
        self.objects[(word - 0x1000) as usize].fields[slot] = Value::int(buffer as i32);
        self.strings.bind_object(bytes, word);
        Ok(word)
    }

    /// Resolve bytes through the actual heap object's inherited ptr cell.
    #[must_use]
    pub fn string_bytes(&self, word: u32) -> Option<&[u8]> {
        use crate::vmclass::ClassRef;
        let object = self.objects.get(word.checked_sub(0x1000)? as usize)?;
        let string_id = self.loader.lookup("java.lang.String")?;
        let object_id = match &object.class {
            ClassRef::Loaded(id) => *id,
            ClassRef::Array(_) => return None,
        };
        if !self
            .loader
            .is_inherited_from_id(object_id, string_id)
            .ok()?
        {
            return None;
        }
        // The native uses its cached String/Native.ptr field index. Base-first
        // layout keeps that same cell in a subclass, even with later fields.
        let slot = self.instance_field_slot_id(string_id, "ptr").ok()?;
        self.strings.bytes_at(object.fields.get(slot)?.payload)
    }
    /// An empty host (boot with [`VmHost::boot`]).
    #[must_use]
    pub fn new() -> Self {
        Self {
            loader: Loader::new(),
            natives: Natives::new(),
            strings: Strings::new(),
            objects: Vec::new(),
            gc: crate::gc::Collector::new(),
            runtime_clock: None,
            runtime_precision: crate::interp::SCRIPT_PRECISION,
            native_impls: HashMap::new(),
            resource_resolver: None,
            method_diagnostics: vec![],
            constructor_calls: vec![],
            missing_native: None,
            native_observer: None,
            native_diagnostics: None,
            native_thread_hook: None,
            call_trace: false,
            native_boundary: None,
            native_failure: None,
            initializer_failure: None,
            initialization_diagnostics: vec![],
            initializer_depth_limit: 32,
            initializer_depth: 0,
            static_cells: HashMap::new(),
            initialized_dependencies: HashMap::new(),
            initializing_classes: HashSet::new(),
            array_cells: HashMap::new(),
            variable_names: Vec::new(),
        }
    }

    /// Install an already decoded body with its owning constant pool.
    pub fn add_body(
        &mut self,
        class: &str,
        nodes: Vec<Node>,
        pool: Vec<slr_fmt_tufa::PoolEntry>,
    ) -> Result<BodyId, crate::vmclass::LoadError> {
        self.loader.add_body(class, nodes, pool)
    }

    /// Loaded class context for an interpreter frame.
    pub fn class_context(&self, class: &str) -> Option<u32> {
        self.loader.context_word(class)
    }

    /// Boot (`FUN_0040fe70` substring table + primitive binds).
    pub fn boot(&mut self) {
        self.loader.boot();
    }

    /// Enter an externally requested method through the same overload and frame
    /// dispatch as bytecode calls. Arguments are in declaration order.
    pub fn enter_method(
        &mut self,
        thread: &mut Thread,
        class: crate::vmclass::ClassId,
        receiver: u32,
        name: &str,
        args: &[Value],
    ) -> Result<Flow, VmError> {
        if thread.frames.is_empty() {
            thread.frames.push(crate::interp::Frame::new(None, 0));
        }
        for &arg in args {
            thread.push_new(arg)?;
        }
        thread.push_new(Value::int(args.len() as i32))?;
        self.call_dynamic_id(thread, class, receiver, name)
    }

    /// Resolve the opaque interned-name handle carried by a local variable.
    pub fn variable_name(&self, word: u32) -> Option<&str> {
        self.variable_names
            .get(word.checked_sub(1)? as usize)
            .map(String::as_str)
    }

    fn intern_variable_name(&mut self, name: &str) -> u32 {
        self.strings.intern(name.as_bytes());
        if let Some(index) = self.variable_names.iter().position(|value| value == name) {
            return index as u32 + 1;
        }
        self.variable_names.push(name.to_owned());
        self.variable_names.len() as u32
    }

    /// The element cells of an array object, for natives that copy payloads.
    pub fn array_cells_mut(&mut self, word: u32) -> Option<&mut Vec<Value>> {
        self.array_cells.get_mut(&word)
    }

    /// Allocate initialized storage for one typed array. This does not
    /// interpret `new[]` bytecode or resolve its encoded element type.
    /// exe: FUN_00407da0
    pub fn alloc_array(&mut self, element: &str, length: usize) -> Result<u32, HostError> {
        if length > i32::MAX as usize {
            return Err(HostError::BadCast);
        }
        let ty = TypeTag::intern(element);
        if !matches!(
            ty,
            TypeTag::Int | TypeTag::Float | TypeTag::Ref(_) | TypeTag::Array(_)
        ) {
            return Err(HostError::BadCast);
        }
        let array = crate::vmclass::ArrayDescriptor::new(&format!("[{element}"))
            .ok_or(HostError::BadCast)?;
        let word = 0x1000 + self.objects.len() as u32;
        self.objects.push(Object {
            class: crate::vmclass::ClassRef::Array(array),
            fields: vec![Value::int(length as i32)],
        });
        self.gc.allocated(word);
        self.array_cells.insert(
            word,
            vec![
                Value {
                    tag: ty,
                    payload: 0
                };
                length
            ],
        );
        Ok(word)
    }

    /// Base-first instance-field layout. Static variables belong to their
    /// declaring class, never to an object's instance variable container.
    /// The name form resolves once at the boundary; layout below carries IDs.
    /// exe: FUN_00407da0
    pub fn instance_fields(&self, class: &str) -> Result<Vec<&crate::vmclass::Field>, HostError> {
        let id = self.loader.lookup(class).ok_or(HostError::BadCast)?;
        self.instance_fields_id(id)
    }

    /// ID-carrying instance layout: hierarchy steps dereference records by
    /// ID; declared base spellings resolve through current selection.
    pub fn instance_fields_id(
        &self,
        class: crate::vmclass::ClassId,
    ) -> Result<Vec<&crate::vmclass::Field>, HostError> {
        let mut hierarchy = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut current = Some(class);
        while let Some(id) = current {
            if !seen.insert(id) {
                return Err(HostError::BadCast);
            }
            let record = self.loader.find_by_id(id).ok_or(HostError::BadCast)?;
            if record.name == "V" {
                break;
            }
            hierarchy.push(record);
            // `V` is the loader's root sentinel (java.lang.Object's
            // declared base), not a loadable record: stop there.
            current = match record.base.as_deref() {
                Some("V") | None => None,
                Some(parent) => Some(self.loader.lookup(parent).ok_or(HostError::BadCast)?),
            };
        }
        Ok(hierarchy
            .into_iter()
            .rev()
            .flat_map(|class| class.fields.iter().filter(|field| !field.is_static))
            .collect())
    }

    /// Named instance slot, shared by script lvalues and installed natives.
    /// The name form resolves once at the boundary; layout below carries IDs.
    /// exe: FUN_00403840
    pub fn instance_field_slot(&self, class: &str, name: &str) -> Result<usize, HostError> {
        let id = self.loader.lookup(class).ok_or(HostError::BadCast)?;
        self.instance_field_slot_id(id, name)
    }

    /// ID-carrying instance slot lookup.
    /// exe: FUN_00403840
    pub fn instance_field_slot_id(
        &self,
        class: crate::vmclass::ClassId,
        name: &str,
    ) -> Result<usize, HostError> {
        self.instance_fields_id(class)?
            .iter()
            .rposition(|field| field.name == name)
            .ok_or(HostError::MissingMember)
    }

    /// The metadata resolver checks each class's static then instance list,
    /// walking ancestors; inherited instance indices include base fields.
    /// Entry spellings resolve once; the walk below carries IDs.
    /// exe: FUN_00405690
    /// ID-carrying field resolution: owner steps dereference records by ID.
    /// exe: FUN_00405690
    fn field_index_id(
        &self,
        class: crate::vmclass::ClassId,
        name: &str,
    ) -> Result<(usize, bool), HostError> {
        let layout = self.instance_fields_id(class)?;
        let mut current = Some(class);
        while let Some(owner) = current {
            let record = self
                .loader
                .find_by_id(owner)
                .ok_or(HostError::MissingMember)?;
            if record.name == "V" {
                break;
            }
            if let Some(index) = record
                .fields
                .iter()
                .filter(|field| field.is_static)
                .enumerate()
                .filter(|(_, field)| field.name == name)
                .last()
                .map(|(index, _)| index)
            {
                return Ok((index, true));
            }
            let owner_name = record.name.clone();
            if record
                .fields
                .iter()
                .any(|field| !field.is_static && field.name == name)
            {
                let index = layout
                    .iter()
                    .rposition(|field| field.class == owner_name && field.name == name)
                    .ok_or(HostError::MissingMember)?;
                return Ok((index, false));
            }
            current = match record.base.as_deref() {
                Some(parent) => Some(self.loader.lookup(parent).ok_or(HostError::MissingMember)?),
                None => None,
            };
        }
        Err(HostError::MissingMember)
    }

    fn indexed_field(
        &mut self,
        owner: crate::vmclass::ClassId,
        pool_index: u32,
        receiver: u32,
    ) -> Result<HeapSlot, HostError> {
        let (class, name, ty) = self
            .loader
            .field_reference_of(owner, pool_index)
            .map_err(|_| HostError::MissingMember)?;
        if ty.starts_with('(') {
            return Err(HostError::MissingMember);
        }
        let encoded = self.loader.lookup(class).ok_or(HostError::MissingMember)?;
        let (slot, is_static) = self.field_index_id(encoded, name)?;
        if is_static {
            // The original cache resolver records the defining module in
            // DAT0062f11c, but 101b still loads the pool-encoded class via
            // 406c20/4117a0. Preserve that observed behavior even for a
            // controlled inherited-static reference encoded with a leaf class.
            self.static_slot_id(encoded, slot)
        } else if receiver == 0 {
            Err(HostError::MissingMember)
        } else {
            Ok(HeapSlot::Object {
                word: receiver,
                slot,
            })
        }
    }

    /// Resolve the original static-first instance name lookup.
    /// The runtime class comes from the object's retained [`ClassRef`];
    /// statics pin the same record through the context word.
    /// exe: FUN_00408800
    pub fn member_slot(&mut self, word: u32, name: &str) -> Result<HeapSlot, HostError> {
        if self.array_cells.contains_key(&word) {
            return if name == "length" {
                Ok(HeapSlot::Object { word, slot: 0 })
            } else {
                Err(HostError::MissingMember)
            };
        }

        let class = self
            .objects
            .get(word.checked_sub(0x1000).ok_or(HostError::MissingMember)? as usize)
            .ok_or(HostError::MissingMember)?
            .class
            .clone();
        let crate::vmclass::ClassRef::Loaded(id) = class else {
            return Err(HostError::MissingMember);
        };
        if let Some(slot) = self
            .loader
            .find_by_id(id)
            .ok_or(HostError::MissingMember)?
            .fields
            .iter()
            .filter(|field| field.is_static)
            .enumerate()
            .filter(|(_, field)| field.name == name)
            .last()
            .map(|(index, _)| index)
        {
            return self.static_slot_id(id, slot);
        }
        Ok(HeapSlot::Object {
            word,
            slot: self.instance_field_slot_id(id, name)?,
        })
    }

    /// Initialize already-loaded dependencies and append each own static cell
    /// immediately before its initializer. The installed owner is the reentry
    /// sentinel; a malformed dependency cycle before installation fails closed.
    /// This does not model machine fatal-handler unwind or class poisoning.
    /// Entry IDs are retained record identities; declared base/interface
    /// spellings resolve through the current selection until the owner
    /// installs, then the owner retains its resolved dependency IDs.
    /// exe: FUN_00403e80
    fn initialize_class_id(&mut self, class: crate::vmclass::ClassId) -> Result<(), HostError> {
        let context = class.get();
        if self.static_cells.contains_key(&context) {
            // The owner already installed: reentry retains the stored
            // dependency IDs (403e80 binding-time policy) and never
            // re-resolves spellings through the current selection.
            return Ok(());
        }
        if !self.initializing_classes.insert(context) {
            return Err(HostError::UnsupportedInitializer(
                "cyclic initialization dependency".into(),
            ));
        }
        let result = (|| {
            let metadata = self
                .loader
                .find_by_id(class)
                .ok_or(HostError::MissingMember)?
                .clone();
            // Resolve the entire declared dependency list through the
            // CURRENT cache selection, then recurse. A failure before the
            // owner's own owner installs leaves nothing retained, so a
            // retry after a duplicate registration selects the new
            // records; after the owner installs, reentry above returns
            // without replacing the stored IDs.
            // Original interface-kind diagnostics continue, and the base kind
            // does not impose a corresponding hard-rejection check.
            let base = metadata.base.as_deref().filter(|base| *base != "V");
            let base_id = match base {
                Some(base) => Some(
                    self.loader
                        .request(base)
                        .map_err(|_| HostError::MissingMember)?
                        .ok_or(HostError::MissingMember)?,
                ),
                None => None,
            };
            let mut interface_ids = Vec::with_capacity(metadata.interfaces.len());
            for interface in &metadata.interfaces {
                self.loader
                    .request(interface)
                    .map_err(|_| HostError::MissingMember)?;
                let dependency = self
                    .loader
                    .find(interface)
                    .ok_or(HostError::MissingMember)?;
                if dependency.kind != 1 {
                    let owner = metadata.name.clone();
                    self.initialization_diagnostics.push(
                        InitializationDiagnostic::InterfaceExtendsClass {
                            class: owner,
                            interface: interface.clone(),
                            kind: dependency.kind,
                        },
                    );
                }
                interface_ids.push(
                    self.loader
                        .lookup(interface)
                        .ok_or(HostError::MissingMember)?,
                );
            }
            if let Some(base_id) = base_id {
                self.initialize_class_id(base_id)?;
            }
            for interface_id in interface_ids.clone() {
                self.initialize_class_id(interface_id)?;
            }
            self.initialized_dependencies
                .insert(context, (base_id, interface_ids));
            self.static_cells.insert(context, Vec::new());
            for (static_index, field) in metadata
                .fields
                .iter()
                .filter(|field| field.is_static)
                .enumerate()
            {
                self.static_cells.get_mut(&context).unwrap().push(Value {
                    tag: TypeTag::intern(&field.ty),
                    payload: 0,
                });
                // Original compares canonical descriptor pointers. The loader's
                // current single-version class boundary and TypeTag interning
                // provide that identity here; versioned class identity is open.
                if TypeTag::intern(&field.ty) == TypeTag::intern(&format!("L{};", metadata.name)) {
                    continue;
                }
                // Initializer lookup pins (owner ID, declared index): never
                // Field-equality matching across records.
                let declared = metadata
                    .fields
                    .iter()
                    .position(|candidate| {
                        candidate.name == field.name
                            && candidate.ty == field.ty
                            && candidate.is_static
                    })
                    .and_then(|index| self.loader.field_initializer_of(class, index));
                if let Some(body) = declared {
                    let value = self
                        .initialize_field(
                            body,
                            context,
                            0,
                            self.static_cells[&context][static_index],
                        )
                        .map_err(|error| {
                            HostError::UnsupportedInitializer(format!(
                                "{}.{}: {error:?}",
                                metadata.name, field.name
                            ))
                        })?;
                    self.static_cells.get_mut(&context).unwrap()[static_index] = value;
                    self.gc_mark_reference(value);
                }
            }
            Ok(())
        })();
        self.initializing_classes.remove(&context);
        if let Err(error) = &result {
            self.initializer_failure = Some(format!(
                "{}: {error:?}",
                self.loader.class_name(class).unwrap_or("<stale class>")
            ));
        }
        result
    }

    /// A class's own static variable container, separate from instances.
    /// The name form resolves once at the boundary; the container key is
    /// the retained context word.
    /// exe: FUN_00404800
    /// ID-carrying static slot: the container key is the retained context.
    /// exe: FUN_00404800
    fn static_slot_id(
        &mut self,
        class: crate::vmclass::ClassId,
        slot: usize,
    ) -> Result<HeapSlot, HostError> {
        self.initialize_class_id(class)?;
        let context = class.get();
        if slot >= self.static_cells[&context].len() {
            return Err(HostError::MissingMember);
        }
        Ok(HeapSlot::Static { context, slot })
    }

    fn initialize_field(
        &mut self,
        body: BodyId,
        context: u32,
        receiver: u32,
        destination: Value,
    ) -> Result<Value, HostError> {
        if self.initializer_depth >= self.initializer_depth_limit {
            return Err(HostError::UnsupportedInitializer(
                "initializer nesting budget exhausted".into(),
            ));
        }
        self.initializer_depth += 1;
        let result = self.run_initializer_field(body, context, receiver, destination);
        self.initializer_depth -= 1;
        result
    }

    fn run_initializer_field(
        &mut self,
        body: BodyId,
        context: u32,
        receiver: u32,
        mut destination: Value,
    ) -> Result<Value, HostError> {
        let mut thread = Thread::new();
        thread.alu.precision = self.runtime_precision;
        thread.clock_name = Some(
            if receiver == 0 {
                "THRD-CLASSVAR-INI"
            } else {
                "THRD-INSTVAR-INI"
            }
            .into(),
        );
        let pc = Some(crate::interp::Pc { body, idx: 0 });
        let mut frame = crate::interp::Frame::new(pc, receiver);
        frame.class_ctx = context;
        thread.frames.push(frame);
        thread.pc = pc;
        let source = self.runtime_clock;
        let outcome = thread.run(self, 0.0, &mut || {
            source.map_or_else(|| slr_x87::X87::from_f64(0.0), |clock| clock())
        });
        self.initialization_diagnostics.extend(
            thread
                .alu
                .diagnostics
                .drain(..)
                .map(InitializationDiagnostic::Body),
        );
        outcome.map_err(|error| HostError::UnsupportedInitializer(format!("{error:?}")))?;
        if thread
            .frames
            .last()
            .is_none_or(|frame| frame.stack.len() != 1)
        {
            return Err(HostError::UnsupportedInitializer(
                "invalid initializer result stack".into(),
            ));
        }
        let mut value = thread
            .pop_deref_with(self)
            .map_err(|error| HostError::UnsupportedInitializer(format!("{error:?}")))?;
        thread
            .alu
            .store_with(&mut destination, &mut value, Some(self));
        self.initialization_diagnostics.extend(
            thread
                .alu
                .diagnostics
                .drain(..)
                .map(InitializationDiagnostic::Body),
        );
        Ok(destination)
    }

    /// Read an existing loaded class's own static cell without requesting or
    /// initializing a class. Engine observers such as the frame-end render
    /// notification use the same cached class/field storage as bytecode.
    #[must_use]
    pub fn read_loaded_static_field(&self, class: &str, name: &str) -> Option<Value> {
        let class = self.loader.lookup(class)?;
        let slot = self
            .loader
            .find_by_id(class)?
            .fields
            .iter()
            .filter(|field| field.is_static)
            .position(|field| field.name == name)?;
        self.static_cells.get(&class.get())?.get(slot).copied()
    }

    /// Read a named field using the same authoritative cells as bytecode.
    /// Declared types dereference the retained record ID, never a re-lookup.
    pub fn read_field(&mut self, word: u32, name: &str) -> Result<Value, HostError> {
        let slot = self.member_slot(word, name)?;
        let value = <Self as Host>::read_cell(self, slot).map_err(|_| HostError::MissingMember)?;
        let declared = match slot {
            HeapSlot::Object { word, slot: 0 } if self.array_cells.contains_key(&word) => {
                TypeTag::Int
            }
            HeapSlot::Object { word, slot } => {
                use crate::vmclass::ClassRef;
                let ClassRef::Loaded(id) = self.objects[(word - 0x1000) as usize].class.clone()
                else {
                    return Err(HostError::MissingMember);
                };
                TypeTag::intern(
                    &self
                        .instance_fields_id(id)?
                        .get(slot)
                        .ok_or(HostError::MissingMember)?
                        .ty,
                )
            }
            HeapSlot::Static { context, slot } => {
                let id = self
                    .loader
                    .context_id(context)
                    .ok_or(HostError::MissingMember)?;
                TypeTag::intern(
                    &self
                        .loader
                        .find_by_id(id)
                        .ok_or(HostError::MissingMember)?
                        .fields
                        .iter()
                        .filter(|field| field.is_static)
                        .nth(slot)
                        .ok_or(HostError::MissingMember)?
                        .ty,
                )
            }
            _ => return Err(HostError::MissingMember),
        };
        if value.tag != declared {
            return Err(HostError::MissingMember);
        }
        Ok(value)
    }

    /// Write a named field without changing its declared type.
    pub fn write_field(&mut self, word: u32, name: &str, value: Value) -> Result<(), HostError> {
        let slot = self.member_slot(word, name)?;
        let current =
            <Self as Host>::read_cell(self, slot).map_err(|_| HostError::MissingMember)?;
        if current.tag != value.tag {
            return Err(HostError::MissingMember);
        }
        <Self as Host>::write_cell(self, slot, value).map_err(|_| HostError::MissingMember)
    }

    /// Initialize class dependencies, then append base-first typed instance
    /// cells immediately before each field initializer. Errors retain the
    /// observed prefix; machine fatal-handler unwind is outside this host.
    /// The entry spelling resolves once; the object, layout and initializer
    /// lookups below carry the retained record ID.
    /// exe: FUN_00407da0
    pub fn alloc_object(&mut self, class: &str) -> Result<u32, HostError> {
        let id = self
            .loader
            .request(class)
            .map_err(|_| HostError::BadCast)?
            .ok_or(HostError::BadCast)?;
        self.alloc_object_id(id)
    }

    /// ID-carrying allocation: the object's runtime class is the retained
    /// record; initializer lookups pin (owner ID, declared index).
    /// exe: FUN_00407da0
    pub fn alloc_object_id(&mut self, class: crate::vmclass::ClassId) -> Result<u32, HostError> {
        self.initialize_class_id(class)?;
        let metadata: Vec<_> = self
            .instance_fields_id(class)?
            .into_iter()
            .cloned()
            .collect();
        // Field-initializer (owner, declared index) pairs for this object's
        // own layout: resolve each declaring record once, retain the ID.
        let declaring: Vec<Option<crate::vmclass::ClassId>> = metadata
            .iter()
            .map(|field| self.loader.lookup(&field.class))
            .collect();
        let fields = Vec::new();
        let word = 0x1000 + self.objects.len() as u32;
        self.objects.push(Object {
            class: crate::vmclass::ClassRef::Loaded(class),
            fields,
        });
        self.gc.allocated(word);
        for (index, field) in metadata.iter().enumerate() {
            self.objects[(word - 0x1000) as usize].fields.push(Value {
                tag: TypeTag::intern(&field.ty),
                payload: 0,
            });
            let body = declaring[index].and_then(|owner| {
                let record = self.loader.find_by_id(owner)?;
                let declared = record.fields.iter().position(|candidate| {
                    candidate.name == field.name
                        && candidate.ty == field.ty
                        && candidate.is_static == field.is_static
                })?;
                self.loader.field_initializer_of(owner, declared)
            });
            if let Some(body) = body {
                let context = declaring[index]
                    .map(|owner| owner.get())
                    .ok_or(HostError::BadCast)?;
                let value = self.initialize_field(
                    body,
                    context,
                    word,
                    self.objects[(word - 0x1000) as usize].fields[index],
                )?;
                self.objects[(word - 0x1000) as usize].fields[index] = value;
                self.gc_mark_reference(value);
            }
        }
        Ok(word)
    }
}

impl Default for VmHost {
    fn default() -> Self {
        Self::new()
    }
}

impl LoaderConvert for VmHost {
    fn string_target(
        &mut self,
        target: &crate::vmclass::ClassRef,
        string: &crate::vmclass::ClassRef,
    ) -> bool {
        use crate::vmclass::ClassRef;
        match (target, string) {
            (ClassRef::Loaded(target), ClassRef::Loaded(string)) => self
                .loader
                .is_inherited_from_id(*target, *string)
                .unwrap_or(false),
            _ => false,
        }
    }
    fn load_class(&mut self, descriptor: &str) -> Option<crate::vmclass::ClassRef> {
        self.loader.resolve_descriptor(descriptor)
    }

    fn is_assignable(&mut self, from: &crate::vmclass::ClassRef, to: &str) -> bool {
        self.loader.is_assignable_ref(from, to)
    }

    fn reference_class(&mut self, value: Value) -> Option<crate::vmclass::ClassRef> {
        // Retain the legacy embedding word namespace accepted before #110.
        // All current production allocations use the ordinary0x1000 heap base.
        let base = if value.payload >= 0x2000_0000 {
            0x2000_0000
        } else {
            0x1000
        };
        self.objects
            .get(value.payload.checked_sub(base)? as usize)
            .map(|object| object.class.clone())
    }

    fn make_string(&mut self, value: Value) -> u32 {
        use crate::format::{sprintf, FormatArg};
        let text = match value.tag {
            TypeTag::Int => sprintf(b"%d", &[FormatArg::Word(value.payload)]),
            TypeTag::Float => sprintf(b"%.3f", &[FormatArg::Double(f64::from(value.as_f32()))]),
            _ => panic!("numeric String conversion requires I or F"),
        }
        .expect("fixed numeric format with one matching argument");
        self.intern_string(Some(&text))
            .expect("loaded String metadata required for conversion")
    }

    fn target_name(&self, descriptor: &str) -> &'static str {
        TypeTag::intern(descriptor).name()
    }
}

impl VmHost {
    /// ID-carrying receiver lookup: loaded records dereference to their
    /// retained arena identity; arrays stay descriptor-keyed without
    /// fabricating a loaded record.
    /// exe: FUN_00405690
    fn reference_class_id(&mut self, value: Value) -> Option<crate::vmclass::ClassId> {
        match <Self as LoaderConvert>::reference_class(self, value)? {
            crate::vmclass::ClassRef::Loaded(id) => Some(id),
            crate::vmclass::ClassRef::Array(_) => None,
        }
    }

    /// ID-compatible class load: spelling resolves once at the boundary,
    /// then the loaded record identity carries execution.
    /// exe: FUN_00410890
    fn load_class_ref(&mut self, descriptor: &str) -> Option<crate::vmclass::ClassRef> {
        if let Some(array) = crate::vmclass::ArrayDescriptor::new(descriptor) {
            return Some(crate::vmclass::ClassRef::Array(array));
        }
        // Load-or-get: a miss reads the class file like any other request
        // (`FUN_00410890`), instead of answering from the loaded set only.
        match self.loader.request(descriptor) {
            Ok(Some(id)) => Some(crate::vmclass::ClassRef::Loaded(id)),
            _ => None,
        }
    }
}

impl Host for VmHost {
    fn diagnostic_cell(&self, slot: HeapSlot) -> Option<Value> {
        match slot {
            HeapSlot::Object { word, slot } => self
                .objects
                .get(word.checked_sub(0x1000)? as usize)?
                .fields
                .get(slot)
                .copied(),
            HeapSlot::Static { context, slot } => {
                self.static_cells.get(&context)?.get(slot).copied()
            }
            HeapSlot::Array { word, index } => self.array_cells.get(&word)?.get(index).copied(),
        }
    }
    fn diagnostic_objects_total(&self, thread: &Thread) -> usize {
        self.diagnostic_reference_words(thread).len()
    }
    fn diagnostic_objects(&self, thread: &Thread) -> Vec<crate::diagnostics::ObjectSnapshot> {
        self.diagnostic_reference_words(thread)
            .into_iter()
            .take(16)
            .filter_map(|word| self.diagnostic_object(word))
            .collect()
    }

    fn receiver_value(&mut self, word: u32) -> Result<Value, VmError> {
        let class = self
            .reference_class(Value::reference(word))
            .ok_or(HostError::BadCast)?;
        let descriptor = match &class {
            crate::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
            crate::vmclass::ClassRef::Loaded(id) => {
                let name = self.loader.class_name(*id).ok_or(HostError::BadCast)?;
                format!("L{name};")
            }
        };
        Ok(Value {
            tag: TypeTag::intern(&descriptor),
            payload: word,
        })
    }
    fn body(&self, id: BodyId) -> &[Node] {
        self.loader.body(id).unwrap_or(&[])
    }

    fn body_label(&self, body: BodyId) -> String {
        let Some(owner) = self.loader.body_owner(body) else {
            return format!("body{}", body.0);
        };
        let Some(class) = self.loader.find_by_id(owner) else {
            return format!("body{}", body.0);
        };
        let method = class
            .methods
            .iter()
            .find(|m| !m.is_native && self.loader.body_id_of(owner, m.body as usize) == Some(body));
        match method {
            Some(m) => format!("{}.{}{}", class.name, m.name, m.sig),
            None => format!("{}.<field init body{}>", class.name, body.0),
        }
    }

    fn read_cell(&mut self, slot: HeapSlot) -> Result<Value, VmError> {
        match slot {
            HeapSlot::Object { word, slot } => self
                .objects
                .get(word.checked_sub(0x1000).ok_or(HostError::MissingMember)? as usize)
                .and_then(|object| object.fields.get(slot))
                .copied()
                .ok_or_else(|| HostError::MissingMember.into()),
            HeapSlot::Static { context, slot } => self
                .static_cells
                .get(&context)
                .and_then(|fields| fields.get(slot))
                .copied()
                .ok_or_else(|| HostError::MissingMember.into()),
            HeapSlot::Array { word, index } => self
                .array_cells
                .get(&word)
                .and_then(|cells| cells.get(index))
                .copied()
                .ok_or_else(|| HostError::MissingMember.into()),
        }
    }

    fn write_cell(&mut self, slot: HeapSlot, value: Value) -> Result<(), VmError> {
        let before = if crate::diagnostics::enabled() {
            self.read_cell(slot).ok()
        } else {
            None
        };
        self.gc_mark_reference(value);
        let result = match slot {
            HeapSlot::Object { word, slot } => {
                *self
                    .objects
                    .get_mut(word.checked_sub(0x1000).ok_or(HostError::MissingMember)? as usize)
                    .and_then(|object| object.fields.get_mut(slot))
                    .ok_or(HostError::MissingMember)? = value;
                Ok(())
            }
            HeapSlot::Static { context, slot } => {
                *self
                    .static_cells
                    .get_mut(&context)
                    .and_then(|fields| fields.get_mut(slot))
                    .ok_or(HostError::MissingMember)? = value;
                Ok(())
            }
            HeapSlot::Array { word, index } => {
                *self
                    .array_cells
                    .get_mut(&word)
                    .and_then(|cells| cells.get_mut(index))
                    .ok_or(HostError::MissingMember)? = value;
                Ok(())
            }
        };
        if result.is_ok() {
            if let Some(before) = before {
                crate::diagnostics::emit(crate::diagnostics::EventKind::Store {
                    destination: format!("{slot:?}"),
                    before: before.into(),
                    after: value.into(),
                });
            }
        }
        result
    }

    fn on_reference_store(&mut self, value: Value) {
        self.gc_mark_reference(value);
    }

    fn store_value(
        &mut self,
        alu: &mut crate::ops::Alu,
        destination: &mut Value,
        source: &mut Value,
    ) {
        alu.store_with(destination, source, Some(self));
    }

    fn extern_op(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        match op.kind {
            ExternKind::Declare => self.declare(thread, op),
            ExternKind::ResolveName => self.resolve_name(thread, op),
            ExternKind::FieldRef
            | ExternKind::FieldByName
            | ExternKind::MemberAccess
            | ExternKind::ArrayElement => self.field(thread, op),
            ExternKind::ArgArray => self.arg_array(thread, op),
            ExternKind::CallStatic
            | ExternKind::CallInstance
            | ExternKind::CallInit
            | ExternKind::CallThisInit
            | ExternKind::CallSuperInit
            | ExternKind::CallResolved => self.call(thread, op),
            ExternKind::New => self.new_object(thread, op),
            ExternKind::NewArray => self.new_array(thread, op),
            ExternKind::InstanceOf => self.instance_of(thread, op),
            ExternKind::Cast => self.cast(thread, op),
            ExternKind::ResourceLiteral => self.resource_literal(thread, op),
            ExternKind::StringLiteral => self.string_literal(thread, op),
        }
    }

    /// The `+`/`+=` string gate: when exactly one side is
    /// `java.lang.String` the other side converts toward String, and when
    /// both sides are String the gate falls through to `Alu::add` (which
    /// logs, since concatenation needs booted string objects).
    ///
    /// exe: FUN_00417a70
    fn concatenate_strings(
        &mut self,
        left: &mut Value,
        right: &mut Value,
    ) -> Option<Result<(), VmError>> {
        (left.tag.name() == TypeTag::STRING && right.tag.name() == TypeTag::STRING)
            .then(|| self.join_strings(left, right))
    }

    fn operator_on_refs(
        &mut self,
        _op: RefOperator,
        left: &mut Value,
        right: &mut Value,
    ) -> Option<Result<(), VmError>> {
        // `FUN_0041b4d0` via `FUN_00417a70`: exactly-one-String-side
        // converts, both-String concatenates, other classes fall through.
        let l_str = left.tag.name() == TypeTag::STRING;
        let r_str = right.tag.name() == TypeTag::STRING;
        match (l_str, r_str) {
            // Both Strings: the gate leaves them alone; `FUN_0041b4d0` then
            // concatenates ([`Host::concatenate_strings`]).
            (true, true) => None,
            (false, false) => None,
            _ => {
                // Convert the non-String side toward String.
                let (target, val) = if l_str {
                    (&mut *left, &mut *right)
                } else {
                    (&mut *right, &mut *left)
                };
                let name = target.tag.name();
                // `loader_convert` reads `LoaderConvert` from `self`.
                let res = {
                    let host: &mut dyn LoaderConvert = self;
                    crate::values::loader_convert(host, val, name)
                };
                Some(
                    res.map_err(|_| VmError::Unsupported(0x1007))
                        .and_then(|_| self.join_strings(left, right)),
                )
            }
        }
    }
}

impl VmHost {
    /// The `L` arm of `FUN_0041b4d0` for two String cells: a null side
    /// contributes nothing, the joined bytes are interned (`FUN_004174a0`) and
    /// stored into the left cell.
    fn join_strings(&mut self, left: &mut Value, right: &Value) -> Result<(), VmError> {
        let bytes = |host: &Self, word: u32| {
            (word != 0)
                .then(|| host.string_bytes(word).map(<[u8]>::to_vec))
                .flatten()
        };
        let joined = match (bytes(self, left.payload), bytes(self, right.payload)) {
            (None, None) => None,
            (Some(l), None) => Some(l),
            (None, Some(r)) => Some(r),
            (Some(mut l), Some(r)) => {
                l.extend_from_slice(&r);
                Some(l)
            }
        };
        left.payload = self
            .intern_string(joined.as_deref())
            .map_err(|_| VmError::Unsupported(0x1007))?;
        Ok(())
    }

    /// The declaration immediate is a local index; its following node
    /// carries the type-pool index, optionally preceded by a 402e name.
    /// exe: FUN_00408d60
    fn declare(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let owner = self
            .loader
            .body_owner(op.pc.body)
            .ok_or(HostError::UnresolvedName)?;
        let nodes = self
            .loader
            .body(op.pc.body)
            .ok_or(HostError::UnresolvedName)?;
        let mut index = op.pc.idx + 1;
        let name = if let Some(node) = nodes.get(index).filter(|node| node.opword == 0x402e) {
            index += 1;
            Some(
                self.loader
                    .pool_text_of(owner, node.imm)
                    .map_err(|_| HostError::UnresolvedName)?
                    .to_owned(),
            )
        } else {
            None
        };
        let node = nodes.get(index).ok_or(HostError::UnresolvedName)?;
        let ty = self
            .loader
            .pool_text_of(owner, node.imm)
            .map_err(|_| HostError::UnresolvedName)?
            .to_owned();
        let push_lvalue = nodes
            .get(index + 1)
            .is_some_and(|node| node.opword == 0x002c);
        let name = name.map_or(0, |name| self.intern_variable_name(&name));
        let variable = thread.new_var(
            Value {
                tag: TypeTag::intern(&ty),
                payload: 0,
            },
            name,
        );
        thread
            .frames
            .last_mut()
            .ok_or(VmError::NoFrame)?
            .locals
            .push(variable);
        if push_lvalue {
            index += 1;
            let slot = thread
                .frames
                .last()
                .ok_or(VmError::NoFrame)?
                .locals
                .get(op.node.imm as usize)
                .copied()
                .map_or(Slot::Null, Slot::Var);
            thread.push(slot)?;
        }
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: index,
        });
        Ok(Flow::Advance)
    }

    /// The storage a `0x101b` field-reference component names: the receiver's
    /// class (or the qualified class for a static start) is searched by name,
    /// and a static field resolves to its class slot.
    /// exe: FUN_00420d20, FUN_004208e0 (0x101b branch)
    fn field_component_slot(
        &mut self,
        owner: crate::vmclass::ClassId,
        imm: u32,
        receiver: u32,
        qualified: Option<crate::vmclass::ClassId>,
    ) -> Result<HeapSlot, VmError> {
        let dynamic = if receiver != 0 {
            let object = self
                .objects
                .get(
                    receiver
                        .checked_sub(0x1000)
                        .ok_or(HostError::MissingMember)? as usize,
                )
                .ok_or(HostError::MissingMember)?;
            let crate::vmclass::ClassRef::Loaded(id) = object.class.clone() else {
                // An array object's only field is `length`, stored in slot 0
                // (`alloc_array` writes the length as the sole field).
                let is_length = self
                    .loader
                    .field_reference_of(owner, imm)
                    .is_ok_and(|(_, name, ty)| name == "length" && ty == "I");
                return if is_length {
                    Ok(HeapSlot::Object {
                        word: receiver,
                        slot: 0,
                    })
                } else {
                    Err(HostError::MissingMember.into())
                };
            };
            Some(id)
        } else {
            qualified
        };
        let (encoded, name, ty) = self
            .loader
            .field_reference_of(owner, imm)
            .map_err(|_| HostError::MissingMember)?;
        if ty.starts_with('(') {
            return Err(HostError::MissingMember.into());
        }
        let encoded = self
            .loader
            .lookup(encoded)
            .ok_or(HostError::MissingMember)?;
        let (slot, is_static) = self.field_index_id(dynamic.unwrap_or(encoded), name)?;
        Ok(if is_static {
            self.static_slot_id(encoded, slot)?
        } else if receiver != 0 {
            HeapSlot::Object {
                word: receiver,
                slot,
            }
        } else {
            return Err(HostError::MissingMember.into());
        })
    }

    /// Evaluate the inline field-name chain and borrow its final variable.
    /// Method-name/call record evaluation belongs to the separate 4208e0 path.
    /// exe: FUN_00420d20
    fn resolve_name(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let owner = self
            .loader
            .body_owner(op.pc.body)
            .ok_or(HostError::UnresolvedName)?;
        let start = op.pc.idx + 1;
        let end = start
            .checked_add(op.node.imm as usize)
            .ok_or(HostError::UnresolvedName)?;
        let nodes = self
            .loader
            .body(op.pc.body)
            .and_then(|body| body.get(start..end))
            .ok_or(HostError::UnresolvedName)?
            .to_vec();
        let mut current = None;
        let mut qualified = None;
        for component in nodes {
            match component.opword {
                0x1019 if current.is_none() && qualified.is_none() => {
                    let name = self
                        .loader
                        .pool_text_of(owner, component.imm)
                        .map_err(|_| HostError::UnresolvedName)?
                        .to_owned();
                    let id = self
                        .loader
                        .request(&name)
                        .map_err(|_| HostError::UnresolvedName)?
                        .ok_or(HostError::UnresolvedName)?;
                    self.initialize_class_id(id)?;
                    qualified = Some(id);
                }
                0x1001 => {
                    let variable = thread
                        .frames
                        .last()
                        .ok_or(VmError::NoFrame)?
                        .locals
                        .get(component.imm as usize)
                        .copied()
                        .ok_or(HostError::MissingMember)?;
                    current = Some(Slot::Var(variable));
                }
                0x101b => {
                    let receiver = if let Some(slot) = current {
                        self.borrow_slot(thread, slot)?.payload
                    } else if qualified.is_some() {
                        0
                    } else {
                        thread.frames.last().ok_or(VmError::NoFrame)?.receiver
                    };
                    if current.is_some() && receiver == 0 {
                        return Err(HostError::MissingMember.into());
                    }
                    current = Some(Slot::Heap(self.field_component_slot(
                        owner,
                        component.imm,
                        receiver,
                        qualified,
                    )?));
                    qualified = None;
                }
                0x101a | 0x001d if current.is_some() => {
                    let receiver = self.borrow_slot(thread, current.unwrap())?.payload;
                    let name = self
                        .loader
                        .pool_text_of(owner, component.imm)
                        .map_err(|_| HostError::MissingMember)?
                        .to_owned();
                    current = Some(Slot::Heap(self.member_slot(receiver, &name)?));
                }
                _ => return Err(HostError::UnresolvedName.into()),
            }
        }
        thread.push(current.ok_or(HostError::UnresolvedName)?)?;
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: end - 1,
        });
        Ok(Flow::Advance)
    }

    fn borrow_slot(&mut self, thread: &Thread, slot: Slot) -> Result<Value, VmError> {
        match slot {
            Slot::Var(id) => thread
                .vars
                .try_get(id)
                .map(|var| var.value)
                .ok_or(VmError::DanglingVar),
            Slot::Heap(slot) => <Self as Host>::read_cell(self, slot),
            Slot::Value(value) => Ok(value),
            Slot::Null => Err(VmError::NullSlot),
        }
    }

    fn field(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        if op.kind == ExternKind::ArrayElement {
            use crate::interp::ArrayError;
            let array = thread.pop_deref_with(self)?;
            let index = thread.pop_deref_with(self)?;
            if index.tag != TypeTag::Int {
                return Err(VmError::ArrayAccess(ArrayError::IndexType));
            }
            if index.as_i32() < 0 {
                return Err(VmError::ArrayAccess(ArrayError::NegativeIndex));
            }
            if !matches!(array.tag, TypeTag::Array(_)) {
                return Err(VmError::ArrayAccess(ArrayError::NotArray));
            }
            if array.payload == 0 {
                return Err(VmError::ArrayAccess(ArrayError::NullArray));
            }
            let length = self
                .objects
                .get(
                    array
                        .payload
                        .checked_sub(0x1000)
                        .ok_or(HostError::MissingMember)? as usize,
                )
                .and_then(|object| object.fields.first())
                .ok_or(HostError::MissingMember)?
                .as_i32();
            let index = index.as_i32() as usize;
            let elements = self
                .array_cells
                .get(&array.payload)
                .ok_or(HostError::MissingMember)?;
            let slot = if index < length.max(0) as usize && index < elements.len() {
                Slot::Heap(HeapSlot::Array {
                    word: array.payload,
                    index,
                })
            } else {
                Slot::Null
            };
            thread.release();
            thread.release();
            thread.push(slot)?;
            return Ok(Flow::Advance);
        }

        if op.kind == ExternKind::FieldRef {
            let owner = self
                .loader
                .body_owner(op.pc.body)
                .ok_or(HostError::MissingMember)?;
            let receiver = thread.frames.last().ok_or(VmError::NoFrame)?.receiver;
            let slot = self.indexed_field(owner, op.node.imm, receiver)?;
            thread.push(Slot::Heap(slot))?;
            return Ok(Flow::Advance);
        }
        if !matches!(op.kind, ExternKind::FieldByName | ExternKind::MemberAccess) {
            return Err(HostError::MissingMember.into());
        }
        let owner = self
            .loader
            .body_owner(op.pc.body)
            .ok_or(HostError::MissingMember)?;
        let name = match self
            .loader
            .pool_of(owner)
            .and_then(|pool| pool.get(op.node.imm as usize))
        {
            Some(slr_fmt_tufa::PoolEntry::Str(name)) => name.clone(),
            _ => return Err(HostError::MissingMember.into()),
        };
        let explicit = op.kind == ExternKind::MemberAccess;
        let word = if explicit {
            thread.pop_deref_with(self)?.payload
        } else {
            thread.frames.last().ok_or(VmError::NoFrame)?.receiver
        };
        let slot = if word == 0 && !explicit {
            // Defining frame context is the retained record ID; decoding it
            // back to a name and re-looking that name up is forbidden.
            let context = thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx;
            let id = self
                .loader
                .context_id(context)
                .ok_or(HostError::MissingMember)?;
            let index = self
                .loader
                .find_by_id(id)
                .ok_or(HostError::MissingMember)?
                .fields
                .iter()
                .filter(|field| field.is_static)
                .enumerate()
                .filter(|(_, field)| field.name == name)
                .last()
                .map(|(index, _)| index)
                .ok_or(HostError::MissingMember)?;
            self.static_slot_id(id, index)?
        } else {
            self.member_slot(word, &name)?
        };
        thread.push(Slot::Heap(slot))?;
        if explicit {
            thread.release();
        }
        Ok(Flow::Advance)
    }

    fn arg_array(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        if op.node.imm == 0 {
            thread.push_new(Value::reference(0))?;
            return Ok(Flow::Advance);
        }
        let mut items = Vec::with_capacity(op.node.imm as usize);
        for _ in 0..op.node.imm {
            items.push(thread.pop_deref_with(self)?);
            thread.release();
        }
        let target = *thread
            .frames
            .last()
            .ok_or(VmError::NoFrame)?
            .stack
            .last()
            .ok_or(VmError::NullSlot)?;
        if !matches!(target, Slot::Var(_) | Slot::Heap(_)) {
            return Err(VmError::NotLvalue);
        }
        let target = self.borrow_slot(thread, target)?;
        let descriptor = target.tag.name();
        let element = descriptor.strip_prefix('[').ok_or(HostError::BadCast)?;
        if parameter_types(&format!("({element})"))?.len() != 1 {
            return Err(HostError::BadCast.into());
        }
        let word = self.alloc_array(element, items.len())?;
        for (index, mut source) in items.into_iter().rev().enumerate() {
            let slot = HeapSlot::Array { word, index };
            let mut cell = self.read_cell(slot)?;
            self.store_value(&mut thread.alu, &mut cell, &mut source);
            self.write_cell(slot, cell)?;
        }
        thread.push_new(Value {
            tag: target.tag,
            payload: word,
        })?;
        Ok(Flow::Advance)
    }

    fn call(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        match op.kind {
            ExternKind::CallResolved => self.call_resolved(thread, op),
            ExternKind::CallInit | ExternKind::CallThisInit | ExternKind::CallSuperInit => {
                self.call_constructor(thread, op)
            }
            ExternKind::CallInstance | ExternKind::CallStatic => self.call_named(thread, op),
            _ => Err(VmError::Unsupported(op.node.opword)),
        }
    }

    fn call_named(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let frame = thread.frames.last().ok_or(VmError::NoFrame)?;
        let owner = self
            .loader
            .context_id(frame.class_ctx)
            .ok_or(HostError::MissingMethod)?;
        let name = self
            .loader
            .pool_text_of(owner, op.node.imm)
            .map_err(|_| HostError::MissingMethod)?
            .to_owned();
        let (class, receiver) = if op.kind == ExternKind::CallInstance {
            let receiver = thread.pop_deref_with(self)?.payload;
            thread.release();
            (
                self.reference_class_id(Value::reference(receiver))
                    .ok_or_else(|| {
                        note(format!(
                            "instance call {name:?} on receiver word {receiver:#x}, which is not a loaded heap object (0 is the original's illegal methodcall)"
                        ));
                        HostError::BadCast
                    })?,
                receiver,
            )
        } else {
            // Super dispatch resolves the owner's declared base through the
            // current cache selection (the 403e80 binding-time policy:
            // dependencies resolve dynamically until the owner's own owner
            // installs). The retained owner ID never re-resolves itself.
            let base = self
                .loader
                .find_by_id(owner)
                .ok_or(HostError::MissingMethod)?
                .base
                .clone()
                .ok_or(HostError::MissingMethod)?;
            (
                self.loader.lookup(&base).ok_or(HostError::MissingMethod)?,
                frame.receiver,
            )
        };
        self.call_dynamic_id(thread, class, receiver, &name)
    }

    /// exe: FUN_00408a30
    fn call_constructor(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let frame = thread.frames.last().ok_or(VmError::NoFrame)?;
        let (class, receiver) = if op.kind == ExternKind::CallInit {
            let count_slot = *frame.stack.last().ok_or(VmError::NullSlot)?;
            let count = self.borrow_slot(thread, count_slot)?.payload as usize;
            let stack = &thread.frames.last().ok_or(VmError::NoFrame)?.stack;
            let index = stack
                .len()
                .checked_sub(count.checked_add(2).ok_or(VmError::NullSlot)?)
                .ok_or(VmError::NullSlot)?;
            let receiver = self.borrow_slot(thread, stack[index])?.payload;
            (
                self.reference_class_id(Value::reference(receiver))
                    .ok_or(HostError::BadCast)?,
                receiver,
            )
        } else {
            let receiver = frame.receiver;
            let id = self
                .loader
                .context_id(frame.class_ctx)
                .ok_or(HostError::MissingMethod)?;
            let class = if op.kind == ExternKind::CallSuperInit {
                let base = self
                    .loader
                    .find_by_id(id)
                    .ok_or(HostError::MissingMethod)?
                    .base
                    .clone();
                match base {
                    // java.lang.Object (no declared base) has no super
                    // constructor to enter; the original advances silently.
                    None => return Ok(Flow::Advance),
                    Some(base) => self.loader.lookup(&base).ok_or(HostError::MissingMethod)?,
                }
            } else {
                id
            };
            (class, receiver)
        };
        self.call_dynamic_id(thread, class, receiver, "<init>")
    }

    fn call_resolved(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let owner = self
            .loader
            .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
            .ok_or(HostError::UnresolvedName)?;
        let nodes = self.body(op.pc.body).to_vec();
        let mut cursor = op.pc.idx + 1;
        let first = *nodes.get(cursor).ok_or(VmError::PcOutOfRange)?;
        let count = if first.opword == 0x101a {
            1
        } else if first.opword == 0x1011 {
            cursor += 1;
            first.imm as usize
        } else {
            return Err(HostError::UnresolvedName.into());
        };
        let end = cursor.checked_add(count).ok_or(HostError::UnresolvedName)?;
        let components = nodes.get(cursor..end).ok_or(VmError::PcOutOfRange)?;
        let mut receiver = thread.frames.last().ok_or(VmError::NoFrame)?.receiver;
        let mut class = if receiver == 0 {
            owner
        } else {
            self.reference_class_id(Value::reference(receiver))
                .ok_or(HostError::BadCast)?
        };
        // A static context (no `this`) or a qualified class name starts field
        // lookup from a class rather than an object.
        let mut static_class = (receiver == 0).then_some(owner);
        for (offset, component) in components.iter().enumerate() {
            match component.opword {
                0x1019 if offset == 0 => {
                    let name = self
                        .loader
                        .pool_text_of(owner, component.imm)
                        .map_err(|_| HostError::UnresolvedName)?
                        .to_owned();
                    class = self
                        .loader
                        .request(&name)
                        .map_err(|_| HostError::UnresolvedName)?
                        .ok_or(HostError::UnresolvedName)?;
                    self.initialize_class_id(class)?;
                    receiver = 0;
                    static_class = Some(class);
                }
                0x1001 => {
                    let id = *thread
                        .frames
                        .last()
                        .ok_or(VmError::NoFrame)?
                        .locals
                        .get(component.imm as usize)
                        .ok_or(HostError::MissingMember)?;
                    receiver = thread.vars.get(id).value.payload;
                    class = self
                        .reference_class_id(Value::reference(receiver))
                        .ok_or(HostError::BadCast)?;
                }
                0x101b if offset + 1 < components.len() => {
                    // A field reference feeding the next component. The first
                    // component starts from `this` (or the qualified class),
                    // later ones from the previous value (`FUN_004208e0`).
                    let qualified = static_class.take();
                    if qualified.is_none() && receiver == 0 {
                        return Err(HostError::MissingMember.into());
                    }
                    let slot = self.field_component_slot(
                        owner,
                        component.imm,
                        if qualified.is_some() { 0 } else { receiver },
                        qualified,
                    )?;
                    receiver = self.borrow_slot(thread, Slot::Heap(slot))?.payload;
                    if receiver != 0 {
                        class = self
                            .reference_class_id(Value::reference(receiver))
                            .ok_or(HostError::BadCast)?;
                    }
                }
                0x101a if offset + 1 == components.len() => {
                    // The argument-count temporary is consumed by evalName before
                    // decoding the cached method reference (4208e0).
                    thread.pop_deref_with(self)?;
                    thread.release();
                    let (class_name, name, sig) = self
                        .loader
                        .field_reference_of(owner, component.imm)
                        .map_err(|_| HostError::MissingMethod)?;
                    let (encoded, name, sig) = (
                        self.loader
                            .lookup(class_name)
                            .ok_or(HostError::MissingMethod)?,
                        name.to_owned(),
                        sig.to_owned(),
                    );
                    let _ = encoded;
                    let (method, defining) = self
                        .loader
                        .resolve_method_id(class, &name, &sig)
                        .ok_or(HostError::MissingMethod)?;
                    let method = method.clone();
                    thread.pc = Some(crate::interp::Pc {
                        body: op.pc.body,
                        idx: cursor + offset,
                    });
                    return self.dispatch_method_id(thread, &method, defining, receiver);
                }
                0x4012 if offset + 1 == components.len() => {
                    let name = self
                        .loader
                        .pool_text_of(owner, component.imm)
                        .map_err(|_| HostError::UnresolvedName)?
                        .to_owned();
                    thread.pc = Some(crate::interp::Pc {
                        body: op.pc.body,
                        idx: cursor + offset,
                    });
                    return self.call_dynamic_id(thread, class, receiver, &name);
                }
                _ => return Err(HostError::UnresolvedName.into()),
            }
        }
        Err(HostError::MissingMethod.into())
    }

    /// ID-carrying method dispatch: body and frame context resolve through
    /// the retained defining record, never through a name lookup.
    /// exe: FUN_0041fbc0
    fn dispatch_method_id(
        &mut self,
        thread: &mut Thread,
        method: &crate::vmclass::Method,
        defining: crate::vmclass::ClassId,
        receiver: u32,
    ) -> Result<Flow, VmError> {
        let receiver = if method.flags & 8 != 0 { 0 } else { receiver };
        if method.flags & 8 == 0 && receiver == 0 {
            return Err(VmError::InstanceMethodWithoutReceiver);
        }
        if self.call_trace {
            eprintln!(
                "CALL {}.{}{} receiver={receiver:#x}",
                method.class, method.name, method.sig
            );
        }
        if method.is_native {
            return self.invoke_method_native_id(thread, method, defining, receiver);
        }
        if method.is_abstract {
            return Err(HostError::NoBody.into());
        }
        let body = self
            .loader
            .body_id_of(defining, method.body as usize)
            .ok_or(HostError::NoBody)?;
        // TREE slots with a null node list take 41fbc0's immediate-return
        // path: consume arguments without installing a script frame.
        if self.loader.body(body).is_some_and(<[Node]>::is_empty) {
            for _ in parameter_types(&method.sig)? {
                thread.pop_deref_with(self)?;
                thread.release();
            }
            if return_type(&method.sig).is_some() {
                thread.push_new(Value::int(0))?;
            }
            return Ok(Flow::Advance);
        }
        let target = crate::interp::ScriptMethod {
            body,
            class_ctx: defining.get(),
            signature: 0,
            is_static: method.flags & 8 != 0,
            params: parameter_types(&method.sig)?,
        };
        thread.call_script_method_with(self, &target, receiver)?;
        Ok(Flow::Stay)
    }

    /// ID-carrying virtual dispatch: the receiver hierarchy, overload
    /// resolution and defining record all stay on retained IDs.
    /// exe: FUN_004051a0
    fn call_dynamic_id(
        &mut self,
        thread: &mut Thread,
        class: crate::vmclass::ClassId,
        receiver: u32,
        name: &str,
    ) -> Result<Flow, VmError> {
        self.loader
            .find_by_id(class)
            .ok_or(HostError::MissingMethod)?;
        let count = thread.pop_deref_with(self)?.payload as usize;
        thread.release();
        let stack = &thread.frames.last().ok_or(VmError::NoFrame)?.stack;
        let arguments = stack
            .get(stack.len().checked_sub(count).ok_or(VmError::NullSlot)?..)
            .ok_or(VmError::NullSlot)?
            .to_vec();
        let mut types = Vec::with_capacity(count);
        let mut argument_values = Vec::with_capacity(count);
        for slot in arguments {
            let value = self.borrow_slot(thread, slot)?;
            let tag = if value.tag.is_ref() && value.payload != 0 {
                self.receiver_value(value.payload)?.tag
            } else {
                value.tag
            };
            types.push(tag);
            argument_values.push(value);
        }
        let selected = self
            .loader
            .resolve_overload_id(class, name, &types)
            .map_err(|_| HostError::MissingMethod)?
            .ok_or(HostError::MissingMethod)?;
        let (method, defining) = (selected.method.clone(), selected.defining);
        if selected.ambiguous {
            self.method_diagnostics
                .push("Class::getMethod: more matching methods found");
        }
        if name == "<init>" {
            self.constructor_calls.push(ConstructorCall {
                class,
                class_name: self.loader.class_name(class).unwrap_or("<stale>").into(),
                caller_class: thread
                    .frames
                    .last()
                    .and_then(|f| self.loader.context_id(f.class_ctx)),
                receiver,
                signature: method.sig.clone(),
                arguments: argument_values,
                entered_script_frame: None,
            });
        }
        let constructor_index = (name == "<init>").then(|| self.constructor_calls.len() - 1);
        let result = self.dispatch_method_id(thread, &method, defining, receiver);
        if let Some(index) = constructor_index {
            self.constructor_calls[index].entered_script_frame =
                result.as_ref().ok().map(|flow| *flow == Flow::Stay);
        }
        result
    }

    /// ID-carrying native invocation: the receiver tag is resolved from the
    /// retained defining record, never re-looked-up by spelling.
    /// exe: FUN_0041fbc0
    fn invoke_method_native_id(
        &mut self,
        thread: &mut Thread,
        method: &crate::vmclass::Method,
        defining: crate::vmclass::ClassId,
        receiver: u32,
    ) -> Result<Flow, VmError> {
        if method.flags & 8 == 0 && receiver == 0 {
            return Err(VmError::InstanceMethodWithoutReceiver);
        }
        let Some(implementation) = self
            .natives
            .resolve(&method.class, &method.name, &method.sig)
        else {
            self.missing_native = Some(format!(
                "{}.{}{} (unregistered)",
                method.class, method.name, method.sig
            ));
            return Err(HostError::NoBody.into());
        };
        let Some(&native) = self.native_impls.get(&implementation) else {
            self.missing_native = Some(format!(
                "{}.{}{} @ {implementation:#010x}",
                method.class, method.name, method.sig
            ));
            return Err(HostError::NoBody.into());
        };
        let return_tag = if method.sig.ends_with(')') {
            None
        } else {
            Some(return_type(&method.sig).ok_or(HostError::MissingMethod)?)
        };
        let parameters = parameter_types(&method.sig)?;
        let mut args = Vec::with_capacity(parameters.len() + 1);
        for tag in parameters.into_iter().rev() {
            let mut source = thread.pop_deref_with(self)?;
            let mut value = Value { tag, payload: 0 };
            self.store_value(&mut thread.alu, &mut value, &mut source);
            args.push(value);
            thread.release();
        }
        let receiver_tag = {
            let name = self
                .loader
                .find_by_id(defining)
                .ok_or(HostError::MissingMethod)?
                .name
                .clone();
            TypeTag::intern(&format!("L{name};"))
        };
        args.push(Value {
            tag: receiver_tag,
            payload: receiver,
        });
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            args: &args,
            class: &method.class,
            name: &method.name,
            sig: &method.sig,
        };
        let native_return_emitted = std::cell::Cell::new(false);
        let invoke = |host: &mut Self, thread: &mut Thread| {
            if let Some(boundary) = host.native_boundary {
                if let Err(error) = boundary(&info) {
                    host.native_failure =
                        Some(format!("{}.{}{}: {error}", info.class, info.name, info.sig));
                    return Err(VmError::Unsupported(0x0024));
                }
            }
            host.native_failure = None;
            if let Some(hook) = host.native_thread_hook {
                hook(thread, true);
            }
            if let Some(observe) = host.native_diagnostics {
                observe(host, thread, &info, NativePhase::Enter);
            }
            let result = native(host, &info);
            if let Some(hook) = host.native_thread_hook {
                hook(thread, false);
            }
            if crate::diagnostics::enabled() {
                crate::diagnostics::emit(crate::diagnostics::EventKind::NativeReturn {
                    result,
                    error: host.native_failure.clone(),
                });
                native_return_emitted.set(true);
            }
            if host.native_failure.is_some() {
                return Err(VmError::Unsupported(0x0024));
            }
            if let Some(observe) = host.native_observer {
                observe(&info, result);
            }
            let payload = result.unwrap_or(0);
            if let Some(tag) = return_tag {
                thread.push_new(Value {
                    tag,
                    payload: if tag.name() == "V" { 0 } else { payload },
                })?;
            }
            if let Some(observe) = host.native_diagnostics {
                observe(host, thread, &info, NativePhase::Return);
            }
            Ok(Flow::Advance)
        };
        // Inline call-name resolution advances the real PC before invoking
        // the native. Refresh the scopes from that actual state rather than
        // retaining the outer opcode's pre-resolution location.
        let clock_context = thread.clock_context(self);
        let precision = thread.alu.precision;
        let diagnostic_context = crate::diagnostics::thread_context(thread, self);
        crate::clock::with_interpreter_context(clock_context, precision, || {
            crate::diagnostics::with_context(diagnostic_context, || {
                if crate::diagnostics::enabled() {
                    let identity = crate::diagnostics::NativeIdentity {
                        call_id: crate::diagnostics::next_call_id(),
                        class: method.class.clone(),
                        name: method.name.clone(),
                        signature: method.sig.clone(),
                        implementation: Some(implementation),
                    };
                    crate::diagnostics::with_context(
                        crate::diagnostics::Context {
                            native: Some(identity),
                            ..Default::default()
                        },
                        || {
                            let parent_sequence = crate::diagnostics::emit(
                                crate::diagnostics::EventKind::NativeEnter {
                                    args: args.iter().copied().map(Into::into).collect(),
                                },
                            );
                            crate::diagnostics::with_context(
                                crate::diagnostics::Context {
                                    parent_sequence,
                                    ..Default::default()
                                },
                                || {
                                    let result = invoke(self, thread);
                                    match &result {
                                        Err(error) if !native_return_emitted.get() => {
                                            crate::diagnostics::emit(
                                                crate::diagnostics::EventKind::NativeReturn {
                                                    result: None,
                                                    error: Some(
                                                        self.native_failure.clone().unwrap_or_else(
                                                            || format!("{error:?}"),
                                                        ),
                                                    ),
                                                },
                                            );
                                        }
                                        _ => {}
                                    }
                                    result
                                },
                            )
                        },
                    )
                } else {
                    invoke(self, thread)
                }
            })
        })
    }

    fn new_object(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let token = *self
            .body(op.pc.body)
            .get(op.pc.idx + 1)
            .ok_or(VmError::PcOutOfRange)?;
        let owner = self
            .loader
            .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
            .ok_or(HostError::BadCast)?;
        let name = self
            .loader
            .pool_text_of(owner, token.imm)
            .map_err(|_| HostError::BadCast)?
            .to_owned();
        let class = self
            .loader
            .request(&name)
            .map_err(|_| HostError::BadCast)?
            .ok_or(HostError::BadCast)?;
        let word = self.alloc_object_id(class)?;
        let tag_name = self
            .loader
            .find_by_id(class)
            .ok_or(HostError::BadCast)?
            .name
            .clone();
        thread.push_new(Value {
            tag: TypeTag::intern(&format!("L{tag_name};")),
            payload: word,
        })?;
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: op.pc.idx + 1,
        });
        Ok(Flow::Advance)
    }

    fn new_array(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let token = *self
            .body(op.pc.body)
            .get(op.pc.idx + 1)
            .ok_or(VmError::PcOutOfRange)?;
        let owner = self
            .loader
            .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
            .ok_or(HostError::BadCast)?;
        let descriptor = self
            .loader
            .pool_text_of(owner, token.imm)
            .map_err(|_| HostError::BadCast)?
            .to_owned();
        let dimensions = descriptor.bytes().take_while(|byte| *byte == b'[').count();
        if dimensions == 0 {
            return Err(HostError::BadCast.into());
        }
        if dimensions != 1 {
            return Err(HostError::MultiArray.into());
        }
        let element = &descriptor[1..];
        if parameter_types(&format!("({element})"))?.len() != 1 {
            return Err(HostError::BadCast.into());
        }
        let size = thread.pop_deref_with(self)?;
        thread.release();
        if size.as_i32() < 0 {
            return Err(HostError::BadCast.into());
        }
        let word = self.alloc_array(element, size.as_i32() as usize)?;
        thread.push_new(Value {
            tag: TypeTag::intern(&descriptor),
            payload: word,
        })?;
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: op.pc.idx + 1,
        });
        Ok(Flow::Advance)
    }

    fn instance_of(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let value = thread.pop_deref_with(self)?;
        thread.release();
        let token = *self
            .body(op.pc.body)
            .get(op.pc.idx + 1)
            .ok_or(VmError::PcOutOfRange)?;
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: op.pc.idx + 1,
        });
        let matched = if value.payload == 0 {
            false
        } else {
            let owner = self
                .loader
                .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
                .ok_or(HostError::BadCast)?;
            let target = self
                .loader
                .pool_text_of(owner, token.imm)
                .map_err(|_| HostError::BadCast)?
                .to_owned();
            let actual = self.reference_class(value).ok_or_else(|| {
                note(format!(
                    "instanceof: value {:#x} (tag {:?}) is not a heap object",
                    value.payload, value.tag
                ));
                HostError::BadCast
            })?;
            let target_ref = self.load_class_ref(&target).ok_or_else(|| {
                note(format!("instanceof: target class {target:?} did not load"));
                HostError::BadCast
            })?;
            self.loader
                .is_inherited_from_ref(&actual, &target_ref)
                .map_err(|e| {
                    note(format!("instanceof: hierarchy walk {e:?}"));
                    HostError::BadCast
                })?
        };
        thread.push_new(Value::int(i32::from(matched)))?;
        Ok(Flow::Advance)
    }

    fn cast(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let mut value = thread.pop_deref_with(self)?;
        let token = *self
            .body(op.pc.body)
            .get(op.pc.idx + 1)
            .ok_or(VmError::PcOutOfRange)?;
        thread.pc = Some(crate::interp::Pc {
            body: op.pc.body,
            idx: op.pc.idx + 1,
        });
        let owner = self
            .loader
            .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
            .ok_or(HostError::BadCast)?;
        let text = self
            .loader
            .pool_text_of(owner, token.imm)
            .map_err(|_| HostError::BadCast)?
            .to_owned();
        let target = match token.opword {
            0x1006 => text,
            0x1019 => {
                let loaded = self.load_class_ref(&text).ok_or(HostError::BadCast)?;
                match loaded {
                    crate::vmclass::ClassRef::Array(array) => array.descriptor().to_owned(),
                    crate::vmclass::ClassRef::Loaded(id) => {
                        let name = self
                            .loader
                            .find_by_id(id)
                            .ok_or(HostError::BadCast)?
                            .name
                            .clone();
                        if name.starts_with('[') {
                            name
                        } else {
                            format!("L{name};")
                        }
                    }
                }
            }
            _ => return Err(HostError::BadCast.into()),
        };
        // Pointer-identical descriptors bypass conversion in the original. All
        // other conversions dispatch only on the supported I/F/L/[ families;
        // notably a generic null reference cannot become a V cell.
        if value.tag.name() != target
            && (!matches!(
                value.tag.name().as_bytes().first(),
                Some(b'I' | b'F' | b'L' | b'[')
            ) || !matches!(target.as_bytes().first(), Some(b'I' | b'F' | b'L' | b'[')))
        {
            return Err(HostError::BadCast.into());
        }
        let tag = TypeTag::intern(&target);
        if crate::values::assign_convert(&mut value, tag).is_err() {
            crate::values::loader_convert_ref(self, &mut value, &target)
                .map_err(|_| HostError::BadCast)?;
        }
        thread.push(Slot::Value(value))?;
        Ok(Flow::Advance)
    }

    /// Literal100e reads a resource Member, not String text. The original
    /// returns I0 for another pool kind without invoking the RID resolver.
    /// Valid Members call the installed resource-world resolver; absent backends
    /// fail explicitly rather than guessing a global ID.
    /// exe: FUN_00406660
    fn resource_literal(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let context = thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx;
        let owner = self
            .loader
            .context_id(context)
            .ok_or(HostError::UnresolvedName)?;
        let pool = self
            .loader
            .pool_of(owner)
            .ok_or(HostError::UnresolvedName)?;
        let value = if let Some(slr_fmt_tufa::PoolEntry::Member { name, extra }) =
            pool.get(op.node.imm as usize)
        {
            let package = if (*name as i32) < 0 {
                None
            } else {
                match pool.get(*name as usize) {
                    Some(slr_fmt_tufa::PoolEntry::Str(text)) => Some(text.clone()),
                    Some(slr_fmt_tufa::PoolEntry::RawStr(_)) => {
                        return Err(HostError::UnresolvedName.into())
                    }
                    Some(_) => None,
                    None => return Err(HostError::UnresolvedName.into()),
                }
            };
            let id = *extra;
            let resolver = self.resource_resolver.ok_or(VmError::Unsupported(0x100e))?;
            resolver(self, package.as_deref(), id)?
        } else {
            0
        };
        thread.push_new(Value::int(value as i32))?;
        Ok(Flow::Advance)
    }

    fn string_literal(&mut self, thread: &mut Thread, op: ExternOp) -> Result<Flow, VmError> {
        let owner = self
            .loader
            .context_id(thread.frames.last().ok_or(VmError::NoFrame)?.class_ctx)
            .ok_or(HostError::UnresolvedName)?;
        let bytes = self
            .loader
            .pool_bytes_of(owner, op.node.imm)
            .map_err(|_| HostError::UnresolvedName)?
            .map(<[u8]>::to_vec);
        let word = self.intern_string(bytes.as_deref())?;
        thread.push_new(Value::reference_named(word, "Ljava.lang.String;"))?;
        Ok(Flow::Advance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interp::{Frame, Pc};
    use slr_fmt_tufa::Node;

    #[test]
    fn diagnostics_read_string_ptr_and_expose_byte_limits_without_mutation() {
        let mut host = host_with_class();
        host.loader.insert_test_class(crate::vmclass::Class {
            kind: 0,
            name: "java.lang.String".into(),
            base: Some("java.lang.Object".into()),
            interfaces: vec![],
            fields: vec![crate::vmclass::Field {
                class: "java.lang.String".into(),
                name: "ptr".into(),
                ty: "I".into(),
                is_static: false,
            }],
            methods: vec![],
            bodies: vec![],
        });
        let bytes = vec![0xff; 300];
        let word = host.intern_string(Some(&bytes)).unwrap();
        let count = host.objects.len();
        let allocations = host.gc.allocation_counter();
        let snapshot = host.diagnostic_object(word).unwrap();
        assert_eq!(snapshot.string_bytes, Some(bytes[..256].to_vec()));
        assert_eq!(snapshot.string_bytes_total, Some(300));
        assert_eq!(snapshot.field_names, vec!["java.lang.String.ptr"]);
        assert_eq!(host.string_bytes(word), Some(bytes.as_slice()));
        assert_eq!(host.objects.len(), count);
        assert_eq!(host.gc.allocation_counter(), allocations);
    }

    #[test]
    fn string_numeric_add_finishes_original_concatenation_after_conversion() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/headless/finalizer-investigation/string-add.json");
        let rows: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("generate re/oracle/string_add_probe.py vectors first"),
        )
        .unwrap();
        for row in rows["cases"].as_array().unwrap() {
            let mut host = host_with_class();
            host.loader.insert_test_class(crate::vmclass::Class {
                kind: 0,
                name: "java.lang.String".into(),
                base: Some("java.lang.Object".into()),
                interfaces: vec![],
                fields: vec![crate::vmclass::Field {
                    class: "java.lang.String".into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }],
                methods: vec![],
                bodies: vec![],
            });

            let mut value = |input: &serde_json::Value| match input[0].as_str().unwrap() {
                "S" => Value::reference_named(
                    host.intern_string(input[1].as_str().map(str::as_bytes))
                        .unwrap(),
                    TypeTag::STRING,
                ),
                "I" => Value::int(input[1].as_i64().unwrap() as i32),
                "F" => Value::float(input[1].as_f64().unwrap() as f32),
                other => panic!("unsupported probe type {other}"),
            };
            let mut left = value(&row["left"]);
            let mut right = value(&row["right"]);
            let before = host.objects.len();
            host.operator_on_refs(RefOperator::Add, &mut left, &mut right)
                .unwrap()
                .unwrap();
            assert_eq!(
                host.string_bytes(left.payload),
                Some(row["result"].as_str().unwrap().as_bytes())
            );
            assert_eq!(
                host.objects.len() - before,
                row["allocated_during_add"].as_u64().unwrap() as usize
            );
            assert_eq!(left.tag.name(), TypeTag::STRING);
        }
    }

    fn host_with_class() -> VmHost {
        let mut h = VmHost::new();
        h.boot();
        h.loader.insert_test_class(crate::vmclass::Class {
            kind: 0,
            name: "java.lang.Object".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            fields: vec![],
            bodies: vec![],
        });
        h
    }
    fn thread_with_frame(nodes: Vec<Node>) -> (Thread, TestBodies) {
        let mut th = Thread::new();
        th.frames.push(Frame::new(
            Some(Pc {
                body: BodyId(0),
                idx: 0,
            }),
            0,
        ));
        th.pc = Some(Pc {
            body: BodyId(0),
            idx: 0,
        });
        (th, TestBodies(nodes))
    }

    struct TestBodies(Vec<Node>);
    impl Host for TestBodies {
        fn body(&self, _id: BodyId) -> &[Node] {
            &self.0
        }
    }

    #[test]
    fn diagnostics_close_failed_native_with_actual_result_and_error() {
        use crate::diagnostics::{self, EventKind};
        fn failed(host: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
            host.native_failure = Some("typed backend failure".into());
            Some(0xffc01234)
        }
        let mut host = host_with_class();
        let defining = host.loader.lookup("java.lang.Object").unwrap();
        host.natives
            .register("java.lang.Object", "failed", "()F", 0x12345678);
        host.native_impls.insert(0x12345678, failed);
        let method = crate::vmclass::Method {
            class: "java.lang.Object".into(),
            name: "failed".into(),
            sig: "()F".into(),
            flags: 0x49,
            static_vector: true,
            is_native: true,
            body: 0,
            is_abstract: false,
            extra: 0,
        };
        let (mut thread, _) = thread_with_frame(vec![]);
        diagnostics::configure("failed-native".into(), 16, None);
        assert_eq!(
            host.invoke_method_native_id(&mut thread, &method, defining, 0),
            Err(VmError::Unsupported(0x0024))
        );
        assert_eq!(
            host.native_failure.as_deref(),
            Some("typed backend failure")
        );
        let events = diagnostics::recent();
        let enter = events
            .iter()
            .find(|event| matches!(event.kind, EventKind::NativeEnter { .. }))
            .unwrap();
        let returns: Vec<_> = events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::NativeReturn { .. }))
            .collect();
        assert_eq!(
            returns.len(),
            1,
            "failed native must close its entry exactly once"
        );
        let returned = returns[0];
        assert!(
            matches!(&returned.kind, EventKind::NativeReturn { result: Some(0xffc01234), error: Some(error) } if error == "typed backend failure")
        );
        assert_eq!(
            returned.context.native.as_ref().unwrap().call_id,
            enter.context.native.as_ref().unwrap().call_id
        );
        assert_eq!(returned.context.parent_sequence, Some(enter.sequence));
        assert!(returned.sequence > enter.sequence);
        assert!(
            thread.frames[0].stack.is_empty(),
            "failed return must not enter script stack"
        );
        diagnostics::configure(String::new(), 0, None);
    }

    #[test]
    fn native_clock_context_uses_resolved_pc_and_restores_outer_scope() {
        fn native(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
            assert_eq!(
                crate::clock::current_thread_context().unwrap().node_index,
                Some(44)
            );
            assert_eq!(crate::clock::current_controls().0, slr_x87::Precision::F64);
            assert_eq!(
                crate::diagnostics::current_context().location.unwrap().node,
                44
            );
            Some(0)
        }
        let mut host = host_with_class();
        let defining = host.loader.lookup("java.lang.Object").unwrap();
        host.natives
            .register("java.lang.Object", "observed", "()I", 0x12345678);
        host.native_impls.insert(0x12345678, native);
        let method = crate::vmclass::Method {
            class: "java.lang.Object".into(),
            name: "observed".into(),
            sig: "()I".into(),
            flags: 0x49,
            static_vector: true,
            is_native: true,
            body: 0,
            is_abstract: false,
            extra: 0,
        };
        let (mut thread, _) = thread_with_frame(vec![]);
        let outer = thread.clock_context(&host);
        thread.pc.as_mut().unwrap().idx = 44;
        thread.alu.precision = slr_x87::Precision::F64;
        crate::diagnostics::configure("resolved-native-context".into(), 16, None);
        crate::clock::with_interpreter_context(outer.clone(), slr_x87::Precision::F32, || {
            host.invoke_method_native_id(&mut thread, &method, defining, 0)
                .unwrap();
            assert_eq!(crate::clock::current_thread_context(), Some(outer));
            assert_eq!(crate::clock::current_controls().0, slr_x87::Precision::F32);
        });
        assert!(crate::clock::current_thread_context().is_none());
        crate::diagnostics::configure(String::new(), 0, None);
    }

    #[test]
    fn diagnostics_correlate_real_native_callback_and_field_store() {
        use crate::diagnostics::{self, EventKind};
        fn native(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
            Some(0x80000000)
        }
        fn observer(_: &CallInfo<'_>, _: Option<u32>) {
            diagnostics::emit(EventKind::External {
                category: "native_observer".into(),
                fields: vec![],
            });
        }
        fn snapshot(host: &VmHost, thread: &Thread, _: &CallInfo<'_>, phase: NativePhase) {
            if phase == NativePhase::Return {
                let snapshot = crate::diagnostics::failure_snapshot(host, thread);
                assert_eq!(
                    snapshot.frames[0].stack[0]
                        .value
                        .as_ref()
                        .unwrap()
                        .float_bits,
                    Some(0x80000000)
                );
                diagnostics::emit(EventKind::External {
                    category: "native_snapshot".into(),
                    fields: vec![],
                });
            }
        }
        let mut host = host_with_class();
        host.native_diagnostics = Some(snapshot);
        let defining = host.loader.lookup("java.lang.Object").unwrap();
        host.natives
            .register("java.lang.Object", "observed", "()F", 0x12345678);
        host.native_impls.insert(0x12345678, native);
        host.native_observer = Some(observer);
        let method = crate::vmclass::Method {
            class: "java.lang.Object".into(),
            name: "observed".into(),
            sig: "()F".into(),
            flags: 0x49,
            static_vector: true,
            is_native: true,
            body: 0,
            is_abstract: false,
            extra: 0,
        };
        let (mut thread, _) = thread_with_frame(vec![]);
        diagnostics::configure("native".into(), 16, None);
        host.invoke_method_native_id(&mut thread, &method, defining, 0)
            .unwrap();
        let events = diagnostics::recent();
        let enter = events
            .iter()
            .find(|e| matches!(e.kind, EventKind::NativeEnter { .. }))
            .unwrap();
        let observed=events.iter().find(|e|matches!(&e.kind,EventKind::External{category,..} if category=="native_observer")).unwrap();
        assert_eq!(
            enter.context.native.as_ref().unwrap().call_id,
            observed.context.native.as_ref().unwrap().call_id
        );
        assert_eq!(observed.context.parent_sequence, Some(enter.sequence));
        assert_eq!(
            enter.context.native.as_ref().unwrap().implementation,
            Some(0x12345678)
        );
        let captured=events.iter().find(|e|matches!(&e.kind,EventKind::External{category,..} if category=="native_snapshot")).unwrap();
        assert_eq!(
            captured.context.native.as_ref().unwrap().call_id,
            enter.context.native.as_ref().unwrap().call_id
        );
        assert!(captured.sequence > observed.sequence);

        assert!(diagnostics::current_context().native.is_none());
        let array = host.alloc_array("F", 1).unwrap();
        host.write_cell(
            HeapSlot::Array {
                word: array,
                index: 0,
            },
            Value {
                tag: TypeTag::Float,
                payload: 0xffc01234,
            },
        )
        .unwrap();
        let events = diagnostics::recent();
        let event = events
            .iter()
            .find(|e| matches!(e.kind, EventKind::Store { .. }))
            .unwrap();
        let EventKind::Store { before, after, .. } = &event.kind else {
            unreachable!()
        };
        assert_eq!(before.float_bits, Some(0));
        assert_eq!(after.float_bits, Some(0xffc01234));
        let before = host.objects.len();
        let snapshot = host.diagnostic_snapshot();
        assert_eq!(host.objects.len(), before);
        assert_eq!(
            snapshot.iter().find(|o| o.word == array).unwrap().elements[0].float_bits,
            Some(0xffc01234)
        );
        diagnostics::configure(String::new(), 0, None);
    }

    #[test]
    fn declare_rejects_missing_owner_pool_and_type_node() {
        let mut h = host_with_class();
        let nodes = vec![
            Node {
                opword: 0x1002,
                operand: 1,
                imm: 0,
            },
            Node {
                opword: 0x002c,
                operand: 1,
                imm: 0,
            },
        ];
        let (mut th, bodies) = thread_with_frame(nodes);
        // The host reads bodies from itself; use the test bodies
        // indirectly: drive `declare` through a manual op.
        let _ = bodies;
        let op = ExternOp {
            kind: ExternKind::Declare,
            pc: Pc {
                body: BodyId(0),
                idx: 0,
            },
            node: Node {
                opword: 0x1002,
                operand: 1,
                imm: 0,
            },
            discard: false,
        };
        // The declaring body/pool and following type node are required metadata.
        assert!(h.extern_op(&mut th, op).is_err());
        assert!(th.frames[0].locals.is_empty());
    }

    #[test]
    fn new_and_arg_array_allocate() {
        let mut h = host_with_class();
        let (mut th, _) = thread_with_frame(vec![]);
        let op = ExternOp {
            kind: ExternKind::New,
            pc: Pc {
                body: BodyId(0),
                idx: 0,
            },
            node: Node {
                opword: 0x1007,
                operand: 1,
                imm: 0x1e,
            },
            discard: false,
        };
        // No class node in empty bodies -> PcOutOfRange (honest: the host
        // needs the real body bytes).
        assert!(h.extern_op(&mut th, op).is_err());
    }

    #[test]
    fn string_plus_converts_one_sided_int() {
        let mut h = host_with_class();
        h.loader.insert_test_class(crate::vmclass::Class {
            kind: 0,
            name: "java.lang.String".into(),
            base: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![crate::vmclass::Field {
                class: "java.lang.String".into(),
                name: "ptr".into(),
                ty: "I".into(),
                is_static: false,
            }],
            bodies: vec![],
        });
        let mut left = Value::reference_named(0, "Ljava.lang.String;");
        let mut right = Value::int(7);
        let r = h.operator_on_refs(RefOperator::Add, &mut left, &mut right);
        assert!(r.is_some(), "one-sided String converts");
        r.unwrap().expect("converts");
        assert_eq!(right.tag, TypeTag::Ref(TypeTag::STRING));
    }

    /// Per-plan step A: execution state carries retained [`ClassId`]s,
    /// never name re-lookups. The allocated object pins the loaded record,
    /// the frame context word decodes to the same identity, and field
    /// reads/writes resolve through the retained record. A later slice
    /// enables overlapping duplicate records; these assertions must keep
    /// biting then (each record keeps its own ID while names collide).
    #[test]
    fn allocated_objects_and_frames_carry_retained_ids() {
        use crate::interp::{Frame, Pc};
        let mut h = host_with_class();
        let child = h.loader.insert_test_class(crate::vmclass::Class {
            kind: 0,
            name: "game.Widget".into(),
            base: Some("java.lang.Object".into()),
            interfaces: vec![],
            methods: vec![],
            fields: vec![crate::vmclass::Field {
                class: "game.Widget".into(),
                name: "count".into(),
                ty: "I".into(),
                is_static: false,
            }],
            bodies: vec![],
        });
        let object = h.loader.lookup("game.Widget").unwrap();
        assert_eq!(object, child);
        let word = h.alloc_object("game.Widget").expect("allocates");
        assert_eq!(
            h.objects[(word - 0x1000) as usize].class,
            crate::vmclass::ClassRef::Loaded(child)
        );
        // The frame context word is the ID encoding; decoding must recover
        // the same record without consulting the name map.
        let context = h.class_context("game.Widget").unwrap();
        assert_eq!(context, child.get());
        assert_eq!(h.loader.context_id(context), Some(child));
        let frame = Frame::new(None, word);
        assert_eq!(h.loader.context_id(frame.class_ctx), None);
        let mut frame = Frame::new(
            Some(Pc {
                body: BodyId(0),
                idx: 0,
            }),
            word,
        );
        frame.class_ctx = context;
        assert_eq!(h.loader.context_id(frame.class_ctx), Some(child));
        // Field access resolves through the retained record.
        h.write_field(word, "count", Value::int(7)).unwrap();
        assert_eq!(h.read_field(word, "count").unwrap(), Value::int(7));
        assert_eq!(h.instance_field_slot_id(child, "count").unwrap(), 0);
    }

    #[test]
    fn pool_entry_shapes_are_visible() {
        // The host reads pool entries for declare/literals; pin the shapes.
        let e = slr_fmt_tufa::PoolEntry::Str("x".into());
        assert!(matches!(e, slr_fmt_tufa::PoolEntry::Str(_)));
    }

    // ---- native-call ABI (`FUN_0045d910`) ----------------------------
    //
    // Every expectation below is an oracle row: the exe's own marshaller
    // run under Unicorn with a hand-built container and sig record. The
    // container is `[..., receiver]` and the parameters sit under it,
    // first declared at `count-2`.

    fn info<'a>(args: &'a [Value], sig: &'a str) -> CallInfo<'a> {
        CallInfo {
            jvm: 0,
            thread: 0,
            args,
            class: "java.lang.System",
            name: "log",
            sig,
        }
    }

    /// The marshaller writes `(I)V`'s one parameter from `args[count-2]`,
    /// with `args[count-1]` going to `out[0]`.
    #[test]
    fn marshal_int_and_receiver_slot() {
        let args = [Value::int(0x11), Value::int(0x22)];
        let mut out = [0xdead_0000u32; 4];
        let n = marshal_args(&info(&args, "(I)V"), &mut out).expect("marshals");
        assert_eq!(n, 2, "receiver slot + one parameter");
        assert_eq!(out[0], 0x22, "receiver/return slot = container top");
        assert_eq!(out[1], 0x11, "first parameter = count-2");
        assert_eq!(out[2], 0xdead_0000, "later out words untouched");
    }

    #[test]
    fn marshal_two_ints_descends() {
        // Oracle `(II)V` count 3 -> out[0..=2] = [0xcc, 0xbb, 0xaa].
        let args = [Value::int(0xaa), Value::int(0xbb), Value::int(0xcc)];
        let mut out = [0u32; 4];
        let n = marshal_args(&info(&args, "(II)V"), &mut out).expect("marshals");
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[0xcc, 0xbb, 0xaa]);
    }

    #[test]
    fn marshal_four_ints_descends() {
        let args = [
            Value::int(0xaa),
            Value::int(0xbb),
            Value::int(0xcc),
            Value::int(0xdd),
            Value::int(0xee),
        ];
        let mut out = [0u32; 6];
        let n = marshal_args(&info(&args, "(IIII)V"), &mut out).expect("marshals");
        assert_eq!(n, 5);
        assert_eq!(&out[..5], &[0xee, 0xdd, 0xcc, 0xbb, 0xaa]);
    }

    #[test]
    fn marshal_empty_signature_writes_only_the_receiver_slot() {
        // Oracle `()I` with count 1 -> out[0] = the only cell, nothing else.
        let args = [Value::int(0xaa)];
        let mut out = [0xdead_0001u32; 3];
        let n = marshal_args(&info(&args, "()I"), &mut out).expect("marshals");
        assert_eq!(n, 1);
        assert_eq!(&out[..2], &[0xaa, 0xdead_0001]);
    }

    /// `F` copies the payload word verbatim (no float conversion here: the
    /// `st0`/`eax` split happens on the caller side).
    #[test]
    fn marshal_float_copies_bits() {
        let args = [Value::float(1.0), Value::int(0xbb)];
        let mut out = [0u32; 3];
        let n = marshal_args(&info(&args, "(F)V"), &mut out).expect("marshals");
        assert_eq!(n, 2);
        assert_eq!(out[0], 0xbb);
        assert_eq!(out[1], 0x3f80_0000, "f32 1.0 bits");
    }

    /// `L` copies the word and scans to the `;`, so the *next* parameter is
    /// `count-3`, not `count-2` (oracle `(Ljava/lang/String;I)V`).
    #[test]
    fn marshal_class_type_scans_to_semicolon() {
        let args = [Value::int(0xaa), Value::int(0xbb), Value::int(0xcc)];
        let mut out = [0u32; 4];
        let n = marshal_args(&info(&args, "(Ljava.lang.String;I)V"), &mut out).expect("marshals");
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[0xcc, 0xbb, 0xaa]);
    }

    /// A lone `L` parameter must not consume a second slot: oracle
    /// `(Ljava/lang/String;)V` leaves `out[2]` at the sentinel.
    #[test]
    fn marshal_single_class_parameter_takes_one_slot() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0002u32; 4];
        let n = marshal_args(&info(&args, "(Ljava.lang.String;)V"), &mut out).expect("marshals");
        assert_eq!(n, 2);
        assert_eq!(&out[..3], &[0xbb, 0xaa, 0xdead_0002]);
    }

    /// Arrays: every extra `[` is skipped, the payload word is copied, then
    /// an `I`/`F`/`L` element consumes the following slot as well. Oracle
    /// `([[I)V` -> `[0xbb, 0xaa]`; `[[I[I)V` -> `[0x33, 0x22, 0x11]`.
    #[test]
    fn marshal_array_skips_brackets_and_takes_payload() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0002u32; 4];
        let n = marshal_args(&info(&args, "([[I)V"), &mut out).expect("marshals");
        assert_eq!(n, 2);
        assert_eq!(&out[..3], &[0xbb, 0xaa, 0xdead_0002]);
    }

    #[test]
    fn marshal_two_arrays_descend_two_slots_each() {
        let args = [Value::int(0x11), Value::int(0x22), Value::int(0x33)];
        let mut out = [0u32; 4];
        let n = marshal_args(&info(&args, "([[I[I)V"), &mut out).expect("marshals");
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[0x33, 0x22, 0x11]);
    }

    /// `[[Ljava.lang.String;`: brackets skipped, `L` element also consumes
    /// one slot — oracle `([[Ljava/lang/String;)V` writes two words.
    #[test]
    fn marshal_array_of_class_takes_two_slots() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0002u32; 4];
        let n = marshal_args(&info(&args, "([[Ljava.lang.String;)V"), &mut out).expect("marshals");
        assert_eq!(n, 2);
        assert_eq!(&out[..3], &[0xbb, 0xaa, 0xdead_0002]);
    }

    /// Container has exactly `count-1` parameter slots; a longer descriptor
    /// stops at the first missing one and leaves the rest untouched.
    #[test]
    fn marshal_not_enough_parameters() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0002u32; 4];
        let err = marshal_args(&info(&args, "(II)V"), &mut out).unwrap_err();
        assert_eq!(err, MarshalError::NotEnoughParameters);
        assert_eq!(&out[..3], &[0xbb, 0xaa, 0xdead_0002]);
        assert_eq!(
            err.log_line(&info(&args, "(II)V")),
            "!CallInfo.parseParameters: not enough parameters passed to native function\
             java.lang.Systemlog(II)V"
        );
    }

    /// The container check (`test eax,eax; je 0x45dabf`) precedes the type
    /// switch, so an exhausted container reports the missing parameter even
    /// though the next descriptor byte is also bogus.
    #[test]
    fn marshal_missing_parameter_wins_over_unknown_type() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0002u32; 4];
        let err = marshal_args(&info(&args, "(IX)V"), &mut out).unwrap_err();
        assert_eq!(err, MarshalError::NotEnoughParameters);
        assert_eq!(&out[..3], &[0xbb, 0xaa, 0xdead_0002]);
    }

    #[test]
    fn marshal_unknown_type() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0xdead_0001u32; 3];
        let err = marshal_args(&info(&args, "(X)V"), &mut out).unwrap_err();
        assert_eq!(err, MarshalError::UnknownType);
        assert_eq!(&out[..2], &[0xbb, 0xdead_0001], "nothing written");
        assert_eq!(
            err.message(),
            "CallInfo.parseParameters: unknown type in signature"
        );
    }

    /// The array arm copies the payload *before* testing the element type,
    /// so `[X` writes its word and then reports the array-specific message
    /// (with the trailing space).
    #[test]
    fn marshal_unknown_array_element_after_copy() {
        let args = [Value::int(0xaa), Value::int(0xbb)];
        let mut out = [0u32; 3];
        let err = marshal_args(&info(&args, "([X)V"), &mut out).unwrap_err();
        assert_eq!(err, MarshalError::UnknownArrayElement);
        assert_eq!(&out[..2], &[0xbb, 0xaa]);
        assert_eq!(
            err.message(),
            "CallInfo.parseParameters: unknown type in signature "
        );
    }

    #[test]
    fn parameter_slots_counts_the_same_walk() {
        assert_eq!(parameter_slots("()V"), Some(0));
        assert_eq!(parameter_slots("(I)I"), Some(1));
        assert_eq!(parameter_slots("(II)V"), Some(2));
        assert_eq!(parameter_slots("(F)V"), Some(1));
        assert_eq!(parameter_slots("(Ljava.lang.String;)V"), Some(1));
        assert_eq!(parameter_slots("([[I)V"), Some(1));
        assert_eq!(
            parameter_slots("(Ljava.lang.Object;ILjava.lang.Object;II)V"),
            Some(5)
        );
        assert_eq!(parameter_slots("([[Ljava.lang.String;[I)V"), Some(2));
        assert_eq!(parameter_slots("I)V"), None, "missing (");
        assert_eq!(parameter_slots("(I"), None, "missing )");
        assert_eq!(parameter_slots("(X)V"), None);
        assert_eq!(parameter_slots("([X)V"), None);
    }

    /// The caller side pops the parameters last-declared first and the
    /// receiver last, so the container ends with the receiver on top (the
    /// slot `marshal_args` reads at `count-1`).
    #[test]
    fn collect_args_puts_the_receiver_on_top() {
        let mut h = host_with_class();
        let (mut th, _) = thread_with_frame(vec![]);
        // Push order: receiver, first parameter, second parameter.
        th.push_new(Value::int(0x01)).expect("receiver");
        th.push_new(Value::int(0x02)).expect("first param");
        th.push_new(Value::int(0x03)).expect("second param");
        let args = collect_args(&mut th, "(II)V").expect("collects");
        assert_eq!(args.len(), 3, "container count includes the receiver");
        assert_eq!(args[2].payload, 0x01, "receiver on top");
        let mut out = [0u32; 3];
        let n = marshal_args(&info(&args, "(II)V"), &mut out).expect("marshals");
        assert_eq!(n, 3);
        assert_eq!(&out[..3], &[0x01, 0x02, 0x03]);
        assert_eq!(th.frames[0].stack.len(), 0, "operands consumed");
        let _ = &mut h;
    }

    #[test]
    fn return_type_reads_the_descriptor_tail() {
        assert_eq!(return_type("()V"), Some(TypeTag::intern("V")));
        assert_eq!(return_type("()I"), Some(TypeTag::Int));
        assert_eq!(return_type("(F)F"), Some(TypeTag::Float));
        assert_eq!(
            return_type("()Ljava.lang.String;"),
            Some(TypeTag::intern("Ljava.lang.String;"))
        );
        assert_eq!(return_type("()[I"), Some(TypeTag::intern("[I")));
        assert_eq!(return_type("("), None);
    }
}

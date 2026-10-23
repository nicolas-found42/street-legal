//! The `ResourceRef` node model and its natives (`M1-10`, issue #37).
//!
//! The exe reaches every resource through three levels, all from the `obj`
//! field word (`DAT_0062e008 = 3`) a `ResourceRef`/`GameRef` object carries:
//!
//! ```text
//! Java object ──Native.ptr field──► R (0x10-byte node) ──+0xc──► X (container)
//!                                                        └── vtable[0xc](1.0f) ──► E (envelope)
//! ```
//!
//! **R** — `0x54f560`-allocated, `0x54f5b0`-freed:
//!
//! | off | meaning |
//! |---|---|
//! | `+0x0` | `prev` in the container's handle list (0 = head) |
//! | `+0x4` | `next` in the same list |
//! | `+0x8` | the resource **id** word (0 when unbound) |
//! | `+0xc` | the **container** `X` (0 when unbound) |
//!
//! The handle list is doubly linked with its head at `X+0x48` and its sentinel
//! pair at `X+0x44`/`X+0x48`; insert-at-head is `0x4290f0` and unlink is
//! `0x429010`.
//!
//! **X** — the engine resource object: `+0x14` parent, `+0x20` first child,
//! `+0x4c` type id (1 and 8 = `GameRef`, 3 and `0xe` = `RenderRef`, else
//! `ResourceRef`), `+0x50` its own id, `+0x54` flags, `+0xcc` WT root.
//!
//! **E** — `X->vtable[0xc](1.0f)`, the live instance. Engine-only tails go
//! through [`ResourceEvent`] so headless backends can record them; the exe's
//! manager singleton (`0x618d48`) and the package/id machinery of `0x536820`
//! are modelled only as far as a ported row's body reads them.
//!
//! Oracle: `re/oracle/res_vectors.py` groups `resource` and `game` execute
//! each native and its marshaller in the stock image. Engine-only lookups,
//! creation, vectors, class hierarchy answers, and virtual getInfo dispatch
//! are supplied through the host's explicit engine-state setters; their
//! calls are recorded as [`ResourceEvent`]s for headless backends. A missing
//! creation or spatial backend is reported explicitly, rather than returning
//! an invented successful result.
//!
//! # Original bug divergences (ADR 0007)
//!
//! The oracle records build-606 null dereferences for the following inputs;
//! the port returns the ordinary absent-resource result instead:
//!
//! - null `obj`: `getFlags` (+0xc), `setFlags` (+0xc), `clearFlags` (+0xc),
//!   `getParent` (+8), `getWTRoot` (+8), `getFirstChild` (+0xc), and
//!   `getNextChild` (+0xc);
//! - bound resource with no parent: `getParentID` dereferences parent+0x50.
//!
//! Replay excludes exactly those recorded triggers and rejects other faults.
//! Recolour, cache, release, event dispatch, and transform effects remain
//! engine calls recorded at the native boundary; rendering and physics are
//! supplied by their engine backends.

use std::cell::RefCell;
use std::collections::HashMap;

use slr_calllog::model::{LogRecord, Value as LogValue};
use slr_vm::host::{marshal_args, CallInfo, VmHost};

// --------------------------------------------------------------------------
// The registry rows
// --------------------------------------------------------------------------

/// `java.util.resource.ResourceRef`.
pub const RESOURCE_REF_CLASS: &str = "java.util.resource.ResourceRef";

/// `ResourceRef.newNative()V`.
///
/// exe: LAB_0047cea0
pub const NEW_NATIVE: u32 = 0x0047_cea0;
/// `ResourceRef.deleteNative()V`.
///
/// exe: LAB_0047cf00
pub const DELETE_NATIVE: u32 = 0x0047_cf00;
/// `ResourceRef.set(I)V`.
///
/// exe: FUN_0047cf80
pub const SET_ID: u32 = 0x0047_cf80;
/// `ResourceRef.set(Ljava.util.resource.ResourceRef;)V`.
///
/// exe: FUN_0047cfc0
pub const SET_REF: u32 = 0x0047_cfc0;
/// `ResourceRef.load()V`.
///
/// exe: LAB_0047d0a0
pub const LOAD: u32 = 0x0047_d0a0;
/// `ResourceRef.unload()V`.
///
/// exe: LAB_0047d170
pub const UNLOAD: u32 = 0x0047_d170;
/// `ResourceRef.destroy()V`.
///
/// exe: LAB_0047d1e0
pub const DESTROY: u32 = 0x0047_d1e0;
/// `ResourceRef.type()I`.
///
/// exe: LAB_0047d210
pub const TYPE_ID: u32 = 0x0047_d210;
/// `ResourceRef.id()I`.
///
/// exe: LAB_0047d290
pub const ID: u32 = 0x0047_d290;
/// `ResourceRef.getParentID()I`.
///
/// exe: LAB_0047d3e0
pub const GET_PARENT_ID: u32 = 0x0047_d3e0;
/// `ResourceRef.getParent()Ljava.util.resource.ResourceRef;`.
///
/// exe: LAB_0047d4b0
pub const GET_PARENT: u32 = 0x0047_d4b0;
/// `ResourceRef.getWTRoot()Ljava.util.resource.ResourceRef;`.
///
/// exe: LAB_0047d570
pub const GET_WT_ROOT: u32 = 0x0047_d570;
/// `ResourceRef.getFirstChild()Ljava.util.resource.ResourceRef;`.
///
/// exe: LAB_0047d630
pub const GET_FIRST_CHILD: u32 = 0x0047_d630;
/// `ResourceRef.getNextChild()Ljava.util.resource.ResourceRef;`.
///
/// exe: LAB_0047d6f0
pub const GET_NEXT_CHILD: u32 = 0x0047_d6f0;
/// `ResourceRef.makeTexture(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V`.
///
/// exe: LAB_0047ffe0
pub const MAKE_TEXTURE: u32 = 0x0047_ffe0;
/// `ResourceRef.makeSound(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V`.
///
/// exe: LAB_00480100
pub const MAKE_SOUND: u32 = 0x0048_0100;
/// `ResourceRef.cache()V`.
///
/// exe: LAB_0047e8d0
pub const CACHE: u32 = 0x0047_e8d0;
/// `ResourceRef.precache()V`.
///
/// exe: LAB_0047e950
pub const PRECACHE: u32 = 0x0047_e950;
/// `ResourceRef.duplicate(Ljava.util.resource.ResourceRef;)V`.
///
/// exe: FUN_004802c0
pub const DUPLICATE: u32 = 0x0048_02c0;
/// `ResourceRef.scaleMesh(FFF)V`.
///
/// exe: LAB_00480390
pub const SCALE_MESH: u32 = 0x0048_0390;

/// `java.util.resource.GameRef`.
pub const GAME_REF_CLASS: &str = "java.util.resource.GameRef";

/// `GameRef.getFlags()I`.
///
/// exe: LAB_0047df40
pub const GET_FLAGS: u32 = 0x0047_df40;
/// `GameRef.setFlags(I)V`.
///
/// exe: FUN_00486c80
pub const SET_FLAGS: u32 = 0x0048_6c80;
/// `GameRef.clearFlags(I)V`.
///
/// exe: FUN_00486cd0
pub const CLEAR_FLAGS: u32 = 0x0048_6cd0;
/// `GameRef.setActiveCollision()V`.
///
/// exe: LAB_0047df80
pub const SET_ACTIVE_COLLISION: u32 = 0x0047_df80;
/// `GameRef.getScriptInstance()Ljava.lang.Object;`.
///
/// exe: LAB_00486f30
pub const GET_SCRIPT_INSTANCE: u32 = 0x0048_6f30;
/// `GameRef.getDetail()F`.
///
/// exe: LAB_00480440
pub const GET_DETAIL: u32 = 0x0048_0440;
/// The flag `setActiveCollision` sets (`0x10000000`).
pub const FLAG_ACTIVE_COLLISION: u32 = 0x1000_0000;
/// The flag `GameRef.setFlags` reacts to by calling `0x544f40` (`0x10`).
pub const FLAG_RECOLOUR: u32 = 0x10;

/// `GameRef.isEmpty()I`.
/// exe: LAB_00486d10
pub const IS_EMPTY: u32 = 0x0048_6d10;
/// `GameRef.isScripted(Ljava.lang.String;)I`.
/// exe: LAB_00486da0
pub const IS_SCRIPTED: u32 = 0x0048_6da0;

/// `getPos` native.
/// exe: LAB_0047dad0
pub const GET_POS: u32 = 0x0047dad0;
/// `getOri` native.
/// exe: LAB_0047dbe0
pub const GET_ORI: u32 = 0x0047dbe0;
/// `getVel` native.
/// exe: LAB_0047dce0
pub const GET_VEL: u32 = 0x0047dce0;
/// `setParent` native.
/// exe: FUN_0047e2d0
pub const SET_PARENT: u32 = 0x0047e2d0;
/// `queueEvent` native.
/// exe: LAB_0047da30
pub const QUEUE_EVENT: u32 = 0x0047da30;
/// `setPos` native.
/// exe: FUN_0047e350
pub const SET_POS: u32 = 0x0047e350;
/// `setMatrix` native.
/// exe: FUN_0047e490
pub const SET_MATRIX: u32 = 0x0047e490;
/// `setState` native.
/// exe: FUN_0047e630
pub const SET_STATE: u32 = 0x0047e630;
/// `create` native.
/// exe: LAB_0047d7b0
pub const CREATE: u32 = 0x0047d7b0;
/// `create_native` native.
/// exe: LAB_0047d900
pub const CREATE_NATIVE: u32 = 0x0047d900;
/// `getInfo` native.
/// exe: LAB_0047ddf0
pub const GET_INFO: u32 = 0x0047ddf0;
/// Registered GameRef natives implemented by this module.
pub const GAME_REF_BINDINGS: &[ResBinding] = &[
    game_row("create", "(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)Ljava.lang.GameType;", CREATE),
    game_row("create_native", "(Ljava.util.resource.GameRef;Ljava.util.resource.GameRef;Ljava.lang.String;Ljava.lang.String;)V", CREATE_NATIVE),
    game_row("getInfo", "(II)I", GET_INFO),
    game_row("getInfo", "(ILjava.lang.String;)I", GET_INFO),

    game_row("setPos", "(Ljava.lang.Vector3;)V", SET_POS),
    game_row("setMatrix", "(Ljava.lang.Vector3;Ljava.lang.Ypr;)V", SET_MATRIX),
    game_row("setState", "(Ljava.lang.Vector3;Ljava.lang.Ypr;Ljava.lang.Vector3;Ljava.lang.Vector3;)V", SET_STATE),

    game_row("getPos", "()Ljava.lang.Vector3;", GET_POS),
    game_row("getOri", "()Ljava.lang.Ypr;", GET_ORI),
    game_row("getVel", "()Ljava.lang.Vector3;", GET_VEL),
    game_row("setParent", "(Ljava.util.resource.ResourceRef;)V", SET_PARENT),
    game_row("queueEvent", "(Ljava.util.resource.ResourceRef;ILjava.lang.String;)V", QUEUE_EVENT),

    game_row("isEmpty", "()I", IS_EMPTY),
    game_row("isScripted", "(Ljava.lang.String;)I", IS_SCRIPTED),
    game_row("getFlags", "()I", GET_FLAGS),
    game_row("setFlags", "(I)V", SET_FLAGS),
    game_row("clearFlags", "(I)V", CLEAR_FLAGS),
    game_row("setActiveCollision", "()V", SET_ACTIVE_COLLISION),
    game_row(
        "getScriptInstance",
        "()Ljava.lang.Object;",
        GET_SCRIPT_INSTANCE,
    ),
    game_row("getDetail", "()F", GET_DETAIL),
];

const fn game_row(name: &'static str, sig: &'static str, impl_addr: u32) -> ResBinding {
    ResBinding {
        class: GAME_REF_CLASS,
        name,
        sig,
        impl_addr,
    }
}

/// The `ptr` field id the natives read (`DAT_0062e008`).
pub const OBJ_FIELD: u32 = 3;
/// Global fallback reference used when create receives a null first argument.
pub const DEFAULT_RESOURCE_WORD: u32 = 0x0063_6460;
/// The flag `load` sets when `ground_precache` is positive (`0x80000`).
pub const FLAG_PRECACHE: u32 = 0x0008_0000;
/// The type ids the wrapper factory maps to `GameRef` (`1` and `8`).
pub const GAME_TYPES: [i32; 2] = [1, 8];
/// The type ids it maps to `RenderRef` (`3` and `0xe`).
pub const RENDER_TYPES: [i32; 2] = [3, 0x0e];

/// One `ResourceRef` registration ported by this module.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResBinding {
    /// The declaring class.
    pub class: &'static str,
    /// The registered method name.
    pub name: &'static str,
    /// The registered descriptor text.
    pub sig: &'static str,
    /// Exe address of the implementation (`record+8`).
    pub impl_addr: u32,
}

/// The twenty rows, in the exe's registration order.
pub const RESOURCE_REF_BINDINGS: &[ResBinding] = &[
    row("newNative", "()V", NEW_NATIVE),
    row("deleteNative", "()V", DELETE_NATIVE),
    row("set", "(I)V", SET_ID),
    row("set", "(Ljava.util.resource.ResourceRef;)V", SET_REF),
    row("load", "()V", LOAD),
    row("unload", "()V", UNLOAD),
    row("destroy", "()V", DESTROY),
    row("type", "()I", TYPE_ID),
    row("id", "()I", ID),
    row("getParentID", "()I", GET_PARENT_ID),
    row(
        "getParent",
        "()Ljava.util.resource.ResourceRef;",
        GET_PARENT,
    ),
    row(
        "getWTRoot",
        "()Ljava.util.resource.ResourceRef;",
        GET_WT_ROOT,
    ),
    row(
        "getFirstChild",
        "()Ljava.util.resource.ResourceRef;",
        GET_FIRST_CHILD,
    ),
    row(
        "getNextChild",
        "()Ljava.util.resource.ResourceRef;",
        GET_NEXT_CHILD,
    ),
    row(
        "makeTexture",
        "(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V",
        MAKE_TEXTURE,
    ),
    row(
        "makeSound",
        "(Ljava.util.resource.ResourceRef;Ljava.lang.String;)V",
        MAKE_SOUND,
    ),
    row("cache", "()V", CACHE),
    row("precache", "()V", PRECACHE),
    row(
        "duplicate",
        "(Ljava.util.resource.ResourceRef;)V",
        DUPLICATE,
    ),
    row("scaleMesh", "(FFF)V", SCALE_MESH),
];

const fn row(name: &'static str, sig: &'static str, impl_addr: u32) -> ResBinding {
    ResBinding {
        class: RESOURCE_REF_CLASS,
        name,
        sig,
        impl_addr,
    }
}

// --------------------------------------------------------------------------
// The object model
// --------------------------------------------------------------------------

/// One `R` node: the per-instance word `newNative` allocates.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Node {
    /// `+0x0` — previous in the container's handle list (0 = head).
    pub prev: u32,
    /// `+0x4` — next in the same list.
    pub next: u32,
    /// `+0x8` — the resource id word.
    pub id: u32,
    /// `+0xc` — the container `X`.
    pub container: u32,
}

impl Node {
    /// A zeroed node, as `0x54f560` hands one back.
    #[must_use]
    pub fn zeroed() -> Node {
        Node {
            prev: 0,
            next: 0,
            id: 0,
            container: 0,
        }
    }
}

/// One `X` container: the engine resource object.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Container {
    /// `+0x14` — the parent container.
    pub parent: u32,
    /// `+0x20` — the first child container.
    pub first_child: u32,
    /// Next sibling (+4); a terminal sentinel has no next sibling.
    pub next_sibling: u32,
    /// `+0x48` — the handle list's head node.
    pub head: u32,
    /// `+0x4c` — the type id.
    pub type_id: i32,
    /// `+0x50` — the container's own id.
    pub id: u32,
    /// `+0x54` — the flags word.
    pub flags: u32,
    /// `+0xcc` — the WT root container.
    pub wt_root: u32,
    /// `+0x6c` — the render detail word `getDetail` reads.
    pub detail: u32,
    /// Live envelope supplied by the engine; absent when resource is unloaded.
    pub instance: Option<GameInstance>,
}

impl Container {
    /// A container with every word zeroed except its type.
    #[must_use]
    pub fn new(type_id: i32, id: u32) -> Container {
        Container {
            parent: 0,
            first_child: 0,
            next_sibling: 0,
            head: 0,
            type_id,
            id,
            flags: 0,
            wt_root: 0,
            detail: 0,
            instance: None,
        }
    }
}

/// Engine instance words read by the GameRef query natives.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct GameInstance {
    /// Engine getInfo dispatch target (+0x44), optional when unavailable.
    pub info: Option<InfoTarget>,
    /// Envelope +0x50, used by type 1 containers.
    pub script: u32,
    /// Envelope +0xc is non-null.
    pub member: bool,
    /// Envelope +0x10 exists for a type 8 container.
    pub child: bool,
    /// Result of the child object's 0x404e20 script-instance lookup.
    pub child_script: u32,
    /// Child +0x10 is non-null, excluding named class tests.
    pub child_compiled: bool,
    /// Result of the engine's 0x4044e0 class hierarchy lookup.
    pub class_match: bool,
}

/// State of the engine target reached by GameRef.getInfo.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InfoTarget {
    /// Secondary resource at the first envelope's +0x44.
    pub resource: u32,
    /// Object at the secondary envelope's +0xc, the virtual-call receiver.
    pub receiver: u32,
    /// Envelope +0x4c, passed as the first dispatch argument.
    pub id: u32,
    /// Return word from the engine virtual method at vtable +0x3c.
    pub result: u32,
}

/// One engine-side effect a row issues, in order.
///
/// The exe's tails reach the resource manager (`0x536820`), the render/audio
/// factories (`0x539170`/`0x539230`), the cache helpers (`0x48a920`) and the
/// mesh scaler (`0x48e7f0`). None of those is a native, so the call-log record
/// is unaffected (`docs/adr/0007`); the port records the call instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceEvent {
    /// Recolour request to 0x544f40 with the container and resource node.
    Recolour { container: u32, node: u32 },
    /// Resource-manager create call, with its exact argument words.
    Create { native: bool, args: Vec<u32> },
    /// Fetch the created resource's script instance through 0x419860.
    CreatedInstance { container: u32 },
    /// Engine virtual information query.
    Info {
        receiver: u32,
        id: u32,
        first: u32,
        second: u32,
    },
    /// Vector/Ypr field getter called by a transform native.
    Field { object: u32, name: &'static str },
    /// Variadic command encoding passed through 0x551fc0 to 0x48b440.
    Command {
        node: u32,
        format: &'static str,
        values: Vec<u64>,
    },
    /// Fetch the current envelope through vtable slot 3.
    Fetch { container: u32 },
    /// Engine vector query (`pos`, `ori`, or `vel`).
    ReadVector { node: u32, kind: &'static str },
    /// Set a resource parent through 0x48aba0.
    Parent { node: u32, parent: u32 },
    /// Queue an event in the global event hub.
    Queue {
        node: u32,
        resource: u32,
        event: u32,
        text: u32,
    },
    /// Envelope materialisation through vtable slot 5.
    Materialise { container: u32 },
    /// Envelope availability gate, 0x5447d0.
    Gate { container: u32, mask: u32 },
    /// Class hierarchy query passed to 0x4044e0.
    Class { name: String },
    /// `0x536820`: look a resource up by id.
    Lookup {
        /// The id word.
        id: u32,
    },
    /// `0x48a920`: cache the resource with a two-word range.
    Cache {
        /// The node the call carried.
        node: u32,
        /// The low word.
        low: u32,
        /// The high word.
        high: u32,
    },
    /// `0x539170` (texture) / `0x539230` (sound): create a sub-resource.
    Make {
        /// The node the call carried.
        node: u32,
        /// The resource word.
        resource: u32,
        /// The formatted request buffer.
        text: String,
        /// `true` for the sound factory.
        sound: bool,
    },
    /// `0x48d860`: duplicate another resource into this node.
    Duplicate {
        /// The node the call carried.
        node: u32,
        /// The source word.
        source: u32,
    },
    /// `0x48e7f0`: scale the mesh by a three-float vector.
    ScaleMesh {
        /// The node the call carried.
        node: u32,
        /// The three floats, as bits.
        scale: [u32; 3],
    },
    /// `0x48a8f0`/`0x48a8d0`: the unload/destroy engine call.
    Release {
        /// The node the call carried.
        node: u32,
        /// `true` for `destroy`.
        destroy: bool,
    },
    /// `0x545fc0`: bind the node to an id (unlink, lookup, insert).
    Bind {
        /// The node.
        node: u32,
        /// The id word.
        id: u32,
    },
    /// `0x47d300`: hand back a wrapper object for `node`.
    Wrap {
        /// The node the factory wrapped.
        node: u32,
    },
}

// --------------------------------------------------------------------------
// The host
// --------------------------------------------------------------------------

/// The `ResourceRef` runtime: the live nodes, their containers and the call
/// records.
///
/// Both maps are keyed by the same `u32` word the exe threads through the
/// `ptr` field; the port's words are its own arena, exactly as
/// [`slr_sound::SoundHost`]'s interned strings are.
#[derive(Debug, Clone, Default)]
pub struct ResHost {
    nodes: HashMap<u32, Node>,
    containers: HashMap<u32, Container>,
    objects: HashMap<u32, u32>,
    events: Vec<ResourceEvent>,
    records: Vec<LogRecord>,
    created_result: Option<u32>,
    lookups: HashMap<u32, u32>,
    vectors: HashMap<u32, [u32; 3]>,
    spatial: HashMap<u32, [[u32; 3]; 3]>,
    next_node: u32,
    next_container: u32,
    ground_precache: i32,
    gate_refused: bool,
    container_gates: HashMap<u32, bool>,
    strings: Vec<(u32, String)>,
    next_string: u32,
}

impl ResHost {
    /// An empty host: no nodes, no containers, `ground_precache` 0.
    #[must_use]
    pub fn new() -> ResHost {
        ResHost {
            next_node: 0x0050_0000,
            next_container: 0x0060_0000,
            next_string: 0x0070_0000,
            ..ResHost::default()
        }
    }

    /// Allocate a container and answer its word.
    pub fn add_container(&mut self, container: Container) -> u32 {
        let word = self.next_container;
        self.next_container = self.next_container.wrapping_add(0x100);
        self.containers.insert(word, container);
        word
    }

    /// The container behind a word.
    #[must_use]
    pub fn container(&self, word: u32) -> Option<&Container> {
        self.containers.get(&word)
    }

    /// The container behind a word, mutably.
    pub fn container_mut(&mut self, word: u32) -> Option<&mut Container> {
        self.containers.get_mut(&word)
    }

    /// The node behind a word.
    #[must_use]
    pub fn node(&self, word: u32) -> Option<&Node> {
        self.nodes.get(&word)
    }

    /// The node behind a word, mutably.
    pub fn node_mut(&mut self, word: u32) -> Option<&mut Node> {
        self.nodes.get_mut(&word)
    }

    /// The live node count.
    #[must_use]
    pub fn node_count(&self) -> usize {
        self.nodes.len()
    }

    /// Allocate a node with the given id, bound to `container` — the seam a
    /// replay test or a driver uses to reach a known state.
    pub fn add_node_for_test(&mut self, id: u32, container: u32) -> u32 {
        let word = self.next_node;
        self.next_node = self.next_node.wrapping_add(0x10);
        self.nodes.insert(
            word,
            Node {
                prev: 0,
                next: 0,
                id,
                container,
            },
        );
        if container != 0 {
            self.insert_head(container, word);
        }
        word
    }

    /// Bind a Java object's `ptr` field to a node word.
    pub fn bind_object(&mut self, object: u32, node: u32) {
        self.objects.insert(object, node);
    }

    /// The node a Java object's `ptr` field answers (0 when unset).
    #[must_use]
    pub fn object_node(&self, object: u32) -> u32 {
        self.objects.get(&object).copied().unwrap_or(0)
    }

    /// The engine effects so far, in order.
    #[must_use]
    pub fn events(&self) -> &[ResourceEvent] {
        &self.events
    }

    /// The call records so far.
    #[must_use]
    pub fn records(&self) -> &[LogRecord] {
        &self.records
    }

    /// Take the call records.
    pub fn take_records(&mut self) -> Vec<LogRecord> {
        core::mem::take(&mut self.records)
    }

    /// Seed the `ground_precache` count `load` tests.
    pub fn set_ground_precache(&mut self, count: i32) {
        self.ground_precache = count;
    }

    /// Seed the gate word's refusal bit (`0x5447d0` bit 31).
    pub fn set_gate_refused(&mut self, refused: bool) {
        self.gate_refused = refused;
    }

    /// Supply one resource's envelope gate result independently of others.
    pub fn set_container_gate(&mut self, container: u32, refused: bool) {
        self.container_gates.insert(container, refused);
    }

    /// The text behind an interned word.
    #[must_use]
    pub fn string(&self, word: u32) -> Option<&str> {
        self.strings
            .iter()
            .find(|(w, _)| *w == word)
            .map(|(_, s)| s.as_str())
    }

    /// Bind an id lookup to its engine container (zero denotes a miss).
    pub fn bind_id(&mut self, id: u32, container: u32) {
        self.lookups.insert(id, container);
    }

    /// Supply the container returned by a create manager call (zero = miss).
    pub fn set_created_result(&mut self, container: u32) {
        self.created_result = Some(container);
    }

    /// Supply position, orientation, and velocity from the engine backend.
    pub fn set_spatial(&mut self, node: u32, vectors: [[u32; 3]; 3]) {
        self.spatial.insert(node, vectors);
    }

    /// Supply a Vector3 or Ypr argument object to a native.
    pub fn intern_vector(&mut self, components: [u32; 3]) -> u32 {
        let object = self.next_string;
        self.next_string = self.next_string.wrapping_add(1);
        self.vectors.insert(object, components);
        object
    }

    /// Components of a Vector3/Ypr returned by a getter native.
    pub fn vector(&self, object: u32) -> Option<&[u32; 3]> {
        self.vectors.get(&object)
    }

    /// `0x4290f0`: insert `node` at the head of `container`'s handle list.
    ///
    /// exe: FUN_004290f0
    fn insert_head(&mut self, container: u32, node: u32) {
        let old_head = self.containers.get(&container).map_or(0, |c| c.head);
        if let Some(n) = self.nodes.get_mut(&node) {
            n.prev = 0;
            n.next = old_head;
            n.container = container;
        }
        if old_head != 0 {
            if let Some(n) = self.nodes.get_mut(&old_head) {
                n.prev = node;
            }
        }
        if let Some(c) = self.containers.get_mut(&container) {
            c.head = node;
        }
    }

    /// `0x429010`: unlink `node` from its container's handle list.
    ///
    /// exe: FUN_00429010
    fn unlink(&mut self, node: u32) {
        let Some(entry) = self.nodes.get(&node).cloned() else {
            return;
        };
        if entry.prev != 0 {
            if let Some(prev) = self.nodes.get_mut(&entry.prev) {
                prev.next = entry.next;
            }
        } else if entry.container != 0 {
            if let Some(c) = self.containers.get_mut(&entry.container) {
                c.head = entry.next;
            }
        }
        if entry.next != 0 {
            if let Some(next) = self.nodes.get_mut(&entry.next) {
                next.prev = entry.prev;
            }
        }
        if let Some(n) = self.nodes.get_mut(&node) {
            n.prev = 0;
            n.next = 0;
            n.container = 0;
            n.id = 0;
        }
    }

    /// `0x47d300`: allocate a wrapper object for `node` and answer its word.
    fn wrap(&mut self, container: u32) -> u32 {
        let id = self.containers.get(&container).map_or(0, |c| c.id);
        let node = self.add_node_for_test(id, container);
        self.events.push(ResourceEvent::Wrap { node });
        let word = self.next_string;
        self.next_string = self.next_string.wrapping_add(1);
        self.bind_object(word, node);
        word
    }

    /// Run one row over the marshalled words (`words[0]` is the receiver
    /// slot), record the call and answer its result word.
    pub fn call(&mut self, row: &ResBinding, words: &[u32]) -> Option<u32> {
        let receiver = word_at(words, 0);
        let node_word = self.object_node(receiver);
        let word = match (row.impl_addr, row.sig) {
            (CREATE, _) | (CREATE_NATIVE, _) => {
                let value = self.create(row.impl_addr, node_word, words);
                if row.impl_addr == CREATE {
                    Some(value)
                } else {
                    None
                }
            }
            (GET_INFO, _) => Some(self.get_info(node_word, word_at(words, 1), word_at(words, 2))),
            (SET_POS, _) | (SET_MATRIX, _) | (SET_STATE, _) => {
                if node_word != 0 {
                    self.transform(row.impl_addr, node_word, words);
                }
                None
            }
            (GET_POS, _) | (GET_ORI, _) | (GET_VEL, _) => {
                if node_word == 0 || (row.impl_addr == GET_POS && self.nodes[&node_word].id == 0) {
                    Some(0)
                } else {
                    let (index, kind) = match row.impl_addr {
                        GET_POS => (0, "pos"),
                        GET_ORI => (1, "ori"),
                        _ => (2, "vel"),
                    };
                    self.events.push(ResourceEvent::ReadVector {
                        node: node_word,
                        kind,
                    });
                    let components = self
                        .spatial
                        .get(&node_word)
                        .expect("engine spatial state must be supplied")[index];
                    let object = self.next_string;
                    self.next_string = self.next_string.wrapping_add(1);
                    self.vectors.insert(object, components);
                    Some(object)
                }
            }
            (SET_PARENT, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Parent {
                        node: node_word,
                        parent: word_at(words, 1),
                    });
                }
                None
            }
            (QUEUE_EVENT, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Queue {
                        node: node_word,
                        resource: word_at(words, 1),
                        event: word_at(words, 2),
                        text: word_at(words, 3),
                    });
                }
                None
            }
            (NEW_NATIVE, _) => {
                // `0x47cea0`: if the field is already set, do nothing.
                if node_word == 0 {
                    let fresh = self.next_node;
                    self.next_node = self.next_node.wrapping_add(0x10);
                    self.nodes.insert(fresh, Node::zeroed());
                    self.bind_object(receiver, fresh);
                }
                None
            }
            (DELETE_NATIVE, _) => {
                if node_word != 0 {
                    self.delete_native(node_word);
                }
                None
            }
            (SET_ID, _) => {
                if node_word != 0 {
                    self.set_id(node_word, word_at(words, 1));
                }
                None
            }
            (SET_REF, _) => {
                // the argument is the *wrapper* object; its `+0xc` is read raw
                let other = word_at(words, 1);
                if node_word != 0 {
                    self.set_other(node_word, other);
                }
                None
            }
            (LOAD, _) => {
                self.load(node_word);
                None
            }
            (UNLOAD, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Release {
                        node: node_word,
                        destroy: false,
                    });
                }
                None
            }
            (DESTROY, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Release {
                        node: node_word,
                        destroy: true,
                    });
                }
                None
            }
            (TYPE_ID, _) => Some(self.type_id(node_word)),
            (ID, _) => Some(self.nodes.get(&node_word).map_or(0, |n| n.id)),
            (GET_PARENT_ID, _) => Some(self.parent_id(node_word)),
            (GET_PARENT, _) => {
                if self.nodes.get(&node_word).is_some_and(|n| n.id != 0) {
                    self.relative(node_word, |c| c.parent)
                } else {
                    Some(0)
                }
            }
            (GET_WT_ROOT, _) => {
                if self.nodes.get(&node_word).is_some_and(|n| n.id != 0) {
                    self.relative(node_word, |c| c.wt_root)
                } else {
                    Some(0)
                }
            }
            (GET_FIRST_CHILD, _) => self.child_relative(node_word, false),
            (GET_NEXT_CHILD, _) => self.child_relative(node_word, true),
            (MAKE_TEXTURE, _) | (MAKE_SOUND, _) => {
                self.make(
                    node_word,
                    word_at(words, 1),
                    word_at(words, 2),
                    row.impl_addr == MAKE_SOUND,
                );
                None
            }
            (CACHE, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Cache {
                        node: node_word,
                        low: 0.0f32.to_bits(),
                        high: 1.0f32.to_bits(),
                    });
                }
                None
            }
            (PRECACHE, _) => {
                if node_word != 0 {
                    self.events.push(ResourceEvent::Cache {
                        node: node_word,
                        low: 1.0f32.to_bits(),
                        high: 1.0f32.to_bits(),
                    });
                }
                None
            }
            (DUPLICATE, _) => {
                let other = word_at(words, 1);
                if node_word != 0 && other != 0 {
                    self.events.push(ResourceEvent::Duplicate {
                        node: node_word,
                        source: other,
                    });
                }
                None
            }
            // ---- GameRef ----
            //
            // `0x47df40` reads `[R+0xc]` WITHOUT testing `R`: a null `obj`
            // word makes the exe dereference 0xc. The port answers 0, which is
            // the value the row produces whenever the container is null; the
            // null-node crash is recorded as an intentional divergence
            // (ADR 0007).
            (GET_FLAGS, _) => Some(
                self.nodes
                    .get(&node_word)
                    .and_then(|n| self.containers.get(&n.container))
                    .map_or(0, |c| c.flags),
            ),
            (SET_FLAGS, _) => {
                let arg = word_at(words, 1);
                if let Some(c) = self
                    .nodes
                    .get(&node_word)
                    .and_then(|n| self.containers.get(&n.container))
                    .cloned()
                {
                    let _ = c;
                    if let Some(c) = self
                        .nodes
                        .get(&node_word)
                        .and_then(|n| self.containers.get_mut(&n.container))
                    {
                        c.flags |= arg;
                    }
                    if arg & FLAG_RECOLOUR != 0 {
                        let container = self.nodes[&node_word].container;
                        self.events.push(ResourceEvent::Recolour {
                            container,
                            node: node_word,
                        });
                    }
                }
                None
            }
            (CLEAR_FLAGS, _) => {
                let arg = word_at(words, 1);
                if let Some(c) = self
                    .nodes
                    .get(&node_word)
                    .and_then(|n| self.containers.get_mut(&n.container))
                {
                    c.flags &= !arg;
                }
                None
            }
            (SET_ACTIVE_COLLISION, _) => {
                if node_word != 0 {
                    if let Some(c) = self
                        .nodes
                        .get(&node_word)
                        .and_then(|n| self.containers.get_mut(&n.container))
                    {
                        c.flags |= FLAG_ACTIVE_COLLISION;
                    }
                }
                None
            }
            // Type 1 reads E+0x50; type 8 asks its child for the script instance.
            (GET_SCRIPT_INSTANCE, _) | (IS_EMPTY, _) | (IS_SCRIPTED, _) => {
                self.game_query(row.impl_addr, node_word, word_at(words, 1))
            }
            // `0x480440`: type 1 or 3 with a nonzero id reads X+0x6c.
            (GET_DETAIL, _) => {
                let type_id = self.type_id(node_word);
                Some(
                    if (type_id == 1 || type_id == 3)
                        && self.nodes.get(&node_word).is_some_and(|n| n.id != 0)
                    {
                        self.detail_word(node_word)
                    } else {
                        0
                    },
                )
            }
            (SCALE_MESH, _) => {
                let scale = [word_at(words, 1), word_at(words, 2), word_at(words, 3)];
                if node_word != 0 {
                    self.events.push(ResourceEvent::ScaleMesh {
                        node: node_word,
                        scale,
                    });
                }
                None
            }
            _ => return None,
        };
        let sig = row.sig;
        let args = param_types(sig)
            .into_iter()
            .enumerate()
            .map(|(index, ty)| log_value(ty, word_at(words, index + 1)))
            .collect();
        let result = word.map(|word| log_value(return_type(sig), word));
        self.records.push(LogRecord::NativeCall {
            class: row.class.to_owned(),
            name: row.name.to_owned(),
            args,
            result,
        });
        word
    }

    fn create(&mut self, addr: u32, node: u32, words: &[u32]) -> u32 {
        if node == 0 {
            return 0;
        }
        let first = match word_at(words, 1) {
            0 => DEFAULT_RESOURCE_WORD,
            word => word,
        };
        let first_node = if self.nodes.contains_key(&first) {
            first
        } else {
            self.object_node(first)
        };
        if !self
            .nodes
            .get(&first_node)
            .is_some_and(|n| n.container != 0)
        {
            return 0;
        }
        let mut args: Vec<u32> = (1..=4).map(|i| word_at(words, i)).collect();
        args[0] = first;
        if addr == CREATE_NATIVE {
            args.insert(2, 0);
        }
        self.events.push(ResourceEvent::Create {
            native: addr == CREATE_NATIVE,
            args,
        });
        let container = self
            .created_result
            .expect("resource-manager creation result must be supplied");
        if self.nodes[&node].container != container {
            self.unlink(node);
            if container != 0 {
                self.insert_head(container, node);
                self.nodes.get_mut(&node).unwrap().id = self.containers[&container].id;
            }
        }
        if addr == CREATE_NATIVE || container == 0 {
            return 0;
        }
        self.events
            .push(ResourceEvent::CreatedInstance { container });
        self.containers
            .get(&container)
            .and_then(|c| c.instance.as_ref())
            .map_or(0, |i| i.script)
    }

    fn get_info(&mut self, node: u32, first: u32, second: u32) -> u32 {
        let Some(entry) = self.nodes.get(&node) else {
            return 0;
        };
        if entry.id == 0 {
            return 0;
        }
        let container = entry.container;
        let Some(c) = self.containers.get(&container) else {
            return 0;
        };
        let instance = c.instance.clone();
        if c.type_id != 1 {
            self.events.push(ResourceEvent::Materialise { container });
        }
        self.events.push(ResourceEvent::Gate {
            container,
            mask: 0x8000_0000,
        });
        if self.gate_refused {
            return 0;
        }
        self.events.push(ResourceEvent::Fetch { container });
        let Some(target) = instance.and_then(|i| i.info).filter(|i| i.id != 0) else {
            return 0;
        };
        let secondary = target.resource;
        let Some(c) = self.containers.get(&secondary) else {
            return 0;
        };
        let available = c.instance.is_some();
        if c.type_id != 1 {
            self.events.push(ResourceEvent::Materialise {
                container: secondary,
            });
        }
        self.events.push(ResourceEvent::Gate {
            container: secondary,
            mask: 0x8000_0000,
        });
        if self
            .container_gates
            .get(&secondary)
            .copied()
            .unwrap_or(self.gate_refused)
        {
            return 0;
        }
        self.events.push(ResourceEvent::Fetch {
            container: secondary,
        });
        if !available {
            return 0;
        }
        self.events.push(ResourceEvent::Info {
            receiver: target.receiver,
            id: target.id,
            first,
            second,
        });
        target.result
    }

    fn transform(&mut self, addr: u32, node: u32, words: &[u32]) {
        let count = if addr == SET_STATE { 4 } else { 2 };
        let mut vectors = [[0u32; 3]; 4];
        for (index, vector) in vectors.iter_mut().enumerate().take(count) {
            let object = word_at(words, index + 1);
            if index == 1 && addr == SET_POS {
                continue;
            }
            // setPos/setState read their required first Vector3 even when null;
            // setMatrix treats both arguments as optional.
            if object == 0 && (index != 0 || addr == SET_MATRIX) {
                continue;
            }
            *vector = if object == 0 {
                [0; 3]
            } else {
                *self
                    .vectors
                    .get(&object)
                    .expect("non-null vector argument must be supplied by the VM")
            };
            for name in if index == 1 {
                ["y", "p", "r"]
            } else {
                ["x", "y", "z"]
            } {
                self.events.push(ResourceEvent::Field { object, name });
            }
        }
        let values = vectors[..count]
            .iter()
            .flatten()
            .map(|bits| f64::from(f32::from_bits(*bits)).to_bits())
            .collect();
        let format = match addr {
            SET_POS => "ffffff",
            SET_MATRIX => "ffffff",
            _ => "ffffffffffff",
        };
        self.events.push(ResourceEvent::Command {
            node,
            format,
            values,
        });
    }

    fn game_query(&mut self, addr: u32, node: u32, name: u32) -> Option<u32> {
        let default = u32::from(addr == IS_EMPTY);
        let Some(container) = self
            .nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
        else {
            return Some(default);
        };
        let ty = container.type_id;
        if (addr == IS_EMPTY && ty != 8) || (addr != IS_EMPTY && ty != 1 && ty != 8) {
            return Some(default);
        }
        let container_word = self.nodes[&node].container;
        let instance = container.instance.clone();
        if ty != 1 {
            self.events.push(ResourceEvent::Materialise {
                container: container_word,
            });
        }
        self.events.push(ResourceEvent::Gate {
            container: container_word,
            mask: if addr == GET_SCRIPT_INSTANCE {
                0x8000_0000
            } else {
                0xa000_0000
            },
        });
        if self.gate_refused {
            return Some(default);
        }
        self.events.push(ResourceEvent::Fetch {
            container: container_word,
        });
        let Some(instance) = instance else {
            return Some(default);
        };
        if addr == IS_EMPTY {
            return Some(u32::from(!instance.member && !instance.child));
        }
        let script = if ty == 1 {
            instance.script
        } else if instance.child {
            instance.child_script
        } else {
            0
        };
        if addr == GET_SCRIPT_INSTANCE {
            return Some(script);
        }
        let live = if ty == 1 {
            instance.script != 0
        } else {
            instance.child
        };
        if !live {
            return Some(0);
        }
        if name == 0 {
            return Some(1);
        }
        if ty == 8 && instance.child_compiled {
            return Some(0);
        }
        self.events.push(ResourceEvent::Class {
            name: format!("L{};", self.string(name).unwrap_or_default()),
        });
        Some(u32::from(instance.class_match))
    }

    /// `0x47cf00`: unlink, zero the four words, free, then clear the field.
    fn delete_native(&mut self, node: u32) {
        if self.nodes.get(&node).is_some_and(|n| n.container != 0) {
            self.unlink(node);
        }
        self.nodes.remove(&node);
        self.objects.retain(|_, v| *v != node);
    }

    /// `0x545fc0`: bind the node to `id`, looking the container up.
    fn set_id(&mut self, node: u32, id: u32) {
        if self.nodes.get(&node).is_some_and(|n| n.id == id) {
            return;
        }
        if self.nodes.get(&node).is_some_and(|n| n.container != 0) {
            self.unlink(node);
        }
        if id == 0 {
            return;
        }
        self.events.push(ResourceEvent::Bind { node, id });
        if let Some(c) = self.container_for_id(id) {
            if let Some(n) = self.nodes.get_mut(&node) {
                n.id = id;
            }
            self.insert_head(c, node);
        }
    }

    /// The container a lookup by id answers, if one is registered.
    fn container_for_id(&self, id: u32) -> Option<u32> {
        if id == 0 {
            return None;
        }
        if let Some(&container) = self.lookups.get(&id) {
            return (container != 0).then_some(container);
        }
        self.containers
            .iter()
            .find(|(_, c)| c.id == id)
            .map(|(word, _)| *word)
    }

    /// `0x47cfc0`: move the node under the container the argument names.
    fn set_other(&mut self, node: u32, other: u32) {
        if other == 0 {
            self.set_id(node, 0);
            return;
        }
        let target = self.nodes.get(&other).map_or(0, |n| n.container);
        if self.nodes.get(&node).is_some_and(|n| n.container == target) {
            return;
        }
        self.unlink(node);
        if target != 0 && self.containers.contains_key(&target) {
            let id = self.containers.get(&target).map_or(0, |c| c.id);
            self.insert_head(target, node);
            if let Some(n) = self.nodes.get_mut(&node) {
                n.id = id;
            }
        }
    }

    /// `0x47d0a0`: the precache flag and the materialise gate.
    fn load(&mut self, node: u32) {
        if node == 0 {
            return;
        }
        let container = self.nodes.get(&node).map_or(0, |n| n.container);
        if self.ground_precache > 0 && container != 0 {
            if let Some(c) = self.containers.get_mut(&container) {
                c.flags |= FLAG_PRECACHE;
            }
        }
        if container == 0 {
            return;
        }
        let type_id = self.containers.get(&container).map_or(0, |c| c.type_id);
        if type_id != 1 {
            self.events.push(ResourceEvent::Materialise { container });
        }
        self.events.push(ResourceEvent::Gate {
            container,
            mask: 0x8000_0001,
        });
        if !self.gate_refused {
            self.events.push(ResourceEvent::Fetch { container });
        }
    }

    /// `[X+0x6c]`, the render detail word `getDetail` answers. The port keeps
    /// no envelope, so this is 0 unless a driver plants one.
    fn detail_word(&self, node: u32) -> u32 {
        self.nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
            .map_or(0, |c| c.detail)
    }

    /// `0x47d210`: the container's type id, or 0.
    fn type_id(&self, node: u32) -> u32 {
        self.nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
            .map_or(0, |c| c.type_id as u32)
    }

    /// `0x48ab80`: `[[R+0xc]+0x14]+0x50`, the parent's id.
    fn parent_id(&self, node: u32) -> u32 {
        let parent = self
            .nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
            .map_or(0, |c| c.parent);
        self.containers.get(&parent).map_or(0, |c| c.id)
    }

    /// The `getParent`/`getWTRoot`/`getFirstChild` shape: one container word,
    /// then `0x47d300`'s wrapper factory (or 0).
    fn relative(&mut self, node: u32, pick: impl Fn(&Container) -> u32) -> Option<u32> {
        let target = self
            .nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
            .map_or(0, &pick);
        self.wrap_or_zero(target)
    }

    fn child_relative(&mut self, node: u32, sibling: bool) -> Option<u32> {
        let target = self
            .nodes
            .get(&node)
            .and_then(|n| self.containers.get(&n.container))
            .map_or(0, |c| {
                if sibling {
                    c.next_sibling
                } else {
                    c.first_child
                }
            });
        if self
            .containers
            .get(&target)
            .is_some_and(|c| c.next_sibling != 0)
        {
            self.wrap_or_zero(target)
        } else {
            Some(0)
        }
    }

    fn wrap_or_zero(&mut self, target: u32) -> Option<u32> {
        if target == 0 {
            Some(0)
        } else {
            Some(self.wrap(target))
        }
    }

    /// `0x47ffe0`/`0x480100`: format the request and call the factory.
    fn make(&mut self, node: u32, resource: u32, name: u32, sound: bool) {
        if node == 0 || resource == 0 || name == 0 {
            return;
        }
        let text = format!(
            "sourcefile {}\r\nflags 0\r\n",
            self.string(name).unwrap_or_default()
        );
        self.events.push(ResourceEvent::Make {
            node,
            resource,
            text,
            sound,
        });
        let container = self
            .created_result
            .expect("resource factory result must be supplied");
        if self.nodes[&node].container != container {
            self.unlink(node);
            if container != 0 {
                self.insert_head(container, node);
                self.nodes.get_mut(&node).unwrap().id = self.containers[&container].id;
            }
        }
    }

    /// Intern a string and answer its word — the seam a driver uses to give a
    /// `makeTexture`/`makeSound` row its name argument.
    pub fn intern(&mut self, text: &str) -> u32 {
        let word = self.next_string;
        self.next_string = self.next_string.wrapping_add(1);
        self.strings.push((word, text.to_owned()));
        word
    }
}

thread_local! {
    static RES: RefCell<ResHost> = RefCell::new(ResHost::new());
}

/// Run `f` over this thread's [`ResHost`].
pub fn with_res<R>(f: impl FnOnce(&mut ResHost) -> R) -> R {
    RES.with(|r| f(&mut r.borrow_mut()))
}

/// Reset this thread's resource state.
pub fn reset_res() {
    RES.with(|r| *r.borrow_mut() = ResHost::new());
}

/// Register every row with the VM's native registry.
pub fn register(vm: &mut VmHost) {
    for row in RESOURCE_REF_BINDINGS.iter().chain(GAME_REF_BINDINGS) {
        vm.natives
            .register(row.class, row.name, row.sig, row.impl_addr);
    }
}

/// The single [`slr_vm::host::NativeFn`] installed at every address.
///
/// exe: FUN_0041fbc0
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let row = bridge_binding(addr, info.name, info.sig)?;
    let mut out = [0u32; 8];
    let written = marshal_args(info, &mut out).unwrap_or(0);
    with_res(|host| {
        if let Some(node) = vm_resource_node(vm, out[0]) {
            host.bind_object(out[0], node);
        }
        // These parameters are native resource words in the executable frame.
        // The public VM passes Java references, so unwrap their inherited ptr;
        // injected backend words remain explicit native-boundary inputs.
        let argument_count = match addr {
            CREATE | CREATE_NATIVE => 2,
            SET_REF | DUPLICATE | MAKE_TEXTURE | MAKE_SOUND | SET_PARENT | QUEUE_EVENT => 1,
            _ => 0,
        };
        let logged_words = out;
        if host.object_node(out[0]) != 0 {
            for argument in out.iter_mut().skip(1).take(argument_count) {
                if let Some(node) = vm_resource_node(vm, *argument) {
                    *argument = node;
                }
            }
        }
        if matches!(addr, SET_POS | SET_MATRIX | SET_STATE) {
            let count = if addr == SET_STATE {
                4
            } else if addr == SET_MATRIX {
                2
            } else {
                1
            };
            for index in 0..count {
                let object = out[index + 1];
                let names = if index == 1 {
                    ["y", "p", "r"]
                } else {
                    ["x", "y", "z"]
                };
                if let Some(components) = vm_vector(vm, object, names) {
                    host.vectors.insert(object, components);
                }
            }
        }
        let result = host.call(row, &out[..written]);
        if let Some(LogRecord::NativeCall { args, .. }) = host.records.last_mut() {
            *args = param_types(info.sig)
                .into_iter()
                .enumerate()
                .map(|(index, ty)| log_value(ty, logged_words[index + 1]))
                .collect();
        }
        if matches!(addr, NEW_NATIVE | DELETE_NATIVE) {
            if let Some(index) = out[0]
                .checked_sub(0x1000)
                .map(|index| index as usize)
                .filter(|index| *index < vm.objects.len())
            {
                let (slot, _) = resource_ptr_layout(vm, &vm.objects[index].class)
                    .expect("resource native receiver requires loaded ptr field");
                vm.objects[index].fields[slot] =
                    slr_vm::values::Value::int(host.object_node(out[0]) as i32);
            }
        }
        if matches!(
            addr,
            GET_PARENT | GET_WT_ROOT | GET_FIRST_CHILD | GET_NEXT_CHILD
        ) {
            if let Some(old) = result.filter(|word| *word != 0) {
                let node = host.object_node(old);
                let ty = host.type_id(node);
                let object = vm_resource_wrapper(vm, node, ty);
                host.objects.remove(&old);
                host.bind_object(object, node);
                if let Some(LogRecord::NativeCall { result, .. }) = host.records.last_mut() {
                    *result = Some(LogValue::Int(object as i32));
                }
                return Some(object);
            }
        }
        if matches!(addr, GET_POS | GET_ORI | GET_VEL) {
            if let Some(components) = result.and_then(|word| host.vectors.get(&word)).copied() {
                let (class, names) = if addr == GET_ORI {
                    ("java.lang.Ypr", ["y", "p", "r"])
                } else {
                    ("java.lang.Vector3", ["x", "y", "z"])
                };
                {
                    let object = vm
                        .alloc_object(class)
                        .expect("vector return class must be loaded");
                    for (name, bits) in names.into_iter().zip(components) {
                        vm.write_field(
                            object,
                            name,
                            slr_vm::values::Value::float(f32::from_bits(bits)),
                        )
                        .expect("vector return requires declared float fields");
                    }
                    host.vectors.insert(object, components);
                    if let Some(LogRecord::NativeCall { result, .. }) = host.records.last_mut() {
                        *result = Some(LogValue::Int(object as i32));
                    }
                    return Some(object);
                }
            }
        }
        result
    })
}

fn bridge_binding(addr: u32, name: &str, sig: &str) -> Option<&'static ResBinding> {
    RESOURCE_REF_BINDINGS
        .iter()
        .chain(GAME_REF_BINDINGS)
        .find(|row| row.impl_addr == addr && row.name == name && row.sig == sig)
}

// Every public resource reference uses the central inherited instance layout.
// Missing metadata or cells are errors, including a fresh newNative receiver.
fn vm_resource_node(vm: &mut VmHost, word: u32) -> Option<u32> {
    let index = word.checked_sub(0x1000)? as usize;
    let object = vm.objects.get(index)?;
    let (slot, count) = resource_ptr_layout(vm, &object.class)
        .expect("resource native receiver requires loaded ptr field");
    assert_eq!(
        vm.objects[index].fields.len(),
        count,
        "resource native receiver requires initialized ptr field"
    );
    Some(
        vm.objects[index]
            .fields
            .get(slot)
            .expect("resource native receiver requires initialized ptr field")
            .payload,
    )
}

// Native and bytecode field accesses share the VM's base-first,
// instance-only layout and typed cells.
fn resource_ptr_layout(vm: &VmHost, class: &str) -> Option<(usize, usize)> {
    let fields = vm.instance_fields(class).ok()?;
    let slot = vm.instance_field_slot(class, "ptr").ok()?;
    if fields.get(slot)?.ty != "I" {
        return None;
    }
    Some((slot, fields.len()))
}

/// Construct the wrapper class selected by the real resource factory.
/// exe: FUN_0047d300
fn vm_resource_wrapper(vm: &mut VmHost, node: u32, ty: u32) -> u32 {
    let class = if GAME_TYPES.contains(&(ty as i32)) {
        GAME_REF_CLASS
    } else if RENDER_TYPES.contains(&(ty as i32)) {
        "java.util.resource.RenderRef"
    } else {
        RESOURCE_REF_CLASS
    };
    let (slot, count) = resource_ptr_layout(vm, class)
        .expect("resource wrapper requires loaded class hierarchy and ptr field");
    let word = vm
        .alloc_object(class)
        .expect("resource wrapper class must be loaded");
    let object = &mut vm.objects[(word - 0x1000) as usize];
    assert_eq!(
        object.fields.len(),
        count,
        "resource wrapper requires initialized inherited fields"
    );
    object.fields[slot] = slr_vm::values::Value::int(node as i32);
    word
}

// The VM's established 0x42a560 instance-field contract: absent/non-F fields
// on a real object answer +0. Unknown injected backend words remain errors.
fn vm_vector(vm: &mut VmHost, word: u32, names: [&str; 3]) -> Option<[u32; 3]> {
    vm.objects.get(word.checked_sub(0x1000)? as usize)?;
    Some(names.map(|name| match vm.read_field(word, name) {
        Ok(value) if value.tag == slr_vm::values::TypeTag::Float => value.payload,
        Err(slr_vm::host::HostError::UnsupportedInitializer(error)) => {
            panic!("unsupported vector field initializer: {error}")
        }
        _ => 0,
    }))
}

/// Register the rows and install [`bridge`] at every address.
pub fn install(vm: &mut VmHost) {
    register(vm);
    for row in RESOURCE_REF_BINDINGS.iter().chain(GAME_REF_BINDINGS) {
        vm.native_impls
            .insert(row.impl_addr, bridge as slr_vm::host::NativeFn);
    }
}

/// The word at `index`, or `0` when the container ran short.
fn word_at(words: &[u32], index: usize) -> u32 {
    words.get(index).copied().unwrap_or(0)
}

/// The declared parameter types of `sig`, in declaration order.
#[must_use]
pub fn param_types(sig: &str) -> Vec<u8> {
    let bytes = sig.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    if bytes.first() == Some(&b'(') {
        i = 1;
    }
    while let Some(&ty) = bytes.get(i) {
        match ty {
            b')' => break,
            b'L' => {
                out.push(b'L');
                while bytes.get(i).is_some_and(|&c| c != b';') {
                    i += 1;
                }
                i += 1;
            }
            b'[' => {
                out.push(b'[');
                while bytes.get(i) == Some(&b'[') {
                    i += 1;
                }
                while bytes.get(i).is_some_and(|&c| c != b';') {
                    i += 1;
                }
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// The return type byte of `sig`.
#[must_use]
pub fn return_type(sig: &str) -> u8 {
    let bytes = sig.as_bytes();
    match bytes.iter().position(|&c| c == b')') {
        Some(close) => bytes.get(close + 1).copied().unwrap_or(b'V'),
        None => b'V',
    }
}

/// The call-log value for one marshalled word of type `ty`.
#[must_use]
pub fn log_value(ty: u8, word: u32) -> LogValue {
    match ty {
        b'F' => LogValue::FloatBits(word),
        b'D' => LogValue::DoubleBits(u64::from(word)),
        b'J' => LogValue::Long(i64::from(word as i32)),
        b'L' | b'[' => LogValue::Int(word as i32),
        _ => LogValue::Int(word as i32),
    }
}

#[cfg(test)]
mod bridge_tests {
    use super::*;

    #[test]
    fn shared_native_entry_preserves_the_requested_overload() {
        for signature in ["(II)I", "(ILjava.lang.String;)I"] {
            let row = bridge_binding(GET_INFO, "getInfo", signature).unwrap();
            assert_eq!(row.sig, signature);
            assert_eq!(row.name, "getInfo");
            let mut vm = VmHost::new();
            install(&mut vm);
            reset_res();
            let args = [
                slr_vm::values::Value::reference(0x999),
                slr_vm::values::Value::int(7),
                slr_vm::values::Value::reference(0),
            ];
            assert_eq!(
                bridge(
                    &mut vm,
                    &CallInfo {
                        jvm: 0,
                        thread: 0,
                        args: &args,
                        class: GAME_REF_CLASS,
                        name: "getInfo",
                        sig: signature
                    }
                ),
                Some(0)
            );
            with_res(|host| {
                assert_eq!(
                    host.records().last(),
                    Some(&LogRecord::NativeCall {
                        class: GAME_REF_CLASS.to_owned(),
                        name: "getInfo".to_owned(),
                        args: vec![LogValue::Int(7), LogValue::Int(0x999)],
                        result: Some(LogValue::Int(0))
                    })
                )
            });
        }
        assert!(bridge_binding(GET_INFO, "getInfo", "(I)I").is_none());
    }
}

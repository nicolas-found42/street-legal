//! GroundRef's global water plane, water limits and fog message producer.
//!
//! `setWater` and `addWaterLimit` use the receiver only as a bound-resource
//! gate; their writes target `DAT_00617650`, the shared physics world.
//! Water writes reach the production physics address space through the required
//! [`WaterDispatch`] callback. Fog is a resource message and is registered only
//! when a host explicitly supplies its consumer to [`install_fog`].
//! The complete original water callees execute in
//! `re/oracle/ground_environment_probe.py`; the fog consumer is a separate seam.

use std::cell::{Cell, RefCell};

use slr_physics::space::Space;
use slr_vm::host::{marshal_args, CallInfo, VmHost};
use slr_x87::{Precision, Rounding, X87};

/// `GroundRef.setWater(FFF)V`.
/// exe: LAB_004866c0
pub const SET_WATER: u32 = 0x0048_66c0;
/// `GroundRef.setWater(Vector3,Vector3,F,F)V`.
/// exe: LAB_004867b0
pub const SET_WATER_PLANE: u32 = 0x0048_67b0;
/// `GroundRef.addWaterLimit(Vector3,Vector3)V`.
/// exe: LAB_00486920
pub const ADD_WATER_LIMIT: u32 = 0x0048_6920;
/// `GroundRef.setFog(IFF)V`.
/// exe: LAB_00486a20
pub const SET_FOG: u32 = 0x0048_6a20;

/// The four native rows, including both overloaded water setters.
pub const BINDINGS: &[(u32, &str, &str)] = &[
    (SET_WATER, "setWater", "(FFF)V"),
    (
        SET_WATER_PLANE,
        "setWater",
        "(Ljava.lang.Vector3;Ljava.lang.Vector3;FF)V",
    ),
    (
        ADD_WATER_LIMIT,
        "addWaterLimit",
        "(Ljava.lang.Vector3;Ljava.lang.Vector3;)V",
    ),
    (SET_FOG, "setFog", "(IFF)V"),
];

/// Payload allocated by `0x486a20`, sent with message code `0x4a`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Fog {
    /// Enabled byte at `+0`.
    pub enabled: bool,
    /// First distance at `+4`, after multiplication by the image's `10.0`.
    pub start: u32,
    /// Second distance at `+8`, after multiplication by `10.0`.
    pub end: u32,
    /// Packed color at `+0xc`.
    pub color: u32,
}

impl Fog {
    /// Encode the native's complete four-field message.
    /// exe: LAB_00486a20
    pub fn new(color: u32, start: u32, end: u32) -> Self {
        Self {
            enabled: true,
            start: (f32::from_bits(start) * 10.0).to_bits(),
            end: (f32::from_bits(end) * 10.0).to_bits(),
            color,
        }
    }

    /// Four message words; only the low byte of the first word is meaningful.
    pub fn words(self) -> [u32; 4] {
        [u32::from(self.enabled), self.start, self.end, self.color]
    }
}

/// World words touched by the water graph, plus pending fog messages.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Environment {
    /// World `+0x80`; setters store one or zero, unbound calls preserve it.
    pub enabled: u32,
    /// World `+0x1b8`, plane point.
    pub position: [u32; 3],
    /// World `+0x1c4`, normalized plane normal.
    pub normal: [u32; 3],
    /// World `+0x1d8`, optional third vector, preserved by both natives.
    pub extra: [u32; 3],
    /// World `+0x1e4/+0x1e8`, the two extent scalars.
    pub sizes: [u32; 2],
    /// World `+0x1d0/+0x1d4`, six-word plane limits in append order.
    pub limits: Vec<[u32; 6]>,
    /// Resource node and message payload; drained by the message consumer.
    pub fog_messages: Vec<(u32, Fog)>,
}

impl Environment {
    /// The ordered x87 `fcomp` tests reject zero, negative and unordered values.
    pub fn water_sizes_valid(sizes: [u32; 2]) -> bool {
        sizes.into_iter().all(|bits| f32::from_bits(bits) > 0.0)
    }

    /// Full `0x49b440`: store scalars, normalize/copy optional vectors, free
    /// every previous limit, then reset the limit count.
    /// exe: FUN_0049b440
    pub fn replace_water(
        &mut self,
        sizes: [u32; 2],
        normal: Option<[u32; 3]>,
        position: Option<[u32; 3]>,
        extra: Option<[u32; 3]>,
    ) {
        self.sizes = sizes;
        if let Some(normal) = normal {
            self.normal = normalize(normal);
        }
        if let Some(position) = position {
            self.position = position;
        }
        if let Some(extra) = extra {
            self.extra = extra;
        }
        self.limits.clear();
    }

    /// Both native setters use this shared valid/disabled/unbound control flow.
    /// exe: LAB_004866c0
    /// exe: LAB_004867b0
    pub fn set_water(
        &mut self,
        bound: bool,
        sizes: [u32; 2],
        normal: [u32; 3],
        position: [u32; 3],
    ) {
        if !bound {
            return;
        }
        if Self::water_sizes_valid(sizes) {
            self.replace_water(sizes, Some(normal), Some(position), None);
            self.enabled = 1;
        } else {
            self.enabled = 0;
        }
    }

    /// Full successful allocator path of `0x49b530`: append two vectors and
    /// return the previous count. Rust allocation failure terminates the host,
    /// as with other native allocations; it cannot invent a successful append.
    /// exe: FUN_0049b530
    pub fn add_water_limit(&mut self, first: [u32; 3], second: [u32; 3]) -> u32 {
        let index = self.limits.len() as u32;
        self.limits.push([
            first[0], first[1], first[2], second[0], second[1], second[2],
        ]);
        index
    }
}

// The machine squares x, then y, sums those, then squares/adds z, fsqrt,
// tests C3, and only then computes reciprocal and three products. Extended
// exponent range matters for huge and subnormal inputs even with PC=24.
fn normalize(words: [u32; 3]) -> [u32; 3] {
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let [x, y, z] = words.map(|word| X87::from_f32(f32::from_bits(word)));
    let length = x
        .mul(x, pc, rc)
        .add(y.mul(y, pc, rc), pc, rc)
        .add(z.mul(z, pc, rc), pc, rc)
        .sqrt(pc, rc);
    // `test ah,0x40` skips normalization for both equal and unordered.
    if length.fcom(X87::from_f32(0.0)).0.c3 {
        return words;
    }
    let inverse = X87::from_f32(1.0).div(length, pc, rc);
    [x, y, z].map(|value| inverse.mul(value, pc, rc).to_f32(pc, rc).to_bits())
}

/// The resource message consumer, supplied by the production engine.
pub type FogDispatch = fn(&mut VmHost, u32, Fog);

/// Fully decoded water mutation; all words retain their original f32 bits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaterCommand {
    /// `0x49b440` plus the native's enabling store at world `+0x80`.
    Set {
        sizes: [u32; 2],
        normal: [u32; 3],
        position: [u32; 3],
    },
    /// The disabled-size branch only clears world `+0x80`.
    Disable,
    /// `0x49b530` appends one six-word limit.
    AddLimit { first: [u32; 3], second: [u32; 3] },
}

/// Connect decoded native calls to the live physics world.
pub type WaterDispatch = fn(&mut VmHost, WaterCommand);

/// Execute a decoded native call directly in the shared physics address space.
/// No duplicate world state is maintained by the production bridge.
/// exe: FUN_0049b440
/// exe: FUN_0049b530
pub fn apply_water(memory: &mut Space, world: u32, command: WaterCommand) {
    match command {
        WaterCommand::Set {
            sizes,
            normal,
            position,
        } => {
            for (offset, values) in [(0x1b8, position), (0x1c4, normalize(normal))] {
                for (index, word) in values.into_iter().enumerate() {
                    memory.w32(world + offset + index as u32 * 4, word);
                }
            }
            memory.w32(world + 0x1e4, sizes[0]);
            memory.w32(world + 0x1e8, sizes[1]);
            // Space's allocator never reuses freed storage, matching the other
            // exact object-graph ports. The original frees the old allocation.
            memory.w32(world + 0x1d0, 0);
            memory.w32(world + 0x1d4, 0);
            memory.w32(world + 0x80, 1);
        }
        WaterCommand::Disable => memory.w32(world + 0x80, 0),
        WaterCommand::AddLimit { first, second } => {
            let count = memory.r32(world + 0x1d4);
            let old = memory.r32(world + 0x1d0);
            let buffer = memory.alloc(count.wrapping_add(1).wrapping_mul(24));
            if old != 0 {
                memory.copy(buffer, old, count * 24);
            }
            for (index, word) in first.into_iter().chain(second).enumerate() {
                memory.w32(buffer + count * 24 + index as u32 * 4, word);
            }
            memory.w32(world + 0x1d0, buffer);
            memory.w32(world + 0x1d4, count.wrapping_add(1));
        }
    }
}

thread_local! {
    static ENVIRONMENT: RefCell<Environment> = RefCell::new(Environment::default());
    static FOG_DISPATCH: Cell<Option<FogDispatch>> = const { Cell::new(None) };
    static WATER_DISPATCH: Cell<Option<WaterDispatch>> = const { Cell::new(None) };
}

/// Inspect or update this thread's shared world environment.
pub fn with_environment<R>(f: impl FnOnce(&mut Environment) -> R) -> R {
    ENVIRONMENT.with(|state| f(&mut state.borrow_mut()))
}

/// Reset the world state and any dispatcher at the start of a replay.
pub fn reset() {
    with_environment(|state| *state = Environment::default());
    FOG_DISPATCH.with(|dispatch| dispatch.set(None));
    WATER_DISPATCH.with(|dispatch| dispatch.set(None));
}

fn vector(vm: &mut VmHost, word: u32) -> [u32; 3] {
    ["x", "y", "z"].map(|field| {
        let bits = vm
            .read_field(word, field)
            .expect("GroundRef vector fields")
            .payload;
        // 42a560 returns the field in ST(0), then the native stores m32.
        // That round trip quiets signaling NaNs before even a plain copy
        // such as addWaterLimit; a raw VM payload copy misses this effect.
        X87::from_f32(f32::from_bits(bits))
            .to_f32(Precision::F32, Rounding::NearestEven)
            .to_bits()
    })
}

/// VM bridge shared by all four environment rows.
pub fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    let mut args = [0; 5];
    marshal_args(info, &mut args).expect("valid GroundRef environment signature");
    let node = vm
        .read_field(args[0], "ptr")
        .expect("GroundRef Native.ptr field")
        .payload;
    if address == SET_FOG {
        if node != 0 {
            let fog = Fog::new(args[1], args[2], args[3]);
            FOG_DISPATCH
                .with(Cell::get)
                .expect("installed fog consumer")(vm, node, fog);
        }
        return None;
    }
    let bound = super::node::with_res(|host| {
        host.node(node)
            .expect("GroundRef water requires a live resource node")
            .container
            != 0
    });
    if !bound {
        return None;
    }
    let command = match address {
        SET_WATER if Environment::water_sizes_valid([args[2], args[3]]) => WaterCommand::Set {
            sizes: [args[2], args[3]],
            normal: [0, 1.0f32.to_bits(), 0],
            position: [0, args[1], 0],
        },
        SET_WATER => WaterCommand::Disable,
        SET_WATER_PLANE => {
            let sizes = [args[3], args[4]];
            if Environment::water_sizes_valid(sizes) {
                let position = vector(vm, args[1]);
                let normal = vector(vm, args[2]);
                WaterCommand::Set {
                    sizes,
                    normal,
                    position,
                }
            } else {
                WaterCommand::Disable
            }
        }
        ADD_WATER_LIMIT => {
            let first = vector(vm, args[1]);
            let second = vector(vm, args[2]);
            WaterCommand::AddLimit { first, second }
        }
        _ => unreachable!("only registered environment natives enter bridge"),
    };
    WATER_DISPATCH
        .with(Cell::get)
        .expect("installed physics world consumer")(vm, command);
    None
}

/// Register the three water natives with a mandatory live-world consumer.
pub fn install(vm: &mut VmHost, dispatch: WaterDispatch) {
    WATER_DISPATCH.with(|slot| slot.set(Some(dispatch)));
    for &(address, name, sig) in &BINDINGS[..3] {
        vm.natives
            .register("java.util.resource.GroundRef", name, sig, address);
        vm.native_impls.insert(address, bridge);
    }
}

/// Register the fog producer only when its resource message consumer exists.
pub fn install_fog(vm: &mut VmHost, dispatch: FogDispatch) {
    FOG_DISPATCH.with(|slot| slot.set(Some(dispatch)));
    let (address, name, sig) = BINDINGS[3];
    vm.natives
        .register("java.util.resource.GroundRef", name, sig, address);
    vm.native_impls.insert(address, bridge);
}

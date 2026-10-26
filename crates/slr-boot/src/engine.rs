//! Behavioural model of the engine's script event system and frame step.
//!
//! The exe keeps these as intrusive lists on the game context and on each
//! gametype instance's loaded data. Scripts observe them only through the
//! `java.lang.GameType` natives (masks, notifications, timers, callback
//! registration) and through the script methods the frame step calls back
//! (`handleEvent`, `control`, `animate`). This module models that observable
//! behaviour from the executable's decompiled control flow:
//!
//! | exe | model |
//! |---|---|
//! | `FUN_0048d3d0`/`FUN_0048d420` | event mask set/clear on instance data `+0x70` |
//! | `FUN_0048b500`/`FUN_0048b670` | notification add/remove on the source's data `+0x5c` |
//! | `FUN_0048b750`/`FUN_0048b8c0` | timer add / remove-all (global list) |
//! | `FUN_00427370`/`FUN_004274e0` | callback registration, kinds 8 `control`, 9 `drive`, 0x1c `animate` |
//! | `FUN_004265c0`/`FUN_00425c60` | event dispatch to the script's `handleEvent` |
//! | `FUN_00427160` | timer pass |
//! | `FUN_004291e0` / `FUN_00426fd0` | `control(F)` / `animate()` callback passes |
//! | `FUN_004a5190` | world step: with no rigid bodies, simulated time advances |
//!
//! None of it is differentially tested against the original yet: the original
//! functions manipulate raw lists and virtual objects that the port does not
//! reproduce. It is validated only by the whole-run native-call comparison.
//! Scenario inputs, declared rather than observed: the fixed frame interval
//! and the physics world's maximum step.
use crate::sched;
use slr_boot::native_event_route;
use slr_resource::node::with_res;
use slr_vm::{
    host::{marshal_args, CallInfo, VmHost},
    values::Value,
};
use std::{cell::RefCell, collections::HashMap};

/// `_DAT_005f09d4`: the world-step and control-accumulator threshold.
const STEP_THRESHOLD: f32 = f32::from_bits(0x38d1_b717);

#[derive(Clone, Debug)]
pub enum EventArg {
    Int(i32),
    Str(Option<String>),
}

#[derive(Clone, Debug)]
struct Timer {
    container: u32,
    time: f32,
    interval: f32,
    kind: u32,
    id: u32,
    name: Option<String>,
    alive: bool,
}

/// Frame-step scenario, declared not observed.
#[derive(Clone, Copy, Debug)]
pub struct FrameConfig {
    /// Seconds per frame (`DAT_0063c534`, the measured previous frame time).
    pub frame_dt: f32,
    /// World maximum step (`*(PTR_DAT_00617650 + 8)`), seconds.
    pub max_step: f32,
    /// Context time scale (`context + 0x110`).
    pub time_scale: f32,
}

impl Default for FrameConfig {
    fn default() -> Self {
        Self {
            frame_dt: 1.0 / 60.0,
            max_step: 0.01,
            time_scale: 1.0,
        }
    }
}

/// Retained bounded host projection, pending actual4797d0 raw player storage.
/// That constructor explicitly clears the target count;436370 is a different
/// class and cannot establish this layout. Nonempty behavior stays explicit.
#[derive(Default)]
struct PlayerFrameState {
    targets: u32,
    focus: u32,
    camera: u32,
    render_refs: [u32; 8],
    unmodelled_command: Option<String>,
}

#[derive(Default)]
struct State {
    masks: HashMap<u32, u32>,
    timers: Vec<Timer>,
    sim_time: f32,
    world_acc: f32,
    control_acc: f32,
    config: FrameConfig,
    previous_animation_time: f32,
    animation_dt: f32,
    player_animation_dt: f32,
    players: HashMap<u32, PlayerFrameState>,
    /// Event dispatches and callback passes, for inspection.
    pub trace: Vec<String>,
}

thread_local! {
    static STATE: RefCell<State> = RefCell::new(State::default());
}

fn with<R>(f: impl FnOnce(&mut State) -> R) -> R {
    STATE.with(|s| f(&mut s.borrow_mut()))
}

pub fn configure(config: FrameConfig) {
    crate::native_body::with_world_memory(|m| {
        crate::native_callbacks::reset(m, crate::native_callbacks::CONTEXT);
    })
    .expect("frame callbacks configured outside native memory lease");
    with(|s| {
        *s = State {
            config,
            animation_dt: config.frame_dt,
            ..State::default()
        };
    });
    slr_lang::seam::with_lang_host(|h| h.sim_time = 0.0);
}

pub fn take_trace() -> Vec<String> {
    with(|s| std::mem::take(&mut s.trace))
}

/// `428a5b/428a77`: initialize the two independent frame-loop clock readings.
pub fn begin_replayed_frames() {
    use slr_vm::clock::{with_site, ClockSite};
    use slr_x87::{Precision, Rounding, X87};
    let seconds = |site| {
        with_site(site, sched::clock_ms)
            .mul(X87::from_f32(0.001), Precision::F32, Rounding::NearestEven)
            .to_f32(Precision::F32, Rounding::NearestEven)
    };
    let epoch = seconds(ClockSite::FramePhysicsEpoch);
    slr_lang::seam::with_lang_host(|host| {
        host.time_warp_rate = epoch;
        host.sim_time = 0.0;
    });
    let previous = seconds(ClockSite::FramePreviousTime);
    with(|state| {
        state.sim_time = 0.0;
        state.previous_animation_time = previous;
        state.config.frame_dt = 0.0;
        state.config.max_step = 0.02;
    });
}

/// `428ad5`: sample animation time independently of the previous frame timer.
pub fn sample_replayed_animation_time() {
    use slr_x87::{Precision, Rounding, X87};
    let seconds = slr_vm::clock::with_site(
        slr_vm::clock::ClockSite::FrameAnimationDelta,
        sched::clock_ms,
    )
    .mul(X87::from_f32(0.001), Precision::F32, Rounding::NearestEven);
    with(|state| {
        let elapsed = seconds
            .sub(
                X87::from_f32(state.previous_animation_time),
                Precision::F32,
                Rounding::NearestEven,
            )
            .to_f32(Precision::F32, Rounding::NearestEven);
        let elapsed = if elapsed > 0.1 { 0.1 } else { elapsed };
        state.previous_animation_time = seconds.to_f32(Precision::F32, Rounding::NearestEven);
        state.animation_dt = elapsed;
        state.trace.push(format!("animation elapsed={elapsed:?}"));
    });
}

pub fn set_frame_dt(seconds: f32) {
    with(|state| state.config.frame_dt = seconds);
}

/// The native player starts with EVENT_COMMAND subscribed. Observed at
/// FUN_004265c0's loaded-data branch in the headless original (mask +0x70).
pub fn player_created(container: u32) {
    with(|s| {
        *s.masks.entry(container).or_insert(0) |= 0x10;
        s.players.insert(container, PlayerFrameState::default());
    });
}

/// Controller native bridge: resolve the same loaded native player resource
/// used by the original wrapper before entering its mapping table.
pub fn controller_bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    let stub = slr_io::input::stub_by_addr(address)?;
    let mut words = [0; slr_io::input::MAX_WORDS];
    let written = marshal_args(info, &mut words).unwrap_or(0);
    let owner = vm
        .read_field(words[0], "ptr")
        .ok()
        .and_then(|ptr| with_res(|host| host.node(ptr.payload).map(|node| node.container)))
        .filter(|container| {
            with_res(|host| {
                host.container(*container)
                    .is_some_and(|c| c.type_id == 1 && c.load_state & 1 != 0)
            }) && crate::factory::native_class(*container).as_deref() == Some("player")
        });
    slr_io::input::with_input(|host| {
        let result = match owner {
            Some(owner) => host.call_controller(stub, &words[..written], owner),
            None => host.call(stub, &words[..written]),
        };
        slr_calllog::headless::record(
            host.records()
                .last()
                .expect("controller call recorded")
                .clone(),
        );
        result
    })
}

/// Simulated world time in seconds (`**0x617650`).
#[must_use]
pub fn sim_time() -> f32 {
    with(|s| s.sim_time)
}

fn container_of(script: u32) -> Option<u32> {
    with_res(|h| {
        let node = h.object_node(script);
        (node != 0).then(|| h.node(node).map(|n| n.container))?
    })
    .filter(|c| *c != 0)
}

fn words(info: &CallInfo<'_>) -> Result<[u32; 6], String> {
    let mut out = [0; 6];
    marshal_args(info, &mut out).map_err(|e| format!("argument marshalling: {e:?}"))?;
    Ok(out)
}

fn string(vm: &VmHost, word: u32) -> Option<String> {
    if word == 0 {
        return None;
    }
    vm.string_bytes(word)
        .map(|b| String::from_utf8_lossy(b).into_owned())
}

fn receiver_container(info: &CallInfo<'_>, w: &[u32; 6]) -> Result<u32, String> {
    container_of(w[0]).ok_or_else(|| {
        format!(
            "{}.{}: receiver {:#x} has no native container",
            info.class, info.name, w[0]
        )
    })
}

fn fail(vm: &mut VmHost, info: &CallInfo<'_>, error: String) -> Option<u32> {
    vm.native_failure = Some(format!("{}.{}{}: {error}", info.class, info.name, info.sig));
    None
}

/// exe: FUN_004819f0 -> FUN_0048d3d0
pub fn set_event_mask(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, mask)) => {
            if std::env::var_os("SLR_DISPATCH_TRACE").is_some() {
                let w = words(info).unwrap_or_default();
                eprintln!(
                    "SETMASK receiver={:#x} container={c:#x} id={:?} mask={mask:#x} script={:#x}",
                    w[0],
                    with_res(|h| h.container(c).map(|x| x.id)),
                    script_of(c)
                );
            }
            if let Err(e) = change_instance_mask(c, mask, true) {
                return fail(vm, info, e);
            }
        }
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: FUN_00481a30 -> FUN_0048d420
pub fn clear_event_mask(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, mask)) => {
            if std::env::var_os("SLR_DISPATCH_TRACE").is_some() {
                eprintln!("CLEARMASK container={c:#x} mask={mask:#x}");
            }
            if let Err(e) = change_instance_mask(c, mask, false) {
                return fail(vm, info, e);
            }
        }
        Err(e) => return fail(vm, info, e),
    }
    None
}

fn change_instance_mask(container: u32, bits: u32, set: bool) -> Result<(), String> {
    let host_mask = with(|s| s.masks.get(&container).copied().unwrap_or(0));
    let native = matches!(
        crate::factory::native_class(container).as_deref(),
        Some("car" | "part" | "cursor" | "player")
    );
    let mask = crate::native_body::with_world_memory(|m| {
        let raw = crate::native_body::raw_resource(m, container)?;
        let mask = if native {
            m.r32(raw + 0x148)
        } else {
            host_mask
        };
        let mask = if set { mask | bits } else { mask & !bits };
        m.w32(raw + 0x148, mask);
        Ok::<_, String>(mask)
    })??;
    with(|s| s.masks.insert(container, mask));
    Ok(())
}

fn add_notification_with(vm: &mut VmHost, info: &CallInfo<'_>, extended: bool) -> Option<u32> {
    let w = match words(info) {
        Ok(w) => w,
        Err(e) => return fail(vm, info, e),
    };
    // The exe guards on the GameRef argument but resolves the receiver.
    if w[1] == 0 {
        return None;
    }
    let (Some(listener), Some(source)) = (container_of(w[0]), container_of(w[1])) else {
        return fail(
            vm,
            info,
            "receiver or source GameRef has no native container (the exe logs and returns)".into(),
        );
    };
    let (mask, kind) = (w[2], w[3]);
    let arg = string(vm, w[4]);
    let name = if extended { string(vm, w[5]) } else { None };
    let result = with_notification_references(source, listener, |m, source, listener| {
        let argument = store_event_text(m, arg.as_deref());
        crate::native_notifications::add(
            m,
            source,
            listener,
            mask,
            kind,
            argument,
            name.as_deref().map(str::as_bytes),
            &mut crate::native_notifications::Runtime,
        )
    });
    if let Err(error) = result {
        return fail(vm, info, error);
    }
    None
}

fn store_event_text(m: &mut slr_physics::space::Space, text: Option<&str>) -> u32 {
    let Some(text) = text else {
        return 0;
    };
    let pointer = m.malloc(text.len() as u32 + 1);
    for (i, byte) in text.bytes().enumerate() {
        m.w8(pointer + i as u32, byte);
    }
    m.w8(pointer + text.len() as u32, 0);
    pointer
}
pub(crate) fn read_event_text(m: &slr_physics::space::Space, pointer: u32) -> Option<String> {
    if pointer == 0 {
        return None;
    }
    let mut bytes = vec![];
    let mut cursor = pointer;
    while m.r8(cursor) != 0 {
        bytes.push(m.r8(cursor));
        cursor = cursor.wrapping_add(1);
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}
fn with_notification_references(
    source: u32,
    listener: u32,
    f: impl FnOnce(&mut slr_physics::space::Space, u32, u32) -> Result<u32, String>,
) -> Result<u32, String> {
    crate::native_body::with_world_memory(|m| {
        let source = crate::native_body::raw_resource(m, source)?;
        let listener = crate::native_body::raw_resource(m, listener)?;
        let refs = m.alloc(32);
        crate::native_part::bind_reference(m, refs, source);
        crate::native_part::bind_reference(m, refs + 16, listener);
        let result = f(m, refs, refs + 16);
        crate::native_part::bind_reference(m, refs, 0);
        crate::native_part::bind_reference(m, refs + 16, 0);
        result
    })?
}

struct NotificationRuntime<'a>(&'a mut VmHost);
impl native_event_route::NotificationHost for NotificationRuntime<'_> {
    fn read(&mut self, address: u32) -> Result<u32, String> {
        crate::native_body::with_world_memory(|m| m.r32(address))
    }
    fn forward(
        &mut self,
        reference: u32,
        source: u32,
        event: u32,
        argument: u32,
        name: u32,
    ) -> Result<(), String> {
        let (listener, arg, name) = crate::native_body::with_world_memory(|m| {
            let listener = crate::native_body::logical_resource(m.r32(reference + 12));
            let arg = if event == 0x0010_0000 || event & 0x0fff_ffff == 0x80 {
                EventArg::Int(argument as i32)
            } else {
                EventArg::Str(read_event_text(m, argument))
            };
            let name = if event == 0x0010_0000 {
                None
            } else {
                read_event_text(m, name)
            };
            (listener, arg, name)
        })?;
        if let Some(listener) = listener {
            // This host projects source identity as the resource handle; the
            // raw4265c0 host uses the retained weak-reference address instead.
            dispatch(self.0, listener, Some(source), event, &arg, name.as_deref())?;
        }
        Ok(())
    }
    fn destroy_notification(&mut self, node: u32) -> Result<(), String> {
        crate::native_body::with_world_memory(|m| {
            crate::native_notifications::destroy(
                m,
                node,
                1,
                &mut crate::native_notifications::Runtime,
            );
        })
    }
}
fn dispatch_notifications(
    vm: &mut VmHost,
    container: u32,
    source: Option<u32>,
    mask: u32,
    arg: &EventArg,
) -> Result<(), String> {
    let (envelope, argument) = crate::native_body::with_world_memory(|m| {
        let raw = crate::native_body::raw_resource(m, container)?;
        let argument = match arg {
            EventArg::Int(value) => *value as u32,
            EventArg::Str(text) => store_event_text(m, text.as_deref()),
        };
        Ok::<_, String>((raw + 0xd8, argument))
    })??;
    native_event_route::walk_notifications(
        &mut NotificationRuntime(vm),
        envelope,
        source.unwrap_or(0),
        mask,
        argument,
        container,
    )
}

/// exe: LAB_0047e050 -> FUN_0048b500
pub fn add_notification(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    add_notification_with(vm, info, false)
}

/// exe: LAB_0047e100 -> FUN_0048b500
pub fn add_notification_extended(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    add_notification_with(vm, info, true)
}

/// exe: LAB_0047e000 -> FUN_0048b670
pub fn rem_notification(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = match words(info) {
        Ok(w) => w,
        Err(e) => return fail(vm, info, e),
    };
    if w[1] == 0 {
        return None;
    }
    let (Some(listener), Some(source)) = (container_of(w[0]), container_of(w[1])) else {
        return None;
    };
    let mask = w[2];
    if let Err(error) = with_notification_references(source, listener, |m, source, listener| {
        crate::native_notifications::remove(
            m,
            source,
            listener,
            mask,
            &mut crate::native_notifications::Runtime,
        )
    }) {
        return fail(vm, info, error);
    }
    None
}

/// exe: LAB_0047e1c0 -> FUN_0048b750. The wrapper offsets the delay by the
/// world time and files a one-shot (`0x80000000`) timer of event type `0x80`.
pub fn add_timer(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1], w[2])));
    match r {
        Ok((container, delay, id)) => with(|s| {
            let time = f32::from_bits(delay) + s.sim_time;
            s.timers.push(Timer {
                container,
                time,
                interval: 0.0,
                kind: 0x8000_0080,
                id,
                name: None,
                alive: true,
            });
        }),
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: LAB_0047e260 -> FUN_0048b8c0
pub fn remove_all_timers(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| receiver_container(info, &w));
    match r {
        Ok(c) => with(|s| {
            for t in s.timers.iter_mut().filter(|t| t.container == c) {
                t.alive = false;
            }
        }),
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: FUN_00481c40 -> FUN_00427370
pub fn register_callback(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, kind)) => {
            if !matches!(kind, 8 | 9 | 0x1c) {
                return None;
            }
            if let Err(error) = append_callback(c, kind) {
                return fail(vm, info, error);
            }
        }
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: FUN_00481c80 -> FUN_004274e0 (the first matching record goes dead)
pub fn unregister_callback(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, kind)) => {
            let id = with_res(|host| host.container(c).map_or(0, |resource| resource.id));
            if let Err(error) = crate::native_body::with_world_memory(|m| {
                crate::native_callbacks::unregister(m, crate::native_callbacks::CONTEXT, id, kind);
            }) {
                return fail(vm, info, error);
            }
        }
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: LAB_00481cc0, which forwards to a bare `ret` in build 606.
pub fn unregister_callbacks(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
    None
}

fn append_callback(container: u32, kind: u32) -> Result<(), String> {
    crate::native_body::with_world_memory(|m| {
        let raw = crate::native_body::raw_resource(m, container)?;
        crate::native_callbacks::register(m, crate::native_callbacks::CONTEXT, raw, kind);
        Ok(())
    })?
}

fn new_game_ref(vm: &mut VmHost, container: u32) -> Result<u32, String> {
    let word = vm
        .alloc_object("java.util.resource.GameRef")
        .map_err(|e| format!("GameRef allocation: {e:?}"))?;
    let node = with_res(|h| {
        let id = h.container(container).map_or(0, |c| c.id);
        let node = h.add_bound_node(id, container);
        h.bind_object(word, node);
        node
    });
    vm.write_field(word, "ptr", Value::int(node as i32))
        .map_err(|e| format!("GameRef.ptr: {e:?}"))?;
    Ok(word)
}

fn script_of(container: u32) -> u32 {
    with_res(|h| {
        h.container(container)
            .and_then(|c| c.instance.as_ref().map(|i| i.script))
    })
    .unwrap_or(0)
}

fn object_class(vm: &VmHost, word: u32) -> Option<slr_vm::vmclass::ClassId> {
    match vm.objects.get(word.checked_sub(0x1000)? as usize)?.class {
        slr_vm::vmclass::ClassRef::Loaded(id) => Some(id),
        slr_vm::vmclass::ClassRef::Array(_) => None,
    }
}

/// Call a script method on `script` synchronously on a fresh queued thread
/// (`FUN_004255e0`: thread creation, argument push, call by name, then an
/// unbudgeted run).
pub(crate) fn call_script(
    vm: &mut VmHost,
    script: u32,
    method: &str,
    args: &[Value],
) -> Result<(), String> {
    let class = object_class(vm, script).ok_or("script instance has no loaded class")?;
    let container = with_res(|host| {
        host.node(host.object_node(script))
            .map(|node| node.container)
    });
    let name = container
        .and_then(crate::factory::runtime_name)
        .ok_or("callback resource name unavailable")?;
    sched::run_callback(vm, &name, class, script, method, args, false)
        .map(|_| ())
        .map_err(|(thread, error)| {
            let detail = crate::script_stop(
                vm,
                &thread,
                &format!("script call {method} on {script:#x}"),
                error,
            );
            if let Some(failure) = &vm.native_failure {
                return format!("{detail}; NativeBackendBoundary: {failure}");
            }
            match slr_vm::host::last_host_error() {
                Some(host) => format!("{detail}; last host error: {host}"),
                None => detail,
            }
        })
}

/// Run the created envelope's getInfo callback, including4255e0's conversion
/// of returned Native objects into their resource-link word.
pub(crate) fn query_script(
    vm: &mut VmHost,
    container: u32,
    script: u32,
    message: u32,
) -> Result<u32, String> {
    let class = object_class(vm, script).ok_or("getInfo script has no loaded class")?;
    let name =
        crate::factory::runtime_name(container).ok_or("getInfo resource name unavailable")?;
    let Some(value) = sched::run_callback(
        vm,
        &name,
        class,
        script,
        "getInfo",
        &[Value::int(message as i32)],
        true,
    )
    .map_err(|(thread, error)| crate::script_stop(vm, &thread, "postcreate getInfo", error))?
    else {
        return Ok(0);
    };
    if value.tag.name().starts_with('L') && value.payload != 0 {
        if let Some(mut class) = object_class(vm, value.payload) {
            loop {
                let entry = vm
                    .loader
                    .find_by_id(class)
                    .ok_or("getInfo result class missing")?;
                if entry.name == "java.lang.Native" {
                    return vm
                        .read_field(value.payload, "ptr")
                        .map(|v| v.payload)
                        .map_err(|e| format!("getInfo Native.ptr: {e:?}"));
                }
                let Some(base) = entry.base.as_deref() else {
                    break;
                };
                let Some(parent) = vm.loader.lookup(base) else {
                    break;
                };
                class = parent;
            }
        }
    }
    Ok(value.payload)
}

/// Original427210 queries8,9,28 before appending any callback records.
pub fn register_created(vm: &mut VmHost, container: u32, _script: u32) -> Result<(), String> {
    let values = crate::native_query::created_capabilities(vm, container)?;
    for (kind, value) in [8, 9, 0x1c].into_iter().zip(values) {
        if value != 0 {
            append_callback(container, kind)?;
        }
    }
    Ok(())
}

pub(crate) fn deliver_script(
    vm: &mut VmHost,
    container: u32,
    source: Option<u32>,
    mask: u32,
    arg: &EventArg,
    name: Option<&str>,
) -> Result<(), String> {
    deliver_script_to(vm, script_of(container), container, source, mask, arg, name)
}

pub(crate) fn deliver_script_to(
    vm: &mut VmHost,
    script: u32,
    container: u32,
    source: Option<u32>,
    mask: u32,
    arg: &EventArg,
    name: Option<&str>,
) -> Result<(), String> {
    let kind = mask & 0x0fff_ffff;
    if script == 0 {
        return Ok(());
    }
    if matches!(kind, 1 | 4 | 0x20 | 0x40) {
        return Err(format!(
            "event type {kind:#x} carries a physics argument structure that is not modelled"
        ));
    }
    // Original425e16 compares the complete mask. This callback carries one
    // VM object, before string conversion, GameRef construction or asyncMode.
    if mask == 0x0010_0000 {
        let word = match arg {
            EventArg::Int(word) => *word as u32,
            EventArg::Str(None) => 0,
            EventArg::Str(Some(_)) => {
                return Err("NativeVmBoundary: object event requires a VM object word".into());
            }
        };
        if word != 0
            && word
                .checked_sub(0x1000)
                .and_then(|index| vm.objects.get(index as usize))
                .is_none()
        {
            return Err("NativeVmBoundary: object event has unknown VM identity".into());
        }
        return call_script(vm, script, "handleEvent", &[Value::reference(word)]);
    }
    // Original425e49 constructs the string before allocating the source ref.
    let argument = match (kind, arg) {
        (0x80, EventArg::Int(value)) => Value::int(*value),
        (0x80, EventArg::Str(None)) => Value::int(0),
        (0x80, EventArg::Str(Some(_))) => {
            return Err("NativeVmBoundary: integer event carries a native text address without VM integer identity".into());
        }
        (_, EventArg::Int(0)) => Value::reference_named(0, "Ljava.lang.String;"),
        (_, EventArg::Int(_)) => {
            return Err("NativeVmBoundary: text event requires a native string conversion".into());
        }
        (_, EventArg::Str(text)) => {
            let word = match text {
                Some(text) => vm
                    .intern_string(Some(text.as_bytes()))
                    .map_err(|e| format!("event string: {e:?}"))?,
                None => 0,
            };
            Value::reference_named(word, "Ljava.lang.String;")
        }
    };
    let origin = source.unwrap_or(container);
    let game_ref = new_game_ref(vm, origin)?;
    let asynchronous = vm
        .read_field(script, "asyncMode")
        .map(|v| v.payload != 0)
        .unwrap_or(false);
    let method = name.unwrap_or(if asynchronous {
        "handleEventAsync"
    } else {
        "handleEvent"
    });
    let mut args = vec![
        Value::reference_named(game_ref, "Ljava.util.resource.GameRef;"),
        Value::int(kind as i32),
    ];
    args.push(argument);
    with(|s| {
        s.trace.push(format!(
            "dispatch container={container:#x} type={kind:#x} method={method} arg={arg:?}"
        ));
    });
    call_script(vm, script, method, &args)?;
    Ok(())
}

/// exe: FUN_004265c0 with FUN_00425c60 for the argument shapes modelled here.
pub fn dispatch(
    vm: &mut VmHost,
    container: u32,
    source: Option<u32>,
    mask: u32,
    arg: &EventArg,
    name: Option<&str>,
) -> Result<(), String> {
    if let EventArg::Str(Some(text)) = arg {
        if let Some(class) = crate::factory::native_class(container) {
            crate::world_scene::command(container, &class, text)?;
        }
    }
    if matches!(
        crate::factory::native_class(container).as_deref(),
        Some("car" | "part" | "cursor")
    ) {
        return crate::native_install_runtime::dispatch(vm, container, source, mask, arg, name);
    }
    let kind = mask & 0x0fff_ffff;
    let is_type1 = with_res(|h| h.container(container).is_some_and(|c| c.type_id == 1));
    let instance_mask = with(|s| s.masks.get(&container).copied().unwrap_or(0));
    let script = script_of(container);
    if std::env::var_os("SLR_DISPATCH_TRACE").is_some() {
        let info = with_res(|h| {
            h.container(container).map(|c| {
                (
                    c.type_id,
                    c.id,
                    c.parent,
                    c.instance.as_ref().map(|i| i.script),
                )
            })
        });
        eprintln!(
            "DISPATCHTRY container={container:#x} type1={is_type1} mask={mask:#x} instance_mask={instance_mask:#x} script={script:#x} info={info:?}"
        );
    }
    if !is_type1 {
        if std::env::var_os("SLR_DISPATCH_TRACE").is_some() {
            eprintln!("DROP_TYPE container={container:#x} mask={mask:#x}");
        }
        return Ok(());
    }
    // FUN_004265c0 first offers the event to the native object's vtable +0x38.
    // FUN_0047a920 consumes Controller commands and invokes setcontrol directly.
    let native_consumed = instance_mask & mask != 0
        && kind == 0x10
        && crate::factory::native_class(container).as_deref() == Some("player");
    if native_consumed {
        if let EventArg::Str(Some(text)) = arg {
            match text.split_whitespace().next() {
                Some("setcontrol") if script != 0 => {
                    call_script(vm, script, "setcontrol", &[])?;
                }
                Some("device") => {
                    return Err(
                        "NativePlayerBoundary: original47a920 device command is not integrated"
                            .into(),
                    );
                }
                Some("camera") => {
                    let id = text
                        .split_whitespace()
                        .nth(1)
                        .and_then(|v| v.parse::<i32>().ok())
                        .ok_or("NativePlayerBoundary: malformed camera command")?
                        as u32;
                    let link = slr_resource::node::with_res(|host| {
                        if id == 0 {
                            return Ok(0);
                        }
                        let target = host
                            .container_for_id(id)
                            .ok_or("camera resource unavailable")?;
                        if host.container(target).is_none_or(|c| c.type_id != 1) {
                            return Err("camera requires a game resource");
                        }
                        Ok(host.add_bound_node(id, target))
                    })?;
                    let old = with(|state| {
                        std::mem::replace(
                            &mut state
                                .players
                                .get_mut(&container)
                                .expect("registered player")
                                .camera,
                            link,
                        )
                    });
                    if old != 0 {
                        slr_resource::node::with_res(|h| h.release_embedded_link(old));
                    }
                }
                Some(
                    command @ ("viewport" | "osd" | "renderinstance" | "controllable" | "control"
                    | "uncontrol"),
                ) => {
                    return Err(format!(
                        "NativePlayerBoundary: original47a920 {command} command is not integrated"
                    ));
                }
                command => {
                    return Err(format!(
                    "NativePlayerBoundary: original47a920 command {command:?} is not integrated"
                ))
                }
            }
        }
    }
    if std::env::var_os("SLR_DISPATCH_TRACE").is_some() && instance_mask & mask == 0 {
        let id = with_res(|h| h.container(container).map(|c| c.id));
        eprintln!(
            "DROP_MASK container={container:#x} id={id:?} mask={mask:#x} instance_mask={instance_mask:#x}"
        );
    }
    if instance_mask & mask != 0 && script != 0 && !native_consumed {
        deliver_script(vm, container, source, mask, arg, name)?;
    }
    dispatch_notifications(vm, container, source, mask, arg)?;
    Ok(())
}

/// exe: FUN_00427160
fn timer_pass(vm: &mut VmHost) -> Result<(), String> {
    let mut i = 0;
    loop {
        let (len, current) = with(|s| (s.timers.len(), s.timers.get(i).cloned()));
        let Some(timer) = current else { break };
        let has_next = i + 1 < len;
        if !timer.alive {
            with(|s| s.timers.remove(i));
            if !has_next {
                break;
            }
            continue;
        }
        let now = sim_time();
        if timer.time <= now {
            dispatch(
                vm,
                timer.container,
                None,
                timer.kind & 0x0fff_ffff,
                &EventArg::Int(timer.id as i32),
                timer.name.as_deref(),
            )?;
            let one_shot = timer.kind & 0x8000_0000 != 0;
            with(|s| {
                if one_shot {
                    if let Some(t) = s.timers.get_mut(i) {
                        if t.id == timer.id && t.container == timer.container {
                            t.alive = false;
                        }
                    }
                } else if let Some(t) = s.timers.get_mut(i) {
                    t.time += t.interval;
                }
            });
            // The exe unlinks and frees a spent one-shot record at once.
            if one_shot {
                with(|s| {
                    if s.timers.get(i).is_some_and(|t| !t.alive) {
                        s.timers.remove(i);
                    }
                });
                if !has_next {
                    break;
                }
                continue;
            }
        }
        if !has_next {
            break;
        }
        i += 1;
    }
    Ok(())
}

fn record_player_frame(container: u32, phase: &str, dt: Option<f32>, supported: bool) {
    if !slr_vm::diagnostics::enabled() {
        return;
    }
    let mut fields = with(|state| {
        let mut fields = vec![
            ("container".into(), format!("{container:#010x}")),
            ("phase".into(), phase.into()),
            ("supported_empty_branch".into(), supported.to_string()),
        ];
        if let Some(player) = state.players.get(&container) {
            fields.extend([
                ("target_count".into(), player.targets.to_string()),
                ("focus".into(), format!("{:#010x}", player.focus)),
                ("render_refs".into(), format!("{:?}", player.render_refs)),
                (
                    "unmodelled_command".into(),
                    player.unmodelled_command.clone().unwrap_or_default(),
                ),
            ]);
        }
        fields
    });
    if let Some(dt) = dt {
        fields.push((
            "animation_dt_bits".into(),
            format!("{:#010x}", dt.to_bits()),
        ));
    }
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: "native-player-frame".into(),
        fields,
    });
}

#[cfg(test)]
fn native_player_control(container: u32) -> Result<(), String> {
    let result = with(|state| {
        let player = state
            .players
            .get(&container)
            .ok_or("NativeFrameBoundary: player state missing")?;
        if player.targets != 0 || player.unmodelled_command.is_some() {
            return Err("NativeFrameBoundary: nonempty player control target state".into());
        }
        //47a5f9..47a604: targetcount0 -> dec -> JS47a76b, without writes.
        Ok(())
    });
    record_player_frame(container, "control", None, result.is_ok());
    result
}

#[cfg(test)]
fn native_player_animate(container: u32, dt: f32) -> Result<(), String> {
    let result = with(|state| {
        //47a78c stores DAT0063c874 before reading the focus or render links.
        state.player_animation_dt = dt;
        let player = state
            .players
            .get(&container)
            .ok_or("NativeFrameBoundary: player state missing")?;
        if player.focus != 0
            || player.render_refs.iter().any(|word| *word != 0)
            || player.unmodelled_command.is_some()
        {
            return Err("NativeFrameBoundary: nonempty player animate state".into());
        }
        //47a8f7..47a91b only scans eight null refs; it does not execute
        //the active-focus branch's write to DAT006439c0 or dispatch scripts.
        state.trace.push(format!(
            "nativeplayer animate dt_bits={:#010x}",
            state.player_animation_dt.to_bits()
        ));
        Ok(())
    });
    record_player_frame(container, "animate", Some(dt), result.is_ok());
    result
}

/// Original00428960 frame end, after rendering and before Timer7: look up
/// Frontend.render and drain its actual object monitor (00408ce0). This is an
/// engine wake-up, so it does not manufacture a script-to-native call record.
pub fn notify_render_waiters(vm: &VmHost) -> Result<(), String> {
    let Some(render) = vm.read_loaded_static_field("java.render.Frontend", "render") else {
        return Ok(());
    };
    if render.payload == 0 {
        return Ok(());
    }
    if render
        .payload
        .checked_sub(0x1000)
        .and_then(|index| vm.objects.get(index as usize))
        .is_none()
    {
        return Err(format!(
            "RenderNotifyBoundary: Frontend.render {:#x} is not a live script object",
            render.payload
        ));
    }
    let woken = slr_lang::thread::with_threads(|host| host.notify_all(render.payload))
        .map_err(|error| format!("RenderNotifyBoundary: {error:?}"))?;
    slr_vm::diagnostics::emit(slr_vm::diagnostics::EventKind::External {
        category: "render-frame-notify".into(),
        fields: vec![
            ("monitor".into(), format!("{:#x}", render.payload)),
            ("woken_threads".into(), format!("{woken:?}")),
        ],
    });
    Ok(())
}

/// Original426c90 control pass over the live kind8 registrations.
fn control_pass(vm: &mut VmHost, dt: f32) -> Result<(), String> {
    crate::native_body::with_world_memory(|m| {
        crate::native_frame::control(
            m,
            crate::native_callbacks::CONTEXT,
            dt.to_bits(),
            &mut Drive { vm },
        )
    })??;
    Ok(())
}

/// exe: FUN_00426fd0 over the kind-0x1c list.
/// Is `container` on the kind-`0x1c` (`animate`) callback list?
pub fn registered_animate(container: u32) -> Result<bool, String> {
    let id = with_res(|h| h.container(container).map_or(0, |r| r.id));
    crate::native_body::with_world_memory(|m| {
        crate::native_callbacks::contains(m, crate::native_callbacks::CONTEXT, id, 0x1c)
    })
}

pub fn animate_pass(vm: &mut VmHost) -> Result<(), String> {
    let dt = with(|state| state.animation_dt);
    crate::native_body::with_world_memory(|m| {
        crate::native_frame::animate(
            m,
            crate::native_callbacks::CONTEXT,
            dt.to_bits(),
            &mut Drive { vm },
        )
    })??;
    Ok(())
}

/// Original426f60/426e10 traverses kind9 and selects script or native drive.
/// The car slot is454500, not the429d80 diagnostic used by some classes.
/// Resource access and script dispatch use the original426e10 graph. Native
/// class drive bodies remain explicit at their actual virtual entry points.
struct Drive<'a> {
    vm: &'a mut VmHost,
}
impl crate::native_spatial::IndexHost for Drive<'_> {
    fn access(
        &mut self,
        memory: &mut slr_physics::space::Space,
        resource: u32,
        flags: u32,
    ) -> Result<u32, String> {
        crate::native_assets::prepare(
            memory,
            resource,
            crate::native_resource_load::Access {
                flags,
                quality: 0,
                priority: 0,
                time: 0,
            },
        )
    }
}
impl crate::native_frame::Host for Drive<'_> {
    fn minimum(&mut self, m: &mut slr_physics::space::Space, resource: u32) -> Result<(), String> {
        crate::native_assets::minimum(m, resource)
    }
    fn prepare(
        &mut self,
        m: &mut slr_physics::space::Space,
        resource: u32,
        flags: u32,
    ) -> Result<u32, String> {
        crate::native_assets::prepare(
            m,
            resource,
            crate::native_resource_load::Access {
                flags,
                quality: 0,
                priority: 0,
                time: 0,
            },
        )
    }
    fn payload(&mut self, m: &mut slr_physics::space::Space, resource: u32) -> Result<u32, String> {
        crate::native_assets::data(m, resource)
    }
    fn native_drive(
        &mut self,
        m: &mut slr_physics::space::Space,
        class: u32,
        _body: u32,
        _dt: u32,
    ) -> Result<(), String> {
        Err(format!(
            "NativeFrameBoundary: original native drive virtual {:#010x} at class {class:#010x}",
            m.r32(m.r32(class) + 0x30)
        ))
    }
    fn script_drive(
        &mut self,
        m: &mut slr_physics::space::Space,
        envelope: u32,
    ) -> Result<(), String> {
        let script = m.r32(envelope + 0x50);
        // The drive wrapper site is not validated: nested natives stay rejected.
        let scope = crate::native_diagnostics::Scope {
            kind: "drive",
            envelope,
            script,
            method: "drive".into(),
            validated: false,
        };
        crate::native_body::during_vm_callback(m, || {
            crate::native_nested::within(None, || {
                crate::native_diagnostics::script_callback(scope, || {
                    call_script(self.vm, script, "drive", &[])
                })
            })
        })
    }
}
impl crate::native_frame::CallbackHost for Drive<'_> {
    fn native_callback(
        &mut self,
        m: &mut slr_physics::space::Space,
        class: u32,
        body: u32,
        dt: u32,
        kind: crate::native_frame::Callback,
    ) -> Result<(), String> {
        use crate::native_frame::Callback;
        match m.r32(class) {
            0x5f11e4 => crate::native_cursor_runtime::frame(
                m,
                self.vm,
                class,
                body,
                dt,
                kind == Callback::Animate,
            ),
            crate::native_player::VTABLE => {
                let container = crate::native_body::body_container(body)
                    .ok_or("NativePlayerBoundary: callback body has no live owner")?;
                if let Some(command) = with(|state| {
                    state
                        .players
                        .get(&container)
                        .and_then(|player| player.unmodelled_command.clone())
                }) {
                    return Err(format!(
                        "NativePlayerBoundary: pending raw command implementation {command}"
                    ));
                }
                let result = match kind {
                    Callback::Control => crate::native_player::control(m, class, body, dt),
                    Callback::Animate => crate::native_player::animate(m, class, body, dt),
                };
                if kind == Callback::Animate {
                    with(|state| state.player_animation_dt = f32::from_bits(m.r32(0x63c874)));
                }
                record_player_frame(
                    container,
                    if kind == Callback::Control {
                        "control"
                    } else {
                        "animate"
                    },
                    (kind == Callback::Animate).then(|| f32::from_bits(dt)),
                    result.is_ok(),
                );
                result
            }
            vtable => Err(format!(
                "NativeFrameBoundary: unported {kind:?} class vtable{vtable:#x}"
            )),
        }
    }
    fn script_callback(
        &mut self,
        m: &mut slr_physics::space::Space,
        envelope: u32,
        dt: u32,
        kind: crate::native_frame::Callback,
    ) -> Result<(), String> {
        let Some(script) = crate::native_install_runtime::Runtime::script_target(m, envelope)?
        else {
            return Ok(());
        };
        let args = if kind == crate::native_frame::Callback::Control {
            vec![Value::float(f32::from_bits(dt))]
        } else {
            vec![]
        };
        let method = if kind == crate::native_frame::Callback::Control {
            "control"
        } else {
            "animate"
        };
        let entered = crate::native_nested::validate(m, envelope);
        let scope = crate::native_diagnostics::Scope {
            kind: method,
            envelope,
            script,
            method: method.into(),
            validated: entered.is_some(),
        };
        crate::native_body::during_vm_callback(m, || {
            crate::native_nested::within(entered, || {
                crate::native_diagnostics::script_callback(scope, || {
                    call_script(self.vm, script, method, &args)
                })
            })
        })
    }
}

impl crate::native_dynamics::Host for Drive<'_> {
    fn drive(&mut self, memory: &mut slr_physics::space::Space, dt: u32) -> Result<(), String> {
        crate::native_callbacks::walk(
            memory,
            crate::native_callbacks::CONTEXT,
            9,
            |m, reference| crate::native_frame::drive(m, reference, dt, self),
        )?;
        Ok(())
    }
}

fn world_step(vm: &mut VmHost, dt: f32) -> Result<bool, String> {
    let mut drive = Drive { vm };
    let (stepped, now, accumulated) = crate::native_body::with_world_memory(|memory| {
        let stepped = crate::native_dynamics::step(memory, dt.to_bits(), &mut drive)?;
        let world = slr_physics::cfg::PHYSICS_WORLD;
        Ok::<_, String>((stepped, memory.rf(world), memory.rf(world + 4)))
    })??;
    with(|state| {
        state.sim_time = now;
        state.world_acc = accumulated;
    });
    slr_lang::seam::with_lang_host(|host| host.sim_time = now);
    Ok(stepped)
}

/// exe: FUN_00428450. One frame's control callbacks, world steps and timers.
pub fn step(vm: &mut VmHost) -> Result<(), String> {
    let config = with(|s| s.config);
    let mut remaining = config.frame_dt * config.time_scale;
    if remaining > 0.1 {
        remaining = 0.1;
    }
    let due = with(|s| {
        s.control_acc += remaining;
        if s.control_acc > STEP_THRESHOLD {
            let acc = s.control_acc;
            s.control_acc = 0.0;
            Some(acc)
        } else {
            None
        }
    });
    if let Some(acc) = due {
        control_pass(vm, acc)?;
    }
    let max_step = config.max_step * config.time_scale;
    let mut stepped = false;
    while remaining > 0.0 {
        let chunk = if remaining < max_step {
            remaining
        } else if remaining - max_step <= max_step {
            remaining * 0.5
        } else {
            max_step
        };
        if !world_step(vm, chunk)? {
            break;
        }
        stepped = true;
        remaining -= chunk;
        timer_pass(vm)?;
        crate::factory::cleanup(vm)?;
    }
    if !stepped {
        crate::factory::cleanup(vm)?;
    }
    drain_file_worker()?;
    Ok(())
}

/// `554ea0` runs on its own OS thread in the original. The port serializes it
/// at the frame boundary: one request quantum per call, which is where
/// `428450` gives the file manager its turn.
fn drain_file_worker() -> Result<(), String> {
    loop {
        let step = crate::native_body::with_world_memory(crate::native_file::pump)??;
        if step != crate::native_file::Step::Idle {
            continue;
        }
        return Ok(());
    }
}

pub fn install(vm: &mut VmHost) {
    for stub in slr_io::input::CONTROLLER_STUBS {
        vm.native_impls
            .insert(stub.binding.impl_addr, controller_bridge);
    }
    for (addr, f) in [
        (
            slr_resource::node::QUEUE_EVENT,
            queue_event as slr_vm::host::NativeFn,
        ),
        (0x0047_e000, rem_notification as slr_vm::host::NativeFn),
        (0x0047_e050, add_notification),
        (0x0047_e100, add_notification_extended),
        (0x0047_e1c0, add_timer),
        (0x0047_e260, remove_all_timers),
        (0x0048_19f0, set_event_mask),
        (0x0048_1a30, clear_event_mask),
        (0x0048_1c40, register_callback),
        (0x0048_1c80, unregister_callback),
        (0x0048_1cc0, unregister_callbacks),
    ] {
        vm.native_impls.insert(addr, f);
    }
}

/// exe: LAB_0047da30 -> FUN_00426800. Despite its name this dispatches immediately.
fn queue_event(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    // Preserve resource capture and native argument conversion before callbacks.
    slr_resource::node::bridge(vm, info);
    let w = match words(info) {
        Ok(w) => w,
        Err(e) => return fail(vm, info, e),
    };
    let target = container_of(w[0])?;
    let source = container_of(w[1]);
    if std::env::var_os("SLR_DISPATCH_TRACE").is_some() {
        eprintln!(
            "QUEUEEVENT target={target:#x} source={source:?} mask={:#x}",
            w[2]
        );
    }
    let arg = match queue_event_argument(w[2], string(vm, w[3])) {
        Ok(arg) => arg,
        Err(error) => return fail(vm, info, error),
    };
    if let Err(e) = dispatch(vm, target, source, w[2], &arg, None) {
        return fail(vm, info, e);
    }
    None
}

/// The47da30 wrapper converts its String argument to a C-string address.
///425c60 can instead interpret that address as an integer or VM object.
/// Null converts exactly to zero; a live native allocation has no equivalent
/// VM identity in this host and must not become an arbitrary callback word.
fn queue_event_argument(mask: u32, text: Option<String>) -> Result<EventArg, String> {
    if mask == 0x0010_0000 || mask & 0x0fff_ffff == 0x80 {
        if text.is_some() {
            return Err(
                "NativeVmBoundary: queueEvent native text address used as a non-text argument"
                    .into(),
            );
        }
        Ok(EventArg::Int(0))
    } else {
        Ok(EventArg::Str(text))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn native_player_empty_frame_branch_stores_only_animation_dt_and_reports_bits() {
        configure(FrameConfig::default());
        player_created(123);
        slr_vm::diagnostics::configure("player-frame-test".into(), 16, None);
        with(|state| state.player_animation_dt = f32::from_bits(0x12345678));
        native_player_control(123).unwrap();
        assert_eq!(
            with(|state| state.player_animation_dt.to_bits()),
            0x12345678
        );
        native_player_animate(123, f32::from_bits(0x80000000)).unwrap();
        assert_eq!(
            with(|state| state.player_animation_dt.to_bits()),
            0x80000000
        );
        let events = slr_vm::diagnostics::recent();
        assert_eq!(events.len(), 2);
        assert!(events.iter().any(|event|matches!(&event.kind,slr_vm::diagnostics::EventKind::External{category,fields} if category=="native-player-frame" && fields.contains(&("animation_dt_bits".into(),"0x80000000".into())))));
        with(|state| state.players.get_mut(&123).unwrap().focus = 1);
        assert!(native_player_animate(123, f32::from_bits(0x3dcccccd))
            .unwrap_err()
            .contains("nonempty"));
        //47a78c precedes the focus test even when a later branch is unavailable.
        assert_eq!(
            with(|state| state.player_animation_dt.to_bits()),
            0x3dcccccd
        );
        with(|state| {
            let player = state.players.get_mut(&123).unwrap();
            player.focus = 0;
            player.render_refs[7] = 1;
        });
        assert!(native_player_animate(123, 0.0).is_err());
        with(|state| state.players.get_mut(&123).unwrap().targets = 1);
        assert!(native_player_control(123).is_err());
        slr_vm::diagnostics::configure(String::new(), 0, None);
    }

    #[test]
    fn script_mask_changes_apply_to_native_player_subscription() {
        configure(FrameConfig::default());
        slr_resource::node::reset_res();
        let receiver = 7;
        let container = with_res(|h| {
            let c = h.add_container(slr_resource::node::Container::new(1, 123));
            let node = h.add_bound_node(123, c);
            h.bind_object(receiver, node);
            c
        });
        player_created(container);
        assert_eq!(with(|s| s.masks[&container]), 0x10);
        let mut vm = VmHost::new();
        let args = [Value::int(0x10), Value::reference(receiver)];
        let info = CallInfo {
            jvm: 0,
            thread: 0,
            class: "java.lang.GameType",
            name: "clearEventMask",
            sig: "(I)V",
            args: &args,
        };
        clear_event_mask(&mut vm, &info);
        assert!(vm.native_failure.is_none());
        assert_eq!(with(|s| s.masks[&container]), 0);
        set_event_mask(&mut vm, &info);
        assert_eq!(with(|s| s.masks[&container]), 0x10);
    }
}

#[cfg(test)]
mod render_notify_tests {
    use super::*;
    use slr_vm::{
        interp::{HeapSlot, Host},
        values::TypeTag,
        vmclass::{Class, Field},
    };

    #[test]
    fn frame_end_drains_actual_frontend_render_monitor_in_lifo_order() {
        slr_lang::thread::reset();
        let mut vm = VmHost::new();
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: "java.render.Frontend".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: vec![Field {
                class: "java.render.Frontend".into(),
                name: "render".into(),
                ty: "Ljava.render.GfxEngine;".into(),
                is_static: true,
            }],
        });
        vm.loader.insert_test_class(Class {
            kind: 0,
            name: "java.render.GfxEngine".into(),
            base: None,
            interfaces: vec![],
            methods: vec![],
            bodies: vec![],
            fields: vec![],
        });
        vm.alloc_object("java.render.Frontend").unwrap();
        let render = vm.alloc_object("java.render.GfxEngine").unwrap();
        let unrelated = vm.alloc_object("java.render.GfxEngine").unwrap();
        let (first, second, other) = slr_lang::thread::with_threads(|host| {
            let first = host.queues_mut().spawn(10, 0, "first");
            let second = host.queues_mut().spawn(10, 0, "second");
            let other = host.queues_mut().spawn(10, 0, "other");
            for (thread, monitor) in [(first, render), (second, render), (other, unrelated)] {
                host.set_current(Some(thread));
                host.wait(monitor).unwrap();
            }
            host.set_current(None);
            (first, second, other)
        });
        notify_render_waiters(&vm).unwrap(); // The actual static cell is still null.
        slr_lang::thread::with_threads(|host| assert!(host.queues().run_order().is_empty()));
        let context = vm.loader.context_word("java.render.Frontend").unwrap();
        vm.write_cell(
            HeapSlot::Static { context, slot: 0 },
            Value {
                tag: TypeTag::intern("Ljava.render.GfxEngine;"),
                payload: render,
            },
        )
        .unwrap();
        let before = vm.objects.len();
        notify_render_waiters(&vm).unwrap();
        slr_lang::thread::with_threads(|host| {
            assert_eq!(host.queues().run_order(), vec![second, first]);
            assert_eq!(host.queues().notify_order(), vec![other]);
            assert!(host.waiters(render).is_empty());
            assert_eq!(host.waiters(unrelated), vec![other]);
        });
        assert_eq!(vm.objects.len(), before);
        notify_render_waiters(&vm).unwrap(); // Empty drains are normal.
        slr_lang::thread::reset();
    }
}

#[cfg(test)]
pub(crate) mod event_argument_tests {
    use super::*;
    use slr_vm::vmclass::{Class, Field, Method};
    thread_local! {
        static CALLS: RefCell<Vec<(String, String, Vec<u32>)>> = const { RefCell::new(Vec::new()) };
    }
    fn observe(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
        let mut words = vec![0; info.args.len()];
        marshal_args(info, &mut words).unwrap();
        CALLS.with(|calls| {
            calls
                .borrow_mut()
                .push((info.name.into(), info.sig.into(), words))
        });
        None
    }
    pub(crate) fn take_calls() -> Vec<(String, String, Vec<u32>)> {
        CALLS.with(|calls| std::mem::take(&mut *calls.borrow_mut()))
    }
    pub(crate) fn setup(asynchronous: bool) -> (VmHost, u32, u32) {
        crate::factory::configure(None, None);
        slr_lang::thread::reset();
        take_calls();
        let mut vm = VmHost::new();
        for name in [
            "java.lang.Object",
            "java.lang.String",
            "java.util.resource.GameRef",
            "EventHarness",
        ] {
            let mut fields = vec![];
            if name == "java.util.resource.GameRef" || name == "java.lang.String" {
                fields.push(Field {
                    class: name.into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                });
            }
            if name == "EventHarness" {
                fields.push(Field {
                    class: name.into(),
                    name: "asyncMode".into(),
                    ty: "I".into(),
                    is_static: false,
                });
            }
            let mut methods = vec![];
            if name == "EventHarness" {
                for method in ["handleEvent", "handleEventAsync", "overrideHandler"] {
                    for sig in [
                        "(Ljava.lang.Object;)V",
                        "(Ljava.util.resource.GameRef;II)V",
                        "(Ljava.util.resource.GameRef;ILjava.lang.String;)V",
                    ] {
                        methods.push(Method {
                            class: name.into(),
                            name: method.into(),
                            sig: sig.into(),
                            flags: 0x41,
                            static_vector: false,
                            is_native: true,
                            body: 0,
                            is_abstract: false,
                            extra: 0,
                        });
                        vm.natives.register(name, method, sig, 0x300f0000);
                    }
                }
            }
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: name.into(),
                base: (name != "java.lang.Object").then(|| "java.lang.Object".into()),
                interfaces: vec![],
                methods,
                fields,
                bodies: vec![],
            });
        }
        vm.native_impls.insert(0x300f0000, observe);
        let script = vm.alloc_object("EventHarness").unwrap();
        vm.write_field(script, "asyncMode", Value::int(i32::from(asynchronous)))
            .unwrap();
        let class = vm.loader.lookup("EventHarness").unwrap();
        let created = crate::factory::instantiate(
            &mut vm,
            None,
            0,
            crate::factory::InstanceScript::Existing(script),
            ("harness", ""),
            Some("EventHarness"),
            Some(class),
        )
        .unwrap();
        with_res(|host| {
            let node = host.add_bound_node(created.id, created.container);
            host.bind_object(script, node);
        });
        (vm, script, created.container)
    }

    #[test]
    fn original_script_event_argument_arity_and_conversion_order() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/script-event-arguments-original.json");
        let capture: serde_json::Value = serde_json::from_slice(
            &std::fs::read(path).expect("run re/oracle/script_event_arguments_probe.py"),
        )
        .unwrap();
        assert_eq!(
            capture["exe_sha256"],
            "7d77466a8c1a34d16a59d9ccf2e8059664dff1bfb421338074a461a932e8edde"
        );
        use sha2::{Digest, Sha256};
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        for (field, relative) in [
            ("exe_sha256", "reference/raw/StreetLegal_Redline.bak.exe"),
            (
                "producer_sha256",
                "re/oracle/script_event_arguments_probe.py",
            ),
            ("oracle_sha256", "re/oracle/oracle.py"),
        ] {
            let bytes =
                std::fs::read(root.join(relative)).expect("required original oracle source/input");
            assert_eq!(
                capture[field],
                format!("{:x}", Sha256::digest(bytes)),
                "{relative} changed; regenerate fixture"
            );
        }
        assert_eq!(
            capture["configuration"],
            serde_json::json!({"entry": 0x425c60, "fpu_cw": 0x7f})
        );
        let rows = capture["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 112);
        let mut seen = std::collections::HashSet::new();
        for row in rows {
            let case = &row["case"];
            let mask = case["event"].as_u64().unwrap() as u32;
            let has_argument = case["argument"].as_bool().unwrap();
            assert!([0x100000, 0x10100000, 0x80, 0x10000080, 0x10, 0x10000010, 0].contains(&mask));
            assert!(
                seen.insert((
                    mask,
                    has_argument,
                    case["async"].as_bool().unwrap(),
                    case["source"].as_bool().unwrap(),
                    case["name"].as_bool().unwrap()
                )),
                "duplicate original scenario"
            );
            let (mut vm, script, container) = setup(case["async"].as_bool().unwrap());
            let original = row["calls"].as_array().unwrap();
            let callback = original.last().unwrap();
            assert_eq!(callback["kind"], "callback");
            assert_eq!(callback["request_return"], 0);
            assert_eq!(callback["terminator"], 0x4f);
            let object = if mask == 0x100000 && has_argument {
                vm.alloc_object("java.lang.Object").unwrap()
            } else {
                0
            };
            let arg = if mask == 0x100000 {
                EventArg::Int(object as i32)
            } else if mask & 0x0fff_ffff == 0x80 {
                EventArg::Int(row["argument_word"].as_u64().unwrap() as i32)
            } else {
                EventArg::Str(has_argument.then(|| "event payload".into()))
            };
            let before = vm.objects.len();
            deliver_script_to(
                &mut vm,
                script,
                container,
                case["source"].as_bool().unwrap().then_some(container),
                mask,
                &arg,
                case["name"].as_bool().unwrap().then_some("overrideHandler"),
            )
            .unwrap();
            let calls = take_calls();
            assert_eq!(calls.len(), 1, "{case}");
            assert_eq!(calls[0].0, callback["method"].as_str().unwrap(), "{case}");
            let arguments = callback["arguments"].as_array().unwrap();
            assert_eq!(calls[0].2.len(), arguments.len() + 1, "{case}"); // receiver is first after marshalling
            assert_eq!(calls[0].2[0], script);
            if mask == 0x100000 {
                assert_eq!(
                    original.len(),
                    1,
                    "special event must bypass all conversion collaborators"
                );
                assert_eq!(arguments.len(), 1);
                assert_eq!(arguments[0][0], 4);
                assert_eq!(calls[0].1, "(Ljava.lang.Object;)V");
                assert_eq!(calls[0].2[1], object);
                assert_eq!(
                    vm.objects.len(),
                    before,
                    "special event must not allocate a string or GameRef"
                );
            } else {
                assert_eq!(arguments.len(), 3);
                assert_eq!(arguments[0], serde_json::json!([4, "GameRef"]));
                assert_eq!(arguments[1], serde_json::json!([2, mask & 0x0fff_ffff]));
                assert_eq!(calls[0].2[2], mask & 0x0fff_ffff);
                let game_ref = calls[0].2[1];
                let node = vm.read_field(game_ref, "ptr").unwrap().payload;
                with_res(|host| assert_eq!(host.node(node).unwrap().container, container));
                if mask & 0x0fff_ffff == 0x80 {
                    assert_eq!(arguments[2][0], 2);
                    assert_eq!(calls[0].1, "(Ljava.util.resource.GameRef;II)V");
                    assert_eq!(calls[0].2[3] as u64, arguments[2][1].as_u64().unwrap());
                    assert_eq!(game_ref, before as u32 + 0x1000);
                } else {
                    assert_eq!(arguments[2][0], 4);
                    assert_eq!(
                        calls[0].1,
                        "(Ljava.util.resource.GameRef;ILjava.lang.String;)V"
                    );
                    let string = calls[0].2[3];
                    if has_argument {
                        assert_eq!(original[0]["kind"], "string");
                        assert_eq!(vm.string_bytes(string), Some(b"event payload".as_slice()));
                        assert_eq!(string, before as u32 + 0x1000);
                        assert_eq!(
                            game_ref,
                            string + 1,
                            "original converts string before constructing GameRef"
                        );
                    } else {
                        assert_eq!(string, 0);
                        assert_eq!(game_ref, before as u32 + 0x1000);
                    }
                }
            }
        }
    }

    #[test]
    fn event_kind_controls_overload_and_rejects_unconverted_addresses_before_allocating() {
        for mask in [0x80, 0x1000_0080, 0x10, 0x1010_0000, 0] {
            let (mut vm, script, container) = setup(true);
            let before = vm.objects.len();
            let wrong = if mask & 0x0fff_ffff == 0x80 {
                EventArg::Str(Some("native text address".into()))
            } else {
                EventArg::Int(1234)
            };
            assert!(
                deliver_script_to(&mut vm, script, container, None, mask, &wrong, None)
                    .unwrap_err()
                    .contains("NativeVmBoundary")
            );
            assert_eq!(vm.objects.len(), before);
            assert!(take_calls().is_empty());
            let null = if mask & 0x0fff_ffff == 0x80 {
                EventArg::Str(None)
            } else {
                EventArg::Int(0)
            };
            deliver_script_to(&mut vm, script, container, None, mask, &null, None).unwrap();
            let calls = take_calls();
            assert_eq!(calls.len(), 1);
            assert_eq!(calls[0].2[3], 0);
            assert_eq!(
                calls[0].1,
                if mask & 0x0fff_ffff == 0x80 {
                    "(Ljava.util.resource.GameRef;II)V"
                } else {
                    "(Ljava.util.resource.GameRef;ILjava.lang.String;)V"
                }
            );
        }
        for mask in [0x80, 0x1000_0080, 0x0010_0000] {
            assert!(queue_event_argument(mask, Some("text".into()))
                .unwrap_err()
                .contains("NativeVmBoundary"));
            assert!(matches!(
                queue_event_argument(mask, None).unwrap(),
                EventArg::Int(0)
            ));
        }
        assert!(matches!(
            queue_event_argument(0x1010_0000, Some("text".into())).unwrap(),
            EventArg::Str(Some(_))
        ));
    }

    #[test]
    fn object_event_keeps_unmodelled_identity_and_wrong_argument_shape_explicit() {
        let (mut vm, script, container) = setup(true);
        deliver_script_to(
            &mut vm,
            script,
            container,
            None,
            0x100000,
            &EventArg::Str(None),
            Some("overrideHandler"),
        )
        .unwrap();
        assert_eq!(
            take_calls(),
            vec![(
                "handleEvent".into(),
                "(Ljava.lang.Object;)V".into(),
                vec![script, 0]
            )]
        );
        for arg in [
            EventArg::Int(0x300f0000),
            EventArg::Str(Some("not an object".into())),
        ] {
            let before = vm.objects.len();
            assert!(
                deliver_script_to(&mut vm, script, container, None, 0x100000, &arg, None)
                    .unwrap_err()
                    .contains("NativeVmBoundary")
            );
            assert_eq!(vm.objects.len(), before);
            assert!(take_calls().is_empty());
        }
    }

    #[test]
    fn notification_forward_preserves_full_mask_argument_interpretation() {
        use native_event_route::NotificationHost;
        for event in [0x80, 0x1000_0080, 0x0010_0000, 0x1010_0000] {
            let (mut vm, script, container) = setup(true);
            let object = vm.alloc_object("java.lang.Object").unwrap();
            with(|state| {
                state.masks.insert(container, u32::MAX);
            });
            let (reference, argument, name) = crate::native_body::with_world_memory(|m| {
                let raw = crate::native_body::raw_resource(m, container).unwrap();
                let reference = crate::native_install::local_reference(m);
                crate::native_part::bind_reference(m, reference, raw);
                let argument = if event == 0x0010_0000 {
                    object
                } else if event & 0x0fff_ffff == 0x80 {
                    0xf1234567
                } else {
                    store_event_text(m, Some("event payload"))
                };
                let name = store_event_text(m, Some("overrideHandler"));
                (reference, argument, name)
            })
            .unwrap();
            let before = vm.objects.len();
            NotificationRuntime(&mut vm)
                .forward(reference, container, event, argument, name)
                .unwrap();
            let calls = take_calls();
            assert_eq!(calls.len(), 1);
            if event == 0x0010_0000 {
                assert_eq!(
                    calls[0],
                    (
                        "handleEvent".into(),
                        "(Ljava.lang.Object;)V".into(),
                        vec![script, object]
                    )
                );
                assert_eq!(vm.objects.len(), before);
            } else if event & 0x0fff_ffff == 0x80 {
                assert_eq!(calls[0].1, "(Ljava.util.resource.GameRef;II)V");
                assert_eq!(calls[0].2[3], argument);
            } else {
                assert_eq!(calls[0].0, "overrideHandler");
                assert_eq!(
                    calls[0].1,
                    "(Ljava.util.resource.GameRef;ILjava.lang.String;)V"
                );
                assert_eq!(
                    vm.string_bytes(calls[0].2[3]),
                    Some(b"event payload".as_slice())
                );
            }
            crate::native_body::with_world_memory(|m| {
                crate::native_part::bind_reference(m, reference, 0)
            })
            .unwrap();
        }
    }

    #[test]
    fn physics_event_structures_remain_explicit_before_vm_conversion() {
        let (mut vm, script, container) = setup(false);
        let before = vm.objects.len();
        for mask in [
            1,
            4,
            0x20,
            0x40,
            0x1000_0001,
            0x1000_0004,
            0x1000_0020,
            0x1000_0040,
        ] {
            assert!(deliver_script_to(
                &mut vm,
                script,
                container,
                None,
                mask,
                &EventArg::Str(Some("structure is not text".into())),
                None
            )
            .unwrap_err()
            .contains("physics argument structure"));
            assert_eq!(vm.objects.len(), before);
            assert!(take_calls().is_empty());
        }
    }
}

#[cfg(test)]
mod native_frame_boundary_tests {
    use super::*;
    use crate::native_dynamics::Host as _;
    #[test]
    fn registered_script_drive_cannot_be_silently_omitted() {
        with(|state| *state = State::default());
        crate::factory::configure(None, None);
        let container = crate::factory::create_resource(crate::factory::ResourceDescriptor {
            kind: 1,
            flags: 0x110,
            parent: None,
            template: None,
            name: "drive regression",
        })
        .unwrap();
        let mut vm = VmHost::new();
        let mut driver = Drive { vm: &mut vm };
        crate::native_body::with_world_memory(|m| {
            crate::native_callbacks::initialize(m, crate::native_callbacks::CONTEXT);
            assert!(driver.drive(m, 0).is_ok());
            let raw = crate::native_body::raw_resource(m, container).unwrap();
            m.w32(raw + 0x64, 1);
            m.w32(raw + 0xd8 + 0x50, 0x1000);
            let id = m.r32(raw + 0x50);
            let flags = m.r32(raw + 0x54);
            crate::native_callbacks::register(m, crate::native_callbacks::CONTEXT, raw, 9);
            assert!(driver
                .drive(m, 0)
                .unwrap_err()
                .contains("script instance has no loaded class"));
            assert_eq!(
                m.r32(raw + 0x54),
                flags,
                "failed VM callback restores flag scope"
            );
            crate::native_callbacks::unregister(m, crate::native_callbacks::CONTEXT, id, 9);
            assert!(driver.drive(m, 0).is_ok());
        })
        .unwrap();
    }
}

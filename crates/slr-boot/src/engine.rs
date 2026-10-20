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
struct Notification {
    listener: u32,
    mask: u32,
    kind: u32,
    arg: Option<String>,
    name: Option<String>,
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

#[derive(Default)]
struct State {
    masks: HashMap<u32, u32>,
    notifications: HashMap<u32, Vec<Notification>>,
    timers: Vec<Timer>,
    /// Registered containers by callback kind (8, 9, 0x1c), in order.
    callbacks: HashMap<u32, Vec<u32>>,
    sim_time: f32,
    world_acc: f32,
    control_acc: f32,
    config: FrameConfig,
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
    with(|s| {
        *s = State {
            config,
            ..State::default()
        };
    });
    slr_lang::seam::with_lang_host(|h| h.sim_time = 0.0);
}

pub fn take_trace() -> Vec<String> {
    with(|s| std::mem::take(&mut s.trace))
}

/// The native player starts with EVENT_COMMAND subscribed. Observed at
/// FUN_004265c0's loaded-data branch in the headless original (mask +0x70).
pub fn player_created(container: u32) {
    with(|s| *s.masks.entry(container).or_insert(0) |= 0x10);
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
        Ok((c, mask)) => with(|s| *s.masks.entry(c).or_insert(0) |= mask),
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: FUN_00481a30 -> FUN_0048d420
pub fn clear_event_mask(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, mask)) => with(|s| *s.masks.entry(c).or_insert(0) &= !mask),
        Err(e) => return fail(vm, info, e),
    }
    None
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
    with(|s| {
        let list = s.notifications.entry(source).or_default();
        // FUN_0048b500 first drops an existing record for the same
        // (listener, mask) pair and appends the new one at the tail.
        list.retain(|n| !(n.listener == listener && n.mask == mask));
        list.push(Notification {
            listener,
            mask,
            kind,
            arg,
            name,
        });
    });
    None
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
    with(|s| {
        if let Some(list) = s.notifications.get_mut(&source) {
            if let Some(i) = list
                .iter()
                .position(|n| n.listener == listener && n.mask == mask)
            {
                list.remove(i);
            }
        }
    });
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
            with(|s| s.callbacks.entry(kind).or_default().push(c));
        }
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: FUN_00481c80 -> FUN_004274e0 (the first matching record goes dead)
pub fn unregister_callback(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let r = words(info).and_then(|w| Ok((receiver_container(info, &w)?, w[1])));
    match r {
        Ok((c, kind)) => with(|s| {
            if let Some(list) = s.callbacks.get_mut(&kind) {
                if let Some(i) = list.iter().position(|x| *x == c) {
                    list.remove(i);
                }
            }
        }),
        Err(e) => return fail(vm, info, e),
    }
    None
}

/// exe: LAB_00481cc0, which forwards to a bare `ret` in build 606.
pub fn unregister_callbacks(_: &mut VmHost, _: &CallInfo<'_>) -> Option<u32> {
    None
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
fn call_script(
    vm: &mut VmHost,
    script: u32,
    method: &str,
    args: &[Value],
    label: &str,
) -> Result<(), String> {
    let class = object_class(vm, script).ok_or("script instance has no loaded class")?;
    sched::run_created(vm, label, class, script, method, args).map_err(|(thread, error)| {
        let detail = crate::script_stop(
            vm,
            &thread,
            &format!("script call {method} on {script:#x}"),
            error,
        );
        match slr_vm::host::last_host_error() {
            Some(host) => format!("{detail}; last host error: {host}"),
            None => detail,
        }
    })
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
    let kind = mask & 0x0fff_ffff;
    let is_type1 = with_res(|h| h.container(container).is_some_and(|c| c.type_id == 1));
    if !is_type1 {
        return Ok(());
    }
    let instance_mask = with(|s| s.masks.get(&container).copied().unwrap_or(0));
    let script = script_of(container);
    // FUN_004265c0 first offers the event to the native object's vtable +0x38.
    // FUN_0047a920 consumes Controller commands and invokes setcontrol directly.
    let native_consumed = instance_mask & mask != 0
        && kind == 0x10
        && crate::factory::native_class(container).as_deref() == Some("player");
    if native_consumed {
        if let EventArg::Str(Some(text)) = arg {
            match text.split_whitespace().next() {
                Some("setcontrol") if script != 0 => {
                    call_script(vm, script, "setcontrol", &[], "EVENT")?;
                }
                Some("device") => {
                    // The stock Controller has no addDeviceMapping body. Its native
                    // mapping is still a declared headless-input gap.
                }
                _ => {}
            }
        }
    }
    if instance_mask & mask != 0 && script != 0 && !native_consumed {
        if matches!(kind, 1 | 4 | 0x20 | 0x40) {
            return Err(format!(
                "event type {kind:#x} carries a physics argument structure that is not modelled"
            ));
        }
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
        match arg {
            EventArg::Int(v) => args.push(Value::int(*v)),
            EventArg::Str(text) => {
                let word = match text {
                    Some(text) => vm
                        .intern_string(Some(text.as_bytes()))
                        .map_err(|e| format!("event string: {e:?}"))?,
                    None => 0,
                };
                args.push(Value::reference_named(word, "Ljava.lang.String;"));
            }
        }
        with(|s| {
            s.trace.push(format!(
                "dispatch container={container:#x} type={kind:#x} method={method} arg={arg:?}"
            ));
        });
        call_script(vm, script, method, &args, "EVENT")?;
    }
    let notifications = with(|s| s.notifications.get(&container).cloned().unwrap_or_default());
    let mut one_shot = Vec::new();
    for n in notifications {
        if mask & n.mask == 0 {
            continue;
        }
        let forwarded = if n.kind & 0x0fff_ffff != 0 {
            n.kind
        } else {
            mask
        };
        let forwarded_arg = if n.kind & 0x1000_0000 != 0 {
            EventArg::Str(n.arg.clone())
        } else {
            arg.clone()
        };
        if source.is_some() && n.mask & 0x4000_0000 != 0 {
            // Forwarded chains skip records flagged `0x40000000`.
        } else {
            dispatch(
                vm,
                n.listener,
                Some(source.unwrap_or(container)),
                forwarded & 0x0fff_ffff,
                &forwarded_arg,
                n.name.as_deref(),
            )?;
        }
        if n.mask & 0x8000_0000 != 0 {
            one_shot.push((n.listener, n.mask));
        }
    }
    if !one_shot.is_empty() {
        with(|s| {
            if let Some(list) = s.notifications.get_mut(&container) {
                list.retain(|n| !one_shot.contains(&(n.listener, n.mask)));
            }
        });
    }
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

fn with_hook_flags<R>(container: u32, f: impl FnOnce() -> R) -> R {
    // FUN_0048d1f0/FUN_0048d210: bit 0x100 of the container's flags is cleared
    // for the duration of the callback and the old bits restored afterwards.
    let old = with_res(|h| {
        h.container_mut(container).map(|c| {
            let old = c.flags;
            c.flags &= !0x100;
            old
        })
    });
    let result = f();
    if let Some(old) = old {
        with_res(|h| {
            if let Some(c) = h.container_mut(container) {
                c.flags |= old;
            }
        });
    }
    result
}

/// exe: FUN_004291e0 over the kind-8 list.
fn control_pass(vm: &mut VmHost, dt: f32) -> Result<(), String> {
    let list = with(|s| s.callbacks.get(&8).cloned().unwrap_or_default());
    for container in list {
        let script = script_of(container);
        if script == 0 {
            continue;
        }
        with(|s| {
            s.trace
                .push(format!("control container={container:#x} dt={dt}"))
        });
        with_hook_flags(container, || {
            call_script(vm, script, "control", &[Value::float(dt)], "CONTROL")
        })?;
    }
    Ok(())
}

/// exe: FUN_00426fd0 over the kind-0x1c list.
pub fn animate_pass(vm: &mut VmHost) -> Result<(), String> {
    let list = with(|s| s.callbacks.get(&0x1c).cloned().unwrap_or_default());
    for container in list {
        let script = script_of(container);
        if script == 0 {
            continue;
        }
        with(|s| s.trace.push(format!("animate container={container:#x}")));
        with_hook_flags(container, || {
            call_script(vm, script, "animate", &[], "ANIMATE")
        })?;
    }
    Ok(())
}

/// exe: FUN_004a5190 for a world with no rigid bodies: the accumulated step is
/// consumed and simulated time advances by it.
fn world_step(dt: f32) -> bool {
    with(|s| {
        let accumulated = dt + s.world_acc;
        s.world_acc = accumulated;
        if accumulated < STEP_THRESHOLD {
            return false;
        }
        s.world_acc -= accumulated;
        s.sim_time += accumulated;
        let now = s.sim_time;
        slr_lang::seam::with_lang_host(|h| h.sim_time = now);
        true
    })
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
    while remaining > 0.0 {
        let chunk = if remaining < max_step {
            remaining
        } else if remaining - max_step <= max_step {
            remaining * 0.5
        } else {
            max_step
        };
        if !world_step(chunk) {
            break;
        }
        remaining -= chunk;
        timer_pass(vm)?;
    }
    Ok(())
}

pub fn install(vm: &mut VmHost) {
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
    let arg = EventArg::Str(string(vm, w[3]));
    if let Err(e) = dispatch(vm, target, source, w[2], &arg, None) {
        return fail(vm, info, e);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

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

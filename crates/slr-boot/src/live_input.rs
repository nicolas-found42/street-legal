//! Platform input projection into stock Hotkey and Group cursor callbacks.
//! Menu navigation/commands and modal decisions stay in unchanged TUFA scripts.
use crate::presentation::{container, field};
use serde_json::json;
use slr_d3d9::{InputEvent, PlatformInput};
use slr_telemetry::journal;
use slr_vm::{
    host::{marshal_args, CallInfo, NativeFn, VmHost},
    values::Value,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet, VecDeque},
    path::Path,
    sync::mpsc::Receiver,
};
#[derive(Clone, Debug)]
struct Hotkey {
    word: u32,
    target: u32,
    osd: u32,
    key: u32,
    flags: u32,
    filter: u32,
}
#[derive(serde::Deserialize)]
struct Scheduled {
    #[serde(default)]
    capture: bool,
    tick: u64,
    event: InputEvent,
}
#[derive(Default)]
struct State {
    original: HashMap<u32, NativeFn>,
    hotkeys: Vec<Option<Hotkey>>,
    input: Option<Receiver<PlatformInput>>,
    schedule: Vec<Scheduled>,
    ready: bool,
    closed: bool,
    tick: u64,
    last_event: Option<serde_json::Value>,
    event_count: u64,
    input_id: u64,
    held: HashSet<u32>,
    last_keys: VecDeque<u32>,
    want_capture: bool,
    recorded: Option<u64>,
    history: Vec<serde_json::Value>,
    pointer: [f32; 2],
    hover: Option<crate::presentation::Hit>,
    down: Option<crate::presentation::Hit>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
pub fn connect(input: Receiver<PlatformInput>) {
    STATE.with(|s| s.borrow_mut().input = Some(input));
}
pub fn script(path: String) -> Result<(), String> {
    let events = serde_json::from_slice(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("input script: {e}"))?;
    STATE.with(|s| s.borrow_mut().schedule = events);
    Ok(())
}
pub fn window_open() -> bool {
    STATE.with(|s| {
        let s = s.borrow();
        s.input.is_some() && !s.closed
    })
}
pub fn closed() -> bool {
    STATE.with(|s| s.borrow().closed)
}
pub fn ready() {
    if !crate::presentation::enabled() {
        return;
    }
    STATE.with(|s| s.borrow_mut().ready = true);
}
fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let addr = vm.natives.resolve(info.class, info.name, info.sig)?;
    let original = STATE.with(|s| s.borrow().original[&addr]);
    if info.class == "java.io.MouseCursor" && interacted() {
        let point = STATE.with(|s| s.borrow().pointer);
        if matches!(info.name, "getPos" | "getPickedPos") {
            let pos = if info.name == "getPickedPos" {
                crate::presentation::pick_position(vm, focused_osd(vm), point)?
            } else {
                [point[0] * 2. - 1., point[1] * 2. - 1., 0.]
            };
            let word = crate::presentation::vector_object(vm, pos)?;
            slr_calllog::headless::record(slr_calllog::LogRecord::NativeCall {
                class: info.class.into(),
                name: info.name.into(),
                args: vec![],
                result: Some(slr_calllog::Value::Int(word as i32)),
            });
            return Some(word);
        }
    }
    if info.name == "lastKey" {
        if let Some(key) = STATE.with(|s| s.borrow_mut().last_keys.pop_front()) {
            slr_calllog::headless::record(slr_calllog::LogRecord::NativeCall {
                class: info.class.into(),
                name: info.name.into(),
                args: vec![],
                result: Some(slr_calllog::Value::Int(key as i32)),
            });
            return Some(key);
        }
    }
    let result = original(vm, info);
    let mut w = [0; 8];
    let _ = marshal_args(info, &mut w);
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        match info.name {
            "createHotkey" => {
                let slot = s.hotkeys.iter().position(|h| h.is_none());
                let hotkey = Some(Hotkey {
                    word: w[3],
                    target: w[4],
                    osd: w[5],
                    key: w[1],
                    flags: w[2],
                    filter: w[6],
                });
                if let Some(slot) = slot {
                    s.hotkeys[slot] = hotkey;
                } else {
                    s.hotkeys.push(hotkey);
                }
            }
            "deleteHotkey" => {
                for slot in &mut s.hotkeys {
                    if slot.as_ref().is_some_and(|h| h.word == w[1]) {
                        *slot = None;
                    }
                }
            }
            _ => {}
        }
    });
    result
}
pub fn install(vm: &mut VmHost) {
    if !crate::presentation::enabled() {
        return;
    }
    for addr in [
        slr_io::input::INPUT_CREATE_HOTKEY,
        slr_io::input::INPUT_DELETE_HOTKEY,
        slr_io::input::INPUT_LAST_KEY,
        slr_io::input::CURSOR_GET_POS,
        slr_io::input::CURSOR_GET_PICKED_POS,
    ] {
        if let Some(original) = vm.native_impls.insert(addr, bridge) {
            STATE.with(|s| s.borrow_mut().original.insert(addr, original));
        }
    }
}
fn constant(vm: &VmHost, class: &str, name: &str) -> Option<u32> {
    let mut current = class;
    loop {
        if let Some(value) = vm.read_loaded_static_field(current, name) {
            return Some(value.payload);
        }
        current = vm.loader.find(current)?.base.as_deref()?;
    }
}
fn keyboard(vm: &mut VmHost, code: u32, pressed: bool, repeat: bool) -> Result<(), String> {
    let deliver = STATE.with(|state| {
        let mut s = state.borrow_mut();
        if pressed {
            let fresh = s.held.insert(code);
            fresh || (repeat && matches!(code, 0x25..=0x28 | 8))
        } else {
            s.held.remove(&code)
        }
    });
    if !deliver {
        route(
            "ignored-key",
            json!({"reason":"duplicate-press-or-orphan-release","code":code}),
        )?;
        return Ok(());
    }
    if pressed && matches!(code, 8 | 13 | 27) {
        STATE.with(|s| s.borrow_mut().last_keys.push_back((code << 16) | code));
    }
    let axis = match code {
        0x25 => Some("AXIS_MENU_LEFT"),
        0x26 => Some("AXIS_MENU_UP"),
        0x27 => Some("AXIS_MENU_RIGHT"),
        0x28 => Some("AXIS_MENU_DOWN"),
        0xd | 0x20 => Some("AXIS_SELECT"),
        0x1b => Some("AXIS_CANCEL"),
        _ => None,
    };
    let physical_name = match code {
        0x25 => Some("RCDIK_LEFT"),
        0x26 => Some("RCDIK_UP"),
        0x27 => Some("RCDIK_RIGHT"),
        0x28 => Some("RCDIK_DOWN"),
        13 => Some("RCDIK_ENTER"),
        27 => Some("RCDIK_ESCAPE"),
        32 => Some("RCDIK_SPACE"),
        8 => Some("RCDIK_BACK"),
        9 => Some("RCDIK_TAB"),
        _ => None,
    };
    let physical_key = physical_name.and_then(|name| constant(vm, "java.io.Input", name));
    let virtual_axis = axis.and_then(|name| constant(vm, "java.io.Input", name));
    let virtual_flag = constant(vm, "java.io.Input", "VIRTUAL").unwrap_or(0);
    let filter = constant(
        vm,
        "java.util.Event",
        if pressed {
            "F_KEY_PRESS"
        } else {
            "F_KEY_RELEASE"
        },
    )
    .unwrap_or(0);
    let focus = focused_osd(vm);
    let hotkeys = STATE.with(|s| s.borrow().hotkeys.clone());
    for h in hotkeys.into_iter().flatten() {
        let owner = if h.osd != 0 {
            h.osd
        } else {
            field(vm, h.target, "osd").unwrap_or(0)
        };
        if owner != focus || focus == 0 {
            continue;
        }
        let matched = if h.flags & virtual_flag != 0 {
            Some(h.key) == virtual_axis
        } else {
            Some(h.key) == physical_key
        };
        if !matched || h.filter & filter == 0 {
            continue;
        }
        route(
            "hotkey",
            json!({"hotkey":h.word,"target":h.target,"osd":focus,"key":h.key,"flags":h.flags,"filter":h.filter,"pressed":pressed}),
        )?;
        vm.write_field(h.word, "state", Value::int(i32::from(pressed)))
            .map_err(|e| format!("Hotkey.state: {e:?}"))?;
        crate::engine::call_script(
            vm,
            h.target,
            "handleEvent",
            &[Value::reference_named(h.word, "Ljava.io.Hotkey;")],
        )?;
        // The original checkHotkeys dispatches the first changed matching slot.
        return Ok(());
    }
    route(
        "unmatched-key",
        json!({"focus":focus,"physical":physical_key,"axis":virtual_axis,"filter":filter}),
    )?;
    Ok(())
}
pub fn input_id() -> u64 {
    STATE.with(|s| s.borrow().input_id)
}
fn route(kind: &str, data: serde_json::Value) -> Result<(), String> {
    journal::emit(
        "input-route",
        json!({"input_id":input_id(),"route":kind,"detail":data}),
    )?;
    Ok(())
}
pub fn poll(vm: &mut VmHost) -> Result<(), String> {
    let (ready, tick, mut events) = STATE.with(|state| {
        let mut s = state.borrow_mut();
        let tick = s.tick;
        let ready = s.ready;
        let mut events: Vec<_> = s
            .input
            .as_ref()
            .map(|input| input.try_iter().collect())
            .unwrap_or_default();
        if ready {
            s.tick += 1;
            let mut remaining = Vec::new();
            for scheduled in std::mem::take(&mut s.schedule) {
                if scheduled.tick == tick {
                    s.want_capture |= scheduled.capture;
                    events.push(PlatformInput {
                        source_id: 0,
                        event: scheduled.event,
                    });
                } else {
                    remaining.push(scheduled);
                }
            }
            s.schedule = remaining;
        }
        (ready, tick, events)
    });
    for received in events.drain(..) {
        let event = received.event;
        if !ready {
            journal::emit(
                "input-ignored",
                json!({"source_id":received.source_id,"event":event,"reason":"menu-not-ready"}),
            )?;
            if matches!(event, InputEvent::Close) {
                STATE.with(|s| s.borrow_mut().closed = true);
            }
            continue;
        }
        // Written and synchronized before any callback can fail or terminate.
        let id = journal::emit(
            "input-apply",
            json!({"tick":tick,"source_id":received.source_id,"event":event,"focus":focused_osd(vm)}),
        )?;
        STATE.with(|state| {
            let mut s = state.borrow_mut();
            s.event_count += 1;
            s.input_id = id;
            s.last_event = Some(json!({"tick":tick+1,"event":event,"input_id":id}));
        });
        let sequence =
            crate::telemetry::detail("input-begin", vec![("input_id".into(), id.to_string())]);
        let result = slr_vm::diagnostics::with_context(
            slr_vm::diagnostics::Context {
                parent_sequence: sequence,
                ..Default::default()
            },
            || match &event {
                InputEvent::Key {
                    code,
                    pressed,
                    repeat,
                } => keyboard(vm, *code, *pressed, *repeat),
                InputEvent::Close => {
                    STATE.with(|s| s.borrow_mut().closed = true);
                    Ok(())
                }
                InputEvent::Pointer { x, y } => pointer(vm, *x, *y),
                InputEvent::Button { pressed } => button(vm, *pressed),
                InputEvent::FocusLost => cancel(vm),
                InputEvent::Text { text } => {
                    let active = field(vm, focused_osd(vm), "actGadget").unwrap_or(0);
                    route(
                        "text",
                        json!({"active_gadget":active,"accepted":active!=0,"characters":text.chars().count()}),
                    )?;
                    if active != 0 {
                        STATE.with(|s| {
                            s.borrow_mut().last_keys.extend(
                                text.chars()
                                    .filter(|c| (*c as u32) >= 32 && (*c as u32) <= 255)
                                    .flat_map(|c| {
                                        let code = c as u32;
                                        [(code << 16) | code, 0]
                                    }),
                            )
                        });
                    }
                    Ok(())
                }
            },
        );
        journal::emit(
            "input-result",
            json!({"input_id":id,"vm_sequence":sequence,"error":result.as_ref().err(),"focus_after":focused_osd(vm)}),
        )?;
        result?;
    }
    Ok(())
}
pub fn record(vm: &VmHost, output: &Path) -> Result<(), String> {
    if !STATE.with(|s| {
        let s = s.borrow();
        s.ready
            && (s.recorded != Some(s.event_count)
                || slr_vm::diagnostics::current_context()
                    .frame
                    .is_some_and(|f| f % 60 == 0))
    }) {
        return Ok(());
    }
    let focus = focused_osd(vm);
    let gadget = field(vm, focus, "pointedGadget").unwrap_or(0);
    let labels = crate::presentation::labels(vm)?;
    let capture = STATE.with(|s| {
        let mut s = s.borrow_mut();
        let capture = s.want_capture;
        s.want_capture = false;
        capture.then_some(s.tick)
    });
    if let Some(tick) = capture {
        crate::presentation::capture(vm, &output.join(format!("input-{tick}.png")))?;
    }
    let state_word = vm
        .read_loaded_static_field("java.game.GameLogic", "actualState")
        .map(|v| v.payload)
        .or_else(|| {
            vm.objects
                .iter()
                .position(|o| o.class_name(&vm.loader) == "java.game.GameLogic")
                .and_then(|i| field(vm, 0x1000 + i as u32, "actualState"))
        });
    let game_state = state_word
        .and_then(|word| vm.objects.get(word.checked_sub(0x1000)? as usize))
        .map(|o| o.class_name(&vm.loader).to_owned());
    let pending = game_state
        .as_ref()
        .is_some_and(|class| crate::sched::pending_state_call(vm, class));
    let snapshot = serde_json::json!({"game_state":game_state,"pending_state_call":pending,"focus":focus,"selected":field(vm,gadget,"cmd"),"pressed":field(vm,gadget,"pressed"),"act":field(vm,focus,"actGadget"),"animates":crate::engine::registered_animate(container(vm,focus)),"queued_keys":STATE.with(|s|s.borrow().last_keys.len()),"labels":labels,"exit":slr_lang::seam::with_lang_host(|h|h.exit_requested())});
    journal::emit(
        "menu-state",
        json!({"input_id":input_id(),"tick":STATE.with(|s|s.borrow().tick),"state":snapshot}),
    )?;
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        let tick = s.tick;
        let event = s.last_event.clone();
        s.history
            .push(serde_json::json!({"tick":tick,"event":event,"state":snapshot}));
        if s.history.len() > 512 {
            s.history.remove(0);
        }
        s.recorded = Some(s.event_count);
        std::fs::write(
            output.join("input-state.json.tmp"),
            serde_json::to_vec_pretty(&serde_json::json!({"history":s.history,"bindings":s.hotkeys.iter().flatten().map(|h|serde_json::json!({"key":h.key,"flags":h.flags,"filter":h.filter,"osd":h.osd,"target":h.target,"command":field(vm,h.word,"command")})).collect::<Vec<_>>(),"escape":constant(vm,"java.io.Input","RCDIK_ESCAPE")}))
                .map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        std::fs::rename(output.join("input-state.json.tmp"),output.join("input-state.json")).map_err(|e|e.to_string())
    })
}

pub fn interacted() -> bool {
    STATE.with(|s| s.borrow().event_count > 0)
}

pub(crate) fn focused_osd(vm: &VmHost) -> u32 {
    let Some(queue) = constant(vm, "java.render.Frontend", "inputQueue") else {
        return 0;
    };
    let Some(count) = field(vm, queue, "elementCount").filter(|c| *c > 0) else {
        return 0;
    };
    field(vm, queue, "elementData")
        .and_then(|data| vm.array_cells(data))
        .and_then(|cells| cells.get(count as usize - 1))
        .map_or(0, |v| v.payload)
}
fn cursor_event(vm: &mut VmHost, hit: crate::presentation::Hit, event: &str) -> Result<(), String> {
    let ec = constant(vm, "java.lang.GameType", event)
        .ok_or_else(|| format!("cursor constant {event}"))?;
    let mask =
        constant(vm, "java.lang.GameType", "EVENT_CURSOR").ok_or("EVENT_CURSOR unavailable")?;
    // Group.handleEvent reads the picked ID at token 2 for click/drop,
    // token 3 for down/up, and tokens 3/4 (ID/enter flag) for hover/leave.
    route(
        "cursor",
        json!({"event":event,"physics":hit.physics,"group":hit.group,"osd":hit.osd}),
    )?;
    let id = hit.physics as i32;
    let text = match event {
        "EC_LCLICK" => format!("{ec} 0 {id}"),
        _ => format!("{ec} 0 0 {id} 1"),
    };
    crate::engine::dispatch(
        vm,
        container(vm, hit.group),
        None,
        mask,
        &crate::engine::EventArg::Str(Some(text)),
        None,
    )
}
fn pointer(vm: &mut VmHost, x: f32, y: f32) -> Result<(), String> {
    let hit = crate::presentation::hit_test(vm, focused_osd(vm), x, y);
    let previous = STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.pointer = [x, y];
        s.hover
    });
    route(
        "pick",
        json!({"point":[x,y],"focus":focused_osd(vm),"hit":hit.map(|h|json!({"physics":h.physics,"group":h.group,"osd":h.osd})),"previous":previous.map(|h|h.physics)}),
    )?;
    if previous != hit {
        if let Some(old) = previous {
            cursor_event(vm, old, "EC_LEAVE")?;
        }
        if let Some(new) = hit {
            cursor_event(vm, new, "EC_HOVER")?;
        }
        STATE.with(|s| s.borrow_mut().hover = hit);
    }
    Ok(())
}
fn button(vm: &mut VmHost, pressed: bool) -> Result<(), String> {
    let pos = STATE.with(|s| s.borrow().pointer);
    pointer(vm, pos[0], pos[1])?;
    let hit = STATE.with(|s| s.borrow().hover);
    if pressed {
        if let Some(hit) = hit {
            cursor_event(vm, hit, "EC_LDOWN")?;
        }
        STATE.with(|s| s.borrow_mut().down = hit);
    } else {
        let down = STATE.with(|s| s.borrow_mut().down.take());
        if let Some(down) = down {
            cursor_event(vm, down, "EC_LUP")?;
            if Some(down) == hit && focused_osd(vm) == down.osd {
                cursor_event(vm, down, "EC_LCLICK")?;
            }
        }
    }
    Ok(())
}
fn cancel(vm: &mut VmHost) -> Result<(), String> {
    let hover = STATE.with(|state| {
        let mut s = state.borrow_mut();
        s.down = None;
        s.held.clear();
        s.last_keys.clear();
        s.hover.take()
    });
    if let Some(hit) = hover {
        cursor_event(vm, hit, "EC_LEAVE")?;
    }
    let osd = focused_osd(vm);
    let group = field(vm, osd, "pointedGadgetGroup").unwrap_or(0);
    let gadget = field(vm, osd, "pointedGadget").unwrap_or(0);
    if let Some(physics) = field(vm, gadget, "phy") {
        let id = slr_resource::node::with_res(|h| {
            h.container(container(vm, physics)).map_or(0, |c| c.id)
        });
        if id != 0 && group != 0 {
            cursor_event(
                vm,
                crate::presentation::Hit {
                    group,
                    osd,
                    physics: id,
                },
                "EC_LEAVE",
            )?;
        }
    }
    Ok(())
}

pub fn finish() -> Result<(), String> {
    STATE.with(|s| {
        let s = s.borrow();
        if s.schedule.is_empty() {
            Ok(())
        } else {
            Err(format!(
                "InputReplayBoundary: {} events unconsumed",
                s.schedule.len()
            ))
        }
    })
}

//! GameType native wrappers over the required engine ABI boundary. The backend
//! owns game objects, timers, callbacks and resource loading; wrappers preserve
//! the executable's marshalling, branching and intrusive-link mutations.
use super::engine::{invoke, unlink, with_engine, Engine};
use crate::seam::{NativeBinding, NativeTable};
use slr_vm::host::{marshal_args, CallInfo, VmHost};
fn words(info: &CallInfo<'_>) -> [u32; 6] {
    let mut out = [0; 6];
    marshal_args(info, &mut out).expect("GameType arguments");
    out
}
fn resolve(e: &mut dyn Engine, receiver: u32) -> u32 {
    let field = e.read(0x62e008);
    invoke(e, 0x42ab50, 0, &[receiver, field])
}
fn diagnostic(e: &mut dyn Engine, marker: u32, message: u32) {
    invoke(e, 0x551140, 0, &[0x62e018, marker, 0x100]);
    invoke(e, 0x551140, 0, &[0x62e018, message, 0x100]);
    invoke(e, 0x5513b0, 0, &[0x62e018]);
    let word = e.read(0x62e018) & !0xff;
    e.write(0x62e018, word);
}
/// exe: LAB_0047e000
pub fn rem_notification(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        if w[1] != 0 && object != 0 {
            invoke(e, 0x48b670, 0, &[object, w[2]]);
        }
    });
    None
}
fn notification(info: &CallInfo<'_>, extended: bool) {
    let w = words(info);
    // The executable guards on the GameRef argument but resolves the receiver.
    if w[1] == 0 {
        return;
    }
    with_engine(|e| {
        let object = resolve(e, w[0]);
        if object == 0 {
            diagnostic(
                e,
                if extended { 0x612f58 } else { 0x612f44 },
                if extended { 0x612f5c } else { 0x612f48 },
            );
        } else {
            invoke(
                e,
                0x48b500,
                0,
                &[object, w[2], w[3], w[4], if extended { w[5] } else { 0 }],
            );
        }
    });
}
/// exe: LAB_0047e050
pub fn add_notification(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    notification(info, false);
    None
}
/// exe: LAB_0047e100
pub fn add_notification_extended(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    notification(info, true);
    None
}
/// exe: LAB_0047e1c0
pub fn add_timer(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    let now = crate::seam::with_lang_host(|h| h.sim_time);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        if object == 0 {
            diagnostic(e, 0x612f6c, 0x612f70);
        } else {
            invoke(
                e,
                0x48b750,
                object,
                &[(f32::from_bits(w[1]) + now).to_bits(), 0x80000080, w[2], 0],
            );
        }
    });
    None
}
/// exe: LAB_0047e260
pub fn remove_all_timers(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        if object == 0 {
            diagnostic(e, 0x612f80, 0x612f84);
        } else {
            invoke(e, 0x48b8c0, object, &[]);
        }
    });
    None
}
/// exe: FUN_004819f0
pub fn set_event_mask(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        invoke(e, 0x48d3d0, 0, &[object, w[1], 0]);
    });
    None
}
/// exe: FUN_00481a30
pub fn clear_event_mask(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        invoke(e, 0x48d420, 0, &[object, w[1]]);
    });
    None
}
/// exe: FUN_00481c40
pub fn register_callback(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        invoke(e, 0x427370, 0x636338, &[object, w[1]]);
    });
    None
}
/// exe: FUN_00481c80
pub fn unregister_callback(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        invoke(e, 0x4274e0, 0x636338, &[object, w[1]]);
    });
    None
}
/// The forwarded engine function is itself `ret 4` in build 606.
/// exe: LAB_00481cc0
pub fn unregister_callbacks(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let object = resolve(e, w[0]);
        invoke(e, 0x4274d0, 0x636338, &[object]);
    });
    None
}
/// Resolve resource/class defaults, create the object and repair both link lists.
/// exe: LAB_00481a70
pub fn create_native_instance(_: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let w = words(info);
    with_engine(|e| {
        let link = e.scratch(4);
        let mut resource = w[2];
        if resource == 0 {
            let class = e.read(w[0] + 12);
            resource = e.read(class + 0x1d4);
            if resource == 0 {
                let name = invoke(e, 0x404ea0, class, &[]);
                let resolved = invoke(e, 0x53a1a0, 0, &[0x62f260, class, name]);
                resource = e.read(resolved + 0x50);
                e.write(class + 0x1d4, resource);
            }
        }
        // The resource resolver writes {prev,next,value,owner} into link.
        invoke(e, 0x546070, link, &[resource, 8, 0]);
        let mut owner = w[1];
        if owner == 0 || e.read(owner + 8) == 0 {
            owner = 0x636460;
        }
        let name = if w[4] == 0 {
            let class = e.read(w[0] + 12);
            invoke(e, 0x404ea0, class, &[])
        } else {
            w[4]
        };
        let native = invoke(e, 0x53a2a0, 0, &[owner, link, w[0], w[3], name]);
        let object = resolve(e, w[0]);
        if object == 0 {
            diagnostic(e, 0x613384, 0x613388);
        } else if e.read(object + 12) != native {
            if e.read(object + 12) != 0 {
                unlink(e, object);
                for offset in [0, 4, 8, 12] {
                    e.write(object + offset, 0);
                }
            }
            if native != 0 {
                invoke(e, 0x4290f0, native + 0x44, &[object]);
                let script = e.read(native + 0x50);
                e.write(object + 8, script);
            } else {
                for offset in [0, 4, 8, 12] {
                    e.write(object + offset, 0);
                }
            }
        }
        unlink(e, link);
    });
    None
}

pub const GAMETYPE_NATIVE_BINDINGS: &[NativeBinding] = &[
    NativeBinding {
        class: "java.lang.GameType",
        name: "remNotification",
        sig: "(Ljava.util.resource.GameRef;I)V",
        impl_addr: 0x0047e000,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "addNotification",
        sig: "(Ljava.util.resource.GameRef;IILjava.lang.String;)V",
        impl_addr: 0x0047e050,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "addNotification",
        sig: "(Ljava.util.resource.GameRef;IILjava.lang.String;Ljava.lang.String;)V",
        impl_addr: 0x0047e100,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "addTimer",
        sig: "(FI)V",
        impl_addr: 0x0047e1c0,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "removeAllTimers",
        sig: "()V",
        impl_addr: 0x0047e260,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "setEventMask",
        sig: "(I)V",
        impl_addr: 0x004819f0,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "clearEventMask",
        sig: "(I)V",
        impl_addr: 0x00481a30,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "registerCallback",
        sig: "(I)V",
        impl_addr: 0x00481c40,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "unregisterCallback",
        sig: "(I)V",
        impl_addr: 0x00481c80,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "unregisterCallbacks",
        sig: "()V",
        impl_addr: 0x00481cc0,
    },
    NativeBinding {
        class: "java.lang.GameType",
        name: "createNativeInstance",
        sig: "(Ljava.util.resource.GameRef;ILjava.lang.String;Ljava.lang.String;)V",
        impl_addr: 0x00481a70,
    },
];
pub fn install(table: &mut NativeTable) {
    table.set_vm(0x0047e000, rem_notification);
    table.set_vm(0x0047e050, add_notification);
    table.set_vm(0x0047e100, add_notification_extended);
    table.set_vm(0x0047e1c0, add_timer);
    table.set_vm(0x0047e260, remove_all_timers);
    table.set_vm(0x004819f0, set_event_mask);
    table.set_vm(0x00481a30, clear_event_mask);
    table.set_vm(0x00481c40, register_callback);
    table.set_vm(0x00481c80, unregister_callback);
    table.set_vm(0x00481cc0, unregister_callbacks);
    table.set_vm(0x00481a70, create_native_instance);
}

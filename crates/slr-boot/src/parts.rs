//! Original Part scalar and slot wrappers over the native constructor's raw cells.
use slr_physics::space::Space;
use slr_vm::host::{marshal_args, CallInfo, VmHost};
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn one_minus(bits: u32) -> u32 {
    X87::from_f32(1.)
        .sub(X87::from_f32(f32::from_bits(bits)), PC, RC)
        .to_f32(PC, RC)
        .to_bits()
}
fn scalar(m: &mut Space, body: u32, name: &str, arg: u32) -> Option<u32> {
    let valid = body != 0;
    match name {
        "getWear" => Some(if valid {
            one_minus(m.r32(body + 0xb8))
        } else {
            1f32.to_bits()
        }),
        "setWear" => {
            if valid {
                m.w32(body + 0xb8, one_minus(arg));
            }
            Some(arg)
        }
        "getTear" => Some(if valid {
            m.r32(body + 0xb4)
        } else {
            1f32.to_bits()
        }),
        "setTear" => {
            if valid {
                m.w32(body + 0xb4, arg);
            }
            Some(arg)
        }
        "setMaxWear" => {
            if valid {
                m.w32(
                    body + 0xbc,
                    X87::from_f32(1.)
                        .div(X87::from_f32(f32::from_bits(arg)), PC, RC)
                        .to_f32(PC, RC)
                        .to_bits(),
                );
            }
            None
        }
        "getCar" => Some(if valid { m.r32(body + 0xc8) } else { 0 }),
        "getWheelID" => {
            let slot = if valid { m.r32(body + 0xd0) } else { 0 };
            Some(if (101..=400).contains(&slot) {
                (slot - 101) % 10
            } else {
                u32::MAX
            })
        }
        _ => unreachable!("scalar Part method {name}"),
    }
}
/// Original429390/40cfc0 traversal excludes the tail sentinel (whose next is0).
fn slots(m: &Space, template: u32) -> Vec<u32> {
    let mut cursor = m.r32(template + 0x115c);
    let mut result = Vec::new();
    let limit = m.r32(template + 0x1150) as usize;
    while cursor != 0 && m.r32(cursor + 4) != 0 && result.len() < limit {
        result.push(cursor);
        cursor = m.r32(cursor + 4);
    }
    result
}
fn slot_query(m: &Space, template: u32, body: u32, name: &str, arg: u32) -> u32 {
    let nodes = slots(m, template);
    match name {
        "getSlots" => m.r32(template + 0x1150),
        "getSlotID" => {
            let index = if (arg as i32) < 0 {
                let runtime = m.r32(body + 0x78);
                (0..nodes.len()).find(|&i| m.r8(runtime + i as u32 * 0x68 + 0x14) & 1 != 0)
            } else {
                Some(arg as usize)
            };
            index
                .and_then(|i| nodes.get(i))
                .map_or(0, |&n| m.r32(n + 0x48))
        }
        "getSlotIndex" => nodes
            .iter()
            .position(|&n| m.r32(n + 0x48) == arg)
            .unwrap_or(0) as u32,
        "isSlotDisabled" => nodes
            .iter()
            .position(|&n| m.r32(n + 0x48) == arg)
            .map_or(0, |i| {
                let flags = m.r32(m.r32(body + 0x78) + i as u32 * 0x68 + 0x14);
                if flags & 0x50 != 0 {
                    2
                } else {
                    (flags >> 4) & 1
                }
            }),
        _ => unreachable!("slot Part method {name}"),
    }
}
fn script(container: u32) -> u32 {
    let container = crate::native_body::logical_resource(container).unwrap_or(container);
    slr_resource::node::with_res(|host| {
        host.container(container)
            .and_then(|c| c.instance.as_ref())
            .map_or(0, |i| i.script)
    })
}
fn runtime_slot(m: &Space, template: u32, body: u32, id: u32) -> Option<u32> {
    let base = m.r32(body + 0x78);
    if base == 0 {
        return None;
    }
    slots(m, template)
        .iter()
        .position(|&n| m.r32(n + 0x48) == id)
        .map(|i| base + i as u32 * 0x68)
}
fn disable_slot(m: &mut Space, template: u32, body: u32, id: u32, mode: u32) -> Option<(u32, u32)> {
    let slot = runtime_slot(m, template, body, id)?;
    let flags = (m.r32(slot + 0x14) & !0x50)
        | match mode {
            1 => 0x10,
            2 => 0x50,
            _ => 0,
        };
    m.w32(slot + 0x14, flags);
    (m.r32(slot + 8) != 0 && m.r32(slot + 0xc) != 0)
        .then(|| (m.r32(slot + 0xc), m.r32(slot + 0x10)))
}
fn native(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut args = vec![0; info.args.len().max(1)];
    if let Err(error) = marshal_args(info, &mut args) {
        vm.native_failure = Some(format!("PartBoundary: argument marshal {error:?}"));
        return None;
    }
    let receiver = args[0];
    let arg = args.get(1).copied().unwrap_or(0);
    let container =
        slr_resource::node::with_res(|h| h.node(h.object_node(receiver)).map(|n| n.container))
            .unwrap_or(0);
    let query = matches!(
        info.name,
        "getSlots"
            | "getSlotID"
            | "getSlotIndex"
            | "isSlotDisabled"
            | "getCarRef"
            | "partOnSlot"
            | "slotIDOnSlot"
    );
    if info.name == "disableSlot" {
        let mode = args.get(2).copied().unwrap_or(0);
        if container != 0 {
            match crate::native_body::with_body(container, |state| {
                disable_slot(&mut state.memory, state.template, state.body, arg, mode)
            }) {
                Some(peer) => {
                    if let Some((container, id)) = peer {
                        let container =
                            crate::native_body::logical_resource(container).unwrap_or(container);
                        if crate::native_body::with_body(container, |state| {
                            if let Some(slot) =
                                runtime_slot(&state.memory, state.template, state.body, id)
                            {
                                let flags = state.memory.r32(slot + 0x14);
                                state.memory.w32(
                                    slot + 0x14,
                                    (flags & !0x50)
                                        | match mode {
                                            1 => 0x10,
                                            2 => 0x50,
                                            _ => 0,
                                        },
                                );
                            }
                        })
                        .is_none()
                        {
                            vm.native_failure = Some(format!(
                                "PartBoundary: attached peer {container:#x} native body missing"
                            ));
                        }
                    }
                }
                None => {
                    vm.native_failure = Some(format!(
                        "PartBoundary: resource {container:#x} has no native body for disableSlot"
                    ))
                }
            }
        }
        return None;
    }
    if container == 0 {
        return if query {
            Some(0)
        } else {
            scalar(&mut Space::new(), 0, info.name, arg)
        };
    }
    match crate::native_body::with_body(container, |state| {
        if info.name == "getCarRef" {
            Some(script(state.memory.r32(state.body + 0xcc)))
        } else if info.name == "partOnSlot" {
            Some(if arg == u32::MAX {
                script(container)
            } else {
                runtime_slot(&state.memory, state.template, state.body, arg)
                    .map_or(0, |slot| script(state.memory.r32(slot + 0xc)))
            })
        } else if info.name == "slotIDOnSlot" {
            Some(
                runtime_slot(&state.memory, state.template, state.body, arg)
                    .map_or(0, |slot| state.memory.r32(slot + 0x10)),
            )
        } else if query {
            Some(slot_query(
                &state.memory,
                state.template,
                state.body,
                info.name,
                arg,
            ))
        } else {
            scalar(&mut state.memory, state.body, info.name, arg)
        }
    }) {
        Some(result) => result,
        None => {
            vm.native_failure = Some(format!(
                "PartBoundary: resource {container:#x} has no loaded native body for {}",
                info.name
            ));
            None
        }
    }
}
pub fn install(vm: &mut VmHost) {
    for &(class, name, sig, address) in crate::missing_rows::MISSING {
        if class == "java.game.parts.Part"
            && matches!(
                name,
                "getTexture" | "setTexture" | "getRenderType" | "setRenderType"
            )
        {
            vm.natives.register(class, name, sig, address);
            vm.native_impls.insert(address, crate::part_render::native);
            continue;
        }
        if class == "java.game.parts.Part"
            && matches!(
                name,
                "getWear"
                    | "setWear"
                    | "getTear"
                    | "setTear"
                    | "setMaxWear"
                    | "getCar"
                    | "getWheelID"
                    | "disableSlot"
                    | "getCarRef"
                    | "partOnSlot"
                    | "slotIDOnSlot"
                    | "getSlots"
                    | "getSlotID"
                    | "getSlotIndex"
                    | "isSlotDisabled"
            )
        {
            vm.natives.register(class, name, sig, address);
            vm.native_impls.insert(address, native);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_scalar_wrapper_rows() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/part-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("optional original probe absent; run re/oracle/part_probe.py");
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let body = m.alloc(0x180);
            for (offset, value) in [
                (0xb4, 0.25f32.to_bits()),
                (0xb8, 0.125f32.to_bits()),
                (0xbc, 2f32.to_bits()),
                (0xc8, 0xffff1234),
                (0xd0, row["slot"].as_i64().unwrap() as u32),
            ] {
                m.w32(body + offset, value);
            }
            let actual = scalar(
                &mut m,
                if row["mode"] == "valid" { body } else { 0 },
                row["name"].as_str().unwrap(),
                row["arg"].as_u64().unwrap() as u32,
            );
            assert_eq!(actual, row["result"].as_u64().map(|v| v as u32), "{row}");
            let expected = row["body"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(
                (0xb4..0xd4)
                    .step_by(4)
                    .map(|o| m.r32(body + o))
                    .collect::<Vec<_>>(),
                expected
            );
        }
    }
    #[test]
    fn original_slot_sentinel_order_and_flag_projection() {
        let mut m = Space::new();
        let template = m.alloc(0x2e98);
        let body = m.alloc(0x180);
        let runtime = m.alloc(3 * 0x68);
        let nodes = [m.alloc(0xc0), m.alloc(0xc0), m.alloc(0xc0)];
        let tail = m.alloc(12);
        m.w32(template + 0x1150, 3);
        m.w32(template + 0x115c, nodes[0]);
        m.w32(body + 0x78, runtime);
        for (i, n) in nodes.into_iter().enumerate() {
            m.w32(n + 4, nodes.get(i + 1).copied().unwrap_or(tail));
            m.w32(n + 0x48, [101, 201, 301][i]);
            m.w32(runtime + i as u32 * 0x68 + 0x14, [0, 0x11, 0x40][i]);
        }
        // Original wrappers4684a0/468570/4686e0/469970, real list helpers;
        // re/oracle/part_probe.py produces the independent 40-row projection.
        let args = [-2i32, -1, 0, 1, 2, 3, 101, 201, 301, 999];
        for (name, expected) in [
            ("getSlots", [3, 3, 3, 3, 3, 3, 3, 3, 3, 3]),
            ("getSlotID", [201, 201, 101, 201, 301, 0, 0, 0, 0, 0]),
            ("getSlotIndex", [0, 0, 0, 0, 0, 0, 0, 1, 2, 0]),
            ("isSlotDisabled", [0, 0, 0, 0, 0, 0, 0, 2, 2, 0]),
        ] {
            for (arg, expected) in args.into_iter().zip(expected) {
                assert_eq!(
                    slot_query(&m, template, body, name, arg as u32),
                    expected,
                    "{name}({arg})"
                );
            }
        }
    }
    #[test]
    fn original_disable_slot_projects_same_mode_to_reciprocal_peer() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/part-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("optional original Part probe absent");
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows["slots"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|r| r["name"] == "disableSlot")
        {
            let mut memory = Space::new();
            let template = memory.alloc(0x2e98);
            let body = memory.alloc(0x180);
            let runtime = memory.alloc(3 * 0x68);
            let descriptors = [memory.alloc(0xc0), memory.alloc(0xc0), memory.alloc(0xc0)];
            let tail = memory.alloc(12);
            memory.w32(template + 0x1150, 3);
            memory.w32(template + 0x115c, descriptors[0]);
            memory.w32(body + 0x78, runtime);
            for (i, n) in descriptors.into_iter().enumerate() {
                memory.w32(n + 4, descriptors.get(i + 1).copied().unwrap_or(tail));
                memory.w32(n + 0x48, [101, 201, 301][i]);
                memory.w32(runtime + i as u32 * 0x68 + 0x14, [0x10, 0x11, 0x40][i]);
            }
            memory.w32(runtime + 0x68 + 8, 0xffff1111);
            memory.w32(runtime + 0x68 + 0xc, 77);
            memory.w32(runtime + 0x68 + 0x10, 101);
            let id = row["args"][0].as_i64().unwrap() as u32;
            let mode = row["args"][1].as_i64().unwrap() as u32;
            let peer = disable_slot(&mut memory, template, body, id, mode);
            if let Some((77, id)) = peer {
                let slot = runtime_slot(&memory, template, body, id).unwrap();
                let flags = memory.r32(slot + 0x14);
                memory.w32(
                    slot + 0x14,
                    (flags & !0x50)
                        | match mode {
                            1 => 0x10,
                            2 => 0x50,
                            _ => 0,
                        },
                );
            }
            assert_eq!(
                (0..3)
                    .map(|i| memory.r32(runtime + i * 0x68 + 0x14))
                    .collect::<Vec<_>>(),
                row["flags"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>(),
                "{row}"
            );
        }
    }
}

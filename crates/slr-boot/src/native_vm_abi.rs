//! Prepared native-entry frame kernel from420025 through the native CALL.
//!
//! This is a partial source projection, not all of41fbc0. The caller must have
//! already executed original lookup, conversion and argument allocation, with
//! their live stack/register history present. Callback creation and Thread.run
//! do not yet provide that context in production; their guard must remain.
use crate::native_stack::{self as stack, Reg};
use slr_physics::space::Space;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
// Pending root: production callback/interpreter context is still unavailable.
#[allow(dead_code)]
pub enum ReturnKind {
    Absent,
    Void,
    Integer,
    Float,
    Reference,
}

#[cfg(test)]
#[derive(Clone, Debug, PartialEq, Eq)]
struct Store {
    pc: u32,
    address: u32,
    value: u32,
}
#[cfg(test)]
thread_local! {
    static STORES: std::cell::RefCell<Vec<Store>> = const { std::cell::RefCell::new(Vec::new()) };
}
fn note(pc: u32, address: u32, value: u32) {
    #[cfg(test)]
    STORES.with(|stores| stores.borrow_mut().push(Store { pc, address, value }));
    #[cfg(not(test))]
    let _ = (pc, address, value);
}
fn write(m: &mut Space, pc: u32, address: u32, value: u32) {
    note(pc, address, value);
    m.w32(address, value);
}

/// Enter the native with its actual pending CALL frame. `native` must consume
/// that frame through the existing native_stack function/enter protocol.
///
/// Preconditions: active41fbc0 frame at its420025 stack base; all seven
/// registers and original frame slots populated; prepared argument vector;
/// cached signature types; live thread/JVM and native record/implementation.
/// The original native suffix (result conversion, release and return) is not
/// implemented here. This function returns at the dispatcher's stack base;
/// native stack stores remain retained. No direct-entry fallback is supplied.
// Pending root: tested with explicit original caller contexts, never wired to
// production until callback/interpreter/conversion history supplies them.
#[allow(dead_code)]
pub fn call_prepared<R>(
    m: &mut Space,
    native: impl FnOnce(&mut Space, u32, u32, ReturnKind) -> R,
) -> Result<R, String> {
    if !stack::active(m) {
        return Err("NativeAbiBoundary: prepared native kernel requires an active caller".into());
    }
    let b = stack::sp(m);
    let thread = m.r32(b + 0x10);
    let record = m.r32(b + 0x14);
    let signature = m.r32(b + 0x50);
    let types = if signature == 0 {
        0
    } else {
        m.r32(signature + 8)
    };
    let implementation = stack::reg(m, Reg::Edi);
    if thread == 0
        || m.r32(thread + 0x10) == 0
        || record == 0
        || implementation == 0
        || m.r32(record + 8) != implementation
        || types == 0
    {
        return Err(
            "NativeAbiBoundary: missing prepared thread/record/cached signature context".into(),
        );
    }
    let count = m.r32(types + 8);
    let capacity = m.r32(types);
    if count == 0 || count > 1024 || capacity > 1024 {
        return Err("NativeAbiBoundary: unsupported prepared signature container".into());
    }
    let result_cell = if (capacity as i32) > (count as i32 - 1) {
        m.r32(m.r32(types + 4) + (count - 1) * 4)
    } else {
        0
    };
    let kind = if result_cell == 0 {
        ReturnKind::Absent
    } else {
        match m.r8(m.r32(result_cell)) {
            b'F' => ReturnKind::Float,
            b'I' => ReturnKind::Integer,
            b'L' | b'[' => ReturnKind::Reference,
            b'V' => ReturnKind::Void,
            _ => return Err("NativeAbiBoundary: unsupported native return branch".into()),
        }
    };
    //42008c's float arm calls the saved implementation slot, while the other
    // arms call EDI. Preserve the actual source operand in both cases.
    let call_target = if kind == ReturnKind::Float {
        m.r32(b + 0x1c)
    } else {
        implementation
    };
    if call_target == 0 {
        return Err("NativeAbiBoundary: missing prepared native CALL target".into());
    }

    stack::set_reg(m, Reg::Ebx, thread);
    write(
        m,
        0x420029,
        stack::reg(m, Reg::Eax).wrapping_add(stack::reg(m, Reg::Ecx).wrapping_mul(4)),
        stack::reg(m, Reg::Esi),
    );
    let count = m.r32(b + 0x2c).wrapping_add(1);
    write(m, 0x420035, b + 0x2c, count);
    let jvm = m.r32(thread + 0x10);
    stack::set_reg(m, Reg::Edx, jvm);
    stack::set_reg(m, Reg::Eax, b + 0x24);
    write(m, 0x420040, b + 0x48, record);
    stack::set_reg(m, Reg::Ecx, signature);
    write(m, 0x420048, b + 0x40, thread);
    write(m, 0x42004c, b + 0x3c, jvm);
    write(m, 0x420050, b + 0x44, b + 0x24);

    // Cached41e2f0 executes a real modeled CALL and its PUSH ESI. Uncached
    //41dfd0 parsing/allocation remains an explicit prerequisite above.
    note(0x420054, b - 4, 0x420059);
    stack::call(m, 0x420059, &[], Some(signature), |m| {
        stack::function(m, &[], Some(signature), |m| {
            note(0x41e2f0, stack::sp(m) - 4, stack::reg(m, Reg::Esi));
            stack::push_reg(m, Reg::Esi);
            stack::set_reg(m, Reg::Esi, signature);
            stack::set_reg(m, Reg::Ecx, types);
            stack::set_reg(m, Reg::Edx, capacity);
            let count = m.r32(types + 8);
            if (capacity as i32) > (count as i32 - 1) {
                stack::set_reg(m, Reg::Ecx, m.r32(types + 4));
            }
            stack::set_reg(m, Reg::Eax, result_cell);
        })
    });

    let info = b + 0x3c;
    let (return_pc, push_pc, call_pc) = if kind == ReturnKind::Absent {
        stack::set_reg(m, Reg::Eax, info);
        (0x4200b9, 0x4200b6, 0x4200b7)
    } else {
        let descriptor = m.r32(result_cell);
        let byte = m.r8(descriptor);
        stack::set_reg(m, Reg::Edx, descriptor);
        stack::set_reg(m, Reg::Eax, (byte as u32).wrapping_sub(0x46));
        match kind {
            ReturnKind::Integer => {
                stack::set_reg(m, Reg::Ecx, 1);
                stack::set_reg(m, Reg::Edx, info);
                (0x420080, 0x42007d, 0x42007e)
            }
            ReturnKind::Float => {
                stack::set_reg(m, Reg::Ecx, 0);
                stack::set_reg(m, Reg::Eax, info);
                (0x420090, 0x42008b, 0x42008c)
            }
            ReturnKind::Reference => {
                stack::set_reg(m, Reg::Ecx, info);
                (0x4200a4, 0x4200a1, 0x4200a2)
            }
            ReturnKind::Void => {
                stack::set_reg(m, Reg::Ecx, 3);
                stack::set_reg(m, Reg::Edx, info);
                (0x4200b9, 0x4200af, 0x4200b7)
            }
            ReturnKind::Absent => unreachable!(),
        }
    };
    note(push_pc, b - 4, info);
    note(call_pc, b - 8, return_pc);
    Ok(stack::call(m, return_pc, &[info], None, |m| {
        native(m, call_target, info, kind)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    const REGS: [(&str, Reg); 7] = [
        ("eax", Reg::Eax),
        ("ebx", Reg::Ebx),
        ("ecx", Reg::Ecx),
        ("edx", Reg::Edx),
        ("esi", Reg::Esi),
        ("edi", Reg::Edi),
        ("ebp", Reg::Ebp),
    ];
    fn word(value: &Value) -> u32 {
        value.as_u64().unwrap().try_into().unwrap()
    }
    fn bytes(text: &Value) -> Vec<u8> {
        let text = text.as_str().unwrap();
        assert_eq!(text.len() % 2, 0);
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }
    fn check(expected: &Value) {
        let mut m = Space::new();
        stack::function(&mut m, &[], None, |m| {
            for page in expected["initial"]["pages"].as_array().unwrap() {
                let base = word(&page["address"]);
                let data = bytes(&page["bytes"]);
                assert_eq!(data.len(), 4096, "complete original input page");
                for (i, byte) in data.into_iter().enumerate() {
                    m.w8(base + i as u32, byte);
                }
            }
            let initial = &expected["initial"]["registers"];
            for (name, reg) in REGS {
                stack::set_reg(m, reg, word(&initial[name]));
            }
            let b = word(&initial["esp"]);
            stack::set_sp(m, b);
            STORES.with(|stores| stores.borrow_mut().clear());
            let mut invoked = false;
            call_prepared(m, |m, implementation, info, _| {
                invoked = true;
                for (name, reg) in REGS {
                    assert_eq!(
                        stack::reg(m, reg),
                        word(&expected["registers"][name]),
                        "{name}"
                    );
                }
                assert_eq!(stack::sp(m), word(&expected["registers"]["esp"]));
                assert_eq!(implementation, stack::reg(m, Reg::Edi));
                assert_eq!(info, word(&expected["call_info"]));
                assert_eq!(m.r32(stack::sp(m)), word(&expected["return_pc"]));
                let base = word(&expected["stack_address"]);
                let expected_bytes = bytes(&expected["stack_hex"]);
                assert_eq!(expected_bytes.len(), 0xa0);
                assert_eq!(
                    (0..0xa0).map(|i| m.r8(base + i)).collect::<Vec<_>>(),
                    expected_bytes,
                    "complete native-entry stack"
                );
                let writes = expected["writes"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        assert_eq!(s["size"], 4);
                        Store {
                            pc: word(&s["pc"]),
                            address: word(&s["address"]),
                            value: word(&s["value"]),
                        }
                    })
                    .collect::<Vec<_>>();
                STORES
                    .with(|stores| assert_eq!(*stores.borrow(), writes, "ordered original stores"));
                // Consume the actual pending CALL using the wrapper protocol.
                stack::function(m, &[info], None, |m| {
                    assert_eq!(stack::argument(m, 0), info)
                });
            })
            .unwrap();
            assert!(invoked);
            assert_eq!(stack::sp(m), b);
        });
    }
    #[test]
    fn prepared_entry_kernel_matches_original_controls_and_genuine_stock_graph() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let data: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-dispatch-kernel-original.json"))
                .expect("run native_dispatch_frame_probe.py"),
        )
        .unwrap();
        assert_eq!(data["schema"], 1);
        assert_eq!(
            data["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        for (name, digest) in data["producer_sources"].as_object().unwrap() {
            assert_eq!(
                digest,
                &Value::String(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                ))
            );
        }
        assert_eq!(
            data["producer_sources"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            [
                "native_dispatch_frame_probe.py",
                "oracle.py",
                "vmexec_vectors.py",
                "thread_vectors.py",
                "vmclass_vectors.py"
            ]
            .into_iter()
            .collect()
        );
        let rows = data["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 16, "complete signature/static/seed inventory");
        let inventory = rows
            .iter()
            .map(|row| {
                (
                    row["signature"].as_str().unwrap().to_string(),
                    row["static"].as_bool().unwrap(),
                    row["scratch_seed"].as_u64().unwrap(),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        let cases = [
            ("(IF)I", false),
            ("(IF)I", true),
            ("(IF)F", false),
            ("(IF)V", true),
            ("(IF)LProbeNative;", false),
            ("(IF)[I", true),
            ("(IF)", false),
            ("(IF)", true),
        ];
        assert_eq!(
            inventory,
            cases
                .into_iter()
                .flat_map(|(sig, static_call)| [606, 20261022].map(move |seed| (
                    sig.to_string(),
                    static_call,
                    seed
                )))
                .collect()
        );
        for row in rows {
            assert_eq!(row["matched"], true);
            check(&row["projection"]);
        }
        let stock = &data["genuine_stock_kernel"];
        assert_eq!(stock["matched"], true);
        for (name, digest) in stock["producer_sources"].as_object().unwrap() {
            assert_eq!(
                digest,
                &Value::String(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                ))
            );
        }
        assert_eq!(
            stock["producer_sources"]
                .as_object()
                .unwrap()
                .keys()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            [
                "native_vm_entry_probe.py",
                "snapshot_replay.py",
                "oracle.py",
                "headless_snapshot.py"
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(
            stock["snapshot_metadata_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/out/snapshots/install-part-queue.json")).unwrap()
                )
            )
        );
        assert_eq!(
            stock["snapshot_pages_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("re/out/snapshots/install-part-queue.pages")).unwrap()
                )
            )
        );
        check(&stock["projection"]);
    }
    #[test]
    fn missing_context_is_rejected_before_writes_or_native_invocation() {
        let mut m = Space::new();
        assert!(call_prepared(&mut m, |_, _, _, _| panic!("native must not run")).is_err());
        stack::function(&mut m, &[], None, |m| {
            m.begin_journal();
            assert!(call_prepared(m, |_, _, _, _| panic!("native must not run")).is_err());
            assert!(m.end_journal().is_empty());
        });
    }

    #[test]
    fn uncached_signature_rejects_before_touching_parent_scratch_or_registers() {
        let mut m = Space::new();
        stack::function(&mut m, &[], None, |m| {
            let b = 0x7fefed00;
            for i in 0..0x100 {
                m.w8(b - 64 + i, (i as u8).wrapping_mul(37));
            }
            let (thread, record, signature, implementation) =
                (0x20000100, 0x20000200, 0x20000300, 0x486f30);
            stack::set_sp(m, b);
            stack::set_reg(m, Reg::Edi, implementation);
            m.w32(b + 0x10, thread);
            m.w32(b + 0x14, record);
            m.w32(b + 0x50, signature);
            m.w32(thread + 0x10, 0x20000400);
            m.w32(record + 8, implementation);
            m.w32(signature + 8, 0);
            let before = (0..0x100).map(|i| m.r8(b - 64 + i)).collect::<Vec<_>>();
            let registers = REGS.map(|(_, reg)| stack::reg(m, reg));
            m.begin_journal();
            assert!(call_prepared(m, |_, _, _, _| panic!(
                "uncached signature must not enter native"
            ))
            .unwrap_err()
            .contains("cached signature"));
            assert!(m.end_journal().is_empty());
            assert_eq!(stack::sp(m), b);
            assert_eq!(REGS.map(|(_, reg)| stack::reg(m, reg)), registers);
            assert_eq!(
                (0..0x100).map(|i| m.r8(b - 64 + i)).collect::<Vec<_>>(),
                before
            );
        });
    }
}

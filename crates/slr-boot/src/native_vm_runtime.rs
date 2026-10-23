//! Source-derived raw operand-container operations.
//!
//! This seam consumes actual thread/frame/container/cell addresses. The cell
//! comes from an explicit owner resolver, never a payload search. It retains
//! real CALL/push stores using native_stack, but does not invent Thread.run's
//! missing opcode frames or metadata. The VM entry guard remains necessary.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[cfg(test)]
thread_local! { static STORES: std::cell::RefCell<Vec<(u32,u32)>> = const { std::cell::RefCell::new(Vec::new()) }; }
fn write(m: &mut Space, address: u32, word: u32) {
    #[cfg(test)]
    STORES.with(|s| s.borrow_mut().push((address, word)));
    m.w32(address, word);
}

/// Append an identified cell through original423ba0. The declared allocator
/// profile controls realloc; its own internal stores/register effects remain
/// outside this seam. Missing or malformed context fails before any writes.
/// Pending source CALLs are consumed by native_stack's normal entry protocol.
/// exe: 0x423ba0
#[allow(dead_code)]
pub fn push_operand(
    m: &mut Space,
    thread: u32,
    cell: u32,
    allocator: &mut impl Allocator,
) -> Result<(), String> {
    let frame = m.r32(thread.wrapping_add(0x20));
    let container = m.r32(frame.wrapping_add(0x28));
    let capacity = m.r32(container);
    let count = m.r32(container.wrapping_add(8));
    if thread == 0
        || frame == 0
        || container == 0
        || cell == 0
        || count > capacity
        || capacity > i32::MAX as u32 / 8
        || (capacity != 0 && m.r32(container + 4) == 0)
    {
        return Err("NativeVmRuntime: missing/malformed live operand context".into());
    }
    stack::function(m, &[cell], Some(thread), |m| {
        stack::set_reg(m, Reg::Eax, frame);
        stack::push_reg(m, Reg::Esi);
        stack::set_reg(m, Reg::Esi, container);
        stack::set_reg(m, Reg::Eax, count + 1);
        stack::set_reg(m, Reg::Ecx, capacity);
        if count + 1 > capacity {
            stack::push_reg(m, Reg::Edi);
            let next_capacity = capacity + count + 1;
            stack::set_reg(m, Reg::Edi, next_capacity);
            let old = m.r32(container + 4);
            stack::set_reg(m, Reg::Eax, old);
            stack::set_reg(m, Reg::Edx, next_capacity * 4);
            let data = stack::external(m, 0x423bdb, &[old, next_capacity * 4], None, |m| {
                allocator.reallocate(m, old, capacity * 4, next_capacity * 4)
            })?;
            stack::set_reg(m, Reg::Eax, data);
            write(m, container + 4, data);
            // The count truncation branch is unreachable for this validated
            // ordinary container shape. No free/reset path is synthesized.
            write(m, container, next_capacity);
            stack::add_sp(m, 4); // POP EDI, restored by the entry protocol.
        }
        stack::set_reg(m, Reg::Ecx, count);
        let data = m.r32(container + 4);
        stack::set_reg(m, Reg::Edx, data);
        stack::set_reg(m, Reg::Eax, cell);
        write(m, data + count * 4, cell);
        write(m, container + 8, count + 1);
        stack::add_sp(m, 4); // POP ESI.
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    struct Bump;
    impl Allocator for Bump {
        fn allocate(&mut self, m: &mut Space, bytes: u32) -> Result<u32, String> {
            Ok(m.alloc(bytes.max(1)))
        }
        fn reallocate(
            &mut self,
            m: &mut Space,
            old: u32,
            old_bytes: u32,
            bytes: u32,
        ) -> Result<u32, String> {
            let p = m.alloc(bytes.max(1));
            for i in 0..old_bytes.min(bytes) {
                m.w8(p + i, m.r8(old + i));
            }
            Ok(p)
        }
    }
    fn word(v: &Value) -> u32 {
        u32::try_from(v.as_u64().unwrap()).unwrap()
    }
    fn bytes(v: &Value) -> Vec<u8> {
        v.as_array()
            .unwrap()
            .iter()
            .map(|v| u8::try_from(word(v)).unwrap())
            .collect()
    }
    #[test]
    fn original_operand_growth_preserves_complete_heap_scratch_and_registers() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-operand-original.json"))
                .expect("run native_vm_operand_probe.py"),
        )
        .unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["fpu_cw"], 0x7f);
        assert_eq!(
            v["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        let sources = v["producer_sources"].as_object().unwrap();
        assert_eq!(
            sources
                .keys()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>(),
            [
                "native_vm_operand_probe.py",
                "oracle.py",
                "thread_vectors.py"
            ]
            .into_iter()
            .collect()
        );
        for (name, digest) in sources {
            assert_eq!(
                digest,
                &Value::String(format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                ))
            );
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 64);
        let inventory = rows
            .iter()
            .map(|r| {
                (
                    word(&r["capacity"]),
                    word(&r["count"]),
                    r["indirect"].as_bool().unwrap(),
                    word(&r["seed"]),
                    r["nested"].as_bool().unwrap(),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inventory,
            [
                (0, 0),
                (1, 0),
                (1, 1),
                (3, 2),
                (3, 3),
                (7, 0),
                (7, 6),
                (7, 7)
            ]
            .into_iter()
            .flat_map(
                |(cap, count)| [false, true]
                    .into_iter()
                    .flat_map(move |indirect| [606, 20261022]
                        .into_iter()
                        .flat_map(move |seed| [false, true]
                            .map(move |nested| (cap, count, indirect, seed, nested))))
            )
            .collect()
        );
        for row in rows {
            let mut m = Space::new();
            let before = bytes(&row["before"]["heap"]);
            assert_eq!(
                before.len(),
                (word(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b);
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            let stack_base = word(&row["stack_base"]);
            let scratch = bytes(&row["before"]["stack"]);
            assert_eq!(scratch.len(), 0x180);
            for (i, b) in scratch.into_iter().enumerate() {
                m.w8(stack_base + i as u32, b);
            }
            let steps = row["steps"].as_array().unwrap();
            assert_eq!(steps.len(), 9);
            for (index, step) in steps.iter().enumerate() {
                STORES.with(|s| s.borrow_mut().clear());
                let registers = [
                    Reg::Eax,
                    Reg::Ebx,
                    Reg::Ecx,
                    Reg::Edx,
                    Reg::Esi,
                    Reg::Edi,
                    Reg::Ebp,
                ];
                let (actual_regs, actual_sp) = if row["nested"].as_bool().unwrap() {
                    m.w32(word(&row["holder"]), word(&step["cell"]));
                    stack::function(&mut m, &[], None, |m| {
                        for (reg, name) in [(Reg::Ebx, "ebx"), (Reg::Esi, "esi"), (Reg::Edi, "edi")]
                        {
                            stack::set_reg(m, reg, word(&row["incoming"][name]));
                        }
                        stack::call(
                            m,
                            word(&row["parent"]) + 16,
                            &[word(&step["cell"])],
                            Some(word(&row["thread"])),
                            |m| {
                                push_operand(
                                    m,
                                    word(&row["thread"]),
                                    word(&step["cell"]),
                                    &mut Bump,
                                )
                            },
                        )
                        .unwrap();
                        (registers.map(|r| stack::reg(m, r)), stack::sp(m))
                    })
                } else {
                    push_operand(&mut m, word(&row["thread"]), word(&step["cell"]), &mut Bump)
                        .unwrap();
                    (registers.map(|r| stack::reg(&m, r)), stack::sp(&m))
                };
                let expected = bytes(&step["heap"]);
                assert_eq!(
                    expected.len(),
                    (word(&step["heap_ptr"]) - 0x20000000) as usize
                );
                assert_eq!(m.heap_ptr(), word(&step["heap_ptr"]));
                assert_eq!(
                    (0..expected.len())
                        .map(|i| m.r8(0x20000000 + i as u32))
                        .collect::<Vec<_>>(),
                    expected,
                    "complete heap at step{index}"
                );
                let expected_stack = bytes(&step["stack"]);
                assert_eq!(expected_stack.len(), 0x180);
                assert_eq!(
                    (0..0x180).map(|i| m.r8(stack_base + i)).collect::<Vec<_>>(),
                    expected_stack,
                    "all retained stack bytes at step{index}"
                );
                for (actual, name) in actual_regs
                    .into_iter()
                    .zip(["eax", "ebx", "ecx", "edx", "esi", "edi", "ebp"])
                {
                    assert_eq!(actual, word(&step["registers"][name]), "{name} step{index}");
                }
                assert_eq!(actual_sp, word(&step["registers"]["esp"]));
                let stores = step["stores"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|s| {
                        assert_eq!(s["size"], 4);
                        (word(&s["address"]), word(&s["value"]))
                    })
                    .collect::<Vec<_>>();
                STORES.with(|s| {
                    assert_eq!(
                        *s.borrow(),
                        stores,
                        "ordered original heap stores at step{index}"
                    )
                });
            }
        }
    }
    #[test]
    fn missing_context_preserves_existing_parent_state() {
        let mut m = Space::new();
        stack::function(&mut m, &[], None, |m| {
            stack::push(m, 0x12345678);
            let sp = stack::sp(m);
            let regs = [
                Reg::Eax,
                Reg::Ebx,
                Reg::Ecx,
                Reg::Edx,
                Reg::Esi,
                Reg::Edi,
                Reg::Ebp,
            ]
            .map(|r| stack::reg(m, r));
            assert!(push_operand(m, 0, 0x20000000, &mut Bump).is_err());
            assert_eq!(stack::sp(m), sp);
            assert_eq!(m.r32(sp), 0x12345678);
            assert_eq!(
                [
                    Reg::Eax,
                    Reg::Ebx,
                    Reg::Ecx,
                    Reg::Edx,
                    Reg::Esi,
                    Reg::Edi,
                    Reg::Ebp
                ]
                .map(|r| stack::reg(m, r)),
                regs
            );
        });
    }
}

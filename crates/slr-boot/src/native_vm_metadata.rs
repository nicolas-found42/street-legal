//! Original raw Thread/frame construction. Callers supply actual JVM, name
//! and object allocations; semantic IDs do not stand in for native pointers.
use crate::native_stack::{self as stack, Reg};
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;
#[path = "native_vm_class.rs"]
pub mod classes;
#[path = "native_vm_constants.rs"]
pub mod constants;
#[path = "native_vm_loader.rs"]
pub mod loader;
#[path = "native_vm_names.rs"]
pub mod names;
#[path = "native_vm_registration.rs"]
pub mod registration;
// Unwired raw-runtime variable pool (#110): oracle-tested, not yet called by
// production. Remove the allow when the raw Runtime is wired.
#[cfg_attr(not(test), allow(dead_code))]
#[path = "native_vm_variables.rs"]
pub mod variables;

#[derive(Clone, Copy, Debug)]
pub struct ThreadInit {
    pub jvm: u32,
    pub priority: u32,
    pub flags: u32,
    pub name: u32,
}
#[derive(Clone, Copy, Debug)]
pub struct FrameInit {
    pub thread: u32,
    pub context: u32,
    pub pc: u32,
    pub receiver: u32,
    pub locals: u32,
}

fn set(m: &mut Space, r: Reg, v: u32) {
    stack::set_reg(m, r, v);
}
fn write(m: &mut Space, a: u32, v: u32) {
    #[cfg(test)]
    crate::native_vm_test_store::observe(a, 4, v);
    m.w32(a, v);
}
fn malloc(m: &mut Space, pc: u32, bytes: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| a.allocate(m, bytes))?;
    set(m, Reg::Eax, p);
    Ok(p)
}
fn realloc(
    m: &mut Space,
    pc: u32,
    old: u32,
    old_bytes: u32,
    bytes: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    let p = stack::external(m, pc, &[], None, |m| a.reallocate(m, old, old_bytes, bytes))?;
    set(m, Reg::Eax, p);
    Ok(p)
}

//551120 ->5548c0, with original byte copy, CL writes and CALL/push history.
pub(crate) fn name_copy(m: &mut Space, dst: u32, src: u32, length: u32) {
    stack::function_cdecl(m, &[], |m| {
        set(m, Reg::Eax, length);
        set(m, Reg::Ecx, src);
        set(m, Reg::Edx, dst);
        stack::call(m, 0x551134, &[dst, src, length], None, |m| {
            stack::function_cdecl(m, &[], |m| {
                set(m, Reg::Eax, dst);
                stack::push_reg(m, Reg::Ebx);
                stack::push_reg(m, Reg::Ebp);
                stack::push_reg(m, Reg::Esi);
                set(m, Reg::Esi, src);
                let mut ch = m.r8(src);
                set(m, Reg::Ecx, (stack::reg(m, Reg::Ecx) & !255) | ch as u32);
                stack::push_reg(m, Reg::Edi);
                set(m, Reg::Edi, 1);
                set(m, Reg::Edx, dst);
                let mut offset = 0;
                if ch != 0 {
                    set(m, Reg::Ebx, length);
                    loop {
                        let edi = stack::reg(m, Reg::Edi);
                        set(m, Reg::Ebp, edi);
                        set(m, Reg::Edi, edi + 1);
                        if edi as i32 >= length as i32 {
                            break;
                        }
                        #[cfg(test)]
                        crate::native_vm_test_store::observe(
                            dst + offset,
                            1,
                            stack::reg(m, Reg::Ecx),
                        );
                        m.w8(dst + offset, ch);
                        offset += 1;
                        ch = m.r8(src + offset);
                        set(m, Reg::Ecx, (stack::reg(m, Reg::Ecx) & !255) | ch as u32);
                        set(m, Reg::Edx, dst + offset);
                        set(m, Reg::Esi, src + offset);
                        if ch == 0 {
                            break;
                        }
                    }
                }
                stack::add_sp(m, 12); // POP EDI/ESI/EBP, then trailing NUL.
                #[cfg(test)]
                crate::native_vm_test_store::observe(dst + offset, 1, 0);
                m.w8(dst + offset, 0);
                stack::add_sp(m, 4);
            });
        });
    });
}

/// exe: 0x407b10
#[allow(dead_code)]
pub fn construct_frame(
    m: &mut Space,
    this: u32,
    input: FrameInit,
    a: &mut impl crate::native_vm_context::Environment,
) -> Result<u32, String> {
    if this == 0 || input.thread == 0 || input.locals > i32::MAX as u32 / 4 {
        return Err("NativeVmMetadata: unresolved frame identity or invalid locals".into());
    }
    let args = [
        input.thread,
        input.context,
        input.pc,
        input.receiver,
        input.locals,
    ];
    stack::function(m, &args, Some(this), |m| {
        stack::push_reg(m, Reg::Ebx);
        set(m, Reg::Ebx, input.locals);
        stack::push_reg(m, Reg::Ebp);
        set(m, Reg::Ebp, 0);
        stack::push_reg(m, Reg::Esi);
        set(m, Reg::Esi, this);
        stack::push_reg(m, Reg::Edi);
        let local = this + 12;
        set(m, Reg::Edi, local);
        for (addr, value) in [
            (this, 0x5e7414),
            (this + 8, 0),
            (this + 4, 0),
            (local, 0),
            (local + 8, 0),
            (local + 4, 0),
        ] {
            write(m, addr, value);
        }
        if input.locals == 0 {
            stack::push(m, 0);
            stack::external(m, 0x407b3d, &[], None, |m| a.deallocate(m, 0))?;
            stack::add_sp(m, 4);
            write(m, local + 4, 0);
        } else {
            set(m, Reg::Eax, input.locals * 4);
            stack::push(m, input.locals * 4);
            stack::push(m, 0);
            let p = realloc(m, 0x407b53, 0, 0, input.locals * 4, a)?;
            stack::add_sp(m, 8);
            write(m, local + 4, p);
        }
        write(m, local, input.locals);
        set(m, Reg::Ecx, input.pc);
        set(m, Reg::Edx, input.context);
        write(m, this + 0x1c, 0);
        write(m, this + 0x24, 0);
        write(m, this + 0x20, 0);
        set(m, Reg::Eax, this + 0x1c);
        write(m, this + 0x28, this + 0x1c);
        set(m, Reg::Eax, input.thread);
        write(m, this + 0x2c, input.pc);
        set(m, Reg::Ecx, input.receiver);
        write(m, this + 0x18, local);
        stack::add_sp(m, 4);
        write(m, this + 0x3c, input.thread);
        write(m, this, 0x5e7410);
        write(m, this + 0x34, input.context);
        write(m, this + 0x30, input.receiver);
        set(m, Reg::Eax, this);
        stack::add_sp(m, 12);
        a.observe_owner(
            m,
            crate::native_vm_context::Observation::FrameConstructed {
                address: this,
                input,
            },
        )?;
        Ok(this)
    })
}

/// Raw ordinary-success Thread constructor. Nonempty source names are copied
/// through551120/5548c0; frame storage comes from the actual global256×64 pool.
/// Allocation failure and invalid source lists remain explicit boundaries.
/// exe: 0x41f340
#[allow(dead_code)]
pub fn construct_thread(
    m: &mut Space,
    this: u32,
    input: ThreadInit,
    a: &mut impl crate::native_vm_context::Environment,
) -> Result<u32, String> {
    let pool = m.r32(0x62ddf0);
    let owner = m.r32(input.jvm.wrapping_add(0x18));
    if this == 0
        || input.jvm == 0
        || input.name == 0
        || m.r32(input.jvm + 4) == 0
        || owner == 0
        || m.r32(owner + 0x18) == 0
        || pool == 0
        || m.r32(pool + 8) > m.r32(pool + 12)
        || m.r32(pool + 12) > i32::MAX as u32 - 256
    {
        return Err("NativeVmMetadata: unresolved JVM/list/frame-pool/thread/name context".into());
    }
    let length = (0..0x10000)
        .find(|i| m.r8(input.name + i) == 0)
        .ok_or("NativeVmMetadata: unterminated source thread name")?
        + 1;
    stack::function(
        m,
        &[input.jvm, input.priority, input.flags, input.name],
        Some(this),
        |m| {
            a.observe_owner(
                m,
                crate::native_vm_context::Observation::ThreadBegin {
                    address: this,
                    input,
                },
            )?;
            set(m, Reg::Eax, input.jvm);
            set(m, Reg::Edx, input.flags);
            stack::push_reg(m, Reg::Ebx);
            set(m, Reg::Ebx, input.name);
            stack::push_reg(m, Reg::Ebp);
            stack::push_reg(m, Reg::Esi);
            set(m, Reg::Esi, this);
            set(m, Reg::Ecx, input.priority);
            set(m, Reg::Ebp, 0);
            for (off, v) in [
                (0x28, input.priority),
                (0x10, input.jvm),
                (8, 0),
                (4, 0),
                (0, 0x5f098c),
                (0x2c, input.flags),
            ] {
                write(m, this + off, v);
            }
            set(m, Reg::Eax, m.r32(input.jvm + 4));
            stack::push_reg(m, Reg::Edi);
            write(m, this + 0x14, m.r32(input.jvm + 4));
            set(m, Reg::Ecx, u32::MAX);
            set(m, Reg::Eax, 0);
            write(m, this + 0x24, 0);
            write(m, this + 0x18, 0);
            set(m, Reg::Edi, input.name + length);
            set(m, Reg::Ecx, length);
            stack::push(m, length);
            let name = malloc(m, 0x41f38a, length, a)?;
            if name == 0 {
                return Err("NativeVmMetadata: thread name allocation failed".into());
            }
            set(m, Reg::Edx, name);
            set(m, Reg::Ecx, u32::MAX);
            set(m, Reg::Eax, 0);
            write(m, this + 0x1c, name);
            set(m, Reg::Edi, input.name + length);
            set(m, Reg::Ecx, length);
            stack::push(m, length);
            stack::push(m, input.name);
            stack::push(m, name);
            stack::call(m, 0x41f3a2, &[], None, |m| {
                name_copy(m, name, input.name, length)
            });
            stack::push(m, 0x1c);
            let list = malloc(m, 0x41f3a9, 0x1c, a)?;
            stack::add_sp(m, 0x14);
            if list == 0 {
                return Err("NativeVmMetadata: frame list allocation failed".into());
            }
            set(m, Reg::Ecx, list + 4);
            for (off, v) in [(4, 0x5e7414), (12, 0), (8, 0)] {
                write(m, list + off, v);
            }
            set(m, Reg::Edx, list + 0x10);
            for (off, v) in [
                (0x10, 0x5e7414),
                (0x18, 0),
                (0x14, 0),
                (8, list + 0x10),
                (0x18, list + 4),
                (0, 0x5f0988),
            ] {
                write(m, list + off, v);
            }
            write(m, this + 12, list);
            set(m, Reg::Edi, pool);
            let mut frame = m.r32(pool + 16);
            set(m, Reg::Ecx, frame);
            if frame != 0 {
                set(m, Reg::Edx, m.r32(frame));
                write(m, pool + 16, m.r32(frame));
            } else {
                let handed = m.r32(pool + 8);
                let capacity = m.r32(pool + 12);
                set(m, Reg::Eax, capacity);
                if handed == capacity {
                    let blocks = m.r32(pool + 4);
                    let table = m.r32(pool);
                    set(m, Reg::Ebx, blocks);
                    set(m, Reg::Ecx, table);
                    set(m, Reg::Eax, capacity + 256);
                    write(m, pool + 12, capacity + 256);
                    set(m, Reg::Eax, blocks + 1);
                    write(m, pool + 4, blocks + 1);
                    set(m, Reg::Eax, (blocks + 1) * 4);
                    stack::push(m, (blocks + 1) * 4);
                    stack::push(m, table);
                    let table = realloc(m, 0x41f41a, table, blocks * 4, (blocks + 1) * 4, a)?;
                    stack::push(m, 0x4000);
                    write(m, pool, table);
                    let block = malloc(m, 0x41f426, 0x4000, a)?;
                    set(m, Reg::Edx, table);
                    stack::add_sp(m, 12);
                    write(m, table + blocks * 4, block);
                }
                set(m, Reg::Ecx, handed);
                set(m, Reg::Eax, handed + 1);
                write(m, pool + 8, handed + 1);
                set(m, Reg::Eax, handed / 256);
                let table = m.r32(pool);
                set(m, Reg::Edx, table);
                set(m, Reg::Edi, (handed % 256) * 64);
                frame = m.r32(table + (handed / 256) * 4) + (handed % 256) * 64;
                set(m, Reg::Ecx, frame);
            }
            if frame == 0 {
                return Err("NativeVmMetadata: null frame allocation".into());
            }
            stack::call(m, 0x41f46f, &[this, 0, 0, 0, 0], Some(frame), |m| {
                construct_frame(
                    m,
                    frame,
                    FrameInit {
                        thread: this,
                        context: 0,
                        pc: 0,
                        receiver: 0,
                        locals: 0,
                    },
                    a,
                )
            })?;
            set(m, Reg::Ecx, list);
            let tail = m.r32(list + 8);
            set(m, Reg::Edx, tail);
            write(m, frame + 4, tail);
            set(m, Reg::Edx, m.r32(tail + 8));
            write(m, frame + 8, m.r32(tail + 8));
            set(m, Reg::Edx, tail);
            write(m, tail + 8, frame);
            write(m, list + 8, frame);
            set(m, Reg::Eax, list);
            set(m, Reg::Ecx, frame);
            set(m, Reg::Eax, frame);
            write(m, this + 0x20, frame);
            set(m, Reg::Eax, owner);
            let head = m.r32(owner + 0x18);
            set(m, Reg::Ecx, head);
            let prev = m.r32(head + 4);
            set(m, Reg::Edx, prev);
            write(m, this + 4, prev);
            set(m, Reg::Ecx, head);
            write(m, this + 8, head);
            set(m, Reg::Edx, head);
            write(m, head + 4, this);
            write(m, owner + 0x18, this);
            stack::add_sp(m, 4);
            set(m, Reg::Eax, this);
            stack::add_sp(m, 12);
            a.observe_owner(
                m,
                crate::native_vm_context::Observation::ThreadCommitted { address: this },
            )?;
            Ok(this)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    struct Bump;
    impl crate::native_vm_context::Environment for Bump {}
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
    fn original_thread_and_frame_constructors_match_entire_heap_and_retained_stack() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) =
            crate::oracle_fixture::file(&root.join("re/out/boot/native-vm-metadata-original.json"))
        else {
            return;
        };
        let v: Value = serde_json::from_slice(
            &std::fs::read(fixture_path).expect("run native_vm_metadata_probe.py"),
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
                "native_vm_metadata_probe.py",
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
        assert_eq!(rows.len(), 32);
        let threads = rows
            .iter()
            .filter(|r| r["kind"] == "thread")
            .map(|r| {
                (
                    r["shape"].as_str().unwrap(),
                    r["name"].as_str().unwrap(),
                    word(&r["seed"]),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            threads,
            ["empty", "existing", "grow", "freelist"]
                .into_iter()
                .flat_map(|shape| ["", "thread", "worker.addpart"]
                    .into_iter()
                    .flat_map(move |name| [606, 20261022].map(move |seed| (shape, name, seed))))
                .collect()
        );
        let frames = rows
            .iter()
            .filter(|r| r["kind"] == "frame")
            .map(|r| (word(&r["shape"]), word(&r["seed"])))
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            frames,
            [0, 1, 3, 9]
                .into_iter()
                .flat_map(|locals| [606, 20261022].map(move |seed| (locals, seed)))
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
            m.w32(0x62ddf0, word(&row["frame_pool"]));
            let base = word(&row["stack_base"]);
            let scratch = bytes(&row["before"]["stack"]);
            assert_eq!(scratch.len(), 0x400);
            for (i, b) in scratch.into_iter().enumerate() {
                m.w8(base + i as u32, b);
            }
            let args = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(word)
                .collect::<Vec<_>>();
            if row["kind"] == "thread" {
                assert_eq!(args.len(), 4);
                construct_thread(
                    &mut m,
                    word(&row["this"]),
                    ThreadInit {
                        jvm: args[0],
                        priority: args[1],
                        flags: args[2],
                        name: args[3],
                    },
                    &mut Bump,
                )
                .unwrap();
            } else {
                assert_eq!(row["kind"], "frame");
                assert_eq!(args.len(), 5);
                construct_frame(
                    &mut m,
                    word(&row["this"]),
                    FrameInit {
                        thread: args[0],
                        context: args[1],
                        pc: args[2],
                        receiver: args[3],
                        locals: args[4],
                    },
                    &mut Bump,
                )
                .unwrap();
            }
            let label = format!(
                "{} shape={} name={} seed={}",
                row["kind"], row["shape"], row["name"], row["seed"]
            );
            let expected = bytes(&row["after"]["heap"]);
            assert_eq!(
                expected.len(),
                (word(&row["after"]["heap_ptr"]) - 0x20000000) as usize
            );
            assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]), "{label}");
            let actual = (0..expected.len())
                .map(|i| m.r8(0x20000000 + i as u32))
                .collect::<Vec<_>>();
            let diffs = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .map(|(i, (a, b))| (0x20000000 + i as u32, *a, *b))
                .collect::<Vec<_>>();
            assert!(diffs.is_empty(), "complete heap {label}: {diffs:x?}");
            let expected = bytes(&row["after"]["stack"]);
            assert_eq!(expected.len(), 0x400);
            let actual = (0..0x400).map(|i| m.r8(base + i)).collect::<Vec<_>>();
            let diffs = actual
                .iter()
                .zip(&expected)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(12)
                .map(|(i, (a, b))| (base + i as u32, *a, *b))
                .collect::<Vec<_>>();
            assert!(
                diffs.is_empty(),
                "complete retained stack {label}: {diffs:x?}"
            );
            for (r, name) in [
                (Reg::Eax, "eax"),
                (Reg::Ebx, "ebx"),
                (Reg::Ecx, "ecx"),
                (Reg::Edx, "edx"),
                (Reg::Esi, "esi"),
                (Reg::Edi, "edi"),
                (Reg::Ebp, "ebp"),
            ] {
                assert_eq!(
                    stack::reg(&m, r),
                    word(&row["after"]["registers"][name]),
                    "{name} {label}"
                );
            }
            assert_eq!(
                stack::sp(&m),
                word(&row["after"]["registers"]["esp"]),
                "{label}"
            );
        }
    }
    #[test]
    fn missing_metadata_rejects_before_touching_parent_stack_or_heap() {
        let mut m = Space::new();
        stack::function(&mut m, &[], None, |m| {
            stack::push(m, 0x12345678);
            let esp = stack::sp(m);
            let heap = m.heap_ptr();
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
            assert!(construct_thread(
                m,
                0x20000000,
                ThreadInit {
                    jvm: 0,
                    priority: 0,
                    flags: 0,
                    name: 0
                },
                &mut Bump
            )
            .is_err());
            assert!(construct_frame(
                m,
                0,
                FrameInit {
                    thread: 0,
                    context: 0,
                    pc: 0,
                    receiver: 0,
                    locals: 0
                },
                &mut Bump
            )
            .is_err());
            assert_eq!(m.heap_ptr(), heap);
            assert_eq!(stack::sp(m), esp);
            assert_eq!(m.r32(esp), 0x12345678);
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

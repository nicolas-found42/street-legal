//! Source411b20 raw module allocation and chunk readers. This preserves memory
//! allocation/store chronology, including TREE completion and physical class
//! registration. It does not claim caller/native stack or register parity.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadedModule {
    pub module: u32,
    pub class: u32,
}
#[derive(Clone, Copy)]
struct Chunk {
    tag: u32,
    source: u32,
    bytes: u32,
}
#[cfg(test)]
thread_local! {static STORES:std::cell::RefCell<Vec<(u32,u32,u32)>>=const{std::cell::RefCell::new(Vec::new())};}
#[cfg(test)]
fn observe(p: u32, n: u32, v: u32) {
    STORES.with(|s| s.borrow_mut().push((p, n, v)));
}
fn write(m: &mut Space, p: u32, v: u32) {
    #[cfg(test)]
    observe(p, 4, v);
    m.w32(p, v)
}
fn half(m: &mut Space, p: u32, v: u16) {
    #[cfg(test)]
    observe(p, 2, v as u32);
    m.w8(p, v as u8);
    m.w8(p + 1, (v >> 8) as u8)
}
fn span(p: u32, n: u32, end: u32) -> Result<(), String> {
    p.checked_add(n)
        .filter(|q| *q <= end)
        .map(|_| ())
        .ok_or_else(|| "NativeVmLoader: truncated source chunk".into())
}
fn validate(m: &Space, source: u32, bytes: u32) -> Result<Vec<Chunk>, String> {
    let end = source
        .checked_add(bytes)
        .filter(|_| source != 0 && bytes <= i32::MAX as u32)
        .ok_or("NativeVmLoader: missing/overflowing serialized input identity")?;
    let mut p = source;
    let mut chunks = Vec::new();
    let mut active = false;
    while p < end {
        span(p, 8, end)?;
        let tag = m.r32(p);
        let n = m.r32(p + 4);
        p += 8;
        span(p, n, end)?;
        let ce = p + n;
        match tag {
            0x41465554 => {
                if n != 4 || m.r32(p) != 0x11ae1 {
                    return Err("NativeVmLoader: unsupported TUFA compiler version".into());
                }
            }
            0x534e4f43 => {
                if active {
                    return Err("NativeVmLoader: CONS before preceding TREE completion".into());
                }
                super::constants::validate(m, p, n)?;
                active = true;
            }
            0x444c4946 | 0x4448544d => {
                if !active {
                    return Err("NativeVmLoader: members without live raw module".into());
                }
                let stride = if tag == 0x444c4946 { 16 } else { 20 };
                let mut q = p;
                for _ in 0..2 {
                    span(q, 4, ce)?;
                    let count = m.r32(q);
                    q += 4;
                    let size = count
                        .checked_mul(stride)
                        .ok_or("NativeVmLoader: member array overflow")?;
                    span(q, size, ce)?;
                    q += size;
                }
                if q != ce {
                    return Err("NativeVmLoader: trailing member bytes".into());
                }
            }
            0x53534c43 => {
                if !active {
                    return Err("NativeVmLoader: CLSS without live raw module".into());
                }
                span(p, 20, ce)?;
                let tails = m
                    .r32(p + 16)
                    .checked_mul(4)
                    .ok_or("NativeVmLoader: interface array overflow")?;
                if tails != n - 20 {
                    return Err("NativeVmLoader: invalid CLSS tail span".into());
                }
            }
            0x45455254 => {
                if !active {
                    return Err("NativeVmLoader: TREE without live raw module".into());
                }
                span(p, 4, ce)?;
                let count = m.r32(p);
                let mut q = p + 4;
                for _ in 0..count {
                    span(q, 4, ce)?;
                    let nodes = m.r32(q);
                    q += 4;
                    for _ in 0..nodes {
                        span(q, 3, ce)?;
                        let opcode = m.r8(q) as usize;
                        let op = *slr_fmt_tufa::OPWORDS
                            .get(opcode)
                            .ok_or("NativeVmLoader: opcode outside original admitted table")?;
                        let size = if matches!(op & 0xf000, 0x1000 | 0x2000 | 0x4000) {
                            7
                        } else {
                            3
                        };
                        span(q, size, ce)?;
                        q += size;
                    }
                }
                if q != ce {
                    return Err("NativeVmLoader: trailing TREE bytes".into());
                }
                active = false;
            }
            _ => return Err(format!("NativeVmLoader: unsupported chunk {tag:08x}")),
        }
        chunks.push(Chunk {
            tag,
            source: p,
            bytes: n,
        });
        p = ce;
    }
    if active {
        return Err("NativeVmLoader: raw module missing TREE completion".into());
    }
    Ok(chunks)
}
fn constant_pool(m: &mut Space, p: u32) {
    for (off, v) in [(8, 0), (16, 0), (12, 0), (4, 0x5e736c), (0, 0x5e735c)] {
        write(m, p + off, v)
    }
}
fn container(m: &mut Space, p: u32) {
    for off in [0, 8, 4] {
        write(m, p + off, 0)
    }
}
fn list(m: &mut Space, p: u32, method: bool, first: bool) {
    if first {
        container(m, p + 4)
    } else {
        for off in [4, 12, 8] {
            write(m, p + off, 0)
        }
    }
    write(m, p, if method { 0x5f08a8 } else { 0x5f08b8 })
}
fn allocate(m: &mut Space, n: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = a.allocate(m, n)?;
    if p == 0 {
        Err(format!(
            "NativeVmLoader: original allocation failed ({n} bytes)"
        ))
    } else {
        Ok(p)
    }
}
fn module(m: &mut Space, jvm: u32, a: &mut impl Allocator) -> Result<u32, String> {
    let p = allocate(m, 100, a)?;
    write(m, p, 0x5f08a4);
    write(m, p + 8, 0);
    write(m, p + 4, 0);
    constant_pool(m, p + 16);
    write(m, p, 0x5f08a0);
    write(m, p + 12, jvm);
    for (off, n, method, first) in [
        (0x50, 16, true, true),
        (0x4c, 16, false, false),
        (0x58, 16, true, false),
        (0x54, 16, false, false),
        (0x5c, 12, false, false),
        (0x60, 12, false, false),
    ] {
        let q = allocate(m, n, a)?;
        if n == 12 {
            container(m, q)
        } else {
            list(m, q, method, first)
        }
        write(m, p + off, q);
    }
    let pool = allocate(m, 20, a)?;
    constant_pool(m, pool);
    write(m, p + 0x24, pool);
    for off in [0x28, 0x34, 0x48, 0x30, 0x34, 0x28] {
        write(m, p + off, 0)
    }
    Ok(p)
}
fn members(
    m: &mut Space,
    this: u32,
    mut source: u32,
    method: bool,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    //419b60/419d00: fresh source-constructed lists, free(NULL) collaborator.
    if m.r32(this + 8) != 0 || m.r32(this + 12) != 0 {
        return Err("NativeVmLoader: repeated member-list destruction not yet projected".into());
    }
    write(m, this + 8, 0);
    write(m, this + 4, 0);
    let count = m.r32(source);
    source += 4;
    if count == 0 {
        write(m, this + 8, 0)
    } else {
        let table = a.reallocate(m, 0, 0, count * 4)?;
        write(m, this + 8, table)
    }
    write(m, this + 4, count);
    write(m, this + 12, count);
    for i in 0..count {
        let record = allocate(m, if method { 32 } else { 20 }, a)?;
        if method {
            write(m, record + 24, 0);
            write(m, record + 28, 0)
        }
        write(m, record, if method { 0x5f08d0 } else { 0x5f0890 });
        write(m, m.r32(this + 8) + i * 4, record);
        for j in 0..if method { 5 } else { 4 } {
            write(m, record + 4 + j * 4, m.r32(source + j * 4))
        }
        source += if method { 20 } else { 16 };
    }
    Ok(source)
}
fn append(m: &mut Space, c: u32, value: u32, a: &mut impl Allocator) -> Result<(), String> {
    let cap = m.r32(c);
    let len = m.r32(c + 8);
    if cap < len + 1 {
        let nc = cap + len + 1;
        let p = a.reallocate(m, m.r32(c + 4), cap * 4, nc * 4)?;
        write(m, c + 4, p);
        if nc < len {
            write(m, c + 8, nc)
        }
        write(m, c, nc)
    }
    write(m, m.r32(c + 4) + m.r32(c + 8) * 4, value);
    write(m, c + 8, m.r32(c + 8) + 1);
    Ok(())
}
/// exe:0x411b20. `source` is the actual loader file allocation, reused across
/// concatenated modules; `bytes` is its exact remaining byte count. `scratch`
/// identifies403a90's actual caller stack local, not a new heap allocation.
#[allow(dead_code)]
pub fn load_memory(
    m: &mut Space,
    jvm: u32,
    source: u32,
    bytes: u32,
    scratch: u32,
    a: &mut impl Allocator,
) -> Result<Vec<LoadedModule>, String> {
    let chunks = validate(m, source, bytes)?;
    if jvm == 0 || scratch == 0 {
        return Err("NativeVmLoader: absent JVM/caller scratch identity".into());
    }
    let mut current = 0;
    let mut loaded = Vec::new();
    for ch in chunks {
        match ch.tag {
            0x41465554 => {}
            0x534e4f43 => {
                current = module(m, jvm, a)?;
                super::constants::deserialize_memory(
                    m,
                    m.r32(current + 0x24),
                    ch.source,
                    ch.bytes,
                    a,
                )?;
            }
            0x4448544d | 0x444c4946 => {
                let (first, second, method) = if ch.tag == 0x4448544d {
                    (0x58, 0x50, true)
                } else {
                    (0x54, 0x4c, false)
                };
                let next = members(m, m.r32(current + first), ch.source, method, a)?;
                members(m, m.r32(current + second), next, method, a)?;
            }
            0x53534c43 => {
                for (i, off) in [0x2c, 0x38, 0x3c, 0x40, 0x44].into_iter().enumerate() {
                    write(m, current + off, m.r32(ch.source + i as u32 * 4))
                }
                let count = m.r32(current + 0x44);
                let table = if count == 0 {
                    0
                } else {
                    allocate(m, count * 4, a)?
                };
                write(m, current + 0x48, table);
                for i in 0..count {
                    write(m, table + i * 4, m.r32(ch.source + 20 + i * 4))
                }
            }
            0x45455254 => {
                let mut p = ch.source + 4;
                for _ in 0..m.r32(ch.source) {
                    let n = m.r32(p);
                    p += 4;
                    let body = if n == 0 { 0 } else { allocate(m, n * 8, a)? };
                    for i in 0..n {
                        let op = slr_fmt_tufa::OPWORDS[m.r8(p) as usize];
                        half(m, body + i * 8, op);
                        let operand = u16::from_le_bytes([m.r8(p + 1), m.r8(p + 2)]);
                        half(m, body + i * 8 + 2, operand);
                        let imm = matches!(op & 0xf000, 0x1000 | 0x2000 | 0x4000);
                        if imm {
                            write(m, body + i * 8 + 4, m.r32(p + 3))
                        }
                        p += if imm { 7 } else { 3 };
                    }
                    append(m, m.r32(current + 0x60), body, a)?;
                }
                let pool = m.r32(current + 0x24);
                let record = m.r32(m.r32(pool + 12) + m.r32(current + 0x3c) * 4);
                if m.r32(record + 4) != 4 {
                    return Err(
                        "NativeVmLoader: TREE Class selector is not raw Class record".into(),
                    );
                }
                let string = m.r32(m.r32(pool + 12) + m.r32(record + 8) * 4);
                if m.r32(string + 4) != 0 {
                    return Err("NativeVmLoader: TREE Class selector is not raw string".into());
                }
                write(m, current + 0x30, m.r32(string + 8));
                let class = allocate(m, 0x1d8, a)?;
                super::classes::register_memory(m, class, jvm, current, scratch, a)?;
                loaded.push(LoadedModule {
                    module: current,
                    class,
                });
                current = 0;
            }
            _ => unreachable!(),
        }
    }
    Ok(loaded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use sha2::{Digest, Sha256};
    struct Bump;
    impl Allocator for Bump {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            Ok(m.alloc(n.max(1)))
        }
        fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
            let q = m.alloc(n.max(1));
            for i in 0..old.min(n) {
                m.w8(q + i, m.r8(p + i))
            }
            Ok(q)
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
    struct Observer;
    impl Drop for Observer {
        fn drop(&mut self) {
            super::super::names::set_test_store_observer(None);
            super::super::constants::set_test_store_observer(None);
            super::super::classes::set_test_store_observer(None)
        }
    }
    struct CausalHost {
        memory: crate::native_vm_live::FileMemory,
    }
    impl crate::native_vm_pool::Allocator for CausalHost {
        fn allocate(&mut self, m: &mut Space, n: u32) -> Result<u32, String> {
            Bump.allocate(m, n)
        }
        fn reallocate(&mut self, m: &mut Space, p: u32, old: u32, n: u32) -> Result<u32, String> {
            Bump.reallocate(m, p, old, n)
        }
    }
    impl crate::native_vm_live::Host for CausalHost {
        fn file_memory(
            &mut self,
            _m: &Space,
            _identity: u64,
            _bytes: &[u8],
        ) -> Result<crate::native_vm_live::FileMemory, String> {
            Ok(self.memory)
        }
    }
    #[test]
    fn original_loader_complete_heap_and_ordered_stores() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let Some(fixture_path) = crate::oracle_fixture::file(
            &root.join("re/out/checkpoints/abi-reconstruction-20261023/loader-original-v1.json"),
        ) else {
            return;
        };
        let v: Value = serde_json::from_slice(&std::fs::read(fixture_path).unwrap()).unwrap();
        assert_eq!(v["schema"], 1);
        assert_eq!(v["fpu_cw"], 127);
        assert_eq!(
            v["exe_sha256"],
            format!(
                "{:x}",
                Sha256::digest(
                    std::fs::read(root.join("reference/raw/StreetLegal_Redline.bak.exe")).unwrap()
                )
            )
        );
        assert_eq!(
            v["opwords"]
                .as_array()
                .unwrap()
                .iter()
                .map(word)
                .collect::<Vec<_>>(),
            slr_fmt_tufa::OPWORDS
                .iter()
                .map(|v| *v as u32)
                .collect::<Vec<_>>()
        );
        for name in [
            "native_vm_loader_probe.py",
            "oracle.py",
            "thread_vectors.py",
        ] {
            assert_eq!(
                v["producer_sources"][name],
                format!(
                    "{:x}",
                    Sha256::digest(std::fs::read(root.join("re/oracle").join(name)).unwrap())
                )
            )
        }
        let rows = v["rows"].as_array().unwrap();
        assert_eq!(rows.len(), 10);
        let inventory = rows
            .iter()
            .map(|r| {
                (
                    r["label"].as_str().unwrap(),
                    word(&r["seed"]),
                    r["modules"].as_array().unwrap().len(),
                )
            })
            .collect::<std::collections::HashSet<_>>();
        assert_eq!(
            inventory,
            [
                ("Input", 1),
                ("Osd", 1),
                ("String", 1),
                ("concatenated_Input_Osd", 2),
                ("synthetic_member_growth_all_nodes", 1)
            ]
            .into_iter()
            .flat_map(|(n, c)| [606, 20261023].map(move |seed| (n, seed, c)))
            .collect()
        );
        for row in rows {
            let before = bytes(&row["before"]["bytes"]);
            assert_eq!(
                before.len(),
                (word(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            let declared_files: &[&str] = match row["label"].as_str().unwrap() {
                "Input" => &["system/scripts/io/Input.class"],
                "Osd" => &["system/scripts/render/Osd.class"],
                "String" => &["system/scripts/lang/String.class"],
                "concatenated_Input_Osd" => &[
                    "system/scripts/io/Input.class",
                    "system/scripts/render/Osd.class",
                ],
                "synthetic_member_growth_all_nodes" => &[],
                _ => panic!("undeclared loader case"),
            };
            assert_eq!(
                row["stock"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|f| f["path"].as_str().unwrap())
                    .collect::<Vec<_>>(),
                declared_files
            );
            let mut consumed = 0;
            for (file, path) in row["stock"].as_array().unwrap().iter().zip(declared_files) {
                let raw = std::fs::read(root.join("stock").join(path)).unwrap();
                assert_eq!(file["sha256"], format!("{:x}", Sha256::digest(&raw)));
                assert_eq!(word(&file["offset"]) as usize, consumed);
                assert_eq!(word(&file["bytes"]) as usize, raw.len());
                let offset = (word(&row["source"]) - 0x20000000) as usize + consumed;
                assert_eq!(&before[offset..offset + raw.len()], raw.as_slice());
                consumed += raw.len();
            }
            if !declared_files.is_empty() {
                assert_eq!(word(&row["source_bytes"]) as usize, consumed)
            }
            let mut m = Space::new();
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b)
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            if row["label"] == "synthetic_member_growth_all_nodes" {
                let chunks =
                    validate(&m, word(&row["source"]), word(&row["source_bytes"])).unwrap();
                assert_eq!(
                    chunks.iter().map(|c| c.tag).collect::<Vec<_>>(),
                    [0x41465554, 0x534e4f43, 0x444c4946, 0x4448544d, 0x53534c43, 0x45455254]
                );
                for (index, stride) in [(2, 16), (3, 20)] {
                    let ch = chunks[index];
                    assert_eq!(m.r32(ch.source), 70);
                    assert_eq!(m.r32(ch.source + 4 + 70 * stride), 1)
                }
                let tree = chunks[5];
                assert_eq!(m.r32(tree.source), 2);
                assert_eq!(m.r32(tree.source + 4), 0);
                assert_eq!(m.r32(tree.source + 8), 47);
                let mut p = tree.source + 12;
                for i in 0..47 {
                    assert_eq!(m.r8(p), i as u8);
                    let op = slr_fmt_tufa::OPWORDS[i];
                    p += if matches!(op & 0xf000, 0x1000 | 0x2000 | 0x4000) {
                        7
                    } else {
                        3
                    };
                }
                assert_eq!(p, tree.source + tree.bytes);
            }

            // Drive the source loader through actual semantic FileRead /
            // ModuleRead / ClassRegister chronology, with explicit input and
            // caller-buffer owners. Compare the same complete original heap.
            let mut causal = m.clone();
            let mut vm = slr_vm::host::VmHost::new();
            vm.start_machine_recording().unwrap();
            let source = (0..word(&row["source_bytes"]))
                .map(|i| m.r8(word(&row["source"]) + i))
                .collect::<Vec<_>>();
            let decoded = vm.loader.load_file(&source);
            if row["label"] == "synthetic_member_growth_all_nodes" {
                assert!(matches!(
                    decoded,
                    Err(slr_vm::vmclass::LoadError::BadIndex("method body"))
                ));
                assert!(!vm
                    .machine_trace()
                    .pending(usize::MAX)
                    .iter()
                    .any(|entry| matches!(
                        entry.event,
                        slr_vm::machine::Event::ClassRegister { .. }
                    )));
            } else {
                let identities = decoded.unwrap();
                let scratch = word(&row["modules"][0]["descriptor_scratch"]);
                let mut environment = CausalHost {
                    memory: crate::native_vm_live::FileMemory {
                        source: word(&row["source"]),
                        bytes: word(&row["source_bytes"]),
                        scratch,
                    },
                };
                let mut metadata =
                    crate::native_vm_live::Metadata::new(&causal, word(&row["jvm"])).unwrap();
                let events = vm.machine_trace().pending(usize::MAX);
                let original = causal.r8(environment.memory.source);
                causal.w8(environment.memory.source, original ^ 1);
                causal.begin_journal();
                assert!(metadata
                    .apply(&mut causal, &events[0], &mut environment)
                    .unwrap_err()
                    .contains("actual raw input owner"));
                assert!(causal.end_journal().is_empty());
                causal.w8(environment.memory.source, original);
                for entry in events {
                    if let slr_vm::machine::Event::ModuleRead { file, .. } = &entry.event {
                        let mut wrong = entry.clone();
                        if let slr_vm::machine::Event::ModuleRead {
                            file: wrong_file, ..
                        } = &mut wrong.event
                        {
                            *wrong_file = file + 1_000_000;
                        }
                        causal.begin_journal();
                        assert!(metadata
                            .apply(&mut causal, &wrong, &mut environment)
                            .is_err());
                        assert!(causal.end_journal().is_empty());
                    }
                    if let slr_vm::machine::Event::ClassRegister { .. } = &entry.event {
                        let mut wrong = entry.clone();
                        if let slr_vm::machine::Event::ClassRegister { bodies, .. } =
                            &mut wrong.event
                        {
                            bodies.push(slr_vm::interp::BodyId(u32::MAX));
                        }
                        causal.begin_journal();
                        assert!(metadata
                            .apply(&mut causal, &wrong, &mut environment)
                            .unwrap_err()
                            .contains("body inventory"));
                        assert!(causal.end_journal().is_empty());
                    }
                    metadata
                        .apply(&mut causal, &entry, &mut environment)
                        .unwrap();
                }
                for ((identity, _), expected) in
                    identities.iter().zip(row["modules"].as_array().unwrap())
                {
                    assert_eq!(
                        metadata.class(identity.get()).unwrap(),
                        LoadedModule {
                            module: word(&expected["module"]),
                            class: word(&expected["class"])
                        }
                    );
                }
                for (identity, _) in &identities {
                    let raw = metadata.class(identity.get()).unwrap();
                    let tree = causal.r32(raw.module + 0x60);
                    let class = vm.loader.find_by_id(*identity).unwrap();
                    let mut selected = 0;
                    for offset in [0x58, 0x50] {
                        let container = causal.r32(raw.module + offset);
                        for index in 0..causal.r32(container + 12) {
                            let slot = causal.r32(container + 8) + index * 4;
                            let record = causal.r32(slot);
                            assert_eq!(
                                metadata.method(&causal, identity.get(), selected).unwrap(),
                                record
                            );
                            let cached = causal.r32(record + 24);
                            causal.w32(record + 24, 0x12345678);
                            assert_eq!(
                                metadata.method(&causal, identity.get(), selected).unwrap(),
                                record,
                                "runtime signature cache is not declaring-record identity"
                            );
                            causal.w32(record + 24, cached);
                            let flags = causal.r32(record + 4);
                            causal.w32(record + 4, flags ^ 1);
                            assert!(metadata.method(&causal, identity.get(), selected).is_err());
                            causal.w32(record + 4, flags);
                            causal.w32(slot, record + 4);
                            assert!(metadata.method(&causal, identity.get(), selected).is_err());
                            causal.w32(slot, record);
                            selected += 1;
                        }
                    }
                    assert_eq!(selected, class.methods.len());
                    assert!(metadata.method(&causal, identity.get(), selected).is_err());
                    for (index, nodes) in class.bodies.iter().enumerate() {
                        let body = vm.loader.body_id_of(*identity, index).unwrap();
                        let address = causal.r32(causal.r32(tree + 4) + index as u32 * 4);
                        for node in 0..nodes.len() {
                            assert_eq!(
                                metadata
                                    .pc(&causal, slr_vm::interp::Pc { body, idx: node })
                                    .unwrap(),
                                address + node as u32 * 8
                            );
                        }
                        assert!(metadata
                            .pc(
                                &causal,
                                slr_vm::interp::Pc {
                                    body,
                                    idx: nodes.len()
                                }
                            )
                            .is_err());
                    }
                }
                assert_eq!(causal.heap_ptr(), word(&row["after"]["heap_ptr"]));
                let expected = bytes(&row["after"]["bytes"]);
                assert_eq!(
                    (0..expected.len())
                        .map(|i| causal.r8(0x20000000 + i as u32))
                        .collect::<Vec<_>>(),
                    expected
                );
            }

            STORES.with(|s| s.borrow_mut().clear());
            super::super::names::set_test_store_observer(Some(observe));
            super::super::constants::set_test_store_observer(Some(observe));
            super::super::classes::set_test_store_observer(Some(observe));
            let _observer = Observer;
            let expected_modules = row["modules"].as_array().unwrap();
            let scratch = word(&expected_modules[0]["descriptor_scratch"]);
            assert!(expected_modules
                .iter()
                .all(|v| word(&v["descriptor_scratch"]) == scratch));
            let loaded = load_memory(
                &mut m,
                word(&row["jvm"]),
                word(&row["source"]),
                word(&row["source_bytes"]),
                scratch,
                &mut Bump,
            )
            .unwrap();
            assert_eq!(
                loaded,
                expected_modules
                    .iter()
                    .map(|v| LoadedModule {
                        module: word(&v["module"]),
                        class: word(&v["class"])
                    })
                    .collect::<Vec<_>>()
            );
            assert_eq!(loaded[0].class, word(&row["result"]));
            assert_eq!(m.heap_ptr(), word(&row["after"]["heap_ptr"]));
            let expected = bytes(&row["after"]["bytes"]);
            assert_eq!(expected.len(), (m.heap_ptr() - 0x20000000) as usize);
            let actual = (0..expected.len())
                .map(|i| m.r8(0x20000000 + i as u32))
                .collect::<Vec<_>>();
            let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
            assert!(
                first.is_none(),
                "complete heap {} {} first {:?} actual {:?} expected {:?}",
                row["label"],
                row["seed"],
                first,
                first.map(|i| (0x20000000 + i as u32, actual[i])),
                first.map(|i| expected[i])
            );
            let expected = row["stores"]
                .as_array()
                .unwrap()
                .iter()
                .map(|s| (word(&s["address"]), word(&s["size"]), word(&s["value"])))
                .collect::<Vec<_>>();
            STORES.with(|s| {
                let actual = s.borrow();
                assert_eq!(actual.len(), expected.len(), "store count {}", row["label"]);
                let first = actual.iter().zip(&expected).position(|(a, b)| a != b);
                assert!(
                    first.is_none(),
                    "ordered stores {} first {:?} actual {:?} expected {:?}",
                    row["label"],
                    first,
                    first.map(|i| actual[i]),
                    first.map(|i| expected[i])
                );
            });
        }
    }
    #[test]
    fn unsupported_loader_source_rejects_before_any_mutation() {
        let mut m = Space::new();
        let source = m.alloc(12);
        m.w32(source, 0x41465554);
        m.w32(source + 4, 4);
        m.w32(source + 8, 0x108f5);
        let hp = m.heap_ptr();
        STORES.with(|s| s.borrow_mut().clear());
        assert!(
            load_memory(&mut m, 0x1234, source, 12, 0x30000000, &mut Bump)
                .unwrap_err()
                .contains("compiler version")
        );
        assert_eq!(m.heap_ptr(), hp);
        assert_eq!(STORES.with(|s| s.borrow().len()), 0);
        m.w32(source, 0x12345678);
        m.w32(source + 4, 0);
        assert!(
            load_memory(&mut m, 0x1234, source, 8, 0x30000000, &mut Bump)
                .unwrap_err()
                .contains("unsupported chunk")
        );
        assert_eq!(m.heap_ptr(), hp);
    }
}

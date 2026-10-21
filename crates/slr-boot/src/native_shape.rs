//! Original505960 asynchronous SCX/SCY shape request and4fe210 storage.
//! The file manager owns actual pending reads; completion/decode remains a
//! separate original501a70/501b90 operation.
//!
//! `complete_file` is live: `native_file::pump` dispatches the `505c00`
//! callback. The remaining items are the original's main-thread pass
//! (`505cd0`/`505db0`/`505da0`) and the synchronous `505960` constructor. Both
//! need the graphics-device collaborator that creates materials and finalizes
//! objects; `slr-d3d9` does not provide one yet, so the pass has no caller and
//! stock boot issues no shape request before MainMenu. They stay compiled and
//! oracle-tested, and fail closed rather than report geometry as ready.
#![allow(dead_code)]
use slr_physics::space::Space;
pub trait Host {
    fn request_file(
        &mut self,
        m: &mut Space,
        path: &str,
        callback: u32,
        shape: u32,
    ) -> Result<i32, String>;
}
/// Original CRT initializer505460 for pending shape requests. Its atexit
/// registration is handled by the host process lifetime, separately.
pub fn initialize_pending_queue(m: &mut Space) {
    for (offset, value) in [
        (0x65bd78, 0x5f09a8),
        (0x65bd7c, 0x5f09ac),
        (0x65bd80, 0x65bd88),
        (0x65bd84, 0),
        (0x65bd88, 0x5f09ac),
        (0x65bd8c, 0),
        (0x65bd90, 0x65bd7c),
    ] {
        m.w32(offset, value);
    }
}
fn initialize_mesh(m: &mut Space, p: u32, kind: u32) {
    //505860 base, followed by4fe210 mesh source fields.
    for offset in [4, 8, 12, 16, 0x20, 0x28, 0x2c, 0x38] {
        m.w32(p + offset, 0);
    }
    m.w32(p + 0x14, u32::MAX);
    m.w32(p + 0x30, 1);
    for index in [
        0x12, 0x13, 0x14, 0x31, 0x30, 0x2f, 0x2e, 0x2c, 0x2b, 0x2a, 0x29, 0x27, 0x26, 0x25, 0x24,
        0x33, 0x34, 0x35, 0x36, 0x37, 0x38, 0x3d, 0x3e, 0x3f, 0x41, 0x42, 0x40,
    ] {
        m.w32(p + index * 4, 0);
    }
    for bounds in [0x54, 0x70] {
        m.w32(p + bounds, 2);
        for offset in (4..28).step_by(4) {
            m.w32(p + bounds + offset, 0);
        }
    }
    for index in [0x32, 0x2d, 0x28, 0x23] {
        m.w32(p + index * 4, 1f32.to_bits());
    }
    m.w32(p + 0xec, 1);
    m.w32(p + 0xf0, u32::MAX);
    m.w32(p, if kind == 3 { 0x5f2d00 } else { 0x5f2ce8 });
    if kind == 4 {
        m.w32(p + 0x110, 0);
    }
}
/// Original CRT505360 queue for file-manager saturation result-5.
pub fn initialize_retry_queue(m: &mut Space) {
    for (offset, value) in [
        (0x65c1b8, 0x5f09a8),
        (0x65c1bc, 0x5f09ac),
        (0x65c1c0, 0x65c1c8),
        (0x65c1c4, 0),
        (0x65c1c8, 0x5f09ac),
        (0x65c1cc, 0),
        (0x65c1d0, 0x65c1bc),
    ] {
        m.w32(offset, value);
    }
}
fn allocate_mesh(
    m: &mut Space,
    kind: u32,
    source: &str,
    skinned: u32,
    resource: u32,
    slot: u32,
) -> u32 {
    let p = m.alloc(if kind == 3 { 0x110 } else { 0x114 });
    initialize_mesh(m, p, kind);
    m.w32(p + 0x44, slot);
    m.w32(p + 0x3c, kind);
    m.w32(p + 0x40, resource);
    m.w32(p + 0x20, 0);
    m.w32(p + 0x24, skinned);
    for (i, b) in source.as_bytes().iter().enumerate() {
        m.w8(0x65bd94 + i as u32, *b);
    }
    m.w8(0x65bd94 + source.len() as u32, 0);
    let name = m.alloc(source.len() as u32 + 1);
    for (i, b) in source.as_bytes().iter().take(255).enumerate() {
        m.w8(name + i as u32, *b);
    }
    m.w8(name + source.len().min(255) as u32, 0);
    m.w32(p + 0x10, name);
    p
}

/// Original505960 branch with asynchronous=1. A successful return means a
/// real file request is pending, not that geometry or GPU objects are loaded.
pub fn request_async(
    m: &mut Space,
    kind: u32,
    source: &str,
    skinned: u32,
    resource: u32,
    slot: u32,
    host: &mut dyn Host,
) -> Result<u32, String> {
    if !matches!(kind, 3 | 4) {
        return Err(format!(
            "NativeShapeBoundary: request kind{kind} requires original505960 constructor"
        ));
    }
    if m.r32(0x65bd90) == 0 {
        return Err(
            "NativeShapeBoundary: pending request queue requires original505460 startup".into(),
        );
    }
    let p = allocate_mesh(m, kind, source, skinned, resource, slot);
    m.w32(p + 0x20, 1);
    let handle = host.request_file(m, source, 0x505c00, p)?;
    m.w32(p + 0x14, handle as u32);
    let (tail, sentinel, counter) = if handle >= 0 {
        (0x65bd90, 0x65bd88, 0x65c204)
    } else if handle == -5 {
        (0x65c1d0, 0x65c1c8, 0x65c200)
    } else {
        return Err(format!(
            "NativeShapeBoundary: file request result{handle} requires original shape destruction"
        ));
    };
    let previous = m.r32(tail);
    if previous == 0 {
        return Err("NativeShapeBoundary: retryqueue requires original505360 startup".into());
    }
    m.w32(previous + 4, p);
    m.w32(p + 8, previous);
    m.w32(p + 4, sentinel);
    m.w32(tail, p);
    m.w32(counter, m.r32(counter).wrapping_add(1));
    Ok(p)
}
#[cfg(test)]
mod tests {
    use super::*;
    struct File {
        calls: Vec<serde_json::Value>,
        handle: i32,
    }
    impl Host for File {
        fn request_file(
            &mut self,
            _: &mut Space,
            path: &str,
            callback: u32,
            shape: u32,
        ) -> Result<i32, String> {
            self.calls
                .push(serde_json::json!({"source":path,"args":[0,callback,shape,0]}));
            Ok(self.handle)
        }
    }
    #[test]
    fn original_async_shape_storage_file_request_and_pending_queue_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-shape-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_shape_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let source = m.alloc(64);
            for (i, b) in b"cars/example/mesh.scx\0".iter().enumerate() {
                m.w8(source + i as u32, *b);
            }
            initialize_pending_queue(&mut m);
            initialize_retry_queue(&mut m);
            let kind = row["kind"].as_u64().unwrap() as u32;
            let next = (m.heap_ptr() + 15) & !15;
            for i in 0..if kind == 3 { 0x110 } else { 0x114 } {
                m.w8(next + i, row["fill"].as_u64().unwrap() as u8);
            }
            let mut host = File {
                calls: vec![],
                handle: row["handle"].as_i64().unwrap() as i32,
            };
            let shape = request_async(
                &mut m,
                kind,
                "cars/example/mesh.scx",
                row["skinned"].as_u64().unwrap() as u32,
                0xffff1234,
                row["slot"].as_u64().unwrap() as u32,
                &mut host,
            )
            .unwrap();
            assert_eq!(shape, row["shape"].as_u64().unwrap() as u32);
            assert_eq!(m.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
            assert_eq!(serde_json::json!(host.calls), row["calls"]);
            for (i, v) in row["words"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "kind{kind} fill{} word{i}",
                    row["fill"]
                );
            }
            for (i, v) in row["queue"].as_array().unwrap().iter().enumerate() {
                assert_eq!(m.r32(0x65bd78 + i as u32 * 4), v.as_u64().unwrap() as u32);
            }
            assert_eq!(m.r32(0x65c204), row["count"].as_u64().unwrap() as u32);
            for (i, v) in row["retry_queue"].as_array().unwrap().iter().enumerate() {
                assert_eq!(m.r32(0x65c1b8 + i as u32 * 4), v.as_u64().unwrap() as u32);
            }
            assert_eq!(m.r32(0x65c200), row["retry_count"].as_u64().unwrap() as u32);
        }
    }
}
/// Original CRT505560 ready/decode queue header.
pub fn initialize_ready_queue(m: &mut Space) {
    for (offset, value) in [
        (0x65c198, 0x5f09a8),
        (0x65c19c, 0x5f09ac),
        (0x65c1a0, 0x65c1a8),
        (0x65c1a4, 0),
        (0x65c1a8, 0x5f09ac),
        (0x65c1ac, 0),
        (0x65c1b0, 0x65c19c),
    ] {
        m.w32(offset, value);
    }
}
/// Original505c00 asynchronous file completion. Bytes and the actual handle
/// result come from the file manager; no geometry is substituted here.
pub fn complete_file(
    m: &mut Space,
    shape: u32,
    result: i32,
    data: u32,
    length: u32,
) -> Result<(), String> {
    if shape == 0 || m.r32(0x65c1b0) == 0 {
        return Err(
            "NativeShapeBoundary: file completion requires live shape and readyqueue".into(),
        );
    }
    m.w32(0x65c204, m.r32(0x65c204).wrapping_sub(1));
    let previous = m.r32(shape + 8);
    let next = m.r32(shape + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(shape + 4, 0);
    m.w32(shape + 8, 0);
    let previous = m.r32(0x65c1b0);
    m.w32(previous + 4, shape);
    m.w32(shape + 8, previous);
    m.w32(shape + 4, 0x65c1a8);
    m.w32(0x65c208, m.r32(0x65c208).wrapping_add(1));
    m.w32(0x65c1b0, shape);
    m.w32(shape + 0x20, (m.r32(shape + 0x20) & !1) | 2);
    m.w32(shape + 0x14, u32::MAX);
    if result >= 0 && data != 0 && length > 0 {
        m.w32(shape + 0x28, data);
        m.w32(shape + 0x2c, length);
    } else {
        m.w32(shape + 0x20, (m.r32(shape + 0x20) & !1) | 0x8000_0004);
    }
    Ok(())
}
#[cfg(test)]
mod completion_tests {
    use super::*;
    #[test]
    fn original_async_file_completion_moves_pending_shapes_and_sets_actual_data() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-shape-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_shape_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["completions"].as_array().unwrap() {
            let mut m = Space::new();
            m.set_heap_ptr(row["heap"].as_u64().unwrap() as u32);
            for (i, v) in row["input"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let queues = row["queues_before"].as_array().unwrap();
            for (i, v) in queues.iter().enumerate() {
                m.w32(
                    if i < 7 {
                        0x65bd78 + i as u32 * 4
                    } else {
                        0x65c198 + (i - 7) as u32 * 4
                    },
                    v.as_u64().unwrap() as u32,
                );
            }
            m.w32(0x65c204, row["counts_before"][0].as_u64().unwrap() as u32);
            m.w32(0x65c208, row["counts_before"][1].as_u64().unwrap() as u32);
            complete_file(
                &mut m,
                row["shape"].as_u64().unwrap() as u32,
                row["result"].as_i64().unwrap() as i32,
                row["data"].as_u64().unwrap() as u32,
                row["length"].as_u64().unwrap() as u32,
            )
            .unwrap();
            for (i, v) in row["output"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "{} index{} word{i}",
                    row["condition"],
                    row["index"]
                );
            }
            let queues = row["queues_after"].as_array().unwrap();
            for (i, want) in queues.iter().enumerate() {
                assert_eq!(
                    m.r32(if i < 7 {
                        0x65bd78 + i as u32 * 4
                    } else {
                        0x65c198 + (i - 7) as u32 * 4
                    }),
                    want.as_u64().unwrap() as u32
                );
            }
            assert_eq!(
                m.r32(0x65c204),
                row["counts_after"][0].as_u64().unwrap() as u32
            );
            assert_eq!(
                m.r32(0x65c208),
                row["counts_after"][1].as_u64().unwrap() as u32
            );
        }
    }
}
/// Original505660 completed shapes and505760 failed shapes queue headers.
pub fn initialize_result_queues(m: &mut Space) {
    for (base, head, tail) in [
        (0x65bd58, 0x65bd5c, 0x65bd68),
        (0x65c1e0, 0x65c1e4, 0x65c1f0),
    ] {
        m.w32(base, 0x5f09a8);
        m.w32(head, 0x5f09ac);
        m.w32(head + 4, tail);
        m.w32(head + 8, 0);
        m.w32(tail, 0x5f09ac);
        m.w32(tail + 4, 0);
        m.w32(tail + 8, head);
    }
}
pub trait Decoder {
    /// Actual501b90 decoder (including bones and SCX vertex/index/material data).
    fn decode(&mut self, m: &mut Space, shape: u32) -> Result<(), String>;
    fn release_file(&mut self, m: &mut Space, data: u32) -> Result<(), String>;
}
fn append_shape(m: &mut Space, shape: u32, tail: u32, last: u32) -> Result<(), String> {
    let previous = m.r32(last);
    if previous == 0 {
        return Err("NativeShapeBoundary: resultqueue original startup missing".into());
    }
    m.w32(previous + 4, shape);
    m.w32(shape + 4, tail);
    m.w32(shape + 8, previous);
    m.w32(last, shape);
    Ok(())
}
/// Original501a70 queue transition around the actual mesh decoder.
pub fn process_current(m: &mut Space, decoder: &mut dyn Decoder) -> Result<(), String> {
    let shape = m.r32(0x65c1d8);
    if shape == 0 {
        return Err("NativeShapeBoundary: decoder has no currentshape".into());
    }
    let previous = m.r32(shape + 8);
    let predecessor = if previous != 0 && m.r32(previous + 8) != 0 {
        previous
    } else {
        0
    };
    let next = m.r32(shape + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(shape + 4, 0);
    m.w32(shape + 8, 0);
    m.w32(0x65c208, m.r32(0x65c208).wrapping_sub(1));
    if m.r32(shape + 0x20) & 0x8000_0000 == 0 {
        let scy = m.r32(shape + 0x3c) == 4;
        if scy || m.r32(shape + 0x20) & 2 != 0 {
            decoder.decode(m, shape)?;
        }
        let flags = m.r32(shape + 0x20);
        if !scy && flags & 2 == 0 && flags & 0x2000_0000 != 0 {
            m.w32(shape + 0x20, (flags & !0x10) | 8);
        }
        let flags = m.r32(shape + 0x20);
        if flags & 2 == 0 && (scy || flags & 0x10 == 0 || m.r32(0x6200a4) != m.r32(shape + 0x34)) {
            m.w32(shape + 0x20, (flags & !0x10) | 8);
            append_shape(m, shape, 0x65bd68, 0x65bd70)?;
        } else {
            if predecessor == 0 {
                let first = m.r32(0x65c1a0);
                m.w32(first + 8, shape);
                m.w32(shape + 4, first);
                m.w32(shape + 8, 0x65c19c);
                m.w32(0x65c1a0, shape);
            } else {
                let next = m.r32(predecessor + 4);
                m.w32(shape + 4, next);
                m.w32(shape + 8, predecessor);
                m.w32(predecessor + 4, shape);
                m.w32(next + 8, shape);
            }
            m.w32(0x65c208, m.r32(0x65c208).wrapping_add(1));
        }
    } else {
        append_shape(m, shape, 0x65c1f0, 0x65c1f8)?;
    }
    m.w32(shape + 0x1c, 0);
    Ok(())
}
/// Original505db0 readyqueue cursor and file-buffer lifetime stage.
pub fn advance(m: &mut Space, decoder: &mut dyn Decoder) -> Result<bool, String> {
    let shape = m.r32(0x65c1d8);
    if shape == 0 {
        return Err("NativeShapeBoundary: advance has no currentshape".into());
    }
    if !matches!(m.r32(shape + 0x3c), 3 | 4) {
        return Err("NativeShapeBoundary: original nonmesh worker unavailable".into());
    }
    let next = m.r32(shape + 4);
    let next = if next != 0 && m.r32(next + 4) != 0 {
        next
    } else {
        0
    };
    if m.r32(shape + 0x20) & 0x10 == 0 {
        m.w32(0x618838, 0);
    }
    process_current(m, decoder)?;
    if m.r32(shape + 0x20) & 4 != 0 && m.r32(shape + 0x28) != 0 {
        decoder.release_file(m, m.r32(shape + 0x28))?;
        m.w32(shape + 0x28, 0);
        m.w32(shape + 0x2c, 0);
    }
    m.w32(0x65c1b4, m.r32(shape + 0x1c));
    m.w32(0x65c1d8, next);
    if next == 0 {
        if m.r32(0x618838) == 0 {
            if m.r32(0x65c20c) != 0 {
                let first = m.r32(0x65c1a0);
                let first = if m.r32(first + 4) != 0 { first } else { 0 };
                m.w32(0x65c1d8, first);
                m.w32(0x65c20c, 0);
                m.w32(0x618838, 1);
                return Ok(first != 0);
            }
        } else {
            m.w32(0x65c20c, 0);
        }
        return Ok(false);
    }
    Ok(true)
}

/// Exact synchronous505960 collaborators; file I/O and decoding supply live
/// bytes/results. The deleting destructor is required on failed file reads.
pub trait SynchronousHost: Decoder {
    fn read_file(&mut self, m: &mut Space, path: &str, data_slot: u32) -> Result<i32, String>;
    fn destroy_shape(&mut self, m: &mut Space, shape: u32) -> Result<(), String>;
}
pub fn request_synchronous(
    m: &mut Space,
    kind: u32,
    source: &str,
    skinned: u32,
    resource: u32,
    slot: u32,
    host: &mut dyn SynchronousHost,
) -> Result<u32, String> {
    if !matches!(kind, 3 | 4) {
        return Err(format!(
            "NativeShapeBoundary: synchronous constructor kind{kind}"
        ));
    }
    let shape = allocate_mesh(m, kind, source, skinned, resource, slot);
    m.w32(shape + 0x20, 0x20000000);
    let length = host.read_file(m, source, shape + 0x28)?;
    m.w32(shape + 0x2c, length as u32);
    if length < 1 {
        host.destroy_shape(m, shape)?;
        return Ok(0);
    }
    m.w32(shape + 0x20, m.r32(shape + 0x20) | 2);
    while m.r32(shape + 0x20) & 4 == 0 {
        m.w32(0x65c1d8, shape);
        process_current(m, host)?;
    }
    let data = m.r32(shape + 0x28);
    if data != 0 {
        host.release_file(m, data)?;
        m.w32(shape + 0x28, 0);
        m.w32(shape + 0x2c, 0);
    }
    Ok(shape)
}
/// Original505cd0 begins the main-thread shape pass while the file callback
/// lock is held. The cooperative host serializes the pass and worker callbacks.
pub fn begin(m: &mut Space, host: &mut dyn Host) -> Result<bool, String> {
    let mut shape = m.r32(0x65c1c0);
    if m.r32(shape + 4) == 0 {
        shape = 0;
    }
    while shape != 0 {
        let bytes = (0..256)
            .map(|i| m.r8(m.r32(shape + 0x10) + i))
            .take_while(|v| *v != 0)
            .collect::<Vec<_>>();
        let source = std::str::from_utf8(&bytes)
            .map_err(|_| "NativeShapeBoundary: retry path is not UTF-8")?;
        let result = host.request_file(m, source, 0x505c00, shape)?;
        m.w32(shape + 0x14, result as u32);
        if result < 0 {
            break;
        }
        let next = m.r32(shape + 4);
        let following = if next != 0 && m.r32(next + 4) != 0 {
            next
        } else {
            0
        };
        let previous = m.r32(shape + 8);
        if previous != 0 && next != 0 {
            m.w32(next + 8, previous);
            m.w32(previous + 4, next);
        }
        m.w32(shape + 4, 0);
        m.w32(shape + 8, 0);
        append_shape(m, shape, 0x65bd88, 0x65bd90)?;
        shape = following;
    }
    let first = m.r32(0x65c1a0);
    let first = if m.r32(first + 4) != 0 { first } else { 0 };
    m.w32(0x65c1d8, first);
    m.w32(0x65c20c, 0);
    m.w32(0x618838, 1);
    Ok(first != 0)
}

#[cfg(test)]
mod synchronous_tests {
    use super::*;
    use serde_json::{json, Value};
    fn restore(m: &mut Space, v: &Value) {
        m.set_heap_ptr(v["heap"].as_u64().unwrap() as u32);
        for (i, w) in v["words"].as_array().unwrap().iter().enumerate() {
            m.w32(0x20000000 + i as u32 * 4, w.as_u64().unwrap() as u32);
        }
        for (p, words) in v["queues"].as_object().unwrap() {
            let p = u32::from_str_radix(&p[2..], 16).unwrap();
            for (i, w) in words.as_array().unwrap().iter().enumerate() {
                m.w32(p + i as u32 * 4, w.as_u64().unwrap() as u32);
            }
        }
        for (p, w) in v["globals"].as_object().unwrap() {
            m.w32(
                u32::from_str_radix(&p[2..], 16).unwrap(),
                w.as_u64().unwrap() as u32,
            );
        }
    }
    fn check(m: &Space, v: &Value) {
        assert_eq!(m.heap_ptr(), v["heap"].as_u64().unwrap() as u32);
        for (i, w) in v["words"].as_array().unwrap().iter().enumerate() {
            assert_eq!(
                m.r32(0x20000000 + i as u32 * 4),
                w.as_u64().unwrap() as u32,
                "heap word{i}"
            );
        }
        for (p, words) in v["queues"].as_object().unwrap() {
            let p = u32::from_str_radix(&p[2..], 16).unwrap();
            for (i, w) in words.as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(p + i as u32 * 4),
                    w.as_u64().unwrap() as u32,
                    "queue{p:x} word{i}"
                );
            }
        }
        for (p, w) in v["globals"].as_object().unwrap() {
            assert_eq!(
                m.r32(u32::from_str_radix(&p[2..], 16).unwrap()),
                w.as_u64().unwrap() as u32,
                "global{p}"
            );
        }
    }
    struct Collaborators {
        data: u32,
        handle: i32,
        calls: Vec<Value>,
    }
    impl Decoder for Collaborators {
        fn decode(&mut self, m: &mut Space, shape: u32) -> Result<(), String> {
            self.calls.push(json!(["decode", shape]));
            m.w32(shape + 0x20, (m.r32(shape + 0x20) & !2) | 0x14);
            Ok(())
        }
        fn release_file(&mut self, _m: &mut Space, data: u32) -> Result<(), String> {
            self.calls.push(json!(["free", data]));
            Ok(())
        }
    }
    impl SynchronousHost for Collaborators {
        fn read_file(&mut self, m: &mut Space, _path: &str, slot: u32) -> Result<i32, String> {
            self.calls.push(json!(["read", slot]));
            m.w32(slot, self.data);
            Ok(16)
        }
        fn destroy_shape(&mut self, _m: &mut Space, _shape: u32) -> Result<(), String> {
            panic!("successful original read")
        }
    }
    impl Host for Collaborators {
        fn request_file(
            &mut self,
            _m: &mut Space,
            _path: &str,
            callback: u32,
            shape: u32,
        ) -> Result<i32, String> {
            self.calls.push(json!([0, callback, shape, 0]));
            Ok(self.handle)
        }
    }
    #[test]
    fn original_synchronous_mesh_queue_lifecycle_and_retry_pass_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-shape-sync-original.json");
        let bytes = std::fs::read(path).expect("run native_shape_sync_probe.py");
        let data: Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            restore(&mut m, &row["before"]);
            let mut host = Collaborators {
                data: row["data"].as_u64().unwrap() as u32,
                handle: 0,
                calls: vec![],
            };
            let result = request_synchronous(
                &mut m,
                row["kind"].as_u64().unwrap() as u32,
                "cars/example/mesh.scx",
                row["skinned"].as_u64().unwrap() as u32,
                0xffff1234,
                row["slot"].as_u64().unwrap() as u32,
                &mut host,
            )
            .unwrap();
            assert_eq!(result, row["shape"].as_u64().unwrap() as u32);
            assert_eq!(json!(host.calls), row["calls"]);
            check(&m, &row["after"]);
        }
        for row in data["retries"].as_array().unwrap() {
            let mut m = Space::new();
            restore(&mut m, &row["before"]);
            let mut host = Collaborators {
                data: 0,
                handle: row["handle"].as_i64().unwrap() as i32,
                calls: vec![],
            };
            assert_eq!(
                begin(&mut m, &mut host).unwrap(),
                row["result"].as_u64().unwrap() & 255 != 0
            );
            assert_eq!(json!(host.calls), row["calls"]);
            check(&m, &row["after"]);
        }
    }
}

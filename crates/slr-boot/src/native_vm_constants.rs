//! Raw407660 constant-pool records from the original serialized CONS bytes.
//! This memory producer does not claim deserializer machine scratch/history.
use crate::native_vm_pool::Allocator;
use slr_physics::space::Space;

/// Read one original CONS payload into the source caller's initialized pool
/// container. String constants allocate their own raw buffers; they are not
/// interned here. The returned size is the exact consumed serialized span.
#[allow(dead_code)]
pub fn read_memory(
    m: &mut Space,
    this: u32,
    source: u32,
    bytes: u32,
    a: &mut impl Allocator,
) -> Result<u32, String> {
    if this == 0 || source == 0 || bytes < 4 {
        return Err("NativeVmConstants: missing source pool/span".into());
    }
    let entries = m.r32(source);
    let mut offset = 4u32;
    let mut parsed = Vec::new();
    for _ in 0..entries {
        if bytes - offset < 4 {
            return Err("NativeVmConstants: truncated source record".into());
        }
        let kind = m.r32(source + offset);
        offset += 4;
        let length = match kind {
            0 => {
                if bytes - offset < 4 {
                    return Err("NativeVmConstants: truncated string length".into());
                }
                4u32.checked_add(m.r32(source + offset))
                    .and_then(|n| n.checked_add(1))
                    .ok_or("NativeVmConstants: overflowing string length")?
            }
            1 | 2 | 4 => 4,
            3 | 5 | 7 => 8,
            _ => {
                return Err(format!(
                    "NativeVmConstants: unsupported original pool kind{kind}"
                ))
            }
        };
        if length > bytes - offset {
            return Err("NativeVmConstants: truncated record payload".into());
        }
        if kind == 0 && m.r8(source + offset + length - 1) != 0 {
            return Err("NativeVmConstants: missing declared string terminator".into());
        }
        if kind == 2 {
            let bits = m.r32(source + offset);
            if bits & 0x7f800000 == 0x7f800000 && bits & 0x3fffff != 0 && bits & 0x400000 == 0 {
                return Err(
                    "NativeVmConstants: signaling-NaN FLD/FSTP target-provider boundary".into(),
                );
            }
        }
        parsed.push((kind, offset));
        offset += length;
    }
    let container = this + 8;
    if m.r32(container + 8) > m.r32(container) || m.r32(container) > i32::MAX as u32 / 8 {
        return Err("NativeVmConstants: malformed source container".into());
    }
    for (kind, off) in parsed {
        let (size, vtable) = match kind {
            0 => (16, 0x5e738c),
            1 => (12, 0x5e739c),
            2 => (12, 0x5e73bc),
            3 => (16, 0x5e73ac),
            4 => (16, 0x5e73f0),
            5 => (20, 0x5e73e0),
            7 => (16, 0x5e73d0),
            _ => unreachable!(),
        };
        let record = a.allocate(m, size)?;
        if record == 0 {
            return Err("NativeVmConstants: record allocation failure".into());
        }
        if kind == 1 || kind == 2 {
            m.w32(record, 0x5e7400);
        }
        m.w32(record, vtable);
        let payload = source + off;
        match kind {
            0 => {
                let len = m.r32(payload);
                let buffer = a.allocate(m, len + 1)?;
                m.w32(record + 4, 0);
                m.w32(record + 12, len);
                m.w32(record + 8, buffer);
                let mut copied = 0;
                while copied < len && m.r8(payload + 4 + copied) != 0 {
                    m.w8(buffer + copied, m.r8(payload + 4 + copied));
                    copied += 1;
                }
                m.w8(buffer + copied, 0);
                m.w8(buffer + len, 0);
            }
            1 | 4 => {
                m.w32(record + 4, kind);
                m.w32(record + 8, m.r32(payload));
            }
            2 => {
                let bits = m.r32(payload);
                // The bounded source load/store preserves these admitted raw
                //words. Signaling NaNs reject before writes: Unicorn retained
                //their signaling bit, contrary to the hardware/model premise.
                m.w32(record + 8, bits);
                m.w32(record + 4, 2);
            }
            3 => {
                m.w32(record + 8, m.r32(payload + 4));
                m.w32(record + 4, 3);
                m.w32(record + 12, m.r32(payload));
            }
            5 | 7 => {
                m.w32(record + 12, m.r32(payload + 4));
                m.w32(record + 4, kind);
                m.w32(record + 8, m.r32(payload));
                if kind == 5 {
                    m.w32(record + 16, u32::MAX);
                }
            }
            _ => unreachable!(),
        }
        let count = m.r32(container + 8);
        let capacity = m.r32(container);
        if count + 1 > capacity {
            let new = capacity + count + 1;
            let data = a.reallocate(m, m.r32(container + 4), capacity * 4, new * 4)?;
            m.w32(container + 4, data);
            m.w32(container, new);
        }
        let data = m.r32(container + 4);
        m.w32(data + count * 4, record);
        m.w32(container + 8, count + 1);
    }
    Ok(offset)
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
        fn reallocate(
            &mut self,
            m: &mut Space,
            old: u32,
            old_n: u32,
            n: u32,
        ) -> Result<u32, String> {
            let p = m.alloc(n.max(1));
            for i in 0..old_n.min(n) {
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
    fn original_constant_records_and_raw_string_buffers_match_all_heap_bytes() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let v: Value = serde_json::from_slice(
            &std::fs::read(root.join("re/out/boot/native-vm-constants-original.json"))
                .expect("run native_vm_constants_probe.py"),
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
                "native_vm_constants_probe.py",
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
        assert_eq!(rows.len(), 12);
        assert_eq!(
            rows.iter()
                .map(|r| (
                    r["label"].as_str().unwrap(),
                    word(&r["records"]),
                    word(&r["seed"])
                ))
                .collect::<std::collections::HashSet<_>>(),
            [
                ("empty", 0),
                ("all_kinds", 8),
                ("float_bits", 9),
                ("signaling_nan_boundary", 2),
                ("string_raw_bytes", 4),
                ("growth", 70)
            ]
            .into_iter()
            .flat_map(|(label, count)| [606, 20261022].map(move |seed| (label, count, seed)))
            .collect()
        );
        for row in rows {
            let mut m = Space::new();
            let before = bytes(&row["before"]["bytes"]);
            assert_eq!(
                before.len(),
                (word(&row["before"]["heap_ptr"]) - 0x20000000) as usize
            );
            for (i, b) in before.into_iter().enumerate() {
                m.w8(0x20000000 + i as u32, b);
            }
            m.set_heap_ptr(word(&row["before"]["heap_ptr"]));
            let result = read_memory(
                &mut m,
                word(&row["pool"]),
                word(&row["source"]),
                word(&row["source_bytes"]),
                &mut Bump,
            );
            if row["label"] == "signaling_nan_boundary" {
                assert!(result.unwrap_err().contains("signaling-NaN"));
                assert_eq!(m.heap_ptr(), word(&row["before"]["heap_ptr"]));
                let before = bytes(&row["before"]["bytes"]);
                assert_eq!(
                    (0..before.len())
                        .map(|i| m.r8(0x20000000 + i as u32))
                        .collect::<Vec<_>>(),
                    before,
                    "unsupported target/provider case rejects before any writes"
                );
                continue;
            }
            assert_eq!(result.unwrap(), word(&row["source_bytes"]));
            let expected = bytes(&row["after"]["bytes"]);
            assert_eq!(
                expected.len(),
                (word(&row["after"]["heap_ptr"]) - 0x20000000) as usize
            );
            assert_eq!(
                m.heap_ptr(),
                word(&row["after"]["heap_ptr"]),
                "{}",
                row["label"]
            );
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
            assert!(
                diffs.is_empty(),
                "complete constant-pool heap {} seed{} {diffs:x?}",
                row["label"],
                row["seed"]
            );
        }
    }
}

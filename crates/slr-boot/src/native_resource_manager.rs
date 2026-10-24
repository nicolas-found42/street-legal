//! Original537240 asynchronous descriptor requests and544540 file completion.
//! Descriptor bytes are delivered by the actual file worker, never by a trace.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
pub fn initialize(m: &mut Space, manager: u32) {
    for base in [manager + 0xdb0, manager + 0xdcc] {
        m.w32(base, 0x5f09a8);
        m.w32(base + 4, 0x5f09ac);
        m.w32(base + 8, base + 0x10);
        m.w32(base + 12, 0);
        m.w32(base + 0x10, 0x5f09ac);
        m.w32(base + 0x14, 0);
        m.w32(base + 0x18, base + 4);
    }
    for i in 0..0x400 {
        let record = manager + 0xf0e20 + i * 0x3c;
        m.w32(record, 0x5f3558);
        for offset in [4, 8, 0x18, 0x1c, 0x20, 0x24] {
            m.w32(record + offset, 0);
        }
        let previous = m.r32(manager + 0xde4);
        m.w32(previous + 4, record);
        m.w32(record + 4, manager + 0xddc);
        m.w32(record + 8, previous);
        m.w32(manager + 0xde4, record);
    }
    for (i, v) in [
        1f32, 5., 0., 0., 0., 0., 0., 0., 2., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0., 0.,
    ]
    .into_iter()
    .enumerate()
    {
        m.wf(0x618d70 + i as u32 * 4, v);
    }
}
pub trait Host {
    fn payload(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    fn minimum(&mut self, m: &mut Space, resource: u32, quality: u32) -> Result<(), String>;
    fn package(&mut self, m: &mut Space, resource: u32) -> Result<u32, String>;
    /// Original5444a0 result: zero means the file request was accepted.
    fn request_file(
        &mut self,
        m: &mut Space,
        package: u32,
        resource: u32,
        record: u32,
    ) -> Result<i32, String>;
    fn read_descriptor(
        &mut self,
        _m: &mut Space,
        _package: u32,
        _resource: u32,
    ) -> Result<u32, String> {
        Err("NativeResourceManagerBoundary: synchronous descriptor read unavailable".into())
    }
    fn load_descriptor(
        &mut self,
        _m: &mut Space,
        _resource: u32,
        _data: u32,
        _quality: u32,
    ) -> Result<(), String> {
        Err("NativeResourceManagerBoundary: descriptor loader unavailable".into())
    }
    fn release_descriptor(&mut self, _m: &mut Space, _data: u32) -> Result<(), String> {
        Err("NativeResourceManagerBoundary: descriptor release unavailable".into())
    }
}
/// Async branch of537240. Synchronous promotion and runtime descriptors remain
/// explicit until their load/release path is supplied by the embedding.
pub fn request_async(
    m: &mut Space,
    resource: u32,
    quality: u32,
    priority: u32,
    time: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    if m.r32(resource + 0x5c) == 0 {
        crate::native_resource_load::mark_loaded(m, resource, 1)?;
        m.wf(resource + 0x68, 1.);
        return Ok(());
    }
    if (f32::from_bits(priority) == 0. && f32::from_bits(time) == 0.)
        || m.r32(resource + 0x50) >> 16 == 0xffff
    {
        return request_synchronous(m, resource, quality, host);
    }
    if m.r32(resource + 0x54) & 0x2000000 != 0 {
        return Ok(());
    }
    let kind = m.r32(resource + 0x4c);
    if kind == 7 {
        let payload = host.payload(m, resource)?;
        if payload != 0 && m.r32(payload + 0x18) != 0 {
            let flags = m.r32(m.r32(payload + 0x18) + 0x20);
            if flags & 0x80000001 == 0 {
                return Ok(());
            }
        }
    } else {
        if kind == 5 && host.payload(m, resource)? != 0 && m.rf(resource + 0x68) == 1. {
            return Ok(());
        }
        if kind != 3 && f32::from_bits(quality) > 0. {
            host.minimum(m, resource, quality)?;
        }
    }
    let package = host.package(m, resource)?;
    if package == 0 || m.r32(resource + 0x64) & 1 != 0 {
        return Ok(());
    }
    let manager = m.r32(0x618d48);
    if manager == 0 {
        return Err("NativeResourceManagerBoundary: manager is absent".into());
    }
    let record = m.r32(manager + 0xdd4);
    if record == 0 {
        return Err(
            "NativeResourceManagerBoundary: descriptor free list is not initialized".into(),
        );
    }
    if m.r32(record + 4) == 0 {
        return Ok(());
    }
    crate::native_part::bind_reference(m, record + 0x18, resource);
    m.w32(record + 0xc, priority);
    m.w32(record + 0x10, time);
    let scaled = X87::from_f32(f32::from_bits(quality))
        .mul(
            X87::from_f32(m.rf(0x618d70 + kind * 4)),
            Precision::F32,
            Rounding::NearestEven,
        )
        .mul(X87::from_f32(256.), Precision::F32, Rounding::NearestEven);
    m.w32(record + 0x14, scaled.to_i64(Rounding::Zero) as u32);
    m.w32(record + 0x30, 0);
    m.w32(record + 0x34, m.r32(resource + 0x5c));
    m.w32(record + 0x2c, 1);
    m.w32(record + 0x28, 0x80000000);
    m.w32(record + 0x38, package);
    if host.request_file(m, package, resource, record)? != 0 {
        m.w32(record + 0x28, 0);
        crate::native_part::bind_reference(m, record + 0x18, 0);
        m.w32(record + 0x34, 0);
        m.w32(record + 0x2c, 0);
        return Ok(());
    }
    m.w32(resource + 0x54, m.r32(resource + 0x54) | 0x2000000);
    let previous = m.r32(record + 8);
    let next = m.r32(record + 4);
    if previous != 0 && next != 0 {
        m.w32(next + 8, previous);
        m.w32(previous + 4, next);
    }
    m.w32(record + 4, 0);
    m.w32(record + 8, 0);
    let previous = m.r32(manager + 0xdc8);
    m.w32(previous + 4, record);
    m.w32(record + 8, previous);
    m.w32(record + 4, manager + 0xdc0);
    m.w32(manager + 0xdc8, record);
    if kind == 7 {
        m.w32(0x765f28, m.r32(0x765f28).wrapping_add(1));
    }
    Ok(())
}
/// Original544540 publishes completion without invoking the resource loader.
pub fn complete(m: &mut Space, record: u32, result: i32, data: u32) {
    if record == 0 {
        return;
    }
    m.w32(record + 0x30, data);
    m.w32(record + 0x2c, 0);
    if result != 0 {
        m.w32(record + 0x28, m.r32(record + 0x28) | 4);
    }
    m.w32(record + 0x28, m.r32(record + 0x28) | 1);
}
/// Synchronous branch537240 consumes a completed queued range when available,
/// otherwise it performs the original direct descriptor read. Pending reads
/// remain pending when the direct read is used.
fn request_synchronous(
    m: &mut Space,
    raw: u32,
    quality: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let manager = m.r32(0x618d48);
    if m.r32(raw + 0x54) & 0x2000000 != 0 {
        let mut record = m.r32(manager + 0xdb8);
        while record != 0 && m.r32(record + 4) != 0 {
            if m.r32(record + 0x20) == m.r32(raw + 0x50) {
                break;
            }
            record = m.r32(record + 4);
        }
        if record != 0
            && m.r32(record + 4) != 0
            && m.r32(record + 0x2c) == 0
            && m.r32(record + 0x28) & 5 == 1
            && m.r32(record + 0x30) != 0
        {
            if m.rf(raw + 0x6c) < 1000. {
                m.wf(raw + 0x6c, 1000.);
                host.load_descriptor(m, raw, m.r32(record + 0x30), 1000f32.to_bits())?;
                m.wf(raw + 0x6c, 1.);
            }
            m.w32(raw + 0x54, m.r32(raw + 0x54) & !0x2000000);
            if host.package(m, raw)? != 0 {
                host.release_descriptor(m, m.r32(record + 0x30))?;
                m.w32(record + 0x30, 0);
            }
            crate::native_part::bind_reference(m, record + 0x18, 0);
            m.w32(record + 0x34, 0);
            let previous = m.r32(record + 8);
            let next = m.r32(record + 4);
            if previous != 0 && next != 0 {
                m.w32(next + 8, previous);
                m.w32(previous + 4, next);
            }
            m.w32(record + 4, 0);
            m.w32(record + 8, 0);
            let first = m.r32(manager + 0xdd4);
            m.w32(first + 8, record);
            m.w32(record + 4, first);
            m.w32(record + 8, manager + 0xdd0);
            m.w32(manager + 0xdd4, record);
            m.w32(record + 0x28, m.r32(record + 0x28) & 0x7ffffffe);
            return Ok(());
        }
    }
    if m.r32(raw + 0x50) >> 16 == 0xffff {
        if m.r32(raw + 0x58) != 0 && m.rf(raw + 0x6c) < 1000. {
            m.wf(raw + 0x6c, 1000.);
            host.load_descriptor(m, raw, m.r32(raw + 0x58), quality)?;
            m.wf(raw + 0x6c, 1.);
        }
        return Ok(());
    }
    m.w32(0x765f1c, m.r32(0x765f1c).wrapping_add(1));
    let package = host.package(m, raw)?;
    if package == 0 {
        return Ok(());
    }
    let data = host.read_descriptor(m, package, raw)?;
    if data == 0 {
        return Ok(());
    }
    if m.rf(raw + 0x6c) < 1000. {
        m.wf(raw + 0x6c, 1000.);
        host.load_descriptor(m, raw, data, 1f32.to_bits())?;
        m.wf(raw + 0x6c, 1.);
    }
    host.release_descriptor(m, data)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{json, Value};
    struct Callbacks {
        accepted: i32,
        calls: Vec<Value>,
        manager: u32,
    }
    impl Host for Callbacks {
        fn read_descriptor(&mut self, _m: &mut Space, p: u32, raw: u32) -> Result<u32, String> {
            self.calls.push(json!(["read", p, raw + 0x4c]));
            Ok(0x30002000)
        }
        fn load_descriptor(
            &mut self,
            m: &mut Space,
            raw: u32,
            data: u32,
            quality: u32,
        ) -> Result<(), String> {
            self.calls
                .push(json!(["load", raw, data, quality, m.r32(raw + 0x6c)]));
            Ok(())
        }
        fn release_descriptor(&mut self, _m: &mut Space, data: u32) -> Result<(), String> {
            self.calls.push(json!(["release", data]));
            Ok(())
        }
        fn payload(&mut self, _m: &mut Space, raw: u32) -> Result<u32, String> {
            self.calls.push(json!(["payload", raw, 1f32.to_bits()]));
            Ok(raw + 0x80)
        }
        fn minimum(&mut self, _m: &mut Space, raw: u32, q: u32) -> Result<(), String> {
            self.calls.push(json!(["minimum", raw, q]));
            Ok(())
        }
        fn package(&mut self, _m: &mut Space, _raw: u32) -> Result<u32, String> {
            let p = self.manager + 0xffe94 + 0x70;
            self.calls.push(json!(["package", p]));
            Ok(p)
        }
        fn request_file(
            &mut self,
            _m: &mut Space,
            p: u32,
            raw: u32,
            r: u32,
        ) -> Result<i32, String> {
            self.calls.push(json!(["file", p, raw + 0x4c, r]));
            Ok(self.accepted)
        }
    }
    #[test]
    fn original_synchronous_descriptor_promotion_preserves_pending_and_recycles_ready() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-manager-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let data: Value = serde_json::from_slice(
            &std::fs::read(path).expect("run native_resource_manager_probe.py"),
        )
        .unwrap();
        for row in data["synchronous"].as_array().unwrap() {
            let manager = row["manager"].as_u64().unwrap() as u32;
            let raw = row["raw"].as_u64().unwrap() as u32;
            let record = row["record_address"].as_u64().unwrap() as u32;
            let mut m = Space::new();
            m.w32(0x618d48, manager);
            for (key, base) in [
                ("resource", raw),
                ("queues", manager + 0xdb0),
                ("record", record),
            ] {
                for (i, v) in row["input"][key].as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, v.as_u64().unwrap() as u32);
                }
            }
            let mut host = Callbacks {
                manager,
                accepted: 0,
                calls: vec![],
            };
            request_async(&mut m, raw, 1f32.to_bits(), 0, 0, &mut host).unwrap();
            assert_eq!(json!(host.calls), row["calls"]);
            for (key, base) in [
                ("resource", raw),
                ("queues", manager + 0xdb0),
                ("record", record),
            ] {
                for (i, v) in row[key].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(base + i as u32 * 4),
                        v.as_u64().unwrap() as u32,
                        "{key}[{i}]"
                    );
                }
            }
            assert_eq!(m.r32(0x765f1c), row["sync_reads"].as_u64().unwrap() as u32);
        }
    }

    #[test]
    fn original_async_descriptor_requests_match_intrusive_memory_and_callbacks() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-manager-original.json");
        let Some(path) = crate::oracle_fixture::file(&path) else {
            return;
        };
        let data: Value = serde_json::from_slice(
            &std::fs::read(path).expect("run native_resource_manager_probe.py"),
        )
        .unwrap();
        for row in data["rows"].as_array().unwrap() {
            let manager = row["manager"].as_u64().unwrap() as u32;
            let raw = row["raw"].as_u64().unwrap() as u32;
            let record = row["record_address"].as_u64().unwrap() as u32;
            let mut m = Space::new();
            m.w32(0x618d48, manager);
            for (key, base) in [
                ("resource", raw),
                ("queues", manager + 0xdb0),
                ("record", record),
            ] {
                for (i, v) in row["input"][key].as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, v.as_u64().unwrap() as u32);
                }
            }
            for (i, v) in row["weights"].as_array().unwrap().iter().enumerate() {
                m.w32(0x618d70 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let mut host = Callbacks {
                manager,
                accepted: row["accepted"].as_i64().unwrap() as i32,
                calls: vec![],
            };
            request_async(&mut m, raw, 1f32.to_bits(), 0, 10f32.to_bits(), &mut host).unwrap();
            if row["repeat"].as_bool().unwrap() {
                request_async(&mut m, raw, 1f32.to_bits(), 0, 10f32.to_bits(), &mut host).unwrap();
            }
            assert_eq!(json!(host.calls), row["calls"]);
            for (key, base) in [
                ("resource", raw),
                ("queues", manager + 0xdb0),
                ("record", record),
            ] {
                for (i, v) in row[key].as_array().unwrap().iter().enumerate() {
                    assert_eq!(
                        m.r32(base + i as u32 * 4),
                        v.as_u64().unwrap() as u32,
                        "{key}[{i}]"
                    );
                }
            }
        }
    }
}

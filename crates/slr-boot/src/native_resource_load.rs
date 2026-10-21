//! Original resource type buckets538520 and loaded/unloaded list transitions.
//! The resource-manager address comes from original global618d48; allocation
//! and resource-ID authority remain the live resource factory's responsibility.
use slr_physics::space::Space;

/// Parameters of original419860, retaining raw float argument words.
#[derive(Clone, Copy)]
pub struct Access {
    pub flags: u32,
    pub quality: u32,
    pub priority: u32,
    pub time: u32,
}
/// Actual manager and resource virtual operations reached by419860/5447d0.
pub trait AccessHost {
    fn set_minimum(&mut self, m: &mut Space, raw: u32, value: u32) -> Result<(), String>;
    fn request(
        &mut self,
        m: &mut Space,
        raw: u32,
        quality: u32,
        priority: u32,
        time: u32,
    ) -> Result<(), String>;
    fn spatial_request(
        &mut self,
        m: &mut Space,
        raw: u32,
        quality: u32,
        priority: u32,
        time: u32,
    ) -> Result<(), String>;
    fn quality_sensitive(&self, kind: u32) -> Result<bool, String>;
    fn minimum_quality(&mut self, m: &mut Space, raw: u32) -> Result<f32, String>;
    fn refresh_quality(&mut self, m: &mut Space, raw: u32) -> Result<(), String>;
    fn touch(&mut self, m: &mut Space, raw: u32) -> Result<(), String>;
    fn payload(&mut self, m: &mut Space, raw: u32, quality: u32) -> Result<u32, String>;
}

/// exe: FUN_00419860
/// Resource access returns null while a real asynchronous request is pending.
pub fn access(
    m: &mut Space,
    raw: u32,
    args: Access,
    host: &mut dyn AccessHost,
) -> Result<u32, String> {
    if m.r32(raw + 0x4c) != 1 {
        host.set_minimum(m, raw, args.quality)?;
    }
    if prepare(m, raw, args, host)? & 0x8000_0000 == 0 {
        host.payload(m, raw, 1f32.to_bits())
    } else {
        Ok(0)
    }
}

/// exe: FUN_005447d0
/// Loading, spatial notification, quality refresh, and current-generation touch.
pub fn prepare(
    m: &mut Space,
    raw: u32,
    args: Access,
    host: &mut dyn AccessHost,
) -> Result<u32, String> {
    let request_allowed = |m: &Space| {
        m.r32(raw + 0x64) & 0x1000_0000 == 0
            && (m.r32(raw + 0x54) & 0x0200_0000 == 0
                || (f32::from_bits(args.priority) == 0. && f32::from_bits(args.time) == 0.))
    };
    if args.flags & 0x4000_0000 == 0 {
        let requested = m.rf(raw + 0x6c);
        let minimum = if requested < 1. { requested } else { 1. };
        let sufficient = m.r32(raw + 0x64) & 1 != 0 && minimum <= m.rf(raw + 0x68);
        if !sufficient && request_allowed(m) {
            host.request(m, raw, m.r32(raw + 0x6c), args.priority, args.time)?;
        }
    }
    if args.flags & 0x2000_0000 == 0 && m.r32(raw + 0x54) & 0x100 != 0 {
        host.spatial_request(m, raw, m.r32(raw + 0x6c), args.priority, args.time)?;
    }
    let quality = m.rf(raw + 0x68);
    if host.quality_sensitive(m.r32(raw + 0x4c))? && quality != m.rf(raw + 0x6c) {
        let minimum = host.minimum_quality(m, raw)?;
        host.refresh_quality(m, raw)?;
        let requested = m.rf(raw + 0x6c);
        let increase = quality == 0. || quality < requested;
        let decrease = requested != 0.
            && !matches!(
                minimum.partial_cmp(&requested),
                Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
            )
            && minimum != 0.;
        if (increase || decrease) && request_allowed(m) {
            host.request(m, raw, m.r32(raw + 0x6c), args.priority, args.time)?;
        }
    }
    if m.r32(raw + 0x64) & 1 == 0 {
        return Ok(0x8000_0001);
    }
    if args.flags & 0x4000_0000 == 0 && m.r32(raw + 0x74) != m.r32(0x6200a4) {
        host.touch(m, raw)?;
    }
    Ok(0)
}

#[cfg(test)]
mod access_tests {
    use super::*;
    struct Callbacks<'a> {
        row: &'a serde_json::Value,
        calls: Vec<serde_json::Value>,
    }
    impl Callbacks<'_> {
        fn record(&mut self, m: &Space, raw: u32, name: &str, args: &[u32]) {
            self.calls.push(serde_json::json!({"name":name,"args":args,
                "state":(0..0xd0/4).map(|i|m.r32(raw+i*4)).collect::<Vec<_>>()}));
        }
    }
    impl AccessHost for Callbacks<'_> {
        fn set_minimum(&mut self, m: &mut Space, raw: u32, value: u32) -> Result<(), String> {
            self.record(m, raw, "minimum", &[value]);
            m.w32(raw + 0x6c, value);
            Ok(())
        }
        fn request(
            &mut self,
            m: &mut Space,
            raw: u32,
            quality: u32,
            priority: u32,
            time: u32,
        ) -> Result<(), String> {
            self.record(m, raw, "request", &[quality, priority, time]);
            if self.row["loaded_by_request"].as_bool().unwrap() {
                m.w32(raw + 0x64, m.r32(raw + 0x64) | 1);
                m.w32(raw + 0x68, quality);
            }
            Ok(())
        }
        fn spatial_request(
            &mut self,
            m: &mut Space,
            raw: u32,
            quality: u32,
            priority: u32,
            time: u32,
        ) -> Result<(), String> {
            self.record(m, raw, "spatial", &[quality, priority, time]);
            Ok(())
        }
        fn quality_sensitive(&self, _: u32) -> Result<bool, String> {
            Ok(self.row["sensitive"].as_u64().unwrap() != 0)
        }
        fn minimum_quality(&mut self, m: &mut Space, raw: u32) -> Result<f32, String> {
            self.record(m, raw, "min_quality", &[]);
            Ok(f32::from_bits(self.row["minimum"].as_u64().unwrap() as u32))
        }
        fn refresh_quality(&mut self, m: &mut Space, raw: u32) -> Result<(), String> {
            self.record(m, raw, "refresh", &[]);
            Ok(())
        }
        fn touch(&mut self, m: &mut Space, raw: u32) -> Result<(), String> {
            self.record(m, raw, "touch", &[]);
            m.w32(raw + 0x74, m.r32(0x6200a4));
            Ok(())
        }
        fn payload(&mut self, m: &mut Space, raw: u32, quality: u32) -> Result<u32, String> {
            self.record(m, raw, "payload", &[quality]);
            Ok(raw + 0x80)
        }
    }
    #[test]
    fn original_access_protocol_matches_callback_order_state_and_result() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-access-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_resource_access_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for (index, row) in data["rows"].as_array().unwrap().iter().enumerate() {
            let mut m = Space::new();
            let raw = row["raw"].as_u64().unwrap() as u32;
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(raw + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            m.w32(0x6200a4, 123);
            let args = row["args"].as_array().unwrap();
            let mut host = Callbacks { row, calls: vec![] };
            let result = access(
                &mut m,
                raw,
                Access {
                    flags: args[0].as_u64().unwrap() as u32,
                    quality: args[1].as_u64().unwrap() as u32,
                    priority: args[2].as_u64().unwrap() as u32,
                    time: args[3].as_u64().unwrap() as u32,
                },
                &mut host,
            )
            .unwrap();
            assert_eq!(result, row["result"].as_u64().unwrap() as u32, "row{index}");
            assert_eq!(serde_json::json!(host.calls), row["calls"], "row{index}");
            assert_eq!(
                serde_json::json!((0..0xd0 / 4)
                    .map(|i| m.r32(raw + i * 4))
                    .collect::<Vec<_>>()),
                row["after"],
                "row{index}"
            );
        }
    }
}

/// Original538520. Three two-link intrusive lists per resource kind.
pub fn initialize_bucket(m: &mut Space, bucket: u32) {
    for base in [bucket, bucket + 0x2c, bucket + 0x58] {
        m.w32(base, 0x5f2af4);
        m.w32(base + 4, 0x5f2af8);
        m.w32(base + 0x18, 0x5f2af8);
        for index in 0..2 {
            m.w32(base + 8 + index * 8, base + 0x18);
            m.w32(base + 12 + index * 8, 0);
            m.w32(base + 0x1c + index * 8, 0);
            m.w32(base + 0x20 + index * 8, base + 4);
        }
    }
    for offset in [0x84, 0x88, 0x8c] {
        m.w32(bucket + offset, 0);
    }
}
/// Initialize only the actual type-bucket component of a supplied manager.
/// This is not an implementation of the rest of535a70's manager constructor.
pub fn initialize_buckets(m: &mut Space, manager: u32) {
    for kind in 0..22 {
        initialize_bucket(m, manager + 0x140 + kind * 0x90);
    }
}
/// Materialize the proved manager-front component used by load registration.
/// Original535a70 places22actual538520 buckets at+140; the other manager
/// subsystems are deliberately outside this projection and remain host-owned.
/// The pointer occupies the original global618d48, used by544b90's two calls.
pub fn initialize_projection(m: &mut Space) -> u32 {
    let existing = m.r32(0x618d48);
    if existing != 0 {
        return existing;
    }
    let manager = m.alloc(0x106ee8);
    initialize_buckets(m, manager);
    crate::native_spatial::initialize_request_pool(m, manager);
    m.w32(0x618d48, manager);
    manager
}
fn move_resource(m: &mut Space, resource: u32, loaded: bool) -> Result<(), String> {
    let manager = m.r32(0x618d48);
    let kind = m.r32(resource + 0x4c);
    if manager == 0 || kind >= 22 {
        return Err("NativeResourceBoundary: original load-list manager/kind unavailable".into());
    }
    let previous = m.r32(resource + 0x10);
    let next = m.r32(resource + 0xc);
    if previous != 0 && next != 0 {
        m.w32(next + 0x10, previous);
        m.w32(previous + 0xc, next);
    }
    m.w32(resource + 0xc, 0);
    m.w32(resource + 0x10, 0);
    let list = manager + kind * 0x90 + if loaded { 0x16c } else { 0x198 };
    let tail = m.r32(list + 0x28);
    if tail == 0 {
        return Err("NativeResourceBoundary: original load-list sentinel unavailable".into());
    }
    m.w32(tail + 0xc, resource);
    m.w32(resource + 0xc, list + 0x18);
    m.w32(resource + 0x10, tail);
    m.w32(list + 0x28, resource);
    m.w32(
        resource + 0x64,
        if loaded {
            (m.r32(resource + 0x64) & !0x50) | 0x20
        } else {
            (m.r32(resource + 0x64) & !0x30) | 0x40
        },
    );
    Ok(())
}
/// Original537d80.
pub fn register_loaded(m: &mut Space, resource: u32) -> Result<(), String> {
    move_resource(m, resource, true)
}
/// Original537de0.
pub fn register_unloaded(m: &mut Space, resource: u32) -> Result<(), String> {
    move_resource(m, resource, false)
}
/// Original544b90, including list transition and unloaded quality reset.
pub fn mark_loaded(m: &mut Space, resource: u32, load: u32) -> Result<(), String> {
    let state = ((m.r32(resource + 0x64) ^ load) & 1) ^ m.r32(resource + 0x64);
    m.w32(resource + 0x64, state);
    if state & 1 == 0 {
        if state & 0x40 == 0 {
            register_unloaded(m, resource)?;
        }
        m.w32(resource + 0x68, 0);
    } else if state & 0x20 == 0 {
        register_loaded(m, resource)?;
    }
    Ok(())
}
/// Live identity and lazy-load collaborators for original545230. The allocator
/// must construct the same resource kind/flags/parent/name using a new real ID.
pub trait CloneHost {
    fn allocate_clone(
        &mut self,
        m: &mut Space,
        source: u32,
        name: Option<&[u8]>,
    ) -> Result<u32, String>;
    fn source_payload(&mut self, m: &mut Space, source: u32) -> Result<Option<u32>, String>;
    fn destroy_clone(&mut self, m: &mut Space, target: u32) -> Result<(), String>;
    fn spatial_request(&mut self, m: &mut Space, target: u32) -> Result<(), String>;
    fn synchronize_loaded(&mut self, m: &mut Space, target: u32) -> Result<(), String>;
}
/// Original54a830 typee minimum-detail request (no generation-stamp write).
pub fn request_mesh_type(m: &mut Space, resource: u32, value: u32) {
    use slr_x87::{Precision, Rounding, X87};
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let f = |v| X87::from_f32(f32::from_bits(v));
    let result = f(m.r32(resource + 0xc8))
        .mul(f(value), pc, rc)
        .add(f(m.r32(resource + 0xc4)), pc, rc)
        .to_f32(pc, rc);
    let result = if !matches!(
        0f32.partial_cmp(&result),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) {
        0.
    } else if result > 1. {
        1.
    } else {
        result
    };
    if m.r32(resource + 0x78) < m.r32(0x6200a4) || result > m.rf(resource + 0x6c) {
        m.wf(resource + 0x6c, result);
    }
}
/// Original545230 for typee. Actual54a190 payload copy and537d80 registration
/// execute here; the source request/get-data and identity allocation stay live.
pub fn clone_mesh_type(
    m: &mut Space,
    source: u32,
    host: &mut dyn CloneHost,
) -> Result<Option<u32>, String> {
    if m.r32(source + 0x4c) != 0xe {
        return Err(
            "NativeResourceBoundary: clone virtual payload copy is only implemented for typee"
                .into(),
        );
    }
    let name_pointer = m.r32(source + 0x60);
    let name = if name_pointer != 0 {
        let temp = m.malloc(32);
        let mut bytes = Vec::new();
        for index in 0..30 {
            let value = m.r8(name_pointer + index);
            if value == 0 {
                break;
            }
            m.w8(temp + index, value);
            bytes.push(value);
        }
        m.w8(temp + bytes.len() as u32, 0);
        Some(bytes)
    } else {
        None
    };
    let target = host.allocate_clone(m, source, name.as_deref())?;
    if target == 0 || m.r32(target + 0x4c) != 0xe {
        return Err(
            "NativeResourceBoundary: typee clone allocator did not construct a live typee resource"
                .into(),
        );
    }
    m.w32(target + 0x78, m.r32(source + 0x78));
    request_mesh_type(m, source, 1f32.to_bits());
    let Some(payload) = host.source_payload(m, source)? else {
        host.destroy_clone(m, target)?;
        return Ok(None);
    };
    if payload == 0 || m.r32(payload) != 0x5f3a5c {
        return Err(
            "NativeResourceBoundary: source clone payload has no original typee vtable".into(),
        );
    }
    crate::native_render_type::clone_payload(m, payload, target + 0x80)?;
    m.w32(target + 0x54, m.r32(target + 0x54) | 0x200);
    let previous = m.r32(target + 0x64);
    m.w32(target + 0x64, previous | 1);
    if previous & 0x20 == 0 {
        register_loaded(m, target)?;
    }
    m.w32(target + 0x68, m.r32(source + 0x68));
    if m.r32(target + 0x54) & 0x100 != 0 {
        host.spatial_request(m, target)?;
    }
    host.synchronize_loaded(m, target)?;
    Ok(Some(target))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_resource_bucket_and_load_transitions_match_complete_buffers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-load-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_resource_load_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let manager = m.alloc(0xda0);
            let resources = [m.alloc(0x90), m.alloc(0x90)];
            m.w32(0x618d48, manager);
            for (i, v) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            initialize_buckets(&mut m, manager);
            for operation in row["operations"].as_array().unwrap() {
                let i = operation[0].as_u64().unwrap() as usize;
                let load = operation[1].as_u64().unwrap() as u32;
                mark_loaded(&mut m, resources[i], load).unwrap();
            }
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "kind={} word{i}",
                    row["kind"]
                );
            }
        }
    }
    struct Cloner {
        target: u32,
        named: bool,
        spatial: Vec<serde_json::Value>,
    }
    impl CloneHost for Cloner {
        fn allocate_clone(
            &mut self,
            _m: &mut Space,
            _source: u32,
            name: Option<&[u8]>,
        ) -> Result<u32, String> {
            assert_eq!(name.is_some(), self.named);
            if self.named {
                assert_eq!(name.unwrap(), b"typee real clone name");
            }
            Ok(self.target)
        }
        fn source_payload(&mut self, _m: &mut Space, source: u32) -> Result<Option<u32>, String> {
            Ok(Some(source + 0x80))
        }
        fn destroy_clone(&mut self, _m: &mut Space, _target: u32) -> Result<(), String> {
            panic!("successful oracle clone never destroys target")
        }
        fn spatial_request(&mut self, m: &mut Space, target: u32) -> Result<(), String> {
            self.spatial.push(serde_json::json!({"raw":target,"args":[1f32.to_bits(),0,0],"state":m.r32(target+0x64),"quality":m.r32(target+0x68)}));
            Ok(())
        }
        fn synchronize_loaded(&mut self, _m: &mut Space, _target: u32) -> Result<(), String> {
            Ok(())
        }
    }
    #[test]
    fn original_typee_resource_clone_matches_full_heap_and_loaded_list() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-resource-clone-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_resource_clone_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            m.set_heap_ptr(0x20000000 + before.len() as u32 * 4);
            m.w32(0x618d48, 0x20000000);
            m.w32(0x6200a4, 123);
            for (i, v) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            let source = row["source"].as_u64().unwrap() as u32;
            let target = row["target"].as_u64().unwrap() as u32;
            let mut host = Cloner {
                target,
                named: row["named"].as_bool().unwrap(),
                spatial: vec![],
            };
            assert_eq!(
                clone_mesh_type(&mut m, source, &mut host).unwrap(),
                Some(target)
            );
            assert_eq!(serde_json::json!(host.spatial), row["spatial"]);
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "count={} named={} word{i}",
                    row["count"],
                    row["named"]
                );
            }
        }
    }
}

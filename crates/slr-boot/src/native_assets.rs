//! Live lazy payload loading for descriptor-backed native render resources.
//! Resource identity and stock bytes come from the factory; mesh file/decode
//! work remains the shape host's responsibility.
use crate::{factory, native_body, native_part, native_resource_load};
use slr_physics::space::Space;
use std::{cell::RefCell, collections::HashMap};
thread_local! {
    static BINDINGS:RefCell<HashMap<u32,HashMap<u32,factory::CfgBinding>>>=RefCell::new(HashMap::new());
}
pub fn clear() {
    let resources = BINDINGS.with(|bindings| std::mem::take(&mut *bindings.borrow_mut()));
    for (_, bindings) in resources {
        for (_, binding) in bindings {
            factory::release_cfg(binding);
        }
    }
}
pub fn owns_payload(container: u32) -> bool {
    BINDINGS.with(|bindings| bindings.borrow().contains_key(&container))
}

/// Retain the logical counterparts of an actual type-e payload's raw references.
/// Cloning and texture replacement have already updated the intrusive raw graph;
/// this keeps the factory's identity/lifetime projection in sync with that graph.
pub fn synchronize_bindings(m: &Space, raw: u32) -> Result<(), String> {
    if m.r32(raw + 0x4c) != 0xe {
        return Err("NativeAssetBoundary: payload reference synchronization requires typee".into());
    }
    let logical = native_body::logical_resource(raw)
        .ok_or("NativeAssetBoundary: payload reference owner lacks live identity")?;
    let mut slots = vec![raw + 0x8c, raw + 0xcc];
    let count = m.r32(raw + 0xa0);
    let array = m.r32(raw + 0xa4);
    for i in 0..count {
        slots.push(array + i * 16);
    }
    let mut bindings = HashMap::new();
    for slot in slots {
        let id = m.r32(slot + 8);
        let binding = match factory::bind_cfg(m.r32(raw + 0x50), id, 0, 0) {
            Ok(binding) => binding,
            Err(error) => {
                for (_, binding) in bindings {
                    factory::release_cfg(binding);
                }
                return Err(error);
            }
        };
        bindings.insert(slot, binding);
    }
    let old = BINDINGS.with(|resources| resources.borrow_mut().insert(logical, bindings));
    if let Some(old) = old {
        for (_, binding) in old {
            factory::release_cfg(binding);
        }
    }
    Ok(())
}
struct DescriptorHost<'a> {
    id: u32,
    bindings: &'a mut HashMap<u32, factory::CfgBinding>,
}
impl crate::native_render_type::Host for DescriptorHost<'_> {
    fn resolve(&mut self, _m: &mut Space, id: u32) -> Result<u32, String> {
        factory::resolve_cfg(self.id, id)
    }
    fn bind(&mut self, m: &mut Space, slot: u32, id: u32, kind: u32) -> Result<(), String> {
        let binding = factory::bind_cfg(self.id, id, kind as i32, 0)?;
        let raw = native_body::raw_resource(m, binding.container)?;
        native_part::bind_reference(m, slot, raw);
        if let Some(old) = self.bindings.insert(slot, binding) {
            factory::release_cfg(old);
        }
        Ok(())
    }
}
pub fn synchronize_loaded(m: &Space, raw: u32) -> Result<(), String> {
    let logical = native_body::logical_resource(raw)
        .ok_or("NativeAssetBoundary: loaded payload lacks live resource identity")?;
    slr_resource::node::with_res(|resources| {
        let resource = resources
            .container_mut(logical)
            .ok_or("NativeAssetBoundary: loaded payload resource disappeared")?;
        resource.flags = m.r32(raw + 0x54);
        resource.load_state = m.r32(raw + 0x64);
        Ok(())
    })
}
/// Original54a790/53b710 receives actual bytes from a completed range or
/// synchronous package read. It never retrieves a captured descriptor result.
fn load_payload(m: &mut Space, raw: u32, data: u32) -> Result<(), String> {
    let kind = m.r32(raw + 0x4c);
    if !matches!(kind, 5 | 0xe) {
        return Err(format!("NativeAssetBoundary: descriptor loader type{kind}"));
    }
    if kind == 5 {
        m.w32(
            0x619994,
            u32::from(m.rf(raw + 0x6c) != 1000. && !m.rf(raw + 0x6c).is_nan()),
        );
    }
    if m.r32(raw + 0x64) & 1 != 0 {
        if kind == 0xe {
            m.wf(raw + 0x68, 1.);
        } else if crate::native_mesh_resource::refresh_progress(m, raw)? {
            <AccessAdapter as native_resource_load::AccessHost>::spatial_request(
                &mut AccessAdapter,
                m,
                raw,
                1f32.to_bits(),
                0,
                0,
            )?;
        }
        return synchronize_loaded(m, raw);
    }
    if data == 0 && kind == 0xe {
        native_resource_load::mark_loaded(m, raw, 1)?;
        m.w32(raw + 0x68, 0);
        return synchronize_loaded(m, raw);
    }
    let logical = native_body::logical_resource(raw)
        .ok_or("NativeAssetBoundary: descriptor target has no live identity")?;
    if owns_payload(logical) {
        return Err("NativeAssetBoundary: reload requires original payload destruction".into());
    }
    let bytes = (0..m.r32(raw + 0x5c))
        .map(|i| m.r8(data + i))
        .collect::<Vec<_>>();
    if kind == 0xe {
        let mut bindings = HashMap::new();
        let result = crate::native_render_type::load(
            m,
            raw + 0x80,
            &bytes,
            &mut DescriptorHost {
                id: m.r32(raw + 0x50),
                bindings: &mut bindings,
            },
        );
        BINDINGS.with(|resources| resources.borrow_mut().insert(logical, bindings));
        result?;
    } else {
        crate::native_mesh_resource::load(m, raw + 0x80, &bytes, &mut FileShape)?;
        BINDINGS.with(|resources| resources.borrow_mut().insert(logical, HashMap::new()));
    }
    native_resource_load::mark_loaded(m, raw, 1)?;
    m.wf(raw + 0x68, if kind == 0xe { 1. } else { 0. });
    synchronize_loaded(m, raw)
}
struct FileShape;
impl crate::native_shape::Host for FileShape {
    fn request_file(
        &mut self,
        m: &mut Space,
        path: &str,
        callback: u32,
        shape: u32,
    ) -> Result<i32, String> {
        crate::native_file::request_shape(m, path, callback, shape)
    }
}
impl crate::native_mesh_resource::Host for FileShape {
    fn create_shape(
        &mut self,
        m: &mut Space,
        kind: u32,
        asynchronous: u32,
        source: &str,
        skinned: u32,
        id: u32,
        slot: u32,
    ) -> Result<u32, String> {
        if asynchronous == 0 {
            return Err(
                "NativeShapeBoundary: synchronous505960 decode path is not integrated".into(),
            );
        }
        crate::native_shape::request_async(m, kind, source, skinned, id, slot, self)
    }
}

/// Header metadata only; descriptor payload bytes are read by the file worker.
pub fn descriptor_location(id: u32) -> Result<(String, u32, u32), String> {
    if id >> 16 == 0xffff {
        return Err("NativeAssetBoundary: runtime resource has no external descriptor".into());
    }
    crate::RES.with(|resources| {
        let mut resources = resources.borrow_mut();
        resources.request_metadata(id)?;
        let (name, package) = resources
            .packages
            .get((id >> 16) as usize)
            .ok_or("NativeAssetBoundary: descriptor package is absent")?;
        let package = package
            .as_ref()
            .ok_or("NativeAssetBoundary: descriptor headers unavailable")?;
        let entry = package
            .entries
            .iter()
            .find(|entry| entry.type_id == id & 0xffff)
            .ok_or("NativeAssetBoundary: descriptor header is absent")?;
        Ok((name.clone(), entry.rsd_offset, entry.rsd_size))
    })
}
pub(crate) struct AccessAdapter;
impl crate::native_resource_manager::Host for AccessAdapter {
    fn payload(&mut self, _m: &mut Space, raw: u32) -> Result<u32, String> {
        Ok(raw + 0x80)
    }
    fn minimum(&mut self, m: &mut Space, raw: u32, quality: u32) -> Result<(), String> {
        <Self as native_resource_load::AccessHost>::set_minimum(self, m, raw, quality)
    }
    fn package(&mut self, m: &mut Space, raw: u32) -> Result<u32, String> {
        let id = m.r32(raw + 0x50);
        let (name, _, _) = descriptor_location(id)?;
        let manager = m.r32(0x618d48);
        let package = manager + 0xffe94 + (id >> 16) * 0x70;
        if id >> 16 >= 256 {
            return Err("NativeAssetBoundary: package index exceeds original table".into());
        }
        if name.len() > 63 {
            return Err("NativeAssetBoundary: package name exceeds original descriptor".into());
        }
        m.w32(package + 8, id >> 16);
        for (i, b) in name
            .as_bytes()
            .iter()
            .chain(std::iter::once(&0))
            .enumerate()
        {
            m.w8(package + 12 + i as u32, *b);
        }
        Ok(package)
    }
    fn request_file(
        &mut self,
        m: &mut Space,
        package: u32,
        raw: u32,
        record: u32,
    ) -> Result<i32, String> {
        let path = (0..64)
            .map(|i| m.r8(package + 12 + i))
            .take_while(|b| *b != 0)
            .collect::<Vec<_>>();
        let path =
            std::str::from_utf8(&path).map_err(|_| "NativeAssetBoundary: non-UTF8 package name")?;
        let result = crate::native_file::request_range(
            m,
            path,
            m.r32(raw + 0x58),
            m.r32(raw + 0x5c),
            0x544540,
            record,
            m.r32(record + 0x14),
        )?;
        Ok(if result >= 0 { 0 } else { result })
    }
    fn read_descriptor(&mut self, m: &mut Space, _package: u32, raw: u32) -> Result<u32, String> {
        use std::io::{Read, Seek, SeekFrom};
        let (name, offset, length) = descriptor_location(m.r32(raw + 0x50))?;
        let mut file = crate::factory::open_cfg_file(&name)?;
        let data = m.malloc(length);
        if data == 0 {
            return Ok(0);
        }
        file.seek(SeekFrom::Start(offset.into()))
            .map_err(|e| format!("NativeAssetBoundary: descriptor seek: {e}"))?;
        let mut bytes = vec![0; length as usize];
        let count = file
            .read(&mut bytes)
            .map_err(|e| format!("NativeAssetBoundary: descriptor read: {e}"))?;
        for (i, b) in bytes[..count].iter().enumerate() {
            m.w8(data + i as u32, *b);
        }
        Ok(data)
    }
    fn load_descriptor(
        &mut self,
        m: &mut Space,
        raw: u32,
        data: u32,
        _quality: u32,
    ) -> Result<(), String> {
        load_payload(m, raw, data)
    }
    fn release_descriptor(&mut self, _m: &mut Space, _data: u32) -> Result<(), String> {
        Ok(())
    }
}
impl native_resource_load::AccessHost for AccessAdapter {
    fn set_minimum(&mut self, m: &mut Space, raw: u32, value: u32) -> Result<(), String> {
        match m.r32(raw + 0x4c) {
            0xe => native_resource_load::request_mesh_type(m, raw, value),
            3 => crate::native_render_resource::set_minimum(m, raw, value),
            1 | 2 | 5 | 8 => {
                if m.r32(raw + 0x7c) < m.r32(0x6200a4)
                    || !matches!(
                        m.rf(raw + 0x6c).partial_cmp(&f32::from_bits(value)),
                        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
                    )
                {
                    m.w32(raw + 0x6c, value);
                }
            }
            kind => {
                return Err(format!(
                    "NativeAssetBoundary: minimum virtual for type{kind}"
                ))
            }
        }
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
        crate::native_resource_manager::request_async(m, raw, quality, priority, time, self)
    }
    fn spatial_request(
        &mut self,
        m: &mut Space,
        raw: u32,
        quality: u32,
        _priority: u32,
        _time: u32,
    ) -> Result<(), String> {
        let needed = match m.r32(raw + 0x4c) {
            1 => true,
            2 => {
                return crate::native_spatial::request_physics(m, raw, [quality, _priority, _time])
            }
            //543c80 calls the embedded render getter5433d0, then requests
            //spatial work only for render payload flags 0800/4000.
            3 => m.r32(raw + 0xd8 + 0xbc) & 0x4800 != 0,
            0xe => true,
            5 => {
                let shape = m.r32(raw + 0x98);
                shape != 0
                    && m.r32(shape + 0x20) & 0x80000008 == 8
                    && m.r32(raw + 0x54) & 0x4000000 == 0
            }
            kind => {
                return Err(format!(
                    "NativeAssetBoundary: spatial virtual for type{kind}"
                ))
            }
        };
        if needed && f32::from_bits(quality) > 0. {
            self.set_minimum(m, raw, quality)?;
        }
        crate::native_spatial::schedule_request(m, raw, needed, [quality, _priority, _time])
    }
    fn quality_sensitive(&self, kind: u32) -> Result<bool, String> {
        match kind {
            1 | 2 | 3 | 5 | 8 | 0xe => Ok(false),
            _ => Err(format!("NativeAssetBoundary: quality table for type{kind}")),
        }
    }
    fn minimum_quality(&mut self, _m: &mut Space, _raw: u32) -> Result<f32, String> {
        Err("NativeAssetBoundary: unexpected quality-sensitive virtual".into())
    }
    fn refresh_quality(&mut self, _m: &mut Space, _raw: u32) -> Result<(), String> {
        Err("NativeAssetBoundary: unexpected quality-sensitive refresh".into())
    }
    fn touch(&mut self, m: &mut Space, raw: u32) -> Result<(), String> {
        m.w32(raw + 0x74, m.r32(0x6200a4));
        if m.r32(raw + 0x64) & 0x10 == 0 {
            let previous = m.r32(raw + 0x10);
            let next = m.r32(raw + 0xc);
            if previous != 0 && next != 0 {
                m.w32(next + 0x10, previous);
                m.w32(previous + 0xc, next);
            }
            m.w32(raw + 0xc, 0);
            m.w32(raw + 0x10, 0);
            let list = m.r32(0x618d48) + m.r32(raw + 0x4c) * 0x90 + 0x140;
            let tail = m.r32(list + 0x28);
            if tail == 0 {
                return Err("NativeAssetBoundary: current-generation list missing".into());
            }
            m.w32(tail + 0xc, raw);
            m.w32(raw + 0xc, list + 0x18);
            m.w32(raw + 0x10, tail);
            m.w32(list + 0x28, raw);
            m.w32(raw + 0x64, (m.r32(raw + 0x64) & !0x60) | 0x10);
        }
        Ok(())
    }
    fn payload(&mut self, m: &mut Space, raw: u32, _quality: u32) -> Result<u32, String> {
        Ok(raw
            + if matches!(m.r32(raw + 0x4c), 1..=3) {
                0xd8
            } else {
                0x80
            })
    }
}
/// Original resource virtual minimum and get-data operations, also used by
/// message routing's explicit prepare/get-data pair.
pub fn minimum(m: &mut Space, raw: u32) -> Result<(), String> {
    native_resource_load::AccessHost::set_minimum(&mut AccessAdapter, m, raw, 1f32.to_bits())
}
pub fn data(m: &mut Space, raw: u32) -> Result<u32, String> {
    native_resource_load::AccessHost::payload(&mut AccessAdapter, m, raw, 1f32.to_bits())
}
/// Real419860 access for descriptor-backed render resources. Returning zero
/// means a queued or failed resource, never a fabricated loaded payload.
pub fn access(m: &mut Space, raw: u32, args: native_resource_load::Access) -> Result<u32, String> {
    let result = native_resource_load::access(m, raw, args, &mut AccessAdapter)?;
    synchronize_loaded(m, raw)?;
    Ok(result)
}

/// Original5447d0 status-only access for physical/spatial collaborators.
pub fn prepare(m: &mut Space, raw: u32, args: native_resource_load::Access) -> Result<u32, String> {
    let result = native_resource_load::prepare(m, raw, args, &mut AccessAdapter)?;
    synchronize_loaded(m, raw)?;
    Ok(result)
}

/// Live identity adapter for original545230. Source loading executes the same
/// prepare/get-data pair as the original after clone_mesh_type requests detail.
struct CloneAdapter;
impl native_resource_load::CloneHost for CloneAdapter {
    fn allocate_clone(
        &mut self,
        m: &mut Space,
        source: u32,
        name: Option<&[u8]>,
    ) -> Result<u32, String> {
        let parent_raw = m.r32(source + 0x14);
        let parent = if parent_raw == 0 {
            None
        } else {
            Some(
                native_body::logical_resource(parent_raw)
                    .ok_or("NativeAssetBoundary: clone parent lacks live identity")?,
            )
        };
        let name_text = std::str::from_utf8(name.unwrap_or_default())
            .map_err(|_| "NativeAssetBoundary: clone name is not UTF-8")?;
        let logical = factory::create_resource(factory::ResourceDescriptor {
            kind: 0xe,
            flags: m.r32(source + 0x54),
            parent,
            template: None,
            name: name_text,
        })?;
        let raw = native_body::raw_resource_named(m, logical, Some(name))?;
        native_resource_load::register_unloaded(m, raw)?;
        synchronize_loaded(m, raw)?;
        Ok(raw)
    }
    fn source_payload(&mut self, m: &mut Space, source: u32) -> Result<Option<u32>, String> {
        let status = prepare(
            m,
            source,
            native_resource_load::Access {
                flags: 0x80000000,
                quality: 1f32.to_bits(),
                priority: 0,
                time: 0,
            },
        )?;
        if status & 0x80000000 != 0 {
            return Ok(None);
        }
        let payload = <AccessAdapter as native_resource_load::AccessHost>::payload(
            &mut AccessAdapter,
            m,
            source,
            1f32.to_bits(),
        )?;
        Ok((payload != 0).then_some(payload))
    }
    fn destroy_clone(&mut self, _m: &mut Space, _target: u32) -> Result<(), String> {
        Err("NativeAssetBoundary: failed clone requires original typee deleting destructor".into())
    }
    fn spatial_request(&mut self, m: &mut Space, target: u32) -> Result<(), String> {
        <AccessAdapter as native_resource_load::AccessHost>::spatial_request(
            &mut AccessAdapter,
            m,
            target,
            1f32.to_bits(),
            0,
            0,
        )
    }
    fn synchronize_loaded(&mut self, m: &mut Space, target: u32) -> Result<(), String> {
        synchronize_loaded(m, target)?;
        synchronize_bindings(m, target)
    }
}
pub fn clone_render_type(m: &mut Space, source: u32) -> Result<u32, String> {
    native_resource_load::clone_mesh_type(m, source, &mut CloneAdapter).map(|raw| raw.unwrap_or(0))
}

#[cfg(test)]
mod clone_tests {
    use super::*;
    #[test]
    fn live_clone_preserves_parent_identity_and_retains_payload_references() {
        factory::configure(None, None);
        let parent = factory::game_root().unwrap();
        let make = |kind, name| {
            factory::create_resource(factory::ResourceDescriptor {
                kind,
                flags: 0,
                parent: Some(parent),
                template: None,
                name,
            })
            .unwrap()
        };
        let source = make(14, "live clone source");
        let mesh = make(5, "live clone mesh");
        native_body::with_world_memory(|m| {
            let source_raw = native_body::raw_resource(m, source).unwrap();
            let mesh_raw = native_body::raw_resource(m, mesh).unwrap();
            native_part::bind_reference(m, source_raw + 0x8c, mesh_raw);
            native_resource_load::mark_loaded(m, source_raw, 1).unwrap();
            m.wf(source_raw + 0x68, 1.);
            synchronize_loaded(m, source_raw).unwrap();
            let target = clone_render_type(m, source_raw).unwrap();
            let logical = native_body::logical_resource(target).unwrap();
            assert_ne!(logical, source);
            assert_eq!(m.r32(target + 0x14), m.r32(source_raw + 0x14));
            assert_eq!(m.r32(target + 0x98), mesh_raw);
            assert_eq!(m.r32(target + 0x50) >> 16, 0xffff);
            assert_eq!(m.r32(target + 0x54) & 0x200, 0x200);
            assert_eq!(m.r32(target + 0x64) & 1, 1);
            assert!(owns_payload(logical));
            slr_resource::node::with_res(|host| {
                let cloned = host.container(logical).unwrap();
                assert_eq!(cloned.parent, parent);
                assert_eq!(cloned.load_state, m.r32(target + 0x64));
                let retained =
                    BINDINGS.with(|bindings| bindings.borrow()[&logical][&(target + 0x8c)]);
                assert_eq!(retained.container, mesh);
                assert_eq!(host.node(retained.link).unwrap().container, mesh);
            });
        })
        .unwrap();
    }
}

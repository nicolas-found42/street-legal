//! Original5439e0 template binding and zero-bone539880 instance tails.
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};

fn inherit_range(
    m: &mut Space,
    resource: u32,
    parent: u32,
    offset: u32,
    stamp: u32,
    update_stamp: bool,
) {
    set_range(
        m,
        resource,
        m.r32(parent + offset),
        offset,
        stamp,
        update_stamp,
    );
}

/// exe: FUN_00543610
pub fn set_minimum(m: &mut Space, resource: u32, value: u32) {
    set_range(m, resource, value, 0x6c, 0x78, true);
}

fn set_range(
    m: &mut Space,
    resource: u32,
    input: u32,
    offset: u32,
    stamp: u32,
    update_stamp: bool,
) {
    let pc = Precision::F32;
    let rc = Rounding::NearestEven;
    let f = |v| X87::from_f32(f32::from_bits(v));
    let value = f(input)
        .mul(f(m.r32(resource + 0xbc)), pc, rc)
        .add(f(m.r32(resource + 0xb8)), pc, rc)
        .to_f32(pc, rc);
    let value = if !matches!(
        0f32.partial_cmp(&value),
        Some(std::cmp::Ordering::Less | std::cmp::Ordering::Equal)
    ) {
        0.
    } else if value > 1. {
        1.
    } else {
        value
    };
    if m.r32(resource + stamp) < m.r32(0x6200a4) || value > f32::from_bits(m.r32(resource + offset))
    {
        m.w32(resource + offset, value.to_bits());
    }
    if update_stamp {
        m.w32(resource + stamp, m.r32(0x6200a4));
    }
}

pub fn bind_template(m: &mut Space, body: u32, template: u32) {
    if m.r32(template + 0x50) != m.r32(body + 0x80) {
        crate::native_part::bind_reference(m, body + 0x78, template);
        m.w32(body + 0x88, 1);
    }
}

/// The constructor and resource allocator execute before this stage. Camera
/// types have no material geometry to decode.
/// Original539880 with no variadic bones. Types6/d/14 have the same tail as
/// cameras and select their actual rendering mode. Typee also obtains bounds
/// from its loaded template payload and must execute that collaborator first.
pub trait Host {
    fn mark_loaded(&mut self, m: &mut Space, resource: u32) -> Result<(), String>;
    fn mesh_payload(&mut self, m: &mut Space, template: u32) -> Result<u32, String>;
}

/// Original539880 zero-bone construction, including callback-visible ordering.
/// Type-e get-data runs only after binding, inherited bounds, and mode setup.
pub fn finish_instance(
    m: &mut Space,
    resource: u32,
    template: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    let kind = m.r32(template + 0x4c);
    let mode = match kind {
        6 => 6,
        0xc => 1,
        0xd => 2,
        0x14 => 5,
        0xe => 3,
        kind => {
            return Err(format!(
                "NativeRenderBoundary: unsupported render template kind{kind}"
            ))
        }
    };
    let body = resource + 0xd8;
    let head = m.r32(body + 0x12c);
    if head == 0 || m.r32(head + 4) != 0 {
        return Err(
            "NativeRenderBoundary: new render instance needs a constructed empty bone list".into(),
        );
    }
    let parent = m.r32(resource + 0x14);
    if parent != 0 {
        inherit_range(m, resource, parent, 0x70, 0x7c, false);
        inherit_range(m, resource, parent, 0x6c, 0x78, true);
        m.w32(resource + 0x68, m.r32(parent + 0x68));
    }
    m.w32(0x765f94, m.r32(0x765f94).wrapping_add(1));
    m.w32(resource + 0x54, m.r32(resource + 0x54) | 0x200);
    host.mark_loaded(m, resource)?;
    m.w32(resource + 0x68, 1f32.to_bits());
    bind_template(m, body, template);
    crate::native_part::bind_reference(m, body + 0xdc, resource);
    if parent != 0 {
        for offset in (0x80..0x9c).step_by(4) {
            m.w32(resource + offset, m.r32(parent + offset));
        }
    }
    m.w32(body + 0x8c, mode);
    if kind == 0xe {
        let payload = host.mesh_payload(m, template)?;
        if payload != 0 {
            let shape = m.r32(payload + 0x28);
            match (m.r32(resource + 0x80), shape) {
                (0, 0) => m.w32(resource + 0x90, m.r32(payload + 0x38)),
                (0, 1) => {
                    let pc = Precision::F32;
                    let rc = Rounding::NearestEven;
                    let f = |offset| X87::from_f32(m.rf(payload + offset));
                    let z = f(0x40);
                    let y = f(0x3c);
                    let x = f(0x38);
                    let value = x
                        .mul(x, pc, rc)
                        .add(y.mul(y, pc, rc), pc, rc)
                        .add(z.mul(z, pc, rc), pc, rc)
                        .sqrt(pc, rc)
                        .to_f32(pc, rc);
                    m.w32(resource + 0x90, value.to_bits());
                }
                (1, 0) => {
                    for offset in [0x90, 0x94, 0x98] {
                        m.w32(resource + offset, m.r32(payload + 0x38));
                    }
                }
                (1, 1) => {
                    for axis in 0..3 {
                        m.w32(resource + 0x90 + axis * 4, m.r32(payload + 0x38 + axis * 4));
                    }
                }
                _ => (),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn minimum_quality_matches_original_arithmetic_and_generations() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-minimum-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_render_minimum_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            m.w32(0x6200a4, 123);
            let raw = row["resource"].as_u64().unwrap() as u32;
            for (i, word) in row["before"].as_array().unwrap().iter().enumerate() {
                m.w32(raw + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            set_minimum(&mut m, raw, row["value"].as_u64().unwrap() as u32);
            for (i, word) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(raw + i as u32 * 4),
                    word.as_u64().unwrap() as u32,
                    "value={} word={i}",
                    row["value"]
                );
            }
        }
    }
    #[test]
    fn original_zero_bone_instance_tails_match_complete_buffers() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-render-resource-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_render_resource_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            m.w32(0x6200a4, 123);
            for (base, words) in row["before"].as_object().unwrap() {
                let base: u32 = base.parse().unwrap();
                for (i, word) in words.as_array().unwrap().iter().enumerate() {
                    m.w32(base + i as u32 * 4, word.as_u64().unwrap() as u32);
                }
            }
            let mesh = if row["kind"].as_u64() == Some(0xe) {
                Some(if row["present"].as_u64() == Some(0) {
                    0
                } else {
                    row["mesh"].as_u64().unwrap() as u32
                })
            } else {
                None
            };
            struct OriginalCallbacks<'a> {
                row: &'a serde_json::Value,
                resource: u32,
                mesh: Option<u32>,
            }
            impl Host for OriginalCallbacks<'_> {
                fn mark_loaded(&mut self, m: &mut Space, resource: u32) -> Result<(), String> {
                    m.w32(resource + 0x64, m.r32(resource + 0x64) | 1);
                    Ok(())
                }
                fn mesh_payload(&mut self, m: &mut Space, _: u32) -> Result<u32, String> {
                    let callback = &self.row["callbacks"][0];
                    assert_eq!(m.r32(0x765f94), callback["count"].as_u64().unwrap() as u32);
                    for (i, word) in callback["resource"].as_array().unwrap().iter().enumerate() {
                        let address = self.resource + i as u32 * 4;
                        assert_eq!(
                            m.r32(address),
                            word.as_u64().unwrap() as u32,
                            "original get-data callback state at {address:08x}"
                        );
                    }
                    Ok(self.mesh.unwrap())
                }
            }
            finish_instance(
                &mut m,
                row["resource"].as_u64().unwrap() as u32,
                row["template"].as_u64().unwrap() as u32,
                &mut OriginalCallbacks {
                    row,
                    resource: row["resource"].as_u64().unwrap() as u32,
                    mesh,
                },
            )
            .unwrap();
            assert_eq!(m.r32(0x765f94), 1);
            for (base, words) in row["after"].as_object().unwrap() {
                let base: u32 = base.parse().unwrap();
                for (i, word) in words.as_array().unwrap().iter().enumerate() {
                    let address = base + i as u32 * 4;
                    assert_eq!(
                        m.r32(address),
                        word.as_u64().unwrap() as u32,
                        "parented={} address={address:08x}",
                        row["parented"]
                    );
                }
            }
        }
    }
}

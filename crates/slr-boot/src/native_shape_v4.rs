//! Original502de0 sequential SCX4 chunks and503400/5038e0 CPU geometry.
//! Graphics material creation and object finalization are live device callbacks.
//!
//! Every entry point needs that device: `create_material`, `material_description`
//! and `finalize_object` are `Host` methods supplied by the embedding. No
//! headless device implements them yet, so the decoder has no production
//! caller. It stays compiled and oracle-tested against the original's CPU
//! material/object/vertex/index graphs.
#![allow(dead_code)]
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn x(v: u32) -> X87 {
    X87::from_f32(f32::from_bits(v))
}
fn b(v: X87) -> u32 {
    v.to_f32(PC, RC).to_bits()
}
#[derive(Clone, Copy)]
pub struct Channel {
    pub source: i32,
    pub scale: [u32; 2],
    pub offset: [u32; 2],
}
pub struct MaterialDescription {
    pub channels: Vec<Channel>,
}
pub trait Host {
    fn create_material(&mut self, m: &mut Space, payload: u32, length: u32) -> Result<u32, String>;
    fn material_description(
        &mut self,
        m: &mut Space,
        material: u32,
    ) -> Result<MaterialDescription, String>;
    fn finalize_object(&mut self, m: &mut Space, object: u32) -> Result<(), String>;
}
fn append(m: &mut Space, header: u32, value: u32, increment: u32) -> Result<(), String> {
    let count = m.r32(header + 4);
    let old_capacity = m.r32(header + 8);
    if count > old_capacity {
        return Err("NativeScx4Boundary: dynamic array count exceeds capacity".into());
    }
    if count == old_capacity {
        let capacity = old_capacity
            .checked_add(increment)
            .ok_or("NativeScx4Boundary: array capacity overflow")?;
        let data = m.malloc(
            capacity
                .checked_mul(4)
                .ok_or("NativeScx4Boundary: array byte overflow")?,
        );
        for i in 0..old_capacity {
            m.w32(data + i * 4, m.r32(m.r32(header) + i * 4));
        }
        m.w32(header, data);
        m.w32(header + 8, capacity);
    }
    m.w32(m.r32(header) + count * 4, value);
    m.w32(header + 4, count + 1);
    Ok(())
}
fn object(m: &mut Space) -> u32 {
    let p = m.malloc(0x54);
    m.w32(p, 0x5f2c08);
    for off in [8, 12, 16, 0x14, 0x1c, 0x20, 0x24, 0x38, 0x3c, 0x40, 0x50] {
        m.w32(p + off, 0);
    }
    m.w32(p + 0x18, 0x80000001);
    p
}
/// Original4fe980 vertex format and offsets; executable constants5f2918..2958.
fn format(m: &mut Space, state: u32, flags: u32) {
    m.w32(state, 0);
    let mut stride = 0;
    let mut native = 0;
    if flags & 1 != 0 {
        stride = 12;
        let weights = (flags >> 8) & 7;
        let count = weights + u32::from(flags & 0x800 != 0);
        if count <= 4 {
            native = [2, 6, 8, 10, 12][count as usize];
        }
        for i in 0..weights {
            m.w32(state + 0x30 + i * 4, stride);
            stride += 4;
        }
    }
    if flags & 0x800 != 0 {
        m.w32(state + 0x40, stride);
        native |= 0x8000;
        stride += 4;
    }
    for (bit, offset, size, code) in [(2, 4, 12, 0x10), (4, 8, 4, 0x40), (8, 12, 4, 0x80)] {
        if flags & bit != 0 {
            m.w32(state + offset, stride);
            stride += size;
            native |= code;
        }
    }
    let textures = (flags >> 4) & 15;
    native |= textures << 8;
    for i in 0..textures {
        let shift = (16 + i * 2) & 31;
        let kind = (flags >> shift) & 3;
        m.w32(state + 0x10 + i * 4, stride);
        stride += match kind {
            0 => 4,
            1 => 8,
            2 => 12,
            _ => 16,
        };
        native |= match kind {
            0 => 3 << shift,
            1 => 0,
            2 => 1 << shift,
            _ => 2 << shift,
        };
    }
    m.w32(state + 0x44, stride);
    m.w32(state + 0x48, native);
}
fn write_vertex(m: &mut Space, buffer: u32, index: u32) {
    let flags = m.r32(buffer + 12);
    let dest = m.r32(buffer + 8) + m.r32(buffer + 0x60) * index;
    let copy = |m: &mut Space, offset: u32, global: u32, count: u32| {
        for i in 0..count {
            m.w32(dest + m.r32(buffer + offset) + i * 4, m.r32(global + i * 4));
        }
    };
    if flags & 1 != 0 {
        copy(m, 0x1c, 0x65bc68, 3);
        for i in 0..((flags >> 8) & 7) {
            copy(m, 0x4c + i * 4, 0x65bd10 + i * 4, 1);
        }
    }
    if flags & 0x800 != 0 {
        copy(m, 0x5c, 0x65bd20, 1);
    }
    for (bit, offset, global, count) in [
        (2, 0x20, 0x65bc78, 3),
        (4, 0x24, 0x65bc88, 1),
        (8, 0x28, 0x65bc8c, 1),
    ] {
        if flags & bit != 0 {
            copy(m, offset, global, count);
        }
    }
    for i in 0..((flags >> 4) & 15) {
        let n = 1 + ((flags >> ((16 + i * 2) & 31)) & 3);
        copy(m, 0x2c + i * 4, 0x65bc90 + i * 16, n);
    }
}
/// Original5038e0, including its triangle-only copy and preserved trailing slots.
pub fn indices(
    m: &mut Space,
    shape: u32,
    object: u32,
    data: u32,
    length: u32,
) -> Result<(), String> {
    if length < 4 {
        return Err("NativeScx4Boundary: truncated index count".into());
    }
    let count = m.r32(data);
    if count > 0x1000000 || count * 2 > length - 4 {
        return Err("NativeScx4Boundary: truncated/oversized index data".into());
    }
    m.w32(shape + 0xe8, m.r32(shape + 0xe8).max(count));
    let wrapper = m.malloc(0x14);
    let array = m.malloc(count * 2);
    for (offset, value) in [(0, count), (4, array), (8, 2), (12, 2)] {
        m.w32(wrapper + offset, value);
    }
    m.w32(object + 0x20, wrapper);
    for i in 0..count / 3 * 3 {
        m.w8(array + i * 2, m.r8(data + 4 + i * 2));
        m.w8(array + i * 2 + 1, m.r8(data + 5 + i * 2));
    }
    Ok(())
}
/// Original503400 input flags and per-vertex scratch state. Device material
/// description supplies its actual UV transforms; vertex buffers stay in Space.
pub fn vertices(
    m: &mut Space,
    shape: u32,
    group: u32,
    object: u32,
    data: u32,
    length: u32,
    host: &mut dyn Host,
) -> Result<(), String> {
    if length < 8 {
        return Err("NativeScx4Boundary: truncated vertex header".into());
    }
    let count = m.r32(data);
    let flags = m.r32(data + 4);
    if count == 0 || count > 0x1000000 {
        return Err("NativeScx4Boundary: zero/oversized vertex count".into());
    }
    let stride = (length - 8) / count;
    if stride * count != length - 8 {
        return Err("NativeScx4Boundary: nonintegral vertex stride".into());
    }
    m.w32(shape + 0xe4, m.r32(shape + 0xe4).max(count));
    let description = host.material_description(m, m.r32(group))?;
    if description.channels.len() > 8 {
        return Err("NativeScx4Boundary: material texture channels exceed eight".into());
    }
    let textures = description.channels.len() as u32;
    let mut out = u32::from(flags & 1 != 0);
    for (from, to) in [(0x40, 2), (0x80, 4), (0x100, 8)] {
        if flags & from != 0 {
            out |= to;
        }
    }
    if flags & 0x1c != 0 {
        out |= 0x300;
    }
    if flags & 0x20 != 0 {
        out |= 0x800;
    }
    out |= textures << 4;
    for i in 0..textures {
        out |= 1u32 << ((16 + i * 2) & 31);
    }
    let buffer = m.malloc(0x68);
    m.w32(buffer + 12, out);
    format(m, buffer + 0x1c, out);
    m.w32(buffer + 4, count);
    let array = m.malloc(
        m.r32(buffer + 0x60)
            .checked_mul(count)
            .ok_or("NativeScx4Boundary: vertex output overflow")?,
    );
    m.w32(buffer + 8, array);
    m.w32(object + 0x1c, buffer);
    m.w32(0x65bd40, m.r32(0x65bd40).wrapping_add(count));
    m.w32(0x65bd48, m.r32(0x65bd48).wrapping_add(1));
    m.w32(
        shape + 0x38,
        m.r32(shape + 0x38)
            .wrapping_add(m.r32(buffer + 0x60) * count),
    );
    let mut texture_offsets = [0; 8];
    let mut tex_offset = 0;
    for (i, slot) in texture_offsets.iter_mut().enumerate() {
        *slot = tex_offset;
        if flags & (0x200 << i) != 0 {
            tex_offset += 8;
        }
    }
    let mut diffuse = u32::MAX;
    let mut specular = u32::MAX;
    for index in 0..count {
        let start = data + 8 + index * stride;
        let mut cursor = start;
        if flags & 1 != 0 {
            let scaled = [m.r32(cursor), m.r32(cursor + 4), m.r32(cursor + 8)]
                .map(|v| b(x(v).mul(x(0x3dcccccd), PC, RC)));
            for axis in 0..3 {
                m.w32(0x65bc68 + axis * 4, scaled[axis as usize]);
            }
            if m.r32(shape + 0xec) != 0 {
                for axis in 0..3 {
                    m.w32(shape + 0xcc + axis * 4, scaled[axis as usize]);
                    m.w32(shape + 0xd8 + axis * 4, scaled[axis as usize]);
                }
                m.w32(shape + 0xec, 0);
            } else {
                for axis in 0..3 {
                    let value = f32::from_bits(scaled[axis as usize]);
                    if value < m.rf(shape + 0xcc + axis * 4) {
                        m.w32(shape + 0xcc + axis * 4, scaled[axis as usize]);
                    }
                    if m.rf(shape + 0xd8 + axis * 4) < value {
                        m.w32(shape + 0xd8 + axis * 4, scaled[axis as usize]);
                    }
                }
            }
            cursor += 12;
        }
        m.w32(0x65bd10, 1f32.to_bits());
        m.w32(0x65bd14, 0);
        m.w32(0x65bd18, 0);
        let weights = if flags & 4 != 0 {
            1
        } else if flags & 8 != 0 {
            2
        } else if flags & 16 != 0 {
            3
        } else {
            0
        };
        for i in 0..weights {
            m.w32(0x65bd10 + i * 4, m.r32(cursor));
            cursor += 4;
        }
        if flags & 0x20 != 0 {
            m.w32(0x65bd20, m.r32(cursor));
            cursor += 4;
        }
        if flags & 0x40 != 0 {
            for i in 0..3 {
                m.w32(0x65bc78 + i * 4, m.r32(cursor + i * 4));
            }
            cursor += 12;
        }
        if flags & 0x80 != 0 {
            diffuse = m.r32(cursor);
            cursor += 4;
        }
        if flags & 0x100 != 0 {
            specular = m.r32(cursor);
            cursor += 4;
        }
        if cursor + tex_offset > start + stride {
            return Err("NativeScx4Boundary: vertex attributes exceed stride".into());
        }
        for (i, channel) in description.channels.iter().take(4).enumerate() {
            let (u, v) = if channel.source == -1 {
                (
                    x(m.r32(0x65bc68)).sub(x(0x3f000000), PC, RC),
                    x(m.r32(0x65bc70) ^ 0x80000000),
                )
            } else {
                if !(0..8).contains(&channel.source) {
                    return Err(
                        "NativeScx4Boundary: material UV source outside eight channels".into(),
                    );
                }
                let tex = cursor + texture_offsets[channel.source as usize];
                (
                    x(m.r32(tex)).sub(x(0x3f000000), PC, RC),
                    x(1f32.to_bits()).sub(x(m.r32(tex + 4)), PC, RC),
                )
            };
            let v = b(v.sub(x(0x3f000000), PC, RC));
            m.w32(
                0x65bc90 + i as u32 * 16,
                b(u.sub(x(channel.offset[0]), PC, RC)
                    .mul(x(channel.scale[0]), PC, RC)
                    .add(x(0x3f000000), PC, RC)),
            );
            m.w32(
                0x65bc94 + i as u32 * 16,
                b(x(0x3f000000).sub(
                    x(v).sub(x(channel.offset[1]), PC, RC)
                        .mul(x(channel.scale[1]), PC, RC),
                    PC,
                    RC,
                )),
            );
        }
        m.w32(0x65bc88, diffuse | 0xff000000);
        m.w32(0x65bc8c, specular);
        write_vertex(m, buffer, index);
    }
    if m.r32(object + 0x14) == u32::MAX {
        let mode = if flags & 2 != 0 {
            2
        } else if flags & 4 != 0 {
            3
        } else if flags & 8 != 0 {
            4
        } else if flags & 16 != 0 {
            5
        } else {
            u32::MAX
        };
        m.w32(object + 0x14, mode);
    }
    Ok(())
}
fn finish_bounds(m: &mut Space, shape: u32) {
    let minimum = [
        m.r32(shape + 0xcc),
        m.r32(shape + 0xd0),
        m.r32(shape + 0xd4),
    ];
    let maximum = [
        m.r32(shape + 0xd8),
        m.r32(shape + 0xdc),
        m.r32(shape + 0xe0),
    ];
    let square = |v: u32| x(v).mul(x(v), PC, RC);
    let min_length =
        square(minimum[1])
            .add(square(minimum[2]), PC, RC)
            .add(square(minimum[0]), PC, RC);
    let max_length =
        square(maximum[1])
            .add(square(maximum[2]), PC, RC)
            .add(x(b(square(maximum[0]))), PC, RC);
    let length = if max_length.to_f32(PC, RC) < min_length.to_f32(PC, RC) {
        min_length
    } else {
        max_length
    };
    let radius = b(length.sqrt(PC, RC).mul(x(0x3dcccccd), PC, RC));
    m.w32(shape + 0x54, 0);
    m.w32(shape + 0x70, 1);
    for axis in 0..3 {
        m.w32(shape + 0x58 + axis * 4, 0);
        m.w32(shape + 0x64 + axis * 4, radius);
        let sum = x(minimum[axis as usize]).add(x(maximum[axis as usize]), PC, RC);
        let diff = x(maximum[axis as usize]).sub(x(minimum[axis as usize]), PC, RC);
        let sum = if axis == 0 { sum } else { x(b(sum)) };
        let diff = if axis == 0 { diff } else { x(b(diff)) };
        m.w32(shape + 0x74 + axis * 4, b(sum.mul(x(0x3d4ccccd), PC, RC)));
        m.w32(
            shape + 0x80 + axis * 4,
            b(diff.mul(x(0x3d4ccccd), PC, RC)) & 0x7fffffff,
        );
    }
    if m.r32(shape + 0xec) == 1 {
        for bounds in [shape + 0x54, shape + 0x70] {
            m.w32(bounds, 2);
            for i in 1..7 {
                m.w32(bounds + i * 4, 0);
            }
        }
    }
    m.w32(shape + 0x20, (m.r32(shape + 0x20) & !2) | 0x14);
    m.w32(shape + 0x34, m.r32(0x6200a4));
}
/// Original502de0. The supplied shape retains the real completed file bytes at
/// +28/+2c. SCX4 header has12bytes +8per table entry, then sequential chunks.
pub fn decode(m: &mut Space, shape: u32, host: &mut dyn Host) -> Result<(), String> {
    let data = m.r32(shape + 0x28);
    let length = m.r32(shape + 0x2c);
    if data == 0 || length < 12 {
        return Err("NativeScx4Boundary: missing completed SCX4 file".into());
    }
    let count = m.r32(data + 8) as i32;
    let mut cursor = data
        .checked_add(12)
        .and_then(|v| v.checked_add((count.max(0) as u32).checked_mul(8)?))
        .ok_or("NativeScx4Boundary: SCX chunk table overflow")?;
    for offset in (0xcc..=0xe8).step_by(4) {
        m.w32(shape + offset, 0);
    }
    m.w32(shape + 0xec, 1);
    let mut group = 0;
    let mut current = 0;
    for _ in 0..count.max(0) {
        if cursor < data || cursor + 8 > data + length {
            return Err("NativeScx4Boundary: truncated SCX chunk header".into());
        }
        let tag = m.r32(cursor);
        let size = m.r32(cursor + 4);
        if size < 8 || size > data + length - cursor {
            return Err("NativeScx4Boundary: truncated SCX chunk payload".into());
        }
        let payload = cursor + 8;
        match tag {
            0 => {
                group = m.malloc(0x18);
                for off in [4, 8, 12] {
                    m.w32(group + off, 0);
                }
                let material = host.create_material(m, payload, size - 8)?;
                m.w32(group, material);
                append(m, shape + 0x48, group, 16)?;
            }
            1..=3 => {
                if group == 0 {
                    return Err("NativeScx4Boundary: object before material group".into());
                }
                current = object(m);
                if tag == 2 {
                    if size < 20 {
                        return Err("NativeScx4Boundary: short two-bone object".into());
                    }
                    for off in [4, 8] {
                        append(m, current + 8, m.r32(payload + off), 2)?;
                    }
                    m.w32(current + 0x14, 1);
                }
                if tag == 3 {
                    if size < 16 {
                        return Err("NativeScx4Boundary: short bone-list object".into());
                    }
                    let n = m.r32(payload + 4);
                    if n > 0x100000 || n * 4 > size - 16 {
                        return Err("NativeScx4Boundary: truncated bone list".into());
                    }
                    for i in 0..n {
                        append(m, current + 8, m.r32(payload + 8 + i * 4), 2)?;
                    }
                    m.w32(current + 0x14, u32::MAX);
                }
                append(m, group + 4, current, 16)?;
            }
            4 => {
                if group == 0 || current == 0 {
                    return Err("NativeScx4Boundary: vertex chunk before material/object".into());
                }
                vertices(m, shape, group, current, payload, size - 8, host)?;
            }
            5 => {
                if current == 0 {
                    return Err("NativeScx4Boundary: index chunk before object".into());
                }
                indices(m, shape, current, payload, size - 8)?;
            }
            _ => (),
        }
        cursor += size;
    }
    let skinned = m.r32(shape + 0x44) > 2;
    m.w32(shape + 0x10c, u32::from(skinned));
    for i in 0..m.r32(shape + 0x4c) {
        let group = m.r32(m.r32(shape + 0x48) + i * 4);
        for j in 0..m.r32(group + 8) {
            let object = m.r32(m.r32(group + 4) + j * 4);
            m.w32(object + 0x4c, if skinned { m.r32(shape + 0x44) } else { 0 });
            m.w32(object + 0x48, u32::from(skinned));
            host.finalize_object(m, object)?;
        }
    }
    finish_bounds(m, shape);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Device {
        material: u32,
        channels: Vec<Channel>,
        calls: Vec<serde_json::Value>,
    }
    impl Host for Device {
        fn create_material(
            &mut self,
            _m: &mut Space,
            payload: u32,
            _length: u32,
        ) -> Result<u32, String> {
            self.calls.push(serde_json::json!(["material", payload, 0]));
            Ok(self.material)
        }
        fn material_description(
            &mut self,
            _m: &mut Space,
            material: u32,
        ) -> Result<MaterialDescription, String> {
            assert_eq!(material, self.material);
            Ok(MaterialDescription {
                channels: self.channels.clone(),
            })
        }
        fn finalize_object(&mut self, _m: &mut Space, object: u32) -> Result<(), String> {
            self.calls.push(serde_json::json!(["finalize", object]));
            Ok(())
        }
    }
    #[test]
    fn original_scx4_cpu_material_object_vertex_index_graphs_and_bounds_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-shape-v4-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_shape_v4_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut m = Space::new();
            let before = row["before"].as_array().unwrap();
            for (i, v) in before.iter().enumerate() {
                m.w32(0x20000000 + i as u32 * 4, v.as_u64().unwrap() as u32);
            }
            for (address, value) in row["globals_before"].as_object().unwrap() {
                m.w32(address.parse().unwrap(), value.as_u64().unwrap() as u32);
            }
            m.set_heap_ptr(row["start"].as_u64().unwrap() as u32);
            m.w32(0x6200a4, 123);
            let channels = row["descriptions"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| Channel {
                    source: v[0].as_u64().unwrap() as u32 as i32,
                    scale: [v[1].as_u64().unwrap() as u32, v[2].as_u64().unwrap() as u32],
                    offset: [v[3].as_u64().unwrap() as u32, v[4].as_u64().unwrap() as u32],
                })
                .collect();
            let mut host = Device {
                material: row["material"].as_u64().unwrap() as u32,
                channels,
                calls: vec![],
            };
            decode(&mut m, row["shape"].as_u64().unwrap() as u32, &mut host).unwrap();
            assert_eq!(
                m.heap_ptr() as u64,
                row["end"].as_u64().unwrap(),
                "profile={}",
                row["profile"]
            );
            assert_eq!(serde_json::to_value(&host.calls).unwrap(), row["calls"]);
            for (i, v) in row["after"].as_array().unwrap().iter().enumerate() {
                assert_eq!(
                    m.r32(0x20000000 + i as u32 * 4),
                    v.as_u64().unwrap() as u32,
                    "profile={} word{i}",
                    row["profile"]
                );
            }
            for (address, value) in row["globals"].as_object().unwrap() {
                assert_eq!(
                    m.r32(address.parse().unwrap()),
                    value.as_u64().unwrap() as u32,
                    "profile={} global{address}",
                    row["profile"]
                );
            }
        }
    }
}

//! Original 546e10 type-9 collision resource payload materialization.
use slr_physics::cfg::ScanBits;
use slr_physics::space::Space;
use slr_x87::{Precision, Rounding, X87};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
fn x(bits: u32) -> X87 {
    X87::from_f32(f32::from_bits(bits))
}
fn bits(value: X87) -> u32 {
    value.to_f32(PC, RC).to_bits()
}

pub struct Definition {
    pub shape: String,
    kind: u32,
    friction: u32,
    collision: u32,
    skin: u32,
    mass: u32,
}
impl Definition {
    /// The original recognizes these case-sensitive words and ignores unknown
    /// records, including `inertia`, whose branch has no implementation.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut result = Self {
            shape: String::new(),
            kind: 7,
            friction: 0x3f19999a,
            collision: 0x3e4ccccd,
            skin: 0,
            mass: 0,
        };
        for line in text.lines() {
            let key = line.split_whitespace().next().unwrap_or("");
            let input = format!("_\0{}", line.split_once(key).map_or("", |(_, rest)| rest));
            match key {
                "shape" => {
                    if let Some(path) = line.split_whitespace().nth(1) {
                        result.shape = path.into();
                    }
                }
                "type" => {
                    if let Some(value) = slr_physics::cfg::scan(input.as_bytes(), "%d").d(0) {
                        result.kind = value as u32;
                    }
                }
                "friction" | "coll_coeff" | "skin" | "mass" => {
                    if let Some(value) = slr_physics::cfg::scan(input.as_bytes(), "%f").fb(0) {
                        match key {
                            "friction" => result.friction = value,
                            "coll_coeff" => result.collision = value,
                            "skin" => result.skin = value,
                            _ => result.mass = value,
                        }
                    }
                }
                _ => {}
            }
        }
        if result.shape.is_empty() {
            return Err("NativeDefinitionBoundary: collision resource has no shape".into());
        }
        if result.kind == 6 {
            return Err(
                "NativeDefinitionBoundary: type6 collision resource requires original4ac150 graph"
                    .into(),
            );
        }
        Ok(result)
    }
    /// Materialize the definition and its embedded standalone body. This body
    /// is owned by the resource payload and is not inserted in the world list.
    /// exe: FUN_00546e10
    pub fn materialize(&self, m: &mut Space, mesh: &[u8]) -> Result<(u32, u32), String> {
        let definition = m.alloc(0xac);
        crate::native_geometry::load_mesh(m, definition, mesh)?;
        m.w32(definition + 8, self.kind);
        m.w32(definition + 0x50, self.friction);
        m.w32(definition + 0x58, self.collision);
        m.w32(definition + 0x10, self.skin);
        let mut radius = x(0);
        for i in 0..m.r32(definition + 0x9c) {
            let p = m.r32(definition + 0xa0) + i * 12;
            let [a, b, c] = [0, 4, 8].map(|offset| x(m.r32(p + offset)));
            let length = bits(a.mul(a, PC, RC).add(b.mul(b, PC, RC), PC, RC).add(
                c.mul(c, PC, RC),
                PC,
                RC,
            ));
            if f32::from_bits(length) > radius.to_f32(PC, RC) {
                radius = x(length);
            }
        }
        m.w32(
            definition + 0xc,
            bits(radius.sqrt(PC, RC).add(x(0x3d4ccccd), PC, RC)),
        );
        m.w32(definition + 0x4c, 0);
        let body = m.alloc(0xa0);
        m.w32(body, 0x5f1444);
        m.w32(body + 0x78, definition);
        m.w32(body + 0x7c, m.r32(definition));
        m.w32(definition, body + 0x78);
        m.w32(definition + 4, m.r32(definition + 4).wrapping_add(1));
        let zero = f32::from_bits(self.mass) == 0.;
        m.w32(body + 0x9c, if zero { 5 } else { 1 });
        if !zero {
            m.w32(
                body + 0x14,
                bits(x(1f32.to_bits()).div(x(self.mass), PC, RC)),
            );
            m.w32(body + 0x80, bits(x(self.mass).mul(x(0xbba3d70a), PC, RC)));
            m.w32(body + 0x84, bits(x(self.mass).mul(x(0xbc23d70a), PC, RC)));
            let inertia = bits(x(0x40200000).div(x(self.mass), PC, RC));
            for offset in [0x18, 0x2c, 0x40] {
                m.w32(body + offset, inertia);
            }
        }
        Ok((definition, body))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_collision_resource_payloads_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/native-definition-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: run re/oracle/native_definition_probe.py");
            return;
        };
        let data: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in data["rows"].as_array().unwrap() {
            let mut memory = Space::new();
            memory.alloc(0x14);
            let config = Definition::parse(row["text"].as_str().unwrap()).unwrap();
            let hex = row["mesh"].as_str().unwrap();
            let mesh = (0..hex.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap())
                .collect::<Vec<_>>();
            let (definition, body) = config.materialize(&mut memory, &mesh).unwrap();
            assert_eq!(definition, row["definition"].as_u64().unwrap() as u32);
            assert_eq!(body, row["body"].as_u64().unwrap() as u32);
            for (field, address, count) in [
                ("definition_words", definition, 43),
                ("body_words", body, 40),
            ] {
                let actual = (0..count)
                    .map(|i| memory.r32(address + i * 4))
                    .collect::<Vec<_>>();
                let expected = row[field]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| v.as_u64().unwrap() as u32)
                    .collect::<Vec<_>>();
                assert_eq!(actual, expected, "{} {field}", row["text"]);
            }
            assert_eq!(memory.heap_ptr(), row["heap"].as_u64().unwrap() as u32);
        }
    }
}

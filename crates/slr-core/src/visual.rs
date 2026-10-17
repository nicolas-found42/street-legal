//! Assembles a visual car body from the shipped body-panel meshes.
//!
//! Each car folder holds `_main_<model>.cfg` (chassis slots with ids) and per-panel part
//! cfgs (`hood.cfg`, `FL_door.cfg`, ...) whose own `slot` line with the same id says which
//! point of the panel mounts there. A panel's origin is therefore
//! `chassis_slot_position - panel_slot_position`; its mesh is `meshes/<panel>.scx`, in
//! centimetres. Cfg space is x right, y up, z backwards; the output here is
//! x right, y up, z FORWARD, in metres.

use slr_formats::cfg::Cfg;
use slr_formats::scx::{self, Scx};
use std::collections::HashMap;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Material {
    Paint,
    Glass,
    Light,
    Dark,
}

#[derive(Debug, Clone)]
pub struct VisualVertex {
    pub pos: [f32; 3],
    pub normal: [f32; 3],
}

#[derive(Debug, Clone)]
pub struct VisualPart {
    pub name: String,
    pub material: Material,
    pub vertices: Vec<VisualVertex>,
    pub indices: Vec<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct CarVisual {
    pub parts: Vec<VisualPart>,
}

impl CarVisual {
    pub fn triangle_count(&self) -> usize {
        self.parts.iter().map(|p| p.indices.len() / 3).sum()
    }

    /// Axis-aligned bounds `(min, max)` in metres.
    pub fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        let mut lo = [f32::MAX; 3];
        let mut hi = [f32::MIN; 3];
        for p in &self.parts {
            for v in &p.vertices {
                for k in 0..3 {
                    lo[k] = lo[k].min(v.pos[k]);
                    hi[k] = hi[k].max(v.pos[k]);
                }
            }
        }
        (lo, hi)
    }
}

fn material_for(name: &str) -> Material {
    let n = name.to_ascii_lowercase();
    if n.contains("window") || n.contains("windshield") {
        Material::Glass
    } else if n.contains("light") {
        Material::Light
    } else if n.contains("seat")
        || n.contains("steering")
        || n.contains("exhaust")
        || n.contains("sideskirt") && false
    {
        Material::Dark
    } else {
        Material::Paint
    }
}

/// Is this a base (default) panel rather than an alternative/state variant?
fn is_base_variant(stem: &str) -> bool {
    let s = stem.to_ascii_lowercase();
    if s.starts_with('_') || s == "cabrio_chassis" {
        return false;
    }
    for suffix in ["_city", "_indicator", "_brake", "_reversing"] {
        if s.ends_with(suffix) {
            return false;
        }
    }
    // hood_2, F_bumper_3, ... are alternative body kits.
    let tail = s.rsplit('_').next().unwrap_or("");
    !(tail.chars().all(|c| c.is_ascii_digit()) && !tail.is_empty() && s.contains('_'))
}

/// Mounted slot id of a part: the id in its `attach <type> <id>` line.
fn attach_slot(cfg: &Cfg) -> Option<u32> {
    cfg.all("attach")
        .find_map(|d| d.args.get(1).and_then(|a| a.parse::<u32>().ok()))
}

pub fn load_car_visual(maker_dir: &Path, main_cfg_stem: &str) -> Option<CarVisual> {
    let scripts = maker_dir.join("scripts");
    let meshes = maker_dir.join("meshes");
    let main = Cfg::open(scripts.join(format!("_main_{main_cfg_stem}.cfg"))).ok()?;
    let chassis_slots: HashMap<u32, [f32; 3]> = main
        .all("slot")
        .filter_map(|d| {
            let v = d.f32s();
            (v.len() >= 7).then(|| (v[6] as u32, [v[0], v[1], v[2]]))
        })
        .collect();
    let mut visual = CarVisual::default();
    let mut used_slots: HashMap<u32, String> = HashMap::new();

    // The chassis shell itself sits at the origin.
    let add = |name: &str, origin: [f32; 3], visual: &mut CarVisual| {
        let Ok(bytes) = std::fs::read(meshes.join(format!("{name}.scx"))) else {
            return;
        };
        let Ok(Scx::V4 { meshes, .. }) = scx::parse(&bytes) else {
            return;
        };
        let mut part = VisualPart {
            name: name.into(),
            material: material_for(name),
            vertices: vec![],
            indices: vec![],
        };
        for m in meshes {
            if m.indices.is_empty() {
                continue;
            }
            let base = part.vertices.len() as u32;
            for (i, p) in m.positions.iter().enumerate() {
                let n = m.normals.get(i).copied().unwrap_or([0.0, 1.0, 0.0]);
                part.vertices.push(VisualVertex {
                    pos: [
                        origin[0] + p[0] * 0.01,
                        origin[1] + p[1] * 0.01,
                        -(origin[2] + p[2] * 0.01),
                    ],
                    normal: [n[0], n[1], -n[2]],
                });
            }
            part.indices
                .extend(m.indices.iter().map(|&i| base + i as u32));
        }
        if !part.indices.is_empty() {
            visual.parts.push(part);
        }
    };
    add("chassis", [0.0; 3], &mut visual);

    let mut names: Vec<String> = std::fs::read_dir(&scripts)
        .ok()?
        .filter_map(|e| e.ok())
        .filter_map(|e| {
            let n = e.file_name().to_string_lossy().into_owned();
            n.strip_suffix(".cfg").map(str::to_owned)
        })
        .filter(|n| is_base_variant(n))
        .collect();
    names.sort();
    for name in names {
        let Ok(cfg) = Cfg::open(scripts.join(format!("{name}.cfg"))) else {
            continue;
        };
        if cfg.first("render").is_none() {
            continue;
        }
        let Some(slot_id) = attach_slot(&cfg) else {
            continue;
        };
        let Some(chassis_pos) = chassis_slots.get(&slot_id) else {
            continue;
        };
        let part_slot = cfg.all("slot").find_map(|d| {
            let v = d.f32s();
            (v.len() >= 7 && v[6] as u32 == slot_id).then(|| [v[0], v[1], v[2]])
        });
        let Some(ps) = part_slot else { continue };
        if used_slots.contains_key(&slot_id) {
            continue;
        }
        used_slots.insert(slot_id, name.clone());
        let origin = [
            chassis_pos[0] - ps[0],
            chassis_pos[1] - ps[1],
            chassis_pos[2] - ps[2],
        ];
        add(&name, origin, &mut visual);
    }
    (!visual.parts.is_empty()).then_some(visual)
}

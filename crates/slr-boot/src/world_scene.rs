//! Tier D world presentation from live native camera commands and package geometry.
//! No career decisions, resource IDs or garage viewpoints are supplied by the host.
use slr_d3d9::{Frame, WorldVertex};
use slr_fmt_rpak::payload::{self, Chunk, PolyVertex};
use std::{cell::RefCell, collections::BTreeMap, rc::Rc};

#[derive(Clone, Debug)]
struct Camera {
    owner: u32,
    viewport: u32,
    position: [f32; 3],
    target: Option<[f32; 3]>,
    fov: Option<f32>,
}
#[derive(Default)]
struct State {
    cameras: BTreeMap<u32, Camera>,
    scenes: BTreeMap<u32, Rc<Vec<Surface>>>,
    meshes: BTreeMap<u32, Rc<Vec<Surface>>>,
    active: u32,
}
struct Surface {
    texture: u32,
    vertices: Vec<PolyVertex>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
pub fn clear() {
    STATE.with(|s| *s.borrow_mut() = State::default());
}
pub fn created(container: u32, owner: u32, parameters: &str) -> Result<(), String> {
    if !crate::presentation::enabled() {
        return Ok(());
    }

    let parsed = slr_fmt_cfg::scan::sscanf(
        parameters.as_bytes(),
        b"%f,%f,%f,%f,%f,%f",
        slr_fmt_cfg::Precision::F32,
    );
    let camera = Camera {
        owner,
        viewport: 0,
        position: std::array::from_fn(|i| parsed.f(i).unwrap_or(0.)),
        target: None,
        fov: None,
    };
    slr_telemetry::journal::emit(
        "world-camera-created",
        serde_json::json!({"container":container,"owner":owner,"parameters":parameters,"position":camera.position,"projection":"tier-d"}),
    )?;
    STATE.with(|s| s.borrow_mut().cameras.insert(container, camera));
    Ok(())
}
fn id(value: &str) -> Result<u32, String> {
    value
        .parse::<i32>()
        .map(|v| v as u32)
        .map_err(|e| e.to_string())
}
fn vector(value: &str) -> Result<[f32; 3], String> {
    let v = value
        .split(',')
        .map(|v| v.parse::<f32>().map_err(|e| e.to_string()))
        .collect::<Result<Vec<_>, _>>()?;
    v.try_into()
        .map_err(|_| "camera vector requires three components".into())
}
pub fn command(container: u32, class: &str, text: &str) -> Result<(), String> {
    if !crate::presentation::enabled() {
        return Ok(());
    }
    let words: Vec<_> = text.split_whitespace().collect();
    if class == "player" && words.first() == Some(&"camera") {
        let resource = id(words.get(1).ok_or("missing player camera")?)?;
        let target = slr_resource::node::with_res(|h| h.container_for_id(resource)).unwrap_or(0);
        STATE.with(|s| s.borrow_mut().active = target);
        slr_telemetry::journal::emit(
            "world-camera-selected",
            serde_json::json!({"player":container,"resource":resource,"container":target}),
        )?;
    }
    if class != "camera" {
        return Ok(());
    }
    STATE.with(|state| -> Result<(), String> {
        let mut state = state.borrow_mut();
        let Some(camera) = state.cameras.get_mut(&container) else {
            return Ok(());
        };
        match words.first().copied() {
            Some("render") => {
                camera.viewport = id(words.get(1).ok_or("missing render viewport")?)?
            }
            Some("hide") => camera.viewport = 0,
            Some("zoom") => {
                camera.fov = Some(
                    words
                        .get(1)
                        .ok_or("missing camera zoom")?
                        .parse::<f32>()
                        .map_err(|e| e.to_string())?,
                );
            }
            Some("look") => {
                camera.target = Some(vector(words.get(2).ok_or("missing camera look target")?)?);
            }
            // Movement dynamics remain native work. The initial script pose is
            // retained here; the empty garage uses a stationary look target.
            _ => {}
        }
        slr_telemetry::journal::emit(
            "world-camera-command",
            serde_json::json!({
                "container": container, "command": text,
                "state": {"owner": camera.owner, "viewport": camera.viewport,
                    "position": camera.position, "target": camera.target, "fov": camera.fov}
            }),
        )
        .map(|_| ())
    })
}
fn scene(id: u32) -> Result<Rc<Vec<Surface>>, String> {
    if let Some(scene) = STATE.with(|s| s.borrow().scenes.get(&id).cloned()) {
        return Ok(scene);
    }
    let (data, offset, size) = crate::RES.with(|r| -> Result<_, String> {
        let r = r.borrow();
        let (name, package) = r
            .packages
            .get((id >> 16) as usize)
            .ok_or("world package missing")?;
        let package = package.as_ref().ok_or("world package unloaded")?;
        let entry = package
            .entries
            .iter()
            .find(|e| e.type_id == id & 0xffff)
            .ok_or("world entry missing")?;
        Ok((
            std::fs::read(r.root.join(name)).map_err(|e| e.to_string())?,
            entry.rsd_offset as usize,
            entry.rsd_size as usize,
        ))
    })?;
    let ranges =
        payload::walk_payload(&data, offset, size).map_err(|e| format!("world {id:#x}: {e}"))?;
    let root = payload::parse_range_at(&data, offset, size).map_err(|e| e.to_string())?;
    let render = root
        .chunks
        .iter()
        .find_map(|chunk| match chunk {
            Chunk::Rsd {
                lines: Some(lines), ..
            } => lines.iter().find_map(|line| {
                line.strip_prefix("params ")
                    .and_then(|v| v.split(',').next())
            }),
            _ => None,
        })
        .ok_or("world render params missing")?;
    let render = crate::presentation::local_id(id, render)?;
    let (desc, _) = crate::presentation::descriptor(render)?;
    let mesh = desc
        .lines()
        .find_map(|l| l.strip_prefix("mesh "))
        .ok_or("world mesh missing")?;
    let mesh = crate::presentation::local_id(render, mesh)?;
    let textures = desc
        .lines()
        .filter_map(|l| l.strip_prefix("texture "))
        .map(|v| crate::presentation::local_id(render, v))
        .collect::<Result<Vec<_>, _>>()?;
    let scx = std::fs::read(crate::presentation::source_file(mesh)?).map_err(|e| e.to_string())?;
    let slr_fmt_scx::Scx::V3 {
        groups: materials, ..
    } = slr_fmt_scx::parse(&scx).map_err(|e| e.to_string())?
    else {
        return Err("world requires v3 material table".into());
    };
    let mut surfaces = Vec::new();
    for range in ranges.iter().chain(std::iter::once(&root)) {
        for chunk in &range.chunks {
            if let Chunk::Poly { owner, groups, .. } = chunk {
                if *owner != id & 0xffff {
                    return Err(format!(
                        "world POLY owner {owner:#x} differs from root {id:#x}"
                    ));
                }
                for group in groups {
                    let material = materials
                        .get(group.mat as usize)
                        .ok_or("world material index outside table")?;
                    let slot = material
                        .material
                        .get(40..44)
                        .ok_or("world material texture absent")?;
                    let slot = u32::from_le_bytes(slot.try_into().unwrap()) & 0xffff;
                    let texture = *textures
                        .get(slot as usize)
                        .ok_or("world texture slot outside descriptor")?;
                    let vertices = group.decoded();
                    let mut triangles = Vec::new();
                    for (a, b, c) in group.triangles() {
                        for i in [a, b, c] {
                            triangles.push(
                                *vertices
                                    .get(i as usize)
                                    .ok_or("world polygon index outside vertices")?,
                            );
                        }
                    }
                    surfaces.push(Surface {
                        texture,
                        vertices: triangles,
                    });
                }
            }
        }
    }
    if surfaces.is_empty() {
        return Err("world contains no render polygons".into());
    }
    slr_telemetry::journal::emit(
        "world-scene-loaded",
        serde_json::json!({"resource":id,"render":render,"mesh":mesh,"surfaces":surfaces.len(),"triangles":surfaces.iter().map(|s|s.vertices.len()/3).sum::<usize>(),"textures":surfaces.iter().map(|s|s.texture).collect::<Vec<_>>(),"projection":"tier-d"}),
    )?;
    let scene = Rc::new(surfaces);
    STATE.with(|s| s.borrow_mut().scenes.insert(id, scene.clone()));
    Ok(scene)
}
fn mesh(resource: u32) -> Result<Rc<Vec<Surface>>, String> {
    if let Some(mesh) = STATE.with(|s| s.borrow().meshes.get(&resource).cloned()) {
        return Ok(mesh);
    }
    let (desc, _) = crate::presentation::descriptor(resource)?;
    let mesh = desc
        .lines()
        .find_map(|l| l.strip_prefix("mesh "))
        .ok_or_else(|| format!("world render mesh absent {resource:#x}: {desc:?}"))?;
    let mesh = crate::presentation::local_id(resource, mesh)?;
    let textures = desc
        .lines()
        .filter_map(|l| l.strip_prefix("texture "))
        .map(|v| crate::presentation::local_id(resource, v))
        .collect::<Result<Vec<_>, _>>()?;
    let bytes =
        std::fs::read(crate::presentation::source_file(mesh)?).map_err(|e| e.to_string())?;
    let mut surfaces = Vec::new();
    match slr_fmt_scx::parse(&bytes).map_err(|e| e.to_string())? {
        slr_fmt_scx::Scx::V4 { meshes, .. } => {
            let slots = v4_texture_slots(&bytes)?;
            if slots.len() != meshes.len() {
                return Err("world material/mesh count differs".into());
            }
            for (mesh, slot) in meshes.into_iter().zip(slots) {
                let texture = *textures
                    .get(slot as usize)
                    .ok_or("world mesh texture slot outside descriptor")?;
                let vertices = mesh
                    .indices
                    .iter()
                    .map(|&i| {
                        let i = i as usize;
                        PolyVertex {
                            pos: mesh.positions[i].map(|v| v * 0.01),
                            normal: mesh.normals.get(i).copied().unwrap_or_default(),
                            diffuse: u32::MAX,
                            specular: 0,
                            uv: mesh.uvs.get(i).copied().unwrap_or_default(),
                            aux: [0; 8],
                            second: [0.; 2],
                        }
                    })
                    .collect();
                surfaces.push(Surface { texture, vertices });
            }
        }
        slr_fmt_scx::Scx::V3 { groups, .. } => {
            let mut base = 0;
            for group in groups {
                let slot = group.material.get(40..44).ok_or("v3 texture slot absent")?;
                let slot = u32::from_le_bytes(slot.try_into().unwrap()) & 0xffff;
                let texture = *textures
                    .get(slot as usize)
                    .ok_or("v3 texture slot outside descriptor")?;
                let vertices = group
                    .mesh
                    .indices
                    .iter()
                    .map(|i| {
                        let i = (i - base) as usize;
                        PolyVertex {
                            pos: group.mesh.positions[i].map(|v| v * 0.01),
                            normal: group.mesh.normals.get(i).copied().unwrap_or_default(),
                            diffuse: u32::MAX,
                            specular: 0,
                            uv: group.mesh.uvs.get(i).copied().unwrap_or_default(),
                            aux: [0; 8],
                            second: [0.; 2],
                        }
                    })
                    .collect::<Vec<_>>();
                base += group.mesh.positions.len() as u32;
                if !vertices.is_empty() {
                    surfaces.push(Surface { texture, vertices });
                }
            }
        }
    }
    slr_telemetry::journal::emit(
        "world-mesh-loaded",
        serde_json::json!({"render":resource,"mesh":mesh,"surfaces":surfaces.len(),"triangles":surfaces.iter().map(|s|s.vertices.len()/3).sum::<usize>(),"projection":"tier-d-base-texture"}),
    )?;
    let mesh = Rc::new(surfaces);
    STATE.with(|s| s.borrow_mut().meshes.insert(resource, mesh.clone()));
    Ok(mesh)
}
// Bounded v4 material projection: the optional color token precedes the base
// texture token. Remaining texture stages are retained in the asset, but the
// full fixed-function material combiner is still outside this renderer.
fn v4_texture_slots(bytes: &[u8]) -> Result<Vec<u32>, String> {
    fn word(bytes: &[u8], o: usize) -> Result<u32, String> {
        Ok(u32::from_le_bytes(
            bytes
                .get(o..o + 4)
                .ok_or("truncated world material")?
                .try_into()
                .unwrap(),
        ))
    }
    let count = word(bytes, 8)? as usize;
    let mut slots = Vec::new();
    for i in 0..count {
        if word(bytes, 20 + i * 8)? != 1 {
            continue;
        }
        let offset = word(bytes, 16 + i * 8)? as usize;
        let end = offset
            .checked_add(word(bytes, offset + 4)? as usize)
            .ok_or("world material length overflow")?;
        let chunk = bytes
            .get(offset..end)
            .ok_or("world material outside file")?;
        let mut token = 0x40;
        if word(chunk, token)? == 2 {
            token += 8;
        }
        if word(chunk, token)? != 0x06000000 {
            return Err("unsupported world base texture token".into());
        }
        slots.push(word(chunk, token + 4)?);
    }
    Ok(slots)
}
struct Light {
    diffuse: [f32; 3],
    ambient: [f32; 3],
    direction: [f32; 3],
}
impl Light {
    fn parse(desc: &str) -> Result<Self, String> {
        if !desc.lines().any(|l| l.trim() == "type directional") {
            return Err("unsupported world light type".into());
        }
        let values = |key: &str, skip: usize| -> Result<[f32; 3], String> {
            let line = desc
                .lines()
                .find_map(|l| l.strip_prefix(key))
                .ok_or("world light field absent")?;
            let values = line
                .split_whitespace()
                .skip(skip)
                .take(3)
                .map(|v| v.parse::<f32>().map_err(|e| e.to_string()))
                .collect::<Result<Vec<_>, _>>()?;
            values
                .try_into()
                .map_err(|_| "world light vector incomplete".into())
        };
        Ok(Self {
            diffuse: values("diffuse ", 1)?,
            ambient: values("ambient ", 1)?,
            direction: norm(values("direction ", 0)?)?,
        })
    }
    fn shade(&self, normal: [f32; 3]) -> [f32; 3] {
        let diffuse = (-dot(normal, self.direction)).max(0.);
        std::array::from_fn(|i| self.ambient[i] + diffuse * self.diffuse[i])
    }
}
fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|i| a[i] - b[i])
}
fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a.iter().zip(b).map(|(a, b)| a * b).sum()
}
fn norm(v: [f32; 3]) -> Result<[f32; 3], String> {
    let len = dot(v, v).sqrt();
    if len < 1e-6 || !len.is_finite() {
        return Err("degenerate world camera basis".into());
    }
    Ok(v.map(|v| v / len))
}
fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
pub fn draw(
    frame: &mut Frame,
    instances: &[(u32, u32, [f32; 3])],
    active_viewport: impl Fn(u32) -> bool,
    mut texture: impl FnMut(&mut Frame, u32) -> Result<String, String>,
) -> Result<(), String> {
    let camera = STATE.with(|s| {
        let s = s.borrow();
        s.cameras.get(&s.active).cloned()
    });
    let alive =
        STATE.with(|s| slr_resource::node::with_res(|h| h.container(s.borrow().active).is_some()));
    if !alive {
        return Ok(());
    }
    let Some(camera) = camera else {
        return Ok(());
    };
    if !active_viewport(camera.viewport) {
        return Ok(());
    }
    let owner = slr_resource::node::with_res(|h| h.container(camera.owner).map(|c| c.id));
    let Some(owner) = owner else {
        return Ok(());
    };
    let Some(target) = camera.target else {
        return Ok(());
    };
    let forward = norm(sub(target, camera.position))?;
    let right = norm(cross(forward, [0., 1., 0.]))?;
    let up = cross(right, forward);
    let Some(fov) = camera.fov else {
        return Ok(());
    };
    if !fov.is_finite() || !(0.1..179.).contains(&fov) {
        return Err("invalid world camera FOV".into());
    }
    let f = 1. / (fov.to_radians() / 2.).tan();
    let near = 0.05;
    let far = 1000.;
    let mut geometry = vec![(scene(owner)?, [0.; 3])];
    let mut lights = Vec::new();
    for &(instance_owner, resource, position) in instances {
        if instance_owner == camera.owner {
            let (desc, kind) = crate::presentation::descriptor(resource)?;
            if kind == 13 {
                lights.push(Light::parse(&desc)?);
            } else {
                geometry.push((mesh(resource)?, position));
            }
        }
    }
    for (geometry, position) in &geometry {
        for surface in geometry.iter() {
            let name = texture(frame, surface.texture)?;
            let vertices = surface
                .vertices
                .iter()
                .map(|v| {
                    let relative = sub(
                        std::array::from_fn(|i| v.pos[i] + position[i]),
                        camera.position,
                    );
                    let z = dot(relative, forward);
                    let shade = if lights.is_empty() {
                        [1.; 3]
                    } else {
                        lights
                            .iter()
                            .fold([0.; 3], |mut color, light| {
                                let value = light.shade(v.normal);
                                for i in 0..3 {
                                    color[i] += value[i];
                                }
                                color
                            })
                            .map(|v| v.clamp(0., 1.))
                    };
                    WorldVertex {
                        position: [
                            dot(relative, right) * f
                                / (crate::viewport::DISPLAY_MODE.0 as f32
                                    / crate::viewport::DISPLAY_MODE.1 as f32),
                            dot(relative, up) * f,
                            z * far / (far - near) - near * far / (far - near),
                            z,
                        ],
                        uv: v.uv,
                        color: [
                            ((v.diffuse >> 16) & 255) as f32 / 255. * shade[0],
                            ((v.diffuse >> 8) & 255) as f32 / 255. * shade[1],
                            (v.diffuse & 255) as f32 / 255. * shade[2],
                            ((v.diffuse >> 24) & 255) as f32 / 255.,
                        ],
                    }
                })
                .collect();
            frame.world_draws.push((name, vertices));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn camera_projection_tracks_script_pose_zoom_and_target() {
        clear();
        crate::presentation::enable();
        created(91, 37, "1,2,3,0,0,0").unwrap();
        command(91, "camera", "render -12 0 0 1 3").unwrap();
        command(91, "camera", "zoom 73 5").unwrap();
        command(91, "camera", "look 123 4,5,6 0,0,0").unwrap();
        STATE.with(|s| {
            let s = s.borrow();
            let camera = &s.cameras[&91];
            assert_eq!(camera.owner, 37);
            assert_eq!(camera.position, [1., 2., 3.]);
            assert_eq!(camera.viewport, (-12i32) as u32);
            assert_eq!(camera.fov, Some(73.));
            assert_eq!(camera.target, Some([4., 5., 6.]));
        });
        clear();
    }
    #[test]
    fn directional_light_keeps_ambient_on_unlit_faces() {
        let light = Light::parse(
            "type directional\ndiffuse 1 0.7 0.6 0.5\nambient 0 0.1 0.2 0.3\ndirection 0 -1 0\n",
        )
        .unwrap();
        assert_eq!(light.shade([1., 0., 0.]), [0.1, 0.2, 0.3]);
        let lit = light.shade([0., 1., 0.]);
        for value in lit {
            assert!((value - 0.8).abs() < 1e-6);
        }
    }
}

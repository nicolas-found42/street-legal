//! Tier-D OSD presentation fed only by unchanged script natives and live fields.
//! This projection never supplies return values or mutates script/engine state.
use slr_d3d9::{Frame, Image, Message, Vertex};
use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::SyncSender,
};

#[derive(Clone, Default)]
struct Visual {
    source: u32,
    replacements: Vec<(u32, u32)>,
    scale: Option<[f32; 3]>,
}
#[derive(Clone)]
struct Text {
    word: u32,
    owner: u32,
    font: u32,
    pos: [f32; 2],
    color: u32,
    align: u32,
}
#[derive(Clone)]
struct Render {
    owner: u32,
    template: u32,
    pos: [f32; 3],
    color: u32,
}
#[derive(Clone)]
struct Camera {
    viewport: u32,
    fov: f32,
    pos: [f32; 3],
}
#[derive(Clone)]
struct UiBox {
    owner: u32,
    pos: [f32; 3],
    size: [f32; 3],
}
#[derive(Default)]
struct State {
    enabled: bool,
    capture: Option<PathBuf>,
    originals: HashMap<u32, NativeFn>,
    ids: HashMap<u32, u32>,
    visuals: HashMap<u32, Visual>,
    renders: HashMap<u32, Render>,
    ui_boxes: HashMap<u32, UiBox>,
    texts: HashMap<u32, Text>,
    cameras: HashMap<u32, Camera>,
    active_viewports: HashMap<u32, u64>,
    next_viewport_order: u64,
    sender: Option<SyncSender<Message>>,
    presented: bool,
    last_recipe: String,
    screenshots: std::collections::VecDeque<PathBuf>,
    image_cache: RefCell<HashMap<u32, (String, Image)>>,
    font_cache: RefCell<HashMap<u32, std::rc::Rc<slr_fmt_scx::V3Mesh>>>,
}
thread_local! {static STATE:RefCell<State> = RefCell::new(State::default());}
pub fn enabled() -> bool {
    STATE.with(|s| s.borrow().enabled)
}
pub fn enable() {
    STATE.with(|s| s.borrow_mut().enabled = true);
}
pub fn capture_path(path: String) {
    STATE.with(|s| {
        let mut s = s.borrow_mut();
        s.enabled = true;
        s.capture = Some(path.into());
    });
}
pub fn connect(sender: SyncSender<Message>) {
    STATE.with(|s| s.borrow_mut().sender = Some(sender));
}
pub(crate) fn field(vm: &VmHost, word: u32, name: &str) -> Option<u32> {
    let object = vm.objects.get(word.checked_sub(0x1000)? as usize)?;
    let slr_vm::vmclass::ClassRef::Loaded(class) = object.class else {
        return None;
    };
    let slot = vm.instance_field_slot_id(class, name).ok()?;
    object.fields.get(slot).map(|v| v.payload)
}
pub(crate) fn container(vm: &VmHost, word: u32) -> u32 {
    field(vm, word, "ptr")
        .and_then(|ptr| slr_resource::node::with_res(|h| h.node(ptr).map(|n| n.container)))
        .unwrap_or(0)
}
fn resource_id(container: u32) -> u32 {
    slr_resource::node::with_res(|h| h.container(container).map_or(0, |c| c.id))
}
fn vector(vm: &VmHost, word: u32) -> Option<[f32; 3]> {
    Some([
        f32::from_bits(field(vm, word, "x")?),
        f32::from_bits(field(vm, word, "y")?),
        f32::from_bits(field(vm, word, "z")?),
    ])
}
fn bridge(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let address = vm.natives.resolve(info.class, info.name, info.sig)?;
    let original = STATE.with(|s| s.borrow().originals[&address]);
    let mut w = [0; 16];
    let _ = marshal_args(info, &mut w);
    // Some natives destroy/rebind the reference, so preserve argument identities.
    let before = w.map(|word| container(vm, word));
    if info.name == "getPos"
        && matches!(
            info.class,
            "java.util.resource.GameRef" | "java.util.resource.PhysicsRef"
        )
    {
        if let Some((_, button)) = crate::native_button::buttons()
            .into_iter()
            .find(|(id, _)| *id == before[0])
        {
            return vector_object(
                vm,
                [
                    button.transform[0],
                    button.transform[1],
                    button.transform[2],
                ],
            );
        }
    }
    if info.class == "java.util.resource.PhysicsRef"
        && info.name == "getPos"
        && crate::live_input::interacted()
    {
        if let Some(pos) = STATE.with(|s| s.borrow().ui_boxes.get(&before[0]).map(|b| b.pos)) {
            return vector_object(vm, pos);
        }
    }
    let result = original(vm, info);
    if vm.native_failure.is_some() {
        return result;
    }
    let c = container(vm, w[0]);
    STATE.with(|state| {
        let mut s = state.borrow_mut();
        for key in before.into_iter().chain([c]) {
            let id = resource_id(key);
            if id != 0 {
                s.ids.insert(key, id);
            }
        }
        if info.name == "destroy" {
            s.ui_boxes.remove(&before[0]);
            s.renders.remove(&before[0]);
            s.cameras.remove(&before[0]);
        }
        match (info.class, info.name) {
            ("java.util.resource.PhysicsRef", "createBox") if c != 0 => {
                s.ui_boxes.insert(
                    c,
                    UiBox {
                        owner: before[1],
                        pos: [0.; 3],
                        size: [
                            f32::from_bits(w[2]),
                            f32::from_bits(w[3]),
                            f32::from_bits(w[4]),
                        ],
                    },
                );
            }
            ("java.util.resource.PhysicsRef", "setMatrix") => {
                if let Some(pos) = vector(vm, w[1]) {
                    if let Some(b) = s.ui_boxes.get_mut(&c) {
                        b.pos = pos;
                    }
                }
            }
            ("java.util.resource.ResourceRef", "duplicate") if c != 0 => {
                let visual = s.visuals.get(&before[1]).cloned().unwrap_or(Visual {
                    source: s.ids.get(&before[1]).copied().unwrap_or(0),
                    ..Default::default()
                });
                s.visuals.insert(c, visual);
            }
            ("java.util.resource.ResourceRef", "scaleMesh") => {
                s.visuals.entry(c).or_default().scale = Some([
                    f32::from_bits(w[1]),
                    f32::from_bits(w[2]),
                    f32::from_bits(w[3]),
                ]);
            }
            ("java.util.resource.RenderRef", "changeResource") => {
                s.visuals
                    .entry(c)
                    .or_default()
                    .replacements
                    .push((before[1], before[2]));
            }
            ("java.util.resource.RenderRef", "create") if c != 0 => {
                s.renders.insert(
                    c,
                    Render {
                        owner: before[1],
                        template: before[2],
                        pos: [0.; 3],
                        color: u32::MAX,
                    },
                );
            }
            ("java.util.resource.RenderRef", "setMatrix")
                if info.sig == "(Ljava.lang.Vector3;Ljava.lang.Ypr;)V" =>
            {
                if let Some(pos) = vector(vm, w[1]) {
                    if let Some(r) = s.renders.get_mut(&c) {
                        r.pos = pos;
                    }
                    if let Some(camera) = s.cameras.get_mut(&c) {
                        camera.pos = pos;
                    }
                }
            }
            ("java.util.resource.RenderRef", "setColor") => {
                if let Some(r) = s.renders.get_mut(&c) {
                    r.color = w[1];
                }
            }
            ("java.render.Viewport", "activate") => {
                s.next_viewport_order += 1;
                let order = s.next_viewport_order;
                s.active_viewports.insert(c, order);
            }
            ("java.render.Viewport", "deactivate" | "destroy") => {
                s.active_viewports.remove(&before[0]);
            }
            ("java.render.Camera", "create") => {
                s.cameras.insert(
                    c,
                    Camera {
                        viewport: before[2],
                        fov: f32::from_bits(w[4]),
                        pos: [0.; 3],
                    },
                );
            }
            ("java.render.Text", "create") => {
                s.texts.insert(
                    w[0],
                    Text {
                        word: w[0],
                        owner: before[1],
                        font: before[2],
                        pos: [f32::from_bits(w[3]), f32::from_bits(w[4])],
                        color: u32::MAX,
                        align: 0,
                    },
                );
            }
            ("java.render.Text", "setPos") => {
                if let Some(t) = s.texts.get_mut(&w[0]) {
                    t.pos = [f32::from_bits(w[1]), f32::from_bits(w[2])];
                }
            }
            ("java.render.Text", "changeColor") => {
                if let Some(t) = s.texts.get_mut(&w[0]) {
                    t.color = w[1];
                }
            }
            ("java.render.Text", "changeAlign") => {
                if let Some(t) = s.texts.get_mut(&w[0]) {
                    t.align = w[1];
                }
            }
            _ => {}
        }
    });
    result
}
pub fn install(vm: &mut VmHost) {
    if !STATE.with(|s| s.borrow().enabled) {
        return;
    }
    let addresses: Vec<_> = vm
        .natives
        .sorted()
        .iter()
        .filter(|r| {
            r.key.class.starts_with("java.render.")
                || r.key.class.starts_with("java.util.resource.")
        })
        .map(|r| r.impl_addr)
        .collect();
    for addr in addresses {
        STATE.with(|s| {
            let mut s = s.borrow_mut();
            if let std::collections::hash_map::Entry::Vacant(e) = s.originals.entry(addr) {
                if let Some(original) = vm.native_impls.insert(addr, bridge) {
                    e.insert(original);
                }
            }
        });
    }
}
// RPK references are resolved from their package, never from oracle outputs.
fn descriptor(id: u32) -> Result<(String, u8), String> {
    crate::RES.with(|r| {
        let r = r.borrow();
        let package = r
            .packages
            .get((id >> 16) as usize)
            .and_then(|(_, p)| p.as_ref())
            .ok_or_else(|| format!("presentation package {id:#x}"))?;
        let entry = package
            .entries
            .iter()
            .find(|e| e.type_id == id & 0xffff)
            .ok_or_else(|| format!("presentation resource {id:#x}"))?;
        Ok((package.descriptor(entry), entry.typeof_))
    })
}
fn number(text: &str) -> Option<u32> {
    let text = text.trim();
    if let Some(hex) = text.strip_prefix("0x") {
        u32::from_str_radix(hex, 16).ok()
    } else {
        text.parse().ok()
    }
}
fn local_id(owner: u32, value: &str) -> Result<u32, String> {
    let id = number(value).ok_or("invalid presentation resource ID")?;
    if id >> 16 == 0 {
        return Ok((owner & 0xffff0000) | id);
    }
    crate::RES.with(|r| {
        let r = r.borrow();
        let p = r
            .packages
            .get((owner >> 16) as usize)
            .and_then(|(_, p)| p.as_ref())
            .ok_or("missing presentation package")?;
        let link = p
            .links
            .iter()
            .find(|l| l.package_id_shifted == id & 0xffff0000)
            .ok_or("missing presentation link")?;
        let name = link
            .name
            .replace('\\', "/")
            .trim_end_matches('\0')
            .to_ascii_lowercase();
        let index = r
            .packages
            .iter()
            .position(|(n, _)| *n == name)
            .ok_or("unloaded presentation link")?;
        Ok(((index as u32) << 16) | (id & 0xffff))
    })
}
#[derive(Default)]
struct Asset {
    mesh: Option<(u32, [f32; 3])>,
    texture: Option<u32>,
}
impl State {
    fn asset(&self, key: u32, depth: usize) -> Result<Asset, String> {
        if depth > 32 {
            return Err("presentation resource cycle".into());
        }
        let visual = self.visuals.get(&key);
        let id = visual.filter(|v| v.source != 0).map_or_else(
            || {
                self.ids
                    .get(&key)
                    .copied()
                    .unwrap_or_else(|| resource_id(key))
            },
            |v| v.source,
        );
        if id == 0 {
            return Ok(Asset::default());
        }
        let (descriptor, kind) = descriptor(id)?;
        let mut asset = Asset::default();
        for line in descriptor.lines() {
            let mut fields = line.split_whitespace();
            match fields.next() {
                Some("mesh") => {
                    let id = local_id(id, fields.next().ok_or("mesh ID absent")?)?;
                    asset.mesh = Some((id, [1.; 3]));
                }
                Some("texture") if asset.texture.is_none() => {
                    asset.texture = Some(local_id(id, fields.next().ok_or("texture ID absent")?)?)
                }
                _ => {}
            }
        }
        if kind == 5 {
            asset.mesh = Some((id, visual.and_then(|v| v.scale).unwrap_or([1.; 3])));
        }
        if kind == 7 {
            asset.texture = Some(id);
        }
        if let Some(visual) = visual {
            for &(old, new) in &visual.replacements {
                let old = self
                    .ids
                    .get(&old)
                    .copied()
                    .unwrap_or_else(|| resource_id(old));
                if asset.texture == Some(old) {
                    asset.texture = self.asset(new, depth + 1)?.texture;
                }
                if asset.mesh.is_some_and(|(id, _)| id == old) {
                    asset.mesh = self.asset(new, depth + 1)?.mesh;
                }
            }
        }
        Ok(asset)
    }
}
fn source_file(id: u32) -> Result<PathBuf, String> {
    let (descriptor, _) = descriptor(id)?;
    let path = descriptor
        .lines()
        .find_map(|l| {
            l.trim()
                .strip_prefix("sourcefile ")
                .or_else(|| l.trim().strip_prefix("shape "))
        })
        .ok_or_else(|| format!("sourcefile absent for {id:#x}"))?
        .trim()
        .replace('\\', "/");
    // Windows paths are case insensitive. Resolve each component against the
    // supplied install instead of requiring a case-insensitive host volume.
    let root = crate::RES.with(|r| r.borrow().root.clone());
    let mut path_on_disk = root;
    for component in Path::new(&path).components() {
        let std::path::Component::Normal(name) = component else {
            return Err("unsafe asset path".into());
        };
        let wanted = name.to_string_lossy();
        let entry = std::fs::read_dir(&path_on_disk)
            .map_err(|e| e.to_string())?
            .filter_map(Result::ok)
            .find(|e| {
                e.file_name()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&wanted)
            })
            .ok_or_else(|| format!("presentation asset {path}"))?;
        path_on_disk = entry.path();
    }
    Ok(path_on_disk)
}
fn image(id: u32) -> Result<(String, Image), String> {
    let path = source_file(id)?;
    let bytes = std::fs::read(&path).map_err(|e| e.to_string())?;
    let ext = path
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_ascii_lowercase();
    let decoded = match ext.as_str() {
        "png" => slr_fmt_texture::pngdec::decode(&bytes).map(|(_, i)| i),
        "dds" => slr_fmt_texture::dds::parse(&bytes).and_then(|d| d.decode_mip(0)),
        "tga" => slr_fmt_texture::tga::decode(&bytes).map(|(_, i)| i),
        "ptx" => return ptx_image(&path, &bytes),
        _ => return Err(format!("unsupported menu texture {}", path.display())),
    }
    .map_err(|e| format!("{}: {e:?}", path.display()))?;
    Ok((
        path.to_string_lossy().into_owned(),
        Image {
            width: decoded.width,
            height: decoded.height,
            rgba: std::sync::Arc::new(decoded.pixels),
        },
    ))
}
fn argb(color: u32) -> [f32; 4] {
    [
        (color >> 16 & 255) as f32 / 255.,
        (color >> 8 & 255) as f32 / 255.,
        (color & 255) as f32 / 255.,
        (color >> 24) as f32 / 255.,
    ]
}
fn quad(rect: [f32; 4], uv: [[f32; 2]; 4], color: u32) -> Vec<Vertex> {
    let [left, top, right, bottom] = rect;
    let p = [[left, top], [right, top], [left, bottom], [right, bottom]];
    [0, 1, 2, 2, 1, 3]
        .map(|i| Vertex {
            position: p[i],
            uv: uv[i],
            color: argb(color),
        })
        .to_vec()
}
fn texture(s: &State, frame: &mut Frame, id: u32) -> Result<String, String> {
    if !s.image_cache.borrow().contains_key(&id) {
        s.image_cache.borrow_mut().insert(id, image(id)?);
    }
    let cache = s.image_cache.borrow();
    let (name, image) = &cache[&id];
    frame
        .images
        .entry(name.clone())
        .or_insert_with(|| image.clone());
    Ok(name.clone())
}
fn font_mesh(id: u32) -> Result<slr_fmt_scx::V3Mesh, String> {
    let path = source_file(id)?;
    match slr_fmt_scx::parse(&std::fs::read(path).map_err(|e| e.to_string())?)
        .map_err(|e| format!("font mesh: {e:?}"))?
    {
        slr_fmt_scx::Scx::V3 { mut groups, .. } if groups.len() == 1 => Ok(groups.remove(0).mesh),
        _ => Err("unsupported menu font geometry".into()),
    }
}
fn ui_mesh(s: &State, id: u32) -> Result<std::rc::Rc<slr_fmt_scx::V3Mesh>, String> {
    if let Some(mesh) = s.font_cache.borrow().get(&id).cloned() {
        return Ok(mesh);
    }
    let bytes = std::fs::read(source_file(id)?).map_err(|e| e.to_string())?;
    let mesh = match slr_fmt_scx::parse(&bytes).map_err(|e| format!("UI mesh {id:#x}: {e:?}"))? {
        slr_fmt_scx::Scx::V3 { mut groups, .. } if groups.len() == 1 => groups.remove(0).mesh,
        slr_fmt_scx::Scx::V4 { mut meshes, .. } if meshes.len() == 1 => {
            let mesh = meshes.remove(0);
            slr_fmt_scx::V3Mesh {
                positions: mesh.positions,
                normals: mesh.normals,
                uvs: mesh.uvs,
                indices: mesh.indices.into_iter().map(u32::from).collect(),
                ..Default::default()
            }
        }
        _ => return Err(format!("unsupported multi-mesh UI resource {id:#x}")),
    };
    let mesh = std::rc::Rc::new(mesh);
    s.font_cache.borrow_mut().insert(id, mesh.clone());
    Ok(mesh)
}
#[derive(Clone, Copy)]
struct Projection {
    half_height: f32,
    viewport: [f32; 4],
}
impl Projection {
    fn half_width(self) -> f32 {
        self.half_height * self.viewport[2] * 800. / (self.viewport[3] * 600.)
    }
    fn x(self, local: f32) -> f32 {
        2. * self.viewport[0] + self.viewport[2] * (local + 1.) - 1.
    }
    fn y(self, local: f32) -> f32 {
        1. - 2. * self.viewport[1] - self.viewport[3] * (1. - local)
    }
}
fn visible_owners(vm: &VmHost, s: &State) -> HashMap<u32, Projection> {
    let mut owners = HashMap::new();
    for (i, object) in vm.objects.iter().enumerate() {
        if object.class_name(&vm.loader) != "java.render.Osd" {
            continue;
        }
        let word = 0x1000 + i as u32;
        let viewport = container(vm, field(vm, word, "vp").unwrap_or(0));
        if field(vm, word, "cam").unwrap_or(0) == 0 || !s.active_viewports.contains_key(&viewport) {
            continue;
        }
        let camera_word = field(vm, word, "cam").unwrap_or(0);
        let Some(camera) = s
            .cameras
            .get(&container(vm, camera_word))
            .filter(|c| c.viewport == viewport)
        else {
            continue;
        };
        let half_height = camera.pos[2].abs() * (camera.fov.to_radians() / 2.).tan();
        if !half_height.is_finite() || half_height <= 0. {
            continue;
        }
        let Some(bounds) = crate::viewport::bounds(viewport) else {
            continue;
        };
        if bounds[2] <= 0. || bounds[3] <= 0. {
            continue;
        }
        let projection = Projection {
            half_height,
            viewport: bounds,
        };
        owners.insert(container(vm, word), projection);
        for (j, group) in vm.objects.iter().enumerate() {
            let group_word = 0x1000 + j as u32;
            if group.class_name(&vm.loader) == "java.render.Group"
                && field(vm, group_word, "osd") == Some(word)
                && field(vm, group_word, "active") == Some(1)
            {
                owners.insert(container(vm, group_word), projection);
            }
        }
    }
    owners
}
fn scene(vm: &VmHost, s: &State) -> Result<Frame, String> {
    let owners = visible_owners(vm, s);
    let mut frame = Frame::default();
    let mut rectangles = Vec::new();
    let mut draw_layers = Vec::new();
    for (i, object) in vm.objects.iter().enumerate() {
        if object.class_name(&vm.loader) != "java.render.Rectangle" {
            continue;
        }
        let word = 0x1000 + i as u32;
        let Some(render) = s
            .renders
            .get(&container(vm, field(vm, word, "ins").unwrap_or(0)))
        else {
            continue;
        };
        if !owners.contains_key(&render.owner) {
            continue;
        }
        let asset = s
            .asset(render.template, 0)
            .map_err(|e| format!("rectangle {word:#x} template {:#x}: {e}", render.template))?;
        let Some(tex) = asset.texture else {
            continue;
        };
        let template = field(vm, word, "rt").ok_or("rectangle template absent")?;
        let width = f32::from_bits(field(vm, template, "width").ok_or("rectangle width absent")?);
        let height =
            f32::from_bits(field(vm, template, "height").ok_or("rectangle height absent")?);
        let pos = render.pos;
        let projection = owners[&render.owner];
        let half_height = projection.half_height;
        let half_width = projection.half_width();
        let name = texture(s, &mut frame, tex)?;
        rectangles.push((
            owner_layer(vm, s, render.owner),
            pos[2],
            name,
            quad(
                [
                    projection.x((pos[0] - width / 2.) / half_width),
                    projection.y((pos[1] + height / 2.) / half_height),
                    projection.x((pos[0] + width / 2.) / half_width),
                    projection.y((pos[1] - height / 2.) / half_height),
                ],
                [[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
                render.color,
            ),
        ));
    }
    rectangles.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.total_cmp(&b.1)));
    for (layer, _, name, vertices) in rectangles {
        draw_layers.push(layer);
        frame.draws.push((name, vertices));
    }
    for (id, button) in crate::native_button::buttons() {
        if slr_resource::node::with_res(|h| h.container(id).is_none()) {
            continue;
        }
        let Some(projection) = owners.get(&button.owner).copied() else {
            continue;
        };
        let render = slr_resource::node::with_res(|h| h.container_for_id(button.render))
            .ok_or("button render unavailable")?;
        let asset = s.asset(render, 0)?;
        let (Some((mesh, scale)), Some(tex)) = (asset.mesh, asset.texture) else {
            continue;
        };
        let mesh = ui_mesh(s, mesh)?;
        let name = texture(s, &mut frame, tex)?;
        let mut vertices = Vec::new();
        for index in &mesh.indices {
            let i = *index as usize;
            let p = mesh
                .positions
                .get(i)
                .ok_or("UI mesh index outside positions")?;
            vertices.push(Vertex {
                position: [
                    projection.x((p[0] * scale[0] + button.transform[0]) / projection.half_width()),
                    projection.y((p[1] * scale[1] + button.transform[1]) / projection.half_height),
                ],
                uv: mesh.uvs.get(i).copied().unwrap_or_default(),
                color: [1.; 4],
            });
        }
        draw_layers.push(owner_layer(vm, s, button.owner));
        frame.draws.push((name, vertices));
    }
    let mut texts: Vec<_> = s
        .texts
        .values()
        .filter(|t| owners.contains_key(&t.owner))
        .collect();
    texts.sort_by_key(|t| t.word);

    for text in texts {
        let Some(content) = field(vm, text.word, "text").and_then(|word| vm.string_bytes(word))
        else {
            continue;
        };
        if content.is_empty() {
            continue;
        }
        let asset = s.asset(text.font, 0).map_err(|e| {
            format!(
                "text {:?} font {:#x}: {e}",
                String::from_utf8_lossy(content),
                text.font
            )
        })?;
        let (Some((mesh, _)), Some(tex)) = (asset.mesh, asset.texture) else {
            return Err("font lacks mesh/texture".into());
        };
        if !s.font_cache.borrow().contains_key(&mesh) {
            s.font_cache
                .borrow_mut()
                .insert(mesh, std::rc::Rc::new(font_mesh(mesh)?));
        }
        let font = s.font_cache.borrow()[&mesh].clone();
        let name = texture(s, &mut frame, tex)?;
        let atlas = &frame.images[&name];
        let pixel_scale = font
            .positions
            .as_chunks::<4>()
            .0
            .iter()
            .zip(font.uvs.as_chunks::<4>().0.iter())
            .find_map(|(p, uv)| {
                let dx = p[1][0] - p[0][0];
                let du = uv[1][0] - uv[0][0];
                (dx.abs() > f32::EPSILON && du.abs() > f32::EPSILON)
                    .then_some(du.abs() * atlas.width as f32 / dx.abs())
            })
            .ok_or("font has no measurable glyphs")?;
        let glyph_width = |c: u8| {
            let start = c as usize * 4;
            font.positions
                .get(start + 1)
                .map_or(0., |p| p[0] * pixel_scale)
        };
        let width: f32 = content.iter().map(|&c| glyph_width(c)).sum();
        let projection = owners[&text.owner];
        let mut x = (projection.x(text.pos[0]) + 1.) * 400.
            - if Some(text.align)
                == vm
                    .read_loaded_static_field("java.render.Text", "ALIGN_RIGHT")
                    .map(|v| v.payload)
            {
                width
            } else if Some(text.align)
                == vm
                    .read_loaded_static_field("java.render.Text", "ALIGN_CENTER")
                    .map(|v| v.payload)
            {
                width / 2.
            } else {
                0.
            };
        let y = (1. - projection.y(-text.pos[1])) * 300.;
        let mut vertices = Vec::new();
        for &c in content {
            let first = c as usize * 4;
            if first + 3 >= font.positions.len() {
                continue;
            }
            let mut uv = [[0.; 2]; 4];
            let mut p = [[0.; 2]; 4];
            for i in 0..4 {
                uv[i] = font.uvs[first + i];
                let pos = font.positions[first + i];
                p[i] = [
                    (x + pos[0] * pixel_scale) / 400. - 1.,
                    1. - (y + pos[1] * pixel_scale) / 300.,
                ];
            }
            for i in [0, 1, 2, 2, 1, 3] {
                vertices.push(Vertex {
                    position: p[i],
                    uv: uv[i],
                    color: argb(text.color),
                });
            }
            x += glyph_width(c);
        }
        if !vertices.is_empty() {
            let bounds = vertices.iter().fold(
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                |mut b, v| {
                    b[0] = b[0].min(v.position[0]);
                    b[1] = b[1].min(v.position[1]);
                    b[2] = b[2].max(v.position[0]);
                    b[3] = b[3].max(v.position[1]);
                    b
                },
            );
            let label = String::from_utf8_lossy(content).into_owned();
            frame.text_bounds.insert(label.clone(), bounds);
            draw_layers.push(owner_layer(vm, s, text.owner));
            frame.draws.push((name, vertices));
            frame.labels.push(label);
        }
    }
    let mut draws: Vec<_> = draw_layers.into_iter().zip(frame.draws).collect();
    draws.sort_by_key(|(layer, _)| *layer);
    frame.draws = draws.into_iter().map(|(_, draw)| draw).collect();
    Ok(frame)
}
fn owner_layer(vm: &VmHost, s: &State, owner: u32) -> u64 {
    let script = slr_resource::node::with_res(|h| {
        h.container(owner)
            .and_then(|c| c.instance.as_ref())
            .map_or(0, |i| i.script)
    });
    let osd = field(vm, script, "osd").unwrap_or(script);
    let vp = container(vm, field(vm, osd, "vp").unwrap_or(0));
    s.active_viewports.get(&vp).copied().unwrap_or(0)
}
pub fn frame(vm: &VmHost, menu: bool, output: &Path) -> Result<(), String> {
    STATE.with(|state| {
        let mut s=state.borrow_mut();
        if !s.enabled || (!menu && !s.presented) || (s.presented && !crate::live_input::interacted() && s.sender.is_none()) {return Ok(());}
        let mut frame=scene(vm,&s)?;
        if frame.draws.is_empty() || frame.labels.is_empty() {return Ok(());}
        frame.id=slr_vm::diagnostics::current_context().frame.unwrap_or(0)+1;
        frame.input_id=crate::live_input::input_id();
        let recipe=frame.fingerprint();
        let changed=recipe!=s.last_recipe;
        if changed {
            let metadata=serde_json::json!({"frame_id":frame.id,"input_id":frame.input_id,"recipe_sha256":recipe,"labels":frame.labels,"text_bounds":frame.text_bounds,"draws":frame.draws.len(),"textures":frame.images.keys().collect::<Vec<_>>(),"picking":picking_state(vm,&s)});
            std::fs::write(output.join("presentation.json.tmp"),serde_json::to_vec_pretty(&metadata).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
            std::fs::rename(output.join("presentation.json.tmp"),output.join("presentation.json")).map_err(|e|e.to_string())?;
            slr_telemetry::journal::emit("render-state",metadata)?;
            if std::env::var_os("SLR_SESSION_CAPTURE").is_some() {
                let path=output.join(format!("frame-{}.png",frame.id));
                slr_d3d9::capture(&frame,&path)?;
                slr_telemetry::journal::emit("state-image",serde_json::json!({"frame_id":frame.id,"recipe_sha256":recipe,"path":path.file_name().unwrap().to_string_lossy(),"png_sha256":png_hash(&path)?}))?;
                s.screenshots.push_back(path);
                if s.screenshots.len()>8 {
                    let old=s.screenshots.pop_front().unwrap();
                    slr_telemetry::journal::emit("image-evicted",serde_json::json!({"path":old.file_name().unwrap().to_string_lossy(),"reason":"latest-eight-state-images"}))?;
                    std::fs::remove_file(old).map_err(|e|e.to_string())?;
                }
            }
            s.last_recipe=recipe.clone();
        }
        if !s.presented {
            if let Some(path)=&s.capture {slr_d3d9::capture(&frame,path)?;}
            println!("WINDOW stock menu: {} draws, {} labels, {} textures",frame.draws.len(),frame.labels.len(),frame.images.len());
        }
        if let Some(sender)=&s.sender {
            let started=std::time::Instant::now();
            let id=frame.id;let input_id=frame.input_id;
            slr_telemetry::journal::emit("frame-enqueue",serde_json::json!({"frame_id":id,"input_id":input_id,"recipe_sha256":recipe}))?;
            sender.send(Message::Frame(frame)).map_err(|e|e.to_string())?;
            slr_telemetry::journal::emit("frame-submitted",serde_json::json!({"frame_id":id,"input_id":input_id,"recipe_sha256":recipe,"send_ns":started.elapsed().as_nanos()}))?;
        }
        s.presented=true;Ok(())
    })
}
pub(crate) fn labels(vm: &VmHost) -> Result<Vec<String>, String> {
    STATE.with(|s| scene(vm, &s.borrow()).map(|f| f.labels))
}

fn ptx_image(path: &Path, bytes: &[u8]) -> Result<(String, Image), String> {
    use zune_core::bytestream::ZCursor;
    let ptx = slr_fmt_texture::parse(bytes).map_err(|e| format!("PTX: {e:?}"))?;
    let mip = ptx.mips.last().ok_or("empty PTX")?;
    let mut decoder = zune_jpeg::JpegDecoder::new(ZCursor::new(&mip.rgb));
    let rgb = decoder.decode().map_err(|e| format!("PTX JPEG: {e:?}"))?;
    let info = decoder.info().ok_or("PTX JPEG info absent")?;
    let alpha = if mip.alpha.is_empty() {
        None
    } else {
        Some(
            zune_jpeg::JpegDecoder::new(ZCursor::new(&mip.alpha))
                .decode()
                .map_err(|e| format!("PTX alpha: {e:?}"))?,
        )
    };
    let count = info.width as usize * info.height as usize;
    let channels = rgb.len() / count;
    if !matches!(channels, 1 | 3)
        || rgb.len() != count * channels
        || alpha
            .as_ref()
            .is_some_and(|a| !matches!(a.len(), n if n == count || n == count * 3))
    {
        return Err("PTX pixel count/channels".into());
    }
    let mut rgba = Vec::with_capacity(count * 4);
    for i in 0..count {
        let p = &rgb[i * channels..(i + 1) * channels];
        rgba.extend_from_slice(&[
            p[0],
            p[if channels == 3 { 1 } else { 0 }],
            p[if channels == 3 { 2 } else { 0 }],
            alpha.as_ref().map_or(255, |a| a[i * (a.len() / count)]),
        ]);
    }
    Ok((
        path.to_string_lossy().into_owned(),
        Image {
            width: info.width as u32,
            height: info.height as u32,
            rgba: std::sync::Arc::new(rgba),
        },
    ))
}

pub(crate) fn vector_object(vm: &mut VmHost, pos: [f32; 3]) -> Option<u32> {
    let result = (|| {
        let word = vm.alloc_object("java.lang.Vector3")?;
        for (name, value) in ["x", "y", "z"].into_iter().zip(pos) {
            vm.write_field(word, name, slr_vm::values::Value::float(value))?;
        }
        Ok::<_, slr_vm::host::HostError>(word)
    })();
    match result {
        Ok(word) => Some(word),
        Err(e) => {
            vm.native_failure = Some(format!("UI position: {e:?}"));
            None
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Hit {
    pub group: u32,
    pub osd: u32,
    pub physics: u32,
}
pub(crate) fn hit_test(vm: &VmHost, osd: u32, x: f32, y: f32) -> Option<Hit> {
    if !x.is_finite() || !y.is_finite() || !(0.0..=1.0).contains(&x) || !(0.0..=1.0).contains(&y) {
        return None;
    }
    STATE.with(|s| {
        let s = s.borrow();
        let owners = visible_owners(vm, &s);
        let mut hits = Vec::new();
        for (&key, b) in &s.ui_boxes {
            let group = slr_resource::node::with_res(|h| {
                h.container(b.owner)
                    .and_then(|c| c.instance.as_ref())
                    .map_or(0, |i| i.script)
            });
            if field(vm, group, "osd") != Some(osd) {
                continue;
            }
            let Some(&projection) = owners.get(&b.owner) else {
                continue;
            };
            let [left, top, width, height] = projection.viewport;
            if x < left || x > left + width || y < top || y > top + height {
                continue;
            }
            let px = ((x - left) / width * 2. - 1.) * projection.half_width();
            let py = (1. - (y - top) / height * 2.) * projection.half_height;
            if (px - b.pos[0]).abs() <= b.size[0] / 2. && (py - b.pos[1]).abs() <= b.size[1] / 2. {
                hits.push((
                    b.pos[2],
                    key,
                    Hit {
                        group,
                        osd,
                        physics: resource_id(key),
                    },
                ));
            }
        }
        for (id, button) in crate::native_button::buttons() {
            let group = slr_resource::node::with_res(|h| {
                h.container(button.owner)
                    .and_then(|c| c.instance.as_ref())
                    .map_or(0, |i| i.script)
            });
            if field(vm, group, "osd") != Some(osd)
                || slr_resource::node::with_res(|h| h.container(id).is_none())
            {
                continue;
            }
            let Some(projection) = owners.get(&button.owner).copied() else {
                continue;
            };
            let Ok(mesh) = ui_mesh(&s, button.body) else {
                continue;
            };
            let [left, top, width, height] = projection.viewport;
            if x < left || x > left + width || y < top || y > top + height {
                continue;
            }
            let px = ((x - left) / width * 2. - 1.) * projection.half_width() - button.transform[0];
            let py = (1. - (y - top) / height * 2.) * projection.half_height - button.transform[1];
            let bounds = mesh.positions.iter().fold(
                [
                    f32::INFINITY,
                    f32::INFINITY,
                    f32::NEG_INFINITY,
                    f32::NEG_INFINITY,
                ],
                |mut b, p| {
                    b[0] = b[0].min(p[0]);
                    b[1] = b[1].min(p[1]);
                    b[2] = b[2].max(p[0]);
                    b[3] = b[3].max(p[1]);
                    b
                },
            );
            if px >= bounds[0] && px <= bounds[2] && py >= bounds[1] && py <= bounds[3] {
                hits.push((
                    button.transform[2],
                    id,
                    Hit {
                        group,
                        osd,
                        physics: resource_id(id),
                    },
                ));
            }
        }
        hits.sort_by(|a, b| b.0.total_cmp(&a.0).then_with(|| b.1.cmp(&a.1)));
        hits.first().map(|(_, _, hit)| *hit)
    })
}

pub(crate) fn pick_position(vm: &VmHost, osd: u32, point: [f32; 2]) -> Option<[f32; 3]> {
    STATE.with(|s| {
        let s = s.borrow();
        let owners = visible_owners(vm, &s);
        let projection = *owners.get(&container(vm, osd))?;
        let [left, top, width, height] = projection.viewport;
        Some([
            ((point[0] - left) / width * 2. - 1.) * projection.half_width(),
            (1. - (point[1] - top) / height * 2.) * projection.half_height,
            0.,
        ])
    })
}

pub(crate) fn capture(vm: &VmHost, path: &Path) -> Result<(), String> {
    STATE.with(|s| slr_d3d9::capture(&scene(vm, &s.borrow())?, path))
}

fn png_hash(path: &Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(std::fs::read(path).map_err(|e| e.to_string())?)
    ))
}
fn picking_state(vm: &VmHost, s: &State) -> serde_json::Value {
    serde_json::json!({"focus":crate::live_input::focused_osd(vm),
        "boxes":s.ui_boxes.iter().map(|(id,b)|serde_json::json!({"container":id,"physics":resource_id(*id),"owner":b.owner,"position":b.pos,"size":b.size})).collect::<Vec<_>>(),
        "viewports":s.active_viewports.iter().map(|(id,order)|serde_json::json!({"container":id,"order":order,"bounds":crate::viewport::bounds(*id)})).collect::<Vec<_>>()})
}
pub(crate) fn failure_state(vm: &VmHost) -> Result<(), String> {
    STATE
        .with(|s| slr_telemetry::write_artifact("ui-failure.json", &picking_state(vm, &s.borrow())))
}

//! Tier-D OSD presentation fed only by unchanged script natives and live fields.
//! This projection never supplies return values or mutates script/engine state.
use slr_d3d9::{Frame, Image, Message, Vertex};
use slr_vm::host::{marshal_args, CallInfo, NativeFn, VmHost};
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::mpsc::Sender,
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
#[derive(Default)]
struct State {
    enabled: bool,
    capture: Option<PathBuf>,
    originals: HashMap<u32, NativeFn>,
    ids: HashMap<u32, u32>,
    visuals: HashMap<u32, Visual>,
    renders: HashMap<u32, Render>,
    texts: HashMap<u32, Text>,
    cameras: HashMap<u32, Camera>,
    active_viewports: HashMap<u32, u32>,
    sender: Option<Sender<Message>>,
    presented: bool,
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
pub fn connect(sender: Sender<Message>) {
    STATE.with(|s| s.borrow_mut().sender = Some(sender));
}
fn field(vm: &VmHost, word: u32, name: &str) -> Option<u32> {
    let object = vm.objects.get(word.checked_sub(0x1000)? as usize)?;
    let slr_vm::vmclass::ClassRef::Loaded(class) = object.class else {
        return None;
    };
    let slot = vm.instance_field_slot_id(class, name).ok()?;
    object.fields.get(slot).map(|v| v.payload)
}
fn container(vm: &VmHost, word: u32) -> u32 {
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
        match (info.class, info.name) {
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
                s.active_viewports.insert(c, w[1]);
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
        .find_map(|l| l.trim().strip_prefix("sourcefile "))
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
            rgba: decoded.pixels,
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
fn texture(frame: &mut Frame, id: u32) -> Result<String, String> {
    let path = source_file(id)?.to_string_lossy().into_owned();
    if !frame.images.contains_key(&path) {
        let (_, im) = image(id)?;
        frame.images.insert(path.clone(), im);
    }
    Ok(path)
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
fn visible_owners(vm: &VmHost, s: &State) -> HashMap<u32, f32> {
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
        owners.insert(container(vm, word), half_height);
        for (j, group) in vm.objects.iter().enumerate() {
            let group_word = 0x1000 + j as u32;
            if group.class_name(&vm.loader) == "java.render.Group"
                && field(vm, group_word, "osd") == Some(word)
                && field(vm, group_word, "active") == Some(1)
            {
                owners.insert(container(vm, group_word), half_height);
            }
        }
    }
    owners
}
fn scene(vm: &VmHost, s: &State) -> Result<Frame, String> {
    let owners = visible_owners(vm, s);
    let mut frame = Frame::default();
    let mut rectangles = Vec::new();
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
        let half_height = owners[&render.owner];
        let half_width = half_height * 800. / 600.;
        let name = texture(&mut frame, tex)?;
        rectangles.push((
            pos[2],
            name,
            quad(
                [
                    (pos[0] - width / 2.) / half_width,
                    (pos[1] + height / 2.) / half_height,
                    (pos[0] + width / 2.) / half_width,
                    (pos[1] - height / 2.) / half_height,
                ],
                [[0., 0.], [1., 0.], [0., 1.], [1., 1.]],
                render.color,
            ),
        ));
    }
    rectangles.sort_by(|a, b| a.0.total_cmp(&b.0));
    frame.draws.extend(
        rectangles
            .into_iter()
            .map(|(_, name, vertices)| (name, vertices)),
    );
    let mut texts: Vec<_> = s
        .texts
        .values()
        .filter(|t| owners.contains_key(&t.owner))
        .collect();
    texts.sort_by_key(|t| t.word);
    let mut fonts = HashMap::new();
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
        if let std::collections::hash_map::Entry::Vacant(e) = fonts.entry(mesh) {
            e.insert(font_mesh(mesh)?);
        }
        let font = &fonts[&mesh];
        let name = texture(&mut frame, tex)?;
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
        let mut x = (text.pos[0] + 1.) * 400.
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
        let y = (text.pos[1] + 1.) * 300.;
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
            frame.draws.push((name, vertices));
            frame.labels.push(label);
        }
    }
    Ok(frame)
}
pub fn frame(vm: &VmHost, menu: bool, output: &Path) -> Result<(), String> {
    if !menu
        || !STATE.with(|s| {
            let s = s.borrow();
            s.enabled && !s.presented
        })
    {
        return Ok(());
    }
    STATE.with(|state| {
        let mut s=state.borrow_mut();
        let frame=scene(vm,&s)?;
        if frame.draws.is_empty() || frame.labels.is_empty() {return Err("PresentationBoundary: menu has no visible draws/text".into());}
        std::fs::write(output.join("presentation.json"),serde_json::to_vec_pretty(&serde_json::json!({"labels":frame.labels,"text_bounds":frame.text_bounds,"draws":frame.draws.len(),"textures":frame.images.keys().collect::<Vec<_>>()})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
        if let Some(path)=&s.capture {slr_d3d9::capture(&frame,path)?;}
        println!("WINDOW stock menu: {} draws, {} labels, {} textures",frame.draws.len(),frame.labels.len(),frame.images.len());
        if let Some(sender)=&s.sender {sender.send(Message::Frame(frame)).map_err(|e|e.to_string())?;}
        s.presented=true;Ok(())
    })
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
            rgba,
        },
    ))
}

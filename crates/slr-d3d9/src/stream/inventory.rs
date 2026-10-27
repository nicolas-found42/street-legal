//! M5-01: the D3D9 features a hooked-original log actually uses.
use super::{
    defaults::{
        primitive_name, render_state_name, sampler_name, texture_stage_name, transform_name,
    },
    taxonomy::{classify, Class},
    Result, StreamError,
};
use serde_json::{json, Map, Value as Json};
use slr_calllog::{LogReader, LogRecord, Value};
use std::{collections::BTreeMap, io::Read};

type Counts = BTreeMap<u32, u64>;

fn word(v: Option<&Value>) -> Option<u32> {
    match v {
        Some(Value::Int(i)) => Some(*i as u32),
        _ => None,
    }
}

pub fn format_name(format: u32) -> String {
    match format {
        20 => "R8G8B8".into(),
        21 => "A8R8G8B8".into(),
        22 => "X8R8G8B8".into(),
        23 => "R5G6B5".into(),
        24 => "X1R5G5B5".into(),
        25 => "A1R5G5B5".into(),
        26 => "A4R4G4B4".into(),
        28 => "A8".into(),
        50 => "L8".into(),
        51 => "A8L8".into(),
        70 => "D16_LOCKABLE".into(),
        71 => "D32".into(),
        73 => "D15S1".into(),
        75 => "D24S8".into(),
        77 => "D24X8".into(),
        80 => "D16".into(),
        101 => "INDEX16".into(),
        102 => "INDEX32".into(),
        0x3154_5844 => "DXT1".into(),
        0x3254_5844 => "DXT2".into(),
        0x3354_5844 => "DXT3".into(),
        0x3454_5844 => "DXT4".into(),
        0x3554_5844 => "DXT5".into(),
        other => format!("D3DFMT_{other}"),
    }
}

fn named(counts: &Counts, name: impl Fn(u32) -> String) -> Json {
    let mut out = Map::new();
    for (k, v) in counts {
        out.insert(format!("{k}"), json!({"name": name(*k), "count": v}));
    }
    Json::Object(out)
}

fn distinct(values: &BTreeMap<String, u64>) -> Json {
    json!(values)
}

#[derive(Default)]
struct Scan {
    methods: BTreeMap<String, u64>,
    classes: BTreeMap<&'static str, u64>,
    render: BTreeMap<u32, Counts>,
    stage: BTreeMap<u32, Counts>,
    sampler: BTreeMap<u32, Counts>,
    stages_used: Counts,
    samplers_used: Counts,
    transforms: Counts,
    fvf: Counts,
    primitives: BTreeMap<String, u64>,
    lights: Counts,
    textures: BTreeMap<String, u64>,
    vertex_buffers: BTreeMap<String, u64>,
    index_formats: Counts,
    render_targets: BTreeMap<String, u64>,
    depth_surfaces: BTreeMap<String, u64>,
    viewports: BTreeMap<String, u64>,
    draws_per_frame: Vec<u64>,
    current_draws: u64,
    presents: u64,
    scene_pairs: (u64, u64),
    nonzero_vertex_shaders: u64,
    nonzero_pixel_shaders: u64,
    declarations_bound: u64,
    unsupported: BTreeMap<String, u64>,
    d3d9_records: u64,
    other_records: u64,
}

pub fn scan<R: Read>(source: R) -> Result<Json> {
    let mut reader = LogReader::open(source)?;
    let mut s = Scan::default();
    loop {
        let ordinal = reader.ordinal();
        let Some(record) = reader.next_record()? else {
            break;
        };
        let LogRecord::D3d9Call { method, args, .. } = record else {
            s.other_records += 1;
            continue;
        };
        let class = classify(method).ok_or_else(|| StreamError::UnknownMethod {
            ordinal,
            method: method.clone(),
        })?;
        s.d3d9_records += 1;
        *s.methods.entry(method.clone()).or_default() += 1;
        *s.classes.entry(class.name()).or_default() += 1;
        let w = |i: usize| word(args.get(i));
        match (class, method.as_str()) {
            (Class::Unsupported, _) => *s.unsupported.entry(method.clone()).or_default() += 1,
            (_, "SetRenderState") => {
                if let (Some(a), Some(b)) = (w(0), w(1)) {
                    *s.render.entry(a).or_default().entry(b).or_default() += 1;
                }
            }
            (_, "SetTextureStageState") => {
                if let (Some(stage), Some(kind), Some(v)) = (w(0), w(1), w(2)) {
                    *s.stage.entry(kind).or_default().entry(v).or_default() += 1;
                    *s.stages_used.entry(stage).or_default() += 1;
                }
            }
            (_, "SetSamplerState") => {
                if let (Some(sampler), Some(kind), Some(v)) = (w(0), w(1), w(2)) {
                    *s.sampler.entry(kind).or_default().entry(v).or_default() += 1;
                    *s.samplers_used.entry(sampler).or_default() += 1;
                }
            }
            (_, "SetTransform") => {
                if let Some(k) = w(0) {
                    *s.transforms.entry(k).or_default() += 1;
                }
            }
            (_, "SetFVF") => {
                if let Some(f) = w(0) {
                    *s.fvf.entry(f).or_default() += 1;
                }
            }
            (_, "SetLight") => {
                if let Some(Value::Bytes(b)) = args.get(1) {
                    if b.len() >= 4 {
                        let t = u32::from_le_bytes(b[0..4].try_into().unwrap());
                        *s.lights.entry(t).or_default() += 1;
                    }
                }
            }
            (_, "SetViewport") => {
                if let Some(Value::Bytes(b)) = args.first() {
                    if b.len() == 24 {
                        let f = |o: usize| u32::from_le_bytes(b[o..o + 4].try_into().unwrap());
                        *s.viewports
                            .entry(format!("{}x{}+{}+{}", f(8), f(12), f(0), f(4)))
                            .or_default() += 1;
                    }
                }
            }
            (_, "SetVertexShader") => s.nonzero_vertex_shaders += u64::from(w(0) != Some(0)),
            (_, "SetPixelShader") => s.nonzero_pixel_shaders += u64::from(w(0) != Some(0)),
            (_, "SetVertexDeclaration") => s.declarations_bound += u64::from(w(0) != Some(0)),
            (_, "CreateTexture") => {
                if let (Some(wd), Some(h), Some(l), Some(u), Some(f), Some(p)) =
                    (w(0), w(1), w(2), w(3), w(4), w(5))
                {
                    *s.textures
                        .entry(format!(
                            "{}x{} levels={l} usage=0x{u:x} format={} pool={p}",
                            wd,
                            h,
                            format_name(f)
                        ))
                        .or_default() += 1;
                }
            }
            (_, "CreateVertexBuffer") => {
                if let (Some(u), Some(f), Some(p)) = (w(1), w(2), w(3)) {
                    *s.vertex_buffers
                        .entry(format!("usage=0x{u:x} fvf=0x{f:x} pool={p}"))
                        .or_default() += 1;
                }
            }
            (_, "CreateIndexBuffer") => {
                if let Some(f) = w(2) {
                    *s.index_formats.entry(f).or_default() += 1;
                }
            }
            (_, "CreateRenderTarget") => {
                if let (Some(a), Some(b), Some(f), Some(m)) = (w(0), w(1), w(2), w(3)) {
                    *s.render_targets
                        .entry(format!("{a}x{b} {} msaa={m}", format_name(f)))
                        .or_default() += 1;
                }
            }
            (_, "CreateDepthStencilSurface") => {
                if let (Some(a), Some(b), Some(f)) = (w(0), w(1), w(2)) {
                    *s.depth_surfaces
                        .entry(format!("{a}x{b} {}", format_name(f)))
                        .or_default() += 1;
                }
            }
            (Class::Draw, _) => {
                s.current_draws += 1;
                if let Some(t) = w(0) {
                    *s.primitives
                        .entry(format!("{} {}", method, primitive_name(i64::from(t))))
                        .or_default() += 1;
                }
            }
            (Class::Present, _) => {
                s.presents += 1;
                s.draws_per_frame.push(std::mem::take(&mut s.current_draws));
            }
            (Class::Scene, "BeginScene") => s.scene_pairs.0 += 1,
            (Class::Scene, _) => s.scene_pairs.1 += 1,
            _ => {}
        }
    }
    let frames = s.draws_per_frame.len() as u64;
    let total: u64 = s.draws_per_frame.iter().sum();
    let fixed_function = s.nonzero_vertex_shaders == 0 && s.nonzero_pixel_shaders == 0;
    Ok(json!({
        "schema": super::CANON_SCHEMA,
        "kind": "inventory",
        "d3d9_records": s.d3d9_records,
        "other_records": s.other_records,
        "presents": s.presents,
        "begin_end_scene": [s.scene_pairs.0, s.scene_pairs.1],
        "classes": s.classes,
        "methods": s.methods,
        "render_states": Json::Object(s.render.iter().map(|(k, v)| (
            k.to_string(), json!({"name": render_state_name(*k), "values": v.iter().map(|(a, b)| (format!("0x{a:x}"), *b)).collect::<BTreeMap<_, _>>()}))).collect()),
        "texture_stage_states": Json::Object(s.stage.iter().map(|(k, v)| (
            k.to_string(), json!({"name": texture_stage_name(*k), "values": v.iter().map(|(a, b)| (format!("0x{a:x}"), *b)).collect::<BTreeMap<_, _>>()}))).collect()),
        "sampler_states": Json::Object(s.sampler.iter().map(|(k, v)| (
            k.to_string(), json!({"name": sampler_name(*k), "values": v.iter().map(|(a, b)| (format!("0x{a:x}"), *b)).collect::<BTreeMap<_, _>>()}))).collect()),
        "texture_stages_used": s.stages_used,
        "samplers_used": s.samplers_used,
        "transforms": named(&s.transforms, transform_name),
        "fvf": named(&s.fvf, |f| format!("0x{f:x}")),
        "light_types": named(&s.lights, |t| match t { 1 => "POINT", 2 => "SPOT", 3 => "DIRECTIONAL", _ => "UNKNOWN" }.to_owned()),
        "draw_primitives": distinct(&s.primitives),
        "viewports": distinct(&s.viewports),
        "texture_creations": distinct(&s.textures),
        "vertex_buffer_creations": distinct(&s.vertex_buffers),
        "index_buffer_formats": named(&s.index_formats, format_name),
        "render_targets": distinct(&s.render_targets),
        "depth_stencil_surfaces": distinct(&s.depth_surfaces),
        "programmable_pipeline": {
            "fixed_function_only": fixed_function,
            "vertex_shaders_bound": s.nonzero_vertex_shaders,
            "pixel_shaders_bound": s.nonzero_pixel_shaders,
            "vertex_declarations_bound": s.declarations_bound,
            "shader_constant_calls": s.methods.iter().filter(|(m, _)| m.contains("ShaderConstant")).map(|(_, c)| c).sum::<u64>(),
        },
        "unsupported_methods": s.unsupported,
        "frames": {
            "count": frames,
            "draws_total": total,
            "draws_min": s.draws_per_frame.iter().min().copied().unwrap_or(0),
            "draws_max": s.draws_per_frame.iter().max().copied().unwrap_or(0),
            "draws_mean_x100": (total * 100).checked_div(frames).unwrap_or(0),
        },
    }))
}

#[cfg(test)]
mod tests {
    use super::super::canon::testing::*;
    use super::*;

    #[test]
    fn inventory_counts_states_and_draws() {
        let bytes = log(&[
            ("SetRenderState", vec![i(27), i(1)]),
            ("SetRenderState", vec![i(27), i(1)]),
            ("SetTextureStageState", vec![i(1), i(1), i(4)]),
            ("SetFVF", vec![i(274)]),
            (
                "DrawIndexedPrimitive",
                vec![i(4), i(0), i(0), i(4), i(0), i(2)],
            ),
            ("Present", vec![slr_calllog::Value::Bool(false); 4]),
        ]);
        let inv = scan(&bytes[..]).unwrap();
        assert_eq!(inv["render_states"]["27"]["name"], "ALPHABLENDENABLE");
        assert_eq!(inv["render_states"]["27"]["values"]["0x1"], 2);
        assert_eq!(inv["texture_stages_used"]["1"], 1);
        assert_eq!(inv["fvf"]["274"]["count"], 1);
        assert_eq!(
            inv["draw_primitives"]["DrawIndexedPrimitive TRIANGLELIST"],
            1
        );
        assert_eq!(inv["frames"]["draws_max"], 1);
        assert_eq!(inv["programmable_pipeline"]["fixed_function_only"], true);
    }

    #[test]
    fn bound_shaders_are_reported() {
        let bytes = log(&[
            ("SetVertexShader", vec![i(9)]),
            ("SetPixelShader", vec![i(0)]),
        ]);
        let inv = scan(&bytes[..]).unwrap();
        assert_eq!(inv["programmable_pipeline"]["fixed_function_only"], false);
        assert_eq!(inv["programmable_pipeline"]["vertex_shaders_bound"], 1);
    }
}

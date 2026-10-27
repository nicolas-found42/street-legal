//! Port-side emission: the D3D9 calls implied by the translator's passes.
//!
//! The wgpu translator draws an optional opaque video quad, then two passes: a depth-tested world pass of
//! pre-transformed clip-space triangles with alpha cutout, then a 2D alpha-blended
//! OSD pass. This module states each pass in D3D9 terms and writes the result as
//! SLRLOG D3D9 records (the format the hooked original also produces), so the
//! same canonicalizer and comparison apply to both sides. It describes what the
//! translator does; it never copies original traces. Camera, light and material
//! state are not available to the translator today (the world pass receives
//! clip-space vertices from `slr-boot`), so no `SetTransform`, `SetLight` or
//! `SetMaterial` call is emitted: that absence is a real, reported divergence.
use super::taxonomy::log_kind;
use crate::Frame;
use slr_calllog::{D3d9CallKind, LogRecord, LogWriter, Value};
use std::{collections::HashMap, io::BufWriter, path::Path, sync::Mutex};

/// Environment variable naming the SLRLOG file the translator appends to.
pub const STREAM_ENV: &str = "SLR_D3D9_STREAM";

const D3DCLEAR_TARGET: i32 = 1;
const D3DCLEAR_ZBUFFER: i32 = 2;
const FVF_XYZRHW_DIFFUSE_TEX1: i32 = 0x004 | 0x040 | 0x100;
const FVF_XYZW_DIFFUSE_TEX1: i32 = 0x4002 | 0x040 | 0x100;
/// Position (4 floats) + packed diffuse + one texture coordinate pair.
const TRANSLATED_STRIDE: i32 = 28;

fn int(v: i32) -> Value {
    Value::Int(v)
}

pub type Call = (String, Vec<Value>);

fn call(method: &str, args: Vec<Value>) -> Call {
    (method.to_owned(), args)
}

fn presentation_parameters(size: (u32, u32)) -> Vec<u8> {
    let words: [u32; 14] = [size.0, size.1, 21, 1, 0, 0, 2, 0, 0, 1, 71, 0, 0, 0];
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

fn viewport(size: (u32, u32)) -> Vec<u8> {
    let mut v = Vec::new();
    for w in [0u32, 0, size.0, size.1] {
        v.extend(w.to_le_bytes());
    }
    v.extend(0.0f32.to_le_bytes());
    v.extend(1.0f32.to_le_bytes());
    v
}

/// Stateful emitter: remembers created textures across frames.
#[derive(Default)]
pub struct Emitter {
    created_device: bool,
    textures: HashMap<String, i32>,
    next_object: i32,
    logical_size: Option<(u32, u32)>,
    video: Option<((u32, u32), i32)>,
}

impl Emitter {
    fn texture(&mut self, name: &str, frame: &Frame, calls: &mut Vec<Call>) -> Option<i32> {
        if let Some(id) = self.textures.get(name) {
            return Some(*id);
        }
        let image = frame.images.get(name)?;
        self.next_object += 1;
        let id = 1000 + self.next_object;
        calls.push(call(
            "CreateTexture",
            vec![
                int(image.width as i32),
                int(image.height as i32),
                int(1),
                int(0),
                int(21),
                int(1),
                Value::Bool(true),
                Value::Bool(false),
            ],
        ));
        self.textures.insert(name.to_owned(), id);
        Some(id)
    }

    /// The calls one presented frame implies, in submission order.
    pub fn frame(&mut self, frame: &Frame) -> Vec<Call> {
        let mut calls = Vec::new();
        let size = frame.back_buffer_size();
        if !self.created_device {
            self.created_device = true;
            calls.push(call("Direct3DCreate9", vec![int(31)]));
            calls.push(call(
                "CreateDevice",
                vec![
                    int(0),
                    int(1),
                    Value::Bool(true),
                    int(0x84),
                    Value::Bytes(presentation_parameters(size)),
                    Value::Bool(true),
                ],
            ));
        }
        if self.logical_size.is_some_and(|previous| previous != size) {
            calls.push(call(
                "Reset",
                vec![Value::Bytes(presentation_parameters(size))],
            ));
        }
        self.logical_size = Some(size);
        if !frame.calls.is_empty() {
            calls.extend(frame.calls.iter().cloned());
            if calls.last().is_none_or(|(m, _)| m != "Present") {
                calls.push(call("Present", vec![Value::Bool(false); 4]));
            }
            return calls;
        }
        let world = !frame.world_draws.is_empty();
        calls.push(call(
            "Clear",
            vec![
                int(0),
                Value::Bool(false),
                int(D3DCLEAR_TARGET | if world { D3DCLEAR_ZBUFFER } else { 0 }),
                int(0xff00_0000u32 as i32),
                Value::float(1.0),
                int(0),
            ],
        ));
        calls.push(call("SetViewport", vec![Value::Bytes(viewport(size))]));
        calls.push(call("BeginScene", vec![]));
        let blend = |calls: &mut Vec<Call>| {
            for (state, value) in [
                (27, 1),
                (19, 5),
                (20, 6),
                (206, 1),
                (207, 2),
                (208, 6),
                (137, 0),
                (22, 1),
            ] {
                calls.push(call("SetRenderState", vec![int(state), int(value)]));
            }
            for (kind, value) in [(1, 4), (2, 2), (3, 0), (4, 4), (5, 2), (6, 0)] {
                calls.push(call(
                    "SetTextureStageState",
                    vec![int(0), int(kind), int(value)],
                ));
            }
        };
        if let Some(image) = &frame.video {
            let dimensions = (image.width, image.height);
            let id = if let Some((_, id)) = self.video.filter(|(size, _)| *size == dimensions) {
                id
            } else {
                self.next_object += 1;
                let id = 1000 + self.next_object;
                calls.push(call(
                    "CreateTexture",
                    vec![
                        int(image.width as i32),
                        int(image.height as i32),
                        int(1),
                        int(0),
                        int(21),
                        int(1),
                        Value::Bool(true),
                        Value::Bool(false),
                    ],
                ));
                self.video = Some((dimensions, id));
                id
            };
            for (state, value) in [(7, 0), (14, 0), (15, 0), (27, 0), (137, 0), (22, 1)] {
                calls.push(call("SetRenderState", vec![int(state), int(value)]));
            }
            for (kind, value) in [(1, 2), (2, 2), (3, 0), (4, 2), (5, 2), (6, 0)] {
                calls.push(call(
                    "SetTextureStageState",
                    vec![int(0), int(kind), int(value)],
                ));
            }
            for (kind, value) in [(1, 3), (2, 3), (5, 2), (6, 2), (7, 0)] {
                calls.push(call("SetSamplerState", vec![int(0), int(kind), int(value)]));
            }
            calls.push(call("SetFVF", vec![int(FVF_XYZRHW_DIFFUSE_TEX1)]));
            calls.push(call("SetTexture", vec![int(0), int(id)]));
            calls.push(call(
                "DrawPrimitiveUP",
                vec![int(4), int(2), Value::Bool(true), int(TRANSLATED_STRIDE)],
            ));
        }
        if world {
            blend(&mut calls);
            for (state, value) in [(7, 1), (14, 1), (23, 4), (15, 1), (24, 2), (25, 5)] {
                calls.push(call("SetRenderState", vec![int(state), int(value)]));
            }
            for (kind, value) in [(1, 1), (2, 1), (5, 2), (6, 2), (7, 0)] {
                calls.push(call("SetSamplerState", vec![int(0), int(kind), int(value)]));
            }
            calls.push(call("SetFVF", vec![int(FVF_XYZW_DIFFUSE_TEX1)]));
            for (name, vertices) in &frame.world_draws {
                if let Some(id) = self.texture(name, frame, &mut calls) {
                    calls.push(call("SetTexture", vec![int(0), int(id)]));
                    calls.push(call(
                        "DrawPrimitiveUP",
                        vec![
                            int(4),
                            int((vertices.len() / 3) as i32),
                            Value::Bool(true),
                            int(TRANSLATED_STRIDE),
                        ],
                    ));
                }
            }
        }
        if !frame.draws.is_empty() {
            blend(&mut calls);
            for (state, value) in [(7, 0), (15, 0)] {
                calls.push(call("SetRenderState", vec![int(state), int(value)]));
            }
            for (kind, value) in [(1, 3), (2, 3), (5, 2), (6, 2), (7, 0)] {
                calls.push(call("SetSamplerState", vec![int(0), int(kind), int(value)]));
            }
            calls.push(call("SetFVF", vec![int(FVF_XYZRHW_DIFFUSE_TEX1)]));
            for (name, vertices) in &frame.draws {
                if let Some(id) = self.texture(name, frame, &mut calls) {
                    calls.push(call("SetTexture", vec![int(0), int(id)]));
                    calls.push(call(
                        "DrawPrimitiveUP",
                        vec![
                            int(4),
                            int((vertices.len() / 3) as i32),
                            Value::Bool(true),
                            int(TRANSLATED_STRIDE),
                        ],
                    ));
                }
            }
        }
        calls.push(call("EndScene", vec![]));
        calls.push(call("Present", vec![Value::Bool(false); 4]));
        calls
    }
}

pub fn to_record((method, args): Call) -> LogRecord {
    LogRecord::D3d9Call {
        kind: D3d9CallKind::try_from(log_kind(method.rsplit('.').next().unwrap_or(&method)))
            .expect("log_kind returns an assigned kind"),
        method,
        args,
    }
}

/// Encode calls as a complete SLRLOG document.
pub fn encode(calls: impl IntoIterator<Item = Call>) -> Result<Vec<u8>, slr_calllog::LogError> {
    let mut writer = LogWriter::create(Vec::new())?;
    for c in calls {
        writer.record(&to_record(c))?;
    }
    Ok(writer.into_inner())
}

struct Sink {
    emitter: Emitter,
    writer: LogWriter<BufWriter<std::fs::File>>,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

/// Append the D3D9 calls implied by `frame` to the file named by
/// [`STREAM_ENV`]; a no-op when the variable is unset. Called once per frame
/// handed to the translator.
pub fn record_frame(frame: &Frame) -> Result<(), String> {
    let Some(path) = std::env::var_os(STREAM_ENV) else {
        return Ok(());
    };
    let mut guard = SINK.lock().map_err(|e| e.to_string())?;
    if guard.is_none() {
        let file = std::fs::File::create(Path::new(&path)).map_err(|e| e.to_string())?;
        *guard = Some(Sink {
            emitter: Emitter::default(),
            writer: LogWriter::create(BufWriter::new(file)).map_err(|e| e.to_string())?,
        });
    }
    let sink = guard.as_mut().expect("sink initialised above");
    for c in sink.emitter.frame(frame) {
        sink.writer
            .record(&to_record(c))
            .map_err(|e| e.to_string())?;
    }
    sink.writer.flush().map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::super::{canon::testing::frames, compare};
    use super::*;
    use crate::{Image, Vertex, WorldVertex};
    use std::sync::Arc;

    fn scene() -> Frame {
        let mut frame = Frame::default();
        frame.images.insert(
            "white".into(),
            Image {
                width: 2,
                height: 2,
                rgba: Arc::new(vec![255; 16]),
            },
        );
        let w = |x| WorldVertex {
            position: [x, 0., 0.5, 1.],
            uv: [0.; 2],
            color: [1.; 4],
        };
        frame
            .world_draws
            .push(("white".into(), vec![w(0.), w(1.), w(2.)]));
        let v = |x| Vertex {
            position: [x, 0.],
            uv: [0.; 2],
            color: [1.; 4],
        };
        frame.draws.push((
            "white".into(),
            vec![v(0.), v(1.), v(2.), v(3.), v(4.), v(5.)],
        ));
        frame
    }

    #[test]
    fn a_translated_frame_is_one_presented_frame_with_both_passes() {
        let mut e = Emitter::default();
        let bytes = encode(e.frame(&scene())).unwrap();
        let f = frames(&bytes);
        assert_eq!(f.len(), 1);
        assert!(f[0].presented);
        let draws: Vec<_> = f[0].events.iter().filter(|e| e["k"] == "draw").collect();
        assert_eq!(draws.len(), 2);
        assert!(
            draws[0]["state"].get("rs.7").is_none(),
            "world pass keeps the default depth test"
        );
        assert_eq!(
            draws[1]["state"]["rs.7"], "0x00000000",
            "OSD pass disables depth"
        );
        assert_eq!(draws[0]["a"][1], 1);
        assert_eq!(draws[1]["a"][1], 2);
        assert_eq!(draws[0]["state"]["vfmt"], "fvf:0x4142");
        assert_eq!(draws[1]["state"]["vfmt"], "fvf:0x144");
    }

    #[test]
    fn both_passes_describe_the_actual_separate_alpha_blend() {
        let bytes = encode(Emitter::default().frame(&scene())).unwrap();
        let frames = frames(&bytes);
        let draws: Vec<_> = frames[0]
            .events
            .iter()
            .filter(|e| e["k"] == "draw")
            .collect();
        assert_eq!(draws.len(), 2);
        for draw in draws {
            assert_eq!(
                draw["state"]["rs.206"], "0x00000001",
                "separate alpha enabled"
            );
            // ONE is the default alpha source factor; normalization omits it.
            assert!(draw["state"].get("rs.207").is_none());
            assert_eq!(super::super::defaults::render_state(207, None), Some(2));
            assert_eq!(
                draw["state"]["rs.208"], "0x00000006",
                "alpha destination is inverse source alpha"
            );
        }
        assert_eq!(
            wgpu::BlendState::ALPHA_BLENDING.alpha.src_factor,
            wgpu::BlendFactor::One
        );
        assert_eq!(
            wgpu::BlendState::ALPHA_BLENDING.alpha.dst_factor,
            wgpu::BlendFactor::OneMinusSrcAlpha
        );
    }
    #[test]
    fn producer_supplied_calls_replace_the_translator_implied_ones() {
        let mut frame = scene();
        frame.calls = vec![
            call("BeginScene", vec![]),
            call("DrawPrimitive", vec![int(1), int(0), int(1)]),
            call("EndScene", vec![]),
        ];
        let bytes = encode(Emitter::default().frame(&frame)).unwrap();
        let f = frames(&bytes);
        assert_eq!(f.len(), 1);
        let kinds: Vec<_> = f[0]
            .events
            .iter()
            .map(|e| e["k"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(kinds, ["begin_scene", "draw", "end_scene", "present"]);
        assert_eq!(f[0].events[1]["m"], "DrawPrimitive");
    }

    #[test]
    fn emission_is_deterministic_and_textures_are_created_once() {
        let a = encode(Emitter::default().frame(&scene())).unwrap();
        let b = encode(Emitter::default().frame(&scene())).unwrap();
        assert_eq!(a, b);
        let mut e = Emitter::default();
        let first = e.frame(&scene());
        let second = e.frame(&scene());
        let creates = |c: &[Call]| c.iter().filter(|(m, _)| m == "CreateTexture").count();
        assert_eq!(creates(&first), 1);
        assert_eq!(creates(&second), 0);
        assert!(!second.iter().any(|(m, _)| m == "CreateDevice"));
    }

    #[test]
    fn the_translator_stream_diverges_from_a_stream_with_camera_state() {
        // The translator has no camera/light/material state; a stream that sets a
        // view transform before the same draw must be reported as divergent.
        let mut e = Emitter::default();
        let port = encode(e.frame(&scene())).unwrap();
        let mut calls = Emitter::default().frame(&scene());
        let at = calls.iter().position(|(m, _)| m == "SetFVF").unwrap();
        let mut moved = super::super::defaults::IDENTITY.to_vec();
        moved[48] = 1;
        calls.insert(at, call("SetTransform", vec![int(2), Value::Bytes(moved)]));
        let reference = encode(calls).unwrap();
        let report = compare::compare(
            &mut super::super::canon::Canonicalizer::new(&reference[..]).unwrap(),
            &mut super::super::canon::Canonicalizer::new(&port[..]).unwrap(),
            &compare::Options::default(),
        )
        .unwrap();
        assert_eq!(report["equal"], false);
        let d = &report["frames"][0]["first_divergence"];
        assert_eq!(d["differences"][0]["field"], "state.xf.2");
    }
}

#[cfg(test)]
mod display_tests {
    use super::*;
    use crate::VideoImage;
    use std::sync::Arc;
    #[test]
    fn video_pass_and_logical_dimensions_are_transcribed_before_world_and_osd() {
        let mut frame = Frame {
            logical_size: Some((640, 480)),
            video: Some(Box::new(VideoImage {
                serial: 0,
                width: 2,
                height: 1,
                rgba: Arc::new(vec![255; 8]),
            })),
            ..Default::default()
        };
        let mut emitter = Emitter::default();
        let calls = emitter.frame(&frame);
        let device = calls.iter().find(|(m, _)| m == "CreateDevice").unwrap();
        let Value::Bytes(parameters) = &device.1[4] else {
            panic!("parameters")
        };
        assert_eq!(
            &parameters[..8],
            &[640u32.to_le_bytes(), 480u32.to_le_bytes()].concat()
        );
        let viewport = calls.iter().find(|(m, _)| m == "SetViewport").unwrap();
        let Value::Bytes(viewport) = &viewport.1[0] else {
            panic!("viewport")
        };
        assert_eq!(
            &viewport[8..16],
            &[640u32.to_le_bytes(), 480u32.to_le_bytes()].concat()
        );
        assert_eq!(
            calls.iter().filter(|(m, _)| m == "DrawPrimitiveUP").count(),
            1
        );
        assert!(calls
            .iter()
            .any(|(m, a)| m == "SetRenderState" && *a == vec![int(27), int(0)]));
        let next = emitter.frame(&frame);
        assert!(!next.iter().any(|(m, _)| m == "CreateTexture"));
        frame.logical_size = Some((800, 600));
        let resized = emitter.frame(&frame);
        assert!(resized.iter().any(|(m, _)| m == "Reset"));
        frame.video = None;
        let no_video = emitter.frame(&frame);
        assert!(!no_video.iter().any(|(m, _)| m == "DrawPrimitiveUP"));
    }
}

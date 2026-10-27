//! Effective D3D9 device state reconstructed from the call stream.
//!
//! The tracker stores only values that differ from the documented default
//! (setting a value equal to its default removes it), so two streams whose
//! redundant or reordered `Set*` calls leave the same effective state produce
//! the same snapshot. Resource handles are normalized to first-use ordinals.
use super::{defaults, Result, StreamError};
use slr_calllog::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        out.push_str(&format!("{b:02x}"));
    }
    out
}

/// Access to one call's arguments with fail-closed diagnostics.
pub struct Args<'a> {
    pub ordinal: u64,
    pub method: &'a str,
    pub values: &'a [Value],
}

impl Args<'_> {
    fn bad(&self, index: usize, why: &'static str) -> StreamError {
        StreamError::BadArgument {
            ordinal: self.ordinal,
            method: self.method.to_owned(),
            index,
            why,
        }
    }
    pub fn word(&self, index: usize) -> Result<u32> {
        match self.values.get(index) {
            Some(Value::Int(v)) => Ok(*v as u32),
            Some(Value::Bool(b)) => Ok(u32::from(*b)),
            Some(_) => Err(self.bad(index, "expected an integer")),
            None => Err(self.bad(index, "missing argument")),
        }
    }
    pub fn raw(&self, index: usize) -> Result<i32> {
        self.word(index).map(|v| v as i32)
    }
    pub fn bytes(&self, index: usize, len: usize) -> Result<Vec<u8>> {
        match self.values.get(index) {
            Some(Value::Bytes(b)) if b.len() == len => Ok(b.clone()),
            Some(Value::Bytes(_)) => Err(self.bad(index, "structure has the wrong size")),
            Some(_) => Err(self.bad(index, "expected a structure payload")),
            None => Err(self.bad(index, "missing argument")),
        }
    }
    /// A pointer argument that the log records as present/absent only.
    pub fn pointer(&self, index: usize) -> Result<bool> {
        match self.values.get(index) {
            Some(Value::Bool(b)) => Ok(*b),
            Some(Value::Bytes(_)) => Ok(true),
            Some(_) => Err(self.bad(index, "expected a pointer flag")),
            None => Err(self.bad(index, "missing argument")),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum VertexFormat {
    Fvf(u32),
    Declaration(u32),
}

#[derive(Clone, Default)]
pub struct DeviceState {
    auto_depth: Option<bool>,
    ordinals: HashMap<i32, u32>,
    render: BTreeMap<u32, u32>,
    stage: BTreeMap<(u32, u32), u32>,
    sampler: BTreeMap<(u32, u32), u32>,
    transforms: BTreeMap<u32, Vec<u8>>,
    viewport: Option<Vec<u8>>,
    material: Option<Vec<u8>>,
    scissor: Option<Vec<u8>>,
    lights: BTreeMap<u32, Vec<u8>>,
    enabled: BTreeSet<u32>,
    clip_planes: BTreeSet<u32>,
    textures: BTreeMap<u32, u32>,
    streams: BTreeMap<u32, (u32, u32, u32)>,
    divider: BTreeMap<u32, u32>,
    indices: Option<u32>,
    vertex_format: Option<VertexFormat>,
    vertex_shader: Option<u32>,
    pixel_shader: Option<u32>,
    targets: BTreeMap<u32, u32>,
    depth_stencil: Option<u32>,
}

impl DeviceState {
    /// First-use ordinal of a raw COM object id; `None` for the null handle.
    pub fn ordinal(&mut self, raw: i32) -> Option<u32> {
        if raw == 0 {
            return None;
        }
        let next = self.ordinals.len() as u32 + 1;
        Some(*self.ordinals.entry(raw).or_insert(next))
    }

    fn present_parameters(&mut self, args: &Args) -> Result<()> {
        let index = match args.method {
            "CreateDevice" => 4,
            _ => 0,
        };
        let bytes = args.bytes(index, 56)?;
        let auto = u32::from_le_bytes(bytes[36..40].try_into().unwrap());
        self.auto_depth = Some(auto != 0);
        Ok(())
    }

    /// Reset to creation defaults, as `Reset` does for non-pool state.
    pub fn reset(&mut self, args: &Args) -> Result<()> {
        let ordinals = std::mem::take(&mut self.ordinals);
        *self = DeviceState {
            ordinals,
            ..DeviceState::default()
        };
        self.present_parameters(args)
    }

    pub fn device_created(&mut self, args: &Args) -> Result<()> {
        self.present_parameters(args)
    }

    /// Apply one state-class call. Returns an ordered event for calls whose
    /// payload the log does not carry (shader constants).
    pub fn apply(&mut self, args: &Args) -> Result<Option<serde_json::Value>> {
        use serde_json::json;
        match args.method {
            "SetRenderState" => {
                let (state, value) = (args.word(0)?, args.word(1)?);
                if defaults::render_state(state, self.auto_depth) == Some(value) {
                    self.render.remove(&state);
                } else {
                    self.render.insert(state, value);
                }
            }
            "SetTextureStageState" => {
                let (stage, kind, value) = (args.word(0)?, args.word(1)?, args.word(2)?);
                if defaults::texture_stage_state(stage, kind) == Some(value) {
                    self.stage.remove(&(stage, kind));
                } else {
                    self.stage.insert((stage, kind), value);
                }
            }
            "SetSamplerState" => {
                let (sampler, kind, value) = (args.word(0)?, args.word(1)?, args.word(2)?);
                if defaults::sampler_state(kind) == Some(value) {
                    self.sampler.remove(&(sampler, kind));
                } else {
                    self.sampler.insert((sampler, kind), value);
                }
            }
            "SetTransform" => {
                let (kind, matrix) = (args.word(0)?, args.bytes(1, 64)?);
                if matrix == defaults::IDENTITY {
                    self.transforms.remove(&kind);
                } else {
                    self.transforms.insert(kind, matrix);
                }
            }
            "MultiplyTransform" => {
                return Err(StreamError::Unsupported {
                    ordinal: args.ordinal,
                    method: args.method.to_owned(),
                    why: "the product depends on the prior matrix; not modelled",
                })
            }
            "SetViewport" => self.viewport = Some(args.bytes(0, 24)?),
            "SetMaterial" => self.material = Some(args.bytes(0, 68)?),
            "SetScissorRect" => self.scissor = Some(args.bytes(0, 16)?),
            "SetLight" => {
                let index = args.word(0)?;
                self.lights.insert(index, args.bytes(1, 104)?);
            }
            "LightEnable" => {
                let index = args.word(0)?;
                if args.word(1)? != 0 {
                    self.enabled.insert(index);
                } else {
                    self.enabled.remove(&index);
                }
            }
            "SetClipPlane" => {
                self.clip_planes.insert(args.word(0)?);
            }
            "SetFVF" => self.vertex_format = Some(VertexFormat::Fvf(args.word(0)?)),
            "SetVertexDeclaration" => {
                self.vertex_format = self.ordinal(args.raw(0)?).map(VertexFormat::Declaration);
            }
            "SetTexture" => {
                let stage = args.word(0)?;
                match self.ordinal(args.raw(1)?) {
                    Some(t) => self.textures.insert(stage, t),
                    None => self.textures.remove(&stage),
                };
            }
            "SetStreamSource" => {
                let stream = args.word(0)?;
                match self.ordinal(args.raw(1)?) {
                    Some(b) => self
                        .streams
                        .insert(stream, (b, args.word(2)?, args.word(3)?)),
                    None => self.streams.remove(&stream),
                };
            }
            "SetStreamSourceFreq" => {
                let (stream, divider) = (args.word(0)?, args.word(1)?);
                if divider == 1 {
                    self.divider.remove(&stream);
                } else {
                    self.divider.insert(stream, divider);
                }
            }
            "SetIndices" => self.indices = self.ordinal(args.raw(0)?),
            "SetVertexShader" => self.vertex_shader = self.ordinal(args.raw(0)?),
            "SetPixelShader" => self.pixel_shader = self.ordinal(args.raw(0)?),
            "SetRenderTarget" => {
                let index = args.word(0)?;
                match self.ordinal(args.raw(1)?) {
                    Some(s) => self.targets.insert(index, s),
                    None => self.targets.remove(&index),
                };
            }
            "SetDepthStencilSurface" => self.depth_stencil = self.ordinal(args.raw(0)?),
            "SetVertexShaderConstantF"
            | "SetVertexShaderConstantI"
            | "SetVertexShaderConstantB"
            | "SetPixelShaderConstantF"
            | "SetPixelShaderConstantI"
            | "SetPixelShaderConstantB" => {
                return Ok(Some(json!({
                    "k": "constants",
                    "m": args.method,
                    "register": args.word(0)?,
                    "count": args.word(2)?,
                    "payload": "unlogged",
                })));
            }
            "SetClipStatus"
            | "SetPaletteEntries"
            | "SetCurrentTexturePalette"
            | "SetNPatchMode"
            | "SetSoftwareVertexProcessing" => {
                return Err(StreamError::Unsupported {
                    ordinal: args.ordinal,
                    method: args.method.to_owned(),
                    why: "legacy state with no canonical representation",
                })
            }
            other => unreachable!("{other} is not a state-class method"),
        }
        Ok(None)
    }

    /// Effective non-default state as stable `key -> value` text.
    pub fn snapshot(&self) -> BTreeMap<String, String> {
        let mut out = BTreeMap::new();
        let word = |v: u32| format!("0x{v:08x}");
        for (k, v) in &self.render {
            out.insert(format!("rs.{k}"), word(*v));
        }
        for ((s, k), v) in &self.stage {
            out.insert(format!("tss.{s}.{k}"), word(*v));
        }
        for ((s, k), v) in &self.sampler {
            out.insert(format!("samp.{s}.{k}"), word(*v));
        }
        for (k, m) in &self.transforms {
            out.insert(format!("xf.{k}"), hex(m));
        }
        if let Some(v) = &self.viewport {
            out.insert("viewport".into(), hex(v));
        }
        if let Some(v) = &self.material {
            out.insert("material".into(), hex(v));
        }
        if let Some(v) = &self.scissor {
            out.insert("scissor".into(), hex(v));
        }
        for index in &self.enabled {
            out.insert(
                format!("light.{index}"),
                self.lights
                    .get(index)
                    .map_or_else(|| "default".to_owned(), |l| hex(l)),
            );
        }
        for index in &self.clip_planes {
            out.insert(format!("clip.{index}"), "set;payload-unlogged".into());
        }
        for (s, t) in &self.textures {
            out.insert(format!("tex.{s}"), format!("r{t}"));
        }
        for (s, (b, off, stride)) in &self.streams {
            out.insert(format!("stream.{s}"), format!("r{b}@{off}/{stride}"));
        }
        for (s, d) in &self.divider {
            out.insert(format!("freq.{s}"), d.to_string());
        }
        if let Some(i) = self.indices {
            out.insert("ib".into(), format!("r{i}"));
        }
        match &self.vertex_format {
            Some(VertexFormat::Fvf(f)) => {
                out.insert("vfmt".into(), format!("fvf:0x{f:x}"));
            }
            Some(VertexFormat::Declaration(d)) => {
                out.insert("vfmt".into(), format!("decl:r{d}"));
            }
            None => {}
        }
        if let Some(s) = self.vertex_shader {
            out.insert("vs".into(), format!("r{s}"));
        }
        if let Some(s) = self.pixel_shader {
            out.insert("ps".into(), format!("r{s}"));
        }
        for (i, s) in &self.targets {
            out.insert(format!("rt.{i}"), format!("r{s}"));
        }
        if let Some(s) = self.depth_stencil {
            out.insert("ds".into(), format!("r{s}"));
        }
        out
    }

    /// The part of the state a `Clear` depends on.
    pub fn clear_snapshot(&self) -> BTreeMap<String, String> {
        let mut out = self.snapshot();
        out.retain(|k, _| {
            k.starts_with("rt.")
                || matches!(
                    k.as_str(),
                    "ds" | "viewport" | "scissor" | "rs.14" | "rs.52" | "rs.168" | "rs.174"
                )
        });
        out
    }

    /// Ordinal for an object argument of an ordered (non-state) event.
    pub fn object_ordinal(&mut self, args: &Args, index: usize) -> Result<serde_json::Value> {
        Ok(match self.ordinal(args.raw(index)?) {
            Some(o) => serde_json::json!(format!("r{o}")),
            None => serde_json::Value::Null,
        })
    }
}

//! Canonical per-frame stream: the shared form compared between the hooked
//! original and the port. See `docs/render.md` for the normative description.
use super::{
    state::{hex, Args, DeviceState},
    taxonomy::{classify, Class},
    Result, StreamError,
};
use serde_json::{json, Value as Json};
use sha2::{Digest, Sha256};
use slr_calllog::{LogReader, LogRecord, Value};
use std::{collections::BTreeMap, io::Read};

/// One presented frame: the events between two `Present` calls.
#[derive(Clone, Debug)]
pub struct Frame {
    pub index: u64,
    pub events: Vec<Json>,
    /// False for a trailing group of events with no closing `Present`.
    pub presented: bool,
}

impl Frame {
    pub fn sha256(&self) -> String {
        let mut h = Sha256::new();
        for e in &self.events {
            h.update(serde_json::to_vec(e).expect("json"));
            h.update(b"\n");
        }
        hex(&h.finalize())
    }
}

#[derive(Default, Clone, Debug)]
pub struct Tallies {
    /// D3D9 records consumed, by method.
    pub methods: BTreeMap<String, u64>,
    /// D3D9 records consumed, by class name.
    pub classes: BTreeMap<String, u64>,
    /// Non-D3D9 records skipped (native calls, DirectSound, ...).
    pub other_records: u64,
    pub d3d9_records: u64,
}

pub struct Canonicalizer<R: Read> {
    reader: LogReader<R>,
    state: DeviceState,
    next_index: u64,
    pub tallies: Tallies,
    done: bool,
    digest: Sha256,
}

fn value_json(v: &Value) -> Json {
    match v {
        Value::Int(i) => json!(i),
        Value::Long(i) => json!(format!("long:{i}")),
        Value::FloatBits(b) => json!(format!("f32:0x{b:08x}")),
        Value::DoubleBits(b) => json!(format!("f64:0x{b:016x}")),
        Value::Str(s) => json!(s),
        Value::Bytes(b) => json!(format!("bytes:{}", hex(b))),
        Value::Bool(b) => json!(b),
    }
}

fn args_json(values: &[Value]) -> Json {
    Json::Array(values.iter().map(value_json).collect())
}

impl<R: Read> Canonicalizer<R> {
    pub fn new(source: R) -> Result<Self> {
        Ok(Self {
            reader: LogReader::open(source)?,
            state: DeviceState::default(),
            next_index: 0,
            tallies: Tallies::default(),
            done: false,
            digest: Sha256::new(),
        })
    }

    /// Digest of every frame returned so far (index, presented flag and frame
    /// hash), meaningful as a whole-stream identity once the log is exhausted.
    pub fn stream_sha256(&self) -> String {
        hex(&self.digest.clone().finalize())
    }

    /// Next frame, or `None` once the log is exhausted.
    pub fn next_frame(&mut self) -> Result<Option<Frame>> {
        let frame = self.read_frame()?;
        if let Some(f) = &frame {
            self.digest
                .update(format!("{}:{}:{}\n", f.index, f.presented, f.sha256()));
        }
        Ok(frame)
    }

    fn read_frame(&mut self) -> Result<Option<Frame>> {
        if self.done {
            return Ok(None);
        }
        let mut events = Vec::new();
        loop {
            let ordinal = self.reader.ordinal();
            let Some(record) = self.reader.next_record()? else {
                self.done = true;
                if events.is_empty() {
                    return Ok(None);
                }
                let index = self.next_index;
                self.next_index += 1;
                return Ok(Some(Frame {
                    index,
                    events,
                    presented: false,
                }));
            };
            let LogRecord::D3d9Call { method, args, .. } = record else {
                self.tallies.other_records += 1;
                continue;
            };
            let (method, values) = (method.clone(), args.clone());
            let class = classify(&method).ok_or_else(|| StreamError::UnknownMethod {
                ordinal,
                method: method.clone(),
            })?;
            self.tallies.d3d9_records += 1;
            *self.tallies.methods.entry(method.clone()).or_default() += 1;
            *self
                .tallies
                .classes
                .entry(class.name().to_owned())
                .or_default() += 1;
            let call = Args {
                ordinal,
                method: &method,
                values: &values,
            };
            match class {
                Class::State => {
                    if let Some(event) = self.state.apply(&call)? {
                        events.push(event);
                    }
                }
                Class::Draw => {
                    // Vertex/index payloads passed by pointer are not in the log.
                    let payload = method.ends_with("UP");
                    events.push(json!({
                        "k": "draw",
                        "m": method,
                        "a": args_json(&values),
                        "payload": if payload { "unlogged" } else { "buffers" },
                        "state": self.state.snapshot(),
                    }));
                }
                Class::Scene => events.push(
                    json!({"k": if method == "BeginScene" {"begin_scene"} else {"end_scene"}}),
                ),
                Class::Clear => events.push(json!({
                    "k": "clear",
                    "a": args_json(&values),
                    "state": self.state.clear_snapshot(),
                })),
                Class::Present => {
                    events.push(json!({"k": "present", "a": args_json(&values)}));
                    let index = self.next_index;
                    self.next_index += 1;
                    return Ok(Some(Frame {
                        index,
                        events,
                        presented: true,
                    }));
                }
                Class::Transfer => {
                    let objects: &[usize] = match method.as_str() {
                        "StretchRect" | "UpdateSurface" => &[0, 2],
                        "ColorFill" => &[0],
                        _ => &[0, 1],
                    };
                    let mut a = values.iter().map(value_json).collect::<Vec<_>>();
                    for &i in objects {
                        a[i] = self.state.object_ordinal(&call, i)?;
                    }
                    events.push(json!({"k": "transfer", "m": method, "a": a}));
                }
                Class::Create => {
                    events.push(json!({"k": "create", "m": method, "a": args_json(&values)}));
                }
                Class::Reset => {
                    self.state.reset(&call)?;
                    events.push(json!({"k": "reset"}));
                }
                Class::Env => {
                    if method == "CreateDevice" {
                        self.state.device_created(&call)?;
                    }
                }
                Class::ResourceData | Class::Query => {}
                Class::Unsupported => {
                    return Err(StreamError::Unsupported {
                        ordinal,
                        method,
                        why: "state blocks, patches and swap chains are not modelled",
                    })
                }
            }
        }
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use slr_calllog::{D3d9CallKind, LogWriter};

    pub fn log(calls: &[(&str, Vec<Value>)]) -> Vec<u8> {
        let mut writer = LogWriter::create(Vec::new()).unwrap();
        for (method, args) in calls {
            writer
                .record(&LogRecord::D3d9Call {
                    kind: D3d9CallKind::DeviceState,
                    method: (*method).to_owned(),
                    args: args.clone(),
                })
                .unwrap();
        }
        writer.into_inner()
    }
    pub fn i(v: i32) -> Value {
        Value::Int(v)
    }
    pub fn frames(bytes: &[u8]) -> Vec<Frame> {
        let mut c = Canonicalizer::new(bytes).unwrap();
        let mut out = Vec::new();
        while let Some(f) = c.next_frame().unwrap() {
            out.push(f);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::testing::*;
    use super::*;

    fn draw() -> (&'static str, Vec<Value>) {
        (
            "DrawIndexedPrimitive",
            vec![i(4), i(0), i(0), i(4), i(0), i(2)],
        )
    }

    #[test]
    fn frames_split_at_present_and_trailing_events_stay_open() {
        let bytes = log(&[
            ("BeginScene", vec![]),
            draw(),
            ("EndScene", vec![]),
            ("Present", vec![Value::Bool(false); 4]),
            ("BeginScene", vec![]),
        ]);
        let f = frames(&bytes);
        assert_eq!(f.len(), 2);
        assert!(f[0].presented && !f[1].presented);
        assert_eq!(f[0].events.len(), 4);
    }

    #[test]
    fn redundant_and_default_sets_do_not_change_the_snapshot() {
        let plain = log(&[draw()]);
        let noisy = log(&[
            ("SetRenderState", vec![i(22), i(3)]), // CULLMODE default
            ("SetRenderState", vec![i(27), i(1)]),
            ("SetRenderState", vec![i(27), i(0)]), // back to default
            ("SetTextureStageState", vec![i(0), i(1), i(4)]), // default COLOROP
            ("SetSamplerState", vec![i(0), i(5), i(1)]),
            draw(),
        ]);
        assert_eq!(frames(&plain)[0].events, frames(&noisy)[0].events);
    }

    #[test]
    fn non_default_state_appears_on_the_draw() {
        let bytes = log(&[
            ("SetRenderState", vec![i(27), i(1)]),
            ("SetTextureStageState", vec![i(1), i(1), i(4)]),
            ("SetTexture", vec![i(0), i(77)]),
            ("SetFVF", vec![i(274)]),
            draw(),
        ]);
        let f = frames(&bytes);
        let state = &f[0].events[0]["state"];
        assert_eq!(state["rs.27"], "0x00000001");
        assert_eq!(state["tss.1.1"], "0x00000004");
        assert_eq!(state["tex.0"], "r1");
        assert_eq!(state["vfmt"], "fvf:0x112");
    }

    #[test]
    fn handles_normalize_to_first_use_ordinals() {
        let a = log(&[("SetTexture", vec![i(0), i(13)]), draw()]);
        let b = log(&[("SetTexture", vec![i(0), i(901)]), draw()]);
        assert_eq!(frames(&a)[0].events, frames(&b)[0].events);
    }

    #[test]
    fn identity_transform_is_default_and_other_matrices_show() {
        let mut m = super::super::defaults::IDENTITY.to_vec();
        let identity = log(&[
            ("SetTransform", vec![i(256), Value::Bytes(m.clone())]),
            draw(),
        ]);
        assert!(frames(&identity)[0].events[0]["state"]
            .get("xf.256")
            .is_none());
        m[48] = 1; // translation x bits change
        let moved = log(&[("SetTransform", vec![i(256), Value::Bytes(m)]), draw()]);
        assert!(frames(&moved)[0].events[0]["state"].get("xf.256").is_some());
    }

    #[test]
    fn unknown_and_unsupported_methods_fail_closed() {
        let bytes = log(&[("Frobnicate", vec![])]);
        let mut c = Canonicalizer::new(&bytes[..]).unwrap();
        assert!(matches!(
            c.next_frame(),
            Err(StreamError::UnknownMethod { .. })
        ));
        let bytes = log(&[("BeginStateBlock", vec![])]);
        let mut c = Canonicalizer::new(&bytes[..]).unwrap();
        assert!(matches!(
            c.next_frame(),
            Err(StreamError::Unsupported { .. })
        ));
    }

    #[test]
    fn zenable_default_follows_the_created_device() {
        let create = |auto: u8| {
            let mut pp = vec![0u8; 56];
            pp[36] = auto; // EnableAutoDepthStencil
            (
                "CreateDevice",
                vec![
                    i(0),
                    i(1),
                    Value::Bool(true),
                    i(0x40),
                    Value::Bytes(pp),
                    Value::Bool(true),
                ],
            )
        };
        let with = |auto: u8| {
            frames(&log(&[
                create(auto),
                ("SetRenderState", vec![i(7), i(1)]),
                draw(),
            ]))
        };
        assert!(with(1)[0].events[0]["state"].get("rs.7").is_none());
        assert_eq!(with(0)[0].events[0]["state"]["rs.7"], "0x00000001");
    }

    #[test]
    fn stream_digest_identifies_the_whole_canonical_stream() {
        let digest = |calls: &[(&str, Vec<Value>)]| {
            let bytes = log(calls);
            let mut c = Canonicalizer::new(&bytes[..]).unwrap();
            while c.next_frame().unwrap().is_some() {}
            c.stream_sha256()
        };
        let a = digest(&[draw(), ("Present", vec![Value::Bool(false); 4])]);
        assert_eq!(
            a,
            digest(&[draw(), ("Present", vec![Value::Bool(false); 4])])
        );
        let other = digest(&[
            ("SetRenderState", vec![i(27), i(1)]),
            draw(),
            ("Present", vec![Value::Bool(false); 4]),
        ]);
        assert_ne!(a, other);
    }

    #[test]
    fn constants_keep_their_order_and_flag_the_missing_payload() {
        let bytes = log(&[
            (
                "SetVertexShaderConstantF",
                vec![i(4), Value::Bool(true), i(2)],
            ),
            draw(),
        ]);
        let f = frames(&bytes);
        assert_eq!(f[0].events[0]["k"], "constants");
        assert_eq!(f[0].events[0]["payload"], "unlogged");
    }
}

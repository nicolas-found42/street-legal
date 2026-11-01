//! Software DirectSound device: the contract the ported sound library calls.
//!
//! This is the port-side twin of the headless original's declared device
//! (`re/headless/dsound_platform.py`): the same COM-contract checks, the same
//! HRESULTs, the same play-cursor model (a pure function of the virtual clock
//! and the call history), and the same canonical event for every call
//! (docs/audio.md). Nothing here renders audio; a sink reads `buffer_bytes`
//! and the play state to mix for the CoreAudio output.
//!
//! Where the Python model stops the run (`Boundary`: an unmodelled corner of the
//! DirectSound contract) this device returns [`DsError::Unmodelled`]. The
//! original cannot have produced a reference stream past such a point, and a
//! port that reaches one has left what is verified.

use std::cell::RefCell;
use std::rc::Rc;

use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

use crate::stream::{f32_word, Event, Stream};

pub const DS_OK: u32 = 0;
pub const DSERR_INVALIDPARAM: u32 = 0x8007_0057;
pub const DSERR_CONTROLUNAVAIL: u32 = 0x8878_001E;
pub const DSERR_PRIOLEVELNEEDED: u32 = 0x8878_0046;
pub const DSERR_BADFORMAT: u32 = 0x8878_0064;
pub const DSERR_INVALIDCALL: u32 = 0x8878_0032;
pub const DSERR_NOINTERFACE: u32 = 0x8000_4002;

pub const DSBCAPS_PRIMARYBUFFER: u32 = 0x1;
pub const DSBCAPS_STATIC: u32 = 0x2;
pub const DSBCAPS_CTRL3D: u32 = 0x10;
pub const DSBCAPS_CTRLFREQUENCY: u32 = 0x20;
pub const DSBCAPS_CTRLPAN: u32 = 0x40;
pub const DSBCAPS_CTRLVOLUME: u32 = 0x80;
const DSBCAPS_KNOWN: u32 = 0x1
    | 0x2
    | 0x4
    | 0x8
    | 0x10
    | 0x20
    | 0x40
    | 0x80
    | 0x100
    | 0x4000
    | 0x8000
    | 0x1_0000
    | 0x2_0000;

pub const DSCAPS_SIZE: u32 = 96;
/// Number of `DSCAPS` words after `dwSize` (`DSCAPS_FIELDS` in the Python model).
pub const DSCAPS_WORDS: usize = 23;

const IID_3D_LISTENER: [u8; 16] = [
    0x84, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0x00, 0x20, 0xaf, 0x0b, 0xe5, 0x60,
];
const IID_3D_BUFFER: [u8; 16] = [
    0x86, 0xfa, 0x9a, 0x27, 0x81, 0x49, 0xce, 0x11, 0xa5, 0x21, 0x00, 0x20, 0xaf, 0x0b, 0xe5, 0x60,
];

/// Declared device inputs (the platform profile's `sound` section).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DsProfile {
    pub caps: [u32; DSCAPS_WORDS],
    pub write_lead_ms: u32,
}

/// `WAVEFORMATEX` as the exe passes it (the first 16 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaveFormat {
    pub tag: u16,
    pub channels: u16,
    pub rate: u32,
    pub avg_bytes_per_sec: u32,
    pub block_align: u16,
    pub bits: u16,
}

impl WaveFormat {
    fn json(&self) -> Value {
        json!({"tag": self.tag, "channels": self.channels, "rate": self.rate,
               "avg_bytes_per_sec": self.avg_bytes_per_sec, "block_align": self.block_align, "bits": self.bits})
    }

    /// The Python `parse_format` validity rules (tag is checked by the caller).
    fn valid(&self) -> bool {
        matches!(self.channels, 1 | 2)
            && matches!(self.bits, 8 | 16)
            && self.rate != 0
            && u32::from(self.block_align) == u32::from(self.channels) * u32::from(self.bits) / 8
            && self.avg_bytes_per_sec == self.rate.wrapping_mul(u32::from(self.block_align))
    }
}

/// `DSBUFFERDESC` (20 or 36 bytes).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufferDesc {
    pub size: u32,
    pub flags: u32,
    pub bytes: u32,
    pub reserved: u32,
    pub format: Option<WaveFormat>,
    /// `guid3DAlgorithm`, present when `size == 36`.
    pub guid_3d_algorithm: [u8; 16],
}

impl BufferDesc {
    fn json(&self) -> Value {
        let mut m = Map::new();
        m.insert("size".into(), self.size.into());
        m.insert("flags".into(), self.flags.into());
        m.insert("bytes".into(), self.bytes.into());
        m.insert("reserved".into(), self.reserved.into());
        m.insert(
            "format".into(),
            self.format.map_or(Value::Null, |f| f.json()),
        );
        if self.size == 36 {
            m.insert(
                "guid_3d_algorithm".into(),
                hex(&self.guid_3d_algorithm).into(),
            );
        }
        Value::Object(m)
    }
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// A buffer handle: the creation ordinal (`buf#k`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BufId(pub usize);

/// Actual Unlock arguments, including malformed null-pointer/length pairs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UnlockArgs {
    pub first_offset: Option<u32>,
    pub len1: u32,
    pub second_offset: Option<u32>,
    pub len2: u32,
}

/// Why a call stopped instead of answering.
#[derive(Debug, PartialEq, Eq)]
pub enum DsError {
    /// The original's declared device does not model this corner; no reference exists.
    Unmodelled(String),
}

type Memory = Rc<RefCell<Vec<u8>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LockState {
    offset: u32,
    len1: u32,
    len2: u32,
}

struct Buffer {
    flags: u32,
    nbytes: u32,
    format: Option<WaveFormat>,
    primary: bool,
    memory: Memory,
    volume: i32,
    pan: i32,
    frequency: u32,
    playing: bool,
    looping: bool,
    start_ms: u64,
    base: u32,
    locked: Option<LockState>,
    refcount: u32,
    released: bool,
    three_d: Option<ThreeDState>,
}

struct ThreeDState {
    refcount: u32,
}

/// What `Lock` returned: byte ranges of the buffer memory (`len2 > 0` wraps).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Locked {
    pub offset: u32,
    pub len1: u32,
    pub len2: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Iid {
    Listener3d,
    Buffer3d,
    Other([u8; 16]),
}

impl Iid {
    pub const LISTENER_BYTES: [u8; 16] = IID_3D_LISTENER;
    pub const BUFFER_BYTES: [u8; 16] = IID_3D_BUFFER;
    fn name(&self) -> String {
        match self {
            Iid::Listener3d => "IID_IDirectSound3DListener".into(),
            Iid::Buffer3d => "IID_IDirectSound3DBuffer".into(),
            Iid::Other(b) => hex(b),
        }
    }
}

/// Result of `QueryInterface`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Queried {
    Listener,
    Buffer3d(BufId),
}

/// The device. Events are appended in call order.
pub struct DirectSound {
    profile: DsProfile,
    clock_ms: u64,
    windows: Vec<u64>,
    seen_windows: Vec<u64>,
    created: bool,
    ds_refcount: u32,
    ds_released: bool,
    coop_level: Option<u32>,
    buffers: Vec<Buffer>,
    primary: Option<BufId>,
    listener: Option<(u32, bool)>,
    natives: Option<u64>,
    events: Vec<Event>,
    resource_remaining: Option<u64>,
}

impl DirectSound {
    /// `windows` are the handles `SetCooperativeLevel` accepts (the declared window).
    pub fn new(profile: DsProfile, windows: Vec<u64>) -> DirectSound {
        DirectSound {
            profile,
            clock_ms: 0,
            windows,
            seen_windows: Vec::new(),
            created: false,
            ds_refcount: 0,
            ds_released: false,
            coop_level: None,
            buffers: Vec::new(),
            primary: None,
            listener: None,
            natives: None,
            events: Vec::new(),
            resource_remaining: None,
        }
    }

    /// Declared remaining shared resource arena; consumed storage is not reused.
    /// Configure before creating any buffers. Duplicates share existing storage.
    pub fn with_resource_budget(mut self, bytes: u64) -> Self {
        assert!(
            self.buffers.is_empty(),
            "resource budget must precede buffers"
        );
        self.resource_remaining = Some(bytes);
        self
    }

    pub fn set_clock_ms(&mut self, ms: u64) {
        self.clock_ms = ms;
    }

    pub fn clock_ms(&self) -> u64 {
        self.clock_ms
    }

    /// Script-native call count, recorded in `meta` for human correlation only.
    pub fn set_natives(&mut self, count: Option<u64>) {
        self.natives = count;
    }

    pub fn events(&self) -> &[Event] {
        &self.events
    }

    pub fn into_stream(self, header: Map<String, Value>) -> Stream {
        let mut stream = Stream::new(header);
        stream.events = self.events;
        stream
    }

    // ------------------------------------------------------------ plumbing
    fn name(&self, id: BufId) -> String {
        format!("buf#{}", id.0)
    }

    fn window_name(&mut self, handle: u64) -> String {
        if handle == 0 {
            return "null".into();
        }
        let index = match self.seen_windows.iter().position(|&h| h == handle) {
            Some(i) => i,
            None => {
                self.seen_windows.push(handle);
                self.seen_windows.len() - 1
            }
        };
        format!("window#{index}")
    }

    fn record(
        &mut self,
        call: &str,
        this: String,
        args: Value,
        ret: u32,
        out: Value,
        extra: Option<(&str, Value)>,
    ) {
        let mut meta = Map::new();
        meta.insert("clock_ms".into(), self.clock_ms.into());
        meta.insert(
            "natives".into(),
            self.natives.map_or(Value::Null, Value::from),
        );
        if let Some((k, v)) = extra {
            meta.insert(k.into(), v);
        }
        let i = self.events.len() as u64;
        self.events.push(Event {
            i,
            call: call.into(),
            this,
            args,
            ret: u64::from(ret),
            out,
            meta: Value::Object(meta),
        });
    }

    fn buf(&mut self, id: BufId) -> Result<&mut Buffer, DsError> {
        let released = self.buffers[id.0].released;
        if released {
            return Err(DsError::Unmodelled(format!(
                "released_com_object buf#{}",
                id.0
            )));
        }
        Ok(&mut self.buffers[id.0])
    }

    fn position(buf: &mut Buffer, clock_ms: u64) -> u32 {
        let Some(format) = buf.format.filter(|_| buf.playing) else {
            return buf.base;
        };
        let elapsed = clock_ms.wrapping_sub(buf.start_ms);
        let rate =
            u64::from(format.avg_bytes_per_sec) * u64::from(buf.frequency) / u64::from(format.rate);
        let mut moved = elapsed * rate / 1000;
        moved -= moved % u64::from(format.block_align);
        let total = u64::from(buf.base) + moved;
        if total >= u64::from(buf.nbytes) && !buf.looping {
            buf.playing = false;
            buf.base = 0;
            return 0;
        }
        (total % u64::from(buf.nbytes)) as u32
    }

    fn new_buffer(
        &mut self,
        flags: u32,
        nbytes: u32,
        format: Option<WaveFormat>,
        primary: bool,
        memory: Option<Memory>,
    ) -> BufId {
        let memory = memory.unwrap_or_else(|| Rc::new(RefCell::new(vec![0u8; nbytes as usize])));
        let frequency = format.map_or(0, |f| f.rate);
        self.buffers.push(Buffer {
            flags,
            nbytes,
            format,
            primary,
            memory,
            volume: 0,
            pan: 0,
            frequency,
            playing: false,
            looping: false,
            start_ms: 0,
            base: 0,
            locked: None,
            refcount: 1,
            released: false,
            three_d: None,
        });
        BufId(self.buffers.len() - 1)
    }

    // ------------------------------------------------------------ DirectSoundCreate
    /// `DirectSoundCreate(NULL, &ds, NULL)`. A second device is not modelled.
    pub fn create(&mut self) -> Result<u32, DsError> {
        if self.created {
            return Err(DsError::Unmodelled("second_directsound_interface".into()));
        }
        self.created = true;
        self.ds_refcount = 1;
        self.record(
            "DirectSoundCreate",
            "api".into(),
            json!({"guid_given": false, "outer": false, "out_given": true}),
            DS_OK,
            json!({"object": "ds"}),
            None,
        );
        Ok(DS_OK)
    }

    // ------------------------------------------------------------ IDirectSound
    fn live_device(&self) -> Result<(), DsError> {
        if !self.created {
            return Err(DsError::Unmodelled("device_not_created".into()));
        }
        if self.ds_released {
            return Err(DsError::Unmodelled("released_com_object ds".into()));
        }
        Ok(())
    }
    pub fn get_caps(&mut self, size: u32) -> Result<u32, DsError> {
        self.live_device()?;
        let (ret, out) = if size != DSCAPS_SIZE {
            (DSERR_INVALIDPARAM, json!({}))
        } else {
            let mut words = vec![DSCAPS_SIZE];
            words.extend_from_slice(&self.profile.caps);
            (DS_OK, json!({"caps": words}))
        };
        self.record(
            "IDirectSound.GetCaps",
            "ds".into(),
            json!({"size": size}),
            ret,
            out,
            None,
        );
        Ok(ret)
    }

    pub fn set_cooperative_level(&mut self, window: u64, level: u32) -> Result<u32, DsError> {
        self.live_device()?;
        let name = self.window_name(window);
        let ret = if !self.windows.contains(&window) || !(1..=4).contains(&level) {
            DSERR_INVALIDPARAM
        } else {
            self.coop_level = Some(level);
            DS_OK
        };
        self.record(
            "IDirectSound.SetCooperativeLevel",
            "ds".into(),
            json!({"window": name, "level": level}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn create_sound_buffer(
        &mut self,
        desc: &BufferDesc,
    ) -> Result<Result<BufId, u32>, DsError> {
        self.live_device()?;
        // The canonical recorder cannot describe other DSBUFFERDESC layouts.
        // Stop at the same boundary before emitting a fabricated call event.
        if !matches!(desc.size, 20 | 36) {
            return Err(DsError::Unmodelled("dsound_trace_desc_size".into()));
        }
        let args = json!({"desc": desc.json(), "outer": false, "out_given": true});
        let outcome = self.create_buffer_inner(desc)?;
        let (ret, out) = match &outcome {
            Ok(id) => (DS_OK, json!({"object": self.name(*id)})),
            Err(hr) => (*hr, json!({})),
        };
        self.record(
            "IDirectSound.CreateSoundBuffer",
            "ds".into(),
            args,
            ret,
            out,
            None,
        );
        Ok(outcome)
    }

    fn create_buffer_inner(&mut self, desc: &BufferDesc) -> Result<Result<BufId, u32>, DsError> {
        if !matches!(desc.size, 20 | 36) {
            return Ok(Err(DSERR_INVALIDPARAM));
        }
        let flags = desc.flags;
        if flags & !DSBCAPS_KNOWN != 0 {
            return Err(DsError::Unmodelled(format!(
                "unsupported_buffer_flags {flags:#x}"
            )));
        }
        if flags & DSBCAPS_PRIMARYBUFFER != 0 {
            if desc.bytes != 0 || desc.format.is_some() || flags & DSBCAPS_STATIC != 0 {
                return Ok(Err(DSERR_INVALIDPARAM));
            }
            if self.coop_level.is_none() {
                return Ok(Err(DSERR_PRIOLEVELNEEDED));
            }
            if self.primary.is_some() {
                return Err(DsError::Unmodelled("second_primary_buffer".into()));
            }
            let id = self.new_buffer(flags, 0, None, true, None);
            self.primary = Some(id);
            Ok(Ok(id))
        } else {
            if desc.bytes < 4 || desc.bytes > 0x0FFF_FFFF {
                return Ok(Err(DSERR_INVALIDPARAM));
            }
            let Some(format) = desc.format else {
                return Ok(Err(DSERR_INVALIDPARAM));
            };
            if !format.valid() || format.tag != 1 {
                return Ok(Err(DSERR_BADFORMAT));
            }
            if flags & DSBCAPS_CTRL3D != 0 && format.channels != 1 {
                return Ok(Err(DSERR_INVALIDPARAM));
            }
            if !desc.bytes.is_multiple_of(u32::from(format.block_align)) {
                return Ok(Err(DSERR_INVALIDPARAM));
            }
            if let Some(remaining) = self.resource_remaining.as_mut() {
                let required = (u64::from(desc.bytes) + 15) & !15;
                if required > *remaining {
                    return Err(DsError::Unmodelled("d3d_resource_arena_exhausted".into()));
                }
                *remaining -= required;
            }
            Ok(Ok(self.new_buffer(
                flags,
                desc.bytes,
                Some(format),
                false,
                None,
            )))
        }
    }

    pub fn duplicate_sound_buffer(&mut self, source: BufId) -> Result<Result<BufId, u32>, DsError> {
        self.live_device()?;
        let args = json!({"source": self.name(source), "out_given": true});
        let src = self.buf(source).map(|b| {
            (
                b.primary,
                b.flags,
                b.nbytes,
                b.format,
                b.memory.clone(),
                b.volume,
                b.pan,
                b.frequency,
            )
        });
        let (primary, flags, nbytes, format, memory, volume, pan, frequency) = src?;
        let outcome = if primary {
            Err(DSERR_INVALIDPARAM)
        } else {
            let id = self.new_buffer(flags, nbytes, format, false, Some(memory));
            let b = &mut self.buffers[id.0];
            b.volume = volume;
            b.pan = pan;
            b.frequency = frequency;
            Ok(id)
        };
        let (ret, out) = match &outcome {
            Ok(id) => (DS_OK, json!({"object": self.name(*id)})),
            Err(hr) => (*hr, json!({})),
        };
        self.record(
            "IDirectSound.DuplicateSoundBuffer",
            "ds".into(),
            args,
            ret,
            out,
            None,
        );
        Ok(outcome)
    }

    // ------------------------------------------------------------ IDirectSoundBuffer
    pub fn get_caps_buffer(&mut self, id: BufId, size: u32) -> Result<u32, DsError> {
        let (flags, nbytes) = {
            let b = self.buf(id)?;
            (b.flags, b.nbytes)
        };
        let (ret, out) = if size != 20 {
            (DSERR_INVALIDPARAM, json!({}))
        } else {
            (DS_OK, json!({"caps": [20, flags, nbytes, 0, 0]}))
        };
        let this = self.name(id);
        self.record(
            "IDirectSoundBuffer.GetCaps",
            this,
            json!({"size": size}),
            ret,
            out,
            None,
        );
        Ok(ret)
    }

    /// `GetCurrentPosition`; the primary buffer's cursors are not modelled.
    pub fn get_current_position(
        &mut self,
        id: BufId,
        want_play: bool,
        want_write: bool,
    ) -> Result<(u32, Option<(u32, u32)>), DsError> {
        let clock = self.clock_ms;
        let lead_ms = self.profile.write_lead_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        if b.primary {
            return Err(DsError::Unmodelled(
                "primary_buffer_position_unmodelled".into(),
            ));
        }
        let position = Self::position(b, clock);
        let format = b.format.expect("secondary buffers have a format");
        let mut lead = (u64::from(lead_ms) * u64::from(format.avg_bytes_per_sec) / 1000) as u32;
        lead -= lead % u32::from(format.block_align);
        let write = ((u64::from(position) + u64::from(lead)) % u64::from(b.nbytes)) as u32;
        let mut out = Map::new();
        if want_play {
            out.insert("play".into(), position.into());
        }
        if want_write {
            out.insert("write".into(), write.into());
        }
        self.record(
            "IDirectSoundBuffer.GetCurrentPosition",
            this,
            json!({"play_given": want_play, "write_given": want_write}),
            DS_OK,
            Value::Object(out),
            None,
        );
        Ok((DS_OK, Some((position, write))))
    }

    pub fn get_format(
        &mut self,
        id: BufId,
        size_allocated: u32,
    ) -> Result<(u32, Option<WaveFormat>), DsError> {
        let this = self.name(id);
        let format = self.buf(id)?.format;
        let Some(format) = format else {
            return Err(DsError::Unmodelled("primary_format_unset".into()));
        };
        let (ret, out) = if size_allocated < 16 {
            (DSERR_INVALIDPARAM, json!({}))
        } else {
            (DS_OK, json!({"format": format.json(), "written": 16}))
        };
        self.record(
            "IDirectSoundBuffer.GetFormat",
            this,
            json!({"size_allocated": size_allocated, "buffer_given": true, "written_given": true}),
            ret,
            out,
            None,
        );
        Ok((ret, (ret == DS_OK).then_some(format)))
    }

    fn scalar_get(
        &mut self,
        id: BufId,
        call: &str,
        field: &str,
        need: u32,
        read: fn(&Buffer) -> Value,
    ) -> Result<(u32, Option<Value>), DsError> {
        let this = self.name(id);
        let b = self.buf(id)?;
        let (ret, value) = if b.flags & need == 0 {
            (DSERR_CONTROLUNAVAIL, None)
        } else {
            (DS_OK, Some(read(b)))
        };
        let out = match &value {
            Some(v) => json!({ field: v }),
            None => json!({}),
        };
        self.record(call, this, json!({}), ret, out, None);
        Ok((ret, value))
    }

    pub fn get_volume(&mut self, id: BufId) -> Result<(u32, Option<i32>), DsError> {
        let (hr, v) = self.scalar_get(
            id,
            "IDirectSoundBuffer.GetVolume",
            "volume",
            DSBCAPS_CTRLVOLUME,
            |b| json!(b.volume),
        )?;
        Ok((hr, v.and_then(|v| v.as_i64()).map(|v| v as i32)))
    }

    pub fn get_pan(&mut self, id: BufId) -> Result<(u32, Option<i32>), DsError> {
        let (hr, v) = self.scalar_get(
            id,
            "IDirectSoundBuffer.GetPan",
            "pan",
            DSBCAPS_CTRLPAN,
            |b| json!(b.pan),
        )?;
        Ok((hr, v.and_then(|v| v.as_i64()).map(|v| v as i32)))
    }

    pub fn get_frequency(&mut self, id: BufId) -> Result<(u32, Option<u32>), DsError> {
        let (hr, v) = self.scalar_get(
            id,
            "IDirectSoundBuffer.GetFrequency",
            "frequency",
            DSBCAPS_CTRLFREQUENCY,
            |b| json!(b.frequency),
        )?;
        Ok((hr, v.and_then(|v| v.as_u64()).map(|v| v as u32)))
    }

    pub fn get_status(&mut self, id: BufId) -> Result<(u32, u32), DsError> {
        let clock = self.clock_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        if b.primary {
            return Err(DsError::Unmodelled(
                "primary_buffer_position_unmodelled".into(),
            ));
        }
        Self::position(b, clock); // lets a finished non-looping buffer stop
        let status = u32::from(b.playing) | if b.playing && b.looping { 4 } else { 0 };
        self.record(
            "IDirectSoundBuffer.GetStatus",
            this,
            json!({}),
            DS_OK,
            json!({"status": status}),
            None,
        );
        Ok((DS_OK, status))
    }

    pub fn set_volume(&mut self, id: BufId, volume: i32) -> Result<u32, DsError> {
        let this = self.name(id);
        let b = self.buf(id)?;
        let ret = if b.flags & DSBCAPS_CTRLVOLUME == 0 {
            DSERR_CONTROLUNAVAIL
        } else if !(-10000..=0).contains(&volume) {
            DSERR_INVALIDPARAM
        } else {
            b.volume = volume;
            DS_OK
        };
        self.record(
            "IDirectSoundBuffer.SetVolume",
            this,
            json!({"volume": volume}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn set_pan(&mut self, id: BufId, pan: i32) -> Result<u32, DsError> {
        let this = self.name(id);
        let b = self.buf(id)?;
        let ret = if b.flags & DSBCAPS_CTRLPAN == 0 {
            DSERR_CONTROLUNAVAIL
        } else if !(-10000..=10000).contains(&pan) {
            DSERR_INVALIDPARAM
        } else {
            b.pan = pan;
            DS_OK
        };
        self.record(
            "IDirectSoundBuffer.SetPan",
            this,
            json!({"pan": pan}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn set_frequency(&mut self, id: BufId, frequency: u32) -> Result<u32, DsError> {
        let clock = self.clock_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        let mut value = frequency;
        let ret = if b.flags & DSBCAPS_CTRLFREQUENCY == 0 {
            DSERR_CONTROLUNAVAIL
        } else {
            if value == 0 {
                value = b.format.map_or(0, |f| f.rate); // DSBFREQUENCY_ORIGINAL
            }
            if !(100..=200_000).contains(&value) {
                DSERR_INVALIDPARAM
            } else {
                b.base = Self::position(b, clock);
                b.start_ms = clock;
                b.frequency = value;
                DS_OK
            }
        };
        self.record(
            "IDirectSoundBuffer.SetFrequency",
            this,
            json!({"frequency": frequency}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn set_format(&mut self, id: BufId, format: Option<WaveFormat>) -> Result<u32, DsError> {
        let this = self.name(id);
        let coop = self.coop_level;
        let b = self.buf(id)?;
        let ret = if !b.primary {
            DSERR_INVALIDCALL
        } else if !matches!(coop, Some(2) | Some(4)) {
            DSERR_PRIOLEVELNEEDED
        } else {
            match format {
                Some(f) if f.valid() && f.tag == 1 => {
                    b.format = Some(f);
                    b.frequency = f.rate;
                    DS_OK
                }
                _ => DSERR_BADFORMAT,
            }
        };
        self.record(
            "IDirectSoundBuffer.SetFormat",
            this,
            json!({"format": format.map_or(Value::Null, |f| f.json())}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn play(
        &mut self,
        id: BufId,
        reserved: u32,
        priority: u32,
        flags: u32,
    ) -> Result<u32, DsError> {
        let clock = self.clock_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        let ret = if reserved != 0 || flags & !0x1F != 0 {
            DSERR_INVALIDPARAM
        } else if flags & !1 != 0 && !b.primary {
            return Err(DsError::Unmodelled(format!(
                "unsupported_play_flags {flags:#x}"
            )));
        } else if b.primary {
            b.playing = true;
            b.looping = true; // the primary buffer plays continuously
            DS_OK
        } else {
            if !b.playing {
                b.base = Self::position(b, clock);
                b.start_ms = clock;
            }
            b.playing = true;
            b.looping = flags & 1 != 0;
            DS_OK
        };
        self.record(
            "IDirectSoundBuffer.Play",
            this,
            json!({"reserved": reserved, "priority": priority, "flags": flags}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn stop(&mut self, id: BufId) -> Result<u32, DsError> {
        let clock = self.clock_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        if b.playing && !b.primary {
            b.base = Self::position(b, clock);
        }
        b.playing = false;
        self.record(
            "IDirectSoundBuffer.Stop",
            this,
            json!({}),
            DS_OK,
            json!({}),
            None,
        );
        Ok(DS_OK)
    }

    pub fn set_current_position(&mut self, id: BufId, position: u32) -> Result<u32, DsError> {
        let clock = self.clock_ms;
        let this = self.name(id);
        let b = self.buf(id)?;
        let ret = if b.primary {
            DSERR_INVALIDCALL
        } else if position >= b.nbytes
            || !position.is_multiple_of(u32::from(b.format.map_or(1, |f| f.block_align)))
        {
            DSERR_INVALIDPARAM
        } else {
            b.base = position;
            b.start_ms = clock;
            DS_OK
        };
        self.record(
            "IDirectSoundBuffer.SetCurrentPosition",
            this,
            json!({"position": position}),
            ret,
            json!({}),
            None,
        );
        Ok(ret)
    }

    pub fn restore(&mut self, id: BufId) -> Result<u32, DsError> {
        let this = self.name(id);
        self.buf(id)?;
        self.record(
            "IDirectSoundBuffer.Restore",
            this,
            json!({}),
            DS_OK,
            json!({}),
            None,
        );
        Ok(DS_OK) // buffers are never lost in this device
    }

    /// `Lock`. `slots` are whether the caller supplied (ptr1, len1, ptr2, len2) out slots.
    pub fn lock(
        &mut self,
        id: BufId,
        offset: u32,
        bytes: u32,
        flags: u32,
        slots: [bool; 4],
    ) -> Result<Result<Locked, u32>, DsError> {
        let this = self.name(id);
        let b = self.buf(id)?;
        if b.primary {
            return Err(DsError::Unmodelled("primary_buffer_lock_unmodelled".into()));
        }
        let (mut off, mut n) = (offset, bytes);
        let args = json!({"offset": offset, "bytes": bytes, "flags": flags,
                          "ptr1_slot": slots[0], "len1_slot": slots[1], "ptr2_slot": slots[2], "len2_slot": slots[3]});
        let outcome = if flags & !3 != 0 || !slots[0] || !slots[1] {
            Err(DSERR_INVALIDPARAM)
        } else {
            if b.locked.is_some() {
                return Err(DsError::Unmodelled("relock_without_unlock".into()));
            }
            if flags & 1 != 0 {
                return Err(DsError::Unmodelled(
                    "lock_from_write_cursor_unmodelled".into(),
                ));
            }
            if flags & 2 != 0 {
                off = 0;
                n = b.nbytes;
            }
            if off >= b.nbytes || n > b.nbytes || (n == 0 && flags & 2 == 0) {
                Err(DSERR_INVALIDPARAM)
            } else {
                let first = n.min(b.nbytes - off);
                let second = n - first;
                let lock = LockState {
                    offset: off,
                    len1: first,
                    len2: second,
                };
                b.locked = Some(lock);
                Ok(Locked {
                    offset: off,
                    len1: first,
                    len2: second,
                })
            }
        };
        let (ret, out) = match &outcome {
            Ok(l) => (
                DS_OK,
                json!({"len1": l.len1, "len2": if slots[3] { l.len2 } else { 0 }, "ptr1": true, "ptr2": slots[2] && l.len2 != 0}),
            ),
            Err(hr) => (*hr, json!({})),
        };
        self.record("IDirectSoundBuffer.Lock", this, args, ret, out, None);
        Ok(outcome)
    }

    /// `Unlock`, handing back what `Lock` returned. A mismatch is not modelled.
    pub fn unlock(&mut self, id: BufId, back: Option<Locked>) -> Result<u32, DsError> {
        self.unlock_regions(
            id,
            UnlockArgs {
                first_offset: back.map(|v| v.offset),
                len1: back.map_or(0, |v| v.len1),
                second_offset: back.filter(|v| v.len2 != 0).map(|_| 0),
                len2: back.map_or(0, |v| v.len2),
            },
        )
    }

    /// Preserve pointer presence independently of lengths, even for invalid calls.
    pub fn unlock_regions(&mut self, id: BufId, back: UnlockArgs) -> Result<u32, DsError> {
        let this = self.name(id);
        let b = self.buf(id)?;
        let args = json!({"len1":back.len1,"len2":back.len2,"ptr1_given":back.first_offset.is_some(),"ptr2_given":back.second_offset.is_some()});
        let Some(locked) = b.locked else {
            self.record(
                "IDirectSoundBuffer.Unlock",
                this,
                args,
                DSERR_INVALIDPARAM,
                json!({}),
                None,
            );
            return Ok(DSERR_INVALIDPARAM);
        };
        let matches = back.first_offset == Some(locked.offset)
            && back.len1 == locked.len1
            && back.second_offset == (locked.len2 != 0).then_some(0)
            && back.len2 == locked.len2;
        if !matches {
            return Err(DsError::Unmodelled("unlock_arguments_differ".into()));
        }
        b.locked = None;
        let region = {
            let mem = b.memory.borrow();
            let mut bytes =
                mem[locked.offset as usize..(locked.offset + locked.len1) as usize].to_vec();
            if locked.len2 != 0 {
                bytes.extend_from_slice(&mem[..locked.len2 as usize]);
            }
            bytes
        };
        let digest = hex(Sha256::digest(&region).as_slice());
        let args = json!({"len1": locked.len1, "len2": locked.len2, "ptr1_given": true, "ptr2_given": locked.len2 != 0});
        self.record(
            "IDirectSoundBuffer.Unlock",
            this,
            args,
            DS_OK,
            json!({"matches_lock": true}),
            Some(("region_sha256", digest.into())),
        );
        Ok(DS_OK)
    }

    /// Bytes of a buffer's memory, for the caller's writes between `Lock` and `Unlock`
    /// and for an output sink. Not a DirectSound call; records nothing.
    pub fn buffer_memory(&self, id: BufId) -> Memory {
        self.buffers[id.0].memory.clone()
    }

    pub fn query_interface(
        &mut self,
        id: BufId,
        iid: &Iid,
    ) -> Result<(u32, Option<Queried>), DsError> {
        let this = self.name(id);
        let args = json!({"iid": iid.name(), "out_given": true});
        let (primary, flags) = {
            let b = self.buf(id)?;
            (b.primary, b.flags)
        };
        let mut queried = None;
        let (ret, out) = match iid {
            Iid::Listener3d => {
                if !primary || flags & DSBCAPS_CTRL3D == 0 {
                    (DSERR_NOINTERFACE, json!({}))
                } else {
                    match &mut self.listener {
                        None => self.listener = Some((1, false)),
                        Some((rc, _)) => *rc += 1,
                    }
                    queried = Some(Queried::Listener);
                    (DS_OK, json!({"object": "listener"}))
                }
            }
            Iid::Buffer3d => {
                if primary || flags & DSBCAPS_CTRL3D == 0 {
                    (DSERR_NOINTERFACE, json!({}))
                } else {
                    let b = &mut self.buffers[id.0];
                    match &mut b.three_d {
                        None => b.three_d = Some(ThreeDState { refcount: 1 }),
                        Some(t) => t.refcount += 1,
                    }
                    queried = Some(Queried::Buffer3d(id));
                    (DS_OK, json!({"object": format!("3d#{}", id.0)}))
                }
            }
            Iid::Other(_) => return Err(DsError::Unmodelled("unsupported_query_interface".into())),
        };
        self.record(
            "IDirectSoundBuffer.QueryInterface",
            this,
            args,
            ret,
            out,
            None,
        );
        Ok((ret, queried))
    }

    // ------------------------------------------------------------ IUnknown
    pub fn add_ref_buffer(&mut self, id: BufId) -> Result<u32, DsError> {
        let this = self.name(id);
        let b = &mut self.buffers[id.0];
        b.refcount += 1;
        let rc = b.refcount;
        self.record(
            "IDirectSoundBuffer.AddRef",
            this,
            json!({}),
            rc,
            json!({}),
            None,
        );
        Ok(rc)
    }

    pub fn release_buffer(&mut self, id: BufId) -> Result<u32, DsError> {
        let this = self.name(id);
        let b = &mut self.buffers[id.0];
        if b.refcount == 0 {
            return Err(DsError::Unmodelled("com_release_underflow".into()));
        }
        b.refcount -= 1;
        let rc = b.refcount;
        if rc == 0 {
            b.released = true;
        }
        self.record(
            "IDirectSoundBuffer.Release",
            this,
            json!({}),
            rc,
            json!({}),
            None,
        );
        Ok(rc)
    }

    pub fn release_device(&mut self) -> Result<u32, DsError> {
        self.live_device()?;
        if self.ds_refcount == 0 {
            return Err(DsError::Unmodelled("com_release_underflow".into()));
        }
        self.ds_refcount -= 1;
        let rc = self.ds_refcount;
        if rc == 0 {
            self.ds_released = true;
        }
        self.record(
            "IDirectSound.Release",
            "ds".into(),
            json!({}),
            rc,
            json!({}),
            None,
        );
        Ok(rc)
    }

    // ------------------------------------------------------------ observers for sinks and tests
    pub fn buffer_count(&self) -> usize {
        self.buffers.len()
    }

    pub fn is_playing(&mut self, id: BufId) -> bool {
        let clock = self.clock_ms;
        let b = &mut self.buffers[id.0];
        Self::position(b, clock);
        b.playing
    }
}

/// `f32:0x...` helper re-exported for callers building 3D arguments.
pub fn float_word(value: f32) -> String {
    f32_word(value.to_bits())
}

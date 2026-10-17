//! The call-log record schema (M0-10).
//!
//! Seven record kinds; see `docs/call-log.md` for the byte-level layout and
//! the rationale for every exact field type.
//!
//! # Extension points
//!
//! New record kinds claim the next free tag (`0x07`, `0x08`, ...); new value
//! tags claim the next free value tag (`0x07`, ...); new file ops claim the
//! next free file-op byte. Readers from this crate MUST reject unknown tags
//! loudly ([`LogError`](crate::error::LogError)) rather than skipping them,
//! so an unreadable log can never pass a diff.

use crate::error::LogError;

/// File magic, the first six bytes of every log: ASCII `SLRLOG`.
pub const MAGIC: &[u8; 6] = b"SLRLOG";

/// The only format version this crate reads and writes.
pub const FORMAT_VERSION: u16 = 1;

/// Hard cap on the argument count of one [`LogRecord::NativeCall`].
///
/// The hook layer (M0-11) serializes a TUFA call frame; frames wider than 64
/// slots indicate a corrupt or hostile log, never a real native.
pub const MAX_ARGS: usize = 64;

/// Hard cap on one serialized record, in bytes (16 MiB).
///
/// Bounds a single allocation against corrupt length prefixes. Real records
/// are small: a 64-argument call of 4 KiB strings is still well under 1 MiB.
pub const MAX_RECORD_BYTES: usize = 16 << 20;

/// The value tags below `VALUE_TAG_MAX`, inclusive, are assigned.
pub const VALUE_TAG_MAX: u8 = 0x06;

/// Tag bytes reserved for future value types. Gaps are intentional: `0x07`
/// is the first free value tag.
const _VALUE_TAG_NEXT_FREE: u8 = 0x07;

/// A TUFA-level argument or result value.
///
/// Floats are stored as their exact bit patterns (`f32::to_bits` /
/// `f64::to_bits`), never as decimal text, so a log round-trips bit-exactly
/// and two emitters can be compared byte-for-byte. `Str` is UTF-8; arbitrary
/// game bytes cross the boundary as raw Latin-1 inside an 8-bit-clean host
/// string, which the hook layer records as `Bytes`.
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    /// 32-bit signed integer (TUFA `int`, handles, enum ordinals).
    Int(i32),
    /// 64-bit signed integer (file sizes, tick counts).
    Long(i64),
    /// 32-bit float by exact bit pattern (most TUFA `float` traffic).
    FloatBits(u32),
    /// 64-bit float by exact bit pattern (timers, physics doubles).
    DoubleBits(u64),
    /// UTF-8 string (class names, method names, paths, text args).
    Str(String),
    /// Raw byte vector (buffers, input reports, snapshot-adjacent blobs).
    Bytes(Vec<u8>),
    /// Boolean (flags, key states, success predicates).
    Bool(bool),
}

impl Value {
    /// The on-wire tag byte of this value. See `docs/call-log.md` §3.
    #[must_use]
    pub fn tag(&self) -> u8 {
        match self {
            Value::Int(_) => 0x00,
            Value::Long(_) => 0x01,
            Value::FloatBits(_) => 0x02,
            Value::DoubleBits(_) => 0x03,
            Value::Str(_) => 0x04,
            Value::Bytes(_) => 0x05,
            Value::Bool(_) => 0x06,
        }
    }

    /// Convenience: wrap an `f32` without losing a single bit.
    #[must_use]
    pub fn float(v: f32) -> Self {
        Value::FloatBits(v.to_bits())
    }

    /// Convenience: unwrap the exact `f32` stored by [`Value::float`].
    #[must_use]
    pub fn as_float(&self) -> Option<f32> {
        if let Value::FloatBits(b) = self {
            Some(f32::from_bits(*b))
        } else {
            None
        }
    }

    /// Convenience: wrap an `f64` without losing a single bit.
    #[must_use]
    pub fn double(v: f64) -> Self {
        Value::DoubleBits(v.to_bits())
    }

    /// Convenience: unwrap the exact `f64` stored by [`Value::double`].
    #[must_use]
    pub fn as_double(&self) -> Option<f64> {
        if let Value::DoubleBits(b) = self {
            Some(f64::from_bits(*b))
        } else {
            None
        }
    }
}

/// A D3D9 entry-point family recorded by the hook layer.
///
/// This is a coarse family tag, not the full D3D9 method table: the method
/// name (e.g. `DrawIndexedPrimitive`) travels in `method`, the arguments in
/// `args`. The tag lets the diff tool and headless backend (M1-12) route
/// without parsing strings. Next free tag: `0x05`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum D3d9CallKind {
    /// Device state (`SetRenderState`, `SetTextureStageState`, ...).
    DeviceState = 0x00,
    /// Draw submission (`DrawPrimitive`, `DrawIndexedPrimitive`, ...).
    Draw = 0x01,
    /// Resource management (`CreateTexture`, `LockRect`, `Release`, ...).
    Resource = 0x02,
    /// Vertex declaration / stream source / shader binding.
    VertexFormat = 0x03,
    /// Presentation (`Present`, `Clear`, `BeginScene`/`EndScene`).
    Present = 0x04,
}

/// All assigned [`D3d9CallKind`] tags, for exhaustive tests.
pub const D3D9_CALL_KINDS: [D3d9CallKind; 5] = [
    D3d9CallKind::DeviceState,
    D3d9CallKind::Draw,
    D3d9CallKind::Resource,
    D3d9CallKind::VertexFormat,
    D3d9CallKind::Present,
];

/// Highest assigned [`D3d9CallKind`] tag.
pub const D3D9_CALL_KIND_MAX: u8 = 0x04;

impl TryFrom<u8> for D3d9CallKind {
    type Error = u8;
    fn try_from(v: u8) -> std::result::Result<Self, u8> {
        match v {
            0x00 => Ok(D3d9CallKind::DeviceState),
            0x01 => Ok(D3d9CallKind::Draw),
            0x02 => Ok(D3d9CallKind::Resource),
            0x03 => Ok(D3d9CallKind::VertexFormat),
            0x04 => Ok(D3d9CallKind::Present),
            other => Err(other),
        }
    }
}

/// A DirectSound entry-point family. Next free tag: `0x03`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DirectSoundKind {
    /// Buffer control (`Play`, `Stop`, `SetVolume`, `SetFrequency`, ...).
    Buffer = 0x00,
    /// 3D positioning (`SetPosition`, `SetVelocity`, listener updates).
    Spatial = 0x01,
    /// Capture / enumeration (`DirectSoundCaptureCreate`, device scan).
    Capture = 0x02,
}

/// All assigned [`DirectSoundKind`] tags, for exhaustive tests.
pub const DIRECT_SOUND_KINDS: [DirectSoundKind; 3] = [
    DirectSoundKind::Buffer,
    DirectSoundKind::Spatial,
    DirectSoundKind::Capture,
];

/// Highest assigned [`DirectSoundKind`] tag.
pub const DIRECT_SOUND_KIND_MAX: u8 = 0x02;

impl TryFrom<u8> for DirectSoundKind {
    type Error = u8;
    fn try_from(v: u8) -> std::result::Result<Self, u8> {
        match v {
            0x00 => Ok(DirectSoundKind::Buffer),
            0x01 => Ok(DirectSoundKind::Spatial),
            0x02 => Ok(DirectSoundKind::Capture),
            other => Err(other),
        }
    }
}

/// A DirectInput entry-point family. Next free tag: `0x03`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum DirectInputKind {
    /// Keyboard state (`GetDeviceState`, buffered reads).
    Keyboard = 0x00,
    /// Mouse state (position deltas, buttons).
    Mouse = 0x01,
    /// Gamepad / wheel (`joyGetPosEx`, force-feedback commands).
    Gamepad = 0x02,
}

/// All assigned [`DirectInputKind`] tags, for exhaustive tests.
pub const DIRECT_INPUT_KINDS: [DirectInputKind; 3] = [
    DirectInputKind::Keyboard,
    DirectInputKind::Mouse,
    DirectInputKind::Gamepad,
];

/// Highest assigned [`DirectInputKind`] tag.
pub const DIRECT_INPUT_KIND_MAX: u8 = 0x02;

impl TryFrom<u8> for DirectInputKind {
    type Error = u8;
    fn try_from(v: u8) -> std::result::Result<Self, u8> {
        match v {
            0x00 => Ok(DirectInputKind::Keyboard),
            0x01 => Ok(DirectInputKind::Mouse),
            0x02 => Ok(DirectInputKind::Gamepad),
            other => Err(other),
        }
    }
}

/// A file-I/O operation family. Next free op byte: `0x05`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum FileOpKind {
    /// `CreateFile` / `fopen`: `path` is the requested path.
    Open = 0x00,
    /// `ReadFile` / `fread`: `path` identifies the handle's file.
    Read = 0x01,
    /// `WriteFile` / `fwrite`: `path` identifies the handle's file.
    Write = 0x02,
    /// `CloseHandle` / `fclose` / `FindClose`.
    Close = 0x03,
    /// `FindFirstFile` / `FindNextFile` enumeration hits.
    Find = 0x04,
}

impl TryFrom<u8> for FileOpKind {
    type Error = u8;
    fn try_from(v: u8) -> std::result::Result<Self, u8> {
        match v {
            0x00 => Ok(FileOpKind::Open),
            0x01 => Ok(FileOpKind::Read),
            0x02 => Ok(FileOpKind::Write),
            0x03 => Ok(FileOpKind::Close),
            0x04 => Ok(FileOpKind::Find),
            other => Err(other),
        }
    }
}

/// A whole-VM state snapshot: register file, float control word, and value stack.
///
/// The hook layer (M0-11) emits these at deterministic replay boundaries
/// (scripted-input injection points); the headless VM (M1) emits them at the
/// same points so the diff tool can bisect a divergence to a single frame.
/// `stack` holds the raw TUFA stack slots; floats inside it are bit patterns
/// packed as [`Value::FloatBits`] / [`Value::DoubleBits`].
#[derive(Debug, Clone, PartialEq)]
pub struct VmSnapshot {
    /// Monotonic frame counter shared with [`LogRecord::FrameVehicle`].
    pub frame: u64,
    /// Program counter / bytecode offset at the snapshot point.
    pub pc: u32,
    /// x87/SSE control word (exact bits).
    pub control_word: u32,
    /// Raw TUFA value-stack slots, bottom first.
    pub stack: Vec<Value>,
}

/// Per-frame vehicle state, one record per simulated frame.
///
/// Positions are metres in cfg axes (x right, y up, z backwards), orientation
/// is yaw/pitch/roll radians, speeds are m/s. All floats are exact bit
/// patterns: physics comparisons are 0-ulp, so decimal text would be lossy.
#[derive(Debug, Clone, PartialEq)]
pub struct FrameVehicleState {
    /// Monotonic frame counter shared with [`VmSnapshot::frame`].
    pub frame: u64,
    /// World position (x, y, z) in metres.
    pub pos: [u32; 3],
    /// Orientation (yaw, pitch, roll) in radians, as f32 bits.
    pub ypr: [u32; 3],
    /// Linear velocity (vx, vy, vz) in m/s, as f32 bits.
    pub vel: [u32; 3],
    /// Engine RPM, as f32 bits.
    pub rpm_bits: u32,
    /// Selected gear (signed: -1 reverse, 0 neutral).
    pub gear: i32,
}

impl FrameVehicleState {
    /// Pack raw floats into the bit-exact record form.
    #[must_use]
    pub fn from_floats(
        frame: u64,
        pos: [f32; 3],
        ypr: [f32; 3],
        vel: [f32; 3],
        rpm: f32,
        gear: i32,
    ) -> Self {
        Self {
            frame,
            pos: pos.map(f32::to_bits),
            ypr: ypr.map(f32::to_bits),
            vel: vel.map(f32::to_bits),
            rpm_bits: rpm.to_bits(),
            gear,
        }
    }
}

/// One record of a call log.
///
/// Tag bytes `0x00`..=`0x06`; any other tag is a hard [`LogError`], never a
/// skip. See `docs/call-log.md` §2 for the layout of each variant.
#[derive(Debug, Clone, PartialEq)]
pub enum LogRecord {
    /// A native method call: declaring class, method name, arguments, result.
    ///
    /// `result` is `None` for `void` natives; `Some` otherwise.
    NativeCall {
        class: String,
        name: String,
        args: Vec<Value>,
        result: Option<Value>,
    },
    /// A D3D9 entry-point call, routed by coarse family in `kind`.
    D3d9Call {
        kind: D3d9CallKind,
        method: String,
        args: Vec<Value>,
    },
    /// A DirectSound entry-point call.
    DirectSoundCall {
        kind: DirectSoundKind,
        method: String,
        args: Vec<Value>,
    },
    /// A DirectInput poll: device family plus the raw report bytes.
    DirectInput {
        kind: DirectInputKind,
        report: Vec<u8>,
    },
    /// A file-I/O operation on `path` with an operation-specific payload.
    FileIo {
        op: FileOpKind,
        path: String,
        detail: Vec<Value>,
    },
    /// A whole-VM state snapshot at a replay boundary.
    VmSnapshot(VmSnapshot),
    /// Per-frame vehicle state for one simulated frame.
    FrameVehicle(FrameVehicleState),
}

impl LogRecord {
    /// The on-wire record tag byte. See `docs/call-log.md` §2.
    #[must_use]
    pub fn tag(&self) -> u8 {
        match self {
            LogRecord::NativeCall { .. } => 0x00,
            LogRecord::D3d9Call { .. } => 0x01,
            LogRecord::DirectSoundCall { .. } => 0x02,
            LogRecord::DirectInput { .. } => 0x03,
            LogRecord::FileIo { .. } => 0x04,
            LogRecord::VmSnapshot(_) => 0x05,
            LogRecord::FrameVehicle(_) => 0x06,
        }
    }

    /// The `snake_case` kind name used in diff reports and text dumps.
    #[must_use]
    pub fn kind_name(&self) -> &'static str {
        match self {
            LogRecord::NativeCall { .. } => "native_call",
            LogRecord::D3d9Call { .. } => "d3d9_call",
            LogRecord::DirectSoundCall { .. } => "direct_sound_call",
            LogRecord::DirectInput { .. } => "direct_input",
            LogRecord::FileIo { .. } => "file_io",
            LogRecord::VmSnapshot(_) => "vm_snapshot",
            LogRecord::FrameVehicle(_) => "frame_vehicle",
        }
    }

    /// Decode a record tag byte, enforcing fail-loud on unknown kinds.
    pub(crate) fn check_tag(tag: u8, at: usize) -> crate::error::Result<()> {
        if tag <= 0x06 {
            Ok(())
        } else {
            Err(LogError::UnknownRecordKind { tag, at })
        }
    }
}

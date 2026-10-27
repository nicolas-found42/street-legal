//! The exe's DirectInput device layer (M6-04): keyboard, mouse and gamepad
//! enumeration, per-frame polling, key/axis queries and shutdown.
//!
//! Ported function by function from the original block `0x556150..0x558130`
//! (`docs/hook-layer.md` names the call-log kinds; the Unicorn oracle and
//! scenario vectors are `re/oracle/input_device_rig.py` and
//! `input_device_vectors.py`). The device table lives in a byte image that
//! keeps the exe's own layout: sixteen `0x7a8`-byte device records from
//! `0x76f9b0`, the globals around them, and the controller words at
//! `0x7686e4`. Keeping the layout lets a differential test compare every
//! word the original wrote, not only return values.
//!
//! | exe | function | here |
//! |---|---|---|
//! | `0x556150` | init: `DirectInput8Create`, keyboard, mouse, `EnumDevices` | [`DeviceLayer::init`] |
//! | `0x5565a0` | `EnumDevices` callback (gamepad) | [`DeviceLayer::enum_devices_callback`] |
//! | `0x556710` | `EnumObjects` callback | [`DeviceLayer::enum_objects_callback`] |
//! | `0x556550` | append one named object to a record | `add_object` |
//! | `0x556bc0` | poll every device, accumulate mouse axes | [`DeviceLayer::poll`] |
//! | `0x556e00` | last buffered key with its character | [`DeviceLayer::last_key`] |
//! | `0x556ec0` / `0x557430` | digital / analog object value | [`DeviceLayer::is_down`] / [`DeviceLayer::axis_value`] |
//! | `0x557990` | is the object an axis | [`DeviceLayer::is_axis`] |
//! | `0x5579f0` | object name | [`DeviceLayer::axis_name`] |
//! | `0x557aa0` | the single active object | [`DeviceLayer::active_axis`] |
//! | `0x558100` / `0x558130` | device name / kind | [`DeviceLayer::device_name`] / [`DeviceLayer::device_kind`] |
//! | `0x556b10` | shutdown | [`DeviceLayer::shutdown`] |
//!
//! # Arithmetic
//!
//! The decompiled code computes in x87 registers. The game runs with the
//! D3D9 control word (24-bit significand, `0x7f`), so every `+ - * / sqrt`
//! rounds to `f32`; this port therefore uses `f32` operations in the
//! original's order, and the integer divisions that start from 64-bit
//! counters are rounded exactly ([`div_round_f32`]). `fsin`/`fcos` evaluate
//! in `f64` and are rounded to `f32` where the original rounds; the x87
//! transcendentals stay APPROX (`docs/x87.md`, ADR 0007).
//!
//! # Divergences
//!
//! - Force feedback is out of scope (ADR 0009): `FUN_00555d40` and the
//!   effect objects are not ported, so [`DeviceLayer::new`] refuses a
//!   non-zero `ForceFeedBack` setting with a joystick present, and effect
//!   slots stay null.
//! - `DirectInput8Create` failing reaches the exe's fatal-error path
//!   (`FUN_00558510`: log file, `MessageBoxA`, exit); the port reports
//!   [`DeviceError::DirectInputCreate`] instead.
//! - Indices outside the modelled globals/table (the exe would read other
//!   static data) stop with a panic naming the address.

use crate::key_names::KEY_NAMES;

/// An opaque COM object handle: the IDirectInput8 object is `0x1000`, devices
/// follow in creation order (the same normalisation the oracle vectors use).
pub type Handle = u32;

/// First modelled address (controller globals).
pub const BASE: u32 = 0x0076_86e4;
/// One past the last modelled address.
pub const END: u32 = 0x0077_7458;
/// `DAT_0076f9b0`: device record 0.
pub const TABLE: u32 = 0x0076_f9b0;
/// Size of one device record.
pub const RECORD: u32 = 0x7a8;
/// Device table capacity.
pub const MAX_DEVICES: u32 = 16;

const G_DT: u32 = 0x0076_f9a0;
const G_QPF: u32 = 0x0076_f9a8;
const G_LAYOUT: u32 = 0x0077_7430;
const G_COUNT: u32 = 0x0077_7434;
const G_SCALE: u32 = 0x0077_7438;
const G_DI: u32 = 0x0077_743c;
const G_QPC: u32 = 0x0077_7440;
const G_FORCE_FEEDBACK: u32 = 0x0077_7448;

// Offsets inside a device record.
const R_DEVICE: u32 = 0x00;
const R_CAPS: u32 = 0x04;
const R_KIND: u32 = 0x30;
const R_BUTTONS: u32 = 0x34;
const R_OBJECTS: u32 = 0x38;
const R_POVS: u32 = 0x3c;
const R_SLIDERS: u32 = 0x40;
const R_ACCUM: u32 = 0x44;
const R_IDS: u32 = 0x144;
const R_NAMES: u32 = 0x244;
const R_STATE: u32 = 0x644;
const R_NAME: u32 = 0x754;
const R_ACTUATORS: u32 = 0x794;
const R_EFFECTS: [u32; 3] = [0x798, 0x79c, 0x7a0];

/// `DAT_0076f9e0` values.
const KIND_KEYBOARD: u32 = 1;
const KIND_MOUSE: u32 = 2;
const KIND_JOYSTICK: u32 = 3;

const DI_VERSION: u32 = 0x800;
const GUID_KEYBOARD: [u8; 16] = [
    0x61, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];
const GUID_MOUSE: [u8; 16] = [
    0x60, 0x2b, 0x1d, 0x6f, 0xa0, 0xd5, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];

/// The static `DIDATAFORMAT` addresses the exe passes to `SetDataFormat`.
pub const FORMAT_KEYBOARD: u32 = 0x0060_310c;
/// `c_dfDIMouse`.
pub const FORMAT_MOUSE: u32 = 0x0060_2f04;
/// `c_dfDIJoystick2`.
pub const FORMAT_JOYSTICK: u32 = 0x0060_3314;
/// `{size, objsize, flags, datasize, numobjs}` of the three formats.
pub const FORMAT_KEYBOARD_WORDS: [u32; 5] = [24, 16, 2, 256, 256];
/// See [`FORMAT_MOUSE`].
pub const FORMAT_MOUSE_WORDS: [u32; 5] = [24, 16, 2, 16, 7];
/// See [`FORMAT_JOYSTICK`].
pub const FORMAT_JOYSTICK_WORDS: [u32; 5] = [24, 16, 1, 272, 164];

/// Object GUIDs of `EnumObjects`, first dword (`GUID_XAxis`... `GUID_POV`).
const OBJ_X: u32 = 0xa36d_02e0;
const OBJ_Y: u32 = 0xa36d_02e1;
const OBJ_Z: u32 = 0xa36d_02e2;
const OBJ_RZ: u32 = 0xa36d_02e3;
const OBJ_SLIDER: u32 = 0xa36d_02e4;
const OBJ_RX: u32 = 0xa36d_02f4;
const OBJ_RY: u32 = 0xa36d_02f5;
const OBJ_BUTTON: u32 = 0xa36d_02f0;
const OBJ_POV: u32 = 0xa36d_02f2;
/// The remaining twelve bytes shared by every `GUID_*` object type.
const OBJ_GUID_TAIL: [u8; 12] = [
    0xf3, 0xc9, 0xcf, 0x11, 0xbf, 0xc7, 0x44, 0x45, 0x53, 0x54, 0x00, 0x00,
];

const ONE: f32 = 1.0;
const NEG_ONE: f32 = -1.0;
/// `_DAT_005f08e8`: the digital threshold.
const DIGITAL: f32 = f32::from_bits(0x3dcc_cccd);
/// `_DAT_005f0910`: milliseconds per second.
const MS: f32 = 1000.0;
/// `_DAT_005f3ac8`.
const MOUSE_STEP: f32 = f32::from_bits(0x3c80_0000);
/// `_DAT_005f1118`.
const SCALE_STEP: f32 = f32::from_bits(0x3c23_d70a);
/// `_DAT_005f150c`.
const MOUSE_VELOCITY: f32 = f32::from_bits(0x3ba3_d70a);
/// `_DAT_005f0dbc`: `1 / 32768`.
const AXIS_SCALE: f32 = f32::from_bits(0x3800_0000);
/// `_DAT_005f3acc`: degrees-in-hundredths to radians.
const POV_RADIANS: f32 = f32::from_bits(0x3937_0172);
/// `_DAT_005f0c80`.
const ACTIVE_HIGH: f32 = 0.25;
/// `_DAT_005f3ad0`.
const ACTIVE_LOW: f32 = -0.25;
/// `_DAT_005f0c60`.
const ACTIVE_MIN: f32 = -0.75;

/// Why a layer function could not run.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceError {
    /// `DirectInput8Create` failed: the exe's fatal-error path.
    DirectInputCreate,
    /// A path the port does not model (force feedback).
    Unsupported(&'static str),
}

/// One `DIDEVICEINSTANCE` handed to the `EnumDevices` callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DeviceInstance {
    /// `guidInstance`.
    pub instance_guid: [u8; 16],
    /// `dwDevType`.
    pub devtype: u32,
    /// `tszInstanceName`.
    pub name: Vec<u8>,
}

/// One `DIDEVICEOBJECTINSTANCE` handed to the `EnumObjects` callback.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ObjectInstance {
    /// `guidType`.
    pub guid: [u8; 16],
    /// `dwType`.
    pub object_type: u32,
    /// `dwFlags`.
    pub flags: u32,
    /// `tszName`.
    pub name: Vec<u8>,
}

/// The DirectInput / input-related Win32 calls the layer makes. Every
/// implementation records the call; [`crate::scripted::ScriptedPlatform`]
/// answers from a declared scenario.
pub trait Platform {
    /// `DirectInput8Create(hinst, 0x800, IID_IDirectInput8A, &di, outer)`; `Err(hr)` on failure.
    fn direct_input_create(&mut self, version: u32, outer: u32) -> Result<Handle, u32>;
    /// `GetKeyboardLayout(thread)`.
    fn get_keyboard_layout(&mut self, thread: u32) -> u32;
    /// `QueryPerformanceFrequency`.
    fn query_performance_frequency(&mut self) -> u64;
    /// `QueryPerformanceCounter`.
    fn query_performance_counter(&mut self) -> u64;
    /// `timeGetTime`.
    fn time_get_time(&mut self) -> u32;
    /// `MapVirtualKeyExA(code, kind, layout)`.
    fn map_virtual_key_ex(&mut self, code: u32, kind: u32, layout: u32) -> u32;
    /// `GetKeyboardState`; `None` when it fails.
    fn get_keyboard_state(&mut self) -> Option<[u8; 256]>;
    /// `ToAsciiEx(vk, scan, state, &out, flags, layout)`: `(count, out word)`.
    fn to_ascii_ex(&mut self, vk: u32, scan: u32, flags: u32, layout: u32) -> (u32, u16);

    /// `IDirectInput8A::Release`.
    fn di_release(&mut self, di: Handle);
    /// `IDirectInput8A::CreateDevice`: `(hr, device)`; the handle is only meaningful when `hr == 0`.
    fn di_create_device(&mut self, di: Handle, guid: [u8; 16], outer: u32) -> (u32, Handle);
    /// `IDirectInput8A::EnumDevices`: the instances the callback will see.
    fn di_enum_devices(
        &mut self,
        di: Handle,
        devtype: u32,
        callback: u32,
        reference: u32,
        flags: u32,
    ) -> Vec<DeviceInstance>;
    /// A device instance is about to be handed to the callback.
    fn di_enum_callback_enter(&mut self, instance: &DeviceInstance);
    /// The `EnumDevices` callback returned `eax`.
    fn di_enum_callback_result(&mut self, eax: u32);

    /// `IDirectInputDevice8A::Release`.
    fn dev_release(&mut self, dev: Handle) -> u32;
    /// `GetCapabilities` with `dwSize == size`: `(hr, words)`.
    fn dev_get_capabilities(&mut self, dev: Handle, size: u32) -> (u32, Vec<u32>);
    /// `EnumObjects(callback, record reference, flags)`: the matching objects.
    fn dev_enum_objects(
        &mut self,
        dev: Handle,
        callback: u32,
        record: u32,
        flags: u32,
    ) -> Vec<ObjectInstance>;
    /// An object is about to be handed to the callback.
    fn dev_enum_callback_enter(&mut self, dev: Handle, object: &ObjectInstance);
    /// The `EnumObjects` callback returned `eax`.
    fn dev_enum_callback_result(&mut self, dev: Handle, eax: u32);
    /// `SetProperty(prop, header words)`.
    fn dev_set_property(&mut self, dev: Handle, prop: u32, header: [u32; 5]) -> u32;
    /// `Acquire`.
    fn dev_acquire(&mut self, dev: Handle) -> u32;
    /// `Unacquire`.
    fn dev_unacquire(&mut self, dev: Handle) -> u32;
    /// `Poll`.
    fn dev_poll(&mut self, dev: Handle) -> u32;
    /// `GetDeviceState(size)`: `(hr, report)`; the report is meaningful when `hr == 0`.
    fn dev_get_device_state(&mut self, dev: Handle, size: u32) -> (u32, Vec<u8>);
    /// `GetDeviceData(cb, count in, flags)`: `(hr, events (offset, data))`.
    fn dev_get_device_data(
        &mut self,
        dev: Handle,
        cb: u32,
        count: u32,
        flags: u32,
    ) -> (u32, Vec<(u32, u32)>);
    /// `SetDataFormat(format address, format words)`.
    fn dev_set_data_format(&mut self, dev: Handle, format: u32, words: [u32; 5]) -> u32;
    /// `SetCooperativeLevel`.
    fn dev_set_cooperative_level(&mut self, dev: Handle, hwnd: u32, flags: u32) -> u32;
}

/// The modelled memory of the exe's input state, byte-addressed like the exe.
#[derive(Clone, Debug)]
pub struct Image {
    mem: Vec<u8>,
}

impl Image {
    fn new() -> Self {
        Image {
            mem: vec![0; (END - BASE) as usize],
        }
    }

    fn off(&self, address: u32, len: u32) -> usize {
        assert!(
            address >= BASE && address + len <= END,
            "input state access outside the modelled image: {address:#x}+{len}"
        );
        (address - BASE) as usize
    }

    /// Read a little-endian word.
    #[must_use]
    pub fn rd32(&self, address: u32) -> u32 {
        let o = self.off(address, 4);
        u32::from_le_bytes(self.mem[o..o + 4].try_into().unwrap())
    }

    /// Write a little-endian word.
    pub fn wr32(&mut self, address: u32, value: u32) {
        let o = self.off(address, 4);
        self.mem[o..o + 4].copy_from_slice(&value.to_le_bytes());
    }

    fn rd8(&self, address: u32) -> u8 {
        self.mem[self.off(address, 1)]
    }

    fn rd_f32(&self, address: u32) -> f32 {
        f32::from_bits(self.rd32(address))
    }

    fn wr_f32(&mut self, address: u32, value: f32) {
        self.wr32(address, value.to_bits());
    }

    fn rd64(&self, address: u32) -> u64 {
        u64::from(self.rd32(address)) | (u64::from(self.rd32(address + 4)) << 32)
    }

    fn wr64(&mut self, address: u32, value: u64) {
        self.wr32(address, value as u32);
        self.wr32(address + 4, (value >> 32) as u32);
    }

    fn write_bytes(&mut self, address: u32, bytes: &[u8]) {
        let o = self.off(address, bytes.len() as u32);
        self.mem[o..o + bytes.len()].copy_from_slice(bytes);
    }

    fn clear(&mut self, address: u32, len: u32) {
        let o = self.off(address, len);
        self.mem[o..o + len as usize].fill(0);
    }

    /// `(address, word)` for every word of the modelled regions that
    /// differs from `previous`, in address order.
    #[must_use]
    pub fn diff(&self, previous: &Image) -> Vec<(u32, u32)> {
        let mut out = Vec::new();
        for (start, end) in REGIONS {
            for address in (start..end).step_by(4) {
                let word = self.rd32(address);
                if word != previous.rd32(address) {
                    out.push((address, word));
                }
            }
        }
        out
    }
}

/// The word regions a differential comparison covers.
pub const REGIONS: [(u32, u32); 4] = [
    (0x0076_86e4, 0x0076_86fc),
    (0x0076_f9a0, 0x0076_f9b0),
    (TABLE, TABLE + RECORD * MAX_DEVICES),
    (0x0077_7430, 0x0077_7458),
];

/// The device table, its globals and the layer's operations.
#[derive(Clone, Debug)]
pub struct DeviceLayer {
    /// The exe-layout memory image.
    pub image: Image,
    hwnd: u32,
}

impl DeviceLayer {
    /// A zeroed layer. `hwnd` is `DAT_006495a0`, the game window every
    /// `SetCooperativeLevel` receives; `force_feedback` is the `ForceFeedBack`
    /// configuration word `DAT_00777448`.
    #[must_use]
    pub fn new(hwnd: u32, force_feedback: u32) -> Self {
        let mut image = Image::new();
        image.wr32(G_FORCE_FEEDBACK, force_feedback);
        DeviceLayer { image, hwnd }
    }

    fn rec(device: u32) -> u32 {
        TABLE + device.wrapping_mul(RECORD)
    }

    fn count(&self) -> u32 {
        self.image.rd32(G_COUNT)
    }

    fn count_signed(&self) -> i32 {
        self.count() as i32
    }

    fn strncpy_record_name(&mut self, rec: u32, name: &[u8]) {
        // `_strncpy(rec + 0x754, name, 0x40)`: copy, then zero-pad to 0x40.
        let mut field = [0u8; 0x40];
        let n = name.len().min(0x40);
        field[..n].copy_from_slice(&name[..n]);
        self.image.write_bytes(rec + R_NAME, &field);
    }

    /// exe: FUN_00556550
    /// `FUN_00556550` and the inline copies of `FUN_00556150`: append `name`
    /// with object id `id` to `rec`.
    fn add_object(&mut self, rec: u32, name: &[u8], id: u32) {
        let slot = self.image.rd32(rec + R_OBJECTS);
        let mut text = name.to_vec();
        text.push(0);
        self.image.write_bytes(rec + R_NAMES + slot * 16, &text);
        self.image.wr32(rec + R_IDS + slot * 4, id);
        self.image.wr32(rec + R_OBJECTS, slot + 1);
    }

    /// exe: FUN_00556150
    /// `FUN_00556150`.
    ///
    /// # Errors
    /// [`DeviceError::DirectInputCreate`] when `DirectInput8Create` fails,
    /// [`DeviceError::Unsupported`] for force feedback.
    pub fn init(&mut self, p: &mut dyn Platform) -> Result<u32, DeviceError> {
        self.image.wr32(G_DI, 0);
        let di = p
            .direct_input_create(DI_VERSION, 0)
            .map_err(|_| DeviceError::DirectInputCreate)?;
        self.image.wr32(G_DI, di);
        self.image.wr32(G_COUNT, 0);
        self.image.clear(TABLE, RECORD);

        // Keyboard: record 0.
        let rec = TABLE;
        let (hr, dev) = p.di_create_device(di, GUID_KEYBOARD, 0);
        if hr == 0 {
            self.image.wr32(rec + R_DEVICE, dev);
            if p.dev_set_data_format(dev, FORMAT_KEYBOARD, FORMAT_KEYBOARD_WORDS) != 0
                || p.dev_set_cooperative_level(dev, self.hwnd, 6) != 0
            {
                p.dev_release(dev);
            } else {
                self.image.wr32(rec + R_CAPS, 0x2c);
                let (hr, words) = p.dev_get_capabilities(dev, 0x2c);
                if hr == 0 {
                    self.store_caps(rec, &words);
                    p.dev_set_property(dev, 1, [0x14, 0x10, 0, 0, 0x10]);
                    if self.image.rd32(rec + R_DEVICE) != 0 {
                        p.dev_acquire(dev);
                    }
                    self.strncpy_record_name(rec, b"SysKeyboard");
                    let count = self.count();
                    self.image.wr32(rec + R_KIND, KIND_KEYBOARD);
                    self.image.wr32(rec + R_OBJECTS, 0x100);
                    self.image.wr32(G_COUNT, count + 1);
                }
            }
        }

        // Mouse: the next free record.
        let rec = Self::rec(self.count());
        self.image.clear(rec, RECORD);
        let (hr, dev) = p.di_create_device(di, GUID_MOUSE, 0);
        if hr == 0 {
            self.image.wr32(rec + R_DEVICE, dev);
            if p.dev_set_data_format(dev, FORMAT_MOUSE, FORMAT_MOUSE_WORDS) != 0
                || p.dev_set_cooperative_level(dev, self.hwnd, 6) != 0
            {
                p.dev_release(dev);
            } else {
                self.image.wr32(rec + R_CAPS, 0x2c);
                let (hr, words) = p.dev_get_capabilities(dev, 0x2c);
                if hr == 0 {
                    self.store_caps(rec, &words);
                    self.mouse_objects(rec);
                    let count = self.count();
                    self.image.wr32(G_COUNT, count + 1);
                }
            }
        }

        let instances = p.di_enum_devices(di, 4, 0x0055_65a0, 0, 1);
        for instance in &instances {
            p.di_enum_callback_enter(instance);
            let eax = self.enum_devices_callback(p, instance);
            p.di_enum_callback_result(eax);
            if eax == 0 {
                break;
            }
        }

        if self.image.rd32(G_FORCE_FEEDBACK) != 0 && self.count() > 0 {
            for index in 0..self.count() {
                if self.image.rd32(Self::rec(index) + R_KIND) == KIND_JOYSTICK {
                    return Err(DeviceError::Unsupported("force feedback"));
                }
            }
        }
        let layout = p.get_keyboard_layout(0);
        self.image.wr32(G_LAYOUT, layout);
        self.image.wr_f32(G_SCALE, 0.5);
        let qpf = p.query_performance_frequency();
        self.image.wr64(G_QPF, qpf);
        let qpc = p.query_performance_counter();
        self.image.wr64(G_QPC, qpc);
        Ok(0)
    }

    fn store_caps(&mut self, rec: u32, words: &[u32]) {
        for (index, word) in words.iter().enumerate() {
            self.image.wr32(rec + R_CAPS + 4 * index as u32, *word);
        }
    }

    fn mouse_objects(&mut self, rec: u32) {
        for (name, id) in [
            (&b"Mouse X"[..], 0x8002),
            (&b"Mouse Y"[..], 0x8004),
            (&b"Mouse Z"[..], 0x8008),
        ] {
            self.add_object(rec, name, id);
        }
        for name in [
            &b"M.Button 1"[..],
            b"M.Button 2",
            b"M.Button 3",
            b"M.Button 4",
        ] {
            let buttons = self.image.rd32(rec + R_BUTTONS);
            self.image.wr32(rec + R_BUTTONS, buttons + 1);
            self.add_object(rec, name, 0x1000 + buttons);
        }
        self.add_object(rec, b"Mouse X", 2);
        self.add_object(rec, b"Mouse Y", 4);
        self.add_object(rec, b"Mouse Z", 8);
        self.strncpy_record_name(rec, b"SysMouse");
        self.image.wr32(rec + R_KIND, KIND_MOUSE);
    }

    /// exe: FUN_005565a0
    /// `FUN_005565a0`: the gamepad `EnumDevices` callback; returns the word
    /// the callback leaves in `eax`.
    pub fn enum_devices_callback(
        &mut self,
        p: &mut dyn Platform,
        instance: &DeviceInstance,
    ) -> u32 {
        let rec = Self::rec(self.count());
        let di = self.image.rd32(G_DI);
        let (hr, dev) = p.di_create_device(di, instance.instance_guid, 0);
        if (hr as i32) < 0 {
            return 1;
        }
        let hr = p.dev_set_data_format(dev, FORMAT_JOYSTICK, FORMAT_JOYSTICK_WORDS);
        if (hr as i32) < 0 {
            return hr;
        }
        let hr = p.dev_set_cooperative_level(dev, self.hwnd, 5);
        if (hr as i32) < 0 {
            return hr;
        }
        self.image.clear(rec, RECORD);
        self.image.wr32(rec + R_DEVICE, dev);
        self.image.wr32(rec + R_KIND, KIND_JOYSTICK);
        self.image.wr32(rec + R_CAPS, 0x2c);
        let (hr, words) = p.dev_get_capabilities(dev, 0x2c);
        if hr == 0 {
            self.store_caps(rec, &words);
        }
        if (hr as i32) < 0 {
            return hr;
        }
        p.dev_unacquire(self.image.rd32(rec + R_DEVICE));
        let hr = p.dev_set_property(dev, 5, [0x14, 0x10, 0, 0, 0]);
        if (hr as i32) < 0 {
            return hr;
        }
        p.dev_acquire(self.image.rd32(rec + R_DEVICE));
        for flags in [3, 0xc, 0x10] {
            let objects = p.dev_enum_objects(dev, 0x0055_6710, self.count(), flags);
            for object in &objects {
                p.dev_enum_callback_enter(dev, object);
                let eax = self.enum_objects_callback(rec, object);
                p.dev_enum_callback_result(dev, eax);
                if eax == 0 {
                    break;
                }
            }
        }
        self.strncpy_record_name(rec, &instance.name);
        let count = self.count() + 1;
        self.image.wr32(G_COUNT, count);
        u32::from(count < 0x10)
    }

    /// exe: FUN_00556710
    /// `FUN_00556710`: the `EnumObjects` callback for the record at `rec`.
    pub fn enum_objects_callback(&mut self, rec: u32, object: &ObjectInstance) -> u32 {
        if self.image.rd32(rec + R_OBJECTS) >= 0x40 {
            return 0;
        }
        if object.flags & 1 != 0 {
            let n = self.image.rd32(rec + R_ACTUATORS);
            self.image.wr32(rec + R_ACTUATORS, n + 1);
        }
        let head = u32::from_le_bytes(object.guid[..4].try_into().unwrap());
        if object.guid[4..] != OBJ_GUID_TAIL {
            return 1;
        }
        let axis_id = match head {
            OBJ_X => Some(2),
            OBJ_Y => Some(4),
            OBJ_Z => Some(8),
            OBJ_RX => Some(0x10),
            OBJ_RY => Some(0x20),
            OBJ_RZ => Some(0x40),
            _ => None,
        };
        if let Some(id) = axis_id {
            self.add_object(rec, &object.name, id);
        } else if head == OBJ_SLIDER {
            let n = self.image.rd32(rec + R_SLIDERS);
            if n < 2 {
                self.image.wr32(rec + R_SLIDERS, n + 1);
                self.add_object(rec, &object.name, 0x80 + n);
            }
        } else if head == OBJ_BUTTON {
            let n = self.image.rd32(rec + R_BUTTONS);
            self.image.wr32(rec + R_BUTTONS, n + 1);
            self.add_object(rec, &object.name, 0x1000 + n);
        } else if head == OBJ_POV {
            let n = self.image.rd32(rec + R_POVS);
            if n < 4 {
                self.add_object(rec, &object.name, 0x2000 + n);
                self.add_object(rec, &object.name, 0x3000 + n);
                self.image.wr32(rec + R_POVS, n + 1);
            }
        }
        1
    }

    /// exe: FUN_00556bc0
    /// `FUN_00556bc0`: poll every device, returning the frame time
    /// `DAT_0076f9a0` in milliseconds.
    pub fn poll(&mut self, p: &mut dyn Platform) -> f32 {
        let freq = p.query_performance_frequency();
        self.image.wr64(G_QPF, freq);
        let now = p.query_performance_counter();
        let previous = self.image.rd64(G_QPC);
        self.image.wr64(G_QPC, now);
        let diff = round_i128_f32(i128::from(now as i64) - i128::from(previous as i64));
        let dt = div_f32_by_i64(diff, freq as i64) * MS;
        self.image.wr_f32(G_DT, dt);
        for index in 0..self.count_signed().max(0) as u32 {
            let rec = Self::rec(index);
            let dev = self.image.rd32(rec + R_DEVICE);
            let state = rec + R_STATE;
            match self.image.rd32(rec + R_KIND) {
                KIND_KEYBOARD => {
                    if !self.get_state(p, dev, state, 0x100) {
                        if p.dev_acquire(dev) == 0 {
                            self.get_state(p, dev, state, 0x100);
                        } else {
                            self.image.clear(state, 0x100);
                        }
                    }
                }
                KIND_MOUSE => {
                    let ok = self.get_state(p, dev, state, 0x10)
                        || (p.dev_acquire(dev) == 0 && self.get_state(p, dev, state, 0x10));
                    if ok {
                        self.accumulate_mouse(rec);
                    } else {
                        self.image.clear(state, 16);
                    }
                }
                KIND_JOYSTICK => {
                    let mut read = true;
                    if (p.dev_poll(dev) as i32) < 0 {
                        // Re-acquire while the device reports a lost input,
                        // at most eleven times.
                        read = false;
                        let mut exhausted = true;
                        for _ in 0..11 {
                            let hr = p.dev_acquire(dev);
                            if hr != 0x8007_001e {
                                exhausted = false;
                                read = (hr as i32) >= 0;
                                break;
                            }
                        }
                        if exhausted {
                            self.image.clear(state, 0x110);
                        }
                    }
                    if read {
                        self.get_state(p, dev, state, 0x110);
                    }
                }
                _ => {}
            }
        }
        self.image.rd_f32(G_DT)
    }

    /// `GetDeviceState(size)` into the record buffer; `true` on `DI_OK`.
    fn get_state(&mut self, p: &mut dyn Platform, dev: Handle, state: u32, size: u32) -> bool {
        let (hr, report) = p.dev_get_device_state(dev, size);
        if hr == 0 {
            self.image.write_bytes(state, &report);
        }
        hr == 0
    }

    /// The mouse accumulation loop of `FUN_00556bc0`.
    fn accumulate_mouse(&mut self, rec: u32) {
        let scale = self.image.rd_f32(G_SCALE);
        let state = rec + R_STATE;
        for j in 0..self.image.rd32(rec + R_OBJECTS) {
            let acc = rec + R_ACCUM + 4 * j;
            let delta = match self.image.rd32(rec + R_IDS + 4 * j) {
                2 => Some(self.image.rd32(state) as i32 as f32),
                4 => Some(self.image.rd32(state + 4) as i32 as f32),
                8 => Some(self.image.rd32(state + 8) as i32 as f32),
                _ => None,
            };
            if let Some(delta) = delta {
                let value = delta * scale * MOUSE_STEP + self.image.rd_f32(acc);
                self.image.wr_f32(acc, value);
            }
            let value = self.image.rd_f32(acc);
            if ONE < value {
                self.image.wr_f32(acc, ONE);
            }
            if self.image.rd_f32(acc) < NEG_ONE {
                self.image.wr_f32(acc, NEG_ONE);
            }
        }
    }

    /// exe: FUN_00556e00
    /// `FUN_00556e00`: the scan code of the last buffered key press in the
    /// low word, its translated character in the high word; `0` when none.
    pub fn last_key(&mut self, p: &mut dyn Platform) -> u32 {
        let dev = self.image.rd32(TABLE + R_DEVICE);
        if (p.dev_acquire(dev) as i32) < 0 {
            return 0;
        }
        let (hr, events) = p.dev_get_device_data(dev, 0x14, 1, 0);
        if (hr as i32) >= 0 && events.len() == 1 {
            let (scan, data) = events[0];
            if scan < 0x100 && data & 0x80 != 0 {
                let layout = self.image.rd32(G_LAYOUT);
                let vk = p.map_virtual_key_ex(scan, 1, layout);
                let mut word = 0u16;
                if p.get_keyboard_state().is_some() {
                    word = p.to_ascii_ex(vk, scan, 0, layout).1;
                }
                return (u32::from(word) << 16) | scan;
            }
        }
        0
    }

    /// exe: FUN_00556ec0
    /// `FUN_00556ec0`: is object `code` of device 0 pressed.
    pub fn is_down(&mut self, _unused: u32, _unused2: u32, code: u32) -> u32 {
        u32::from(self.sample(0, code) > DIGITAL)
    }

    /// exe: FUN_00557430
    /// `FUN_00557430`: the value of object `code` of device `dev`.
    pub fn axis_value(&mut self, dev: u32, code: u32) -> f32 {
        self.sample(dev, code)
    }

    /// The shared body of `FUN_00557430` and `FUN_00556ec0`.
    fn sample(&mut self, dev: u32, code: u32) -> f32 {
        let rec = Self::rec(dev);
        let objects = self.image.rd32(rec + R_OBJECTS) as i32;
        let state = rec + R_STATE;
        let signed = code as i32;
        match self.image.rd32(rec + R_KIND) {
            KIND_KEYBOARD => {
                if signed >= 0
                    && signed < objects
                    && self.image.rd8(state + (code & 0xff)) & 0x80 != 0
                {
                    ONE
                } else {
                    0.0
                }
            }
            KIND_MOUSE => {
                if code >= 0x8000_0000 && signed > -0x66 {
                    let scale = (-1i32).wrapping_sub(signed) as f32;
                    self.image.wr_f32(G_SCALE, scale * SCALE_STEP);
                    for j in 0..objects.max(0) as u32 {
                        self.image.wr32(rec + R_ACCUM + 4 * j, 0);
                    }
                    return 0.0;
                }
                if signed < 0 || signed >= objects {
                    return 0.0;
                }
                let id = self.image.rd32(rec + R_IDS + 4 * code);
                let buttons = self.image.rd32(rec + R_BUTTONS);
                if id >= 0x1000 && id < buttons + 0x1000 {
                    return if self.image.rd8(rec + 0x650 + (id - 0x1000)) & 0x80 != 0 {
                        ONE
                    } else {
                        0.0
                    };
                }
                let value = match id {
                    2 | 4 | 8 => self.image.rd_f32(rec + R_ACCUM + 4 * code),
                    0x8002 | 0x8004 | 0x8008 => {
                        let dt = self.image.rd_f32(G_DT);
                        if dt <= 0.0 {
                            return 0.0;
                        }
                        let offset = match id {
                            0x8002 => 0,
                            0x8004 => 4,
                            _ => 8,
                        };
                        let delta = self.image.rd32(state + offset) as i32;
                        div_i32_by_f32(delta, dt) * MOUSE_VELOCITY
                    }
                    _ => return 0.0,
                };
                clamp_unit(value)
            }
            KIND_JOYSTICK => {
                if signed < 0 || signed >= objects {
                    return 0.0;
                }
                let id = self.image.rd32(rec + R_IDS + 4 * code);
                self.joystick_value(rec, id).unwrap_or(0.0)
            }
            _ => 0.0,
        }
    }

    /// The joystick arm shared by the three samplers; `None` is the zero case.
    fn joystick_value(&self, rec: u32, id: u32) -> Option<f32> {
        let state = rec + R_STATE;
        let buttons = self.image.rd32(rec + R_BUTTONS);
        if id >= 0x1000 && id < buttons + 0x1000 {
            return Some(if self.image.rd8(rec + 0x674 + (id - 0x1000)) & 0x80 != 0 {
                ONE
            } else {
                0.0
            });
        }
        let axis = |offset: u32| {
            let raw = 0x8000i32.wrapping_sub(self.image.rd32(state + offset) as i32);
            Some(raw as f32 * AXIS_SCALE)
        };
        let pov = |slot: u32| {
            let value = self.image.rd32(state + 0x20 + 4 * slot);
            (value != 0xffff_ffff).then_some(value)
        };
        match id {
            2 => axis(0),
            4 => axis(4),
            8 => axis(8),
            0x10 => axis(0xc),
            0x20 => axis(0x10),
            0x40 => axis(0x14),
            0x80 => axis(0x18),
            0x81 => axis(0x1c),
            0x2000..=0x2003 => pov(id - 0x2000).map(|v| -(fsin(pov_angle(v)) as f32)),
            0x3000..=0x3003 => pov(id - 0x3000).map(|v| fcos(pov_angle(v)) as f32),
            _ => None,
        }
    }

    /// exe: FUN_00557990
    /// `FUN_00557990`: is object `idx` of device `dev` an axis (not a button).
    #[must_use]
    pub fn is_axis(&self, dev: u32, idx: u32) -> u32 {
        let rec = Self::rec(dev);
        let kind = self.image.rd32(rec + R_KIND);
        if kind != KIND_KEYBOARD && kind > 1 && kind < 4 {
            let id = self.image.rd32(rec + R_IDS + 4 * idx);
            let buttons = self.image.rd32(rec + R_BUTTONS);
            if id < 0x1000 || buttons + 0x1000 <= id {
                return 1;
            }
        }
        0
    }

    /// exe: FUN_005579f0
    /// `FUN_005579f0`: the name of object `idx` of device `dev` into `out`
    /// (NUL-terminated); `0` on success, `-1` otherwise.
    pub fn axis_name(&self, dev: i32, idx: i32, out: &mut [u8]) -> u32 {
        out[0] = 0;
        if dev >= 0 && dev < self.count_signed() {
            let rec = Self::rec(dev as u32);
            if idx >= 0 && idx < self.image.rd32(rec + R_OBJECTS) as i32 {
                let kind = self.image.rd32(rec + R_KIND);
                let text: Vec<u8> = if kind == KIND_KEYBOARD {
                    KEY_NAMES[idx as usize & 0xff].as_bytes().to_vec()
                } else if kind > 1 && kind < 4 {
                    self.cstr(rec + R_NAMES + 16 * idx as u32, 0x10_0000)
                } else {
                    return u32::MAX;
                };
                out[..text.len()].copy_from_slice(&text);
                out[text.len()] = 0;
                return 0;
            }
        }
        u32::MAX
    }

    fn cstr(&self, address: u32, max: u32) -> Vec<u8> {
        let mut out = Vec::new();
        for i in 0..max {
            let byte = self.image.rd8(address + i);
            if byte == 0 {
                break;
            }
            out.push(byte);
        }
        out
    }

    /// exe: FUN_00557aa0
    /// `FUN_00557aa0`: the one object of device `dev` that is currently
    /// active, or `u32::MAX` when none or several are.
    pub fn active_axis(&mut self, dev: i32) -> u32 {
        if dev < 0 || dev >= self.count_signed() {
            return u32::MAX;
        }
        let rec = Self::rec(dev as u32);
        let objects = self.image.rd32(rec + R_OBJECTS);
        let kind = self.image.rd32(rec + R_KIND);
        let mut found = u32::MAX;
        let mut hits = 0;
        for j in 0..objects {
            let id = self.image.rd32(rec + R_IDS + 4 * j);
            if id & 0x8000 != 0 {
                continue;
            }
            let value = match kind {
                KIND_KEYBOARD => {
                    if self.image.rd8(rec + R_STATE + (j & 0xff)) & 0x80 != 0 {
                        ONE
                    } else {
                        0.0
                    }
                }
                KIND_MOUSE => {
                    let buttons = self.image.rd32(rec + R_BUTTONS);
                    if id >= 0x1000 && id < buttons + 0x1000 {
                        if self.image.rd8(rec + 0x650 + (id - 0x1000)) & 0x80 != 0 {
                            ONE
                        } else {
                            0.0
                        }
                    } else if matches!(id, 2 | 4 | 8) {
                        clamp_unit(self.image.rd_f32(rec + R_ACCUM + 4 * j))
                    } else {
                        0.0
                    }
                }
                KIND_JOYSTICK => self.joystick_value(rec, id).unwrap_or(0.0),
                _ => {
                    self.image.wr_f32(rec + R_ACCUM + 4 * j, 0.0);
                    continue;
                }
            };
            let accumulated = self.image.rd_f32(rec + R_ACCUM + 4 * j);
            if ACTIVE_HIGH <= value {
                hits += 1;
                found = j;
            }
            if (ACTIVE_MIN..=ACTIVE_LOW).contains(&value) {
                hits += 1;
                found = j;
            }
            if value <= ACTIVE_MIN && ACTIVE_LOW <= accumulated {
                hits += 1;
                found = j;
            }
            // Every sampled object leaves its value in the per-object slot.
            self.image.wr_f32(rec + R_ACCUM + 4 * j, value);
        }
        if objects > 0 && hits == 1 {
            return found;
        }
        u32::MAX
    }

    /// exe: FUN_00558100
    /// `FUN_00558100`: the device name bytes (up to the NUL), `None` outside the table.
    #[must_use]
    pub fn device_name(&self, dev: i32) -> Option<Vec<u8>> {
        if dev >= 0 && dev < self.count_signed() {
            let rec = Self::rec(dev as u32);
            let mut name = self.cstr(rec + R_NAME, 0x40);
            name.truncate(0x40);
            return Some(name);
        }
        None
    }

    /// exe: FUN_00558130
    /// `FUN_00558130`: the kind word of device `dev`, `0` outside the table.
    #[must_use]
    pub fn device_kind(&self, dev: i32) -> u32 {
        if dev >= 0 && dev < self.count_signed() {
            return self.image.rd32(Self::rec(dev as u32) + R_KIND);
        }
        0
    }

    /// exe: FUN_00556b10
    /// `FUN_00556b10`.
    pub fn shutdown(&mut self, p: &mut dyn Platform) -> u32 {
        for index in 0..self.count() {
            let rec = Self::rec(index);
            if self.image.rd32(rec + R_KIND) == KIND_JOYSTICK {
                for offset in R_EFFECTS {
                    // Force feedback is unmodelled: the slots are never filled.
                    assert_eq!(self.image.rd32(rec + offset), 0, "effect object");
                }
            }
        }
        for index in 0..self.count() {
            let rec = Self::rec(index);
            let dev = self.image.rd32(rec + R_DEVICE);
            if dev != 0 {
                p.dev_unacquire(dev);
                p.dev_release(dev);
                self.image.wr32(rec + R_DEVICE, 0);
            }
        }
        self.image.wr32(G_COUNT, 0);
        let di = self.image.rd32(G_DI);
        if di != 0 {
            p.di_release(di);
        }
        0
    }
}

fn clamp_unit(value: f32) -> f32 {
    value.clamp(NEG_ONE, ONE)
}

fn fsin(x: f32) -> f64 {
    f64::from(x).sin()
}

fn fcos(x: f32) -> f64 {
    f64::from(x).cos()
}

/// `(float10)(uint)pov * (float10)_DAT_005f3acc` at 24-bit precision.
fn pov_angle(pov: u32) -> f32 {
    let bits = POV_RADIANS.to_bits();
    let mantissa = u64::from((bits & 0x7f_ffff) | 0x80_0000);
    let exponent = ((bits >> 23) & 0xff) as i32 - 150;
    let product = u64::from(pov) * mantissa;
    (product as f32) * 2f32.powi(exponent)
}

/// Round an exact integer to the nearest-even `f32`.
fn round_i128_f32(value: i128) -> f32 {
    let magnitude = value.unsigned_abs();
    let rounded = round_u128_f32(magnitude, false, 0);
    if value < 0 {
        -rounded
    } else {
        rounded
    }
}

/// `m * 2^e` with `m` an integer plus a sticky bit, rounded to nearest even.
fn round_u128_f32(m: u128, sticky: bool, e: i32) -> f32 {
    if m == 0 {
        return 0.0;
    }
    let len = 128 - m.leading_zeros() as i32;
    if len <= 24 {
        return (m as f32) * 2f32.powi(e);
    }
    let shift = (len - 24) as u32;
    let keep = (m >> shift) as u64;
    let rest = m & ((1u128 << shift) - 1);
    let half = 1u128 << (shift - 1);
    let up = rest > half || (rest == half && (sticky || keep & 1 == 1));
    let keep = keep + u64::from(up);
    (keep as f32) * 2f32.powi(e + shift as i32)
}

/// Exact `a / b`, rounded once to `f32` (the 24-bit-precision `fdiv`).
fn div_round_f32(negative: bool, n: u128, ne: i32, d: u128, de: i32) -> f32 {
    assert!(d != 0);
    if n == 0 {
        return if negative { -0.0 } else { 0.0 };
    }
    // Scale so the integer quotient has at least 26 significant bits.
    let nl = 128 - n.leading_zeros() as i32;
    let dl = 128 - d.leading_zeros() as i32;
    let s = (26 - (nl - dl)).max(0) as u32;
    let q = (n << s) / d;
    let sticky = !(n << s).is_multiple_of(d);
    let r = round_u128_f32(q, sticky, ne - de - s as i32);
    if negative {
        -r
    } else {
        r
    }
}

/// `f32 / integer` at 24-bit precision (`fild` divisor).
fn div_f32_by_i64(value: f32, divisor: i64) -> f32 {
    let bits = value.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32;
    let (mant, e) = if exp == 0 {
        (u128::from(bits & 0x7f_ffff), -149)
    } else {
        (u128::from((bits & 0x7f_ffff) | 0x80_0000), exp - 150)
    };
    div_round_f32(
        (bits >> 31 != 0) ^ (divisor < 0),
        mant,
        e,
        u128::from(divisor.unsigned_abs()),
        0,
    )
}

/// `(float10)int / (float10)f32` at 24-bit precision.
fn div_i32_by_f32(value: i32, divisor: f32) -> f32 {
    let bits = divisor.to_bits();
    let exp = ((bits >> 23) & 0xff) as i32;
    let (mant, e) = if exp == 0 {
        (u128::from(bits & 0x7f_ffff), -149)
    } else {
        (u128::from((bits & 0x7f_ffff) | 0x80_0000), exp - 150)
    };
    div_round_f32(
        (value < 0) ^ (bits >> 31 != 0),
        u128::from(value.unsigned_abs()),
        0,
        mant,
        e,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_division_rounds_once() {
        // 1 / 3 correctly rounded.
        assert_eq!(div_f32_by_i64(1.0, 3), 1.0f32 / 3.0);
        assert_eq!(div_i32_by_f32(7, 3.0), 7.0f32 / 3.0);
        assert_eq!(div_i32_by_f32(-7, 3.0), -7.0f32 / 3.0);
        assert_eq!(
            div_f32_by_i64(16_000.0, 1_000_000),
            16_000.0f32 / 1_000_000.0
        );
        assert_eq!(round_i128_f32(16_777_217), 16_777_216.0);
        assert_eq!(round_i128_f32(16_777_219), 16_777_220.0);
        assert_eq!(round_i128_f32(-3), -3.0);
    }

    #[test]
    fn pov_angle_matches_the_f32_product_below_2_24() {
        for pov in [0u32, 1, 4500, 9000, 17_999, 35_999] {
            assert_eq!(pov_angle(pov), pov as f32 * POV_RADIANS);
        }
    }
}

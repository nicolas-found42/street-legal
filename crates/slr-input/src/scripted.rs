//! A deterministic, scenario-driven [`Platform`]: declared keyboard, mouse and
//! gamepad devices answering from JSON-described queues.
//!
//! The scenario schema is the one `re/oracle/input_device_vectors.py` writes
//! and the Unicorn rig consumes, so the same declared schedule drives the
//! original device layer and this port. Every call is recorded as one
//! canonical string (the DirectInput call stream); both sides format the
//! strings identically, and the strings are compared verbatim.
//!
//! Answer queues are named: `qpc`, `time`, `mapvk`, `kbstate`,
//! `kbstate.ok`, `toascii`, `<device>.state`, `<device>.data`,
//! `<device>.<Method>.hr` and `di.CreateDevice.hr`. An exhausted queue
//! yields its default (`DI_OK`, a zeroed report, no events). Device labels are
//! `kbd`, `mouse`, `joy0`... in creation order.
//!
//! Declared platform rule (not an observation of Windows): an enumeration
//! callback that returns `DIENUM_STOP` (0) ends the enumeration; any other
//! value continues it.

use std::collections::{HashMap, VecDeque};
use std::fmt::{Display, Write as _};

use serde_json::Value;

use crate::device::{DeviceInstance, Handle, ObjectInstance, Platform};

const DIERR_DEVICENOTREG: u32 = 0x8004_0154;
const GUID_KEYBOARD_HEX: &str = "612b1d6fa0d5cf11bfc7444553540000";
const GUID_MOUSE_HEX: &str = "602b1d6fa0d5cf11bfc7444553540000";

/// Hex of a byte string, lowercase.
#[must_use]
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        write!(out, "{byte:02x}").unwrap();
    }
    out
}

fn unhex(text: &str) -> Vec<u8> {
    (0..text.len() / 2)
        .map(|i| u8::from_str_radix(&text[2 * i..2 * i + 2], 16).expect("hex"))
        .collect()
}

fn list<T: Display>(items: &[T]) -> String {
    let mut out = String::from("[");
    for (index, item) in items.iter().enumerate() {
        if index > 0 {
            out.push_str(", ");
        }
        write!(out, "{item}").unwrap();
    }
    out.push(']');
    out
}

/// Declared gamepad: instance identity and its enumerable objects.
#[derive(Clone, Debug)]
struct Pad {
    instance_guid: String,
    devtype: u32,
    name: String,
    caps: [u32; 5],
    objects: Vec<ObjectInstance>,
}

/// The scripted platform.
#[derive(Debug)]
pub struct ScriptedPlatform {
    qpf: u64,
    layout: u32,
    caps: HashMap<&'static str, [u32; 5]>,
    pads: Vec<Pad>,
    queues: HashMap<String, VecDeque<Value>>,
    labels: HashMap<Handle, String>,
    next_handle: Handle,
    calls: Vec<String>,
}

impl ScriptedPlatform {
    /// Build from a scenario object (`env`, `devices`).
    ///
    /// # Panics
    /// On a malformed scenario.
    #[must_use]
    pub fn new(scenario: &Value) -> Self {
        let env = &scenario["env"];
        let words = |v: &Value| {
            [
                v["flags"].as_u64().unwrap() as u32,
                v["devtype"].as_u64().unwrap() as u32,
                v["axes"].as_u64().unwrap() as u32,
                v["buttons"].as_u64().unwrap() as u32,
                v["povs"].as_u64().unwrap() as u32,
            ]
        };
        let mut caps = HashMap::new();
        caps.insert("kbd", words(&scenario["devices"]["caps"]["kbd"]));
        caps.insert("mouse", words(&scenario["devices"]["caps"]["mouse"]));
        let pads = scenario["devices"]["gamepads"]
            .as_array()
            .map(|pads| {
                pads.iter()
                    .map(|pad| Pad {
                        instance_guid: pad["instance_guid"].as_str().unwrap().to_owned(),
                        devtype: pad["devtype"].as_u64().unwrap() as u32,
                        name: pad["name"].as_str().unwrap().to_owned(),
                        caps: words(&pad["caps"]),
                        objects: pad["objects"]
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|o| ObjectInstance {
                                guid: unhex(o["guid"].as_str().unwrap()).try_into().unwrap(),
                                object_type: o["type"].as_u64().unwrap() as u32,
                                flags: o["flags"].as_u64().unwrap_or(0) as u32,
                                name: o["name"].as_str().unwrap().as_bytes().to_vec(),
                            })
                            .collect(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        ScriptedPlatform {
            qpf: env["qpf"].as_u64().unwrap(),
            layout: env["layout"].as_u64().unwrap() as u32,
            caps,
            pads,
            queues: HashMap::new(),
            labels: HashMap::new(),
            next_handle: 0x1000,
            calls: Vec::new(),
        }
    }

    /// Append answers: `feed` maps a queue name to an array of values.
    pub fn feed(&mut self, feed: &Value) {
        if let Some(map) = feed.as_object() {
            for (key, values) in map {
                self.queues
                    .entry(key.clone())
                    .or_default()
                    .extend(values.as_array().unwrap().iter().cloned());
            }
        }
    }

    /// Take the calls recorded since the last call.
    pub fn take_calls(&mut self) -> Vec<String> {
        std::mem::take(&mut self.calls)
    }

    fn pop(&mut self, key: &str) -> Option<Value> {
        self.queues.get_mut(key).and_then(VecDeque::pop_front)
    }

    fn pop_u64(&mut self, key: &str, default: u64) -> u64 {
        self.pop(key).map_or(default, |v| v.as_u64().unwrap())
    }

    fn hr(&mut self, label: &str, method: &str, default: u32) -> u32 {
        self.pop_u64(&format!("{label}.{method}.hr"), u64::from(default)) as u32
    }

    fn label(&self, dev: Handle) -> String {
        self.labels[&dev].clone()
    }

    fn log(&mut self, line: String) {
        self.calls.push(line);
    }

    fn new_handle(&mut self, label: &str) -> Handle {
        let handle = self.next_handle;
        self.next_handle += 1;
        self.labels.insert(handle, label.to_owned());
        handle
    }
}

impl Platform for ScriptedPlatform {
    fn direct_input_create(&mut self, version: u32, outer: u32) -> Result<Handle, u32> {
        self.log(format!(
            "DirectInput8Create(version={version:#x},outer={outer})"
        ));
        Ok(self.new_handle("di"))
    }

    fn get_keyboard_layout(&mut self, thread: u32) -> u32 {
        let value = self.layout;
        self.log(format!("win.GetKeyboardLayout({thread})={value:#x}"));
        value
    }

    fn query_performance_frequency(&mut self) -> u64 {
        let value = self.qpf;
        self.log(format!("win.QueryPerformanceFrequency()={value}"));
        value
    }

    fn query_performance_counter(&mut self) -> u64 {
        let value = self.pop_u64("qpc", 0);
        self.log(format!("win.QueryPerformanceCounter()={value}"));
        value
    }

    fn time_get_time(&mut self) -> u32 {
        let value = self.pop_u64("time", 0) as u32;
        self.log(format!("win.timeGetTime()={value}"));
        value
    }

    fn map_virtual_key_ex(&mut self, code: u32, kind: u32, layout: u32) -> u32 {
        let result = self.pop_u64("mapvk", 0) as u32;
        self.log(format!(
            "win.MapVirtualKeyExA({code:#x},{kind},{layout:#x})={result:#x}"
        ));
        result
    }

    fn get_keyboard_state(&mut self) -> Option<[u8; 256]> {
        let data = self
            .pop("kbstate")
            .map_or_else(|| vec![0; 256], |v| unhex(v.as_str().unwrap()));
        let ok = self.pop_u64("kbstate.ok", 1);
        if ok != 0 {
            self.log(format!("win.GetKeyboardState()={ok} {}", hex(&data)));
            Some(data.try_into().unwrap())
        } else {
            self.log(format!("win.GetKeyboardState()={ok}"));
            None
        }
    }

    fn to_ascii_ex(&mut self, vk: u32, scan: u32, flags: u32, layout: u32) -> (u32, u16) {
        let (ret, word) = self.pop("toascii").map_or((0, 0), |v| {
            (v[0].as_u64().unwrap() as u32, v[1].as_u64().unwrap() as u16)
        });
        self.log(format!(
            "win.ToAsciiEx(vk={vk:#x},sc={scan:#x},flags={flags},layout={layout:#x})={ret} word={:#x}",
            u32::from(word)
        ));
        (ret, word)
    }

    fn di_release(&mut self, _di: Handle) {
        self.log("di.Release()".into());
    }

    fn di_create_device(&mut self, _di: Handle, guid: [u8; 16], outer: u32) -> (u32, Handle) {
        let raw = hex(&guid);
        let label = if raw == GUID_KEYBOARD_HEX {
            Some("kbd".to_owned())
        } else if raw == GUID_MOUSE_HEX {
            Some("mouse".to_owned())
        } else {
            self.pads
                .iter()
                .position(|pad| pad.instance_guid == raw)
                .map(|index| format!("joy{index}"))
        };
        let default = if label.is_some() {
            0
        } else {
            DIERR_DEVICENOTREG
        };
        let hr = self.hr("di", "CreateDevice", default);
        if hr != 0 {
            self.log(format!("di.CreateDevice({raw},outer={outer})=>{hr:#x}"));
            return (hr, 0);
        }
        let label = label.expect("CreateDevice forced to succeed for an undeclared device");
        let handle = self.new_handle(&label);
        self.log(format!("di.CreateDevice({raw},outer={outer})=>0 {label}"));
        (0, handle)
    }

    fn di_enum_devices(
        &mut self,
        _di: Handle,
        devtype: u32,
        callback: u32,
        reference: u32,
        flags: u32,
    ) -> Vec<DeviceInstance> {
        self.log(format!(
            "di.EnumDevices(devtype={devtype},callback={callback:#x},ref={reference:#x},flags={flags})"
        ));
        self.pads
            .iter()
            .map(|pad| DeviceInstance {
                instance_guid: unhex(&pad.instance_guid).try_into().unwrap(),
                devtype: pad.devtype,
                name: pad.name.as_bytes().to_vec(),
            })
            .collect()
    }

    fn di_enum_callback_enter(&mut self, instance: &DeviceInstance) {
        self.log(format!(
            "di.EnumDevices.callback(instance={},devtype={:#x},name={})",
            hex(&instance.instance_guid),
            instance.devtype,
            String::from_utf8_lossy(&instance.name)
        ));
    }

    fn di_enum_callback_result(&mut self, eax: u32) {
        self.log(format!("di.EnumDevices.callback=>{eax}"));
    }

    fn dev_release(&mut self, dev: Handle) -> u32 {
        let label = self.label(dev);
        self.log(format!("{label}.Release()"));
        self.hr(&label, "Release", 0)
    }

    fn dev_get_capabilities(&mut self, dev: Handle, size: u32) -> (u32, Vec<u32>) {
        let label = self.label(dev);
        let hr = self.hr(&label, "GetCapabilities", 0);
        let caps = match label.strip_prefix("joy") {
            Some(index) => self.pads[index.parse::<usize>().unwrap()].caps,
            None => self.caps[label.as_str()],
        };
        let mut words = vec![
            44, caps[0], caps[1], caps[2], caps[3], caps[4], 0, 0, 0, 0, 0,
        ];
        words.truncate((size / 4) as usize);
        let tail = if hr == 0 {
            format!(" {}", list(&words))
        } else {
            String::new()
        };
        self.log(format!(
            "{label}.GetCapabilities(size={size})=>{hr:#x}{tail}"
        ));
        (hr, words)
    }

    fn dev_enum_objects(
        &mut self,
        dev: Handle,
        callback: u32,
        record: u32,
        flags: u32,
    ) -> Vec<ObjectInstance> {
        let label = self.label(dev);
        self.log(format!(
            "{label}.EnumObjects(callback={callback:#x},ref=rec{record},flags={flags})"
        ));
        let index: usize = label["joy".len()..].parse().unwrap();
        self.pads[index]
            .objects
            .iter()
            .filter(|o| o.object_type & flags != 0)
            .cloned()
            .collect()
    }

    fn dev_enum_callback_enter(&mut self, dev: Handle, object: &ObjectInstance) {
        let label = self.label(dev);
        self.log(format!(
            "{label}.EnumObjects.callback(guid={},type={:#x},flags={:#x},name={})",
            hex(&object.guid),
            object.object_type,
            object.flags,
            String::from_utf8_lossy(&object.name)
        ));
    }

    fn dev_enum_callback_result(&mut self, dev: Handle, eax: u32) {
        let label = self.label(dev);
        self.log(format!("{label}.EnumObjects.callback=>{eax}"));
    }

    fn dev_set_property(&mut self, dev: Handle, prop: u32, header: [u32; 5]) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "SetProperty", 0);
        self.log(format!(
            "{label}.SetProperty(prop={prop:#x},{})=>{hr:#x}",
            list(&header)
        ));
        hr
    }

    fn dev_acquire(&mut self, dev: Handle) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "Acquire", 0);
        self.log(format!("{label}.Acquire()=>{hr:#x}"));
        hr
    }

    fn dev_unacquire(&mut self, dev: Handle) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "Unacquire", 0);
        self.log(format!("{label}.Unacquire()=>{hr:#x}"));
        hr
    }

    fn dev_poll(&mut self, dev: Handle) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "Poll", 0);
        self.log(format!("{label}.Poll()=>{hr:#x}"));
        hr
    }

    fn dev_get_device_state(&mut self, dev: Handle, size: u32) -> (u32, Vec<u8>) {
        let label = self.label(dev);
        let hr = self.hr(&label, "GetDeviceState", 0);
        let mut report = String::new();
        let mut data = Vec::new();
        if hr == 0 {
            data = self
                .pop(&format!("{label}.state"))
                .map_or_else(|| vec![0; size as usize], |v| unhex(v.as_str().unwrap()));
            assert_eq!(data.len(), size as usize, "{label} report size");
            report = format!(" {}", hex(&data));
        }
        self.log(format!("{label}.GetDeviceState({size})=>{hr:#x}{report}"));
        (hr, data)
    }

    fn dev_get_device_data(
        &mut self,
        dev: Handle,
        cb: u32,
        count: u32,
        flags: u32,
    ) -> (u32, Vec<(u32, u32)>) {
        let label = self.label(dev);
        let hr = self.hr(&label, "GetDeviceData", 0);
        let mut events = Vec::new();
        if hr == 0 {
            if let Some(batch) = self.pop(&format!("{label}.data")) {
                events = batch
                    .as_array()
                    .unwrap()
                    .iter()
                    .take(count as usize)
                    .map(|e| (e[0].as_u64().unwrap() as u32, e[1].as_u64().unwrap() as u32))
                    .collect();
            }
        }
        let shown: Vec<String> = events.iter().map(|(o, v)| format!("[{o}, {v}]")).collect();
        self.log(format!(
            "{label}.GetDeviceData(cb={cb},in={count},flags={flags})=>{hr:#x} events={}",
            list(&shown)
        ));
        (hr, events)
    }

    fn dev_set_data_format(&mut self, dev: Handle, format: u32, words: [u32; 5]) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "SetDataFormat", 0);
        self.log(format!(
            "{label}.SetDataFormat(format={format:#x},{})=>{hr:#x}",
            list(&words)
        ));
        hr
    }

    fn dev_set_cooperative_level(&mut self, dev: Handle, hwnd: u32, flags: u32) -> u32 {
        let label = self.label(dev);
        let hr = self.hr(&label, "SetCooperativeLevel", 0);
        self.log(format!(
            "{label}.SetCooperativeLevel(hwnd={hwnd:#x},flags={flags:#x})=>{hr:#x}"
        ));
        hr
    }
}

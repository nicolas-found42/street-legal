//! Native WheelRef cells (stride0x2b4) and their spring parameter projection.
//! Original wrappers440c80..442090; the script Native.ptr names shared wheel
//! storage, while every successful Chassis.getWheel allocates a fresh wrapper.
use slr_vm::{
    host::{CallInfo, VmHost},
    values::Value,
};
use slr_x87::{Precision, Rounding, X87};
use std::{cell::RefCell, collections::HashMap};
const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;
#[derive(Clone, Debug)]
pub struct WheelState {
    pub words: [u32; 173],
    pub spring: [u32; 64],
    template_ypr: Option<[f32; 3]>,
}
impl WheelState {
    pub fn from_words(words: [u32; 173]) -> Self {
        Self {
            words,
            spring: [0; 64],
            template_ypr: None,
        }
    }
    fn read(&self, offset: usize) -> u32 {
        self.words[offset / 4]
    }
    fn set(&mut self, offset: usize, value: u32) {
        self.words[offset / 4] = value;
    }
    fn spring_update(&mut self) {
        // Original491f80, with its seven stack arguments from the wrapper.
        let a = [
            self.read(0x44),
            self.read(0x48),
            self.read(0x5c),
            add(self.read(0x54), self.read(0x60)),
            add(self.read(0x58), self.read(0x60)),
            self.read(0x4c),
            self.read(0x60),
        ];
        self.spring[0x44 / 4] = a[0];
        self.spring[0x48 / 4] = a[1];
        self.spring[0x34 / 4] = a[2];
        self.spring[0x4c / 4] = if f32::from_bits(a[5]) <= 0.0 {
            mul(a[1], 0.7f32.to_bits())
        } else {
            a[5]
        };
        self.spring[0x38 / 4] = if f32::from_bits(a[3]) > 0.0 {
            a[3]
        } else {
            a[2]
        };
        self.spring[0x3c / 4] = if f32::from_bits(a[4]) > 0.0 { a[4] } else { 0 };
        if f32::from_bits(a[6]) <= 0.0 {
            self.spring[0x50 / 4] = a[2];
            self.spring[0x40 / 4] = 0;
        } else {
            self.spring[0x50 / 4] = a[6];
            self.spring[0x40 / 4] = a[6];
        }
    }
    fn normalise(&mut self, offset: usize, order: [usize; 3]) {
        let v = [
            self.read(offset),
            self.read(offset + 4),
            self.read(offset + 8),
        ];
        let square = |i: usize| x(v[i]).mul(x(v[i]), PC, RC);
        let len = square(order[0])
            .add(square(order[1]), PC, RC)
            .add(square(order[2]), PC, RC)
            .sqrt(PC, RC);
        if !len.to_f32(PC, RC).is_nan() && len.to_f32(PC, RC) != 0.0 {
            let inv = X87::from_f32(1.0).div(len, PC, RC);
            for (i, value) in v.into_iter().enumerate() {
                self.set(
                    offset + 4 * i,
                    inv.mul(x(value), PC, RC).to_f32(PC, RC).to_bits(),
                );
            }
        }
    }
    pub fn apply(
        &mut self,
        name: &str,
        sig: &str,
        a: &[u32],
        ypr: Option<[f32; 3]>,
    ) -> Result<(), String> {
        let first = || {
            a.first()
                .copied()
                .ok_or("missing wheel argument".to_owned())
        };
        let simple = match name {
            "setSteer" => Some(0xc8),
            "setFrictn_x" => Some(0x218),
            "setSliction" => Some(0x1e8),
            "setStiffness" => Some(0x1f8),
            "setRollRes" => Some(0x74),
            "setBearing" => Some(0xf0),
            "setMaxLoad" => Some(0x214),
            "setLoadSmooth" => Some(0x220),
            _ => None,
        };
        if let Some(offset) = simple {
            self.set(offset, first()?);
            return Ok(());
        }
        match name {
            "setDrive" => {
                self.set(0xcc, first()?);
                self.set(0xe0, first()?);
            }
            "setRadius" | "setRestLen" => {
                self.set(if name == "setRadius" { 0x60 } else { 0x50 }, first()?);
                self.set(0x5c, add(self.read(0x60), self.read(0x50)));
                self.spring_update();
            }
            "setForce" | "setMinLen" | "setMaxLen" => {
                self.set(
                    match name {
                        "setForce" => 0x44,
                        "setMinLen" => 0x58,
                        _ => 0x54,
                    },
                    first()?,
                );
                self.spring_update();
            }
            "setDamping" => {
                self.set(0x48, first()?);
                self.set(
                    0x4c,
                    if sig == "(FF)V" {
                        a[1]
                    } else {
                        mul(first()?, 0.7f32.to_bits())
                    },
                );
                self.spring_update();
            }
            "setFriction" => {
                for off in [0x1f0, 0x64, 0x68] {
                    self.set(off, first()?);
                }
                let scale = friction_scale(self.read(0x15c));
                self.set(0x6c, mul(scale, first()?));
                self.set(0x70, mul(scale, first()?));
            }
            "setBrake" | "setHBrake" => {
                let offset = if name == "setBrake" { 0xd0 } else { 0xd4 };
                self.set(offset, first()?);
                self.set(offset + 8, mul(friction_scale(self.read(0x164)), first()?));
            }
            "setWidth" => self.set(0x1d4, mul(first()?, 0.5f32.to_bits())),
            "setPacejka" => {
                if (a[0] as i32) >= 0 && a[0] < 17 {
                    self.set(0x1e8 + a[0] as usize * 4, a[1]);
                }
            }
            "setCPatch" => {
                self.spring[0x84 / 4] = a[0] & 0x7fffffff;
                self.spring[0x88 / 4] = slr_x87::trans::approx_sin(x(a[1] & 0x7fffffff))
                    .value
                    .to_f32(PC, RC)
                    .to_bits();
                self.spring[0x8c / 4] = a[2];
            }
            "setInstantCenter" => {
                for (i, value) in a.iter().enumerate() {
                    self.set(0x27c + i * 4, add(*value, self.read(0x78 + (i % 3) * 4)));
                }
            }
            "setArm" => {
                self.set(0x234, a[0]);
                for i in 0..3 {
                    self.set(0x244 + i * 4, add(a[1 + i], self.read(0x78 + i * 4)));
                    self.set(0x238 + i * 4, a[4 + i]);
                }
                self.set(
                    0x248,
                    sub(self.read(0x248), add(self.read(0x3c), self.read(0x38))),
                );
                self.normalise(0x238, [1, 2, 0]);
            }
            "setHub" => {
                self.set(0x254, a[0]);
                for i in 0..3 {
                    self.set(0x258 + i * 4, a[1 + i]);
                    self.set(0x264 + i * 4, a[4 + i]);
                    self.set(0x270 + i * 4, add(a[7 + i], self.read(0x78 + i * 4)));
                }
                self.set(
                    0x274,
                    sub(self.read(0x274), add(self.read(0x3c), self.read(0x38))),
                );
                if f32::from_bits(a[0]) == 0.0 || f32::from_bits(a[0]).is_nan() {
                    self.normalise(0x264, [0, 1, 2]);
                    for i in 0..3 {
                        self.set(
                            0x270 + i * 4,
                            sub(self.read(0x270 + i * 4), self.read(0x258 + i * 4)),
                        );
                    }
                }
            }
            "setYpr" => {
                let angles = ypr.or(self.template_ypr).ok_or(
                    "WheelRef.setYpr null requires nativecar template orientation authority",
                )?;
                let m = slr_lang::vec::Mat3::from_ypr(slr_lang::vec::Ypr::new(
                    angles[0], angles[1], angles[2],
                ));
                for r in 0..3 {
                    for c in 0..3 {
                        self.set(0x90 + r * 16 + c * 4, m.m[r * 3 + c].to_bits());
                    }
                }
                for i in 0..12 {
                    self.set(0x110 + i * 4, self.read(0x90 + i * 4));
                }
            }
            _ => return Err(format!("unimplemented wheel state mutation {name}{sig}")),
        }
        Ok(())
    }
}
fn x(bits: u32) -> X87 {
    X87::from_f32(f32::from_bits(bits))
}
fn add(a: u32, b: u32) -> u32 {
    x(a).add(x(b), PC, RC).to_f32(PC, RC).to_bits()
}
fn sub(a: u32, b: u32) -> u32 {
    x(a).sub(x(b), PC, RC).to_f32(PC, RC).to_bits()
}
fn mul(a: u32, b: u32) -> u32 {
    x(a).mul(x(b), PC, RC).to_f32(PC, RC).to_bits()
}
fn friction_scale(w: u32) -> u32 {
    if !matches!(
        f32::from_bits(w).partial_cmp(&1.0),
        Some(std::cmp::Ordering::Greater | std::cmp::Ordering::Equal)
    ) {
        X87::from_f32(1.0)
            .sub(
                x(w).mul(x(w), PC, RC)
                    .mul(X87::from_f32(f32::from_bits(0x3f19999a)), PC, RC),
                PC,
                RC,
            )
            .to_f32(PC, RC)
            .to_bits()
    } else {
        0x3dccccd0
    }
}
#[derive(Default)]
struct State {
    #[cfg(test)]
    next: u32,
    wheels: HashMap<u32, WheelState>,
    chassis: HashMap<u32, Vec<u32>>,
    owners: HashMap<u32, u32>,
}
thread_local! {static STATE:RefCell<State>=RefCell::new(State::default());}
/// Nativecar embedding supplies actual constructor/template-initialized cells.
#[cfg(test)]
pub fn bind_chassis(container: u32, wheels: Vec<WheelState>) {
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let mut handles = vec![];
        for wheel in wheels {
            state.next += 1;
            let handle = 0x77000000 + state.next;
            state.wheels.insert(handle, wheel);
            handles.push(handle);
        }
        state.chassis.insert(container, handles);
    });
}
/// Original45e840 default constructor. Preserve untouched input cells: the
/// three ResourceRef base ctors zero their four-word links, followed by the
/// exact scalar and intrusive-list initialization in the executable.
pub fn construct_wheel(memory: &mut slr_physics::space::Space, wheel: u32) {
    for base in [0, 0x10, 0x20] {
        for i in 0..4 {
            memory.w32(wheel + base + i * 4, 0);
        }
    }
    for offset in [
        0x30, 0x34, 0x194, 0x198, 0x19c, 0x1a0, 0x1b4, 0x1b8, 0x1bc, 0x1c0, 0x1d8, 0x1dc, 0x1e0,
        0x1e4, 0x2a0, 0x2a8,
    ] {
        memory.w32(wheel + offset, 0);
    }
    for (offset, value) in [
        (0x1a8, 0x5f105c),
        (0x1c8, 0x5f1058),
        (0x294, 0x5f1054),
        (0x298, 0x5f09ac),
        (0x2a4, 0x5f09ac),
        (0x29c, wheel + 0x2a4),
        (0x2ac, wheel + 0x298),
    ] {
        memory.w32(wheel + offset, value);
    }
}
/// Original44a9ab..44ab58: configured scalar cells before physics-world
/// transforms and spring creation. This does not fabricate those collaborators.
pub fn copy_template_fields(
    memory: &mut slr_physics::space::Space,
    wheel: u32,
    body: u32,
    template: u32,
    index: u32,
) {
    let end = template + 0x1ffc + index * 0xc8;
    memory.w32(wheel + 0x22c, body);
    for (dst, relative) in [
        (0x230, -0x54),
        (0xc8, -0xa0),
        (0xcc, -0x9c),
        (0xd0, -0x98),
        (0xd4, -0x94),
        (0xd8, -0x98),
        (0xdc, -0x94),
        (0xe0, -0x9c),
        (0xf0, -0x8c),
        (0xc4, -0x90),
        (0x64, -0x88),
        (0x68, -0x84),
        (0x38, -0x80),
        (0x3c, -0x7c),
        (0x40, -0x78),
        (0x60, -0x74),
    ] {
        memory.w32(wheel + dst, memory.r32(end.wrapping_add_signed(relative)));
    }
    memory.w32(wheel + 0x6c, memory.r32(wheel + 0x64));
    memory.w32(wheel + 0x70, memory.r32(wheel + 0x68));
    memory.w32(wheel + 0x74, 0x3c75c28f);
    memory.w32(
        wheel + 0x50,
        add(memory.r32(end - 0x7c), memory.r32(end - 0x80)),
    );
    memory.w32(wheel + 0x54, add(memory.r32(end - 0x80), 0x3dcccccd));
    memory.w32(wheel + 0x58, 0xbf800000);
    for offset in [
        0xf4, 0xf8, 0x100, 0xc0, 0x140, 0x144, 0x148, 0xe8, 0xe4, 0xfc, 0x14c, 0x150, 0x154, 0x158,
        0x1a4, 0x1c4, 0xec,
    ] {
        memory.w32(wheel + offset, 0);
    }
    for i in 0..3 {
        memory.w32(wheel + 0x84 + i * 4, memory.r32(end - 0x60 + i * 4));
    }
    let length = x(memory.r32(end - 0x74))
        .add(x(memory.r32(end - 0x7c)), PC, RC)
        .add(x(memory.r32(end - 0x80)), PC, RC)
        .to_f32(PC, RC)
        .to_bits();
    memory.w32(wheel + 0x5c, length);
    for (offset, value) in [
        (0x1ac, body),
        (0x1b0, index),
        (0x1d0, index),
        (0x1cc, body),
        (0x1d4, 0x3dcccccd),
    ] {
        memory.w32(wheel + offset, value);
    }
    memory.copy(wheel + 0x1e8, template + 0x2e50, 0x44);
}
/// Recover real configured wheel cells from the native car constructor. Raw
/// engine addresses remain stable handles; later setters write back to Space.
pub fn bind_native_body(
    container: u32,
    memory: &slr_physics::space::Space,
    body: u32,
    template: u32,
) -> Result<(), String> {
    let count = memory.r32(template + 0x1f40);
    if count > 1024 {
        return Err(format!(
            "WheelBoundary: configured wheel count{count} exceeds native layout"
        ));
    }
    let base = memory.r32(body + 0x13e4);
    if count != 0 && base == 0 {
        return Err("WheelBoundary: constructor has no wheel storage".into());
    }
    STATE.with(|state| {
        let mut state = state.borrow_mut();
        let mut handles = vec![];
        for i in 0..count {
            let handle = base + i * 0x2b4;
            let mut wheel =
                WheelState::from_words(std::array::from_fn(|j| memory.r32(handle + j as u32 * 4)));
            let spring = wheel.read(0x30);
            if spring != 0 {
                wheel.spring = std::array::from_fn(|j| memory.r32(spring + j as u32 * 4));
            }
            wheel.template_ypr = Some(std::array::from_fn(|j| {
                memory.rf(template + 0x1f50 + i * 0xc8 + j as u32 * 4)
            }));
            state.wheels.insert(handle, wheel);
            state.owners.insert(handle, container);
            handles.push(handle);
        }
        state.chassis.insert(container, handles);
    });
    Ok(())
}
fn fail(vm: &mut VmHost, e: String) -> Option<u32> {
    vm.native_failure = Some(format!("WheelBoundary: {e}"));
    None
}
fn native(vm: &mut VmHost, info: &CallInfo<'_>) -> Option<u32> {
    let mut args = vec![0; info.args.len().max(1)];
    if let Err(e) = slr_vm::host::marshal_args(info, &mut args) {
        return fail(vm, format!("arguments {e:?}"));
    }
    let receiver = args[0];
    if info.name == "getWheel" {
        if (args[1] as i32) < 0 {
            return Some(0);
        }
        let container =
            slr_resource::node::with_res(|h| h.node(h.object_node(receiver)).map(|n| n.container));
        let Some(container) = container else {
            return Some(0);
        };
        let handle = STATE.with(|s| {
            s.borrow()
                .chassis
                .get(&container)
                .map(|w| w.get(args[1] as usize).copied())
        });
        let Some(handle) = handle else {
            return fail(
                vm,
                format!("chassis{container:#x} lacks observed wheel cells"),
            );
        };
        let Some(handle) = handle else { return Some(0) };
        let word = match vm.alloc_object("java.game.parts.WheelRef") {
            Ok(word) => word,
            Err(e) => return fail(vm, format!("WheelRef allocation {e:?}")),
        };
        if let Err(e) = vm.write_field(word, "ptr", Value::int(handle as i32)) {
            return fail(vm, format!("WheelRef.ptr {e:?}"));
        }
        return Some(word);
    }
    let handle = vm.read_field(receiver, "ptr").map_or(0, |v| v.payload);
    if handle == 0 {
        return (!info.sig.ends_with('V')).then_some(0);
    }
    let ypr = if info.name == "setYpr" && args[1] != 0 {
        let mut values = [0.; 3];
        for (i, name) in ["y", "p", "r"].into_iter().enumerate() {
            match vm.read_field(args[1], name) {
                Ok(v) => values[i] = f32::from_bits(v.payload),
                Err(e) => return fail(vm, format!("Ypr.{name}: {e:?}")),
            }
        }
        Some(values)
    } else {
        None
    };
    let result = STATE.with(|state| {
        let mut state = state.borrow_mut();
        let owner = state.owners.get(&handle).copied();
        let wheel = state
            .wheels
            .get_mut(&handle)
            .ok_or(format!("unknown wheel pointer{handle:#x}"))?;
        if let Some(owner) = owner {
            crate::native_body::with_body(owner, |body| {
                wheel.words = std::array::from_fn(|j| body.memory.r32(handle + j as u32 * 4));
                let spring = wheel.read(0x30);
                if spring != 0 {
                    wheel.spring = std::array::from_fn(|j| body.memory.r32(spring + j as u32 * 4));
                }
            });
        }
        let offset = match info.name {
            "getDrive" => Some(0xcc),
            "getSteer" => Some(0xc8),
            "getRadius" => Some(0x60),
            "getBrake" => Some(0xd0),
            "getHBrake" => Some(0xd4),
            _ => None,
        };
        if let Some(offset) = offset {
            return Ok(Some(wheel.read(offset)));
        }
        if owner.is_some() && wheel.read(0x30) == 0 {
            if info.name == "setCPatch" {
                return Ok(None);
            }
            if matches!(
                info.name,
                "setRadius" | "setRestLen" | "setForce" | "setDamping" | "setMinLen" | "setMaxLen"
            ) {
                return Err(
                    "native wheel lacks the original spring object required by this setter".into(),
                );
            }
        }
        wheel.apply(info.name, info.sig, &args[1..], ypr)?;
        if let Some(owner) = owner {
            crate::native_body::with_body(owner, |body| {
                for (i, word) in wheel.words.iter().enumerate() {
                    let address = handle + i as u32 * 4;
                    if body.memory.r32(address) != *word {
                        body.memory.w32(address, *word);
                    }
                }
                let spring = wheel.read(0x30);
                if spring != 0 {
                    for (i, word) in wheel.spring.iter().enumerate() {
                        let address = spring + i as u32 * 4;
                        if body.memory.r32(address) != *word {
                            body.memory.w32(address, *word);
                        }
                    }
                }
            })
            .ok_or("wheel owning native body disappeared".to_owned())?;
        }
        Ok(None)
    });
    match result {
        Ok(result) => result,
        Err(e) => fail(vm, e),
    }
}
pub fn install(vm: &mut VmHost) {
    STATE.with(|s| *s.borrow_mut() = State::default());
    for &(class, name, sig, address) in crate::missing_rows::MISSING {
        if (class == "java.game.parts.WheelRef"
            && !matches!(name, "getPos" | "getYpr" | "setPos" | "setOppWheel"))
            || (class == "java.game.parts.bodypart.Chassis" && name == "getWheel")
        {
            vm.natives.register(class, name, sig, address);
            vm.native_impls.insert(address, native);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn original_default_constructor_preserves_uninitialised_cells() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/wheel-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_default_constructor_preserves_uninitialised_cells; run re/oracle/wheel_probe.py");
            return;
        };
        let source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in source["constructors"].as_array().unwrap() {
            let mut memory = slr_physics::space::Space::new();
            let wheel = memory.alloc(0x2b4);
            assert_eq!(wheel, row["address"].as_u64().unwrap() as u32);
            for offset in 0..0x2b4 {
                memory.w8(wheel + offset, row["fill"].as_u64().unwrap() as u8);
            }
            construct_wheel(&mut memory, wheel);
            let actual = (0..173)
                .map(|i| memory.r32(wheel + i * 4))
                .collect::<Vec<_>>();
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn original_template_copy_region_matches_all_cells() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/wheel-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_template_copy_region_matches_all_cells; run re/oracle/wheel_probe.py");
            return;
        };
        let source: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in source["template_copy"].as_array().unwrap() {
            let mut memory = slr_physics::space::Space::new();
            let template = memory.alloc(0x3000);
            let body = memory.alloc(0x1500);
            let wheels = memory.alloc(0x568);
            let index = row["index"].as_u64().unwrap() as u32;
            let wheel = wheels + index * 0x2b4;
            assert_eq!(wheel, row["wheel"].as_u64().unwrap() as u32);
            for (i, word) in row["template_words"].as_array().unwrap().iter().enumerate() {
                memory.w32(template + i as u32 * 4, word.as_u64().unwrap() as u32);
            }
            construct_wheel(&mut memory, wheel);
            copy_template_fields(&mut memory, wheel, body, template, index);
            let actual = (0..173)
                .map(|i| memory.r32(wheel + i * 4))
                .collect::<Vec<_>>();
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(actual, expected);
        }
    }
    #[test]
    fn original_setter_state_words_match() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../re/out/boot/wheel-original.json");
        let Ok(bytes) = std::fs::read(path) else {
            eprintln!("SLR_ORACLE_SKIPPED: original_setter_state_words_match; run re/oracle/wheel_probe.py");
            return;
        };
        let rows: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
        for row in rows["rows"].as_array().unwrap() {
            let seed = row["seed"].as_u64().unwrap() as u32;
            let mut wheel = WheelState::from_words([seed; 173]);
            wheel.set(0x30, 0);
            let a = row["args"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            let ypr = row["ypr"].as_array().map(|x| {
                [
                    x[0].as_f64().unwrap() as f32,
                    x[1].as_f64().unwrap() as f32,
                    x[2].as_f64().unwrap() as f32,
                ]
            });
            if row["null"] == true {
                let sig = row["signature"].as_str().unwrap();
                let params = sig.trim_start_matches('(').split(')').next().unwrap();
                let mut args = a
                    .iter()
                    .zip(params.bytes())
                    .map(|(&word, ty)| Value {
                        tag: if ty == b'I' {
                            slr_vm::values::TypeTag::Int
                        } else {
                            slr_vm::values::TypeTag::Float
                        },
                        payload: word,
                    })
                    .rev()
                    .collect::<Vec<_>>();
                args.push(Value::reference(0));
                let mut vm = VmHost::new();
                assert_eq!(
                    native(
                        &mut vm,
                        &CallInfo {
                            jvm: 0,
                            thread: 0,
                            class: "java.game.parts.WheelRef",
                            name: row["name"].as_str().unwrap(),
                            sig,
                            args: &args
                        }
                    ),
                    None
                );
                assert_eq!(vm.native_failure, None);
                assert!(vm.objects.is_empty());
            } else {
                wheel
                    .apply(
                        row["name"].as_str().unwrap(),
                        row["signature"].as_str().unwrap(),
                        &a,
                        ypr,
                    )
                    .unwrap();
            }
            let expected = row["words"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(
                wheel.words.as_slice(),
                expected,
                "{}{}",
                row["name"],
                row["signature"]
            );
            let spring = row["spring"]
                .as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_u64().unwrap() as u32)
                .collect::<Vec<_>>();
            assert_eq!(
                wheel.spring.as_slice(),
                spring,
                "spring {}{}",
                row["name"],
                row["signature"]
            );
        }
    }
    #[test]
    fn fresh_wrappers_share_wheel_cells_without_sharing_script_identity() {
        use slr_vm::vmclass::{Class, Field};
        let mut vm = VmHost::new();
        slr_resource::node::install(&mut vm);
        install(&mut vm);
        for name in [
            "java.game.parts.WheelRef",
            "java.game.parts.bodypart.Chassis",
        ] {
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: name.into(),
                base: None,
                interfaces: vec![],
                fields: vec![Field {
                    class: name.into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }],
                methods: vec![],
                bodies: vec![],
            });
        }
        let chassis = vm.alloc_object("java.game.parts.bodypart.Chassis").unwrap();
        let container = slr_resource::node::with_res(|h| {
            let c = h.add_container(slr_resource::node::Container::new(1, 42));
            let n = h.add_bound_node(42, c);
            h.bind_object(chassis, n);
            c
        });
        bind_chassis(container, vec![WheelState::from_words([0; 173])]);
        let call = |vm: &mut VmHost, receiver, name, sig, args: Vec<Value>| {
            let mut args = args.into_iter().rev().collect::<Vec<_>>();
            args.push(Value::reference(receiver));
            native(
                vm,
                &CallInfo {
                    jvm: 0,
                    thread: 0,
                    class: if name == "getWheel" {
                        "java.game.parts.bodypart.Chassis"
                    } else {
                        "java.game.parts.WheelRef"
                    },
                    name,
                    sig,
                    args: &args,
                },
            )
        };
        let first = call(
            &mut vm,
            chassis,
            "getWheel",
            "(I)Ljava.game.parts.WheelRef;",
            vec![Value::int(0)],
        )
        .unwrap();
        let second = call(
            &mut vm,
            chassis,
            "getWheel",
            "(I)Ljava.game.parts.WheelRef;",
            vec![Value::int(0)],
        )
        .unwrap();
        assert_ne!(first, second);
        assert_eq!(vm.read_field(first, "ptr"), vm.read_field(second, "ptr"));
        call(&mut vm, first, "setDrive", "(F)V", vec![Value::float(0.25)]);
        assert_eq!(
            call(&mut vm, second, "getDrive", "()F", vec![]),
            Some(0.25f32.to_bits())
        );
        assert_eq!(
            call(
                &mut vm,
                chassis,
                "getWheel",
                "(I)Ljava.game.parts.WheelRef;",
                vec![Value::int(-1)]
            ),
            Some(0)
        );
        assert_eq!(
            call(
                &mut vm,
                chassis,
                "getWheel",
                "(I)Ljava.game.parts.WheelRef;",
                vec![Value::int(1)]
            ),
            Some(0)
        );
        assert_eq!(vm.native_failure, None);
    }
    #[test]
    fn native_body_mutations_and_template_orientation_stay_shared() {
        use slr_vm::vmclass::{Class, Field};
        let mut vm = VmHost::new();
        slr_resource::node::install(&mut vm);
        install(&mut vm);
        crate::native_body::clear();
        for name in [
            "java.game.parts.WheelRef",
            "java.game.parts.bodypart.Chassis",
        ] {
            vm.loader.insert_test_class(Class {
                kind: 0,
                name: name.into(),
                base: None,
                interfaces: vec![],
                fields: vec![Field {
                    class: name.into(),
                    name: "ptr".into(),
                    ty: "I".into(),
                    is_static: false,
                }],
                methods: vec![],
                bodies: vec![],
            });
        }
        let chassis = vm.alloc_object("java.game.parts.bodypart.Chassis").unwrap();
        let container = slr_resource::node::with_res(|h| {
            let c = h.add_container(slr_resource::node::Container::new(1, 43));
            let n = h.add_bound_node(43, c);
            h.bind_object(chassis, n);
            c
        });
        let mut memory = slr_physics::space::Space::new();
        let template = memory.alloc(0x2200);
        let body = memory.alloc(0x1500);
        let wheel = memory.alloc(0x2b4);
        let spring = memory.alloc(0xec);
        construct_wheel(&mut memory, wheel);
        memory.w32(template + 0x1f40, 1);
        memory.w32(body + 0x13e4, wheel);
        memory.w32(wheel + 0x30, spring);
        bind_native_body(container, &memory, body, template).unwrap();
        crate::native_body::insert(crate::native_body::NativeBody {
            memory,
            template,
            body,
            container,
            resource_bindings: std::collections::HashMap::new(),
        });
        let call = |vm: &mut VmHost, receiver, name, sig, args: Vec<Value>| {
            let mut args = args.into_iter().rev().collect::<Vec<_>>();
            args.push(Value::reference(receiver));
            native(
                vm,
                &CallInfo {
                    jvm: 0,
                    thread: 0,
                    class: if name == "getWheel" {
                        "java.game.parts.bodypart.Chassis"
                    } else {
                        "java.game.parts.WheelRef"
                    },
                    name,
                    sig,
                    args: &args,
                },
            )
        };
        let wrapper = call(
            &mut vm,
            chassis,
            "getWheel",
            "(I)Ljava.game.parts.WheelRef;",
            vec![Value::int(0)],
        )
        .unwrap();
        assert_eq!(vm.read_field(wrapper, "ptr").unwrap().payload, wheel);
        crate::native_body::with_body(container, |state| state.memory.wf(wheel + 0xcc, 0.5));
        assert_eq!(
            call(&mut vm, wrapper, "getDrive", "()F", vec![]),
            Some(0.5f32.to_bits())
        );
        call(
            &mut vm,
            wrapper,
            "setDrive",
            "(F)V",
            vec![Value::float(0.25)],
        );
        call(
            &mut vm,
            wrapper,
            "setForce",
            "(F)V",
            vec![Value::float(123.)],
        );
        call(
            &mut vm,
            wrapper,
            "setYpr",
            "(Ljava.lang.Ypr;)V",
            vec![Value::reference(0)],
        );
        assert_eq!(vm.native_failure, None);
        crate::native_body::with_body(container, |state| {
            assert_eq!(state.memory.rf(wheel + 0xcc), 0.25);
            assert_eq!(state.memory.rf(wheel + 0xe0), 0.25);
            assert_eq!(state.memory.rf(spring + 0x44), 123.);
            assert_eq!(state.memory.rf(wheel + 0x90), 1.);
            assert_eq!(state.memory.rf(wheel + 0x110), 1.);
        });
    }
}

//! Config settings application. Resource/field lookup and graphics refresh are
//! supplied boundaries, matching the controlled original-function oracle.
use slr_x87::{Precision, Rounding, X87};
use std::collections::BTreeMap;

const PC: Precision = Precision::F32;
const RC: Rounding = Rounding::NearestEven;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    Has,
}

pub trait Source {
    /// Integers/has return raw integer words; floats return f32 bits.
    fn read(&mut self, kind: Kind, name: &'static str) -> Result<u32, String>;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Read {
        kind: Kind,
        name: &'static str,
        bits: u32,
    },
    Write {
        address: u32,
        bits: u32,
    },
    /// The graphics refresh was requested. Its implementation is a separate
    /// engine boundary, not a successful graphics operation.
    Refresh {
        address: u32,
    },
}

#[derive(Default, Debug)]
pub struct Settings {
    /// Authoritative written words. Optional absent fields preserve prior
    /// words; an absent map entry does not fabricate an original default.
    pub globals: BTreeMap<u32, u32>,
    pub events: Vec<Event>,
}

impl Settings {
    fn read(
        &mut self,
        source: &mut impl Source,
        kind: Kind,
        name: &'static str,
    ) -> Result<u32, String> {
        let bits = source
            .read(kind, name)
            .map_err(|e| format!("SettingsFieldBoundary: {name}: {e}"))?;
        self.events.push(Event::Read { kind, name, bits });
        Ok(bits)
    }
    fn write(&mut self, address: u32, bits: u32) {
        self.globals.insert(address, bits);
        self.events.push(Event::Write { address, bits });
    }
    fn copy(
        &mut self,
        source: &mut impl Source,
        kind: Kind,
        name: &'static str,
        address: u32,
    ) -> Result<(), String> {
        let bits = self.read(source, kind, name)?;
        self.write(address, bits);
        Ok(())
    }
    fn optional(
        &mut self,
        source: &mut impl Source,
        kind: Kind,
        name: &'static str,
        address: u32,
    ) -> Result<(), String> {
        if self.read(source, Kind::Has, name)? != 0 {
            self.copy(source, kind, name, address)?;
        }
        Ok(())
    }
    /// Apply the original field reads, arithmetic and ordered global writes.
    /// Stops at a recorded final refresh request; no graphics result is implied.
    /// exe: FUN_00427bf0
    pub fn apply(&mut self, source: &mut impl Source) -> Result<(), String> {
        for (name, address) in [
            ("headlight_rays", 0x6187bc),
            ("flares", 0x6187c0),
            ("shadow_size", 0x6187b8),
            ("shadows", 0x6187b4),
        ] {
            self.copy(source, Kind::Int, name, address)?;
        }
        self.copy(source, Kind::Float, "shadow_detail", 0x65aed0)?;
        self.copy(source, Kind::Int, "texture_size", 0x6188b4)?;
        self.copy(source, Kind::Float, "object_detail", 0x65c424)?;
        self.copy(source, Kind::Float, "object_detail", 0x60c8c8)?;
        self.copy(source, Kind::Float, "object_detail_amp", 0x6188c4)?;
        self.copy(source, Kind::Int, "texture_format", 0x6188b8)?;
        if self.read(source, Kind::Has, "video_gamma")? != 0 {
            let gamma = f32::from_bits(self.read(source, Kind::Float, "video_gamma")?);
            self.write(
                0x617728,
                X87::from_f32(1.0)
                    .div(X87::from_f32(gamma), PC, RC)
                    .to_f32(PC, RC)
                    .to_bits(),
            );
        }
        self.copy(source, Kind::Float, "particle_density", 0x618948)?;
        self.copy(source, Kind::Int, "skidmark_max", 0x618bf4)?;
        let quality = f32::from_bits(self.read(source, Kind::Float, "texture_save_quality")?);
        // 427f78 fmul [5f09c8]; __ftol returns an i64, only EAX is stored.
        self.write(
            0x6188b0,
            X87::from_f32(quality)
                .mul(X87::from_f32(100.0), PC, RC)
                .to_i64(Rounding::Zero) as u32,
        );
        for (name, address) in [
            ("external_damage", 0x611358),
            ("internal_damage", 0x61135c),
            ("deformation", 0x611360),
        ] {
            self.copy(source, Kind::Float, name, address)?;
        }
        for (name, address) in [
            ("mem_vertex_max", 0x618d4c),
            ("mem_vertex_min", 0x618d50),
            ("mem_texture_max", 0x618d54),
            ("mem_texture_min", 0x618d58),
            ("mem_instance_max", 0x618d5c),
            ("mem_instance_min", 0x618d60),
        ] {
            self.copy(source, Kind::Int, name, address)?;
        }
        if self.read(source, Kind::Has, "mem_sound_max")? != 0 {
            self.copy(source, Kind::Int, "mem_sound_max", 0x618d64)?;
            self.copy(source, Kind::Int, "mem_sound_min", 0x618d68)?;
        }
        self.copy(source, Kind::Int, "resource_loadrate", 0x618d6c)?;
        self.copy(source, Kind::Int, "ForceFeedBack", 0x777448)?;
        for (name, address, scale) in [
            ("FFB_strength", 0x61ac40, 0x461c4000),
            ("FFB_strength_emulated", 0x61ab14, 0x358637bd),
        ] {
            let value = f32::from_bits(self.read(source, Kind::Float, name)?);
            self.write(
                address,
                X87::from_f32(value)
                    .mul(X87::from_f32(f32::from_bits(scale)), PC, RC)
                    .to_f32(PC, RC)
                    .to_bits(),
            );
        }
        for (name, address) in [
            ("engine_inertia_factor", 0x60e028),
            ("wheel_gndfeedback_factor", 0x60e020),
            ("wheel_brake_factor", 0x60e024),
        ] {
            self.optional(source, Kind::Float, name, address)?;
        }
        self.optional(source, Kind::Int, "MouseHelp", 0x63c83c)?;
        for (name, address) in [
            ("steerhelp_turn", 0x63c7a8),
            ("esp_steer_scale", 0x63c7ac),
            ("esp_angvel_scale", 0x60e050),
            ("esp_yaw_allowed", 0x60e054),
            ("esp_brake_level", 0x60e058),
            ("esp_power_level", 0x60e05c),
        ] {
            self.optional(source, Kind::Float, name, address)?;
        }
        if self.read(source, Kind::Has, "head_move_steer")? != 0 {
            for (name, address) in [
                ("head_move_steer", 0x60e014),
                ("head_move_vel", 0x60e018),
                ("head_move_acc", 0x60e01c),
            ] {
                self.copy(source, Kind::Float, name, address)?;
            }
        }
        self.events.push(Event::Refresh { address: 0x4fc590 });
        Ok(())
    }
}

impl Settings {
    /// Original refresh's absent-backend branch performs no handle release,
    /// allocation or global write, including when old handles are retained.
    /// The present-backend virtual calls are not implemented here.
    /// exe: FUN_004fc590
    pub fn finish_refresh(&mut self, backend_present: bool) -> Result<(), String> {
        if backend_present {
            Err("GraphicsRefreshBoundary: FUN_004fc590 present-backend virtual slot0x134 is not installed".into())
        } else {
            Ok(())
        }
    }
}

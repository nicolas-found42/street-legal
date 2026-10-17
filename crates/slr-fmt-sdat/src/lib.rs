//! `SDAT` save / configuration containers (`save/game/options`, `save/controls/*`,
//! per-vehicle save files such as `cars/racers/ROC_cars/ROC_*`, plus the numbered
//! JPEG companions beside them).
//!
//! The header is `"SDAT" u32 version(=0x00020100) u32 type`, where `type` is either a
//! four-character tag (`CTRL`) or a magic number (`0xFEDCBA98` options,
//! `0x88664422` vehicle). Bodies are little-endian records written by the script
//! `save` methods through the exe's `File` natives and read back by the matching
//! `load` methods:
//!
//! - options (`0xFEDCBA98`): `java.util.Config.saveConfig` / `loadConfig`
//!   (`system/Scripts/util/Config.class`): `(id, version)` then one `u32`/`f32`
//!   word per field, in script order (stock build: 37 fields, no `windowed` on
//!   disk; modded `Config.class`: 42 fields).
//! - controls (`CTRL`): `java.io.ControlSet.save` / `load`
//!   (`system/Scripts/io/ControlSet.class`): `(id, version)`, device-name
//!   strings, `u32` control count, per-control structs, the `vasp` vector.
//! - vehicle (`0x88664422`): `java.game.Vehicle.save` / `load`
//!   (`sl/Scripts/game/Vehicle.class`): `(id, version)`, the part-record run
//!   (`Part.save` recursion, `parts/scripts/Part.class`), the JPEG-slot run,
//!   the resource (RPK path) table, and a trailing `u32`.
//!
//! The numbered companions (`<basename>.<i>`, exe store `FUN_00484fa0` formats
//! them with `sprintf "%s.%d"`, refs `0x485141` / `0x4850f6`) are plain JFIF
//! JPEG screenshots, one per JPEG slot; [`is_jpeg`] / [`companion_index`]
//! cover them.

use slr_fmt_core::reader::Reader;
use slr_fmt_core::{FormatError, Result};

pub const VERSION: u32 = 0x0002_0100;

/// Options magic, written by `Config.saveConfig`
/// (`system/Scripts/util/Config.class` body 111); transport `File.write(int)`
/// native `FUN_00485b20` / `File.write(float)` `FUN_00485bc0`.
///
/// exe: FUN_00484fa0
pub const TYPE_OPTIONS: u32 = 0xFEDC_BA98; // options magic
/// Vehicle magic, written by `Vehicle.save`
/// (`sl/Scripts/game/Vehicle.class` body 44); scalars via the same `File`
/// natives, parts via `Part.save` recursion.
///
/// exe: FUN_00484fa0
pub const TYPE_VEHICLE: u32 = 0x8866_4422; // vehicle magic
/// `"CTRL"` as a little-endian `u32`; `ControlSet.save` / `load`
/// (`system/Scripts/io/ControlSet.class`); strings via `File.write(String)`
/// (`u32 len` + bytes incl. NUL) / `File.readString`.
pub const TYPE_CONTROLS: u32 = 0x4C52_5443; // "CTRL"
/// One body word: `false` = int, `true` = float (the `File.readInt` /
/// `File.readFloat` pair).
///
/// exe: FUN_00485b70
/// exe: FUN_00485c10
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Word {
    I(u32),
    F(f32),
}

impl Word {
    /// Read one body word: `u32` int or `f32` float, matching what the
    /// script-ordered writers (`File.write(int)` `FUN_00485b20` /
    /// `File.write(float)` `FUN_00485bc0`) emit.
    fn read(r: &mut Reader<'_>, is_float: bool) -> Result<Word> {
        if is_float {
            Ok(Word::F(r.f32()?))
        } else {
            Ok(Word::I(r.u32()?))
        }
    }

    fn emit(&self, out: &mut Vec<u8>) {
        match *self {
            Word::I(v) => out.extend_from_slice(&v.to_le_bytes()),
            Word::F(v) => out.extend_from_slice(&v.to_le_bytes()),
        }
    }
}

/// Options body: `(save_id, save_version)`, one word per script field in
/// `saveConfig` order, then the trailing words the scripts do not write
/// (`[0, 0, 8]` on both corpus files, also present on the `CTRL` file).
/// The stock `Config.class` writes 37 fields (`x, y, depth, gamma, texsize,
/// shadowsize, shadow_detail, shadows, ext_viewrange, int_viewrange, flares,
/// headlight, particle, objdetail, objdetailamp, transmission, steerhelp,
/// asr, abs, dmgmult, deformation, intdamage, traffic, ped, mouse, vol_fx,
/// vol_music, vol_eng, metric, gps, ffb, ffb_emu, snd3d, sndmix, headsteer,
/// headvel, headacc`); the modded one writes 42 (adds `cameras_viewrange,
/// mirror_viewrange, mirror, BgScene, skidmark_max, refreshrate, loadrate`
/// and renames the viewranges). `layout_floats` selects the variant.
#[derive(Debug, Clone, PartialEq)]
pub struct OptionsBody {
    pub save_id: u32,
    pub save_version: u32,
    pub words: Vec<Word>,
    pub footer: Vec<u32>,
}

/// Stock `saveConfig` field float-flags (37 entries, `windowed` not on disk).
pub const STOCK_OPTIONS_FLOATS: [bool; 37] = [
    false, false, false, true, false, false, true, false, true, true, false, false, true, true,
    true, false, true, true, true, true, true, true, true, true, true, true, true, true, false,
    false, true, true, false, false, true, true, true,
];

/// Modded `saveConfig` field float-flags (42 entries).
pub const REF_OPTIONS_FLOATS: [bool; 42] = [
    false, false, false, true, false, false, true, false, true, true, false, false, false, false,
    true, false, false, false, true, true, false, true, true, true, true, true, true, true, true,
    true, true, true, true, false, false, true, true, false, false, true, true, true,
];

impl OptionsBody {
    pub fn parse(r: &mut Reader<'_>, layout_floats: &[bool]) -> Result<OptionsBody> {
        let save_id = r.u32()?;
        let save_version = r.u32()?;
        let mut words = Vec::with_capacity(layout_floats.len());
        for &is_float in layout_floats {
            words.push(Word::read(r, is_float)?);
        }
        let mut footer = Vec::new();
        while r.remaining() >= 4 {
            footer.push(r.u32()?);
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "sdat options body",
                detail: format!("{} trailing bytes", r.remaining()),
            });
        }
        Ok(OptionsBody {
            save_id,
            save_version,
            words,
            footer,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + 4 * (self.words.len() + self.footer.len()));
        out.extend_from_slice(&self.save_id.to_le_bytes());
        out.extend_from_slice(&self.save_version.to_le_bytes());
        for w in &self.words {
            w.emit(&mut out);
        }
        for v in &self.footer {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// One control mapping (`ControlSet.save` / `load`): four `u32` ids then five
/// `f32` ranges. The scripts keep struct-of-arrays (`group[]`, `vaxisID[]`,
/// ...); the file stores array-of-structs (exe `File.write(int)` /
/// `write(float)` per element in the `op 0x20`-indexed loops).
#[derive(Debug, Clone, PartialEq)]
pub struct Control {
    pub group: u32,
    pub vaxis_id: u32,
    pub device_id: u32,
    pub axis_id: u32,
    pub from_min: f32,
    pub from_max: f32,
    pub to_min: f32,
    pub to_max: f32,
    pub dead_zone: f32,
}

/// One `vasp` (`VirtualAxisSmoothProperties`) record: `u32` id then five
/// `f32` smoothing parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct Vasp {
    pub id: u32,
    pub params: [f32; 5],
}

/// Controls body: `(save_id, save_version)`, device-name strings, `u32`
/// control count, the structs, the `vasp` vector (`u32` count + records),
/// then the shared `[0, 0, 8]` footer.
#[derive(Debug, Clone, PartialEq)]
pub struct ControlsBody {
    pub save_id: u32,
    pub save_version: u32,
    pub device_names: Vec<String>,
    pub controls: Vec<Control>,
    pub vasp: Vec<Vasp>,
    pub footer: Vec<u32>,
}

fn write_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(&(s.len() as u32 + 1).to_le_bytes());
    out.extend_from_slice(s.as_bytes());
    out.push(0);
}

impl ControlsBody {
    pub fn parse(r: &mut Reader<'_>) -> Result<ControlsBody> {
        let save_id = r.u32()?;
        let save_version = r.u32()?;
        // Device names: the scripts loop `deviceName.length` times; the
        // corpus file holds two (`SysKeyboard`, `SysMouse`). The count is
        // implicit, so scan greedily: a name is `u32 len` + NUL-terminated
        // Latin-1 followed by a sane control count whose structs, vasp and
        // footer fit exactly.
        let mut device_names = Vec::new();
        loop {
            let save = r.pos;
            if r.remaining() < 8 {
                return Err(FormatError::Inconsistent {
                    what: "sdat controls body",
                    detail: "missing control count".into(),
                });
            }
            let len = u32::from_le_bytes(r.data[r.pos..r.pos + 4].try_into().unwrap()) as usize;
            if !(1..=64).contains(&len) || r.remaining() < 4 + len + 4 {
                r.pos = save;
                break;
            }
            let bytes = &r.data[r.pos + 4..r.pos + 4 + len];
            if bytes.last() != Some(&0) || bytes[..len - 1].iter().any(|&b| b == 0 || b > 0x7f) {
                r.pos = save;
                break;
            }
            let ncontrols =
                u32::from_le_bytes(r.data[r.pos + 4 + len..r.pos + 8 + len].try_into().unwrap())
                    as usize;
            if ncontrols == 0 || ncontrols > 4096 {
                r.pos = save;
                break;
            }
            r.pos += 4;
            let bytes = r.bytes(len)?;
            device_names.push(String::from_utf8_lossy(&bytes[..len - 1]).into_owned());
        }
        if device_names.is_empty() {
            return Err(FormatError::Inconsistent {
                what: "sdat controls body",
                detail: "no device names".into(),
            });
        }
        let ncontrols = r.u32()? as usize;
        let mut controls = Vec::with_capacity(ncontrols);
        for _ in 0..ncontrols {
            controls.push(Control {
                group: r.u32()?,
                vaxis_id: r.u32()?,
                device_id: r.u32()?,
                axis_id: r.u32()?,
                from_min: r.f32()?,
                from_max: r.f32()?,
                to_min: r.f32()?,
                to_max: r.f32()?,
                dead_zone: r.f32()?,
            });
        }
        let nvasp = r.u32()? as usize;
        if nvasp > 1024 {
            return Err(FormatError::Inconsistent {
                what: "sdat controls vasp",
                detail: format!("count {nvasp}"),
            });
        }
        let mut vasp = Vec::with_capacity(nvasp);
        for _ in 0..nvasp {
            let id = r.u32()?;
            let mut params = [0.0f32; 5];
            for p in &mut params {
                *p = r.f32()?;
            }
            vasp.push(Vasp { id, params });
        }
        let mut footer = Vec::new();
        while r.remaining() >= 4 {
            footer.push(r.u32()?);
        }
        if r.remaining() != 0 {
            return Err(FormatError::Inconsistent {
                what: "sdat controls body",
                detail: format!("{} trailing bytes", r.remaining()),
            });
        }
        Ok(ControlsBody {
            save_id,
            save_version,
            device_names,
            controls,
            vasp,
            footer,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.save_id.to_le_bytes());
        out.extend_from_slice(&self.save_version.to_le_bytes());
        for s in &self.device_names {
            write_string(&mut out, s);
        }
        out.extend_from_slice(&(self.controls.len() as u32).to_le_bytes());
        for c in &self.controls {
            for v in [c.group, c.vaxis_id, c.device_id, c.axis_id] {
                out.extend_from_slice(&v.to_le_bytes());
            }
            for v in [c.from_min, c.from_max, c.to_min, c.to_max, c.dead_zone] {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&(self.vasp.len() as u32).to_le_bytes());
        for v in &self.vasp {
            out.extend_from_slice(&v.id.to_le_bytes());
            for p in v.params {
                out.extend_from_slice(&p.to_le_bytes());
            }
        }
        for v in &self.footer {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out
    }
}

/// One JPEG-slot record: the store (`FUN_00484fa0`) writes `[index, 7, 0]`
/// per saved screenshot slot; the `<basename>.<index>` companion holds the
/// JFIF bytes (`sprintf "%s.%d"`, refs `0x485141` / `0x4850f6`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegSlot {
    pub index: u32,
}

/// One resource-table entry: `(index, len, NUL-terminated RPK path)`.
/// `resID` words in the part run index this table (`File.write(ResourceRef)`
/// `FUN_00485c60` / `File.readResID` `FUN_00485ea0` = plain `u32`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceEntry {
    pub index: u32,
    pub path: String,
}

/// Vehicle body: `(save_id, save_version)`, the part-record run
/// (`Part.save` recursion; interior slot grammar still open, kept as explicit
/// opaque bytes with exact accounting), the JPEG-slot run, the resource
/// table (`u32` count + entries), and the trailing `u32`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VehicleBody {
    pub save_id: u32,
    pub save_version: u32,
    pub part_bytes: Vec<u8>,
    pub jpeg_slots: Vec<JpegSlot>,
    pub resources: Vec<ResourceEntry>,
    pub trailer: u32,
}

impl VehicleBody {
    pub fn parse(r: &mut Reader<'_>) -> Result<VehicleBody> {
        let save_id = r.u32()?;
        let save_version = r.u32()?;
        // Anchor on the resource table, parsed backwards from EOF: entries
        // are `(u32 index, u32 len, bytes+NUL)` with 1-based dense indices,
        // preceded by the `u32` count; a final `u32` trailer closes the file.
        // Everything between the head and the JPEG-slot run is the part run.
        let data = r.data;
        let file_end = data.len();
        if file_end < r.pos + 16 {
            return Err(FormatError::Inconsistent {
                what: "sdat vehicle body",
                detail: "too short".into(),
            });
        }
        let trailer = u32::from_le_bytes(data[file_end - 4..].try_into().unwrap());
        let mut pos = file_end - 4;
        let mut resources_rev = Vec::new();
        loop {
            if pos < r.pos + 12 {
                return Err(FormatError::Inconsistent {
                    what: "sdat vehicle resources",
                    detail: "table overruns head".into(),
                });
            }
            let end = pos;
            // String bytes run back to the preceding NUL-adjacent length.
            // Walk back over the NUL-terminated path first.
            if data[end - 1] != 0 {
                break;
            }
            let mut s = end - 1;
            while s > r.pos + 8 && data[s - 1] != 0 && data[s - 1] >= 0x20 && data[s - 1] < 0x7f {
                s -= 1;
            }
            if s < r.pos + 8 {
                return Err(FormatError::Inconsistent {
                    what: "sdat vehicle resources",
                    detail: "path overruns head".into(),
                });
            }
            let len = u32::from_le_bytes(data[s - 4..s].try_into().unwrap()) as usize;
            let index = u32::from_le_bytes(data[s - 8..s - 4].try_into().unwrap());
            if len != end - s || len == 0 || len > 300 {
                break;
            }
            resources_rev.push(ResourceEntry {
                index,
                path: String::from_utf8_lossy(&data[s..end - 1]).into_owned(),
            });
            pos = s - 8;
            if pos >= 4 {
                let maybe_count =
                    u32::from_le_bytes(data[pos - 4..pos].try_into().unwrap()) as usize;
                if maybe_count == resources_rev.len() {
                    // Dense 1-based check before accepting.
                    let mut idxs: Vec<u32> = resources_rev.iter().map(|e| e.index).collect();
                    idxs.sort_unstable();
                    if idxs == (1..=resources_rev.len() as u32).collect::<Vec<_>>() {
                        pos -= 4;
                        break;
                    }
                }
            }
        }
        if resources_rev.is_empty() {
            return Err(FormatError::Inconsistent {
                what: "sdat vehicle resources",
                detail: "no table".into(),
            });
        }
        resources_rev.reverse();
        let resources = resources_rev;
        // JPEG-slot run: `[index, 7, 0]` triples immediately before the table.
        // (Byte offsets drift through the string-bearing part run, so no
        // alignment is assumed; `u32::from_le_bytes` needs none.)
        let mut words_end = pos;
        let mut slots_rev = Vec::new();
        while words_end >= r.pos + 12 {
            let a = u32::from_le_bytes(data[words_end - 12..words_end - 8].try_into().unwrap());
            let b = u32::from_le_bytes(data[words_end - 8..words_end - 4].try_into().unwrap());
            let c = u32::from_le_bytes(data[words_end - 4..words_end].try_into().unwrap());
            if b != 7 || c != 0 || a == 0 || a > 40 {
                break;
            }
            slots_rev.push(JpegSlot { index: a });
            words_end -= 12;
        }
        slots_rev.reverse();
        let part_bytes = data[r.pos..words_end].to_vec();
        r.pos = file_end;
        Ok(VehicleBody {
            save_id,
            save_version,
            part_bytes,
            jpeg_slots: slots_rev,
            resources,
            trailer,
        })
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(&self.save_id.to_le_bytes());
        out.extend_from_slice(&self.save_version.to_le_bytes());
        out.extend_from_slice(&self.part_bytes);
        for s in &self.jpeg_slots {
            out.extend_from_slice(&s.index.to_le_bytes());
            out.extend_from_slice(&7u32.to_le_bytes());
            out.extend_from_slice(&0u32.to_le_bytes());
        }
        out.extend_from_slice(&(self.resources.len() as u32).to_le_bytes());
        for (i, e) in self.resources.iter().enumerate() {
            debug_assert_eq!(e.index as usize, i + 1);
            out.extend_from_slice(&e.index.to_le_bytes());
            out.extend_from_slice(&(e.path.len() as u32 + 1).to_le_bytes());
            out.extend_from_slice(e.path.as_bytes());
            out.push(0);
        }
        out.extend_from_slice(&self.trailer.to_le_bytes());
        out
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Body {
    Options(OptionsBody),
    Controls(ControlsBody),
    Vehicle(VehicleBody),
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Options,
    Controls,
    Vehicle,
    Other(u32),
}

#[derive(Debug, Clone)]
pub struct Sdat {
    pub kind: Kind,
    pub type_word: u32,
    pub body: Body,
}

/// Store dispatches on the type word; load reads the header the same way
/// (`SDAT`, version `0x00020100`, then the per-type body). `layout_floats`
/// selects the options variant (`STOCK_OPTIONS_FLOATS` /
/// `REF_OPTIONS_FLOATS`); pass `None` for non-options files.
///
/// exe: FUN_00484fa0
/// exe: 0x00485870
pub fn parse(data: &[u8], layout_floats: Option<&[bool]>) -> Result<Sdat> {
    let mut r = Reader::new(data);
    let magic = r.bytes(4)?;
    if magic != b"SDAT" {
        return Err(FormatError::BadMagic {
            expected: "SDAT",
            found: String::from_utf8_lossy(magic).into_owned(),
        });
    }
    let version = r.u32()?;
    if version != VERSION {
        return Err(FormatError::UnsupportedVersion(version));
    }
    let type_word = r.u32()?;
    let kind = match type_word {
        TYPE_OPTIONS => Kind::Options,
        TYPE_CONTROLS => Kind::Controls,
        TYPE_VEHICLE => Kind::Vehicle,
        other => Kind::Other(other),
    };
    let mut body_reader = Reader::at(data, r.pos);
    let body = match kind {
        Kind::Options => {
            let layout = layout_floats.unwrap_or(&STOCK_OPTIONS_FLOATS);
            Body::Options(OptionsBody::parse(&mut body_reader, layout)?)
        }
        Kind::Controls => Body::Controls(ControlsBody::parse(&mut body_reader)?),
        Kind::Vehicle => Body::Vehicle(VehicleBody::parse(&mut body_reader)?),
        Kind::Other(_) => Body::Raw(data[r.pos..].to_vec()),
    };
    if !matches!(kind, Kind::Other(_)) && body_reader.remaining() != 0 {
        return Err(FormatError::Inconsistent {
            what: "sdat body",
            detail: format!("{} trailing bytes", body_reader.remaining()),
        });
    }
    Ok(Sdat {
        kind,
        type_word,
        body,
    })
}

pub fn encode(sdat: &Sdat) -> Vec<u8> {
    let mut out = Vec::with_capacity(12);
    out.extend_from_slice(b"SDAT");
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&sdat.type_word.to_le_bytes());
    match &sdat.body {
        Body::Options(b) => out.extend_from_slice(&b.encode()),
        Body::Controls(b) => out.extend_from_slice(&b.encode()),
        Body::Vehicle(b) => out.extend_from_slice(&b.encode()),
        Body::Raw(b) => out.extend_from_slice(b),
    }
    out
}

/// JPEG companions: `<basename>.<i>` next to the SDAT (exe store
/// `FUN_00484fa0` formats them with `sprintf "%s.%d"`, refs `0x485141` /
/// `0x4850f6`; per-part screenshots saved by `FUN_0048daa0` / loaded by
/// `FUN_0048da30`). They are plain JFIF files, not SDAT.
pub fn is_jpeg(data: &[u8]) -> bool {
    data.len() >= 4 && data[0] == 0xFF && data[1] == 0xD8 && data[2] == 0xFF
}

/// Split `ROC_GT2_1.16` into (`ROC_GT2_1`, 16). Returns `None` when the name
/// has no numeric companion suffix.
pub fn companion_index(name: &str) -> Option<(&str, u32)> {
    let (base, ext) = name.rsplit_once('.')?;
    if ext.is_empty() || !ext.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if base.is_empty() || base.ends_with('.') {
        return None;
    }
    ext.parse::<u32>().ok().map(|i| (base, i))
}

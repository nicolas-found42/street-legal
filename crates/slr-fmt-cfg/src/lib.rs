//! `.cfg` vehicle / part / click / camera configuration files.
//!
//! These are plain text, one directive per line: a keyword followed by tab- or
//! space-separated arguments. `#`, `;` and `//` start comments (a `;0x...` suffix on a
//! line carries the importer's resource id). The keyword vocabulary is the one registered
//! in the native keyword table at `.rdata` 0x0060e2xx (`mass`, `enginepower`, ...).

use slr_fmt_core::Result;

#[derive(Debug, Clone)]
pub struct Directive {
    pub line: usize,
    pub name: String,
    pub args: Vec<String>,
}

impl Directive {
    pub fn f32s(&self) -> Vec<f32> {
        self.args
            .iter()
            .filter_map(|a| a.parse::<f32>().ok())
            .collect()
    }

    pub fn f32_at(&self, i: usize) -> Option<f32> {
        self.args.get(i).and_then(|a| a.parse().ok())
    }

    /// Parse an argument as a `0x...` or decimal u32 resource id.
    pub fn id_at(&self, i: usize) -> Option<u32> {
        let a = self.args.get(i)?;
        if let Some(h) = a.strip_prefix("0x").or_else(|| a.strip_prefix("0X")) {
            u32::from_str_radix(h, 16).ok()
        } else {
            a.parse().ok()
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Cfg {
    pub directives: Vec<Directive>,
}

impl Cfg {
    pub fn parse(text: &str) -> Cfg {
        let mut directives = Vec::new();
        for (i, raw) in text.lines().enumerate() {
            let mut line = raw;
            for marker in ["//", "#", ";"] {
                if let Some(p) = line.find(marker) {
                    line = &line[..p];
                }
            }
            let mut it = line.split_whitespace();
            let Some(name) = it.next() else { continue };
            directives.push(Directive {
                line: i + 1,
                name: name.to_ascii_lowercase(),
                args: it.map(str::to_owned).collect(),
            });
        }
        Cfg { directives }
    }

    pub fn open(path: impl AsRef<std::path::Path>) -> Result<Cfg> {
        let bytes = std::fs::read(path)?;
        // Files are Latin-1 / ASCII; map bytes 1:1.
        let text: String = bytes.iter().map(|&b| b as char).collect();
        Ok(Cfg::parse(&text))
    }

    pub fn first(&self, name: &str) -> Option<&Directive> {
        self.directives.iter().find(|d| d.name == name)
    }

    pub fn all<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Directive> + 'a {
        self.directives.iter().filter(move |d| d.name == name)
    }
}

#[derive(Debug, Clone)]
pub struct Wheel {
    pub position: [f32; 3],
    pub radius: f32,
    pub wheel_mass: f32,
    /// `[steered, driven, braked, handbrake]` as the four trailing flags of the `wheel` line.
    pub flags: [f32; 4],
    /// The three numbers of the preceding `spring` line (semantics undecoded).
    pub spring: [f32; 3],
}

impl Wheel {
    pub fn steered(&self) -> bool {
        self.flags[0] > 0.5
    }
    pub fn driven(&self) -> bool {
        self.flags[1] > 0.5
    }
    pub fn handbraked(&self) -> bool {
        self.flags[3] > 0.5
    }
}

#[derive(Debug, Clone)]
pub struct Slot {
    pub position: [f32; 3],
    pub id: u32,
    pub line: usize,
}

/// Typed view of a `_main_*.cfg` vehicle definition.
#[derive(Debug, Clone)]
pub struct VehicleCfg {
    pub body_mass: f32,
    pub wheels: Vec<Wheel>,
    pub slots: Vec<Slot>,
    /// Forward gear ratios, first to last.
    pub gear_ratios: Vec<f32>,
    pub reverse_ratio: f32,
    pub rearend_ratio: f32,
    /// The six floats parsed by the native `enginepower` handler (handler 0x00443f90).
    pub enginepower: [f32; 6],
    pub brakepower: [f32; 2],
    pub maxsteer: [f32; 2],
    pub steerhelp: Vec<f32>,
    pub cameras: usize,
    pub category: Option<u32>,
}

impl VehicleCfg {
    pub fn from_cfg(c: &Cfg) -> Option<VehicleCfg> {
        let ratios = c.first("ratios")?.f32s();
        let n = *ratios.first()? as usize;
        if ratios.len() < n + 2 {
            return None;
        }
        let gear_ratios = ratios[1..=n].to_vec();
        let reverse_ratio = ratios[n + 1];
        let ep = c.first("enginepower")?.f32s();
        let enginepower: [f32; 6] = ep.get(..6)?.try_into().ok()?;
        let bp = c.first("brakepower")?.f32s();
        let ms = c.first("maxsteer")?.f32s();

        let mut body_mass = 0.0;
        let mut wheels = Vec::new();
        let mut pending_type: Option<(f32, f32)> = None;
        let mut pending_spring = [0.0f32; 3];
        for d in &c.directives {
            match d.name.as_str() {
                "body" => {
                    if let Some(m) = d.f32_at(6) {
                        body_mass += m;
                    }
                }
                "type" => {
                    // type <mass> sphere <radius>
                    if let (Some(m), Some(r)) = (d.f32_at(0), d.f32_at(2)) {
                        pending_type = Some((m, r));
                    }
                }
                "spring" => {
                    let v = d.f32s();
                    if v.len() >= 3 {
                        pending_spring = [v[0], v[1], v[2]];
                    }
                }
                "wheel" => {
                    let v = d.f32s();
                    if v.len() >= 10 {
                        let (wheel_mass, radius) = pending_type.unwrap_or((10.0, 0.315));
                        wheels.push(Wheel {
                            position: [v[0], v[1], v[2]],
                            radius,
                            wheel_mass,
                            flags: [v[6], v[7], v[8], v[9]],
                            spring: pending_spring,
                        });
                    }
                }
                _ => {}
            }
        }
        let slots = c
            .all("slot")
            .filter_map(|d| {
                let v = d.f32s();
                if v.len() >= 7 {
                    Some(Slot {
                        position: [v[0], v[1], v[2]],
                        id: v[6] as u32,
                        line: d.line,
                    })
                } else {
                    None
                }
            })
            .collect();
        Some(VehicleCfg {
            body_mass,
            wheels,
            slots,
            gear_ratios,
            reverse_ratio,
            rearend_ratio: c.first("rearendratio")?.f32_at(0)?,
            enginepower,
            brakepower: [*bp.first()?, *bp.get(1)?],
            maxsteer: [*ms.first()?, *ms.get(1)?],
            steerhelp: c.first("steerhelp").map(|d| d.f32s()).unwrap_or_default(),
            cameras: c.all("camera").count(),
            category: c.first("category").and_then(|d| d.id_at(0)),
        })
    }
}

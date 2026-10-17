//! Content catalog built from the reference install: cars from `cars/racers/*/scripts/_main_*.cfg`,
//! tyres from `parts/wheels/tyres`, engines from `parts/engines/*/scripts/*_block.cfg`.

use crate::engine::{stock_engines, EngineSpec};
use crate::tyre::TyreSpec;
use slr_formats::cfg::{Cfg, VehicleCfg};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct CarModel {
    /// `Maker/Model`, e.g. `Baiern/CoupeSport`.
    pub id: String,
    pub maker: String,
    pub name: String,
    pub cfg: VehicleCfg,
    pub stock_engine: String,
    pub base_price: f32,
    /// Directory holding this maker's `meshes/*.scx`.
    pub mesh_dir: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct Catalog {
    pub cars: Vec<CarModel>,
    pub engines: Vec<EngineSpec>,
    pub tyres: Vec<TyreSpec>,
    pub root: Option<PathBuf>,
}

/// Which stock engine block a car carries, inferred from the part-name vocabulary
/// (`Emer_Nonus_*` pistons belong to the Nonus cars, `Duhen_D18V` to the SunStrip 1.8, ...).
pub fn stock_engine_for(maker: &str, model: &str) -> &'static str {
    match (maker, model) {
        ("Baiern", m) if m.starts_with("Devil") => "Baiern_Devils_6SFi_3_6",
        ("Baiern", _) => "Baiern_Kraftwerk_2_5",
        ("Duhen", m) if m.contains("1_5") => "Duhen_D15V",
        ("Duhen", m) if m.contains("1_8") => "Duhen_D18V",
        ("Duhen", _) => "Duhen_D22V",
        ("Einvagen", m) if m.contains("GTA") => "Einvagen_PerPro_GTA",
        ("Einvagen", _) => "Einvagen_110_GT",
        ("Enula", _) => "Emer_StreetGT",
        ("Focer", _) => "Shimutshibu_RC_B94S200",
        ("MC", _) => "MC_block",
        ("Nonus", m) if m.contains("MotorSport") => "Emer_MotorSport_GT2",
        ("Nonus", _) => "Emer_StreetGT",
        ("Prime", _) => "104er_engine_block",
        ("SuperDuty", _) => "SuperDuty_5_4",
        _ => "Baiern_Kraftwerk_2_5",
    }
}

impl Catalog {
    pub fn load(root: &Path) -> Result<Catalog, slr_formats::FormatError> {
        let engines = stock_engines();
        let mut cars = Vec::new();
        let racers = root.join("cars/racers");
        let mut makers: Vec<_> = std::fs::read_dir(&racers)?
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().ends_with("_data"))
            .collect();
        makers.sort_by_key(|e| e.file_name());
        for m in makers {
            let maker = m
                .file_name()
                .to_string_lossy()
                .trim_end_matches("_data")
                .to_string();
            let scripts = m.path().join("scripts");
            let mut cfgs: Vec<_> = std::fs::read_dir(&scripts)?
                .filter_map(|e| e.ok())
                .filter(|e| {
                    let n = e.file_name().to_string_lossy().into_owned();
                    n.starts_with("_main_") && n.ends_with(".cfg")
                })
                .collect();
            cfgs.sort_by_key(|e| e.file_name());
            for c in cfgs {
                let stem = c
                    .file_name()
                    .to_string_lossy()
                    .trim_start_matches("_main_")
                    .trim_end_matches(".cfg")
                    .to_string();
                let cfg = Cfg::open(c.path())?;
                let Some(v) = VehicleCfg::from_cfg(&cfg) else {
                    continue;
                };
                let stock_engine = stock_engine_for(&maker, &stem).to_string();
                let eng = engines
                    .iter()
                    .find(|e| e.id == stock_engine)
                    .expect("stock engine table");
                let base_price =
                    (2500.0 + eng.peak_power_kw() * 55.0 + (1400.0 - v.body_mass).max(0.0) * 4.0)
                        .round();
                cars.push(CarModel {
                    id: format!("{maker}/{stem}"),
                    name: stem.replace('_', " "),
                    maker: maker.clone(),
                    cfg: v,
                    stock_engine,
                    base_price,
                    mesh_dir: Some(m.path().join("meshes")),
                });
            }
        }
        let mut tyres: Vec<TyreSpec> = std::fs::read_dir(root.join("parts/wheels/tyres/scripts"))?
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                n.strip_suffix(".cfg").and_then(TyreSpec::from_part_name)
            })
            .collect();
        tyres.sort_by(|a, b| {
            (a.width_mm, a.rim_in)
                .partial_cmp(&(b.width_mm, b.rim_in))
                .unwrap()
        });
        Ok(Catalog {
            cars,
            engines,
            tyres,
            root: Some(root.to_path_buf()),
        })
    }

    /// Small built-in catalog (the Baiern Coupe Sport numbers from the shipped cfg) so the
    /// game logic and its tests run without the reference install.
    pub fn fallback() -> Catalog {
        let cfg = VehicleCfg {
            body_mass: 838.5,
            wheels: (0..4)
                .map(|i| slr_formats::cfg::Wheel {
                    position: [
                        if i % 2 == 0 { -0.709 } else { 0.709 },
                        -0.59,
                        if i < 2 { -1.48 } else { 1.256 },
                    ],
                    radius: 0.315,
                    wheel_mass: 10.0,
                    flags: if i < 2 {
                        [1.0, 0.0, 1.0, 0.0]
                    } else {
                        [0.0, 1.0, 1.0, 1.0]
                    },
                    spring: [0.29, 0.1, 0.05],
                })
                .collect(),
            slots: vec![],
            gear_ratios: vec![4.2, 2.5, 1.3, 1.0, 0.758],
            reverse_ratio: -4.4,
            rearend_ratio: 4.41,
            enginepower: [100.0, 4.0, 0.00006, 0.003, 700.0, 32.0],
            brakepower: [1000.0, 2000.0],
            maxsteer: [0.65, 2.736],
            steerhelp: vec![1.0, 30.0, 0.5, 0.0, 1.57, 1.0],
            cameras: 7,
            category: Some(2),
        };
        let mut cfg2 = cfg.clone();
        cfg2.body_mass = 804.0;
        let tyres = [
            "tyre_195_50_16_7_5",
            "tyre_205_50_16_8_0",
            "tyre_225_45_17_9_0",
            "tyre_245_40_21_9_5",
            "tyre_265_45_17_11_0",
        ]
        .iter()
        .filter_map(|n| TyreSpec::from_part_name(n))
        .collect();
        Catalog {
            cars: vec![
                CarModel {
                    id: "Baiern/CoupeSport".into(),
                    maker: "Baiern".into(),
                    name: "CoupeSport".into(),
                    cfg,
                    stock_engine: "Baiern_Kraftwerk_2_5".into(),
                    base_price: 9000.0,
                    mesh_dir: None,
                },
                CarModel {
                    id: "Baiern/DevilSport".into(),
                    maker: "Baiern".into(),
                    name: "DevilSport".into(),
                    cfg: cfg2,
                    stock_engine: "Baiern_Devils_6SFi_3_6".into(),
                    base_price: 15000.0,
                    mesh_dir: None,
                },
            ],
            engines: stock_engines(),
            tyres,
            root: None,
        }
    }

    pub fn car(&self, id: &str) -> Option<&CarModel> {
        self.cars.iter().find(|c| c.id == id)
    }

    pub fn engine(&self, id: &str) -> Option<&EngineSpec> {
        self.engines.iter().find(|e| e.id == id)
    }

    pub fn tyre(&self, id: &str) -> Option<&TyreSpec> {
        self.tyres.iter().find(|t| t.id == id)
    }

    /// The tyre closest to the stock 205/50 R16.
    pub fn stock_tyre(&self) -> &TyreSpec {
        self.tyre("tyre_205_50_16_8_0").unwrap_or(&self.tyres[0])
    }
}

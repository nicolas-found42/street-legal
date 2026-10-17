//! Persistent game state: the player's garage, money and progress, and every action that
//! changes them (buying, tuning, repairing, entering events), plus JSON save/load.

use crate::catalog::{CarModel, Catalog};
use crate::condition::Condition;
use crate::race::{Phase, Race, RaceKind, DRAG_LENGTH};
use crate::track::Track;
use crate::tyre::TyreSpec;
use crate::vehicle::{Setup, Vehicle};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;

pub const SAVE_VERSION: u32 = 1;
pub const STARTING_MONEY: f32 = 2500.0;

/// Body colours offered by the painter (the `used` colour set from `cars.rpk`).
pub const PAINTS: [(&str, [u8; 3]); 12] = [
    ("black", [24, 24, 26]),
    ("blue", [40, 70, 170]),
    ("brown", [110, 70, 40]),
    ("cyan", [40, 170, 190]),
    ("gold", [200, 165, 60]),
    ("gray", [130, 132, 138]),
    ("green", [45, 130, 70]),
    ("orange", [230, 120, 30]),
    ("purple", [110, 55, 150]),
    ("red", [185, 30, 35]),
    ("white", [235, 235, 232]),
    ("yellow", [235, 205, 40]),
];

#[derive(Debug, Clone, PartialEq)]
pub enum GameError {
    NotEnoughMoney { need: f32, have: f32 },
    NoSuchCar,
    NoSuchPart(String),
    LastCar,
    Locked(String),
    InvalidValue(String),
}

impl std::fmt::Display for GameError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GameError::NotEnoughMoney { need, have } => {
                write!(f, "Not enough money: need ${need:.0}, have ${have:.0}")
            }
            GameError::NoSuchCar => write!(f, "No such car"),
            GameError::NoSuchPart(p) => write!(f, "Unknown part {p}"),
            GameError::LastCar => write!(f, "You cannot sell your only car"),
            GameError::Locked(s) => write!(f, "{s}"),
            GameError::InvalidValue(s) => write!(f, "{s}"),
        }
    }
}
impl std::error::Error for GameError {}

pub type GameResult<T> = Result<T, GameError>;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OwnedCar {
    pub model: String,
    pub setup: Setup,
    pub cond: Condition,
    pub paint: usize,
    pub tune_stage: u8,
    pub has_lsd: bool,
    /// Prestige of the parts bolted on, used for resale value.
    pub invested: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GameState {
    pub version: u32,
    pub money: f32,
    pub reputation: u32,
    pub day: u32,
    pub cars: Vec<OwnedCar>,
    pub active: usize,
    pub races: u32,
    pub wins: u32,
    pub best_times: BTreeMap<String, f32>,
    pub seed: u64,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Axle {
    Front,
    Rear,
    Both,
}

#[derive(Debug, Clone)]
pub struct Listing {
    pub model: String,
    pub condition: Condition,
    pub price: f32,
}

#[derive(Debug, Clone)]
pub struct EventDef {
    pub id: &'static str,
    pub name: &'static str,
    pub kind: RaceKind,
    pub night: bool,
    pub entry_fee: f32,
    pub prize: f32,
    pub rep_gain: u32,
    pub rep_required: u32,
    pub opponent: &'static str,
    pub ai_skill: f32,
}

pub const EVENTS: &[EventDef] = &[
    EventDef {
        id: "drag_rookie",
        name: "Street Drag: Rookie",
        kind: RaceKind::Drag,
        night: false,
        entry_fee: 0.0,
        prize: 350.0,
        rep_gain: 1,
        rep_required: 0,
        opponent: "Duhen/SunStrip_1_5_DVC",
        ai_skill: 0.5,
    },
    EventDef {
        id: "circuit_club",
        name: "Club Circuit Sprint (2 laps)",
        kind: RaceKind::Circuit { laps: 2 },
        night: false,
        entry_fee: 100.0,
        prize: 900.0,
        rep_gain: 2,
        rep_required: 1,
        opponent: "Einvagen/110_GT",
        ai_skill: 0.6,
    },
    EventDef {
        id: "drag_night",
        name: "Midnight Drag",
        kind: RaceKind::Drag,
        night: true,
        entry_fee: 250.0,
        prize: 1400.0,
        rep_gain: 2,
        rep_required: 3,
        opponent: "Baiern/CoupeSport",
        ai_skill: 0.75,
    },
    EventDef {
        id: "circuit_night",
        name: "Night Run (3 laps)",
        kind: RaceKind::Circuit { laps: 3 },
        night: true,
        entry_fee: 400.0,
        prize: 2600.0,
        rep_gain: 3,
        rep_required: 5,
        opponent: "Baiern/DevilSport",
        ai_skill: 0.8,
    },
    EventDef {
        id: "drag_pro",
        name: "Pro Drag Challenge",
        kind: RaceKind::Drag,
        night: false,
        entry_fee: 800.0,
        prize: 4200.0,
        rep_gain: 4,
        rep_required: 8,
        opponent: "Enula/Enula_WRZ",
        ai_skill: 0.9,
    },
    EventDef {
        id: "circuit_champ",
        name: "Championship Final (5 laps)",
        kind: RaceKind::Circuit { laps: 5 },
        night: false,
        entry_fee: 1500.0,
        prize: 9000.0,
        rep_gain: 6,
        rep_required: 12,
        opponent: "Focer/Focer_RC_300",
        ai_skill: 0.93,
    },
];

#[derive(Debug, Clone)]
pub struct RaceOutcome {
    pub position: usize,
    pub won: bool,
    pub time: Option<f32>,
    pub prize: f32,
    pub entry_fee: f32,
    pub rep_gain: u32,
    pub repair_estimate: f32,
    pub new_record: bool,
}

fn lcg(seed: &mut u64) -> f32 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 33) as f32) / (u32::MAX >> 1) as f32
}

impl GameState {
    pub fn new_game(cat: &Catalog, seed: u64) -> GameState {
        let starter = cat
            .cars
            .iter()
            .min_by(|a, b| a.base_price.partial_cmp(&b.base_price).unwrap())
            .expect("catalog has cars");
        let mut car = OwnedCar::stock(cat, starter);
        // The starter is a tired used car, so the garage has something to fix.
        car.cond.tyre_wear = [0.35, 0.3, 0.45, 0.5];
        car.cond.engine = 0.82;
        car.cond.body = [0.12, 0.05, 0.03, 0.06];
        car.cond.brake_wear = 0.3;
        car.cond.odometer_km = 141_000.0;
        GameState {
            version: SAVE_VERSION,
            money: STARTING_MONEY,
            reputation: 0,
            day: 1,
            cars: vec![car],
            active: 0,
            races: 0,
            wins: 0,
            best_times: BTreeMap::new(),
            seed,
        }
    }

    pub fn car(&self) -> &OwnedCar {
        &self.cars[self.active]
    }

    pub fn car_mut(&mut self) -> &mut OwnedCar {
        &mut self.cars[self.active]
    }

    fn pay(&mut self, amount: f32) -> GameResult<()> {
        if self.money + 1e-3 < amount {
            return Err(GameError::NotEnoughMoney {
                need: amount,
                have: self.money,
            });
        }
        self.money -= amount;
        Ok(())
    }

    fn idx(&self, idx: usize) -> GameResult<usize> {
        if idx < self.cars.len() {
            Ok(idx)
        } else {
            Err(GameError::NoSuchCar)
        }
    }

    // ---- car dealer ----

    pub fn buy_new(&mut self, cat: &Catalog, model: &str) -> GameResult<usize> {
        let m = cat.car(model).ok_or(GameError::NoSuchCar)?;
        self.pay(m.base_price)?;
        self.cars.push(OwnedCar::stock(cat, m));
        self.active = self.cars.len() - 1;
        Ok(self.active)
    }

    /// Six used cars for sale today, deterministic for (seed, day).
    pub fn used_lot(&self, cat: &Catalog) -> Vec<Listing> {
        let mut s = self.seed ^ (self.day as u64).wrapping_mul(0x9E3779B97F4A7C15);
        (0..6)
            .map(|_| {
                let m = &cat.cars[(lcg(&mut s) * cat.cars.len() as f32) as usize % cat.cars.len()];
                let wear = 0.2 + 0.7 * lcg(&mut s);
                let mut c = Condition::new();
                c.tyre_wear = [wear * 0.9, wear * 0.9, wear, wear];
                c.engine = 1.0 - wear * 0.45;
                c.brake_wear = wear * 0.6;
                c.body = [
                    wear * 0.3 * lcg(&mut s),
                    wear * 0.3 * lcg(&mut s),
                    wear * 0.2 * lcg(&mut s),
                    wear * 0.2 * lcg(&mut s),
                ];
                c.odometer_km = 40_000.0 + 160_000.0 * wear;
                let price = (m.base_price * (0.62 - 0.32 * wear)).round();
                Listing {
                    model: m.id.clone(),
                    condition: c,
                    price,
                }
            })
            .collect()
    }

    pub fn buy_used(&mut self, cat: &Catalog, listing: &Listing) -> GameResult<usize> {
        let m = cat.car(&listing.model).ok_or(GameError::NoSuchCar)?;
        self.pay(listing.price)?;
        let mut car = OwnedCar::stock(cat, m);
        car.cond = listing.condition.clone();
        self.cars.push(car);
        self.active = self.cars.len() - 1;
        Ok(self.active)
    }

    pub fn resale_value(&self, cat: &Catalog, idx: usize) -> GameResult<f32> {
        let c = &self.cars[self.idx(idx)?];
        let m = cat.car(&c.model).ok_or(GameError::NoSuchCar)?;
        let health = 0.5 * c.cond.engine
            + 0.3 * (1.0 - c.cond.body_total())
            + 0.2 * (1.0 - c.cond.tyre_wear.iter().sum::<f32>() / 4.0);
        Ok(((m.base_price * 0.55 + c.invested * 0.4) * (0.45 + 0.55 * health)).round())
    }

    pub fn sell(&mut self, cat: &Catalog, idx: usize) -> GameResult<f32> {
        if self.cars.len() <= 1 {
            return Err(GameError::LastCar);
        }
        let v = self.resale_value(cat, idx)?;
        self.cars.remove(idx);
        self.money += v;
        if self.active >= self.cars.len() {
            self.active = self.cars.len() - 1;
        }
        Ok(v)
    }

    // ---- parts ----

    pub fn engine_swap_cost(&self, cat: &Catalog, idx: usize, engine: &str) -> GameResult<f32> {
        let c = &self.cars[self.idx(idx)?];
        let new = cat
            .engine(engine)
            .ok_or_else(|| GameError::NoSuchPart(engine.into()))?;
        let old = &c.setup.engine;
        let price = |e: &crate::engine::EngineSpec| e.peak_power_kw() * 38.0 + 400.0;
        Ok((price(new) - 0.4 * price(old)).max(150.0).round())
    }

    pub fn install_engine(&mut self, cat: &Catalog, idx: usize, engine: &str) -> GameResult<f32> {
        let cost = self.engine_swap_cost(cat, idx, engine)?;
        self.pay(cost)?;
        let new = cat.engine(engine).unwrap().clone();
        let c = &mut self.cars[idx];
        c.setup.engine = new;
        c.setup.engine.boost = tune_boost(c.tune_stage);
        c.cond.engine = 1.0;
        c.invested += cost;
        Ok(cost)
    }

    pub fn tyre_cost(&self, cat: &Catalog, tyre: &str, axle: Axle) -> GameResult<f32> {
        let t = cat
            .tyre(tyre)
            .ok_or_else(|| GameError::NoSuchPart(tyre.into()))?;
        let n = if axle == Axle::Both { 4.0 } else { 2.0 };
        Ok((t.price() * n).round())
    }

    pub fn install_tyres(
        &mut self,
        cat: &Catalog,
        idx: usize,
        tyre: &str,
        axle: Axle,
    ) -> GameResult<f32> {
        self.idx(idx)?;
        let cost = self.tyre_cost(cat, tyre, axle)?;
        self.pay(cost)?;
        let t: TyreSpec = cat.tyre(tyre).unwrap().clone();
        let c = &mut self.cars[idx];
        if axle != Axle::Rear {
            c.setup.tyre_front = t.clone();
            c.cond.tyre_wear[0] = 0.0;
            c.cond.tyre_wear[1] = 0.0;
        }
        if axle != Axle::Front {
            c.setup.tyre_rear = t;
            c.cond.tyre_wear[2] = 0.0;
            c.cond.tyre_wear[3] = 0.0;
        }
        c.invested += cost * 0.3;
        Ok(cost)
    }

    pub const TUNE_COSTS: [f32; 3] = [800.0, 2200.0, 5000.0];

    pub fn set_tune_stage(&mut self, idx: usize, stage: u8) -> GameResult<f32> {
        self.idx(idx)?;
        if stage > 3 {
            return Err(GameError::InvalidValue("tune stage is 0..=3".into()));
        }
        let have = self.cars[idx].tune_stage;
        let cost = if stage > have {
            Self::TUNE_COSTS[stage as usize - 1]
                - if have > 0 {
                    Self::TUNE_COSTS[have as usize - 1] * 0.5
                } else {
                    0.0
                }
        } else {
            0.0
        };
        self.pay(cost)?;
        let c = &mut self.cars[idx];
        c.tune_stage = stage;
        c.setup.engine.boost = tune_boost(stage);
        c.invested += cost * 0.5;
        Ok(cost)
    }

    pub fn buy_lsd(&mut self, idx: usize) -> GameResult<f32> {
        self.idx(idx)?;
        if self.cars[idx].has_lsd {
            return Ok(0.0);
        }
        self.pay(650.0)?;
        let c = &mut self.cars[idx];
        c.has_lsd = true;
        c.setup.lsd = 0.4;
        c.invested += 300.0;
        Ok(650.0)
    }

    pub fn set_final_drive(&mut self, scale: f32) -> GameResult<()> {
        if !(0.7..=1.4).contains(&scale) {
            return Err(GameError::InvalidValue(
                "final drive scale must be within 0.7..1.4".into(),
            ));
        }
        self.car_mut().setup.final_drive_scale = scale;
        Ok(())
    }

    pub fn set_brake_bias(&mut self, bias: f32) {
        self.car_mut().setup.brake_bias = bias.clamp(0.3, 0.8);
    }

    pub fn set_wing(&mut self, wing: f32) {
        self.car_mut().setup.wing = wing.clamp(0.0, 1.0);
    }

    pub fn set_lsd(&mut self, lock: f32) -> GameResult<()> {
        if !self.car().has_lsd {
            return Err(GameError::Locked(
                "Install a limited-slip differential first".into(),
            ));
        }
        self.car_mut().setup.lsd = lock.clamp(0.0, 1.0);
        Ok(())
    }

    pub fn set_automatic(&mut self, auto: bool) {
        self.car_mut().setup.automatic = auto;
    }

    // ---- mechanic & painter ----

    pub fn repair_quote(&self, idx: usize) -> GameResult<f32> {
        Ok(self.cars[self.idx(idx)?].cond.repair_cost().round())
    }

    pub fn repair(&mut self, idx: usize) -> GameResult<f32> {
        let cost = self.repair_quote(idx)?;
        self.pay(cost)?;
        self.cars[idx].cond.repair_all();
        Ok(cost)
    }

    pub fn tyre_replacement_quote(&self, idx: usize) -> GameResult<f32> {
        let c = &self.cars[self.idx(idx)?];
        let per = (c.setup.tyre_front.price() + c.setup.tyre_rear.price()) / 2.0;
        Ok(c.cond.tyre_cost(per).round())
    }

    pub fn replace_worn_tyres(&mut self, idx: usize) -> GameResult<f32> {
        let cost = self.tyre_replacement_quote(idx)?;
        self.pay(cost)?;
        self.cars[idx].cond.replace_tyres();
        Ok(cost)
    }

    pub fn paint(&mut self, idx: usize, colour: usize) -> GameResult<f32> {
        self.idx(idx)?;
        if colour >= PAINTS.len() {
            return Err(GameError::InvalidValue("no such paint".into()));
        }
        if self.cars[idx].paint == colour {
            return Ok(0.0);
        }
        self.pay(150.0)?;
        self.cars[idx].paint = colour;
        Ok(150.0)
    }

    // ---- events ----

    pub fn available_events(&self) -> Vec<&'static EventDef> {
        EVENTS
            .iter()
            .filter(|e| e.rep_required <= self.reputation)
            .collect()
    }

    pub fn event(&self, id: &str) -> Option<&'static EventDef> {
        EVENTS.iter().find(|e| e.id == id)
    }

    pub fn vehicle_for(&self, cat: &Catalog, idx: usize) -> GameResult<Vehicle> {
        let c = &self.cars[self.idx(idx)?];
        let m = cat.car(&c.model).ok_or(GameError::NoSuchCar)?;
        let mut v = Vehicle::new(&m.cfg, c.setup.clone());
        v.cond = c.cond.clone();
        Ok(v)
    }

    /// Pay the entry fee and set up the race with the active car against the event's AI opponent.
    pub fn start_event(&mut self, cat: &Catalog, id: &str) -> GameResult<Race> {
        let ev = self
            .event(id)
            .ok_or_else(|| GameError::NoSuchPart(id.into()))?;
        if ev.rep_required > self.reputation {
            return Err(GameError::Locked(format!(
                "Needs {} reputation",
                ev.rep_required
            )));
        }
        self.pay(ev.entry_fee)?;
        let track = match ev.kind {
            RaceKind::Drag => Track::drag_strip(DRAG_LENGTH),
            RaceKind::Circuit { .. } => Track::circuit(),
        };
        let mut race = Race::new(ev.kind, track);
        let player = self.vehicle_for(cat, self.active)?;
        let opp_model: &CarModel = cat.car(ev.opponent).unwrap_or_else(|| {
            cat.cars
                .iter()
                .min_by(|a, b| a.base_price.partial_cmp(&b.base_price).unwrap())
                .unwrap()
        });
        let mut opp = Vehicle::new(
            &opp_model.cfg,
            Setup {
                engine: cat.engine(&opp_model.stock_engine).unwrap().clone(),
                tyre_front: cat.stock_tyre().clone(),
                tyre_rear: cat.stock_tyre().clone(),
                final_drive_scale: 1.0,
                brake_bias: 0.6,
                wing: 0.3,
                lsd: 0.0,
                automatic: true,
            },
        );
        opp.setup.engine.boost = 1.0 + 0.15 * ev.ai_skill * ev.ai_skill;
        race.add_racer("You", player, None, 0);
        race.add_racer(&opp_model.name, opp, Some(ev.ai_skill), 1);
        Ok(race)
    }

    /// Apply a finished race to the save: wear carries over, money/reputation are awarded.
    pub fn settle_race(&mut self, race: &Race, ev: &EventDef) -> RaceOutcome {
        let position = race.player_position();
        let p = race.player().expect("race has a player");
        let time = p.finish_time;
        let won = time.is_some() && position == 1;
        let key = format!("{}:{}", ev.id, self.car().setup.engine.id);
        let mut new_record = false;
        let best = match ev.kind {
            RaceKind::Circuit { .. } => p.best_lap(),
            RaceKind::Drag => time,
        };
        if let Some(t) = best {
            let entry = self.best_times.entry(key).or_insert(f32::MAX);
            if t < *entry {
                new_record = *entry != f32::MAX;
                *entry = t;
            }
        }
        let active = self.active;
        self.cars[active].cond = p.vehicle.cond.clone();
        self.races += 1;
        self.day += 1;
        let prize = if won {
            ev.prize
        } else if time.is_some() && position == 2 {
            (ev.prize * 0.15).round()
        } else {
            0.0
        };
        let rep_gain = if won { ev.rep_gain } else { 0 };
        self.money += prize;
        self.reputation += rep_gain;
        if won {
            self.wins += 1;
        }
        RaceOutcome {
            position,
            won,
            time,
            prize,
            entry_fee: ev.entry_fee,
            rep_gain,
            repair_estimate: self.cars[active].cond.repair_cost(),
            new_record,
        }
    }

    // ---- persistence ----

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, json)?;
        std::fs::rename(tmp, path)
    }

    pub fn load(path: &Path) -> std::io::Result<GameState> {
        let text = std::fs::read_to_string(path)?;
        let s: GameState = serde_json::from_str(&text).map_err(std::io::Error::other)?;
        if s.version != SAVE_VERSION {
            return Err(std::io::Error::other(format!(
                "unsupported save version {}",
                s.version
            )));
        }
        if s.cars.is_empty() || s.active >= s.cars.len() {
            return Err(std::io::Error::other("corrupt save: no active car"));
        }
        Ok(s)
    }

    pub fn phase_is_over(race: &Race) -> bool {
        race.phase == Phase::Finished
    }
}

fn tune_boost(stage: u8) -> f32 {
    [1.0, 1.08, 1.17, 1.28][stage.min(3) as usize]
}

impl OwnedCar {
    pub fn stock(cat: &Catalog, m: &CarModel) -> OwnedCar {
        let engine = cat.engine(&m.stock_engine).expect("stock engine").clone();
        let tyre = cat.stock_tyre().clone();
        OwnedCar {
            model: m.id.clone(),
            setup: Setup {
                engine,
                tyre_front: tyre.clone(),
                tyre_rear: tyre,
                final_drive_scale: 1.0,
                brake_bias: 0.6,
                wing: 0.3,
                lsd: 0.0,
                automatic: true,
            },
            cond: Condition::new(),
            paint: 9,
            tune_stage: 0,
            has_lsd: false,
            invested: 0.0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::race::Race;

    fn game() -> (Catalog, GameState) {
        let cat = Catalog::fallback();
        let gs = GameState::new_game(&cat, 42);
        (cat, gs)
    }

    #[test]
    fn new_game_starts_with_a_worn_car_and_some_money() {
        let (_, gs) = game();
        assert_eq!(gs.cars.len(), 1);
        assert!(gs.money > 0.0);
        assert!(gs.car().cond.repair_cost() > 0.0);
    }

    #[test]
    fn repairing_costs_money_and_restores_condition() {
        let (_, mut gs) = game();
        let quote = gs.repair_quote(0).unwrap();
        let before = gs.money;
        assert_eq!(gs.repair(0).unwrap(), quote);
        assert_eq!(gs.money, before - quote);
        assert_eq!(gs.car().cond.engine, 1.0);
        assert_eq!(gs.car().cond.body_total(), 0.0);
        assert_eq!(gs.repair(0).unwrap(), 0.0);
    }

    #[test]
    fn cannot_afford_what_you_cannot_afford() {
        let (cat, mut gs) = game();
        gs.money = 10.0;
        let err = gs
            .install_engine(&cat, 0, "Baiern_Devils_6SFi_3_6")
            .unwrap_err();
        assert!(matches!(err, GameError::NotEnoughMoney { .. }));
        assert_eq!(gs.money, 10.0);
        assert_eq!(gs.car().setup.engine.id, "Baiern_Kraftwerk_2_5");
    }

    #[test]
    fn engine_swap_changes_setup_and_resets_wear() {
        let (cat, mut gs) = game();
        gs.money = 50_000.0;
        gs.install_engine(&cat, 0, "Baiern_Devils_6SFi_3_6")
            .unwrap();
        assert_eq!(gs.car().setup.engine.id, "Baiern_Devils_6SFi_3_6");
        assert_eq!(gs.car().cond.engine, 1.0);
        assert!(gs.money < 50_000.0);
        assert!(gs.install_engine(&cat, 0, "nope").is_err());
    }

    #[test]
    fn tyres_install_per_axle_and_reset_that_axle_only() {
        let (cat, mut gs) = game();
        gs.money = 50_000.0;
        let wide = "tyre_225_45_17_9_0";
        gs.install_tyres(&cat, 0, wide, Axle::Rear).unwrap();
        assert_eq!(gs.car().setup.tyre_rear.id, wide);
        assert_ne!(gs.car().setup.tyre_front.id, wide);
        assert_eq!(gs.car().cond.tyre_wear[2], 0.0);
        assert!(gs.car().cond.tyre_wear[0] > 0.0);
    }

    #[test]
    fn tune_stage_scales_power_and_lsd_needs_purchase() {
        let (cat, mut gs) = game();
        gs.money = 50_000.0;
        let stock = gs.car().setup.engine.peak_power_kw();
        gs.set_tune_stage(0, 2).unwrap();
        assert!(gs.car().setup.engine.peak_power_kw() > stock * 1.1);
        assert!(gs.set_lsd(0.5).is_err());
        gs.buy_lsd(0).unwrap();
        gs.set_lsd(0.5).unwrap();
        assert_eq!(gs.car().setup.lsd, 0.5);
        // The tune survives an engine swap.
        gs.install_engine(&cat, 0, "Baiern_Devils_6SFi_3_6")
            .unwrap();
        assert_eq!(gs.car().setup.engine.boost, tune_boost(2));
    }

    #[test]
    fn buying_and_selling_cars() {
        let (cat, mut gs) = game();
        gs.money = 100_000.0;
        let i = gs.buy_new(&cat, "Baiern/DevilSport").unwrap();
        assert_eq!(gs.active, i);
        assert_eq!(gs.cars.len(), 2);
        let got = gs.sell(&cat, 0).unwrap();
        assert!(got > 0.0);
        assert_eq!(gs.cars.len(), 1);
        assert_eq!(gs.sell(&cat, 0), Err(GameError::LastCar));
    }

    #[test]
    fn used_lot_is_deterministic_per_day() {
        let (cat, mut gs) = game();
        let a = gs.used_lot(&cat);
        let b = gs.used_lot(&cat);
        assert_eq!(a.len(), 6);
        assert_eq!(
            a.iter().map(|l| (&l.model, l.price)).collect::<Vec<_>>(),
            b.iter().map(|l| (&l.model, l.price)).collect::<Vec<_>>()
        );
        gs.day += 1;
        let c = gs.used_lot(&cat);
        assert_ne!(
            a.iter().map(|l| l.price).collect::<Vec<_>>(),
            c.iter().map(|l| l.price).collect::<Vec<_>>()
        );
    }

    #[test]
    fn paint_costs_once() {
        let (_, mut gs) = game();
        let m = gs.money;
        assert_eq!(gs.paint(0, 1).unwrap(), 150.0);
        assert_eq!(gs.paint(0, 1).unwrap(), 0.0);
        assert_eq!(gs.money, m - 150.0);
        assert!(gs.paint(0, 99).is_err());
    }

    #[test]
    fn events_unlock_with_reputation() {
        let (cat, mut gs) = game();
        assert_eq!(gs.available_events().len(), 1);
        assert!(matches!(
            gs.start_event(&cat, "circuit_club"),
            Err(GameError::Locked(_))
        ));
        gs.reputation = 100;
        assert_eq!(gs.available_events().len(), EVENTS.len());
    }

    fn run_event(gs: &mut GameState, cat: &Catalog, id: &str, player_throttle: f32) -> RaceOutcome {
        let ev = gs.event(id).unwrap();
        let mut race: Race = gs.start_event(cat, id).unwrap();
        // A stand-in human: steers with the AI's pure-pursuit controller, throttle as given.
        let mut pilot = crate::ai::Driver::new(0.7, 0.0, false);
        let mut guard = 0;
        while race.phase != Phase::Finished && guard < 60 * 700 {
            let mut c = pilot.drive(&race.racers[0].vehicle, &race.track, true);
            c.throttle = player_throttle;
            c.brake = 0.0;
            race.step(&c, 1.0 / 60.0);
            guard += 1;
        }
        race.simulate_to_end(60.0);
        gs.settle_race(&race, ev)
    }

    #[test]
    fn full_loop_race_repair_save_reload() {
        let (cat, mut gs) = game();
        gs.money = 5000.0;
        let money0 = gs.money;
        let out = run_event(&mut gs, &cat, "drag_rookie", 1.0);
        assert!(out.time.is_some());
        assert_eq!(gs.races, 1);
        if out.won {
            assert!(gs.money > money0);
            assert_eq!(gs.reputation, 1);
        }
        // Wear from the race persists on the owned car.
        let tyre_wear = gs.car().cond.tyre_wear;
        assert!(tyre_wear.iter().any(|w| *w > 0.3));
        // Repair, then round-trip through disk.
        gs.money = 20_000.0;
        gs.repair(0).unwrap();
        let dir = std::env::temp_dir().join(format!("slr-test-{}", std::process::id()));
        let path = dir.join("slot1.json");
        gs.save(&path).unwrap();
        let back = GameState::load(&path).unwrap();
        assert_eq!(back, gs);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn corrupt_saves_are_rejected_not_panicked_on() {
        let dir = std::env::temp_dir().join(format!("slr-test-bad-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("bad.json");
        std::fs::write(&p, "{not json").unwrap();
        assert!(GameState::load(&p).is_err());
        let (_, mut gs) = game();
        gs.version = 99;
        gs.save(&p).unwrap();
        assert!(GameState::load(&p).is_err());
        gs.version = SAVE_VERSION;
        gs.active = 5;
        gs.save(&p).unwrap();
        assert!(GameState::load(&p).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn entering_costs_the_fee_and_a_loss_pays_little() {
        let (cat, mut gs) = game();
        gs.reputation = 100;
        gs.money = 5000.0;
        let before = gs.money;
        // Coasting never beats the AI.
        let out = run_event(&mut gs, &cat, "circuit_club", 0.0);
        assert!(!out.won);
        assert!(gs.money <= before);
    }
}

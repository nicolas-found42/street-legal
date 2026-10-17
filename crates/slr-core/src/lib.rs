//! Game logic for the Street Legal Racing: Redline recreation: catalog, vehicle simulation,
//! garage, races, economy and persistence. No rendering and no I/O besides the catalog loader
//! and save files, so everything here is unit-testable headless.

pub mod ai;
pub mod catalog;
pub mod condition;
pub mod engine;
pub mod perf;
pub mod race;
pub mod state;
pub mod track;
pub mod tyre;
pub mod vehicle;
pub mod visual;

pub const G: f32 = 9.81;

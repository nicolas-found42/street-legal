//! Parsers for the shipped file formats of Street Legal Racing: Redline (build 606).
//!
//! Every parser here was derived from the shipped binaries and verified against the
//! reference corpus; see `docs/formats.md` for the evidence behind each layout.
//!
//! This crate is a facade over the per-family crates (`slr-fmt-rpak`, `-scx`,
//! `-cfg`, `-tufa`, `-sdat`, all sharing `slr-fmt-core`) plus the
//! whole-install `corpus` inventory whose counts the corpus tests assert. It
//! exists so the two-profile corpus suite (`stock/modded/skip`) keeps one home
//! with no behavioral change, and so downstream crates (`slr-core`, `slr-game`)
//! keep a single dependency until M1-14 removes them.

pub use slr_fmt_cfg as cfg;
pub use slr_fmt_core::{FormatError, Result};
pub use slr_fmt_rpak as rpak;
pub use slr_fmt_scx as scx;
pub use slr_fmt_sdat as sdat;
pub use slr_fmt_tufa as tufa;

pub mod corpus;

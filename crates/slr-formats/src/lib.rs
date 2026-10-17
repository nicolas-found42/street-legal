//! Parsers for the shipped file formats of Street Legal Racing: Redline (build 606).
//!
//! Every parser here was derived from the shipped binaries and verified against the
//! reference corpus; see `docs/formats.md` for the evidence behind each layout.
//!
//! This crate is a facade over the per-family crates (`slr-fmt-rpak`, `-scx`,
//! `-cfg`, `-tufa`, `-sdat`, `-spline`, `-ground`, all sharing `slr-fmt-core`)
//! plus the
pub use slr_fmt_cfg as cfg;
pub use slr_fmt_core::{FormatError, Result};
pub use slr_fmt_ground as ground;
pub use slr_fmt_rpak as rpak;
pub use slr_fmt_scx as scx;
pub use slr_fmt_sdat as sdat;
pub use slr_fmt_spline as spline;
pub use slr_fmt_traffic as traffic;
pub use slr_fmt_tufa as tufa;
pub mod corpus;

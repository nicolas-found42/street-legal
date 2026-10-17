//! Map-payload corpus tests: every stock map `.rpk` payload walks with the reader,
//! with complete byte accounting (every payload byte belongs to exactly one chunk or
//! one record header) and asserted chunk/tag counts.
//!
//! Profiles follow the workspace convention: `SLR_STOCK` (default `stock/` override)
//! and `SLR_REFERENCE` (modded install), skip-when-absent so CI stays data-free.

use slr_fmt_rpak::payload::{self, Chunk};
use slr_fmt_rpak::Package;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const MAPS: [&str; 4] = ["city.rpk", "city2.rpk", "ROC_track.rpk", "test_track.rpk"];

/// Expected walk totals per map, derived by running the reader over the installs.
struct MapExpect {
    file: &'static str,
    /// Payload gap: `(root descriptor end, first tail descriptor start)`.
    lo: usize,
    hi: usize,
    ranges: usize,
    rsd: usize,
    phys: usize,
    poly: usize,
    planes: usize,
    pverts: usize,
    poly_verts: usize,
    poly_indices: usize,
    text_rsd: usize,
}

// 2026-10-17, reader over the clean stock install.
const STOCK: [MapExpect; 4] = [
    MapExpect {
        file: "city.rpk",
        lo: 10570,
        hi: 32196889,
        ranges: 58209,
        rsd: 12722,
        phys: 23976,
        poly: 624,
        planes: 197286,
        pverts: 236064,
        poly_verts: 257560,
        poly_indices: 198579,
        text_rsd: 10168,
    },
    MapExpect {
        file: "city2.rpk",
        lo: 12304,
        hi: 37987348,
        ranges: 56688,
        rsd: 15653,
        phys: 19040,
        poly: 748,
        planes: 251902,
        pverts: 276107,
        poly_verts: 326849,
        poly_indices: 252505,
        text_rsd: 15653,
    },
    MapExpect {
        file: "ROC_track.rpk",
        lo: 1797,
        hi: 7520519,
        ranges: 11265,
        rsd: 1791,
        phys: 5147,
        poly: 155,
        planes: 55040,
        pverts: 61777,
        poly_verts: 61038,
        poly_indices: 55030,
        text_rsd: 1791,
    },
    MapExpect {
        file: "test_track.rpk",
        lo: 12644,
        hi: 9822065,
        ranges: 12223,
        rsd: 1413,
        phys: 6420,
        poly: 242,
        planes: 66825,
        pverts: 77132,
        poly_verts: 88946,
        poly_indices: 67021,
        text_rsd: 1413,
    },
];

// Modded install keeps the same maps; counts asserted equal to stock here would
// couple the test to mod edits, so the modded profile only asserts exact
// accounting (walk clean + bytes tile), not the count table.
fn stock_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_STOCK")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock/maps"));
    p.is_dir().then_some(p)
}

fn modded_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/raw/maps")
        });
    p.is_dir().then_some(p)
}

struct WalkStats {
    ranges: usize,
    node_bytes: usize,
    tail_bytes: usize,
    chunks: BTreeMap<[u8; 4], usize>,
    planes: usize,
    pverts: usize,
    poly_verts: usize,
    poly_indices: usize,
    text_rsd: usize,
    unknown: usize,
}

fn walk_file(path: &Path) -> WalkStats {
    let pkg = Package::open(path).unwrap();
    let (seed, desc_chunks) = pkg.map_payload_seed().unwrap();
    assert!(
        !desc_chunks.is_empty(),
        "{}: root descriptor holds no chunks",
        path.display()
    );
    let data = pkg.data().to_vec();
    let ranges = payload::walk_ranges(&data, &seed).unwrap();
    let mut st = WalkStats {
        ranges: ranges.len(),
        node_bytes: 0,
        tail_bytes: 0,
        chunks: BTreeMap::new(),
        planes: 0,
        pverts: 0,
        poly_verts: 0,
        poly_indices: 0,
        text_rsd: 0,
        unknown: 0,
    };
    let mut seen = std::collections::BTreeSet::new();
    for r in &ranges {
        assert!(
            seen.insert(r.offset),
            "{}: range {:#x} visited twice",
            path.display(),
            r.offset
        );
        st.node_bytes += r.header_len;
        // Every record header byte plus every chunk byte must tile [offset, offset+size).
        let mut pos = r.offset + r.header_len;
        for c in &r.chunks {
            assert_eq!(
                c.offset() - 8,
                pos,
                "{}: chunk gap at {:#x}",
                path.display(),
                pos
            );
            *st.chunks.entry(c.tag()).or_default() += 1;
            match c {
                Chunk::Rsd { bytes, lines, .. } => {
                    assert_eq!(8 + bytes.len(), 8 + c.len());
                    if lines.is_some() {
                        st.text_rsd += 1;
                    }
                }
                Chunk::Phys { planes, verts, .. } => {
                    st.planes += planes.len();
                    st.pverts += verts.len();
                    for pl in planes {
                        assert!(
                            pl.normal.iter().all(|v| v.is_finite()),
                            "{}: non-finite PHYS normal",
                            path.display()
                        );
                    }
                }
                Chunk::Poly { groups, .. } => {
                    for g in groups {
                        st.poly_verts += g.verts.len();
                        st.poly_indices += g.indices.len();
                    }
                }
                Chunk::Extp { .. } => {}
                Chunk::Unknown { .. } => st.unknown += 1,
            }
            pos += 8 + c.len();
        }
        assert_eq!(
            pos,
            r.offset + r.size,
            "{}: range {:#x} tail mismatch",
            path.display(),
            r.offset
        );
        st.tail_bytes += r.size - r.header_len;
        for rec in &r.records {
            assert_eq!(
                rec.stride(),
                payload::record_stride(
                    u8::try_from(rec.name.len() + usize::from(!rec.name.is_empty())).unwrap()
                ),
                "{}: record stride",
                path.display()
            );
            assert!(
                rec.bbox.iter().all(|v| v.is_finite()),
                "{}: non-finite bbox",
                path.display()
            );
        }
    }
    st
}

fn check_stock(root: &Path) {
    for exp in &STOCK {
        let path = root.join(exp.file);
        let st = walk_file(&path);
        eprintln!(
            "{}: ranges={} node={} tail={} chunks={:?} planes={} pverts={} polyv={} polyi={} textrsd={}",
            exp.file,
            st.ranges,
            st.node_bytes,
            st.tail_bytes,
            st.chunks,
            st.planes,
            st.pverts,
            st.poly_verts,
            st.poly_indices,
            st.text_rsd
        );
        assert_eq!(st.ranges, exp.ranges, "{} range count", exp.file);
        // Payload gap tiles exactly: node headers + chunk streams.
        let pkg = Package::open(&path).unwrap();
        let root_entry = pkg.entries.iter().find(|e| e.typeof_ == 1).unwrap();
        let lo = root_entry.rsd_offset as usize + root_entry.rsd_size as usize;
        let mut hi = usize::MAX;
        for e in &pkg.entries {
            let o = e.rsd_offset as usize;
            if o >= lo && o < hi {
                hi = o;
            }
        }
        assert_eq!(lo, exp.lo, "{} payload start", exp.file);
        assert_eq!(hi, exp.hi, "{} payload end", exp.file);
        assert_eq!(
            st.node_bytes + st.tail_bytes,
            hi - lo,
            "{} byte accounting",
            exp.file
        );
        assert_eq!(
            st.chunks.get(b"RSD\x00").copied().unwrap_or(0),
            exp.rsd,
            "{} RSD",
            exp.file
        );
        assert_eq!(
            st.chunks.get(b"PHYS").copied().unwrap_or(0),
            exp.phys,
            "{} PHYS",
            exp.file
        );
        assert_eq!(
            st.chunks.get(b"POLY").copied().unwrap_or(0),
            exp.poly,
            "{} POLY",
            exp.file
        );
        assert_eq!(st.planes, exp.planes, "{} planes", exp.file);
        assert_eq!(st.pverts, exp.pverts, "{} pverts", exp.file);
        assert_eq!(st.poly_verts, exp.poly_verts, "{} poly verts", exp.file);
        assert_eq!(
            st.poly_indices, exp.poly_indices,
            "{} poly indices",
            exp.file
        );
        assert_eq!(st.text_rsd, exp.text_rsd, "{} text RSD", exp.file);
        assert_eq!(st.unknown, 0, "{} unknown tags", exp.file);
    }
}

fn check_accounting_only(root: &Path, label: &str) {
    for file in &MAPS {
        let path = root.join(file);
        if !path.is_file() {
            continue;
        }
        let st = walk_file(&path);
        eprintln!(
            "{label} {file}: ranges={} node={} tail={} chunks={:?} unknown={}",
            st.ranges, st.node_bytes, st.tail_bytes, st.chunks, st.unknown
        );
        assert_eq!(st.unknown, 0, "{label} {file} unknown tags");
        let pkg = Package::open(&path).unwrap();
        let root_entry = pkg.entries.iter().find(|e| e.typeof_ == 1).unwrap();
        let lo = root_entry.rsd_offset as usize + root_entry.rsd_size as usize;
        let mut hi = usize::MAX;
        for e in &pkg.entries {
            let o = e.rsd_offset as usize;
            if o >= lo && o < hi {
                hi = o;
            }
        }
        assert_eq!(
            st.node_bytes + st.tail_bytes,
            hi - lo,
            "{label} {file} byte accounting"
        );
    }
}

#[test]
fn stock_map_payloads_walk_exactly() {
    let Some(root) = stock_root() else {
        eprintln!("stock maps not present; skipping");
        return;
    };
    check_stock(&root);
}

#[test]
fn modded_map_payloads_walk_exactly() {
    let Some(root) = modded_root() else {
        eprintln!("reference maps not present; skipping");
        return;
    };
    check_accounting_only(&root, "modded");
}

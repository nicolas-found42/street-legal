//! Map-payload corpus tests: every stock map `.rpk` payload walks with the reader,
//! with complete byte accounting (every payload byte belongs to exactly one chunk or
//! one record header) and asserted chunk/tag counts, plus field-level invariants
//! over the decoded geometry (PHYS/POLY), placements (RSD) and record chains.
//!
//! Profiles follow the workspace convention: `SLR_STOCK` (default `stock/`) and
//! `SLR_REFERENCE` (default `reference/raw`), skip-when-absent so CI stays
//! data-free. A direct `maps/` dir is also accepted for either variable.
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
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    // `SLR_STOCK` is the install root (maps live in `maps/`); accept a direct
    // `maps/` dir too.
    if p.join("maps").is_dir() {
        Some(p.join("maps"))
    } else {
        p.is_dir().then_some(p)
    }
}

fn modded_root() -> Option<PathBuf> {
    let p = std::env::var_os("SLR_REFERENCE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../reference/raw"));
    if p.join("maps").is_dir() {
        Some(p.join("maps"))
    } else {
        p.is_dir().then_some(p)
    }
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
    named_records: usize,
    named_with_point: usize,
    k2_total: usize,
    k2_distinct: usize,
}

/// World bounds (centimetres) for `POLY`/`PHYS` positions, per map: the decoded
/// position lanes must land inside these. Derived by running the reader over the
/// clean stock install (min/max over all verts, padded by 1cm).
fn world_bounds(file: &str) -> ([f32; 3], [f32; 3]) {
    match file {
        "city.rpk" => ([-1658.0, -28.0, -2802.0], [1502.0, 71.0, 329.0]),
        "city2.rpk" => ([-1767.0, -19.0, -264.0], [1512.0, 76.0, 1643.0]),
        "ROC_track.rpk" => ([-829.0, -20.0, -1763.0], [1591.0, 266.0, 864.0]),
        "test_track.rpk" => ([-525.0, -5.0, -1540.0], [482.0, 71.0, 476.0]),
        other => panic!("no world bounds for {other}"),
    }
}

fn walk_file(path: &Path) -> WalkStats {
    let pkg = Package::open(path).unwrap();
    let (seed, desc_chunks) = pkg.map_payload_seed().unwrap();
    assert!(
        !desc_chunks.is_empty(),
        "{}: root descriptor holds no chunks",
        path.display()
    );
    let file = path.file_name().unwrap().to_string_lossy().into_owned();
    let (wmin, wmax) = world_bounds(&file);
    let in_bounds = |p: &[f32; 3]| {
        p.iter()
            .zip(wmin.iter().zip(wmax.iter()))
            .all(|(v, (lo, hi))| *v >= *lo && *v <= *hi)
    };
    let data = pkg.data().to_vec();
    let ranges = payload::walk_ranges(&data, &seed).unwrap();
    // Child offsets keyed once: the record tree is a strict tree (no range is
    // linked twice) and every child's `k1` equals its parent record's `k2`.
    let mut parent_of: BTreeMap<usize, (usize, u32)> = BTreeMap::new();
    for r in &ranges {
        for rec in &r.records {
            assert!(
                parent_of
                    .insert(rec.x as usize, (r.offset, rec.k2))
                    .is_none(),
                "{}: range {:#x} linked twice",
                path.display(),
                rec.x
            );
        }
    }
    for r in &ranges {
        if let Some((_, pk2)) = parent_of.get(&r.offset) {
            for rec in &r.records {
                assert_eq!(
                    rec.k1,
                    *pk2,
                    "{}: range {:#x} child k1 != parent k2",
                    path.display(),
                    r.offset
                );
            }
        }
    }
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
        named_records: 0,
        named_with_point: 0,
        k2_total: seed.len(),
        k2_distinct: 0,
    };
    let mut k2seen = std::collections::BTreeSet::new();
    for rec in &seed {
        k2seen.insert(rec.k2);
    }
    // Named placements own exactly one single-`RSD` child each; every other
    // record kind owns a fixed chunk pattern (checked in `check_stock`).
    let mut child_chunks: BTreeMap<usize, Vec<[u8; 4]>> = BTreeMap::new();
    for r in &ranges {
        child_chunks.insert(r.offset, r.chunks.iter().map(|c| c.tag()).collect());
    }
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
                        // Gametype ids are `0x000XYYYY` (map id in the high word).
                        let gt = c.rsd_gametype().unwrap_or_else(|| {
                            panic!("{}: text RSD without gametype", path.display())
                        });
                        assert_ne!(
                            gt & 0xffff,
                            0,
                            "{}: text RSD with zero lo word",
                            path.display()
                        );
                        // Placement points are finite; most land inside the
                        // geometry bounds, but sky/high-placed props (city
                        // `0x205xx`, 7 chunks) and far-field placements (city2,
                        // 213 chunks past the streamed-geometry z edge) exceed
                        // them, so only finiteness is asserted here.
                        if let Some(p) = c.rsd_point() {
                            assert!(
                                p.iter().all(|v| v.is_finite()),
                                "{}: non-finite RSD point",
                                path.display()
                            );
                        }
                    }
                }
                Chunk::Phys { planes, verts, .. } => {
                    st.planes += planes.len();
                    st.pverts += verts.len();
                    for v in verts.iter() {
                        assert!(
                            v.iter().all(|x| x.is_finite()),
                            "{}: non-finite PHYS vert",
                            path.display()
                        );
                        assert!(
                            in_bounds(v),
                            "{}: PHYS vert outside world bounds",
                            path.display()
                        );
                    }
                    for pl in planes {
                        assert!(
                            pl.normal.iter().all(|v| v.is_finite()),
                            "{}: non-finite PHYS normal",
                            path.display()
                        );
                        // Unit face normal: length ~1 and perpendicular to the edges.
                        let len =
                            (pl.normal[0].powi(2) + pl.normal[1].powi(2) + pl.normal[2].powi(2))
                                .sqrt();
                        assert!(
                            (len - 1.0).abs() < 1e-4,
                            "{}: PHYS normal len {len}",
                            path.display()
                        );
                        let tri: Vec<[f32; 3]> =
                            pl.refs.iter().map(|i| verts[*i as usize]).collect();
                        for (a, b) in [(tri[0], tri[1]), (tri[0], tri[2])] {
                            let d = pl.normal[0] * (b[0] - a[0])
                                + pl.normal[1] * (b[1] - a[1])
                                + pl.normal[2] * (b[2] - a[2]);
                            let scale = a[0]
                                .abs()
                                .max(a[1].abs())
                                .max(a[2].abs())
                                .max(b[0].abs())
                                .max(b[1].abs())
                                .max(b[2].abs())
                                .max(1.0);
                            assert!(
                                d.abs() < 1e-3 * scale,
                                "{}: PHYS plane not coplanar (d={d})",
                                path.display()
                            );
                        }
                        // Winding is uniform CCW-into-normal except for two city2
                        // w0=7 skirt planes with an i=j repeat (zero area, valid
                        // up normal, no winding); the dot check skips zero-area
                        // planes and asserts them separately.
                        let ab = [
                            tri[1][0] - tri[0][0],
                            tri[1][1] - tri[0][1],
                            tri[1][2] - tri[0][2],
                        ];
                        let ac = [
                            tri[2][0] - tri[0][0],
                            tri[2][1] - tri[0][1],
                            tri[2][2] - tri[0][2],
                        ];
                        let cross = [
                            ab[1] * ac[2] - ab[2] * ac[1],
                            ab[2] * ac[0] - ab[0] * ac[2],
                            ab[0] * ac[1] - ab[1] * ac[0],
                        ];
                        let area2 = (cross[0].powi(2) + cross[1].powi(2) + cross[2].powi(2)).sqrt();
                        if area2 == 0.0 {
                            assert!(
                                pl.refs[0] == pl.refs[2]
                                    || pl.refs[0] == pl.refs[1]
                                    || pl.refs[1] == pl.refs[2],
                                "{}: zero-area PHYS plane without index repeat",
                                path.display()
                            );
                        } else {
                            let dot = cross[0] * pl.normal[0]
                                + cross[1] * pl.normal[1]
                                + cross[2] * pl.normal[2];
                            assert!(dot > 0.0, "{}: PHYS winding flipped", path.display());
                        }
                    }
                }
                Chunk::Poly { groups, .. } => {
                    for g in groups {
                        st.poly_verts += g.verts.len();
                        st.poly_indices += g.indices.len();
                        for raw in &g.verts {
                            let v = payload::PolyVertex::decode(raw);
                            assert!(
                                v.pos.iter().all(|x| x.is_finite()),
                                "{}: non-finite POLY pos",
                                path.display()
                            );
                            // UVs are finite except 2 ROC_track + 3 test_track
                            // verts whose uv words are sentinel bit patterns
                            // (`0x00000130/0xFFFFFFFF`, `0x7FFFFFFF/0x3F800000`)
                            // on untextured faces; exempt exact-zero normals'
                            // siblings by allowing those two patterns.
                            let uv_ok = v.uv.iter().all(|x| x.is_finite())
                                || v.aux_words() == [0, 0]
                                    && (v.uv[0].to_bits() == 0x00000130
                                        || v.uv[0].to_bits() == 0x7FFFFFFF);
                            assert!(uv_ok, "{}: non-finite POLY uv", path.display());
                            // Auxiliary lane: zeros, finite float pairs, or small ints.
                            // Non-float bit patterns occur (city2 `(0, 1)` tuples), so
                            // only assert: either all-zero or both words finite floats.
                            let aux = v.aux_floats();
                            let words = v.aux_words();
                            assert!(
                                words == [0, 0] || (aux[0].is_finite() && aux[1].is_finite()),
                                "{}: non-finite POLY aux",
                                path.display()
                            );
                            // Normals are unit length (exact-zero billboard normals exempt).
                            let nl = v.normal_len();
                            if nl != 0.0 {
                                assert!(
                                    (nl - 1.0).abs() < 1e-3,
                                    "{}: POLY normal len {nl}",
                                    path.display()
                                );
                            }
                        }
                        for (a, b, cc) in g.triangles() {
                            let nv = g.verts.len() as u32;
                            assert!(
                                a < nv && b < nv && cc < nv,
                                "{}: POLY index out of range",
                                path.display()
                            );
                        }
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
            st.k2_total += 1;
            k2seen.insert(rec.k2);
        }
    }
    st.k2_distinct = k2seen.len();
    // Named-placement linkage: every `b == 0x21` record owns exactly one child
    // range holding exactly one text `RSD` chunk.
    let all_records: Vec<&payload::Record> = seed
        .iter()
        .chain(ranges.iter().flat_map(|r| r.records.iter()))
        .collect();
    for rec in all_records {
        if rec.b != 0x21 {
            continue;
        }
        st.named_records += 1;
        let tags = child_chunks
            .get(&(rec.x as usize))
            .unwrap_or_else(|| panic!("{}: named record points outside walk", path.display()));
        assert_eq!(
            tags.len(),
            1,
            "{}: named record owns {} chunks",
            path.display(),
            tags.len()
        );
        assert_eq!(
            tags[0],
            *b"RSD\x00",
            "{}: named record owns {:?}",
            path.display(),
            tags[0]
        );
        st.named_with_point += 1;
    }
    st
}

fn check_stock(root: &Path) {
    for exp in &STOCK {
        let path = root.join(exp.file);
        let st = walk_file(&path);
        eprintln!(
            "{}: ranges={} node={} tail={} chunks={:?} planes={} pverts={} polyv={} polyi={} textrsd={} named={} k2={}/{}",
            exp.file,
            st.ranges,
            st.node_bytes,
            st.tail_bytes,
            st.chunks,
            st.planes,
            st.pverts,
            st.poly_verts,
            st.poly_indices,
            st.text_rsd,
            st.named_records,
            st.k2_distinct,
            st.k2_total
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
        // Record linkage: `k2` ids are globally unique per map (parent links
        // resolve 1:1), and named placements 1:1 own their text `RSD` chunk.
        assert_eq!(st.k2_distinct, st.k2_total, "{} duplicate k2", exp.file);
        assert_eq!(
            st.named_with_point, st.named_records,
            "{} named without RSD",
            exp.file
        );
        assert_eq!(
            st.named_records, st.text_rsd,
            "{} named != text RSD",
            exp.file
        );
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

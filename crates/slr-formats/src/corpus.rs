//! Whole-install inventory: classify and parse every file under a reference directory.

use crate::{cfg, ground, pedestrian, rpak, scx, sdat, skeleton, spline, traffic, tufa, Result};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn walk(root: &Path) -> Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let p = e.path();
            let ft = e.file_type()?;
            if ft.is_dir() {
                stack.push(p);
            } else if ft.is_file()
                && e.file_name() != ".DS_Store"
                // checksum manifest written by tools/extract-stock.sh (M7-04), not a game file
                && e.file_name() != ".slr-manifest"
            {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

#[derive(Debug, Default)]
pub struct Report {
    pub files: usize,
    pub by_ext: BTreeMap<String, usize>,
    pub parsed: BTreeMap<&'static str, usize>,
    pub failures: Vec<(PathBuf, String)>,
    pub scx_v3: usize,
    pub scx_v4: usize,
    pub scx_v3_groups: usize,
    pub scx_v3_verts: usize,
    pub scx_v3_tris: usize,
    pub scx_meshes: usize,
    pub scx_triangles: usize,
    pub scy: usize,
    pub rpak_entries: usize,
    pub tufa_modules: usize,
    pub vehicle_cfgs: usize,
    pub trc_files: usize,
    pub trc_lanes: usize,
    pub trc_lane_items: usize,
    pub walk_files: usize,
    pub walk_lines: usize,
    pub walk_leaf_indices: usize,
    pub rest_files: usize,
    pub rest_lines: usize,
    pub spl_files: usize,
    pub spl_points: usize,
    pub scm_files: usize,
    pub scm_materials: usize,
    /// `.scm`/`.SCM` paths left unparsed (binary addon variants).
    pub scm_binary: usize,
    /// Emitter scripts under `particles/scripts/particle_clouds/` (`[STATE]`/`[ACTION]`).
    pub particle_cfgs: usize,
    /// `[ACTION]` lines across them.
    pub particle_actions: usize,
    /// Skidmark scripts under `particles/scripts/skids/`.
    pub skid_cfgs: usize,
    /// `;`-commented lines across both particle dialects.
    pub particle_comment_lines: usize,
    /// Lines of either dialect the parsers had to skip (not comments).
    pub particle_ignored_lines: usize,
    pub bon_files: usize,
    pub bon_bones: usize,
    pub skl_files: usize,
    pub skl_bones: usize,
    pub matrixdata_files: usize,
    /// Keys (bones x keys per bone) across all `.matrixdata` files.
    pub matrixdata_keys: usize,
}
/// Parse every file whose format this crate understands; collect failures.
pub fn verify(root: &Path) -> Result<Report> {
    let mut rep = Report::default();
    for p in walk(root)? {
        rep.files += 1;
        let ext = p
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        *rep.by_ext.entry(ext.clone()).or_default() += 1;
        let res: std::result::Result<Option<&'static str>, String> = (|| {
            match ext.as_str() {
                "rpk" => {
                    let pk = rpak::Package::open(&p).map_err(|e| e.to_string())?;
                    rep.rpak_entries += pk.entries.len();
                    Ok(Some("rpk"))
                }
                "scx" | "scy" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    match scx::parse(&data).map_err(|e| e.to_string())? {
                        scx::Scx::V3 { groups, .. } => {
                            rep.scx_v3 += 1;
                            if ext == "scy" {
                                rep.scy += 1;
                            }
                            rep.scx_v3_groups += groups.len();
                            rep.scx_v3_verts +=
                                groups.iter().map(|g| g.mesh.positions.len()).sum::<usize>();
                            rep.scx_v3_tris += groups
                                .iter()
                                .map(|g| g.mesh.indices.len() / 3)
                                .sum::<usize>();
                        }
                        scx::Scx::V4 { meshes, .. } => {
                            rep.scx_v4 += 1;
                            rep.scx_meshes += meshes.len();
                            rep.scx_triangles +=
                                meshes.iter().map(|m| m.indices.len() / 3).sum::<usize>();
                        }
                    }
                    Ok(Some(if ext == "scy" { "scy" } else { "scx" }))
                }
                "class" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let mods = tufa::parse(&data).map_err(|e| e.to_string())?;
                    rep.tufa_modules += mods.len();
                    Ok(Some("class"))
                }
                "cfg" => {
                    cfg::Cfg::open(&p).map_err(|e| e.to_string())?;
                    let path = p.to_string_lossy().replace('\\', "/");
                    if path.contains("/particles/scripts/particle_clouds/") {
                        let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                        let (c, d) =
                            cfg::particle::ParticleCfg::parse_diagnosed(&data, cfg::Precision::F32);
                        rep.particle_cfgs += 1;
                        rep.particle_actions += c.actions.len();
                        rep.particle_comment_lines += d.comments;
                        rep.particle_ignored_lines += d.ignored.len();
                    } else if path.contains("/particles/scripts/skids/") {
                        let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                        let (_, d) =
                            cfg::skid::SkidCfg::parse_diagnosed(&data, cfg::Precision::F32);
                        rep.skid_cfgs += 1;
                        rep.particle_comment_lines += d.comments;
                        rep.particle_ignored_lines += d.ignored.len();
                    }
                    Ok(Some("cfg"))
                }
                "walk" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let lines = pedestrian::parse_walk(&data).map_err(|e| e.to_string())?;
                    rep.walk_files += 1;
                    rep.walk_lines += lines.len();
                    rep.walk_leaf_indices +=
                        lines.iter().map(|l| l.leaf_indices().len()).sum::<usize>();
                    Ok(Some("walk"))
                }
                "rest" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let lines = pedestrian::parse_rest(&data).map_err(|e| e.to_string())?;
                    rep.rest_files += 1;
                    rep.rest_lines += lines.len();
                    Ok(Some("rest"))
                }
                "trc" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let trc = traffic::parse(&data).map_err(|e| e.to_string())?;
                    rep.trc_files += 1;
                    rep.trc_lanes += trc.lanes.len();
                    rep.trc_lane_items += trc.lanes.iter().map(|l| l.items.len()).sum::<usize>();
                    Ok(Some("trc"))
                }
                "bon" | "skl" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let sk = skeleton::parse_skeleton(&data).map_err(|e| e.to_string())?;
                    if ext == "bon" {
                        rep.bon_files += 1;
                        rep.bon_bones += sk.bones.len();
                        Ok(Some("bon"))
                    } else {
                        rep.skl_files += 1;
                        rep.skl_bones += sk.bones.len();
                        Ok(Some("skl"))
                    }
                }
                "matrixdata" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let a = skeleton::parse_animation(&data).map_err(|e| e.to_string())?;
                    rep.matrixdata_files += 1;
                    rep.matrixdata_keys += a.keys.len();
                    Ok(Some("matrixdata"))
                }
                "spl" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let spl = spline::parse_stock(&data).map_err(|e| e.to_string())?;
                    rep.spl_files += 1;
                    rep.spl_points += spl.points.len();
                    Ok(Some("spl"))
                }
                "scm" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    match ground::parse(&data) {
                        Ok(g) => {
                            rep.scm_files += 1;
                            rep.scm_materials += g.materials.len();
                            Ok(Some("scm"))
                        }
                        // Binary addon `.SCM` variants (MP1, Physic_Lab,
                        // Caynon) are a different format, out of scope for
                        // this ticket; leave them unparsed, not failures.
                        Err(_) if data.contains(&0) => {
                            rep.scm_binary += 1;
                            Ok(None)
                        }
                        Err(e) => Err(e.to_string()),
                    }
                }
                _ => {
                    // Extensionless save files start with SDAT.
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    if data.starts_with(b"SDAT") {
                        sdat::parse(&data, None).map_err(|e| e.to_string())?;
                        Ok(Some("sdat"))
                    } else {
                        Ok(None)
                    }
                }
            }
        })();
        match res {
            Ok(Some(k)) => *rep.parsed.entry(k).or_default() += 1,
            Ok(None) => {}
            Err(e) => rep.failures.push((p, e)),
        }
    }
    Ok(rep)
}

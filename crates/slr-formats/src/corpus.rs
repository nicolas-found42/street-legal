//! Whole-install inventory: classify and parse every file under a reference directory.

use crate::{cfg, rpak, scx, sdat, traffic, tufa, Result};
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
            } else if ft.is_file() && e.file_name() != ".DS_Store" {
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
    pub scx_meshes: usize,
    pub scx_triangles: usize,
    pub rpak_entries: usize,
    pub tufa_modules: usize,
    pub vehicle_cfgs: usize,
    pub trc_files: usize,
    pub trc_lanes: usize,
    pub trc_lane_items: usize,
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
                "scx" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    match scx::parse(&data).map_err(|e| e.to_string())? {
                        scx::Scx::V3 { .. } => rep.scx_v3 += 1,
                        scx::Scx::V4 { meshes, .. } => {
                            rep.scx_v4 += 1;
                            rep.scx_meshes += meshes.len();
                            rep.scx_triangles +=
                                meshes.iter().map(|m| m.indices.len() / 3).sum::<usize>();
                        }
                    }
                    Ok(Some("scx"))
                }
                "class" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let mods = tufa::parse(&data).map_err(|e| e.to_string())?;
                    rep.tufa_modules += mods.len();
                    Ok(Some("class"))
                }
                "cfg" => {
                    cfg::Cfg::open(&p).map_err(|e| e.to_string())?;
                    Ok(Some("cfg"))
                }
                "trc" => {
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    let trc = traffic::parse(&data).map_err(|e| e.to_string())?;
                    rep.trc_files += 1;
                    rep.trc_lanes += trc.lanes.len();
                    rep.trc_lane_items += trc.lanes.iter().map(|l| l.items.len()).sum::<usize>();
                    Ok(Some("trc"))
                }
                _ => {
                    // Extensionless save files start with SDAT.
                    let data = std::fs::read(&p).map_err(|e| e.to_string())?;
                    if data.starts_with(b"SDAT") {
                        sdat::parse(&data).map_err(|e| e.to_string())?;
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

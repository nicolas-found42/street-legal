//! Whole-install census (acceptance gate G2): every file under an install
//! root is routed to exactly one decoder or to an explained non-game class,
//! and anything neither decoded nor explained is reported as unexplained.
//!
//! The census never reads a file twice and never trusts an extension alone for
//! formats that carry a magic (extensionless SDAT saves, numbered JPEG
//! companions). Counts are aggregates only; nothing in the report derives from
//! stock content beyond numbers, so reports may be compared against checked-in
//! golden counts without committing stock data.

use crate::{cfg, ground, pedestrian, rpak, scx, sdat, skeleton, spline, traffic, tufa};
use slr_fmt_sound::{mp3, wav};
use slr_fmt_texture::{dds, pngdec, tga};
use slr_fmt_video::{avi::Avi, cinepak};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// Why a file is in the install: parsed by a game-format decoder, a numbered
/// companion of an SDAT save, a bundled manual/doc file or an installer/exe
/// by-product the game does not decode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Game,
    Companion,
    Documentation,
    InstallArtifact,
}

impl Class {
    pub fn name(self) -> &'static str {
        match self {
            Class::Game => "game",
            Class::Companion => "companion",
            Class::Documentation => "documentation",
            Class::InstallArtifact => "install-artifact",
        }
    }
}

/// Aggregates for one decoder.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Entry {
    pub class: Option<Class>,
    pub files: u64,
    pub bytes: u64,
    pub units: BTreeMap<&'static str, u64>,
}

#[derive(Debug, Default)]
pub struct Census {
    pub files: u64,
    pub bytes: u64,
    pub decoders: BTreeMap<&'static str, Entry>,
    /// Files no decoder accepted or no rule explained: must be empty.
    pub unexplained: Vec<(String, String)>,
}

/// What a decoder reports for one file: the decoder's name and its unit counts.
struct Outcome {
    decoder: &'static str,
    class: Class,
    units: Vec<(&'static str, u64)>,
}

fn out(
    decoder: &'static str,
    class: Class,
    units: impl IntoIterator<Item = (&'static str, u64)>,
) -> Result<Outcome, String> {
    Ok(Outcome {
        decoder,
        class,
        units: units.into_iter().collect(),
    })
}

fn e<T: std::fmt::Display>(x: T) -> String {
    x.to_string()
}

/// Every file below `root` (sorted), skipping only Finder metadata and the
/// extraction manifest, which are not part of the install.
pub fn files(root: &Path) -> std::io::Result<Vec<PathBuf>> {
    crate::corpus::walk(root).map_err(|err| match err {
        crate::FormatError::Io(io) => io,
        other => std::io::Error::other(other.to_string()),
    })
}

fn rel(root: &Path, p: &Path) -> String {
    p.strip_prefix(root)
        .unwrap_or(p)
        .to_string_lossy()
        .replace('\\', "/")
}

fn lower_ext(rel: &str) -> String {
    let name = rel.rsplit('/').next().unwrap_or(rel);
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => ext.to_ascii_lowercase(),
        _ => String::new(),
    }
}

/// Route one file. `rel` uses `/` separators relative to the install root.
fn decode(rel: &str, data: &[u8]) -> Result<Outcome, String> {
    let ext = lower_ext(rel);
    let name = rel.rsplit('/').next().unwrap_or(rel);
    // Bundled HTML manual (`SLR Manual.htm` plus its `_files` folders).
    if rel.starts_with("SLR Manual") {
        return manual(&ext, data);
    }
    match ext.as_str() {
        "rpk" => {
            let pk = rpak::Package::parse(data.to_vec()).map_err(e)?;
            out(
                "rpk",
                Class::Game,
                [
                    ("entries", pk.entries.len() as u64),
                    ("links", pk.links.len() as u64),
                ],
            )
        }
        "scx" | "scy" => {
            let key = if ext == "scy" { "scy" } else { "scx" };
            match scx::parse(data).map_err(e)? {
                scx::Scx::V3 { groups, .. } => out(
                    key,
                    Class::Game,
                    [
                        ("v3_files", 1),
                        ("v3_groups", groups.len() as u64),
                        (
                            "v3_vertices",
                            groups.iter().map(|g| g.mesh.positions.len() as u64).sum(),
                        ),
                        (
                            "v3_triangles",
                            groups.iter().map(|g| g.mesh.indices.len() as u64 / 3).sum(),
                        ),
                    ],
                ),
                scx::Scx::V4 { meshes, .. } => out(
                    key,
                    Class::Game,
                    [
                        ("v4_files", 1),
                        ("v4_meshes", meshes.len() as u64),
                        (
                            "v4_triangles",
                            meshes.iter().map(|m| m.indices.len() as u64 / 3).sum(),
                        ),
                    ],
                ),
            }
        }
        "class" => {
            let modules = tufa::parse(data).map_err(e)?;
            out("class", Class::Game, [("modules", modules.len() as u64)])
        }
        "cfg" => {
            let text: String = data.iter().map(|&b| b as char).collect();
            let c = cfg::Cfg::parse(&text);
            let mut units = vec![("directives", c.directives.len() as u64)];
            if rel.contains("particles/scripts/particle_clouds/") {
                let (p, d) = cfg::particle::ParticleCfg::parse_diagnosed(data, cfg::Precision::F32);
                if !d.ignored.is_empty() {
                    return Err(format!("{} ignored particle lines", d.ignored.len()));
                }
                units.push(("particle_actions", p.actions.len() as u64));
            } else if rel.contains("particles/scripts/skids/") {
                let (_, d) = cfg::skid::SkidCfg::parse_diagnosed(data, cfg::Precision::F32);
                if !d.ignored.is_empty() {
                    return Err(format!("{} ignored skid lines", d.ignored.len()));
                }
                units.push(("skid_files", 1));
            }
            out("cfg", Class::Game, units)
        }
        "ptx" => {
            let ptx = slr_fmt_texture::parse(data).map_err(e)?;
            let mut alpha = 0;
            for mip in &ptx.mips {
                let (w, h, _) = slr_fmt_texture::decode_rgb(mip)?;
                if mip.has_alpha() {
                    alpha += 1;
                }
                if w == 0 || h == 0 {
                    return Err("empty jpeg mip".into());
                }
            }
            out(
                "ptx",
                Class::Game,
                [("mips", ptx.mips.len() as u64), ("alpha_mips", alpha)],
            )
        }
        "dds" => {
            let d = dds::parse(data).map_err(e)?;
            let mut mips = 0;
            for level in 0..d.mip_count {
                d.decode_mip(level).map_err(e)?;
                mips += 1;
            }
            out("dds", Class::Game, [("mips", mips)])
        }
        "png" => {
            let (info, img) = pngdec::decode(data).map_err(e)?;
            if (img.width, img.height) != (info.width, info.height) {
                return Err("png decoded size differs from header".into());
            }
            out(
                "png",
                Class::Game,
                [("pixels", u64::from(info.width) * u64::from(info.height))],
            )
        }
        "tga" => {
            let (info, _) = tga::decode(data).map_err(e)?;
            out(
                "tga",
                Class::Game,
                [("pixels", u64::from(info.width) * u64::from(info.height))],
            )
        }
        "wav" => {
            let (info, pcm) = wav::decode(data).map_err(e)?;
            if pcm.frames() != info.frames as usize {
                return Err("decoded wav frames differ from header".into());
            }
            out("wav", Class::Game, [("frames", u64::from(info.frames))])
        }
        "mp3" => {
            let (info, pcm) = mp3::decode(data).map_err(e)?;
            if pcm.frames() != info.frames * info.samples_per_frame {
                return Err("decoded mp3 samples differ from the frame walk".into());
            }
            out("mp3", Class::Game, [("frames", info.frames as u64)])
        }
        "avi" => {
            let avi = Avi::parse(data).map_err(e)?;
            avi.verify_index().map_err(e)?;
            let v = avi.video_stream().ok_or("no video stream")?;
            let mut dec = cinepak::Decoder::new(v.width as usize, v.height as usize).map_err(e)?;
            let mut frames = 0;
            for f in avi.video_frames() {
                dec.decode(f).map_err(e)?;
                frames += 1;
            }
            if frames != avi.index_video_count() as u64 {
                return Err("decoded frames differ from the idx1 video count".into());
            }
            out("avi", Class::Game, [("frames", frames)])
        }
        "bon" | "skl" => {
            let sk = skeleton::parse_skeleton(data).map_err(e)?;
            out(
                if ext == "bon" { "bon" } else { "skl" },
                Class::Game,
                [("bones", sk.bones.len() as u64)],
            )
        }
        "matrixdata" => {
            let a = skeleton::parse_animation(data).map_err(e)?;
            out("matrixdata", Class::Game, [("keys", a.keys.len() as u64)])
        }
        "spl" => {
            let s = spline::parse_stock(data).map_err(e)?;
            out("spl", Class::Game, [("points", s.points.len() as u64)])
        }
        "scm" => {
            let g = ground::parse(data).map_err(e)?;
            out(
                "scm",
                Class::Game,
                [("materials", g.materials.len() as u64)],
            )
        }
        "trc" => {
            let t = traffic::parse(data).map_err(e)?;
            out(
                "trc",
                Class::Game,
                [
                    ("lanes", t.lanes.len() as u64),
                    (
                        "lane_items",
                        t.lanes.iter().map(|l| l.items.len() as u64).sum(),
                    ),
                ],
            )
        }
        "walk" => {
            let lines = pedestrian::parse_walk(data).map_err(e)?;
            out(
                "walk",
                Class::Game,
                [
                    ("lines", lines.len() as u64),
                    (
                        "leaf_indices",
                        lines.iter().map(|l| l.leaf_indices().len() as u64).sum(),
                    ),
                ],
            )
        }
        "rest" => {
            let lines = pedestrian::parse_rest(data).map_err(e)?;
            out("rest", Class::Game, [("lines", lines.len() as u64)])
        }
        _ => other(rel, name, &ext, data),
    }
}

/// Extensionless saves (`SDAT`), numbered JPEG companions and the install
/// by-products. Each rule is an exact path or exact structural check, never a
/// blanket "unknown extension is fine".
fn other(rel: &str, name: &str, ext: &str, data: &[u8]) -> Result<Outcome, String> {
    if data.starts_with(b"SDAT") {
        let parsed = sdat::parse(data, None).map_err(e)?;
        if sdat::encode(&parsed) != data {
            return Err("SDAT writer does not reproduce the file byte for byte".into());
        }
        return out("sdat", Class::Game, [("roundtrip_files", 1)]);
    }
    if sdat::companion_index(name).is_some() && !ext.is_empty() {
        return jpeg(data).and_then(|(w, h)| {
            out(
                "sdat_companion_jpeg",
                Class::Companion,
                [("pixels", u64::from(w) * u64::from(h))],
            )
        });
    }
    install_artifact(rel, name, ext, data)
}

fn jpeg(data: &[u8]) -> Result<(u16, u16), String> {
    if !sdat::is_jpeg(data) || !data.ends_with(&[0xFF, 0xD9]) {
        return Err("not a complete JPEG (SOI..EOI)".into());
    }
    let (w, h, _) = slr_fmt_texture::jpeg_info(data)?;
    Ok((w, h))
}

fn manual(ext: &str, data: &[u8]) -> Result<Outcome, String> {
    match ext {
        "htm" | "xml" => {
            if data.is_empty() || data.contains(&0) {
                return Err("manual text file is empty or binary".into());
            }
            out(
                if ext == "htm" {
                    "manual_htm"
                } else {
                    "manual_xml"
                },
                Class::Documentation,
                [("bytes", data.len() as u64)],
            )
        }
        "gif" => {
            if data.len() < 10 || !(data.starts_with(b"GIF87a") || data.starts_with(b"GIF89a")) {
                return Err("bad GIF header".into());
            }
            let (w, h) = (
                u16::from_le_bytes([data[6], data[7]]),
                u16::from_le_bytes([data[8], data[9]]),
            );
            out(
                "manual_gif",
                Class::Documentation,
                [("pixels", u64::from(w) * u64::from(h))],
            )
        }
        "jpg" => {
            let (w, h) = jpeg(data)?;
            out(
                "manual_jpg",
                Class::Documentation,
                [("pixels", u64::from(w) * u64::from(h))],
            )
        }
        "db" => thumbs(data).and_then(|()| out("thumbs_db", Class::Documentation, [])),
        _ => Err(format!("unrecognised manual file extension {ext:?}")),
    }
}

/// Windows `Thumbs.db`: an OLE2 compound file (CFB signature).
fn thumbs(data: &[u8]) -> Result<(), String> {
    if data.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1]) {
        Ok(())
    } else {
        Err("Thumbs.db lacks the OLE2 signature".into())
    }
}

fn install_artifact(rel: &str, name: &str, ext: &str, data: &[u8]) -> Result<Outcome, String> {
    let lower = name.to_ascii_lowercase();
    if ext == "db" && lower == "thumbs.db" {
        return thumbs(data).and_then(|()| out("thumbs_db", Class::Documentation, []));
    }
    match (rel, ext) {
        ("Readme.txt", _) => {
            text(data).and_then(|()| out("readme_txt", Class::InstallArtifact, []))
        }
        ("Readme.ico", _) => {
            // ICONDIR: reserved 0, type 1, count > 0.
            if data.len() < 6 || data[..4] != [0, 0, 1, 0] || data[4] == 0 {
                return Err("bad ICO directory".into());
            }
            out(
                "readme_ico",
                Class::InstallArtifact,
                [("images", u64::from(data[4]))],
            )
        }
        (_, "url") => {
            let t = std::str::from_utf8(data).map_err(e)?;
            if !t.starts_with("[InternetShortcut]") || !t.contains("URL=") {
                return Err("not an InternetShortcut".into());
            }
            out("url_shortcut", Class::InstallArtifact, [])
        }
        ("INSTALL.LOG", _) => {
            text(data).and_then(|()| out("install_log", Class::InstallArtifact, []))
        }
        ("diag.log", _) | ("res_ban.log", _) => {
            // Written by the exe itself (`diag.log` at `FUN_00558e40`/CreateFileA,
            // `res_ban.log` in the engine-exit path `FUN_005360d0`).
            text(data).and_then(|()| out("exe_log", Class::InstallArtifact, []))
        }
        ("texture.dat", _) => {
            // The CD-check signature table of `FUN_00401480`: the track count
            // word followed by per-track words, 19 words in all (the check
            // compares word `count` against the drive's track count).
            if !data.len().is_multiple_of(4)
                || data.len() / 4 != 19
                || data[..4] != 19u32.to_le_bytes()
            {
                return Err("texture.dat is not the 19-word CD signature table".into());
            }
            out(
                "cd_signature",
                Class::InstallArtifact,
                [("words", (data.len() / 4) as u64)],
            )
        }
        ("frontend/font.dat", _) => {
            // Glyph character tables are unreferenced by the exe (no `font*.dat`
            // literal) and by every stock script; the fonts in use are SCX glyph
            // meshes. `font.dat` is the contiguous byte run 0x21..=0xa7.
            if !data.iter().copied().eq(0x21..=0xa7) {
                return Err("font.dat is not the contiguous run 0x21..=0xa7".into());
            }
            out(
                "glyph_table",
                Class::InstallArtifact,
                [("glyphs", data.len() as u64)],
            )
        }
        ("frontend/font_pointers.dat", _) | ("frontend/font_damage.dat", _) => {
            if data.is_empty() || !data.iter().all(u8::is_ascii_alphanumeric) {
                return Err("glyph pointer table holds non-alphanumeric bytes".into());
            }
            out(
                "glyph_table",
                Class::InstallArtifact,
                [("glyphs", data.len() as u64)],
            )
        }
        ("StreetLegal_Redline.exe", _) | ("StreetLegal_Redline.bak.exe", _) => {
            if data.len() < 0x40 || &data[..2] != b"MZ" {
                return Err("exe lacks the MZ header".into());
            }
            let pe = u32::from_le_bytes([data[0x3c], data[0x3d], data[0x3e], data[0x3f]]) as usize;
            if data.get(pe..pe + 4) != Some(b"PE\0\0") {
                return Err("exe lacks the PE signature".into());
            }
            out(
                "exe",
                Class::InstallArtifact,
                [("bytes", data.len() as u64)],
            )
        }
        _ => Err("no decoder or explanation for this file".into()),
    }
}

fn text(data: &[u8]) -> Result<(), String> {
    if data.is_empty() || data.contains(&0) {
        Err("text file is empty or binary".into())
    } else {
        Ok(())
    }
}

/// Classify and decode every file under `root`.
pub fn run(root: &Path) -> std::io::Result<Census> {
    let mut census = Census::default();
    for path in files(root)? {
        let rel = rel(root, &path);
        let data = std::fs::read(&path)?;
        census.files += 1;
        census.bytes += data.len() as u64;
        match decode(&rel, &data) {
            Ok(outcome) => {
                let entry = census.decoders.entry(outcome.decoder).or_default();
                entry.class = Some(outcome.class);
                entry.files += 1;
                entry.bytes += data.len() as u64;
                for (key, n) in outcome.units {
                    *entry.units.entry(key).or_default() += n;
                }
            }
            Err(reason) => census.unexplained.push((rel, reason)),
        }
    }
    Ok(census)
}

impl Census {
    /// Stable JSON report (no stock content: counts only).
    pub fn to_json(&self) -> serde_json::Value {
        let decoders: serde_json::Map<String, serde_json::Value> = self
            .decoders
            .iter()
            .map(|(name, entry)| {
                (
                    (*name).to_string(),
                    serde_json::json!({
                        "class": entry.class.map(Class::name),
                        "files": entry.files,
                        "bytes": entry.bytes,
                        "units": entry.units,
                    }),
                )
            })
            .collect();
        serde_json::json!({
            "schema": 1,
            "files": self.files,
            "bytes": self.bytes,
            "decoders": decoders,
            "unexplained": self.unexplained.iter().map(|(p, r)| serde_json::json!({"path": p, "reason": r})).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unknown_files_and_spoofed_media_are_unexplained() {
        assert!(decode("unknown.bin", b"arbitrary bytes").is_err());
        assert!(decode("texture.png", b"not png").is_err());
        assert!(decode("garage/0.1", b"not jpeg").is_err());
        assert!(decode("SLR Manual_files/a.gif", b"invalid").is_err());
        assert!(decode("texture.dat", &[0; 76]).is_err());
    }
    #[test]
    fn exact_install_rules_do_not_explain_unrelated_files() {
        assert_eq!(
            decode("Readme.txt", b"read me").unwrap().decoder,
            "readme_txt"
        );
        assert!(decode("elsewhere/Readme.txt", b"read me").is_err());
        assert!(decode("Readme.txt", b"read\0me").is_err());
        assert_eq!(
            decode(
                "texture.dat",
                &[19u32.to_le_bytes(), [0; 4]]
                    .concat()
                    .into_iter()
                    .chain([0; 68])
                    .collect::<Vec<_>>()
            )
            .unwrap()
            .decoder,
            "cd_signature"
        );
    }
}

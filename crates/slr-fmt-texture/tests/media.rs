//! Corpus tests for the standard-media wrappers (M2-11): every shipped
//! DDS/PNG/TGA decodes without error and its decoded dimensions/format match
//! the file header *as read independently here from the raw bytes*.
//!
//! Pixels are tier D (ADR 0009) and never compared. Roots default to the
//! workspace `stock/` and `reference/raw`; override with `SLR_STOCK` /
//! `SLR_REFERENCE`. Tests skip with a note when the data is absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root(var: &str, rel: &str) -> Option<PathBuf> {
    let p = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel));
    p.is_dir().then_some(p)
}

fn stock_root() -> Option<PathBuf> {
    root("SLR_STOCK", "../../stock")
}

fn modded_root() -> Option<PathBuf> {
    root("SLR_REFERENCE", "../../reference/raw")
}

fn walk(root: &Path, ext: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap() {
            let p = e.unwrap().path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| e.eq_ignore_ascii_case(ext))
            {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn u32le(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

/// Returns (census by fourcc, total mips decoded).
fn check_dds(files: &[PathBuf], allow_unsupported: bool) -> (BTreeMap<String, usize>, usize) {
    let mut census = BTreeMap::new();
    let mut mips = 0;
    for f in files {
        let data = std::fs::read(f).unwrap();
        let ctx = f.display();
        // Independent raw-header reads: width@16, height@12, mips@28, fourcc@84.
        assert_eq!(&data[..4], b"DDS ", "{ctx}");
        let (rh, rw) = (u32le(&data, 12), u32le(&data, 16));
        let rmips = u32le(&data, 28).max(1);
        let fourcc = String::from_utf8_lossy(&data[84..88]).into_owned();
        let dds = match slr_fmt_texture::dds::parse(&data) {
            Ok(d) => d,
            // Modded-only content (uncompressed cubemaps etc.) is outside
            // the DXT1/3/5 scope of M2-11; count it, never hide other errors.
            Err(e) if allow_unsupported && e.to_string().contains("unsupported") => {
                *census.entry("unsupported".to_string()).or_insert(0) += 1;
                continue;
            }
            Err(e) => panic!("{ctx}: {e}"),
        };
        assert_eq!(
            (dds.width, dds.height, dds.mip_count),
            (rw, rh, rmips),
            "{ctx}"
        );
        let want = match fourcc.as_str() {
            "DXT1" => slr_fmt_texture::dds::DdsFormat::Dxt1,
            "DXT3" => slr_fmt_texture::dds::DdsFormat::Dxt3,
            "DXT5" => slr_fmt_texture::dds::DdsFormat::Dxt5,
            other => panic!("{ctx}: unexpected fourcc {other}"),
        };
        assert_eq!(dds.format, want, "{ctx}");
        for l in 0..dds.mip_count {
            let img = dds
                .decode_mip(l)
                .unwrap_or_else(|e| panic!("{ctx} mip {l}: {e}"));
            assert_eq!(
                (img.width, img.height),
                ((rw >> l).max(1), (rh >> l).max(1)),
                "{ctx} mip {l}"
            );
            assert_eq!(
                img.pixels.len(),
                img.width as usize * img.height as usize * 4
            );
            mips += 1;
        }
        *census.entry(fourcc).or_insert(0) += 1;
    }
    (census, mips)
}

/// Returns census keyed by "<colortype> <bitdepth>".
fn check_png(files: &[PathBuf]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for f in files {
        let data = std::fs::read(f).unwrap();
        let ctx = f.display();
        // Independent IHDR read: signature, then width@16, height@20, depth@24, colour type@25.
        assert_eq!(&data[..8], b"\x89PNG\r\n\x1a\n", "{ctx}");
        assert_eq!(&data[12..16], b"IHDR", "{ctx}");
        let (rw, rh) = (
            u32::from_be_bytes(data[16..20].try_into().unwrap()),
            u32::from_be_bytes(data[20..24].try_into().unwrap()),
        );
        let (depth, ct) = (data[24], data[25]);
        let (info, img) =
            slr_fmt_texture::pngdec::decode(&data).unwrap_or_else(|e| panic!("{ctx}: {e}"));
        assert_eq!((info.width, info.height), (rw, rh), "{ctx}");
        assert_eq!((img.width, img.height), (rw, rh), "{ctx}");
        assert_eq!(info.bit_depth as u8, depth, "{ctx}");
        assert_eq!(info.color_type as u8, ct, "{ctx}");
        assert_eq!(img.pixels.len(), rw as usize * rh as usize * 4, "{ctx}");
        *census
            .entry(format!("{:?} {depth}", info.color_type))
            .or_insert(0) += 1;
    }
    census
}

/// Returns census keyed by "type<N> <bits>bpp".
fn check_tga(files: &[PathBuf]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for f in files {
        let data = std::fs::read(f).unwrap();
        let ctx = f.display();
        // Independent header read (TGA 1.0 layout).
        let (img_type, rw, rh, bpp) = (
            data[2],
            u16::from_le_bytes([data[12], data[13]]),
            u16::from_le_bytes([data[14], data[15]]),
            data[16],
        );
        let (info, img) =
            slr_fmt_texture::tga::decode(&data).unwrap_or_else(|e| panic!("{ctx}: {e}"));
        assert_eq!(
            (info.width, info.height),
            (u32::from(rw), u32::from(rh)),
            "{ctx}"
        );
        assert_eq!(
            (img.width, img.height),
            (u32::from(rw), u32::from(rh)),
            "{ctx}"
        );
        assert_eq!(info.pixel_bits, bpp, "{ctx}");
        assert!(!info.rle, "{ctx}");
        *census
            .entry(format!("type{img_type} {bpp}bpp"))
            .or_insert(0) += 1;
    }
    census
}

fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn stock_dds_all_decode() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk(&root, "dds");
    assert_eq!(files.len(), 1462, "stock dds file count");
    let (census, mips) = check_dds(&files, false);
    eprintln!(
        "stock dds: files={} mips={mips} census={census:?}",
        files.len()
    );
    assert_eq!(
        census,
        counts(&[("DXT1", 1138), ("DXT3", 122), ("DXT5", 202)])
    );
}

#[test]
fn stock_png_all_decode() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk(&root, "png");
    assert_eq!(files.len(), 370, "stock png file count");
    let census = check_png(&files);
    eprintln!("stock png: files={} census={census:?}", files.len());
    assert_eq!(
        census,
        counts(&[("Rgb 8", 103), ("Rgba 8", 257), ("GrayscaleAlpha 8", 10)])
    );
}

#[test]
fn stock_tga_all_decode() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk(&root, "tga");
    assert_eq!(files.len(), 12, "stock tga file count");
    let census = check_tga(&files);
    eprintln!("stock tga: files={} census={census:?}", files.len());
    assert_eq!(
        census,
        counts(&[("type1 8bpp", 7), ("type2 32bpp", 2), ("type3 8bpp", 3)])
    );
}

/// Modded profile (reference/raw) is a compat check only: everything that
/// ships there must also decode, except formats outside DXT1/3/5 which are
/// counted as `unsupported`. Counts are printed, not asserted.
#[test]
fn modded_media_all_decode() {
    let Some(root) = modded_root() else {
        eprintln!("SKIP: no reference dir (set SLR_REFERENCE)");
        return;
    };
    let (d, p, t) = (walk(&root, "dds"), walk(&root, "png"), walk(&root, "tga"));
    let (dc, dm) = check_dds(&d, true);
    let pc = check_png(&p);
    let tc = check_tga(&t);
    eprintln!(
        "modded: dds={} (mips {dm}) {dc:?}; png={} {pc:?}; tga={} {tc:?}",
        d.len(),
        p.len(),
        t.len()
    );
}

//! Corpus tests: the shipped `Data/Fmv/*.avi` files.
//!
//! Acceptance (ticket M2-12): all stock AVIs decode every frame and the
//! frame count equals the AVI index. Roots default to the workspace
//! `stock/` and `reference/raw`; override with `SLR_STOCK` /
//! `SLR_REFERENCE`. Tests skip with a note when the data is absent.
//!
//! The exe plays video through WINMM (Windows' own Cinepak codec), so no
//! exe oracle exists. The pixel check is a differential test against
//! FFmpeg's independent Cinepak decoder (`ffmpeg` on `PATH`, or
//! `SLR_FFMPEG`); it skips with a note when FFmpeg is absent.

use slr_fmt_video::avi::Avi;
use slr_fmt_video::cinepak::Decoder;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn root(var: &str, default: &str) -> Option<PathBuf> {
    let p = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(default));
    p.is_dir().then_some(p)
}

fn stock_root() -> Option<PathBuf> {
    root("SLR_STOCK", "../../stock")
}

fn modded_root() -> Option<PathBuf> {
    root("SLR_REFERENCE", "../../reference/raw")
}

fn avis(root: &Path) -> Vec<PathBuf> {
    let dir = root.join("Data/Fmv");
    let mut v: Vec<PathBuf> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.map(|e| e.unwrap().path())
                .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("avi")))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    v
}

/// Decode every frame; returns `(frames, width, height)` after checking all
/// the count invariants.
fn check_file(path: &Path) -> (usize, usize, usize) {
    let name = path.display();
    let data = std::fs::read(path).unwrap();
    let avi = Avi::parse(&data).unwrap_or_else(|e| panic!("{name}: {e}"));
    let v = avi
        .video_stream()
        .unwrap_or_else(|| panic!("{name}: no video stream"));
    assert_eq!(&v.compression, b"cvid", "{name}: codec");
    assert_eq!(v.bit_count, 24, "{name}: depth");
    let (w, h) = (v.width as usize, v.height as usize);
    assert_eq!(
        (avi.header.width as usize, avi.header.height as usize),
        (w, h),
        "{name}"
    );
    avi.verify_index().unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut dec = Decoder::new(w, h).unwrap_or_else(|e| panic!("{name}: {e}"));
    let mut n = 0usize;
    for (i, f) in avi.video_frames().enumerate() {
        dec.decode(f)
            .unwrap_or_else(|e| panic!("{name}: frame {i}: {e}"));
        n += 1;
    }
    let idx = avi.index_video_count();
    assert_eq!(n, idx, "{name}: decoded frames vs idx1 video chunks");
    assert_eq!(
        n, avi.header.total_frames as usize,
        "{name}: vs avih.totalFrames"
    );
    assert_eq!(n, v.length as usize, "{name}: vs strh.dwLength");
    if let Some(t) = avi.odml_total_frames {
        assert_eq!(n, t as usize, "{name}: vs dmlh.dwTotalFrames");
    }
    (n, w, h)
}

fn check_root(label: &str, root: &Path) {
    let files = avis(root);
    assert_eq!(
        files.len(),
        3,
        "{label}: expected Activision/Invictus/Prime.avi"
    );
    let mut total = 0;
    for f in &files {
        let (n, w, h) = check_file(f);
        eprintln!(
            "{label}: {}: {n} frames {w}x{h}",
            f.file_name().unwrap().to_string_lossy()
        );
        total += n;
    }
    eprintln!("{label}: {} files, {total} frames", files.len());
}

#[test]
fn stock_avis_decode_every_frame_and_match_the_index() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock/ directory (set SLR_STOCK)");
        return;
    };
    check_root("stock", &root);
}

#[test]
fn modded_avis_decode_every_frame_and_match_the_index() {
    let Some(root) = modded_root() else {
        eprintln!("SKIP: no reference/raw directory (set SLR_REFERENCE)");
        return;
    };
    check_root("modded", &root);
}

fn ffmpeg() -> Option<PathBuf> {
    let c = std::env::var_os("SLR_FFMPEG")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ffmpeg"));
    Command::new(&c)
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .ok()
        .filter(|s| s.success())
        .map(|_| c)
}

#[test]
fn pixels_equal_ffmpeg_cinepak_decoder_for_every_frame() {
    let Some(root) = stock_root() else {
        eprintln!("SKIP: no stock/ directory (set SLR_STOCK)");
        return;
    };
    let Some(ff) = ffmpeg() else {
        eprintln!("SKIP: no ffmpeg (put it on PATH or set SLR_FFMPEG)");
        return;
    };
    for path in avis(&root) {
        let data = std::fs::read(&path).unwrap();
        let avi = Avi::parse(&data).unwrap();
        let v = avi.video_stream().unwrap();
        let (w, h) = (v.width as usize, v.height as usize);
        let mut child = Command::new(&ff)
            .args(["-v", "error", "-i"])
            .arg(&path)
            .args([
                "-map",
                "0:v:0",
                "-fps_mode",
                "passthrough",
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let mut out = child.stdout.take().unwrap();
        let mut dec = Decoder::new(w, h).unwrap();
        let mut want = vec![0u8; w * h * 3];
        let mut frames = 0usize;
        for (i, f) in avi.video_frames().enumerate() {
            dec.decode(f).unwrap();
            out.read_exact(&mut want)
                .unwrap_or_else(|e| panic!("{}: ffmpeg frame {i}: {e}", path.display()));
            let diff = dec
                .frame()
                .iter()
                .zip(&want)
                .filter(|(a, b)| a != b)
                .count();
            assert_eq!(
                diff,
                0,
                "{}: frame {i}: {diff} differing bytes",
                path.display()
            );
            frames += 1;
        }
        let mut rest = Vec::new();
        out.read_to_end(&mut rest).unwrap();
        assert!(
            rest.is_empty(),
            "{}: ffmpeg produced extra frames",
            path.display()
        );
        assert!(child.wait().unwrap().success());
        eprintln!(
            "ffmpeg oracle: {}: {frames} frames byte-identical ({w}x{h} rgb24)",
            path.file_name().unwrap().to_string_lossy()
        );
    }
}

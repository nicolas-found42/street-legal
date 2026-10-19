//! Corpus tests for the WAV/MP3 wrappers (M2-11): every shipped file decodes
//! and the decoded format matches the file header as read independently here
//! from the raw bytes. Samples are tier D (ADR 0009) and never compared.
//!
//! Roots default to the workspace `stock/` and `reference/raw`; override with
//! `SLR_STOCK` / `SLR_REFERENCE`. Tests skip with a note when absent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root(var: &str, rel: &str) -> Option<PathBuf> {
    let p = std::env::var_os(var)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel));
    p.is_dir().then_some(p)
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

fn le16(d: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(d[at..at + 2].try_into().unwrap())
}
fn le32(d: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(d[at..at + 4].try_into().unwrap())
}

/// Independent RIFF walk: (format tag, channels, rate, bits, data bytes).
fn raw_wav(d: &[u8]) -> (u16, u16, u32, u16, u32) {
    assert_eq!(&d[..4], b"RIFF");
    assert_eq!(&d[8..12], b"WAVE");
    let (mut p, mut fmt, mut data) = (12, None, None);
    while p + 8 <= d.len() {
        let size = le32(d, p + 4);
        match &d[p..p + 4] {
            b"fmt " => {
                fmt = Some((
                    le16(d, p + 8),
                    le16(d, p + 10),
                    le32(d, p + 12),
                    le16(d, p + 22),
                ))
            }
            b"data" => data = Some(size),
            _ => {}
        }
        p += 8 + size as usize + (size as usize & 1);
    }
    let (tag, ch, rate, bits) = fmt.expect("fmt chunk");
    (tag, ch, rate, bits, data.expect("data chunk"))
}

fn check_wav(files: &[PathBuf]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for f in files {
        let d = std::fs::read(f).unwrap();
        let ctx = f.display();
        let (tag, ch, rate, bits, bytes) = raw_wav(&d);
        assert_eq!(tag, 1, "{ctx}: PCM");
        let (info, pcm) = slr_fmt_sound::wav::decode(&d).unwrap_or_else(|e| panic!("{ctx}: {e}"));
        let frames = bytes / (u32::from(ch) * u32::from(bits) / 8);
        assert_eq!(
            (
                info.channels,
                info.sample_rate,
                info.bits_per_sample,
                info.frames
            ),
            (ch, rate, bits, frames),
            "{ctx}"
        );
        assert_eq!((pcm.channels, pcm.sample_rate), (ch, rate), "{ctx}");
        assert_eq!(
            pcm.samples.len(),
            frames as usize * usize::from(ch),
            "{ctx}"
        );
        *census
            .entry(format!("{rate}Hz {ch}ch {bits}bit"))
            .or_insert(0) += 1;
    }
    census
}

/// Independent MPEG-audio header walk from the first sync after any ID3v2
/// tag: returns (frames, sample rate, channels, samples/frame, first frame
/// offset). Layer III only.
fn raw_mp3(d: &[u8]) -> (usize, u32, u16, usize, usize) {
    const B1: [u32; 16] = [
        0, 32, 40, 48, 56, 64, 80, 96, 112, 128, 160, 192, 224, 256, 320, 0,
    ];
    const B2: [u32; 16] = [
        0, 8, 16, 24, 32, 40, 48, 56, 64, 80, 96, 112, 128, 144, 160, 0,
    ];
    let mut p = 0;
    if &d[..3] == b"ID3" {
        p = 10
            + (((d[6] & 0x7F) as usize) << 21
                | ((d[7] & 0x7F) as usize) << 14
                | ((d[8] & 0x7F) as usize) << 7
                | (d[9] & 0x7F) as usize);
    }
    while !(d[p] == 0xFF && d[p + 1] & 0xE0 == 0xE0) {
        p += 1;
    }
    let first = p;
    let (mut n, mut fmt) = (0, None);
    while p + 4 <= d.len() {
        let h = u32::from_be_bytes(d[p..p + 4].try_into().unwrap());
        if h >> 21 != 0x7FF {
            break;
        }
        let (ver, layer) = ((h >> 19) & 3, (h >> 17) & 3);
        let (br, sr, pad, chm) = ((h >> 12) & 15, (h >> 10) & 3, (h >> 9) & 1, (h >> 6) & 3);
        assert_eq!(layer, 1, "Layer III only");
        let (len, rate, spf) = match ver {
            3 => (
                144 * B1[br as usize] * 1000 / [44100, 48000, 32000][sr as usize] + pad,
                [44100, 48000, 32000][sr as usize],
                1152,
            ),
            2 => (
                72 * B2[br as usize] * 1000 / [22050, 24000, 16000][sr as usize] + pad,
                [22050, 24000, 16000][sr as usize],
                576,
            ),
            _ => panic!("unexpected MPEG version {ver}"),
        };
        let ch = if chm == 3 { 1 } else { 2 };
        match fmt {
            None => fmt = Some((rate, ch, spf)),
            Some(f) => assert_eq!(f, (rate, ch, spf), "format change"),
        }
        p += len as usize;
        n += 1;
    }
    let (rate, ch, spf) = fmt.unwrap();
    (n, rate, ch, spf, first)
}

fn check_mp3(files: &[PathBuf]) -> BTreeMap<String, usize> {
    let mut census = BTreeMap::new();
    for f in files {
        let d = std::fs::read(f).unwrap();
        let ctx = f.display();
        let (frames, rate, ch, spf, _first) = raw_mp3(&d);
        let (info, pcm) = slr_fmt_sound::mp3::decode(&d).unwrap_or_else(|e| panic!("{ctx}: {e}"));
        assert_eq!(
            (
                info.frames,
                info.sample_rate,
                info.channels,
                info.samples_per_frame
            ),
            (frames, rate, ch, spf),
            "{ctx}: header walk"
        );
        assert_eq!((pcm.sample_rate, pcm.channels), (rate, ch), "{ctx}");
        // Decoded length: all frames minus the optional Xing/Info tag frame,
        // minus at most the encoder delay + padding (< 3 frames in practice).
        let per_ch = pcm.frames();
        eprintln!(
            "{ctx}: frames={frames} decoded_per_ch={per_ch} full={}",
            frames * spf
        );
        assert!(per_ch <= frames * spf, "{ctx}");
        assert!(per_ch + 3 * spf >= frames * spf, "{ctx}");
        *census.entry(format!("{rate}Hz {ch}ch")).or_insert(0) += 1;
    }
    census
}

fn counts(pairs: &[(&str, usize)]) -> BTreeMap<String, usize> {
    pairs.iter().map(|(k, v)| (k.to_string(), *v)).collect()
}

#[test]
fn stock_wav_all_decode() {
    let Some(root) = root("SLR_STOCK", "../../stock") else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk(&root, "wav");
    assert_eq!(files.len(), 147, "stock wav file count");
    let census = check_wav(&files);
    eprintln!("stock wav: files={} census={census:?}", files.len());
    assert_eq!(
        census,
        counts(&[
            ("11025Hz 1ch 16bit", 11),
            ("11025Hz 1ch 8bit", 2),
            ("22050Hz 1ch 16bit", 69),
            ("22050Hz 1ch 8bit", 1),
            ("32000Hz 1ch 16bit", 4),
            ("44100Hz 1ch 16bit", 30),
            ("44100Hz 1ch 8bit", 2),
            ("44100Hz 2ch 16bit", 26),
            ("44100Hz 2ch 8bit", 2),
        ])
    );
}

#[test]
fn stock_mp3_all_decode() {
    let Some(root) = root("SLR_STOCK", "../../stock") else {
        eprintln!("SKIP: no stock dir (set SLR_STOCK)");
        return;
    };
    let files = walk(&root, "mp3");
    assert_eq!(files.len(), 15, "stock mp3 file count");
    let census = check_mp3(&files);
    eprintln!("stock mp3: files={} census={census:?}", files.len());
    assert_eq!(census, counts(&[("22050Hz 2ch", 3), ("44100Hz 2ch", 12)]));
}

/// Modded profile: compat check only, counts printed.
#[test]
fn modded_audio_all_decode() {
    let Some(root) = root("SLR_REFERENCE", "../../reference/raw") else {
        eprintln!("SKIP: no reference dir (set SLR_REFERENCE)");
        return;
    };
    let (w, m) = (walk(&root, "wav"), walk(&root, "mp3"));
    let (wc, mc) = (check_wav(&w), check_mp3(&m));
    eprintln!("modded: wav={} {wc:?}; mp3={} {mc:?}", w.len(), m.len());
}

//! Container-level behaviour of the AVI/RIFF demuxer on hand-built files
//! (data-free; the stock corpus lives in `tests/corpus.rs`).

use slr_fmt_video::avi::Avi;

fn chunk(id: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut v = id.to_vec();
    v.extend((body.len() as u32).to_le_bytes());
    v.extend(body);
    if body.len() % 2 == 1 {
        v.push(0);
    }
    v
}

fn list(kind: &[u8; 4], list_type: &[u8; 4], body: &[u8]) -> Vec<u8> {
    let mut inner = list_type.to_vec();
    inner.extend(body);
    chunk(kind, &inner)
}

fn avih(us: u32, total: u32, w: u32, h: u32) -> Vec<u8> {
    let mut b = Vec::new();
    for v in [us, 0, 0, 0x10, total, 0, 1, 0, w, h, 0, 0, 0, 0] {
        b.extend(v.to_le_bytes());
    }
    chunk(b"avih", &b)
}

fn strh(kind: &[u8; 4], handler: &[u8; 4], scale: u32, rate: u32, length: u32) -> Vec<u8> {
    let mut b = kind.to_vec();
    b.extend(handler);
    for v in [0u32, 0, 0, scale, rate, 0, length, 0, 0, 0] {
        b.extend(v.to_le_bytes());
    }
    b.extend([0u8; 8]); // rcFrame
    chunk(b"strh", &b)
}

fn vids_strf(w: u32, h: u32, comp: &[u8; 4]) -> Vec<u8> {
    let mut b = Vec::new();
    b.extend(40u32.to_le_bytes());
    b.extend(w.to_le_bytes());
    b.extend(h.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(24u16.to_le_bytes());
    b.extend(comp);
    b.extend([0u8; 20]);
    chunk(b"strf", &b)
}

fn auds_strf() -> Vec<u8> {
    chunk(
        b"strf",
        &[1, 0, 2, 0, 0x44, 0xac, 0, 0, 0x10, 0xb1, 2, 0, 4, 0, 16, 0],
    )
}

/// Two streams: 0 = video 8x4 cvid, 1 = audio. `video` and `audio` are chunk
/// payloads in movi order.
struct Built {
    bytes: Vec<u8>,
}

fn build(video: &[&[u8]], with_idx1: bool, dml_total: Option<u32>) -> Built {
    let mut strl_v = strh(b"vids", b"cvid", 1001, 30000, video.len() as u32);
    strl_v.extend(vids_strf(8, 4, b"cvid"));
    let mut strl_a = strh(b"auds", b"\0\0\0\0", 1, 44100, 0);
    strl_a.extend(auds_strf());
    let mut hdrl = avih(33367, video.len() as u32, 8, 4);
    hdrl.extend(list(b"LIST", b"strl", &strl_v));
    hdrl.extend(list(b"LIST", b"strl", &strl_a));
    if let Some(t) = dml_total {
        let mut dmlh = t.to_le_bytes().to_vec();
        dmlh.extend([0u8; 244]);
        hdrl.extend(list(b"LIST", b"odml", &chunk(b"dmlh", &dmlh)));
    }
    let mut movi = Vec::new();
    let mut idx = Vec::new();
    let mut off = 4u32; // relative to the 'movi' fourcc
    for (i, f) in video.iter().enumerate() {
        let c = chunk(b"00dc", f);
        idx.extend(b"00dc");
        idx.extend(0x10u32.to_le_bytes());
        idx.extend(off.to_le_bytes());
        idx.extend((f.len() as u32).to_le_bytes());
        off += c.len() as u32;
        movi.extend(c);
        let a = chunk(b"01wb", &[i as u8; 6]);
        idx.extend(b"01wb");
        idx.extend(0x10u32.to_le_bytes());
        idx.extend(off.to_le_bytes());
        idx.extend(6u32.to_le_bytes());
        off += a.len() as u32;
        movi.extend(a);
    }
    let mut body = list(b"LIST", b"hdrl", &hdrl);
    body.extend(chunk(b"JUNK", &[0; 5]));
    body.extend(list(b"LIST", b"movi", &movi));
    if with_idx1 {
        body.extend(chunk(b"idx1", &idx));
    }
    Built {
        bytes: list(b"RIFF", b"AVI ", &body),
    }
}

#[test]
fn parses_header_streams_and_video_chunks() {
    let b = build(&[b"aaa", b"bbbb", b"c"], true, None);
    let avi = Avi::parse(&b.bytes).unwrap();
    assert_eq!(avi.header.micros_per_frame, 33367);
    assert_eq!(avi.header.total_frames, 3);
    assert_eq!((avi.header.width, avi.header.height), (8, 4));
    assert_eq!(avi.streams.len(), 2);
    let v = avi.video_stream().unwrap();
    assert_eq!(v.fcc_handler, *b"cvid");
    assert_eq!(v.compression, *b"cvid");
    assert_eq!((v.scale, v.rate, v.length), (1001, 30000, 3));
    let frames: Vec<&[u8]> = avi.video_frames().collect();
    assert_eq!(frames, vec![&b"aaa"[..], b"bbbb", b"c"]);
    assert_eq!(avi.audio_chunks().count(), 3);
}

#[test]
fn idx1_entries_resolve_to_movi_chunks() {
    let b = build(&[b"aaa", b"bbbb"], true, None);
    let avi = Avi::parse(&b.bytes).unwrap();
    assert_eq!(avi.index.len(), 4);
    assert_eq!(avi.index_video_count(), 2);
    avi.verify_index().unwrap();
}

#[test]
fn bad_idx1_offset_is_rejected() {
    let mut b = build(&[b"aaa", b"bbbb"], true, None);
    // corrupt the first idx1 entry's offset field
    let at = b.bytes.len() - 4 * 16 + 8;
    b.bytes[at] ^= 0x40;
    let avi = Avi::parse(&b.bytes).unwrap();
    assert!(avi.verify_index().is_err());
}

#[test]
fn opendml_dmlh_total_and_avix_continuation() {
    let mut b = build(&[b"aa", b"bb"], false, Some(3)).bytes;
    // second segment: RIFF AVIX holding a third frame
    let c = chunk(b"00dc", b"cc");
    let mut movi = Vec::new();
    movi.extend(c);
    movi.extend(chunk(b"ix00", &[0; 8])); // standard index chunk is skipped
    b.extend(list(b"RIFF", b"AVIX", &list(b"LIST", b"movi", &movi)));
    let avi = Avi::parse(&b).unwrap();
    assert_eq!(avi.header.total_frames, 2);
    assert_eq!(avi.odml_total_frames, Some(3));
    let frames: Vec<&[u8]> = avi.video_frames().collect();
    assert_eq!(frames, vec![&b"aa"[..], b"bb", b"cc"]);
}

#[test]
fn rec_lists_and_odd_sizes_are_walked() {
    let mut rec = chunk(b"00dc", b"xyz");
    rec.extend(chunk(b"01wb", b"w"));
    let mut hdrl = avih(40000, 2, 8, 4);
    let mut s = strh(b"vids", b"cvid", 1, 25, 2);
    s.extend(vids_strf(8, 4, b"cvid"));
    hdrl.extend(list(b"LIST", b"strl", &s));
    let mut movi = list(b"LIST", b"rec ", &rec);
    movi.extend(chunk(b"00db", b"q")); // db = uncompressed-flagged, same stream
    let mut body = list(b"LIST", b"hdrl", &hdrl);
    body.extend(list(b"LIST", b"movi", &movi));
    let bytes = list(b"RIFF", b"AVI ", &body);
    let avi = Avi::parse(&bytes).unwrap();
    let frames: Vec<&[u8]> = avi.video_frames().collect();
    assert_eq!(frames, vec![&b"xyz"[..], b"q"]);
}

#[test]
fn not_riff_and_truncation_are_errors() {
    assert!(Avi::parse(b"RIFX\0\0\0\0AVI ").is_err());
    assert!(Avi::parse(b"").is_err());
    let b = build(&[b"aaa"], true, None).bytes;
    assert!(Avi::parse(&b[..b.len() / 2]).is_err());
    let mut wave = b.clone();
    wave[8..12].copy_from_slice(b"WAVE");
    assert!(Avi::parse(&wave).is_err());
}

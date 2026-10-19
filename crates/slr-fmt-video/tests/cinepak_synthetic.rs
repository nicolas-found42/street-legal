//! Cinepak (`cvid`) decoding on hand-built frames. Expected pixels are
//! worked out by hand from the documented transform
//! `r = y + 2v, g = y - trunc(u / 2) - v, b = y + 2u` (u, v signed bytes,
//! results clipped to 0..=255), not recomputed with the decoder's formula.

use slr_fmt_video::cinepak::Decoder;

fn be16(v: u16) -> [u8; 2] {
    v.to_be_bytes()
}

/// Chunk and strip headers: one id byte and a 24-bit big-endian size that
/// counts the header itself.
fn hdr(id: u8, total: usize) -> Vec<u8> {
    vec![id, (total >> 16) as u8, (total >> 8) as u8, total as u8]
}

fn chunk(id: u8, payload: &[u8]) -> Vec<u8> {
    let mut v = hdr(id, payload.len() + 4);
    v.extend(payload);
    v
}

/// A strip covering columns `0..w` and rows `y1..y2` (`y1 == 0` means
/// "directly below the previous strip", `y2` is then the height).
fn strip(id: u8, y1: u16, y2: u16, w: u16, chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut v = hdr(id, body.len() + 12);
    for x in [y1, 0, y2, w] {
        v.extend(be16(x));
    }
    v.extend(body);
    v
}

fn frame(flags: u8, w: u16, h: u16, strips: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = strips.concat();
    let total = body.len() + 10;
    let mut v = vec![flags];
    v.extend(&(total as u32).to_be_bytes()[1..]);
    v.extend(be16(w));
    v.extend(be16(h));
    v.extend(be16(strips.len() as u16));
    v.extend(body);
    v
}

/// Flatten rows of RGB triples into the decoder's RGB24 layout.
fn rgb(rows: &[&[(u8, u8, u8)]]) -> Vec<u8> {
    rows.iter()
        .flat_map(|r| r.iter().flat_map(|&(r, g, b)| [r, g, b]))
        .collect()
}

const GRAY: [u8; 6] = [10, 20, 30, 40, 0, 0];
const COLOR: [u8; 6] = [100, 100, 100, 100, 10, 0xEC]; // u=10, v=-20

#[test]
fn v1_codebook_expands_2x2_to_4x4_with_colour_transform() {
    let mut cb = GRAY.to_vec();
    cb.extend(COLOR);
    let f = frame(
        1,
        8,
        4,
        &[strip(
            0x10,
            0,
            4,
            8,
            &[chunk(0x22, &cb), chunk(0x32, &[0, 1])],
        )],
    );
    let mut d = Decoder::new(8, 4).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let c = (60, 115, 120);
    let top = [g(10), g(10), g(20), g(20), c, c, c, c];
    let bot = [g(30), g(30), g(40), g(40), c, c, c, c];
    assert_eq!(d.frame(), rgb(&[&top, &top, &bot, &bot]));
}

#[test]
fn v4_blocks_use_four_codebook_entries_one_per_quadrant() {
    // V1 book unused; V4 book entries 0..4: flat greys 1,2,3,4 (u=v=0).
    let mut v4 = Vec::new();
    for y in [1u8, 2, 3, 4] {
        v4.extend([y, y, y, y, 0, 0]);
    }
    // 0x30: per block a flag bit, 1 => V4 (4 index bytes TL,TR,BL,BR)
    let flags = 0x8000_0000u32.to_be_bytes();
    let mut vec_payload = flags.to_vec();
    vec_payload.extend([0, 1, 2, 3]);
    let f = frame(
        1,
        4,
        4,
        &[strip(
            0x10,
            0,
            4,
            4,
            &[chunk(0x20, &v4), chunk(0x30, &vec_payload)],
        )],
    );
    let mut d = Decoder::new(4, 4).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let top = [g(1), g(1), g(2), g(2)];
    let bot = [g(3), g(3), g(4), g(4)];
    assert_eq!(d.frame(), rgb(&[&top, &top, &bot, &bot]));
}

#[test]
fn v4_entry_pixels_are_row_major_within_each_2x2() {
    let mut v4 = vec![10u8, 20, 30, 40, 0, 0]; // entry 0 distinct per pixel
    v4.extend([0u8; 18]); // entries 1..3 black
    let mut p = 0x8000_0000u32.to_be_bytes().to_vec();
    p.extend([0, 1, 2, 3]);
    let f = frame(
        1,
        4,
        4,
        &[strip(0x10, 0, 4, 4, &[chunk(0x20, &v4), chunk(0x30, &p)])],
    );
    let mut d = Decoder::new(4, 4).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let z = g(0);
    let r0 = [g(10), g(20), z, z];
    let r1 = [g(30), g(40), z, z];
    let r2 = [z; 4];
    assert_eq!(d.frame(), rgb(&[&r0, &r1, &r2, &r2]));
}

#[test]
fn colour_transform_clips_and_truncates_negative_u_half_toward_zero() {
    // (y,u,v): (50,-3,0) -> u/2 = -1 -> (50,51,44)
    //          (250,100,100) -> (255,100,255)   (r,b clipped high)
    //          (5,-100,-100) -> (0,155,0)       (r,b clipped low)
    //          (0,1,0) -> u/2 = 0 -> (0,0,2)
    let mut cb = Vec::new();
    for (y, u, v) in [
        (50u8, -3i8, 0i8),
        (250, 100, 100),
        (5, -100, -100),
        (0, 1, 0),
    ] {
        cb.extend([y, y, y, y, u as u8, v as u8]);
    }
    let f = frame(
        1,
        16,
        4,
        &[strip(
            0x10,
            0,
            4,
            16,
            &[chunk(0x22, &cb), chunk(0x32, &[0, 1, 2, 3])],
        )],
    );
    let mut d = Decoder::new(16, 4).unwrap();
    d.decode(&f).unwrap();
    let px = |x: usize| {
        let o = x * 3;
        (d.frame()[o], d.frame()[o + 1], d.frame()[o + 2])
    };
    assert_eq!(px(0), (50, 51, 44));
    assert_eq!(px(4), (255, 100, 255));
    assert_eq!(px(8), (0, 155, 0));
    assert_eq!(px(12), (0, 0, 2));
}

#[test]
fn inter_frame_skips_keep_previous_pixels_and_update_codebooks_selectively() {
    let mut cb = GRAY.to_vec();
    cb.extend(COLOR);
    let key = frame(
        1,
        8,
        4,
        &[strip(
            0x10,
            0,
            4,
            8,
            &[chunk(0x22, &cb), chunk(0x32, &[0, 0])],
        )],
    );
    let mut d = Decoder::new(8, 4).unwrap();
    d.decode(&key).unwrap();
    let g = |v: u8| (v, v, v);
    let kt = [g(10), g(10), g(20), g(20)];
    let kb = [g(30), g(30), g(40), g(40)];

    // Inter frame (0x1100): selective V1 update (0x23): flag bit 31 clear,
    // bit 30 set => entry 1 replaced by entry data, entry 0 untouched.
    let upd_flags = 0x4000_0000u32.to_be_bytes();
    let mut upd = upd_flags.to_vec();
    upd.extend([200, 200, 200, 200, 0, 0]); // new entry 1
                                            // 0x31: flags word; block0: bit31=0 -> skip; block1: bit30=1 (coded),
                                            // then bit29=0 -> V1 index byte.
    let vflags = 0x4000_0000u32.to_be_bytes();
    let mut vecs = vflags.to_vec();
    vecs.push(1);
    let inter = frame(
        0,
        8,
        4,
        &[strip(
            0x11,
            0,
            4,
            8,
            &[chunk(0x23, &upd), chunk(0x31, &vecs)],
        )],
    );
    d.decode(&inter).unwrap();
    let n = g(200);
    let left = [kt, kt, kb, kb]; // untouched block 0
    let rows: Vec<Vec<(u8, u8, u8)>> = left
        .iter()
        .map(|l| l.iter().copied().chain([n; 4]).collect())
        .collect();
    let refs: Vec<&[(u8, u8, u8)]> = rows.iter().map(|r| r.as_slice()).collect();
    assert_eq!(d.frame(), rgb(&refs));
}

#[test]
fn zero_length_chunk_repeats_previous_frame() {
    let f = frame(
        1,
        4,
        4,
        &[strip(
            0x10,
            0,
            4,
            4,
            &[chunk(0x22, &GRAY), chunk(0x32, &[0])],
        )],
    );
    let mut d = Decoder::new(4, 4).unwrap();
    d.decode(&f).unwrap();
    let before = d.frame().to_vec();
    d.decode(&[]).unwrap();
    assert_eq!(d.frame(), before);
}

#[test]
fn strips_stack_vertically_and_inherit_codebooks_unless_flag_bit0() {
    // Frame 4x8, two strips of height 4. Strip 0 loads V1 entry 0 = GRAY.
    // Strip 1 loads nothing and draws entry 0 with 0x32.
    let s0 = strip(0x10, 0, 4, 4, &[chunk(0x22, &GRAY), chunk(0x32, &[0])]);
    let s1 = strip(0x11, 4, 8, 4, &[chunk(0x32, &[0])]);
    // flags bit0 = 0 => strip 1 starts from strip 0's codebooks.
    let f = frame(0, 4, 8, &[s0.clone(), s1.clone()]);
    let mut d = Decoder::new(4, 8).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let top = [g(10), g(10), g(20), g(20)];
    let bot = [g(30), g(30), g(40), g(40)];
    let want = rgb(&[&top, &top, &bot, &bot, &top, &top, &bot, &bot]);
    assert_eq!(d.frame(), want);

    // flags bit0 = 1 => strip 1 keeps its own (never loaded, all-black) book.
    let f = frame(1, 4, 8, &[s0, s1]);
    let mut d = Decoder::new(4, 8).unwrap();
    d.decode(&f).unwrap();
    let z = [g(0); 4];
    let want = rgb(&[&top, &top, &bot, &bot, &z, &z, &z, &z]);
    assert_eq!(d.frame(), want);
}

#[test]
fn grayscale_codebooks_have_four_byte_entries() {
    // 0x26 = V1 grayscale full book (id 0x22 | 0x04): y0..y3 only.
    let f = frame(
        1,
        4,
        4,
        &[strip(
            0x10,
            0,
            4,
            4,
            &[chunk(0x26, &[7, 8, 9, 10]), chunk(0x32, &[0])],
        )],
    );
    let mut d = Decoder::new(4, 4).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let top = [g(7), g(7), g(8), g(8)];
    let bot = [g(9), g(9), g(10), g(10)];
    assert_eq!(d.frame(), rgb(&[&top, &top, &bot, &bot]));
}

#[test]
fn malformed_frames_are_errors_not_panics() {
    let mut d = Decoder::new(4, 4).unwrap();
    assert!(d.decode(&[1, 0, 0]).is_err(), "short header");
    // dimensions disagree with the decoder
    let f = frame(1, 8, 4, &[]);
    assert!(d.decode(&f).is_err());
    // bad strip id
    let f = frame(1, 4, 4, &[strip(0x20, 0, 4, 4, &[])]);
    assert!(d.decode(&f).is_err());
    // strip claims more bytes than the frame holds
    let mut f = frame(
        1,
        4,
        4,
        &[strip(
            0x10,
            0,
            4,
            4,
            &[chunk(0x22, &GRAY), chunk(0x32, &[0])],
        )],
    );
    f.truncate(f.len() - 3);
    assert!(d.decode(&f).is_err());
    // vector index byte missing
    let f = frame(
        1,
        4,
        4,
        &[strip(
            0x10,
            0,
            4,
            4,
            &[chunk(0x22, &GRAY), chunk(0x32, &[])],
        )],
    );
    assert!(d.decode(&f).is_err());
    // zero-sized or non-multiple-of-4 decoder
    assert!(Decoder::new(0, 4).is_err());
    assert!(Decoder::new(6, 4).is_err());
}

#[test]
fn zero_y1_places_strip_directly_below_the_previous_one() {
    // second strip: y1 = 0 (relative), y2 = 4 is its height
    let s0 = strip(0x10, 0, 4, 4, &[chunk(0x22, &GRAY), chunk(0x32, &[0])]);
    let s1 = strip(0x11, 0, 4, 4, &[chunk(0x32, &[0])]);
    let f = frame(0, 4, 8, &[s0, s1]);
    let mut d = Decoder::new(4, 8).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let top = [g(10), g(10), g(20), g(20)];
    let bot = [g(30), g(30), g(40), g(40)];
    assert_eq!(
        d.frame(),
        rgb(&[&top, &top, &bot, &bot, &top, &top, &bot, &bot])
    );
}

#[test]
fn strip_column_range_limits_the_blocks_drawn() {
    // 8x4 frame; strip header x1 = 4, x2 = 8: only the right block is coded.
    let body = [chunk(0x22, &GRAY), chunk(0x32, &[0])].concat();
    let mut s = hdr(0x10, body.len() + 12);
    for x in [0u16, 4, 4, 8] {
        s.extend(be16(x));
    }
    s.extend(body);
    let f = frame(1, 8, 4, &[s]);
    let mut d = Decoder::new(8, 4).unwrap();
    d.decode(&f).unwrap();
    let g = |v: u8| (v, v, v);
    let z = g(0);
    let top = [z, z, z, z, g(10), g(10), g(20), g(20)];
    let bot = [z, z, z, z, g(30), g(30), g(40), g(40)];
    assert_eq!(d.frame(), rgb(&[&top, &top, &bot, &bot]));
}

#[test]
fn strip_rectangles_that_cut_a_4x4_block_are_errors_not_panics() {
    let body = [chunk(0x22, &GRAY), chunk(0x32, &[0, 0])].concat();
    // extents that are not a multiple of 4, or rows past the frame
    for (x1, y1, x2, y2) in [(2u16, 0u16, 8u16, 4u16), (0, 2, 8, 7), (0, 4, 8, 10)] {
        let mut s = hdr(0x10, body.len() + 12);
        for x in [y1, x1, y2, x2] {
            s.extend(be16(x));
        }
        s.extend(&body);
        let f = frame(1, 8, 8, &[s]);
        let mut d = Decoder::new(8, 8).unwrap();
        assert!(d.decode(&f).is_err(), "{x1},{y1},{x2},{y2}");
    }
}

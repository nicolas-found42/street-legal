use slr_fmt_scx::{parse, Scx};

fn words(values: &[u32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}
fn container(chunks: &[(u32, Vec<u8>)]) -> Vec<u8> {
    let mut out = b"INVO".to_vec();
    out.extend(words(&[4, chunks.len() as u32]));
    let mut offset = 12 + 8 * chunks.len();
    for (tag, payload) in chunks {
        out.extend(words(&[*tag, offset as u32]));
        offset += 8 + payload.len();
    }
    for (tag, payload) in chunks {
        out.extend(words(&[*tag, (8 + payload.len()) as u32]));
        out.extend(payload);
    }
    out
}
#[test]
fn table_starts_at_first_tag_and_geometry_uses_tags_four_and_five() {
    let vertex = words(&[
        1,
        0x200,
        1f32.to_bits(),
        2f32.to_bits(),
        3f32.to_bits(),
        0.25f32.to_bits(),
        0.75f32.to_bits(),
    ]);
    let mut index = words(&[3]);
    index.extend([0; 6]);
    let bytes = container(&[(0, words(&[3, 0, 0])), (4, vertex), (5, index)]);
    let Scx::V4 {
        meshes,
        materials,
        chunk_tags,
        ..
    } = parse(&bytes).unwrap()
    else {
        panic!("v4")
    };
    assert_eq!(chunk_tags, [0, 4, 5]);
    assert_eq!(materials.len(), 1);
    assert_eq!(materials[0].kind, 3);
    assert_eq!(meshes.len(), 1);
    assert_eq!(meshes[0].positions, [[1., 2., 3.]]);
    assert_eq!(meshes[0].uvs, [[0.25, 0.75]]);
    assert_eq!(meshes[0].indices, [0, 0, 0]);
}
#[test]
fn rejects_mismatched_table_offsets_tags_sizes_and_unaccounted_bytes() {
    let good = container(&[(0, words(&[3, 0, 0]))]);
    for (at, value) in [(12, 1), (16, 21), (24, 7), (24, 100)] {
        let mut bad = good.clone();
        bad[at..at + 4].copy_from_slice(&u32::to_le_bytes(value));
        assert!(parse(&bad).is_err(), "mutation at {at}: {value}");
    }
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(parse(&trailing).is_err());
    for n in 0..good.len() {
        assert!(parse(&good[..n]).is_err(), "truncation {n}");
    }
}

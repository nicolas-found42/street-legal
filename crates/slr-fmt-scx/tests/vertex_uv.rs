//! Packed vertex colors precede texture coordinates in the v4 0x7c1 layout.
#[test]
fn v4_uv_skips_diffuse_and_specular_words() {
    let mut bytes = b"INVO".to_vec();
    for word in [4u32, 2, 0, 32, 5, 96, 0, 4, 64, 1, 0x7c1] {
        bytes.extend(word.to_le_bytes());
    }
    for value in [0f32, 0., 0., 0., 1., 0.] {
        bytes.extend(value.to_le_bytes());
    }
    for color in [0xffffffffu32, 0xff000000] {
        bytes.extend(color.to_le_bytes());
    }
    for value in [0.25f32, 0.75, 0.8, 0.2] {
        bytes.extend(value.to_le_bytes());
    }
    for word in [5u32, 18, 3] {
        bytes.extend(word.to_le_bytes());
    }
    bytes.extend([0u8; 6]);
    let slr_fmt_scx::Scx::V4 { meshes, .. } = slr_fmt_scx::parse(&bytes).unwrap() else {
        panic!("wrong version");
    };
    assert_eq!(meshes[0].uvs, vec![[0.25, 0.75]]);
}

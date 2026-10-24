//! The garage POLY owner word differs from the four map packages' root 1.
use slr_fmt_rpak::{
    payload::{walk_payload, Chunk},
    Package,
};
#[test]
fn garage_scene_keeps_root_owner_and_render_triangles() {
    let stock = std::env::var_os("SLR_STOCK")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../stock"));
    let path = stock.join("misc/garage.rpk");
    if !path.exists() {
        eprintln!("SKIP: stock unavailable");
        return;
    }
    let bytes = std::fs::read(path).unwrap();
    let package = Package::parse(bytes.clone()).unwrap();
    let root = package
        .entries
        .iter()
        .find(|e| e.alias == "garage1")
        .unwrap();
    let ranges = walk_payload(&bytes, root.rsd_offset as usize, root.rsd_size as usize).unwrap();
    let mut groups = 0;
    let mut triangles = 0;
    for range in ranges {
        for chunk in range.chunks {
            if let Chunk::Poly {
                owner,
                groups: surfaces,
                ..
            } = chunk
            {
                assert_eq!(owner, root.type_id);
                for group in surfaces {
                    groups += 1;
                    triangles += group.indices.len();
                    assert!(group.mat < 9);
                    let vertices = group.decoded();
                    assert!(vertices.iter().all(|v| v.pos.iter().all(|f| f.is_finite())));
                    assert!(group
                        .triangles()
                        .all(|(a, b, c)| [a, b, c].iter().all(|i| (*i as usize) < vertices.len())));
                }
            }
        }
    }
    assert_eq!(groups, 10);
    assert_eq!(triangles, 1274);
}

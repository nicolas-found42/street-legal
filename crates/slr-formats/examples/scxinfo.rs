use slr_formats::scx;
fn main() {
    for p in std::env::args().skip(1) {
        let data = std::fs::read(&p).unwrap();
        match scx::parse(&data).unwrap() {
            scx::Scx::V3 { groups, .. } => {
                let verts: usize = groups.iter().map(|g| g.mesh.positions.len()).sum();
                let tris: usize = groups.iter().map(|g| g.mesh.indices.len() / 3).sum();
                println!("{p}: v3 {} groups {verts} verts {tris} tris", groups.len());
            }
            scx::Scx::V4 { meshes, .. } => {
                let mut lo = [f32::MAX; 3];
                let mut hi = [f32::MIN; 3];
                let mut tris = 0;
                for m in &meshes {
                    tris += m.indices.len() / 3;
                    for v in &m.positions {
                        for k in 0..3 {
                            lo[k] = lo[k].min(v[k]);
                            hi[k] = hi[k].max(v[k]);
                        }
                    }
                }
                println!(
                    "{p}: {} meshes {tris} tris  x[{:.2},{:.2}] y[{:.2},{:.2}] z[{:.2},{:.2}]",
                    meshes.len(),
                    lo[0],
                    hi[0],
                    lo[1],
                    hi[1],
                    lo[2],
                    hi[2]
                );
            }
        }
    }
}

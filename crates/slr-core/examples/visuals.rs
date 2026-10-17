//! Assembles every car's visual body and prints its bounds.
use slr_core::catalog::Catalog;
use slr_core::visual::load_car_visual;
fn main() {
    let root = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "reference/raw".into());
    let cat = Catalog::load(std::path::Path::new(&root)).unwrap();
    for car in &cat.cars {
        let dir = car
            .mesh_dir
            .as_ref()
            .unwrap()
            .parent()
            .unwrap()
            .to_path_buf();
        let stem = car.id.split('/').nth(1).unwrap();
        match load_car_visual(&dir, stem) {
            Some(v) => {
                let (lo, hi) = v.bounds();
                println!(
                    "{:<34} parts {:>3} tris {:>6}  W {:.2} H {:.2} L {:.2}  front z {:.2}",
                    car.id,
                    v.parts.len(),
                    v.triangle_count(),
                    hi[0] - lo[0],
                    hi[1] - lo[1],
                    hi[2] - lo[2],
                    hi[2]
                );
            }
            None => println!("{:<34} NO VISUAL", car.id),
        }
    }
}

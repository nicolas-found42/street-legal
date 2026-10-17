//! 3D drawing helpers: batched meshes, shaded car bodies built from the shipped panel
//! meshes, the procedural world around a track, and the cameras.

use macroquad::prelude::*;
use slr_core::track::Track;
use slr_core::visual::{CarVisual, Material};
use std::rc::Rc;

pub const SUN: [f32; 3] = [0.45, 0.8, 0.35];

#[derive(Clone, Copy)]
pub struct Light {
    pub ambient: f32,
    pub sun: f32,
    /// RGB tint multiplied into every surface (cool at night).
    pub tint: [f32; 3],
}

impl Light {
    pub fn day() -> Light {
        Light {
            ambient: 0.45,
            sun: 0.6,
            tint: [1.0, 1.0, 1.0],
        }
    }
    pub fn night() -> Light {
        Light {
            ambient: 0.16,
            sun: 0.12,
            tint: [0.55, 0.65, 1.0],
        }
    }
    pub fn shade(&self, rgb: [f32; 3], n: [f32; 3]) -> Color {
        let l = (SUN[0] * SUN[0] + SUN[1] * SUN[1] + SUN[2] * SUN[2]).sqrt();
        let d = ((n[0] * SUN[0] + n[1] * SUN[1] + n[2] * SUN[2]) / l).max(0.0);
        let k = self.ambient + self.sun * d;
        Color::new(
            (rgb[0] * k * self.tint[0]).min(1.0),
            (rgb[1] * k * self.tint[1]).min(1.0),
            (rgb[2] * k * self.tint[2]).min(1.0),
            1.0,
        )
    }
}

/// Collects triangles and flushes to as many u16-indexed meshes as needed.
#[derive(Default)]
pub struct MeshSink {
    meshes: Vec<Mesh>,
    cur_v: Vec<Vertex>,
    cur_i: Vec<u16>,
}

impl MeshSink {
    pub fn new() -> Self {
        Self::default()
    }

    fn flush(&mut self) {
        if !self.cur_i.is_empty() {
            self.meshes.push(Mesh {
                vertices: std::mem::take(&mut self.cur_v),
                indices: std::mem::take(&mut self.cur_i),
                texture: None,
            });
        }
    }

    /// World-space triangle with per-vertex colours.
    pub fn tri(&mut self, p: [[f32; 3]; 3], c: [Color; 3]) {
        if self.cur_v.len() + 3 > 9990 || self.cur_i.len() + 3 > 4990 {
            self.flush();
        }
        let base = self.cur_v.len() as u16;
        for k in 0..3 {
            self.cur_v
                .push(Vertex::new(p[k][0], p[k][1], p[k][2], 0.0, 0.0, c[k]));
            self.cur_i.push(base + k as u16);
        }
    }

    pub fn quad(&mut self, a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3], col: Color) {
        self.tri([a, b, c], [col; 3]);
        self.tri([a, c, d], [col; 3]);
    }

    /// Axis-aligned box.
    pub fn cuboid(&mut self, centre: [f32; 3], half: [f32; 3], col: Color) {
        let (x, y, z) = (centre[0], centre[1], centre[2]);
        let (hx, hy, hz) = (half[0], half[1], half[2]);
        let v = |sx: f32, sy: f32, sz: f32| [x + sx * hx, y + sy * hy, z + sz * hz];
        let shade = |k: f32| Color::new(col.r * k, col.g * k, col.b * k, 1.0);
        self.quad(
            v(-1., 1., -1.),
            v(1., 1., -1.),
            v(1., 1., 1.),
            v(-1., 1., 1.),
            shade(1.0),
        );
        self.quad(
            v(-1., -1., 1.),
            v(1., -1., 1.),
            v(1., 1., 1.),
            v(-1., 1., 1.),
            shade(0.85),
        );
        self.quad(
            v(-1., -1., -1.),
            v(1., -1., -1.),
            v(1., 1., -1.),
            v(-1., 1., -1.),
            shade(0.8),
        );
        self.quad(
            v(-1., -1., -1.),
            v(-1., -1., 1.),
            v(-1., 1., 1.),
            v(-1., 1., -1.),
            shade(0.7),
        );
        self.quad(
            v(1., -1., -1.),
            v(1., -1., 1.),
            v(1., 1., 1.),
            v(1., 1., -1.),
            shade(0.75),
        );
    }

    pub fn draw(&mut self) {
        self.flush();
        for m in &self.meshes {
            draw_mesh(m);
        }
    }
}

/// Convert game-world (x east, y north, height) to render space.
pub fn to_render(x: f32, y: f32, h: f32) -> [f32; 3] {
    [x, h, -y]
}

#[derive(Clone)]
pub struct CarRender {
    visual: Rc<CarVisual>,
    pub wheel_positions: [[f32; 3]; 4],
    pub wheel_radius: [f32; 4],
    pub wheel_width: [f32; 4],
    /// Height of the body origin above the ground.
    pub ride_height: f32,
}

impl CarRender {
    pub fn new(
        visual: Rc<CarVisual>,
        wheels: &[slr_formats::cfg::Wheel],
        tyre_radius: [f32; 4],
        tyre_width: [f32; 4],
    ) -> CarRender {
        let mut wp = [[0.0; 3]; 4];
        // Body-local: x right, y up, z forward (cfg z is backwards).
        let mut sorted: Vec<&slr_formats::cfg::Wheel> = wheels.iter().collect();
        sorted.sort_by(|a, b| {
            let ka = (a.position[2] > 0.0, a.position[0] > 0.0);
            let kb = (b.position[2] > 0.0, b.position[0] > 0.0);
            ka.cmp(&kb)
        });
        for (i, w) in sorted.iter().take(4).enumerate() {
            wp[i] = [w.position[0], w.position[1], -w.position[2]];
        }
        let low = wp.iter().map(|w| w[1]).fold(f32::MAX, f32::min);
        let ride_height = tyre_radius[0] - low;
        CarRender {
            visual,
            wheel_positions: wp,
            wheel_radius: tyre_radius,
            wheel_width: tyre_width,
            ride_height,
        }
    }

    fn paint_colour(m: Material, paint: [u8; 3]) -> [f32; 3] {
        match m {
            Material::Paint => [
                paint[0] as f32 / 255.0,
                paint[1] as f32 / 255.0,
                paint[2] as f32 / 255.0,
            ],
            Material::Glass => [0.12, 0.17, 0.22],
            Material::Light => [0.85, 0.82, 0.6],
            Material::Dark => [0.1, 0.1, 0.11],
        }
    }

    /// Append this car to `sink`. `heading` follows the game convention (0 = +y, CCW).
    #[allow(clippy::too_many_arguments)]
    pub fn emit(
        &self,
        sink: &mut MeshSink,
        pos: [f32; 2],
        heading: f32,
        steer: f32,
        spin: [f32; 4],
        paint: [u8; 3],
        light: Light,
        brake_lights: bool,
        damage: f32,
    ) {
        let (sin_h, cos_h) = heading.sin_cos();
        let fwd = [-sin_h, cos_h];
        let right = [cos_h, sin_h];
        let xf = |p: [f32; 3]| -> [f32; 3] {
            let wx = pos[0] + right[0] * p[0] + fwd[0] * p[2];
            let wy = pos[1] + right[1] * p[0] + fwd[1] * p[2];
            to_render(wx, wy, p[1] + self.ride_height)
        };
        let nf = |n: [f32; 3]| -> [f32; 3] {
            // Rotate the normal by the heading into render space.
            let wx = right[0] * n[0] + fwd[0] * n[2];
            let wy = right[1] * n[0] + fwd[1] * n[2];
            [wx, n[1], -wy]
        };
        for part in &self.visual.parts {
            let mut rgb = Self::paint_colour(part.material, paint);
            if part.material == Material::Paint && damage > 0.0 {
                let d = damage.min(1.0) * 0.55;
                rgb = [
                    rgb[0] * (1.0 - d) + 0.18 * d,
                    rgb[1] * (1.0 - d) + 0.15 * d,
                    rgb[2] * (1.0 - d) + 0.12 * d,
                ];
            }
            if part.material == Material::Light
                && brake_lights
                && part.name.to_ascii_lowercase().contains("tail")
            {
                rgb = [1.0, 0.1, 0.08];
            }
            let cols: Vec<Color> = part
                .vertices
                .iter()
                .map(|v| light.shade(rgb, nf(v.normal)))
                .collect();
            let pts: Vec<[f32; 3]> = part.vertices.iter().map(|v| xf(v.pos)).collect();
            for t in part.indices.as_chunks::<3>().0 {
                let (a, b, c) = (t[0] as usize, t[1] as usize, t[2] as usize);
                sink.tri([pts[a], pts[b], pts[c]], [cols[a], cols[b], cols[c]]);
            }
        }
        // Wheels: tyre cylinder along the axle plus a spoked rim face.
        for i in 0..4 {
            let wp = self.wheel_positions[i];
            let (r, w) = (self.wheel_radius[i], self.wheel_width[i]);
            let steer_a = if i < 2 { steer } else { 0.0 };
            let (ss, sc) = steer_a.sin_cos();
            let outward = if wp[0] < 0.0 { -1.0 } else { 1.0 };
            let seg = 14;
            let ang = |k: usize| spin[i] + k as f32 * std::f32::consts::TAU / seg as f32;
            // wheel-local point: axle = x, rolling plane = (z fwd, y up)
            let pt = |ax: f32, k: usize, rad: f32| -> [f32; 3] {
                let (s, c) = ang(k).sin_cos();
                let lz = s * rad;
                let ly = c * rad;
                // steer about the vertical axis
                let (px, pz) = (ax * sc + -(lz * ss), ax * ss + lz * sc);
                [wp[0] + px, wp[1] + ly, wp[2] + pz]
            };
            let tyre = light.shade([0.06, 0.06, 0.065], [0.0, 1.0, 0.0]);
            let rim = light.shade([0.62, 0.64, 0.68], [0.0, 1.0, 0.0]);
            let dark = light.shade([0.2, 0.2, 0.22], [0.0, 1.0, 0.0]);
            for k in 0..seg {
                let (a0, a1) = (-w / 2.0, w / 2.0);
                let p00 = xf(pt(a0, k, r));
                let p01 = xf(pt(a0, k + 1, r));
                let p10 = xf(pt(a1, k, r));
                let p11 = xf(pt(a1, k + 1, r));
                sink.quad(
                    p00,
                    p01,
                    p11,
                    p10,
                    if k % 2 == 0 {
                        tyre
                    } else {
                        Color::new(tyre.r * 1.3, tyre.g * 1.3, tyre.b * 1.3, 1.0)
                    },
                );
                // outward face: spokes alternate rim / dark
                let face = outward * w / 2.0;
                let c0 = xf(pt(face, 0, 0.0));
                let q0 = xf(pt(face, k, r * 0.72));
                let q1 = xf(pt(face, k + 1, r * 0.72));
                sink.tri([c0, q0, q1], [if k % 2 == 0 { rim } else { dark }; 3]);
                let tq0 = xf(pt(face, k, r));
                let tq1 = xf(pt(face, k + 1, r));
                sink.quad(q0, tq0, tq1, q1, tyre);
            }
        }
    }
}

/// Procedural world around a track.
pub struct World {
    pub sink: MeshSink,
}

fn lcg(seed: &mut u64) -> f32 {
    *seed = seed
        .wrapping_mul(6364136223846793005)
        .wrapping_add(1442695040888963407);
    ((*seed >> 33) as f32) / ((u32::MAX >> 1) as f32)
}

impl World {
    pub fn build(track: &Track, night: bool, drag: bool) -> World {
        let light = if night { Light::night() } else { Light::day() };
        let mut sink = MeshSink::new();
        let c = |rgb: [f32; 3]| light.shade(rgb, [0.0, 1.0, 0.0]);
        // Ground tiles (patchwork so motion reads at speed).
        let (mut lo, mut hi) = ([f32::MAX; 2], [f32::MIN; 2]);
        for p in &track.points {
            for k in 0..2 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        let pad = 400.0;
        let tile = 60.0;
        let mut gx = ((lo[0] - pad) / tile).floor() * tile;
        let mut ix = 0;
        while gx < hi[0] + pad {
            let mut gy = ((lo[1] - pad) / tile).floor() * tile;
            let mut iy = 0;
            while gy < hi[1] + pad {
                let k = if (ix + iy) % 2 == 0 {
                    [0.20, 0.42, 0.18]
                } else {
                    [0.17, 0.38, 0.16]
                };
                let col = c(k);
                sink.quad(
                    to_render(gx, gy, -0.02),
                    to_render(gx + tile, gy, -0.02),
                    to_render(gx + tile, gy + tile, -0.02),
                    to_render(gx, gy + tile, -0.02),
                    col,
                );
                gy += tile;
                iy += 1;
            }
            gx += tile;
            ix += 1;
        }
        // Road, edge lines, centre line, barriers.
        let n = track.points.len();
        let segs = if track.closed { n } else { n - 1 };
        let hw = track.half_width;
        let wall = track.wall_distance();
        let dash_period = 8.0;
        for i in 0..segs {
            let a = track.points[i];
            let b = track.points[(i + 1) % n];
            let t = [b[0] - a[0], b[1] - a[1]];
            let l = t[0].hypot(t[1]).max(1e-6);
            let t = [t[0] / l, t[1] / l];
            let r = [t[1], -t[0]];
            let off = |p: [f32; 2], d: f32| [p[0] + r[0] * d, p[1] + r[1] * d];
            let shade = if i % 2 == 0 {
                [0.23, 0.23, 0.25]
            } else {
                [0.21, 0.21, 0.23]
            };
            let road = c(shade);
            let q = |p: [f32; 2], h: f32| to_render(p[0], p[1], h);
            sink.quad(
                q(off(a, -hw), 0.0),
                q(off(a, hw), 0.0),
                q(off(b, hw), 0.0),
                q(off(b, -hw), 0.0),
                road,
            );
            // Shoulder strip between road and barrier.
            let shoulder = c([0.32, 0.30, 0.26]);
            sink.quad(
                q(off(a, hw), -0.005),
                q(off(a, wall), -0.005),
                q(off(b, wall), -0.005),
                q(off(b, hw), -0.005),
                shoulder,
            );
            sink.quad(
                q(off(a, -wall), -0.005),
                q(off(a, -hw), -0.005),
                q(off(b, -hw), -0.005),
                q(off(b, -wall), -0.005),
                shoulder,
            );
            let line = c([0.9, 0.9, 0.9]);
            let lw = 0.15;
            sink.quad(
                q(off(a, hw - 0.5 - lw), 0.01),
                q(off(a, hw - 0.5), 0.01),
                q(off(b, hw - 0.5), 0.01),
                q(off(b, hw - 0.5 - lw), 0.01),
                line,
            );
            sink.quad(
                q(off(a, -hw + 0.5), 0.01),
                q(off(a, -hw + 0.5 + lw), 0.01),
                q(off(b, -hw + 0.5 + lw), 0.01),
                q(off(b, -hw + 0.5), 0.01),
                line,
            );
            let s_here = track.cum[i];
            if (s_here / dash_period) as i32 % 2 == 0 && !drag {
                sink.quad(
                    q(off(a, -lw), 0.01),
                    q(off(a, lw), 0.01),
                    q(off(b, lw), 0.01),
                    q(off(b, -lw), 0.01),
                    c([0.95, 0.85, 0.2]),
                );
            }
            // Barriers: a low wall on each side with alternating red/white blocks.
            let blk = if i % 4 < 2 {
                [0.85, 0.85, 0.86]
            } else {
                [0.8, 0.15, 0.15]
            };
            for side in [-1.0f32, 1.0] {
                let mid = [
                    (a[0] + b[0]) / 2.0 + r[0] * side * (wall + 0.25),
                    (a[1] + b[1]) / 2.0 + r[1] * side * (wall + 0.25),
                ];
                // Boxes are axis-aligned, so draw the wall as a quad strip instead.
                let w0 = off(a, side * (wall + 0.1));
                let w1 = off(b, side * (wall + 0.1));
                let _ = mid;
                let col = c(blk);
                sink.quad(q(w0, 0.0), q(w1, 0.0), q(w1, 0.9), q(w0, 0.9), col);
                let top = c([0.55, 0.55, 0.56]);
                sink.quad(
                    q(off(a, side * (wall - 0.15)), 0.9),
                    q(off(a, side * (wall + 0.35)), 0.9),
                    q(off(b, side * (wall + 0.35)), 0.9),
                    q(off(b, side * (wall - 0.15)), 0.9),
                    top,
                );
            }
        }
        // Start / finish and drag markers.
        let mark = |sink: &mut MeshSink, s: f32, col: [f32; 3], depth: f32| {
            let (p, t) = track.at(s);
            let r = [t[1], -t[0]];
            let a = [p[0] - t[0] * depth / 2.0, p[1] - t[1] * depth / 2.0];
            let b = [p[0] + t[0] * depth / 2.0, p[1] + t[1] * depth / 2.0];
            let off = |p: [f32; 2], d: f32| [p[0] + r[0] * d, p[1] + r[1] * d];
            sink.quad(
                to_render(off(a, -hw)[0], off(a, -hw)[1], 0.02),
                to_render(off(a, hw)[0], off(a, hw)[1], 0.02),
                to_render(off(b, hw)[0], off(b, hw)[1], 0.02),
                to_render(off(b, -hw)[0], off(b, -hw)[1], 0.02),
                c(col),
            );
        };
        mark(&mut sink, 0.0, [0.95, 0.95, 0.95], 1.0);
        if drag {
            mark(&mut sink, 402.3, [0.95, 0.9, 0.2], 1.5);
            for k in 1..4 {
                mark(&mut sink, 100.0 * k as f32, [0.6, 0.6, 0.65], 0.5);
            }
        } else {
            mark(&mut sink, 0.0, [0.9, 0.9, 0.9], 1.0);
        }
        // Gantry at the start line.
        {
            let (p, t) = track.at(0.0);
            let r = [t[1], -t[0]];
            for side in [-1.0f32, 1.0] {
                let x = p[0] + r[0] * side * (wall + 1.0);
                let y = p[1] + r[1] * side * (wall + 1.0);
                sink.cuboid(to_render(x, y, 3.0), [0.3, 3.0, 0.3], c([0.7, 0.7, 0.72]));
            }
            let beam = to_render(p[0], p[1], 6.2);
            sink.cuboid(beam, [wall + 1.3, 0.35, 0.35], c([0.85, 0.15, 0.15]));
        }
        // Scenery: trees and light poles beyond the barriers.
        let mut seed = 0xC0FFEEu64;
        let count = if drag { 140 } else { 420 };
        for _ in 0..count {
            let s = lcg(&mut seed) * track.length();
            let (p, t) = track.at(s);
            let r = [t[1], -t[0]];
            let side = if lcg(&mut seed) < 0.5 { -1.0 } else { 1.0 };
            let d = wall + 6.0 + lcg(&mut seed) * 40.0;
            let (x, y) = (p[0] + r[0] * side * d, p[1] + r[1] * side * d);
            let h = 5.0 + lcg(&mut seed) * 6.0;
            let g = lcg(&mut seed) * 0.08;
            sink.cuboid(
                to_render(x, y, h * 0.2),
                [0.25, h * 0.2, 0.25],
                c([0.35, 0.22, 0.12]),
            );
            sink.cuboid(
                to_render(x, y, h * 0.75),
                [1.6 + g * 10.0, h * 0.35, 1.6 + g * 10.0],
                c([0.12, 0.34 + g, 0.14]),
            );
        }
        if night {
            // Pole lamps along the road so it reads as lit.
            let mut s = 0.0;
            while s < track.length() {
                let (p, t) = track.at(s);
                let r = [t[1], -t[0]];
                let (x, y) = (p[0] + r[0] * (wall + 1.5), p[1] + r[1] * (wall + 1.5));
                sink.cuboid(
                    to_render(x, y, 4.0),
                    [0.1, 4.0, 0.1],
                    Color::new(0.3, 0.3, 0.32, 1.0),
                );
                sink.cuboid(
                    to_render(x, y, 8.2),
                    [0.5, 0.15, 0.5],
                    Color::new(1.0, 0.95, 0.7, 1.0),
                );
                s += 45.0;
            }
        }
        World { sink }
    }
}

pub struct ChaseCam {
    pub yaw: f32,
    pub pos: Vec3,
    pub mode: usize,
}

impl ChaseCam {
    pub fn new() -> Self {
        Self {
            yaw: 0.0,
            pos: vec3(0.0, 5.0, 10.0),
            mode: 0,
        }
    }

    pub const MODES: usize = 4;

    pub fn camera(&mut self, car_pos: [f32; 2], heading: f32, speed: f32, dt: f32) -> Camera3D {
        let (sh, ch) = heading.sin_cos();
        let fwd = [-sh, ch];
        // Smooth heading so the view does not whip around in slides.
        let mut d = heading - self.yaw;
        while d > std::f32::consts::PI {
            d -= std::f32::consts::TAU;
        }
        while d < -std::f32::consts::PI {
            d += std::f32::consts::TAU;
        }
        let follow = if self.mode == 0 { 4.0 } else { 14.0 };
        self.yaw += d * (follow * dt).min(1.0);
        let (sy, cy) = self.yaw.sin_cos();
        let cf = [-sy, cy];
        let (position, target, fovy) = match self.mode {
            // chase
            0 => {
                let back = 7.5 + speed * 0.03;
                (
                    to_render(
                        car_pos[0] - cf[0] * back,
                        car_pos[1] - cf[1] * back,
                        2.5 + speed * 0.01,
                    ),
                    to_render(car_pos[0] + fwd[0] * 6.0, car_pos[1] + fwd[1] * 6.0, 1.0),
                    0.95,
                )
            }
            // close chase
            1 => (
                to_render(car_pos[0] - cf[0] * 4.8, car_pos[1] - cf[1] * 4.8, 1.55),
                to_render(car_pos[0] + fwd[0] * 8.0, car_pos[1] + fwd[1] * 8.0, 1.0),
                1.0,
            ),
            // hood
            2 => (
                to_render(car_pos[0] + fwd[0] * 0.6, car_pos[1] + fwd[1] * 0.6, 1.25),
                to_render(car_pos[0] + fwd[0] * 20.0, car_pos[1] + fwd[1] * 20.0, 1.1),
                1.15,
            ),
            // helicopter
            _ => (
                to_render(car_pos[0] - fwd[0] * 6.0, car_pos[1] - fwd[1] * 6.0, 28.0),
                to_render(car_pos[0], car_pos[1], 0.0),
                0.9,
            ),
        };
        self.pos = vec3(position[0], position[1], position[2]);
        Camera3D {
            position: self.pos,
            target: vec3(target[0], target[1], target[2]),
            up: vec3(0.0, 1.0, 0.0),
            fovy,
            projection: Projection::Perspective,
            ..Default::default()
        }
    }
}

impl Default for ChaseCam {
    fn default() -> Self {
        Self::new()
    }
}

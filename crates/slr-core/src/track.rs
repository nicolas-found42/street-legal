//! Tracks: a centreline polyline with a width, a start grid and barrier walls.
//!
//! The shipped maps are `INVO` v3 map pieces plus `.scy`/`.scm` collision and spline files
//! that are not decoded yet (`docs/fidelity.md`), so the reference environments here are
//! procedural: a drag strip and a closed circuit.

use serde::{Deserialize, Serialize};

pub type V2 = [f32; 2];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Track {
    pub name: String,
    pub points: Vec<V2>,
    /// Cumulative arc length at each point.
    pub cum: Vec<f32>,
    pub half_width: f32,
    pub closed: bool,
    /// Distance from the barrier to the road edge.
    pub run_off: f32,
}

#[derive(Debug, Clone, Copy)]
pub struct Projection {
    pub index: usize,
    /// Arc length along the centreline (wraps for closed tracks).
    pub s: f32,
    /// Signed distance from the centreline; positive to the right of the travel direction.
    pub lateral: f32,
    pub tangent: V2,
}

fn sub(a: V2, b: V2) -> V2 {
    [a[0] - b[0], a[1] - b[1]]
}
fn dot(a: V2, b: V2) -> f32 {
    a[0] * b[0] + a[1] * b[1]
}
fn len(a: V2) -> f32 {
    a[0].hypot(a[1])
}

impl Track {
    pub fn from_points(name: &str, points: Vec<V2>, half_width: f32, closed: bool) -> Track {
        let mut cum = vec![0.0];
        for i in 1..points.len() {
            cum.push(cum[i - 1] + len(sub(points[i], points[i - 1])));
        }
        Track {
            name: name.into(),
            points,
            cum,
            half_width,
            closed,
            run_off: 3.0,
        }
    }

    pub fn length(&self) -> f32 {
        let last = *self.cum.last().unwrap();
        if self.closed {
            last + len(sub(self.points[0], *self.points.last().unwrap()))
        } else {
            last
        }
    }

    /// Straight strip of `length` metres along +y, two lanes wide.
    pub fn drag_strip(length: f32) -> Track {
        let pts: Vec<V2> = (0..=((length + 300.0) / 5.0) as usize)
            .map(|i| [0.0, i as f32 * 5.0 - 40.0])
            .collect();
        let mut t = Track::from_points("Drag Strip", pts, 7.5, false);
        t.run_off = 2.0;
        t
    }

    /// A ~2.6 km closed circuit with a long straight, a hairpin and a set of esses.
    pub fn circuit() -> Track {
        let ctrl: [V2; 18] = [
            [0.0, 0.0],
            [0.0, 260.0],
            [50.0, 430.0],
            [190.0, 520.0],
            [380.0, 535.0],
            [530.0, 470.0],
            [575.0, 320.0],
            [500.0, 200.0],
            [370.0, 160.0],
            [320.0, 70.0],
            [400.0, -50.0],
            [530.0, -110.0],
            [610.0, -250.0],
            [540.0, -390.0],
            [360.0, -420.0],
            [190.0, -350.0],
            [90.0, -240.0],
            [40.0, -110.0],
        ];
        let n = ctrl.len();
        let mut pts = Vec::new();
        for i in 0..n {
            let p0 = ctrl[(i + n - 1) % n];
            let p1 = ctrl[i];
            let p2 = ctrl[(i + 1) % n];
            let p3 = ctrl[(i + 2) % n];
            let seg = len(sub(p2, p1));
            let steps = (seg / 4.0).ceil().max(2.0) as usize;
            for k in 0..steps {
                let t = k as f32 / steps as f32;
                let (t2, t3) = (t * t, t * t * t);
                let c = |a: f32, b: f32, c_: f32, d: f32| {
                    0.5 * ((2.0 * b)
                        + (-a + c_) * t
                        + (2.0 * a - 5.0 * b + 4.0 * c_ - d) * t2
                        + (-a + 3.0 * b - 3.0 * c_ + d) * t3)
                };
                pts.push([c(p0[0], p1[0], p2[0], p3[0]), c(p0[1], p1[1], p2[1], p3[1])]);
            }
        }
        Track::from_points("Redline Circuit", pts, 7.0, true)
    }

    fn seg_end(&self, i: usize) -> V2 {
        self.points[(i + 1) % self.points.len()]
    }

    fn seg_count(&self) -> usize {
        if self.closed {
            self.points.len()
        } else {
            self.points.len() - 1
        }
    }

    pub fn project(&self, p: V2) -> Projection {
        let mut best = (f32::MAX, 0usize, 0.0f32);
        for i in 0..self.seg_count() {
            let a = self.points[i];
            let b = self.seg_end(i);
            let ab = sub(b, a);
            let l2 = dot(ab, ab).max(1e-9);
            let t = (dot(sub(p, a), ab) / l2).clamp(0.0, 1.0);
            let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
            let d = len(sub(p, q));
            if d < best.0 {
                best = (d, i, t);
            }
        }
        let (_, i, t) = best;
        let a = self.points[i];
        let b = self.seg_end(i);
        let ab = sub(b, a);
        let l = len(ab).max(1e-9);
        let tangent = [ab[0] / l, ab[1] / l];
        let q = [a[0] + ab[0] * t, a[1] + ab[1] * t];
        // Right-hand normal of the tangent.
        let right = [tangent[1], -tangent[0]];
        let lateral = dot(sub(p, q), right);
        Projection {
            index: i,
            s: self.cum[i] + l * t,
            lateral,
            tangent,
        }
    }

    /// Point on the centreline at arc length `s` plus the tangent there.
    pub fn at(&self, s: f32) -> (V2, V2) {
        let total = self.length();
        let s = if self.closed {
            s.rem_euclid(total)
        } else {
            s.clamp(0.0, total)
        };
        let i = match self.cum.binary_search_by(|c| c.partial_cmp(&s).unwrap()) {
            Ok(i) => i,
            Err(i) => i.saturating_sub(1),
        }
        .min(self.seg_count() - 1);
        let a = self.points[i];
        let b = self.seg_end(i);
        let ab = sub(b, a);
        let l = len(ab).max(1e-9);
        let t = ((s - self.cum[i]) / l).clamp(0.0, 1.0);
        ([a[0] + ab[0] * t, a[1] + ab[1] * t], [ab[0] / l, ab[1] / l])
    }

    /// Signed curvature (1/m, positive = turning left) estimated over `span` metres at `s`.
    pub fn curvature(&self, s: f32, span: f32) -> f32 {
        let (_, t0) = self.at(s - span / 2.0);
        let (_, t1) = self.at(s + span / 2.0);
        let cross = t0[0] * t1[1] - t0[1] * t1[0];
        cross.atan2(dot(t0, t1)) / span
    }

    /// Heading angle (our convention: 0 = +y, CCW positive) of a direction vector.
    pub fn heading_of(t: V2) -> f32 {
        (-t[0]).atan2(t[1])
    }

    /// Grid slot `n` (0 = front) behind the start line, offset by `lane` metres to the right.
    pub fn grid_slot(&self, behind: f32, lane: f32) -> (V2, f32) {
        let (p, t) = self.at(0.0);
        let right = [t[1], -t[0]];
        let pos = [
            p[0] - t[0] * behind + right[0] * lane,
            p[1] - t[1] * behind + right[1] * lane,
        ];
        (pos, Self::heading_of(t))
    }

    /// Barrier distance from the centreline.
    pub fn wall_distance(&self) -> f32 {
        self.half_width + self.run_off
    }

    /// Check a point against the barriers; returns (outward-pointing normal, penetration).
    pub fn wall_contact(&self, p: V2) -> Option<(V2, f32)> {
        let pr = self.project(p);
        // Open tracks have no wall across their ends: only the sides.
        let over = pr.lateral.abs() - self.wall_distance();
        if over > 0.0 {
            let right = [pr.tangent[1], -pr.tangent[0]];
            let sign = pr.lateral.signum();
            // Normal points back into the track.
            Some(([-right[0] * sign, -right[1] * sign], over))
        } else {
            None
        }
    }

    pub fn on_road(&self, p: V2) -> bool {
        self.project(p).lateral.abs() <= self.half_width
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drag_strip_is_straight_and_long_enough() {
        let t = Track::drag_strip(402.3);
        assert!(t.length() > 402.3 + 200.0);
        let pr = t.project([3.0, 100.0]);
        assert!((pr.lateral - 3.0).abs() < 1e-3 || (pr.lateral + 3.0).abs() < 1e-3);
        assert!((pr.s - 140.0).abs() < 1.0, "{}", pr.s);
    }

    #[test]
    fn projection_sign_is_right_of_travel() {
        let t = Track::drag_strip(402.3);
        // Travel is +y; the right-hand side is +x.
        assert!(t.project([2.0, 50.0]).lateral > 0.0);
        assert!(t.project([-2.0, 50.0]).lateral < 0.0);
    }

    #[test]
    fn circuit_is_closed_reasonably_sized_and_does_not_cross_itself() {
        let t = Track::circuit();
        let l = t.length();
        assert!((2000.0..4000.0).contains(&l), "length {l}");
        let n = t.points.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let arc = (t.cum[j] - t.cum[i]).min(l - (t.cum[j] - t.cum[i]));
                if arc > 4.0 * t.half_width + 20.0 {
                    let d = len(sub(t.points[i], t.points[j]));
                    assert!(
                        d > 2.0 * t.half_width,
                        "points {i},{j} only {d} apart (arc {arc})"
                    );
                }
            }
        }
    }

    #[test]
    fn walls_push_back_towards_the_road() {
        let t = Track::drag_strip(402.3);
        assert!(t.wall_contact([0.0, 100.0]).is_none());
        let (n, depth) = t.wall_contact([t.wall_distance() + 0.5, 100.0]).unwrap();
        assert!(n[0] < 0.0 && depth > 0.4);
    }

    #[test]
    fn curvature_sign_matches_turn_direction() {
        let t = Track::circuit();
        // Find a left and right turn somewhere on the lap.
        let (mut left, mut right) = (false, false);
        let mut s = 0.0;
        while s < t.length() {
            let k = t.curvature(s, 20.0);
            left |= k > 0.01;
            right |= k < -0.01;
            s += 10.0;
        }
        assert!(left && right);
    }

    #[test]
    fn at_returns_unit_tangent_and_wraps() {
        let t = Track::circuit();
        let (_, tan) = t.at(t.length() + 10.0);
        assert!((len(tan) - 1.0).abs() < 1e-3);
        let (p0, _) = t.at(10.0);
        let (p1, _) = t.at(t.length() + 10.0);
        assert!(len(sub(p0, p1)) < 1e-2);
    }
}

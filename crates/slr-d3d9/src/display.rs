//! Window/display geometry for the real macOS path (obligation S9, P30).
//!
//! The game renders a fixed-size back buffer (the D3D9 mode, 800x600 by default)
//! and the host presents it to whatever the window or display is, exactly as
//! a D3D9 `Present` to a resized window would: the picture is scaled uniformly,
//! centred, with black bars. Nothing here touches the render stream.

/// Back-buffer size of the exe's default mode (`Options` offers others).
pub const DEFAULT_LOGICAL: (u32, u32) = (800, 600);

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Rect {
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

/// Largest centred uniform scaling of `logical` that fits `drawable`.
pub fn letterbox(logical: (u32, u32), drawable: (u32, u32)) -> Rect {
    let (lw, lh) = (u64::from(logical.0.max(1)), u64::from(logical.1.max(1)));
    let (dw, dh) = (u64::from(drawable.0), u64::from(drawable.1));
    // Compare aspect ratios without floating point: lw/lh <= dw/dh.
    let (w, h) = if lw * dh <= dw * lh {
        ((dh * lw / lh).max(1).min(dw.max(1)), dh)
    } else {
        (dw, (dw * lh / lw).max(1).min(dh.max(1)))
    };
    Rect {
        x: ((dw - w.min(dw)) / 2) as u32,
        y: ((dh - h.min(dh)) / 2) as u32,
        w: w as u32,
        h: h as u32,
    }
}

/// Window pointer position (drawable pixels) to the normalised coordinates the
/// game sees across its back buffer. Positions in the black bars clamp to the
/// nearest edge of the picture.
pub fn pointer_to_logical(px: f64, py: f64, rect: Rect) -> (f32, f32) {
    let u = (px - f64::from(rect.x)) / f64::from(rect.w.max(1));
    let v = (py - f64::from(rect.y)) / f64::from(rect.h.max(1));
    (u.clamp(0., 1.) as f32, v.clamp(0., 1.) as f32)
}

/// Whether a drawable position lies on the picture rather than a bar.
pub fn on_picture(px: f64, py: f64, rect: Rect) -> bool {
    px >= f64::from(rect.x)
        && py >= f64::from(rect.y)
        && px < f64::from(rect.x + rect.w)
        && py < f64::from(rect.y + rect.h)
}

/// Screen-space quad `[x0, y0, x1, y1]` (back-buffer pixels) the exe draws its
/// video on: the aspect-fit of `video` inside the back buffer, with the D3D9
/// half-pixel texel offset (`FUN_004f8b30`: offsets start at -0.5 and the
/// slack axis gets `(slack) * 0.5 - 0.5`).
pub fn video_quad(video: (u32, u32), back_buffer: (u32, u32)) -> [f32; 4] {
    let (vw, vh) = (video.0.max(1) as f32, video.1.max(1) as f32);
    let (bw, bh) = (back_buffer.0 as f32, back_buffer.1 as f32);
    let aspect = vw / vh;
    let (mut ox, mut oy) = (-0.5f32, -0.5f32);
    if aspect <= bw / bh {
        ox = (bw - bh * aspect) * 0.5 - 0.5;
    } else {
        oy = (bh - bw / aspect) * 0.5 - 0.5;
    }
    [ox, oy, bw - 1.0 - ox, bh - 1.0 - oy]
}

/// Back-buffer pixel quad to clip space (`y` up).
pub fn to_clip(quad: [f32; 4], back_buffer: (u32, u32)) -> [f32; 4] {
    let (bw, bh) = (back_buffer.0 as f32, back_buffer.1 as f32);
    [
        quad[0] / bw * 2. - 1.,
        1. - quad[1] / bh * 2.,
        quad[2] / bw * 2. - 1.,
        1. - quad[3] / bh * 2.,
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_aspect_fills_the_drawable() {
        let r = letterbox((800, 600), (1600, 1200));
        assert_eq!(
            r,
            Rect {
                x: 0,
                y: 0,
                w: 1600,
                h: 1200
            }
        );
    }

    #[test]
    fn wide_drawable_gets_side_bars() {
        // 16:10 laptop panel in native pixels.
        let r = letterbox((800, 600), (3024, 1964));
        assert_eq!((r.h, r.y), (1964, 0));
        assert_eq!(r.w, 1964 * 4 / 3);
        assert_eq!(r.x, (3024 - r.w) / 2);
        assert!(r.x > 0 && r.x + r.w <= 3024);
    }

    #[test]
    fn tall_drawable_gets_top_and_bottom_bars() {
        let r = letterbox((800, 600), (600, 900));
        assert_eq!((r.x, r.w), (0, 600));
        assert_eq!(r.h, 450);
        assert_eq!(r.y, 225);
    }

    #[test]
    fn degenerate_drawables_do_not_panic() {
        for d in [(0, 0), (1, 1), (0, 5), (5, 0), (1, 100000)] {
            let r = letterbox((800, 600), d);
            assert!(r.x + r.w <= d.0.max(1) + 1 && r.y + r.h <= d.1.max(1) + 1);
        }
    }

    #[test]
    fn pointer_maps_through_the_picture_and_clamps_in_bars() {
        let r = letterbox((800, 600), (1600, 900));
        assert_eq!((r.x, r.w), (200, 1200));
        assert_eq!(pointer_to_logical(200., 0., r), (0., 0.));
        assert_eq!(pointer_to_logical(800., 450., r), (0.5, 0.5));
        assert_eq!(pointer_to_logical(1400., 900., r), (1., 1.));
        assert_eq!(pointer_to_logical(10., 450., r), (0., 0.5));
        assert!(!on_picture(10., 450., r));
        assert!(on_picture(200., 0., r));
        assert!(!on_picture(1400., 0., r));
    }

    #[test]
    fn video_quad_matches_the_exe_formula() {
        // Prime.avi is 4:3: it exactly fills an 800x600 back buffer.
        assert_eq!(
            video_quad((160, 120), (800, 600)),
            [-0.5, -0.5, 799.5, 599.5]
        );
        // A 16:10 back buffer pillarboxes it: (1280 - 800*4/3)/2 - 0.5.
        let q = video_quad((160, 120), (1280, 800));
        let ox = (1280. - 800. * (4. / 3.)) * 0.5 - 0.5;
        assert!((q[0] - ox).abs() < 1e-4 && (q[1] + 0.5).abs() < 1e-6);
        assert!((q[2] - (1280. - 1. - ox)).abs() < 1e-4);
        // A 640x400 (16:10) video letterboxes in 800x600.
        let q = video_quad((640, 400), (800, 600));
        let oy = (600. - 800. / 1.6) * 0.5 - 0.5;
        assert!((q[1] - oy).abs() < 1e-4 && (q[0] + 0.5).abs() < 1e-6);
    }

    #[test]
    fn clip_space_corners() {
        let c = to_clip([-0.5, -0.5, 799.5, 599.5], (800, 600));
        assert!((c[0] + 1.00125).abs() < 1e-5 && (c[1] - 1.001_666_7).abs() < 1e-5);
        assert!((c[2] - 0.99875).abs() < 1e-5 && (c[3] + 0.998_333_3).abs() < 1e-5);
    }
}

/// Shortcut ownership stays with the initial key press until its own release.
#[derive(Clone, Copy)]
pub enum ShortcutKey {
    Enter,
    NumpadEnter,
    F11,
}
#[derive(Default)]
pub struct ShortcutKeys {
    owners: [Option<bool>; 3],
}
impl ShortcutKeys {
    /// Returns (consumed by host, toggle fullscreen).
    pub fn event(
        &mut self,
        key: ShortcutKey,
        pressed: bool,
        repeat: bool,
        modified: bool,
    ) -> (bool, bool) {
        let index = match key {
            ShortcutKey::Enter => 0,
            ShortcutKey::NumpadEnter => 1,
            ShortcutKey::F11 => 2,
        };
        if !pressed {
            return (self.owners[index].take().unwrap_or(index == 2), false);
        }
        if let Some(host) = self.owners[index] {
            return (host, false);
        }
        let host = index == 2 || modified && !repeat;
        self.owners[index] = Some(host);
        (host, host && !repeat)
    }
}
#[cfg(test)]
mod shortcut_tests {
    use super::*;
    #[test]
    fn shortcut_repeats_and_release_stay_consumed_after_modifier_release() {
        let mut keys = ShortcutKeys::default();
        assert_eq!(
            keys.event(ShortcutKey::Enter, true, false, true),
            (true, true)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, true, true, false),
            (true, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, false, false, false),
            (true, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, true, false, false),
            (false, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, false, false, false),
            (false, false)
        );
    }
    #[test]
    fn game_press_does_not_become_a_shortcut_mid_repeat_or_steal_numpad_release() {
        let mut keys = ShortcutKeys::default();
        assert_eq!(
            keys.event(ShortcutKey::Enter, true, false, false),
            (false, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, true, true, true),
            (false, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::NumpadEnter, true, false, true),
            (true, true)
        );
        assert_eq!(
            keys.event(ShortcutKey::Enter, false, false, true),
            (false, false)
        );
        assert_eq!(
            keys.event(ShortcutKey::NumpadEnter, false, false, false),
            (true, false)
        );
    }
}

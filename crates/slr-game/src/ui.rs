//! A tiny immediate-mode UI on a fixed 1280x720 virtual canvas with mouse and keyboard
//! navigation (Up/Down moves the selection, Enter activates, Left/Right adjusts sliders).

use macroquad::prelude::*;

pub const VW: f32 = 1280.0;
pub const VH: f32 = 720.0;

pub const PANEL: Color = Color::new(0.11, 0.13, 0.16, 0.94);
pub const PANEL2: Color = Color::new(0.16, 0.18, 0.22, 1.0);
pub const ACCENT: Color = Color::new(0.93, 0.27, 0.22, 1.0);
pub const TEXT: Color = Color::new(0.92, 0.93, 0.95, 1.0);
pub const DIM: Color = Color::new(0.58, 0.62, 0.68, 1.0);
pub const GOOD: Color = Color::new(0.35, 0.8, 0.45, 1.0);
pub const WARN: Color = Color::new(0.95, 0.75, 0.25, 1.0);

pub struct Ui {
    pub scale: f32,
    ox: f32,
    oy: f32,
    pub mouse: Vec2,
    pub mouse_down: bool,
    pub clicked: bool,
    items: usize,
    last_items: usize,
    pub selected: usize,
    enter: bool,
    left: bool,
    right: bool,
    pub font: Option<Font>,
    pub wheel: f32,
    /// Click to synthesise on the next frame (virtual coordinates), for the automated self-test.
    pub inject: Option<Vec2>,
}

impl Ui {
    pub fn new(font: Option<Font>) -> Ui {
        Ui {
            scale: 1.0,
            ox: 0.0,
            oy: 0.0,
            mouse: vec2(0.0, 0.0),
            mouse_down: false,
            clicked: false,
            items: 0,
            last_items: 0,
            selected: 0,
            enter: false,
            left: false,
            right: false,
            font,
            wheel: 0.0,
            inject: None,
        }
    }

    pub fn begin(&mut self) {
        let (sw, sh) = (screen_width(), screen_height());
        self.scale = (sw / VW).min(sh / VH);
        self.ox = (sw - VW * self.scale) / 2.0;
        self.oy = (sh - VH * self.scale) / 2.0;
        let (mx, my) = mouse_position();
        self.mouse = vec2((mx - self.ox) / self.scale, (my - self.oy) / self.scale);
        self.mouse_down = is_mouse_button_down(MouseButton::Left);
        self.clicked = is_mouse_button_pressed(MouseButton::Left);
        self.enter = is_key_pressed(KeyCode::Enter) || is_key_pressed(KeyCode::KpEnter);
        self.left = is_key_pressed(KeyCode::Left);
        self.right = is_key_pressed(KeyCode::Right);
        self.wheel = mouse_wheel().1;
        if let Some(p) = self.inject.take() {
            self.mouse = p;
            self.clicked = true;
            self.mouse_down = true;
        }
        if self.last_items > 0 {
            if is_key_pressed(KeyCode::Down) {
                self.selected = (self.selected + 1) % self.last_items;
            }
            if is_key_pressed(KeyCode::Up) {
                self.selected = (self.selected + self.last_items - 1) % self.last_items;
            }
        }
        self.items = 0;
    }

    pub fn end(&mut self) {
        self.last_items = self.items;
        if self.selected >= self.items && self.items > 0 {
            self.selected = self.items - 1;
        }
    }

    pub fn reset_selection(&mut self) {
        self.selected = 0;
    }

    fn sx(&self, v: f32) -> f32 {
        self.ox + v * self.scale
    }
    fn sy(&self, v: f32) -> f32 {
        self.oy + v * self.scale
    }

    pub fn rect(&self, x: f32, y: f32, w: f32, h: f32, c: Color) {
        draw_rectangle(self.sx(x), self.sy(y), w * self.scale, h * self.scale, c);
    }

    pub fn outline(&self, x: f32, y: f32, w: f32, h: f32, t: f32, c: Color) {
        draw_rectangle_lines(
            self.sx(x),
            self.sy(y),
            w * self.scale,
            h * self.scale,
            t * self.scale,
            c,
        );
    }

    pub fn text(&self, s: &str, x: f32, y: f32, size: f32, c: Color) {
        let params = TextParams {
            font: self.font.as_ref(),
            font_size: (size * self.scale).round() as u16,
            color: c,
            ..Default::default()
        };
        draw_text_ex(s, self.sx(x), self.sy(y), params);
    }

    pub fn text_width(&self, s: &str, size: f32) -> f32 {
        measure_text(
            s,
            self.font.as_ref(),
            (size * self.scale).round() as u16,
            1.0,
        )
        .width
            / self.scale
    }

    pub fn text_centered(&self, s: &str, cx: f32, y: f32, size: f32, c: Color) {
        let w = self.text_width(s, size);
        self.text(s, cx - w / 2.0, y, size, c);
    }

    pub fn text_right(&self, s: &str, rx: f32, y: f32, size: f32, c: Color) {
        let w = self.text_width(s, size);
        self.text(s, rx - w, y, size, c);
    }

    pub fn hovered(&self, x: f32, y: f32, w: f32, h: f32) -> bool {
        self.mouse.x >= x && self.mouse.x <= x + w && self.mouse.y >= y && self.mouse.y <= y + h
    }

    /// Clickable button; also activated with Enter when it holds the keyboard selection.
    pub fn button(&mut self, x: f32, y: f32, w: f32, h: f32, label: &str, enabled: bool) -> bool {
        let id = self.items;
        self.items += 1;
        let hover = self.hovered(x, y, w, h);
        if hover && (self.mouse_delta_moved()) {
            self.selected = id;
        }
        let sel = self.selected == id;
        let base = if !enabled {
            Color::new(0.13, 0.14, 0.16, 1.0)
        } else if sel || hover {
            ACCENT
        } else {
            PANEL2
        };
        self.rect(x, y, w, h, base);
        if sel {
            self.outline(x, y, w, h, 2.0, TEXT);
        }
        let col = if enabled {
            TEXT
        } else {
            Color::new(0.45, 0.47, 0.5, 1.0)
        };
        self.text_centered(label, x + w / 2.0, y + h / 2.0 + 6.0, 22.0, col);
        enabled && ((hover && self.clicked) || (sel && self.enter))
    }

    fn mouse_delta_moved(&self) -> bool {
        let d = mouse_delta_position();
        d.x != 0.0 || d.y != 0.0 || self.clicked
    }

    /// Horizontal slider returning the new value.
    pub fn slider(
        &mut self,
        x: f32,
        y: f32,
        w: f32,
        label: &str,
        value: f32,
        min: f32,
        max: f32,
        fmt: &dyn Fn(f32) -> String,
        enabled: bool,
    ) -> f32 {
        let id = self.items;
        self.items += 1;
        let h = 34.0;
        let hover = self.hovered(x, y, w, h);
        let mut v = value;
        if enabled {
            if hover && self.mouse_down {
                let t = ((self.mouse.x - (x + 190.0)) / (w - 190.0 - 90.0)).clamp(0.0, 1.0);
                v = min + t * (max - min);
                self.selected = id;
            }
            if self.selected == id {
                let step = (max - min) / 40.0;
                if self.left {
                    v = (v - step).max(min);
                }
                if self.right {
                    v = (v + step).min(max);
                }
            }
        }
        let sel = self.selected == id;
        self.rect(
            x,
            y,
            w,
            h,
            if sel {
                PANEL2
            } else {
                Color::new(0.0, 0.0, 0.0, 0.0)
            },
        );
        self.text(
            label,
            x + 8.0,
            y + 24.0,
            20.0,
            if enabled { TEXT } else { DIM },
        );
        let tx = x + 190.0;
        let tw = w - 190.0 - 90.0;
        self.rect(tx, y + 15.0, tw, 4.0, Color::new(0.3, 0.33, 0.38, 1.0));
        let t = ((v - min) / (max - min)).clamp(0.0, 1.0);
        self.rect(
            tx,
            y + 15.0,
            tw * t,
            4.0,
            if enabled { ACCENT } else { DIM },
        );
        self.rect(
            tx + tw * t - 6.0,
            y + 8.0,
            12.0,
            18.0,
            if enabled { TEXT } else { DIM },
        );
        self.text_right(
            &fmt(v),
            x + w - 6.0,
            y + 24.0,
            20.0,
            if enabled { TEXT } else { DIM },
        );
        v
    }

    pub fn bar(&self, x: f32, y: f32, w: f32, h: f32, frac: f32, c: Color) {
        self.rect(x, y, w, h, Color::new(0.2, 0.22, 0.26, 1.0));
        self.rect(x, y, w * frac.clamp(0.0, 1.0), h, c);
    }

    pub fn line(&self, x1: f32, y1: f32, x2: f32, y2: f32, t: f32, c: Color) {
        draw_line(
            self.sx(x1),
            self.sy(y1),
            self.sx(x2),
            self.sy(y2),
            t * self.scale,
            c,
        );
    }

    pub fn circle(&self, x: f32, y: f32, r: f32, c: Color) {
        draw_circle(self.sx(x), self.sy(y), r * self.scale, c);
    }
}

pub fn money(v: f32) -> String {
    let n = v.round() as i64;
    let s = n.abs().to_string();
    let mut out = String::new();
    for (i, ch) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(ch);
    }
    format!("{}${}", if n < 0 { "-" } else { "" }, out)
}

pub fn condition_colour(frac_good: f32) -> Color {
    if frac_good > 0.66 {
        GOOD
    } else if frac_good > 0.33 {
        WARN
    } else {
        ACCENT
    }
}

pub fn fmt_time(t: f32) -> String {
    let m = (t / 60.0).floor() as i32;
    format!("{}:{:05.2}", m, t - m as f32 * 60.0)
}

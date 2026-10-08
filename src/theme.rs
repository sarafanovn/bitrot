//! macOS-flavoured look: system colours, SF Pro, and a few custom widgets
//! (segmented control, slider, cards, vector icons) that egui lacks.

use std::ops::RangeInclusive;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Painter, Pos2, Rect,
    Response, Sense, Shadow, Stroke, TextStyle, Theme, Ui, Vec2, pos2, vec2,
};

#[derive(Clone, Copy)]
pub struct Palette {
    pub toolbar: Color32,
    pub sidebar: Color32,
    pub card: Color32,
    pub canvas: Color32,
    pub separator: Color32,
    pub text: Color32,
    pub secondary: Color32,
    pub accent: Color32,
    pub control: Color32,
    pub control_selected: Color32,
    pub hover: Color32,
    pub button: Color32,
    pub track: Color32,
    pub checker: [u8; 2],
}

pub fn palette(dark: bool) -> Palette {
    if dark {
        Palette {
            toolbar: Color32::from_rgb(40, 40, 42),
            sidebar: Color32::from_rgb(32, 32, 34),
            card: Color32::from_rgb(44, 44, 46),
            canvas: Color32::from_rgb(24, 24, 26),
            separator: Color32::from_rgb(58, 58, 60),
            text: Color32::from_rgb(242, 242, 247),
            secondary: Color32::from_rgb(152, 152, 157),
            accent: Color32::from_rgb(10, 132, 255),
            control: Color32::from_rgb(28, 28, 30),
            control_selected: Color32::from_rgb(99, 99, 102),
            hover: Color32::from_rgb(58, 58, 60),
            button: Color32::from_rgb(72, 72, 74),
            track: Color32::from_rgb(72, 72, 74),
            checker: [58, 48],
        }
    } else {
        Palette {
            toolbar: Color32::from_rgb(246, 246, 246),
            sidebar: Color32::from_rgb(240, 240, 242),
            card: Color32::WHITE,
            canvas: Color32::from_rgb(226, 226, 230),
            separator: Color32::from_rgb(214, 214, 218),
            text: Color32::from_rgb(29, 29, 31),
            secondary: Color32::from_rgb(110, 110, 115),
            accent: Color32::from_rgb(0, 122, 255),
            control: Color32::from_rgb(226, 226, 230),
            control_selected: Color32::WHITE,
            hover: Color32::from_rgb(214, 214, 218),
            button: Color32::WHITE,
            track: Color32::from_rgb(218, 218, 222),
            checker: [255, 236],
        }
    }
}

pub fn pal(ui: &Ui) -> Palette {
    palette(ui.visuals().dark_mode)
}

pub fn install(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    // SF Pro ships with macOS but may not be loadable everywhere; fall back to egui's font.
    if let Ok(bytes) = std::fs::read("/System/Library/Fonts/SFNS.ttf") {
        fonts.font_data.insert("sf".into(), Arc::new(FontData::from_owned(bytes)));
        if let Some(family) = fonts.families.get_mut(&FontFamily::Proportional) {
            family.insert(0, "sf".into());
        }
    }
    ctx.set_fonts(fonts);
    for theme in [Theme::Dark, Theme::Light] {
        ctx.style_mut_of(theme, |style| configure(style, palette(theme == Theme::Dark)));
    }
}

fn configure(style: &mut egui::Style, p: Palette) {
    style.text_styles = [
        (TextStyle::Small, FontId::proportional(11.0)),
        (TextStyle::Body, FontId::proportional(13.0)),
        (TextStyle::Button, FontId::proportional(13.0)),
        (TextStyle::Heading, FontId::proportional(15.0)),
        (TextStyle::Monospace, FontId::monospace(12.0)),
    ]
    .into();

    let s = &mut style.spacing;
    s.item_spacing = vec2(8.0, 8.0);
    s.button_padding = vec2(10.0, 4.0);
    s.interact_size.y = 24.0;
    s.menu_margin = egui::Margin::same(6);

    let v = &mut style.visuals;
    v.override_text_color = Some(p.text);
    v.panel_fill = p.sidebar;
    v.window_fill = p.card;
    v.extreme_bg_color = p.control;
    v.faint_bg_color = p.card;
    v.window_corner_radius = CornerRadius::same(10);
    v.menu_corner_radius = CornerRadius::same(8);
    v.window_stroke = Stroke::new(0.5, p.separator);
    v.window_shadow = Shadow { offset: [0, 8], blur: 28, spread: 0, color: Color32::from_black_alpha(70) };
    v.popup_shadow = Shadow { offset: [0, 4], blur: 16, spread: 0, color: Color32::from_black_alpha(60) };
    v.selection.bg_fill = p.accent;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.hyperlink_color = p.accent;

    let radius = CornerRadius::same(6);
    let w = &mut v.widgets;
    w.noninteractive.bg_fill = p.card;
    w.noninteractive.weak_bg_fill = p.card;
    w.noninteractive.bg_stroke = Stroke::new(1.0, p.separator);
    w.noninteractive.fg_stroke = Stroke::new(1.0, p.text);
    w.noninteractive.corner_radius = radius;
    for (state, fill) in [
        (&mut w.inactive, p.button),
        (&mut w.hovered, p.hover),
        (&mut w.active, p.control_selected),
        (&mut w.open, p.hover),
    ] {
        state.bg_fill = fill;
        state.weak_bg_fill = fill;
        state.bg_stroke = Stroke::NONE;
        state.fg_stroke = Stroke::new(1.0, p.text);
        state.corner_radius = radius;
        state.expansion = 0.0;
    }
}

// ---------- layout helpers ----------

/// Grouped card with a small heading, like System Settings.
pub fn section(ui: &mut Ui, title: &str, trailing: impl FnOnce(&mut Ui), body: impl FnOnce(&mut Ui)) {
    let p = pal(ui);
    ui.horizontal(|ui| {
        ui.add_space(4.0);
        ui.label(egui::RichText::new(title).size(12.0).color(p.secondary));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), trailing);
    });
    ui.add_space(-4.0);
    egui::Frame::new()
        .fill(p.card)
        .corner_radius(10)
        .stroke(Stroke::new(0.5, p.separator))
        .inner_margin(12)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            body(ui);
        });
    ui.add_space(10.0);
}

pub fn hint(ui: &mut Ui, text: &str) {
    let p = pal(ui);
    ui.label(egui::RichText::new(text).size(11.5).color(p.secondary));
}

// ---------- slider ----------

pub struct SliderSpec<'a> {
    pub label: &'a str,
    pub range: RangeInclusive<f32>,
    pub log: bool,
    pub format: fn(f32) -> String,
}

pub fn px(v: f32) -> String {
    format!("{v:.0} px")
}
pub fn percent(v: f32) -> String {
    format!("{:.0}%", v * 100.0)
}
pub fn degrees(v: f32) -> String {
    format!("{v:.0}°")
}

/// macOS-style slider: label and value above a thin track with a white knob.
pub fn slider(ui: &mut Ui, value: &mut f32, spec: SliderSpec) -> Response {
    let p = pal(ui);
    let (min, max) = (*spec.range.start(), *spec.range.end());
    let to_t = |v: f32| {
        if spec.log { (v / min).ln() / (max / min).ln() } else { (v - min) / (max - min) }
    };
    let from_t = |t: f32| if spec.log { min * (max / min).powf(t) } else { min + (max - min) * t };

    ui.horizontal(|ui| {
        ui.label(spec.label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(egui::RichText::new((spec.format)(*value)).color(p.secondary).monospace().size(11.5));
        });
    });
    ui.add_space(-6.0);
    let (rect, mut resp) = ui.allocate_exact_size(vec2(ui.available_width(), 20.0), Sense::click_and_drag());
    let knob_r = 9.0;
    let left = rect.left() + knob_r;
    let width = (rect.width() - knob_r * 2.0).max(1.0);
    if let Some(pos) = resp.interact_pointer_pos() {
        if resp.dragged() || resp.clicked() || resp.is_pointer_button_down_on() {
            let t = ((pos.x - left) / width).clamp(0.0, 1.0);
            let new = from_t(t);
            if new != *value {
                *value = new;
                resp.mark_changed();
            }
        }
    }
    let t = to_t(*value).clamp(0.0, 1.0);
    let cy = rect.center().y;
    let painter = ui.painter();
    let track = Rect::from_min_max(pos2(left, cy - 2.0), pos2(left + width, cy + 2.0));
    painter.rect_filled(track, 2.0, p.track);
    let knob = pos2(left + width * t, cy);
    painter.rect_filled(Rect::from_min_max(track.min, pos2(knob.x, track.max.y)), 2.0, p.accent);
    painter.circle_filled(knob + vec2(0.0, 1.0), knob_r + 0.5, Color32::from_black_alpha(45));
    let knob_fill = if resp.is_pointer_button_down_on() { Color32::from_gray(236) } else { Color32::WHITE };
    painter.circle(knob, knob_r, knob_fill, Stroke::new(0.5, Color32::from_black_alpha(40)));
    ui.add_space(2.0);
    resp
}

// ---------- segmented control ----------

/// Row of equally sized segments; `paint` draws the content of segment `i`.
pub fn segmented<T: PartialEq + Copy>(
    ui: &mut Ui,
    current: &mut T,
    values: &[T],
    seg: Vec2,
    tooltips: &[&str],
    paint: impl Fn(&Painter, Rect, usize, Color32),
) {
    let p = pal(ui);
    let base = ui.next_auto_id();
    let (rect, _) = ui.allocate_exact_size(vec2(seg.x * values.len() as f32, seg.y), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 7.0, p.control);
    for (i, value) in values.iter().enumerate() {
        let r = Rect::from_min_size(rect.min + vec2(seg.x * i as f32, 0.0), seg);
        let mut resp = ui.interact(r, base.with(i), Sense::click());
        if let Some(tip) = tooltips.get(i) {
            resp = resp.on_hover_text(*tip);
        }
        if resp.clicked() {
            *current = *value;
        }
        let selected = *current == *value;
        let pill = r.shrink(2.0);
        if selected {
            painter.rect_filled(pill.translate(vec2(0.0, 0.5)), 5.0, Color32::from_black_alpha(30));
            painter.rect_filled(pill, 5.0, p.control_selected);
        } else if resp.hovered() {
            painter.rect_filled(pill, 5.0, p.hover.gamma_multiply(0.6));
        }
        let color = if selected { p.text } else { p.secondary };
        paint(&painter, r, i, color);
    }
}

pub fn segmented_text<T: PartialEq + Copy>(ui: &mut Ui, current: &mut T, items: &[(T, &str)]) {
    let values: Vec<T> = items.iter().map(|(v, _)| *v).collect();
    let seg = vec2(ui.available_width() / items.len() as f32, 26.0);
    segmented(ui, current, &values, seg, &[], |painter, r, i, color| {
        painter.text(r.center(), egui::Align2::CENTER_CENTER, items[i].1, FontId::proportional(12.5), color);
    });
}

// ---------- buttons ----------

pub fn text_button(ui: &mut Ui, text: &str, enabled: bool, primary: bool) -> Response {
    let p = pal(ui);
    let galley = ui.painter().layout_no_wrap(text.into(), FontId::proportional(13.0), Color32::PLACEHOLDER);
    let size = vec2(galley.size().x + 24.0, 26.0);
    let (rect, resp) = ui.allocate_exact_size(size, if enabled { Sense::click() } else { Sense::hover() });
    let (fill, fg) = if primary {
        (p.accent, Color32::WHITE)
    } else {
        (if resp.hovered() && enabled { p.hover } else { p.button }, p.text)
    };
    let fill = if primary && resp.hovered() && enabled { fill.gamma_multiply(0.88) } else { fill };
    let alpha = if enabled { 1.0 } else { 0.4 };
    let painter = ui.painter();
    if !primary {
        painter.rect_filled(rect.translate(vec2(0.0, 0.5)), 6.0, Color32::from_black_alpha(25).gamma_multiply(alpha));
    }
    painter.rect_filled(rect, 6.0, fill.gamma_multiply(alpha));
    painter.galley(rect.center() - galley.size() * 0.5, galley, fg.gamma_multiply(alpha));
    resp
}

pub fn icon_button(ui: &mut Ui, icon: Icon, tooltip: &str, enabled: bool) -> Response {
    let p = pal(ui);
    let (rect, resp) = ui.allocate_exact_size(vec2(30.0, 26.0), if enabled { Sense::click() } else { Sense::hover() });
    if enabled && resp.hovered() {
        ui.painter().rect_filled(rect, 6.0, p.hover.gamma_multiply(0.7));
    }
    let color = if enabled { p.text.gamma_multiply(0.85) } else { p.secondary.gamma_multiply(0.5) };
    paint_icon(ui.painter(), rect, icon, color);
    resp.on_hover_text(tooltip)
}

/// Rounded pill floating over the canvas.
pub fn pill(painter: &Painter, p: Palette, center_bottom: Pos2, text: &str) {
    let galley = painter.layout_no_wrap(text.into(), FontId::proportional(12.5), p.text);
    let rect = Rect::from_center_size(center_bottom - vec2(0.0, 16.0 + galley.size().y * 0.5), galley.size() + vec2(28.0, 14.0));
    painter.add(Shadow { offset: [0, 3], blur: 14, spread: 0, color: Color32::from_black_alpha(50) }.as_shape(rect, CornerRadius::same(255)));
    painter.rect(rect, 255.0, p.card, Stroke::new(0.5, p.separator), egui::StrokeKind::Inside);
    painter.galley(rect.center() - galley.size() * 0.5, galley, p.text);
}

// ---------- icons ----------

#[derive(Clone, Copy)]
pub enum Icon {
    Brush,
    Eraser,
    Gradient,
    Rect,
    Ellipse,
    Undo,
    Redo,
    Fit,
    Plus,
    Close,
    Eye,
    EyeOff,
}

pub fn paint_icon(painter: &Painter, rect: Rect, icon: Icon, color: Color32) {
    let c = rect.center();
    let stroke = Stroke::new(1.5, color);
    match icon {
        Icon::Brush => {
            painter.circle_filled(c, 4.5, color);
            painter.circle_stroke(c, 7.5, Stroke::new(1.2, color.gamma_multiply(0.45)));
        }
        Icon::Eraser => {
            let (dir, nrm) = (vec2(0.707, -0.707), vec2(0.707, 0.707));
            let corner = |a: f32, b: f32| c + dir * a + nrm * b;
            let outline = vec![corner(-8.0, -4.0), corner(8.0, -4.0), corner(8.0, 4.0), corner(-8.0, 4.0)];
            let front = vec![corner(-8.0, -4.0), corner(-1.0, -4.0), corner(-1.0, 4.0), corner(-8.0, 4.0)];
            painter.add(egui::Shape::convex_polygon(front, color, Stroke::NONE));
            painter.add(egui::Shape::closed_line(outline, stroke));
        }
        Icon::Gradient => {
            let r = Rect::from_center_size(c, vec2(16.0, 14.0));
            let strips = 8;
            for i in 0..strips {
                let t = (i as f32 + 0.5) / strips as f32;
                let x0 = r.left() + r.width() * i as f32 / strips as f32;
                let x1 = r.left() + r.width() * (i + 1) as f32 / strips as f32;
                painter.rect_filled(
                    Rect::from_min_max(pos2(x0, r.top()), pos2(x1, r.bottom())),
                    0.0,
                    color.gamma_multiply(0.08 + 0.92 * t),
                );
            }
            painter.rect_stroke(r, 3.0, Stroke::new(1.2, color), egui::StrokeKind::Middle);
        }
        Icon::Rect => {
            painter.rect_stroke(Rect::from_center_size(c, vec2(17.0, 13.0)), 2.5, stroke, egui::StrokeKind::Middle);
        }
        Icon::Ellipse => {
            painter.add(egui::Shape::ellipse_stroke(c, vec2(8.5, 6.5), stroke));
        }
        Icon::Undo | Icon::Redo => {
            let flip = if matches!(icon, Icon::Undo) { 1.0 } else { -1.0 };
            let pts: Vec<Pos2> = (0..=16)
                .map(|i| {
                    let a = std::f32::consts::PI * (1.15 - 1.3 * i as f32 / 16.0);
                    c + vec2(-a.cos() * 6.0 * flip, -a.sin() * 6.0 + 2.5)
                })
                .collect();
            let tip = pts[0];
            painter.add(egui::Shape::line(pts, stroke));
            painter.line_segment([tip, tip + vec2(4.0 * flip, -0.5)], stroke);
            painter.line_segment([tip, tip + vec2(0.5 * flip, -4.5)], stroke);
        }
        Icon::Fit => {
            let r = Rect::from_center_size(c, vec2(14.0, 12.0));
            for (corner, dx, dy) in [
                (r.left_top(), 1.0, 1.0),
                (r.right_top(), -1.0, 1.0),
                (r.left_bottom(), 1.0, -1.0),
                (r.right_bottom(), -1.0, -1.0),
            ] {
                painter.line_segment([corner, corner + vec2(4.0 * dx, 0.0)], stroke);
                painter.line_segment([corner, corner + vec2(0.0, 4.0 * dy)], stroke);
            }
        }
        Icon::Plus => {
            painter.line_segment([c - vec2(5.5, 0.0), c + vec2(5.5, 0.0)], stroke);
            painter.line_segment([c - vec2(0.0, 5.5), c + vec2(0.0, 5.5)], stroke);
        }
        Icon::Close => {
            let s = Stroke::new(1.3, color);
            painter.line_segment([c - vec2(3.5, 3.5), c + vec2(3.5, 3.5)], s);
            painter.line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
        }
        Icon::Eye | Icon::EyeOff => {
            let s = Stroke::new(1.2, color);
            let top: Vec<Pos2> = (0..=12)
                .map(|i| {
                    let t = i as f32 / 12.0;
                    c + vec2((t - 0.5) * 14.0, -(t * std::f32::consts::PI).sin() * 5.0)
                })
                .collect();
            let bottom: Vec<Pos2> = top.iter().map(|q| pos2(q.x, 2.0 * c.y - q.y)).collect();
            painter.add(egui::Shape::line(top, s));
            painter.add(egui::Shape::line(bottom, s));
            painter.circle_filled(c, 2.2, color);
            if matches!(icon, Icon::EyeOff) {
                painter.line_segment([c + vec2(-6.0, 6.0), c + vec2(6.0, -6.0)], s);
            }
        }
    }
}

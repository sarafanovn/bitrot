use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui::{
    self, Align, Align2, Color32, ColorImage, CornerRadius, CursorIcon, FontId, Key, KeyboardShortcut, Layout,
    Modifiers, PointerButton, Pos2, Rect, Sense, Shadow, Stroke, TextureHandle, TextureOptions, UiBuilder, Vec2,
    ViewportCommand, pos2, vec2,
};
use image::RgbaImage;
use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;

use crate::effects::{Effect, Transition};
use crate::mask::{Gradient, Mask, Shape};
use crate::render::{self, Job, Layer, Worker};
use crate::theme::{self, Icon, SliderSpec};

/// Preview and masks live at this resolution; export re-renders at full size.
const WORK_MAX_SIDE: u32 = 1600;
const HISTORY_LIMIT: usize = 40;
const TOOLBAR_HEIGHT: f32 = 38.0;
/// Room for the window's traffic-light buttons.
const TRAFFIC_LIGHTS_WIDTH: f32 = 80.0;
const TOAST_DURATION: Duration = Duration::from_secs(3);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tool {
    Brush,
    Eraser,
    Gradient,
    Rect,
    Ellipse,
}

const TOOLS: [(Tool, Icon, &str, Key); 5] = [
    (Tool::Brush, Icon::Brush, "Brush (B)", Key::B),
    (Tool::Eraser, Icon::Eraser, "Eraser (E)", Key::E),
    (Tool::Gradient, Icon::Gradient, "Gradient (G)", Key::G),
    (Tool::Rect, Icon::Rect, "Rectangle (M)", Key::M),
    (Tool::Ellipse, Icon::Ellipse, "Ellipse (L)", Key::L),
];

struct Doc {
    path: PathBuf,
    full: Arc<RgbaImage>,
    work: Arc<RgbaImage>,
    scale: f32,
}

enum LayerAction {
    Select(usize),
    Toggle(usize),
    Delete(usize),
}

pub struct App {
    doc: Option<Doc>,
    layers: Vec<Layer>,
    /// Last state recorded in history; `layers` differs from it while an edit is in progress.
    committed: Vec<Layer>,
    undo: Vec<Vec<Layer>>,
    redo: Vec<Vec<Layer>>,
    selected: usize,

    tool: Tool,
    brush_radius: f32,
    brush_softness: f32,
    shape_feather: f32,
    gradient: Gradient,

    zoom: f32,
    pan: Vec2,
    fit_pending: bool,
    last_paint: Option<(f32, f32)>,
    /// Gradient/shape drag: start point and the mask as it was before the drag.
    drag: Option<((f32, f32), Mask)>,

    worker: Worker,
    next_job: u64,
    shown_job: u64,
    sent: Option<Vec<Layer>>,
    preview: Option<TextureHandle>,
    checker: Option<(bool, TextureHandle)>,

    export_rx: Option<Receiver<Result<PathBuf, String>>>,
    toast: Option<(String, Instant)>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, initial: Option<PathBuf>) -> Self {
        theme::install(&cc.egui_ctx);
        let mut app = Self {
            doc: None,
            layers: Vec::new(),
            committed: Vec::new(),
            undo: Vec::new(),
            redo: Vec::new(),
            selected: 0,
            tool: Tool::Brush,
            brush_radius: 60.0,
            brush_softness: 0.7,
            shape_feather: 60.0,
            gradient: Gradient::Linear,
            zoom: 1.0,
            pan: Vec2::ZERO,
            fit_pending: true,
            last_paint: None,
            drag: None,
            worker: Worker::new(cc.egui_ctx.clone()),
            next_job: 0,
            shown_job: 0,
            sent: None,
            preview: None,
            checker: None,
            export_rx: None,
            toast: None,
        };
        if let Some(path) = initial {
            app.open(&cc.egui_ctx, &path);
        }
        app
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.toast = Some((text.into(), Instant::now()));
    }

    // ---------- document ----------

    fn open(&mut self, ctx: &egui::Context, path: &Path) {
        let full = match image::open(path) {
            Ok(img) => img.to_rgba8(),
            Err(e) => {
                self.notify(format!("Couldn't open the image: {e}"));
                return;
            }
        };
        let (fw, fh) = full.dimensions();
        let k = (WORK_MAX_SIDE as f32 / fw.max(fh) as f32).min(1.0);
        let work = if k < 1.0 {
            let (ww, wh) = (((fw as f32 * k).round() as u32).max(1), ((fh as f32 * k).round() as u32).max(1));
            image::imageops::resize(&full, ww, wh, FilterType::Triangle)
        } else {
            full.clone()
        };
        let scale = work.width() as f32 / fw as f32;
        let name = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        ctx.send_viewport_cmd(ViewportCommand::Title(format!("{name} — Bitrot")));
        self.notify(format!("{name} · {fw} × {fh}"));
        self.doc = Some(Doc { path: path.to_path_buf(), full: Arc::new(full), work: Arc::new(work), scale });
        self.layers.clear();
        self.undo.clear();
        self.redo.clear();
        self.selected = 0;
        self.sent = None;
        self.preview = None;
        self.drag = None;
        self.fit_pending = true;
        self.add_layer(0);
        self.committed = self.layers.clone();
    }

    fn open_dialog(&mut self, ctx: &egui::Context) {
        if let Some(path) = rfd::FileDialog::new()
            .add_filter("Images", &["png", "jpg", "jpeg", "webp", "bmp", "tif", "tiff", "gif"])
            .pick_file()
        {
            self.open(ctx, &path);
        }
    }

    fn export_dialog(&mut self, ctx: &egui::Context) {
        let Some(doc) = &self.doc else { return };
        if self.export_rx.is_some() {
            return;
        }
        let stem = doc.path.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or("image".into());
        let Some(path) = rfd::FileDialog::new()
            .add_filter("PNG", &["png"])
            .add_filter("JPEG", &["jpg", "jpeg"])
            .set_file_name(format!("{stem} bitrot.png"))
            .save_file()
        else {
            return;
        };
        let full = doc.full.clone();
        let layers = self.layers.clone();
        let (tx, rx) = channel();
        let ctx = ctx.clone();
        std::thread::spawn(move || {
            let image = render::render(&full, &layers, 1.0);
            let _ = tx.send(save(&image, &path).map(|_| path));
            ctx.request_repaint();
        });
        self.export_rx = Some(rx);
    }

    // ---------- layers & history ----------

    fn add_layer(&mut self, effect: usize) {
        let Some(doc) = &self.doc else { return };
        let effect = Effect::by_index(effect);
        let n = self.layers.iter().filter(|l| l.effect.name() == effect.name()).count() + 1;
        let seed = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos() as u64).unwrap_or(1);
        self.layers.push(Layer {
            name: if n == 1 { effect.name().to_string() } else { format!("{} {n}", effect.name()) },
            enabled: true,
            transition: Transition::Growth,
            effect,
            mask: Mask::new(doc.work.width() as usize, doc.work.height() as usize),
            seed,
        });
        self.selected = self.layers.len() - 1;
    }

    fn commit(&mut self) {
        if self.layers == self.committed {
            return;
        }
        let prev = std::mem::replace(&mut self.committed, self.layers.clone());
        self.undo.push(prev);
        if self.undo.len() > HISTORY_LIMIT {
            self.undo.remove(0);
        }
        self.redo.clear();
    }

    fn undo(&mut self) {
        self.commit();
        if let Some(prev) = self.undo.pop() {
            self.redo.push(std::mem::replace(&mut self.committed, prev.clone()));
            self.layers = prev;
        }
    }

    fn redo(&mut self) {
        self.commit();
        if let Some(next) = self.redo.pop() {
            self.undo.push(std::mem::replace(&mut self.committed, next.clone()));
            self.layers = next;
        }
    }

    // ---------- per-frame plumbing ----------

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        let cmd = Modifiers::COMMAND;
        let (redo, undo, open, save, fit) = ctx.input_mut(|i| {
            (
                i.consume_shortcut(&KeyboardShortcut::new(cmd | Modifiers::SHIFT, Key::Z)),
                i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Z)),
                i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::O)),
                i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::S)),
                i.consume_shortcut(&KeyboardShortcut::new(cmd, Key::Num0)),
            )
        });
        if redo {
            self.redo();
        } else if undo {
            self.undo();
        }
        if open {
            self.open_dialog(ctx);
        }
        if save {
            self.export_dialog(ctx);
        }
        if fit {
            self.fit_pending = true;
        }

        if !ctx.egui_wants_keyboard_input() {
            ctx.input(|i| {
                for (tool, _, _, key) in TOOLS {
                    if i.key_pressed(key) && i.modifiers.is_none() {
                        self.tool = tool;
                    }
                }
                if i.key_pressed(Key::OpenBracket) {
                    self.brush_radius = (self.brush_radius / 1.2).max(1.0);
                }
                if i.key_pressed(Key::CloseBracket) {
                    self.brush_radius = (self.brush_radius * 1.2).min(600.0);
                }
            });
        }

        let dropped = ctx.input(|i| i.raw.dropped_files.iter().next().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open(ctx, &path);
        }
    }

    fn poll(&mut self, ctx: &egui::Context) {
        while let Ok(done) = self.worker.results.try_recv() {
            if done.id < self.shown_job {
                continue;
            }
            self.shown_job = done.id;
            let size = [done.image.width() as usize, done.image.height() as usize];
            let image = ColorImage::from_rgba_unmultiplied(size, done.image.as_raw());
            let options = TextureOptions { magnification: egui::TextureFilter::Nearest, ..TextureOptions::LINEAR };
            match &mut self.preview {
                Some(tex) => tex.set(image, options),
                None => self.preview = Some(ctx.load_texture("preview", image, options)),
            }
        }
        if let Some(rx) = &self.export_rx {
            if let Ok(result) = rx.try_recv() {
                self.export_rx = None;
                match result {
                    Ok(path) => self.notify(format!(
                        "Saved {}",
                        path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default()
                    )),
                    Err(e) => self.notify(format!("Export failed: {e}")),
                }
            }
        }
    }

    fn request_render(&mut self) {
        let Some(doc) = &self.doc else { return };
        if self.sent.as_ref() == Some(&self.layers) {
            return;
        }
        self.next_job += 1;
        self.worker.submit(Job {
            id: self.next_job,
            source: doc.work.clone(),
            layers: self.layers.clone(),
            scale: doc.scale,
        });
        self.sent = Some(self.layers.clone());
    }

    // ---------- toolbar ----------

    fn toolbar(&mut self, ui: &mut egui::Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();

        // The toolbar doubles as the title bar: drag to move, double-click to zoom.
        let bg = ui.interact(rect, ui.id().with("titlebar"), Sense::click_and_drag());
        if bg.drag_started_by(PointerButton::Primary) {
            ctx.send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let maximized = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(ViewportCommand::Maximized(!maximized));
        }

        let left = Rect::from_min_max(rect.min + vec2(TRAFFIC_LIGHTS_WIDTH, 0.0), rect.max);
        ui.scope_builder(UiBuilder::new().max_rect(left).layout(Layout::left_to_right(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            if theme::text_button(ui, "Open", true, false).on_hover_text("⌘O").clicked() {
                self.open_dialog(&ctx);
            }
            let exporting = self.export_rx.is_some();
            if theme::text_button(ui, "Export", self.doc.is_some() && !exporting, true)
                .on_hover_text("Save PNG or JPEG at full resolution (⌘S)")
                .clicked()
            {
                self.export_dialog(&ctx);
            }
            if exporting {
                ui.spinner();
            }
        });

        let seg = vec2(40.0, 26.0);
        let tools_rect = Rect::from_center_size(rect.center(), vec2(seg.x * TOOLS.len() as f32, seg.y));
        ui.scope_builder(UiBuilder::new().max_rect(tools_rect), |ui| {
            let values = TOOLS.map(|t| t.0);
            let tips = TOOLS.map(|t| t.2);
            theme::segmented(ui, &mut self.tool, &values, seg, &tips, |painter, r, i, color| {
                theme::paint_icon(painter, r, TOOLS[i].1, color);
            });
        });

        let right = rect.shrink2(vec2(12.0, 0.0));
        ui.scope_builder(UiBuilder::new().max_rect(right).layout(Layout::right_to_left(Align::Center)), |ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            if let Some(doc) = &self.doc {
                let pal = theme::pal(ui);
                // Zoom relative to the original image, not the work copy.
                let text = format!("{:.0}%", self.zoom * doc.scale * 100.0);
                ui.add_sized([44.0, 26.0], egui::Label::new(egui::RichText::new(text).color(pal.secondary)));
            }
            if theme::icon_button(ui, Icon::Fit, "Fit to window (⌘0)", self.doc.is_some()).clicked() {
                self.fit_pending = true;
            }
            ui.add_space(10.0);
            if theme::icon_button(ui, Icon::Redo, "Redo (⇧⌘Z)", !self.redo.is_empty()).clicked() {
                self.redo();
            }
            let can_undo = !self.undo.is_empty() || self.layers != self.committed;
            if theme::icon_button(ui, Icon::Undo, "Undo (⌘Z)", can_undo).clicked() {
                self.undo();
            }
        });
    }

    // ---------- inspector ----------

    fn inspector(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.add_space(12.0);
            self.tool_section(ui);
            self.layers_section(ui);
            self.effect_section(ui);
        });
    }

    fn tool_section(&mut self, ui: &mut egui::Ui) {
        let title = match self.tool {
            Tool::Brush => "Brush",
            Tool::Eraser => "Eraser",
            Tool::Gradient => "Gradient",
            Tool::Rect => "Rectangle",
            Tool::Ellipse => "Ellipse",
        };
        let has_layer = self.layers.get(self.selected).is_some();
        let mut op = None;
        theme::section(ui, title, |_| {}, |ui| {
            match self.tool {
                Tool::Brush | Tool::Eraser => {
                    theme::slider(ui, &mut self.brush_radius, SliderSpec {
                        label: "Size",
                        range: 1.0..=600.0,
                        log: true,
                        format: theme::px,
                    });
                    theme::slider(ui, &mut self.brush_softness, SliderSpec {
                        label: "Softness",
                        range: 0.0..=1.0,
                        log: false,
                        format: theme::percent,
                    });
                }
                Tool::Gradient => {
                    theme::segmented_text(ui, &mut self.gradient, &[
                        (Gradient::Linear, "Linear"),
                        (Gradient::Radial, "Radial"),
                    ]);
                    theme::hint(ui, "Drag from where the image should stay intact to where it should be fully destroyed.");
                }
                Tool::Rect | Tool::Ellipse => {
                    theme::slider(ui, &mut self.shape_feather, SliderSpec {
                        label: "Edge softness",
                        range: 0.0..=400.0,
                        log: false,
                        format: theme::px,
                    });
                    theme::hint(ui, "Hold ⌥ while dragging to subtract.");
                }
            }
            ui.add_space(2.0);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                for (label, tip, which) in [
                    ("Fill", "Apply the effect to the whole image", 0),
                    ("Clear", "Remove the effect everywhere", 1),
                    ("Invert", "Swap affected and untouched areas", 2),
                ] {
                    if theme::text_button(ui, label, has_layer, false).on_hover_text(tip).clicked() {
                        op = Some(which);
                    }
                }
            });
        });
        if let (Some(op), Some(layer)) = (op, self.layers.get_mut(self.selected)) {
            match op {
                0 => layer.mask.fill(1.0),
                1 => layer.mask.fill(0.0),
                _ => layer.mask.invert(),
            }
        }
    }

    fn layers_section(&mut self, ui: &mut egui::Ui) {
        let has_doc = self.doc.is_some();
        let mut add = None;
        let mut action = None;
        let layers = &self.layers;
        let selected = self.selected;
        theme::section(
            ui,
            "Layers",
            |ui| {
                let resp = theme::icon_button(ui, Icon::Plus, "Add an effect layer", has_doc);
                egui::Popup::menu(&resp).show(|ui| {
                    ui.set_min_width(140.0);
                    for (i, name) in Effect::ALL_NAMES.iter().enumerate() {
                        if ui.button(*name).clicked() {
                            add = Some(i);
                        }
                    }
                });
            },
            |ui| {
                let pal = theme::pal(ui);
                if layers.is_empty() {
                    theme::hint(ui, if has_doc { "No layers. Add an effect with +." } else { "Open an image to start." });
                    return;
                }
                ui.spacing_mut().item_spacing.y = 2.0;
                for i in (0..layers.len()).rev() {
                    let layer = &layers[i];
                    let (rect, row) = ui.allocate_exact_size(vec2(ui.available_width(), 30.0), Sense::click());
                    if i == selected {
                        ui.painter().rect_filled(rect, 6.0, pal.accent.gamma_multiply(0.18));
                    } else if row.hovered() {
                        ui.painter().rect_filled(rect, 6.0, pal.hover.gamma_multiply(0.5));
                    }
                    if row.clicked() {
                        action = Some(LayerAction::Select(i));
                    }

                    let eye_rect = Rect::from_min_size(rect.min + vec2(2.0, 3.0), vec2(26.0, 24.0));
                    let eye = ui.interact(eye_rect, ui.id().with(("eye", i)), Sense::click());
                    if eye.clicked() {
                        action = Some(LayerAction::Toggle(i));
                    }
                    let icon = if layer.enabled { Icon::Eye } else { Icon::EyeOff };
                    let eye_color = if eye.hovered() { pal.text } else { pal.secondary };
                    theme::paint_icon(ui.painter(), eye_rect, icon, eye_color);

                    let text_color = if layer.enabled { pal.text } else { pal.secondary };
                    ui.painter().text(
                        pos2(rect.left() + 34.0, rect.center().y),
                        Align2::LEFT_CENTER,
                        &layer.name,
                        FontId::proportional(13.0),
                        text_color,
                    );

                    if row.hovered() || i == selected {
                        let close_rect =
                            Rect::from_center_size(pos2(rect.right() - 14.0, rect.center().y), vec2(22.0, 22.0));
                        let close = ui.interact(close_rect, ui.id().with(("close", i)), Sense::click());
                        if close.hovered() {
                            ui.painter().circle_filled(close_rect.center(), 9.0, pal.hover);
                        }
                        theme::paint_icon(ui.painter(), close_rect, Icon::Close, pal.secondary);
                        if close.on_hover_text("Delete layer").clicked() {
                            action = Some(LayerAction::Delete(i));
                        }
                    }
                }
            },
        );

        if let Some(i) = add {
            self.add_layer(i);
        }
        match action {
            Some(LayerAction::Select(i)) => self.selected = i,
            Some(LayerAction::Toggle(i)) => self.layers[i].enabled ^= true,
            Some(LayerAction::Delete(i)) => {
                self.layers.remove(i);
            }
            None => {}
        }
        self.selected = self.selected.min(self.layers.len().saturating_sub(1));
    }

    fn effect_section(&mut self, ui: &mut egui::Ui) {
        let Some(layer) = self.layers.get_mut(self.selected) else { return };
        theme::section(ui, layer.effect.name(), |_| {}, |ui| {
            ui.label("Transition");
            ui.add_space(-4.0);
            theme::segmented_text(ui, &mut layer.transition, &[
                (Transition::Growth, "Organic"),
                (Transition::Feather, "Smooth"),
            ]);
            theme::hint(ui, match layer.transition {
                Transition::Growth => "The effect breaks apart and thins out toward the mask edge.",
                Transition::Feather => "The full effect fades softly into the original.",
            });
            ui.add_space(6.0);

            let slider = |ui: &mut egui::Ui, value: &mut f32, label, range, log, format| {
                theme::slider(ui, value, SliderSpec { label, range, log, format });
            };
            match &mut layer.effect {
                Effect::Disintegrate(p) => {
                    slider(ui, &mut p.size, "Particle size", 1.0..=96.0, true, theme::px);
                    slider(ui, &mut p.amount, "Amount", 0.0..=1.0, false, theme::percent);
                    slider(ui, &mut p.distance, "Distance", 0.0..=1500.0, false, theme::px);
                    slider(ui, &mut p.direction, "Direction", -180.0..=180.0, false, theme::degrees);
                }
                Effect::Pixelate(p) => {
                    slider(ui, &mut p.size, "Block size", 2.0..=256.0, true, theme::px);
                }
                Effect::PixelSort(p) => {
                    theme::segmented_text(ui, &mut p.vertical, &[(false, "Horizontal"), (true, "Vertical")]);
                    ui.add_space(2.0);
                    slider(ui, &mut p.threshold, "Threshold", 0.0..=1.0, false, theme::percent);
                }
                Effect::Glitch(p) => {
                    slider(ui, &mut p.shift, "Shift", 0.0..=1000.0, false, theme::px);
                    slider(ui, &mut p.rgb_split, "RGB split", 0.0..=100.0, false, theme::px);
                    slider(ui, &mut p.blocks, "Broken blocks", 0.0..=1.0, false, theme::percent);
                }
            }
        });
    }

    // ---------- canvas ----------

    fn image_origin(&self, canvas: Rect, size: Vec2) -> Pos2 {
        canvas.center() + self.pan - size * self.zoom * 0.5
    }

    fn checker(&mut self, ctx: &egui::Context, dark: bool) -> egui::TextureId {
        if self.checker.as_ref().is_none_or(|(d, _)| *d != dark) {
            let [a, b] = theme::palette(dark).checker;
            let pixels = [a, a, a, 255, b, b, b, 255, b, b, b, 255, a, a, a, 255];
            let image = ColorImage::from_rgba_unmultiplied([2, 2], &pixels);
            let options = TextureOptions { wrap_mode: egui::TextureWrapMode::Repeat, ..TextureOptions::NEAREST };
            self.checker = Some((dark, ctx.load_texture("checker", image, options)));
        }
        self.checker.as_ref().expect("just created").1.id()
    }

    fn canvas(&mut self, ui: &mut egui::Ui) {
        let pal = theme::pal(ui);
        let (resp, painter) = ui.allocate_painter(ui.available_size(), Sense::click_and_drag());
        let canvas = resp.rect;
        let Some((size, scale)) =
            self.doc.as_ref().map(|d| (vec2(d.work.width() as f32, d.work.height() as f32), d.scale))
        else {
            self.empty_state(ui, canvas);
            return;
        };

        if self.fit_pending {
            self.zoom = (canvas.width() / size.x).min(canvas.height() / size.y) * 0.9;
            self.pan = Vec2::ZERO;
            self.fit_pending = false;
        }

        // Pinch / ⌘+scroll zooms around the pointer; plain scroll and space+drag pan.
        let (zoom_delta, scroll, hover, panning, alt, primary_down) = ui.input(|i| {
            (
                i.zoom_delta(),
                i.smooth_scroll_delta,
                i.pointer.hover_pos(),
                i.key_down(Key::Space),
                i.modifiers.alt,
                i.pointer.primary_down(),
            )
        });
        if resp.hovered() {
            if zoom_delta != 1.0 {
                if let Some(p) = hover {
                    let img_pt = (p - self.image_origin(canvas, size)) / self.zoom;
                    self.zoom = (self.zoom * zoom_delta).clamp(0.02, 40.0);
                    let new_origin = p - img_pt * self.zoom;
                    self.pan = new_origin - canvas.center() + size * self.zoom * 0.5;
                }
            } else {
                self.pan += scroll;
            }
        }
        if (panning && resp.dragged_by(PointerButton::Primary)) || resp.dragged_by(PointerButton::Middle) {
            self.pan += resp.drag_delta();
        }

        let origin = self.image_origin(canvas, size);
        let zoom = self.zoom;
        let to_img = |p: Pos2| ((p.x - origin.x) / zoom, (p.y - origin.y) / zoom);
        let pointer = resp.interact_pointer_pos().or(hover);

        // Mask editing.
        if !panning && self.selected < self.layers.len() {
            let layer = &mut self.layers[self.selected];
            match self.tool {
                Tool::Brush | Tool::Eraser => {
                    if resp.is_pointer_button_down_on() && primary_down {
                        if let Some(p) = pointer {
                            let pt = to_img(p);
                            let erase = self.tool == Tool::Eraser;
                            let (r, h) = (self.brush_radius, 1.0 - self.brush_softness);
                            match self.last_paint {
                                None => layer.mask.dab(pt.0, pt.1, r, h, 1.0, erase),
                                Some(last) if last != pt => layer.mask.stroke(last, pt, r, h, 1.0, erase),
                                Some(_) => {}
                            }
                            self.last_paint = Some(pt);
                        }
                    } else {
                        self.last_paint = None;
                    }
                }
                Tool::Gradient | Tool::Rect | Tool::Ellipse => {
                    if resp.drag_started_by(PointerButton::Primary) {
                        if let Some(p) = resp.interact_pointer_pos() {
                            self.drag = Some((to_img(p), layer.mask.clone()));
                        }
                    }
                    // Re-apply from the pre-drag mask every frame for a live preview.
                    if let (Some((start, base)), Some(p)) = (&self.drag, pointer) {
                        if resp.dragged() || resp.drag_stopped() {
                            let end = to_img(p);
                            layer.mask = base.clone();
                            match self.tool {
                                Tool::Gradient => layer.mask.gradient(self.gradient, *start, end),
                                tool => {
                                    let shape = if tool == Tool::Rect { Shape::Rect } else { Shape::Ellipse };
                                    layer.mask.shape(shape, *start, end, self.shape_feather * scale, alt);
                                }
                            }
                        }
                    }
                    if resp.drag_stopped() {
                        self.drag = None;
                    }
                }
            }
        }

        // Image with a soft shadow on a checkerboard (shows transparency).
        let img_rect = Rect::from_min_size(origin, size * zoom);
        painter.add(
            Shadow { offset: [0, 6], blur: 30, spread: 0, color: Color32::from_black_alpha(70) }
                .as_shape(img_rect, CornerRadius::ZERO),
        );
        let checker = self.checker(ui.ctx(), ui.visuals().dark_mode);
        let tiles = Rect::from_min_max(Pos2::ZERO, pos2(img_rect.width() / 16.0, img_rect.height() / 16.0));
        painter.image(checker, img_rect, tiles, Color32::WHITE);
        if let Some(tex) = &self.preview {
            painter.image(tex.id(), img_rect, Rect::from_min_max(Pos2::ZERO, pos2(1.0, 1.0)), Color32::WHITE);
        }

        // Drag guides.
        if let (Some((start, _)), Some(p), true) = (&self.drag, pointer, resp.dragged()) {
            let a = origin + vec2(start.0, start.1) * zoom;
            let r = Rect::from_two_pos(a, p);
            for stroke in [Stroke::new(3.0, Color32::from_black_alpha(110)), Stroke::new(1.2, Color32::WHITE)] {
                let shape = match self.tool {
                    Tool::Rect => egui::Shape::rect_stroke(r, 0.0, stroke, egui::StrokeKind::Middle),
                    Tool::Ellipse => egui::Shape::ellipse_stroke(r.center(), r.size() * 0.5, stroke),
                    Tool::Gradient if self.gradient == Gradient::Radial => {
                        egui::Shape::circle_stroke(a, a.distance(p), stroke)
                    }
                    _ => egui::Shape::line_segment([a, p], stroke),
                };
                painter.add(shape);
            }
            if self.tool == Tool::Gradient {
                for (pt, fill) in [(a, Color32::WHITE), (p, pal.accent)] {
                    painter.circle(pt, 5.0, fill, Stroke::new(1.5, Color32::from_black_alpha(120)));
                }
            }
        }

        // Cursor.
        if resp.hovered() {
            let icon = if panning {
                if resp.dragged() { CursorIcon::Grabbing } else { CursorIcon::Grab }
            } else if matches!(self.tool, Tool::Brush | Tool::Eraser) {
                CursorIcon::None
            } else {
                CursorIcon::Crosshair
            };
            ui.ctx().set_cursor_icon(icon);
            if let (Some(pt), Tool::Brush | Tool::Eraser, false) = (hover, self.tool, panning) {
                let r = (self.brush_radius * zoom).max(1.5);
                painter.circle_stroke(pt, r, Stroke::new(3.0, Color32::from_black_alpha(110)));
                painter.circle_stroke(pt, r, Stroke::new(1.0, Color32::WHITE));
            }
        }

        // Toast, or a hint when the selected layer has nothing to show yet.
        let bottom = canvas.center_bottom();
        if let Some((text, at)) = &self.toast {
            if at.elapsed() < TOAST_DURATION {
                theme::pill(&painter, pal, bottom, text);
                ui.ctx().request_repaint_after(TOAST_DURATION - at.elapsed());
                return;
            }
        }
        let hint = match self.layers.get(self.selected) {
            None => Some("Add an effect layer with + in the sidebar"),
            Some(l) if l.mask.is_empty() && self.drag.is_none() => Some(match self.tool {
                Tool::Gradient => "Drag across the image to fade the effect in",
                Tool::Rect | Tool::Ellipse => "Drag over the image to apply the effect",
                _ => "Paint over the image to apply the effect",
            }),
            _ => None,
        };
        if let Some(hint) = hint {
            theme::pill(&painter, pal, bottom, hint);
        }
    }

    fn empty_state(&mut self, ui: &mut egui::Ui, canvas: Rect) {
        let pal = theme::pal(ui);
        let ctx = ui.ctx().clone();
        let hovering_file = ctx.input(|i| !i.raw.hovered_files.is_empty());
        let zone = Rect::from_center_size(canvas.center(), vec2(360.0, 220.0));
        let painter = ui.painter();
        let border = if hovering_file { pal.accent } else { pal.separator };
        let outline = [zone.left_top(), zone.right_top(), zone.right_bottom(), zone.left_bottom(), zone.left_top()];
        painter.extend(egui::Shape::dashed_line(&outline, Stroke::new(1.5, border), 8.0, 6.0));
        painter.text(
            zone.center() - vec2(0.0, 36.0),
            Align2::CENTER_CENTER,
            "Drop an image here",
            FontId::proportional(17.0),
            pal.text,
        );
        painter.text(
            zone.center() - vec2(0.0, 12.0),
            Align2::CENTER_CENTER,
            "PNG, JPEG, WebP, TIFF…",
            FontId::proportional(12.0),
            pal.secondary,
        );
        let button = Rect::from_center_size(zone.center() + vec2(0.0, 32.0), vec2(140.0, 28.0));
        ui.scope_builder(UiBuilder::new().max_rect(button).layout(Layout::top_down(Align::Center)), |ui| {
            if theme::text_button(ui, "Choose Image…", true, true).clicked() {
                self.open_dialog(&ctx);
            }
        });
    }
}

fn save(image: &RgbaImage, path: &Path) -> Result<(), String> {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("png").to_ascii_lowercase();
    if ext == "jpg" || ext == "jpeg" {
        // JPEG has no alpha: flatten onto black.
        let rgb: Vec<u8> = image
            .as_raw()
            .chunks_exact(4)
            .flat_map(|c| {
                let a = c[3] as u32;
                [c[0], c[1], c[2]].map(|v| (v as u32 * a / 255) as u8)
            })
            .collect();
        let rgb = image::RgbImage::from_raw(image.width(), image.height(), rgb).ok_or("buffer")?;
        let file = File::create(path).map_err(|e| e.to_string())?;
        JpegEncoder::new_with_quality(BufWriter::new(file), 95).encode_image(&rgb).map_err(|e| e.to_string())
    } else {
        image.save_with_format(path, image::ImageFormat::Png).map_err(|e| e.to_string())
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let pal = theme::pal(ui);
        self.handle_shortcuts(&ctx);
        self.poll(&ctx);

        egui::Panel::top("toolbar")
            .exact_size(TOOLBAR_HEIGHT)
            .frame(egui::Frame::new().fill(pal.toolbar))
            .show(ui, |ui| self.toolbar(ui));
        egui::Panel::right("inspector")
            .exact_size(284.0)
            .frame(egui::Frame::new().fill(pal.sidebar).inner_margin(egui::Margin::symmetric(12, 0)))
            .show(ui, |ui| self.inspector(ui));
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(pal.canvas))
            .show(ui, |ui| self.canvas(ui));

        // History records a state once the pointer is released, so a whole
        // stroke or slider drag is one undo step.
        if !ctx.input(|i| i.pointer.any_down()) {
            self.commit();
        }
        self.request_render();
    }
}

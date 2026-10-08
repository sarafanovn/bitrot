mod app;
mod effects;
mod mask;
mod render;
mod theme;

use eframe::egui;

fn main() -> eframe::Result {
    let initial = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let mut viewport = egui::ViewportBuilder::default()
        .with_title("Bitrot")
        .with_inner_size([1400.0, 900.0])
        .with_min_inner_size([960.0, 600.0])
        .with_drag_and_drop(true)
        // Unified toolbar: content goes under a transparent title bar.
        .with_fullsize_content_view(true)
        .with_titlebar_shown(false)
        .with_title_shown(false);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    eframe::run_native(
        "Bitrot",
        options,
        Box::new(|cc| Ok(Box::new(app::App::new(cc, initial)))),
    )
}

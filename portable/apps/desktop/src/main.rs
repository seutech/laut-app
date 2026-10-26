#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod player;
mod secrets;
mod ui;
mod waveform;
fn app_icon() -> Option<eframe::egui::IconData> {
    let image = image::load_from_memory(include_bytes!(
        "../../../../Resources/Branding/app-icon.png"
    ))
    .ok()?
    .into_rgba8();
    let (width, height) = image.dimensions();
    Some(eframe::egui::IconData {
        rgba: image.into_raw(),
        width,
        height,
    })
}
fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_inner_size([1180., 820.])
        .with_min_inner_size([820., 560.]);
    if let Some(icon) = app_icon() {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "Laut",
        options,
        Box::new(move |cc| Ok(Box::new(ui::LautApp::new(cc, path)))),
    )
}

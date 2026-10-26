#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]
mod player;
mod ui;
mod waveform;
fn main() -> eframe::Result {
    let path = std::env::args_os().nth(1).map(std::path::PathBuf::from);
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_inner_size([1180., 820.])
            .with_min_inner_size([820., 560.]),
        ..Default::default()
    };
    eframe::run_native(
        "Laut",
        options,
        Box::new(move |cc| Ok(Box::new(ui::LautApp::new(cc, path)))),
    )
}

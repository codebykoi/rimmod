mod app;
mod models;
mod services;
mod ui;

use crate::app::App;

const APP_TITLE: &str = concat!("RimMod ", env!("CARGO_PKG_VERSION"));
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/app_icon.png");

fn main() -> eframe::Result {
    let viewport = match eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        Ok(icon) => eframe::egui::ViewportBuilder::default().with_icon(icon),
        Err(_) => eframe::egui::ViewportBuilder::default(),
    };
    let native_options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(|cc| Ok(Box::new(App::new(cc)))),
    )
}

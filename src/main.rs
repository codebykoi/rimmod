mod app;
mod models;
mod services;
mod ui;

use crate::app::App;
use crate::services::updater::{self, StartupMode};

const APP_TITLE: &str = concat!("RimMod ", env!("CARGO_PKG_VERSION"));
const APP_ICON_PNG: &[u8] = include_bytes!("../assets/icons/app_icon.png");

fn main() -> eframe::Result {
    let startup_notice = match updater::parse_startup_args(std::env::args_os()) {
        StartupMode::ApplyUpdate(args) => {
            updater::run_updater_helper(args);
            return Ok(());
        }
        StartupMode::Normal(notice) => notice,
    };

    updater::cleanup_after_update(startup_notice.cleanup_updater.clone());

    let viewport = match eframe::icon_data::from_png_bytes(APP_ICON_PNG) {
        Ok(icon) => eframe::egui::ViewportBuilder::default().with_icon(icon),
        Err(_) => eframe::egui::ViewportBuilder::default(),
    }
    .with_decorations(false)
    .with_min_inner_size([1000.0, 640.0]);
    let surface_config = eframe::SurfaceConfig {
        present_mode: eframe::wgpu::PresentMode::Fifo,
        ..eframe::SurfaceConfig::HIGH_THROUGHPUT
    };
    let native_options = eframe::NativeOptions {
        viewport,
        wgpu_options: eframe::WgpuConfiguration::default().with_surface_config(surface_config),
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        native_options,
        Box::new(move |cc| {
            ui::window_resize::configure_borderless_window(cc);
            Ok(Box::new(App::new(cc, startup_notice)))
        }),
    )
}

use eframe::egui;

use crate::models::{ModSource, ModType};
use crate::services::log_parser::LogLevel;

pub(crate) const INLINE_ICON_SCALE: f32 = 1.075;

pub(crate) fn mod_source_icon(source: &ModSource) -> egui::ImageSource<'static> {
    match source {
        ModSource::SteamWorkshop { .. } => {
            egui::include_image!("../../assets/icons/sources/steam_icon.svg")
        }
        ModSource::Official => egui::include_image!("../../assets/icons/sources/ludeon_icon.svg"),
        ModSource::Local => egui::include_image!("../../assets/icons/sources/local_icon.svg"),
        ModSource::Git { .. } => egui::include_image!("../../assets/icons/sources/git_icon.svg"),
        ModSource::Unknown => egui::include_image!("../../assets/icons/sources/local_icon.svg"),
    }
}

pub(crate) fn mod_type_icon(mod_type: &ModType) -> egui::ImageSource<'static> {
    match mod_type {
        ModType::CSharp => egui::include_image!("../../assets/icons/types/csharp_icon.svg"),
        ModType::Xml => egui::include_image!("../../assets/icons/types/xml_icon.svg"),
        ModType::Unknown => egui::include_image!("../../assets/icons/types/xml_icon.svg"),
    }
}

pub(crate) fn warning_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/statuses/warning.svg")
}

pub(crate) fn incompatible_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/statuses/incompatible.svg")
}

pub(crate) fn log_level_icon(level: LogLevel) -> egui::ImageSource<'static> {
    match level {
        LogLevel::Error => egui::include_image!("../../assets/icons/statuses/error.svg"),
        LogLevel::Warning => warning_icon(),
        LogLevel::Info => egui::include_image!("../../assets/icons/statuses/info.svg"),
        LogLevel::Debug => egui::include_image!("../../assets/icons/statuses/debug.svg"),
    }
}

pub(crate) fn no_version_warning_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/no_version_warning.svg")
}

pub(crate) fn checkbox_checked_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/controls/checkbox_checked.svg")
}

pub(crate) fn checkbox_unchecked_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/controls/checkbox_unchecked.svg")
}

pub(crate) fn download_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/controls/download.svg")
}

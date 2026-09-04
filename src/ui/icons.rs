use eframe::egui;

use crate::models::{ModSource, ModType};

pub(crate) const INLINE_ICON_SCALE: f32 = 1.075;

pub(crate) fn mod_source_icon(source: &ModSource) -> egui::ImageSource<'static> {
    match source {
        ModSource::SteamWorkshop { .. } => {
            egui::include_image!("../../assets/icons/sources/steam_icon.png")
        }
        ModSource::Official => egui::include_image!("../../assets/icons/sources/ludeon_icon.png"),
        ModSource::Local => egui::include_image!("../../assets/icons/sources/local_icon.png"),
        ModSource::Git { .. } => egui::include_image!("../../assets/icons/sources/git_icon.png"),
        ModSource::Unknown => egui::include_image!("../../assets/icons/sources/local_icon.png"),
    }
}

pub(crate) fn mod_type_icon(mod_type: &ModType) -> egui::ImageSource<'static> {
    match mod_type {
        ModType::CSharp => egui::include_image!("../../assets/icons/types/csharp_icon.png"),
        ModType::Xml => egui::include_image!("../../assets/icons/types/xml_icon.png"),
        ModType::Unknown => egui::include_image!("../../assets/icons/types/xml_icon.png"),
    }
}

pub(crate) fn warning_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/statuses/warning.png")
}

pub(crate) fn no_version_warning_icon() -> egui::ImageSource<'static> {
    egui::include_image!("../../assets/icons/no_version_warning.png")
}

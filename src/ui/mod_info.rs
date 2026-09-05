use eframe::egui;

use crate::{
    models::{ModSource, RimworldMod, versions_are_compatible},
    ui::{
        OpenTarget,
        icons::{INLINE_ICON_SCALE, no_version_warning_icon},
    },
};

pub(crate) fn show_mod_info(
    ui: &mut egui::Ui,
    selected_mod: Option<&RimworldMod>,
    game_version: Option<&str>,
) -> Option<OpenTarget> {
    let mut requested_open = None;

    ui.push_id("mod_information", |ui| {
        ui.heading("Mod information");
        ui.separator();

        if let Some(selected_mod) = selected_mod {
            let preview_path = selected_mod.folder.join("About").join("Preview.png");

            if preview_path.is_file() {
                let preview_path_text = preview_path.to_string_lossy().replace('\\', "/");
                let preview_uri = format!("file:///{preview_path_text}");

                ui.vertical_centered(|ui| {
                    ui.add(
                        egui::Image::from_uri(preview_uri)
                            .max_width(ui.available_width())
                            .max_height(300.0),
                    );
                });
            } else {
                ui.label("No preview image");
            }

            ui.label(&selected_mod.name);
            ui.label(selected_mod.package_id.as_str());

            show_supported_versions(
                ui,
                &selected_mod.supported_versions,
                &selected_mod.community_supported_versions,
                game_version,
            );

            if let Some(open_target) = show_source_link(ui, &selected_mod.source) {
                requested_open = Some(open_target);
            }

            ui.horizontal_wrapped(|ui| {
                ui.label("Folder:");

                let folder_text = selected_mod.folder.display().to_string();

                if ui
                    .link(folder_text)
                    .on_hover_text("Open mod folder")
                    .clicked()
                {
                    requested_open = Some(OpenTarget::Folder(selected_mod.folder.clone()));
                }
            });

            ui.separator();

            let description_rect = ui.available_rect_before_wrap();
            let available_height = ui.available_height();

            ui.scope(|ui| {
                // Keep painting inside the information column.
                ui.set_clip_rect(ui.clip_rect().intersect(description_rect));

                egui::ScrollArea::vertical()
                    .id_salt("mod_description")
                    .max_width(description_rect.width())
                    .max_height(available_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.set_max_width(description_rect.width());
                        ui.add(egui::Label::new(&selected_mod.description).wrap());
                    });
            });
        } else {
            ui.label("Select a mod");
        }
    });

    requested_open
}

pub(crate) fn show_supported_versions(
    ui: &mut egui::Ui,
    supported_versions: &[String],
    community_supported_versions: &[String],
    game_version: Option<&str>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Supported versions:");

        if supported_versions.is_empty() && community_supported_versions.is_empty() {
            ui.label("Not specified");
            return;
        }

        for version in supported_versions {
            let is_compatible = game_version
                .is_some_and(|game_version| versions_are_compatible(version, game_version));
            let mut text = egui::RichText::new(version).strong();

            if is_compatible {
                text = text.color(egui::Color32::LIGHT_GREEN);
            }

            let response = ui.label(text);

            if is_compatible {
                response.on_hover_text("Matches the installed RimWorld version");
            }
        }

        for version in community_supported_versions {
            let is_compatible = game_version
                .is_some_and(|game_version| versions_are_compatible(version, game_version));
            let mut text = egui::RichText::new(version).strong();

            if is_compatible {
                text = text.color(egui::Color32::LIGHT_GREEN);
            }

            ui.label(text);

            let icon_height = ui.text_style_height(&egui::TextStyle::Body) * INLINE_ICON_SCALE;
            ui.add(egui::Image::new(no_version_warning_icon()).max_height(icon_height))
                .on_hover_text(
                    "This version is supported by a No Version Warning community report",
                );
        }
    });
}

fn show_source_link(ui: &mut egui::Ui, source: &ModSource) -> Option<OpenTarget> {
    match source {
        ModSource::SteamWorkshop { workshop_id } => {
            let workshop_url =
                format!("https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}");
            let mut requested_open = None;

            ui.horizontal_wrapped(|ui| {
                ui.label("Source:");
                if ui.link("Steam Workshop").clicked() {
                    requested_open = Some(OpenTarget::Url(workshop_url));
                }
            });

            requested_open
        }

        ModSource::Git { remote_url } => {
            let mut requested_open = None;

            ui.horizontal_wrapped(|ui| {
                ui.label("Source:");

                if let Some(web_url) = git_remote_web_url(remote_url) {
                    if ui.link(remote_url).clicked() {
                        requested_open = Some(OpenTarget::Url(web_url));
                    }
                } else {
                    ui.label(remote_url)
                        .on_hover_text("This Git remote is not a browser URL");
                }
            });

            requested_open
        }

        ModSource::Official => {
            ui.label("Source: Official");
            None
        }

        ModSource::Local => {
            ui.label("Source: Local");
            None
        }

        ModSource::Unknown => {
            ui.label("Source: Unknown");
            None
        }
    }
}

/// Convert the two most common Git remote formats into browser URLs:
///
/// https://github.com/user/repository.git
/// git@github.com:user/repository.git
fn git_remote_web_url(remote_url: &str) -> Option<String> {
    let remote_url = remote_url.trim().trim_end_matches(".git");

    if remote_url.starts_with("https://") || remote_url.starts_with("http://") {
        return Some(remote_url.to_owned());
    }

    let ssh_remote = remote_url.strip_prefix("git@")?;
    let (host, repository_path) = ssh_remote.split_once(':')?;

    Some(format!("https://{host}/{repository_path}"))
}

#[cfg(test)]
#[path = "../../tests/unit/mod_info.rs"]
mod tests;

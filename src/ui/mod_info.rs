use eframe::egui;

use crate::{
    models::{ModSource, RimworldMod, versions_are_compatible},
    ui::{OpenTarget, theme},
};

pub(crate) fn show_mod_info(
    ui: &mut egui::Ui,
    selected_mod: Option<&RimworldMod>,
    game_version: Option<&str>,
) -> Option<OpenTarget> {
    let mut requested_open = None;

    ui.push_id("mod_information", |ui| {
        theme::card().show(ui, |ui| {
            ui.set_width(ui.available_width());
            theme::title(ui, "Mod information");
            ui.add_space(8.0);

            if let Some(selected_mod) = selected_mod {
                let preview_path = selected_mod.folder.join("About").join("Preview.png");

                if preview_path.is_file() {
                    let preview_path_text = preview_path.to_string_lossy().replace('\\', "/");
                    let preview_uri = format!("file:///{preview_path_text}");

                    ui.vertical_centered(|ui| {
                        ui.add(
                            egui::Image::from_uri(preview_uri)
                                .max_width(ui.available_width())
                                .max_height(300.0)
                                .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD)),
                        );
                    });
                } else {
                    let (rect, _) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 140.0),
                        egui::Sense::hover(),
                    );

                    ui.painter().rect_filled(
                        rect,
                        egui::CornerRadius::same(theme::RADIUS_MD),
                        theme::CANVAS,
                    );

                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "No preview image",
                        theme::medium_font(12.0),
                        theme::TEXT_FAINT,
                    );
                }

                ui.add_space(8.0);

                ui.label(egui::RichText::new(&selected_mod.name).text_style(theme::strong_style()));

                ui.label(
                    egui::RichText::new(selected_mod.package_id.as_str())
                        .monospace()
                        .small()
                        .color(theme::TEXT_MUTED),
                );

                ui.add_space(6.0);

                show_supported_versions(
                    ui,
                    &selected_mod.supported_versions,
                    &selected_mod.community_supported_versions,
                    game_version,
                );

                if let Some(open_target) = show_source_link(ui, &selected_mod.source) {
                    requested_open = Some(open_target);
                }

                ui.horizontal(|ui| {
                    theme::hint(ui, "Folder:");

                    let folder_text = selected_mod.folder.display().to_string();

                    if theme::link(ui, folder_text).clicked() {
                        requested_open = Some(OpenTarget::Folder(selected_mod.folder.clone()));
                    }
                });

                ui.add_space(8.0);

                ui.add_space(8.0);

                egui::Frame::new()
                    .fill(theme::CANVAS)
                    .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD))
                    .inner_margin(egui::Margin::same(10))
                    .show(ui, |ui| {
                        // Sizes must come from inside the frame: measuring the
                        // outer column here would make the frame (and the card)
                        // wider than the column it lives in.
                        let max_height = ui.available_height();

                        egui::ScrollArea::vertical()
                            .id_salt("mod_description")
                            .max_height(max_height)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        egui::RichText::new(&selected_mod.description)
                                            .color(theme::TEXT_MUTED),
                                    )
                                    .wrap(),
                                );
                            });
                    });
            } else {
                theme::hint(ui, "Select a mod to see its details");
            }
        });
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
        theme::hint(ui, "Supported versions");

        if supported_versions.is_empty() && community_supported_versions.is_empty() {
            theme::hint(ui, "Not specified");
            return;
        }

        for version in supported_versions {
            let is_compatible = game_version
                .is_some_and(|game_version| versions_are_compatible(version, game_version));
            let color = if is_compatible {
                theme::SUCCESS
            } else {
                theme::TEXT_MUTED
            };

            let response = theme::chip(ui, version.clone(), color);

            if is_compatible {
                response.on_hover_text("Matches the installed RimWorld version");
            }
        }

        for version in community_supported_versions {
            let is_compatible = game_version
                .is_some_and(|game_version| versions_are_compatible(version, game_version));
            let color = if is_compatible {
                theme::WARNING
            } else {
                theme::TEXT_MUTED
            };

            theme::chip(ui, version.clone(), color).on_hover_text(
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
                theme::hint(ui, "Source:");
                if ui.link("Steam Workshop").clicked() {
                    requested_open = Some(OpenTarget::Url(workshop_url));
                }
            });

            requested_open
        }

        ModSource::Git { remote_url } => {
            let mut requested_open = None;

            ui.horizontal_wrapped(|ui| {
                theme::hint(ui, "Source:");

                if let Some(web_url) = git_remote_web_url(remote_url) {
                    if theme::link(ui, remote_url).clicked() {
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
            theme::hint(ui, "Source: Official");
            None
        }

        ModSource::Local => {
            theme::hint(ui, "Source: Local");
            None
        }

        ModSource::Unknown => {
            theme::hint(ui, "Source: Unknown");
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

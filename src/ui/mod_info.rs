use eframe::egui;

use crate::{
    models::{
        InferredDependency, InferredDependencyKind, ModCollection, ModId, ModSource, PackageId,
        RimworldMod, versions_are_compatible,
    },
    services::organization::Organization,
    ui::{
        OpenTarget,
        icons::{INLINE_ICON_SCALE, no_version_warning_icon},
        theme,
    },
};

/// How many source files a tooltip lists before summarizing the rest.
const MAX_TOOLTIP_FILES: usize = 4;

pub(crate) enum ModInfoAction {
    Open(OpenTarget),
    Organize { mod_ids: Vec<ModId> },
}

pub(crate) fn show_mod_info(
    ui: &mut egui::Ui,
    selected_mod: Option<&RimworldMod>,
    mods: &ModCollection,
    organization: &Organization,
    organize_mod_ids: &[ModId],
    game_version: Option<&str>,
) -> Option<ModInfoAction> {
    let mut action = None;

    ui.push_id("mod_information", |ui| {
        theme::gradient_surface(
            ui,
            theme::SURFACE_HOVER,
            theme::SURFACE,
            egui::Margin::same(12),
            |ui| {
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

                    ui.label(
                        egui::RichText::new(&selected_mod.name).text_style(theme::strong_style()),
                    );

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
                        action = Some(ModInfoAction::Open(open_target));
                    }

                    ui.horizontal(|ui| {
                        theme::hint(ui, "Folder:");

                        let folder_text = selected_mod.folder.display().to_string();

                        if theme::link(ui, folder_text).clicked() {
                            action = Some(ModInfoAction::Open(OpenTarget::Folder(
                                selected_mod.folder.clone(),
                            )));
                        }
                    });

                    if show_organization(
                        ui,
                        &selected_mod.package_id,
                        organization,
                        organize_mod_ids,
                    ) {
                        action = Some(ModInfoAction::Organize {
                            mod_ids: organize_mod_ids.to_vec(),
                        });
                    }

                    show_dependency_hints(ui, selected_mod, mods);

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
                                    // `columns_const` stretches each column and makes
                                    // egui full-justify every label. Justification spreads
                                    // the last row out to the wrap edge, where snapped
                                    // glyphs poke out of the card, and prose reads worse.
                                    // Lay the description out left-aligned instead.
                                    ui.with_layout(
                                        egui::Layout::top_down(egui::Align::LEFT),
                                        |ui| {
                                            ui.add(
                                                egui::Label::new(
                                                    egui::RichText::new(&selected_mod.description)
                                                        .color(theme::TEXT_MUTED),
                                                )
                                                .wrap(),
                                            );
                                        },
                                    );
                                });
                        });
                } else {
                    theme::hint(ui, "Select a mod to see its details");
                }
            },
        );
    });

    action
}

/// Tags and color of the selected mod, with one entry point to edit them for
/// the whole current selection. Returns true when organizing was requested.
fn show_organization(
    ui: &mut egui::Ui,
    package_id: &PackageId,
    organization: &Organization,
    organize_mod_ids: &[ModId],
) -> bool {
    let tags = organization.tags(package_id);
    let color = organization.color(package_id);
    let mut requested = false;

    ui.horizontal_wrapped(|ui| {
        theme::hint(ui, "Tags");

        if tags.is_empty() {
            theme::hint(ui, "None");
        } else {
            for tag in tags {
                theme::chip(ui, tag.clone(), theme::TEXT_MUTED)
                    .on_hover_text("Right-click the mod to change its tags");
            }
        }

        if theme::link(ui, "Edit tags & color…").clicked() && !organize_mod_ids.is_empty() {
            requested = true;
        }
    });

    if let Some(color) = color {
        ui.horizontal_wrapped(|ui| {
            theme::hint(ui, "Color");
            theme::chip(ui, color.label(), theme::mod_color(color));
        });
    }

    requested
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

        show_version_chips(
            ui,
            supported_versions,
            community_supported_versions,
            game_version,
        );
    });
}

pub(crate) fn show_version_chips(
    ui: &mut egui::Ui,
    supported_versions: &[String],
    community_supported_versions: &[String],
    game_version: Option<&str>,
) {
    for version in supported_versions {
        let is_compatible =
            game_version.is_some_and(|game_version| versions_are_compatible(version, game_version));
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

    let icon_size =
        egui::Vec2::splat(ui.text_style_height(&egui::TextStyle::Body) * INLINE_ICON_SCALE);

    for version in community_supported_versions {
        if supported_versions
            .iter()
            .any(|official| versions_are_compatible(official, version))
        {
            continue;
        }

        let is_compatible =
            game_version.is_some_and(|game_version| versions_are_compatible(version, game_version));
        let color = if is_compatible {
            theme::WARNING
        } else {
            theme::TEXT_MUTED
        };
        let hover = "This version is supported by a No Version Warning community report";

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            ui.add(
                egui::Image::new(no_version_warning_icon())
                    .fit_to_exact_size(icon_size)
                    .sense(egui::Sense::hover()),
            )
            .on_hover_text(hover);
            theme::chip(ui, version.clone(), color).on_hover_text(hover);
        });
    }
}

fn show_dependency_hints(ui: &mut egui::Ui, rimworld_mod: &RimworldMod, mods: &ModCollection) {
    if rimworld_mod.mod_dependencies.is_empty()
        && rimworld_mod.inferred_after.is_empty()
        && rimworld_mod.incompatible_with.is_empty()
    {
        return;
    }

    let inferred_groups = rimworld_mod
        .inferred_after
        .chunk_by(|left, right| left.package_id == right.package_id)
        .count();

    let mut header = String::from("Dependencies");

    if !rimworld_mod.mod_dependencies.is_empty() {
        header.push_str(&format!(
            " · {} required",
            rimworld_mod.mod_dependencies.len()
        ));
    }

    if inferred_groups > 0 {
        header.push_str(&format!(" · {inferred_groups} inferred"));
    }

    if !rimworld_mod.incompatible_with.is_empty() {
        header.push_str(&format!(
            " · {} incompatible",
            rimworld_mod.incompatible_with.len()
        ));
    }

    ui.add_space(6.0);

    // Long dependency lists must not push the description out of reach, so
    // they start collapsed.
    theme::collapsible(ui, "mod_dependencies", header, false, |ui| {
        // Even when expanded, the list must not push the description away.
        let max_height = ui.available_height().clamp(80.0, 180.0);

        egui::ScrollArea::vertical()
            .id_salt("mod_dependencies_scroll")
            .max_height(max_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                if !rimworld_mod.mod_dependencies.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        theme::hint(ui, "Requires:");

                        for package_id in &rimworld_mod.mod_dependencies {
                            theme::chip(ui, mod_display_name(mods, package_id), theme::TEXT_MUTED)
                                .on_hover_text(format!(
                                    "{}\nDeclared in About.xml as a mod dependency",
                                    package_id.as_str()
                                ));
                        }
                    });
                }

                if !rimworld_mod.incompatible_with.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        theme::hint(ui, "Incompatible with:");

                        for package_id in &rimworld_mod.incompatible_with {
                            theme::chip(ui, mod_display_name(mods, package_id), theme::DANGER)
                                .on_hover_text(format!(
                                    "{}\nDeclared in About.xml as incompatible",
                                    package_id.as_str()
                                ));
                        }
                    });
                }

                if !rimworld_mod.inferred_after.is_empty() {
                    ui.horizontal_wrapped(|ui| {
                        theme::hint(ui, "Likely loads after:");

                        // The same package id is usually found in many files, so one
                        // chip per dependency keeps this list readable. Sources are in
                        // the tooltip.
                        for dependencies in rimworld_mod
                            .inferred_after
                            .chunk_by(|left, right| left.package_id == right.package_id)
                        {
                            let package_id = &dependencies[0].package_id;

                            theme::chip(ui, mod_display_name(mods, package_id), theme::TEXT_FAINT)
                                .on_hover_text(inferred_dependency_tooltip(dependencies));
                        }
                    });
                }
            });
    });
}

/// Show the installed mod's name when the dependency is present, so long
/// package ids do not dominate the panel. Unknown apps keep their raw id.
fn mod_display_name(mods: &ModCollection, package_id: &PackageId) -> String {
    mods.find_id_by_package_id(package_id)
        .and_then(|mod_id| mods.get(mod_id))
        .map_or_else(
            || package_id.as_str().to_owned(),
            |rimworld_mod| rimworld_mod.name.clone(),
        )
}

fn inferred_dependency_tooltip(dependencies: &[InferredDependency]) -> String {
    let mut tooltip = format!(
        "Inferred after {}\nNot declared in About.xml; found in the files below.",
        dependencies[0].package_id.as_str()
    );

    for kind in [
        InferredDependencyKind::AssemblyReference,
        InferredDependencyKind::LoadFolderCondition,
        InferredDependencyKind::MayRequire,
        InferredDependencyKind::FindMod,
    ] {
        let files = dependencies
            .iter()
            .filter(|dependency| dependency.kind == kind)
            .map(|dependency| dependency.source_file.display().to_string())
            .collect::<Vec<_>>();

        if files.is_empty() {
            continue;
        }

        tooltip.push_str(&format!("\n\n{} ({}):", kind.label(), files.len()));

        for file in files.iter().take(MAX_TOOLTIP_FILES) {
            tooltip.push_str(&format!("\n{file}"));
        }

        if files.len() > MAX_TOOLTIP_FILES {
            tooltip.push_str(&format!("\n+{} more", files.len() - MAX_TOOLTIP_FILES));
        }
    }

    tooltip
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

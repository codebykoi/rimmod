use std::collections::HashSet;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, CornerRadius, Margin, RichText, Sense, Stroke, StrokeKind, Vec2};

use crate::services::modpack::{self, PackMod, PackSource, PackSummary};
use crate::ui::theme;

const LIST_WIDTH: f32 = 318.0;
const ROW_HEIGHT: f32 = 64.0;
const ROW_ICON: f32 = 44.0;
const DETAIL_ICON: f32 = 96.0;
const EDITOR_ICON: f32 = 96.0;

pub(crate) struct ModpacksView<'a> {
    pub(crate) packs: &'a [PackSummary],
    pub(crate) selected: Option<&'a Path>,
    pub(crate) library_error: Option<&'a str>,
}

pub(crate) enum ModpacksAction {
    NewFromActive,
    Import,
    Refresh,
    Select(PathBuf),
    Apply(PathBuf),
    Update(PathBuf),
    Export(PathBuf),
    Delete(PathBuf),
    SaveEditor,
    CancelEditor,
    ChooseIcon,
    RemoveIcon,
}

pub(crate) struct PackEditorView<'a> {
    pub(crate) name: &'a mut String,
    pub(crate) description: &'a mut String,
    pub(crate) tags_input: &'a mut String,
    pub(crate) icon: Option<&'a Arc<[u8]>>,
    pub(crate) icon_generation: u64,
    pub(crate) mods: &'a [PackMod],
    pub(crate) excluded_mods: &'a mut HashSet<String>,
    pub(crate) config_files: &'a [(String, PathBuf)],
    pub(crate) excluded_configs: &'a mut HashSet<String>,
    pub(crate) saves: &'a [(String, PathBuf)],
    pub(crate) selected_saves: &'a mut HashSet<String>,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) updating: bool,
}

pub(crate) struct PackEditorOutcome {
    pub(crate) action: Option<ModpacksAction>,
    pub(crate) open: bool,
}

pub(crate) fn show_modpacks(ui: &mut egui::Ui, view: ModpacksView<'_>) -> Option<ModpacksAction> {
    let ModpacksView {
        packs,
        selected,
        library_error,
    } = view;
    let mut action = None;

    ui.add_space(2.0);

    egui::containers::Sides::new()
        .height(theme::CONTROL_HEIGHT)
        .show(
            ui,
            |ui| {
                theme::title(ui, "Modpacks");
                ui.add_space(8.0);
                theme::hint(
                    ui,
                    "Save a whole mod list, settings, saves and local mods as one file",
                );
            },
            |ui| {
                if theme::primary_button_sized(
                    ui,
                    "New from active mods",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    action = Some(ModpacksAction::NewFromActive);
                }

                if theme::secondary_button_sized(
                    ui,
                    "Import…",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    action = Some(ModpacksAction::Import);
                }

                if theme::secondary_button_sized(
                    ui,
                    "Refresh",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    action = Some(ModpacksAction::Refresh);
                }
            },
        );

    ui.add_space(6.0);

    if let Some(error) = library_error {
        egui::Frame::new()
            .fill(theme::tint(theme::DANGER, 26))
            .corner_radius(CornerRadius::same(theme::RADIUS_MD))
            .inner_margin(Margin::symmetric(12, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(RichText::new(error).strong().color(theme::DANGER));
            });
        ui.add_space(6.0);
    }

    if packs.is_empty() {
        show_empty_state(ui);
        return action;
    }

    let selected_pack =
        selected.and_then(|selected| packs.iter().find(|pack| same_path(&pack.path, selected)));

    let body_height = ui.available_height().max(220.0);

    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            Vec2::new(LIST_WIDTH.min(ui.available_width() * 0.38), body_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("modpack_library")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for pack in packs {
                            let is_selected =
                                selected.is_some_and(|selected| same_path(&pack.path, selected));

                            if show_pack_row(ui, pack, is_selected) {
                                action = Some(ModpacksAction::Select(pack.path.clone()));
                            }
                        }
                    });
            },
        );

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        ui.allocate_ui_with_layout(
            Vec2::new(ui.available_width(), body_height),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("modpack_details")
                    .auto_shrink([false, false])
                    .show(ui, |ui| match selected_pack {
                        Some(pack) => show_pack_details(ui, pack, &mut action),
                        None => {
                            theme::hint(ui, "Select a modpack to see what it contains");
                        }
                    });
            },
        );
    });

    action
}

fn show_empty_state(ui: &mut egui::Ui) {
    let height = ui.available_height().max(240.0);

    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), height),
        egui::Layout::centered_and_justified(egui::Direction::TopDown),
        |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space((height * 0.18).min(90.0));
                ui.label(
                    RichText::new("No modpacks yet")
                        .text_style(theme::title_style())
                        .color(theme::TEXT_MUTED),
                );
                ui.add_space(4.0);
                theme::hint(
                    ui,
                    "Create one from the active mod list, or import a .rimpack file.",
                );
            });
        },
    );
}

fn show_pack_row(ui: &mut egui::Ui, pack: &PackSummary, selected: bool) -> bool {
    let width = ui.available_width();
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, ROW_HEIGHT), Sense::click());
    let hovered = response.hovered();

    if ui.is_rect_visible(rect) {
        let fill = if selected {
            theme::tint(theme::ACCENT, 40)
        } else if hovered {
            theme::SURFACE_HOVER
        } else {
            theme::SURFACE
        };

        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), fill);

        if selected {
            let rail = egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.left() + 2.0, rect.bottom()),
            );
            ui.painter()
                .rect_filled(rail, CornerRadius::ZERO, theme::ACCENT);
        }

        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 8.0, rect.center().y - ROW_ICON / 2.0),
            Vec2::splat(ROW_ICON),
        );
        show_icon(
            ui,
            icon_rect,
            pack.icon.as_ref(),
            &pack.manifest.name,
            pack_icon_key(pack),
        );

        let text_left = icon_rect.right() + 10.0;
        let text_width = (rect.right() - text_left - 8.0).max(20.0);

        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 9.0),
            &truncate(&pack.manifest.name, 42),
            theme::medium_font(13.0),
            theme::TEXT,
            text_width,
        );

        let (workshop, local, official) = source_counts(&pack.manifest);

        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 29.0),
            &summary_text(workshop, local, official, pack.manifest.saves.len()),
            theme::medium_font(11.0),
            theme::TEXT_MUTED,
            text_width,
        );

        let tags = if pack.manifest.tags.is_empty() {
            if pack.manifest.description.trim().is_empty() {
                "No description".to_owned()
            } else {
                truncate(pack.manifest.description.trim(), 46)
            }
        } else {
            truncate(&pack.manifest.tags.join(" · "), 46)
        };

        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 45.0),
            &tags,
            theme::medium_font(10.0),
            theme::TEXT_FAINT,
            text_width,
        );
    }

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .clicked()
}

fn show_pack_details(ui: &mut egui::Ui, pack: &PackSummary, action: &mut Option<ModpacksAction>) {
    ui.set_width(ui.available_width());

    ui.horizontal_top(|ui| {
        let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(DETAIL_ICON), Sense::hover());
        show_icon(
            ui,
            icon_rect,
            pack.icon.as_ref(),
            &pack.manifest.name,
            pack_icon_key(pack),
        );

        ui.add_space(12.0);

        ui.vertical(|ui| {
            ui.label(
                RichText::new(&pack.manifest.name)
                    .text_style(theme::title_style())
                    .color(theme::TEXT),
            );

            let (workshop, local, official) = source_counts(&pack.manifest);
            theme::hint(
                ui,
                summary_text(workshop, local, official, pack.manifest.saves.len()),
            );

            if !pack.manifest.tags.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    for tag in &pack.manifest.tags {
                        theme::chip(ui, tag.clone(), theme::ACCENT);
                    }
                });
            }

            if let Some(game_version) = &pack.manifest.game_version {
                theme::hint(ui, format!("Saved from RimWorld {game_version}"));
            }

            if !pack.manifest.created_with.is_empty() {
                theme::hint(ui, format!("Created with {}", pack.manifest.created_with));
            }
        });
    });

    ui.add_space(12.0);

    if !pack.manifest.description.trim().is_empty() {
        egui::Frame::new()
            .fill(theme::CANVAS)
            .corner_radius(CornerRadius::same(theme::RADIUS_MD))
            .inner_margin(Margin::same(10))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(pack.manifest.description.trim()).color(theme::TEXT_MUTED),
                    )
                    .wrap(),
                );
            });

        ui.add_space(10.0);
    }

    ui.horizontal(|ui| {
        if theme::primary_button(ui, "Apply modpack").clicked() {
            *action = Some(ModpacksAction::Apply(pack.path.clone()));
        }

        if theme::secondary_button(ui, "Edit…").clicked() {
            *action = Some(ModpacksAction::Update(pack.path.clone()));
        }

        if theme::secondary_button(ui, "Export…").clicked() {
            *action = Some(ModpacksAction::Export(pack.path.clone()));
        }

        if theme::danger_button(ui, "Delete").clicked() {
            *action = Some(ModpacksAction::Delete(pack.path.clone()));
        }
    });

    ui.add_space(10.0);

    if !pack.manifest.configs.is_empty() {
        theme::hint(
            ui,
            format!(
                "{} mod settings file{} will be restored when applied",
                pack.manifest.configs.len(),
                plural(pack.manifest.configs.len())
            ),
        );
        ui.add_space(6.0);
    }

    if !pack.manifest.saves.is_empty() {
        theme::hint(
            ui,
            format!(
                "{} save{} included; existing saves are kept unless overwrite is enabled",
                pack.manifest.saves.len(),
                plural(pack.manifest.saves.len())
            ),
        );
        ui.add_space(6.0);
    }

    theme::hint(
        ui,
        "Use Edit… to capture the current mod list, settings and saves.",
    );
    ui.add_space(6.0);

    theme::collapsible(
        ui,
        "modpack_mods",
        format!("Included mods ({})", pack.manifest.mods.len()),
        true,
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt("modpack_mods_scroll")
                .max_height(260.0)
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    for pack_mod in &pack.manifest.mods {
                        show_pack_mod_row(ui, pack_mod);
                    }
                });
        },
    );

    if !pack.manifest.configs.is_empty() {
        theme::collapsible(
            ui,
            "modpack_configs",
            format!("Settings files ({})", pack.manifest.configs.len()),
            false,
            |ui| {
                for config in &pack.manifest.configs {
                    theme::hint(ui, format!("- {config}"));
                }
            },
        );
    }

    if !pack.manifest.saves.is_empty() {
        theme::collapsible(
            ui,
            "modpack_saves",
            format!("Saves ({})", pack.manifest.saves.len()),
            false,
            |ui| {
                for save in &pack.manifest.saves {
                    theme::hint(ui, format!("- {save}"));
                }
            },
        );
    }
}

fn show_pack_mod_row(ui: &mut egui::Ui, pack_mod: &PackMod) {
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(&pack_mod.name)
                .text_style(theme::strong_style())
                .color(theme::TEXT),
        );

        ui.label(
            RichText::new(pack_mod.package_id.as_str())
                .monospace()
                .small()
                .color(theme::TEXT_FAINT),
        );

        theme::chip(
            ui,
            source_label(&pack_mod.source),
            source_color(&pack_mod.source),
        )
        .on_hover_text(source_tooltip(pack_mod));
    });
}

pub(crate) fn show_pack_editor(
    context: &egui::Context,
    view: PackEditorView<'_>,
) -> PackEditorOutcome {
    let PackEditorView {
        name,
        description,
        tags_input,
        icon,
        icon_generation,
        mods,
        excluded_mods,
        config_files,
        excluded_configs,
        saves,
        selected_saves,
        game_version,
        updating,
    } = view;

    let mut action = None;
    let mut done = false;

    let response = egui::Modal::new(egui::Id::new("modpack_editor")).show(context, |ui| {
        ui.set_width(600.0);

        ui.heading(if updating {
            "Edit modpack"
        } else {
            "New modpack"
        });
        ui.add_space(4.0);
        theme::hint(
            ui,
            if updating {
                "Saving replaces this instance with the current mod list, settings and selected saves."
            } else {
                "Only active mods are listed. Workshop mods are saved as links; local mods, settings and the saves you pick are copied into the pack."
            },
        );

        ui.add_space(10.0);

        let scroll_height = (context.content_rect().height() - 260.0).clamp(260.0, 560.0);

        egui::ScrollArea::vertical()
            .id_salt("modpack_editor_scroll")
            .max_height(scroll_height)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal_top(|ui| {
                    let (icon_rect, _) =
                        ui.allocate_exact_size(Vec2::splat(EDITOR_ICON), Sense::hover());

                    if let Some(icon) = icon {
                        let uri = icon_uri("modpack-editor", icon_generation);
                        ui.put(
                            icon_rect,
                            egui::Image::from_bytes(uri, icon.clone())
                                .fit_to_exact_size(Vec2::splat(EDITOR_ICON))
                                .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
                        );
                    } else {
                        ui.painter().rect_filled(
                            icon_rect,
                            CornerRadius::same(theme::RADIUS_MD),
                            theme::CANVAS,
                        );
                        ui.painter().text(
                            icon_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "No icon",
                            theme::medium_font(11.0),
                            theme::TEXT_FAINT,
                        );
                    }

                    ui.add_space(12.0);

                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            if theme::secondary_button(ui, "Choose icon…").clicked() {
                                action = Some(ModpacksAction::ChooseIcon);
                            }

                            if icon.is_some()
                                && theme::secondary_button(ui, "Remove icon").clicked()
                            {
                                action = Some(ModpacksAction::RemoveIcon);
                            }
                        });

                        ui.add_space(6.0);
                        theme::hint(
                            ui,
                            "PNG, JPEG or GIF. The icon is resized to fit 512×512.",
                        );
                    });
                });

                ui.add_space(14.0);
                section_label(ui, "NAME");
                ui.add(
                    egui::TextEdit::singleline(name)
                        .hint_text("Modpack name")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(10.0);
                section_label(ui, "TAGS");
                ui.add(
                    egui::TextEdit::singleline(tags_input)
                        .hint_text("Comma separated, e.g. Vanilla+, Performance, QoL")
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(10.0);
                section_label(ui, "DESCRIPTION");
                ui.add(
                    egui::TextEdit::multiline(description)
                        .hint_text("What is this pack for?")
                        .desired_rows(3)
                        .desired_width(f32::INFINITY),
                );

                ui.add_space(12.0);

                let included_mod_count = mods
                    .iter()
                    .filter(|pack_mod| !excluded_mods.contains(&pack_mod.package_id))
                    .count();
                let local = mods
                    .iter()
                    .filter(|pack_mod| {
                        !excluded_mods.contains(&pack_mod.package_id) && pack_mod.is_local()
                    })
                    .count();
                let workshop = mods
                    .iter()
                    .filter(|pack_mod| {
                        !excluded_mods.contains(&pack_mod.package_id)
                            && pack_mod.workshop_url().is_some()
                    })
                    .count();
                let official = included_mod_count.saturating_sub(local + workshop);
                let included_config_count = config_files
                    .iter()
                    .filter(|(relative_path, _)| !excluded_configs.contains(relative_path))
                    .count();
                let selected_save_count = saves
                    .iter()
                    .filter(|(file_name, _)| selected_saves.contains(file_name))
                    .count();
                let selected_save_bytes = saves
                    .iter()
                    .filter(|(file_name, _)| selected_saves.contains(file_name))
                    .filter_map(|(_, path)| std::fs::metadata(path).ok())
                    .map(|metadata| metadata.len())
                    .sum::<u64>();

                let mut summary = format!(
                    "{included_mod_count} mod{} · {local} local · {workshop} Workshop · {included_config_count} setting file{} · {selected_save_count} save{}",
                    plural(included_mod_count),
                    plural(included_config_count),
                    plural(selected_save_count)
                );

                if selected_save_bytes > 0 {
                    summary.push_str(&format!(" ({})", format_size(selected_save_bytes)));
                }

                ui.horizontal_wrapped(|ui| {
                    theme::chip(ui, summary, theme::TEXT_MUTED);
                    if official > 0 {
                        theme::chip(ui, format!("{official} official"), theme::TEXT_FAINT)
                            .on_hover_text("Official content is referenced, not copied into the pack");
                    }
                    if let Some(game_version) = game_version {
                        theme::chip(ui, format!("RimWorld {game_version}"), theme::TEXT_FAINT);
                    }
                });

                ui.add_space(10.0);

                let included_mods_title =
                    format!("Included mods ({included_mod_count}/{})", mods.len());

                theme::collapsible(ui, "modpack_editor_mods", included_mods_title, true, |ui| {
                    if let Some(selection) = selection_buttons(
                        ui,
                        included_mod_count == mods.len(),
                        included_mod_count == 0,
                    ) {
                        match selection {
                            SelectionAction::All => excluded_mods.clear(),
                            SelectionAction::None => {
                                for pack_mod in mods {
                                    excluded_mods.insert(pack_mod.package_id.clone());
                                }
                            }
                        }
                    }

                    for pack_mod in mods {
                        show_editor_mod_row(ui, pack_mod, excluded_mods);
                    }
                });

                if !config_files.is_empty() {
                    let settings_title = format!(
                        "Settings files ({included_config_count}/{})",
                        config_files.len()
                    );

                    theme::collapsible(ui, "modpack_editor_configs", settings_title, false, |ui| {
                        if let Some(selection) = selection_buttons(
                            ui,
                            included_config_count == config_files.len(),
                            included_config_count == 0,
                        ) {
                            match selection {
                                SelectionAction::All => excluded_configs.clear(),
                                SelectionAction::None => {
                                    for (relative_path, _) in config_files {
                                        excluded_configs.insert(relative_path.clone());
                                    }
                                }
                            }
                        }

                        for (relative_path, _) in config_files {
                            let mut included = !excluded_configs.contains(relative_path);

                            if theme::checkbox(ui, &mut included, relative_path.as_str())
                                .changed()
                            {
                                if included {
                                    excluded_configs.remove(relative_path);
                                } else {
                                    excluded_configs.insert(relative_path.clone());
                                }
                            }
                        }
                    });
                }

                let saves_title = format!("Saves ({selected_save_count}/{})", saves.len());

                theme::collapsible(ui, "modpack_editor_saves", saves_title, true, |ui| {
                    if saves.is_empty() {
                        theme::hint(
                            ui,
                            "No .rws files were found in RimWorld's Saves folder.",
                        );
                    } else {
                        if let Some(selection) = selection_buttons(
                            ui,
                            selected_save_count == saves.len(),
                            selected_save_count == 0,
                        ) {
                            match selection {
                                SelectionAction::All => {
                                    for (file_name, _) in saves {
                                        selected_saves.insert(file_name.clone());
                                    }
                                }
                                SelectionAction::None => selected_saves.clear(),
                            }
                        }

                        for (file_name, path) in saves {
                            let mut included = selected_saves.contains(file_name);
                            let size = std::fs::metadata(path)
                                .ok()
                                .map(|metadata| format_size(metadata.len()));

                            ui.horizontal_wrapped(|ui| {
                                if theme::checkbox(ui, &mut included, file_name.as_str()).changed() {
                                    if included {
                                        selected_saves.insert(file_name.clone());
                                    } else {
                                        selected_saves.remove(file_name);
                                    }
                                }

                                if let Some(size) = size {
                                    theme::hint(ui, size);
                                }
                            });
                        }

                        ui.add_space(4.0);
                        theme::hint(
                            ui,
                            "Applying keeps newer saves by default; enable \"Replace existing saves\" to overwrite them.",
                        );
                    }
                });
            });

        ui.add_space(10.0);
        theme::hint(
            ui,
            if updating {
                "The instance is replaced only after the new files are staged."
            } else {
                "Saved in RimMod's library. Use Export in the Modpacks tab to share it."
            },
        );
        ui.add_space(6.0);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_save = !name.trim().is_empty();

            if ui
                .add_enabled_ui(can_save, |ui| {
                    theme::primary_button(ui, if updating { "Save changes" } else { "Save modpack" })
                })
                .inner
                .on_disabled_hover_text("Give the modpack a name first")
                .clicked()
            {
                action = Some(ModpacksAction::SaveEditor);
                done = true;
            }

            if theme::secondary_button(ui, "Cancel").clicked() {
                action = Some(ModpacksAction::CancelEditor);
                done = true;
            }
        });
    });

    PackEditorOutcome {
        action,
        open: !(done || response.should_close()),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SelectionAction {
    All,
    None,
}

fn selection_buttons(
    ui: &mut egui::Ui,
    all_selected: bool,
    none_selected: bool,
) -> Option<SelectionAction> {
    let mut selection = None;

    ui.horizontal(|ui| {
        if !all_selected && theme::link(ui, "All").clicked() {
            selection = Some(SelectionAction::All);
        }

        if !none_selected && theme::link(ui, "None").clicked() {
            selection = Some(SelectionAction::None);
        }
    });

    selection
}

fn show_editor_mod_row(ui: &mut egui::Ui, pack_mod: &PackMod, excluded_mods: &mut HashSet<String>) {
    let mut included = !excluded_mods.contains(&pack_mod.package_id);

    ui.horizontal_wrapped(|ui| {
        let name_color = if included {
            theme::TEXT
        } else {
            theme::TEXT_FAINT
        };

        if theme::checkbox(
            ui,
            &mut included,
            RichText::new(&pack_mod.name)
                .text_style(theme::strong_style())
                .color(name_color),
        )
        .changed()
        {
            if included {
                excluded_mods.remove(&pack_mod.package_id);
            } else {
                excluded_mods.insert(pack_mod.package_id.clone());
            }
        }

        ui.label(
            RichText::new(pack_mod.package_id.as_str())
                .monospace()
                .small()
                .color(theme::TEXT_FAINT),
        );

        theme::chip(
            ui,
            source_label(&pack_mod.source),
            source_color(&pack_mod.source),
        )
        .on_hover_text(source_tooltip(pack_mod));
    });
}

fn format_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = KB * 1024.0;

    if bytes as f64 >= MB {
        format!("{:.1} MB", bytes as f64 / MB)
    } else if bytes as f64 >= KB {
        format!("{:.0} KB", bytes as f64 / KB)
    } else {
        format!("{bytes} B")
    }
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::semibold_font(11.0))
            .color(theme::TEXT_FAINT),
    );
    ui.add_space(4.0);
}

fn show_icon(ui: &mut egui::Ui, rect: egui::Rect, icon: Option<&Arc<[u8]>>, name: &str, key: u64) {
    match icon {
        Some(icon) => {
            let uri = icon_uri("modpack-icon", key);
            ui.put(
                rect,
                egui::Image::from_bytes(uri, icon.clone())
                    .fit_to_exact_size(rect.size())
                    .corner_radius(CornerRadius::same(theme::RADIUS_MD)),
            );
        }
        None => {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), theme::CANVAS);

            let letter = name
                .chars()
                .find(|character| !character.is_whitespace())
                .unwrap_or('?');

            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                letter.to_uppercase().next().unwrap_or('?').to_string(),
                theme::semibold_font((rect.height() * 0.42).clamp(14.0, 34.0)),
                theme::TEXT_FAINT,
            );
        }
    }

    if ui.is_rect_visible(rect) {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::same(theme::RADIUS_MD),
            Stroke::new(1.0, theme::tint(theme::TEXT_FAINT, 40)),
            StrokeKind::Inside,
        );
    }
}

fn paint_text(
    ui: &egui::Ui,
    position: egui::Pos2,
    text: &str,
    font: egui::FontId,
    color: egui::Color32,
    max_width: f32,
) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let clipped = ui.painter().with_clip_rect(egui::Rect::from_min_size(
        position,
        Vec2::new(max_width, galley.size().y),
    ));

    clipped.galley(position, galley, color);
}

fn icon_uri(prefix: &str, key: u64) -> String {
    // The extension matters: egui's image loader uses it to pick a decoder.
    // Pack icons are always normalized to PNG before they are written.
    format!("bytes://{prefix}-{key:x}.png")
}

fn pack_icon_key(pack: &PackSummary) -> u64 {
    let mut hasher = DefaultHasher::new();
    pack.path.hash(&mut hasher);
    pack.manifest.created_at.hash(&mut hasher);
    hasher.finish()
}

fn source_counts(manifest: &modpack::ModpackManifest) -> (usize, usize, usize) {
    source_counts_from_mods(&manifest.mods)
}

fn source_counts_from_mods(mods: &[PackMod]) -> (usize, usize, usize) {
    let mut workshop = 0;
    let mut local = 0;
    let mut official = 0;

    for pack_mod in mods {
        match &pack_mod.source {
            PackSource::Official => official += 1,
            PackSource::Workshop { .. } => workshop += 1,
            PackSource::Local { .. } => local += 1,
        }
    }

    (workshop, local, official)
}

fn summary_text(workshop: usize, local: usize, official: usize, saves: usize) -> String {
    let mut parts = Vec::new();

    if workshop > 0 {
        parts.push(format!("{workshop} Workshop"));
    }

    if local > 0 {
        parts.push(format!("{local} local"));
    }

    if official > 0 {
        parts.push(format!("{official} official"));
    }

    if saves > 0 {
        parts.push(format!("{saves} save{}", plural(saves)));
    }

    if parts.is_empty() {
        "No mods".to_owned()
    } else {
        parts.join(" · ")
    }
}

fn source_label(source: &PackSource) -> &'static str {
    match source {
        PackSource::Official => "Official",
        PackSource::Workshop { .. } => "Workshop",
        PackSource::Local {
            remote_url: Some(_),
            ..
        } => "Local Git",
        PackSource::Local { .. } => "Local",
    }
}

fn source_color(source: &PackSource) -> egui::Color32 {
    match source {
        PackSource::Official => theme::TEXT_FAINT,
        PackSource::Workshop { .. } => theme::ACCENT,
        PackSource::Local { .. } => theme::SUCCESS,
    }
}

fn source_tooltip(pack_mod: &PackMod) -> String {
    match &pack_mod.source {
        PackSource::Official => "Included with RimWorld; never copied into the pack".to_owned(),
        PackSource::Workshop { workshop_id } => {
            format!("Steam Workshop item {workshop_id}; saved as a link")
        }
        PackSource::Local {
            remote_url: Some(remote_url),
            path,
        } => format!("Copied from the local mod folder\n{remote_url}\n{path}"),
        PackSource::Local { path, .. } => {
            format!("Copied from the local mod folder\n{path}")
        }
    }
}

fn truncate(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_owned();
    }

    let mut truncated = text
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    truncated.push('…');
    truncated
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

fn same_path(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_library_and_editor_render() {
        let context = egui::Context::default();
        theme::install(&context);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..egui::RawInput::default()
        };

        let mut name = "Test pack".to_owned();
        let mut description = String::new();
        let mut tags = String::new();
        let mut excluded_mods = HashSet::new();
        let mut excluded_configs = HashSet::new();
        let mut selected_saves = HashSet::new();
        let mods = Vec::new();
        let config_files = Vec::new();
        let saves = Vec::new();

        context
            .run_ui(input, |ui| {
                let action = show_modpacks(
                    ui,
                    ModpacksView {
                        packs: &[],
                        selected: None,
                        library_error: None,
                    },
                );

                assert!(action.is_none());

                let outcome = show_pack_editor(
                    ui.ctx(),
                    PackEditorView {
                        name: &mut name,
                        description: &mut description,
                        tags_input: &mut tags,
                        icon: None,
                        icon_generation: 0,
                        mods: &mods,
                        excluded_mods: &mut excluded_mods,
                        config_files: &config_files,
                        excluded_configs: &mut excluded_configs,
                        saves: &saves,
                        selected_saves: &mut selected_saves,
                        game_version: None,
                        updating: false,
                    },
                );

                assert!(outcome.open);
                assert!(outcome.action.is_none());
            })
            .drop_without_applying_deltas();
    }
}

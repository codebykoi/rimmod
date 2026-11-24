use std::collections::hash_map::DefaultHasher;
use std::collections::{BTreeMap, HashSet};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, Margin, RichText, Sense, Stroke, StrokeKind, Vec2,
};

use crate::models::{ModCollection, ModSource, major_minor_version_text, versions_are_compatible};
use crate::services::modpack::{
    self, DiffMod, ModpackManifest, PackApplyOptions, PackDiff, PackMod, PackSource, PackStatus,
    PackSummary,
};
use crate::services::workshop;
use crate::ui::{OpenTarget, icons, theme};

const CARD_MARGIN: f32 = 12.0;
const PACK_ROW_HEIGHT: f32 = 72.0;
const PACK_ROW_ICON: f32 = 48.0;
const DETAIL_ICON: f32 = 84.0;
const EDITOR_ICON: f32 = 96.0;
const MOD_ROW_HEIGHT: f32 = 30.0;
const INLINE_ICON_SIZE: f32 = 15.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackSort {
    #[default]
    Name,
    Newest,
    MostMods,
}

impl PackSort {
    pub(crate) const ALL: [Self; 3] = [Self::Name, Self::Newest, Self::MostMods];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Name => "Name A–Z",
            Self::Newest => "Recently updated",
            Self::MostMods => "Most mods",
        }
    }
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackDetailTab {
    #[default]
    Mods,
    Diff,
    Settings,
    Saves,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackModFilter {
    #[default]
    All,
    Missing,
    Local,
    Workshop,
    Official,
}

#[derive(Debug, Default, Clone)]
pub(crate) struct ModpacksUiState {
    pub(crate) search: String,
    pub(crate) sort: PackSort,
    pub(crate) tag_filter: Option<String>,
    pub(crate) detail_tab: PackDetailTab,
    pub(crate) mod_search: String,
    pub(crate) mod_filter: PackModFilter,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackEditorMode {
    Create,
    EditDetails,
    UpdateFromActive,
}

pub(crate) struct ModpacksView<'a> {
    pub(crate) packs: &'a [PackSummary],
    pub(crate) selected: Option<&'a Path>,
    pub(crate) active_pack: Option<&'a Path>,
    pub(crate) mods: &'a ModCollection,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) library_error: Option<&'a str>,
    pub(crate) ui_state: &'a mut ModpacksUiState,
    pub(crate) modal_open: bool,
}

pub(crate) enum ModpacksAction {
    NewFromActive,
    Import,
    Refresh,
    Select(PathBuf),
    Apply(PathBuf),
    EditDetails(PathBuf),
    UpdateFromActive(PathBuf),
    Duplicate(PathBuf),
    Export(PathBuf),
    CopyModList(PathBuf),
    CopyText { text: String, label: &'static str },
    Open(OpenTarget),
    Delete(PathBuf),
    SaveEditor,
    CancelEditor,
    ChooseIcon,
    RemoveIcon,
}

pub(crate) struct PackEditorView<'a> {
    pub(crate) mode: PackEditorMode,
    pub(crate) name: &'a mut String,
    pub(crate) description: &'a mut String,
    pub(crate) tags_input: &'a mut String,
    pub(crate) mod_search: &'a mut String,
    pub(crate) icon: Option<&'a Arc<[u8]>>,
    pub(crate) icon_generation: u64,
    pub(crate) icon_drop_hovered: bool,
    pub(crate) mods: &'a [PackMod],
    pub(crate) excluded_mods: &'a mut HashSet<String>,
    pub(crate) config_files: &'a [(String, PathBuf)],
    pub(crate) excluded_configs: &'a mut HashSet<String>,
    pub(crate) saves: &'a [(String, PathBuf)],
    pub(crate) selected_saves: &'a mut HashSet<String>,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) update_diff: Option<&'a PackDiff>,
}

pub(crate) struct PackEditorOutcome {
    pub(crate) action: Option<ModpacksAction>,
    pub(crate) open: bool,
}

pub(crate) struct ApplyDiffView<'a> {
    pub(crate) pack: &'a PackSummary,
    pub(crate) diff: &'a PackDiff,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) api_key_configured: bool,
    pub(crate) steamcmd_available: bool,
    pub(crate) options: &'a mut PackApplyOptions,
}

pub(crate) enum ApplyDiffDecision {
    Apply,
    Cancel,
}

pub(crate) struct DropPreviewItem {
    pub(crate) path: PathBuf,
    pub(crate) file_size: u64,
    pub(crate) manifest: ModpackManifest,
    pub(crate) icon: Option<Arc<[u8]>>,
    pub(crate) already_in_library: bool,
}

pub(crate) struct DropPreviewView<'a> {
    pub(crate) items: &'a [DropPreviewItem],
    pub(crate) diff: Option<&'a PackDiff>,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) api_key_configured: bool,
    pub(crate) steamcmd_available: bool,
}

pub(crate) enum DropPreviewDecision {
    ImportOnly,
    ImportAndApply,
    Cancel,
}

pub(crate) fn show_modpacks(ui: &mut egui::Ui, view: ModpacksView<'_>) -> Option<ModpacksAction> {
    let ModpacksView {
        packs,
        selected,
        active_pack,
        mods,
        game_version,
        library_error,
        ui_state,
        modal_open,
    } = view;
    let mut action = None;

    ui.add_space(2.0);
    show_toolbar(ui, packs, ui_state, &mut action);
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
        show_empty_state(ui, &mut action);
        return action;
    }

    let all_tags = collect_library_tags(packs);
    if !all_tags.is_empty() {
        show_tag_filter_bar(ui, &all_tags, &mut ui_state.tag_filter);
        ui.add_space(6.0);
    }

    let diffs: Vec<PackDiff> = packs
        .iter()
        .map(|pack| modpack::compute_pack_diff(&pack.manifest, mods))
        .collect();
    let statuses: Vec<PackStatus> = packs
        .iter()
        .zip(&diffs)
        .map(|(pack, diff)| {
            modpack::compute_pack_status(&pack.path, active_pack, &pack.manifest, diff)
        })
        .collect();

    let visible_indices = filtered_and_sorted_indices(packs, ui_state);

    if !modal_open && !ui.ctx().egui_wants_keyboard_input() && !visible_indices.is_empty() {
        handle_library_keyboard_nav(ui, packs, &visible_indices, selected, &mut action);
    }

    let selected_index = selected
        .and_then(|selected| {
            packs
                .iter()
                .position(|pack| same_path(&pack.path, selected))
        })
        .or_else(|| visible_indices.first().copied());

    // Fixed screen rectangles keep long descriptions and wide mod names from
    // pushing sibling panes around.
    let spacing = 10.0;
    let available_width = ui.available_width();
    let body_height = ui.available_height().max(260.0);
    let top_left = ui.cursor().min;

    let list_width = (available_width * 0.30).clamp(276.0, 360.0);
    let detail_width = (available_width - list_width - spacing).max(260.0);

    let list_rect = egui::Rect::from_min_size(top_left, Vec2::new(list_width, body_height));
    let detail_rect = egui::Rect::from_min_size(
        egui::pos2(top_left.x + list_width + spacing, top_left.y),
        Vec2::new(detail_width, body_height),
    );

    {
        let mut list_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(list_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        list_ui.set_width(list_width);
        list_ui.set_clip_rect(list_rect);

        show_library_pane(
            &mut list_ui,
            packs,
            &statuses,
            &visible_indices,
            selected,
            game_version,
            ui_state,
            list_width,
            &mut action,
        );
    }

    {
        let mut detail_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(detail_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        detail_ui.set_width(detail_width);
        detail_ui.set_clip_rect(detail_rect);

        show_detail_pane(
            &mut detail_ui,
            selected_index.map(|index| (&packs[index], &diffs[index], statuses[index])),
            mods,
            game_version,
            ui_state,
            detail_width,
            &mut action,
        );
    }

    ui.advance_cursor_after_rect(egui::Rect::from_min_size(
        top_left,
        Vec2::new(available_width, body_height),
    ));

    action
}

fn show_toolbar(
    ui: &mut egui::Ui,
    packs: &[PackSummary],
    ui_state: &mut ModpacksUiState,
    action: &mut Option<ModpacksAction>,
) {
    egui::containers::Sides::new()
        .height(theme::CONTROL_HEIGHT)
        .show(
            ui,
            |ui| {
                ui.horizontal(|ui| {
                    theme::title(ui, "Modpacks");
                    if !packs.is_empty() {
                        ui.add_space(10.0);

                        ui.allocate_ui(Vec2::new(220.0, theme::CONTROL_HEIGHT), |ui| {
                            theme::search_field(ui, &mut ui_state.search, "Filter modpacks…");
                        });

                        egui::ComboBox::from_id_salt("modpack_sort")
                            .selected_text(ui_state.sort.label())
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for sort in PackSort::ALL {
                                    ui.selectable_value(&mut ui_state.sort, sort, sort.label());
                                }
                            });
                    } else {
                        ui.add_space(6.0);
                        theme::hint(
                            ui,
                            "Snapshot, switch and share full mod lists with settings and saves",
                        );
                    }
                });
            },
            |ui| {
                if theme::primary_button_sized(
                    ui,
                    "New from active",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    *action = Some(ModpacksAction::NewFromActive);
                }

                if theme::secondary_button_sized(
                    ui,
                    "Import…",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    *action = Some(ModpacksAction::Import);
                }

                if theme::secondary_button_sized(
                    ui,
                    "Refresh",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    *action = Some(ModpacksAction::Refresh);
                }
            },
        );
}

fn show_tag_filter_bar(
    ui: &mut egui::Ui,
    tags: &[(String, usize)],
    active_tag: &mut Option<String>,
) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
        theme::hint(ui, "Tags:");

        for (tag, count) in tags {
            let selected = active_tag
                .as_deref()
                .is_some_and(|current| current.eq_ignore_ascii_case(tag));
            let label = format!("{tag} ({count})");
            let (color, hover) = if selected {
                (theme::ACCENT, theme::ACCENT)
            } else {
                (theme::TEXT_MUTED, theme::TEXT)
            };

            if theme::chip_button(ui, label, color, hover).clicked() {
                if selected {
                    *active_tag = None;
                } else {
                    *active_tag = Some(tag.clone());
                }
            }
        }

        if active_tag.is_some() && theme::link(ui, "Clear").clicked() {
            *active_tag = None;
        }
    });
}

fn show_empty_state(ui: &mut egui::Ui, action: &mut Option<ModpacksAction>) {
    let height = ui.available_height().max(260.0);

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(24),
        |ui| {
            ui.set_width(ui.available_width());
            ui.set_min_height((height - 48.0).max(200.0));

            ui.vertical_centered(|ui| {
                ui.add_space(((height - 240.0) * 0.35).clamp(24.0, 100.0));
                ui.label(
                    RichText::new("No modpacks yet")
                        .text_style(egui::TextStyle::Heading)
                        .color(theme::TEXT),
                );
                ui.add_space(6.0);
                theme::hint(
                    ui,
                    "Save your active load order, mod settings, local mods and saves into one portable pack.",
                );

                ui.add_space(16.0);
                ui.horizontal(|ui| {
                    let buttons_width = 304.0;
                    let pad = ((ui.available_width() - buttons_width) * 0.5).max(0.0);
                    ui.add_space(pad);

                    if theme::primary_button_sized(
                        ui,
                        "New from active mods",
                        Vec2::new(168.0, theme::CONTROL_HEIGHT),
                    )
                    .clicked()
                    {
                        *action = Some(ModpacksAction::NewFromActive);
                    }

                    if theme::secondary_button_sized(
                        ui,
                        "Import .rimpack…",
                        Vec2::new(128.0, theme::CONTROL_HEIGHT),
                    )
                    .clicked()
                    {
                        *action = Some(ModpacksAction::Import);
                    }
                });

                ui.add_space(22.0);

                let drop_width = ui.available_width().min(420.0);
                let (rect, _) = ui.allocate_exact_size(Vec2::new(drop_width, 54.0), Sense::hover());
                if ui.is_rect_visible(rect) {
                    ui.painter()
                        .rect_filled(rect, CornerRadius::ZERO, theme::CANVAS);
                    ui.painter().rect_stroke(
                        rect,
                        CornerRadius::ZERO,
                        Stroke::new(1.0, theme::tint(theme::ACCENT, 80)),
                        StrokeKind::Inside,
                    );
                    ui.painter().text(
                        rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "Or drag & drop a .rimpack file anywhere in this window",
                        theme::medium_font(12.0),
                        theme::TEXT_MUTED,
                    );
                }
            });
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn show_library_pane(
    ui: &mut egui::Ui,
    packs: &[PackSummary],
    statuses: &[PackStatus],
    visible_indices: &[usize],
    selected: Option<&Path>,
    game_version: Option<&str>,
    ui_state: &mut ModpacksUiState,
    column_width: f32,
    action: &mut Option<ModpacksAction>,
) {
    let inner_width = (column_width - CARD_MARGIN * 2.0).max(160.0);

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(CARD_MARGIN as i8),
        |ui| {
            ui.set_width(inner_width);
            ui.set_min_height(ui.available_height());

            ui.horizontal(|ui| {
                theme::title(ui, "Library");
                let count_label = if visible_indices.len() == packs.len() {
                    packs.len().to_string()
                } else {
                    format!("{}/{}", visible_indices.len(), packs.len())
                };
                theme::chip(ui, count_label, theme::TEXT_MUTED);
            });

            ui.add_space(8.0);

            let scroll_height = ui.available_height().max(100.0);

            if visible_indices.is_empty() {
                ui.allocate_ui(Vec2::new(inner_width, scroll_height), |ui| {
                    ui.add_space(24.0);
                    ui.vertical_centered(|ui| {
                        theme::hint(ui, "No modpacks match this filter.");
                        ui.add_space(6.0);
                        if theme::link(ui, "Clear filters").clicked() {
                            ui_state.search.clear();
                            ui_state.tag_filter = None;
                        }
                    });
                });
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("modpack_library")
                    .max_height(scroll_height)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        let row_width = (inner_width - theme::SCROLL_GUTTER).max(140.0);

                        for &index in visible_indices {
                            let pack = &packs[index];
                            let status = statuses[index];
                            let is_selected =
                                selected.is_some_and(|selected| same_path(&pack.path, selected));

                            show_pack_row(
                                ui,
                                pack,
                                status,
                                is_selected,
                                game_version,
                                row_width,
                                action,
                            );
                        }
                    });
            }
        },
    );
}

#[allow(clippy::too_many_arguments)]
fn show_pack_row(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    status: PackStatus,
    selected: bool,
    game_version: Option<&str>,
    width: f32,
    action: &mut Option<ModpacksAction>,
) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(width, PACK_ROW_HEIGHT), Sense::click());
    let hovered = response.hovered();

    let hover_t = ui.ctx().animate_bool_with_time(
        ui.id().with(("pack_row_hover", &pack.path)),
        hovered && !selected,
        0.12,
    );

    if ui.is_rect_visible(rect) {
        let fill = if selected {
            theme::tint(theme::ACCENT, 42)
        } else if hover_t > 0.0 {
            lerp_color(theme::CANVAS, theme::SURFACE_HOVER, hover_t)
        } else {
            theme::CANVAS
        };

        ui.painter()
            .rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), fill);

        let rail_color = if selected {
            Some(if status.is_active() {
                theme::SUCCESS
            } else {
                theme::ACCENT
            })
        } else if status.is_active() {
            Some(theme::tint(theme::SUCCESS, 180))
        } else if status.is_modified() {
            Some(theme::tint(theme::WARNING, 180))
        } else {
            None
        };

        if let Some(color) = rail_color {
            let rail = egui::Rect::from_min_max(
                rect.left_top(),
                egui::pos2(rect.left() + 3.0, rect.bottom()),
            );
            ui.painter().rect_filled(rail, CornerRadius::ZERO, color);
        }

        let icon_rect = egui::Rect::from_min_size(
            egui::pos2(rect.left() + 10.0, rect.center().y - PACK_ROW_ICON / 2.0),
            Vec2::splat(PACK_ROW_ICON),
        );
        show_icon(
            ui,
            icon_rect,
            pack.icon.as_ref(),
            &pack.manifest.name,
            pack_icon_key(pack),
        );

        let text_left = icon_rect.right() + 10.0;
        let right_edge = rect.right() - 10.0;

        // Paint optional right-aligned status pill on line 1.
        let badge = match status {
            PackStatus::Active => Some(("Active".to_owned(), theme::SUCCESS)),
            PackStatus::Modified { .. } => {
                let count = status.modified_total();
                let label = if count > 0 {
                    format!("Modified ({count})")
                } else {
                    "Modified".to_owned()
                };
                Some((label, theme::WARNING))
            }
            PackStatus::NotApplied => None,
        };

        let mut name_right = right_edge;
        if let Some((badge_text, badge_color)) = badge {
            let badge_font = theme::medium_font(11.0);
            let badge_galley =
                ui.painter()
                    .layout_no_wrap(badge_text, badge_font, Color32::PLACEHOLDER);
            let pad = Vec2::new(6.0, 2.0);
            let badge_size = badge_galley.size() + pad * 2.0;
            let badge_rect = egui::Rect::from_min_size(
                egui::pos2(right_edge - badge_size.x, rect.top() + 9.0),
                badge_size,
            );

            ui.painter()
                .rect_filled(badge_rect, CornerRadius::ZERO, theme::tint(badge_color, 36));
            ui.painter()
                .galley(badge_rect.min + pad, badge_galley, badge_color);

            name_right = badge_rect.left() - 6.0;
        }

        let name_width = (name_right - text_left).max(20.0);
        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 10.0),
            &pack.manifest.name,
            theme::semibold_font(14.0),
            theme::TEXT,
            name_width,
        );

        let full_text_width = (right_edge - text_left).max(20.0);
        let meta = compact_pack_meta(pack, game_version);
        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 31.0),
            &meta,
            theme::medium_font(12.0),
            theme::TEXT_MUTED,
            full_text_width,
        );

        let subtitle = if !pack.manifest.tags.is_empty() {
            pack.manifest.tags.join(", ")
        } else if !pack.manifest.description.trim().is_empty() {
            pack.manifest
                .description
                .lines()
                .next()
                .unwrap_or("")
                .trim()
                .to_owned()
        } else {
            format_size(pack.size_bytes)
        };

        paint_text(
            ui,
            egui::pos2(text_left, rect.top() + 49.0),
            &subtitle,
            theme::medium_font(11.0),
            theme::TEXT_FAINT,
            full_text_width,
        );
    }

    let response = response.on_hover_cursor(CursorIcon::PointingHand);

    if response.double_clicked() {
        *action = Some(ModpacksAction::Apply(pack.path.clone()));
    } else if response.clicked() {
        *action = Some(ModpacksAction::Select(pack.path.clone()));
    }

    egui::Popup::context_menu(&response)
        .frame(theme::menu_frame())
        .show(|ui| {
            ui.set_min_width(210.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);

            if ui.button("Apply modpack…").clicked() {
                *action = Some(ModpacksAction::Apply(pack.path.clone()));
                ui.close();
            }

            if ui.button("Update from active mods…").clicked() {
                *action = Some(ModpacksAction::UpdateFromActive(pack.path.clone()));
                ui.close();
            }

            if ui.button("Edit details…").clicked() {
                *action = Some(ModpacksAction::EditDetails(pack.path.clone()));
                ui.close();
            }

            if ui.button("Duplicate").clicked() {
                *action = Some(ModpacksAction::Duplicate(pack.path.clone()));
                ui.close();
            }

            theme::menu_separator(ui);

            if ui.button("Export .rimpack…").clicked() {
                *action = Some(ModpacksAction::Export(pack.path.clone()));
                ui.close();
            }

            if ui.button("Copy mod list").clicked() {
                *action = Some(ModpacksAction::CopyModList(pack.path.clone()));
                ui.close();
            }

            if ui.button("Open pack folder").clicked() {
                *action = Some(ModpacksAction::Open(OpenTarget::Folder(pack.path.clone())));
                ui.close();
            }

            theme::menu_separator(ui);

            if ui
                .button(RichText::new("Delete").color(theme::DANGER))
                .clicked()
            {
                *action = Some(ModpacksAction::Delete(pack.path.clone()));
                ui.close();
            }
        });
}

fn show_detail_pane(
    ui: &mut egui::Ui,
    selected: Option<(&PackSummary, &PackDiff, PackStatus)>,
    mods: &ModCollection,
    game_version: Option<&str>,
    ui_state: &mut ModpacksUiState,
    column_width: f32,
    action: &mut Option<ModpacksAction>,
) {
    let inner_width = (column_width - CARD_MARGIN * 2.0).max(220.0);

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(CARD_MARGIN as i8),
        |ui| {
            ui.set_width(inner_width);
            ui.set_min_height(ui.available_height());

            let Some((pack, diff, status)) = selected else {
                ui.vertical_centered(|ui| {
                    ui.add_space(64.0);
                    theme::hint(ui, "Select a modpack to inspect its contents and diff.");
                });
                return;
            };

            show_detail_header(ui, pack, diff, status, game_version);
            ui.add_space(10.0);

            show_detail_actions(ui, pack, status, action);
            ui.add_space(8.0);

            if let PackStatus::Modified { .. } = status {
                show_modified_banner(ui, pack, status, ui_state, action);
                ui.add_space(8.0);
            }

            // If Settings/Saves tab was selected on another pack that had them,
            // fall back cleanly when switching to a pack without them.
            if ui_state.detail_tab == PackDetailTab::Settings && pack.manifest.configs.is_empty() {
                ui_state.detail_tab = PackDetailTab::Mods;
            }
            if ui_state.detail_tab == PackDetailTab::Saves && pack.manifest.saves.is_empty() {
                ui_state.detail_tab = PackDetailTab::Mods;
            }

            show_detail_tabs(ui, pack, diff, &mut ui_state.detail_tab);
            ui.add_space(8.0);

            match ui_state.detail_tab {
                PackDetailTab::Mods => {
                    show_detail_mods_tab(ui, pack, diff, mods, ui_state, inner_width, action);
                }
                PackDetailTab::Diff => {
                    show_detail_diff_tab(ui, diff, inner_width, action);
                }
                PackDetailTab::Settings => {
                    show_detail_files_tab(
                        ui,
                        "modpack_settings_list",
                        &pack.path.join("configs"),
                        &pack.manifest.configs,
                        "Mod settings files restored when applying this pack.",
                        action,
                    );
                }
                PackDetailTab::Saves => {
                    show_detail_files_tab(
                        ui,
                        "modpack_saves_list",
                        &pack.path.join("saves"),
                        &pack.manifest.saves,
                        "Existing saves with the same name are kept unless overwrite is enabled when applying.",
                        action,
                    );
                }
            }
        },
    );
}

fn show_detail_header(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    diff: &PackDiff,
    status: PackStatus,
    game_version: Option<&str>,
) {
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
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&pack.manifest.name)
                        .text_style(egui::TextStyle::Heading)
                        .color(theme::TEXT),
                );

                match status {
                    PackStatus::Active => {
                        theme::chip(ui, "Active", theme::SUCCESS)
                            .on_hover_text("Matches your currently active load order.");
                    }
                    PackStatus::Modified {
                        active_added,
                        active_removed,
                        reordered,
                    } => {
                        let label = status
                            .modified_delta_summary()
                            .map(|summary| format!("Modified ({summary})"))
                            .unwrap_or_else(|| "Modified".to_owned());
                        theme::chip(ui, label, theme::WARNING).on_hover_text(format!(
                            "Active load order diverged from this pack: +{active_added} enabled, -{active_removed} disabled, ~{reordered} reordered."
                        ));
                    }
                    PackStatus::NotApplied => {
                        if !diff.is_in_sync() {
                            let changes = diff.total_changes();
                            if changes > 0 {
                                theme::chip(
                                    ui,
                                    format!("+{}/-{}", diff.added.len(), diff.removed.len()),
                                    theme::TEXT_MUTED,
                                )
                                .on_hover_text(format!(
                                    "Applying will enable {}, disable {}, and reorder {} mod(s).",
                                    diff.added.len(),
                                    diff.removed.len(),
                                    diff.reordered.len()
                                ));
                            }
                        }
                    }
                }

                if let Some(pack_ver) = pack.manifest.game_version.as_deref() {
                    let short = major_minor_version_text(pack_ver)
                        .unwrap_or_else(|| pack_ver.to_owned());
                    let mismatch = game_version
                        .is_some_and(|current| !versions_are_compatible(pack_ver, current));
                    let color = if mismatch {
                        theme::WARNING
                    } else {
                        theme::TEXT_MUTED
                    };
                    let resp = theme::chip(ui, format!("RimWorld {short}"), color);
                    if mismatch {
                        resp.on_hover_text(format!(
                            "Saved from RimWorld {pack_ver}; your installed game is {}",
                            game_version.unwrap_or("unknown")
                        ));
                    } else {
                        resp.on_hover_text(format!("Saved from RimWorld {pack_ver}"));
                    }
                }

                if !diff.missing.is_empty() {
                    theme::chip(
                        ui,
                        format!("{} missing", diff.missing.len()),
                        theme::DANGER,
                    )
                    .on_hover_text("Some mods in this pack are not installed yet.");
                }
            });

            ui.add_space(2.0);

            let (workshop, local, official) = source_counts(&pack.manifest);
            let mut primary_stats = vec![format!(
                "{} mod{} ({})",
                pack.manifest.mods.len(),
                plural(pack.manifest.mods.len()),
                summary_text(workshop, local, official, 0)
            )];
            if !pack.manifest.configs.is_empty() {
                primary_stats.push(format!(
                    "{} setting{}",
                    pack.manifest.configs.len(),
                    plural(pack.manifest.configs.len())
                ));
            }
            if !pack.manifest.saves.is_empty() {
                primary_stats.push(format!(
                    "{} save{}",
                    pack.manifest.saves.len(),
                    plural(pack.manifest.saves.len())
                ));
            }
            theme::hint(ui, primary_stats.join(", "));

            let mut secondary_stats = vec![format_size(pack.size_bytes)];
            if pack.manifest.created_at > 0
                && let Some(date) = workshop::format_date(pack.manifest.created_at)
            {
                secondary_stats.push(format!("Saved {date}"));
            }
            ui.label(
                RichText::new(secondary_stats.join(", "))
                    .small()
                    .color(theme::TEXT_FAINT),
            );

            if !pack.manifest.tags.is_empty() {
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    for tag in &pack.manifest.tags {
                        theme::chip(ui, tag.clone(), theme::ACCENT);
                    }
                });
            }
        });
    });

    if !pack.manifest.description.trim().is_empty() {
        ui.add_space(8.0);
        egui::Frame::new()
            .fill(theme::CANVAS)
            .corner_radius(CornerRadius::same(theme::RADIUS_MD))
            .inner_margin(Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    egui::Label::new(
                        RichText::new(pack.manifest.description.trim()).color(theme::TEXT_MUTED),
                    )
                    .wrap(),
                );
            });
    }
}

fn show_detail_actions(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    status: PackStatus,
    action: &mut Option<ModpacksAction>,
) {
    ui.horizontal_wrapped(|ui| {
        let apply_label = if status.is_active() {
            "Re-apply…"
        } else {
            "Apply modpack…"
        };

        if theme::primary_button_sized(
            ui,
            apply_label,
            Vec2::new(0.0, theme::CONTROL_HEIGHT),
        )
        .clicked()
        {
            *action = Some(ModpacksAction::Apply(pack.path.clone()));
        }

        if theme::secondary_button_sized(
            ui,
            "Update from active…",
            Vec2::new(0.0, theme::CONTROL_HEIGHT),
        )
        .on_hover_text("Replace this pack's mod list, settings and saves with your current active setup")
        .clicked()
        {
            *action = Some(ModpacksAction::UpdateFromActive(pack.path.clone()));
        }

        if theme::secondary_button_sized(
            ui,
            "Edit details…",
            Vec2::new(0.0, theme::CONTROL_HEIGHT),
        )
        .on_hover_text("Change name, icon, tags, description or trim included items without overwriting the mod list")
        .clicked()
        {
            *action = Some(ModpacksAction::EditDetails(pack.path.clone()));
        }

        if theme::secondary_button_sized(
            ui,
            "Export…",
            Vec2::new(0.0, theme::CONTROL_HEIGHT),
        )
        .on_hover_text("Export as a shareable .rimpack file")
        .clicked()
        {
            *action = Some(ModpacksAction::Export(pack.path.clone()));
        }

        let more = theme::secondary_button_sized(
            ui,
            "More",
            Vec2::new(0.0, theme::CONTROL_HEIGHT),
        );

        egui::Popup::menu(&more)
            .frame(theme::menu_frame())
            .show(|ui| {
                ui.set_min_width(190.0);
                ui.spacing_mut().item_spacing.y = 4.0;
                ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);

                if ui.button("Duplicate").clicked() {
                    *action = Some(ModpacksAction::Duplicate(pack.path.clone()));
                    ui.close();
                }

                if ui.button("Copy mod list").clicked() {
                    *action = Some(ModpacksAction::CopyModList(pack.path.clone()));
                    ui.close();
                }

                if ui.button("Open pack folder").clicked() {
                    *action = Some(ModpacksAction::Open(OpenTarget::Folder(pack.path.clone())));
                    ui.close();
                }

                theme::menu_separator(ui);

                if ui
                    .button(RichText::new("Delete modpack").color(theme::DANGER))
                    .clicked()
                {
                    *action = Some(ModpacksAction::Delete(pack.path.clone()));
                    ui.close();
                }
            });
    });
}

fn show_modified_banner(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    status: PackStatus,
    ui_state: &mut ModpacksUiState,
    action: &mut Option<ModpacksAction>,
) {
    let summary = status
        .modified_delta_summary()
        .unwrap_or_else(|| "order changed".to_owned());

    egui::Frame::new()
        .fill(theme::tint(theme::WARNING, 22))
        .inner_margin(Margin::symmetric(10, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::containers::Sides::new().show(
                ui,
                |ui| {
                    ui.label(
                        RichText::new(format!(
                            "Active mods changed since applying this pack ({summary})."
                        ))
                        .small()
                        .color(theme::WARNING),
                    );
                },
                |ui| {
                    if theme::compact_button(ui, "Update pack", theme::WARNING, theme::WARNING)
                        .clicked()
                    {
                        *action = Some(ModpacksAction::UpdateFromActive(pack.path.clone()));
                    }

                    if ui_state.detail_tab != PackDetailTab::Diff
                        && theme::compact_button(ui, "View diff", theme::TEXT_MUTED, theme::TEXT)
                            .clicked()
                    {
                        ui_state.detail_tab = PackDetailTab::Diff;
                    }
                },
            );
        });
}

fn show_detail_tabs(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    diff: &PackDiff,
    active_tab: &mut PackDetailTab,
) {
    ui.horizontal(|ui| {
        if theme::tab_with_count(
            ui,
            *active_tab == PackDetailTab::Mods,
            "Mods",
            Some(pack.manifest.mods.len().to_string()),
        )
        .clicked()
        {
            *active_tab = PackDetailTab::Mods;
        }

        let diff_count = (!diff.is_in_sync()).then(|| diff.total_changes().to_string());
        if theme::tab_with_count(ui, *active_tab == PackDetailTab::Diff, "Diff", diff_count)
            .clicked()
        {
            *active_tab = PackDetailTab::Diff;
        }

        if !pack.manifest.configs.is_empty()
            && theme::tab_with_count(
                ui,
                *active_tab == PackDetailTab::Settings,
                "Settings",
                Some(pack.manifest.configs.len().to_string()),
            )
            .clicked()
        {
            *active_tab = PackDetailTab::Settings;
        }

        if !pack.manifest.saves.is_empty()
            && theme::tab_with_count(
                ui,
                *active_tab == PackDetailTab::Saves,
                "Saves",
                Some(pack.manifest.saves.len().to_string()),
            )
            .clicked()
        {
            *active_tab = PackDetailTab::Saves;
        }
    });
}

fn show_detail_mods_tab(
    ui: &mut egui::Ui,
    pack: &PackSummary,
    diff: &PackDiff,
    mods: &ModCollection,
    ui_state: &mut ModpacksUiState,
    width: f32,
    action: &mut Option<ModpacksAction>,
) {
    let missing_ids: HashSet<String> = diff
        .missing
        .iter()
        .map(|pack_mod| pack_mod.package_id.to_ascii_lowercase())
        .collect();
    let local_to_install_ids: HashSet<String> = diff
        .local_to_install
        .iter()
        .map(|pack_mod| pack_mod.package_id.to_ascii_lowercase())
        .collect();
    let active_ids: HashSet<&str> = mods
        .enabled_ids()
        .iter()
        .filter_map(|&id| mods.get(id))
        .map(|rimworld_mod| rimworld_mod.package_id.as_str())
        .collect();

    let (workshop_count, local_count, official_count) = source_counts(&pack.manifest);

    // Filter row: search + source/status chips.
    ui.horizontal_wrapped(|ui| {
        ui.allocate_ui(
            Vec2::new(220.0_f32.min(width * 0.45), theme::CONTROL_HEIGHT),
            |ui| {
                theme::search_field(ui, &mut ui_state.mod_search, "Filter mods in pack…");
            },
        );

        let filters: &[(PackModFilter, String, bool)] = &[
            (
                PackModFilter::All,
                format!("All ({})", pack.manifest.mods.len()),
                true,
            ),
            (
                PackModFilter::Missing,
                format!("Missing ({})", diff.missing.len()),
                !diff.missing.is_empty(),
            ),
            (
                PackModFilter::Local,
                format!("Local ({local_count})"),
                local_count > 0,
            ),
            (
                PackModFilter::Workshop,
                format!("Workshop ({workshop_count})"),
                workshop_count > 0,
            ),
            (
                PackModFilter::Official,
                format!("Official ({official_count})"),
                official_count > 0,
            ),
        ];

        for (filter, label, visible) in filters {
            if !visible && ui_state.mod_filter != *filter {
                continue;
            }
            let selected = ui_state.mod_filter == *filter;
            let (color, hover) = if selected {
                if *filter == PackModFilter::Missing {
                    (theme::DANGER, theme::DANGER)
                } else {
                    (theme::ACCENT, theme::ACCENT)
                }
            } else if *filter == PackModFilter::Missing {
                (theme::WARNING, theme::DANGER)
            } else {
                (theme::TEXT_MUTED, theme::TEXT)
            };

            if theme::chip_button(ui, label.clone(), color, hover).clicked() {
                ui_state.mod_filter = *filter;
            }
        }
    });

    ui.add_space(6.0);

    let query = ui_state.mod_search.trim().to_ascii_lowercase();
    let visible_mods: Vec<&PackMod> = pack
        .manifest
        .mods
        .iter()
        .filter(|pack_mod| {
            let matches_filter = match ui_state.mod_filter {
                PackModFilter::All => true,
                PackModFilter::Missing => {
                    missing_ids.contains(&pack_mod.package_id.to_ascii_lowercase())
                }
                PackModFilter::Local => pack_mod.is_local(),
                PackModFilter::Workshop => matches!(pack_mod.source, PackSource::Workshop { .. }),
                PackModFilter::Official => matches!(pack_mod.source, PackSource::Official),
            };
            if !matches_filter {
                return false;
            }
            if query.is_empty() {
                return true;
            }
            pack_mod.name.to_ascii_lowercase().contains(&query)
                || pack_mod.package_id.to_ascii_lowercase().contains(&query)
        })
        .collect();

    if visible_mods.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            theme::hint(ui, "No mods match this filter.");
        });
        return;
    }

    let list_height = ui.available_height().max(120.0);
    let row_width = (width - theme::SCROLL_GUTTER).max(160.0);

    egui::Frame::new()
        .fill(theme::CANVAS)
        .inner_margin(Margin::symmetric(6, 4))
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt(("modpack_mods_scroll", &pack.path))
                .max_height((list_height - 8.0).max(100.0))
                .auto_shrink([false, false])
                .show_rows(ui, MOD_ROW_HEIGHT, visible_mods.len(), |ui, range| {
                    ui.spacing_mut().item_spacing.y = 0.0;

                    for index in range {
                        let pack_mod = visible_mods[index];
                        let lower_id = pack_mod.package_id.to_ascii_lowercase();
                        let is_missing = missing_ids.contains(&lower_id);
                        let is_local_bundled = local_to_install_ids.contains(&lower_id);
                        let is_active = active_ids.contains(lower_id.as_str());

                        show_virtual_pack_mod_row(
                            ui,
                            pack_mod,
                            is_missing,
                            is_local_bundled,
                            is_active,
                            (row_width - 12.0).max(140.0),
                            action,
                        );
                    }
                });
        });
}

fn show_virtual_pack_mod_row(
    ui: &mut egui::Ui,
    pack_mod: &PackMod,
    is_missing: bool,
    is_local_bundled: bool,
    is_active: bool,
    width: f32,
    action: &mut Option<ModpacksAction>,
) {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, MOD_ROW_HEIGHT), Sense::click());

    if ui.is_rect_visible(rect) {
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, CornerRadius::ZERO, theme::SURFACE_HOVER);
        }

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 14.0, rect.center().y),
            Vec2::splat(INLINE_ICON_SIZE),
        );
        egui::Image::new(pack_source_icon(&pack_mod.source))
            .fit_to_exact_size(icon_rect.size())
            .paint_at(ui, icon_rect);

        // Right-aligned signal badge (only when meaningful).
        let mut right_edge = rect.right() - 8.0;
        let status_badge = if is_missing {
            Some(("Missing", theme::DANGER))
        } else if is_local_bundled {
            Some(("Bundled", theme::SUCCESS))
        } else if !is_active {
            Some(("Inactive", theme::TEXT_FAINT))
        } else {
            None
        };

        if let Some((badge_label, badge_color)) = status_badge {
            let font = theme::medium_font(11.0);
            let galley =
                ui.painter()
                    .layout_no_wrap(badge_label.to_owned(), font, Color32::PLACEHOLDER);
            let pad = Vec2::new(6.0, 2.0);
            let size = galley.size() + pad * 2.0;
            let badge_rect = egui::Rect::from_min_size(
                egui::pos2(right_edge - size.x, rect.center().y - size.y / 2.0),
                size,
            );
            ui.painter()
                .rect_filled(badge_rect, CornerRadius::ZERO, theme::tint(badge_color, 30));
            ui.painter()
                .galley(badge_rect.min + pad, galley, badge_color);
            right_edge = badge_rect.left() - 8.0;
        }

        let name_left = icon_rect.right() + 8.0;
        let name_color = if is_missing {
            theme::TEXT_MUTED
        } else {
            theme::TEXT
        };

        let name_galley = ui.painter().layout_no_wrap(
            pack_mod.name.clone(),
            theme::semibold_font(13.0),
            name_color,
        );
        let max_name_width = (right_edge - name_left).max(20.0);
        let clipped_name_width = name_galley.size().x.min(max_name_width);
        let name_pos = egui::pos2(name_left, rect.center().y - name_galley.size().y / 2.0);
        ui.painter()
            .with_clip_rect(egui::Rect::from_min_size(
                name_pos,
                Vec2::new(max_name_width, name_galley.size().y),
            ))
            .galley(name_pos, name_galley, name_color);

        let id_left = name_left + clipped_name_width + 8.0;
        let id_width = right_edge - id_left;
        if id_width > 40.0 {
            let id_galley = ui.painter().layout_no_wrap(
                pack_mod.package_id.clone(),
                egui::FontId::monospace(12.0),
                theme::TEXT_FAINT,
            );
            let id_pos = egui::pos2(id_left, rect.center().y - id_galley.size().y / 2.0);
            ui.painter()
                .with_clip_rect(egui::Rect::from_min_size(
                    id_pos,
                    Vec2::new(id_width, id_galley.size().y),
                ))
                .galley(id_pos, id_galley, theme::TEXT_FAINT);
        }
    }

    let response = response.on_hover_text(source_tooltip(pack_mod));

    egui::Popup::context_menu(&response)
        .frame(theme::menu_frame())
        .show(|ui| {
            ui.set_min_width(190.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);

            if let Some(url) = pack_mod.workshop_url()
                && ui.button("Open on Steam Workshop").clicked()
            {
                *action = Some(ModpacksAction::Open(OpenTarget::Url(url)));
                ui.close();
            }

            if let PackSource::Local {
                remote_url: Some(url),
                ..
            } = &pack_mod.source
                && ui.button("Open repository URL").clicked()
            {
                *action = Some(ModpacksAction::Open(OpenTarget::Url(url.clone())));
                ui.close();
            }

            if ui.button("Copy package ID").clicked() {
                *action = Some(ModpacksAction::CopyText {
                    text: pack_mod.package_id.clone(),
                    label: "Package ID",
                });
                ui.close();
            }
        });
}

fn show_detail_diff_tab(
    ui: &mut egui::Ui,
    diff: &PackDiff,
    width: f32,
    action: &mut Option<ModpacksAction>,
) {
    if diff.is_in_sync() {
        egui::Frame::new()
            .fill(theme::CANVAS)
            .inner_margin(Margin::same(16))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    theme::chip(ui, "In sync", theme::SUCCESS);
                    ui.label(
                        RichText::new("Your active load order matches this modpack exactly.")
                            .color(theme::TEXT_MUTED),
                    );
                });
            });
        return;
    }

    show_diff_summary_chips(ui, diff);
    ui.add_space(8.0);

    let scroll_height = ui.available_height().max(120.0);

    egui::Frame::new()
        .fill(theme::CANVAS)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width((width - 20.0).max(160.0));
            egui::ScrollArea::vertical()
                .id_salt("modpack_detail_diff_scroll")
                .max_height((scroll_height - 20.0).max(100.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    show_diff_sections(ui, diff, true, Some(action));
                });
        });
}

fn show_detail_files_tab(
    ui: &mut egui::Ui,
    id_salt: &str,
    folder: &Path,
    files: &[String],
    hint_text: &str,
    action: &mut Option<ModpacksAction>,
) {
    egui::containers::Sides::new().show(
        ui,
        |ui| {
            theme::hint(ui, hint_text);
        },
        |ui| {
            if folder.is_dir() && theme::link(ui, "Open folder").clicked() {
                *action = Some(ModpacksAction::Open(OpenTarget::Folder(
                    folder.to_path_buf(),
                )));
            }
        },
    );

    ui.add_space(6.0);

    let list_height = ui.available_height().max(100.0);
    egui::Frame::new()
        .fill(theme::CANVAS)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            egui::ScrollArea::vertical()
                .id_salt(id_salt)
                .max_height((list_height - 20.0).max(80.0))
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 4.0;
                    for file in files {
                        let size = std::fs::metadata(folder.join(file))
                            .ok()
                            .map(|meta| format_size(meta.len()));

                        egui::containers::Sides::new().show(
                            ui,
                            |ui| {
                                ui.label(
                                    RichText::new(file).monospace().small().color(theme::TEXT),
                                );
                            },
                            |ui| {
                                if let Some(size) = size {
                                    theme::hint(ui, size);
                                }
                            },
                        );
                    }
                });
        });
}

fn show_diff_summary_chips(ui: &mut egui::Ui, diff: &PackDiff) {
    ui.horizontal_wrapped(|ui| {
        if !diff.added.is_empty() {
            theme::chip(
                ui,
                format!("+{} to enable", diff.added.len()),
                theme::SUCCESS,
            );
        }
        if !diff.removed.is_empty() {
            theme::chip(
                ui,
                format!("-{} to disable", diff.removed.len()),
                theme::DANGER,
            );
        }
        if !diff.reordered.is_empty() {
            theme::chip(
                ui,
                format!("~{} reordered", diff.reordered.len()),
                theme::ACCENT,
            );
        }
        if !diff.local_to_install.is_empty() {
            theme::chip(
                ui,
                format!("{} local to copy", diff.local_to_install.len()),
                theme::TEXT_MUTED,
            );
        }
        if !diff.missing.is_empty() {
            theme::chip(
                ui,
                format!("{} missing", diff.missing.len()),
                theme::WARNING,
            );
        }
    });
}

fn show_diff_sections(
    ui: &mut egui::Ui,
    diff: &PackDiff,
    default_open_changes: bool,
    mut action: Option<&mut Option<ModpacksAction>>,
) {
    if !diff.missing.is_empty() {
        theme::collapsible(
            ui,
            "diff_missing",
            format!("Missing mods ({})", diff.missing.len()),
            true,
            |ui| {
                for pack_mod in &diff.missing {
                    show_diff_pack_mod_line(
                        ui,
                        "!",
                        theme::WARNING,
                        pack_mod,
                        action.as_deref_mut(),
                    );
                }
            },
        );
        ui.add_space(4.0);
    }

    if !diff.added.is_empty() {
        theme::collapsible(
            ui,
            "diff_added",
            format!("Will be enabled ({})", diff.added.len()),
            default_open_changes && diff.added.len() <= 40,
            |ui| {
                for pack_mod in &diff.added {
                    show_diff_pack_mod_line(
                        ui,
                        "+",
                        theme::SUCCESS,
                        pack_mod,
                        action.as_deref_mut(),
                    );
                }
            },
        );
        ui.add_space(4.0);
    }

    if !diff.removed.is_empty() {
        theme::collapsible(
            ui,
            "diff_removed",
            format!("Will be disabled ({})", diff.removed.len()),
            default_open_changes && diff.removed.len() <= 40,
            |ui| {
                for removed in &diff.removed {
                    show_diff_removed_line(ui, removed);
                }
            },
        );
        ui.add_space(4.0);
    }

    if !diff.reordered.is_empty() {
        theme::collapsible(
            ui,
            "diff_reordered",
            format!("Order changes ({})", diff.reordered.len()),
            false,
            |ui| {
                for pack_mod in &diff.reordered {
                    show_diff_pack_mod_line(
                        ui,
                        "~",
                        theme::ACCENT,
                        pack_mod,
                        action.as_deref_mut(),
                    );
                }
            },
        );
        ui.add_space(4.0);
    }

    if !diff.local_to_install.is_empty() {
        theme::collapsible(
            ui,
            "diff_local",
            format!("Local mods to copy ({})", diff.local_to_install.len()),
            false,
            |ui| {
                for pack_mod in &diff.local_to_install {
                    show_diff_pack_mod_line(
                        ui,
                        "↓",
                        theme::TEXT_MUTED,
                        pack_mod,
                        action.as_deref_mut(),
                    );
                }
            },
        );
    }
}

fn show_diff_pack_mod_line(
    ui: &mut egui::Ui,
    prefix: &str,
    prefix_color: Color32,
    pack_mod: &PackMod,
    action: Option<&mut Option<ModpacksAction>>,
) {
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(prefix)
                .monospace()
                .strong()
                .color(prefix_color),
        );
        ui.add(
            egui::Image::new(pack_source_icon(&pack_mod.source))
                .fit_to_exact_size(Vec2::splat(14.0)),
        );
        ui.label(RichText::new(&pack_mod.name).color(theme::TEXT));
        ui.label(
            RichText::new(&pack_mod.package_id)
                .monospace()
                .small()
                .color(theme::TEXT_FAINT),
        );

        if let (Some(url), Some(action)) = (pack_mod.workshop_url(), action)
            && theme::link(ui, "Workshop").clicked()
        {
            *action = Some(ModpacksAction::Open(OpenTarget::Url(url)));
        }
    });
}

fn show_diff_removed_line(ui: &mut egui::Ui, removed: &DiffMod) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("-").monospace().strong().color(theme::DANGER));
        ui.add(
            egui::Image::new(pack_source_icon(&removed.source))
                .fit_to_exact_size(Vec2::splat(14.0)),
        );
        ui.label(RichText::new(&removed.name).color(theme::TEXT_MUTED));
        ui.label(
            RichText::new(&removed.package_id)
                .monospace()
                .small()
                .color(theme::TEXT_FAINT),
        );
    });
}

/// Apply confirmation modal with full diff preview and restore options.
pub(crate) fn show_apply_diff_modal(
    context: &egui::Context,
    view: ApplyDiffView<'_>,
) -> Option<ApplyDiffDecision> {
    let ApplyDiffView {
        pack,
        diff,
        game_version,
        api_key_configured,
        steamcmd_available,
        options,
    } = view;

    let mut decision = None;

    let response = egui::Modal::new(egui::Id::new("apply_modpack")).show(context, |ui| {
        ui.set_width(560.0);

        ui.horizontal_top(|ui| {
            let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(44.0), Sense::hover());
            show_icon(
                ui,
                icon_rect,
                pack.icon.as_ref(),
                &pack.manifest.name,
                pack_icon_key(pack),
            );

            ui.add_space(10.0);

            ui.vertical(|ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(format!("Apply \"{}\"?", pack.manifest.name))
                            .text_style(theme::title_style())
                            .color(theme::TEXT),
                    );

                    if let Some(pack_ver) = pack.manifest.game_version.as_deref()
                        && let Some(current) = game_version
                        && !versions_are_compatible(pack_ver, current)
                    {
                        theme::chip(
                            ui,
                            format!("Saved for {pack_ver}"),
                            theme::WARNING,
                        );
                    }
                });

                theme::hint(
                    ui,
                    format!(
                        "{} mod{}, {} settings, {} save{}",
                        pack.manifest.mods.len(),
                        plural(pack.manifest.mods.len()),
                        pack.manifest.configs.len(),
                        pack.manifest.saves.len(),
                        plural(pack.manifest.saves.len()),
                    ),
                );
            });
        });

        ui.add_space(12.0);

        theme::group().show(ui, |ui| {
            ui.set_width(ui.available_width());

            if diff.is_in_sync() {
                ui.horizontal(|ui| {
                    theme::chip(ui, "In sync", theme::SUCCESS);
                    ui.label(
                        RichText::new("Your active load order already matches this modpack.")
                            .color(theme::TEXT),
                    );
                });
            } else {
                show_diff_summary_chips(ui, diff);
                ui.add_space(8.0);

                egui::ScrollArea::vertical()
                    .id_salt("apply_diff_scroll")
                    .max_height(210.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        show_diff_sections(ui, diff, true, None);
                    });
            }

            let missing_workshop = diff.missing_workshop_count();
            if missing_workshop > 0 {
                ui.add_space(8.0);
                if api_key_configured || steamcmd_available {
                    theme::hint(
                        ui,
                        format!(
                            "{missing_workshop} missing Workshop mod{} will be downloaded/subscribed automatically.",
                            plural(missing_workshop)
                        ),
                    );
                } else {
                    ui.label(
                        RichText::new(format!(
                            "{missing_workshop} missing Workshop mod{} cannot be downloaded automatically. Add a Steam Web API key or SteamCMD in Settings.",
                            plural(missing_workshop)
                        ))
                        .small()
                        .color(theme::WARNING),
                    );
                }
            }
        });

        let has_configs = !pack.manifest.configs.is_empty();
        let has_saves = !pack.manifest.saves.is_empty();

        if has_configs || has_saves {
            ui.add_space(10.0);
            theme::group().show(ui, |ui| {
                ui.set_width(ui.available_width());

                if has_configs {
                    theme::checkbox(
                        ui,
                        &mut options.restore_configs,
                        format!(
                            "Restore {} mod settings file{}",
                            pack.manifest.configs.len(),
                            plural(pack.manifest.configs.len())
                        ),
                    );
                }

                if has_saves {
                    theme::checkbox(
                        ui,
                        &mut options.restore_saves,
                        format!(
                            "Restore {} save file{}",
                            pack.manifest.saves.len(),
                            plural(pack.manifest.saves.len())
                        ),
                    );

                    if options.restore_saves {
                        ui.indent("overwrite_saves_indent", |ui| {
                            theme::checkbox(
                                ui,
                                &mut options.overwrite_saves,
                                "Replace existing saves with the same name",
                            );
                            theme::hint(
                                ui,
                                "Off by default so applying cannot overwrite newer progress.",
                            );
                        });
                    }
                }
            });
        }

        ui.add_space(8.0);
        theme::hint(
            ui,
            "ModsConfig.xml is backed up automatically before the load order changes.",
        );
        ui.add_space(12.0);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if theme::primary_button_sized(
                ui,
                "Apply modpack",
                Vec2::new(0.0, theme::CONTROL_HEIGHT),
            )
            .clicked()
            {
                decision = Some(ApplyDiffDecision::Apply);
            }

            if theme::secondary_button(ui, "Cancel").clicked() {
                decision = Some(ApplyDiffDecision::Cancel);
            }
        });
    });

    if response.should_close() {
        decision = Some(ApplyDiffDecision::Cancel);
    }

    decision
}

/// Full-window translucent overlay shown while dragging `.rimpack` file(s) over
/// the application window.
pub(crate) fn show_drop_overlay(context: &egui::Context, pack_names: &[String]) {
    let screen = context.content_rect();

    egui::Area::new(egui::Id::new("rimpack_drop_overlay"))
        .order(egui::Order::Foreground)
        .interactable(false)
        .fixed_pos(screen.min)
        .show(context, |ui| {
            let painter = ui.painter();
            painter.rect_filled(screen, CornerRadius::ZERO, theme::tint(theme::CANVAS, 200));

            let card_size = Vec2::new(420.0_f32.min(screen.width() - 48.0), 148.0);
            let card_rect = egui::Rect::from_center_size(screen.center(), card_size);

            painter.add(egui::Shape::from(
                theme::soft_shadow().as_shape(card_rect, CornerRadius::ZERO),
            ));
            painter.rect_filled(card_rect, CornerRadius::ZERO, theme::SURFACE);
            painter.rect_stroke(
                card_rect,
                CornerRadius::ZERO,
                Stroke::new(2.0, theme::ACCENT),
                StrokeKind::Inside,
            );

            // Subtle accent top strip inside the card.
            let accent_bar = egui::Rect::from_min_max(
                card_rect.left_top(),
                egui::pos2(card_rect.right(), card_rect.top() + 3.0),
            );
            painter.rect_filled(accent_bar, CornerRadius::ZERO, theme::ACCENT);

            let title = if pack_names.len() > 1 {
                format!("Drop {} modpacks to import", pack_names.len())
            } else {
                "Drop .rimpack to preview & import".to_owned()
            };

            let subtitle = if pack_names.len() == 1 {
                pack_names[0].clone()
            } else if pack_names.len() > 1 {
                let mut preview = pack_names
                    .iter()
                    .take(2)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                if pack_names.len() > 2 {
                    preview.push_str(&format!(" +{} more", pack_names.len() - 2));
                }
                preview
            } else {
                "Choose whether to import to library or apply right away".to_owned()
            };

            painter.text(
                egui::pos2(card_rect.center().x, card_rect.center().y - 26.0),
                egui::Align2::CENTER_CENTER,
                "RIMMOD MODPACK",
                theme::semibold_font(11.0),
                theme::ACCENT,
            );
            painter.text(
                egui::pos2(card_rect.center().x, card_rect.center().y + 2.0),
                egui::Align2::CENTER_CENTER,
                title,
                theme::semibold_font(17.0),
                theme::TEXT,
            );
            painter.text(
                egui::pos2(card_rect.center().x, card_rect.center().y + 30.0),
                egui::Align2::CENTER_CENTER,
                truncate(&subtitle, 52),
                theme::medium_font(12.0),
                theme::TEXT_MUTED,
            );
        });
}

/// Preview modal shown immediately after dropping one or more `.rimpack` files
/// onto the window.
pub(crate) fn show_drop_preview_modal(
    context: &egui::Context,
    view: DropPreviewView<'_>,
) -> Option<DropPreviewDecision> {
    let DropPreviewView {
        items,
        diff,
        game_version,
        api_key_configured,
        steamcmd_available,
    } = view;

    if items.is_empty() {
        return None;
    }

    let mut decision = None;

    let response = egui::Modal::new(egui::Id::new("drop_rimpack_preview")).show(context, |ui| {
        ui.set_width(540.0);

        if items.len() == 1 {
            let item = &items[0];
            let icon_key = hash_path(&item.path);

            ui.horizontal_top(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(Vec2::splat(64.0), Sense::hover());
                show_icon(
                    ui,
                    icon_rect,
                    item.icon.as_ref(),
                    &item.manifest.name,
                    icon_key,
                );

                ui.add_space(12.0);

                ui.vertical(|ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(&item.manifest.name)
                                .text_style(theme::title_style())
                                .color(theme::TEXT),
                        );

                        if let Some(pack_ver) = item.manifest.game_version.as_deref() {
                            let short = major_minor_version_text(pack_ver)
                                .unwrap_or_else(|| pack_ver.to_owned());
                            let mismatch = game_version
                                .is_some_and(|current| !versions_are_compatible(pack_ver, current));
                            theme::chip(
                                ui,
                                format!("RimWorld {short}"),
                                if mismatch {
                                    theme::WARNING
                                } else {
                                    theme::TEXT_MUTED
                                },
                            );
                        }
                    });

                    let (workshop, local, official) = source_counts(&item.manifest);
                    theme::hint(
                        ui,
                        format!(
                            "{} mod{} ({}), {}",
                            item.manifest.mods.len(),
                            plural(item.manifest.mods.len()),
                            summary_text(workshop, local, official, item.manifest.saves.len()),
                            format_size(item.file_size),
                        ),
                    );

                    if !item.manifest.tags.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal_wrapped(|ui| {
                            for tag in &item.manifest.tags {
                                theme::chip(ui, tag.clone(), theme::ACCENT);
                            }
                        });
                    }
                });
            });

            if !item.manifest.description.trim().is_empty() {
                ui.add_space(8.0);
                theme::group().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.add(
                        egui::Label::new(
                            RichText::new(truncate(item.manifest.description.trim(), 280))
                                .color(theme::TEXT_MUTED),
                        )
                        .wrap(),
                    );
                });
            }

            if let Some(diff) = diff {
                ui.add_space(10.0);
                theme::group().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    section_label(ui, "IF APPLIED NOW");

                    if diff.is_in_sync() {
                        theme::hint(ui, "Matches your active load order.");
                    } else {
                        show_diff_summary_chips(ui, diff);
                        ui.add_space(6.0);
                        egui::ScrollArea::vertical()
                            .id_salt("drop_preview_diff_scroll")
                            .max_height(150.0)
                            .auto_shrink([false, true])
                            .show(ui, |ui| {
                                show_diff_sections(ui, diff, false, None);
                            });
                    }

                    let missing_workshop = diff.missing_workshop_count();
                    if missing_workshop > 0 && !api_key_configured && !steamcmd_available {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(format!(
                                "{missing_workshop} missing Workshop mod{} need SteamCMD or a Steam Web API key to auto-download.",
                                plural(missing_workshop)
                            ))
                            .small()
                            .color(theme::WARNING),
                        );
                    }
                });
            }

            if item.already_in_library {
                ui.add_space(8.0);
                theme::hint(
                    ui,
                    format!(
                        "A modpack named \"{}\" is already in your library; importing will keep both.",
                        item.manifest.name
                    ),
                );
            }

            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::primary_button_sized(
                    ui,
                    "Import & Apply",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    decision = Some(DropPreviewDecision::ImportAndApply);
                }

                if theme::secondary_button_sized(
                    ui,
                    "Import to library",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    decision = Some(DropPreviewDecision::ImportOnly);
                }

                if theme::secondary_button(ui, "Cancel").clicked() {
                    decision = Some(DropPreviewDecision::Cancel);
                }
            });
        } else {
            ui.label(
                RichText::new(format!("Import {} modpacks?", items.len()))
                    .text_style(theme::title_style())
                    .color(theme::TEXT),
            );
            ui.add_space(8.0);

            theme::group().show(ui, |ui| {
                ui.set_width(ui.available_width());
                egui::ScrollArea::vertical()
                    .id_salt("drop_multi_preview")
                    .max_height(240.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 6.0;
                        for item in items {
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(&item.manifest.name)
                                        .text_style(theme::strong_style())
                                        .color(theme::TEXT),
                                );
                                theme::hint(
                                    ui,
                                    format!(
                                        "{} mod{}, {}",
                                        item.manifest.mods.len(),
                                        plural(item.manifest.mods.len()),
                                        format_size(item.file_size)
                                    ),
                                );
                            });
                        }
                    });
            });

            ui.add_space(14.0);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::primary_button_sized(
                    ui,
                    &format!("Import all ({})", items.len()),
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    decision = Some(DropPreviewDecision::ImportOnly);
                }

                if theme::secondary_button(ui, "Cancel").clicked() {
                    decision = Some(DropPreviewDecision::Cancel);
                }
            });
        }
    });

    if response.should_close() {
        decision = Some(DropPreviewDecision::Cancel);
    }

    decision
}

pub(crate) fn show_pack_editor(
    context: &egui::Context,
    view: PackEditorView<'_>,
) -> PackEditorOutcome {
    let PackEditorView {
        mode,
        name,
        description,
        tags_input,
        mod_search,
        icon,
        icon_generation,
        icon_drop_hovered,
        mods,
        excluded_mods,
        config_files,
        excluded_configs,
        saves,
        selected_saves,
        game_version,
        update_diff,
    } = view;

    let mut action = None;
    let mut done = false;

    let heading = match mode {
        PackEditorMode::Create => "New modpack",
        PackEditorMode::EditDetails => "Edit modpack details",
        PackEditorMode::UpdateFromActive => "Update modpack from active mods",
    };

    let subtitle = match mode {
        PackEditorMode::Create => {
            "Snapshot your active load order. Workshop mods are saved as links; local mods, settings and selected saves are copied into the pack."
        }
        PackEditorMode::EditDetails => {
            "Change metadata or trim mods, settings and saves inside this pack. Its mod list is kept as-is."
        }
        PackEditorMode::UpdateFromActive => {
            "Saving replaces this pack's contents with your currently active mods, settings and selected saves."
        }
    };

    let save_label = match mode {
        PackEditorMode::Create => "Save modpack",
        PackEditorMode::EditDetails => "Save details",
        PackEditorMode::UpdateFromActive => "Update modpack",
    };

    let response = egui::Modal::new(egui::Id::new("modpack_editor")).show(context, |ui| {
        ui.set_width(620.0);

        ui.heading(heading);
        ui.add_space(4.0);
        theme::hint(ui, subtitle);

        if let Some(diff) = update_diff {
            ui.add_space(8.0);
            egui::Frame::new()
                .fill(theme::CANVAS)
                .inner_margin(Margin::symmetric(10, 6))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    if diff.is_in_sync() {
                        theme::hint(ui, "Active mod list matches the saved pack (metadata, settings or saves can still be updated).");
                    } else {
                        ui.horizontal_wrapped(|ui| {
                            theme::hint(ui, "Changes vs saved pack:");
                            if !diff.removed.is_empty() {
                                theme::chip(
                                    ui,
                                    format!("+{} added", diff.removed.len()),
                                    theme::SUCCESS,
                                );
                            }
                            if !diff.added.is_empty() {
                                theme::chip(
                                    ui,
                                    format!("-{} removed", diff.added.len()),
                                    theme::DANGER,
                                );
                            }
                            if !diff.reordered.is_empty() {
                                theme::chip(
                                    ui,
                                    format!("~{} reordered", diff.reordered.len()),
                                    theme::ACCENT,
                                );
                            }
                        });
                    }
                });
        }

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
                            if icon_drop_hovered {
                                "Drop image"
                            } else {
                                "No icon"
                            },
                            theme::medium_font(12.0),
                            if icon_drop_hovered {
                                theme::ACCENT
                            } else {
                                theme::TEXT_FAINT
                            },
                        );
                    }

                    let border = if icon_drop_hovered {
                        Stroke::new(2.0, theme::ACCENT)
                    } else {
                        Stroke::new(1.0, theme::tint(theme::TEXT_FAINT, 40))
                    };
                    ui.painter().rect_stroke(
                        icon_rect,
                        CornerRadius::same(theme::RADIUS_MD),
                        border,
                        StrokeKind::Inside,
                    );

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
                            "PNG, JPEG or GIF (resized to 512×512). You can also drag & drop an image onto this dialog.",
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
                    "{included_mod_count} mod{} ({local} local, {workshop} Workshop), {included_config_count} setting{}, {selected_save_count} save{}",
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

                theme::collapsible(
                    ui,
                    "modpack_editor_mods",
                    included_mods_title,
                    mode != PackEditorMode::EditDetails,
                    |ui| {
                        ui.horizontal(|ui| {
                            ui.allocate_ui(Vec2::new(240.0, theme::CONTROL_HEIGHT), |ui| {
                                theme::search_field(ui, mod_search, "Filter mods…");
                            });

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
                        });

                        ui.add_space(4.0);
                        let query = mod_search.trim().to_ascii_lowercase();

                        for pack_mod in mods {
                            if !query.is_empty()
                                && !pack_mod.name.to_ascii_lowercase().contains(&query)
                                && !pack_mod.package_id.to_ascii_lowercase().contains(&query)
                            {
                                continue;
                            }

                            show_editor_mod_row(ui, pack_mod, excluded_mods);
                        }
                    },
                );

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

                            if theme::checkbox(ui, &mut included, relative_path.as_str()).changed()
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

                theme::collapsible(
                    ui,
                    "modpack_editor_saves",
                    saves_title,
                    !saves.is_empty() && mode != PackEditorMode::EditDetails,
                    |ui| {
                        if saves.is_empty() {
                            theme::hint(
                                ui,
                                if mode == PackEditorMode::EditDetails {
                                    "No saves are included in this modpack."
                                } else {
                                    "No .rws files were found in RimWorld's Saves folder."
                                },
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
                                    if theme::checkbox(ui, &mut included, file_name.as_str())
                                        .changed()
                                    {
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
                        }
                    },
                );
            });

        ui.add_space(10.0);
        theme::hint(
            ui,
            match mode {
                PackEditorMode::Create => {
                    "Saved in RimMod's library. Use Export to create a shareable .rimpack file."
                }
                PackEditorMode::EditDetails | PackEditorMode::UpdateFromActive => {
                    "Changes are staged first and swapped in atomically so the existing pack is never half-written."
                }
            },
        );
        ui.add_space(6.0);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let can_save = !name.trim().is_empty();

            if ui
                .add_enabled_ui(can_save, |ui| {
                    theme::primary_button_sized(ui, save_label, Vec2::new(0.0, theme::CONTROL_HEIGHT))
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

fn handle_library_keyboard_nav(
    ui: &egui::Ui,
    packs: &[PackSummary],
    visible_indices: &[usize],
    selected: Option<&Path>,
    action: &mut Option<ModpacksAction>,
) {
    let current_pos = selected.and_then(|sel| {
        visible_indices
            .iter()
            .position(|&index| same_path(&packs[index].path, sel))
    });

    let (up, down, enter) = ui.ctx().input(|input| {
        (
            input.key_pressed(egui::Key::ArrowUp),
            input.key_pressed(egui::Key::ArrowDown),
            input.key_pressed(egui::Key::Enter),
        )
    });

    if up {
        let next = match current_pos {
            Some(pos) if pos > 0 => visible_indices[pos - 1],
            Some(_) => visible_indices[0],
            None => visible_indices[0],
        };
        *action = Some(ModpacksAction::Select(packs[next].path.clone()));
    } else if down {
        let next = match current_pos {
            Some(pos) if pos + 1 < visible_indices.len() => visible_indices[pos + 1],
            Some(_) => *visible_indices.last().unwrap_or(&visible_indices[0]),
            None => visible_indices[0],
        };
        *action = Some(ModpacksAction::Select(packs[next].path.clone()));
    } else if enter && let Some(pos) = current_pos {
        *action = Some(ModpacksAction::Apply(
            packs[visible_indices[pos]].path.clone(),
        ));
    }
}

fn collect_library_tags(packs: &[PackSummary]) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();

    for pack in packs {
        for tag in &pack.manifest.tags {
            let key = tag.to_ascii_lowercase();
            counts
                .entry(key)
                .and_modify(|(_, count)| *count += 1)
                .or_insert_with(|| (tag.clone(), 1));
        }
    }

    let mut result: Vec<(String, usize)> = counts.into_values().collect();
    result.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.to_lowercase().cmp(&right.0.to_lowercase()))
    });
    result
}

fn filtered_and_sorted_indices(packs: &[PackSummary], ui_state: &ModpacksUiState) -> Vec<usize> {
    let query = ui_state.search.trim().to_ascii_lowercase();

    let mut indices: Vec<usize> = packs
        .iter()
        .enumerate()
        .filter(|(_, pack)| {
            if let Some(tag_filter) = ui_state.tag_filter.as_deref()
                && !pack
                    .manifest
                    .tags
                    .iter()
                    .any(|tag| tag.eq_ignore_ascii_case(tag_filter))
            {
                return false;
            }

            if query.is_empty() {
                return true;
            }

            pack.manifest.name.to_ascii_lowercase().contains(&query)
                || pack
                    .manifest
                    .description
                    .to_ascii_lowercase()
                    .contains(&query)
                || pack
                    .manifest
                    .tags
                    .iter()
                    .any(|tag| tag.to_ascii_lowercase().contains(&query))
        })
        .map(|(index, _)| index)
        .collect();

    match ui_state.sort {
        PackSort::Name => {
            indices.sort_by(|&left, &right| {
                packs[left]
                    .manifest
                    .name
                    .to_lowercase()
                    .cmp(&packs[right].manifest.name.to_lowercase())
            });
        }
        PackSort::Newest => {
            indices.sort_by(|&left, &right| {
                packs[right]
                    .manifest
                    .created_at
                    .cmp(&packs[left].manifest.created_at)
                    .then_with(|| {
                        packs[left]
                            .manifest
                            .name
                            .to_lowercase()
                            .cmp(&packs[right].manifest.name.to_lowercase())
                    })
            });
        }
        PackSort::MostMods => {
            indices.sort_by(|&left, &right| {
                packs[right]
                    .manifest
                    .mods
                    .len()
                    .cmp(&packs[left].manifest.mods.len())
                    .then_with(|| {
                        packs[left]
                            .manifest
                            .name
                            .to_lowercase()
                            .cmp(&packs[right].manifest.name.to_lowercase())
                    })
            });
        }
    }

    indices
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

    ui.horizontal(|ui| {
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

        ui.add(
            egui::Image::new(pack_source_icon(&pack_mod.source))
                .fit_to_exact_size(Vec2::splat(14.0)),
        )
        .on_hover_text(source_tooltip(pack_mod));

        ui.label(
            RichText::new(pack_mod.package_id.as_str())
                .monospace()
                .small()
                .color(theme::TEXT_FAINT),
        );
    });
}

fn pack_source_icon(source: &PackSource) -> egui::ImageSource<'static> {
    match source {
        PackSource::Official => icons::mod_source_icon(&ModSource::Official),
        PackSource::Workshop { workshop_id } => icons::mod_source_icon(&ModSource::SteamWorkshop {
            workshop_id: *workshop_id,
        }),
        PackSource::Local {
            remote_url: Some(remote_url),
            ..
        } => icons::mod_source_icon(&ModSource::Git {
            remote_url: remote_url.clone(),
        }),
        PackSource::Local { .. } => icons::mod_source_icon(&ModSource::Local),
    }
}

fn compact_pack_meta(pack: &PackSummary, game_version: Option<&str>) -> String {
    let mut parts = vec![format!(
        "{} mod{}",
        pack.manifest.mods.len(),
        plural(pack.manifest.mods.len())
    )];

    if !pack.manifest.configs.is_empty() {
        parts.push(format!("{} cfg", pack.manifest.configs.len()));
    }

    if !pack.manifest.saves.is_empty() {
        parts.push(format!(
            "{} save{}",
            pack.manifest.saves.len(),
            plural(pack.manifest.saves.len())
        ));
    }

    if let Some(pack_ver) = pack.manifest.game_version.as_deref()
        && let Some(short) = major_minor_version_text(pack_ver)
    {
        let mismatch =
            game_version.is_some_and(|current| !versions_are_compatible(pack_ver, current));
        if mismatch {
            parts.push(format!("{short} !"));
        } else {
            parts.push(short);
        }
    }

    parts.join(", ")
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
                .rect_filled(rect, CornerRadius::same(theme::RADIUS_MD), theme::SURFACE);

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
    color: Color32,
    max_width: f32,
) {
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let clipped = ui.painter().with_clip_rect(egui::Rect::from_min_size(
        position,
        Vec2::new(max_width, galley.size().y),
    ));

    clipped.galley(position, galley, color);
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let mix = |x: u8, y: u8| -> u8 { ((x as f32) + ((y as f32) - (x as f32)) * t).round() as u8 };
    Color32::from_rgba_unmultiplied(
        mix(a.r(), b.r()),
        mix(a.g(), b.g()),
        mix(a.b(), b.b()),
        mix(a.a(), b.a()),
    )
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

fn hash_path(path: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    path.hash(&mut hasher);
    hasher.finish()
}

fn source_counts(manifest: &ModpackManifest) -> (usize, usize, usize) {
    let mut workshop = 0;
    let mut local = 0;
    let mut official = 0;

    for pack_mod in &manifest.mods {
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
        parts.join(", ")
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

        let mods_collection = ModCollection::default();
        let mut ui_state = ModpacksUiState::default();
        let mut name = "Test pack".to_owned();
        let mut description = String::new();
        let mut tags = String::new();
        let mut mod_search = String::new();
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
                        active_pack: None,
                        mods: &mods_collection,
                        game_version: Some("1.7.5014"),
                        library_error: None,
                        ui_state: &mut ui_state,
                        modal_open: false,
                    },
                );

                assert!(action.is_none());

                let outcome = show_pack_editor(
                    ui.ctx(),
                    PackEditorView {
                        mode: PackEditorMode::Create,
                        name: &mut name,
                        description: &mut description,
                        tags_input: &mut tags,
                        mod_search: &mut mod_search,
                        icon: None,
                        icon_generation: 0,
                        icon_drop_hovered: false,
                        mods: &mods,
                        excluded_mods: &mut excluded_mods,
                        config_files: &config_files,
                        excluded_configs: &mut excluded_configs,
                        saves: &saves,
                        selected_saves: &mut selected_saves,
                        game_version: None,
                        update_diff: None,
                    },
                );

                assert!(outcome.open);
                assert!(outcome.action.is_none());
            })
            .drop_without_applying_deltas();
    }

    #[test]
    fn populated_library_tabs_and_modals_render() {
        use crate::models::{ModId, ModType, PackageId, RimworldMod};

        let context = egui::Context::default();
        theme::install(&context);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..egui::RawInput::default()
        };

        let installed = vec![
            RimworldMod {
                name: "Core".to_owned(),
                package_id: PackageId::new("ludeon.rimworld").unwrap(),
                description: String::new(),
                supported_versions: vec!["1.7".to_owned()],
                community_supported_versions: Vec::new(),
                loader_after: Vec::new(),
                loader_before: Vec::new(),
                force_load_after: Vec::new(),
                force_load_before: Vec::new(),
                mod_dependencies: Vec::new(),
                incompatible_with: Vec::new(),
                inferred_after: Vec::new(),
                is_patch_only: false,
                folder: PathBuf::from("Core"),
                source: ModSource::Official,
                mod_type: ModType::Xml,
            },
            RimworldMod {
                name: "Harmony".to_owned(),
                package_id: PackageId::new("brrainz.harmony").unwrap(),
                description: String::new(),
                supported_versions: vec!["1.7".to_owned()],
                community_supported_versions: Vec::new(),
                loader_after: Vec::new(),
                loader_before: Vec::new(),
                force_load_after: Vec::new(),
                force_load_before: Vec::new(),
                mod_dependencies: Vec::new(),
                incompatible_with: Vec::new(),
                inferred_after: Vec::new(),
                is_patch_only: false,
                folder: PathBuf::from("Harmony"),
                source: ModSource::SteamWorkshop {
                    workshop_id: 2009463077,
                },
                mod_type: ModType::CSharp,
            },
        ];
        let mods_collection = ModCollection::new(
            installed,
            Vec::new(),
            vec![ModId::from_index(1), ModId::from_index(0)],
            Vec::new(),
        );

        let mut active_manifest = ModpackManifest::new("Cozy Vanilla+");
        active_manifest.description = "Balanced QoL pack".to_owned();
        active_manifest.tags = vec!["Vanilla+".to_owned(), "QoL".to_owned()];
        active_manifest.game_version = Some("1.7.5014".to_owned());
        active_manifest.configs = vec!["Mod_Harmony.xml".to_owned()];
        active_manifest.saves = vec!["Colony.rws".to_owned()];
        active_manifest.mods = vec![
            PackMod {
                package_id: "brrainz.harmony".to_owned(),
                name: "Harmony".to_owned(),
                folder_name: String::new(),
                source: PackSource::Workshop {
                    workshop_id: 2009463077,
                },
            },
            PackMod {
                package_id: "ludeon.rimworld".to_owned(),
                name: "Core".to_owned(),
                folder_name: String::new(),
                source: PackSource::Official,
            },
        ];

        let mut modified_manifest = active_manifest.clone();
        modified_manifest.name = "Heavy Modded".to_owned();
        modified_manifest.tags = vec!["Overhaul".to_owned()];
        modified_manifest.mods.push(PackMod {
            package_id: "missing.workshop".to_owned(),
            name: "Missing Mod".to_owned(),
            folder_name: String::new(),
            source: PackSource::Workshop { workshop_id: 999 },
        });

        let packs = vec![
            PackSummary {
                path: PathBuf::from("packs/cozy"),
                manifest: active_manifest.clone(),
                icon: None,
                size_bytes: 2_400_000,
            },
            PackSummary {
                path: PathBuf::from("packs/heavy"),
                manifest: modified_manifest.clone(),
                icon: None,
                size_bytes: 18_500_000,
            },
        ];

        let diff = modpack::compute_pack_diff(&modified_manifest, &mods_collection);
        let mut options = PackApplyOptions::default();
        let drop_items = vec![DropPreviewItem {
            path: PathBuf::from("downloads/heavy.rimpack"),
            file_size: 4_200_000,
            manifest: modified_manifest.clone(),
            icon: None,
            already_in_library: true,
        }];

        for tab in [
            PackDetailTab::Mods,
            PackDetailTab::Diff,
            PackDetailTab::Settings,
            PackDetailTab::Saves,
        ] {
            let mut ui_state = ModpacksUiState {
                detail_tab: tab,
                ..ModpacksUiState::default()
            };

            context
                .run_ui(input.clone(), |ui| {
                    let action = show_modpacks(
                        ui,
                        ModpacksView {
                            packs: &packs,
                            selected: Some(Path::new("packs/heavy")),
                            active_pack: Some(Path::new("packs/heavy")),
                            mods: &mods_collection,
                            game_version: Some("1.7.5014"),
                            library_error: None,
                            ui_state: &mut ui_state,
                            modal_open: false,
                        },
                    );
                    assert!(action.is_none());
                })
                .drop_without_applying_deltas();
        }

        context
            .run_ui(input, |ui| {
                let apply_decision = show_apply_diff_modal(
                    ui.ctx(),
                    ApplyDiffView {
                        pack: &packs[1],
                        diff: &diff,
                        game_version: Some("1.7.5014"),
                        api_key_configured: true,
                        steamcmd_available: false,
                        options: &mut options,
                    },
                );
                assert!(apply_decision.is_none());

                show_drop_overlay(ui.ctx(), &["heavy.rimpack".to_owned()]);

                let drop_decision = show_drop_preview_modal(
                    ui.ctx(),
                    DropPreviewView {
                        items: &drop_items,
                        diff: Some(&diff),
                        game_version: Some("1.7.5014"),
                        api_key_configured: true,
                        steamcmd_available: false,
                    },
                );
                assert!(drop_decision.is_none());
            })
            .drop_without_applying_deltas();
    }
}

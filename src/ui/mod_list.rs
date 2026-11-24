use std::collections::{HashMap, HashSet};

use eframe::egui::{self, Atom, AtomExt, Atoms, RichText};
use nucleo_matcher::Matcher;

use crate::{
    models::{
        ModCollection, ModId, ModSource, RimworldMod, VersionSupport, major_minor_version_text,
        newest_known_version, version_support,
    },
    services::{
        fuzzy_search::fuzzy_mod_indices, mod_sorter::OrderWarning, organization::Organization,
    },
    ui::{
        icons::{
            INLINE_ICON_SCALE, incompatible_icon, mod_source_icon, mod_type_icon,
            no_version_warning_icon, warning_icon,
        },
        mod_filters::short_tag,
        mod_info::show_version_chips,
        theme,
    },
};

/// Tags visible on one row before collapsing the rest into "+N".
const MAX_ROW_TAGS: usize = 2;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ModListKind {
    Enabled,
    Disabled,
}

impl ModListKind {
    fn id(self) -> &'static str {
        match self {
            Self::Enabled => "active_mods",
            Self::Disabled => "disabled_mods",
        }
    }

    fn heading(self) -> &'static str {
        match self {
            Self::Enabled => "Active mods",
            Self::Disabled => "Disabled mods",
        }
    }

    fn other(self) -> Self {
        match self {
            Self::Enabled => Self::Disabled,
            Self::Disabled => Self::Enabled,
        }
    }
}

pub(crate) enum ModListAction {
    Select {
        mod_id: ModId,
        kind: ModListKind,
        mode: SelectionMode,
    },
    Transfer {
        mod_ids: Vec<ModId>,
        from: ModListKind,
        to: ModListKind,
        before_mod_id: Option<ModId>,
    },
    Reorder {
        mod_ids: Vec<ModId>,
        kind: ModListKind,
        before_mod_id: Option<ModId>,
    },
    Delete {
        mod_ids: Vec<ModId>,
    },
    Organize {
        mod_ids: Vec<ModId>,
    },
    Unsubscribe {
        published_file_ids: Vec<u64>,
    },
}

pub(crate) enum SelectionMode {
    Replace,
    Toggle,
    Range(Vec<ModId>),
}

/// The small owned value egui carries while one or more mod rows are dragged.
#[derive(Clone, PartialEq, Eq)]
struct DraggedMods {
    mod_ids: Vec<ModId>,
    from: ModListKind,
}

fn action_for_drop(
    dragged_mods: &DraggedMods,
    to: ModListKind,
    before_mod_id: Option<ModId>,
) -> ModListAction {
    if dragged_mods.from == to {
        ModListAction::Reorder {
            mod_ids: dragged_mods.mod_ids.clone(),
            kind: to,
            before_mod_id,
        }
    } else {
        ModListAction::Transfer {
            mod_ids: dragged_mods.mod_ids.clone(),
            from: dragged_mods.from,
            to,
            before_mod_id,
        }
    }
}

fn selection_range(
    visible_mod_ids: &[ModId],
    anchor_mod_id: Option<ModId>,
    clicked_mod_id: ModId,
) -> Option<Vec<ModId>> {
    let anchor_position = visible_mod_ids
        .iter()
        .position(|&mod_id| Some(mod_id) == anchor_mod_id)?;
    let clicked_position = visible_mod_ids
        .iter()
        .position(|&mod_id| mod_id == clicked_mod_id)?;
    let first = anchor_position.min(clicked_position);
    let last = anchor_position.max(clicked_position);

    Some(visible_mod_ids[first..=last].to_vec())
}

/// Workshop subscriptions in the selection that can actually be removed.
/// When the account's subscriptions are known, items installed without a
/// subscription (for example through SteamCMD) are left out.
fn unsubscribe_workshop_ids(
    mods: &ModCollection,
    mod_ids: &[ModId],
    subscribed_ids: Option<&HashSet<u64>>,
) -> Vec<u64> {
    let workshop_ids = mod_ids
        .iter()
        .filter_map(|&mod_id| mods.get(mod_id))
        .filter_map(|rimworld_mod| match &rimworld_mod.source {
            ModSource::SteamWorkshop { workshop_id } => Some(*workshop_id),
            _ => None,
        });

    match subscribed_ids {
        Some(subscribed_ids) => workshop_ids
            .filter(|workshop_id| subscribed_ids.contains(workshop_id))
            .collect(),
        None => workshop_ids.collect(),
    }
}

fn order_warning_text(warning: &OrderWarning, all_mods: &[RimworldMod]) -> String {
    let mut lines = Vec::new();

    if !warning.incompatible_mod_ids.is_empty() {
        let mod_names = warning
            .incompatible_mod_ids
            .iter()
            .map(|&mod_id| all_mods[mod_id.index()].name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        lines.push(format!("Incompatible with: {mod_names}"));
    }

    if !warning.after_mod_ids.is_empty() {
        let mod_names = warning
            .after_mod_ids
            .iter()
            .map(|&mod_id| all_mods[mod_id.index()].name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        lines.push(format!("Move below: {mod_names}"));
    }

    if !warning.before_mod_ids.is_empty() {
        let mod_names = warning
            .before_mod_ids
            .iter()
            .map(|&mod_id| all_mods[mod_id.index()].name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        lines.push(format!("Move above: {mod_names}"));
    }

    lines.join("\n")
}

#[derive(Debug, PartialEq)]
struct VersionBadge {
    text: String,
    color: egui::Color32,
    community_report: bool,
    matches_game_version: bool,
}

fn version_badge_tooltip(badge: &VersionBadge) -> String {
    match (badge.community_report, badge.matches_game_version) {
        (true, true) => format!(
            "No Version Warning: reported compatible with RimWorld {}",
            badge.text
        ),
        (true, false) => format!(
            "No Version Warning: reported compatible up to RimWorld {} (does not list your game version)",
            badge.text
        ),
        (false, _) => format!(
            "Lists up to RimWorld {} (does not list your game version)",
            badge.text
        ),
    }
}

fn version_badge(rimworld_mod: &RimworldMod, game_version: Option<&str>) -> Option<VersionBadge> {
    let game_version = game_version?;
    let game_version_text = major_minor_version_text(game_version)?;

    match version_support(
        &rimworld_mod.supported_versions,
        &rimworld_mod.community_supported_versions,
        game_version,
    ) {
        VersionSupport::Official => None,
        VersionSupport::Community => Some(VersionBadge {
            text: game_version_text,
            color: theme::WARNING,
            community_report: true,
            matches_game_version: true,
        }),
        VersionSupport::Unsupported => {
            let (newest_version, community_report) = newest_known_version(
                &rimworld_mod.supported_versions,
                &rimworld_mod.community_supported_versions,
            )?;

            Some(VersionBadge {
                text: newest_version.to_owned(),
                color: theme::TEXT_FAINT,
                community_report,
                matches_game_version: false,
            })
        }
    }
}

pub(crate) struct ModListView<'a> {
    pub(crate) kind: ModListKind,
    pub(crate) mods: &'a ModCollection,
    pub(crate) mod_ids: &'a [ModId],
    pub(crate) game_version: Option<&'a str>,
    pub(crate) order_warnings: &'a HashMap<ModId, OrderWarning>,
    pub(crate) organization: &'a Organization,
    pub(crate) tag_filters: &'a mut Vec<String>,
    pub(crate) search_string: &'a mut String,
    pub(crate) matcher: &'a mut Matcher,
    pub(crate) selected_mod_ids: &'a [ModId],
    pub(crate) selection_anchor_id: Option<ModId>,
    pub(crate) selection_kind: Option<ModListKind>,
    /// Known Steam Workshop subscriptions, or `None` while they load.
    pub(crate) subscribed_workshop_ids: Option<&'a HashSet<u64>>,
    pub(crate) pending_workshop_subscriptions: &'a HashSet<u64>,
    pub(crate) can_unsubscribe: bool,
}

pub(crate) fn show_mod_list(ui: &mut egui::Ui, view: ModListView<'_>) -> Option<ModListAction> {
    let ModListView {
        kind,
        mods,
        mod_ids,
        game_version,
        order_warnings,
        organization,
        tag_filters,
        search_string,
        matcher,
        selected_mod_ids,
        selection_anchor_id,
        selection_kind,
        subscribed_workshop_ids,
        pending_workshop_subscriptions,
        can_unsubscribe,
    } = view;
    let mut requested_action = None;

    let heading = kind.heading();
    let count = mod_ids.len();
    let selected_count = if selection_kind == Some(kind) {
        selected_mod_ids.len()
    } else {
        0
    };

    ui.push_id(kind.id(), |ui| {
        theme::gradient_surface(
            ui,
            theme::SURFACE_HOVER,
            theme::SURFACE,
            egui::Margin::same(12),
            |ui| {
                ui.set_min_height(ui.available_height());

                ui.horizontal(|ui| {
                    theme::title(ui, heading);

                    if selected_count > 1 {
                        theme::chip(ui, format!("{selected_count} selected"), theme::ACCENT);
                    } else {
                        theme::chip(ui, count.to_string(), theme::TEXT_MUTED);
                    }
                });

                ui.add_space(6.0);

                let visible_list_positions =
                    fuzzy_mod_indices(&mods.all, mod_ids, search_string, matcher)
                        .into_iter()
                        .filter(|&list_position| {
                            mods.get(mod_ids[list_position])
                                .is_some_and(|rimworld_mod| {
                                    organization.matches_tags(
                                        &rimworld_mod.package_id,
                                        tag_filters.as_slice(),
                                    )
                                })
                        })
                        .collect::<Vec<_>>();

                let visible_mod_ids = visible_list_positions
                    .iter()
                    .map(|&list_position| mod_ids[list_position])
                    .collect::<Vec<_>>();

                if visible_mod_ids.is_empty() {
                    if !tag_filters.is_empty() {
                        theme::hint(ui, "No mods match these tags");
                    } else if search_string.is_empty() {
                        theme::hint(ui, "No mods in this list yet");
                    } else {
                        theme::hint(ui, "No mods match this search");
                    }
                }

                let row_height = theme::ROW_HEIGHT;

                let list_response = ui
                    .scope(|ui| {
                        // Rows stay flat at rest; only hover, selection and drag paint.
                        let widgets = &mut ui.style_mut().visuals.widgets;
                        widgets.inactive.weak_bg_fill = egui::Color32::TRANSPARENT;
                        widgets.inactive.bg_fill = egui::Color32::TRANSPARENT;
                        widgets.inactive.bg_stroke = egui::Stroke::NONE;

                        ui.spacing_mut().item_spacing.y = 4.0;

                        egui::ScrollArea::vertical()
                            .auto_shrink([false, false])
                            .show_rows(
                                ui,
                                row_height,
                                visible_list_positions.len(),
                                |ui, visible_rows| {
                                    for visible_row in visible_rows {
                                        let list_position = visible_list_positions[visible_row];
                                        let mod_id = mod_ids[list_position];

                                        let Some(rimworld_mod) = mods.get(mod_id) else {
                                            continue;
                                        };

                                        let is_selected = selection_kind == Some(kind)
                                            && selected_mod_ids.contains(&mod_id);

                                        let dragged_mod_ids = if is_selected {
                                            mod_ids
                                                .iter()
                                                .copied()
                                                .filter(|candidate| {
                                                    selected_mod_ids.contains(candidate)
                                                })
                                                .collect()
                                        } else {
                                            vec![mod_id]
                                        };
                                        let dragged_mods = DraggedMods {
                                            mod_ids: dragged_mod_ids,
                                            from: kind,
                                        };

                                        let order_warning = if kind == ModListKind::Enabled {
                                            order_warnings.get(&mod_id)
                                        } else {
                                            None
                                        };

                                        let icon_height = ui
                                            .text_style_height(&egui::TextStyle::Body)
                                            * INLINE_ICON_SCALE;

                                        let source_icon =
                                            egui::Image::new(mod_source_icon(&rimworld_mod.source))
                                                .tint(egui::Color32::from_white_alpha(170))
                                                .atom_max_height(icon_height);

                                        let mut left_side = Vec::<Atom<'static>>::new();
                                        left_side.push(source_icon);

                                        let type_icon = egui::Image::new(mod_type_icon(
                                            &rimworld_mod.mod_type,
                                        ))
                                        .tint(egui::Color32::from_white_alpha(170))
                                        .atom_max_height(icon_height);

                                        left_side.push(type_icon);

                                        left_side.push(rimworld_mod.name.as_str().into());

                                        // One response handles both gestures. egui waits for enough
                                        // pointer movement before classifying the gesture as a drag.

                                        let row_width =
                                            (ui.available_width() - theme::SCROLL_GUTTER).max(80.0);

                                        // The fill is painted by the button, so the state has to be
                                        // known before it is added. The row rect is predictable:
                                        // show_rows starts it at the current cursor.
                                        let row_rect = egui::Rect::from_min_size(
                                            ui.cursor().min,
                                            egui::vec2(row_width, row_height),
                                        );
                                        let hovered = ui.rect_contains_pointer(row_rect);
                                        let hover = ui.ctx().animate_bool_with_time(
                                            ui.id().with(("row_hover", mod_id)),
                                            hovered,
                                            0.12,
                                        );
                                        let selection = ui.ctx().animate_bool_with_time(
                                            ui.id().with(("row_selection", mod_id)),
                                            is_selected,
                                            0.12,
                                        );

                                        let fill = if selection > 0.0 {
                                            theme::tint(
                                                theme::ACCENT,
                                                (55.0 * selection + 20.0 * hover) as u8,
                                            )
                                        } else {
                                            theme::tint(
                                                theme::SURFACE_HOVER,
                                                (hover * 255.0) as u8,
                                            )
                                        };

                                        let mut button = egui::Button::selectable(
                                            is_selected,
                                            Atoms::from(left_side),
                                        )
                                        .fill(fill)
                                        .min_size(egui::vec2(row_width, row_height))
                                        .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD));

                                        let version_badge =
                                            version_badge(rimworld_mod, game_version);
                                        let tags = organization.tags(&rimworld_mod.package_id);
                                        let color = organization.color(&rimworld_mod.package_id);

                                        let mut right_side = Vec::<Atom<'static>>::new();

                                        for tag in tags.iter().take(MAX_ROW_TAGS) {
                                            right_side
                                                .push(theme::tag_text(&short_tag(tag)).into());
                                        }

                                        if tags.len() > MAX_ROW_TAGS {
                                            right_side.push(
                                                theme::tag_overflow_text(tags.len() - MAX_ROW_TAGS)
                                                    .into(),
                                            );
                                        }

                                        if let Some(warning) = order_warning {
                                            if !warning.incompatible_mod_ids.is_empty() {
                                                right_side.push(
                                                    incompatible_icon()
                                                        .atom_max_height(icon_height),
                                                );
                                            }

                                            if !warning.after_mod_ids.is_empty()
                                                || !warning.before_mod_ids.is_empty()
                                            {
                                                right_side.push(
                                                    warning_icon().atom_max_height(icon_height),
                                                );
                                            }
                                        }

                                        if let Some(badge) = &version_badge {
                                            if badge.community_report {
                                                let community_icon = no_version_warning_icon()
                                                    .atom_max_height(icon_height);
                                                right_side.push(community_icon);
                                            }

                                            // Same boxed treatment as the Workshop
                                            // cards, with spaces acting as padding.
                                            let version_text =
                                                RichText::new(format!("  {}  ", badge.text))
                                                    .font(theme::medium_font(11.0))
                                                    .color(badge.color)
                                                    .background_color(theme::tint(badge.color, 34));
                                            right_side.push(version_text.into());
                                        }

                                        if !right_side.is_empty() {
                                            button = button.right_text(Atoms::from(right_side));
                                        }

                                        let response =
                                            ui.add(button.sense(egui::Sense::click_and_drag()));

                                        // The left rail carries the mod's color. A
                                        // selected colored row keeps the accent as a
                                        // second sliver so selection stays visible.
                                        let rails = [
                                            color.map(theme::mod_color),
                                            is_selected.then_some(theme::ACCENT),
                                        ];
                                        let row_rect = response.rect;
                                        let mut rail_left = row_rect.left() + 1.0;

                                        for rail_color in rails.into_iter().flatten() {
                                            let marker = egui::Rect::from_min_max(
                                                egui::pos2(rail_left, row_rect.top() + 4.0),
                                                egui::pos2(
                                                    rail_left + 2.0,
                                                    row_rect.bottom() - 4.0,
                                                ),
                                            );

                                            ui.painter().rect_filled(
                                                marker,
                                                egui::CornerRadius::same(1),
                                                rail_color,
                                            );

                                            rail_left += 2.0;
                                        }

                                        let mut tooltip = Vec::new();

                                        if let Some(badge) = &version_badge {
                                            tooltip.push(version_badge_tooltip(badge));
                                        }

                                        if let Some(warning) = order_warning {
                                            tooltip.push(order_warning_text(warning, &mods.all));
                                        }

                                        if let Some(color) = color {
                                            tooltip.push(format!("Color: {}", color.label()));
                                        }

                                        if !tags.is_empty() {
                                            tooltip.push(format!("Tags: {}", tags.join(", ")));
                                        }

                                        let response = if tooltip.is_empty() {
                                            response
                                        } else {
                                            response.on_hover_text(tooltip.join("\n\n"))
                                        };

                                        egui::Popup::context_menu(&response)
                                            .frame(theme::menu_frame())
                                            .show(|ui| {
                                                ui.set_min_width(240.0);
                                                ui.set_max_width(320.0);
                                                ui.spacing_mut().item_spacing.y = 4.0;
                                                ui.spacing_mut().button_padding =
                                                    egui::vec2(8.0, 4.0);

                                                theme::menu_inset(ui, |ui| {
                                                    if dragged_mods.mod_ids.len() > 1 {
                                                        ui.label(
                                                            RichText::new(format!(
                                                                "{} mods selected",
                                                                dragged_mods.mod_ids.len()
                                                            ))
                                                            .text_style(theme::strong_style()),
                                                        );

                                                        theme::hint(
                                                            ui,
                                                            "Actions apply to the whole selection",
                                                        );
                                                    } else {
                                                        ui.label(
                                                            RichText::new(&rimworld_mod.name)
                                                                .text_style(theme::strong_style()),
                                                        );

                                                        ui.label(
                                                            RichText::new(
                                                                rimworld_mod.package_id.as_str(),
                                                            )
                                                            .monospace()
                                                            .small()
                                                            .color(theme::TEXT_MUTED),
                                                        );

                                                        if !rimworld_mod
                                                            .supported_versions
                                                            .is_empty()
                                                            || !rimworld_mod
                                                                .community_supported_versions
                                                                .is_empty()
                                                        {
                                                            ui.add_space(2.0);

                                                            ui.horizontal_wrapped(|ui| {
                                                                theme::hint(ui, "Supports");
                                                                show_version_chips(
                                                                    ui,
                                                                    &rimworld_mod
                                                                        .supported_versions,
                                                                    &rimworld_mod
                                                                        .community_supported_versions,
                                                                    game_version,
                                                                );
                                                            });
                                                        }
                                                    }
                                                });

                                                theme::menu_separator(ui);

                                                let transfer_label =
                                                    match (kind, dragged_mods.mod_ids.len()) {
                                                        (ModListKind::Enabled, 1) => {
                                                            "Disable".to_owned()
                                                        }
                                                        (ModListKind::Disabled, 1) => {
                                                            "Enable".to_owned()
                                                        }
                                                        (ModListKind::Enabled, count) => {
                                                            format!("Disable {count} mods")
                                                        }
                                                        (ModListKind::Disabled, count) => {
                                                            format!("Enable {count} mods")
                                                        }
                                                    };

                                                if ui.button(transfer_label).clicked() {
                                                    requested_action =
                                                        Some(ModListAction::Transfer {
                                                            mod_ids: dragged_mods.mod_ids.clone(),
                                                            from: kind,
                                                            to: kind.other(),
                                                            before_mod_id: None,
                                                        });

                                                    ui.close();
                                                }

                                                if ui.button("Tags & color…").clicked() {
                                                    requested_action =
                                                        Some(ModListAction::Organize {
                                                            mod_ids: dragged_mods.mod_ids.clone(),
                                                        });

                                                    ui.close();
                                                }

                                                let unsubscribe_ids = unsubscribe_workshop_ids(
                                                    mods,
                                                    &dragged_mods.mod_ids,
                                                    subscribed_workshop_ids,
                                                );

                                                if !unsubscribe_ids.is_empty() {
                                                    theme::menu_separator(ui);

                                                    let label = match unsubscribe_ids.len() {
                                                        1 => "Unsubscribe".to_owned(),
                                                        count => {
                                                            format!(
                                                                "Unsubscribe {count} Workshop mods"
                                                            )
                                                        }
                                                    };
                                                    let pending = unsubscribe_ids.iter().any(
                                                        |workshop_id| {
                                                            pending_workshop_subscriptions
                                                                .contains(workshop_id)
                                                        },
                                                    );
                                                    let unsubscribe = ui.add_enabled(
                                                        can_unsubscribe && !pending,
                                                        egui::Button::new(
                                                            RichText::new(label)
                                                                .color(theme::DANGER),
                                                        ),
                                                    );

                                                    let unsubscribe =
                                                        if can_unsubscribe && !pending {
                                                            unsubscribe.on_hover_text(
                                                                "Unsubscribe on Steam. Its files are removed after RimWorld closes.",
                                                            )
                                                        } else if pending {
                                                            unsubscribe
                                                                .on_disabled_hover_text(
                                                                    "A subscription change is already running",
                                                                )
                                                        } else {
                                                            unsubscribe.on_disabled_hover_text(
                                                                "Add a Steam Web API key in Settings to unsubscribe",
                                                            )
                                                        };

                                                    if unsubscribe.clicked() {
                                                        requested_action =
                                                            Some(ModListAction::Unsubscribe {
                                                                published_file_ids:
                                                                    unsubscribe_ids,
                                                            });
                                                        ui.close();
                                                    }
                                                }

                                                theme::menu_separator(ui);

                                                let can_delete =
                                                    dragged_mods.mod_ids.iter().all(|&mod_id| {
                                                        mods.get(mod_id).is_some_and(
                                                            |rimworld_mod| {
                                                                !matches!(
                                                                    rimworld_mod.source,
                                                                    ModSource::Official
                                                                )
                                                            },
                                                        )
                                                    });
                                                let delete_label = match dragged_mods.mod_ids.len()
                                                {
                                                    1 => "Delete".to_owned(),
                                                    count => format!("Delete {count} mods"),
                                                };
                                                let delete_button = if can_delete {
                                                    egui::Button::new(
                                                        egui::RichText::new(delete_label)
                                                            .color(theme::DANGER),
                                                    )
                                                } else {
                                                    egui::Button::new(delete_label)
                                                };

                                                if ui
                                                    .add_enabled(can_delete, delete_button)
                                                    .clicked()
                                                {
                                                    requested_action =
                                                        Some(ModListAction::Delete {
                                                            mod_ids: dragged_mods.mod_ids.clone(),
                                                        });
                                                    ui.close();
                                                }

                                                if !can_delete {
                                                    theme::menu_inset(ui, |ui| {
                                                        theme::hint(
                                                            ui,
                                                            "Official content cannot be deleted",
                                                        );
                                                    });
                                                }

                                                // ui.separator();

                                                // TODO: Make check for the Git
                                                // ui.add_enabled_ui(false, |ui| {
                                                //     ui.button("Git: Update").on_disabled_hover_ui(|ui| {
                                                //         ui.style_mut().interaction.selectable_labels = true;
                                                //         ui.label("Requires Git to be installed");
                                                //         ui.hyperlink_to(
                                                //             "Download Git",
                                                //             "https://git-scm.com/install/windows",
                                                //         );
                                                //     });
                                                // });
                                            });

                                        response.dnd_set_drag_payload(dragged_mods.clone());

                                        let was_activated =
                                            response.double_clicked() || response.triple_clicked();

                                        if is_selected && was_activated {
                                            requested_action = Some(ModListAction::Transfer {
                                                mod_ids: dragged_mods.mod_ids.clone(),
                                                from: kind,
                                                to: kind.other(),
                                                before_mod_id: None,
                                            });
                                        } else if response.clicked() {
                                            let modifiers = ui.input(|input| input.modifiers);
                                            let mode = if modifiers.shift {
                                                selection_range(
                                                    &visible_mod_ids,
                                                    selection_anchor_id
                                                        .filter(|_| selection_kind == Some(kind)),
                                                    mod_id,
                                                )
                                                .map_or(
                                                    SelectionMode::Replace,
                                                    SelectionMode::Range,
                                                )
                                            } else if modifiers.command {
                                                SelectionMode::Toggle
                                            } else {
                                                SelectionMode::Replace
                                            };

                                            requested_action =
                                                Some(ModListAction::Select { mod_id, kind, mode });
                                        } else if response.secondary_clicked() && !is_selected {
                                            requested_action = Some(ModListAction::Select {
                                                mod_id,
                                                kind,
                                                mode: SelectionMode::Replace,
                                            });
                                        }

                                        let pointer_position =
                                            ui.input(|input| input.pointer.interact_pos());
                                        let hovered_mods =
                                            response.dnd_hover_payload::<DraggedMods>();

                                        if let (Some(pointer_position), Some(hovered_mods)) =
                                            (pointer_position, hovered_mods)
                                        {
                                            let row_rect = response.rect;
                                            let insertion_stroke = egui::Stroke::new(
                                                2.0,
                                                ui.visuals().selection.stroke.color,
                                            );

                                            let before_mod_id = if hovered_mods.from == kind
                                                && hovered_mods.mod_ids.contains(&mod_id)
                                            {
                                                ui.painter().hline(
                                                    row_rect.x_range(),
                                                    row_rect.center().y,
                                                    insertion_stroke,
                                                );
                                                Some(mod_id)
                                            } else if pointer_position.y < row_rect.center().y {
                                                ui.painter().hline(
                                                    row_rect.x_range(),
                                                    row_rect.top(),
                                                    insertion_stroke,
                                                );
                                                Some(mod_id)
                                            } else {
                                                ui.painter().hline(
                                                    row_rect.x_range(),
                                                    row_rect.bottom(),
                                                    insertion_stroke,
                                                );

                                                visible_list_positions
                                                    .get(visible_row + 1)
                                                    .and_then(|&next_list_position| {
                                                        mod_ids.get(next_list_position)
                                                    })
                                                    .copied()
                                            };

                                            if let Some(dropped_mods) =
                                                response.dnd_release_payload::<DraggedMods>()
                                            {
                                                requested_action = Some(action_for_drop(
                                                    &dropped_mods,
                                                    kind,
                                                    before_mod_id,
                                                ));
                                            }
                                        }
                                    }
                                },
                            );
                    })
                    .response;

                if let Some(dropped_mods) = list_response.dnd_release_payload::<DraggedMods>() {
                    // Dropping on the list's empty space appends the mod to that list.
                    requested_action = Some(action_for_drop(&dropped_mods, kind, None));
                }
            },
        );
    });

    requested_action
}

/// Floating label that follows the cursor while mods are dragged. It tells the
/// user what is being moved without changing the drop target itself.
pub(crate) fn show_drag_ghost(context: &egui::Context, mods: &ModCollection) {
    let Some(dragged) = egui::DragAndDrop::payload::<DraggedMods>(context) else {
        return;
    };

    if dragged.mod_ids.is_empty() {
        return;
    }

    let Some(pointer) = context.input(|input| input.pointer.interact_pos()) else {
        return;
    };

    let count = dragged.mod_ids.len();
    let first_name = dragged
        .mod_ids
        .iter()
        .find_map(|&mod_id| mods.get(mod_id))
        .map_or("mod", |rimworld_mod| rimworld_mod.name.as_str());

    let label = if count == 1 {
        first_name.to_owned()
    } else {
        format!("{count} mods · {first_name}")
    };

    let appear = context.animate_bool_with_time(egui::Id::new("mod_drag_ghost_appear"), true, 0.12);

    egui::Area::new(egui::Id::new("mod_drag_ghost"))
        .order(egui::Order::Tooltip)
        .fixed_pos(pointer + egui::vec2(16.0, 18.0))
        .interactable(false)
        .show(context, |ui| {
            ui.set_opacity(appear);

            egui::Frame::new()
                .fill(theme::SURFACE_HOVER)
                .corner_radius(egui::CornerRadius::same(theme::RADIUS_MD))
                .shadow(theme::soft_shadow())
                .inner_margin(egui::Margin::symmetric(12, 8))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(count.to_string())
                                .font(theme::semibold_font(12.0))
                                .color(theme::ACCENT),
                        );

                        ui.label(
                            egui::RichText::new(label)
                                .text_style(theme::strong_style())
                                .color(theme::TEXT),
                        );
                    });
                });
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ModType, PackageId};
    use std::path::PathBuf;

    #[test]
    fn selection_range_uses_visible_list_order_in_both_directions() {
        let first = ModId::from_index(1);
        let second = ModId::from_index(4);
        let third = ModId::from_index(7);
        let visible_mod_ids = [first, second, third];

        assert_eq!(
            selection_range(&visible_mod_ids, Some(first), third),
            Some(vec![first, second, third])
        );
        assert_eq!(
            selection_range(&visible_mod_ids, Some(third), first),
            Some(vec![first, second, third])
        );
    }

    #[test]
    fn missing_selection_anchor_does_not_create_a_range() {
        let visible_mod_ids = [ModId::from_index(1), ModId::from_index(2)];

        assert_eq!(
            selection_range(
                &visible_mod_ids,
                Some(ModId::from_index(9)),
                visible_mod_ids[1]
            ),
            None
        );
    }

    fn badge_mod(supported: &[&str], community: &[&str]) -> RimworldMod {
        RimworldMod {
            name: "Test mod".to_owned(),
            package_id: PackageId::new("test.mod").expect("valid package ID"),
            description: String::new(),
            supported_versions: supported
                .iter()
                .map(|version| (*version).to_owned())
                .collect(),
            community_supported_versions: community
                .iter()
                .map(|version| (*version).to_owned())
                .collect(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            force_load_after: Vec::new(),
            force_load_before: Vec::new(),
            mod_dependencies: Vec::new(),
            incompatible_with: Vec::new(),
            inferred_after: Vec::new(),
            is_patch_only: false,
            folder: PathBuf::new(),
            source: ModSource::Local,
            mod_type: ModType::Xml,
        }
    }

    #[test]
    fn official_support_gets_no_badge() {
        let rimworld_mod = badge_mod(&["1.4", "1.5", "1.6"], &[]);

        assert!(version_badge(&rimworld_mod, Some("1.6.4211")).is_none());
    }

    #[test]
    fn community_support_shows_the_no_version_warning_report() {
        let rimworld_mod = badge_mod(&["1.4"], &["1.6"]);

        let badge = version_badge(&rimworld_mod, Some("1.6")).expect("badge");

        assert_eq!(badge.text, "1.6");
        assert!(badge.community_report);
        assert!(version_badge_tooltip(&badge).contains("No Version Warning"));
    }

    #[test]
    fn older_community_report_shows_icon_in_faint_badge() {
        let rimworld_mod = badge_mod(&["1.5"], &["1.6"]);
        let badge = version_badge(&rimworld_mod, Some("1.7")).expect("badge");

        assert_eq!(badge.text, "1.6");
        assert_eq!(badge.color, theme::TEXT_FAINT);
        assert!(badge.community_report);
        assert!(!badge.matches_game_version);
        assert!(version_badge_tooltip(&badge).contains("No Version Warning"));
        assert!(version_badge_tooltip(&badge).contains("1.6"));
    }

    #[test]
    fn outdated_version_badge_tooltip_mentions_listed_version() {
        let rimworld_mod = badge_mod(&["1.5"], &[]);
        let badge = version_badge(&rimworld_mod, Some("1.6")).expect("badge");

        assert!(version_badge_tooltip(&badge).contains("1.5"));
        assert!(!version_badge_tooltip(&badge).contains("No Version Warning"));
    }

    #[test]
    fn unsupported_mods_show_their_newest_supported_version() {
        let rimworld_mod = badge_mod(&["1.0", "1.5", "1.4"], &[]);

        let badge = version_badge(&rimworld_mod, Some("1.6")).expect("badge");

        assert_eq!(badge.text, "1.5");
        assert!(!badge.community_report);
    }

    #[test]
    fn mod_without_listed_versions_gets_no_badge() {
        let rimworld_mod = badge_mod(&[], &[]);

        assert_eq!(version_badge(&rimworld_mod, Some("1.6")), None);
    }

    #[test]
    fn unknown_game_version_gets_no_badge() {
        let rimworld_mod = badge_mod(&["1.6"], &[]);

        assert_eq!(version_badge(&rimworld_mod, None), None);
    }

    #[test]
    fn short_tags_stay_intact_and_long_tags_are_ellipsized() {
        assert_eq!(short_tag("Fixes"), "Fixes");
        assert_eq!(short_tag("abcdefghijklmnop"), "abcdefghijklm…");
    }

    #[test]
    fn unsubscribe_skips_non_workshop_and_unsubscribed_mods() {
        let mut subscribed_workshop = badge_mod(&[], &[]);
        subscribed_workshop.source = ModSource::SteamWorkshop { workshop_id: 42 };

        let mut unsubscribed_workshop = badge_mod(&[], &[]);
        unsubscribed_workshop.source = ModSource::SteamWorkshop { workshop_id: 43 };

        let local = badge_mod(&[], &[]);
        let mods = ModCollection::new(
            vec![subscribed_workshop, unsubscribed_workshop, local],
            Vec::new(),
            Vec::new(),
            Vec::new(),
        );
        let selected = [
            ModId::from_index(0),
            ModId::from_index(1),
            ModId::from_index(2),
        ];

        assert_eq!(
            unsubscribe_workshop_ids(&mods, &selected, Some(&HashSet::from([42]))),
            [42]
        );
        assert_eq!(unsubscribe_workshop_ids(&mods, &selected, None), [42, 43]);
    }

    #[test]
    fn order_warning_text_describes_incompatibilities_and_order_rules() {
        let mut mod_a = badge_mod(&[], &[]);
        mod_a.name = "Mod A".to_owned();
        let mut mod_b = badge_mod(&[], &[]);
        mod_b.name = "Mod B".to_owned();
        let mut mod_c = badge_mod(&[], &[]);
        mod_c.name = "Mod C".to_owned();
        let all_mods = [mod_a, mod_b, mod_c];

        let warning = OrderWarning {
            incompatible_mod_ids: vec![ModId::from_index(0)],
            after_mod_ids: vec![ModId::from_index(1)],
            before_mod_ids: vec![ModId::from_index(2)],
        };

        assert_eq!(
            order_warning_text(&warning, &all_mods),
            "Incompatible with: Mod A\nMove below: Mod B\nMove above: Mod C"
        );
    }
}

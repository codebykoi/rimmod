use std::collections::HashMap;

use eframe::egui::{self, AtomExt};
use nucleo_matcher::Matcher;

use crate::{
    models::{ModCollection, ModId, ModSource, RimworldMod},
    services::{fuzzy_search::fuzzy_mod_indices, mod_sorter::OrderWarning},
    ui::icons::{INLINE_ICON_SCALE, mod_source_icon, mod_type_icon, warning_icon},
};

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

fn order_warning_text(warning: &OrderWarning, all_mods: &[RimworldMod]) -> String {
    let mut lines = Vec::new();

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

pub(crate) struct ModListView<'a> {
    pub(crate) kind: ModListKind,
    pub(crate) mods: &'a ModCollection,
    pub(crate) mod_ids: &'a [ModId],
    pub(crate) order_warnings: &'a HashMap<ModId, OrderWarning>,
    pub(crate) search_string: &'a mut String,
    pub(crate) matcher: &'a mut Matcher,
    pub(crate) selected_mod_ids: &'a [ModId],
    pub(crate) selection_anchor_id: Option<ModId>,
    pub(crate) selection_kind: Option<ModListKind>,
}

pub(crate) fn show_mod_list(ui: &mut egui::Ui, view: ModListView<'_>) -> Option<ModListAction> {
    let ModListView {
        kind,
        mods,
        mod_ids,
        order_warnings,
        search_string,
        matcher,
        selected_mod_ids,
        selection_anchor_id,
        selection_kind,
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
        let heading_text = if selected_count > 1 {
            format!("{heading}: {count} ({selected_count} selected)")
        } else {
            format!("{heading}: {count}")
        };

        ui.heading(heading_text);
        ui.separator();

        ui.text_edit_singleline(search_string);

        let visible_list_positions = fuzzy_mod_indices(&mods.all, mod_ids, search_string, matcher);
        let visible_mod_ids = visible_list_positions
            .iter()
            .map(|&list_position| mod_ids[list_position])
            .collect::<Vec<_>>();

        let row_height = ui.spacing().interact_size.y;

        let list_response = ui
            .scope(|ui| {
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
                                        .filter(|candidate| selected_mod_ids.contains(candidate))
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

                                let icon_height = ui.text_style_height(&egui::TextStyle::Body)
                                    * INLINE_ICON_SCALE;

                                let source_icon =
                                    egui::Image::new(mod_source_icon(&rimworld_mod.source))
                                        .atom_max_height(icon_height);

                                let type_icon =
                                    egui::Image::new(mod_type_icon(&rimworld_mod.mod_type))
                                        .atom_max_height(icon_height);

                                // One response handles both gestures. egui waits for enough
                                // pointer movement before classifying the gesture as a drag.

                                let mut button = egui::Button::selectable(
                                    is_selected,
                                    (source_icon, type_icon, &rimworld_mod.name),
                                );

                                if order_warning.is_some() {
                                    let warning_icon = warning_icon().atom_max_height(icon_height);
                                    button = button.right_text(warning_icon);
                                }

                                let response = ui.add(button.sense(egui::Sense::click_and_drag()));

                                let response = match order_warning {
                                    Some(warning) => response
                                        .on_hover_text(order_warning_text(warning, &mods.all)),
                                    None => response,
                                };

                                response.context_menu(|ui| {
                                    if dragged_mods.mod_ids.len() > 1 {
                                        ui.strong(format!(
                                            "{} mods selected",
                                            dragged_mods.mod_ids.len()
                                        ));
                                    } else {
                                        ui.strong(&rimworld_mod.name);
                                        ui.label(rimworld_mod.package_id.as_str());
                                    }

                                    ui.separator();

                                    let transfer_label = match (kind, dragged_mods.mod_ids.len()) {
                                        (ModListKind::Enabled, 1) => "Disable".to_owned(),
                                        (ModListKind::Disabled, 1) => "Enable".to_owned(),
                                        (ModListKind::Enabled, count) => {
                                            format!("Disable {count} mods")
                                        }
                                        (ModListKind::Disabled, count) => {
                                            format!("Enable {count} mods")
                                        }
                                    };

                                    if ui.button(transfer_label).clicked() {
                                        requested_action = Some(ModListAction::Transfer {
                                            mod_ids: dragged_mods.mod_ids.clone(),
                                            from: kind,
                                            to: kind.other(),
                                            before_mod_id: None,
                                        });

                                        ui.close();
                                    }

                                    ui.separator();

                                    let can_delete = dragged_mods.mod_ids.iter().all(|&mod_id| {
                                        mods.get(mod_id).is_some_and(|rimworld_mod| {
                                            !matches!(rimworld_mod.source, ModSource::Official)
                                        })
                                    });
                                    let delete_label = match dragged_mods.mod_ids.len() {
                                        1 => "Delete".to_owned(),
                                        count => format!("Delete {count} mods"),
                                    };
                                    let delete_button = egui::Button::new(
                                        egui::RichText::new(delete_label)
                                            .color(ui.visuals().error_fg_color),
                                    );

                                    if ui.add_enabled(can_delete, delete_button).clicked() {
                                        requested_action = Some(ModListAction::Delete {
                                            mod_ids: dragged_mods.mod_ids.clone(),
                                        });
                                        ui.close();
                                    }

                                    if !can_delete {
                                        ui.label(
                                            egui::RichText::new(
                                                "Official RimWorld content cannot be deleted",
                                            )
                                            .weak(),
                                        );
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
                                        .map_or(SelectionMode::Replace, SelectionMode::Range)
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
                                let hovered_mods = response.dnd_hover_payload::<DraggedMods>();

                                if let (Some(pointer_position), Some(hovered_mods)) =
                                    (pointer_position, hovered_mods)
                                {
                                    let row_rect = response.rect;
                                    let insertion_stroke =
                                        egui::Stroke::new(2.0, ui.visuals().selection.stroke.color);

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
    });

    requested_action
}

#[cfg(test)]
mod tests {
    use super::*;

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
}

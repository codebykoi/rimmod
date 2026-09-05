use crate::models::ModId;
use crate::ui::mod_list::{ModListAction, ModListKind, SelectionMode};

use super::App;

impl App {
    pub(super) fn clear_mod_selection(&mut self) {
        self.state.selected_mod_id = None;
        self.state.selected_mod_ids.clear();
        self.state.selection_anchor_id = None;
        self.state.selection_kind = None;
    }

    fn select_mod_group(&mut self, mod_ids: Vec<ModId>, kind: ModListKind) {
        self.state.selected_mod_id = self
            .state
            .selected_mod_id
            .filter(|mod_id| mod_ids.contains(mod_id))
            .or_else(|| mod_ids.last().copied());
        self.state.selection_anchor_id = self
            .state
            .selection_anchor_id
            .filter(|mod_id| mod_ids.contains(mod_id))
            .or_else(|| mod_ids.first().copied());
        self.state.selected_mod_ids = mod_ids;
        self.state.selection_kind = Some(kind);
    }

    pub(super) fn apply_mod_list_action(&mut self, action: ModListAction) {
        match action {
            ModListAction::Select { mod_id, kind, mode } => {
                if self.state.selection_kind != Some(kind) {
                    self.state.selected_mod_ids.clear();
                }

                match mode {
                    SelectionMode::Replace => {
                        self.state.selected_mod_ids.clear();
                        self.state.selected_mod_ids.push(mod_id);
                        self.state.selection_anchor_id = Some(mod_id);
                    }
                    SelectionMode::Toggle => {
                        if let Some(position) = self
                            .state
                            .selected_mod_ids
                            .iter()
                            .position(|&selected_id| selected_id == mod_id)
                        {
                            self.state.selected_mod_ids.remove(position);
                        } else {
                            self.state.selected_mod_ids.push(mod_id);
                        }
                        self.state.selection_anchor_id = Some(mod_id);
                    }
                    SelectionMode::Range(mod_ids) => {
                        self.state.selected_mod_ids = mod_ids;
                    }
                }

                self.state.selection_kind = if self.state.selected_mod_ids.is_empty() {
                    None
                } else {
                    Some(kind)
                };
                self.state.selected_mod_id = if self.state.selected_mod_ids.contains(&mod_id) {
                    Some(mod_id)
                } else {
                    self.state.selected_mod_ids.last().copied()
                };
            }

            ModListAction::Transfer {
                mod_ids,
                from,
                to,
                before_mod_id,
            } => {
                let previous = self.load_order_snapshot();
                let result = match (from, to) {
                    (ModListKind::Disabled, ModListKind::Enabled) => {
                        self.mods.enable_many(&mod_ids, before_mod_id)
                    }

                    (ModListKind::Enabled, ModListKind::Disabled) => {
                        self.mods.disable_many(&mod_ids, before_mod_id)
                    }

                    _ => return,
                };

                match result {
                    Ok(()) => {
                        self.clear_action_error();
                        self.select_mod_group(mod_ids, to);
                        self.record_load_order_change(previous);
                    }
                    Err(error) => {
                        self.report_action_error(format!("Could not update mod list: {error}"));
                    }
                }
            }

            ModListAction::Reorder {
                mod_ids,
                kind,
                before_mod_id,
            } => {
                let previous = self.load_order_snapshot();
                let result = match kind {
                    ModListKind::Enabled => self.mods.reorder_enabled_many(&mod_ids, before_mod_id),

                    ModListKind::Disabled => {
                        self.mods.reorder_disabled_many(&mod_ids, before_mod_id)
                    }
                };

                match result {
                    Ok(()) => {
                        self.clear_action_error();
                        self.select_mod_group(mod_ids, kind);
                        self.record_load_order_change(previous);
                    }
                    Err(error) => {
                        self.report_action_error(format!("Could not reorder mod: {error}"));
                    }
                }
            }

            ModListAction::Delete { mod_ids } => {
                self.request_mod_deletion(&mod_ids);
            }
        }
    }
}

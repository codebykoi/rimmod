use std::collections::VecDeque;

use eframe::egui;

use crate::models::ModId;
use crate::ui::mod_list::ModListKind;
use crate::ui::top_panel;

use super::App;

const LOAD_ORDER_HISTORY_LIMIT: usize = 100;

#[derive(Clone, PartialEq, Eq)]
pub(super) struct LoadOrderSnapshot {
    disabled_mod_ids: Vec<ModId>,
    enabled_mod_ids: Vec<ModId>,
    selected_mod_id: Option<ModId>,
    selected_mod_ids: Vec<ModId>,
    selection_anchor_id: Option<ModId>,
    selection_kind: Option<ModListKind>,
}

impl App {
    pub(super) fn load_order_snapshot(&self) -> LoadOrderSnapshot {
        LoadOrderSnapshot {
            disabled_mod_ids: self.mods.disabled_ids().to_vec(),
            enabled_mod_ids: self.mods.enabled_ids().to_vec(),
            selected_mod_id: self.state.selected_mod_id,
            selected_mod_ids: self.state.selected_mod_ids.clone(),
            selection_anchor_id: self.state.selection_anchor_id,
            selection_kind: self.state.selection_kind,
        }
    }

    fn restore_load_order_snapshot(&mut self, snapshot: LoadOrderSnapshot) {
        self.mods
            .restore_list_orders(snapshot.disabled_mod_ids, snapshot.enabled_mod_ids);
        self.state.selected_mod_id = snapshot.selected_mod_id;
        self.state.selected_mod_ids = snapshot.selected_mod_ids;
        self.state.selection_anchor_id = snapshot.selection_anchor_id;
        self.state.selection_kind = snapshot.selection_kind;
    }

    fn push_history_entry(history: &mut VecDeque<LoadOrderSnapshot>, snapshot: LoadOrderSnapshot) {
        if history.len() == LOAD_ORDER_HISTORY_LIMIT {
            history.pop_front();
        }

        history.push_back(snapshot);
    }

    pub(super) fn record_load_order_change(&mut self, previous: LoadOrderSnapshot) {
        let order_changed = previous.disabled_mod_ids != self.mods.disabled_ids()
            || previous.enabled_mod_ids != self.mods.enabled_ids();

        if !order_changed {
            return;
        }

        Self::push_history_entry(&mut self.undo_history, previous);
        self.redo_history.clear();
    }

    pub(super) fn clear_load_order_history(&mut self) {
        self.undo_history.clear();
        self.redo_history.clear();
    }

    pub(super) fn undo_load_order_change(&mut self) {
        let Some(previous) = self.undo_history.pop_back() else {
            return;
        };

        let current = self.load_order_snapshot();
        Self::push_history_entry(&mut self.redo_history, current);
        self.restore_load_order_snapshot(previous);
        self.clear_action_error();
        self.toasts.info("Load-order change undone");
    }

    pub(super) fn redo_load_order_change(&mut self) {
        let Some(next) = self.redo_history.pop_back() else {
            return;
        };

        let current = self.load_order_snapshot();
        Self::push_history_entry(&mut self.undo_history, current);
        self.restore_load_order_snapshot(next);
        self.clear_action_error();
        self.toasts.info("Load-order change redone");
    }

    pub(super) fn handle_history_shortcuts(&mut self, context: &egui::Context) {
        if context.egui_wants_keyboard_input() {
            return;
        }

        let redo_requested = context.input_mut(|input| {
            input.consume_shortcut(&top_panel::ALTERNATE_REDO_SHORTCUT)
                || input.consume_shortcut(&top_panel::REDO_SHORTCUT)
        });

        if redo_requested {
            self.redo_load_order_change();
        } else if context.input_mut(|input| input.consume_shortcut(&top_panel::UNDO_SHORTCUT)) {
            self.undo_load_order_change();
        }
    }
}

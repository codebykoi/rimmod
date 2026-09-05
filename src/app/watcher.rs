use std::path::Path;
use std::time::{Duration, Instant};

use eframe::egui;

use crate::services::mod_loader;
use crate::services::mod_watcher::{ModWatcher, ModWatcherPoll};
use crate::services::workshop;

use super::{App, LoadOrderStatus};

const MOD_RELOAD_DEBOUNCE: Duration = Duration::from_millis(750);

impl App {
    pub(super) fn restart_mod_watcher(&mut self, context: &egui::Context) {
        self.mod_watcher = None;
        self.pending_mod_reload_at = None;

        let mut paths = [
            self.settings.official_path(),
            self.settings.local_path(),
            self.settings.workshop_path().map(Path::to_path_buf),
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path()),
        ]
        .into_iter()
        .flatten()
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

        paths.sort();
        paths.dedup();

        if paths.is_empty() {
            return;
        }

        let repaint_context = context.clone();
        match ModWatcher::new(paths, move || repaint_context.request_repaint()) {
            Ok(watcher) => self.mod_watcher = Some(watcher),
            Err(error) => {
                self.report_action_error(format!("Could not watch mod folders: {error}"));
            }
        }
    }

    fn reload_mods_after_folder_change(&mut self) -> bool {
        let active_package_ids = self.mods.active_package_ids();
        let selected_package_ids = self
            .state
            .selected_mod_ids
            .iter()
            .filter_map(|&mod_id| self.mods.get(mod_id))
            .map(|rimworld_mod| rimworld_mod.package_id.clone())
            .collect::<Vec<_>>();
        let selected_package_id = self
            .state
            .selected_mod_id
            .and_then(|mod_id| self.mods.get(mod_id))
            .map(|rimworld_mod| rimworld_mod.package_id.clone());
        let selection_anchor_package_id = self
            .state
            .selection_anchor_id
            .and_then(|mod_id| self.mods.get(mod_id))
            .map(|rimworld_mod| rimworld_mod.package_id.clone());

        match mod_loader::load_mods(&self.settings, self.game_version.as_deref()) {
            Ok(mut report) => {
                report.mods.replace_active_package_ids(active_package_ids);

                self.mods = report.mods;
                self.mod_load_warnings = report.warnings;
                self.load_order_status = LoadOrderStatus::Ready;
                self.clear_load_order_history();
                self.state.selected_mod_ids = selected_package_ids
                    .iter()
                    .filter_map(|package_id| self.mods.find_id_by_package_id(package_id))
                    .collect();
                self.state.selected_mod_id = selected_package_id
                    .as_ref()
                    .and_then(|package_id| self.mods.find_id_by_package_id(package_id))
                    .or_else(|| self.state.selected_mod_ids.last().copied());

                if self.state.selected_mod_ids.is_empty() {
                    self.clear_mod_selection();
                } else {
                    self.state.selection_anchor_id = selection_anchor_package_id
                        .as_ref()
                        .and_then(|package_id| self.mods.find_id_by_package_id(package_id))
                        .filter(|mod_id| self.state.selected_mod_ids.contains(mod_id));
                }

                true
            }
            Err(error) => {
                self.mod_load_warnings.clear();
                self.load_order_status = LoadOrderStatus::Unavailable {
                    reason: error.to_string(),
                };

                false
            }
        }
    }

    pub(super) fn process_mod_watcher(&mut self, context: &egui::Context) {
        let poll = self
            .mod_watcher
            .as_ref()
            .map_or(ModWatcherPoll::Idle, ModWatcher::poll);

        match poll {
            ModWatcherPoll::Idle => {}
            ModWatcherPoll::Changed => {
                self.pending_mod_reload_at = Some(Instant::now() + MOD_RELOAD_DEBOUNCE);
            }
            ModWatcherPoll::Error(error) => {
                self.report_action_error(format!("Could not watch mod folders: {error}"));
            }
        }

        let Some(reload_at) = self.pending_mod_reload_at else {
            return;
        };
        let now = Instant::now();

        if now < reload_at {
            context.request_repaint_after(reload_at - now);
            return;
        }

        self.pending_mod_reload_at = None;
        if self.reload_mods_after_folder_change() {
            self.toasts.info("Mods reloaded after folder changes");
        }
    }
}

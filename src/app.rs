use std::collections::{HashMap, VecDeque};
use std::io;
use std::time::Instant;

use eframe::egui::{self, Color32, Shadow};
use egui_notify::Toasts;
use nucleo_matcher::Matcher;

use crate::models::{ModCollection, ModId};
use crate::services::mod_loader::{self, ModLoadWarning};
use crate::services::mod_sorter::sort_mods;
use crate::services::mod_watcher::ModWatcher;
use crate::services::settings::{Settings, SettingsErrors};
use crate::services::workshop;
use crate::services::{load_order, mod_sorter};

use crate::ui::mod_info::show_mod_info;
use crate::ui::mod_list::{ModListKind, ModListView, show_mod_list};
use crate::ui::settings_window::{SettingsAction, display_settings};
use crate::ui::workshop::{CoveredModsState, WorkshopAction, WorkshopView, show_workshop};
use crate::ui::{OpenTarget, bottom_panel, top_panel};

mod deletion;
mod history;
mod launch;
mod mod_lists;
mod settings_apply;
mod watcher;
mod workshop_ctrl;

use self::deletion::PendingModDeletion;
use self::history::LoadOrderSnapshot;
use self::settings_apply::{path_from_input, path_to_input, persist_settings};
use self::workshop_ctrl::{CoveredModsStatus, WorkshopState};

const SETTINGS_STORAGE_KEY: &str = "rimmod_settings";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    #[default]
    Mods,
    Workshop,
}

fn open_settings_action() -> egui::Id {
    egui::Id::new("open_settings")
}

pub(crate) enum LoadOrderStatus {
    Unavailable { reason: String },
    Ready,
}

impl LoadOrderStatus {
    pub(crate) fn can_save(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

impl Default for LoadOrderStatus {
    fn default() -> Self {
        Self::Unavailable {
            reason: String::from("Unknown"),
        }
    }
}

#[derive(Default)]
struct SearchStrings {
    disabled_mods: String,
    active_mods: String,
}

#[derive(Default)]
pub(crate) struct UiState {
    selected_mod_id: Option<ModId>,
    selected_mod_ids: Vec<ModId>,
    selection_anchor_id: Option<ModId>,
    selection_kind: Option<ModListKind>,

    search_strings: SearchStrings,
    fuzzy_matcher: Matcher,

    pub(crate) settings_errors: SettingsErrors,

    pub(crate) settings_open: bool,
    pub(crate) rimworld_path_input: String,
    pub(crate) workshop_path_input: String,
    pub(crate) config_path_input: String,
    pub(crate) steamcmd_path_input: String,
    pub(crate) steam_web_api_key_input: String,
}

#[derive(Default)]
pub(crate) struct App {
    pub(crate) mods: ModCollection,
    mod_load_warnings: Vec<ModLoadWarning>,
    pub(crate) settings: Settings,
    state: UiState,
    toasts: Toasts,
    action_error: Option<String>,
    game_version: Option<String>,
    load_order_status: LoadOrderStatus,
    mod_watcher: Option<ModWatcher>,
    pending_mod_reload_at: Option<Instant>,
    pending_mod_deletion: Option<PendingModDeletion>,
    undo_history: VecDeque<LoadOrderSnapshot>,
    redo_history: VecDeque<LoadOrderSnapshot>,
    current_tab: AppTab,
    workshop: WorkshopState,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.options_mut(|options| {
            options.input_options.max_double_click_delay = 0.5;

            options.input_options.max_click_dist = 10.0;
        });

        cc.egui_ctx.all_styles_mut(|style| {
            style.interaction.tooltip_delay = 0.15;
            style.interaction.show_tooltips_only_when_still = false;
        });

        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut settings = cc
            .storage
            .and_then(|storage| eframe::get_value::<Settings>(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();

        settings.fill_missing_from(Settings::auto_discover());

        let mut app = Self {
            settings,
            toasts: Toasts::default().with_shadow(Shadow {
                offset: Default::default(),
                blur: 30,
                spread: 5,
                color: Color32::from_black_alpha(70),
            }),
            ..Self::default()
        };

        app.game_version = match app.settings.rimworld_path() {
            Some(rimworld_path) => match Self::load_game_version(rimworld_path) {
                Ok(version) => Some(version),

                Err(error) => {
                    app.report_action_error(format!("Could not read RimWorld version: {error}"));

                    None
                }
            },

            None => None,
        };

        app.reload_mods();
        app.restart_mod_watcher(&cc.egui_ctx);

        app
    }

    fn report_action_error(&mut self, message: impl Into<String>) {
        let message = message.into();
        self.action_error = Some(message.clone());
        self.toasts.error(message);
    }

    fn clear_action_error(&mut self) {
        self.action_error = None;
    }

    fn open_target(&mut self, target: OpenTarget) {
        let description = target.description();
        let result = match target {
            OpenTarget::Folder(path) => open::that(path),
            OpenTarget::Url(url) => open::that(url),
        };

        self.handle_open_result(description, result);
    }

    fn handle_open_result(&mut self, description: String, result: io::Result<()>) {
        match result {
            Ok(()) => self.clear_action_error(),
            Err(error) => {
                self.report_action_error(format!("Could not open {description}: {error}"));
            }
        }
    }

    fn open_settings(&mut self) {
        self.state.rimworld_path_input = path_to_input(self.settings.rimworld_path());

        self.state.workshop_path_input = path_to_input(self.settings.workshop_path());

        self.state.config_path_input = path_to_input(self.settings.config_path());

        self.state.steamcmd_path_input = path_to_input(self.settings.steamcmd_path());

        self.state.steam_web_api_key_input = self.settings.steam_web_api_key.clone();

        self.state.settings_errors = SettingsErrors::default();

        self.state.settings_open = true;
    }

    fn save_mods(&mut self) {
        if !self.load_order_status.can_save() {
            self.report_action_error("Cannot save: the load order is unavailable");
            return;
        }

        let result = match self.settings.config_path() {
            Some(config_folder) => load_order::save_load_order(config_folder, &self.mods),

            None => Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "RimWorld configuration path is not configured",
            )),
        };

        match result {
            Ok(outcome) => {
                self.clear_action_error();
                self.toasts.success(format!(
                    "Load order saved. Backup: {}",
                    outcome.backup_path.display()
                ));
            }
            Err(error) => {
                self.report_action_error(format!("Could not save load order: {error}"));
            }
        }
    }

    fn reload_mods(&mut self) {
        self.clear_load_order_history();

        match mod_loader::load_mods(&self.settings, self.game_version.as_deref()) {
            Ok(report) => {
                self.mods = report.mods;
                self.mod_load_warnings = report.warnings;
                self.load_order_status = LoadOrderStatus::Ready;
            }
            Err(error) => {
                self.mod_load_warnings.clear();
                self.load_order_status = LoadOrderStatus::Unavailable {
                    reason: error.to_string(),
                };
            }
        }
    }

    fn sort_mods(&mut self) {
        let previous = self.load_order_snapshot();

        match sort_mods(&mut self.mods) {
            Ok(()) => {
                self.clear_action_error();
                self.record_load_order_change(previous);
                self.toasts.success("Active mods sorted");
            }
            Err(error) => {
                self.report_action_error(format!("Could not sort active mods: {error}"));
            }
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_STORAGE_KEY, &self.settings);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.process_mod_watcher(ui.ctx());
        self.poll_workshop_tasks(ui.ctx());

        if let Some(action) = top_panel::show_top_panel(
            ui,
            !self.undo_history.is_empty(),
            !self.redo_history.is_empty(),
        ) {
            match action {
                top_panel::TopPanelAction::Undo => {
                    self.undo_load_order_change();
                }

                top_panel::TopPanelAction::Redo => {
                    self.redo_load_order_change();
                }

                top_panel::TopPanelAction::Settings => {
                    self.open_settings();
                }
            }
        }

        if self.state.settings_open {
            let settings_action = display_settings(ui, &mut self.state);

            match settings_action {
                Some(SettingsAction::Apply) => {
                    let candidate = Settings {
                        rimworld_path: path_from_input(&self.state.rimworld_path_input),
                        workshop_path: path_from_input(&self.state.workshop_path_input),
                        config_path: path_from_input(&self.state.config_path_input),
                        steamcmd_path: path_from_input(&self.state.steamcmd_path_input),
                        steam_web_api_key: self.state.steam_web_api_key_input.clone(),
                    };

                    match self.try_apply_settings(candidate) {
                        Ok(()) => {
                            self.restart_mod_watcher(ui.ctx());
                            self.state.settings_errors = SettingsErrors::default();
                            self.state.settings_open = false;

                            if let Some(storage) = frame.storage_mut() {
                                persist_settings(storage, &self.settings);
                            }

                            self.toasts.success("Settings applied");
                        }
                        Err(errors) => {
                            self.state.settings_errors = errors;
                        }
                    }
                }

                Some(SettingsAction::Cancel) => {
                    self.state.settings_errors = SettingsErrors::default();
                    self.state.settings_open = false;
                }

                Some(SettingsAction::Open(target)) => {
                    self.open_target(target);
                }

                None => {}
            }
        }

        if let Some(action) = bottom_panel::show_bottom_panel(
            ui,
            self.game_version.as_deref(),
            &self.load_order_status,
            &self.mod_load_warnings,
            self.action_error.as_deref(),
        ) {
            match action {
                bottom_panel::BottomPanelAction::DismissError => {
                    self.clear_action_error();
                }

                bottom_panel::BottomPanelAction::Save => {
                    self.save_mods();
                }

                bottom_panel::BottomPanelAction::Run => {
                    self.run_game();
                }

                bottom_panel::BottomPanelAction::Sort => {
                    self.sort_mods();
                }
            }
        }

        let order_check = mod_sorter::find_order_warnings(&self.mods);
        let no_warnings = HashMap::new();

        let order_warnings = match &order_check {
            Ok(warnings) => warnings,
            Err(_) => &no_warnings,
        };

        let mut requested_open = None;
        let mut workshop_action = None;
        let steamcmd_workshop_path =
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path());

        egui::CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.current_tab, AppTab::Mods, "Mods");
                ui.selectable_value(&mut self.current_tab, AppTab::Workshop, "Workshop");
            });
            ui.separator();

            match self.current_tab {
                AppTab::Mods => {
                    ui.columns_const(|[info_column, disabled_column, active_column]| {
                        let selected_mod = self
                            .state
                            .selected_mod_id
                            .and_then(|mod_id| self.mods.get(mod_id));

                        requested_open =
                            show_mod_info(info_column, selected_mod, self.game_version.as_deref());

                        let disabled_action = show_mod_list(
                            disabled_column,
                            ModListView {
                                kind: ModListKind::Disabled,
                                mods: &self.mods,
                                mod_ids: self.mods.disabled_ids(),
                                game_version: self.game_version.as_deref(),
                                order_warnings,
                                search_string: &mut self.state.search_strings.disabled_mods,
                                matcher: &mut self.state.fuzzy_matcher,
                                selected_mod_ids: &self.state.selected_mod_ids,
                                selection_anchor_id: self.state.selection_anchor_id,
                                selection_kind: self.state.selection_kind,
                            },
                        );

                        let active_action = show_mod_list(
                            active_column,
                            ModListView {
                                kind: ModListKind::Enabled,
                                mods: &self.mods,
                                mod_ids: self.mods.enabled_ids(),
                                game_version: self.game_version.as_deref(),
                                order_warnings,
                                search_string: &mut self.state.search_strings.active_mods,
                                matcher: &mut self.state.fuzzy_matcher,
                                selected_mod_ids: &self.state.selected_mod_ids,
                                selection_anchor_id: self.state.selection_anchor_id,
                                selection_kind: self.state.selection_kind,
                            },
                        );

                        if let Some(action) = disabled_action {
                            self.apply_mod_list_action(action);
                        }

                        if let Some(action) = active_action {
                            self.apply_mod_list_action(action);
                        }
                    });
                }
                AppTab::Workshop => {
                    self.ensure_covered_mods(ui.ctx());

                    // Borrow only the coverage field so the view can still
                    // mutably borrow the other WorkshopState fields.
                    let covered_mods_state = match &self.workshop.covered_mods {
                        CoveredModsStatus::Ready(covered_mods) => {
                            CoveredModsState::Ready(covered_mods)
                        }
                        CoveredModsStatus::Failed(error) => CoveredModsState::Failed(error.clone()),
                        CoveredModsStatus::NotRequested | CoveredModsStatus::Loading(_) => {
                            CoveredModsState::Loading
                        }
                    };

                    workshop_action = show_workshop(
                        ui,
                        WorkshopView {
                            query: &mut self.workshop.query,
                            sort: &mut self.workshop.sort,
                            page: self.workshop.page,
                            total: self.workshop.total,
                            items: &self.workshop.items,
                            load_status: &self.workshop.load_status,
                            installing_items: &self.workshop.installing_items,
                            selected_items: &self.workshop.selected_items,
                            workshop_path: self.settings.workshop_path(),
                            steamcmd_workshop_path: steamcmd_workshop_path.as_deref(),
                            api_key_configured: self.settings.steam_web_api_key().is_some(),
                            mods: &self.mods,
                            game_version: self.game_version.as_deref(),
                            covered_mods_state,
                        },
                    );
                }
            }
        });

        if let Some(target) = requested_open {
            self.open_target(target);
        }

        match workshop_action {
            Some(WorkshopAction::Search) => self.start_workshop_query(1, ui.ctx()),
            Some(WorkshopAction::GoToPage(page)) => self.start_workshop_query(page, ui.ctx()),
            Some(WorkshopAction::Install(published_file_ids)) => {
                self.start_workshop_install(published_file_ids, ui.ctx());
            }
            Some(WorkshopAction::ToggleSelect(published_file_id)) => {
                if !self.workshop.selected_items.remove(&published_file_id) {
                    self.workshop.selected_items.insert(published_file_id);
                }
            }
            Some(WorkshopAction::ClearSelection) => self.workshop.selected_items.clear(),
            Some(WorkshopAction::Open(target)) => self.open_target(target),
            Some(WorkshopAction::OpenSettings) => self.open_settings(),
            Some(WorkshopAction::RetryCoveredMods) => self.retry_covered_mods(ui.ctx()),
            None => {}
        }

        self.show_mod_deletion_confirmation(ui.ctx());

        for action in self.toasts.show(ui.ctx()) {
            if action == open_settings_action() {
                self.open_settings();
            }
        }

        self.handle_history_shortcuts(ui.ctx());
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/actions.rs"]
mod action_tests;

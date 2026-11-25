use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::PathBuf;

use eframe::egui;
use egui_notify::Toasts;
use nucleo_matcher::Matcher;

use crate::models::{ModCollection, ModId};
use crate::services::mod_loader::{self, ModLoadWarning};
use crate::services::mod_sorter::sort_mods;
use crate::services::organization::{self, Organization};
use crate::services::settings::{Settings, SettingsErrors};
use crate::services::updater::StartupNotice;
use crate::services::workshop;
use crate::services::{load_order, mod_sorter};

use crate::ui::logs::{LogView, show_logs};
use crate::ui::mod_info::{ModInfoAction, show_mod_info};
use crate::ui::mod_list::{ModListKind, ModListView, show_mod_list};
use crate::ui::modpacks::{ModpacksView, show_modpacks};
use crate::ui::settings_window::{SettingsAction, display_settings};
use crate::ui::workshop::{
    CoveredModsState, WorkshopAction, WorkshopTab, WorkshopView, show_workshop,
};
use crate::ui::{OpenTarget, bottom_panel, mod_filters, theme, top_panel};

mod deletion;
mod history;
mod launch;
pub(crate) mod log_ctrl;
mod mod_lists;
mod modpacks_ctrl;
mod organization_ctrl;
mod settings_apply;
mod update_ctrl;
mod watcher;
mod workshop_ctrl;

use self::deletion::PendingModDeletion;
use self::history::LoadOrderSnapshot;
use self::log_ctrl::LogState;
use self::modpacks_ctrl::{ModpackState, open_exported_pack_folder_action};
use self::settings_apply::{path_from_input, path_to_input, persist_settings};
use self::update_ctrl::{UpdateState, open_update_release_action};
use self::watcher::ModWatcherState;
use self::workshop_ctrl::{CoveredModsStatus, SubscriptionStatus, WorkshopState};

const SETTINGS_STORAGE_KEY: &str = "rimmod_settings";
const ACTIVE_MODPACK_STORAGE_KEY: &str = "rimmod_active_modpack";

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum AppTab {
    #[default]
    Mods,
    Workshop,
    Modpacks,
    Log,
}

fn open_settings_action() -> egui::Id {
    egui::Id::new("open_settings")
}

pub(crate) enum LoadOrderStatus {
    Unavailable {
        reason: String,
    },
    /// The load order is written, but some Workshop mods are still downloading.
    Pending {
        reason: String,
    },
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
pub(crate) struct UiState {
    selected_mod_id: Option<ModId>,
    selected_mod_ids: Vec<ModId>,
    selection_anchor_id: Option<ModId>,
    selection_kind: Option<ModListKind>,

    /// Shared search query for both mod columns.
    mod_search: String,
    fuzzy_matcher: Matcher,

    /// Tag filters shared by both mod lists.
    tag_filters: Vec<String>,

    /// Mods being edited in the organize dialog.
    organize_target: Option<Vec<ModId>>,
    organize_tag_input: String,

    pub(crate) settings_errors: SettingsErrors,

    pub(crate) settings_open: bool,
    pub(crate) shortcuts_open: bool,
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
    organization: Organization,
    organization_path: Option<std::path::PathBuf>,
    state: UiState,
    toasts: Toasts,
    action_error: Option<String>,
    game_version: Option<String>,
    load_order_status: LoadOrderStatus,
    mod_watcher: ModWatcherState,
    pending_mod_deletion: Option<PendingModDeletion>,
    undo_history: VecDeque<LoadOrderSnapshot>,
    redo_history: VecDeque<LoadOrderSnapshot>,
    current_tab: AppTab,
    workshop: WorkshopState,
    modpacks: ModpackState,
    logs: LogState,
    update: UpdateState,
}

impl App {
    pub(crate) fn new(cc: &eframe::CreationContext<'_>, startup_notice: StartupNotice) -> Self {
        cc.egui_ctx.options_mut(|options| {
            options.input_options.max_double_click_delay = 0.5;

            options.input_options.max_click_dist = 10.0;
        });

        cc.egui_ctx.all_styles_mut(|style| {
            style.interaction.tooltip_delay = 0.15;
            style.interaction.show_tooltips_only_when_still = false;
        });

        egui_extras::install_image_loaders(&cc.egui_ctx);
        theme::install(&cc.egui_ctx);

        let mut settings = cc
            .storage
            .and_then(|storage| eframe::get_value::<Settings>(storage, SETTINGS_STORAGE_KEY))
            .unwrap_or_default();

        settings.fill_missing_from(Settings::auto_discover());

        let active_pack = cc.storage.and_then(|storage| {
            eframe::get_value::<Option<PathBuf>>(storage, ACTIVE_MODPACK_STORAGE_KEY).flatten()
        });

        let mut app = Self {
            settings,
            toasts: Toasts::default().with_shadow(theme::soft_shadow()),
            organization_path: organization::file_path(),
            ..Self::default()
        };

        app.modpacks.active_pack = active_pack;
        app.ensure_modpack_library_loaded();
        app.load_organization();

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
        app.start_update_check(&cc.egui_ctx);

        if let Some(error) = startup_notice.update_error {
            app.toasts
                .error(format!("Could not update RimMod: {error}"));
        }

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
            OpenTarget::Folder(path) | OpenTarget::File(path) => open::that(path),
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
        self.cancel_pending_watcher_reload();
        self.clear_load_order_history();
        self.logs.invalidate();

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
            Ok(report) => {
                self.clear_action_error();
                self.record_load_order_change(previous);

                let mut notes = Vec::new();

                if report.inferred_edges_applied > 0 {
                    notes.push(format!(
                        "{} inferred {}",
                        report.inferred_edges_applied,
                        if report.inferred_edges_applied == 1 {
                            "rule"
                        } else {
                            "rules"
                        }
                    ));
                }

                let skipped = report.explicit_edges_skipped
                    + report.inferred_edges_skipped
                    + report.dependency_edges_skipped;

                if skipped > 0 {
                    notes.push(format!("{skipped} conflicting skipped"));
                }

                let message = if notes.is_empty() {
                    "Active mods sorted".to_owned()
                } else {
                    format!("Active mods sorted ({})", notes.join(", "))
                };

                self.toasts.success(message);
            }
            Err(error) => {
                self.report_action_error(format!("Could not sort active mods: {error}"));
            }
        }
    }

    fn title_status(&self) -> top_panel::StatusDot {
        if self.action_error.is_some() {
            top_panel::StatusDot::Error
        } else if !self.mod_load_warnings.is_empty() {
            top_panel::StatusDot::Warning
        } else if !matches!(self.load_order_status, LoadOrderStatus::Ready) {
            top_panel::StatusDot::Unavailable
        } else {
            top_panel::StatusDot::Ready
        }
    }

    fn handle_global_shortcuts(&mut self, context: &egui::Context) {
        if self.state.settings_open
            || self.state.shortcuts_open
            || context.egui_wants_keyboard_input()
        {
            return;
        }

        if context.input_mut(|input| input.consume_shortcut(&top_panel::SAVE_SHORTCUT)) {
            self.save_mods();
            return;
        }

        if context.input_mut(|input| input.consume_shortcut(&top_panel::RELOAD_SHORTCUT)) {
            self.reload_mods();
            return;
        }

        if context.input(|input| input.key_pressed(egui::Key::Questionmark)) {
            self.state.shortcuts_open = true;
            return;
        }

        if !self.state.mod_search.is_empty()
            && context.input(|input| input.key_pressed(egui::Key::Escape))
        {
            self.state.mod_search.clear();
            return;
        }

        let tabs = [
            (egui::Key::Num1, AppTab::Mods),
            (egui::Key::Num2, AppTab::Workshop),
            (egui::Key::Num3, AppTab::Modpacks),
            (egui::Key::Num4, AppTab::Log),
        ];

        for (key, tab) in tabs {
            let shortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, key);

            if context.input_mut(|input| input.consume_shortcut(&shortcut)) {
                self.current_tab = tab;
                break;
            }
        }
    }
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_STORAGE_KEY, &self.settings);
        eframe::set_value(
            storage,
            ACTIVE_MODPACK_STORAGE_KEY,
            &self.modpacks.active_pack,
        );

        // Safety net: tags are also written after every edit, but a periodic
        // save keeps an unexpectedly closed app from losing the last change.
        if let Some(path) = self.organization_path.as_deref() {
            let _ = organization::save(&self.organization, path);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.process_mod_watcher(ui.ctx());
        self.poll_workshop_tasks(ui.ctx());
        self.poll_pack_tasks(ui.ctx());
        self.poll_log_tasks(ui.ctx());
        self.poll_update_check();

        self.handle_global_shortcuts(ui.ctx());

        let can_undo = !self.undo_history.is_empty();
        let can_redo = !self.redo_history.is_empty();
        let can_run_with_mods = launch::supports_mod_argument(self.game_version.as_deref());
        let status = self.title_status();
        let available_update = self
            .update
            .available
            .as_ref()
            .map(|update| update.version.as_str());
        let update_launching = self.update.launching;
        let search = if self.current_tab == AppTab::Mods {
            Some(&mut self.state.mod_search)
        } else {
            None
        };

        if let Some(action) = top_panel::show_top_panel(
            ui,
            top_panel::TopPanelView {
                can_undo,
                can_redo,
                can_run_with_mods,
                search,
                status,
                available_update,
                update_launching,
            },
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

                top_panel::TopPanelAction::RunWithCurrentMods => {
                    self.run_game_with_mods();
                }

                top_panel::TopPanelAction::Save => {
                    self.save_mods();
                }

                top_panel::TopPanelAction::Reload => {
                    self.reload_mods();
                }

                top_panel::TopPanelAction::ShowShortcuts => {
                    self.state.shortcuts_open = true;
                }

                top_panel::TopPanelAction::InstallUpdate => {
                    self.start_update_install(ui.ctx());
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
                            self.reset_workshop_data();
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

        let mut mod_info_action = None;
        let mut modpacks_action = None;
        let mut workshop_action = None;
        let steamcmd_workshop_path =
            workshop::steamcmd_workshop_path(self.settings.steamcmd_path());

        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ui, |ui| {
                theme::paint_background_gradient(ui, theme::CANVAS_TOP, theme::CANVAS_BOTTOM);

                egui::Frame::new()
                    .inner_margin(egui::Margin::symmetric(18, 10))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            if theme::tab(ui, self.current_tab == AppTab::Mods, "Mods").clicked() {
                                self.current_tab = AppTab::Mods;
                            }

                            if theme::tab(ui, self.current_tab == AppTab::Workshop, "Workshop")
                                .clicked()
                            {
                                self.current_tab = AppTab::Workshop;
                            }

                            if theme::tab(ui, self.current_tab == AppTab::Modpacks, "Modpacks")
                                .clicked()
                            {
                                self.current_tab = AppTab::Modpacks;
                            }

                            let log_errors = self
                                .logs
                                .report
                                .as_ref()
                                .map_or(0, |report| report.counts.error);
                            let log_count = (log_errors > 0).then(|| log_errors.to_string());

                            if theme::tab_with_count(
                                ui,
                                self.current_tab == AppTab::Log,
                                "Log",
                                log_count,
                            )
                            .clicked()
                            {
                                self.current_tab = AppTab::Log;
                            }
                        });

                        ui.add_space(8.0);

                        match self.current_tab {
                            AppTab::Mods => {
                                self.ensure_subscriptions_started(ui.ctx());

                                let context = ui.ctx().clone();

                                mod_filters::show_mod_filters(
                                    ui,
                                    &self.mods,
                                    &self.organization,
                                    &mut self.state.tag_filters,
                                );
                                ui.add_space(6.0);

                                ui.columns_const(
                                    |[info_column, disabled_column, active_column]| {
                                        let selected_mod = self
                                            .state
                                            .selected_mod_id
                                            .and_then(|mod_id| self.mods.get(mod_id));

                                        let organize_mod_ids =
                                            if !self.state.selected_mod_ids.is_empty() {
                                                self.state.selected_mod_ids.clone()
                                            } else {
                                                self.state.selected_mod_id.into_iter().collect()
                                            };

                                        mod_info_action = show_mod_info(
                                            info_column,
                                            selected_mod,
                                            &self.mods,
                                            &self.organization,
                                            &organize_mod_ids,
                                            self.game_version.as_deref(),
                                        );

                                        let disabled_action = show_mod_list(
                                            disabled_column,
                                            ModListView {
                                                kind: ModListKind::Disabled,
                                                mods: &self.mods,
                                                mod_ids: self.mods.disabled_ids(),
                                                game_version: self.game_version.as_deref(),
                                                order_warnings,
                                                organization: &self.organization,
                                                tag_filters: &mut self.state.tag_filters,
                                                search_string: &mut self.state.mod_search,
                                                matcher: &mut self.state.fuzzy_matcher,
                                                selected_mod_ids: &self.state.selected_mod_ids,
                                                selection_anchor_id: self.state.selection_anchor_id,
                                                selection_kind: self.state.selection_kind,
                                                subscribed_workshop_ids: self
                                                    .workshop
                                                    .subscriptions
                                                    .ids(),
                                                pending_workshop_subscriptions: &self
                                                    .workshop
                                                    .pending_subscriptions,
                                                can_unsubscribe: self
                                                    .settings
                                                    .steam_web_api_key()
                                                    .is_some(),
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
                                                organization: &self.organization,
                                                tag_filters: &mut self.state.tag_filters,
                                                search_string: &mut self.state.mod_search,
                                                matcher: &mut self.state.fuzzy_matcher,
                                                selected_mod_ids: &self.state.selected_mod_ids,
                                                selection_anchor_id: self.state.selection_anchor_id,
                                                selection_kind: self.state.selection_kind,
                                                subscribed_workshop_ids: self
                                                    .workshop
                                                    .subscriptions
                                                    .ids(),
                                                pending_workshop_subscriptions: &self
                                                    .workshop
                                                    .pending_subscriptions,
                                                can_unsubscribe: self
                                                    .settings
                                                    .steam_web_api_key()
                                                    .is_some(),
                                            },
                                        );

                                        if let Some(action) = disabled_action {
                                            self.apply_mod_list_action(action, &context);
                                        }

                                        if let Some(action) = active_action {
                                            self.apply_mod_list_action(action, &context);
                                        }
                                    },
                                );

                                crate::ui::mod_list::show_drag_ghost(ui.ctx(), &self.mods);
                            }
                            AppTab::Modpacks => {
                                self.ensure_modpack_library_loaded();
                                let modal_open = self.state.settings_open
                                    || self.state.shortcuts_open
                                    || self.modpacks.has_open_modal();

                                modpacks_action = show_modpacks(
                                    ui,
                                    ModpacksView {
                                        packs: &self.modpacks.packs,
                                        selected: self.modpacks.selected.as_deref(),
                                        active_pack: self.modpacks.active_pack.as_deref(),
                                        mods: &self.mods,
                                        game_version: self.game_version.as_deref(),
                                        library_error: self.modpacks.library_error.as_deref(),
                                        ui_state: &mut self.modpacks.ui_state,
                                        modal_open,
                                    },
                                );
                            }
                            AppTab::Log => {
                                self.ensure_logs_started(ui.ctx());
                                let data_directory = self
                                    .settings
                                    .config_path()
                                    .and_then(|config_path| config_path.parent());

                                let logs = &mut self.logs;

                                let log_action = show_logs(
                                    ui,
                                    LogView {
                                        sessions: &logs.sessions,
                                        session: logs.session,
                                        status: &logs.status,
                                        report: logs.report.as_ref(),
                                        mods: &self.mods,
                                        filter: &mut logs.filter,
                                        selected_issue: &mut logs.selected_issue,
                                        auto_refresh: &mut logs.auto_refresh,
                                        data_directory,
                                    },
                                );

                                if let Some(action) = log_action {
                                    self.apply_log_action(action, ui.ctx());
                                }
                            }
                            AppTab::Workshop => {
                                self.ensure_workshop_started(ui.ctx());
                                self.ensure_covered_mods(ui.ctx());

                                // Borrow only the coverage field so the view can still
                                // mutably borrow the other WorkshopState fields.
                                let covered_mods_state = match &self.workshop.covered_mods {
                                    CoveredModsStatus::Ready(covered_mods) => {
                                        CoveredModsState::Ready(covered_mods)
                                    }
                                    CoveredModsStatus::Failed(error) => {
                                        CoveredModsState::Failed(error.clone())
                                    }
                                    CoveredModsStatus::NotRequested
                                    | CoveredModsStatus::Loading(_) => CoveredModsState::Loading,
                                };

                                workshop_action = show_workshop(
                                    ui,
                                    WorkshopView {
                                        tab: self.workshop.tab,
                                        query: &mut self.workshop.query,
                                        subscribed_query: &mut self.workshop.subscribed_query,
                                        sort: &mut self.workshop.sort,
                                        subscribed_sort: &mut self.workshop.subscribed_sort,
                                        versions: &mut self.workshop.versions,
                                        dlc: &mut self.workshop.dlc,
                                        my_version_only: &mut self.workshop.my_version_only,
                                        hide_subscribed: &mut self.workshop.hide_subscribed,
                                        not_installed_only: &mut self.workshop.not_installed_only,
                                        page: self.workshop.page,
                                        subscribed_page: &mut self.workshop.subscribed_page,
                                        total: self.workshop.total,
                                        items: &self.workshop.items,
                                        load_status: &self.workshop.load_status,
                                        installing_items: &self.workshop.installing_items,
                                        workshop_path: self.settings.workshop_path(),
                                        steamcmd_workshop_path: steamcmd_workshop_path.as_deref(),
                                        steamcmd_available: workshop::steamcmd_available(
                                            self.settings.steamcmd_path(),
                                        ),
                                        api_key_configured: self
                                            .settings
                                            .steam_web_api_key()
                                            .is_some(),
                                        mods: &self.mods,
                                        game_version: self.game_version.as_deref(),
                                        covered_mods_state,
                                        subscribed_items: &self.workshop.subscribed_items,
                                        subscribed_load_status: &self.workshop.subscribed_load,
                                        subscribed_ids: self.workshop.subscriptions.ids(),
                                        subscriptions_loading: matches!(
                                            &self.workshop.subscriptions,
                                            SubscriptionStatus::Loading
                                        ),
                                        subscriptions_error: self.workshop.subscriptions.error(),
                                        pending_subscriptions: &self.workshop.pending_subscriptions,
                                        grid_generation: self.workshop.grid_generation,
                                        subscribed_generation: self.workshop.subscribed_generation,
                                        detail: self.workshop.detail.as_ref(),
                                        detail_image: self.workshop.detail_image.as_ref(),
                                    },
                                );
                            }
                        }
                    });
            });

        match mod_info_action {
            Some(ModInfoAction::Open(target)) => self.open_target(target),
            Some(ModInfoAction::Organize { mod_ids }) => self.open_organize(mod_ids),
            None => {}
        }

        if let Some(action) = modpacks_action {
            self.handle_modpacks_action(action, ui.ctx());
        }

        match workshop_action {
            Some(WorkshopAction::Search) => {
                self.search_workshop(ui.ctx());
            }
            Some(WorkshopAction::GoToPage(page)) => match self.workshop.tab {
                WorkshopTab::Browse => self.start_workshop_query(page, ui.ctx()),
                WorkshopTab::Subscribed => self.workshop.subscribed_page = page.max(1),
            },
            Some(WorkshopAction::SwitchTab(tab)) => {
                self.workshop.tab = tab;

                if tab == WorkshopTab::Subscribed
                    && matches!(
                        &self.workshop.subscriptions,
                        SubscriptionStatus::NotLoaded | SubscriptionStatus::Failed(_)
                    )
                {
                    self.start_subscription_load(ui.ctx());
                }
            }
            Some(WorkshopAction::Install(published_file_id)) => {
                self.start_workshop_install(vec![published_file_id], ui.ctx());
            }
            Some(WorkshopAction::Subscribe(published_file_id)) => {
                self.set_subscription(published_file_id, true, ui.ctx());
            }
            Some(WorkshopAction::Unsubscribe(published_file_id)) => {
                self.set_subscription(published_file_id, false, ui.ctx());
            }
            Some(WorkshopAction::RefreshSubscriptions) => {
                self.start_subscription_load(ui.ctx());
            }
            Some(WorkshopAction::Open(target)) => self.open_target(target),
            Some(WorkshopAction::OpenSettings) => self.open_settings(),
            Some(WorkshopAction::RetryCoveredMods) => self.retry_covered_mods(ui.ctx()),
            Some(WorkshopAction::OpenDetail(item)) => {
                self.workshop.detail = Some(*item);
                self.workshop.detail_image = None;
            }
            Some(WorkshopAction::CloseDetail) => {
                self.workshop.detail = None;
                self.workshop.detail_image = None;
            }
            Some(WorkshopAction::ShowImage(viewer)) => {
                self.workshop.detail_image = Some(viewer);
            }
            Some(WorkshopAction::CloseImage) => {
                self.workshop.detail_image = None;
            }
            Some(WorkshopAction::Copy { text, label }) => {
                ui.ctx().copy_text(text);
                self.toasts.info(format!("{label} copied to clipboard"));
            }
            None => {}
        }

        self.show_mod_deletion_confirmation(ui.ctx());
        self.show_organize_dialog(ui.ctx());
        self.show_pack_editor(ui.ctx());
        self.show_drop_preview_modal(ui.ctx());
        self.show_pack_apply_confirmation(ui.ctx());
        self.show_pack_delete_confirmation(ui.ctx());
        self.show_pack_progress_dialog(ui.ctx());
        self.handle_file_drop(ui.ctx());

        for action in self.toasts.show(ui.ctx()) {
            if action == open_settings_action() {
                self.open_settings();
            } else if action == open_update_release_action() {
                self.open_update_release_page();
            } else if action == open_exported_pack_folder_action() {
                self.open_last_exported_pack_folder();
            }
        }

        self.handle_history_shortcuts(ui.ctx());

        crate::ui::shortcuts::show_shortcuts(ui.ctx(), &mut self.state.shortcuts_open);
        crate::ui::window_resize::show_resize_handles(ui.ctx());
    }
}

#[cfg(test)]
#[path = "../tests/unit/settings.rs"]
mod tests;

#[cfg(test)]
#[path = "../tests/unit/actions.rs"]
mod action_tests;

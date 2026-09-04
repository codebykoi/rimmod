use std::collections::{HashMap, VecDeque};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use std::process::Command;

use eframe::egui::{self, Color32, Shadow};
use egui_notify::Toasts;
use nucleo_matcher::Matcher;

use crate::models::{ModCollection, ModId};
use crate::services::mod_loader::{self, ModLoadWarning};
use crate::services::mod_sorter::sort_mods;
use crate::services::mod_watcher::{ModWatcher, ModWatcherPoll};
use crate::services::settings::{Settings, SettingsErrors};
use crate::services::{load_order, mod_sorter};

use crate::ui::mod_info::show_mod_info;
use crate::ui::mod_list::{ModListAction, ModListKind, ModListView, SelectionMode, show_mod_list};
use crate::ui::settings_window::{SettingsAction, display_settings};
use crate::ui::{OpenTarget, bottom_panel, top_panel};

const SETTINGS_STORAGE_KEY: &str = "rimmod_settings";
const MOD_RELOAD_DEBOUNCE: Duration = Duration::from_millis(750);
const LOAD_ORDER_HISTORY_LIMIT: usize = 100;

#[cfg(target_os = "windows")]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &["RimWorldWin64.exe", "RimWorldWin.exe"];

#[cfg(target_os = "linux")]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &["RimWorldLinux", "RimWorldLinux.x86_64"];

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &[];

fn open_settings_action() -> egui::Id {
    egui::Id::new("open_settings")
}

fn find_rimworld_executable(rimworld_folder: &Path) -> io::Result<PathBuf> {
    if RIMWORLD_EXECUTABLE_NAMES.is_empty() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            format!(
                "running RimWorld is not supported on {}",
                std::env::consts::OS,
            ),
        ));
    }

    for executable_name in RIMWORLD_EXECUTABLE_NAMES {
        let executable = rimworld_folder.join(executable_name);

        if executable.is_file() {
            return Ok(executable);
        }
    }

    Err(io::Error::new(
        io::ErrorKind::NotFound,
        format!(
            "RimWorld executable was not found in {}; expected one of: {}",
            rimworld_folder.display(),
            RIMWORLD_EXECUTABLE_NAMES.join(", "),
        ),
    ))
}

fn start_rimworld(rimworld_folder: &Path) -> io::Result<()> {
    let executable = find_rimworld_executable(rimworld_folder)?;

    Command::new(executable)
        .current_dir(rimworld_folder)
        .spawn()
        .map(|_child| ())
}

pub(crate) enum LoadOrderStatus {
    Unavailable { reason: String },
    Ready,
}

struct PreparedSettings {
    mods: ModCollection,
    mod_load_warnings: Vec<ModLoadWarning>,
    game_version: String,
}

fn prepare_settings(settings: &Settings) -> Result<PreparedSettings, SettingsErrors> {
    let mut errors = settings.validate_paths();

    if errors.has_errors() {
        return Err(errors);
    }

    let game_version = match settings.rimworld_path() {
        Some(rimworld_path) => match App::load_game_version(rimworld_path) {
            Ok(version) if !version.is_empty() => version,
            Ok(_) => {
                errors.rimworld = Some("RimWorld's Version.txt is empty".to_owned());
                return Err(errors);
            }
            Err(error) => {
                errors.rimworld = Some(format!("Could not read RimWorld version: {error}"));
                return Err(errors);
            }
        },
        None => {
            errors.rimworld = Some("Choose the RimWorld installation folder".to_owned());
            return Err(errors);
        }
    };

    if let Some(config_path) = settings.config_path()
        && let Err(error) = load_order::parse_config(config_path)
    {
        errors.config = Some(format!("Could not read ModsConfig.xml: {error}"));
        return Err(errors);
    }

    let mod_load_report = match mod_loader::load_mods(settings, Some(&game_version)) {
        Ok(report) => report,
        Err(error) => {
            errors.general = Some(format!("Could not load mods with these settings: {error}"));
            return Err(errors);
        }
    };

    Ok(PreparedSettings {
        mods: mod_load_report.mods,
        mod_load_warnings: mod_load_report.warnings,
        game_version,
    })
}

fn persist_settings(storage: &mut dyn eframe::Storage, settings: &Settings) {
    eframe::set_value(storage, SETTINGS_STORAGE_KEY, settings);
    storage.flush();
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
}

#[derive(Clone, PartialEq, Eq)]
struct LoadOrderSnapshot {
    disabled_mod_ids: Vec<ModId>,
    enabled_mod_ids: Vec<ModId>,
    selected_mod_id: Option<ModId>,
    selected_mod_ids: Vec<ModId>,
    selection_anchor_id: Option<ModId>,
    selection_kind: Option<ModListKind>,
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
    undo_history: VecDeque<LoadOrderSnapshot>,
    redo_history: VecDeque<LoadOrderSnapshot>,
}

impl App {
    fn clear_mod_selection(&mut self) {
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

    fn load_order_snapshot(&self) -> LoadOrderSnapshot {
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

    fn record_load_order_change(&mut self, previous: LoadOrderSnapshot) {
        let order_changed = previous.disabled_mod_ids != self.mods.disabled_ids()
            || previous.enabled_mod_ids != self.mods.enabled_ids();

        if !order_changed {
            return;
        }

        Self::push_history_entry(&mut self.undo_history, previous);
        self.redo_history.clear();
    }

    fn clear_load_order_history(&mut self) {
        self.undo_history.clear();
        self.redo_history.clear();
    }

    fn undo_load_order_change(&mut self) {
        let Some(previous) = self.undo_history.pop_back() else {
            return;
        };

        let current = self.load_order_snapshot();
        Self::push_history_entry(&mut self.redo_history, current);
        self.restore_load_order_snapshot(previous);
        self.clear_action_error();
        self.toasts.info("Load-order change undone");
    }

    fn redo_load_order_change(&mut self) {
        let Some(next) = self.redo_history.pop_back() else {
            return;
        };

        let current = self.load_order_snapshot();
        Self::push_history_entry(&mut self.undo_history, current);
        self.restore_load_order_snapshot(next);
        self.clear_action_error();
        self.toasts.info("Load-order change redone");
    }

    fn handle_history_shortcuts(&mut self, context: &egui::Context) {
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

        self.state.settings_errors = SettingsErrors::default();

        self.state.settings_open = true;
    }

    fn try_apply_settings(&mut self, candidate: Settings) -> Result<(), SettingsErrors> {
        let prepared = prepare_settings(&candidate)?;

        self.settings = candidate;
        self.mods = prepared.mods;
        self.mod_load_warnings = prepared.mod_load_warnings;
        self.game_version = Some(prepared.game_version);
        self.load_order_status = LoadOrderStatus::Ready;
        self.clear_mod_selection();
        self.clear_load_order_history();

        Ok(())
    }

    fn apply_mod_list_action(&mut self, action: ModListAction) {
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
        }
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

    fn restart_mod_watcher(&mut self, context: &egui::Context) {
        self.mod_watcher = None;
        self.pending_mod_reload_at = None;

        let paths = [
            self.settings.official_path(),
            self.settings.local_path(),
            self.settings.workshop_path().map(Path::to_path_buf),
        ]
        .into_iter()
        .flatten()
        .filter(|path| path.is_dir())
        .collect::<Vec<_>>();

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

    fn process_mod_watcher(&mut self, context: &egui::Context) {
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

    pub(crate) fn load_game_version(rimworld_path: &Path) -> io::Result<String> {
        let version_path = rimworld_path.join("Version.txt");
        let version = std::fs::read_to_string(version_path)?;

        Ok(version.trim().to_owned())
    }

    fn run_game(&mut self) {
        let result = match self.settings.rimworld_path() {
            Some(rimworld_folder) => start_rimworld(rimworld_folder),

            None => Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "RimWorld installation path is not configured",
            )),
        };

        match result {
            Ok(()) => {
                self.clear_action_error();
                self.toasts.info("RimWorld started".to_owned());
            }
            Err(error) => {
                let message = format!("Could not start RimWorld: {error}");
                self.action_error = Some(message.clone());
                self.toasts
                    .error(message)
                    .click_action(open_settings_action());
            }
        };
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

fn path_from_input(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn path_to_input(path: Option<&Path>) -> String {
    path.map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl eframe::App for App {
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, SETTINGS_STORAGE_KEY, &self.settings);
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.process_mod_watcher(ui.ctx());

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

        egui::CentralPanel::default().show(ui, |ui| {
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
        });

        if let Some(target) = requested_open {
            self.open_target(target);
        }

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

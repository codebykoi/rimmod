use std::path::{Path, PathBuf};

use crate::models::ModCollection;
use crate::services::load_order;
use crate::services::mod_loader::{self, ModLoadWarning};
use crate::services::settings::{Settings, SettingsErrors};

use super::{App, LoadOrderStatus, SETTINGS_STORAGE_KEY};

pub(super) struct PreparedSettings {
    mods: ModCollection,
    mod_load_warnings: Vec<ModLoadWarning>,
    game_version: String,
}

pub(super) fn prepare_settings(settings: &Settings) -> Result<PreparedSettings, SettingsErrors> {
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

pub(super) fn persist_settings(storage: &mut dyn eframe::Storage, settings: &Settings) {
    eframe::set_value(storage, SETTINGS_STORAGE_KEY, settings);
    storage.flush();
}

pub(super) fn path_from_input(input: &str) -> Option<PathBuf> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

pub(super) fn path_to_input(path: Option<&Path>) -> String {
    path.map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_default()
}

impl App {
    pub(super) fn try_apply_settings(&mut self, candidate: Settings) -> Result<(), SettingsErrors> {
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
}

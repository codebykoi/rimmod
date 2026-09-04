// This module is probably for saving and managing the app settings, like paths and etc

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

const RIMWORLD_APP_ID: u32 = 294_100;
const RIMWORLD_CONFIG_COMPONENTS: &[&str] =
    &["Ludeon Studios", "RimWorld by Ludeon Studios", "Config"];

#[derive(Debug, Default, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Settings {
    pub(crate) rimworld_path: Option<PathBuf>,

    pub(crate) workshop_path: Option<PathBuf>,

    pub(crate) config_path: Option<PathBuf>,
}

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct SettingsErrors {
    pub(crate) rimworld: Option<String>,
    pub(crate) workshop: Option<String>,
    pub(crate) config: Option<String>,
    pub(crate) general: Option<String>,
}

impl SettingsErrors {
    pub(crate) fn has_errors(&self) -> bool {
        self.rimworld.is_some()
            || self.workshop.is_some()
            || self.config.is_some()
            || self.general.is_some()
    }
}

#[cfg(any(target_os = "windows", test))]
fn windows_config_path(user_profile: &Path) -> PathBuf {
    user_profile
        .join("AppData")
        .join("LocalLow")
        .join(RIMWORLD_CONFIG_COMPONENTS[0])
        .join(RIMWORLD_CONFIG_COMPONENTS[1])
        .join(RIMWORLD_CONFIG_COMPONENTS[2])
}

#[cfg(any(target_os = "linux", test))]
fn linux_config_path(config_home: Option<PathBuf>, home: Option<PathBuf>) -> Option<PathBuf> {
    let config_root = config_home
        .filter(|value| !value.as_os_str().is_empty())
        .or_else(|| home.map(|home| home.join(".config")))?;

    Some(
        config_root
            .join("unity3d")
            .join(RIMWORLD_CONFIG_COMPONENTS[0])
            .join(RIMWORLD_CONFIG_COMPONENTS[1])
            .join(RIMWORLD_CONFIG_COMPONENTS[2]),
    )
}

#[cfg(target_os = "windows")]
fn discover_config_path() -> Option<PathBuf> {
    use std::env;

    let user_profile = env::var_os("USERPROFILE")?;

    Some(windows_config_path(Path::new(&user_profile)))
}

#[cfg(target_os = "linux")]
fn discover_config_path() -> Option<PathBuf> {
    linux_config_path(
        std::env::var_os("XDG_CONFIG_HOME").map(PathBuf::from),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
fn discover_config_path() -> Option<PathBuf> {
    None
}

#[cfg(test)]
#[path = "../../tests/unit/settings_discovery.rs"]
mod tests;

impl Settings {
    pub(crate) fn auto_discover() -> Self {
        let mut discovered = Self {
            config_path: discover_config_path().filter(|path| path.is_dir()),
            ..Self::default()
        };

        let steam_dirs = match steamlocate::locate_all() {
            Ok(steam_dirs) => steam_dirs,
            Err(_) => return discovered,
        };

        for steam_dir in steam_dirs {
            let found_app = match steam_dir.find_app(RIMWORLD_APP_ID) {
                Ok(found_app) => found_app,
                Err(_) => continue,
            };

            let Some((app, library)) = found_app else {
                continue;
            };

            let rimworld_path = library.resolve_app_dir(&app);

            let workshop_path = library
                .path()
                .join("steamapps")
                .join("workshop")
                .join("content")
                .join(RIMWORLD_APP_ID.to_string());

            discovered.rimworld_path = rimworld_path.is_dir().then_some(rimworld_path);

            discovered.workshop_path = workshop_path.is_dir().then_some(workshop_path);

            break;
        }

        discovered
    }

    pub(crate) fn fill_missing_from(&mut self, discovered: Self) {
        if self.rimworld_path.is_none() {
            self.rimworld_path = discovered.rimworld_path;
        }

        if self.workshop_path.is_none() {
            self.workshop_path = discovered.workshop_path;
        }

        if self.config_path.is_none() {
            self.config_path = discovered.config_path;
        }
    }

    pub(crate) fn validate_paths(&self) -> SettingsErrors {
        let mut errors = SettingsErrors::default();

        match self.rimworld_path() {
            None => {
                errors.rimworld = Some("Choose the RimWorld installation folder".to_owned());
            }
            Some(path) if !path.is_dir() => {
                errors.rimworld = Some(format!(
                    "The RimWorld folder does not exist or is not accessible: {}",
                    path.display()
                ));
            }
            Some(path) if !path.join("Data").is_dir() => {
                errors.rimworld =
                    Some("The selected folder does not contain RimWorld's Data folder".to_owned());
            }
            Some(path) if !path.join("Mods").is_dir() => {
                errors.rimworld =
                    Some("The selected folder does not contain RimWorld's Mods folder".to_owned());
            }
            Some(path) if !path.join("Version.txt").is_file() => {
                errors.rimworld =
                    Some("The selected folder does not contain RimWorld's Version.txt".to_owned());
            }
            Some(_) => {}
        }

        // Workshop mods are optional. The loader reports a non-fatal warning
        // when this path is missing or unavailable.

        match self.config_path() {
            None => {
                errors.config = Some("Choose the RimWorld configuration folder".to_owned());
            }
            Some(path) if !path.is_dir() => {
                errors.config = Some(format!(
                    "The configuration folder does not exist or is not accessible: {}",
                    path.display()
                ));
            }
            Some(path) if !path.join("ModsConfig.xml").is_file() => {
                errors.config =
                    Some("The selected folder does not contain ModsConfig.xml".to_owned());
            }
            Some(_) => {}
        }

        errors
    }

    pub(crate) fn official_path(&self) -> Option<PathBuf> {
        self.rimworld_path.as_ref().map(|path| path.join("Data"))
    }

    pub(crate) fn local_path(&self) -> Option<PathBuf> {
        self.rimworld_path.as_ref().map(|path| path.join("Mods"))
    }

    pub(crate) fn rimworld_path(&self) -> Option<&Path> {
        self.rimworld_path.as_deref()
    }

    pub(crate) fn workshop_path(&self) -> Option<&Path> {
        self.workshop_path.as_deref()
    }

    pub(crate) fn config_path(&self) -> Option<&Path> {
        self.config_path.as_deref()
    }
}

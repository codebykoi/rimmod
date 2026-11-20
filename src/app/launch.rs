use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::models::{ModSource, PackageId, major_minor_version};
use crate::services::settings::Settings;

use super::{App, open_settings_action};

#[cfg(target_os = "windows")]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] =
    &["RimWorld.exe", "RimWorldWin64.exe", "RimWorldWin.exe"];

#[cfg(target_os = "linux")]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &["RimWorldLinux", "RimWorldLinux.x86_64"];

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &[];

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

fn start_rimworld(rimworld_folder: &Path, arguments: &[String]) -> io::Result<()> {
    let executable = find_rimworld_executable(rimworld_folder)?;

    Command::new(executable)
        .args(arguments)
        .current_dir(rimworld_folder)
        .spawn()
        .map(|_child| ())
}

/// Windows caps a process command line at 32,767 characters.
const MAX_COMMAND_LINE_LEN: usize = 30_000;

/// RimWorld 1.7 added `-mod=<package IDs>`, which starts the game with an
/// explicit mod list instead of the one stored in `ModsConfig.xml`.
pub(super) fn supports_mod_argument(game_version: Option<&str>) -> bool {
    game_version
        .and_then(major_minor_version)
        .is_some_and(|version| version >= (1, 7))
}

/// Build the `-mod=` argument. The game splits the list on `,` or `;` and
/// always loads the base game and installed DLCs on its own.
fn mod_argument(package_ids: &[PackageId]) -> Option<String> {
    if package_ids.is_empty() {
        return None;
    }

    let joined = package_ids
        .iter()
        .map(PackageId::as_str)
        .collect::<Vec<_>>()
        .join(",");

    Some(format!("-mod={joined}"))
}

impl App {
    pub(super) fn load_game_version(rimworld_path: &Path) -> io::Result<String> {
        let version_path = rimworld_path.join("Version.txt");
        let version = std::fs::read_to_string(version_path)?;

        Ok(version.trim().to_owned())
    }

    pub(super) fn run_game(&mut self) {
        let result = start_with_arguments(&self.settings, &[]);

        self.report_launch_result(result);
    }

    /// Launch with the in-memory mod list instead of the last saved one.
    ///
    /// Only installed mods are passed: `-mod=` treats an unknown package ID as
    /// a hard error rather than the missing-mod warning the config file gets.
    pub(super) fn run_game_with_mods(&mut self) {
        let package_ids = self
            .mods
            .enabled_ids()
            .iter()
            .filter_map(|&mod_id| self.mods.get(mod_id))
            .filter(|rimworld_mod| !matches!(rimworld_mod.source, ModSource::Official))
            .map(|rimworld_mod| rimworld_mod.package_id.clone())
            .collect::<Vec<_>>();

        let argument = mod_argument(&package_ids);

        if argument
            .as_ref()
            .is_some_and(|argument| argument.len() > MAX_COMMAND_LINE_LEN)
        {
            self.report_action_error(
                "The active mod list is too long for the -mod launch argument; \
                 save the load order and use Run instead",
            );
            return;
        }

        let arguments = argument.into_iter().collect::<Vec<_>>();
        let result = start_with_arguments(&self.settings, &arguments);

        self.report_launch_result(result);
    }

    fn report_launch_result(&mut self, result: io::Result<()>) {
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
        }
    }
}

fn start_with_arguments(settings: &Settings, arguments: &[String]) -> io::Result<()> {
    match settings.rimworld_path() {
        Some(rimworld_folder) => start_rimworld(rimworld_folder, arguments),

        None => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "RimWorld installation path is not configured",
        )),
    }
}

#[cfg(test)]
#[path = "../../tests/unit/launch.rs"]
mod tests;

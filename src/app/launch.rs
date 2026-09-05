use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{App, open_settings_action};

#[cfg(target_os = "windows")]
const RIMWORLD_EXECUTABLE_NAMES: &[&str] = &["RimWorldWin64.exe", "RimWorldWin.exe"];

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

fn start_rimworld(rimworld_folder: &Path) -> io::Result<()> {
    let executable = find_rimworld_executable(rimworld_folder)?;

    Command::new(executable)
        .current_dir(rimworld_folder)
        .spawn()
        .map(|_child| ())
}

impl App {
    pub(super) fn load_game_version(rimworld_path: &Path) -> io::Result<String> {
        let version_path = rimworld_path.join("Version.txt");
        let version = std::fs::read_to_string(version_path)?;

        Ok(version.trim().to_owned())
    }

    pub(super) fn run_game(&mut self) {
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
}

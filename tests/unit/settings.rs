use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use crate::services::settings::Settings;

use super::settings_apply::{persist_settings, prepare_settings};
use super::*;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rimmod-settings-test-{}-{unique_number}",
            std::process::id()
        ));

        fs::create_dir(&path)?;

        Ok(Self { path })
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

#[derive(Default)]
struct MemoryStorage {
    values: HashMap<String, String>,
    was_flushed: bool,
}

impl eframe::Storage for MemoryStorage {
    fn get_string(&self, key: &str) -> Option<String> {
        self.values.get(key).cloned()
    }

    fn set_string(&mut self, key: &str, value: String) {
        self.values.insert(key.to_owned(), value);
    }

    fn remove_string(&mut self, key: &str) {
        self.values.remove(key);
    }

    fn flush(&mut self) {
        self.was_flushed = true;
    }
}

fn create_valid_settings() -> io::Result<(TestDirectory, Settings)> {
    let test_directory = TestDirectory::new()?;
    let rimworld_path = test_directory.path.join("RimWorld");
    let workshop_path = test_directory.path.join("Workshop");
    let config_path = test_directory.path.join("Config");

    fs::create_dir_all(rimworld_path.join("Data"))?;
    fs::create_dir_all(rimworld_path.join("Mods"))?;
    fs::create_dir_all(&workshop_path)?;
    fs::create_dir_all(&config_path)?;

    fs::write(rimworld_path.join("Version.txt"), "1.6.1234 rev1")?;
    fs::write(
        config_path.join("ModsConfig.xml"),
        "<ModsConfigData><activeMods></activeMods></ModsConfigData>",
    )?;

    let settings = Settings {
        rimworld_path: Some(rimworld_path),
        workshop_path: Some(workshop_path),
        config_path: Some(config_path),
        ..Settings::default()
    };

    Ok((test_directory, settings))
}

#[test]
fn empty_settings_report_required_paths() {
    let errors = Settings::default().validate_paths();

    assert!(errors.rimworld.is_some());
    assert!(errors.workshop.is_none());
    assert!(errors.config.is_some());
    assert!(errors.has_errors());
}

#[test]
fn applying_valid_settings_reloads_application_data() -> io::Result<()> {
    let (_test_directory, candidate) = create_valid_settings()?;
    let mut app = App::default();

    app.try_apply_settings(candidate.clone())
        .map_err(|errors| io::Error::other(format!("{errors:?}")))?;

    assert_eq!(app.settings, candidate);
    assert_eq!(app.game_version.as_deref(), Some("1.6.1234 rev1"));
    assert!(app.load_order_status.can_save());

    Ok(())
}

#[test]
fn failed_settings_application_preserves_active_settings() -> io::Result<()> {
    let (_test_directory, original_settings) = create_valid_settings()?;
    let mut app = App::default();

    app.try_apply_settings(original_settings.clone())
        .map_err(|errors| io::Error::other(format!("{errors:?}")))?;

    let result = app.try_apply_settings(Settings::default());

    assert!(result.is_err());
    assert_eq!(app.settings, original_settings);
    assert!(app.load_order_status.can_save());

    Ok(())
}

#[test]
fn failed_mod_reload_prevents_save_and_preserves_mods_config() -> io::Result<()> {
    let (_test_directory, settings) = create_valid_settings()?;
    let rimworld_path = settings
        .rimworld_path()
        .ok_or_else(|| io::Error::other("test RimWorld path is missing"))?;
    let local_mods_path = rimworld_path.join("Mods");
    let config_path = settings
        .config_path()
        .ok_or_else(|| io::Error::other("test config path is missing"))?;
    let config_file = config_path.join("ModsConfig.xml");
    let backup_file = config_path.join("ModsConfig.rimmod-backup.xml");
    let original_xml = concat!(
        "<ModsConfigData><activeMods>",
        "<li>ludeon.rimworld</li>",
        "</activeMods></ModsConfigData>"
    );

    let about_path = rimworld_path.join("Data").join("Core").join("About");
    fs::create_dir_all(&about_path)?;
    fs::write(
        about_path.join("About.xml"),
        concat!(
            "<ModMetaData>",
            "<name>Core</name>",
            "<packageId>ludeon.rimworld</packageId>",
            "</ModMetaData>"
        ),
    )?;
    fs::write(&config_file, original_xml)?;

    let mut app = App {
        settings,
        ..App::default()
    };

    // First establish a known-good load order containing one active mod.
    app.reload_mods();
    assert!(app.load_order_status.can_save());
    assert_eq!(app.mods.enabled_ids().len(), 1);

    // Removing a required directory makes the next reload fail. The previous
    // collection remains available for display, but it must no longer be saved.
    fs::remove_dir(&local_mods_path)?;
    app.reload_mods();
    assert!(!app.load_order_status.can_save());
    assert_eq!(app.mods.enabled_ids().len(), 1);

    // Make the in-memory order differ from the file so this test would catch a
    // save that accidentally slipped past the unavailable-state guard.
    let active_mod_id = app.mods.enabled_ids()[0];
    assert!(app.mods.disable_many(&[active_mod_id], None).is_ok());
    app.save_mods();

    assert_eq!(fs::read_to_string(&config_file)?, original_xml);
    assert!(!backup_file.exists());

    Ok(())
}

#[test]
fn malformed_mods_config_is_reported_as_a_config_error() -> io::Result<()> {
    let (_test_directory, settings) = create_valid_settings()?;
    let config_path = settings
        .config_path()
        .ok_or_else(|| io::Error::other("test config path is missing"))?;

    fs::write(config_path.join("ModsConfig.xml"), "<not-valid-xml>")?;

    let errors = match prepare_settings(&settings) {
        Ok(_) => return Err(io::Error::other("invalid XML was accepted")),
        Err(errors) => errors,
    };

    assert!(errors.config.is_some());
    assert!(errors.general.is_none());

    Ok(())
}

#[test]
fn persisted_settings_can_be_loaded_again() -> io::Result<()> {
    let (_test_directory, settings) = create_valid_settings()?;
    let mut storage = MemoryStorage::default();

    persist_settings(&mut storage, &settings);

    let restored = eframe::get_value::<Settings>(&storage, SETTINGS_STORAGE_KEY)
        .ok_or_else(|| io::Error::other("settings were not stored"))?;

    assert_eq!(restored, settings);
    assert!(storage.was_flushed);

    Ok(())
}

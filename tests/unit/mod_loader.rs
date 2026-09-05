use std::sync::atomic::{AtomicU64, Ordering};

use super::*;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rimmod-mod-loader-test-{}-{unique_number}",
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

fn write_about_xml(mod_folder: &Path, xml: &str) -> io::Result<()> {
    let about_folder = mod_folder.join("About");
    fs::create_dir_all(&about_folder)?;
    fs::write(about_folder.join("About.xml"), xml)
}

#[test]
fn invalid_mod_is_reported_while_valid_mod_is_loaded() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let valid_mod_folder = mods_folder.join("ValidMod");
    let invalid_mod_folder = mods_folder.join("InvalidMod");

    write_about_xml(
        &valid_mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Valid Mod</name>",
            "<packageId>example.valid</packageId>",
            "</ModMetaData>"
        ),
    )?;
    write_about_xml(&invalid_mod_folder, "<not-valid-xml>")?;

    let load = load_mods_from(&mods_folder, ModRoot::Local)?;

    assert_eq!(load.mods.len(), 1);
    assert_eq!(load.mods[0].package_id.as_str(), "example.valid");
    assert_eq!(load.warnings.len(), 1);
    assert!(load.warnings[0].is_skipped_mod());
    assert_eq!(
        load.warnings[0].path.as_deref(),
        Some(invalid_mod_folder.as_path())
    );

    Ok(())
}

#[test]
fn supported_versions_are_loaded_from_about_xml() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let mod_folder = mods_folder.join("VersionedMod");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Versioned Mod</name>",
            "<packageId>example.versioned</packageId>",
            "<supportedVersions><li>1.5</li><li> 1.6 </li></supportedVersions>",
            "</ModMetaData>"
        ),
    )?;

    let load = load_mods_from(&mods_folder, ModRoot::Local)?;

    assert_eq!(load.mods.len(), 1);
    assert_eq!(load.mods[0].supported_versions, ["1.5", "1.6"]);

    Ok(())
}

#[test]
fn no_version_warning_adds_reported_support_for_the_current_version() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let reported_mod_folder = mods_folder.join("ReportedMod");
    let no_version_warning_folder = mods_folder.join("NoVersionWarning");

    write_about_xml(
        &reported_mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Reported Mod</name>",
            "<packageId>Example.Reported</packageId>",
            "<supportedVersions><li>1.5</li></supportedVersions>",
            "</ModMetaData>"
        ),
    )?;
    write_about_xml(
        &no_version_warning_folder,
        concat!(
            "<ModMetaData>",
            "<name>No Version Warning</name>",
            "<packageId>Mlie.NoVersionWarning</packageId>",
            "</ModMetaData>"
        ),
    )?;

    let compatibility_folder = no_version_warning_folder.join("1.6");
    fs::create_dir_all(&compatibility_folder)?;
    fs::write(
        compatibility_folder.join("ModIdsToFix.xml"),
        "\u{feff}<ModIdsToFix><li>example.reported</li></ModIdsToFix>",
    )?;

    let mut load = load_mods_from(&mods_folder, ModRoot::Local)?;
    apply_no_version_warning_support(&mut load.mods, Some("1.6.1234 rev1"))?;

    let reported_mod = load
        .mods
        .iter()
        .find(|rimworld_mod| rimworld_mod.package_id.as_str() == "example.reported")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reported mod was not loaded"))?;

    assert_eq!(reported_mod.supported_versions, ["1.5"]);
    assert_eq!(reported_mod.community_supported_versions, ["1.6"]);

    Ok(())
}

#[test]
fn no_version_warning_does_not_duplicate_official_support() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let reported_mod_folder = mods_folder.join("ReportedMod");
    let no_version_warning_folder = mods_folder.join("NoVersionWarning");

    write_about_xml(
        &reported_mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Reported Mod</name>",
            "<packageId>example.reported</packageId>",
            "<supportedVersions><li>1.6</li></supportedVersions>",
            "</ModMetaData>"
        ),
    )?;
    write_about_xml(
        &no_version_warning_folder,
        concat!(
            "<ModMetaData>",
            "<name>No Version Warning</name>",
            "<packageId>mlie.noversionwarning</packageId>",
            "</ModMetaData>"
        ),
    )?;

    let compatibility_folder = no_version_warning_folder.join("1.6");
    fs::create_dir_all(&compatibility_folder)?;
    fs::write(
        compatibility_folder.join("ModIdsToFix.xml"),
        "<ModIdsToFix><li>example.reported</li></ModIdsToFix>",
    )?;

    let mut load = load_mods_from(&mods_folder, ModRoot::Local)?;
    apply_no_version_warning_support(&mut load.mods, Some("1.6"))?;

    let reported_mod = load
        .mods
        .iter()
        .find(|rimworld_mod| rimworld_mod.package_id.as_str() == "example.reported")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reported mod was not loaded"))?;

    assert!(reported_mod.community_supported_versions.is_empty());

    Ok(())
}

#[test]
fn missing_workshop_directory_is_a_non_fatal_warning() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let rimworld_path = test_directory.path.join("RimWorld");
    let config_path = test_directory.path.join("Config");
    let missing_workshop_path = test_directory.path.join("MissingWorkshop");

    fs::create_dir_all(rimworld_path.join("Data"))?;
    fs::create_dir_all(rimworld_path.join("Mods"))?;
    fs::create_dir_all(&config_path)?;
    fs::write(
        config_path.join("ModsConfig.xml"),
        "<ModsConfigData><activeMods></activeMods></ModsConfigData>",
    )?;

    let settings = Settings {
        rimworld_path: Some(rimworld_path),
        workshop_path: Some(missing_workshop_path.clone()),
        config_path: Some(config_path),
        ..Settings::default()
    };

    let report = load_mods(&settings, None)?;

    assert!(report.mods.all.is_empty());
    assert_eq!(report.warnings.len(), 1);
    assert!(!report.warnings[0].is_skipped_mod());
    assert_eq!(
        report.warnings[0].path.as_deref(),
        Some(missing_workshop_path.as_path())
    );

    Ok(())
}

#[test]
fn configured_and_steamcmd_workshop_folders_are_loaded_without_duplicate_ids() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let rimworld_path = test_directory.path.join("RimWorld");
    let config_path = test_directory.path.join("Config");
    let workshop_path = test_directory.path.join("SteamWorkshop");
    let steamcmd_root = test_directory.path.join("SteamCmd");
    let steamcmd_path = steamcmd_root.join("steamcmd-test");
    let steamcmd_workshop_path = steamcmd_root
        .join("steamapps")
        .join("workshop")
        .join("content")
        .join("294100");

    fs::create_dir_all(rimworld_path.join("Data"))?;
    fs::create_dir_all(rimworld_path.join("Mods"))?;
    fs::create_dir_all(&config_path)?;
    fs::create_dir_all(&steamcmd_root)?;
    fs::write(&steamcmd_path, "test executable")?;
    fs::write(
        config_path.join("ModsConfig.xml"),
        "<ModsConfigData><activeMods></activeMods></ModsConfigData>",
    )?;

    write_about_xml(
        &workshop_path.join("123"),
        "<ModMetaData><name>Primary</name><packageId>example.primary</packageId></ModMetaData>",
    )?;
    write_about_xml(
        &steamcmd_workshop_path.join("123"),
        "<ModMetaData><name>Duplicate</name><packageId>example.duplicate</packageId></ModMetaData>",
    )?;
    write_about_xml(
        &steamcmd_workshop_path.join("456"),
        "<ModMetaData><name>SteamCMD</name><packageId>example.steamcmd</packageId></ModMetaData>",
    )?;

    let settings = Settings {
        rimworld_path: Some(rimworld_path),
        workshop_path: Some(workshop_path),
        steamcmd_path: Some(steamcmd_path),
        config_path: Some(config_path),
        ..Settings::default()
    };

    let report = load_mods(&settings, None)?;
    let package_ids = report
        .mods
        .all
        .iter()
        .map(|rimworld_mod| rimworld_mod.package_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(package_ids, ["example.primary", "example.steamcmd"]);

    Ok(())
}

#[test]
fn missing_git_executable_falls_back_to_local_mod() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mod_folder = test_directory.path.join("GitMod");
    let missing_git_program = test_directory.path.join("git-does-not-exist");

    fs::create_dir_all(mod_folder.join(".git"))?;

    let remote_url = git_remote_url_with_program(&mod_folder, missing_git_program.as_os_str());

    assert!(remote_url.is_none());

    Ok(())
}

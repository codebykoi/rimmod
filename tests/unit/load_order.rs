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
            "rimmod-load-order-test-{}-{unique_number}",
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

fn temporary_files(config_folder: &Path) -> io::Result<Vec<PathBuf>> {
    let mut paths = Vec::new();

    for entry_result in fs::read_dir(config_folder)? {
        let entry = entry_result?;
        let path = entry.path();

        if path.extension().is_some_and(|extension| extension == "tmp") {
            paths.push(path);
        }
    }

    Ok(paths)
}

#[test]
fn existing_mods_config_returns_normalized_active_ids() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    fs::write(
        test_directory.path.join("ModsConfig.xml"),
        concat!(
            "<ModsConfigData><activeMods>",
            "<li> Ludeon.RimWorld </li>",
            "<li>Example.Mod</li>",
            "</activeMods></ModsConfigData>"
        ),
    )?;

    let package_ids = parse_config(&test_directory.path)?;
    let package_id_text = package_ids
        .iter()
        .map(PackageId::as_str)
        .collect::<Vec<_>>();

    assert_eq!(package_id_text, ["ludeon.rimworld", "example.mod"]);

    Ok(())
}

#[test]
fn successful_save_creates_a_recoverable_backup() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let config_file = test_directory.path.join("ModsConfig.xml");
    let original_xml = concat!(
        "<ModsConfigData>",
        "<activeMods><li>example.original</li></activeMods>",
        "<knownExpansions><li>ludeon.rimworld.royalty</li></knownExpansions>",
        "</ModsConfigData>"
    );
    fs::write(&config_file, original_xml)?;

    let outcome = save_load_order(&test_directory.path, &ModCollection::default())?;

    assert_eq!(
        outcome.backup_path,
        test_directory.path.join("ModsConfig.rimmod-backup.xml")
    );
    assert_eq!(fs::read_to_string(&outcome.backup_path)?, original_xml);
    assert!(parse_config(&test_directory.path)?.is_empty());
    assert!(temporary_files(&test_directory.path)?.is_empty());

    // A backup is recoverable using a normal file copy.
    fs::copy(&outcome.backup_path, &config_file)?;
    assert_eq!(fs::read_to_string(&config_file)?, original_xml);

    Ok(())
}

#[test]
fn failed_final_replace_does_not_modify_existing_config() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let config_file = test_directory.path.join("ModsConfig.xml");
    let backup_file = test_directory.path.join("ModsConfig.rimmod-backup.xml");
    let original_xml =
        "<ModsConfigData><activeMods></activeMods><marker>keep me</marker></ModsConfigData>";
    fs::write(&config_file, original_xml)?;

    let result = save_load_order_with_replace(
        &test_directory.path,
        &ModCollection::default(),
        |_temporary_file, _config_file| {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "simulated replacement failure",
            ))
        },
    );

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&config_file)?, original_xml);
    assert_eq!(fs::read_to_string(&backup_file)?, original_xml);
    assert!(temporary_files(&test_directory.path)?.is_empty());

    Ok(())
}

#[test]
fn missing_active_package_ids_prevent_saving() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let config_file = test_directory.path.join("ModsConfig.xml");
    let original_xml = concat!(
        "<ModsConfigData><activeMods>",
        "<li>example.missing</li>",
        "</activeMods></ModsConfigData>"
    );
    fs::write(&config_file, original_xml)?;
    let missing_package_id = PackageId::new("example.missing")
        .ok_or_else(|| io::Error::other("test package ID is invalid"))?;
    let mods = ModCollection::new(Vec::new(), Vec::new(), Vec::new(), vec![missing_package_id]);

    let result = save_load_order(&test_directory.path, &mods);

    assert!(result.is_err());
    assert_eq!(fs::read_to_string(&config_file)?, original_xml);
    assert!(
        !test_directory
            .path
            .join("ModsConfig.rimmod-backup.xml")
            .exists()
    );

    Ok(())
}

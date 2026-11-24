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
fn incompatible_mods_are_loaded_from_about_xml() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let mod_folder = mods_folder.join("IncompatibleMod");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Incompatible Mod</name>",
            "<packageId>example.incompatible</packageId>",
            "<incompatibleWith>",
            "<li>other.mod</li>",
            "<li>other.mod</li>",
            "<li>example.incompatible</li>",
            "</incompatibleWith>",
            "</ModMetaData>"
        ),
    )?;

    let load = load_mods_from(&mods_folder, ModRoot::Local)?;

    assert_eq!(load.mods.len(), 1);
    assert_eq!(
        load.mods[0]
            .incompatible_with
            .iter()
            .map(|package_id| package_id.as_str())
            .collect::<Vec<_>>(),
        ["other.mod"]
    );

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

    for version in ["1.7", "1.6"] {
        let compatibility_folder = no_version_warning_folder.join(version);
        fs::create_dir_all(&compatibility_folder)?;
        fs::write(
            compatibility_folder.join("ModIdsToFix.xml"),
            "\u{feff}<ModIdsToFix><li>example.reported</li></ModIdsToFix>",
        )?;
    }

    let mut load = load_mods_from(&mods_folder, ModRoot::Local)?;
    let warnings = apply_no_version_warning_support(&mut load.mods);
    assert!(warnings.is_empty());

    let reported_mod = load
        .mods
        .iter()
        .find(|rimworld_mod| rimworld_mod.package_id.as_str() == "example.reported")
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "reported mod was not loaded"))?;

    assert_eq!(reported_mod.supported_versions, ["1.5"]);
    assert_eq!(reported_mod.community_supported_versions, ["1.6", "1.7"]);

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
    let warnings = apply_no_version_warning_support(&mut load.mods);
    assert!(warnings.is_empty());

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
fn mod_dependencies_are_loaded_from_about_xml() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let mod_folder = mods_folder.join("AddonMod");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Addon Mod</name>",
            "<packageId>example.addon</packageId>",
            "<modDependencies>",
            "<li><packageId>Example.Base</packageId><displayName>Base</displayName></li>",
            "<li><packageId>example.addon</packageId></li>",
            "<li></li>",
            "</modDependencies>",
            "</ModMetaData>"
        ),
    )?;

    let load = load_mods_from(&mods_folder, ModRoot::Local)?;

    assert_eq!(load.mods.len(), 1);
    assert_eq!(
        load.mods[0]
            .mod_dependencies
            .iter()
            .map(|package_id| package_id.as_str())
            .collect::<Vec<_>>(),
        ["example.base"]
    );

    Ok(())
}

#[test]
fn patch_xml_produces_inferred_dependencies() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let mod_folder = mods_folder.join("CompatMod");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Compat Mod</name>",
            "<packageId>example.compat</packageId>",
            "</ModMetaData>"
        ),
    )?;

    let patches_folder = mod_folder.join("Patches");
    fs::create_dir_all(&patches_folder)?;
    fs::write(
        patches_folder.join("Compat.xml"),
        concat!(
            "<Patch>",
            r#"<Operation Class="PatchOperationFindMod">"#,
            "<mods><li>example.target</li></mods>",
            r#"<match Class="PatchOperationAdd">"#,
            r#"<xpath>/Defs/ThingDef[defName="Target"]</xpath>"#,
            "<value><foo/></value>",
            "</match></Operation></Patch>"
        ),
    )?;

    let load = load_mods_from(&mods_folder, ModRoot::Local)?;

    assert_eq!(load.mods.len(), 1);
    assert_eq!(
        load.mods[0]
            .inferred_after
            .iter()
            .map(|dependency| dependency.package_id.as_str())
            .collect::<Vec<_>>(),
        ["example.target"]
    );

    Ok(())
}

#[test]
fn official_content_does_not_produce_inferred_dependencies() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mod_folder = test_directory.path.join("Core");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<packageId>Ludeon.RimWorld</packageId>",
            "</ModMetaData>"
        ),
    )?;

    let defs_folder = mod_folder.join("Defs");
    fs::create_dir_all(&defs_folder)?;
    fs::write(
        defs_folder.join("Gated.xml"),
        concat!(
            "<Defs><ThingDef><defName>Gated</defName>",
            r#"<li MayRequire="Ludeon.RimWorld.Biotech"/>"#,
            "</ThingDef></Defs>"
        ),
    )?;

    let rimworld_mod = parse_mod(&mod_folder, ModRoot::Official)?;

    assert!(rimworld_mod.inferred_after.is_empty());

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

#[test]
fn about_xml_by_version_rules_are_selected_for_active_game_version() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mod_folder = test_directory.path.join("VersionedRulesMod");

    write_about_xml(
        &mod_folder,
        concat!(
            "<ModMetaData>",
            "<name>Versioned Rules</name>",
            "<packageId>example.versionedrules</packageId>",
            "<loadAfter><li>example.common</li></loadAfter>",
            "<loadAfterByVersion>",
            "<v1.4><li>example.old14</li></v1.4>",
            "<v1.5><li>example.only15</li><li>example.common</li></v1.5>",
            "</loadAfterByVersion>",
            "<loadBeforeByVersion>",
            "<v1.5><li>example.before15</li></v1.5>",
            "</loadBeforeByVersion>",
            "<forceLoadAfterByVersion>",
            "<v1.5><li>example.forceafter15</li></v1.5>",
            "</forceLoadAfterByVersion>",
            "<forceLoadBeforeByVersion>",
            "<v1.5><li>example.forcebefore15</li></v1.5>",
            "</forceLoadBeforeByVersion>",
            "<modDependenciesByVersion>",
            "<v1.4><li><packageId>example.dep14</packageId></li></v1.4>",
            "<v1.5><li><packageId>example.dep15</packageId></li></v1.5>",
            "</modDependenciesByVersion>",
            "<incompatibleWithByVersion>",
            "<v1.5><li>example.incompat15</li></v1.5>",
            "</incompatibleWithByVersion>",
            "</ModMetaData>"
        ),
    )?;

    let mod_15 = parse_mod_for_version(&mod_folder, ModRoot::Local, Some("1.5.4104 rev435"))?;

    assert_eq!(
        mod_15
            .loader_after
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.common", "example.only15"]
    );
    assert_eq!(
        mod_15
            .loader_before
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.before15"]
    );
    assert_eq!(
        mod_15
            .force_load_after
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.forceafter15"]
    );
    assert_eq!(
        mod_15
            .force_load_before
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.forcebefore15"]
    );
    assert_eq!(
        mod_15
            .mod_dependencies
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.dep15"]
    );
    assert_eq!(
        mod_15
            .incompatible_with
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.incompat15"]
    );

    // Running on 1.6 without a <v1.6> entry falls back to the newest <= 1.6 (<v1.5>).
    let mod_16_fallback = parse_mod_for_version(&mod_folder, ModRoot::Local, Some("1.6.4400"))?;
    assert_eq!(
        mod_16_fallback
            .mod_dependencies
            .iter()
            .map(PackageId::as_str)
            .collect::<Vec<_>>(),
        ["example.dep15"]
    );

    Ok(())
}

#[test]
fn patch_operation_find_mod_resolves_mod_names_to_installed_package_ids() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");
    let target_folder = mods_folder.join("CombatExtended");
    let patch_folder = mods_folder.join("CompatPatch");

    write_about_xml(
        &target_folder,
        concat!(
            "<ModMetaData>",
            "<name>Combat Extended</name>",
            "<packageId>CETeam.CombatExtended</packageId>",
            "</ModMetaData>"
        ),
    )?;
    write_about_xml(
        &patch_folder,
        concat!(
            "<ModMetaData>",
            "<name>Compat Patch</name>",
            "<packageId>example.compat</packageId>",
            "</ModMetaData>"
        ),
    )?;

    let patches_dir = patch_folder.join("Patches");
    fs::create_dir_all(&patches_dir)?;
    fs::write(
        patches_dir.join("CE.xml"),
        concat!(
            "<Patch>",
            r#"<Operation Class="PatchOperationFindMod">"#,
            "<mods>",
            "<li>Combat Extended</li>",
            "<li>Some Uninstalled Mod</li>",
            "</mods>",
            "<match/>",
            "</Operation></Patch>"
        ),
    )?;

    let load = load_mods_from_for_version(&mods_folder, ModRoot::Local, Some("1.5.4104"))?;
    let patch_mod = load
        .mods
        .iter()
        .find(|m| m.package_id.as_str() == "example.compat")
        .expect("compat mod loaded");

    assert!(patch_mod.is_patch_only);
    assert_eq!(
        patch_mod
            .inferred_after
            .iter()
            .map(|dep| (dep.package_id.as_str(), dep.kind))
            .collect::<Vec<_>>(),
        [(
            "ceteam.combatextended",
            crate::models::InferredDependencyKind::FindMod
        )]
    );

    Ok(())
}

#[test]
fn csharp_assembly_references_resolve_to_owning_mods_and_ignore_bundled_or_ambiguous_dlls()
-> io::Result<()> {
    use crate::models::InferredDependencyKind;
    use crate::services::assembly_metadata::tests::build_synthetic_dotnet_dll;

    let test_directory = TestDirectory::new()?;
    let mods_folder = test_directory.path.join("Mods");

    // 1. Canonical Harmony mod providing 0Harmony.dll
    let harmony_folder = mods_folder.join("Harmony");
    write_about_xml(
        &harmony_folder,
        "<ModMetaData><name>Harmony</name><packageId>brrainz.harmony</packageId></ModMetaData>",
    )?;
    fs::create_dir_all(harmony_folder.join("1.5/Assemblies"))?;
    fs::write(
        harmony_folder.join("1.5/Assemblies/0Harmony.dll"),
        build_synthetic_dotnet_dll(false, "0Harmony", &["mscorlib"]),
    )?;

    // 2. Framework mod providing UniqueFramework.dll and also bundling copies of 0Harmony + SharedLib
    let framework_folder = mods_folder.join("Framework");
    write_about_xml(
        &framework_folder,
        "<ModMetaData><name>Framework</name><packageId>author.framework</packageId></ModMetaData>",
    )?;
    fs::create_dir_all(framework_folder.join("1.5/Assemblies"))?;
    fs::write(
        framework_folder.join("1.5/Assemblies/UniqueFramework.dll"),
        build_synthetic_dotnet_dll(false, "UniqueFramework", &["mscorlib", "Assembly-CSharp"]),
    )?;
    fs::write(
        framework_folder.join("1.5/Assemblies/0Harmony.dll"),
        build_synthetic_dotnet_dll(false, "0Harmony", &["mscorlib"]),
    )?;
    fs::write(
        framework_folder.join("1.5/Assemblies/SharedLib.dll"),
        build_synthetic_dotnet_dll(false, "SharedLib", &["mscorlib"]),
    )?;

    // 3. Another unrelated mod also bundling SharedLib.dll (making SharedLib ambiguous)
    let other_folder = mods_folder.join("OtherMod");
    write_about_xml(
        &other_folder,
        "<ModMetaData><name>Other Mod</name><packageId>author.other</packageId></ModMetaData>",
    )?;
    fs::create_dir_all(other_folder.join("1.5/Assemblies"))?;
    fs::write(
        other_folder.join("1.5/Assemblies/SharedLib.dll"),
        build_synthetic_dotnet_dll(false, "SharedLib", &["mscorlib"]),
    )?;

    // 4. Addon mod with an active 1.5 DLL referencing UniqueFramework, 0Harmony, SharedLib,
    //    its own SelfHelper, and system assemblies, plus a stale 1.4 DLL that must be ignored.
    let addon_folder = mods_folder.join("Addon");
    write_about_xml(
        &addon_folder,
        "<ModMetaData><name>Addon</name><packageId>author.addon</packageId></ModMetaData>",
    )?;
    fs::create_dir_all(addon_folder.join("1.5/Assemblies"))?;
    fs::write(
        addon_folder.join("1.5/Assemblies/Addon.dll"),
        build_synthetic_dotnet_dll(
            false,
            "Addon",
            &[
                "mscorlib",
                "System.Core",
                "UnityEngine.CoreModule",
                "Assembly-CSharp",
                "UniqueFramework",
                "0Harmony",
                "SharedLib",
                "SelfHelper",
            ],
        ),
    )?;
    fs::write(
        addon_folder.join("1.5/Assemblies/SelfHelper.dll"),
        build_synthetic_dotnet_dll(false, "SelfHelper", &["mscorlib"]),
    )?;
    fs::create_dir_all(addon_folder.join("1.4/Assemblies"))?;
    fs::write(
        addon_folder.join("1.4/Assemblies/OldAddon.dll"),
        build_synthetic_dotnet_dll(false, "OldAddon", &["LegacyOnly"]),
    )?;

    let load = load_mods_from_for_version(&mods_folder, ModRoot::Local, Some("1.5.4104"))?;
    let addon = load
        .mods
        .iter()
        .find(|m| m.package_id.as_str() == "author.addon")
        .expect("addon loaded");

    assert!(!addon.is_patch_only);
    assert_eq!(
        addon
            .inferred_after
            .iter()
            .map(|dep| (dep.package_id.as_str(), dep.kind, dep.source_file.clone()))
            .collect::<Vec<_>>(),
        [
            (
                "author.framework",
                InferredDependencyKind::AssemblyReference,
                PathBuf::from("1.5").join("Assemblies").join("Addon.dll"),
            ),
            (
                "brrainz.harmony",
                InferredDependencyKind::AssemblyReference,
                PathBuf::from("1.5").join("Assemblies").join("Addon.dll"),
            ),
        ]
    );

    Ok(())
}

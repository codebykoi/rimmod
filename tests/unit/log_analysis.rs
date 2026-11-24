use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::services::log_parser::parse_log_text;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rimmod-log-analysis-test-{}-{unique_number}",
            std::process::id()
        ));

        fs::create_dir(&path)?;

        Ok(Self { path })
    }

    fn mod_folder(&self, name: &str) -> io::Result<PathBuf> {
        let folder = self.path.join(name);
        fs::create_dir_all(&folder)?;
        Ok(folder)
    }
}

impl Drop for TestDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

fn snapshot(
    package_id: &str,
    name: &str,
    folder: PathBuf,
    assembly_paths: Vec<PathBuf>,
    assembly_stems: Vec<String>,
    enabled: bool,
) -> ModSnapshot {
    snapshot_with_official(
        package_id,
        name,
        folder,
        assembly_paths,
        assembly_stems,
        enabled,
        false,
    )
}

#[allow(clippy::too_many_arguments)]
fn snapshot_with_official(
    package_id: &str,
    name: &str,
    folder: PathBuf,
    assembly_paths: Vec<PathBuf>,
    assembly_stems: Vec<String>,
    enabled: bool,
    official: bool,
) -> ModSnapshot {
    ModSnapshot {
        package_id: PackageId::new(package_id).expect("valid package id"),
        name: name.to_owned(),
        folder,
        workshop_id: None,
        enabled,
        official,
        assembly_paths,
        assembly_stems,
    }
}

fn write_assembly(folder: &Path, file_name: &str, contents: &[u8]) -> io::Result<PathBuf> {
    let assemblies = folder.join("Assemblies");
    fs::create_dir_all(&assemblies)?;
    let path = assemblies.join(file_name);
    fs::write(&path, contents)?;
    Ok(path)
}

fn write_defs(folder: &Path, contents: &str) -> io::Result<()> {
    let defs = folder.join("Defs");
    fs::create_dir_all(&defs)?;
    fs::write(defs.join("Things.xml"), contents)
}

#[test]
fn groups_repeated_errors_and_matches_package_id() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("RepeatMod")?;
    let mods = vec![snapshot(
        "test.repeat",
        "Repeat Mod",
        folder,
        Vec::new(),
        Vec::new(),
        true,
    )];

    let text = concat!(
        "2026-11-22 10:00:00.000  ERROR [Verse] Could not register node named \"Broken\" in mod test.repeat because it is already used.\n",
        "    at Verse.XmlInheritance.TryRegister(XmlNode node, ModContentPack mod) [0x1]\n",
        "2026-11-22 10:00:01.000  ERROR [Verse] Could not register node named \"Broken\" in mod test.repeat because it is already used.\n",
        "    at Verse.XmlInheritance.TryRegister(XmlNode node, ModContentPack mod) [0x1]\n",
    );

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.counts.error, 2);
    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].count, 2);
    assert_eq!(report.issues[0].suspects.len(), 1);

    let suspect = &report.issues[0].suspects[0];
    assert_eq!(suspect.package_id.as_str(), "test.repeat");
    assert!(suspect.score >= 110);
    assert!(
        suspect
            .evidence
            .iter()
            .any(|evidence| evidence.contains("Package ID"))
    );

    assert_eq!(report.suspects.len(), 1);
    assert_eq!(report.suspects[0].issues, 1);

    Ok(())
}

#[test]
fn matches_namespaces_in_mod_assemblies() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("NamespaceMod")?;
    let assembly = write_assembly(&folder, "Payload.dll", b"MZ TestAsm.Core.Patch metadata")?;
    let mods = vec![snapshot(
        "test.namespace",
        "Namespace Mod",
        folder,
        vec![assembly],
        vec!["payload".to_owned()],
        true,
    )];

    let text = concat!(
        "2026-11-22 10:00:00.000  ERROR [Verse] Exception in mod code\n",
        "    at TestAsm.Core.Patch.Method() [0x1]\n",
    );

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].suspects.len(), 1);

    let suspect = &report.issues[0].suspects[0];
    assert_eq!(suspect.package_id.as_str(), "test.namespace");
    assert!(
        suspect
            .evidence
            .iter()
            .any(|evidence| evidence.contains("Stack frame namespace"))
    );

    Ok(())
}

#[test]
fn matches_xml_context_def_names() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("DefOwner")?;
    write_defs(
        &folder,
        "<Defs><ThingDef><defName>BrokenDef</defName></ThingDef></Defs>",
    )?;
    let mods = vec![snapshot(
        "test.defowner",
        "Def Owner",
        folder,
        Vec::new(),
        Vec::new(),
        true,
    )];

    let text = "2026-11-22 10:00:00.000  ERROR [Verse] XML error: 0.5 doesn't correspond to any field in type ThingDef. Context: BrokenDef test value\n";

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].suspects.len(), 1);

    let suspect = &report.issues[0].suspects[0];
    assert_eq!(suspect.package_id.as_str(), "test.defowner");
    assert!(
        suspect
            .evidence
            .iter()
            .any(|evidence| evidence.contains("Def `BrokenDef`"))
    );

    Ok(())
}

#[test]
fn framework_frames_do_not_blame_installed_mods() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("InnocentMod")?;
    let mods = vec![snapshot(
        "test.innocent",
        "Innocent Mod",
        folder,
        Vec::new(),
        Vec::new(),
        true,
    )];

    let text = concat!(
        "2026-11-22 10:00:00.000  ERROR [Verse] Something went wrong\n",
        "    at Verse.Log.Error(String text) [0x1]\n",
        "    at RimWorld.Planet.WorldGrid.ChangeTileTemperatures() [0x2]\n",
    );

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.issues.len(), 1);
    assert!(report.issues[0].suspects.is_empty());

    Ok(())
}

#[test]
fn longer_package_id_is_not_matched_by_a_shorter_one() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let short_folder = test_directory.mod_folder("Short")?;
    let long_folder = test_directory.mod_folder("Long")?;
    let mods = vec![
        snapshot(
            "test.repeat",
            "Short Package",
            short_folder,
            Vec::new(),
            Vec::new(),
            true,
        ),
        snapshot(
            "test.repeat.extra",
            "Long Package",
            long_folder,
            Vec::new(),
            Vec::new(),
            true,
        ),
    ];

    let text = "2026-11-22 10:00:00.000  ERROR [Verse] Mod test.repeat.extra failed to load.\n";

    let report = analyze(parse_log_text(text), &mods);
    let suspects = &report.issues[0].suspects;

    assert_eq!(suspects.len(), 1);
    assert_eq!(suspects[0].package_id.as_str(), "test.repeat.extra");

    Ok(())
}

#[test]
fn official_content_is_not_in_the_culprit_summary() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("Core")?;
    let mods = vec![snapshot_with_official(
        "ludeon.rimworld",
        "Core",
        folder,
        Vec::new(),
        Vec::new(),
        true,
        true,
    )];

    let text = "2026-11-22 10:00:00.000  ERROR [Verse] Could not register node named \"Broken\" in mod ludeon.rimworld because it is already used.\n";

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.issues[0].suspects.len(), 1);
    assert!(report.issues[0].suspects[0].official);
    assert!(report.suspects.is_empty());

    Ok(())
}

#[test]
fn plain_assembly_words_do_not_blame_a_mod() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("Harmony")?;
    let mods = vec![snapshot(
        "brrainz.harmony",
        "Harmony",
        folder,
        Vec::new(),
        vec!["harmony".to_owned()],
        true,
    )];

    let text = "2026-11-22 10:00:00.000  INFO [Verse] Harmony patch applied successfully.\n";

    let report = analyze(parse_log_text(text), &mods);

    assert!(report.issues[0].suspects.is_empty());

    Ok(())
}

#[test]
fn info_entries_do_not_create_culprit_summaries() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let folder = test_directory.mod_folder("ChattyMod")?;
    let mods = vec![snapshot(
        "test.chatty",
        "Chatty Mod",
        folder,
        Vec::new(),
        Vec::new(),
        false,
    )];

    let text = concat!(
        "2026-11-22 10:00:00.000  INFO [Verse] Loading test.chatty\n",
        "2026-11-22 10:00:01.000  INFO [Verse] test.chatty finished loading\n",
    );

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.counts.info, 2);
    assert_eq!(report.issues.len(), 2);

    for issue in &report.issues {
        assert_eq!(issue.suspects.len(), 1);
        assert!(!issue.suspects[0].enabled);
    }

    assert!(report.suspects.is_empty());

    Ok(())
}

#[test]
fn logging_hook_mods_rank_after_real_suspects_and_stay_out_of_summary() -> io::Result<()> {
    let test_directory = TestDirectory::new()?;
    let hook_folder = test_directory.mod_folder("LoadingProgress")?;
    let mod_folder = test_directory.mod_folder("SquishyMelee")?;
    let mods = vec![
        snapshot(
            "ilyvion.loadingprogress",
            "Loading Progress",
            hook_folder,
            Vec::new(),
            Vec::new(),
            true,
        ),
        snapshot(
            "local.gunrecoil.melee",
            "SQUISHY Melee",
            mod_folder,
            Vec::new(),
            Vec::new(),
            true,
        ),
    ];

    let text = concat!(
        "2026-11-24 18:17:53.082  WARNING [Verse] [Squishy Melee] the host window changed\n",
        "    Harmony postfix from ilyvion.loadingprogress\n",
    );

    let report = analyze(parse_log_text(text), &mods);

    assert_eq!(report.issues.len(), 1);
    assert_eq!(report.issues[0].suspects.len(), 2);
    assert_eq!(
        report.issues[0].suspects[0].package_id.as_str(),
        "local.gunrecoil.melee"
    );
    assert_eq!(
        report.issues[0].suspects[1].package_id.as_str(),
        "ilyvion.loadingprogress"
    );

    assert_eq!(report.suspects.len(), 1);
    assert_eq!(
        report.suspects[0].package_id.as_str(),
        "local.gunrecoil.melee"
    );
    assert_eq!(report.suspects[0].errors, 0);
    assert_eq!(report.suspects[0].warnings, 1);
    assert_eq!(report.suspects[0].issues, 1);

    Ok(())
}

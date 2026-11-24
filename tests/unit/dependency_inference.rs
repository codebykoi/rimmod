use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use super::*;
use crate::models::InferredDependencyKind;

static NEXT_TEST_DIRECTORY: AtomicU64 = AtomicU64::new(0);

struct TestDirectory {
    path: PathBuf,
}

impl TestDirectory {
    fn new() -> io::Result<Self> {
        let unique_number = NEXT_TEST_DIRECTORY.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "rimmod-dependency-inference-test-{}-{unique_number}",
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

fn write_xml(folder: &Path, relative_path: &str, contents: &str) -> io::Result<()> {
    let path = folder.join(relative_path);

    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    fs::write(path, contents)
}

fn package_ids(inferred: &[InferredDependency]) -> Vec<&str> {
    inferred
        .iter()
        .map(|dependency| dependency.package_id.as_str())
        .collect()
}

#[test]
fn may_require_attributes_are_inferred_as_load_after_dependencies() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Defs/Addon.xml",
        concat!(
            "<Defs><ThingDef><defName>Example</defName><comps>",
            r#"<li MayRequire="Author.Base" Class="CompProperties_Example"/>"#,
            "</comps></ThingDef></Defs>"
        ),
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert_eq!(package_ids(&inferred), ["author.base"]);
    assert_eq!(inferred[0].kind, InferredDependencyKind::MayRequire);
    assert_eq!(
        inferred[0].source_file,
        PathBuf::from("Defs").join("Addon.xml")
    );

    Ok(())
}

#[test]
fn may_require_any_of_and_comma_lists_are_split() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Defs/AnyOf.xml",
        concat!(
            "<Defs><ThingDef><defName>Example</defName>",
            r#"<li MayRequireAnyOf="author.one, author.two"/>"#,
            "</ThingDef></Defs>"
        ),
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert_eq!(package_ids(&inferred), ["author.one", "author.two"]);

    Ok(())
}

#[test]
fn find_mod_lists_are_inferred_as_load_after_dependencies() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Patches/Compat.xml",
        concat!(
            "<Patch>",
            r#"<Operation Class="PatchOperationFindMod">"#,
            "<mods><li>Author.Target</li></mods>",
            r#"<match Class="PatchOperationAdd">"#,
            r#"<xpath>/Defs/ThingDef[defName="Target"]</xpath>"#,
            "<value><foo/></value>",
            "</match></Operation></Patch>"
        ),
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert_eq!(package_ids(&inferred), ["author.target"]);
    assert_eq!(inferred[0].kind, InferredDependencyKind::FindMod);
    assert_eq!(
        inferred[0].source_file,
        PathBuf::from("Patches").join("Compat.xml")
    );

    Ok(())
}

#[test]
fn duplicate_markers_in_one_file_are_collapsed() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Defs/Duplicates.xml",
        concat!(
            "<Defs><ThingDef><defName>Example</defName>",
            r#"<li MayRequire="author.base"/>"#,
            r#"<li MayRequire="author.base"/>"#,
            "</ThingDef></Defs>"
        ),
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert_eq!(package_ids(&inferred), ["author.base"]);

    Ok(())
}

#[test]
fn commented_out_markers_are_ignored() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Defs/Commented.xml",
        concat!(
            "<Defs><!-- ",
            r#"<li MayRequire="author.commented"/>"#,
            " --><ThingDef><defName>Example</defName></ThingDef></Defs>"
        ),
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert!(inferred.is_empty());

    Ok(())
}

#[test]
fn about_folder_is_not_scanned() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "About/About.xml",
        r#"<ModMetaData><name>Example</name><packageId>example.package</packageId><loadAfter><li>author.hidden</li></loadAfter></ModMetaData>"#,
    )?;

    let inferred = infer_dependencies(&directory.path);

    assert!(inferred.is_empty());

    Ok(())
}

#[test]
fn unreadable_xml_does_not_fail_inference() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Defs/Broken.xml",
        r#"<Defs><ThingDef><defName>Example</defName><li MayRequire="author.base""#,
    )?;

    // Best effort: whatever the parser can recover is fine, but it must not panic.
    let _ = infer_dependencies(&directory.path);

    Ok(())
}

#[test]
fn load_folders_xml_extracts_if_mod_active_and_filters_to_active_version() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "LoadFolders.xml",
        concat!(
            "<loadFolders>",
            "<v1.4>",
            "<li>1.4</li>",
            r#"<li IfModActive="author.legacy">Compat/Legacy</li>"#,
            "</v1.4>",
            "<v1.5>",
            "<li>/</li>",
            "<li>1.5</li>",
            r#"<li IfModActive="CETeam.CombatExtended">Compat/CE</li>"#,
            r#"<li IfModActiveAll="author.a, author.b">Compat/Both</li>"#,
            r#"<li IfModActiveAny="author.c">Compat/Any</li>"#,
            r#"<li IfModNotActive="author.ignored">Compat/Fallback</li>"#,
            "</v1.5>",
            "</loadFolders>"
        ),
    )?;

    // Stale 1.4 XML must be ignored when running 1.5.
    write_xml(
        &directory.path,
        "1.4/Defs/Old.xml",
        r#"<Defs><ThingDef><li MayRequire="author.stale"/></ThingDef></Defs>"#,
    )?;

    // Active 1.5 conditional folder XML must be scanned.
    write_xml(
        &directory.path,
        "Compat/CE/Defs/Extra.xml",
        r#"<Defs><ThingDef><li MayRequire="author.fromcompat"/></ThingDef></Defs>"#,
    )?;

    let inferred = infer_dependencies_for_version(&directory.path, Some("1.5.4104 rev435"));

    assert_eq!(
        package_ids(&inferred),
        [
            "author.a",
            "author.b",
            "author.c",
            "author.fromcompat",
            "ceteam.combatextended",
        ]
    );
    assert!(inferred.iter().any(|dep| {
        dep.package_id.as_str() == "ceteam.combatextended"
            && dep.kind == InferredDependencyKind::LoadFolderCondition
            && dep.source_file == Path::new("LoadFolders.xml")
    }));

    Ok(())
}

#[test]
fn default_folder_layout_skips_obsolete_version_folders() -> io::Result<()> {
    let directory = TestDirectory::new()?;

    write_xml(
        &directory.path,
        "Common/Defs/Shared.xml",
        r#"<Defs><ThingDef><li MayRequire="author.common"/></ThingDef></Defs>"#,
    )?;
    write_xml(
        &directory.path,
        "1.4/Defs/Old.xml",
        r#"<Defs><ThingDef><li MayRequire="author.old"/></ThingDef></Defs>"#,
    )?;
    write_xml(
        &directory.path,
        "1.5/Defs/Current.xml",
        r#"<Defs><ThingDef><li MayRequire="author.current"/></ThingDef></Defs>"#,
    )?;

    let inferred = infer_dependencies_for_version(&directory.path, Some("1.5.4104"));

    assert_eq!(package_ids(&inferred), ["author.common", "author.current"]);

    Ok(())
}

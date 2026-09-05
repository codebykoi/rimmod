use std::path::PathBuf;

use super::*;

fn package_id(raw: &str) -> PackageId {
    let Some(package_id) = PackageId::new(raw) else {
        panic!("test package IDs must not be empty");
    };

    package_id
}

fn test_mod(package: &str) -> RimworldMod {
    RimworldMod {
        name: package.to_owned(),
        package_id: package_id(package),
        description: String::new(),
        supported_versions: Vec::new(),
        community_supported_versions: Vec::new(),
        loader_after: Vec::new(),
        loader_before: Vec::new(),
        folder: PathBuf::new(),
        source: ModSource::Local,
        mod_type: ModType::Xml,
    }
}

#[test]
fn active_package_ids_are_restored_after_mod_folders_change() {
    let mut mods = ModCollection::new(
        vec![test_mod("example.a"), test_mod("example.b")],
        vec![ModId::from_index(1)],
        vec![ModId::from_index(0)],
        vec![package_id("example.missing")],
    );
    let active_package_ids = mods.active_package_ids();

    mods.all.push(test_mod("example.new"));
    mods.replace_active_package_ids(active_package_ids);

    let enabled_packages = mods
        .enabled_ids()
        .iter()
        .filter_map(|&mod_id| mods.get(mod_id))
        .map(|rimworld_mod| rimworld_mod.package_id.as_str())
        .collect::<Vec<_>>();
    let disabled_packages = mods
        .disabled_ids()
        .iter()
        .filter_map(|&mod_id| mods.get(mod_id))
        .map(|rimworld_mod| rimworld_mod.package_id.as_str())
        .collect::<Vec<_>>();

    assert_eq!(enabled_packages, ["example.a"]);
    assert_eq!(disabled_packages, ["example.b", "example.new"]);
    assert_eq!(
        mods.missing_active_package_ids(),
        [package_id("example.missing")]
    );
}

#[test]
fn enabling_multiple_mods_preserves_their_disabled_list_order() {
    let mut mods = ModCollection::new(
        vec![test_mod("a"), test_mod("b"), test_mod("c"), test_mod("d")],
        vec![
            ModId::from_index(0),
            ModId::from_index(1),
            ModId::from_index(2),
        ],
        vec![ModId::from_index(3)],
        Vec::new(),
    );

    let result = mods.enable_many(
        &[ModId::from_index(2), ModId::from_index(0)],
        Some(ModId::from_index(3)),
    );

    assert!(result.is_ok());
    assert_eq!(mods.disabled_ids(), [ModId::from_index(1)]);
    assert_eq!(
        mods.enabled_ids(),
        [
            ModId::from_index(0),
            ModId::from_index(2),
            ModId::from_index(3)
        ]
    );
}

#[test]
fn failed_group_transfer_does_not_move_any_mods() {
    let mut mods = ModCollection::new(
        vec![test_mod("a"), test_mod("b")],
        vec![ModId::from_index(0)],
        vec![ModId::from_index(1)],
        Vec::new(),
    );

    let result = mods.enable_many(&[ModId::from_index(0), ModId::from_index(1)], None);

    assert!(matches!(
        result,
        Err(ModCollectionError::NotDisabled(mod_id))
            if mod_id == ModId::from_index(1)
    ));
    assert_eq!(mods.disabled_ids(), [ModId::from_index(0)]);
    assert_eq!(mods.enabled_ids(), [ModId::from_index(1)]);
}

#[test]
fn reordering_multiple_mods_keeps_their_relative_order() {
    let mut mods = ModCollection::new(
        vec![test_mod("a"), test_mod("b"), test_mod("c"), test_mod("d")],
        Vec::new(),
        vec![
            ModId::from_index(0),
            ModId::from_index(1),
            ModId::from_index(2),
            ModId::from_index(3),
        ],
        Vec::new(),
    );

    let result = mods.reorder_enabled_many(
        &[ModId::from_index(3), ModId::from_index(1)],
        Some(ModId::from_index(0)),
    );

    assert!(result.is_ok());
    assert_eq!(
        mods.enabled_ids(),
        [
            ModId::from_index(1),
            ModId::from_index(3),
            ModId::from_index(0),
            ModId::from_index(2)
        ]
    );
}

fn versions(raw_versions: &[&str]) -> Vec<String> {
    raw_versions
        .iter()
        .map(|version| (*version).to_owned())
        .collect()
}

#[test]
fn version_support_prefers_the_official_list() {
    let supported_versions = versions(&["1.4", "1.5"]);
    let community_supported_versions = versions(&["1.5"]);

    assert_eq!(
        version_support(&supported_versions, &community_supported_versions, "1.5"),
        VersionSupport::Official
    );
    assert_eq!(
        version_support(&supported_versions, &community_supported_versions, "1.4"),
        VersionSupport::Official
    );
}

#[test]
fn version_support_falls_back_to_the_community_list() {
    let supported_versions = versions(&["1.4"]);
    let community_supported_versions = versions(&["1.5"]);

    assert_eq!(
        version_support(&supported_versions, &community_supported_versions, "1.5"),
        VersionSupport::Community
    );
}

#[test]
fn version_support_reports_unlisted_game_versions() {
    let supported_versions = versions(&["1.4"]);
    let community_supported_versions = Vec::new();

    assert_eq!(
        version_support(&supported_versions, &community_supported_versions, "1.6"),
        VersionSupport::Unsupported
    );
}

#[test]
fn highest_supported_version_compares_major_minor_numerically() {
    assert_eq!(
        highest_supported_version(&versions(&["1.0", "1.5", "1.4"])),
        Some("1.5")
    );
    assert_eq!(
        highest_supported_version(&versions(&["1.9", "1.10"])),
        Some("1.10")
    );
    assert_eq!(
        highest_supported_version(&versions(&["2.0", "1.10"])),
        Some("2.0")
    );
    assert_eq!(highest_supported_version(&versions(&[])), None);
    assert_eq!(
        highest_supported_version(&versions(&["not a version"])),
        None
    );
}

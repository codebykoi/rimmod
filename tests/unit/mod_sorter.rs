use std::path::PathBuf;

use super::*;
use crate::models::{ModSource, ModType, PackageId, RimworldMod};

fn package_id(raw: &str) -> PackageId {
    let Some(package_id) = PackageId::new(raw) else {
        panic!("test package IDs must not be empty");
    };

    package_id
}

fn test_mod(package: &str, load_after: &[&str], load_before: &[&str]) -> RimworldMod {
    RimworldMod {
        name: package.to_owned(),
        package_id: package_id(package),
        description: String::new(),
        supported_versions: Vec::new(),
        community_supported_versions: Vec::new(),
        loader_after: load_after.iter().map(|raw| package_id(raw)).collect(),
        loader_before: load_before.iter().map(|raw| package_id(raw)).collect(),
        folder: PathBuf::new(),
        source: ModSource::Local,
        mod_type: ModType::Xml,
    }
}

fn active_collection(mods: Vec<RimworldMod>) -> ModCollection {
    let enabled_ids = (0..mods.len()).map(ModId::from_index).collect();

    ModCollection::new(mods, Vec::new(), enabled_ids, Vec::new())
}

fn active_package_ids(mods: &ModCollection) -> Vec<&str> {
    mods.enabled_ids()
        .iter()
        .filter_map(|&mod_id| mods.get(mod_id))
        .map(|rimworld_mod| rimworld_mod.package_id.as_str())
        .collect()
}

#[test]
fn duplicate_active_package_ids_are_rejected() {
    let mut mods = active_collection(vec![
        test_mod("example.duplicate", &[], &[]),
        test_mod("example.duplicate", &[], &[]),
    ]);

    let result = sort_mods(&mut mods);

    assert!(matches!(
        result,
        Err(SortError::DuplicatePackageId(package_id))
            if package_id == "example.duplicate"
    ));
}

#[test]
fn dependency_cycles_are_rejected_without_changing_the_order() {
    let mut mods = active_collection(vec![
        test_mod("example.a", &["example.b"], &[]),
        test_mod("example.b", &["example.a"], &[]),
    ]);
    let original_order = mods.enabled_ids().to_vec();

    let result = sort_mods(&mut mods);

    assert!(matches!(result, Err(SortError::DependencyCycle(_))));
    assert_eq!(mods.enabled_ids(), original_order);
}

#[test]
fn automatic_sorting_is_stable() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.a", &[], &[]),
        test_mod("example.b", &["example.c"], &[]),
        test_mod("example.c", &[], &[]),
        test_mod("example.d", &[], &[]),
    ]);

    sort_mods(&mut mods)?;
    assert_eq!(
        active_package_ids(&mods),
        ["example.a", "example.c", "example.b", "example.d"]
    );

    sort_mods(&mut mods)?;
    assert_eq!(
        active_package_ids(&mods),
        ["example.a", "example.c", "example.b", "example.d"]
    );

    Ok(())
}

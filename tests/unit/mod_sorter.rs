use std::path::PathBuf;

use super::*;
use crate::models::{
    InferredDependency, InferredDependencyKind, ModSource, ModType, PackageId, RimworldMod,
};

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
        force_load_after: Vec::new(),
        force_load_before: Vec::new(),
        mod_dependencies: Vec::new(),
        incompatible_with: Vec::new(),
        inferred_after: Vec::new(),
        is_patch_only: false,
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
fn forced_dependency_cycles_are_rejected_without_changing_the_order() {
    let mut mod_a = test_mod("example.a", &[], &[]);
    mod_a.force_load_after.push(package_id("example.b"));

    let mut mod_b = test_mod("example.b", &[], &[]);
    mod_b.force_load_after.push(package_id("example.a"));

    let mut mods = active_collection(vec![mod_a, mod_b]);
    let original_order = mods.enabled_ids().to_vec();

    let result = sort_mods(&mut mods);

    assert!(matches!(result, Err(SortError::DependencyCycle(_))));
    assert_eq!(mods.enabled_ids(), original_order);
}

#[test]
fn regular_load_after_cycle_skips_conflicting_edge_and_stays_idempotent() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.a", &["example.b"], &[]),
        test_mod("example.b", &["example.a"], &[]),
        test_mod("example.d", &["example.c"], &[]),
        test_mod("example.c", &[], &[]),
    ]);

    let first_report = sort_mods(&mut mods)?;
    let first_order = active_package_ids(&mods)
        .into_iter()
        .map(str::to_owned)
        .collect::<Vec<_>>();

    assert_eq!(first_report.explicit_edges_skipped, 1);
    // Unrelated mods in the same list are still sorted properly.
    let c_pos = first_order.iter().position(|id| id == "example.c").unwrap();
    let d_pos = first_order.iter().position(|id| id == "example.d").unwrap();
    assert!(c_pos < d_pos);

    // Sorting a second time must not flip the broken cycle back and forth.
    let second_report = sort_mods(&mut mods)?;
    assert_eq!(second_report.explicit_edges_skipped, 1);
    assert_eq!(active_package_ids(&mods), first_order);

    Ok(())
}

#[test]
fn force_load_after_overrides_conflicting_regular_load_after() -> Result<(), SortError> {
    let mod_a = test_mod("example.a", &["example.b"], &[]);
    let mut mod_b = test_mod("example.b", &[], &[]);
    mod_b.force_load_after.push(package_id("example.a"));

    let mut mods = active_collection(vec![mod_b, mod_a]);

    let report = sort_mods(&mut mods)?;

    assert_eq!(active_package_ids(&mods), ["example.a", "example.b"]);
    assert_eq!(report.explicit_edges_skipped, 1);

    Ok(())
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

#[test]
fn declared_dependencies_are_ordered_before_dependents() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.addon", &[], &[]),
        test_mod("example.base", &[], &[]),
    ]);

    mods.all[0]
        .mod_dependencies
        .push(package_id("example.base"));

    let report = sort_mods(&mut mods)?;

    assert_eq!(active_package_ids(&mods), ["example.base", "example.addon"]);
    assert_eq!(report.dependency_edges_skipped, 0);

    Ok(())
}

#[test]
fn inferred_dependencies_are_ordered_before_the_patching_mod() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.compat", &[], &[]),
        test_mod("example.target", &[], &[]),
    ]);

    mods.all[0].inferred_after.push(InferredDependency {
        package_id: package_id("example.target"),
        kind: InferredDependencyKind::FindMod,
        source_file: PathBuf::from("Patches/Compat.xml"),
    });

    let report = sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        ["example.target", "example.compat"]
    );
    assert_eq!(report.inferred_edges_applied, 1);
    assert_eq!(report.inferred_edges_skipped, 0);

    Ok(())
}

#[test]
fn assembly_reference_inferred_edge_outranks_conflicting_xml_hint() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.addon", &[], &[]),
        test_mod("example.framework", &[], &[]),
    ]);

    // The framework has an XML hint pointing at the addon, while the addon's
    // C# DLL actually compiles against the framework.
    mods.all[1].inferred_after.push(InferredDependency {
        package_id: package_id("example.addon"),
        kind: InferredDependencyKind::MayRequire,
        source_file: PathBuf::from("Defs/Compat.xml"),
    });
    mods.all[0].inferred_after.push(InferredDependency {
        package_id: package_id("example.framework"),
        kind: InferredDependencyKind::AssemblyReference,
        source_file: PathBuf::from("Assemblies/Addon.dll"),
    });

    let report = sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        ["example.framework", "example.addon"]
    );
    assert_eq!(report.inferred_edges_applied, 1);
    assert_eq!(report.inferred_edges_skipped, 1);

    Ok(())
}

#[test]
fn soft_tiers_place_libraries_early_and_patch_only_mods_last() -> Result<(), SortError> {
    let mut patch_only = test_mod("example.tweak", &[], &[]);
    patch_only.is_patch_only = true;

    let regular = test_mod("example.content", &[], &[]);

    let mut addon = test_mod("example.addon", &[], &[]);
    addon.mod_dependencies.push(package_id("example.library"));

    let library = test_mod("example.library", &[], &[]);

    let mut mods = active_collection(vec![patch_only, regular, addon, library]);

    sort_mods(&mut mods)?;
    assert_eq!(
        active_package_ids(&mods),
        [
            "example.library",
            "example.content",
            "example.addon",
            "example.tweak"
        ]
    );

    // Must remain stable on repeated sorts.
    sort_mods(&mut mods)?;
    assert_eq!(
        active_package_ids(&mods),
        [
            "example.library",
            "example.content",
            "example.addon",
            "example.tweak"
        ]
    );

    Ok(())
}

#[test]
fn inferred_edge_that_would_create_a_cycle_is_skipped() -> Result<(), SortError> {
    // The target explicitly loads after the compat mod.
    let mut mods = active_collection(vec![
        test_mod("example.compat", &[], &[]),
        test_mod("example.target", &["example.compat"], &[]),
    ]);

    // An inference claims the opposite, which must not break sorting.
    mods.all[0].inferred_after.push(InferredDependency {
        package_id: package_id("example.target"),
        kind: InferredDependencyKind::MayRequire,
        source_file: PathBuf::from("Defs/Compat.xml"),
    });

    let report = sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        ["example.compat", "example.target"]
    );
    assert_eq!(report.inferred_edges_applied, 0);
    assert_eq!(report.inferred_edges_skipped, 1);

    Ok(())
}

#[test]
fn conflicting_declared_dependency_is_skipped_instead_of_failing() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.a", &[], &[]),
        test_mod("example.b", &["example.a"], &[]),
    ]);

    mods.all[0].mod_dependencies.push(package_id("example.b"));

    let report = sort_mods(&mut mods)?;

    assert_eq!(active_package_ids(&mods), ["example.a", "example.b"]);
    assert_eq!(report.dependency_edges_skipped, 1);

    Ok(())
}

#[test]
fn a_mod_that_loads_before_official_content_moves_above_it() -> Result<(), SortError> {
    // Harmony / Loading Progress shape: `loadBefore Ludeon.RimWorld` means
    // "above Core", not "Core loads last".
    let mut mods = active_collection(vec![
        test_mod("example.library", &[], &["ludeon.rimworld"]),
        test_mod("ludeon.rimworld", &[], &[]),
        test_mod("example.addon", &[], &[]),
    ]);

    mods.all[1].source = ModSource::Official;

    sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        ["example.library", "ludeon.rimworld", "example.addon"]
    );

    Ok(())
}

#[test]
fn official_content_ignores_inferred_edges_that_would_sink_it() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("ludeon.rimworld", &[], &[]),
        test_mod("ludeon.rimworld.biotech", &[], &[]),
        test_mod("example.addon", &[], &[]),
    ]);

    mods.all[0].source = ModSource::Official;
    mods.all[1].source = ModSource::Official;

    // Core defs gate Biotech content with MayRequire; that is not a reason for
    // Core to load after the DLC.
    mods.all[0].inferred_after.push(InferredDependency {
        package_id: package_id("example.addon"),
        kind: InferredDependencyKind::MayRequire,
        source_file: PathBuf::from("Defs/Core.xml"),
    });

    let report = sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        [
            "ludeon.rimworld",
            "ludeon.rimworld.biotech",
            "example.addon"
        ]
    );
    assert_eq!(report.inferred_edges_skipped, 1);

    Ok(())
}

#[test]
fn official_content_is_ordered_by_its_own_rules() -> Result<(), SortError> {
    // Royalty is currently below Ideology, but declares forceLoadBefore it.
    let mut royalty = test_mod("ludeon.rimworld.royalty", &[], &[]);
    royalty.force_load_after.push(package_id("ludeon.rimworld"));
    royalty
        .force_load_before
        .push(package_id("ludeon.rimworld.ideology"));

    let mut ideology = test_mod("ludeon.rimworld.ideology", &[], &[]);
    ideology
        .force_load_after
        .push(package_id("ludeon.rimworld"));

    let mut mods = active_collection(vec![
        test_mod("ludeon.rimworld", &[], &[]),
        ideology,
        royalty,
        test_mod("example.addon", &[], &[]),
    ]);

    for rimworld_mod in mods.all.iter_mut().take(3) {
        rimworld_mod.source = ModSource::Official;
    }

    sort_mods(&mut mods)?;

    assert_eq!(
        active_package_ids(&mods),
        [
            "ludeon.rimworld",
            "ludeon.rimworld.royalty",
            "ludeon.rimworld.ideology",
            "example.addon"
        ]
    );

    Ok(())
}

#[test]
fn order_warnings_report_a_mod_below_official_content_it_must_precede() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("ludeon.rimworld", &[], &[]),
        test_mod("example.library", &[], &["ludeon.rimworld"]),
    ]);

    mods.all[0].source = ModSource::Official;

    let warnings = find_order_warnings(&mods)?;
    let warning = warnings
        .get(&ModId::from_index(1))
        .expect("library must be warned about Core");

    assert!(warning.before_mod_ids.contains(&ModId::from_index(0)));

    Ok(())
}

#[test]
fn order_warnings_report_incompatibilities_in_both_directions() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("example.cooltabs", &[], &[]),
        test_mod("example.researchtree", &[], &[]),
    ]);

    mods.all[0]
        .incompatible_with
        .push(package_id("example.researchtree"));

    let warnings = find_order_warnings(&mods)?;

    let declaring = warnings
        .get(&ModId::from_index(0))
        .expect("declaring mod must be warned");
    assert_eq!(declaring.incompatible_mod_ids, [ModId::from_index(1)]);

    let named = warnings
        .get(&ModId::from_index(1))
        .expect("the named mod must be warned too");
    assert_eq!(named.incompatible_mod_ids, [ModId::from_index(0)]);

    Ok(())
}

#[test]
fn order_warnings_ignore_incompatibilities_with_inactive_mods() -> Result<(), SortError> {
    let mut mods = active_collection(vec![test_mod("example.cooltabs", &[], &[])]);

    mods.all[0]
        .incompatible_with
        .push(package_id("example.notinstalled"));

    assert!(find_order_warnings(&mods)?.is_empty());

    Ok(())
}

#[test]
fn order_warnings_only_list_misplaced_mods() -> Result<(), SortError> {
    let mut mods = active_collection(vec![
        test_mod("ludeon.rimworld", &[], &[]),
        test_mod("ludeon.rimworld.royalty", &["ludeon.rimworld"], &[]),
    ]);

    mods.all[0].source = ModSource::Official;
    mods.all[1].source = ModSource::Official;

    let warnings = find_order_warnings(&mods)?;

    assert!(warnings.is_empty());

    Ok(())
}

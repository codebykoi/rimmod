use std::path::PathBuf;

use super::*;

fn package_id(raw: &str) -> PackageId {
    PackageId::new(raw).expect("test package IDs must not be empty")
}

fn temporary_path(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "rimmod-organization-{}-{name}.json",
        std::process::id()
    ))
}

#[test]
fn tags_are_keyed_by_package_id_not_by_load_order_index() {
    let mut organization = Organization::default();
    let installed = package_id("example.mod");

    assert!(organization.add_tag(&installed, "Quality of life"));
    assert!(organization.add_tag(&installed, "Chinese"));

    // The mod folder disappears, then comes back later with a different index.
    // Lookups only ever use the package ID, so the tags survive.
    let reinstalled = package_id("example.mod");

    assert_eq!(
        organization.tags(&reinstalled),
        ["Quality of life", "Chinese"]
    );
}

#[test]
fn package_ids_are_normalized_before_lookup() {
    let mut organization = Organization::default();

    organization.add_tag(&package_id("Author.Mod"), "Fixes");

    assert_eq!(organization.tags(&package_id("author.mod")), ["Fixes"]);
}

#[test]
fn duplicate_tags_are_ignored_case_insensitively() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    assert!(organization.add_tag(&package, "Fixes"));
    assert!(!organization.add_tag(&package, "fixes"));
    assert!(!organization.add_tag(&package, "  FIXES  "));

    assert_eq!(organization.tags(&package).len(), 1);
}

#[test]
fn tags_are_collapsed_and_capped() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");
    let long_tag = "a".repeat(MAX_TAG_CHARS + 10);

    assert!(organization.add_tag(&package, "  Quality   of   life  "));
    assert!(organization.add_tag(&package, &long_tag));

    let tags = organization.tags(&package);

    assert_eq!(tags[0], "Quality of life");
    assert_eq!(tags[1].chars().count(), MAX_TAG_CHARS);
}

#[test]
fn empty_tags_are_rejected() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    assert!(!organization.add_tag(&package, "   "));
    assert!(!organization.add_tag(&package, ""));

    assert!(organization.tags(&package).is_empty());
}

#[test]
fn removing_a_tag_empties_the_entry() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    organization.add_tag(&package, "Fixes");
    assert!(organization.remove_tag(&package, "fixes"));

    // Nothing left to remember, so the entry is dropped.
    assert!(organization.mods.is_empty());
}

#[test]
fn removing_an_absent_tag_reports_no_change() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    assert!(!organization.remove_tag(&package, "missing"));
    assert!(!organization.clear_tags(&package));
}

#[test]
fn color_can_be_set_and_cleared() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    assert!(organization.set_color(&package, Some(ModColor::Teal)));
    assert_eq!(organization.color(&package), Some(ModColor::Teal));

    assert!(!organization.set_color(&package, Some(ModColor::Teal)));

    assert!(organization.set_color(&package, None));
    assert_eq!(organization.color(&package), None);
    assert!(organization.mods.is_empty());
}

#[test]
fn color_survives_without_tags_and_tags_survive_without_color() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    organization.set_color(&package, Some(ModColor::Purple));
    organization.add_tag(&package, "Keep");

    organization.set_color(&package, None);

    assert_eq!(organization.tags(&package), ["Keep"]);
}

#[test]
fn tag_filter_requires_every_selected_tag() {
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    organization.add_tag(&package, "Fixes");
    organization.add_tag(&package, "Slow");

    assert!(organization.matches_tags(&package, &[]));
    assert!(organization.matches_tags(&package, &["fixes".to_owned()]));
    assert!(organization.matches_tags(&package, &["Fixes".to_owned(), "Slow".to_owned()]));
    assert!(!organization.matches_tags(&package, &["Fixes".to_owned(), "Missing".to_owned()]));
}

#[test]
fn all_tags_counts_mods_and_is_sorted_case_insensitively() {
    let mut organization = Organization::default();
    let first = package_id("example.a");
    let second = package_id("example.b");

    organization.add_tag(&first, "zeta");
    organization.add_tag(&first, "Fixes");
    organization.add_tag(&second, "fixes");

    assert_eq!(
        organization.all_tags(),
        [("Fixes".to_owned(), 2), ("zeta".to_owned(), 1)]
    );
}

#[test]
fn saved_tags_can_be_loaded_again() {
    let path = temporary_path("round-trip");
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    organization.add_tag(&package, "Quality of life");
    organization.set_color(&package, Some(ModColor::Amber));

    save(&organization, &path).expect("save organization");
    let loaded = load(&path).expect("load organization");

    assert_eq!(loaded, organization);

    fs::remove_file(path).expect("remove test file");
}

#[test]
fn a_missing_file_loads_as_an_empty_organization() {
    let path = temporary_path("missing");
    let _ = fs::remove_file(&path);

    let loaded = load(&path).expect("missing file is not an error");

    assert!(loaded.mods.is_empty());
}

#[test]
fn malformed_files_are_reported_instead_of_silently_discarded() {
    let path = temporary_path("malformed");
    fs::write(&path, "{ not json").expect("write malformed file");

    let result = load(&path);

    assert!(result.is_err());

    fs::remove_file(path).expect("remove test file");
}

#[test]
fn saved_file_uses_package_ids_and_readable_color_names() {
    let path = temporary_path("schema");
    let mut organization = Organization::default();
    let package = package_id("example.mod");

    organization.add_tag(&package, "Fixes");
    organization.set_color(&package, Some(ModColor::Purple));
    save(&organization, &path).expect("save organization");

    let contents = fs::read_to_string(&path).expect("read saved file");

    assert!(contents.contains("\"example.mod\""));
    assert!(contents.contains("\"purple\""));

    fs::remove_file(path).expect("remove test file");
}

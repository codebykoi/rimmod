use super::*;

#[test]
fn parses_versions_and_workshop_ids() {
    let markdown = "\
# Currently covered mods
## 1.6
Some intro text that is not a link
- [Fuse Plus - Retexture](https://steamcommunity.com/sharedfiles/filedetails/?id=2525222816)
## 1.4
- [An old mod](https://steamcommunity.com/sharedfiles/filedetails/?id=111)
";

    let covered = parse_covered_mods(markdown);

    assert_eq!(covered.get(&2525222816), Some(&vec!["1.6".to_owned()]));
    assert_eq!(covered.get(&111), Some(&vec!["1.4".to_owned()]));
    assert!(!covered.contains_key(&2222));
}

#[test]
fn mods_covered_for_multiple_versions_accumulate() {
    let markdown = "\
## 1.5
- [A mod](https://steamcommunity.com/sharedfiles/filedetails/?id=42)
## 1.6
- [A mod](https://steamcommunity.com/sharedfiles/filedetails/?id=42)
";

    let covered = parse_covered_mods(markdown);

    assert_eq!(
        covered.get(&42),
        Some(&vec!["1.5".to_owned(), "1.6".to_owned()])
    );
}

#[test]
fn duplicate_listings_are_ignored() {
    let markdown = "\
## 1.6
- [A mod](https://steamcommunity.com/sharedfiles/filedetails/?id=42)
- [A mod](https://steamcommunity.com/sharedfiles/filedetails/?id=42)
";

    let covered = parse_covered_mods(markdown);

    assert_eq!(covered.get(&42), Some(&vec!["1.6".to_owned()]));
}

#[test]
fn entries_before_any_version_heading_are_ignored() {
    let markdown = "\
- [Early mod](https://steamcommunity.com/sharedfiles/filedetails/?id=7)
## 1.6
- [A mod](https://steamcommunity.com/sharedfiles/filedetails/?id=42)
";

    let covered = parse_covered_mods(markdown);

    assert!(!covered.contains_key(&7));
    assert!(covered.contains_key(&42));
}

#[test]
fn mod_ids_to_fix_fills_in_versions_missing_from_mods_md_headings() {
    let markdown = "\
## 1.6
- [Marios Pizza](https://steamcommunity.com/sharedfiles/filedetails/?id=3226188356)
## 1.3
- [Marios Pizza](https://steamcommunity.com/sharedfiles/filedetails/?id=3226188356)
";
    let xml = "\
\u{feff}<?xml version=\"1.0\" encoding=\"utf-8\"?>
<ModIdsToFix>
  <!-- Marios Pizza -->
  <li>Sov.Pizza</li>
</ModIdsToFix>
";

    let (mut covered, name_index, headings) = parse_covered_mods_with_names(markdown);
    merge_mod_ids_to_fix(&mut covered, &name_index, "1.7", xml);

    assert_eq!(
        versions_to_cross_check(&headings, Some("1.6.4211")),
        vec!["1.7".to_owned(), "1.6".to_owned()]
    );
    assert_eq!(
        covered.get(&3226188356),
        Some(&vec!["1.6".to_owned(), "1.3".to_owned(), "1.7".to_owned()])
    );
}

#[test]
fn mod_ids_to_fix_handles_brackets_in_names_and_skips_ambiguous_names() {
    let markdown = "\
## 1.6
- [[KV] Hand 'n' Footwear [Adopted]](https://steamcommunity.com/sharedfiles/filedetails/?id=3266890815)
- [Wheelchairs](https://steamcommunity.com/sharedfiles/filedetails/?id=100)
- [Wheelchairs](https://steamcommunity.com/sharedfiles/filedetails/?id=200)
";
    let xml = "\
<ModIdsToFix>
  <!-- [KV] Hand 'n' Footwear [Adopted] -->
  <li>handandfootwearcontinued.kv.rw</li>
  <!-- Wheelchairs -->
  <li>example.wheelchairs</li>
</ModIdsToFix>
";

    let (mut covered, name_index, _) = parse_covered_mods_with_names(markdown);
    merge_mod_ids_to_fix(&mut covered, &name_index, "1.7", xml);

    assert_eq!(
        covered.get(&3266890815),
        Some(&vec!["1.6".to_owned(), "1.7".to_owned()])
    );
    assert_eq!(covered.get(&100), Some(&vec!["1.6".to_owned()]));
    assert_eq!(covered.get(&200), Some(&vec!["1.6".to_owned()]));
}

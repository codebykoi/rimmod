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

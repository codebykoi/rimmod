use super::*;

#[test]
fn supported_version_matches_a_full_game_build() {
    assert!(versions_are_compatible("1.6", "1.6.1234 rev1"));
}

#[test]
fn different_minor_versions_do_not_match() {
    assert!(!versions_are_compatible("1.5", "1.6.1234 rev1"));
}

#[test]
fn invalid_versions_do_not_match() {
    assert!(!versions_are_compatible("unknown", "unknown"));
}

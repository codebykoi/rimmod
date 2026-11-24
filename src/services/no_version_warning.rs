use std::collections::{HashMap, HashSet};
use std::time::Duration;

use crate::models::{major_minor_version, major_minor_version_text};
use crate::services::workshop::RIMWORLD_VERSIONS;

const REPO_RAW_BASE_URL: &str = "https://raw.githubusercontent.com/emipa606/NoVersionWarning/main";
const COVERED_MODS_URL: &str =
    "https://raw.githubusercontent.com/emipa606/NoVersionWarning/main/MODS.md";

/// Workshop item IDs covered by the No Version Warning community reports,
/// mapped to the RimWorld versions they are reported to work with.
pub(crate) type CoveredMods = HashMap<u64, Vec<String>>;

/// Lowercased mod names from `MODS.md` mapped to their Workshop ID when unique,
/// or `None` if multiple different Workshop IDs share the same name.
type ModNameIndex = HashMap<String, Option<u64>>;

pub(crate) fn fetch_covered_mods(game_version: Option<&str>) -> Result<CoveredMods, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(20))
        .user_agent(concat!("RimMod/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("could not create the HTTP client: {error}"))?;

    let markdown = client
        .get(COVERED_MODS_URL)
        .send()
        .map_err(|error| format!("request failed: {error}"))?
        .error_for_status()
        .map_err(|error| format!("request rejected: {error}"))?
        .text()
        .map_err(|error| format!("could not read the response: {error}"))?;

    let (mut covered, name_index, headings) = parse_covered_mods_with_names(&markdown);

    // `MODS.md` can occasionally miss or misfile entries for a newly added
    // RimWorld version (e.g. entries appended without a new `## X.Y` heading).
    // Cross-referencing `<version>/ModIdsToFix.xml` fills in those gaps by
    // matching its `<!-- Mod Name -->` comments against unique `MODS.md` links.
    for version in versions_to_cross_check(&headings, game_version) {
        let version_url = format!("{REPO_RAW_BASE_URL}/{version}/ModIdsToFix.xml");
        if let Ok(response) = client.get(version_url).send()
            && let Ok(response) = response.error_for_status()
            && let Ok(xml) = response.text()
        {
            merge_mod_ids_to_fix(&mut covered, &name_index, &version, &xml);
        }
    }

    for versions in covered.values_mut() {
        versions.sort_by_key(|version| major_minor_version(version));
    }

    Ok(covered)
}

/// Choose which `<version>/ModIdsToFix.xml` files to cross-check against
/// `MODS.md`: any known RimWorld version newer than `MODS.md`'s highest
/// heading, plus the user's current game version.
fn versions_to_cross_check(headings: &HashSet<String>, game_version: Option<&str>) -> Vec<String> {
    let highest_heading = headings
        .iter()
        .filter_map(|heading| major_minor_version(heading))
        .max();

    let mut versions = Vec::new();

    for candidate in RIMWORLD_VERSIONS {
        let Some(candidate_key) = major_minor_version(candidate) else {
            continue;
        };
        if highest_heading.is_none_or(|highest| candidate_key > highest) {
            versions.push((*candidate).to_owned());
        }
    }

    if let Some(current) = game_version.and_then(major_minor_version_text)
        && !versions.contains(&current)
    {
        versions.push(current);
    }

    versions
}

/// Parse the No Version Warning "covered mods" list:
///
/// ```markdown
/// ## 1.6
/// - [Fuse Plus - Retexture](https://steamcommunity.com/sharedfiles/filedetails/?id=2525222816)
/// ```
#[cfg(test)]
fn parse_covered_mods(markdown: &str) -> CoveredMods {
    parse_covered_mods_with_names(markdown).0
}

fn parse_covered_mods_with_names(markdown: &str) -> (CoveredMods, ModNameIndex, HashSet<String>) {
    const STEAM_LINK_START: &str = "](https://";

    let mut covered = CoveredMods::new();
    let mut name_index = ModNameIndex::new();
    let mut headings = HashSet::new();
    let mut current_version: Option<String> = None;

    for line in markdown.lines() {
        let line = line.trim();

        if let Some(version) = line.strip_prefix("## ") {
            let version = version.trim().to_owned();
            headings.insert(version.clone());
            current_version = Some(version);
            continue;
        }

        let Some(version) = current_version.as_ref() else {
            continue;
        };

        let Some(link_start) = line.find(STEAM_LINK_START) else {
            continue;
        };
        let url_start = link_start + STEAM_LINK_START.len();
        let Some(url_end) = line[url_start..].find(')') else {
            continue;
        };
        let url = &line[url_start..url_start + url_end];
        let Some(id_start) = url.find("id=") else {
            continue;
        };

        let id_text: String = url[id_start + "id=".len()..]
            .chars()
            .take_while(|character| character.is_ascii_digit())
            .collect();
        let Ok(workshop_id) = id_text.parse::<u64>() else {
            continue;
        };

        if let Some(name) = extract_markdown_link_name(&line[..link_start]) {
            name_index
                .entry(name)
                .and_modify(|existing| {
                    if *existing != Some(workshop_id) {
                        *existing = None;
                    }
                })
                .or_insert(Some(workshop_id));
        }

        let versions = covered.entry(workshop_id).or_default();
        if !versions.contains(version) {
            versions.push(version.clone());
        }
    }

    (covered, name_index, headings)
}

fn extract_markdown_link_name(prefix: &str) -> Option<String> {
    let trimmed = prefix.trim().strip_prefix('-').unwrap_or(prefix).trim();
    let name = trimmed.strip_prefix('[')?.trim();
    if name.is_empty() {
        None
    } else {
        Some(name.to_lowercase())
    }
}

fn merge_mod_ids_to_fix(
    covered: &mut CoveredMods,
    name_index: &ModNameIndex,
    version: &str,
    xml: &str,
) {
    let mut rest = xml;

    while let Some(start) = rest.find("<!--") {
        let after_start = &rest[start + "<!--".len()..];
        let Some(end) = after_start.find("-->") else {
            break;
        };
        let name = after_start[..end].trim().to_lowercase();
        rest = &after_start[end + "-->".len()..];

        if let Some(Some(workshop_id)) = name_index.get(&name) {
            let versions = covered.entry(*workshop_id).or_default();
            if !versions.iter().any(|existing| existing == version) {
                versions.push(version.to_owned());
            }
        }
    }
}

#[cfg(test)]
#[path = "../../tests/unit/no_version_warning.rs"]
mod tests;

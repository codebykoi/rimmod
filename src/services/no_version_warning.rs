use std::collections::HashMap;
use std::time::Duration;

const COVERED_MODS_URL: &str =
    "https://raw.githubusercontent.com/emipa606/NoVersionWarning/master/MODS.md";

/// Workshop item IDs covered by the No Version Warning community reports,
/// mapped to the RimWorld versions they are reported to work with.
pub(crate) type CoveredMods = HashMap<u64, Vec<String>>;

pub(crate) fn fetch_covered_mods() -> Result<CoveredMods, String> {
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

    Ok(parse_covered_mods(&markdown))
}

/// Parse the No Version Warning "covered mods" list:
///
/// ```markdown
/// ## 1.6
/// - [Fuse Plus - Retexture](https://steamcommunity.com/sharedfiles/filedetails/?id=2525222816)
/// ```
fn parse_covered_mods(markdown: &str) -> CoveredMods {
    const STEAM_LINK_START: &str = "](https://";

    let mut covered = CoveredMods::new();
    let mut current_version: Option<String> = None;

    for line in markdown.lines() {
        let line = line.trim();

        if let Some(version) = line.strip_prefix("## ") {
            current_version = Some(version.trim().to_owned());
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

        let versions = covered.entry(workshop_id).or_default();
        if !versions.contains(version) {
            versions.push(version.clone());
        }
    }

    covered
}

#[cfg(test)]
#[path = "../../tests/unit/no_version_warning.rs"]
mod tests;

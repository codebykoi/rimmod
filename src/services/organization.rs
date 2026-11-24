//! Persistent tags and colors for mods.
//!
//! Entries are keyed by package ID rather than by `ModId`, because `ModId` is
//! only an index into the current scan. A mod can be disabled, deleted or
//! reinstalled without losing the tags the user attached to it: as soon as a
//! folder with the same package ID appears again, its organization comes back.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::models::PackageId;

/// Upper bound for one tag. Keeps row chips readable and the file tidy.
pub(crate) const MAX_TAG_CHARS: usize = 32;

const FILE_NAME: &str = "organization.json";

/// Colors available to mod entries. Kept intentionally small and muted so a
/// colored list still reads as part of the app theme instead of a rainbow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ModColor {
    Blue,
    Teal,
    Green,
    Amber,
    Orange,
    Red,
    Pink,
    Purple,
    Slate,
}

impl ModColor {
    pub(crate) const ALL: [Self; 9] = [
        Self::Blue,
        Self::Teal,
        Self::Green,
        Self::Amber,
        Self::Orange,
        Self::Red,
        Self::Pink,
        Self::Purple,
        Self::Slate,
    ];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Blue => "Blue",
            Self::Teal => "Teal",
            Self::Green => "Green",
            Self::Amber => "Amber",
            Self::Orange => "Orange",
            Self::Red => "Red",
            Self::Pink => "Pink",
            Self::Purple => "Purple",
            Self::Slate => "Slate",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
struct ModOrganization {
    tags: Vec<String>,
    color: Option<ModColor>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Organization {
    /// Keyed by normalized package ID (`author.mod`).
    mods: BTreeMap<String, ModOrganization>,
}

impl Organization {
    pub(crate) fn tags(&self, package_id: &PackageId) -> &[String] {
        self.mods
            .get(package_id.as_str())
            .map_or(&[], |organization| organization.tags.as_slice())
    }

    pub(crate) fn color(&self, package_id: &PackageId) -> Option<ModColor> {
        self.mods
            .get(package_id.as_str())
            .and_then(|organization| organization.color)
    }

    pub(crate) fn set_color(&mut self, package_id: &PackageId, color: Option<ModColor>) -> bool {
        let current = self.color(package_id);

        if current == color {
            return false;
        }

        self.entry_mut(package_id).color = color;
        self.prune(package_id);

        true
    }

    /// Add one tag to a mod. Returns `false` when the tag was already present
    /// or normalizes to nothing.
    pub(crate) fn add_tag(&mut self, package_id: &PackageId, tag: &str) -> bool {
        let Some(tag) = normalize_tag(tag) else {
            return false;
        };

        if self.has_tag(package_id, &tag) {
            return false;
        }

        self.entry_mut(package_id).tags.push(tag);
        true
    }

    pub(crate) fn remove_tag(&mut self, package_id: &PackageId, tag: &str) -> bool {
        let Some(entry) = self.mods.get_mut(package_id.as_str()) else {
            return false;
        };

        let Some(position) = entry
            .tags
            .iter()
            .position(|candidate| candidate.eq_ignore_ascii_case(tag))
        else {
            return false;
        };

        entry.tags.remove(position);
        self.prune(package_id);

        true
    }

    pub(crate) fn clear_tags(&mut self, package_id: &PackageId) -> bool {
        let Some(entry) = self.mods.get_mut(package_id.as_str()) else {
            return false;
        };

        if entry.tags.is_empty() {
            return false;
        }

        entry.tags.clear();
        self.prune(package_id);

        true
    }

    fn has_tag(&self, package_id: &PackageId, tag: &str) -> bool {
        self.tags(package_id)
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(tag))
    }

    /// True when the mod carries every requested tag. An empty filter matches
    /// everything, so the list never hides mods unless a tag was picked.
    pub(crate) fn matches_tags(&self, package_id: &PackageId, filters: &[String]) -> bool {
        if filters.is_empty() {
            return true;
        }

        let tags = self.tags(package_id);

        filters.iter().all(|filter| {
            tags.iter()
                .any(|tag| tag.eq_ignore_ascii_case(filter.trim()))
        })
    }

    /// Every remembered tag, with the number of mods carrying it, sorted
    /// case-insensitively by name. Includes mods that are currently uninstalled
    /// but still remembered.
    pub(crate) fn all_tags(&self) -> Vec<(String, usize)> {
        let mut tags: BTreeMap<String, (String, usize)> = BTreeMap::new();

        for organization in self.mods.values() {
            for tag in &organization.tags {
                tags.entry(tag.to_lowercase())
                    .and_modify(|(_, count)| *count += 1)
                    .or_insert_with(|| (tag.clone(), 1));
            }
        }

        tags.into_values().collect()
    }

    fn entry_mut(&mut self, package_id: &PackageId) -> &mut ModOrganization {
        self.mods.entry(package_id.as_str().to_owned()).or_default()
    }

    /// Drop entries that no longer carry tags or a color, so the file stays
    /// limited to real organization data.
    fn prune(&mut self, package_id: &PackageId) {
        let key = package_id.as_str();

        if self.mods.get(key).is_some_and(|organization| {
            organization.tags.is_empty() && organization.color.is_none()
        }) {
            self.mods.remove(key);
        }
    }
}

fn normalize_tag(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.is_empty() {
        return None;
    }

    Some(collapsed.chars().take(MAX_TAG_CHARS).collect())
}

/// `organization.json` in the app's per-user data directory.
pub(crate) fn file_path() -> Option<PathBuf> {
    crate::services::data_directory().map(|directory| directory.join(FILE_NAME))
}

pub(crate) fn load(path: &Path) -> io::Result<Organization> {
    match fs::read_to_string(path) {
        Ok(contents) => serde_json::from_str(&contents)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error)),

        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Organization::default()),

        Err(error) => Err(error),
    }
}

pub(crate) fn save(organization: &Organization, path: &Path) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let contents = serde_json::to_string_pretty(organization)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    // Write beside the target and rename over it, so an interrupted save never
    // leaves a half-written file behind.
    let temporary_path = path.with_extension("json.tmp");
    fs::write(&temporary_path, contents)?;

    if let Err(error) = fs::rename(&temporary_path, path) {
        let _ = fs::remove_file(&temporary_path);

        return Err(error);
    }

    Ok(())
}

#[cfg(test)]
#[path = "../../tests/unit/organization.rs"]
mod tests;

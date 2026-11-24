use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::models::{ModCollection, ModSource, PackageId};

use super::{ModpackManifest, PackMod, PackSource};

/// One currently active mod that is not in the compared modpack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DiffMod {
    pub(crate) package_id: String,
    pub(crate) name: String,
    pub(crate) source: PackSource,
}

/// Difference between applying a modpack and the current active load order.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct PackDiff {
    /// Mods in the pack that are not currently active (applying will enable them).
    pub(crate) added: Vec<PackMod>,
    /// Mods currently active that are not in the pack (applying will disable them).
    pub(crate) removed: Vec<DiffMod>,
    /// Mods active in both lists whose relative load order differs.
    pub(crate) reordered: Vec<PackMod>,
    /// Mods in the pack that are neither installed locally nor bundled inside the pack.
    pub(crate) missing: Vec<PackMod>,
    /// Local mods bundled in the pack that are not currently installed in RimWorld.
    pub(crate) local_to_install: Vec<PackMod>,
}

impl PackDiff {
    pub(crate) fn is_in_sync(&self) -> bool {
        self.added.is_empty()
            && self.removed.is_empty()
            && self.reordered.is_empty()
            && self.missing.is_empty()
    }

    pub(crate) fn total_changes(&self) -> usize {
        self.added.len() + self.removed.len() + self.reordered.len()
    }

    pub(crate) fn missing_workshop_count(&self) -> usize {
        self.missing
            .iter()
            .filter(|pack_mod| pack_mod.workshop_id().is_some())
            .count()
    }
}

/// How a library modpack relates to the currently active load order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PackStatus {
    /// Active mods and their order match this modpack exactly.
    Active,
    /// This was the last applied/saved modpack, and the active load order has
    /// since diverged from it.
    Modified {
        /// Mods enabled in active that are not in the pack.
        active_added: usize,
        /// Mods in the pack that are no longer active.
        active_removed: usize,
        /// Common mods whose relative load order changed.
        reordered: usize,
    },
    /// Not currently active.
    NotApplied,
}

impl PackStatus {
    pub(crate) fn is_active(self) -> bool {
        matches!(self, Self::Active)
    }

    pub(crate) fn is_modified(self) -> bool {
        matches!(self, Self::Modified { .. })
    }

    pub(crate) fn modified_total(self) -> usize {
        match self {
            Self::Modified {
                active_added,
                active_removed,
                reordered,
            } => active_added + active_removed + reordered,
            _ => 0,
        }
    }

    /// Compact summary of how active mods diverged from this pack, e.g. `+2 / -1`.
    pub(crate) fn modified_delta_summary(self) -> Option<String> {
        let Self::Modified {
            active_added,
            active_removed,
            reordered,
        } = self
        else {
            return None;
        };

        let mut parts = Vec::new();
        if active_added > 0 {
            parts.push(format!("+{active_added}"));
        }
        if active_removed > 0 {
            parts.push(format!("-{active_removed}"));
        }
        if reordered > 0 && parts.is_empty() {
            parts.push(format!("~{reordered} order"));
        }

        if parts.is_empty() {
            None
        } else {
            Some(parts.join(" / "))
        }
    }
}

/// Compare `manifest` against the currently loaded `mods` collection.
pub(crate) fn compute_pack_diff(manifest: &ModpackManifest, mods: &ModCollection) -> PackDiff {
    let mut active_order = Vec::new();
    let mut active_positions = HashMap::new();
    let mut active_mods_by_id = HashMap::new();

    for &mod_id in mods.enabled_ids() {
        let Some(rimworld_mod) = mods.get(mod_id) else {
            continue;
        };
        let key = rimworld_mod.package_id.as_str().to_owned();
        if active_positions.contains_key(&key) {
            continue;
        }
        active_positions.insert(key.clone(), active_order.len());
        active_order.push(key.clone());
        active_mods_by_id.insert(key, rimworld_mod);
    }

    let mut seen_pack_ids = HashSet::new();
    let mut added = Vec::new();
    let mut missing = Vec::new();
    let mut local_to_install = Vec::new();
    let mut common_pack_mods: Vec<(usize, &PackMod)> = Vec::new();

    for pack_mod in &manifest.mods {
        let normalized = normalize_package_id(&pack_mod.package_id);
        if normalized.is_empty() || !seen_pack_ids.insert(normalized.clone()) {
            continue;
        }

        let installed = PackageId::new(&normalized)
            .and_then(|package_id| mods.find_id_by_package_id(&package_id))
            .is_some();

        if !installed {
            if pack_mod.is_local() {
                local_to_install.push(pack_mod.clone());
            } else {
                missing.push(pack_mod.clone());
            }
        }

        if let Some(&active_index) = active_positions.get(&normalized) {
            common_pack_mods.push((active_index, pack_mod));
        } else {
            added.push(pack_mod.clone());
        }
    }

    let mut removed = Vec::new();
    for key in &active_order {
        if !seen_pack_ids.contains(key)
            && let Some(rimworld_mod) = active_mods_by_id.get(key)
        {
            removed.push(DiffMod {
                package_id: rimworld_mod.package_id.as_str().to_owned(),
                name: rimworld_mod.name.clone(),
                source: pack_source_from_mod_source(&rimworld_mod.source),
            });
        }
    }

    let active_indices = common_pack_mods
        .iter()
        .map(|(index, _)| *index)
        .collect::<Vec<_>>();
    let stationary = longest_increasing_subsequence_mask(&active_indices);
    let reordered = common_pack_mods
        .into_iter()
        .enumerate()
        .filter(|&(pos, _)| !stationary[pos])
        .map(|(_, (_, pack_mod))| pack_mod.clone())
        .collect();

    PackDiff {
        added,
        removed,
        reordered,
        missing,
        local_to_install,
    }
}

/// Derive a pack's status badge from its diff and the tracked active pack path.
pub(crate) fn compute_pack_status(
    pack_path: &Path,
    active_pack: Option<&Path>,
    manifest: &ModpackManifest,
    diff: &PackDiff,
) -> PackStatus {
    if !manifest.mods.is_empty() && diff.is_in_sync() {
        return PackStatus::Active;
    }

    if active_pack.is_some_and(|active| active == pack_path) {
        return PackStatus::Modified {
            active_added: diff.removed.len(),
            active_removed: diff.added.len(),
            reordered: diff.reordered.len(),
        };
    }

    PackStatus::NotApplied
}

/// Format a clean, shareable summary of a modpack's mod list (suitable for
/// Discord, Reddit, or plain text).
pub(crate) fn format_mod_list_for_share(manifest: &ModpackManifest) -> String {
    let mut out = String::new();
    out.push_str(&format!("# {}\n", manifest.name));

    let mut meta = Vec::new();
    if let Some(version) = manifest.game_version.as_deref() {
        meta.push(format!("RimWorld {version}"));
    }
    meta.push(format!(
        "{} mod{}",
        manifest.mods.len(),
        if manifest.mods.len() == 1 { "" } else { "s" }
    ));
    if !manifest.configs.is_empty() {
        meta.push(format!(
            "{} settings file{}",
            manifest.configs.len(),
            if manifest.configs.len() == 1 { "" } else { "s" }
        ));
    }
    if !manifest.saves.is_empty() {
        meta.push(format!(
            "{} save{}",
            manifest.saves.len(),
            if manifest.saves.len() == 1 { "" } else { "s" }
        ));
    }
    out.push_str(&meta.join(", "));
    out.push_str("\n\n");

    for (index, pack_mod) in manifest.mods.iter().enumerate() {
        let number = index + 1;
        match &pack_mod.source {
            PackSource::Workshop { workshop_id } => {
                out.push_str(&format!(
                    "{number}. {} (`{}`) — https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}\n",
                    pack_mod.name, pack_mod.package_id
                ));
            }
            PackSource::Official => {
                out.push_str(&format!(
                    "{number}. {} (`{}`) [Official]\n",
                    pack_mod.name, pack_mod.package_id
                ));
            }
            PackSource::Local {
                remote_url: Some(url),
                ..
            } => {
                out.push_str(&format!(
                    "{number}. {} (`{}`) [Git: {url}]\n",
                    pack_mod.name, pack_mod.package_id
                ));
            }
            PackSource::Local { .. } => {
                out.push_str(&format!(
                    "{number}. {} (`{}`) [Local]\n",
                    pack_mod.name, pack_mod.package_id
                ));
            }
        }
    }

    out
}

fn normalize_package_id(raw: &str) -> String {
    raw.trim().to_ascii_lowercase()
}

fn pack_source_from_mod_source(source: &ModSource) -> PackSource {
    match source {
        ModSource::Official => PackSource::Official,
        ModSource::SteamWorkshop { workshop_id } => PackSource::Workshop {
            workshop_id: *workshop_id,
        },
        ModSource::Git { remote_url } => PackSource::Local {
            remote_url: Some(remote_url.clone()),
            path: String::new(),
        },
        ModSource::Local | ModSource::Unknown => PackSource::Local {
            remote_url: None,
            path: String::new(),
        },
    }
}

/// Returns a boolean mask marking elements that belong to a longest strictly
/// increasing subsequence. Elements with `false` are the minimal set of mods
/// whose relative order changed.
fn longest_increasing_subsequence_mask(values: &[usize]) -> Vec<bool> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }

    let mut tails: Vec<usize> = Vec::with_capacity(n);
    let mut tail_indices: Vec<usize> = Vec::with_capacity(n);
    let mut prev: Vec<Option<usize>> = vec![None; n];

    for (i, &value) in values.iter().enumerate() {
        let pos = tails.partition_point(|&tail| tail < value);
        if pos > 0 {
            prev[i] = Some(tail_indices[pos - 1]);
        }
        if pos == tails.len() {
            tails.push(value);
            tail_indices.push(i);
        } else {
            tails[pos] = value;
            tail_indices[pos] = i;
        }
    }

    let mut mask = vec![false; n];
    let mut cursor = tail_indices.last().copied();
    while let Some(index) = cursor {
        mask[index] = true;
        cursor = prev[index];
    }

    mask
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::models::{ModId, ModType, RimworldMod};

    use super::*;

    fn make_mod(package_id: &str, name: &str, source: ModSource) -> RimworldMod {
        RimworldMod {
            name: name.to_owned(),
            package_id: PackageId::new(package_id).expect("valid package id"),
            description: String::new(),
            supported_versions: vec!["1.7".to_owned()],
            community_supported_versions: Vec::new(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            force_load_after: Vec::new(),
            force_load_before: Vec::new(),
            mod_dependencies: Vec::new(),
            incompatible_with: Vec::new(),
            inferred_after: Vec::new(),
            is_patch_only: false,
            folder: PathBuf::from(package_id),
            source,
            mod_type: ModType::Xml,
        }
    }

    fn make_collection(all: Vec<RimworldMod>, enabled_indices: &[usize]) -> ModCollection {
        let enabled_ids: Vec<ModId> = enabled_indices
            .iter()
            .copied()
            .map(ModId::from_index)
            .collect();
        let disabled_ids: Vec<ModId> = (0..all.len())
            .filter(|idx| !enabled_indices.contains(idx))
            .map(ModId::from_index)
            .collect();
        ModCollection::new(all, disabled_ids, enabled_ids, Vec::new())
    }

    fn make_pack_mod(package_id: &str, name: &str, source: PackSource) -> PackMod {
        PackMod {
            package_id: package_id.to_owned(),
            name: name.to_owned(),
            folder_name: String::new(),
            source,
        }
    }

    #[test]
    fn exact_match_is_in_sync_and_active() {
        let mods = make_collection(
            vec![
                make_mod("ludeon.rimworld", "Core", ModSource::Official),
                make_mod(
                    "brrainz.harmony",
                    "Harmony",
                    ModSource::SteamWorkshop { workshop_id: 1 },
                ),
            ],
            &[1, 0],
        );

        let mut manifest = ModpackManifest::new("Exact");
        manifest.mods = vec![
            make_pack_mod(
                "brrainz.harmony",
                "Harmony",
                PackSource::Workshop { workshop_id: 1 },
            ),
            make_pack_mod("ludeon.rimworld", "Core", PackSource::Official),
        ];

        let diff = compute_pack_diff(&manifest, &mods);
        assert!(diff.is_in_sync());
        assert_eq!(diff.total_changes(), 0);
        assert_eq!(
            compute_pack_status(Path::new("packs/exact"), None, &manifest, &diff),
            PackStatus::Active
        );
    }

    #[test]
    fn diff_detects_added_removed_missing_local_and_minimal_reorders() {
        let mods = make_collection(
            vec![
                make_mod("a.one", "One", ModSource::Official),
                make_mod("b.two", "Two", ModSource::Official),
                make_mod("c.three", "Three", ModSource::Official),
                make_mod("d.extra", "Extra", ModSource::Local),
            ],
            &[0, 1, 2, 3],
        );

        let mut manifest = ModpackManifest::new("Variant");
        manifest.mods = vec![
            make_pack_mod(
                "e.missing",
                "Missing Workshop",
                PackSource::Workshop { workshop_id: 99 },
            ),
            make_pack_mod("a.one", "One", PackSource::Official),
            make_pack_mod("c.three", "Three", PackSource::Official),
            make_pack_mod("b.two", "Two", PackSource::Official),
            make_pack_mod(
                "f.local",
                "Bundled Local",
                PackSource::Local {
                    remote_url: None,
                    path: "mods/Local".to_owned(),
                },
            ),
        ];

        let diff = compute_pack_diff(&manifest, &mods);

        assert_eq!(diff.added.len(), 2);
        assert_eq!(diff.removed.len(), 1);
        assert_eq!(diff.removed[0].package_id, "d.extra");
        // Only one of Two/Three moved relative to [One, Two, Three].
        assert_eq!(diff.reordered.len(), 1);
        assert_eq!(diff.missing.len(), 1);
        assert_eq!(diff.missing[0].package_id, "e.missing");
        assert_eq!(diff.local_to_install.len(), 1);
        assert_eq!(diff.local_to_install[0].package_id, "f.local");

        let status = compute_pack_status(
            Path::new("packs/variant"),
            Some(Path::new("packs/variant")),
            &manifest,
            &diff,
        );
        assert_eq!(
            status,
            PackStatus::Modified {
                active_added: 1,
                active_removed: 2,
                reordered: 1,
            }
        );
        assert_eq!(status.modified_delta_summary().as_deref(), Some("+1 / -2"));
    }

    #[test]
    fn share_text_lists_workshop_urls_and_sources() {
        let mut manifest = ModpackManifest::new("Cozy Vanilla+");
        manifest.game_version = Some("1.7.5014".to_owned());
        manifest.mods = vec![
            make_pack_mod(
                "brrainz.harmony",
                "Harmony",
                PackSource::Workshop {
                    workshop_id: 2009463077,
                },
            ),
            make_pack_mod("ludeon.rimworld", "Core", PackSource::Official),
        ];

        let text = format_mod_list_for_share(&manifest);
        assert!(text.contains("# Cozy Vanilla+"));
        assert!(text.contains("RimWorld 1.7.5014, 2 mods"));
        assert!(text.contains("https://steamcommunity.com/sharedfiles/filedetails/?id=2009463077"));
        assert!(text.contains("Core (`ludeon.rimworld`) [Official]"));
    }
}

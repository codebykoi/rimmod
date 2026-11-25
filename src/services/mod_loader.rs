use std::collections::{HashMap, HashSet};
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::models::{
    ModCollection, ModId, ModSource, ModType, PackageId, RimworldMod, major_minor_version,
    parse_version_tag, select_best_version_key, versions_are_compatible,
};
use crate::services::dependency_inference;
use crate::services::load_order::parse_config;
use crate::services::settings::Settings;

/// Expected root folder of the mod
#[derive(Clone, Copy)]
enum ModRoot {
    Official,
    Local,
    SteamWorkshop,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModLoadWarningKind {
    SkippedMod,
    UnavailableDirectory,
    CompatibilityData,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ModLoadWarning {
    kind: ModLoadWarningKind,
    path: Option<PathBuf>,
    reason: String,
}

impl ModLoadWarning {
    fn skipped_mod(path: PathBuf, error: impl std::fmt::Display) -> Self {
        Self {
            kind: ModLoadWarningKind::SkippedMod,
            path: Some(path),
            reason: error.to_string(),
        }
    }

    fn unavailable_directory(path: Option<PathBuf>, error: impl std::fmt::Display) -> Self {
        Self {
            kind: ModLoadWarningKind::UnavailableDirectory,
            path,
            reason: error.to_string(),
        }
    }

    fn compatibility_data(path: PathBuf, error: impl std::fmt::Display) -> Self {
        Self {
            kind: ModLoadWarningKind::CompatibilityData,
            path: Some(path),
            reason: error.to_string(),
        }
    }

    pub(crate) fn is_skipped_mod(&self) -> bool {
        self.kind == ModLoadWarningKind::SkippedMod
    }

    pub(crate) fn is_compatibility_data(&self) -> bool {
        self.kind == ModLoadWarningKind::CompatibilityData
    }
}

impl std::fmt::Display for ModLoadWarning {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.path {
            Some(path) => write!(formatter, "{}: {}", path.display(), self.reason),
            None => formatter.write_str(&self.reason),
        }
    }
}

pub(crate) struct ModLoadReport {
    pub(crate) mods: ModCollection,
    pub(crate) warnings: Vec<ModLoadWarning>,
}

#[derive(Default)]
struct ModDirectoryLoad {
    mods: Vec<RimworldMod>,
    warnings: Vec<ModLoadWarning>,
}

#[derive(Debug, Deserialize)]
struct AboutXml {
    name: Option<String>,
    description: Option<String>,

    #[serde(rename = "supportedVersions", default)]
    supported_versions: ModIdListXml,

    #[serde(rename = "loadAfter", default)]
    load_after: ModIdListXml,
    #[serde(rename = "loadAfterByVersion", default)]
    load_after_by_version: HashMap<String, ModIdListXml>,

    #[serde(rename = "loadBefore", default)]
    load_before: ModIdListXml,
    #[serde(rename = "loadBeforeByVersion", default)]
    load_before_by_version: HashMap<String, ModIdListXml>,

    #[serde(rename = "forceLoadAfter", default)]
    force_load_after: ModIdListXml,
    #[serde(rename = "forceLoadAfterByVersion", default)]
    force_load_after_by_version: HashMap<String, ModIdListXml>,

    #[serde(rename = "forceLoadBefore", default)]
    force_load_before: ModIdListXml,
    #[serde(rename = "forceLoadBeforeByVersion", default)]
    force_load_before_by_version: HashMap<String, ModIdListXml>,

    #[serde(rename = "modDependencies", default)]
    mod_dependencies: ModDependencyListXml,
    #[serde(rename = "modDependenciesByVersion", default)]
    mod_dependencies_by_version: HashMap<String, ModDependencyListXml>,

    #[serde(rename = "incompatibleWith", default)]
    incompatible_with: ModIdListXml,
    #[serde(rename = "incompatibleWithByVersion", default)]
    incompatible_with_by_version: HashMap<String, ModIdListXml>,

    #[serde(rename = "packageId")]
    package_id: String,
}

#[derive(Debug, Deserialize, Default)]
struct ModDependencyListXml {
    #[serde(rename = "li", default)]
    dependencies: Vec<ModDependencyXml>,
}

#[derive(Debug, Deserialize)]
struct ModDependencyXml {
    #[serde(rename = "packageId")]
    package_id: Option<String>,
}

#[derive(Debug, Deserialize, Default)]
struct ModIdsToFixXml {
    #[serde(rename = "li", default)]
    package_ids: Vec<String>,
}

const NO_VERSION_WARNING_PACKAGE_ID: &str = "mlie.noversionwarning";

#[derive(Debug, Deserialize)]
pub(crate) struct ModsConfigXml {
    #[serde(rename = "activeMods")]
    pub(crate) active_mods: ModIdListXml,
}

#[derive(Debug, Deserialize, Default)]
pub(crate) struct ModIdListXml {
    #[serde(rename = "li", default)]
    pub(crate) package_ids: Vec<String>,
}

fn select_versioned<'a, T>(
    by_version: &'a HashMap<String, T>,
    game_version: Option<&str>,
) -> Option<&'a T> {
    select_best_version_key(
        by_version
            .iter()
            .filter_map(|(tag, value)| Some((parse_version_tag(tag)?, value))),
        game_version,
    )
}

fn collect_unique_package_ids(
    raw_ids: impl IntoIterator<Item = String>,
    self_package_id: &PackageId,
) -> Vec<PackageId> {
    let mut seen = HashSet::new();
    let mut result = Vec::new();

    for raw_id in raw_ids {
        let Some(package_id) = PackageId::new(&raw_id) else {
            continue;
        };

        if &package_id != self_package_id && seen.insert(package_id.clone()) {
            result.push(package_id);
        }
    }

    result
}

fn merge_versioned_ids(
    base: ModIdListXml,
    by_version: &HashMap<String, ModIdListXml>,
    game_version: Option<&str>,
    self_package_id: &PackageId,
) -> Vec<PackageId> {
    let mut combined = base.package_ids;

    if let Some(versioned) = select_versioned(by_version, game_version) {
        combined.extend(versioned.package_ids.iter().cloned());
    }

    collect_unique_package_ids(combined, self_package_id)
}

fn merge_versioned_dependencies(
    base: ModDependencyListXml,
    by_version: &HashMap<String, ModDependencyListXml>,
    game_version: Option<&str>,
    self_package_id: &PackageId,
) -> Vec<PackageId> {
    let mut combined = base
        .dependencies
        .into_iter()
        .filter_map(|dependency| dependency.package_id)
        .collect::<Vec<_>>();

    if let Some(versioned) = select_versioned(by_version, game_version) {
        combined.extend(
            versioned
                .dependencies
                .iter()
                .filter_map(|dependency| dependency.package_id.clone()),
        );
    }

    collect_unique_package_ids(combined, self_package_id)
}

#[cfg(test)]
fn parse_mod(folder: &Path, root: ModRoot) -> io::Result<RimworldMod> {
    parse_mod_for_version(folder, root, None)
}

fn parse_mod_for_version(
    folder: &Path,
    root: ModRoot,
    game_version: Option<&str>,
) -> io::Result<RimworldMod> {
    let about_path = folder.join("About").join("About.xml");

    let xml = fs::read_to_string(about_path)?;
    let xml = xml.trim_start_matches('\u{feff}');

    let metadata: AboutXml = quick_xml::de::from_str(xml)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    // Official content does not include a <name> element in About.xml.
    // In that case, use the content folder's name, such as "Core" or "Anomaly".
    let fallback_name = folder
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Unknown content")
        .to_owned();

    let mod_type = detect_mod_type(folder)?;

    let source = match root {
        ModRoot::Official => ModSource::Official,

        ModRoot::Local => match git_remote_url(folder) {
            Some(remote_url) => ModSource::Git { remote_url },
            None => ModSource::Local,
        },

        ModRoot::SteamWorkshop => {
            let workshop_id = folder
                .file_name()
                .and_then(|name| name.to_str())
                .and_then(|name| name.parse::<u64>().ok())
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::InvalidData, "invalid Workshop folder ID")
                })?;

            ModSource::SteamWorkshop { workshop_id }
        }
    };

    let package_id = PackageId::new(&metadata.package_id).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            "mod contains an empty package ID",
        )
    })?;

    let loader_after = merge_versioned_ids(
        metadata.load_after,
        &metadata.load_after_by_version,
        game_version,
        &package_id,
    );
    let loader_before = merge_versioned_ids(
        metadata.load_before,
        &metadata.load_before_by_version,
        game_version,
        &package_id,
    );
    let force_load_after = merge_versioned_ids(
        metadata.force_load_after,
        &metadata.force_load_after_by_version,
        game_version,
        &package_id,
    );
    let force_load_before = merge_versioned_ids(
        metadata.force_load_before,
        &metadata.force_load_before_by_version,
        game_version,
        &package_id,
    );
    let mod_dependencies = merge_versioned_dependencies(
        metadata.mod_dependencies,
        &metadata.mod_dependencies_by_version,
        game_version,
        &package_id,
    );
    let incompatible_with = merge_versioned_ids(
        metadata.incompatible_with,
        &metadata.incompatible_with_by_version,
        game_version,
        &package_id,
    );

    let supported_versions = metadata
        .supported_versions
        .package_ids
        .into_iter()
        .map(|version| version.trim().to_owned())
        .filter(|version| !version.is_empty())
        .collect();

    Ok(RimworldMod {
        name: metadata.name.unwrap_or(fallback_name),
        description: metadata.description.unwrap_or_default(),
        supported_versions,
        community_supported_versions: Vec::new(),
        package_id,
        folder: folder.to_path_buf(),
        source,
        mod_type,
        loader_after,
        loader_before,
        force_load_after,
        force_load_before,
        mod_dependencies,
        incompatible_with,
        inferred_after: Vec::new(),
        is_patch_only: false,
    })
}

fn apply_no_version_warning_support(mods: &mut [RimworldMod]) -> Vec<ModLoadWarning> {
    let Some(no_version_warning_folder) = mods
        .iter()
        .find(|rimworld_mod| rimworld_mod.package_id.as_str() == NO_VERSION_WARNING_PACKAGE_ID)
        .map(|rimworld_mod| rimworld_mod.folder.clone())
    else {
        return Vec::new();
    };

    let entries = match fs::read_dir(&no_version_warning_folder) {
        Ok(entries) => entries,
        Err(error) => {
            return vec![ModLoadWarning::compatibility_data(
                no_version_warning_folder,
                error,
            )];
        }
    };

    let mut version_files = Vec::new();

    for entry_result in entries {
        let Ok(entry) = entry_result else {
            continue;
        };
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some(folder_name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Some(version_key) = major_minor_version(folder_name) else {
            continue;
        };
        let compatibility_path = path.join("ModIdsToFix.xml");
        if !compatibility_path.is_file() {
            continue;
        }
        let version_text = format!("{}.{}", version_key.0, version_key.1);
        version_files.push((version_key, version_text, compatibility_path));
    }

    version_files.sort_by_key(|(version_key, _, _)| *version_key);

    let mut warnings = Vec::new();

    for (_, version, compatibility_path) in version_files {
        let xml = match fs::read_to_string(&compatibility_path) {
            Ok(xml) => xml,
            Err(error) => {
                warnings.push(ModLoadWarning::compatibility_data(
                    compatibility_path,
                    error,
                ));
                continue;
            }
        };

        let compatibility_data: ModIdsToFixXml = match quick_xml::de::from_str(&xml) {
            Ok(data) => data,
            Err(error) => {
                warnings.push(ModLoadWarning::compatibility_data(
                    compatibility_path,
                    io::Error::new(io::ErrorKind::InvalidData, error),
                ));
                continue;
            }
        };

        let compatible_package_ids = compatibility_data
            .package_ids
            .iter()
            .filter_map(|raw_id| PackageId::new(raw_id))
            .collect::<HashSet<_>>();

        for rimworld_mod in mods.iter_mut() {
            let is_reported_compatible = compatible_package_ids.contains(&rimworld_mod.package_id);
            let is_officially_compatible = rimworld_mod
                .supported_versions
                .iter()
                .any(|supported_version| versions_are_compatible(supported_version, &version));

            if is_reported_compatible
                && !is_officially_compatible
                && !rimworld_mod
                    .community_supported_versions
                    .iter()
                    .any(|existing| versions_are_compatible(existing, &version))
            {
                rimworld_mod
                    .community_supported_versions
                    .push(version.clone());
            }
        }
    }

    warnings
}

fn load_directory(
    root_folder: &Path,
    root: ModRoot,
    game_version: Option<&str>,
) -> io::Result<ModDirectoryLoad> {
    let entries = fs::read_dir(root_folder).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "could not scan mod directory {}: {error}",
                root_folder.display()
            ),
        )
    })?;
    let mut load = ModDirectoryLoad::default();

    for entry_result in entries {
        let entry = match entry_result {
            Ok(entry) => entry,
            Err(error) => {
                load.warnings.push(ModLoadWarning::skipped_mod(
                    root_folder.to_path_buf(),
                    error,
                ));
                continue;
            }
        };
        let folder = entry.path();
        let file_type = match entry.file_type() {
            Ok(file_type) => file_type,
            Err(error) => {
                load.warnings
                    .push(ModLoadWarning::skipped_mod(folder, error));
                continue;
            }
        };

        if file_type.is_dir() {
            match parse_mod_for_version(&folder, root, game_version) {
                Ok(rimworld_mod) => load.mods.push(rimworld_mod),
                Err(error) => load
                    .warnings
                    .push(ModLoadWarning::skipped_mod(folder, error)),
            }
        }
    }

    Ok(load)
}

#[cfg(test)]
fn load_mods_from(root_folder: &Path, root: ModRoot) -> io::Result<ModDirectoryLoad> {
    load_mods_from_for_version(root_folder, root, None)
}

#[cfg(test)]
fn load_mods_from_for_version(
    root_folder: &Path,
    root: ModRoot,
    game_version: Option<&str>,
) -> io::Result<ModDirectoryLoad> {
    let mut load = load_directory(root_folder, root, game_version)?;
    dependency_inference::resolve_cross_mod_dependencies(&mut load.mods, game_version);
    Ok(load)
}

pub(crate) fn load_mods(
    settings: &Settings,
    game_version: Option<&str>,
) -> io::Result<ModLoadReport> {
    let official_path = settings.official_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "RimWorld installation path is not configured",
        )
    })?;

    let local_path = settings.local_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "RimWorld installation path is not configured",
        )
    })?;

    let config_path = settings.config_path().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "RimWorld configuration path is not configured",
        )
    })?;

    // Put official content first because it must load before normal mods.
    let mut official = load_directory(&official_path, ModRoot::Official, game_version)?;
    let mut local = load_directory(&local_path, ModRoot::Local, game_version)?;

    let mut mods = Vec::new();
    let mut warnings = Vec::new();

    mods.append(&mut official.mods);
    warnings.append(&mut official.warnings);
    mods.append(&mut local.mods);
    warnings.append(&mut local.warnings);

    let steamcmd_workshop_path =
        crate::services::workshop::steamcmd_workshop_path(settings.steamcmd_path());
    let workshop_paths = [settings.workshop_path(), steamcmd_workshop_path.as_deref()];
    let mut seen_workshop_paths = HashSet::new();
    let mut loaded_workshop_ids = HashSet::new();
    let mut has_workshop_path = false;

    for workshop_path in workshop_paths.into_iter().flatten() {
        if !seen_workshop_paths.insert(workshop_path.to_path_buf()) {
            continue;
        }

        has_workshop_path = true;

        match load_directory(workshop_path, ModRoot::SteamWorkshop, game_version) {
            Ok(mut workshop) => {
                workshop.mods.retain(|rimworld_mod| {
                    let ModSource::SteamWorkshop { workshop_id } = &rimworld_mod.source else {
                        return true;
                    };

                    loaded_workshop_ids.insert(*workshop_id)
                });
                mods.append(&mut workshop.mods);
                warnings.append(&mut workshop.warnings);
            }
            Err(error) => warnings.push(ModLoadWarning::unavailable_directory(
                Some(workshop_path.to_path_buf()),
                error,
            )),
        }
    }

    if !has_workshop_path {
        warnings.push(ModLoadWarning::unavailable_directory(
            None,
            "Steam Workshop directory is not configured; Workshop mods were not loaded",
        ));
    }

    // Run XML and C# assembly inference once across the full installed mod set
    // so `PatchOperationFindMod` names and `AssemblyRef` links resolve across
    // official, local, and Workshop directories.
    dependency_inference::resolve_cross_mod_dependencies(&mut mods, game_version);

    warnings.extend(apply_no_version_warning_support(&mut mods));

    let active_mod_ids = parse_config(config_path)?;
    let mut enabled_ids = Vec::new();
    let mut missing_active_package_ids = Vec::new();

    for active_mod_id in active_mod_ids {
        let matching_index = mods
            .iter()
            .position(|rimworld_mod| rimworld_mod.package_id == active_mod_id);

        match matching_index {
            Some(index) => enabled_ids.push(ModId::from_index(index)),
            None => missing_active_package_ids.push(active_mod_id),
        }
    }

    let disabled_ids = (0..mods.len())
        .map(ModId::from_index)
        .filter(|mod_id| !enabled_ids.contains(mod_id))
        .collect();

    Ok(ModLoadReport {
        mods: ModCollection::new(mods, disabled_ids, enabled_ids, missing_active_package_ids),
        warnings,
    })
}

fn is_git_worktree(folder: &Path) -> bool {
    folder.join(".git").exists()
}

/// Return the public Git remote when it can be read.
///
/// Git metadata is optional. A missing executable, missing origin, or invalid
/// command output simply makes this a normal local mod.
fn git_remote_url(folder: &Path) -> Option<String> {
    git_remote_url_with_program(folder, OsStr::new("git"))
}

fn git_remote_url_with_program(folder: &Path, program: &OsStr) -> Option<String> {
    if !is_git_worktree(folder) {
        return None;
    }

    let output = Command::new(program)
        .arg("-C")
        .arg(folder)
        .args(["remote", "get-url", "origin"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let url = String::from_utf8(output.stdout).ok()?;
    let trimmed_url = url.trim();

    (!trimmed_url.is_empty()).then(|| trimmed_url.to_owned())
}

fn detect_mod_type(mod_folder: &Path) -> io::Result<ModType> {
    let mut folders_to_check = vec![(mod_folder.to_path_buf(), false)];

    while let Some((current_folder, inside_assemblies)) = folders_to_check.pop() {
        for entry_result in fs::read_dir(current_folder)? {
            let entry = entry_result?;
            let file_type = entry.file_type()?;
            let path = entry.path();

            if file_type.is_dir() {
                let is_assemblies_folder = entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.eq_ignore_ascii_case("Assemblies"));

                folders_to_check.push((path, inside_assemblies || is_assemblies_folder));
            } else if inside_assemblies && file_type.is_file() {
                let is_dll = path
                    .extension()
                    .and_then(|extension| extension.to_str())
                    .is_some_and(|extension| extension.eq_ignore_ascii_case("dll"));

                if is_dll {
                    return Ok(ModType::CSharp);
                }
            }
        }
    }

    Ok(ModType::Xml)
}

#[cfg(test)]
#[path = "../../tests/unit/mod_loader.rs"]
mod tests;

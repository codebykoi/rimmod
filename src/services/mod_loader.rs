use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::Deserialize;

use crate::models::ModCollection;
use crate::models::ModId;
use crate::models::ModSource;
use crate::models::ModType;
use crate::models::PackageId;
use crate::models::RimworldMod;
use crate::models::major_minor_version_text;
use crate::models::versions_are_compatible;
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

#[derive(Debug)]
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
    #[serde(rename = "loadBefore", default)]
    load_before: ModIdListXml,

    #[serde(rename = "forceLoadAfter", default)]
    force_load_after: ModIdListXml,

    #[serde(rename = "forceLoadBefore", default)]
    force_load_before: ModIdListXml,

    #[serde(rename = "packageId")]
    package_id: String,
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

fn parse_package_ids(raw_ids: Vec<String>) -> io::Result<Vec<PackageId>> {
    raw_ids
        .into_iter()
        .map(|raw_id| {
            PackageId::new(&raw_id).ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "mod contains an empty dependency package ID",
                )
            })
        })
        .collect()
}

fn parse_mod(folder: &Path, root: ModRoot) -> io::Result<RimworldMod> {
    let about_path = folder.join("About").join("About.xml");

    let xml = fs::read_to_string(about_path)?;

    let metadata: AboutXml = quick_xml::de::from_str(&xml)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    // dbg!(&metadata);

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

    let mut load_after = metadata.load_after.package_ids;
    load_after.extend(metadata.force_load_after.package_ids);
    let loader_after = parse_package_ids(load_after)?;

    let mut load_before = metadata.load_before.package_ids;
    load_before.extend(metadata.force_load_before.package_ids);
    let loader_before = parse_package_ids(load_before)?;

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
    })
}

fn apply_no_version_warning_support(
    mods: &mut [RimworldMod],
    game_version: Option<&str>,
) -> io::Result<()> {
    let Some(game_version) = game_version else {
        return Ok(());
    };
    let Some(version) = major_minor_version_text(game_version) else {
        return Ok(());
    };

    let Some(no_version_warning_folder) = mods
        .iter()
        .find(|rimworld_mod| rimworld_mod.package_id.as_str() == NO_VERSION_WARNING_PACKAGE_ID)
        .map(|rimworld_mod| rimworld_mod.folder.clone())
    else {
        return Ok(());
    };

    let compatibility_path = no_version_warning_folder
        .join(&version)
        .join("ModIdsToFix.xml");

    // No data file for this RimWorld version simply means that the integration
    // has no community compatibility reports to apply.
    if !compatibility_path.is_file() {
        return Ok(());
    }

    let xml = fs::read_to_string(&compatibility_path)?;
    let compatibility_data: ModIdsToFixXml = quick_xml::de::from_str(&xml)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    let compatible_package_ids = compatibility_data
        .package_ids
        .iter()
        .filter_map(|raw_id| PackageId::new(raw_id))
        .collect::<std::collections::HashSet<_>>();

    for rimworld_mod in mods {
        let is_reported_compatible = compatible_package_ids.contains(&rimworld_mod.package_id);
        let is_officially_compatible = rimworld_mod
            .supported_versions
            .iter()
            .any(|supported_version| versions_are_compatible(supported_version, &version));

        if is_reported_compatible && !is_officially_compatible {
            rimworld_mod
                .community_supported_versions
                .push(version.clone());
        }
    }

    Ok(())
}

fn load_mods_from(root_folder: &Path, root: ModRoot) -> io::Result<ModDirectoryLoad> {
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
            match parse_mod(&folder, root) {
                Ok(rimworld_mod) => load.mods.push(rimworld_mod),
                Err(error) => load
                    .warnings
                    .push(ModLoadWarning::skipped_mod(folder, error)),
            }
        }
    }

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
    let mut official = load_mods_from(&official_path, ModRoot::Official)?;
    let mut local = load_mods_from(&local_path, ModRoot::Local)?;

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

        match load_mods_from(workshop_path, ModRoot::SteamWorkshop) {
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

    if let Err(error) = apply_no_version_warning_support(&mut mods, game_version) {
        let compatibility_path = mods
            .iter()
            .find(|rimworld_mod| rimworld_mod.package_id.as_str() == NO_VERSION_WARNING_PACKAGE_ID)
            .map(|rimworld_mod| rimworld_mod.folder.clone())
            .unwrap_or_default();

        warnings.push(ModLoadWarning::compatibility_data(
            compatibility_path,
            error,
        ));
    }

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

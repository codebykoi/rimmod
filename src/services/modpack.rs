//! Portable modpack archives (`.rimpack`) and persistent modpack instances.
//!
//! The library stores each modpack as a directory, not as an archive:
//!
//! ```text
//! modpacks/<instance>/
//!     pack.json          Pack metadata, load order, tags/colors
//!     icon.png           Optional user-chosen icon
//!     mods/<folder>/...  Fully copied local mods
//!     configs/<file>     Selected per-mod settings files
//!     saves/<file>.rws   Selected save games
//! ```
//!
//! Editing an instance only touches its own folder, which makes metadata and
//! save updates cheap. `.rimpack` files are used for importing and exporting
//! between machines. Steam Workshop mods are stored as IDs in the manifest
//! instead of files, keeping archives compact.

pub(crate) mod diff;

use std::collections::{BTreeMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufReader, BufWriter, Cursor, Read, Seek, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::services::organization::{MAX_TAG_CHARS, ModColor};

pub(crate) use self::diff::{
    DiffMod, PackDiff, PackStatus, compute_pack_diff, compute_pack_status,
    format_mod_list_for_share,
};

pub(crate) const PACK_EXTENSION: &str = "rimpack";
pub(crate) const PACK_FORMAT: &str = "rimmod.modpack";
pub(crate) const PACK_FORMAT_VERSION: u32 = 2;

pub(crate) const MAX_PACK_NAME_CHARS: usize = 80;
pub(crate) const MAX_PACK_DESCRIPTION_CHARS: usize = 8000;
pub(crate) const MAX_PACK_TAGS: usize = 20;
const MAX_ICON_DIMENSION: u32 = 512;

const ARCHIVE_MANIFEST_NAME: &str = "manifest.json";
const INSTANCE_MANIFEST_NAME: &str = "pack.json";
const ICON_NAME: &str = "icon.png";
const MODS_PREFIX: &str = "mods";
const CONFIGS_PREFIX: &str = "configs";
const SAVES_PREFIX: &str = "saves";

const SKIPPED_DIRECTORIES: &[&str] = &[".git", ".hg", ".svn", ".vs", ".idea"];

/// Progress callback where `0.0` is the start and `1.0` the end.
pub(crate) type ProgressCallback<'a> = &'a mut dyn FnMut(f32);

/// Everything a pack remembers besides the archived files.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ModpackManifest {
    pub format: String,
    pub format_version: u32,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub game_version: Option<String>,
    #[serde(default)]
    pub created_with: String,
    #[serde(default)]
    pub created_at: u64,
    #[serde(default)]
    pub mods: Vec<PackMod>,
    #[serde(default)]
    pub organization: BTreeMap<String, PackOrganization>,
    #[serde(default)]
    pub configs: Vec<String>,
    /// File names of the saves included in the pack, stored under `saves/`.
    #[serde(default)]
    pub saves: Vec<String>,
}

impl ModpackManifest {
    pub(crate) fn new(name: impl Into<String>) -> Self {
        Self {
            format: PACK_FORMAT.to_owned(),
            format_version: PACK_FORMAT_VERSION,
            name: name.into(),
            description: String::new(),
            tags: Vec::new(),
            game_version: None,
            created_with: concat!("RimMod ", env!("CARGO_PKG_VERSION")).to_owned(),
            created_at: unix_now(),
            mods: Vec::new(),
            organization: BTreeMap::new(),
            configs: Vec::new(),
            saves: Vec::new(),
        }
    }
}

/// One mod referenced by a pack. Local mods also have their files inside the
/// instance or archive; Workshop and official mods only keep enough data to
/// find them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PackMod {
    pub package_id: String,
    pub name: String,
    #[serde(default)]
    pub folder_name: String,
    #[serde(flatten)]
    pub source: PackSource,
}

impl PackMod {
    pub(crate) fn is_local(&self) -> bool {
        matches!(self.source, PackSource::Local { .. })
    }

    pub(crate) fn workshop_id(&self) -> Option<u64> {
        match self.source {
            PackSource::Workshop { workshop_id } => Some(workshop_id),
            _ => None,
        }
    }

    pub(crate) fn workshop_url(&self) -> Option<String> {
        self.workshop_id().map(|workshop_id| {
            format!("https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}")
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", rename_all = "snake_case")]
pub(crate) enum PackSource {
    Official,
    Workshop {
        workshop_id: u64,
    },
    Local {
        #[serde(default)]
        remote_url: Option<String>,
        /// Archive-relative path, always `mods/<folder_name>`.
        path: String,
    },
}

/// User tags and color saved for one package ID.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PackOrganization {
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub color: Option<ModColor>,
}

pub(crate) struct LoadedPack {
    pub(crate) manifest: ModpackManifest,
    pub(crate) icon: Option<Arc<[u8]>>,
}

pub(crate) struct PackSummary {
    pub(crate) path: PathBuf,
    pub(crate) manifest: ModpackManifest,
    pub(crate) icon: Option<Arc<[u8]>>,
    pub(crate) size_bytes: u64,
}

/// Options controlling which non-mod files are copied when applying a pack.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct PackApplyOptions {
    pub(crate) restore_configs: bool,
    pub(crate) restore_saves: bool,
    pub(crate) overwrite_saves: bool,
}

impl Default for PackApplyOptions {
    fn default() -> Self {
        Self {
            restore_configs: true,
            restore_saves: true,
            overwrite_saves: false,
        }
    }
}

/// Files selected for one save/update operation.
pub(crate) struct PackWriteRequest<'a> {
    pub(crate) manifest: &'a ModpackManifest,
    pub(crate) icon: Option<&'a [u8]>,
    /// `(folder name inside the pack, source folder on disk)`.
    pub(crate) local_mods: &'a [(String, PathBuf)],
    /// `(relative path under the configs folder, source file on disk)`.
    pub(crate) configs: &'a [(String, PathBuf)],
    /// `(save file name, source file on disk)`.
    pub(crate) saves: &'a [(String, PathBuf)],
}

/// Result of copying an instance into the game's folders.
pub(crate) struct AppliedPack {
    pub(crate) manifest: ModpackManifest,
    pub(crate) installed_local_mods: Vec<String>,
    pub(crate) skipped_local_mods: Vec<String>,
    pub(crate) restored_configs: Vec<String>,
    pub(crate) restored_saves: Vec<String>,
    pub(crate) skipped_saves: Vec<String>,
}

/// Directory holding all modpack instances.
pub(crate) fn library_directory() -> Option<PathBuf> {
    crate::services::data_directory().map(|directory| directory.join("modpacks"))
}

pub(crate) fn is_pack_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(PACK_EXTENSION))
}

pub(crate) fn is_supported_icon_path(path: &Path) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| {
            matches!(
                extension.to_ascii_lowercase().as_str(),
                "png" | "jpg" | "jpeg" | "gif"
            )
        })
}

/// Read every instance in the library. Legacy loose `.rimpack` files are
/// migrated into instances on first sight so the library has one shape.
pub(crate) fn list_library() -> io::Result<Vec<PackSummary>> {
    let Some(directory) = library_directory() else {
        return Ok(Vec::new());
    };

    let entries = match fs::read_dir(&directory) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };

    let mut instance_paths = Vec::new();
    let mut legacy_archives = Vec::new();

    for entry in entries {
        let Ok(entry) = entry else {
            continue;
        };
        let path = entry.path();

        if path.is_dir() {
            instance_paths.push(path);
        } else if path.is_file() && is_pack_path(&path) {
            legacy_archives.push(path);
        }
    }

    for archive in legacy_archives {
        let Ok(manifest) = read_archive_manifest(&archive) else {
            continue;
        };
        let Ok(destination) = unique_instance_path(&manifest.name) else {
            continue;
        };

        if import_archive(&archive, &destination, &mut |_| {}).is_ok() {
            // Migration succeeded; the instance is now the source of truth.
            let _ = fs::remove_file(&archive);
            instance_paths.push(destination);
        }
    }

    let mut packs = Vec::new();

    for path in instance_paths {
        let Ok(pack) = read_instance(&path) else {
            continue;
        };
        let size_bytes = directory_size(&path);

        packs.push(PackSummary {
            path,
            manifest: pack.manifest,
            icon: pack.icon,
            size_bytes,
        });
    }

    packs.sort_by(|left, right| {
        left.manifest
            .name
            .to_lowercase()
            .cmp(&right.manifest.name.to_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });

    Ok(packs)
}

pub(crate) fn read_instance(path: &Path) -> io::Result<LoadedPack> {
    let manifest_path = path.join(INSTANCE_MANIFEST_NAME);
    let contents = fs::read_to_string(&manifest_path)
        .map_err(|error| path_error("could not read modpack manifest", &manifest_path, error))?;

    let manifest = serde_json::from_str::<ModpackManifest>(&contents).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} has an invalid manifest: {error}",
                manifest_path.display()
            ),
        )
    })?;

    validate_manifest(&manifest, &manifest_path)?;

    let icon_path = path.join(ICON_NAME);
    let icon = if icon_path.is_file() {
        Some(Arc::from(fs::read(&icon_path).map_err(|error| {
            path_error("could not read modpack icon", &icon_path, error)
        })?))
    } else {
        None
    };

    Ok(LoadedPack { manifest, icon })
}

pub(crate) fn read_instance_summary(path: &Path) -> io::Result<PackSummary> {
    let loaded = read_instance(path)?;
    let size_bytes = directory_size(path);

    Ok(PackSummary {
        path: path.to_path_buf(),
        manifest: loaded.manifest,
        icon: loaded.icon,
        size_bytes,
    })
}

pub(crate) fn read_archive_manifest(path: &Path) -> io::Result<ModpackManifest> {
    let file =
        File::open(path).map_err(|error| path_error("could not open modpack", path, error))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|error| zip_error("could not read modpack", path, error))?;

    read_manifest_from_archive(&mut archive, path)
}

/// Read both manifest and optional icon from a `.rimpack` archive without
/// extracting its contents, for drag-and-drop and import previews.
pub(crate) fn read_archive_preview(path: &Path) -> io::Result<LoadedPack> {
    let file =
        File::open(path).map_err(|error| path_error("could not open modpack", path, error))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|error| zip_error("could not read modpack", path, error))?;

    let manifest = read_manifest_from_archive(&mut archive, path)?;
    let icon = match archive.by_name(ICON_NAME) {
        Ok(mut entry) => {
            let mut bytes = Vec::new();
            entry.read_to_end(&mut bytes)?;
            Some(Arc::from(bytes))
        }
        Err(_) => None,
    };

    Ok(LoadedPack { manifest, icon })
}

/// Local mod folders currently bundled inside an existing library instance.
pub(crate) fn instance_local_mods(
    instance: &Path,
    manifest: &ModpackManifest,
) -> Vec<(String, PathBuf)> {
    manifest
        .mods
        .iter()
        .filter(|pack_mod| pack_mod.is_local())
        .filter_map(|pack_mod| {
            let folder_name = sanitize_component(&pack_mod.folder_name);
            let source = instance.join(MODS_PREFIX).join(&folder_name);
            source.is_dir().then_some((folder_name, source))
        })
        .collect()
}

/// Mod settings files currently bundled inside an existing library instance.
pub(crate) fn instance_configs(
    instance: &Path,
    manifest: &ModpackManifest,
) -> Vec<(String, PathBuf)> {
    manifest
        .configs
        .iter()
        .filter_map(|relative_path| {
            let safe_path = safe_relative_path(relative_path)?;
            let source = instance.join(CONFIGS_PREFIX).join(safe_path);
            source.is_file().then_some((relative_path.clone(), source))
        })
        .collect()
}

/// Save files currently bundled inside an existing library instance.
pub(crate) fn instance_saves(
    instance: &Path,
    manifest: &ModpackManifest,
) -> Vec<(String, PathBuf)> {
    manifest
        .saves
        .iter()
        .filter_map(|file_name| {
            let safe_name = safe_file_name(file_name)?;
            let source = instance.join(SAVES_PREFIX).join(&safe_name);
            source.is_file().then_some((safe_name, source))
        })
        .collect()
}

/// Duplicate an existing instance folder under a new name.
pub(crate) fn duplicate_instance(
    source: &Path,
    destination: &Path,
    new_name: &str,
) -> io::Result<PackSummary> {
    let loaded = read_instance(source)?;
    let mut manifest = loaded.manifest;
    manifest.name = new_name
        .trim()
        .chars()
        .take(MAX_PACK_NAME_CHARS)
        .collect::<String>();
    manifest.created_at = unix_now();

    let local_mods = instance_local_mods(source, &manifest);
    let configs = instance_configs(source, &manifest);
    let saves = instance_saves(source, &manifest);

    write_instance(
        destination,
        PackWriteRequest {
            manifest: &manifest,
            icon: loaded.icon.as_deref(),
            local_mods: &local_mods,
            configs: &configs,
            saves: &saves,
        },
        &mut |_| {},
    )?;

    read_instance_summary(destination)
}

/// Total size in bytes of all files stored inside an instance directory.
pub(crate) fn directory_size(root: &Path) -> u64 {
    let mut total = 0u64;
    let mut pending = vec![root.to_path_buf()];

    while let Some(folder) = pending.pop() {
        let Ok(entries) = fs::read_dir(&folder) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_symlink() {
                continue;
            }
            if file_type.is_dir() {
                pending.push(entry.path());
            } else if file_type.is_file()
                && let Ok(metadata) = entry.metadata()
            {
                total = total.saturating_add(metadata.len());
            }
        }
    }

    total
}

fn read_manifest_from_archive<R: Read + Seek>(
    archive: &mut ZipArchive<R>,
    path: &Path,
) -> io::Result<ModpackManifest> {
    let manifest = {
        let mut entry = archive
            .by_name(ARCHIVE_MANIFEST_NAME)
            .map_err(|error| zip_error("modpack has no manifest", path, error))?;
        let mut text = String::new();
        entry.read_to_string(&mut text)?;

        serde_json::from_str::<ModpackManifest>(&text).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("{} has an invalid manifest: {error}", path.display()),
            )
        })?
    };

    validate_manifest(&manifest, path)?;

    Ok(manifest)
}

fn validate_manifest(manifest: &ModpackManifest, path: &Path) -> io::Result<()> {
    if manifest.format != PACK_FORMAT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("{} is not a RimMod modpack", path.display()),
        ));
    }

    if manifest.format_version > PACK_FORMAT_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "{} was created by a newer RimMod and cannot be opened",
                path.display()
            ),
        ));
    }

    Ok(())
}

/// Pick a free directory name in the library.
pub(crate) fn unique_instance_path(name: &str) -> io::Result<PathBuf> {
    let directory = library_directory().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "the RimMod data folder is not available on this system",
        )
    })?;

    fs::create_dir_all(&directory)
        .map_err(|error| path_error("could not create modpack library", &directory, error))?;

    let stem = sanitize_file_stem(name);
    let mut candidate = directory.join(&stem);
    let mut number = 2;

    while candidate.exists() {
        candidate = directory.join(format!("{stem} ({number})"));
        number += 1;
    }

    Ok(candidate)
}

/// Write an instance directory. The new contents are staged in a sibling
/// directory and swapped in only after every file has been copied.
pub(crate) fn write_instance(
    path: &Path,
    request: PackWriteRequest<'_>,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| path_error("could not create modpack folder", parent, error))?;

    let temporary = temporary_directory(path);
    let result = write_instance_directory(&temporary, &request, progress);

    if let Err(error) = result {
        let _ = fs::remove_dir_all(&temporary);
        return Err(error);
    }

    replace_directory(&temporary, path)
}

fn write_instance_directory(
    path: &Path,
    request: &PackWriteRequest<'_>,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    fs::create_dir_all(path)
        .map_err(|error| path_error("could not create temporary modpack", path, error))?;

    let mut manifest = request.manifest.clone();
    manifest.configs = request
        .configs
        .iter()
        .map(|(relative_path, _)| relative_path.clone())
        .collect();
    manifest.saves = request
        .saves
        .iter()
        .map(|(file_name, _)| file_name.clone())
        .collect();

    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("could not serialize modpack manifest: {error}"),
        )
    })?;

    let plan = build_instance_plan(request)?;
    let total = plan.len() + 1 + usize::from(request.icon.is_some());
    let mut done = 0;

    fs::write(path.join(INSTANCE_MANIFEST_NAME), manifest_json).map_err(|error| {
        path_error(
            "could not write modpack manifest",
            &path.join(INSTANCE_MANIFEST_NAME),
            error,
        )
    })?;
    done += 1;
    progress(fraction(done, total));

    if let Some(icon) = request.icon {
        fs::write(path.join(ICON_NAME), icon).map_err(|error| {
            path_error("could not write modpack icon", &path.join(ICON_NAME), error)
        })?;
        done += 1;
        progress(fraction(done, total));
    }

    for (relative_path, source) in plan {
        let target = path.join(&relative_path);

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| path_error("could not create pack folder", parent, error))?;
        }

        fs::copy(&source, &target)
            .map_err(|error| path_error("could not copy into modpack", &source, error))?;
        done += 1;
        progress(fraction(done, total));
    }

    Ok(())
}

/// Export an instance as a shareable `.rimpack` archive.
pub(crate) fn export_instance(
    instance: &Path,
    destination: &Path,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    let loaded = read_instance(instance)?;
    let manifest = loaded.manifest;

    let local_mods = manifest
        .mods
        .iter()
        .filter(|pack_mod| pack_mod.is_local())
        .map(|pack_mod| {
            (
                pack_mod.folder_name.clone(),
                instance
                    .join(MODS_PREFIX)
                    .join(sanitize_component(&pack_mod.folder_name)),
            )
        })
        .collect::<Vec<_>>();

    let configs = manifest
        .configs
        .iter()
        .filter_map(|relative_path| {
            safe_relative_path(relative_path).map(|safe_path| {
                (
                    relative_path.clone(),
                    instance.join(CONFIGS_PREFIX).join(safe_path),
                )
            })
        })
        .collect::<Vec<_>>();

    let saves = manifest
        .saves
        .iter()
        .filter_map(|file_name| {
            safe_file_name(file_name).map(|safe_name| {
                (
                    safe_name.clone(),
                    instance.join(SAVES_PREFIX).join(safe_name),
                )
            })
        })
        .collect::<Vec<_>>();

    write_archive(
        destination,
        PackWriteRequest {
            manifest: &manifest,
            icon: loaded.icon.as_deref(),
            local_mods: &local_mods,
            configs: &configs,
            saves: &saves,
        },
        progress,
    )
}

/// Import an archive into a new instance directory.
pub(crate) fn import_archive(
    archive_path: &Path,
    destination: &Path,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    if destination.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            format!("{} already exists", destination.display()),
        ));
    }

    let result = import_archive_inner(archive_path, destination, progress);

    if result.is_err() {
        let _ = fs::remove_dir_all(destination);
    }

    result
}

fn import_archive_inner(
    archive_path: &Path,
    destination: &Path,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    let file = File::open(archive_path)
        .map_err(|error| path_error("could not open modpack", archive_path, error))?;
    let mut archive = ZipArchive::new(BufReader::new(file))
        .map_err(|error| zip_error("could not read modpack", archive_path, error))?;

    let manifest = read_manifest_from_archive(&mut archive, archive_path)?;
    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("could not serialize modpack manifest: {error}"),
        )
    })?;

    fs::create_dir_all(destination)
        .map_err(|error| path_error("could not create modpack instance", destination, error))?;

    let manifest_path = destination.join(INSTANCE_MANIFEST_NAME);
    fs::write(&manifest_path, manifest_json)
        .map_err(|error| path_error("could not write modpack manifest", &manifest_path, error))?;

    if let Ok(mut icon) = archive.by_name(ICON_NAME) {
        let icon_path = destination.join(ICON_NAME);
        let mut output = File::create(&icon_path)
            .map_err(|error| path_error("could not write modpack icon", &icon_path, error))?;
        io::copy(&mut icon, &mut output)?;
    }

    let file_names = archive
        .file_names()
        .filter_map(|name| name.ok().map(|name| name.into_owned()))
        .collect::<Vec<_>>();
    let total = file_names.len();
    let mut done = 0;

    for name in file_names {
        let Some(relative_path) = safe_zip_path(&name) else {
            continue;
        };

        let first = relative_path.components().next();
        let is_pack_content = matches!(
            first,
            Some(Component::Normal(prefix))
                if prefix == MODS_PREFIX || prefix == CONFIGS_PREFIX || prefix == SAVES_PREFIX
        );

        if !is_pack_content {
            done += 1;
            progress(fraction(done, total));
            continue;
        }

        let target = destination.join(&relative_path);

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| path_error("could not create pack folder", parent, error))?;
        }

        let mut entry = match archive.by_name(&name) {
            Ok(entry) => entry,
            Err(_) => {
                done += 1;
                progress(fraction(done, total));
                continue;
            }
        };

        if entry.is_dir() {
            fs::create_dir_all(&target)
                .map_err(|error| path_error("could not create pack folder", &target, error))?;
        } else {
            let mut output = File::create(&target)
                .map_err(|error| path_error("could not extract modpack file", &target, error))?;
            io::copy(&mut entry, &mut output)?;
        }

        done += 1;
        progress(fraction(done, total));
    }

    progress(1.0);

    Ok(())
}

/// Copy an instance's selected files into RimWorld's folders.
#[cfg(test)]
pub(crate) fn apply_instance(
    instance: &Path,
    mods_root: &Path,
    config_root: &Path,
    saves_root: &Path,
    overwrite_saves: bool,
    progress: ProgressCallback<'_>,
) -> io::Result<AppliedPack> {
    apply_instance_with_options(
        instance,
        mods_root,
        config_root,
        saves_root,
        PackApplyOptions {
            restore_configs: true,
            restore_saves: true,
            overwrite_saves,
        },
        progress,
    )
}

/// Copy an instance's files into RimWorld's folders according to `options`.
pub(crate) fn apply_instance_with_options(
    instance: &Path,
    mods_root: &Path,
    config_root: &Path,
    saves_root: &Path,
    options: PackApplyOptions,
    progress: ProgressCallback<'_>,
) -> io::Result<AppliedPack> {
    let loaded = read_instance(instance)?;
    let manifest = loaded.manifest;

    let mut local_files = Vec::new();
    let mut config_files = Vec::new();
    let mut save_files = Vec::new();
    let mut skipped_local_mods = Vec::new();
    let mut skipped_saves = HashSet::new();
    let mut total = 0;

    for pack_mod in &manifest.mods {
        let PackSource::Local { .. } = &pack_mod.source else {
            continue;
        };

        let folder_name = sanitize_component(&pack_mod.folder_name);
        let source = instance.join(MODS_PREFIX).join(&folder_name);

        if !source.is_dir() {
            continue;
        }

        if mods_root.join(&folder_name).exists() {
            skipped_local_mods.push(folder_name);
            continue;
        }

        for relative_path in collect_mod_files(&source)? {
            total += 1;
            local_files.push((
                folder_name.clone(),
                source.join(&relative_path),
                mods_root.join(&folder_name).join(&relative_path),
            ));
        }
    }

    if options.restore_configs {
        for relative_path in &manifest.configs {
            let Some(safe_path) = safe_relative_path(relative_path) else {
                continue;
            };
            let source = instance.join(CONFIGS_PREFIX).join(&safe_path);

            if source.is_file() {
                total += 1;
                config_files.push((relative_path.clone(), source, config_root.join(&safe_path)));
            }
        }
    }

    if options.restore_saves {
        for file_name in &manifest.saves {
            let Some(file_name) = safe_file_name(file_name) else {
                continue;
            };
            let source = instance.join(SAVES_PREFIX).join(&file_name);

            if !source.is_file() {
                continue;
            }

            let target = saves_root.join(&file_name);

            if target.exists() && !options.overwrite_saves {
                skipped_saves.insert(file_name);
                continue;
            }

            total += 1;
            save_files.push((file_name, source, target));
        }
    }

    let mut done = 0;
    let mut installed_local_mods = HashSet::new();
    let mut restored_configs = HashSet::new();
    let mut restored_saves = HashSet::new();

    for (folder_name, source, target) in local_files {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| path_error("could not create mod folder", parent, error))?;
        }

        fs::copy(&source, &target)
            .map_err(|error| path_error("could not copy mod file", &source, error))?;
        installed_local_mods.insert(folder_name);
        done += 1;
        progress(fraction(done, total));
    }

    for (relative_path, source, target) in config_files {
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| path_error("could not create settings folder", parent, error))?;
        }

        fs::copy(&source, &target)
            .map_err(|error| path_error("could not restore settings", &source, error))?;
        restored_configs.insert(relative_path);
        done += 1;
        progress(fraction(done, total));
    }

    if !save_files.is_empty() {
        fs::create_dir_all(saves_root)
            .map_err(|error| path_error("could not create the Saves folder", saves_root, error))?;
    }

    for (file_name, source, target) in save_files {
        fs::copy(&source, &target)
            .map_err(|error| path_error("could not restore save", &source, error))?;
        restored_saves.insert(file_name);
        done += 1;
        progress(fraction(done, total));
    }

    progress(1.0);

    let mut installed_local_mods = installed_local_mods.into_iter().collect::<Vec<_>>();
    let mut restored_configs = restored_configs.into_iter().collect::<Vec<_>>();
    let mut restored_saves = restored_saves.into_iter().collect::<Vec<_>>();
    let mut skipped_saves = skipped_saves.into_iter().collect::<Vec<_>>();

    installed_local_mods.sort();
    skipped_local_mods.sort();
    restored_configs.sort();
    restored_saves.sort();
    skipped_saves.sort();

    Ok(AppliedPack {
        manifest,
        installed_local_mods,
        skipped_local_mods,
        restored_configs,
        restored_saves,
        skipped_saves,
    })
}

/// Settings files that belong to mods rather than the whole game.
pub(crate) fn discover_config_files(config_root: &Path) -> Vec<(String, PathBuf)> {
    let mut files = Vec::new();

    let Ok(entries) = fs::read_dir(config_root) else {
        return files;
    };

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let lower = name.to_ascii_lowercase();

        let is_mod_settings =
            (lower.starts_with("mod_") || lower == "modsettings.xml") && lower.ends_with(".xml");

        if is_mod_settings {
            files.push((name.to_owned(), path));
        }
    }

    let hugslib_settings = config_root.join("HugsLib").join("ModSettings.xml");
    if hugslib_settings.is_file() {
        files.push(("HugsLib/ModSettings.xml".to_owned(), hugslib_settings));
    }

    files.sort_by_key(|(relative_path, _)| relative_path.to_lowercase());

    files
}

/// Save games in `<data directory>/Saves`. Returns flat file names because
/// RimWorld does not use subfolders for saves.
pub(crate) fn discover_save_files(data_directory: &Path) -> Vec<(String, PathBuf)> {
    let saves_root = data_directory.join("Saves");
    let Ok(entries) = fs::read_dir(&saves_root) else {
        return Vec::new();
    };

    let mut saves = Vec::new();

    for entry in entries.flatten() {
        let path = entry.path();

        if !path.is_file() {
            continue;
        }

        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let is_save = path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| extension.eq_ignore_ascii_case("rws"));

        if is_save && safe_file_name(name).is_some() {
            saves.push((name.to_owned(), path));
        }
    }

    saves.sort_by_key(|(file_name, _)| file_name.to_lowercase());

    saves
}

fn build_instance_plan(request: &PackWriteRequest<'_>) -> io::Result<Vec<(PathBuf, PathBuf)>> {
    let mut plan = Vec::new();

    for (folder_name, folder) in request.local_mods {
        let folder_name = sanitize_component(folder_name);

        if folder_name.is_empty() {
            continue;
        }

        for relative_path in collect_mod_files(folder)? {
            plan.push((
                Path::new(MODS_PREFIX)
                    .join(&folder_name)
                    .join(&relative_path),
                folder.join(&relative_path),
            ));
        }
    }

    for (relative_path, source) in request.configs {
        let safe_path = safe_relative_path(relative_path).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid settings path: {relative_path}"),
            )
        })?;
        plan.push((Path::new(CONFIGS_PREFIX).join(safe_path), source.clone()));
    }

    for (file_name, source) in request.saves {
        let file_name = safe_file_name(file_name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("invalid save file name: {file_name}"),
            )
        })?;
        plan.push((Path::new(SAVES_PREFIX).join(file_name), source.clone()));
    }

    Ok(plan)
}

fn write_archive(
    path: &Path,
    request: PackWriteRequest<'_>,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    fs::create_dir_all(parent)
        .map_err(|error| path_error("could not create modpack folder", parent, error))?;

    let temporary = temporary_file(path);
    let result = write_archive_file(&temporary, &request, progress);

    if let Err(error) = result {
        let _ = fs::remove_file(&temporary);
        return Err(error);
    }

    if let Err(error) = fs::rename(&temporary, path) {
        let _ = fs::remove_file(&temporary);
        return Err(path_error("could not replace modpack", path, error));
    }

    Ok(())
}

fn write_archive_file(
    path: &Path,
    request: &PackWriteRequest<'_>,
    progress: ProgressCallback<'_>,
) -> io::Result<()> {
    let file =
        File::create(path).map_err(|error| path_error("could not create modpack", path, error))?;
    let mut archive = ZipWriter::new(BufWriter::new(file));
    let options = SimpleFileOptions::default()
        .compression_method(CompressionMethod::Deflated)
        .large_file(true);

    let mut manifest = request.manifest.clone();
    manifest.configs = request
        .configs
        .iter()
        .map(|(relative_path, _)| relative_path.clone())
        .collect();
    manifest.saves = request
        .saves
        .iter()
        .map(|(file_name, _)| file_name.clone())
        .collect();

    let manifest_json = serde_json::to_vec_pretty(&manifest).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("could not serialize modpack manifest: {error}"),
        )
    })?;

    let plan = build_instance_plan(request)?;
    let total = plan.len() + 1 + usize::from(request.icon.is_some());
    let mut done = 0;

    archive
        .start_file(ARCHIVE_MANIFEST_NAME, options)
        .map_err(|error| zip_error("could not write manifest", path, error))?;
    archive.write_all(&manifest_json)?;
    done += 1;
    progress(fraction(done, total));

    if let Some(icon) = request.icon {
        archive
            .start_file(ICON_NAME, options)
            .map_err(|error| zip_error("could not write icon", path, error))?;
        archive.write_all(icon)?;
        done += 1;
        progress(fraction(done, total));
    }

    for (relative_path, source) in plan {
        let entry_name = relative_path_to_zip(&relative_path);

        archive
            .start_file(entry_name, options)
            .map_err(|error| zip_error("could not write modpack file", path, error))?;

        let mut source = File::open(&source)
            .map_err(|error| path_error("could not read modpack file", &source, error))?;
        io::copy(&mut source, &mut archive)?;
        done += 1;
        progress(fraction(done, total));
    }

    let mut writer = archive
        .finish()
        .map_err(|error| zip_error("could not finish modpack", path, error))?;
    writer.flush()?;
    writer.get_ref().sync_all()?;

    Ok(())
}

fn collect_mod_files(root: &Path) -> io::Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), PathBuf::new())];

    while let Some((folder, relative)) = pending.pop() {
        for entry in fs::read_dir(&folder)
            .map_err(|error| path_error("could not read mod folder", &folder, error))?
        {
            let entry = entry
                .map_err(|error| path_error("could not read mod folder entry", &folder, error))?;
            let file_type = entry
                .file_type()
                .map_err(|error| path_error("could not inspect mod file", &entry.path(), error))?;

            // Symbolic links could point outside the mod and make the pack
            // much larger than expected. The real files are already included.
            if file_type.is_symlink() {
                continue;
            }

            let name = entry.file_name();
            let entry_relative = relative.join(&name);

            if file_type.is_dir() {
                let is_skipped = name
                    .to_str()
                    .is_some_and(|name| SKIPPED_DIRECTORIES.contains(&name));

                if !is_skipped {
                    pending.push((entry.path(), entry_relative));
                }
            } else if file_type.is_file() {
                files.push(entry_relative);
            }
        }
    }

    files.sort();

    Ok(files)
}

fn replace_directory(temporary: &Path, destination: &Path) -> io::Result<()> {
    if !destination.exists() {
        return fs::rename(temporary, destination)
            .map_err(|error| path_error("could not install modpack", destination, error));
    }

    let old = temporary_directory(destination);

    fs::rename(destination, &old)
        .map_err(|error| path_error("could not replace modpack", destination, error))?;

    match fs::rename(temporary, destination) {
        Ok(()) => {
            let _ = fs::remove_dir_all(&old);
            Ok(())
        }
        Err(error) => {
            let _ = fs::rename(&old, destination);
            let _ = fs::remove_dir_all(temporary);
            Err(path_error("could not replace modpack", destination, error))
        }
    }
}

fn fraction(done: usize, total: usize) -> f32 {
    if total == 0 {
        1.0
    } else {
        (done as f32 / total as f32).clamp(0.0, 1.0)
    }
}

/// Validate a ZIP entry name and turn it into a relative, traversal-free path.
fn safe_zip_path(name: &str) -> Option<PathBuf> {
    let mut path = PathBuf::new();

    for component in Path::new(name).components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let part = part.to_str()?;

                // A backslash or colon would be interpreted differently on
                // Windows than it was inside the archive.
                if part.contains('\\') || part.contains(':') {
                    return None;
                }

                path.push(part);
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (!path.as_os_str().is_empty()).then_some(path)
}

/// Validate a relative path that will be joined under a known root.
fn safe_relative_path(raw: &str) -> Option<PathBuf> {
    let path = Path::new(raw);
    let mut safe = PathBuf::new();

    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::Normal(part) => {
                let part = part.to_str()?;

                if part.contains('\\') || part.contains(':') {
                    return None;
                }

                safe.push(part);
            }
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    (!safe.as_os_str().is_empty()).then_some(safe)
}

/// Validate a single file name that will be written directly into the game's
/// Saves folder.
fn safe_file_name(raw: &str) -> Option<String> {
    let path = Path::new(raw);
    let mut components = path.components();
    let Some(Component::Normal(part)) = components.next() else {
        return None;
    };

    if components.next().is_some() {
        return None;
    }

    let part = part.to_str()?;

    if part.is_empty() || part == "." || part == ".." || part.contains('\\') || part.contains(':') {
        return None;
    }

    Some(part.to_owned())
}

fn relative_path_to_zip(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(part) => Some(part.to_string_lossy().into_owned()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn temporary_file(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());

    path.with_extension(format!(
        "{PACK_EXTENSION}.{}.{nanos}.tmp",
        std::process::id()
    ))
}

fn temporary_directory(path: &Path) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("modpack");

    path.with_file_name(format!(".{name}.{}.{nanos}.tmp", std::process::id()))
}

/// A path-safe folder or archive component, preserving readable names.
pub(crate) fn sanitize_component(raw: &str) -> String {
    let cleaned = raw
        .chars()
        .map(|character| {
            if character.is_alphanumeric()
                || matches!(character, ' ' | '-' | '_' | '.' | '(' | ')' | '[' | ']')
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();

    // Leading dots make hidden folders and can produce `.` or `..`.
    let cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches([' ', '.']);

    if cleaned.is_empty() {
        return "mod".to_owned();
    }

    let mut limited = cleaned.chars().take(120).collect::<String>();

    if is_reserved_windows_name(&limited) {
        limited.insert(0, '_');
    }

    limited
}

fn is_reserved_windows_name(name: &str) -> bool {
    let stem = name
        .split('.')
        .next()
        .unwrap_or(name)
        .trim()
        .to_ascii_uppercase();

    matches!(
        stem.as_str(),
        "CON"
            | "PRN"
            | "AUX"
            | "NUL"
            | "COM1"
            | "COM2"
            | "COM3"
            | "COM4"
            | "COM5"
            | "COM6"
            | "COM7"
            | "COM8"
            | "COM9"
            | "LPT1"
            | "LPT2"
            | "LPT3"
            | "LPT4"
            | "LPT5"
            | "LPT6"
            | "LPT7"
            | "LPT8"
            | "LPT9"
    )
}

/// Path-safe file name stem for an instance directory.
pub(crate) fn sanitize_file_stem(raw: &str) -> String {
    let cleaned = raw
        .chars()
        .map(|character| {
            if character.is_alphanumeric()
                || matches!(character, ' ' | '-' | '_' | '(' | ')' | '[' | ']')
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>();

    let cleaned = cleaned
        .trim()
        .trim_start_matches('.')
        .trim_end_matches([' ', '.']);

    if cleaned.is_empty() {
        return "modpack".to_owned();
    }

    let mut limited = cleaned
        .chars()
        .take(MAX_PACK_NAME_CHARS)
        .collect::<String>();

    if is_reserved_windows_name(&limited) {
        limited.insert(0, '_');
    }

    limited
}

/// Decode a user-selected image, cap its size and re-encode it as PNG so every
/// pack carries a format the app can always display.
pub(crate) fn normalize_icon(bytes: &[u8]) -> io::Result<Vec<u8>> {
    let image = image::load_from_memory(bytes)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let thumbnail = image.thumbnail(MAX_ICON_DIMENSION, MAX_ICON_DIMENSION);
    let mut encoded = Vec::new();

    thumbnail
        .write_to(&mut Cursor::new(&mut encoded), image::ImageFormat::Png)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    Ok(encoded)
}

/// Split a comma- or newline-separated tag field into normalized, unique tags.
pub(crate) fn normalize_tags(input: &str) -> Vec<String> {
    let mut tags = Vec::new();

    for raw in input.split([',', '\n']) {
        let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");

        if collapsed.is_empty() {
            continue;
        }

        let tag = collapsed.chars().take(MAX_TAG_CHARS).collect::<String>();

        if tags
            .iter()
            .any(|existing: &String| existing.eq_ignore_ascii_case(&tag))
        {
            continue;
        }

        tags.push(tag);

        if tags.len() >= MAX_PACK_TAGS {
            break;
        }
    }

    tags
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
}

fn path_error(action: &str, path: &Path, error: io::Error) -> io::Error {
    io::Error::new(
        error.kind(),
        format!("{action} {}: {error}", path.display()),
    )
}

fn zip_error(action: &str, path: &Path, error: zip::result::ZipError) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!("{action} {}: {error}", path.display()),
    )
}

#[cfg(test)]
#[path = "../../tests/unit/modpack.rs"]
mod tests;

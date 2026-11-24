use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Component, Path, PathBuf};

use quick_xml::Reader;
use quick_xml::escape::unescape;
use quick_xml::events::{BytesStart, Event};

use crate::models::{
    InferredDependency, InferredDependencyKind, ModSource, PackageId, RimworldMod,
    parse_version_tag, select_best_version_key,
};
use crate::services::assembly_metadata::{AssemblyMetadata, read_assembly_metadata_file};

/// Byte pre-filters: files without any of these cannot produce an XML edge, so
/// they are not parsed at all. This keeps large modlists fast to scan.
const SCAN_MARKERS: [&[u8]; 2] = [b"MayRequire", b"PatchOperationFindMod"];

const HARMONY_PACKAGE_ID: &str = "brrainz.harmony";
const HARMONY_ASSEMBLY_NAME: &str = "0harmony";

/// Active content roots for a single mod and any conditional dependencies
/// declared in its `LoadFolders.xml`.
#[derive(Debug, Default)]
pub(crate) struct ActiveModFolders {
    /// Content roots relative to the mod folder (empty `PathBuf` means the mod
    /// root itself).
    pub(crate) content_roots: Vec<PathBuf>,

    /// Dependencies inferred from `IfModActive`, `IfModActiveAll`, and
    /// `IfModActiveAny` attributes in `LoadFolders.xml`.
    pub(crate) load_folder_dependencies: Vec<InferredDependency>,
}

/// Raw `PatchOperationFindMod` target awaiting cross-mod name resolution.
#[derive(Debug, Clone)]
pub(crate) struct FindModTarget {
    pub(crate) raw_name: String,
    pub(crate) source_file: PathBuf,
}

/// Everything gathered from scanning a single mod's active folders on disk.
#[derive(Debug, Default)]
pub(crate) struct ModContentScan {
    pub(crate) direct_dependencies: Vec<InferredDependency>,
    pub(crate) find_mod_targets: Vec<FindModTarget>,
    pub(crate) assembly_files: Vec<PathBuf>,
    pub(crate) is_patch_only: bool,
}

/// Resolve which subfolders RimWorld loads for `game_version`.
///
/// Uses `LoadFolders.xml` when present and valid; otherwise falls back to
/// RimWorld's default layout (mod root, `Common/`, and the best matching
/// `major.minor` folder).
pub(crate) fn active_mod_folders(
    mod_folder: &Path,
    game_version: Option<&str>,
) -> ActiveModFolders {
    if let Some(from_xml) = parse_load_folders_xml(mod_folder, game_version) {
        return from_xml;
    }

    default_active_mod_folders(mod_folder, game_version)
}

/// Scan a single mod's active folders for XML dependencies, `PatchOperationFindMod`
/// entries, active C# `.dll` files, and whether the mod is patch-only.
pub(crate) fn scan_mod_content(mod_folder: &Path, game_version: Option<&str>) -> ModContentScan {
    let active = active_mod_folders(mod_folder, game_version);
    let mut scan = ModContentScan {
        direct_dependencies: active.load_folder_dependencies,
        ..ModContentScan::default()
    };

    let mut has_defs = false;
    let mut has_patches = false;
    let mut xml_files = Vec::new();
    let mut seen_xml = HashSet::new();
    let mut seen_dlls = HashSet::new();

    for content_root in &active.content_roots {
        let root_path = if content_root.as_os_str().is_empty() {
            mod_folder.to_path_buf()
        } else {
            mod_folder.join(content_root)
        };

        let Ok(entries) = fs::read_dir(&root_path) else {
            continue;
        };

        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let Some(folder_name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };

            let child_path = entry.path();

            if folder_name.eq_ignore_ascii_case("Defs") {
                let before = xml_files.len();
                collect_files_with_extension(&child_path, "xml", &mut xml_files, &mut seen_xml);
                if xml_files.len() > before {
                    has_defs = true;
                }
            } else if folder_name.eq_ignore_ascii_case("Patches") {
                let before = xml_files.len();
                collect_files_with_extension(&child_path, "xml", &mut xml_files, &mut seen_xml);
                if xml_files.len() > before {
                    has_patches = true;
                }
            } else if folder_name.eq_ignore_ascii_case("Assemblies") {
                let mut dlls = Vec::new();
                collect_files_with_extension(&child_path, "dll", &mut dlls, &mut seen_dlls);
                for dll_path in dlls {
                    let relative = dll_path
                        .strip_prefix(mod_folder)
                        .unwrap_or(&dll_path)
                        .to_path_buf();
                    scan.assembly_files.push(relative);
                }
            }
        }
    }

    for path in xml_files {
        let Ok(contents) = fs::read(&path) else {
            continue;
        };

        if !SCAN_MARKERS
            .iter()
            .any(|marker| contains_subslice(&contents, marker))
        {
            continue;
        }

        let source_file = path.strip_prefix(mod_folder).unwrap_or(&path).to_path_buf();
        scan_xml(
            &contents,
            &source_file,
            &mut scan.direct_dependencies,
            &mut scan.find_mod_targets,
        );
    }

    scan.assembly_files.sort();
    scan.is_patch_only = has_patches && !has_defs && scan.assembly_files.is_empty();

    scan
}

/// Find load-order dependencies for a single mod folder without cross-mod
/// context (used by standalone XML inference tests).
#[cfg(test)]
pub(crate) fn infer_dependencies(mod_folder: &Path) -> Vec<InferredDependency> {
    infer_dependencies_for_version(mod_folder, None)
}

#[cfg(test)]
pub(crate) fn infer_dependencies_for_version(
    mod_folder: &Path,
    game_version: Option<&str>,
) -> Vec<InferredDependency> {
    let scan = scan_mod_content(mod_folder, game_version);
    let mut inferred = scan.direct_dependencies;

    for target in scan.find_mod_targets {
        if looks_like_package_id(&target.raw_name)
            && let Some(package_id) = PackageId::new(&target.raw_name)
        {
            inferred.push(InferredDependency {
                package_id,
                kind: InferredDependencyKind::FindMod,
                source_file: target.source_file,
            });
        }
    }

    deduplicate_and_sort(&mut inferred);
    inferred
}

/// Populate `inferred_after` and `is_patch_only` across the full installed mod
/// list, resolving:
/// - `LoadFolders.xml` conditional folders (`IfModActive*`)
/// - `MayRequire` / `MayRequireAnyOf` XML attributes
/// - `PatchOperationFindMod` entries against installed mod `<name>`s (and `packageId` fallback)
/// - C# `AssemblyRef` links mapped to the unique installed mod providing each assembly
pub(crate) fn resolve_cross_mod_dependencies(mods: &mut [RimworldMod], game_version: Option<&str>) {
    // Pass 1: build cross-mod lookups from all installed mods (including
    // official content so `PatchOperationFindMod` entries like "Biotech" resolve).
    let mut known_package_ids = HashSet::with_capacity(mods.len());
    let mut name_owners: HashMap<String, Vec<PackageId>> = HashMap::new();

    for rimworld_mod in mods.iter() {
        known_package_ids.insert(rimworld_mod.package_id.clone());

        let normalized_name = rimworld_mod.name.trim().to_ascii_lowercase();
        if !normalized_name.is_empty() {
            let owners = name_owners.entry(normalized_name).or_default();
            if !owners.contains(&rimworld_mod.package_id) {
                owners.push(rimworld_mod.package_id.clone());
            }
        }
    }

    let package_id_by_unique_name: HashMap<String, PackageId> = name_owners
        .into_iter()
        .filter_map(|(name, mut owners)| (owners.len() == 1).then(|| (name, owners.remove(0))))
        .collect();

    // Pass 2: scan active folders and parse C# assembly metadata for each mod.
    let mut scans: Vec<ModContentScan> = Vec::with_capacity(mods.len());
    let mut mod_assemblies: Vec<Vec<(PathBuf, AssemblyMetadata)>> = Vec::with_capacity(mods.len());
    let mut self_provided_assemblies: Vec<HashSet<String>> = Vec::with_capacity(mods.len());
    let mut providers_by_assembly: HashMap<String, Vec<PackageId>> = HashMap::new();

    for rimworld_mod in mods.iter() {
        if matches!(rimworld_mod.source, ModSource::Official) {
            scans.push(ModContentScan::default());
            mod_assemblies.push(Vec::new());
            self_provided_assemblies.push(HashSet::new());
            continue;
        }

        let scan = scan_mod_content(&rimworld_mod.folder, game_version);
        let mut parsed_assemblies = Vec::new();
        let mut provided_here = HashSet::new();

        for rel_dll in &scan.assembly_files {
            let full_path = rimworld_mod.folder.join(rel_dll);
            let Some(metadata) = read_assembly_metadata_file(&full_path) else {
                continue;
            };

            let lower_name = metadata.name.to_ascii_lowercase();
            provided_here.insert(lower_name.clone());

            if !is_ignored_system_assembly(&lower_name) {
                let providers = providers_by_assembly.entry(lower_name).or_default();
                if !providers.contains(&rimworld_mod.package_id) {
                    providers.push(rimworld_mod.package_id.clone());
                }
            }

            parsed_assemblies.push((rel_dll.clone(), metadata));
        }

        scans.push(scan);
        mod_assemblies.push(parsed_assemblies);
        self_provided_assemblies.push(provided_here);
    }

    // Pass 3: attach resolved dependencies and patch-only status to each mod.
    for (index, rimworld_mod) in mods.iter_mut().enumerate() {
        if matches!(rimworld_mod.source, ModSource::Official) {
            rimworld_mod.inferred_after.clear();
            rimworld_mod.is_patch_only = false;
            continue;
        }

        let scan = &mut scans[index];
        rimworld_mod.is_patch_only = scan.is_patch_only;

        let mut inferred = Vec::new();

        // Direct XML dependencies (LoadFolders.xml + MayRequire).
        for dependency in scan.direct_dependencies.drain(..) {
            if dependency.package_id != rimworld_mod.package_id {
                inferred.push(dependency);
            }
        }

        // PatchOperationFindMod entries: match mod <name> first, then known
        // packageId, then package-id-shaped fallback for uninstalled targets.
        for target in scan.find_mod_targets.drain(..) {
            let normalized = target.raw_name.trim().to_ascii_lowercase();
            if normalized.is_empty() {
                continue;
            }

            let resolved = package_id_by_unique_name
                .get(&normalized)
                .cloned()
                .or_else(|| {
                    let candidate = PackageId::new(&target.raw_name)?;
                    if known_package_ids.contains(&candidate)
                        || looks_like_package_id(&target.raw_name)
                    {
                        Some(candidate)
                    } else {
                        None
                    }
                });

            if let Some(package_id) = resolved
                && package_id != rimworld_mod.package_id
            {
                inferred.push(InferredDependency {
                    package_id,
                    kind: InferredDependencyKind::FindMod,
                    source_file: target.source_file,
                });
            }
        }

        // C# AssemblyRef resolution.
        let provided_here = &self_provided_assemblies[index];
        for (rel_dll, metadata) in &mod_assemblies[index] {
            for reference in &metadata.references {
                let lower_ref = reference.to_ascii_lowercase();

                if is_ignored_system_assembly(&lower_ref) || provided_here.contains(&lower_ref) {
                    continue;
                }

                let Some(providers) = providers_by_assembly.get(&lower_ref) else {
                    continue;
                };

                let target_package_id = resolve_assembly_provider(&lower_ref, providers);

                if let Some(package_id) = target_package_id
                    && package_id != rimworld_mod.package_id
                {
                    inferred.push(InferredDependency {
                        package_id,
                        kind: InferredDependencyKind::AssemblyReference,
                        source_file: rel_dll.clone(),
                    });
                }
            }
        }

        deduplicate_and_sort(&mut inferred);
        rimworld_mod.inferred_after = inferred;
    }
}

fn resolve_assembly_provider(lower_assembly: &str, providers: &[PackageId]) -> Option<PackageId> {
    if providers.len() == 1 {
        return Some(providers[0].clone());
    }

    // Many legacy mods bundle a copy of 0Harmony.dll alongside Harmony itself.
    // When brrainz.harmony is installed and a mod references 0Harmony without
    // bundling it, resolve cleanly to brrainz.harmony.
    if lower_assembly == HARMONY_ASSEMBLY_NAME {
        return providers
            .iter()
            .find(|package_id| package_id.as_str() == HARMONY_PACKAGE_ID)
            .cloned();
    }

    // Ambiguous shared library bundled by multiple unrelated mods: skip rather
    // than guessing the wrong dependency.
    None
}

fn is_ignored_system_assembly(lower_name: &str) -> bool {
    matches!(
        lower_name,
        "mscorlib"
            | "netstandard"
            | "system"
            | "unityengine"
            | "unityeditor"
            | "assembly-csharp"
            | "assembly-csharp-firstpass"
            | "com.rlabrecque.steamworks.net"
            | "icsharpcode.sharpziplib"
            | "naudio"
            | "nvorbis"
            | "newtonsoft.json"
            | "isurvey"
    ) || lower_name.starts_with("system.")
        || lower_name.starts_with("microsoft.")
        || lower_name.starts_with("mono.")
        || lower_name.starts_with("unityengine.")
        || lower_name.starts_with("unity.")
        || lower_name.starts_with("unityeditor.")
}

fn looks_like_package_id(raw: &str) -> bool {
    let trimmed = raw.trim();
    !trimmed.is_empty() && trimmed.contains('.') && !trimmed.chars().any(char::is_whitespace)
}

fn deduplicate_and_sort(inferred: &mut Vec<InferredDependency>) {
    let mut seen = HashSet::new();

    inferred.retain(|dependency| {
        seen.insert((
            dependency.package_id.clone(),
            dependency.kind,
            dependency.source_file.clone(),
        ))
    });

    inferred.sort_by(|left, right| {
        left.package_id
            .as_str()
            .cmp(right.package_id.as_str())
            .then_with(|| left.kind.sort_rank().cmp(&right.kind.sort_rank()))
            .then_with(|| left.source_file.cmp(&right.source_file))
    });
}

#[derive(Default)]
struct LoadFolderEntry {
    relative_path: PathBuf,
    required_package_ids: Vec<PackageId>,
}

fn parse_load_folders_xml(
    mod_folder: &Path,
    game_version: Option<&str>,
) -> Option<ActiveModFolders> {
    let load_folders_path = find_child_file_case_insensitive(mod_folder, "LoadFolders.xml")?;
    let contents = fs::read(&load_folders_path).ok()?;
    let source_file = load_folders_path
        .strip_prefix(mod_folder)
        .unwrap_or(&load_folders_path)
        .to_path_buf();

    let mut reader = Reader::from_reader(contents.as_slice());
    reader.config_mut().trim_text(true);

    let mut buffer = Vec::new();
    let mut depth = 0usize;
    let mut current_version: Option<(u32, u32)> = None;
    let mut current_entry: Option<LoadFolderEntry> = None;
    let mut sections: Vec<((u32, u32), Vec<LoadFolderEntry>)> = Vec::new();

    loop {
        buffer.clear();

        let event = match reader.read_event_into(&mut buffer) {
            Ok(event) => event,
            Err(_) => break,
        };

        match event {
            Event::Start(start) => {
                let tag = start.name();
                let tag_name = tag.as_ref();

                if depth == 1 {
                    if let Some(version) = parse_version_tag(tag_name) {
                        current_version = Some(version);
                        if !sections.iter().any(|(v, _)| *v == version) {
                            sections.push((version, Vec::new()));
                        }
                    } else {
                        current_version = None;
                    }
                } else if depth == 2
                    && current_version.is_some()
                    && tag_name.eq_ignore_ascii_case("li")
                {
                    current_entry = Some(LoadFolderEntry {
                        relative_path: PathBuf::new(),
                        required_package_ids: collect_load_folder_conditions(&start),
                    });
                }

                depth += 1;
            }

            Event::Empty(start) => {
                let tag = start.name();
                if depth == 2
                    && let Some(version) = current_version
                    && tag.as_ref().eq_ignore_ascii_case("li")
                {
                    let entry = LoadFolderEntry {
                        relative_path: PathBuf::new(),
                        required_package_ids: collect_load_folder_conditions(&start),
                    };
                    if let Some((_, entries)) = sections.iter_mut().find(|(v, _)| *v == version) {
                        entries.push(entry);
                    }
                }
            }

            Event::Text(text) => {
                if depth == 3
                    && let Some(entry) = current_entry.as_mut()
                {
                    let raw = text.into_inner();
                    if let Ok(value) = unescape(&raw)
                        && let Some(normalized) = normalize_load_folder_path(&value)
                    {
                        entry.relative_path = normalized;
                    }
                }
            }

            Event::End(_) => {
                depth = depth.saturating_sub(1);

                if depth == 2
                    && let Some(entry) = current_entry.take()
                    && let Some(version) = current_version
                    && let Some((_, entries)) = sections.iter_mut().find(|(v, _)| *v == version)
                {
                    entries.push(entry);
                } else if depth == 1 {
                    current_version = None;
                }
            }

            Event::Eof => break,

            _ => {}
        }
    }

    let selected = select_best_version_key(
        sections
            .iter()
            .map(|(version, entries)| (*version, entries)),
        game_version,
    )?;

    let mut content_roots = Vec::new();
    let mut seen_roots = HashSet::new();
    let mut load_folder_dependencies = Vec::new();

    for entry in selected {
        if seen_roots.insert(entry.relative_path.clone()) {
            content_roots.push(entry.relative_path.clone());
        }

        for package_id in &entry.required_package_ids {
            load_folder_dependencies.push(InferredDependency {
                package_id: package_id.clone(),
                kind: InferredDependencyKind::LoadFolderCondition,
                source_file: source_file.clone(),
            });
        }
    }

    Some(ActiveModFolders {
        content_roots,
        load_folder_dependencies,
    })
}

fn collect_load_folder_conditions(element: &BytesStart<'_>) -> Vec<PackageId> {
    let mut required = Vec::new();

    for attribute in element.attributes().flatten() {
        let key = attribute.key.as_ref();
        let is_positive_condition = key.eq_ignore_ascii_case("IfModActive")
            || key.eq_ignore_ascii_case("IfModActiveAll")
            || key.eq_ignore_ascii_case("IfModActiveAny");

        if !is_positive_condition {
            continue;
        }

        for raw_package_id in attribute.value.split(',') {
            if let Some(package_id) = PackageId::new(raw_package_id) {
                required.push(package_id);
            }
        }
    }

    required
}

fn normalize_load_folder_path(raw: &str) -> Option<PathBuf> {
    let trimmed = raw.trim().replace('\\', "/");
    let stripped = trimmed.trim_start_matches('/').trim_end_matches('/');

    if stripped.is_empty() || stripped == "." {
        return Some(PathBuf::new());
    }

    let mut normalized = PathBuf::new();
    for component in Path::new(stripped).components() {
        match component {
            Component::Normal(part) => normalized.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    Some(normalized)
}

fn default_active_mod_folders(mod_folder: &Path, game_version: Option<&str>) -> ActiveModFolders {
    let mut content_roots = vec![PathBuf::new()];
    let mut version_dirs: Vec<((u32, u32), PathBuf)> = Vec::new();

    if let Ok(entries) = fs::read_dir(mod_folder) {
        for entry in entries.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if !file_type.is_dir() {
                continue;
            }

            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };

            if name.eq_ignore_ascii_case("Common") {
                content_roots.push(PathBuf::from(&name));
            } else if !name.starts_with(['v', 'V'])
                && let Some(version) = parse_version_tag(&name)
            {
                version_dirs.push((version, PathBuf::from(&name)));
            }
        }
    }

    if let Some(selected_version_dir) = select_best_version_key(
        version_dirs.iter().map(|(version, path)| (*version, path)),
        game_version,
    ) {
        content_roots.push(selected_version_dir.clone());
    }

    ActiveModFolders {
        content_roots,
        load_folder_dependencies: Vec::new(),
    }
}

fn find_child_file_case_insensitive(folder: &Path, target_name: &str) -> Option<PathBuf> {
    let direct = folder.join(target_name);
    if direct.is_file() {
        return Some(direct);
    }

    let entries = fs::read_dir(folder).ok()?;
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_file()
            && entry
                .file_name()
                .to_str()
                .is_some_and(|name| name.eq_ignore_ascii_case(target_name))
        {
            return Some(entry.path());
        }
    }

    None
}

fn collect_files_with_extension(
    folder: &Path,
    extension: &str,
    files: &mut Vec<PathBuf>,
    seen: &mut HashSet<PathBuf>,
) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };

    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };

        let path = entry.path();

        if file_type.is_dir() {
            collect_files_with_extension(&path, extension, files, seen);
        } else if file_type.is_file()
            && has_extension(&path, extension)
            && seen.insert(path.clone())
        {
            files.push(path);
        }
    }
}

fn has_extension(path: &Path, expected: &str) -> bool {
    path.extension()
        .and_then(|extension| extension.to_str())
        .is_some_and(|extension| extension.eq_ignore_ascii_case(expected))
}

fn contains_subslice(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn scan_xml(
    contents: &[u8],
    source_file: &Path,
    direct_dependencies: &mut Vec<InferredDependency>,
    find_mod_targets: &mut Vec<FindModTarget>,
) {
    let mut reader = Reader::from_reader(contents);
    reader.config_mut().trim_text(true);

    let mut buffer = Vec::new();
    let mut element_stack: Vec<String> = Vec::new();

    // Stack length when the current PatchOperationFindMod element started.
    let mut find_mod_depth = None;

    loop {
        buffer.clear();

        let event = match reader.read_event_into(&mut buffer) {
            Ok(event) => event,
            // A broken file should not discard the edges found before the error.
            Err(_) => return,
        };

        match event {
            Event::Start(start) => {
                collect_may_require(&start, source_file, direct_dependencies);

                if is_find_mod(&start) {
                    find_mod_depth = Some(element_stack.len());
                }

                element_stack.push(start.name().as_ref().to_owned());
            }

            Event::Empty(start) => {
                collect_may_require(&start, source_file, direct_dependencies);
            }

            Event::End(_) => {
                element_stack.pop();

                if find_mod_depth.is_some_and(|depth| element_stack.len() <= depth) {
                    find_mod_depth = None;
                }
            }

            Event::Text(text) => {
                if find_mod_depth.is_some() && is_mods_list_item(&element_stack) {
                    let raw = text.into_inner();

                    if let Ok(value) = unescape(&raw) {
                        let trimmed = value.trim();
                        if !trimmed.is_empty() {
                            find_mod_targets.push(FindModTarget {
                                raw_name: trimmed.to_owned(),
                                source_file: source_file.to_path_buf(),
                            });
                        }
                    }
                }
            }

            Event::Eof => break,

            _ => {}
        }
    }
}

fn collect_may_require(
    element: &BytesStart<'_>,
    source_file: &Path,
    inferred: &mut Vec<InferredDependency>,
) {
    for attribute in element.attributes().flatten() {
        let kind = match attribute.key.as_ref() {
            "MayRequire" | "MayRequireAnyOf" => InferredDependencyKind::MayRequire,
            _ => continue,
        };

        for raw_package_id in attribute.value.split(',') {
            let Some(package_id) = PackageId::new(raw_package_id) else {
                continue;
            };

            inferred.push(InferredDependency {
                package_id,
                kind,
                source_file: source_file.to_path_buf(),
            });
        }
    }
}

fn is_find_mod(element: &BytesStart<'_>) -> bool {
    element.attributes().flatten().any(|attribute| {
        attribute.key.as_ref() == "Class" && attribute.value.trim() == "PatchOperationFindMod"
    })
}

/// Matches the `<mods><li>Mod Name</li></mods>` shape of a FindMod list.
fn is_mods_list_item(element_stack: &[String]) -> bool {
    let Some((item, parents)) = element_stack.split_last() else {
        return false;
    };

    item == "li" && parents.last().is_some_and(|parent| parent == "mods")
}

#[cfg(test)]
#[path = "../../tests/unit/dependency_inference.rs"]
mod tests;

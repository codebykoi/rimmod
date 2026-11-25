use std::{fmt, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct ModId(usize);

impl ModId {
    pub(crate) fn from_index(index: usize) -> Self {
        Self(index)
    }

    pub(crate) fn index(self) -> usize {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct PackageId(String);

impl PackageId {
    pub(crate) fn new(raw: &str) -> Option<Self> {
        let normalized = raw.trim().to_ascii_lowercase();

        if normalized.is_empty() {
            return None;
        }

        Some(Self(normalized))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for PackageId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

#[derive(Default, PartialEq, Eq)]
pub(crate) enum ModType {
    /// Contains CSharp code
    CSharp,

    /// XML only mod, no CSharp code
    Xml,

    // Fallback
    #[default]
    Unknown,
}

/// Mod sources, ordered by priority
#[derive(Default, PartialEq, Eq)]
pub(crate) enum ModSource {
    /// Official game content
    Official,

    /// From steam workshop
    SteamWorkshop { workshop_id: u64 },

    /// Has public Git repository
    Git { remote_url: String },

    /// Local folder, used for everything else
    Local,

    /// For conflicting or unreadable data, used as fallback
    #[default]
    Unknown,
}

/// Where a load-order dependency was found when it was not declared in
/// `About.xml`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) enum InferredDependencyKind {
    /// A compiled `AssemblyRef` entry in one of the mod's C# `.dll` assemblies.
    AssemblyReference,

    /// An `IfModActive`, `IfModActiveAll`, or `IfModActiveAny` condition in `LoadFolders.xml`.
    LoadFolderCondition,

    /// A `MayRequire` or `MayRequireAnyOf` attribute in defs or patch XML.
    MayRequire,

    /// A `PatchOperationFindMod` list in patch XML.
    FindMod,
}

impl InferredDependencyKind {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::AssemblyReference => "C# assembly reference",
            Self::LoadFolderCondition => "LoadFolders.xml condition",
            Self::MayRequire => "MayRequire attribute",
            Self::FindMod => "PatchOperationFindMod",
        }
    }

    pub(crate) fn sort_rank(self) -> u8 {
        match self {
            Self::AssemblyReference => 0,
            Self::LoadFolderCondition => 1,
            Self::MayRequire => 2,
            Self::FindMod => 3,
        }
    }
}

/// A load-order dependency guessed from a mod's content (XML or C# assemblies).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) struct InferredDependency {
    pub(crate) package_id: PackageId,
    pub(crate) kind: InferredDependencyKind,

    /// File that produced the dependency, relative to the mod folder.
    pub(crate) source_file: PathBuf,
}

#[derive(PartialEq, Eq)]
pub(crate) struct RimworldMod {
    pub(crate) name: String,
    pub(crate) package_id: PackageId,
    pub(crate) description: String,
    pub(crate) supported_versions: Vec<String>,
    pub(crate) community_supported_versions: Vec<String>,

    /// Declared in `About.xml` as `loadAfter` (and versioned `loadAfterByVersion`).
    pub(crate) loader_after: Vec<PackageId>,

    /// Declared in `About.xml` as `loadBefore` (and versioned `loadBeforeByVersion`).
    pub(crate) loader_before: Vec<PackageId>,

    /// Declared in `About.xml` as `forceLoadAfter` (and versioned `forceLoadAfterByVersion`).
    pub(crate) force_load_after: Vec<PackageId>,

    /// Declared in `About.xml` as `forceLoadBefore` (and versioned `forceLoadBeforeByVersion`).
    pub(crate) force_load_before: Vec<PackageId>,

    /// Declared in `About.xml` as `modDependencies`; these must load first.
    pub(crate) mod_dependencies: Vec<PackageId>,

    /// Declared in `About.xml` as `incompatibleWith`; the game itself warns
    /// when one of these is active at the same time.
    pub(crate) incompatible_with: Vec<PackageId>,

    /// Guessed from the mod's XML and C# assemblies; weaker than explicit declarations.
    pub(crate) inferred_after: Vec<InferredDependency>,

    /// True when the mod's active folders contain XML patches without any defs
    /// or C# assemblies; patch-only mods softly sort after content mods.
    pub(crate) is_patch_only: bool,

    pub(crate) folder: PathBuf,
    pub(crate) source: ModSource,
    pub(crate) mod_type: ModType,
}

pub(crate) fn versions_are_compatible(supported_version: &str, game_version: &str) -> bool {
    match (
        major_minor_version(supported_version),
        major_minor_version(game_version),
    ) {
        (Some(supported), Some(game)) => supported == game,
        _ => false,
    }
}

pub(crate) fn major_minor_version_text(version: &str) -> Option<String> {
    let (major, minor) = major_minor_version(version)?;
    Some(format!("{major}.{minor}"))
}

pub(crate) fn major_minor_version(version: &str) -> Option<(u32, u32)> {
    let mut parts = version.trim().split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.split_whitespace().next()?.parse().ok()?;

    Some((major, minor))
}

/// Parse an XML version tag like `v1.5` or `1.5` into `(major, minor)`.
pub(crate) fn parse_version_tag(tag: &str) -> Option<(u32, u32)> {
    let trimmed = tag.trim();
    let without_prefix = trimmed
        .strip_prefix('v')
        .or_else(|| trimmed.strip_prefix('V'))
        .unwrap_or(trimmed);

    // Reject trailing junk so folder names like "1.5_backup" do not match.
    let mut parts = without_prefix.split('.');
    let major = parts.next()?.parse::<u32>().ok()?;
    let minor = parts.next()?.parse::<u32>().ok()?;
    if parts.next().is_some() {
        return None;
    }

    Some((major, minor))
}

/// Pick the best matching versioned entry for `game_version`.
///
/// Prefers an exact `major.minor` match, then the highest version not newer
/// than `game_version`, then the lowest available version. When `game_version`
/// is unknown, returns the highest available version.
pub(crate) fn select_best_version_key<'a, T>(
    candidates: impl IntoIterator<Item = ((u32, u32), &'a T)>,
    game_version: Option<&str>,
) -> Option<&'a T> {
    let mut entries = candidates.into_iter().collect::<Vec<_>>();
    if entries.is_empty() {
        return None;
    }

    entries.sort_by_key(|(version, _)| *version);

    let Some(target) = game_version.and_then(major_minor_version) else {
        return entries.last().map(|(_, value)| *value);
    };

    if let Some((_, exact)) = entries.iter().find(|(version, _)| *version == target) {
        return Some(*exact);
    }

    if let Some((_, fallback)) = entries.iter().rev().find(|(version, _)| *version <= target) {
        return Some(*fallback);
    }

    entries.first().map(|(_, value)| *value)
}

/// How a mod's version data relates to the running game version.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum VersionSupport {
    /// Listed in the mod's `supportedVersions`
    Official,

    /// Not listed, but reported working by the No Version Warning mod
    Community,

    /// Neither listed nor reported for this game version
    Unsupported,
}

pub(crate) fn version_support(
    supported_versions: &[String],
    community_supported_versions: &[String],
    game_version: &str,
) -> VersionSupport {
    if supported_versions
        .iter()
        .any(|version| versions_are_compatible(version, game_version))
    {
        return VersionSupport::Official;
    }

    if community_supported_versions
        .iter()
        .any(|version| versions_are_compatible(version, game_version))
    {
        return VersionSupport::Community;
    }

    VersionSupport::Unsupported
}

/// Return the newest listed version, compared by major.minor numbers.
pub(crate) fn highest_supported_version(supported_versions: &[String]) -> Option<&str> {
    supported_versions
        .iter()
        .filter_map(|version| Some((major_minor_version(version)?, version.as_str())))
        .max_by_key(|(version, _)| *version)
        .map(|(_, version)| version)
}

/// Return the newest version across official and community reports, along with
/// whether it came from a community report (preferring official on ties).
pub(crate) fn newest_known_version<'a>(
    supported_versions: &'a [String],
    community_supported_versions: &'a [String],
) -> Option<(&'a str, bool)> {
    let official = highest_supported_version(supported_versions)
        .and_then(|version| Some((major_minor_version(version)?, version)));
    let community = highest_supported_version(community_supported_versions)
        .and_then(|version| Some((major_minor_version(version)?, version)));

    match (official, community) {
        (Some((official_key, official_version)), Some((community_key, community_version))) => {
            if community_key > official_key {
                Some((community_version, true))
            } else {
                Some((official_version, false))
            }
        }
        (Some((_, official_version)), None) => Some((official_version, false)),
        (None, Some((_, community_version))) => Some((community_version, true)),
        (None, None) => None,
    }
}

#[derive(Debug)]
pub(crate) enum ModCollectionError {
    NotDisabled(ModId),
    NotActive(ModId),
}

impl fmt::Display for ModCollectionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotDisabled(mod_id) => {
                write!(
                    formatter,
                    "mod {} is not in the disabled list",
                    mod_id.index()
                )
            }
            Self::NotActive(mod_id) => {
                write!(
                    formatter,
                    "mod {} is not in the active list",
                    mod_id.index()
                )
            }
        }
    }
}

impl std::error::Error for ModCollectionError {}

#[derive(Default)]
pub(crate) struct ModCollection {
    pub(crate) all: Vec<RimworldMod>,
    disabled_ids: Vec<ModId>,
    enabled_ids: Vec<ModId>,
    missing_active_package_ids: Vec<PackageId>,
}

impl ModCollection {
    pub(crate) fn new(
        all: Vec<RimworldMod>,
        disabled_ids: Vec<ModId>,
        enabled_ids: Vec<ModId>,
        missing_active_package_ids: Vec<PackageId>,
    ) -> Self {
        Self {
            all,
            disabled_ids,
            enabled_ids,
            missing_active_package_ids,
        }
    }

    pub(crate) fn get(&self, mod_id: ModId) -> Option<&RimworldMod> {
        self.all.get(mod_id.0)
    }

    pub(crate) fn enabled_ids(&self) -> &[ModId] {
        &self.enabled_ids
    }

    pub(crate) fn disabled_ids(&self) -> &[ModId] {
        &self.disabled_ids
    }

    pub(crate) fn missing_active_package_ids(&self) -> &[PackageId] {
        &self.missing_active_package_ids
    }

    /// Return the current in-memory active order, including entries whose mod
    /// folders are currently missing.
    pub(crate) fn active_package_ids(&self) -> Vec<PackageId> {
        let mut package_ids = self
            .enabled_ids
            .iter()
            .filter_map(|&mod_id| self.get(mod_id))
            .map(|rimworld_mod| rimworld_mod.package_id.clone())
            .collect::<Vec<_>>();

        package_ids.extend(self.missing_active_package_ids.iter().cloned());
        package_ids
    }

    /// Rebuild active and disabled IDs after the backing mod folders change.
    pub(crate) fn replace_active_package_ids(&mut self, package_ids: Vec<PackageId>) {
        let mut enabled_ids = Vec::new();
        let mut missing_active_package_ids = Vec::new();

        for package_id in package_ids {
            let matching_id = self
                .all
                .iter()
                .position(|rimworld_mod| rimworld_mod.package_id == package_id)
                .map(ModId::from_index);

            match matching_id {
                Some(mod_id) => enabled_ids.push(mod_id),
                None => missing_active_package_ids.push(package_id),
            }
        }

        let disabled_ids = (0..self.all.len())
            .map(ModId::from_index)
            .filter(|mod_id| !enabled_ids.contains(mod_id))
            .collect();

        self.enabled_ids = enabled_ids;
        self.disabled_ids = disabled_ids;
        self.missing_active_package_ids = missing_active_package_ids;
    }

    pub(crate) fn find_id_by_package_id(&self, package_id: &PackageId) -> Option<ModId> {
        self.all
            .iter()
            .position(|rimworld_mod| &rimworld_mod.package_id == package_id)
            .map(ModId::from_index)
    }

    fn move_many_before(
        mod_ids: &mut Vec<ModId>,
        moved_mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) {
        if before_mod_id.is_some_and(|mod_id| moved_mod_ids.contains(&mod_id)) {
            return;
        }

        let moved_in_list_order = mod_ids
            .iter()
            .copied()
            .filter(|mod_id| moved_mod_ids.contains(mod_id))
            .collect::<Vec<_>>();

        mod_ids.retain(|mod_id| !moved_mod_ids.contains(mod_id));

        let target_position = before_mod_id
            .and_then(|before_id| mod_ids.iter().position(|&mod_id| mod_id == before_id))
            .unwrap_or(mod_ids.len());

        mod_ids.splice(target_position..target_position, moved_in_list_order);
    }

    fn transfer_many(
        source_ids: &mut Vec<ModId>,
        target_ids: &mut Vec<ModId>,
        moved_mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) -> Option<ModId> {
        let invalid_id = moved_mod_ids
            .iter()
            .find(|mod_id| !source_ids.contains(mod_id))
            .copied();

        if invalid_id.is_some() {
            return invalid_id;
        }

        let moved_in_list_order = source_ids
            .iter()
            .copied()
            .filter(|mod_id| moved_mod_ids.contains(mod_id))
            .collect::<Vec<_>>();

        source_ids.retain(|mod_id| !moved_mod_ids.contains(mod_id));

        let target_position = before_mod_id
            .and_then(|before_id| target_ids.iter().position(|&mod_id| mod_id == before_id))
            .unwrap_or(target_ids.len());

        target_ids.splice(target_position..target_position, moved_in_list_order);

        None
    }

    pub(crate) fn enable_many(
        &mut self,
        mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) -> Result<(), ModCollectionError> {
        match Self::transfer_many(
            &mut self.disabled_ids,
            &mut self.enabled_ids,
            mod_ids,
            before_mod_id,
        ) {
            Some(invalid_id) => Err(ModCollectionError::NotDisabled(invalid_id)),
            None => Ok(()),
        }
    }

    pub(crate) fn disable_many(
        &mut self,
        mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) -> Result<(), ModCollectionError> {
        match Self::transfer_many(
            &mut self.enabled_ids,
            &mut self.disabled_ids,
            mod_ids,
            before_mod_id,
        ) {
            Some(invalid_id) => Err(ModCollectionError::NotActive(invalid_id)),
            None => Ok(()),
        }
    }

    pub(crate) fn reorder_enabled_many(
        &mut self,
        mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) -> Result<(), ModCollectionError> {
        if let Some(invalid_id) = mod_ids
            .iter()
            .find(|mod_id| !self.enabled_ids.contains(mod_id))
            .copied()
        {
            return Err(ModCollectionError::NotActive(invalid_id));
        }

        Self::move_many_before(&mut self.enabled_ids, mod_ids, before_mod_id);
        Ok(())
    }

    pub(crate) fn reorder_disabled_many(
        &mut self,
        mod_ids: &[ModId],
        before_mod_id: Option<ModId>,
    ) -> Result<(), ModCollectionError> {
        if let Some(invalid_id) = mod_ids
            .iter()
            .find(|mod_id| !self.disabled_ids.contains(mod_id))
            .copied()
        {
            return Err(ModCollectionError::NotDisabled(invalid_id));
        }

        Self::move_many_before(&mut self.disabled_ids, mod_ids, before_mod_id);
        Ok(())
    }

    pub(crate) fn replace_enabled_order(&mut self, enabled_ids: Vec<ModId>) {
        self.enabled_ids = enabled_ids;
    }

    pub(crate) fn restore_list_orders(
        &mut self,
        disabled_ids: Vec<ModId>,
        enabled_ids: Vec<ModId>,
    ) {
        self.disabled_ids = disabled_ids;
        self.enabled_ids = enabled_ids;
    }
}

#[cfg(test)]
#[path = "../tests/unit/models.rs"]
mod tests;

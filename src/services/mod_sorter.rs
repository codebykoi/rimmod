use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::models::{InferredDependencyKind, ModCollection, ModId, ModSource};

#[derive(Debug)]
pub(crate) enum SortError {
    DuplicatePackageId(String),
    DependencyCycle(Vec<String>),
}

impl fmt::Display for SortError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicatePackageId(package_id) => {
                write!(formatter, "duplicate active package ID: {package_id}")
            }
            Self::DependencyCycle(package_ids) => {
                write!(
                    formatter,
                    "dependency cycle involving: {}",
                    package_ids.join(", ")
                )
            }
        }
    }
}

impl Error for SortError {}

#[derive(Debug, Default)]
pub(crate) struct OrderWarning {
    pub(crate) after_mod_ids: Vec<ModId>,
    pub(crate) before_mod_ids: Vec<ModId>,

    /// Active mods that share an incompatibility with this one, in either
    /// direction. Only one About.xml has to name the other.
    pub(crate) incompatible_mod_ids: Vec<ModId>,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct SortReport {
    /// Explicit `loadAfter`/`loadBefore` rules skipped because they contradict
    /// `forceLoad*` rules or form a cycle with other explicit rules.
    pub(crate) explicit_edges_skipped: usize,

    /// Guessed dependencies that produced a usable ordering constraint.
    pub(crate) inferred_edges_applied: usize,

    /// Guessed dependencies ignored because they would create a cycle.
    pub(crate) inferred_edges_skipped: usize,

    /// Declared dependencies ignored because they contradict explicit ordering.
    pub(crate) dependency_edges_skipped: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EdgeOutcome {
    Added,
    AlreadyPresent,
    WouldCycle,
}

pub(crate) fn sort_mods(mods: &mut ModCollection) -> Result<SortReport, SortError> {
    let active_count = mods.enabled_ids().len();

    // Maps a package ID to its position in the current active load order.
    let mut positions_by_id = HashMap::new();

    for (position, &mod_id) in mods.enabled_ids().iter().enumerate() {
        let package_id = &mods.all[mod_id.index()].package_id;

        if positions_by_id.insert(package_id, position).is_some() {
            return Err(SortError::DuplicatePackageId(package_id.to_string()));
        }
    }

    // outgoing[A] contains every mod that must load after A.
    let mut outgoing = vec![Vec::new(); active_count];

    // Number of requirements that must come before each mod.
    let mut incoming_count = vec![0; active_count];

    // Prevent the same rule from being counted twice.
    let mut edges = HashSet::new();

    // Process cycle-breakable rules in a canonical order independent of the
    // user's current list positions so sorting is always idempotent.
    let mut canonical_positions = (0..active_count).collect::<Vec<_>>();
    canonical_positions.sort_by(|&left, &right| {
        let left_id = mods.all[mods.enabled_ids()[left].index()]
            .package_id
            .as_str();
        let right_id = mods.all[mods.enabled_ids()[right].index()]
            .package_id
            .as_str();
        left_id.cmp(right_id)
    });

    // Official content (Core and the DLCs) is anchored before normal mods, but
    // its internal order is defined by the DLCs' own forceLoadAfter/Before
    // rules. Do not pin the current order here: the user may have it wrong, and
    // the explicit rules must be able to fix it.
    let official_positions = mods
        .enabled_ids()
        .iter()
        .enumerate()
        .filter(|(_, mod_id)| matches!(&mods.all[mod_id.index()].source, ModSource::Official))
        .map(|(position, _)| position)
        .collect::<Vec<_>>();

    let official_position_set = official_positions.iter().copied().collect::<HashSet<_>>();

    let mut report = SortReport::default();

    // Phase 1: `forceLoadAfter` / `forceLoadBefore` are the strictest rules.
    // Non-official forced cycles remain errors; rules touching official content
    // use `try_add_edge` so official rules cannot deadlock the graph.
    for &position in &canonical_positions {
        let mod_id = mods.enabled_ids()[position];
        let rimworld_mod = &mods.all[mod_id.index()];

        for package_id in &rimworld_mod.force_load_after {
            let Some(&required_position) = positions_by_id.get(package_id) else {
                continue;
            };

            if official_position_set.contains(&position)
                || official_position_set.contains(&required_position)
            {
                try_add_edge(
                    required_position,
                    position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            } else {
                add_edge(
                    required_position,
                    position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            }
        }

        for package_id in &rimworld_mod.force_load_before {
            let Some(&later_position) = positions_by_id.get(package_id) else {
                continue;
            };

            if official_position_set.contains(&position)
                || official_position_set.contains(&later_position)
            {
                try_add_edge(
                    position,
                    later_position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            } else {
                add_edge(
                    position,
                    later_position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            }
        }
    }

    // Phase 2: regular `loadAfter` / `loadBefore`. These run before official
    // anchoring so `loadBefore Ludeon.RimWorld` (e.g. Harmony) wins over the
    // default "everything after Core" anchor. Conflicting third-party cycles
    // skip only the contradicting edge instead of blocking the entire sort.
    for &position in &canonical_positions {
        let mod_id = mods.enabled_ids()[position];
        let rimworld_mod = &mods.all[mod_id.index()];

        for package_id in &rimworld_mod.loader_after {
            let Some(&required_position) = positions_by_id.get(package_id) else {
                continue;
            };

            if try_add_edge(
                required_position,
                position,
                &mut outgoing,
                &mut incoming_count,
                &mut edges,
            ) == EdgeOutcome::WouldCycle
            {
                report.explicit_edges_skipped += 1;
            }
        }

        for package_id in &rimworld_mod.loader_before {
            let Some(&later_position) = positions_by_id.get(package_id) else {
                continue;
            };

            if try_add_edge(
                position,
                later_position,
                &mut outgoing,
                &mut incoming_count,
                &mut edges,
            ) == EdgeOutcome::WouldCycle
            {
                report.explicit_edges_skipped += 1;
            }
        }
    }

    // Phase 3: anchor everything else after official content. Rules that would
    // close a cycle here are skipped, preserving pre-Core mods (like Harmony)
    // and anything transitively required before official content.
    for &official_position in &official_positions {
        for later_position in 0..active_count {
            if !official_position_set.contains(&later_position) {
                try_add_edge(
                    official_position,
                    later_position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            }
        }
    }

    // Phase 4: declared `modDependencies` (softer than explicit load rules,
    // stronger than inferred edges). Also track active library/framework mods
    // for soft tiering.
    let mut is_library = vec![false; active_count];

    for &position in &canonical_positions {
        let mod_id = mods.enabled_ids()[position];
        let rimworld_mod = &mods.all[mod_id.index()];
        let mut seen_targets = HashSet::new();

        for package_id in &rimworld_mod.mod_dependencies {
            let Some(&required_position) = positions_by_id.get(package_id) else {
                continue;
            };

            if required_position == position || official_position_set.contains(&required_position) {
                continue;
            }

            if !seen_targets.insert(required_position) {
                continue;
            }

            is_library[required_position] = true;

            if try_add_edge(
                required_position,
                position,
                &mut outgoing,
                &mut incoming_count,
                &mut edges,
            ) == EdgeOutcome::WouldCycle
            {
                report.dependency_edges_skipped += 1;
            }
        }
    }

    // Phase 5: strong inferred dependencies (compiled C# `AssemblyRef` links)
    // outrank XML heuristics.
    for &position in &canonical_positions {
        let mod_id = mods.enabled_ids()[position];
        let rimworld_mod = &mods.all[mod_id.index()];
        let mut seen_targets = HashSet::new();

        for dependency in rimworld_mod
            .inferred_after
            .iter()
            .filter(|dep| dep.kind == InferredDependencyKind::AssemblyReference)
        {
            let Some(&required_position) = positions_by_id.get(&dependency.package_id) else {
                continue;
            };

            if required_position == position || official_position_set.contains(&required_position) {
                continue;
            }

            if !seen_targets.insert(required_position) {
                continue;
            }

            is_library[required_position] = true;

            match try_add_edge(
                required_position,
                position,
                &mut outgoing,
                &mut incoming_count,
                &mut edges,
            ) {
                EdgeOutcome::Added => report.inferred_edges_applied += 1,
                EdgeOutcome::WouldCycle => report.inferred_edges_skipped += 1,
                EdgeOutcome::AlreadyPresent => {}
            }
        }
    }

    // Phase 6: softer XML inferred dependencies (`LoadFolders.xml`,
    // `MayRequire`, `PatchOperationFindMod`).
    for &position in &canonical_positions {
        let mod_id = mods.enabled_ids()[position];
        let rimworld_mod = &mods.all[mod_id.index()];
        let mut seen_targets = HashSet::new();

        for dependency in rimworld_mod
            .inferred_after
            .iter()
            .filter(|dep| dep.kind != InferredDependencyKind::AssemblyReference)
        {
            let Some(&required_position) = positions_by_id.get(&dependency.package_id) else {
                continue;
            };

            if required_position == position || official_position_set.contains(&required_position) {
                continue;
            }

            if !seen_targets.insert(required_position) {
                continue;
            }

            match try_add_edge(
                required_position,
                position,
                &mut outgoing,
                &mut incoming_count,
                &mut edges,
            ) {
                EdgeOutcome::Added => report.inferred_edges_applied += 1,
                EdgeOutcome::WouldCycle => report.inferred_edges_skipped += 1,
                EdgeOutcome::AlreadyPresent => {}
            }
        }
    }

    // Soft tiers for unconstrained mods (never override any DAG edge):
    // 0 = Official, 1 = Libraries/Frameworks, 2 = Regular mods, 3 = Patch-only XML mods.
    // Within each tier, the user's current list position breaks ties.
    let tiers: Vec<u8> = (0..active_count)
        .map(|position| {
            let rimworld_mod = &mods.all[mods.enabled_ids()[position].index()];
            if official_position_set.contains(&position) {
                0
            } else if is_library[position] {
                1
            } else if rimworld_mod.is_patch_only {
                3
            } else {
                2
            }
        })
        .collect();

    // BTreeSet keyed by (tier, current_position) keeps libraries above regular
    // mods and patch-only mods below content mods while preserving the user's
    // existing relative order within each tier (and remaining idempotent).
    let mut ready = BTreeSet::new();

    for (position, &count) in incoming_count.iter().enumerate() {
        if count == 0 {
            ready.insert((tiers[position], position));
        }
    }

    let mut sorted_positions = Vec::with_capacity(active_count);

    while let Some((_, position)) = ready.pop_first() {
        sorted_positions.push(position);

        for &later_position in &outgoing[position] {
            incoming_count[later_position] -= 1;

            if incoming_count[later_position] == 0 {
                ready.insert((tiers[later_position], later_position));
            }
        }
    }

    if sorted_positions.len() != active_count {
        let blocked_package_ids = incoming_count
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(position, _)| {
                let mod_id = mods.enabled_ids()[position];

                mods.get(mod_id)
                    .expect("enabled ModId should exist in ModCollection")
                    .package_id
                    .to_string()
            })
            .collect();

        return Err(SortError::DependencyCycle(blocked_package_ids));
    }

    let sorted_ids = sorted_positions
        .into_iter()
        .map(|position| mods.enabled_ids()[position])
        .collect();

    mods.replace_enabled_order(sorted_ids);

    Ok(report)
}

fn add_edge(
    earlier: usize,
    later: usize,
    outgoing: &mut [Vec<usize>],
    incoming_count: &mut [usize],
    edges: &mut HashSet<(usize, usize)>,
) {
    if edges.insert((earlier, later)) {
        outgoing[earlier].push(later);
        incoming_count[later] += 1;
    }
}

/// Add `earlier -> later` only if it keeps the graph acyclic.
fn try_add_edge(
    earlier: usize,
    later: usize,
    outgoing: &mut [Vec<usize>],
    incoming_count: &mut [usize],
    edges: &mut HashSet<(usize, usize)>,
) -> EdgeOutcome {
    if earlier == later || reaches(later, earlier, outgoing) {
        return EdgeOutcome::WouldCycle;
    }

    if edges.contains(&(earlier, later)) {
        return EdgeOutcome::AlreadyPresent;
    }

    add_edge(earlier, later, outgoing, incoming_count, edges);

    EdgeOutcome::Added
}

/// Depth-first check for a path from `start` to `target`.
fn reaches(start: usize, target: usize, outgoing: &[Vec<usize>]) -> bool {
    if start == target {
        return true;
    }

    let mut visited = vec![false; outgoing.len()];
    let mut stack = vec![start];
    visited[start] = true;

    while let Some(current) = stack.pop() {
        for &later in &outgoing[current] {
            if later == target {
                return true;
            }

            if !visited[later] {
                visited[later] = true;
                stack.push(later);
            }
        }
    }

    false
}

pub(crate) fn find_order_warnings(
    mods: &ModCollection,
) -> Result<HashMap<ModId, OrderWarning>, SortError> {
    let mut positions_by_id = HashMap::new();

    for (position, &mod_id) in mods.enabled_ids().iter().enumerate() {
        let package_id = mods.all[mod_id.index()].package_id.as_str();

        if positions_by_id
            .insert(package_id, (position, mod_id))
            .is_some()
        {
            return Err(SortError::DuplicatePackageId(package_id.to_owned()));
        }
    }

    let mut warnings = HashMap::new();

    // Incompatibility is mutual in practice: the game warns about the pair even
    // when only one About.xml names the other, so both rows get the warning.
    let mut incompatible_mods = HashMap::<ModId, Vec<ModId>>::new();

    for &mod_id in mods.enabled_ids() {
        let rimworld_mod = &mods.all[mod_id.index()];

        for package_id in &rimworld_mod.incompatible_with {
            let Some(&(_, other_mod_id)) = positions_by_id.get(package_id.as_str()) else {
                // The other mod is not active, so there is no conflict.
                continue;
            };

            push_unique(&mut incompatible_mods, mod_id, other_mod_id);
            push_unique(&mut incompatible_mods, other_mod_id, mod_id);
        }
    }

    for (position, &mod_id) in mods.enabled_ids().iter().enumerate() {
        let rimworld_mod = &mods.all[mod_id.index()];
        let mut warning = OrderWarning {
            incompatible_mod_ids: incompatible_mods.remove(&mod_id).unwrap_or_default(),
            ..OrderWarning::default()
        };

        for package_id in rimworld_mod
            .force_load_after
            .iter()
            .chain(&rimworld_mod.loader_after)
        {
            let Some(&(required_position, required_mod_index)) =
                positions_by_id.get(package_id.as_str())
            else {
                // The referenced mod is not active, so it does not affect this order.
                continue;
            };

            // The required mod must appear before this mod.
            if required_position >= position && !warning.after_mod_ids.contains(&required_mod_index)
            {
                warning.after_mod_ids.push(required_mod_index);
            }
        }

        for package_id in rimworld_mod
            .force_load_before
            .iter()
            .chain(&rimworld_mod.loader_before)
        {
            let Some(&(required_position, required_mod_index)) =
                positions_by_id.get(package_id.as_str())
            else {
                continue;
            };

            // The required mod must appear after this mod.
            if required_position <= position
                && !warning.before_mod_ids.contains(&required_mod_index)
            {
                warning.before_mod_ids.push(required_mod_index);
            }
        }

        if !warning.after_mod_ids.is_empty()
            || !warning.before_mod_ids.is_empty()
            || !warning.incompatible_mod_ids.is_empty()
        {
            warnings.insert(mod_id, warning);
        }
    }

    Ok(warnings)
}

fn push_unique(conflicts: &mut HashMap<ModId, Vec<ModId>>, mod_id: ModId, other_mod_id: ModId) {
    let other_mod_ids = conflicts.entry(mod_id).or_default();

    if !other_mod_ids.contains(&other_mod_id) {
        other_mod_ids.push(other_mod_id);
    }
}

#[cfg(test)]
#[path = "../../tests/unit/mod_sorter.rs"]
mod tests;

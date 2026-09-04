use std::collections::{BTreeSet, HashMap, HashSet};
use std::error::Error;
use std::fmt;

use crate::models::{ModCollection, ModId};

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
}

pub(crate) fn sort_mods(mods: &mut ModCollection) -> Result<(), SortError> {
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

    for (position, &mod_id) in mods.enabled_ids().iter().enumerate() {
        let rimworld_mod = &mods.all[mod_id.index()];

        // If this mod loads after X, the edge is X -> this mod.
        for package_id in &rimworld_mod.loader_after {
            if let Some(&required_position) = positions_by_id.get(package_id) {
                add_edge(
                    required_position,
                    position,
                    &mut outgoing,
                    &mut incoming_count,
                    &mut edges,
                );
            }
        }

        // If this mod loads before X, the edge is this mod -> X.
        for package_id in &rimworld_mod.loader_before {
            if let Some(&later_position) = positions_by_id.get(package_id) {
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

    // BTreeSet chooses the earliest currently valid mod. This preserves as
    // much of the user's existing order as possible.
    let mut ready = BTreeSet::new();

    for (position, &count) in incoming_count.iter().enumerate() {
        if count == 0 {
            ready.insert(position);
        }
    }

    let mut sorted_positions = Vec::with_capacity(active_count);

    while let Some(position) = ready.pop_first() {
        sorted_positions.push(position);

        for &later_position in &outgoing[position] {
            incoming_count[later_position] -= 1;

            if incoming_count[later_position] == 0 {
                ready.insert(later_position);
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

    Ok(())
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

    for (position, &mod_id) in mods.enabled_ids().iter().enumerate() {
        let rimworld_mod = &mods.all[mod_id.index()];
        let mut warning = OrderWarning::default();
        let mut has_violation = false;

        for package_id in &rimworld_mod.loader_after {
            let Some(&(required_position, required_mod_index)) =
                positions_by_id.get(package_id.as_str())
            else {
                // The referenced mod is not active, so it does not affect this order.
                continue;
            };

            warning.after_mod_ids.push(required_mod_index);

            // The required mod must appear before this mod.
            if required_position >= position {
                has_violation = true;
            }
        }

        for package_id in &rimworld_mod.loader_before {
            let Some(&(required_position, required_mod_index)) =
                positions_by_id.get(package_id.as_str())
            else {
                continue;
            };

            warning.before_mod_ids.push(required_mod_index);

            // The required mod must appear after this mod.
            if required_position <= position {
                has_violation = true;
            }
        }

        if has_violation {
            warnings.insert(mod_id, warning);
        }
    }

    Ok(warnings)
}

#[cfg(test)]
#[path = "../../tests/unit/mod_sorter.rs"]
mod tests;

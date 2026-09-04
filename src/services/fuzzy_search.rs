use nucleo_matcher::{
    Matcher, Utf32Str,
    pattern::{AtomKind, CaseMatching, Normalization, Pattern},
};
use std::cmp::Reverse;

use crate::models::{ModId, RimworldMod};

pub(crate) fn fuzzy_mod_indices(
    all_mods: &[RimworldMod],
    mod_ids: &[ModId],
    query: &str,
    matcher: &mut Matcher,
) -> Vec<usize> {
    if query.trim().is_empty() {
        return (0..mod_ids.len()).collect();
    }

    let pattern = Pattern::new(
        query,
        CaseMatching::Ignore,
        Normalization::Smart,
        AtomKind::Fuzzy,
    );

    let mut character_buffer = Vec::new();
    let mut matches: Vec<(usize, u32)> = Vec::new();

    for (list_position, &mod_id) in mod_ids.iter().enumerate() {
        character_buffer.clear();

        let rimworld_mod = &all_mods[mod_id.index()];

        let mod_name = Utf32Str::new(&rimworld_mod.name, &mut character_buffer);

        if let Some(score) = pattern.score(mod_name, matcher) {
            matches.push((list_position, score));
        }
    }

    matches.sort_unstable_by_key(|match_result| Reverse(match_result.1));

    matches
        .into_iter()
        .map(|(list_position, _score)| list_position)
        .collect()
}

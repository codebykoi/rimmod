use std::collections::BTreeMap;

use eframe::egui;

use crate::{models::ModCollection, services::organization::Organization, ui::theme};

/// Filter chips shown before the row starts summarizing the rest.
const MAX_TAG_FILTERS: usize = 10;

/// Long tag names are shortened on rows; the tooltip keeps the full name.
const MAX_ROW_TAG_CHARS: usize = 14;

/// Short label for a tag chip on a dense row. The tooltip and the organize
/// dialog always show the full tag.
pub(crate) fn short_tag(tag: &str) -> String {
    if tag.chars().count() <= MAX_ROW_TAG_CHARS {
        return tag.to_owned();
    }

    let mut shortened = tag.chars().take(MAX_ROW_TAG_CHARS - 1).collect::<String>();
    shortened.push('…');
    shortened
}

/// Tags on installed mods, with how many carry each one. Tags of deleted mods
/// stay in the organization file, but they should not show up as filters.
fn installed_tags(organization: &Organization, mods: &ModCollection) -> Vec<(String, usize)> {
    let mut counts: BTreeMap<String, (String, usize)> = BTreeMap::new();

    for rimworld_mod in &mods.all {
        for tag in organization.tags(&rimworld_mod.package_id) {
            counts
                .entry(tag.to_lowercase())
                .and_modify(|(_, count)| *count += 1)
                .or_insert_with(|| (tag.clone(), 1));
        }
    }

    counts.into_values().collect()
}

/// Active filters first so they can always be removed, then the most-used
/// tags. Keeps the filter row useful without turning it into a second list.
fn visible_filter_tags(
    all_tags: &[(String, usize)],
    filters: &[String],
) -> (Vec<(String, usize)>, usize) {
    let mut visible = Vec::new();

    for filter in filters {
        if let Some(tag) = all_tags
            .iter()
            .find(|(tag, _)| tag.eq_ignore_ascii_case(filter))
        {
            visible.push(tag.clone());
        }
    }

    let mut remaining = all_tags
        .iter()
        .filter(|(tag, _)| {
            !filters
                .iter()
                .any(|filter| filter.eq_ignore_ascii_case(tag))
        })
        .cloned()
        .collect::<Vec<_>>();

    remaining.sort_by(|left, right| {
        right
            .1
            .cmp(&left.1)
            .then_with(|| left.0.to_lowercase().cmp(&right.0.to_lowercase()))
    });

    let room = MAX_TAG_FILTERS.saturating_sub(visible.len());
    visible.extend(remaining.into_iter().take(room));

    (visible, all_tags.len())
}

/// Shared tag filter row above both mod columns.
pub(crate) fn show_mod_filters(
    ui: &mut egui::Ui,
    mods: &ModCollection,
    organization: &Organization,
    tag_filters: &mut Vec<String>,
) {
    // Filters can outlive the tag they reference if the last mod carrying it
    // was reorganized away. Drop those silently.
    let all_tags = installed_tags(organization, mods);
    tag_filters.retain(|filter| {
        all_tags
            .iter()
            .any(|(tag, _)| tag.eq_ignore_ascii_case(filter))
    });

    let (filter_tags, total_tags) = visible_filter_tags(&all_tags, tag_filters);

    if filter_tags.is_empty() {
        return;
    }

    ui.horizontal_wrapped(|ui| {
        for (tag, count) in &filter_tags {
            let active = tag_filters
                .iter()
                .any(|filter| filter.eq_ignore_ascii_case(tag));
            let label = if active {
                format!("{}  ×", short_tag(tag))
            } else {
                short_tag(tag)
            };
            let color = if active {
                theme::ACCENT
            } else {
                theme::TEXT_MUTED
            };
            let hover_color = if active { theme::DANGER } else { theme::ACCENT };

            let response = theme::chip_button(ui, label, color, hover_color);
            let clicked = response.clicked();

            if active {
                response.on_hover_text("Remove this tag filter");
            } else {
                response.on_hover_text(format!("Show only mods tagged \"{tag}\" ({count} mods)"));
            }

            if clicked {
                if active {
                    tag_filters.retain(|filter| !filter.eq_ignore_ascii_case(tag));
                } else {
                    tag_filters.push(tag.clone());
                }
            }
        }

        let hidden = total_tags.saturating_sub(filter_tags.len());

        if hidden > 0 || !tag_filters.is_empty() {
            let menu_label = if hidden > 0 {
                format!("All tags (+{hidden})")
            } else {
                "All tags".to_owned()
            };

            ui.menu_button(menu_label, |ui| {
                ui.set_min_width(180.0);

                for (tag, count) in &all_tags {
                    let mut active = tag_filters
                        .iter()
                        .any(|filter| filter.eq_ignore_ascii_case(tag));

                    if theme::checkbox(ui, &mut active, format!("{tag}  ({count})")).changed() {
                        if active {
                            tag_filters.push(tag.clone());
                        } else {
                            tag_filters.retain(|filter| !filter.eq_ignore_ascii_case(tag));
                        }
                    }
                }

                if !tag_filters.is_empty() {
                    theme::menu_separator(ui);

                    if ui.button("Clear filters").clicked() {
                        tag_filters.clear();
                        ui.close();
                    }
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::models::{ModId, ModSource, ModType, PackageId, RimworldMod};

    fn installed_mod(package_id: &str) -> RimworldMod {
        RimworldMod {
            name: package_id.to_owned(),
            package_id: PackageId::new(package_id).expect("valid package ID"),
            description: String::new(),
            supported_versions: Vec::new(),
            community_supported_versions: Vec::new(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            force_load_after: Vec::new(),
            force_load_before: Vec::new(),
            mod_dependencies: Vec::new(),
            incompatible_with: Vec::new(),
            inferred_after: Vec::new(),
            is_patch_only: false,
            folder: PathBuf::new(),
            source: ModSource::Local,
            mod_type: ModType::Xml,
        }
    }

    #[test]
    fn active_filters_come_first_then_the_most_used_tags() {
        let tags = [
            ("Big".to_owned(), 2),
            ("Fixes".to_owned(), 3),
            ("Slow".to_owned(), 1),
        ];

        let (visible, total) = visible_filter_tags(&tags, &["Slow".to_owned()]);

        assert_eq!(
            visible,
            [
                ("Slow".to_owned(), 1),
                ("Fixes".to_owned(), 3),
                ("Big".to_owned(), 2),
            ]
        );
        assert_eq!(total, 3);
    }

    #[test]
    fn filters_ignore_tags_of_uninstalled_mods() {
        let mut organization = Organization::default();
        let installed = PackageId::new("test.mod").expect("valid package ID");
        let uninstalled = PackageId::new("gone.mod").expect("valid package ID");

        organization.add_tag(&installed, "Fixes");
        organization.add_tag(&uninstalled, "Old");

        let mods = ModCollection::new(
            vec![installed_mod("test.mod")],
            vec![ModId::from_index(0)],
            Vec::new(),
            Vec::new(),
        );

        assert_eq!(
            installed_tags(&organization, &mods),
            [("Fixes".to_owned(), 1)]
        );
    }
}

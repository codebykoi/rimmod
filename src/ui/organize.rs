use eframe::egui::{self, Stroke, StrokeKind};

use crate::{
    models::PackageId,
    services::organization::{ModColor, Organization},
    ui::theme,
};

/// How many existing tags the add row offers as one-click suggestions.
const MAX_TAG_SUGGESTIONS: usize = 24;
const SWATCH_SIZE: f32 = 24.0;

pub(crate) struct OrganizeView<'a> {
    pub(crate) mod_names: &'a [String],
    pub(crate) package_ids: &'a [PackageId],
    pub(crate) tag_input: &'a mut String,
    pub(crate) organization: &'a mut Organization,
}

pub(crate) struct OrganizeOutcome {
    pub(crate) changed: bool,
    pub(crate) open: bool,
}

/// Editor for the tags and color of one or more mods. Changes apply
/// immediately; the caller persists them when `changed` is set.
pub(crate) fn show_organize_dialog(
    context: &egui::Context,
    view: OrganizeView<'_>,
) -> OrganizeOutcome {
    let OrganizeView {
        mod_names,
        package_ids,
        tag_input,
        organization,
    } = view;

    let mut changed = false;
    let mut done = false;

    let response = egui::Modal::new(egui::Id::new("organize_mods")).show(context, |ui| {
        ui.set_width(460.0);

        ui.heading("Organize");
        ui.add_space(4.0);

        if package_ids.is_empty() {
            theme::hint(ui, "No mods selected");
            return;
        }

        if package_ids.len() == 1 {
            ui.label(egui::RichText::new(&mod_names[0]).text_style(theme::strong_style()));
            ui.label(
                egui::RichText::new(package_ids[0].as_str())
                    .monospace()
                    .small()
                    .color(theme::TEXT_MUTED),
            );
        } else {
            theme::hint(
                ui,
                format!("Applying to {} selected mods", package_ids.len()),
            );
        }

        ui.add_space(4.0);
        theme::hint(
            ui,
            "Saved per package ID, so tags and colors survive reinstalling or deleting the mod.",
        );

        ui.add_space(12.0);
        section_label(ui, "COLOR");

        let colors = package_ids
            .iter()
            .map(|package_id| organization.color(package_id))
            .collect::<Vec<_>>();
        let shared_color = colors
            .first()
            .copied()
            .filter(|first| colors.iter().all(|color| color == first))
            .flatten();

        ui.horizontal(|ui| {
            for color in ModColor::ALL {
                let selected = shared_color == Some(color);
                let response = color_swatch(ui, Some(color), selected);
                let response = response.on_hover_text(color.label());

                if response.clicked() {
                    for package_id in package_ids {
                        changed |= organization.set_color(package_id, Some(color));
                    }
                }
            }

            let selected = shared_color.is_none() && colors.iter().all(Option::is_none);
            let response = color_swatch(ui, None, selected);
            let response = response.on_hover_text("No color");

            if response.clicked() {
                for package_id in package_ids {
                    changed |= organization.set_color(package_id, None);
                }
            }
        });

        ui.add_space(14.0);
        section_label(ui, "TAGS");

        let assigned = assigned_tags(organization, package_ids);

        if assigned.is_empty() {
            theme::hint(ui, "No tags yet");
        } else {
            ui.horizontal_wrapped(|ui| {
                for (tag, count) in &assigned {
                    let response = theme::chip_button(
                        ui,
                        format!("{tag}  ×"),
                        theme::TEXT_MUTED,
                        theme::DANGER,
                    );

                    if response.clicked() {
                        for package_id in package_ids {
                            changed |= organization.remove_tag(package_id, tag);
                        }
                    }

                    response.on_hover_text(if *count == package_ids.len() {
                        format!("Remove \"{tag}\" from these mods")
                    } else {
                        format!("Remove \"{tag}\" ({count} of {} mods)", package_ids.len())
                    });
                }

                let response =
                    theme::chip_button(ui, "Clear all", theme::TEXT_FAINT, theme::DANGER);

                if response.clicked() {
                    for package_id in package_ids {
                        changed |= organization.clear_tags(package_id);
                    }
                }

                response.on_hover_text("Remove every tag from these mods");
            });
        }

        ui.add_space(6.0);

        ui.horizontal(|ui| {
            let add_width = (ui.available_width() - 64.0).max(120.0);
            let response = ui.add_sized(
                [add_width, theme::CONTROL_HEIGHT],
                egui::TextEdit::singleline(tag_input).hint_text("Add a tag"),
            );

            let submitted =
                response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
            let add_clicked =
                theme::secondary_button_sized(ui, "Add", egui::vec2(56.0, theme::CONTROL_HEIGHT))
                    .clicked();

            if submitted || add_clicked {
                if !tag_input.trim().is_empty() {
                    for package_id in package_ids {
                        changed |= organization.add_tag(package_id, tag_input.as_str());
                    }

                    tag_input.clear();
                }

                response.request_focus();
            }
        });

        // Tags already used elsewhere make a good vocabulary: offer them
        // instead of forcing the user to retype a tag they have used before.
        let suggestions = organization
            .all_tags()
            .into_iter()
            .map(|(tag, _)| tag)
            .filter(|tag| {
                !package_ids.iter().all(|package_id| {
                    organization
                        .tags(package_id)
                        .iter()
                        .any(|assigned| assigned.eq_ignore_ascii_case(tag))
                })
            })
            .take(MAX_TAG_SUGGESTIONS)
            .collect::<Vec<_>>();

        if !suggestions.is_empty() {
            ui.add_space(8.0);
            theme::hint(ui, "Existing tags");

            ui.horizontal_wrapped(|ui| {
                for tag in suggestions {
                    if theme::chip_button(ui, tag.clone(), theme::TEXT_FAINT, theme::ACCENT)
                        .clicked()
                    {
                        for package_id in package_ids {
                            changed |= organization.add_tag(package_id, &tag);
                        }
                    }
                }
            });
        }

        ui.add_space(16.0);

        ui.horizontal(|ui| {
            if theme::primary_button(ui, "Done").clicked() {
                done = true;
            }

            theme::hint(ui, "Changes save automatically");
        });
    });

    OrganizeOutcome {
        changed,
        open: !(done || response.should_close()),
    }
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .font(theme::semibold_font(11.0))
            .color(theme::TEXT_FAINT),
    );
    ui.add_space(4.0);
}

fn color_swatch(ui: &mut egui::Ui, color: Option<ModColor>, selected: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::Vec2::splat(SWATCH_SIZE), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let fill = color.map(theme::mod_color).unwrap_or(theme::CONTROL);

        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::RADIUS_SM), fill);

        if color.is_none() {
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "×",
                theme::medium_font(13.0),
                theme::TEXT_MUTED,
            );
        }

        if selected {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                egui::CornerRadius::same(theme::RADIUS_SM),
                Stroke::new(1.0, theme::TEXT),
                StrokeKind::Outside,
            );
        } else if response.hovered() {
            ui.painter().rect_stroke(
                rect.expand(2.0),
                egui::CornerRadius::same(theme::RADIUS_SM),
                Stroke::new(1.0, theme::ACCENT),
                StrokeKind::Outside,
            );
        }
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Tags on any of the selected mods, with how many carry each one.
fn assigned_tags(organization: &Organization, package_ids: &[PackageId]) -> Vec<(String, usize)> {
    let mut assigned: Vec<(String, usize)> = Vec::new();

    for package_id in package_ids {
        for tag in organization.tags(package_id) {
            if let Some((_, count)) = assigned
                .iter_mut()
                .find(|(assigned, _)| assigned.eq_ignore_ascii_case(tag))
            {
                *count += 1;
            } else {
                assigned.push((tag.clone(), 1));
            }
        }
    }

    assigned.sort_by_key(|(tag, _)| tag.to_lowercase());

    assigned
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialog_renders_an_already_tagged_and_colored_mod() {
        let mut organization = Organization::default();
        let package = PackageId::new("test.mod").expect("valid package ID");

        organization.add_tag(&package, "Fixes");
        organization.set_color(&package, Some(ModColor::Teal));

        let mod_names = vec!["Test mod".to_owned()];
        let package_ids = vec![package];
        let mut tag_input = String::new();
        let mut changed = true;
        let mut open = false;

        let context = egui::Context::default();
        theme::install(&context);
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..egui::RawInput::default()
        };

        context
            .run_ui(input, |ui| {
                let outcome = show_organize_dialog(
                    ui.ctx(),
                    OrganizeView {
                        mod_names: &mod_names,
                        package_ids: &package_ids,
                        tag_input: &mut tag_input,
                        organization: &mut organization,
                    },
                );

                changed = outcome.changed;
                open = outcome.open;
            })
            .drop_without_applying_deltas();

        assert!(!changed);
        assert!(open);
    }
}

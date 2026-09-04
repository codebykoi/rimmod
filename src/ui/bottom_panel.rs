use eframe::egui;

use crate::app::LoadOrderStatus;
use crate::services::mod_loader::ModLoadWarning;

pub(crate) enum BottomPanelAction {
    DismissError,
    Save,
    Run,
    Sort,
}

pub(crate) fn show_bottom_panel(
    ui: &mut egui::Ui,
    game_version: Option<&str>,
    load_order_status: &LoadOrderStatus,
    mod_load_warnings: &[ModLoadWarning],
    action_error: Option<&str>,
) -> Option<BottomPanelAction> {
    let mut requested_action = None;

    let version = game_version.unwrap_or("Unknown");

    egui::Panel::bottom("bottom_bar")
        .show_separator_line(true)
        .show(ui, |ui| {
            if let LoadOrderStatus::Unavailable { reason } = load_order_status {
                let error_color = ui.visuals().error_fg_color;

                egui::Frame::new()
                    .fill(error_color.gamma_multiply(0.12))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.label(
                            egui::RichText::new("Load order unavailable: saving is disabled")
                                .strong()
                                .color(error_color),
                        );

                        ui.label(reason);
                    });

                ui.add_space(4.0);
            }

            if let Some(error) = action_error {
                let error_color = ui.visuals().error_fg_color;

                egui::Frame::new()
                    .fill(error_color.gamma_multiply(0.12))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        egui::containers::Sides::new().show(
                            ui,
                            |ui| {
                                ui.label(egui::RichText::new(error).strong().color(error_color));
                            },
                            |ui| {
                                if ui.small_button("Dismiss").clicked() {
                                    requested_action = Some(BottomPanelAction::DismissError);
                                }
                            },
                        );
                    });

                ui.add_space(4.0);
            }

            if !mod_load_warnings.is_empty() {
                let warning_color = ui.visuals().warn_fg_color;
                let skipped_mods = mod_load_warnings
                    .iter()
                    .filter(|warning| warning.is_skipped_mod())
                    .count();
                let compatibility_data_issues = mod_load_warnings
                    .iter()
                    .filter(|warning| warning.is_compatibility_data())
                    .count();
                let unavailable_directories =
                    mod_load_warnings.len() - skipped_mods - compatibility_data_issues;
                let mut warning_summary = Vec::new();

                if skipped_mods > 0 {
                    warning_summary.push(format!("{skipped_mods} invalid mod(s) skipped"));
                }
                if unavailable_directories > 0 {
                    warning_summary.push(format!(
                        "{unavailable_directories} optional mod directory issue(s)"
                    ));
                }
                if compatibility_data_issues > 0 {
                    warning_summary.push(format!(
                        "{compatibility_data_issues} compatibility data issue(s)"
                    ));
                }

                egui::Frame::new()
                    .fill(warning_color.gamma_multiply(0.12))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());

                        ui.label(
                            egui::RichText::new("Mods loaded with warnings")
                                .strong()
                                .color(warning_color),
                        );

                        ui.label(format!("{}.", warning_summary.join("; ")));

                        egui::CollapsingHeader::new("Warning details").show(ui, |ui| {
                            for warning in mod_load_warnings {
                                ui.label(format!("- {warning}"));
                            }
                        });
                    });

                ui.add_space(4.0);
            }

            egui::containers::Sides::new().height(28.0).show(
                ui,
                // Left side
                |ui| {
                    ui.label(format!("RimWorld version {version}"));
                },
                // Right side
                |ui| {
                    // Sides places right-hand widgets from right to left,
                    // so these appear visually in reverse order.
                    if ui
                        .add_sized([90.0, 24.0], egui::Button::new("Run"))
                        .clicked()
                    {
                        requested_action = Some(BottomPanelAction::Run);
                    }

                    let disabled_reason = match load_order_status {
                        LoadOrderStatus::Unavailable { reason } => reason.as_str(),
                        LoadOrderStatus::Ready => "",
                    };

                    let save_response = ui
                        .add_enabled(
                            load_order_status.can_save(),
                            egui::Button::new("Save").min_size(egui::vec2(90.0, 24.0)),
                        )
                        .on_disabled_hover_text(disabled_reason);

                    if save_response.clicked() {
                        requested_action = Some(BottomPanelAction::Save);
                    }

                    if ui
                        .add_sized([90.0, 24.0], egui::Button::new("Sort"))
                        .clicked()
                    {
                        requested_action = Some(BottomPanelAction::Sort);
                    }
                },
            );
        });

    requested_action
}

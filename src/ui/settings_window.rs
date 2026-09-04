use eframe::egui;
use std::path::PathBuf;

use crate::{app::UiState, ui::OpenTarget};

pub(crate) enum SettingsAction {
    Apply,
    Cancel,
    Open(OpenTarget),
}

fn show_path_setting(
    ui: &mut egui::Ui,
    title: &str,
    path: &mut String,
    error_text: Option<&str>,
) -> Option<OpenTarget> {
    let mut requested_open = None;

    ui.group(|ui| {
        egui::containers::Sides::new().height(24.0).show(
            ui,
            |ui| {
                ui.label(egui::RichText::new(title).strong());
            },
            |ui| {
                // Sides places right-side widgets from right to left.
                if ui.button("Clear...").clicked() {
                    path.clear();
                }

                if ui.button("Choose...").clicked() {
                    let mut dialog = rfd::FileDialog::new().set_title(format!("Choose {title}"));

                    if !path.trim().is_empty() {
                        dialog = dialog.set_directory(path.as_str());
                    }

                    if let Some(selected_folder) = dialog.pick_folder() {
                        *path = selected_folder.to_string_lossy().into_owned();
                    }
                }

                let can_open = !path.trim().is_empty();

                if ui
                    .add_enabled(can_open, egui::Button::new("Open..."))
                    .clicked()
                {
                    requested_open = Some(OpenTarget::Folder(PathBuf::from(path.trim())));
                }
            },
        );

        ui.add(egui::TextEdit::singleline(path).desired_width(f32::INFINITY));

        if let Some(error_text) = error_text {
            ui.colored_label(ui.visuals().error_fg_color, error_text);
        }
    });

    requested_open
}

pub(crate) fn display_settings(ui: &mut egui::Ui, state: &mut UiState) -> Option<SettingsAction> {
    let mut requested_action = None;
    let mut window_open = state.settings_open;

    egui::Window::new("Settings")
        .collapsible(false)
        .open(&mut window_open)
        .default_width(700.0)
        .show(ui.ctx(), |ui| {
            if let Some(error) = state.settings_errors.general.as_deref() {
                let error_color = ui.visuals().error_fg_color;

                egui::Frame::new()
                    .fill(error_color.gamma_multiply(0.12))
                    .inner_margin(egui::Margin::symmetric(10, 6))
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        ui.colored_label(error_color, error);
                    });

                ui.add_space(10.0);
            }

            if let Some(open_target) = show_path_setting(
                ui,
                "Game location",
                &mut state.rimworld_path_input,
                state.settings_errors.rimworld.as_deref(),
            ) {
                requested_action = Some(SettingsAction::Open(open_target));
            }

            ui.add_space(10.0);

            if let Some(open_target) = show_path_setting(
                ui,
                "Workshop location (optional)",
                &mut state.workshop_path_input,
                state.settings_errors.workshop.as_deref(),
            ) {
                requested_action = Some(SettingsAction::Open(open_target));
            }

            ui.add_space(10.0);

            if let Some(open_target) = show_path_setting(
                ui,
                "Config location",
                &mut state.config_path_input,
                state.settings_errors.config.as_deref(),
            ) {
                requested_action = Some(SettingsAction::Open(open_target));
            }

            ui.add_space(10.0);

            egui::containers::Sides::new().height(28.0).show(
                ui,
                |_| {},
                |ui| {
                    // Sides places right-side widgets from right to left.
                    if ui.button("Apply").clicked() {
                        requested_action = Some(SettingsAction::Apply);
                    }

                    if ui.button("Cancel").clicked() {
                        requested_action = Some(SettingsAction::Cancel);
                    }
                },
            );
        });

    if !window_open {
        Some(SettingsAction::Cancel)
    } else {
        requested_action
    }
}

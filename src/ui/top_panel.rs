use eframe::egui;

pub(crate) const UNDO_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
pub(crate) const REDO_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
pub(crate) const ALTERNATE_REDO_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    egui::Key::Z,
);

pub(crate) enum TopPanelAction {
    Undo,
    Redo,
    Settings,
    RunWithCurrentMods,
}

pub(crate) fn show_top_panel(
    ui: &mut egui::Ui,
    can_undo: bool,
    can_redo: bool,
    can_run_with_mods: bool,
) -> Option<TopPanelAction> {
    let mut request = None;
    egui::Panel::top("menu_bar")
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(crate::ui::theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(10, 3)),
        )
        .show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Settings").clicked() {
                        request = Some(TopPanelAction::Settings);
                        ui.close();
                    }

                    let run_with_mods = ui.add_enabled(
                        can_run_with_mods,
                        egui::Button::new("Run with current mod list"),
                    );

                    if run_with_mods
                        .on_disabled_hover_text("Requires RimWorld 1.7 or newer")
                        .clicked()
                    {
                        request = Some(TopPanelAction::RunWithCurrentMods);
                        ui.close();
                    }

                    ui.separator();

                    if ui.button("Exit").clicked() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });

                ui.menu_button("Edit", |ui| {
                    let undo_button = egui::Button::new("Undo")
                        .shortcut_text(ui.ctx().format_shortcut(&UNDO_SHORTCUT));

                    if ui.add_enabled(can_undo, undo_button).clicked() {
                        request = Some(TopPanelAction::Undo);
                        ui.close();
                    }

                    let redo_button = egui::Button::new("Redo")
                        .shortcut_text(ui.ctx().format_shortcut(&REDO_SHORTCUT));

                    if ui.add_enabled(can_redo, redo_button).clicked() {
                        request = Some(TopPanelAction::Redo);
                        ui.close();
                    }
                });
            });
        });

    request
}

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
}

pub(crate) fn show_top_panel(
    ui: &mut egui::Ui,
    can_undo: bool,
    can_redo: bool,
) -> Option<TopPanelAction> {
    let mut request = None;
    egui::Panel::top("menu_bar").show(ui, |ui| {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("Settings").clicked() {
                    request = Some(TopPanelAction::Settings);
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

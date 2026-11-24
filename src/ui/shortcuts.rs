use eframe::egui;

use crate::ui::theme;

struct Shortcut {
    keys: &'static str,
    action: &'static str,
}

const SHORTCUTS: &[Shortcut] = &[
    Shortcut {
        keys: "Ctrl+F  /  /",
        action: "Focus the mod search",
    },
    Shortcut {
        keys: "Ctrl+S",
        action: "Save the load order",
    },
    Shortcut {
        keys: "F5",
        action: "Reload mods from disk",
    },
    Shortcut {
        keys: "Ctrl+Z",
        action: "Undo the last load-order change",
    },
    Shortcut {
        keys: "Ctrl+Y  /  Ctrl+Shift+Z",
        action: "Redo",
    },
    Shortcut {
        keys: "Ctrl+1 … Ctrl+4",
        action: "Switch tab",
    },
    Shortcut {
        keys: "Esc",
        action: "Clear search, then the selection",
    },
    Shortcut {
        keys: "?",
        action: "Show this list",
    },
];

const MOUSE: &[Shortcut] = &[
    Shortcut {
        keys: "Double-click",
        action: "Enable or disable a mod",
    },
    Shortcut {
        keys: "Drag",
        action: "Reorder, or move between columns",
    },
    Shortcut {
        keys: "Ctrl+click",
        action: "Add to the selection",
    },
    Shortcut {
        keys: "Shift+click",
        action: "Select a range",
    },
    Shortcut {
        keys: "Right-click",
        action: "Open mod actions",
    },
];

pub(crate) fn show_shortcuts(context: &egui::Context, open: &mut bool) {
    if !*open {
        return;
    }

    let response = egui::Modal::new(egui::Id::new("keyboard_shortcuts")).show(context, |ui| {
        ui.set_width(460.0);

        ui.heading("Keyboard shortcuts");
        ui.add_space(4.0);
        theme::hint(ui, "Most actions also work from the mod row context menu.");

        ui.add_space(12.0);
        shortcut_grid(ui, "keyboard_shortcuts", SHORTCUTS);

        ui.add_space(16.0);
        ui.label(
            egui::RichText::new("MOUSE")
                .font(theme::semibold_font(11.0))
                .color(theme::TEXT_FAINT),
        );
        ui.add_space(4.0);
        shortcut_grid(ui, "mouse_shortcuts", MOUSE);

        ui.add_space(16.0);

        if theme::primary_button(ui, "Done").clicked() {
            *open = false;
        }
    });

    if response.should_close() {
        *open = false;
    }
}

fn shortcut_grid(ui: &mut egui::Ui, id_salt: &str, shortcuts: &[Shortcut]) {
    egui::Grid::new(id_salt)
        .num_columns(2)
        .spacing([24.0, 8.0])
        .show(ui, |ui| {
            for shortcut in shortcuts {
                ui.label(
                    egui::RichText::new(shortcut.keys)
                        .monospace()
                        .small()
                        .color(theme::TEXT),
                );
                ui.label(
                    egui::RichText::new(shortcut.action)
                        .small()
                        .color(theme::TEXT_MUTED),
                );
                ui.end_row();
            }
        });
}

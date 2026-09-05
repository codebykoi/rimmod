use std::path::Path;

use eframe::egui;

use crate::services::workshop::{WorkshopItem, WorkshopSort, is_item_installed};
use crate::ui::OpenTarget;

pub(crate) const ITEMS_PER_PAGE: u64 = 30;
const PREVIEW_SIZE: egui::Vec2 = egui::vec2(144.0, 96.0);

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum WorkshopLoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error(String),
}

pub(crate) struct WorkshopView<'a> {
    pub(crate) query: &'a mut String,
    pub(crate) sort: &'a mut WorkshopSort,
    pub(crate) page: u32,
    pub(crate) total: u64,
    pub(crate) items: &'a [WorkshopItem],
    pub(crate) load_status: &'a WorkshopLoadStatus,
    pub(crate) installing_item: Option<u64>,
    pub(crate) workshop_path: Option<&'a Path>,
    pub(crate) steamcmd_workshop_path: Option<&'a Path>,
    pub(crate) api_key_configured: bool,
}

pub(crate) enum WorkshopAction {
    Search,
    GoToPage(u32),
    Install(u64),
    Open(OpenTarget),
    OpenSettings,
}

pub(crate) fn show_workshop(ui: &mut egui::Ui, view: WorkshopView<'_>) -> Option<WorkshopAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        let search_response = ui.add(
            egui::TextEdit::singleline(view.query)
                .hint_text("Search the RimWorld Workshop")
                .desired_width(360.0),
        );

        egui::ComboBox::from_id_salt("workshop_sort")
            .selected_text(view.sort.label())
            .show_ui(ui, |ui| {
                for sort in WorkshopSort::ALL {
                    ui.selectable_value(view.sort, sort, sort.label());
                }
            });

        let enter_pressed =
            search_response.lost_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));

        if ui.button("Search").clicked() || enter_pressed {
            action = Some(WorkshopAction::Search);
        }

        if matches!(view.load_status, WorkshopLoadStatus::Loading) {
            ui.add(egui::Spinner::new());
        }
    });

    ui.add_space(6.0);

    if !view.api_key_configured {
        egui::Frame::new()
            .fill(ui.visuals().warn_fg_color.gamma_multiply(0.12))
            .inner_margin(egui::Margin::symmetric(10, 8))
            .show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("A Steam Web API key is required to search the Workshop catalog.");
                    if ui.button("Open Settings").clicked() {
                        action = Some(WorkshopAction::OpenSettings);
                    }
                });
            });
        ui.add_space(6.0);
    }

    if let WorkshopLoadStatus::Error(message) = view.load_status {
        ui.colored_label(ui.visuals().error_fg_color, message);
        ui.add_space(6.0);
    }

    if matches!(view.load_status, WorkshopLoadStatus::Idle) && view.items.is_empty() {
        ui.centered_and_justified(|ui| {
            ui.label("Search for mods or choose a ranking to browse the Workshop.");
        });
        return action;
    }

    let page_count = view.total.div_ceil(ITEMS_PER_PAGE).max(1);
    ui.horizontal(|ui| {
        let previous = ui.add_enabled(view.page > 1, egui::Button::new("Previous"));
        if previous.clicked() {
            action = Some(WorkshopAction::GoToPage(view.page - 1));
        }

        ui.label(format!(
            "Page {} of {} · {} items",
            view.page, page_count, view.total
        ));

        let next = ui.add_enabled(u64::from(view.page) < page_count, egui::Button::new("Next"));
        if next.clicked() {
            action = Some(WorkshopAction::GoToPage(view.page + 1));
        }
    });

    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for item in view.items {
            let installed = is_item_installed(
                view.workshop_path,
                view.steamcmd_workshop_path,
                item.published_file_id,
            );
            let is_installing = view.installing_item == Some(item.published_file_id);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    if let Some(preview_url) = item.preview_url.as_deref() {
                        let preview = egui::Image::new(preview_url)
                            .fit_to_exact_size(PREVIEW_SIZE)
                            .corner_radius(4)
                            .show_loading_spinner(true)
                            .alt_text(format!("Preview image for {}", item.title))
                            .sense(egui::Sense::click());

                        if ui.add(preview).on_hover_text("Open Steam page").clicked() {
                            action = Some(WorkshopAction::Open(OpenTarget::Url(item.page_url())));
                        }
                    } else {
                        let (preview_rect, _) =
                            ui.allocate_exact_size(PREVIEW_SIZE, egui::Sense::hover());
                        ui.painter()
                            .rect_filled(preview_rect, 4, ui.visuals().faint_bg_color);
                        ui.painter().text(
                            preview_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "No preview",
                            egui::FontId::default(),
                            ui.visuals().weak_text_color(),
                        );
                    }

                    ui.vertical(|ui| {
                        ui.set_width(ui.available_width());

                        ui.horizontal(|ui| {
                            ui.vertical(|ui| {
                                let title = egui::RichText::new(&item.title).strong().size(16.0);
                                if ui
                                    .add(egui::Link::new(title))
                                    .on_hover_text("Open Steam Workshop page")
                                    .clicked()
                                {
                                    action = Some(WorkshopAction::Open(OpenTarget::Url(
                                        item.page_url(),
                                    )));
                                }
                                ui.small(format!("Workshop item {}", item.published_file_id));
                            });

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    let button_text = install_button_text(installed, is_installing);

                                    let mut button = ui.add_enabled(
                                        view.installing_item.is_none(),
                                        egui::Button::new(button_text),
                                    );
                                    if installed {
                                        button = button.on_hover_text(
                                            "Download this item again with SteamCMD",
                                        );
                                    }

                                    if button.clicked() {
                                        action =
                                            Some(WorkshopAction::Install(item.published_file_id));
                                    }

                                    if installed {
                                        ui.colored_label(egui::Color32::LIGHT_GREEN, "Installed");
                                    }
                                },
                            );
                        });

                        if !item.description.trim().is_empty() {
                            ui.add_space(4.0);
                            ui.label(short_description(&item.description));
                        }

                        if let Some(subscriptions) = item.subscriptions {
                            ui.small(format!("{subscriptions} subscribers"));
                        }
                    });
                });
            });

            ui.add_space(6.0);
        }
    });

    action
}

fn short_description(description: &str) -> String {
    const MAX_CHARACTERS: usize = 300;
    let mut characters = description.chars();
    let shortened = characters.by_ref().take(MAX_CHARACTERS).collect::<String>();

    if characters.next().is_some() {
        format!("{shortened}…")
    } else {
        shortened
    }
}

fn install_button_text(installed: bool, is_installing: bool) -> &'static str {
    match (installed, is_installing) {
        (true, true) => "Reinstalling...",
        (true, false) => "Reinstall",
        (false, true) => "Installing...",
        (false, false) => "Install",
    }
}

#[cfg(test)]
mod tests {
    use super::{install_button_text, short_description};

    #[test]
    fn description_shortening_respects_utf8_characters() {
        let description = "🙂".repeat(301);
        let shortened = short_description(&description);

        assert_eq!(shortened.chars().count(), 301);
        assert!(shortened.ends_with('…'));
    }

    #[test]
    fn installed_items_offer_reinstall_instead_of_update() {
        assert_eq!(install_button_text(true, false), "Reinstall");
        assert_eq!(install_button_text(true, true), "Reinstalling...");
        assert_eq!(install_button_text(false, false), "Install");
        assert_eq!(install_button_text(false, true), "Installing...");
    }
}

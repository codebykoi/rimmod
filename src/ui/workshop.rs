use std::collections::HashSet;
use std::path::Path;

use eframe::egui;

use crate::models::{ModCollection, ModSource, RimworldMod};
use crate::services::no_version_warning::CoveredMods;
use crate::services::workshop::{WorkshopItem, WorkshopSort, is_item_installed};
use crate::ui::OpenTarget;
use crate::ui::mod_info::show_supported_versions;

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

/// Display state of the background No Version Warning data fetch.
pub(crate) enum CoveredModsState<'a> {
    Loading,
    Ready(&'a CoveredMods),
    Failed(String),
}

impl CoveredModsState<'_> {
    fn ready_mods(&self) -> Option<&CoveredMods> {
        match self {
            Self::Ready(covered_mods) => Some(covered_mods),
            _ => None,
        }
    }
}

pub(crate) struct WorkshopView<'a> {
    pub(crate) query: &'a mut String,
    pub(crate) sort: &'a mut WorkshopSort,
    pub(crate) page: u32,
    pub(crate) total: u64,
    pub(crate) items: &'a [WorkshopItem],
    pub(crate) load_status: &'a WorkshopLoadStatus,
    pub(crate) installing_items: &'a [u64],
    pub(crate) selected_items: &'a HashSet<u64>,
    pub(crate) workshop_path: Option<&'a Path>,
    pub(crate) steamcmd_workshop_path: Option<&'a Path>,
    pub(crate) api_key_configured: bool,
    pub(crate) mods: &'a ModCollection,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) covered_mods_state: CoveredModsState<'a>,
}

pub(crate) enum WorkshopAction {
    Search,
    GoToPage(u32),
    Install(Vec<u64>),
    ToggleSelect(u64),
    ClearSelection,
    Open(OpenTarget),
    OpenSettings,
    RetryCoveredMods,
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

    match &view.covered_mods_state {
        CoveredModsState::Loading => {
            ui.small("Loading No Version Warning compatibility data...");
        }
        CoveredModsState::Failed(error) => {
            ui.horizontal_wrapped(|ui| {
                ui.small(format!("No Version Warning data unavailable: {error}"));
                if ui.small_button("Retry").clicked() {
                    action = Some(WorkshopAction::RetryCoveredMods);
                }
            });
        }
        CoveredModsState::Ready(_) => {}
    }

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

    if !view.items.is_empty() {
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let download_button = ui.add_enabled(
                !view.selected_items.is_empty() && view.installing_items.is_empty(),
                egui::Button::new(format!("Download selected ({})", view.selected_items.len())),
            );
            if download_button.clicked() {
                let mut published_file_ids: Vec<u64> =
                    view.selected_items.iter().copied().collect();
                published_file_ids.sort_unstable();
                action = Some(WorkshopAction::Install(published_file_ids));
            }

            if !view.selected_items.is_empty() && ui.button("Clear selection").clicked() {
                action = Some(WorkshopAction::ClearSelection);
            }

            if !view.installing_items.is_empty() {
                ui.small(format!(
                    "Downloading {} item(s) with SteamCMD...",
                    view.installing_items.len()
                ));
            }
        });
    }

    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        for item in view.items {
            let installed = is_item_installed(
                view.workshop_path,
                view.steamcmd_workshop_path,
                item.published_file_id,
            );
            let installed_mod = installed_workshop_mod(view.mods, item.published_file_id);
            let mut is_selected = view.selected_items.contains(&item.published_file_id);

            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_width(ui.available_width());

                ui.horizontal(|ui| {
                    if ui.checkbox(&mut is_selected, "").changed() {
                        action = Some(WorkshopAction::ToggleSelect(item.published_file_id));
                    }

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

                            if installed {
                                ui.colored_label(egui::Color32::LIGHT_GREEN, "Installed");
                            }
                        });

                        if !item.description.trim().is_empty() {
                            ui.add_space(4.0);
                            ui.label(short_description(&item.description));
                        }

                        if let Some(subscriptions) = item.subscriptions {
                            ui.small(format!("{subscriptions} subscribers"));
                        }

                        // Installed mods report their versions through About.xml,
                        // which also carries No Version Warning community reports.
                        // Un-installed items fall back to Steam tags plus the
                        // No Version Warning "covered mods" list.
                        let (supported_versions, community_versions) = match installed_mod {
                            Some(rimworld_mod) => (
                                rimworld_mod.supported_versions.as_slice(),
                                rimworld_mod.community_supported_versions.as_slice(),
                            ),
                            None => {
                                let covered = view
                                    .covered_mods_state
                                    .ready_mods()
                                    .and_then(|covered_mods| {
                                        covered_mods.get(&item.published_file_id)
                                    })
                                    .map(|versions| versions.as_slice())
                                    .unwrap_or(&[]);

                                (item.supported_versions.as_slice(), covered)
                            }
                        };

                        if !supported_versions.is_empty() || !community_versions.is_empty() {
                            show_supported_versions(
                                ui,
                                supported_versions,
                                community_versions,
                                view.game_version,
                            );
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

fn installed_workshop_mod(mods: &ModCollection, published_file_id: u64) -> Option<&RimworldMod> {
    mods.all.iter().find(|rimworld_mod| {
        matches!(
            rimworld_mod.source,
            ModSource::SteamWorkshop { workshop_id } if workshop_id == published_file_id
        )
    })
}

#[cfg(test)]
mod tests {
    use super::short_description;
    use super::*;
    use crate::models::{ModId, ModType, PackageId};
    use std::path::PathBuf;

    #[test]
    fn description_shortening_respects_utf8_characters() {
        let description = "🙂".repeat(301);
        let shortened = short_description(&description);

        assert_eq!(shortened.chars().count(), 301);
        assert!(shortened.ends_with('…'));
    }

    fn workshop_mod(package: &str, workshop_id: u64) -> RimworldMod {
        RimworldMod {
            name: package.to_owned(),
            package_id: PackageId::new(package).expect("valid package ID"),
            description: String::new(),
            supported_versions: vec!["1.5".to_owned()],
            community_supported_versions: Vec::new(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            folder: PathBuf::new(),
            source: ModSource::SteamWorkshop { workshop_id },
            mod_type: ModType::Xml,
        }
    }

    #[test]
    fn installed_mod_lookup_matches_the_workshop_id() {
        let local = RimworldMod {
            source: ModSource::Local,
            ..workshop_mod("example.local", 0)
        };
        let workshop = workshop_mod("example.workshop", 1234);
        let mods = ModCollection::new(
            vec![local, workshop],
            vec![ModId::from_index(0), ModId::from_index(1)],
            Vec::new(),
            Vec::new(),
        );

        let found = installed_workshop_mod(&mods, 1234).expect("workshop mod");
        assert_eq!(found.package_id.as_str(), "example.workshop");

        assert!(installed_workshop_mod(&mods, 9999).is_none());
    }
}

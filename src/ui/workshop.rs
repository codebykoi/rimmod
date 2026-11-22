use std::collections::HashSet;
use std::path::Path;

use eframe::egui::{self, Color32, CornerRadius, Margin, RichText, Sense, Vec2};

use crate::models::{
    ModCollection, ModSource, RimworldMod, highest_supported_version, major_minor_version_text,
    versions_are_compatible,
};
use crate::services::no_version_warning::CoveredMods;
use crate::services::workshop::{
    RIMWORLD_DLC, RIMWORLD_VERSIONS, SubscribedSort, WorkshopItem, WorkshopSort, format_count,
    format_date, is_item_installed, strip_bbcode,
};
use crate::ui::OpenTarget;
use crate::ui::mod_info::show_supported_versions;
use crate::ui::theme;

pub(crate) const ITEMS_PER_PAGE: u64 = 30;
const MIN_CARD_WIDTH: f32 = 192.0;
const CARD_GAP: f32 = 12.0;
const SIDEBAR_WIDTH: f32 = 212.0;
const CARD_IMAGE_ASPECT: f32 = 0.56;
const CARD_TITLE_HEIGHT: f32 = 18.0;
const CARD_AUTHOR_HEIGHT: f32 = 16.0;
const CARD_META_HEIGHT: f32 = 18.0;
const CARD_VERSION_HEIGHT: f32 = 20.0;
const CARD_ACTION_HEIGHT: f32 = 26.0;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) enum WorkshopLoadStatus {
    #[default]
    Idle,
    Loading,
    Ready,
    Error(String),
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) enum WorkshopTab {
    #[default]
    Browse,
    Subscribed,
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
    pub(crate) tab: WorkshopTab,
    pub(crate) query: &'a mut String,
    pub(crate) subscribed_query: &'a mut String,
    pub(crate) sort: &'a mut WorkshopSort,
    pub(crate) subscribed_sort: &'a mut SubscribedSort,
    pub(crate) versions: &'a mut Vec<String>,
    pub(crate) dlc: &'a mut Vec<String>,
    pub(crate) my_version_only: &'a mut bool,
    pub(crate) page: u32,
    pub(crate) total: u64,
    pub(crate) items: &'a [WorkshopItem],
    pub(crate) load_status: &'a WorkshopLoadStatus,
    pub(crate) installing_items: &'a [u64],
    pub(crate) workshop_path: Option<&'a Path>,
    pub(crate) steamcmd_workshop_path: Option<&'a Path>,
    pub(crate) steamcmd_available: bool,
    pub(crate) api_key_configured: bool,
    pub(crate) mods: &'a ModCollection,
    pub(crate) game_version: Option<&'a str>,
    pub(crate) covered_mods_state: CoveredModsState<'a>,
    pub(crate) subscribed_items: &'a [WorkshopItem],
    pub(crate) subscribed_load_status: &'a WorkshopLoadStatus,
    pub(crate) subscribed_ids: Option<&'a HashSet<u64>>,
    pub(crate) subscriptions_loading: bool,
    pub(crate) subscriptions_error: Option<&'a str>,
    pub(crate) pending_subscriptions: &'a HashSet<u64>,
    pub(crate) subscribed_visible: usize,
    pub(crate) grid_generation: u64,
    pub(crate) subscribed_generation: u64,
    pub(crate) detail: Option<&'a WorkshopItem>,
    pub(crate) detail_image: Option<usize>,
}

pub(crate) enum WorkshopAction {
    Search,
    GoToPage(u32),
    SwitchTab(WorkshopTab),
    Install(u64),
    Subscribe(u64),
    Unsubscribe(u64),
    RefreshSubscriptions,
    ShowMoreSubscribed,
    Open(OpenTarget),
    OpenSettings,
    RetryCoveredMods,
    OpenDetail(Box<WorkshopItem>),
    CloseDetail,
    ShowImage(usize),
    CloseImage,
}

/// Everything a card or the detail window needs to describe one item.
struct ItemContext<'a> {
    workshop_path: Option<&'a Path>,
    steamcmd_workshop_path: Option<&'a Path>,
    steamcmd_available: bool,
    api_key_configured: bool,
    mods: &'a ModCollection,
    game_version: Option<&'a str>,
    covered_mods: &'a CoveredModsState<'a>,
    subscribed_ids: Option<&'a HashSet<u64>>,
    subscriptions_loading: bool,
    pending_subscriptions: &'a HashSet<u64>,
    installing_items: &'a [u64],
}

impl ItemContext<'_> {
    fn is_subscribed(&self, published_file_id: u64) -> bool {
        self.subscribed_ids
            .is_some_and(|ids| ids.contains(&published_file_id))
    }

    fn is_installed(&self, published_file_id: u64) -> bool {
        is_item_installed(
            self.workshop_path,
            self.steamcmd_workshop_path,
            published_file_id,
        )
    }

    fn is_pending(&self, published_file_id: u64) -> bool {
        self.pending_subscriptions.contains(&published_file_id)
    }

    fn is_installing(&self, published_file_id: u64) -> bool {
        self.installing_items.contains(&published_file_id)
    }
}

struct SidebarState<'a> {
    tab: WorkshopTab,
    versions: &'a mut Vec<String>,
    dlc: &'a mut Vec<String>,
    my_version_only: &'a mut bool,
    game_version: Option<&'a str>,
    subscribed_count: usize,
    subscriptions_loading: bool,
    subscriptions_error: Option<&'a str>,
    steamcmd_available: bool,
    covered_mods_state: &'a CoveredModsState<'a>,
}

#[derive(Default)]
struct SidebarOutcome {
    filters_changed: bool,
    retry_covered_mods: bool,
}

pub(crate) fn show_workshop(ui: &mut egui::Ui, view: WorkshopView<'_>) -> Option<WorkshopAction> {
    let WorkshopView {
        tab,
        query,
        subscribed_query,
        sort,
        subscribed_sort,
        versions,
        dlc,
        my_version_only,
        page,
        total,
        items,
        load_status,
        installing_items,
        workshop_path,
        steamcmd_workshop_path,
        steamcmd_available,
        api_key_configured,
        mods,
        game_version,
        covered_mods_state,
        subscribed_items,
        subscribed_load_status,
        subscribed_ids,
        subscriptions_loading,
        subscriptions_error,
        pending_subscriptions,
        subscribed_visible,
        grid_generation,
        subscribed_generation,
        detail,
        detail_image,
    } = view;

    let mut action = None;

    apply_workshop_style(ui);
    ui.add_space(2.0);

    ui.horizontal(|ui| {
        if theme::tab(ui, tab == WorkshopTab::Browse, "Browse").clicked() {
            action = Some(WorkshopAction::SwitchTab(WorkshopTab::Browse));
        }

        let subscription_count = format_count(subscribed_items.len() as u64);

        if theme::tab_with_count(
            ui,
            tab == WorkshopTab::Subscribed,
            "Subscribed",
            Some(subscription_count),
        )
        .clicked()
        {
            action = Some(WorkshopAction::SwitchTab(WorkshopTab::Subscribed));
        }

        if subscriptions_loading {
            ui.add(egui::Spinner::new().size(14.0));
        }
    });

    ui.add_space(6.0);

    if let Some(key_action) = show_key_notice(ui, api_key_configured) {
        action = Some(key_action);
    }

    let item_context = ItemContext {
        workshop_path,
        steamcmd_workshop_path,
        steamcmd_available,
        api_key_configured,
        mods,
        game_version,
        covered_mods: &covered_mods_state,
        subscribed_ids,
        subscriptions_loading,
        pending_subscriptions,
        installing_items,
    };

    match tab {
        WorkshopTab::Browse => {
            show_browse_header(ui, load_status);

            let query_is_empty = query.trim().is_empty();
            let (enter_pressed, (sort_changed, clear_clicked, search_clicked)) =
                egui::containers::Sides::new()
                    .shrink_left()
                    .height(theme::CONTROL_HEIGHT)
                    .show(
                        ui,
                        |ui| {
                            let search =
                                theme::search_field(ui, query, "Search the RimWorld Workshop");

                            // "/" jumps to the search box, like the Steam page.
                            if !ui.ctx().egui_wants_keyboard_input()
                                && ui.input(|input| input.key_pressed(egui::Key::Slash))
                            {
                                search.request_focus();
                            }

                            search.lost_focus()
                                && ui.input(|input| input.key_pressed(egui::Key::Enter))
                        },
                        |ui| {
                            let previous_sort = *sort;
                            let mut sort_changed = false;

                            egui::ComboBox::from_id_salt("workshop_sort")
                                .selected_text(sort.label())
                                .width(150.0)
                                .show_ui(ui, |ui| {
                                    if !query_is_empty {
                                        ui.selectable_value(
                                            sort,
                                            WorkshopSort::Relevance,
                                            WorkshopSort::Relevance.label(),
                                        );
                                    }

                                    for candidate in WorkshopSort::ALL {
                                        ui.selectable_value(sort, candidate, candidate.label());
                                    }
                                });

                            if *sort != previous_sort {
                                sort_changed = true;
                            }

                            let clear_clicked = !query_is_empty && ui.button("Clear").clicked();

                            let enabled = api_key_configured
                                && !matches!(load_status, WorkshopLoadStatus::Loading);

                            let search_clicked = ui
                                .add_enabled(enabled, egui::Button::new("Search"))
                                .on_disabled_hover_text(if api_key_configured {
                                    "A search is already running"
                                } else {
                                    "Add a Steam Web API key in Settings"
                                })
                                .clicked();

                            (sort_changed, clear_clicked, search_clicked)
                        },
                    );

            if clear_clicked {
                query.clear();
            }

            if enter_pressed || sort_changed || clear_clicked || search_clicked {
                action = Some(WorkshopAction::Search);
            }

            show_active_filters(
                ui,
                WorkshopTab::Browse,
                query,
                versions,
                dlc,
                my_version_only,
                &mut action,
            );
            ui.add_space(6.0);

            let body_height = ui.available_height().max(220.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(SIDEBAR_WIDTH.min(ui.available_width() * 0.32), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("workshop_sidebar")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let outcome = show_sidebar(
                                    ui,
                                    SidebarState {
                                        tab: WorkshopTab::Browse,
                                        versions,
                                        dlc,
                                        my_version_only,
                                        game_version,
                                        subscribed_count: subscribed_items.len(),
                                        subscriptions_loading,
                                        subscriptions_error,
                                        steamcmd_available,
                                        covered_mods_state: &covered_mods_state,
                                    },
                                );

                                if outcome.filters_changed {
                                    action = Some(WorkshopAction::Search);
                                }

                                if outcome.retry_covered_mods {
                                    action = Some(WorkshopAction::RetryCoveredMods);
                                }
                            });
                    },
                );

                ui.separator();

                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        let cards = items.iter().collect::<Vec<_>>();

                        if let Some(grid_action) = show_grid(
                            ui,
                            &cards,
                            &item_context,
                            ("workshop_grid", page, sort.label(), grid_generation),
                            Some((page, total, api_key_configured)),
                            load_status,
                            WorkshopAction::Search,
                        ) {
                            action = Some(grid_action);
                        }
                    },
                );
            });
        }
        WorkshopTab::Subscribed => {
            let cards = visible_subscribed_items(
                subscribed_items,
                subscribed_query,
                *subscribed_sort,
                versions,
                dlc,
                *my_version_only,
                game_version,
            );

            let total_matches = cards.len();
            let shown = total_matches.min(subscribed_visible);

            ui.label(RichText::new("Your subscriptions").text_style(theme::strong_style()));

            egui::containers::Sides::new()
                .shrink_left()
                .height(theme::CONTROL_HEIGHT)
                .show(
                    ui,
                    |ui| {
                        theme::search_field(ui, subscribed_query, "Filter your subscriptions");
                    },
                    |ui| {
                        egui::ComboBox::from_id_salt("subscribed_sort")
                            .selected_text(subscribed_sort.label())
                            .width(150.0)
                            .show_ui(ui, |ui| {
                                for candidate in SubscribedSort::ALL {
                                    ui.selectable_value(
                                        subscribed_sort,
                                        candidate,
                                        candidate.label(),
                                    );
                                }
                            });

                        let refresh =
                            ui.add_enabled(!subscriptions_loading, egui::Button::new("Refresh"));

                        if refresh.clicked() {
                            action = Some(WorkshopAction::RefreshSubscriptions);
                        }
                    },
                );

            show_active_filters(
                ui,
                WorkshopTab::Subscribed,
                subscribed_query,
                versions,
                dlc,
                my_version_only,
                &mut action,
            );
            ui.add_space(6.0);

            let body_height = ui.available_height().max(220.0);

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    Vec2::new(SIDEBAR_WIDTH.min(ui.available_width() * 0.32), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .id_salt("workshop_sidebar_subscribed")
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                let outcome = show_sidebar(
                                    ui,
                                    SidebarState {
                                        tab: WorkshopTab::Subscribed,
                                        versions,
                                        dlc,
                                        my_version_only,
                                        game_version,
                                        subscribed_count: subscribed_items.len(),
                                        subscriptions_loading,
                                        subscriptions_error,
                                        steamcmd_available,
                                        covered_mods_state: &covered_mods_state,
                                    },
                                );

                                if outcome.retry_covered_mods {
                                    action = Some(WorkshopAction::RetryCoveredMods);
                                }
                            });
                    },
                );

                ui.separator();

                ui.allocate_ui_with_layout(
                    Vec2::new(ui.available_width(), body_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        if shown < total_matches {
                            ui.horizontal(|ui| {
                                if ui
                                    .button(format!(
                                        "Show {} more",
                                        (total_matches - shown).min(60)
                                    ))
                                    .clicked()
                                {
                                    action = Some(WorkshopAction::ShowMoreSubscribed);
                                }
                            });
                            ui.add_space(4.0);
                        }

                        if subscribed_items.is_empty()
                            && matches!(subscribed_load_status, WorkshopLoadStatus::Ready)
                        {
                            ui.vertical_centered(|ui| {
                                ui.add_space(48.0);
                                ui.label(
                                    RichText::new("No Workshop subscriptions yet")
                                        .text_style(theme::strong_style()),
                                );
                                theme::hint(
                                    ui,
                                    "Subscribe to mods while browsing and they will appear here.",
                                );
                                ui.add_space(8.0);

                                if theme::primary_button(ui, "Browse the Workshop").clicked() {
                                    action = Some(WorkshopAction::SwitchTab(WorkshopTab::Browse));
                                }
                            });
                        } else if let Some(grid_action) = show_grid(
                            ui,
                            &cards[..shown],
                            &item_context,
                            (
                                "subscribed_grid",
                                *subscribed_sort,
                                subscribed_query.as_str(),
                                subscribed_generation,
                            ),
                            None,
                            subscribed_load_status,
                            WorkshopAction::RefreshSubscriptions,
                        ) {
                            action = Some(grid_action);
                        }
                    },
                );
            });
        }
    }

    if let Some(item) = detail
        && let Some(detail_action) = show_detail_window(ui, item, &item_context, detail_image)
    {
        action = Some(detail_action);
    }

    action
}

/// The Workshop page uses square controls and surfaces throughout.
fn apply_workshop_style(ui: &mut egui::Ui) {
    let visuals = ui.visuals_mut();

    for widget in [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ] {
        widget.corner_radius = CornerRadius::ZERO;
    }

    visuals.window_corner_radius = CornerRadius::ZERO;
    visuals.menu_corner_radius = CornerRadius::ZERO;
}

fn chip(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(theme::tint(color, 34))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(text.into()).small().color(color))
        })
        .response
}

fn show_key_notice(ui: &mut egui::Ui, api_key_configured: bool) -> Option<WorkshopAction> {
    if api_key_configured {
        return None;
    }

    let mut action = None;

    egui::Frame::new()
        .fill(theme::tint(theme::WARNING, 22))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            egui::containers::Sides::new().shrink_left().show(
                ui,
                |ui| {
                    ui.label(
                        RichText::new("Connect your Steam account")
                            .text_style(theme::strong_style()),
                    );
                    theme::hint(
                        ui,
                        "A Steam Web API key is required to search the Workshop and to subscribe or unsubscribe.",
                    );
                },
                |ui| {
                    if theme::primary_button(ui, "Open Settings").clicked() {
                        action = Some(WorkshopAction::OpenSettings);
                    }

                    if theme::secondary_button(ui, "Get a key").clicked() {
                        action = Some(WorkshopAction::Open(OpenTarget::Url(
                            "https://steamcommunity.com/dev/apikey".to_owned(),
                        )));
                    }
                },
            );
        });

    ui.add_space(8.0);

    action
}

fn show_browse_header(ui: &mut egui::Ui, load_status: &WorkshopLoadStatus) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("Browsing: Items").text_style(theme::strong_style()));

        if matches!(load_status, WorkshopLoadStatus::Loading) {
            ui.add(egui::Spinner::new().size(14.0));
        }
    });
}

fn show_active_filters(
    ui: &mut egui::Ui,
    tab: WorkshopTab,
    query: &mut String,
    versions: &mut Vec<String>,
    dlc: &mut Vec<String>,
    my_version_only: &mut bool,
    action: &mut Option<WorkshopAction>,
) {
    if query.trim().is_empty() && versions.is_empty() && dlc.is_empty() && !*my_version_only {
        return;
    }

    let search = |action: &mut Option<WorkshopAction>| {
        if tab == WorkshopTab::Browse {
            *action = Some(WorkshopAction::Search);
        }
    };

    ui.add_space(2.0);
    ui.horizontal_wrapped(|ui| {
        theme::hint(ui, "Filters:");

        if !query.trim().is_empty()
            && removable_chip(ui, &format!("\"{}\"", query.trim())).clicked()
        {
            query.clear();
            search(action);
        }

        if *my_version_only && removable_chip(ui, "My game version").clicked() {
            *my_version_only = false;
            search(action);
        }

        for version in versions.clone() {
            if removable_chip(ui, &version).clicked() {
                versions.retain(|candidate| candidate != &version);
                search(action);
            }
        }

        for dlc_tag in dlc.clone() {
            if removable_chip(ui, &dlc_tag).clicked() {
                dlc.retain(|candidate| candidate != &dlc_tag);
                search(action);
            }
        }

        if ui.link("Clear all").clicked() {
            query.clear();
            versions.clear();
            dlc.clear();
            *my_version_only = false;
            search(action);
        }
    });
}

fn removable_chip(ui: &mut egui::Ui, text: &str) -> egui::Response {
    egui::Frame::new()
        .fill(theme::tint(theme::ACCENT, 30))
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.add(
                egui::Button::new(
                    RichText::new(format!("{text}  ×"))
                        .small()
                        .color(theme::ACCENT),
                )
                .frame(false)
                .sense(Sense::click()),
            )
        })
        .inner
        .on_hover_text("Remove this filter")
        .on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn show_sidebar(ui: &mut egui::Ui, state: SidebarState<'_>) -> SidebarOutcome {
    let SidebarState {
        tab,
        versions,
        dlc,
        my_version_only,
        game_version,
        subscribed_count,
        subscriptions_loading,
        subscriptions_error,
        steamcmd_available,
        covered_mods_state,
    } = state;

    let mut outcome = SidebarOutcome::default();

    ui.spacing_mut().item_spacing.y = 3.0;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Filters").small().color(theme::TEXT_FAINT));

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let has_filters = !versions.is_empty() || !dlc.is_empty() || *my_version_only;

            if has_filters {
                if theme::link(ui, "Reset").clicked() {
                    versions.clear();
                    dlc.clear();
                    *my_version_only = false;
                    outcome.filters_changed = true;
                }
            } else {
                theme::hint(ui, "Reset");
            }
        });
    });

    ui.add_space(4.0);

    let mut my_version = *my_version_only;
    let version_label = game_version
        .and_then(major_minor_version_text)
        .map(|version| format!("Only mods for my version ({version})"))
        .unwrap_or_else(|| "Only mods for my version".to_owned());

    if ui
        .add_enabled(
            game_version.is_some(),
            egui::Checkbox::new(&mut my_version, version_label),
        )
        .on_disabled_hover_text("RimWorld's version is not known yet")
        .changed()
    {
        *my_version_only = my_version;
        outcome.filters_changed = true;
    }

    ui.add_space(4.0);

    egui::CollapsingHeader::new("Game version")
        .default_open(true)
        .show(ui, |ui| {
            for version in RIMWORLD_VERSIONS {
                let mut selected = versions.iter().any(|candidate| candidate == version);

                if ui.checkbox(&mut selected, *version).changed() {
                    toggle_value(versions, version);
                    outcome.filters_changed = true;
                }
            }
        });

    egui::CollapsingHeader::new("Required DLC")
        .default_open(true)
        .show(ui, |ui| {
            for dlc_tag in RIMWORLD_DLC {
                let mut selected = dlc.iter().any(|candidate| candidate == dlc_tag);

                if ui.checkbox(&mut selected, *dlc_tag).changed() {
                    toggle_value(dlc, dlc_tag);
                    outcome.filters_changed = true;
                }
            }
        });

    if tab == WorkshopTab::Subscribed {
        theme::hint(
            ui,
            "Version and DLC filters apply to the subscriptions already loaded.",
        );
    }

    ui.add_space(8.0);
    ui.separator();
    ui.add_space(6.0);

    ui.label(
        RichText::new("STEAM STATUS")
            .font(theme::semibold_font(11.0))
            .color(theme::TEXT_FAINT),
    );
    ui.add_space(2.0);

    let (steam_color, steam_hover) = if subscriptions_loading {
        (theme::TEXT_MUTED, "Loading your Steam subscriptions…")
    } else if let Some(error) = subscriptions_error {
        (theme::WARNING, error)
    } else if subscribed_count == 0 {
        (theme::TEXT_MUTED, "No Workshop subscriptions found")
    } else {
        (theme::SUCCESS, "Subscriptions loaded")
    };

    ui.horizontal_wrapped(|ui| {
        chip(ui, "Steam", steam_color).on_hover_text(steam_hover);

        if steamcmd_available {
            chip(ui, "SteamCMD", theme::SUCCESS).on_hover_text("SteamCMD ready for downloads");
        } else {
            chip(ui, "SteamCMD", theme::WARNING).on_hover_text(
                "SteamCMD not found — set its path in Settings to download without Steam",
            );
        }
    });

    if let CoveredModsState::Failed(error) = covered_mods_state {
        ui.add_space(6.0);
        ui.label(
            RichText::new("Compatibility data unavailable")
                .small()
                .color(theme::WARNING),
        )
        .on_hover_text(error);

        if theme::link(ui, "Retry").clicked() {
            outcome.retry_covered_mods = true;
        }
    }

    outcome
}

fn toggle_value(values: &mut Vec<String>, value: &str) {
    if let Some(position) = values.iter().position(|candidate| candidate == value) {
        values.remove(position);
    } else {
        values.push(value.to_owned());
    }
}

fn show_grid(
    ui: &mut egui::Ui,
    cards: &[&WorkshopItem],
    context: &ItemContext<'_>,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    pager: Option<(u32, u64, bool)>,
    load_status: &WorkshopLoadStatus,
    retry: WorkshopAction,
) -> Option<WorkshopAction> {
    let mut action = None;

    if matches!(load_status, WorkshopLoadStatus::Loading) && cards.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.add(egui::Spinner::new());
            theme::hint(ui, "Searching the Workshop...");
        });
        return None;
    }

    if let WorkshopLoadStatus::Error(message) = load_status
        && cards.is_empty()
    {
        ui.vertical_centered(|ui| {
            ui.add_space(40.0);
            ui.label(RichText::new(message).color(theme::DANGER));
            ui.add_space(6.0);

            if theme::secondary_button(ui, "Retry").clicked() {
                action = Some(retry);
            }
        });

        return action;
    }

    if cards.is_empty() {
        let message = match load_status {
            WorkshopLoadStatus::Idle => {
                "Search for mods, or pick a ranking to browse the Workshop."
            }
            _ => "No items match these filters.",
        };

        ui.vertical_centered(|ui| {
            ui.add_space(48.0);
            ui.label(RichText::new(message).color(theme::TEXT_MUTED));
        });

        return action;
    }

    // Reserve a lane for the floating scrollbar: cards must never sit under
    // it, or clicking near the edge hits the wrong thing.
    let available_width = (ui.available_width() - theme::SCROLL_GUTTER).max(200.0);
    let columns =
        (((available_width + CARD_GAP) / (MIN_CARD_WIDTH + CARD_GAP)).floor() as usize).clamp(1, 6);
    let card_width = ((available_width - CARD_GAP * (columns.saturating_sub(1) as f32))
        / columns as f32)
        .floor()
        .max(160.0);

    // The pager is reserved outside the scroll area so changing pages never
    // requires scrolling to the bottom first.
    let pager_height = if pager.is_some() {
        theme::CONTROL_HEIGHT + 18.0
    } else {
        0.0
    };
    let grid_height = (ui.available_height() - pager_height).max(120.0);

    ui.allocate_ui_with_layout(
        Vec2::new(ui.available_width(), grid_height),
        egui::Layout::top_down(egui::Align::Min),
        |ui| {
            egui::ScrollArea::vertical()
                .id_salt(id_salt)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(CARD_GAP, CARD_GAP);

                    for row in cards.chunks(columns) {
                        ui.horizontal_top(|ui| {
                            for item in row {
                                if let Some(card_action) = show_card(ui, card_width, item, context)
                                {
                                    action = Some(card_action);
                                }
                            }
                        });
                    }

                    ui.add_space(10.0);
                });
        },
    );

    if let Some((page, total, api_key_configured)) = pager {
        ui.add_space(6.0);
        show_pager(ui, page, total, api_key_configured, &mut action);
    }

    action
}

fn show_card(
    ui: &mut egui::Ui,
    width: f32,
    item: &WorkshopItem,
    context: &ItemContext<'_>,
) -> Option<WorkshopAction> {
    let mut action = None;
    let installed = context.is_installed(item.published_file_id);
    let preview = item
        .preview_url
        .as_deref()
        .or_else(|| item.images.first().map(String::as_str));

    let inner_width = (width - 16.0).max(120.0);
    let image_height = (inner_width * CARD_IMAGE_ASPECT).round();
    let content_height = image_height
        + 6.0
        + CARD_TITLE_HEIGHT
        + CARD_AUTHOR_HEIGHT
        + CARD_META_HEIGHT
        + CARD_VERSION_HEIGHT
        + 6.0
        + CARD_ACTION_HEIGHT;

    theme::gradient_card(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(8),
        |ui| {
            // The frame inherits the row's horizontal layout, so the card
            // needs its own vertical layout and an exact height.
            ui.vertical(|ui| {
                ui.set_width(inner_width);
                ui.set_min_height(content_height);

                let image_area = ui.allocate_ui_with_layout(
                    Vec2::new(inner_width, image_height),
                    egui::Layout::centered_and_justified(egui::Direction::TopDown),
                    |ui| {
                        ui.set_min_size(Vec2::new(inner_width, image_height));
                        ui.painter().rect_filled(
                            ui.max_rect(),
                            CornerRadius::ZERO,
                            theme::SURFACE_HOVER,
                        );

                        if let Some(preview) = preview {
                            ui.add(
                                egui::Image::new(preview)
                                    .max_width(inner_width)
                                    .max_height(image_height)
                                    .maintain_aspect_ratio(true)
                                    .show_loading_spinner(true)
                                    .alt_text(format!("Preview image for {}", item.title)),
                            );
                        } else {
                            ui.painter().text(
                                ui.max_rect().center(),
                                egui::Align2::CENTER_CENTER,
                                "No preview",
                                theme::medium_font(11.0),
                                theme::TEXT_FAINT,
                            );
                        }
                    },
                );

                let image_response = ui
                    .interact(
                        image_area.response.rect,
                        ui.id().with(("preview", item.published_file_id)),
                        Sense::click(),
                    )
                    .on_hover_text("Open details")
                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                if image_response.clicked() {
                    action = Some(WorkshopAction::OpenDetail(Box::new(item.clone())));
                }

                ui.add_space(6.0);

                let title = ui
                    .allocate_ui_with_layout(
                        Vec2::new(inner_width, CARD_TITLE_HEIGHT),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_size(Vec2::new(inner_width, CARD_TITLE_HEIGHT));
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&item.title)
                                        .font(theme::semibold_font(13.0))
                                        .color(theme::TEXT),
                                )
                                .truncate()
                                .sense(Sense::click()),
                            )
                        },
                    )
                    .inner
                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                if title.clicked() {
                    action = Some(WorkshopAction::OpenDetail(Box::new(item.clone())));
                }

                ui.allocate_ui_with_layout(
                    Vec2::new(inner_width, CARD_AUTHOR_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(Vec2::new(inner_width, CARD_AUTHOR_HEIGHT));

                        if let Some(author) = item.author.as_deref() {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!("By {author}"))
                                        .small()
                                        .color(theme::TEXT_MUTED),
                                )
                                .truncate(),
                            );
                        }
                    },
                );

                ui.allocate_ui_with_layout(
                    Vec2::new(inner_width, CARD_META_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(Vec2::new(inner_width, CARD_META_HEIGHT));

                        if let Some(rating) = item.rating_percent() {
                            let color = if rating >= 80 {
                                theme::SUCCESS
                            } else if rating >= 60 {
                                theme::WARNING
                            } else {
                                theme::TEXT_MUTED
                            };

                            ui.label(RichText::new(format!("{rating}%")).small().color(color))
                                .on_hover_text("Share of positive votes");
                        }

                        if let Some(subscriptions) = item.subscriptions {
                            theme::hint(ui, format!("{} subscribers", format_count(subscriptions)));
                        }
                    },
                );

                let (supported, community) = versions_for_item(item, context);
                let badge = card_version_badge(supported, community, context.game_version);

                ui.allocate_ui_with_layout(
                    Vec2::new(inner_width, CARD_VERSION_HEIGHT),
                    egui::Layout::left_to_right(egui::Align::Center),
                    |ui| {
                        ui.set_min_size(Vec2::new(inner_width, CARD_VERSION_HEIGHT));

                        if let Some((label, color, hover)) = badge {
                            chip(ui, label, color).on_hover_text(hover);
                        }
                    },
                );

                ui.add_space(6.0);

                let button_size = Vec2::new(
                    ((ui.available_width() - 6.0) / 2.0).max(60.0),
                    CARD_ACTION_HEIGHT,
                );

                ui.horizontal(|ui| {
                    ui.set_min_height(CARD_ACTION_HEIGHT);
                    ui.spacing_mut().item_spacing.x = 6.0;
                    ui.spacing_mut().button_padding = Vec2::new(6.0, 4.0);

                    if let Some(subscribe_action) =
                        subscription_button(ui, button_size, item, context)
                    {
                        action = Some(subscribe_action);
                    }

                    if installed {
                        theme::state_pill(ui, button_size, "✓ Installed", theme::SUCCESS)
                            .on_hover_text("Already downloaded to your Workshop folder");
                    } else if context.is_installing(item.published_file_id) {
                        ui.allocate_ui_with_layout(
                            button_size,
                            egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
                            |ui| {
                                ui.add(egui::Spinner::new().size(12.0));
                                theme::hint(ui, "Downloading");
                            },
                        );
                    } else {
                        let install = ui
                            .add_enabled_ui(
                                context.steamcmd_available && context.installing_items.is_empty(),
                                |ui| theme::secondary_button_sized(ui, "Install", button_size),
                            )
                            .inner;

                        if install
                            .on_hover_text("Download with SteamCMD without subscribing")
                            .on_disabled_hover_text(if context.steamcmd_available {
                                "A download is already running"
                            } else {
                                "SteamCMD was not found. Set its path in Settings."
                            })
                            .clicked()
                        {
                            action = Some(WorkshopAction::Install(item.published_file_id));
                        }
                    }
                });
            });
        },
    );

    action
}

fn subscription_button(
    ui: &mut egui::Ui,
    size: Vec2,
    item: &WorkshopItem,
    context: &ItemContext<'_>,
) -> Option<WorkshopAction> {
    let subscribed = context.is_subscribed(item.published_file_id);
    let pending = context.is_pending(item.published_file_id);
    let loading = context.subscriptions_loading;
    let enabled = context.api_key_configured && !pending && !loading;

    let response = if pending {
        theme::state_pill(ui, size, "Working…", theme::TEXT_MUTED)
    } else if subscribed {
        theme::toggle_button(
            ui,
            size,
            "✓ Subscribed",
            "Unsubscribe",
            theme::SUCCESS,
            theme::DANGER,
            enabled,
        )
        .on_hover_text("Subscribed on Steam. Click to unsubscribe.")
    } else {
        ui.add_enabled_ui(enabled, |ui| {
            theme::primary_button_sized(ui, "Subscribe", size)
        })
        .inner
        .on_hover_text("Subscribe with your Steam account")
        .on_disabled_hover_text(if loading {
            "Loading your Steam subscriptions..."
        } else {
            "Add a Steam Web API key in Settings to subscribe"
        })
    };

    if !response.clicked() {
        return None;
    }

    if subscribed {
        Some(WorkshopAction::Unsubscribe(item.published_file_id))
    } else {
        Some(WorkshopAction::Subscribe(item.published_file_id))
    }
}

fn versions_for_item<'a>(
    item: &'a WorkshopItem,
    context: &'a ItemContext<'a>,
) -> (&'a [String], &'a [String]) {
    if let Some(installed_mod) = installed_workshop_mod(context.mods, item.published_file_id) {
        return (
            installed_mod.supported_versions.as_slice(),
            installed_mod.community_supported_versions.as_slice(),
        );
    }

    let community = context
        .covered_mods
        .ready_mods()
        .and_then(|covered_mods| covered_mods.get(&item.published_file_id))
        .map(Vec::as_slice)
        .unwrap_or(&[]);

    (item.supported_versions.as_slice(), community)
}

fn card_version_badge(
    supported: &[String],
    community: &[String],
    game_version: Option<&str>,
) -> Option<(String, Color32, String)> {
    let game_version = game_version?;

    if let Some(version) = supported
        .iter()
        .find(|version| versions_are_compatible(version, game_version))
    {
        return Some((
            major_minor_version_text(version).unwrap_or_else(|| version.clone()),
            theme::SUCCESS,
            format!("Lists {version}, which matches your game version"),
        ));
    }

    if let Some(version) = community
        .iter()
        .find(|version| versions_are_compatible(version, game_version))
    {
        return Some((
            major_minor_version_text(version).unwrap_or_else(|| version.clone()),
            theme::WARNING,
            "A No Version Warning community report says this works with your game version"
                .to_owned(),
        ));
    }

    let newest = highest_supported_version(supported)?;

    Some((
        newest.to_owned(),
        theme::TEXT_FAINT,
        "Does not list your game version".to_owned(),
    ))
}

fn show_pager(
    ui: &mut egui::Ui,
    page: u32,
    total: u64,
    api_key_configured: bool,
    action: &mut Option<WorkshopAction>,
) {
    let page_count = (total.div_ceil(ITEMS_PER_PAGE).max(1)) as u32;
    let enabled = api_key_configured;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(enabled && page > 1, egui::Button::new("Previous"))
            .clicked()
        {
            *action = Some(WorkshopAction::GoToPage(page - 1));
        }

        for number in page_numbers(page, page_count) {
            match number {
                Some(number) if number == page => {
                    ui.add(
                        egui::Button::selectable(true, number.to_string())
                            .min_size(Vec2::new(32.0, theme::CONTROL_HEIGHT)),
                    );
                }
                Some(number) => {
                    if ui
                        .add_enabled(
                            enabled,
                            egui::Button::new(number.to_string())
                                .min_size(Vec2::new(32.0, theme::CONTROL_HEIGHT)),
                        )
                        .clicked()
                    {
                        *action = Some(WorkshopAction::GoToPage(number));
                    }
                }
                None => {
                    ui.label(RichText::new("…").color(theme::TEXT_FAINT));
                }
            }
        }

        if ui
            .add_enabled(enabled && page < page_count, egui::Button::new("Next"))
            .clicked()
        {
            *action = Some(WorkshopAction::GoToPage(page + 1));
        }
    });
}

/// Page links with gaps collapsed to ellipses, keeping first, last and the
/// pages around the current one visible.
fn page_numbers(current: u32, page_count: u32) -> Vec<Option<u32>> {
    if page_count <= 7 {
        return (1..=page_count).map(Some).collect();
    }

    let mut pages = vec![1, page_count];
    for page in current.saturating_sub(1).max(1)..=(current + 1).min(page_count) {
        pages.push(page);
    }

    pages.sort_unstable();
    pages.dedup();

    let mut result = Vec::new();
    let mut previous = 0;

    for page in pages {
        if previous > 0 && page > previous + 1 {
            result.push(None);
        }

        result.push(Some(page));
        previous = page;
    }

    result
}

fn visible_subscribed_items<'a>(
    items: &'a [WorkshopItem],
    query: &str,
    sort: SubscribedSort,
    versions: &[String],
    dlc: &[String],
    my_version_only: bool,
    game_version: Option<&str>,
) -> Vec<&'a WorkshopItem> {
    let tokens = query
        .split_whitespace()
        .map(|token| token.to_ascii_lowercase())
        .collect::<Vec<_>>();

    let mut visible = items
        .iter()
        .filter(|item| {
            if my_version_only
                && let Some(game_version) = game_version
                && !item.supports_version(game_version)
            {
                return false;
            }

            if !versions.is_empty()
                && !versions.iter().all(|version| {
                    item.supported_versions
                        .iter()
                        .any(|candidate| candidate == version)
                })
            {
                return false;
            }

            if !dlc.is_empty()
                && !dlc
                    .iter()
                    .all(|dlc_tag| item.tags.iter().any(|candidate| candidate == dlc_tag))
            {
                return false;
            }

            if tokens.is_empty() {
                return true;
            }

            let title = item.title.to_ascii_lowercase();
            let description = item.description.to_ascii_lowercase();

            tokens
                .iter()
                .all(|token| title.contains(token) || description.contains(token))
        })
        .collect::<Vec<_>>();

    match sort {
        SubscribedSort::RecentlyUpdated => {
            visible.sort_by_key(|item| std::cmp::Reverse(item.time_updated));
        }
        SubscribedSort::MostSubscribed => {
            visible.sort_by(|left, right| {
                right
                    .subscriptions
                    .unwrap_or(0)
                    .cmp(&left.subscriptions.unwrap_or(0))
            });
        }
        SubscribedSort::Name => {
            visible.sort_by_key(|item| item.title.to_ascii_lowercase());
        }
    }

    visible
}

fn show_detail_window(
    ui: &mut egui::Ui,
    item: &WorkshopItem,
    context: &ItemContext<'_>,
    detail_image: Option<usize>,
) -> Option<WorkshopAction> {
    if detail_image.is_none() && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        return Some(WorkshopAction::CloseDetail);
    }

    let mut open = true;
    let mut action = None;
    let installed = context.is_installed(item.published_file_id);
    let screen = ui.ctx().content_rect();
    let default_width = (screen.width() * 0.62).clamp(480.0, 780.0);
    let default_pos = screen.center() - Vec2::new(default_width * 0.5, screen.height() * 0.35);
    let preview_height = (screen.height() * 0.3).clamp(150.0, 320.0);
    let gallery_height = (screen.height() * 0.2).clamp(90.0, 200.0);
    let detail_scroll_height = (screen.height() - 80.0).max(200.0);
    let preview = item
        .preview_url
        .as_deref()
        .or_else(|| item.images.first().map(String::as_str));

    egui::Window::new("Workshop item")
        .id(egui::Id::new("workshop_detail"))
        .default_pos(default_pos)
        .default_width(default_width)
        .max_width((screen.width() - 40.0).max(320.0))
        .max_height((screen.height() - 40.0).max(240.0))
        .constrain_to(screen)
        .movable(true)
        .collapsible(false)
        .resizable(true)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE)
                .corner_radius(CornerRadius::ZERO)
                .shadow(theme::soft_shadow()),
        )
        .open(&mut open)
        .show(ui.ctx(), |ui| {
            apply_workshop_style(ui);
            theme::paint_background_gradient(ui, theme::SURFACE_HOVER, theme::SURFACE);

            egui::Frame::new()
                .inner_margin(Margin::same(12))
                .show(ui, |ui| {
                    egui::ScrollArea::vertical()
                .id_salt(("workshop_detail_body", item.published_file_id))
                .auto_shrink([false, true])
                .max_height(detail_scroll_height)
                .show(ui, |ui| {
                    let available_width = ui.available_width();

            ui.allocate_ui_with_layout(
                Vec2::new(available_width, preview_height),
                egui::Layout::centered_and_justified(egui::Direction::TopDown),
                |ui| {
                    ui.set_min_size(Vec2::new(available_width, preview_height));
                    ui.painter().rect_filled(
                        ui.max_rect(),
                        CornerRadius::ZERO,
                        theme::SURFACE_HOVER,
                    );

                    if let Some(preview) = preview {
                        ui.add(
                            egui::Image::new(preview)
                                .max_width(available_width)
                                .max_height(preview_height)
                                .maintain_aspect_ratio(true)
                                .show_loading_spinner(true),
                        );
                    } else {
                        ui.painter().text(
                            ui.max_rect().center(),
                            egui::Align2::CENTER_CENTER,
                            "No preview image",
                            theme::medium_font(11.0),
                            theme::TEXT_FAINT,
                        );
                    }
                },
            );

            ui.add_space(8.0);

            ui.label(
                RichText::new(&item.title)
                    .font(theme::semibold_font(20.0))
                    .color(theme::TEXT),
            );

            if let Some(author) = item.author.as_deref()
                && ui.link(format!("By {author}")).clicked()
                && let Some(url) = item.author_url()
            {
                action = Some(WorkshopAction::Open(OpenTarget::Url(url)));
            }

            ui.horizontal_wrapped(|ui| {
                theme::hint(ui, format!("Workshop item {}", item.published_file_id));

                if ui.link("Copy ID").clicked() {
                    ui.ctx().copy_text(item.published_file_id.to_string());
                }

                if ui.link("Copy link").clicked() {
                    ui.ctx().copy_text(item.page_url());
                }
            });

            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                if let Some(rating) = item.rating_percent() {
                    let color = if rating >= 80 {
                        theme::SUCCESS
                    } else if rating >= 60 {
                        theme::WARNING
                    } else {
                        theme::TEXT_MUTED
                    };

                    chip(ui, format!("{rating}% positive"), color)
                        .on_hover_text("Share of positive votes");
                }

                if let Some(subscriptions) = item.subscriptions {
                    chip(
                        ui,
                        format!("{} subscribers", format_count(subscriptions)),
                        theme::TEXT_MUTED,
                    );
                }

                if let Some(favorites) = item.favorites {
                    chip(
                        ui,
                        format!("{} favorites", format_count(favorites)),
                        theme::TEXT_MUTED,
                    );
                }

                if let Some(date) = format_date(item.time_updated) {
                    chip(ui, format!("Updated {date}"), theme::TEXT_MUTED);
                }
            });

            ui.add_space(4.0);

            let (supported, community) = versions_for_item(item, context);

            if !supported.is_empty() || !community.is_empty() {
                show_supported_versions(ui, supported, community, context.game_version);
            }

            ui.add_space(8.0);

            ui.horizontal_wrapped(|ui| {
                let action_size = Vec2::new(132.0, theme::CONTROL_HEIGHT);

                if let Some(subscribe_action) =
                    subscription_button(ui, action_size, item, context)
                {
                    action = Some(subscribe_action);
                }

                if installed {
                    theme::state_pill(ui, action_size, "✓ Installed", theme::SUCCESS)
                        .on_hover_text("Already downloaded to your Workshop folder");
                } else {
                    let install = ui
                        .add_enabled_ui(
                            context.steamcmd_available && context.installing_items.is_empty(),
                            |ui| {
                                theme::secondary_button_sized(
                                    ui,
                                    "Install with SteamCMD",
                                    Vec2::new(170.0, theme::CONTROL_HEIGHT),
                                )
                            },
                        )
                        .inner;

                    if install
                        .on_hover_text(
                            "Download the files without subscribing. Steam will not keep them updated.",
                        )
                        .on_disabled_hover_text(if context.steamcmd_available {
                            "A download is already running"
                        } else {
                            "SteamCMD was not found. Set its path in Settings."
                        })
                        .clicked()
                    {
                        action = Some(WorkshopAction::Install(item.published_file_id));
                    }
                }

                if ui.link("Open in Steam").clicked() {
                    action = Some(WorkshopAction::Open(OpenTarget::Url(item.steam_url())));
                }

                if ui.link("Steam page").clicked() {
                    action = Some(WorkshopAction::Open(OpenTarget::Url(item.page_url())));
                }
            });

            if !item.images.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new("More images").text_style(theme::strong_style()));
                theme::hint(ui, "Click an image to view it larger.");

                let gallery_item_size =
                    Vec2::new((gallery_height * 1.6).max(200.0), gallery_height);

                egui::ScrollArea::horizontal()
                    .id_salt(("workshop_detail_images", item.published_file_id))
                    .max_height(gallery_height + 8.0)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            for (index, url) in item.images.iter().enumerate() {
                                let image_area = ui
                                    .allocate_ui_with_layout(
                                        gallery_item_size,
                                        egui::Layout::centered_and_justified(
                                            egui::Direction::TopDown,
                                        ),
                                        |ui| {
                                            ui.set_min_size(gallery_item_size);
                                            ui.painter().rect_filled(
                                                ui.max_rect(),
                                                CornerRadius::ZERO,
                                                theme::SURFACE_HOVER,
                                            );
                                            ui.add(
                                                egui::Image::new(url.as_str())
                                                    .max_width(gallery_item_size.x)
                                                    .max_height(gallery_item_size.y)
                                                    .maintain_aspect_ratio(true)
                                                    .show_loading_spinner(true)
                                                    .alt_text(format!(
                                                        "Screenshot for {}",
                                                        item.title
                                                    )),
                                            );
                                        },
                                    )
                                    .response;

                                let image_area = ui
                                    .interact(
                                        image_area.rect,
                                        ui.id().with((
                                            "detail_image",
                                            item.published_file_id,
                                            index,
                                        )),
                                        Sense::click(),
                                    )
                                    .on_hover_text("View image")
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);

                                if image_area.clicked() {
                                    action = Some(WorkshopAction::ShowImage(index));
                                }
                            }
                        });
                    });
            }

            ui.separator();

            let description = strip_bbcode(&item.description);
            egui::Frame::new()
                .fill(theme::CANVAS)
                .corner_radius(CornerRadius::ZERO)
                .inner_margin(Margin::same(10))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());

                    if description.is_empty() {
                        theme::hint(ui, "This item has no description.");
                    } else {
                        ui.add(
                            egui::Label::new(RichText::new(description).color(theme::TEXT_MUTED))
                                .wrap()
                                .selectable(true),
                        );
                    }
                });

                });
                });
        });

    if let Some(image_index) = detail_image
        && let Some(viewer_action) = show_image_viewer(ui, item, image_index)
    {
        action = Some(viewer_action);
    }

    if !open {
        action = Some(WorkshopAction::CloseDetail);
    }

    action
}

/// In-app lightbox for Workshop screenshots. Images never leave the app, so
/// clicking a thumbnail does not hijack the user's browser.
fn show_image_viewer(
    ui: &mut egui::Ui,
    item: &WorkshopItem,
    index: usize,
) -> Option<WorkshopAction> {
    let images = &item.images;
    let Some(url) = images.get(index) else {
        return Some(WorkshopAction::CloseImage);
    };

    if ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        return Some(WorkshopAction::CloseImage);
    }

    let screen = ui.ctx().content_rect();
    let max_width = (screen.width() * 0.86).max(360.0);
    let max_height = (screen.height() * 0.82).max(280.0);
    let image_size = Vec2::new(
        (max_width - 24.0).max(240.0),
        (max_height - 92.0).max(160.0),
    );
    let mut open = true;
    let mut action = None;

    egui::Window::new(format!(
        "{} · image {} of {}",
        item.title,
        index + 1,
        images.len()
    ))
    .id(egui::Id::new("workshop_image_viewer"))
    .anchor(egui::Align2::CENTER_CENTER, Vec2::ZERO)
    .collapsible(false)
    .resizable(false)
    .movable(false)
    .max_width(max_width)
    .max_height(max_height)
    .frame(
        egui::Frame::new()
            .fill(theme::SURFACE)
            .corner_radius(CornerRadius::ZERO)
            .shadow(theme::soft_shadow())
            .inner_margin(Margin::same(12)),
    )
    .open(&mut open)
    .show(ui.ctx(), |ui| {
        apply_workshop_style(ui);

        ui.allocate_ui_with_layout(
            image_size,
            egui::Layout::centered_and_justified(egui::Direction::TopDown),
            |ui| {
                ui.set_min_size(image_size);
                ui.painter()
                    .rect_filled(ui.max_rect(), CornerRadius::ZERO, theme::CANVAS);
                ui.add(
                    egui::Image::new(url.as_str())
                        .max_width(image_size.x)
                        .max_height(image_size.y)
                        .maintain_aspect_ratio(true)
                        .show_loading_spinner(true)
                        .alt_text(format!("Screenshot for {}", item.title)),
                );
            },
        );

        ui.add_space(8.0);

        let mut close_clicked = false;

        egui::containers::Sides::new().shrink_left().show(
            ui,
            |ui| {
                if ui
                    .add_enabled(index > 0, egui::Button::new("Previous"))
                    .clicked()
                {
                    action = Some(WorkshopAction::ShowImage(index - 1));
                }

                if ui
                    .add_enabled(index + 1 < images.len(), egui::Button::new("Next"))
                    .clicked()
                {
                    action = Some(WorkshopAction::ShowImage(index + 1));
                }
            },
            |ui| {
                if theme::secondary_button(ui, "Close").clicked() {
                    close_clicked = true;
                }
            },
        );

        if close_clicked {
            action = Some(WorkshopAction::CloseImage);
        }
    });

    if !open {
        action = Some(WorkshopAction::CloseImage);
    }

    action
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
    use super::*;
    use crate::models::{ModId, ModType, PackageId};
    use std::path::PathBuf;

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

    #[test]
    fn page_numbers_keep_the_ends_and_the_current_page() {
        assert_eq!(page_numbers(1, 3), vec![Some(1), Some(2), Some(3)]);
        assert_eq!(
            page_numbers(10, 20),
            vec![Some(1), None, Some(9), Some(10), Some(11), None, Some(20)]
        );
        assert_eq!(
            page_numbers(2, 20),
            vec![Some(1), Some(2), Some(3), None, Some(20)]
        );
    }

    #[test]
    fn card_badge_prefers_an_explicit_version_match() {
        let badge =
            card_version_badge(&["1.5".to_owned(), "1.6".to_owned()], &[], Some("1.6.4211"))
                .expect("badge");

        assert_eq!(badge.0, "1.6");
        assert_eq!(badge.1, theme::SUCCESS);
    }

    #[test]
    fn card_badge_uses_community_reports_as_a_warning() {
        let badge = card_version_badge(&["1.5".to_owned()], &["1.6".to_owned()], Some("1.6"))
            .expect("badge");

        assert_eq!(badge.0, "1.6");
        assert_eq!(badge.1, theme::WARNING);
    }

    #[test]
    fn card_badge_shows_the_newest_supported_version_otherwise() {
        let badge = card_version_badge(&["1.4".to_owned(), "1.5".to_owned()], &[], Some("1.6"))
            .expect("badge");

        assert_eq!(badge.0, "1.5");
        assert_eq!(badge.1, theme::TEXT_FAINT);
    }

    fn subscribed_item(
        id: u64,
        title: &str,
        description: &str,
        updated: u64,
        subscribers: u64,
        versions: &[&str],
        dlc: &[&str],
    ) -> WorkshopItem {
        WorkshopItem {
            published_file_id: id,
            title: title.to_owned(),
            description: description.to_owned(),
            preview_url: None,
            images: Vec::new(),
            author_steam_id: None,
            author: None,
            subscriptions: Some(subscribers),
            favorites: None,
            time_created: 0,
            time_updated: updated,
            votes_up: 0,
            votes_down: 0,
            tags: versions
                .iter()
                .chain(dlc.iter())
                .map(|tag| (*tag).to_owned())
                .collect(),
            supported_versions: versions.iter().map(|tag| (*tag).to_owned()).collect(),
        }
    }

    #[test]
    fn subscribed_search_matches_titles_and_descriptions() {
        let items = vec![
            subscribed_item(
                1,
                "Vanilla Furniture",
                "Adds couches",
                10,
                100,
                &["1.6"],
                &[],
            ),
            subscribed_item(2, "Fishing", "Adds ocean fish", 30, 50, &["1.6"], &[]),
        ];

        let visible = visible_subscribed_items(
            &items,
            "ocean",
            SubscribedSort::RecentlyUpdated,
            &[],
            &[],
            false,
            Some("1.6"),
        );

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].published_file_id, 2);
    }

    #[test]
    fn subscribed_filters_require_every_selected_version_and_dlc() {
        let items = vec![
            subscribed_item(1, "Both", "", 0, 10, &["1.5", "1.6"], &["Biotech"]),
            subscribed_item(2, "Only 1.6", "", 0, 10, &["1.6"], &[]),
        ];

        let visible = visible_subscribed_items(
            &items,
            "",
            SubscribedSort::Name,
            &["1.5".to_owned(), "1.6".to_owned()],
            &["Biotech".to_owned()],
            false,
            None,
        );

        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].title, "Both");
    }

    #[test]
    fn subscribed_sorting_prefers_recent_updates_and_subscribers() {
        let items = vec![
            subscribed_item(1, "Old popular", "", 10, 900, &[], &[]),
            subscribed_item(2, "New quiet", "", 30, 10, &[], &[]),
        ];

        let recent = visible_subscribed_items(
            &items,
            "",
            SubscribedSort::RecentlyUpdated,
            &[],
            &[],
            false,
            None,
        );
        assert_eq!(recent[0].published_file_id, 2);

        let popular = visible_subscribed_items(
            &items,
            "",
            SubscribedSort::MostSubscribed,
            &[],
            &[],
            false,
            None,
        );
        assert_eq!(popular[0].published_file_id, 1);
    }
}

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, Color32, Margin, RichText, TextStyle, Vec2};

use crate::app::log_ctrl::LogLoadStatus;
use crate::models::{ModCollection, ModSource, PackageId, RimworldMod};
use crate::services::log_analysis::{LogIssue, LogReport, ModSuspect, SuspectSummary};
use crate::services::log_parser::{LogEntry, LogFileInfo, LogFormat, LogLevel, LogSession};
use crate::ui::{OpenTarget, icons, theme};

const CARD_MARGIN: f32 = 12.0;
const SIDEBAR_ROW_HEIGHT: f32 = 28.0;
const ISSUE_ICON_SIZE: f32 = 16.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum LogSort {
    #[default]
    Newest,
    Oldest,
    Frequent,
}

impl LogSort {
    pub(crate) const ALL: [Self; 3] = [Self::Newest, Self::Oldest, Self::Frequent];

    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::Newest => "Newest first",
            Self::Oldest => "Oldest first",
            Self::Frequent => "Most frequent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) enum LogModFilter {
    #[default]
    All,
    Unattributed,
    Mod(PackageId),
}

#[derive(Debug, Clone)]
pub(crate) struct LogFilter {
    pub(crate) search: String,
    pub(crate) show_errors: bool,
    pub(crate) show_warnings: bool,
    pub(crate) show_info: bool,
    pub(crate) show_debug: bool,
    pub(crate) mod_filter: LogModFilter,
    pub(crate) sort: LogSort,
    pub(crate) show_suspects: bool,
}

impl Default for LogFilter {
    fn default() -> Self {
        Self {
            search: String::new(),
            show_errors: true,
            show_warnings: true,
            show_info: false,
            show_debug: false,
            mod_filter: LogModFilter::All,
            sort: LogSort::Newest,
            show_suspects: true,
        }
    }
}

impl LogFilter {
    fn is_default(&self) -> bool {
        self.search.trim().is_empty()
            && self.show_errors
            && self.show_warnings
            && !self.show_info
            && !self.show_debug
            && self.mod_filter == LogModFilter::All
    }

    fn clear(&mut self) {
        self.search.clear();
        self.show_errors = true;
        self.show_warnings = true;
        self.show_info = false;
        self.show_debug = false;
        self.mod_filter = LogModFilter::All;
    }

    fn level_visible(&self, level: LogLevel) -> bool {
        match level {
            LogLevel::Error => self.show_errors,
            LogLevel::Warning => self.show_warnings,
            LogLevel::Info => self.show_info,
            LogLevel::Debug => self.show_debug,
        }
    }
}

pub(crate) struct LogView<'a> {
    pub(crate) sessions: &'a [LogFileInfo],
    pub(crate) session: LogSession,
    pub(crate) status: &'a LogLoadStatus,
    pub(crate) report: Option<&'a LogReport>,
    pub(crate) mods: &'a ModCollection,
    pub(crate) filter: &'a mut LogFilter,
    pub(crate) selected_issue: &'a mut Option<usize>,
    pub(crate) auto_refresh: &'a mut bool,
    pub(crate) data_directory: Option<&'a Path>,
}

pub(crate) enum LogAction {
    Reload,
    SelectSession(LogSession),
    SelectMod(PackageId),
    SetModEnabled {
        package_id: PackageId,
        enabled: bool,
    },
    Open(OpenTarget),
}

pub(crate) fn show_logs(ui: &mut egui::Ui, view: LogView<'_>) -> Option<LogAction> {
    let LogView {
        sessions,
        session,
        status,
        report,
        mods,
        filter,
        selected_issue,
        auto_refresh,
        data_directory,
    } = view;

    let mut action = None;

    theme::apply_square_style(ui);
    ui.add_space(2.0);

    ui.horizontal(|ui| {
        for info in sessions {
            let count = if info.session == session
                && report.is_some_and(|report| report.counts.error > 0)
            {
                Some(report.map_or(0, |report| report.counts.error).to_string())
            } else {
                None
            };

            if theme::tab_with_count(
                ui,
                session == info.session,
                match info.session {
                    LogSession::Current => "Current",
                    LogSession::Previous => "Previous",
                },
                count,
            )
            .on_hover_text(format!(
                "{}\n{:.1} KB",
                info.path.display(),
                info.size as f64 / 1024.0
            ))
            .clicked()
                && session != info.session
            {
                action = Some(LogAction::SelectSession(info.session));
            }
        }

        if matches!(status, LogLoadStatus::Loading) {
            ui.add(egui::Spinner::new().size(14.0));
        }

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let reload = ui
                .add_enabled_ui(!matches!(status, LogLoadStatus::Loading), |ui| {
                    theme::secondary_button_sized(
                        ui,
                        "Reload",
                        Vec2::new(76.0, theme::CONTROL_HEIGHT),
                    )
                })
                .inner
                .on_disabled_hover_text("The log is already being read");

            if reload.clicked() {
                action = Some(LogAction::Reload);
            }

            theme::checkbox(ui, auto_refresh, "Live")
                .on_hover_text("Reload automatically when RimWorld appends to the log");
        });
    });

    ui.add_space(6.0);

    match status {
        LogLoadStatus::Loading if report.is_none() => {
            show_loading(ui);
            return action;
        }
        LogLoadStatus::Failed(error) => {
            show_failure(ui, error, data_directory, &mut action);
        }
        LogLoadStatus::Idle if sessions.is_empty() => {
            show_missing_log(ui, data_directory, &mut action);
            return action;
        }
        _ => {}
    }

    let Some(report) = report else {
        show_missing_log(ui, data_directory, &mut action);
        return action;
    };

    show_toolbar(ui, report, filter, mods, selected_issue);
    ui.add_space(6.0);

    if report.document.entries.is_empty() {
        ui.vertical_centered(|ui| {
            ui.add_space(48.0);
            ui.label(RichText::new("The log is empty").text_style(theme::strong_style()));
            theme::hint(ui, "RimWorld has not written anything to this session yet.");
        });

        return action;
    }

    let visible = visible_issues(report, filter);
    let selected = selected_issue.filter(|index| visible.contains(index));

    if selected != *selected_issue {
        *selected_issue = selected.or_else(|| visible.first().copied());
    }

    // Give each pane a fixed screen rectangle and advance the parent cursor
    // manually. This keeps long messages and stack traces from influencing
    // sibling pane widths or overflowing off-screen.
    let spacing = ui.spacing().item_spacing.x;
    let available_width = ui.available_width();
    let body_height = ui.available_height().max(240.0);
    let top_left = ui.cursor().min;

    let show_sidebar = !report.suspects.is_empty() && filter.show_suspects;
    let mut cursor_x = top_left.x;

    if show_sidebar {
        let sidebar_width = (available_width * 0.22).clamp(205.0, 250.0);
        let sidebar_rect = egui::Rect::from_min_size(
            egui::pos2(cursor_x, top_left.y),
            Vec2::new(sidebar_width, body_height),
        );

        let mut sidebar_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(sidebar_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        sidebar_ui.set_width(sidebar_width);
        sidebar_ui.set_clip_rect(sidebar_rect);

        if let Some(sidebar_action) = show_suspects_sidebar(
            &mut sidebar_ui,
            report,
            filter,
            mods,
            selected_issue,
            sidebar_width,
        ) {
            action = Some(sidebar_action);
        }

        cursor_x += sidebar_width + spacing;
    }

    let remaining_width = (top_left.x + available_width - cursor_x).max(240.0);
    let list_column_width = ((remaining_width - spacing) / 2.0).max(120.0);
    let detail_column_width = (remaining_width - list_column_width - spacing).max(120.0);

    let list_rect = egui::Rect::from_min_size(
        egui::pos2(cursor_x, top_left.y),
        Vec2::new(list_column_width, body_height),
    );
    let detail_rect = egui::Rect::from_min_size(
        egui::pos2(cursor_x + list_column_width + spacing, top_left.y),
        Vec2::new(detail_column_width, body_height),
    );

    {
        let mut list_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(list_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        list_ui.set_width(list_column_width);
        list_ui.set_clip_rect(list_rect);
        show_issue_list(
            &mut list_ui,
            report,
            &visible,
            selected_issue,
            list_column_width,
        );
    }

    let selected = *selected_issue;
    let issue = selected.and_then(|index| report.issues.get(index));

    {
        let mut detail_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(detail_rect)
                .layout(egui::Layout::top_down(egui::Align::Min)),
        );
        detail_ui.set_width(detail_column_width);
        detail_ui.set_clip_rect(detail_rect);

        if let Some(detail_action) =
            show_issue_detail(&mut detail_ui, report, issue, mods, detail_column_width)
        {
            action = Some(detail_action);
        }
    }

    ui.advance_cursor_after_rect(egui::Rect::from_min_size(
        top_left,
        Vec2::new(available_width, body_height),
    ));

    action
}

fn show_loading(ui: &mut egui::Ui) {
    ui.vertical_centered(|ui| {
        ui.add_space(64.0);
        ui.add(egui::Spinner::new().size(22.0));
        ui.add_space(8.0);
        ui.label(RichText::new("Reading Player.log").text_style(theme::strong_style()));
        theme::hint(ui, "Large logs can take a moment to parse.");
    });
}

fn show_failure(
    ui: &mut egui::Ui,
    error: &str,
    data_directory: Option<&Path>,
    action: &mut Option<LogAction>,
) {
    egui::Frame::new()
        .fill(theme::tint(theme::DANGER, 22))
        .inner_margin(Margin::symmetric(12, 10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.label(
                RichText::new("Could not read the log")
                    .text_style(theme::strong_style())
                    .color(theme::DANGER),
            );
            theme::hint(ui, error);

            if let Some(data_directory) = data_directory {
                ui.add_space(6.0);

                if theme::secondary_button_sized(
                    ui,
                    "Open game folder",
                    Vec2::new(140.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    *action = Some(LogAction::Open(OpenTarget::Folder(
                        data_directory.to_path_buf(),
                    )));
                }
            }
        });
}

fn show_missing_log(
    ui: &mut egui::Ui,
    data_directory: Option<&Path>,
    action: &mut Option<LogAction>,
) {
    ui.vertical_centered(|ui| {
        ui.add_space(48.0);
        ui.label(RichText::new("No Player.log found").text_style(theme::strong_style()));
        theme::hint(
            ui,
            "Start RimWorld once so the game creates a log, or check the configuration folder in Settings.",
        );

        if let Some(data_directory) = data_directory {
            ui.add_space(4.0);
            theme::hint(
                ui,
                format!(
                    "Expected at {} or {}",
                    data_directory.join("Logs").join("Player.log").display(),
                    data_directory.join("Player.log").display()
                ),
            );

            ui.add_space(10.0);

            if theme::primary_button(ui, "Open game folder").clicked() {
                *action = Some(LogAction::Open(OpenTarget::Folder(
                    data_directory.to_path_buf(),
                )));
            }
        }
    });
}

fn show_toolbar(
    ui: &mut egui::Ui,
    report: &LogReport,
    filter: &mut LogFilter,
    mods: &ModCollection,
    selected_issue: &mut Option<usize>,
) {
    egui::containers::Sides::new()
        .shrink_left()
        .height(theme::CONTROL_HEIGHT)
        .show(
            ui,
            |ui| {
                theme::search_field(
                    ui,
                    &mut filter.search,
                    "Search errors, mod names and categories",
                );
            },
            |ui| {
                egui::ComboBox::from_id_salt("log_sort")
                    .selected_text(filter.sort.label())
                    .width(140.0)
                    .show_ui(ui, |ui| {
                        for candidate in LogSort::ALL {
                            ui.selectable_value(&mut filter.sort, candidate, candidate.label());
                        }
                    });

                if !report.suspects.is_empty() {
                    let toggle_label = format!("Suspected ({})", report.suspects.len());
                    let active = filter.show_suspects || filter.mod_filter != LogModFilter::All;
                    let color = if active { theme::ACCENT } else { theme::TEXT };
                    let response = theme::compact_button(ui, &toggle_label, color, theme::ACCENT);

                    if response.clicked() {
                        filter.show_suspects = !filter.show_suspects;
                    }

                    response.on_hover_text(if filter.show_suspects {
                        "Hide suspected mods sidebar"
                    } else {
                        "Show suspected mods sidebar"
                    });
                }
            },
        );

    ui.add_space(6.0);

    egui::containers::Sides::new().shrink_left().show(
        ui,
        |ui| {
            ui.horizontal_wrapped(|ui| {
                level_filter_checkbox(ui, &mut filter.show_errors, "Errors", report.counts.error);
                level_filter_checkbox(
                    ui,
                    &mut filter.show_warnings,
                    "Warnings",
                    report.counts.warning,
                );
                level_filter_checkbox(ui, &mut filter.show_info, "Info", report.counts.info);
                level_filter_checkbox(ui, &mut filter.show_debug, "Debug", report.counts.debug);

                if !filter.show_suspects
                    && let Some(active_mod_label) = active_mod_filter_label(&filter.mod_filter, mods)
                {
                    let chip = theme::chip_button(
                        ui,
                        format!("Mod: {} ×", short_name(&active_mod_label, 28)),
                        theme::ACCENT,
                        theme::DANGER,
                    );

                    if chip.clicked() {
                        filter.mod_filter = LogModFilter::All;
                        *selected_issue = None;
                    }

                    chip.on_hover_text("Click to clear the mod filter");
                }

                if !filter.is_default() && theme::link(ui, "Clear filters").clicked() {
                    filter.clear();
                    *selected_issue = None;
                }
            });
        },
        |ui| {
            let entries_hint = theme::hint(ui, format!("{} entries", report.counts.total()));

            if let Some(path) = report.document.header.documented_path.as_ref() {
                entries_hint.on_hover_text(path);
            }

            if report.document.truncated {
                theme::chip(ui, "Partial log", theme::WARNING).on_hover_text(
                    "This log is larger than the viewer's memory budget. Startup and recent errors are shown.",
                );
            }

            if report.document.format == LogFormat::Legacy {
                theme::chip(ui, "Severity inferred", theme::TEXT_FAINT).on_hover_text(
                    "RimWorld 1.6 and older logs do not store a level, so errors are recognised from their text.",
                );
            }

            if let Some(version) = &report.document.header.version {
                theme::chip(ui, version.clone(), theme::TEXT_MUTED);
            }
        },
    );
}

fn level_filter_checkbox(ui: &mut egui::Ui, checked: &mut bool, label: &str, count: usize) {
    let text = if count > 0 {
        format!("{label} ({count})")
    } else {
        label.to_owned()
    };

    theme::checkbox(ui, checked, text);
}

fn active_mod_filter_label(mod_filter: &LogModFilter, mods: &ModCollection) -> Option<String> {
    match mod_filter {
        LogModFilter::All => None,
        LogModFilter::Unattributed => Some("Unattributed".to_owned()),
        LogModFilter::Mod(package_id) => Some(
            find_mod(mods, package_id)
                .map(|rimworld_mod| rimworld_mod.name.clone())
                .unwrap_or_else(|| package_id.as_str().to_owned()),
        ),
    }
}

fn show_suspects_sidebar(
    ui: &mut egui::Ui,
    report: &LogReport,
    filter: &mut LogFilter,
    mods: &ModCollection,
    selected_issue: &mut Option<usize>,
    column_width: f32,
) -> Option<LogAction> {
    let mut action = None;
    let card_width = (column_width - CARD_MARGIN * 2.0).max(120.0);
    let row_width = (card_width - theme::SCROLL_GUTTER).max(100.0);

    let level_issue_count = report
        .issues
        .iter()
        .filter(|issue| filter.level_visible(issue.level))
        .count();
    let unattributed_count = report
        .issues
        .iter()
        .filter(|issue| filter.level_visible(issue.level) && issue.suspects.is_empty())
        .count();

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(CARD_MARGIN as i8),
        |ui| {
            ui.set_width(card_width);
            ui.set_min_height(ui.available_height());

            ui.horizontal(|ui| {
                theme::title(ui, "Suspected");
                theme::chip(ui, report.suspects.len().to_string(), theme::TEXT_MUTED);

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if sidebar_collapse_button(ui)
                        .on_hover_text("Hide suspected mods sidebar")
                        .clicked()
                    {
                        filter.show_suspects = false;
                    }
                });
            });

            ui.add_space(6.0);
            ui.spacing_mut().item_spacing.y = 3.0;

            let all_response = show_sidebar_row(
                ui,
                "sidebar_all_issues",
                "All issues",
                level_issue_count,
                theme::TEXT,
                theme::TEXT_MUTED,
                filter.mod_filter == LogModFilter::All,
                row_width,
            );
            if all_response.clicked() && filter.mod_filter != LogModFilter::All {
                filter.mod_filter = LogModFilter::All;
                *selected_issue = None;
            }

            if unattributed_count > 0 || filter.mod_filter == LogModFilter::Unattributed {
                let unattributed_response = show_sidebar_row(
                    ui,
                    "sidebar_unattributed",
                    "Unattributed",
                    unattributed_count,
                    theme::TEXT_MUTED,
                    theme::TEXT_FAINT,
                    filter.mod_filter == LogModFilter::Unattributed,
                    row_width,
                );
                if unattributed_response.clicked() {
                    filter.mod_filter = if filter.mod_filter == LogModFilter::Unattributed {
                        LogModFilter::All
                    } else {
                        LogModFilter::Unattributed
                    };
                    *selected_issue = None;
                }
            }

            ui.add_space(2.0);
            theme::menu_separator(ui);
            ui.add_space(2.0);

            egui::ScrollArea::vertical()
                .id_salt("log_suspects_sidebar")
                .auto_shrink([false, false])
                .show_rows(ui, SIDEBAR_ROW_HEIGHT, report.suspects.len(), |ui, rows| {
                    ui.set_max_width(row_width);

                    for row in rows {
                        let Some(suspect) = report.suspects.get(row) else {
                            continue;
                        };

                        let is_selected =
                            filter.mod_filter == LogModFilter::Mod(suspect.package_id.clone());
                        let name_color = if suspect.enabled {
                            theme::TEXT
                        } else {
                            theme::TEXT_MUTED
                        };
                        let badge_color = if !suspect.enabled {
                            theme::TEXT_FAINT
                        } else if suspect.errors > 0 {
                            theme::DANGER
                        } else {
                            theme::WARNING
                        };

                        let response = show_sidebar_row(
                            ui,
                            ("sidebar_suspect", suspect.package_id.as_str()),
                            &suspect.name,
                            suspect.issues,
                            name_color,
                            badge_color,
                            is_selected,
                            row_width,
                        );

                        if response.clicked() {
                            filter.mod_filter = if is_selected {
                                LogModFilter::All
                            } else {
                                LogModFilter::Mod(suspect.package_id.clone())
                            };
                            *selected_issue = None;
                        }

                        let rimworld_mod = find_mod(mods, &suspect.package_id);
                        show_suspect_context_menu(&response, suspect, rimworld_mod, &mut action);

                        response.on_hover_text(suspect_tooltip(suspect));
                    }
                });
        },
    );

    action
}

fn sidebar_collapse_button(ui: &mut egui::Ui) -> egui::Response {
    let size = Vec2::new(24.0, 22.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        if hovered {
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(theme::RADIUS_SM),
                theme::CONTROL_HOVER,
            );
        }

        let color = if hovered {
            theme::TEXT
        } else {
            theme::TEXT_MUTED
        };
        let galley = ui
            .painter()
            .layout_no_wrap("«".to_owned(), theme::medium_font(13.0), color);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

#[allow(clippy::too_many_arguments)]
fn show_sidebar_row(
    ui: &mut egui::Ui,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    label: &str,
    count: usize,
    text_color: Color32,
    badge_color: Color32,
    selected: bool,
    row_width: f32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(row_width, SIDEBAR_ROW_HEIGHT),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        paint_selectable_row_backdrop(ui, rect, id_salt, response.hovered(), selected);

        let count_galley = ui.painter().layout_no_wrap(
            count.to_string(),
            theme::medium_font(11.0),
            Color32::PLACEHOLDER,
        );
        let badge_pad_x = 6.0;
        let badge_pad_y = 2.0;
        let badge_size = Vec2::new(
            (count_galley.size().x + badge_pad_x * 2.0).max(20.0),
            count_galley.size().y + badge_pad_y * 2.0,
        );
        let badge_rect = egui::Rect::from_min_size(
            egui::pos2(
                rect.right() - 6.0 - badge_size.x,
                rect.center().y - badge_size.y / 2.0,
            ),
            badge_size,
        );

        ui.painter().rect_filled(
            badge_rect,
            egui::CornerRadius::same(theme::RADIUS_SM),
            theme::tint(badge_color, 30),
        );
        ui.painter().galley(
            badge_rect.center() - count_galley.size() / 2.0,
            count_galley,
            badge_color,
        );

        let text_left = rect.left() + 8.0;
        let text_right = (badge_rect.left() - 8.0).max(text_left + 1.0);
        let text_max_width = (text_right - text_left).max(1.0);
        let text_rect = egui::Rect::from_min_max(
            egui::pos2(text_left, rect.top()),
            egui::pos2(text_right, rect.bottom()),
        );
        let title_color = if selected || response.hovered() {
            theme::TEXT
        } else {
            text_color
        };
        let label_galley = single_line_galley(
            ui,
            label,
            TextStyle::Small.resolve(ui.style()),
            title_color,
            text_max_width,
        );
        let clipped_painter = ui
            .painter()
            .with_clip_rect(text_rect.intersect(ui.clip_rect()));
        clipped_painter.galley(
            egui::pos2(text_left, rect.center().y - label_galley.size().y / 2.0),
            label_galley,
            title_color,
        );
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn show_suspect_context_menu(
    response: &egui::Response,
    suspect: &SuspectSummary,
    rimworld_mod: Option<&RimworldMod>,
    action: &mut Option<LogAction>,
) {
    egui::Popup::context_menu(response)
        .frame(theme::menu_frame())
        .show(|ui| {
            ui.set_min_width(210.0);
            ui.spacing_mut().item_spacing.y = 4.0;
            ui.spacing_mut().button_padding = Vec2::new(8.0, 4.0);

            theme::menu_inset(ui, |ui| {
                ui.label(RichText::new(&suspect.name).text_style(theme::strong_style()));
                ui.label(
                    RichText::new(suspect.package_id.as_str())
                        .monospace()
                        .small()
                        .color(theme::TEXT_MUTED),
                );
            });

            theme::menu_separator(ui);

            if ui.button("Show in Mods").clicked() {
                *action = Some(LogAction::SelectMod(suspect.package_id.clone()));
                ui.close();
            }

            let toggle_label = if suspect.enabled {
                "Disable mod"
            } else {
                "Enable mod"
            };
            if ui.button(toggle_label).clicked() {
                *action = Some(LogAction::SetModEnabled {
                    package_id: suspect.package_id.clone(),
                    enabled: !suspect.enabled,
                });
                ui.close();
            }

            if let Some(rimworld_mod) = rimworld_mod {
                if ui.button("Open folder").clicked() {
                    *action = Some(LogAction::Open(OpenTarget::Folder(
                        rimworld_mod.folder.clone(),
                    )));
                    ui.close();
                }

                if let ModSource::SteamWorkshop { workshop_id } = &rimworld_mod.source
                    && ui.button("Open Workshop page").clicked()
                {
                    *action = Some(LogAction::Open(OpenTarget::Url(format!(
                        "https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}"
                    ))));
                    ui.close();
                }
            }
        });
}

fn suspect_tooltip(suspect: &SuspectSummary) -> String {
    let mut parts = Vec::new();
    if suspect.errors > 0 {
        parts.push(format!(
            "{} error{}",
            suspect.errors,
            plural(suspect.errors)
        ));
    }
    if suspect.warnings > 0 {
        parts.push(format!(
            "{} warning{}",
            suspect.warnings,
            plural(suspect.warnings)
        ));
    }

    let summary = if parts.is_empty() {
        format!("{} issue{}", suspect.issues, plural(suspect.issues))
    } else {
        parts.join(", ")
    };

    let status = if suspect.enabled {
        ""
    } else {
        "\nCurrently disabled"
    };

    format!(
        "{}\n{}\n{}{status}\nClick to filter, right-click for actions",
        suspect.name,
        suspect.package_id.as_str(),
        summary,
    )
}

fn visible_issues(report: &LogReport, filter: &LogFilter) -> Vec<usize> {
    let search = filter.search.trim().to_lowercase();

    let mut visible = report
        .issues
        .iter()
        .enumerate()
        .filter(|(_, issue)| {
            if !filter.level_visible(issue.level) {
                return false;
            }

            match &filter.mod_filter {
                LogModFilter::All => {}
                LogModFilter::Unattributed => {
                    if !issue.suspects.is_empty() {
                        return false;
                    }
                }
                LogModFilter::Mod(package_id) => {
                    if !issue
                        .suspects
                        .iter()
                        .any(|suspect| &suspect.package_id == package_id)
                    {
                        return false;
                    }
                }
            }

            if !search.is_empty() {
                let title_matches = issue.title.to_lowercase().contains(&search);
                let category_matches = issue
                    .category
                    .as_ref()
                    .is_some_and(|category| category.to_lowercase().contains(&search));
                let suspect_matches = issue
                    .suspects
                    .iter()
                    .any(|suspect| suspect.name.to_lowercase().contains(&search));

                if !title_matches && !category_matches && !suspect_matches {
                    return false;
                }
            }

            true
        })
        .map(|(index, _)| index)
        .collect::<Vec<_>>();

    match filter.sort {
        LogSort::Newest => {
            visible.sort_by_key(|&index| std::cmp::Reverse(report.issues[index].last_index))
        }
        LogSort::Oldest => visible.sort_by_key(|&index| report.issues[index].first_index),
        LogSort::Frequent => visible.sort_by(|&left, &right| {
            report.issues[right]
                .count
                .cmp(&report.issues[left].count)
                .then_with(|| {
                    report.issues[right]
                        .last_index
                        .cmp(&report.issues[left].last_index)
                })
        }),
    }

    visible
}

fn show_issue_list(
    ui: &mut egui::Ui,
    report: &LogReport,
    visible: &[usize],
    selected_issue: &mut Option<usize>,
    column_width: f32,
) {
    let selected = *selected_issue;
    let card_width = (column_width - CARD_MARGIN * 2.0).max(120.0);

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(CARD_MARGIN as i8),
        |ui| {
            ui.set_width(card_width);
            ui.set_min_height(ui.available_height());

            // Rows must stop before the floating scrollbar so text never
            // collides with it or extends into the details pane.
            let row_width = (card_width - theme::SCROLL_GUTTER).max(120.0);

            ui.horizontal(|ui| {
                theme::title(ui, "Log issues");
                theme::chip(
                    ui,
                    format!("{} / {}", visible.len(), report.issues.len()),
                    theme::TEXT_MUTED,
                );
            });

            ui.add_space(6.0);

            if visible.is_empty() {
                theme::hint(ui, "No issues match these filters");
                return;
            }

            ui.spacing_mut().item_spacing.y = 4.0;

            egui::ScrollArea::vertical()
                .id_salt("log_issue_list")
                .auto_shrink([false, false])
                .show_rows(ui, theme::ROW_HEIGHT, visible.len(), |ui, rows| {
                    ui.set_max_width(row_width);

                    for row in rows {
                        let issue_index = visible[row];
                        let Some(issue) = report.issues.get(issue_index) else {
                            continue;
                        };

                        if show_issue_row(
                            ui,
                            issue_index,
                            issue,
                            selected == Some(issue_index),
                            row_width,
                        ) {
                            *selected_issue = Some(issue_index);
                        }
                    }
                });
        },
    );
}

fn show_issue_row(
    ui: &mut egui::Ui,
    issue_index: usize,
    issue: &LogIssue,
    selected: bool,
    row_width: f32,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(row_width, theme::ROW_HEIGHT),
        egui::Sense::click(),
    );

    if ui.is_rect_visible(rect) {
        paint_selectable_row_backdrop(
            ui,
            rect,
            ("log_issue_row", issue_index),
            response.hovered(),
            selected,
        );

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 8.0 + ISSUE_ICON_SIZE / 2.0, rect.center().y),
            Vec2::splat(ISSUE_ICON_SIZE),
        );
        egui::Image::new(icons::log_level_icon(issue.level))
            .fit_to_exact_size(icon_rect.size())
            .paint_at(ui, icon_rect);

        let right_pad = 8.0;
        let mut title_right = rect.right() - right_pad;

        if issue.count > 1 {
            let count_galley = ui.painter().layout_no_wrap(
                format!("×{}", issue.count),
                TextStyle::Small.resolve(ui.style()),
                theme::TEXT_MUTED,
            );
            let count_left = rect.right() - right_pad - count_galley.size().x;
            ui.painter().galley(
                egui::pos2(count_left, rect.center().y - count_galley.size().y / 2.0),
                count_galley,
                theme::TEXT_MUTED,
            );
            title_right = count_left - 10.0;
        } else if row_width >= 240.0
            && let Some(suspect) = issue.suspects.first()
        {
            let max_suspect_width = (row_width * 0.28).min(120.0);
            let suspect_galley = single_line_galley(
                ui,
                &suspect.name,
                TextStyle::Small.resolve(ui.style()),
                theme::TEXT_FAINT,
                max_suspect_width,
            );
            let suspect_width = suspect_galley.size().x.min(max_suspect_width);
            let suspect_left = rect.right() - right_pad - suspect_width;
            let suspect_rect = egui::Rect::from_min_max(
                egui::pos2(suspect_left, rect.top()),
                egui::pos2(rect.right() - right_pad, rect.bottom()),
            );
            let suspect_painter = ui
                .painter()
                .with_clip_rect(suspect_rect.intersect(ui.clip_rect()));
            suspect_painter.galley(
                egui::pos2(
                    suspect_left,
                    rect.center().y - suspect_galley.size().y / 2.0,
                ),
                suspect_galley,
                theme::TEXT_FAINT,
            );
            title_right = suspect_left - 10.0;
        }

        let title_left = icon_rect.right() + 8.0;
        let title_max_width = (title_right - title_left).max(1.0);
        let title_rect = egui::Rect::from_min_max(
            egui::pos2(title_left, rect.top()),
            egui::pos2(title_left + title_max_width, rect.bottom()),
        );
        let title_color = if selected || response.hovered() {
            Color32::WHITE
        } else {
            theme::TEXT
        };
        let title_galley = single_line_galley(
            ui,
            &issue.title,
            TextStyle::Body.resolve(ui.style()),
            title_color,
            title_max_width,
        );
        let title_painter = ui
            .painter()
            .with_clip_rect(title_rect.intersect(ui.clip_rect()));
        title_painter.galley(
            egui::pos2(title_left, rect.center().y - title_galley.size().y / 2.0),
            title_galley,
            title_color,
        );
    }

    let hover = if issue.suspects.is_empty() {
        format!(
            "[{}] {}\n{} occurrence{}.",
            issue.level.label(),
            issue.title,
            issue.count,
            plural(issue.count),
        )
    } else {
        let suspects = issue
            .suspects
            .iter()
            .take(3)
            .map(|suspect| suspect.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");

        format!(
            "[{}] {}\n{} occurrence{} - Matched: {suspects}",
            issue.level.label(),
            issue.title,
            issue.count,
            plural(issue.count),
        )
    };

    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(hover)
        .clicked()
}

fn paint_selectable_row_backdrop(
    ui: &egui::Ui,
    rect: egui::Rect,
    id_salt: impl std::hash::Hash + std::fmt::Debug,
    hovered: bool,
    selected: bool,
) {
    let hover = ui
        .ctx()
        .animate_bool_with_time(ui.id().with(("hover", &id_salt)), hovered, 0.12);
    let selection =
        ui.ctx()
            .animate_bool_with_time(ui.id().with(("selection", &id_salt)), selected, 0.12);

    let fill = if selection > 0.0 {
        theme::tint(theme::ACCENT, (55.0 * selection + 20.0 * hover) as u8)
    } else if hover > 0.0 {
        theme::tint(theme::SURFACE_HOVER, (hover * 255.0) as u8)
    } else {
        Color32::TRANSPARENT
    };

    if fill != Color32::TRANSPARENT {
        ui.painter()
            .rect_filled(rect, egui::CornerRadius::same(theme::RADIUS_MD), fill);
    }

    if selected {
        let accent_bar = egui::Rect::from_min_max(
            egui::pos2(rect.left() + 1.0, rect.top() + 4.0),
            egui::pos2(rect.left() + 3.0, rect.bottom() - 4.0),
        );

        ui.painter()
            .rect_filled(accent_bar, egui::CornerRadius::same(1), theme::ACCENT);
    }
}

fn single_line_galley(
    ui: &egui::Ui,
    text: &str,
    font_id: egui::FontId,
    color: Color32,
    max_width: f32,
) -> Arc<egui::Galley> {
    let sanitized = if text.contains(['\n', '\r']) {
        text.replace(['\r', '\n'], " ")
    } else {
        text.to_owned()
    };

    let mut job = egui::text::LayoutJob::simple(sanitized, font_id, color, max_width.max(1.0));
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('…');

    ui.painter().layout_job(job)
}

fn show_level_badge(ui: &mut egui::Ui, level: LogLevel) -> egui::Response {
    const ICON_SIZE: f32 = 14.0;
    const GAP: f32 = 5.0;
    const PAD_X: f32 = 7.0;
    const PAD_Y: f32 = 3.0;

    let color = level_color(level);
    let galley = ui.painter().layout_no_wrap(
        level.label().to_owned(),
        theme::medium_font(12.0),
        Color32::PLACEHOLDER,
    );
    let size = Vec2::new(
        PAD_X * 2.0 + ICON_SIZE + GAP + galley.size().x,
        ICON_SIZE.max(galley.size().y) + PAD_Y * 2.0,
    );

    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        ui.painter().rect_filled(
            rect,
            egui::CornerRadius::same(theme::RADIUS_SM),
            theme::tint(color, 34),
        );

        let icon_rect = egui::Rect::from_center_size(
            egui::pos2(rect.left() + PAD_X + ICON_SIZE / 2.0, rect.center().y),
            Vec2::splat(ICON_SIZE),
        );
        egui::Image::new(icons::log_level_icon(level))
            .fit_to_exact_size(icon_rect.size())
            .paint_at(ui, icon_rect);

        ui.painter().galley(
            egui::pos2(
                icon_rect.right() + GAP,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }

    response
}

fn show_issue_detail(
    ui: &mut egui::Ui,
    report: &LogReport,
    issue: Option<&LogIssue>,
    mods: &ModCollection,
    column_width: f32,
) -> Option<LogAction> {
    let mut action = None;
    let card_width = (column_width - CARD_MARGIN * 2.0).max(120.0);
    let content_width = (card_width - theme::SCROLL_GUTTER).max(80.0);

    theme::gradient_surface(
        ui,
        theme::SURFACE_HOVER,
        theme::SURFACE,
        Margin::same(CARD_MARGIN as i8),
        |ui| {
            ui.set_width(card_width);
            ui.set_min_height(ui.available_height());

            let Some(issue) = issue else {
                theme::title(ui, "Issue details");
                ui.add_space(8.0);
                theme::hint(
                    ui,
                    "Select an issue to see its message, stack trace and matched mods",
                );
                return;
            };

            let entry = report.document.entries.get(issue.last_index);

            egui::ScrollArea::vertical()
                .id_salt("log_issue_detail")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    // Keep text clear of the floating scrollbar, like the
                    // Workshop grid reserves a lane for it.
                    ui.set_width(content_width);

                    ui.horizontal(|ui| {
                        show_level_badge(ui, issue.level);
                        theme::title(ui, "Issue details");

                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Center),
                            |ui| {
                                if let Some(entry) = entry
                                    && theme::compact_button(
                                        ui,
                                        "Copy",
                                        theme::TEXT,
                                        theme::ACCENT,
                                    )
                                    .clicked()
                                {
                                    ui.ctx().copy_text(entry.full_text());
                                }

                                theme::hint(
                                    ui,
                                    format!(
                                        "{} occurrence{}",
                                        issue.count,
                                        plural(issue.count)
                                    ),
                                );
                            },
                        );
                    });

                    ui.add_space(6.0);

                    ui.add(
                        egui::Label::new(
                            RichText::new(&issue.title)
                                .text_style(theme::strong_style())
                                .color(theme::TEXT),
                        )
                        .wrap(),
                    );

                    ui.add_space(4.0);

                    ui.horizontal_wrapped(|ui| {
                        if let Some(entry) = entry {
                            if let Some(timestamp) = &entry.timestamp {
                                theme::chip(ui, timestamp.clone(), theme::TEXT_MUTED);
                            }

                            if entry.line_number > 0 {
                                theme::hint(ui, format!("line {}", entry.line_number));
                            }
                        }

                        if let Some(category) = &issue.category {
                            theme::chip(ui, category.clone(), theme::TEXT_FAINT);
                        }
                    });

                    ui.add_space(10.0);
                    section_title(ui, "Suspected mods");

                    if issue.suspects.is_empty() {
                        egui::Frame::new()
                            .fill(theme::CANVAS)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                ui.label(
                                    RichText::new("No installed mod matched this entry")
                                        .small()
                                        .color(theme::TEXT_MUTED),
                                );
                                theme::hint(
                                    ui,
                                    "This may be a load-order or cross-mod problem. Use the message and stack trace below to narrow it down.",
                                );
                            });
                    } else {
                        ui.spacing_mut().item_spacing.y = 6.0;

                        for suspect in &issue.suspects {
                            if let Some(suspect_action) = show_suspect(ui, suspect, mods) {
                                action = Some(suspect_action);
                            }
                        }
                    }

                    if let Some(entry) = entry {
                        ui.add_space(10.0);
                        section_title(ui, "Message");

                        egui::Frame::new()
                            .fill(theme::CANVAS)
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());

                                let mut job = egui::text::LayoutJob::simple(
                                    entry.message.clone(),
                                    TextStyle::Monospace.resolve(ui.style()),
                                    theme::TEXT,
                                    (content_width - 20.0).max(80.0),
                                );
                                job.wrap.break_anywhere = true;

                                ui.add(egui::Label::new(job).selectable(true));
                            });

                        if !entry.details.is_empty() {
                            ui.add_space(10.0);
                            section_title(ui, "Stack trace");

                            egui::Frame::new()
                                .fill(theme::CANVAS)
                                .inner_margin(Margin::same(10))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());

                                    let mut job = details_job(ui, entry, &issue.suspects);
                                    job.wrap.max_width = (content_width - 20.0).max(80.0);
                                    job.wrap.break_anywhere = true;

                                    ui.add(
                                        egui::Label::new(job)
                                            .wrap()
                                            .selectable(true),
                                    );
                                });
                        }

                        let paths = mentioned_paths(entry);

                        if !paths.is_empty() {
                            ui.add_space(10.0);
                            section_title(ui, "Files in this entry");

                            for path in paths {
                                let is_folder = path.is_dir();
                                let response =
                                    theme::link(ui, path.display().to_string());

                                if response.clicked() {
                                    action = Some(LogAction::Open(if is_folder {
                                        OpenTarget::Folder(path)
                                    } else {
                                        OpenTarget::File(path)
                                    }));
                                }
                            }
                        }

                        if issue.count > 1 {
                            ui.add_space(10.0);

                            theme::collapsible(
                                ui,
                                "log_issue_occurrences",
                                format!("Occurrences ({})", issue.count),
                                false,
                                |ui| {
                                    for &sample_index in issue.samples.iter().take(20) {
                                        let Some(sample) =
                                            report.document.entries.get(sample_index)
                                        else {
                                            continue;
                                        };

                                        ui.horizontal(|ui| {
                                            if let Some(timestamp) = &sample.timestamp {
                                                theme::hint(ui, timestamp);
                                            } else if sample.line_number > 0 {
                                                theme::hint(
                                                    ui,
                                                    format!("line {}", sample.line_number),
                                                );
                                            }

                                            ui.add(
                                                egui::Label::new(
                                                    RichText::new(short_name(&sample.message, 90))
                                                        .small()
                                                        .color(theme::TEXT_MUTED),
                                                )
                                                .truncate(),
                                            );
                                        });
                                    }

                                    if issue.count > issue.samples.len() {
                                        theme::hint(
                                            ui,
                                            format!(
                                                "{} more occurrence(s) not shown",
                                                issue.count - issue.samples.len()
                                            ),
                                        );
                                    }
                                },
                            );
                        }
                    }
                });
        },
    );

    action
}

fn show_suspect(
    ui: &mut egui::Ui,
    suspect: &ModSuspect,
    mods: &ModCollection,
) -> Option<LogAction> {
    let mut action = None;
    let rimworld_mod = find_mod(mods, &suspect.package_id);

    egui::Frame::new()
        .fill(theme::CANVAS)
        .inner_margin(Margin::same(10))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());

            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(&suspect.name)
                        .text_style(theme::strong_style())
                        .color(theme::TEXT),
                );

                if suspect.official {
                    theme::chip(ui, "Base game", theme::TEXT_FAINT);
                } else if !suspect.enabled {
                    theme::chip(ui, "Disabled", theme::TEXT_MUTED);
                }
            });

            ui.add_space(2.0);

            ui.label(
                RichText::new(suspect.package_id.as_str())
                    .monospace()
                    .small()
                    .color(theme::TEXT_MUTED),
            );

            if !suspect.evidence.is_empty() {
                ui.add_space(3.0);

                for evidence in &suspect.evidence {
                    theme::hint(ui, format!("• {evidence}"));
                }
            }

            ui.add_space(6.0);

            ui.horizontal_wrapped(|ui| {
                if theme::primary_button_sized(
                    ui,
                    "Show in Mods",
                    Vec2::new(0.0, theme::CONTROL_HEIGHT),
                )
                .clicked()
                {
                    action = Some(LogAction::SelectMod(suspect.package_id.clone()));
                }

                if !suspect.official {
                    let (toggle_label, hover_color) = if suspect.enabled {
                        ("Disable", theme::DANGER)
                    } else {
                        ("Enable", theme::SUCCESS)
                    };

                    if theme::compact_button(ui, toggle_label, theme::TEXT, hover_color).clicked() {
                        action = Some(LogAction::SetModEnabled {
                            package_id: suspect.package_id.clone(),
                            enabled: !suspect.enabled,
                        });
                    }
                }

                if let Some(rimworld_mod) = rimworld_mod {
                    if theme::compact_button(ui, "Open folder", theme::TEXT, theme::ACCENT)
                        .clicked()
                    {
                        action = Some(LogAction::Open(OpenTarget::Folder(
                            rimworld_mod.folder.clone(),
                        )));
                    }

                    if let ModSource::SteamWorkshop { workshop_id } = &rimworld_mod.source
                        && theme::compact_button(ui, "Workshop", theme::TEXT, theme::ACCENT)
                            .clicked()
                    {
                        action = Some(LogAction::Open(OpenTarget::Url(format!(
                            "https://steamcommunity.com/sharedfiles/filedetails/?id={workshop_id}"
                        ))));
                    }
                }
            });
        });

    action
}

fn find_mod<'a>(mods: &'a ModCollection, package_id: &PackageId) -> Option<&'a RimworldMod> {
    mods.all
        .iter()
        .find(|rimworld_mod| &rimworld_mod.package_id == package_id)
}

fn details_job(ui: &egui::Ui, entry: &LogEntry, suspects: &[ModSuspect]) -> egui::text::LayoutJob {
    let font_id = TextStyle::Monospace.resolve(ui.style());
    let mut job = egui::text::LayoutJob::default();

    for (index, line) in entry.details.iter().enumerate() {
        let trimmed = line.trim();
        let color = if line_mentions_suspect(trimmed, suspects) {
            theme::ACCENT
        } else if is_framework_line(trimmed) {
            theme::TEXT_FAINT
        } else {
            theme::TEXT_MUTED
        };

        job.append(
            line,
            0.0,
            egui::text::TextFormat {
                font_id: font_id.clone(),
                color,
                ..Default::default()
            },
        );

        if index + 1 < entry.details.len() {
            job.append("\n", 0.0, egui::text::TextFormat::default());
        }
    }

    job
}

fn line_mentions_suspect(line: &str, suspects: &[ModSuspect]) -> bool {
    let line = line.to_lowercase();

    suspects.iter().any(|suspect| {
        line.contains(&suspect.name.to_lowercase()) || line.contains(suspect.package_id.as_str())
    })
}

fn is_framework_line(line: &str) -> bool {
    const PREFIXES: &[&str] = &[
        "at System.",
        "at Microsoft.",
        "at Mono.",
        "at UnityEngine.",
        "at Unity.",
        "at Verse.",
        "at RimWorld.",
        "at RimWorldWaylith.",
        "at LudeonTK.",
        "at Brutal.",
        "at Waylith.",
        "at HarmonyLib.",
        "at Harmony.",
        "at 0Harmony.",
        "System.",
        "Verse.",
        "RimWorld.",
        "UnityEngine.",
        "Microsoft.",
        "Mono.",
        "Waylith.",
        "Brutal.",
        "HarmonyLib.",
    ];

    PREFIXES.iter().any(|prefix| line.starts_with(prefix))
}

fn mentioned_paths(entry: &LogEntry) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    let lines = std::iter::once(&entry.message).chain(entry.details.iter());

    for line in lines {
        for marker in [" in ", "file: ", "file:"] {
            let Some(position) = line.find(marker) else {
                continue;
            };

            let mut candidate = line[position + marker.len()..].trim().to_owned();
            candidate = candidate
                .trim_matches(|character| matches!(character, '"' | '\'' | ')' | ']' | ','))
                .to_owned();

            if let Some(colon) = candidate.rfind(':')
                && candidate[colon + 1..]
                    .chars()
                    .all(|character| character.is_ascii_digit())
                && !candidate[colon + 1..].is_empty()
            {
                candidate.truncate(colon);
            }

            let path = PathBuf::from(candidate);

            if (path.is_file() || path.is_dir()) && !paths.contains(&path) {
                paths.push(path);

                if paths.len() >= 6 {
                    return paths;
                }
            }
        }
    }

    paths
}

fn section_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        RichText::new(text)
            .font(theme::semibold_font(11.0))
            .color(theme::TEXT_FAINT),
    );
}

fn level_color(level: LogLevel) -> Color32 {
    match level {
        LogLevel::Error => theme::DANGER,
        LogLevel::Warning => theme::WARNING,
        LogLevel::Info => theme::ACCENT,
        LogLevel::Debug => theme::TEXT_FAINT,
    }
}

fn short_name(name: &str, limit: usize) -> String {
    if name.chars().count() <= limit {
        return name.to_owned();
    }

    let mut shortened = name
        .chars()
        .take(limit.saturating_sub(1))
        .collect::<String>();
    shortened.push('…');
    shortened
}

fn plural(count: usize) -> &'static str {
    if count == 1 { "" } else { "s" }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::services::log_analysis::{ModSnapshot, analyze};
    use crate::services::log_parser::parse_log_text;

    #[test]
    fn logs_view_renders_with_sidebar_open_and_collapsed() {
        let log_text = concat!(
            "2026-11-24 18:17:53.082  WARNING [Verse] [Squishy Melee] the host window changed (missing: SelectPawnPrimaryWeapon): only the options page of this mod is available and this title is intentionally long to test ellipsis truncation.\n",
            "2026-11-24 18:17:54.000  ERROR [Verse] Failed to load patch for local.gunrecoil.melee\n",
        );
        let document = parse_log_text(log_text);
        let mut mods = ModCollection::default();
        mods.all.push(RimworldMod {
            name: "SQUISHY Melee".to_owned(),
            package_id: PackageId::new("local.gunrecoil.melee").expect("valid package id"),
            description: String::new(),
            supported_versions: vec!["1.7".to_owned()],
            community_supported_versions: Vec::new(),
            loader_after: Vec::new(),
            loader_before: Vec::new(),
            force_load_after: Vec::new(),
            force_load_before: Vec::new(),
            mod_dependencies: Vec::new(),
            incompatible_with: Vec::new(),
            inferred_after: Vec::new(),
            is_patch_only: false,
            folder: PathBuf::from("Mods/SquishyMelee"),
            source: ModSource::Local,
            mod_type: crate::models::ModType::CSharp,
        });
        let report = analyze(document, &ModSnapshot::collect(&mods));
        assert_eq!(report.suspects.len(), 1);
        let sessions = vec![LogFileInfo {
            session: LogSession::Current,
            path: PathBuf::from("Player.log"),
            size: 2048,
        }];

        let context = egui::Context::default();
        theme::install(&context);
        egui_extras::install_image_loaders(&context);

        for show_suspects in [true, false] {
            let mut filter = LogFilter {
                show_suspects,
                ..LogFilter::default()
            };
            let mut selected_issue = Some(0);
            let mut auto_refresh = true;

            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1000.0, 640.0),
                )),
                ..egui::RawInput::default()
            };

            let mut output = context.run_ui(input, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    let _ = show_logs(
                        ui,
                        LogView {
                            sessions: &sessions,
                            session: LogSession::Current,
                            status: &LogLoadStatus::Ready,
                            report: Some(&report),
                            mods: &mods,
                            filter: &mut filter,
                            selected_issue: &mut selected_issue,
                            auto_refresh: &mut auto_refresh,
                            data_directory: None,
                        },
                    );
                });
            });
            output.textures_delta.clear();
        }
    }
}

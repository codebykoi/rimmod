use eframe::egui::{
    self, Color32, CornerRadius, CursorIcon, PointerButton, Rect, Sense, Stroke, StrokeKind, Vec2,
};

use crate::ui::{icons, theme};

pub(crate) const UNDO_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
pub(crate) const REDO_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
pub(crate) const ALTERNATE_REDO_SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(
    egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT),
    egui::Key::Z,
);
pub(crate) const SEARCH_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::F);
pub(crate) const SAVE_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
pub(crate) const RELOAD_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::NONE, egui::Key::F5);

const TITLE_BAR_HEIGHT: f32 = 40.0;
const WINDOW_BUTTON_WIDTH: f32 = 44.0;

pub(crate) enum TopPanelAction {
    Undo,
    Redo,
    Settings,
    RunWithCurrentMods,
    Save,
    Reload,
    ShowShortcuts,
    InstallUpdate,
}

/// The single status dot in the title bar. The colour carries the most urgent
/// state; details stay in the hover tooltip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StatusDot {
    Ready,
    Warning,
    Error,
    Unavailable,
}

impl StatusDot {
    fn color(self) -> Color32 {
        match self {
            Self::Ready => theme::SUCCESS,
            Self::Warning => theme::WARNING,
            Self::Error => theme::DANGER,
            Self::Unavailable => theme::TEXT_FAINT,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            Self::Ready => "Saved and ready.",
            Self::Warning => "Warnings or unsaved changes.",
            Self::Error => "Needs attention: save error, conflict or missing mods.",
            Self::Unavailable => "Load order unavailable. Check the game and config paths.",
        }
    }
}

pub(crate) struct TopPanelView<'a> {
    pub(crate) can_undo: bool,
    pub(crate) can_redo: bool,
    pub(crate) can_run_with_mods: bool,
    pub(crate) search: Option<&'a mut String>,
    pub(crate) status: StatusDot,
    pub(crate) available_update: Option<&'a str>,
    pub(crate) update_launching: bool,
}

pub(crate) fn show_top_panel(ui: &mut egui::Ui, view: TopPanelView<'_>) -> Option<TopPanelAction> {
    let TopPanelView {
        can_undo,
        can_redo,
        can_run_with_mods,
        search,
        status,
        available_update,
        update_launching,
    } = view;
    let mut left_request = None;
    let mut right_request = None;

    egui::Panel::top("title_bar")
        .exact_size(TITLE_BAR_HEIGHT)
        .show_separator_line(false)
        .frame(
            egui::Frame::new()
                .fill(theme::SURFACE)
                .inner_margin(egui::Margin::symmetric(8, 0)),
        )
        .show(ui, |ui| {
            // Register the whole bar as a drag target before adding widgets.
            // Widgets added later sit on top, so buttons and the search field
            // keep their own interactions.
            let drag = ui.interact(
                ui.max_rect(),
                ui.id().with("title_bar_drag"),
                Sense::click_and_drag(),
            );

            if drag.drag_started_by(PointerButton::Primary) {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
            }

            if drag.double_clicked() {
                toggle_maximized(ui.ctx());
            }

            egui::containers::Sides::new()
                .height(TITLE_BAR_HEIGHT)
                .show(
                    ui,
                    |ui| {
                        ui.horizontal(|ui| {
                            brand(ui);
                            ui.add_space(6.0);

                            let menu = menu_button(ui);

                            egui::Popup::menu(&menu)
                                .frame(theme::menu_frame())
                                .show(|ui| {
                                    ui.set_min_width(240.0);
                                    ui.spacing_mut().item_spacing.y = 4.0;
                                    ui.spacing_mut().button_padding = egui::vec2(8.0, 5.0);

                                    if menu_item(ui, "Save load order", Some(&SAVE_SHORTCUT), true)
                                    {
                                        left_request = Some(TopPanelAction::Save);
                                        ui.close();
                                    }

                                    if menu_item(ui, "Reload mods", Some(&RELOAD_SHORTCUT), true) {
                                        left_request = Some(TopPanelAction::Reload);
                                        ui.close();
                                    }

                                    theme::menu_separator(ui);

                                    if menu_item(ui, "Settings", None, true) {
                                        left_request = Some(TopPanelAction::Settings);
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
                                        left_request = Some(TopPanelAction::RunWithCurrentMods);
                                        ui.close();
                                    }

                                    theme::menu_separator(ui);

                                    if menu_item(ui, "Undo", Some(&UNDO_SHORTCUT), can_undo) {
                                        left_request = Some(TopPanelAction::Undo);
                                        ui.close();
                                    }

                                    if menu_item(ui, "Redo", Some(&REDO_SHORTCUT), can_redo) {
                                        left_request = Some(TopPanelAction::Redo);
                                        ui.close();
                                    }

                                    theme::menu_separator(ui);

                                    let shortcuts =
                                        egui::Button::new("Keyboard shortcuts").shortcut_text("?");

                                    if ui.add(shortcuts).clicked() {
                                        left_request = Some(TopPanelAction::ShowShortcuts);
                                        ui.close();
                                    }

                                    theme::menu_separator(ui);

                                    if ui.button("Exit").clicked() {
                                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                });

                            if let Some(search) = search {
                                ui.add_space(10.0);
                                search_field(ui, search);
                            }
                        });
                    },
                    |ui| {
                        // Sides lays the right side out from right to left.
                        ui.spacing_mut().item_spacing.x = 0.0;

                        window_button(ui, WindowGlyph::Close, true);
                        let maximized = ui
                            .ctx()
                            .input(|input| input.viewport().maximized.unwrap_or(false));
                        window_button(
                            ui,
                            if maximized {
                                WindowGlyph::Restore
                            } else {
                                WindowGlyph::Maximize
                            },
                            false,
                        );
                        window_button(ui, WindowGlyph::Minimize, false);

                        ui.add_space(6.0);
                        status_dot(ui, status);

                        if let Some(version) = available_update {
                            ui.add_space(4.0);
                            if update_button(ui, version, update_launching).clicked() {
                                right_request = Some(TopPanelAction::InstallUpdate);
                            }
                        }
                    },
                );

            let rect = ui.max_rect();
            ui.painter().hline(
                rect.x_range(),
                rect.bottom() - 0.5,
                Stroke::new(1.0, theme::tint(theme::TEXT_FAINT, 26)),
            );
        });

    left_request.or(right_request)
}

fn brand(ui: &mut egui::Ui) {
    ui.add(
        egui::Image::new(egui::include_image!("../../assets/icons/app_icon.svg")).max_height(18.0),
    );

    ui.label(
        egui::RichText::new("RimMod")
            .text_style(theme::strong_style())
            .color(theme::TEXT),
    );

    ui.label(
        egui::RichText::new(env!("CARGO_PKG_VERSION"))
            .small()
            .color(theme::TEXT_FAINT),
    );
}

fn menu_button(ui: &mut egui::Ui) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(28.0, theme::CONTROL_HEIGHT), Sense::click());

    let hover = ui.ctx().animate_bool_with_time(
        ui.id().with("menu_button_hover"),
        response.hovered(),
        0.12,
    );

    if hover > 0.0 {
        ui.painter().rect_filled(
            rect,
            CornerRadius::ZERO,
            theme::tint(theme::SURFACE_HOVER, (hover * 255.0) as u8),
        );
    }

    let color = if response.hovered() {
        theme::TEXT
    } else {
        theme::TEXT_MUTED
    };
    let stroke = Stroke::new(1.0, color);
    let center = rect.center();

    for offset in [-4.0, 0.0, 4.0] {
        ui.painter().line_segment(
            [
                egui::pos2(center.x - 6.0, center.y + offset),
                egui::pos2(center.x + 6.0, center.y + offset),
            ],
            stroke,
        );
    }

    response
        .on_hover_text("Menu")
        .on_hover_cursor(CursorIcon::PointingHand)
}

fn menu_item(
    ui: &mut egui::Ui,
    label: &str,
    shortcut: Option<&egui::KeyboardShortcut>,
    enabled: bool,
) -> bool {
    let mut button = egui::Button::new(label);

    if let Some(shortcut) = shortcut {
        button = button.shortcut_text(ui.ctx().format_shortcut(shortcut));
    }

    ui.add_enabled(enabled, button).clicked()
}

fn search_field(ui: &mut egui::Ui, search: &mut String) {
    let width = (ui.available_width() - 8.0)
        .clamp(200.0, 520.0)
        .min(ui.available_width());
    let line_height = ui.text_style_height(&egui::TextStyle::Body);
    let margin_y = ((theme::CONTROL_HEIGHT - line_height) / 2.0)
        .round()
        .clamp(0.0, 12.0) as i8;

    // Keep the field visually centred in the space between the menu and the
    // window controls instead of leaving all the empty space on one side.
    let leftover = (ui.available_width() - width).max(0.0);
    ui.add_space(leftover * 0.5);

    let response = ui.add_sized(
        Vec2::new(width, theme::CONTROL_HEIGHT),
        egui::TextEdit::singleline(search)
            .hint_text("Search all mods")
            .margin(egui::Margin {
                left: 26,
                right: 8,
                top: margin_y,
                bottom: margin_y,
            }),
    );

    let rect = response.rect;
    let icon_center = egui::pos2(rect.left() + 12.0, rect.center().y);
    let icon_color = if response.has_focus() {
        theme::ACCENT
    } else {
        theme::TEXT_FAINT
    };

    ui.painter()
        .circle_stroke(icon_center, 3.5, Stroke::new(1.2, icon_color));
    ui.painter().line_segment(
        [
            icon_center + Vec2::new(2.6, 2.6),
            icon_center + Vec2::new(5.2, 5.2),
        ],
        Stroke::new(1.2, icon_color),
    );

    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            CornerRadius::ZERO,
            Stroke::new(1.0, theme::ACCENT),
            StrokeKind::Inside,
        );
    }

    let wants_keyboard = ui.ctx().egui_wants_keyboard_input();
    let shortcut = ui
        .ctx()
        .input_mut(|input| input.consume_shortcut(&SEARCH_SHORTCUT))
        || (!wants_keyboard && ui.ctx().input(|input| input.key_pressed(egui::Key::Slash)));

    if shortcut {
        response.request_focus();
    }

    if response.has_focus() && ui.ctx().input(|input| input.key_pressed(egui::Key::Escape)) {
        search.clear();
        response.surrender_focus();
    }
}

fn status_dot(ui: &mut egui::Ui, status: StatusDot) {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(24.0, TITLE_BAR_HEIGHT), Sense::hover());
    let color = status.color();

    if ui.is_rect_visible(rect) {
        ui.painter().circle_filled(rect.center(), 4.0, color);
        ui.painter()
            .circle_stroke(rect.center(), 6.0, Stroke::new(1.0, theme::tint(color, 70)));
    }

    response.on_hover_text(status.tooltip());
}

fn update_button(ui: &mut egui::Ui, version: &str, launching: bool) -> egui::Response {
    const ICON_SIZE: f32 = 18.0;

    let sense = if launching {
        Sense::hover()
    } else {
        Sense::click()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::new(28.0, theme::CONTROL_HEIGHT), sense);

    let hovered = !launching && response.hovered();
    let active = !launching && response.is_pointer_button_down_on();
    let hover_anim =
        ui.ctx()
            .animate_bool_with_time(ui.id().with("update_button_hover"), hovered, 0.12);

    if ui.is_rect_visible(rect) {
        if hover_anim > 0.0 {
            ui.painter().rect_filled(
                rect,
                CornerRadius::ZERO,
                theme::tint(theme::SURFACE_HOVER, (hover_anim * 255.0) as u8),
            );
        }

        let icon_rect = Rect::from_center_size(rect.center(), Vec2::splat(ICON_SIZE));
        let tint = if launching {
            Color32::from_white_alpha(130)
        } else {
            Color32::WHITE
        };

        egui::Image::new(icons::download_icon())
            .fit_to_exact_size(icon_rect.size())
            .tint(tint)
            .paint_at(ui, icon_rect);

        // Subtle state overlay clipped to the 22/24 plate inside the 24x24 viewBox.
        if active || hovered {
            let plate_rect = icon_rect.shrink(ICON_SIZE / 24.0);
            let overlay = if active {
                Color32::from_black_alpha(38)
            } else {
                Color32::from_white_alpha(26)
            };
            ui.painter()
                .rect_filled(plate_rect, CornerRadius::ZERO, overlay);
        }
    }

    let tooltip = if launching {
        format!("Updating to v{version}...")
    } else {
        format!("Update available: v{version}\nClick to update and restart")
    };
    let response = response.on_hover_text(tooltip);

    if launching {
        response
    } else {
        response.on_hover_cursor(CursorIcon::PointingHand)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum WindowGlyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

fn window_button(ui: &mut egui::Ui, glyph: WindowGlyph, danger: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(WINDOW_BUTTON_WIDTH, TITLE_BAR_HEIGHT),
        Sense::click(),
    );

    let hover = ui.ctx().animate_bool_with_time(
        ui.id().with(("window_button", glyph)),
        response.hovered(),
        0.12,
    );

    if hover > 0.0 {
        let fill = if danger {
            theme::DANGER
        } else {
            theme::SURFACE_HOVER
        };

        ui.painter().rect_filled(
            rect,
            CornerRadius::ZERO,
            theme::tint(fill, (hover * 255.0) as u8),
        );
    }

    let color = if danger && response.hovered() {
        Color32::WHITE
    } else {
        theme::TEXT_MUTED
    };
    let stroke = Stroke::new(1.0, color);
    let center = rect.center();

    match glyph {
        WindowGlyph::Minimize => {
            ui.painter().line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y),
                    egui::pos2(center.x + 5.0, center.y),
                ],
                stroke,
            );
        }
        WindowGlyph::Maximize => {
            let box_rect = Rect::from_center_size(center, Vec2::splat(10.0));
            ui.painter()
                .rect_stroke(box_rect, CornerRadius::ZERO, stroke, StrokeKind::Inside);
        }
        WindowGlyph::Restore => {
            let back = Rect::from_min_size(center + Vec2::new(-3.0, -5.0), Vec2::splat(8.0));
            let front = Rect::from_min_size(center + Vec2::new(-5.0, -3.0), Vec2::splat(8.0));
            ui.painter()
                .rect_stroke(back, CornerRadius::ZERO, stroke, StrokeKind::Inside);
            ui.painter()
                .rect_stroke(front, CornerRadius::ZERO, stroke, StrokeKind::Inside);
        }
        WindowGlyph::Close => {
            ui.painter().line_segment(
                [
                    egui::pos2(center.x - 5.0, center.y - 5.0),
                    egui::pos2(center.x + 5.0, center.y + 5.0),
                ],
                stroke,
            );
            ui.painter().line_segment(
                [
                    egui::pos2(center.x + 5.0, center.y - 5.0),
                    egui::pos2(center.x - 5.0, center.y + 5.0),
                ],
                stroke,
            );
        }
    }

    let response = response.on_hover_cursor(CursorIcon::PointingHand);

    if response.clicked() {
        match glyph {
            WindowGlyph::Minimize => {
                ui.ctx()
                    .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
            }
            WindowGlyph::Maximize | WindowGlyph::Restore => toggle_maximized(ui.ctx()),
            WindowGlyph::Close => {
                ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    response
}

fn toggle_maximized(context: &egui::Context) {
    let maximized = context.input(|input| input.viewport().maximized.unwrap_or(false));
    context.send_viewport_cmd(egui::ViewportCommand::Maximized(!maximized));
}

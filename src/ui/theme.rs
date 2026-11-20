//! Design tokens and the global egui theme.
//!
//! Every visual constant in the app comes from here: one palette, one type
//! scale, one spacing rhythm. UI code should not invent colors or sizes, so
//! changes stay consistent across panels.

use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, Stroke,
    TextStyle, Vec2,
};

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Application background, behind every panel.
pub(crate) const CANVAS: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);

/// Panels, cards and menus: one step above the canvas.
pub(crate) const SURFACE: Color32 = Color32::from_rgb(0x15, 0x1b, 0x23);

/// Hovered surfaces.
pub(crate) const SURFACE_HOVER: Color32 = Color32::from_rgb(0x1d, 0x24, 0x2e);

/// Pressed or open surfaces.
pub(crate) const SURFACE_ACTIVE: Color32 = Color32::from_rgb(0x25, 0x2d, 0x39);

/// Pressed buttons: one step lighter than [`SURFACE_ACTIVE`].
pub(crate) const SURFACE_PRESSED: Color32 = Color32::from_rgb(0x2c, 0x35, 0x42);

pub(crate) const TEXT: Color32 = Color32::from_rgb(0xe6, 0xed, 0xf3);
pub(crate) const TEXT_MUTED: Color32 = Color32::from_rgb(0x91, 0x98, 0xa1);
pub(crate) const TEXT_FAINT: Color32 = Color32::from_rgb(0x6e, 0x76, 0x81);

pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x44, 0x93, 0xf8);
pub(crate) const ACCENT_BUTTON: Color32 = Color32::from_rgb(0x1f, 0x6f, 0xeb);
pub(crate) const ACCENT_BUTTON_HOVER: Color32 = Color32::from_rgb(0x2b, 0x7f, 0xff);
pub(crate) const ACCENT_BUTTON_ACTIVE: Color32 = Color32::from_rgb(0x1a, 0x5f, 0xc9);

pub(crate) const SUCCESS: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
pub(crate) const WARNING: Color32 = Color32::from_rgb(0xd2, 0x99, 0x22);
pub(crate) const DANGER: Color32 = Color32::from_rgb(0xf8, 0x51, 0x49);

// ---------------------------------------------------------------------------
// Shape and rhythm
// ---------------------------------------------------------------------------

pub(crate) const RADIUS_SM: u8 = 4;
pub(crate) const RADIUS_MD: u8 = 6;
pub(crate) const RADIUS_LG: u8 = 8;

/// Height of one mod list row.
pub(crate) const ROW_HEIGHT: f32 = 26.0;

/// Control height used by buttons and the bottom bar.
pub(crate) const CONTROL_HEIGHT: f32 = 26.0;

// ---------------------------------------------------------------------------
// Typography
// ---------------------------------------------------------------------------

const INTER_REGULAR: &[u8] = include_bytes!("../../assets/fonts/Inter-Regular.ttf");
const INTER_MEDIUM: &[u8] = include_bytes!("../../assets/fonts/Inter-Medium.ttf");
const INTER_SEMIBOLD: &[u8] = include_bytes!("../../assets/fonts/Inter-SemiBold.ttf");

fn family(name: &str) -> FontFamily {
    FontFamily::Name(name.into())
}

pub(crate) fn medium_font(size: f32) -> FontId {
    FontId::new(size, family("medium"))
}

pub(crate) fn semibold_font(size: f32) -> FontId {
    FontId::new(size, family("semibold"))
}

/// 13px semibold: row titles, section labels, emphasis.
pub(crate) fn strong_style() -> TextStyle {
    TextStyle::Name("strong".into())
}

/// 15px semibold: panel headings.
pub(crate) fn title_style() -> TextStyle {
    TextStyle::Name("title".into())
}

fn fonts() -> FontDefinitions {
    let mut fonts = FontDefinitions::default();

    for (name, bytes) in [
        ("inter", INTER_REGULAR),
        ("inter-medium", INTER_MEDIUM),
        ("inter-semibold", INTER_SEMIBOLD),
    ] {
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_static(bytes)));
    }

    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, "inter".to_owned());

    fonts.families.insert(
        family("medium"),
        vec!["inter-medium".to_owned(), "inter".to_owned()],
    );

    fonts.families.insert(
        family("semibold"),
        vec!["inter-semibold".to_owned(), "inter".to_owned()],
    );

    fonts
}

// ---------------------------------------------------------------------------
// Installation
// ---------------------------------------------------------------------------

pub(crate) fn install(ctx: &egui::Context) {
    ctx.set_theme(egui::ThemePreference::Dark);
    ctx.set_fonts(fonts());
    ctx.all_styles_mut(style);
}

fn style(style: &mut egui::Style) {
    // One size per role; feature code must not pass ad-hoc font sizes.
    style.text_styles = [
        (
            TextStyle::Small,
            FontId::new(11.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(13.0, FontFamily::Proportional),
        ),
        (TextStyle::Heading, semibold_font(17.0)),
        (
            TextStyle::Monospace,
            FontId::new(12.0, FontFamily::Monospace),
        ),
        (strong_style(), semibold_font(13.0)),
        (title_style(), semibold_font(15.0)),
    ]
    .into();

    style.spacing.item_spacing = Vec2::new(8.0, 6.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.spacing.interact_size.y = CONTROL_HEIGHT;
    style.spacing.menu_margin = Margin::same(6);
    style.spacing.window_margin = Margin::same(12);
    style.spacing.scroll.bar_width = 8.0;
    style.spacing.scroll.floating = true;

    let visuals = &mut style.visuals;
    visuals.panel_fill = CANVAS;
    visuals.window_fill = SURFACE;
    visuals.window_stroke = Stroke::NONE;
    visuals.window_shadow = egui::epaint::Shadow {
        offset: [0, 8],
        blur: 28,
        spread: 0,
        color: Color32::from_black_alpha(140),
    };
    visuals.window_corner_radius = CornerRadius::same(10);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.extreme_bg_color = SURFACE_HOVER;
    visuals.faint_bg_color = SURFACE_HOVER;
    visuals.hyperlink_color = ACCENT;
    visuals.selection.bg_fill = tint(ACCENT, 60);
    visuals.selection.stroke = Stroke::new(1.0, ACCENT);

    let widgets = &mut visuals.widgets;

    // Labels and other static text.
    widgets.noninteractive.bg_fill = SURFACE;
    widgets.noninteractive.weak_bg_fill = SURFACE;
    widgets.noninteractive.bg_stroke = Stroke::NONE;
    widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.noninteractive.corner_radius = CornerRadius::same(RADIUS_MD);

    // Controls are flat: state is carried by fill only, never by an outline.
    widgets.inactive.bg_fill = SURFACE;
    widgets.inactive.weak_bg_fill = SURFACE_HOVER;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.hovered.bg_fill = SURFACE_HOVER;
    widgets.hovered.weak_bg_fill = SURFACE_ACTIVE;
    widgets.hovered.bg_stroke = Stroke::NONE;
    widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.active.bg_fill = SURFACE_ACTIVE;
    widgets.active.weak_bg_fill = SURFACE_PRESSED;
    widgets.active.bg_stroke = Stroke::NONE;
    widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.active.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.open.bg_fill = SURFACE_ACTIVE;
    widgets.open.weak_bg_fill = SURFACE_ACTIVE;
    widgets.open.bg_stroke = Stroke::NONE;
    widgets.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.open.corner_radius = CornerRadius::same(RADIUS_MD);
}

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

/// Translucent version of a palette color, for chips and selected rows.
pub(crate) fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Raised card used to group related content. Separation comes from the fill
/// step against the canvas; no outline.
pub(crate) fn card() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(12))
}

/// Recessed group, used for field clusters inside a surface (e.g. settings).
pub(crate) fn group() -> egui::Frame {
    egui::Frame::new()
        .fill(CANVAS)
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(12))
}

/// Small rounded label, for statuses and versions.
pub(crate) fn chip(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) -> egui::Response {
    egui::Frame::new()
        .fill(tint(color, 34))
        .corner_radius(CornerRadius::same(RADIUS_SM))
        .inner_margin(Margin::symmetric(6, 2))
        .show(ui, |ui| {
            ui.label(egui::RichText::new(text.into()).small().color(color))
        })
        .response
}

/// Panel heading, matching the mod list and information columns.
pub(crate) fn title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).text_style(title_style()))
}

/// Muted hint text.
pub(crate) fn hint(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(egui::RichText::new(text.into()).small().color(TEXT_MUTED))
}

/// Link-styled text that never widens its column. Paths and URLs are long
/// single words, so they must truncate instead of pushing a layout sideways.
pub(crate) fn link(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.add(
        egui::Label::new(egui::RichText::new(text.into()).small().color(ACCENT))
            .truncate()
            .sense(egui::Sense::click()),
    )
    .on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Filled, high-contrast button for the one primary action of a view.
pub(crate) fn primary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.scope(|ui| {
        let widgets = &mut ui.style_mut().visuals.widgets;

        for (visual, fill) in [
            (&mut widgets.inactive, ACCENT_BUTTON),
            (&mut widgets.hovered, ACCENT_BUTTON_HOVER),
            (&mut widgets.active, ACCENT_BUTTON_ACTIVE),
            (&mut widgets.open, ACCENT_BUTTON),
        ] {
            visual.bg_fill = fill;
            visual.weak_bg_fill = fill;
            visual.bg_stroke = Stroke::NONE;
            visual.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        }

        ui.add(
            egui::Button::new(egui::RichText::new(text).color(Color32::WHITE))
                .min_size(Vec2::new(88.0, CONTROL_HEIGHT)),
        )
    })
    .inner
}

/// Secondary button with the resting control treatment.
pub(crate) fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.add(
        egui::Button::new(text)
            .min_size(Vec2::new(88.0, CONTROL_HEIGHT))
            .corner_radius(CornerRadius::same(RADIUS_MD)),
    )
}

/// Text tab with an accent underline, used for top-level navigation.
pub(crate) fn tab(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    let font = medium_font(13.0);
    let padding = Vec2::new(10.0, 7.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), TEXT);
    let size = galley.size() + padding * 2.0 + Vec2::new(0.0, 2.0);

    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let color = if selected || hovered {
            TEXT
        } else {
            TEXT_MUTED
        };

        if hovered && !selected {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(RADIUS_MD), SURFACE_HOVER);
        }

        let text_pos = rect.center() - galley.size() / 2.0 - Vec2::new(0.0, 1.0);
        ui.painter().galley(text_pos, galley, color);

        if selected {
            let underline = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 8.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 8.0, rect.bottom()),
            );

            ui.painter()
                .rect_filled(underline, CornerRadius::same(1), ACCENT);
        }
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

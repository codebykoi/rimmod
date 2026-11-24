//! Design tokens and the global egui theme.
//!
//! Every visual constant in the app comes from here: one palette, one type
//! scale, one spacing rhythm. UI code should not invent colors or sizes, so
//! changes stay consistent across panels.

use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, FontId, Margin, RichText,
    Stroke, TextStyle, Vec2,
};

use crate::services::organization::ModColor;
use crate::ui::icons;

// ---------------------------------------------------------------------------
// Palette
// ---------------------------------------------------------------------------

/// Application background, behind every panel. Also the base fill under the
/// [`CANVAS_TOP`] → [`CANVAS_BOTTOM`] gradient.
pub(crate) const CANVAS: Color32 = Color32::from_rgb(0x0d, 0x11, 0x17);

/// Top of the application background gradient.
pub(crate) const CANVAS_TOP: Color32 = Color32::from_rgb(0x12, 0x19, 0x23);

/// Bottom of the application background gradient.
pub(crate) const CANVAS_BOTTOM: Color32 = Color32::from_rgb(0x08, 0x0b, 0x10);

/// Panels, cards and menus: two steps above the canvas so content reads as a
/// layer instead of melting into the background.
pub(crate) const SURFACE: Color32 = Color32::from_rgb(0x17, 0x1e, 0x27);

/// Hovered surfaces. Also the light end of card gradients.
pub(crate) const SURFACE_HOVER: Color32 = Color32::from_rgb(0x21, 0x2a, 0x35);

/// Resting fill for buttons, combo boxes, checkboxes and other controls.
/// Deliberately brighter than [`SURFACE`] so a control never blends into the
/// card it sits on.
pub(crate) const CONTROL: Color32 = Color32::from_rgb(0x24, 0x2e, 0x3b);

/// Hovered controls.
pub(crate) const CONTROL_HOVER: Color32 = Color32::from_rgb(0x30, 0x3c, 0x4c);

/// Pressed controls.
pub(crate) const CONTROL_ACTIVE: Color32 = Color32::from_rgb(0x39, 0x47, 0x59);

/// Recessed text fields, so inputs read as holes rather than buttons without
/// needing an outline.
pub(crate) const FIELD: Color32 = Color32::from_rgb(0x1b, 0x23, 0x2e);

pub(crate) const TEXT: Color32 = Color32::from_rgb(0xe6, 0xed, 0xf3);
pub(crate) const TEXT_MUTED: Color32 = Color32::from_rgb(0x9e, 0xa7, 0xb2);
pub(crate) const TEXT_FAINT: Color32 = Color32::from_rgb(0x8a, 0x93, 0x9f);

pub(crate) const ACCENT: Color32 = Color32::from_rgb(0x44, 0x93, 0xf8);

/// Accent button gradient, at rest.
pub(crate) const ACCENT_BUTTON_TOP: Color32 = Color32::from_rgb(0x49, 0x8f, 0xf8);
pub(crate) const ACCENT_BUTTON_BOTTOM: Color32 = Color32::from_rgb(0x1c, 0x66, 0xd6);

/// Accent button gradient, hovered.
pub(crate) const ACCENT_BUTTON_HOVER_TOP: Color32 = Color32::from_rgb(0x5c, 0x9e, 0xff);
pub(crate) const ACCENT_BUTTON_HOVER_BOTTOM: Color32 = Color32::from_rgb(0x24, 0x73, 0xea);

/// Accent button gradient, pressed.
pub(crate) const ACCENT_BUTTON_ACTIVE_TOP: Color32 = Color32::from_rgb(0x1e, 0x62, 0xcb);
pub(crate) const ACCENT_BUTTON_ACTIVE_BOTTOM: Color32 = Color32::from_rgb(0x17, 0x4f, 0xa8);

pub(crate) const SUCCESS: Color32 = Color32::from_rgb(0x3f, 0xb9, 0x50);
pub(crate) const WARNING: Color32 = Color32::from_rgb(0xd2, 0x99, 0x22);
pub(crate) const DANGER: Color32 = Color32::from_rgb(0xf8, 0x51, 0x49);

/// Destructive button gradient, at rest.
pub(crate) const DANGER_BUTTON_TOP: Color32 = Color32::from_rgb(0xe8, 0x4c, 0x46);
pub(crate) const DANGER_BUTTON_BOTTOM: Color32 = Color32::from_rgb(0xb8, 0x2d, 0x30);

/// Destructive button gradient, hovered.
pub(crate) const DANGER_BUTTON_HOVER_TOP: Color32 = Color32::from_rgb(0xf6, 0x5e, 0x57);
pub(crate) const DANGER_BUTTON_HOVER_BOTTOM: Color32 = Color32::from_rgb(0xcb, 0x38, 0x3a);

/// Destructive button gradient, pressed.
pub(crate) const DANGER_BUTTON_ACTIVE_TOP: Color32 = Color32::from_rgb(0xa5, 0x27, 0x2a);
pub(crate) const DANGER_BUTTON_ACTIVE_BOTTOM: Color32 = Color32::from_rgb(0x86, 0x20, 0x23);

// ---------------------------------------------------------------------------
// Organization palette
// ---------------------------------------------------------------------------
//
// Entry colors reuse the semantic tokens where they overlap, and add a few
// muted hues that sit at the same brightness. No neon: a colored list should
// still read as the same app, only easier to scan.

const MOD_TEAL: Color32 = Color32::from_rgb(0x2f, 0xb3, 0xa3);
const MOD_ORANGE: Color32 = Color32::from_rgb(0xe0, 0x8a, 0x3c);
const MOD_PINK: Color32 = Color32::from_rgb(0xe5, 0x67, 0xa8);
const MOD_PURPLE: Color32 = Color32::from_rgb(0xa3, 0x71, 0xf7);
const MOD_SLATE: Color32 = Color32::from_rgb(0x8b, 0x98, 0xa8);

/// Color assigned to a mod entry. Kept separate from [`chip`] callers so the
/// palette stays in one place.
pub(crate) fn mod_color(color: ModColor) -> Color32 {
    match color {
        ModColor::Blue => ACCENT,
        ModColor::Teal => MOD_TEAL,
        ModColor::Green => SUCCESS,
        ModColor::Amber => WARNING,
        ModColor::Orange => MOD_ORANGE,
        ModColor::Red => DANGER,
        ModColor::Pink => MOD_PINK,
        ModColor::Purple => MOD_PURPLE,
        ModColor::Slate => MOD_SLATE,
    }
}

// ---------------------------------------------------------------------------
// Shape and rhythm
// ---------------------------------------------------------------------------

pub(crate) const RADIUS_SM: u8 = 0;
pub(crate) const RADIUS_MD: u8 = 0;
pub(crate) const RADIUS_LG: u8 = 0;

/// Height of one mod list row.
pub(crate) const ROW_HEIGHT: f32 = 32.0;

/// Control height used by buttons and the bottom bar.
pub(crate) const CONTROL_HEIGHT: f32 = 30.0;

/// Space kept at the right of scrolling content so the floating scrollbar
/// never sits on top of cards or buttons.
pub(crate) const SCROLL_GUTTER: f32 = 16.0;

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

    install_cjk_fonts(&mut fonts);

    fonts
}

/// Inter has no CJK coverage. Load a system CJK font as a fallback so Chinese
/// and Japanese titles, descriptions and tags render instead of boxes.
fn install_cjk_fonts(fonts: &mut FontDefinitions) {
    let mut installed = 0;

    for path in cjk_font_paths() {
        if installed >= 2 {
            break;
        }

        let Ok(bytes) = std::fs::read(&path) else {
            continue;
        };

        let name = format!("cjk-{installed}");
        fonts
            .font_data
            .insert(name.clone(), Arc::new(FontData::from_owned(bytes)));

        if let Some(proportional) = fonts.families.get_mut(&FontFamily::Proportional) {
            // Keep Inter first so Latin text uses the app font, then try CJK
            // before the default emoji fallbacks.
            proportional.insert(1 + installed, name.clone());
        }

        for family_name in ["medium", "semibold"] {
            fonts
                .families
                .entry(family(family_name))
                .or_default()
                .push(name.clone());
        }

        installed += 1;
    }
}

fn cjk_font_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    if let Some(custom) = std::env::var_os("RIMMOD_CJK_FONT") {
        paths.push(PathBuf::from(custom));
    }

    #[cfg(target_os = "windows")]
    {
        let fonts_folder = std::env::var_os("WINDIR")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Windows"))
            .join("Fonts");

        paths.extend([
            fonts_folder.join("msyh.ttc"), // Microsoft YaHei (Chinese)
            fonts_folder.join("msyh.ttf"),
            fonts_folder.join("YuGothM.ttc"), // Yu Gothic (Japanese)
            fonts_folder.join("YuGothM.ttf"),
            fonts_folder.join("meiryo.ttc"), // Meiryo (Japanese)
            fonts_folder.join("simhei.ttf"), // SimHei (Chinese)
            fonts_folder.join("simsun.ttc"), // SimSun (Chinese)
        ]);
    }

    #[cfg(target_os = "linux")]
    {
        paths.extend([
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/noto/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/google-noto-cjk/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/noto-cjk/NotoSansCJK-Regular.ttc"),
            PathBuf::from("/usr/share/fonts/opentype/noto/NotoSansCJKsc-Regular.otf"),
            PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-zenhei.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/wqy/wqy-microhei.ttc"),
            PathBuf::from("/usr/share/fonts/truetype/droid/DroidSansFallbackFull.ttf"),
        ]);
    }

    #[cfg(target_os = "macos")]
    {
        paths.extend([
            PathBuf::from("/System/Library/Fonts/PingFang.ttc"),
            PathBuf::from("/System/Library/Fonts/Hiragino Sans GB.ttc"),
            PathBuf::from("/System/Library/Fonts/ヒラギノ角ゴシック W3.ttc"),
            PathBuf::from("/Library/Fonts/Arial Unicode.ttf"),
        ]);
    }

    paths
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
            FontId::new(12.0, FontFamily::Proportional),
        ),
        (TextStyle::Body, FontId::new(14.0, FontFamily::Proportional)),
        (
            TextStyle::Button,
            FontId::new(14.0, FontFamily::Proportional),
        ),
        (TextStyle::Heading, semibold_font(18.0)),
        (
            TextStyle::Monospace,
            FontId::new(13.0, FontFamily::Monospace),
        ),
        (strong_style(), semibold_font(14.0)),
        (title_style(), semibold_font(16.0)),
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
    visuals.window_shadow = soft_shadow();
    visuals.popup_shadow = soft_shadow();
    visuals.window_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.menu_corner_radius = CornerRadius::same(RADIUS_LG);
    visuals.extreme_bg_color = FIELD;
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

    // Controls are separated by fill alone: no outlines anywhere. Hover and
    // press states step the fill up so interactive things still read as such.
    widgets.inactive.bg_fill = CONTROL;
    widgets.inactive.weak_bg_fill = CONTROL;
    widgets.inactive.bg_stroke = Stroke::NONE;
    widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    widgets.inactive.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.hovered.bg_fill = CONTROL_HOVER;
    widgets.hovered.weak_bg_fill = CONTROL_HOVER;
    widgets.hovered.bg_stroke = Stroke::NONE;
    widgets.hovered.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.hovered.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.active.bg_fill = CONTROL_ACTIVE;
    widgets.active.weak_bg_fill = CONTROL_ACTIVE;
    widgets.active.bg_stroke = Stroke::NONE;
    widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.active.corner_radius = CornerRadius::same(RADIUS_MD);

    widgets.open.bg_fill = CONTROL;
    widgets.open.weak_bg_fill = CONTROL_HOVER;
    widgets.open.bg_stroke = Stroke::NONE;
    widgets.open.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    widgets.open.corner_radius = CornerRadius::same(RADIUS_MD);
}

/// Square controls and surfaces, used by dense full-width pages such as the
/// Workshop and Log tabs.
pub(crate) fn apply_square_style(ui: &mut egui::Ui) {
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

// ---------------------------------------------------------------------------
// Building blocks
// ---------------------------------------------------------------------------

/// Translucent version of a palette color, for chips and selected rows.
pub(crate) fn tint(color: Color32, alpha: u8) -> Color32 {
    Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), alpha)
}

/// Soft elevation used by cards, windows and toasts. Depth replaces outlines:
/// surfaces sit above the background through blur, not through borders.
pub(crate) fn soft_shadow() -> egui::epaint::Shadow {
    egui::epaint::Shadow {
        offset: [0, 10],
        blur: 30,
        spread: 0,
        color: Color32::from_black_alpha(120),
    }
}

/// Vertical two-stop gradient, used for backgrounds, cards and buttons.
pub(crate) fn vertical_gradient(rect: egui::Rect, top: Color32, bottom: Color32) -> egui::Shape {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);

    egui::Shape::mesh(mesh)
}

/// Paint a vertical gradient across `rect` as the first thing in the current
/// layer, so everything added afterwards draws on top of it.
pub(crate) fn paint_background_gradient(ui: &egui::Ui, top: Color32, bottom: Color32) {
    ui.painter()
        .add(vertical_gradient(ui.max_rect(), top, bottom));
}

/// Raised card with a gradient fill and a soft shadow. The shadow and fill are
/// reserved below the contents, so they never cover the widgets inside.
pub(crate) fn gradient_card<R>(
    ui: &mut egui::Ui,
    top: Color32,
    bottom: Color32,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    gradient_container(ui, top, bottom, margin, Some(soft_shadow()), add_contents)
}

/// Gradient surface without elevation, for panel-like groups that already sit
/// on a distinct background.
pub(crate) fn gradient_surface<R>(
    ui: &mut egui::Ui,
    top: Color32,
    bottom: Color32,
    margin: Margin,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    gradient_container(ui, top, bottom, margin, None, add_contents)
}

fn gradient_container<R>(
    ui: &mut egui::Ui,
    top: Color32,
    bottom: Color32,
    margin: Margin,
    shadow: Option<egui::epaint::Shadow>,
    add_contents: impl FnOnce(&mut egui::Ui) -> R,
) -> egui::InnerResponse<R> {
    let shadow_slot = shadow.map(|_| ui.painter().add(egui::Shape::Noop));
    let backdrop = ui.painter().add(egui::Shape::Noop);

    let inner = egui::Frame::new()
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(margin)
        .show(ui, add_contents);

    let rect = inner.response.rect;
    let corner_radius = CornerRadius::same(RADIUS_LG);

    if let (Some(slot), Some(shadow)) = (shadow_slot, shadow) {
        ui.painter().set(
            slot,
            egui::Shape::from(shadow.as_shape(rect, corner_radius)),
        );
    }

    ui.painter()
        .set(backdrop, vertical_gradient(rect, top, bottom));

    inner
}

/// Recessed group, used for field clusters inside a surface (e.g. settings).
pub(crate) fn group() -> egui::Frame {
    egui::Frame::new()
        .fill(CANVAS)
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .inner_margin(Margin::same(12))
}

/// Popup frame for context menus, matching cards and windows: the same
/// surface, square corners and soft elevation. Menus must not read as an
/// OS widget dropped into the app.
pub(crate) fn menu_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(RADIUS_LG))
        .shadow(soft_shadow())
        .inner_margin(Margin::same(8))
}

/// Content inset to line up with menu action labels. Menu buttons use their
/// own horizontal padding, so headings and captions need the same inset.
pub(crate) fn menu_inset<R>(ui: &mut egui::Ui, add_contents: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::new()
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, add_contents)
        .inner
}

/// Divider between groups of menu actions. `ui.separator` uses
/// `noninteractive.bg_stroke`, which is none in this theme so panels stay
/// outline-free; menus still need a faint line to separate action groups.
pub(crate) fn menu_separator(ui: &mut egui::Ui) {
    ui.scope(|ui| {
        ui.visuals_mut().widgets.noninteractive.bg_stroke = Stroke::new(1.0, tint(TEXT_FAINT, 40));
        ui.separator();
    });
}

/// Small rounded label, for statuses and versions.
///
/// Measured and allocated up front rather than drawn with a [`egui::Frame`]:
/// frames ask for the space that is left on the current row, so inside a
/// wrapped row a chip that does not fit would be squeezed into a column of
/// single characters instead of moving to the next row.
pub(crate) fn chip(ui: &mut egui::Ui, text: impl Into<String>, color: Color32) -> egui::Response {
    chip_with_corner_radius(ui, text, color, CornerRadius::same(RADIUS_SM))
}

/// [`chip`] with an explicit corner radius, for square-cornered surfaces.
pub(crate) fn chip_with_corner_radius(
    ui: &mut egui::Ui,
    text: impl Into<String>,
    color: Color32,
    corner_radius: CornerRadius,
) -> egui::Response {
    let font = TextStyle::Small.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.into(), font, Color32::PLACEHOLDER);
    let padding = Vec2::new(12.0, 4.0);
    let (rect, response) = ui.allocate_exact_size(galley.size() + padding, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        ui.painter()
            .rect_filled(rect, corner_radius, tint(color, 34));
        ui.painter().galley(rect.min + padding / 2.0, galley, color);
    }

    response
}

/// Inline tag label for dense rows and the information panel.
///
/// Tags are deliberately neutral: the colored rail already encodes the mod's
/// color, and tinted tag text in the same row would compete with version and
/// warning badges.
pub(crate) fn tag_text(tag: &str) -> RichText {
    RichText::new(format!("  {tag}  "))
        .font(medium_font(11.0))
        .color(TEXT_MUTED)
        .background_color(tint(TEXT_MUTED, 26))
}

/// "+N" marker shown when a row has more tags than it can display.
pub(crate) fn tag_overflow_text(hidden_count: usize) -> RichText {
    RichText::new(format!("  +{hidden_count}  "))
        .font(medium_font(11.0))
        .color(TEXT_FAINT)
        .background_color(tint(TEXT_FAINT, 22))
}

/// Panel heading, matching the mod list and information columns.
pub(crate) fn title(ui: &mut egui::Ui, text: &str) -> egui::Response {
    ui.label(egui::RichText::new(text).text_style(title_style()))
}

/// Muted hint text.
pub(crate) fn hint(ui: &mut egui::Ui, text: impl Into<String>) -> egui::Response {
    ui.label(egui::RichText::new(text.into()).small().color(TEXT_MUTED))
}

/// Collapsible section. Painted from scratch instead of using
/// [`egui::CollapsingHeader`] so the disclosure row follows the rest of the
/// theme: full-width hover fill, no outlines, a muted chevron and the same
/// small muted label used by section headings elsewhere.
pub(crate) fn collapsible(
    ui: &mut egui::Ui,
    id_salt: &str,
    title: impl Into<String>,
    default_open: bool,
    add_contents: impl FnOnce(&mut egui::Ui),
) -> egui::Response {
    const HEADER_HEIGHT: f32 = 22.0;
    const CHEVRON_LEFT: f32 = 8.0;
    const TEXT_LEFT: f32 = 20.0;

    let id = ui.make_persistent_id(id_salt);
    let mut state = egui::containers::collapsing_header::CollapsingState::load_with_default_open(
        ui.ctx(),
        id,
        default_open,
    );

    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), HEADER_HEIGHT),
        egui::Sense::click(),
    );

    if response.clicked() {
        state.toggle(ui);
    }

    let openness = state.openness(ui.ctx());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let color = if hovered { TEXT } else { TEXT_MUTED };

        if hovered {
            ui.painter()
                .rect_filled(rect, CornerRadius::same(RADIUS_SM), SURFACE_HOVER);
        }

        // Chevron points right when closed and rotates down while opening.
        let center = egui::pos2(rect.left() + CHEVRON_LEFT, rect.center().y);
        let rotation = egui::emath::Rot2::from_angle(egui::remap(
            openness,
            0.0..=1.0,
            0.0..=std::f32::consts::FRAC_PI_2,
        ));
        let points = [
            egui::vec2(-2.0, -4.0),
            egui::vec2(2.0, 0.0),
            egui::vec2(-2.0, 4.0),
        ]
        .into_iter()
        .map(|offset| center + rotation * offset)
        .collect::<Vec<_>>();

        ui.painter()
            .add(egui::Shape::line(points, Stroke::new(1.5, color)));

        let galley =
            ui.painter()
                .layout_no_wrap(title.into(), TextStyle::Small.resolve(ui.style()), color);
        ui.painter().galley(
            egui::pos2(
                rect.left() + TEXT_LEFT,
                rect.center().y - galley.size().y / 2.0,
            ),
            galley,
            color,
        );
    }

    let _ = state.show_body_unindented(ui, |ui| {
        ui.indent(id, |ui| {
            ui.expand_to_include_x(rect.right());
            add_contents(ui);
        });
    });

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
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
    primary_button_sized(ui, text, Vec2::new(88.0, CONTROL_HEIGHT))
}

/// [`primary_button`] at an explicit size, for card grids and detail rows.
pub(crate) fn primary_button_sized(ui: &mut egui::Ui, text: &str, size: Vec2) -> egui::Response {
    gradient_button_sized(
        ui,
        text,
        size,
        (ACCENT_BUTTON_TOP, ACCENT_BUTTON_BOTTOM),
        (ACCENT_BUTTON_HOVER_TOP, ACCENT_BUTTON_HOVER_BOTTOM),
        (ACCENT_BUTTON_ACTIVE_TOP, ACCENT_BUTTON_ACTIVE_BOTTOM),
        Color32::WHITE,
    )
}

/// Destructive button for irreversible actions, such as deleting mods.
pub(crate) fn danger_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    danger_button_sized(ui, text, Vec2::new(88.0, CONTROL_HEIGHT))
}

/// [`danger_button`] at an explicit size.
pub(crate) fn danger_button_sized(ui: &mut egui::Ui, text: &str, size: Vec2) -> egui::Response {
    gradient_button_sized(
        ui,
        text,
        size,
        (DANGER_BUTTON_TOP, DANGER_BUTTON_BOTTOM),
        (DANGER_BUTTON_HOVER_TOP, DANGER_BUTTON_HOVER_BOTTOM),
        (DANGER_BUTTON_ACTIVE_TOP, DANGER_BUTTON_ACTIVE_BOTTOM),
        Color32::WHITE,
    )
}

/// Shared painted button. A vertical gradient plus a fill-only state change
/// keeps controls consistent with the rest of the app, where depth and hover
/// are expressed without outlines.
fn gradient_button_sized(
    ui: &mut egui::Ui,
    text: &str,
    size: Vec2,
    rest: (Color32, Color32),
    hover: (Color32, Color32),
    active: (Color32, Color32),
    text_color: Color32,
) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_owned(), font, Color32::PLACEHOLDER);

    let padding = Vec2::new(16.0, 0.0);
    let width = size.x.max(galley.size().x + padding.x * 2.0);
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, size.y), egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let (top, bottom, color) = if !ui.is_enabled() {
            (CONTROL, CONTROL, TEXT_FAINT)
        } else if response.is_pointer_button_down_on() {
            (active.0, active.1, Color32::WHITE)
        } else if response.hovered() {
            (hover.0, hover.1, Color32::WHITE)
        } else {
            (rest.0, rest.1, text_color)
        };

        ui.painter().add(vertical_gradient(rect, top, bottom));
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }

    if ui.is_enabled() {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Secondary button: dark gradient at rest, brighter on hover.
pub(crate) fn secondary_button(ui: &mut egui::Ui, text: &str) -> egui::Response {
    secondary_button_sized(ui, text, Vec2::new(88.0, CONTROL_HEIGHT))
}

/// [`secondary_button`] at an explicit size.
pub(crate) fn secondary_button_sized(ui: &mut egui::Ui, text: &str, size: Vec2) -> egui::Response {
    gradient_button_sized(
        ui,
        text,
        size,
        (CONTROL_HOVER, CONTROL),
        (CONTROL_ACTIVE, CONTROL_HOVER),
        (FIELD, CONTROL),
        TEXT,
    )
}

/// Single-line search field padded to exactly the standard control height, so
/// it lines up with the buttons and combo boxes next to it.
pub(crate) fn search_field(ui: &mut egui::Ui, text: &mut String, hint: &str) -> egui::Response {
    let line_height = ui.text_style_height(&TextStyle::Body);
    let margin_y = ((CONTROL_HEIGHT - line_height) / 2.0)
        .round()
        .clamp(0.0, 12.0) as i8;

    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint.to_owned())
            .desired_width(f32::INFINITY)
            .margin(Margin::symmetric(8, margin_y)),
    )
}

/// Linear progress bar: a recessed square track filled with the accent button
/// gradient. The percentage is left to the caller so it sits with the other
/// text in the row instead of inside the bar, and there is no spinner: motion
/// in this app comes from the values changing, not from decoration.
pub(crate) fn progress_bar(ui: &mut egui::Ui, fraction: f32) -> egui::Response {
    const BAR_HEIGHT: f32 = 10.0;

    let fraction = fraction.clamp(0.0, 1.0);
    let (rect, response) = ui.allocate_exact_size(
        Vec2::new(ui.available_width(), BAR_HEIGHT),
        egui::Sense::hover(),
    );

    if ui.is_rect_visible(rect) {
        let corner_radius = CornerRadius::same(RADIUS_SM);
        ui.painter().rect_filled(rect, corner_radius, FIELD);

        let filled =
            egui::Rect::from_min_size(rect.min, Vec2::new(rect.width() * fraction, rect.height()));

        if filled.width() >= 1.0 {
            ui.painter().add(vertical_gradient(
                filled,
                ACCENT_BUTTON_TOP,
                ACCENT_BUTTON_BOTTOM,
            ));
        }
    }

    response
}

/// Read-only state label ("Installed"). It has no fill, outline or hover
/// treatment, so it can never be mistaken for a button next to one.
pub(crate) fn state_pill(
    ui: &mut egui::Ui,
    size: Vec2,
    text: &str,
    color: Color32,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());

    if ui.is_rect_visible(rect) {
        let font = TextStyle::Button.resolve(ui.style());
        let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, color);
    }

    response
}

/// Toggle button that spells out the action on hover: a "Subscribed" button
/// becomes "Unsubscribe" under the pointer. The tinted gradient and
/// pointing-hand cursor mark it as clickable, unlike [`state_pill`].
pub(crate) fn toggle_button(
    ui: &mut egui::Ui,
    size: Vec2,
    label: &str,
    hover_label: &str,
    color: Color32,
    hover_color: Color32,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };

    let (rect, response) = ui.allocate_exact_size(size, sense);

    if ui.is_rect_visible(rect) {
        let hovered = enabled && response.hovered();

        let (top, bottom, text_color, text) = if hovered {
            (
                tint(hover_color, 66),
                tint(hover_color, 34),
                hover_color,
                hover_label,
            )
        } else {
            (tint(color, 48), tint(color, 26), color, label)
        };

        ui.painter().add(vertical_gradient(rect, top, bottom));

        let font = TextStyle::Button.resolve(ui.style());
        let galley = ui
            .painter()
            .layout_no_wrap(text.to_owned(), font, text_color);
        ui.painter()
            .galley(rect.center() - galley.size() / 2.0, galley, text_color);
    }

    if enabled {
        response.on_hover_cursor(egui::CursorIcon::PointingHand)
    } else {
        response
    }
}

/// Auto-sized gradient action button for dense panels. The neutral text color
/// keeps secondary actions quiet until hover, when the gradient and text switch
/// to `hover_color` (accent, success or danger).
pub(crate) fn compact_button(
    ui: &mut egui::Ui,
    label: &str,
    color: Color32,
    hover_color: Color32,
) -> egui::Response {
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font, Color32::PLACEHOLDER);
    let size = Vec2::new(galley.size().x + 24.0, CONTROL_HEIGHT);

    toggle_button(ui, size, label, label, color, hover_color, true)
}

/// Compact clickable chip used for filters. It shares the gradient and hover
/// treatment with [`compact_button`] so a clickable chip never reads as a tag.
pub(crate) fn chip_button(
    ui: &mut egui::Ui,
    label: impl Into<String>,
    color: Color32,
    hover_color: Color32,
) -> egui::Response {
    let label = label.into();
    let font = TextStyle::Button.resolve(ui.style());
    let galley = ui
        .painter()
        .layout_no_wrap(label.clone(), font, Color32::PLACEHOLDER);
    let size = Vec2::new(galley.size().x + 16.0, 22.0);

    toggle_button(ui, size, &label, &label, color, hover_color, true)
}

/// Text tab with an accent underline, used for top-level navigation.
pub(crate) fn tab(ui: &mut egui::Ui, selected: bool, label: &str) -> egui::Response {
    tab_with_count(ui, selected, label, None)
}

/// [`tab`] with a count drawn inside the same clickable area. Keeping the
/// number in the button avoids a floating badge that reads as a second control.
pub(crate) fn tab_with_count(
    ui: &mut egui::Ui,
    selected: bool,
    label: &str,
    count: Option<String>,
) -> egui::Response {
    let font = medium_font(14.0);
    let padding = Vec2::new(12.0, 9.0);
    let label_galley =
        ui.painter()
            .layout_no_wrap(label.to_owned(), font.clone(), Color32::PLACEHOLDER);
    let count_galley = count.map(|count| {
        ui.painter().layout_no_wrap(
            format!("  {count}"),
            medium_font(11.0),
            Color32::PLACEHOLDER,
        )
    });

    let label_size = label_galley.size();
    let count_size = count_galley
        .as_ref()
        .map_or(Vec2::ZERO, |galley| galley.size());
    let text_size = Vec2::new(label_size.x + count_size.x, label_size.y.max(count_size.y));
    let size = text_size + padding * 2.0 + Vec2::new(0.0, 2.0);

    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());

    if ui.is_rect_visible(rect) {
        let hovered = response.hovered();
        let hover = ui.ctx().animate_bool_with_time(
            ui.id().with(("tab_hover", label)),
            hovered && !selected,
            0.12,
        );
        let reveal =
            ui.ctx()
                .animate_bool_with_time(ui.id().with(("tab_underline", label)), selected, 0.16);
        let color = if selected || hovered {
            TEXT
        } else {
            TEXT_MUTED
        };

        if hover > 0.0 {
            ui.painter().rect_filled(
                rect,
                CornerRadius::ZERO,
                tint(SURFACE_HOVER, (hover * 255.0) as u8),
            );
        }

        let block_left = rect.center().x - text_size.x / 2.0;
        let center_y = rect.center().y - 1.0;
        let label_pos = egui::pos2(block_left, center_y - label_size.y / 2.0);
        ui.painter().galley(label_pos, label_galley, color);

        if let Some(count_galley) = count_galley {
            let count_color = if selected || hovered {
                TEXT_MUTED
            } else {
                TEXT_FAINT
            };
            let count_pos = egui::pos2(
                block_left + label_size.x,
                center_y - count_galley.size().y / 2.0,
            );
            ui.painter().galley(count_pos, count_galley, count_color);
        }

        if reveal > 0.0 {
            let underline = egui::Rect::from_min_max(
                egui::pos2(rect.left() + 8.0, rect.bottom() - 2.0),
                egui::pos2(rect.right() - 8.0, rect.bottom()),
            );

            ui.painter().rect_filled(
                underline,
                CornerRadius::ZERO,
                tint(ACCENT, (reveal * 255.0) as u8),
            );
        }
    }

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

/// Custom square checkbox rendered with RimMod's check/uncheck SVG icons
/// instead of egui's default vector checkbox.
pub(crate) struct Checkbox<'a> {
    checked: &'a mut bool,
    text: egui::WidgetText,
}

impl<'a> Checkbox<'a> {
    pub(crate) fn new(checked: &'a mut bool, text: impl Into<egui::WidgetText>) -> Self {
        Self {
            checked,
            text: text.into(),
        }
    }
}

impl egui::Widget for Checkbox<'_> {
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        const ICON_SIZE: f32 = 16.0;
        const ICON_SPACING: f32 = 6.0;
        const PADDING: Vec2 = Vec2::new(2.0, 2.0);
        const MIN_HEIGHT: f32 = 22.0;

        let has_text = !self.text.is_empty();
        let extra_x = PADDING.x * 2.0 + ICON_SIZE + if has_text { ICON_SPACING } else { 0.0 };
        let wrap_width = (ui.available_width() - extra_x).max(0.0);
        let galley = has_text.then(|| {
            self.text
                .into_galley(ui, None, wrap_width, TextStyle::Button)
        });
        let text_size = galley.as_ref().map_or(Vec2::ZERO, |g| g.size());
        let desired_size = Vec2::new(
            extra_x + text_size.x,
            (ICON_SIZE.max(text_size.y) + PADDING.y * 2.0).max(MIN_HEIGHT),
        );

        let (rect, mut response) = ui.allocate_exact_size(desired_size, egui::Sense::click());
        let enabled = ui.is_enabled();

        if enabled && response.clicked() {
            *self.checked = !*self.checked;
            response.mark_changed();
        }

        response.widget_info(|| {
            egui::WidgetInfo::selected(
                egui::WidgetType::Checkbox,
                enabled,
                *self.checked,
                galley.as_ref().map_or("", |g| g.text()),
            )
        });

        if ui.is_rect_visible(rect) {
            let hovered = enabled && (response.hovered() || response.has_focus());
            let active = enabled && response.is_pointer_button_down_on();

            let icon_center_y = galley.as_ref().map_or(rect.center().y, |g| {
                let text_top = rect.center().y - g.size().y / 2.0;
                let first_line_height = g.rows.first().map_or(g.size().y, |row| row.height());
                text_top + first_line_height / 2.0
            });
            let icon_rect = egui::Rect::from_center_size(
                egui::pos2(rect.left() + PADDING.x + ICON_SIZE / 2.0, icon_center_y),
                Vec2::splat(ICON_SIZE),
            );

            let icon_source = if *self.checked {
                icons::checkbox_checked_icon()
            } else {
                icons::checkbox_unchecked_icon()
            };
            let icon_tint = if enabled {
                Color32::WHITE
            } else {
                Color32::from_white_alpha(110)
            };

            egui::Image::new(icon_source)
                .fit_to_exact_size(icon_rect.size())
                .tint(icon_tint)
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

            if let Some(galley) = galley {
                let text_color = if !enabled {
                    TEXT_FAINT
                } else if hovered || active {
                    Color32::WHITE
                } else {
                    TEXT
                };
                let text_pos = egui::pos2(
                    icon_rect.right() + ICON_SPACING,
                    rect.center().y - galley.size().y / 2.0,
                );
                ui.painter().galley(text_pos, galley, text_color);
            }
        }

        if enabled {
            response.on_hover_cursor(egui::CursorIcon::PointingHand)
        } else {
            response
        }
    }
}

/// Convenience wrapper around [`Checkbox`], matching `ui.checkbox`.
pub(crate) fn checkbox(
    ui: &mut egui::Ui,
    checked: &mut bool,
    text: impl Into<egui::WidgetText>,
) -> egui::Response {
    ui.add(Checkbox::new(checked, text))
}

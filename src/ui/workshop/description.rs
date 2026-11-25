//! Steam Workshop BBCode description renderer.

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;

use eframe::egui::{
    self, Color32, CornerRadius, FontFamily, FontId, Margin, Rect, RichText, Sense, Stroke,
    TextFormat, Vec2, text::LayoutJob,
};

use crate::services::workshop::WorkshopItem;
use crate::services::workshop::bbcode::{
    DescriptionBlock, DescriptionImage, DescriptionSpan, TableCell, parse_bbcode,
};
use crate::ui::OpenTarget;
use crate::ui::theme;

use super::{WorkshopAction, WorkshopImageViewer};

#[derive(Clone)]
struct CachedDescription {
    source_len: usize,
    source_hash: u64,
    blocks: Arc<Vec<DescriptionBlock>>,
}

fn hash_description(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn cached_description_blocks(
    ctx: &egui::Context,
    item: &WorkshopItem,
) -> Arc<Vec<DescriptionBlock>> {
    let id = egui::Id::new(("workshop_detail_bbcode", item.published_file_id));
    let source_len = item.description.len();
    let source_hash = hash_description(&item.description);

    if let Some(cached) = ctx.data(|data| data.get_temp::<CachedDescription>(id))
        && cached.source_len == source_len
        && cached.source_hash == source_hash
    {
        return cached.blocks;
    }

    let blocks = Arc::new(parse_bbcode(&item.description));
    ctx.data_mut(|data| {
        data.insert_temp(
            id,
            CachedDescription {
                source_len,
                source_hash,
                blocks: Arc::clone(&blocks),
            },
        );
    });
    blocks
}

pub(super) fn show_description(
    ui: &mut egui::Ui,
    item: &WorkshopItem,
    action: &mut Option<WorkshopAction>,
) {
    let blocks = cached_description_blocks(ui.ctx(), item);

    egui::Frame::new()
        .fill(theme::CANVAS)
        .corner_radius(CornerRadius::ZERO)
        .inner_margin(Margin::same(12))
        .show(ui, |ui| {
            let content_width = ui.available_width();
            ui.set_width(content_width);

            if blocks.is_empty() {
                theme::hint(ui, "This item has no description.");
            } else {
                show_description_blocks(ui, &blocks, &item.title, content_width, false, action);
            }
        });
}

fn show_description_blocks(
    ui: &mut egui::Ui,
    blocks: &[DescriptionBlock],
    item_title: &str,
    available_width: f32,
    default_bold: bool,
    action: &mut Option<WorkshopAction>,
) {
    let width = available_width.max(40.0);

    for (index, block) in blocks.iter().enumerate() {
        if index > 0 {
            let spacing = match block {
                DescriptionBlock::Heading { level: 1, .. } => 10.0,
                DescriptionBlock::Heading { level: 2, .. } => 8.0,
                DescriptionBlock::Heading { .. } => 6.0,
                _ => 6.0,
            };
            ui.add_space(spacing);
        }

        match block {
            DescriptionBlock::Heading { level, spans } => {
                let (font_size, bold) = match level {
                    1 => (18.0, true),
                    2 => (16.0, false),
                    _ => (14.0, true),
                };
                render_rich_spans(ui, spans, font_size, theme::ACCENT, bold, width, action);
                ui.add_space(2.0);
            }
            DescriptionBlock::Paragraph(spans) => {
                let base_color = if default_bold {
                    theme::TEXT
                } else {
                    theme::TEXT_MUTED
                };
                render_rich_spans(ui, spans, 13.0, base_color, default_bold, width, action);
            }
            DescriptionBlock::ImageRow(images) => {
                render_description_images(ui, images, item_title, width, action);
            }
            DescriptionBlock::List { ordered, items } => {
                let indent = 12.0;
                let marker_width = if *ordered { 24.0 } else { 14.0 };
                let item_width = (width - indent - marker_width - 4.0).max(60.0);

                for (item_index, item_blocks) in items.iter().enumerate() {
                    if item_index > 0 {
                        ui.add_space(3.0);
                    }

                    ui.horizontal_top(|ui| {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        ui.add_space(indent);

                        let marker = if *ordered {
                            format!("{}.", item_index + 1)
                        } else {
                            "•".to_owned()
                        };
                        ui.allocate_ui_with_layout(
                            Vec2::new(marker_width, 0.0),
                            egui::Layout::right_to_left(egui::Align::Min),
                            |ui| {
                                ui.label(RichText::new(marker).color(theme::TEXT_MUTED));
                            },
                        );

                        ui.allocate_ui_with_layout(
                            Vec2::new(item_width, 0.0),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                ui.set_width(item_width);
                                show_description_blocks(
                                    ui,
                                    item_blocks,
                                    item_title,
                                    item_width,
                                    default_bold,
                                    action,
                                );
                            },
                        );
                    });
                }
            }
            DescriptionBlock::Quote { author, blocks } => {
                let frame = egui::Frame::new()
                    .fill(theme::SURFACE)
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin {
                        left: 12,
                        right: 10,
                        top: 6,
                        bottom: 6,
                    })
                    .show(ui, |ui| {
                        let inner_width = (width - 22.0).max(60.0);
                        ui.set_width(inner_width);

                        if let Some(author) = author {
                            ui.label(
                                RichText::new(format!("{author} wrote:"))
                                    .small()
                                    .color(theme::TEXT_FAINT),
                            );
                            ui.add_space(2.0);
                        }

                        show_description_blocks(ui, blocks, item_title, inner_width, false, action);
                    });

                let quote_rect = frame.response.rect;
                let bar_rect = Rect::from_min_max(
                    quote_rect.left_top(),
                    egui::pos2(quote_rect.left() + 2.0, quote_rect.bottom()),
                );
                ui.painter()
                    .rect_filled(bar_rect, CornerRadius::ZERO, theme::ACCENT);
            }
            DescriptionBlock::Code(code) => {
                egui::Frame::new()
                    .fill(theme::SURFACE)
                    .corner_radius(CornerRadius::ZERO)
                    .inner_margin(Margin::same(8))
                    .show(ui, |ui| {
                        let inner_width = (width - 16.0).max(60.0);
                        ui.set_width(inner_width);
                        ui.add(
                            egui::Label::new(RichText::new(code).monospace().color(theme::TEXT))
                                .wrap()
                                .selectable(true),
                        );
                    });
            }
            DescriptionBlock::Table { bordered, rows } => {
                render_description_table(ui, *bordered, rows, item_title, width, action);
            }
            DescriptionBlock::Divider => {
                ui.add_space(2.0);
                ui.separator();
                ui.add_space(2.0);
            }
            DescriptionBlock::YouTube { video_id } => {
                let url = format!("https://www.youtube.com/watch?v={video_id}");
                if ui
                    .link(format!("▶ Watch on YouTube ({video_id})"))
                    .on_hover_text(&url)
                    .clicked()
                {
                    *action = Some(WorkshopAction::Open(OpenTarget::Url(url)));
                }
            }
        }
    }
}

fn render_description_images(
    ui: &mut egui::Ui,
    images: &[DescriptionImage],
    item_title: &str,
    available_width: f32,
    action: &mut Option<WorkshopAction>,
) {
    let gap = 8.0;

    match images.len() {
        0 => {}
        1 => {
            render_single_description_image(ui, &images[0], item_title, available_width, action);
        }
        count @ 2..=4 => {
            let slot_width =
                ((available_width - gap * (count - 1) as f32) / count as f32).max(60.0);
            ui.horizontal_top(|ui| {
                ui.spacing_mut().item_spacing.x = gap;
                for image in images {
                    render_single_description_image(ui, image, item_title, slot_width, action);
                }
            });
        }
        _ => {
            let slot_width = ((available_width - gap * 3.0) / 4.0).max(60.0);
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = Vec2::splat(gap);
                for image in images {
                    render_single_description_image(ui, image, item_title, slot_width, action);
                }
            });
        }
    }
}

fn render_single_description_image(
    ui: &mut egui::Ui,
    image: &DescriptionImage,
    item_title: &str,
    max_width: f32,
    action: &mut Option<WorkshopAction>,
) {
    let mut response = ui
        .add(
            egui::Image::new(image.url.as_str())
                .max_width(max_width)
                .maintain_aspect_ratio(true)
                .show_loading_spinner(true)
                .sense(Sense::click())
                .alt_text(format!("Description image for {item_title}")),
        )
        .on_hover_cursor(egui::CursorIcon::PointingHand);

    response = if let Some(link) = &image.link {
        response.on_hover_text(link)
    } else {
        response.on_hover_text("Click to view larger")
    };

    if response.clicked() {
        *action = Some(match &image.link {
            Some(link) => WorkshopAction::Open(OpenTarget::Url(link.clone())),
            None => WorkshopAction::ShowImage(WorkshopImageViewer::Single(image.url.clone())),
        });
    }
}

fn render_description_table(
    ui: &mut egui::Ui,
    bordered: bool,
    rows: &[Vec<TableCell>],
    item_title: &str,
    available_width: f32,
    action: &mut Option<WorkshopAction>,
) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    if cols == 0 {
        return;
    }

    let image_only = rows.iter().flatten().all(|cell| {
        cell.blocks
            .iter()
            .all(|block| matches!(block, DescriptionBlock::ImageRow(_)))
    });
    let show_borders = bordered && !image_only;
    let gap = 8.0;
    let col_width = ((available_width - gap * (cols - 1) as f32) / cols as f32).max(40.0);

    for (row_index, row) in rows.iter().enumerate() {
        if row_index > 0 {
            if show_borders {
                ui.add_space(4.0);
                ui.separator();
                ui.add_space(4.0);
            } else {
                ui.add_space(gap);
            }
        }

        ui.horizontal_top(|ui| {
            ui.spacing_mut().item_spacing.x = gap;

            for cell in row {
                ui.allocate_ui_with_layout(
                    Vec2::new(col_width, 0.0),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.set_width(col_width);

                        if cell.header && show_borders {
                            egui::Frame::new()
                                .fill(theme::SURFACE)
                                .corner_radius(CornerRadius::ZERO)
                                .inner_margin(Margin::symmetric(6, 4))
                                .show(ui, |ui| {
                                    let inner = (col_width - 12.0).max(24.0);
                                    ui.set_width(inner);
                                    show_description_blocks(
                                        ui,
                                        &cell.blocks,
                                        item_title,
                                        inner,
                                        true,
                                        action,
                                    );
                                });
                        } else {
                            show_description_blocks(
                                ui,
                                &cell.blocks,
                                item_title,
                                col_width,
                                cell.header,
                                action,
                            );
                        }
                    },
                );
            }
        });
    }
}

fn build_rich_layout_job(
    spans: &[DescriptionSpan],
    base_font_size: f32,
    base_color: Color32,
    base_bold: bool,
    max_width: f32,
    hovered_range: Option<&Range<usize>>,
) -> (LayoutJob, Vec<(Range<usize>, String)>) {
    let mut job = LayoutJob::default();
    job.wrap.max_width = max_width;

    let mut link_ranges = Vec::new();
    let mut char_offset = 0usize;

    for span in spans {
        let char_len = span.text.chars().count();
        let range = char_offset..char_offset + char_len;
        let is_hovered_link = hovered_range == Some(&range);

        let is_bold = base_bold || span.style.bold;
        let font_id = if is_bold {
            theme::semibold_font(base_font_size)
        } else if base_font_size >= 15.0 {
            theme::medium_font(base_font_size)
        } else {
            FontId::new(base_font_size, FontFamily::Proportional)
        };

        let color = if span.link.is_some() {
            theme::ACCENT
        } else if span.style.spoiler {
            theme::TEXT_FAINT
        } else if span.style.bold && base_color == theme::TEXT_MUTED {
            theme::TEXT
        } else {
            base_color
        };

        let underline = if span.style.underline || is_hovered_link {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        };
        let strikethrough = if span.style.strike {
            Stroke::new(1.0, color)
        } else {
            Stroke::NONE
        };
        let background = if span.style.spoiler {
            theme::SURFACE_HOVER
        } else {
            Color32::TRANSPARENT
        };

        job.append(
            &span.text,
            0.0,
            TextFormat {
                font_id,
                color,
                background,
                italics: span.style.italic,
                underline,
                strikethrough,
                ..Default::default()
            },
        );

        if let Some(link) = &span.link {
            link_ranges.push((range, link.clone()));
        }

        char_offset += char_len;
    }

    (job, link_ranges)
}

fn render_rich_spans(
    ui: &mut egui::Ui,
    spans: &[DescriptionSpan],
    base_font_size: f32,
    base_color: Color32,
    base_bold: bool,
    available_width: f32,
    action: &mut Option<WorkshopAction>,
) {
    let (job, link_ranges) = build_rich_layout_job(
        spans,
        base_font_size,
        base_color,
        base_bold,
        available_width,
        None,
    );

    if link_ranges.is_empty() {
        ui.add(egui::Label::new(job).wrap().selectable(true));
        return;
    }

    let galley = ui.painter().layout_job(job);
    let (rect, response) = ui.allocate_exact_size(galley.size(), Sense::click());

    let hovered_link = response.hover_pos().and_then(|pos| {
        let local = pos - rect.min;
        let cursor = galley.cursor_from_pos(local);
        let glyph_rect = galley.pos_from_cursor(cursor);

        if local.x > glyph_rect.right() + 6.0 || local.y > glyph_rect.bottom() + 2.0 {
            return None;
        }

        link_ranges
            .iter()
            .find(|(range, _)| range.contains(&usize::from(cursor.index)))
            .map(|(range, url)| (range.clone(), url.clone()))
    });

    let painted_galley = if let Some((hovered_range, hovered_url)) = hovered_link {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        response.clone().on_hover_text(&hovered_url);

        if response.clicked() {
            *action = Some(WorkshopAction::Open(OpenTarget::Url(hovered_url)));
        }

        let (hover_job, _) = build_rich_layout_job(
            spans,
            base_font_size,
            base_color,
            base_bold,
            available_width,
            Some(&hovered_range),
        );
        ui.painter().layout_job(hover_job)
    } else {
        galley
    };

    ui.painter().galley(rect.min, painted_galley, base_color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steam_description_with_images_tables_and_formatting_renders_and_caches() {
        let context = egui::Context::default();
        theme::install(&context);
        let item = WorkshopItem {
            published_file_id: 999,
            title: "Dead Man's Switch".to_owned(),
            description: "[url=https://steamcommunity.com/id/test][img]https://example.com/banner.png[/img][/url]\n\
                [h1]Introduction[/h1]\n\
                [b]DMS2[/b] with [url=https://example.com]docs[/url].\n\
                [list]\n\
                [*][b]Harmony[/b]\n\
                [*]Biotech DLC\n\
                [/list]\n\
                [h2]Content[/h2]\n\
                [table noborder=1]\n\
                [tr]\n\
                [td][img]https://example.com/left.png[/img][/td]\n\
                [td][img]https://example.com/right.png[/img][/td]\n\
                [/tr]\n\
                [/table]\n\
                [quote=Author]Quoted text[/quote]\n\
                [code]raw_code()[/code]\n\
                [hr][/hr]\n\
                [previewyoutube=dQw4w9WgXcQ;full][/previewyoutube]"
                .to_owned(),
            preview_url: None,
            images: Vec::new(),
            author_steam_id: None,
            author: None,
            subscriptions: None,
            favorites: None,
            time_created: 0,
            time_updated: 0,
            votes_up: 0,
            votes_down: 0,
            tags: Vec::new(),
            supported_versions: Vec::new(),
        };

        let mut action = None;
        let mut output = context.run_ui(egui::RawInput::default(), |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                show_description(ui, &item, &mut action);
            });
        });
        output.textures_delta.clear();

        let first = cached_description_blocks(&context, &item);
        let second = cached_description_blocks(&context, &item);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(action.is_none());
    }
}

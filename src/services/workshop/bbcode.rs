//! Parser for Steam Workshop BBCode descriptions.
//!
//! Steam descriptions mix headings, inline text styles, links, embedded images
//! (often wrapped in `[url=...]` or arranged in `[table]` grids), lists,
//! quotes, and code blocks. Parsing them into structured blocks lets the detail
//! window render descriptions close to how they appear on the Steam Workshop.

const MAX_NESTING_DEPTH: usize = 12;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum DescriptionBlock {
    Heading {
        level: u8,
        spans: Vec<DescriptionSpan>,
    },
    Paragraph(Vec<DescriptionSpan>),
    ImageRow(Vec<DescriptionImage>),
    List {
        ordered: bool,
        items: Vec<Vec<DescriptionBlock>>,
    },
    Quote {
        author: Option<String>,
        blocks: Vec<DescriptionBlock>,
    },
    Code(String),
    Table {
        bordered: bool,
        rows: Vec<Vec<TableCell>>,
    },
    Divider,
    YouTube {
        video_id: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TableCell {
    pub(crate) header: bool,
    pub(crate) blocks: Vec<DescriptionBlock>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DescriptionImage {
    pub(crate) url: String,
    pub(crate) link: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) struct SpanStyle {
    pub(crate) bold: bool,
    pub(crate) italic: bool,
    pub(crate) underline: bool,
    pub(crate) strike: bool,
    pub(crate) spoiler: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DescriptionSpan {
    pub(crate) text: String,
    pub(crate) style: SpanStyle,
    pub(crate) link: Option<String>,
}

pub(crate) fn normalize_image_url(url: &str) -> Option<String> {
    let url = url.trim().trim_matches('"').trim_matches('\'').trim();

    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url.to_owned())
    } else {
        url.strip_prefix("//").map(|rest| format!("https://{rest}"))
    }
}

pub(crate) fn normalize_link_url(url: &str) -> Option<String> {
    let url = url.trim().trim_matches('"').trim_matches('\'').trim();

    if url.starts_with("https://") || url.starts_with("http://") {
        Some(url.to_owned())
    } else if let Some(rest) = url.strip_prefix("//") {
        Some(format!("https://{rest}"))
    } else if url.starts_with("www.") {
        Some(format!("https://{url}"))
    } else {
        None
    }
}

/// Parse a Steam Workshop BBCode description into renderable blocks.
pub(crate) fn parse_bbcode(input: &str) -> Vec<DescriptionBlock> {
    let normalized = input.replace("\r\n", "\n").replace('\r', "\n");
    parse_blocks(&normalized, 0, StyleStack::default())
}

#[derive(Clone, Default)]
struct StyleStack {
    bold: usize,
    italic: usize,
    underline: usize,
    strike: usize,
    spoiler: usize,
    links: Vec<Option<String>>,
}

impl StyleStack {
    fn current_style(&self) -> SpanStyle {
        SpanStyle {
            bold: self.bold > 0,
            italic: self.italic > 0,
            underline: self.underline > 0,
            strike: self.strike > 0,
            spoiler: self.spoiler > 0,
        }
    }

    fn current_link(&self) -> Option<String> {
        self.links.iter().rev().find_map(Clone::clone)
    }
}

struct BlockBuilder {
    blocks: Vec<DescriptionBlock>,
    spans: Vec<DescriptionSpan>,
    images: Vec<DescriptionImage>,
    pending_newlines: usize,
}

impl BlockBuilder {
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            spans: Vec::new(),
            images: Vec::new(),
            pending_newlines: 0,
        }
    }

    fn flush_images(&mut self) {
        if !self.images.is_empty() {
            self.blocks
                .push(DescriptionBlock::ImageRow(std::mem::take(&mut self.images)));
        }
    }

    fn flush_paragraph(&mut self) {
        let mut spans = std::mem::take(&mut self.spans);
        trim_spans(&mut spans);
        if !spans.is_empty() {
            self.blocks.push(DescriptionBlock::Paragraph(spans));
        }
        self.pending_newlines = 0;
    }

    fn flush_inline(&mut self) {
        self.flush_paragraph();
        self.flush_images();
        self.pending_newlines = 0;
    }

    fn push_image(&mut self, image: DescriptionImage) {
        if !self.spans.is_empty() {
            self.flush_paragraph();
        }
        self.pending_newlines = 0;
        self.images.push(image);
    }

    fn push_text(&mut self, text: &str, style: SpanStyle, link: Option<String>) {
        if text.is_empty() {
            return;
        }

        // If we are accumulating adjacent images on the same line, ignore
        // horizontal whitespace between them so `[img]a[/img] [img]b[/img]`
        // stays on one row.
        if !self.images.is_empty() && text.chars().all(|ch| ch == ' ' || ch == '\t') {
            return;
        }

        let mut remaining = text;
        while let Some(newline_pos) = remaining.find('\n') {
            let segment = &remaining[..newline_pos];
            self.push_line_segment(segment, style, link.clone());

            if !self.images.is_empty() {
                self.flush_images();
                self.pending_newlines = 0;
            } else if !self.spans.is_empty() {
                self.pending_newlines += 1;
                if self.pending_newlines >= 2 {
                    self.flush_paragraph();
                }
            }

            remaining = &remaining[newline_pos + 1..];
        }

        self.push_line_segment(remaining, style, link);
    }

    fn push_line_segment(&mut self, segment: &str, style: SpanStyle, link: Option<String>) {
        if segment.is_empty() {
            return;
        }

        if !self.images.is_empty() {
            if segment.chars().all(|ch| ch == ' ' || ch == '\t') {
                return;
            }
            self.flush_images();
        }

        // Skip leading whitespace-only segments before any real text starts a
        // paragraph so indentation around block tags does not create blank gaps.
        if self.spans.is_empty() && segment.trim().is_empty() {
            return;
        }

        if self.pending_newlines == 1 && !self.spans.is_empty() {
            self.append_span("\n", style, None);
            self.pending_newlines = 0;
        }

        self.append_span(segment, style, link);
    }

    fn append_span(&mut self, text: &str, style: SpanStyle, link: Option<String>) {
        if text.is_empty() {
            return;
        }

        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && last.link == link
        {
            last.text.push_str(text);
        } else {
            self.spans.push(DescriptionSpan {
                text: text.to_owned(),
                style,
                link,
            });
        }
    }

    fn finish(mut self) -> Vec<DescriptionBlock> {
        self.flush_inline();
        self.blocks
    }
}

fn trim_spans(spans: &mut Vec<DescriptionSpan>) {
    while let Some(first) = spans.first_mut() {
        let trimmed = first.text.trim_start_matches(['\n', '\r']);
        if trimmed.is_empty() && first.text.trim().is_empty() {
            spans.remove(0);
        } else {
            let trimmed = first.text.trim_start();
            if trimmed.len() != first.text.len() {
                first.text = trimmed.to_owned();
            }
            break;
        }
    }

    while let Some(last) = spans.last_mut() {
        if last.text.trim().is_empty() {
            spans.pop();
        } else {
            let trimmed = last.text.trim_end();
            if trimmed.len() != last.text.len() {
                last.text = trimmed.to_owned();
            }
            break;
        }
    }
}

struct ParsedTag<'a> {
    closing: bool,
    name: String,
    attr: Option<&'a str>,
    end: usize,
}

fn parse_tag_at(input: &str, start: usize) -> Option<ParsedTag<'_>> {
    if !input[start..].starts_with('[') {
        return None;
    }

    let rest = &input[start + 1..];
    let close_offset = rest.find(']')?;
    let raw = &rest[..close_offset];

    // Real Steam BBCode tags are single-line and do not contain nested `[`.
    if raw.is_empty() || raw.contains('\n') || raw.contains('[') {
        return None;
    }

    let end = start + 1 + close_offset + 1;
    let trimmed = raw.trim();
    let (closing, body) = match trimmed.strip_prefix('/') {
        Some(rest) => (true, rest.trim()),
        None => (false, trimmed),
    };

    if body.is_empty() {
        return None;
    }

    let (name_raw, attr) = match (body.find('='), body.find(char::is_whitespace)) {
        (Some(eq), Some(space)) if space < eq => {
            let rest = body[space + 1..].trim();
            let rest = rest.strip_prefix('=').map_or(rest, str::trim);
            (&body[..space], Some(rest))
        }
        (Some(eq), _) => (&body[..eq], Some(body[eq + 1..].trim())),
        (None, Some(space)) => (&body[..space], Some(body[space + 1..].trim())),
        (None, None) => (body, None),
    };

    let name = name_raw.trim().to_ascii_lowercase();
    if name.is_empty() {
        return None;
    }

    Some(ParsedTag {
        closing,
        name,
        attr,
        end,
    })
}

fn is_recognized_tag(name: &str) -> bool {
    matches!(
        name,
        "b" | "i"
            | "u"
            | "s"
            | "strike"
            | "h1"
            | "h2"
            | "h3"
            | "list"
            | "olist"
            | "*"
            | "url"
            | "img"
            | "quote"
            | "code"
            | "noparse"
            | "spoiler"
            | "hr"
            | "p"
            | "table"
            | "tr"
            | "td"
            | "th"
            | "previewyoutube"
    )
}

/// Find the byte range of the matching closing tag `[/tag_name]` starting at
/// `search_from`, respecting nested `[tag_name]` blocks as well as `[code]` and
/// `[noparse]` sections. Returns `(close_tag_start, close_tag_end)`.
fn find_matching_close(input: &str, search_from: usize, tag_name: &str) -> Option<(usize, usize)> {
    let mut pos = search_from;
    let mut depth = 1usize;

    while let Some(rel) = input[pos..].find('[') {
        let open = pos + rel;
        let Some(tag) = parse_tag_at(input, open) else {
            pos = open + 1;
            continue;
        };

        // Ignore tags inside literal `[code]` or `[noparse]` unless we are
        // actively searching for their own closing tag.
        if !tag.closing && (tag.name == "code" || tag.name == "noparse") && tag.name != tag_name {
            if let Some((_, literal_end)) = find_simple_close(input, tag.end, &tag.name) {
                pos = literal_end;
                continue;
            }
            return None;
        }

        if tag.name == tag_name {
            if tag.closing {
                depth -= 1;
                if depth == 0 {
                    return Some((open, tag.end));
                }
            } else {
                depth += 1;
            }
        }

        pos = tag.end;
    }

    None
}

/// Find the first `[/tag_name]` without nesting (used for `[code]`, `[noparse]`,
/// `[img]`, and `[previewyoutube]`).
fn find_simple_close(input: &str, search_from: usize, tag_name: &str) -> Option<(usize, usize)> {
    let lowercase = input.to_ascii_lowercase();
    let needle = format!("[/{tag_name}]");
    let rel = lowercase[search_from..].find(&needle)?;
    let start = search_from + rel;
    Some((start, start + needle.len()))
}

fn skip_one_newline(input: &str, pos: usize) -> usize {
    if input[pos..].starts_with('\n') {
        pos + 1
    } else {
        pos
    }
}

fn parse_blocks(input: &str, depth: usize, mut style: StyleStack) -> Vec<DescriptionBlock> {
    let mut builder = BlockBuilder::new();
    let mut pos = 0;

    while pos < input.len() {
        let Some(rel_open) = input[pos..].find('[') else {
            builder.push_text(&input[pos..], style.current_style(), style.current_link());
            break;
        };

        if rel_open > 0 {
            builder.push_text(
                &input[pos..pos + rel_open],
                style.current_style(),
                style.current_link(),
            );
        }

        let open = pos + rel_open;
        let Some(tag) = parse_tag_at(input, open) else {
            builder.push_text("[", style.current_style(), style.current_link());
            pos = open + 1;
            continue;
        };

        if !is_recognized_tag(&tag.name) {
            builder.push_text(
                &input[open..tag.end],
                style.current_style(),
                style.current_link(),
            );
            pos = tag.end;
            continue;
        }

        match tag.name.as_str() {
            "b" => {
                if tag.closing {
                    style.bold = style.bold.saturating_sub(1);
                } else {
                    style.bold += 1;
                }
                pos = tag.end;
            }
            "i" => {
                if tag.closing {
                    style.italic = style.italic.saturating_sub(1);
                } else {
                    style.italic += 1;
                }
                pos = tag.end;
            }
            "u" => {
                if tag.closing {
                    style.underline = style.underline.saturating_sub(1);
                } else {
                    style.underline += 1;
                }
                pos = tag.end;
            }
            "s" | "strike" => {
                if tag.closing {
                    style.strike = style.strike.saturating_sub(1);
                } else {
                    style.strike += 1;
                }
                pos = tag.end;
            }
            "spoiler" => {
                if tag.closing {
                    style.spoiler = style.spoiler.saturating_sub(1);
                } else {
                    style.spoiler += 1;
                }
                pos = tag.end;
            }
            "url" => {
                if tag.closing {
                    style.links.pop();
                    pos = tag.end;
                } else if let Some(attr) = tag.attr {
                    style.links.push(normalize_link_url(attr));
                    pos = tag.end;
                } else if let Some((close_start, close_end)) =
                    find_simple_close(input, tag.end, "url")
                {
                    let inner = &input[tag.end..close_start];
                    if !inner.contains('[') {
                        let label = inner.trim();
                        if !label.is_empty() {
                            builder.push_text(
                                label,
                                style.current_style(),
                                normalize_link_url(label),
                            );
                        }
                        pos = close_end;
                    } else {
                        style.links.push(None);
                        pos = tag.end;
                    }
                } else {
                    style.links.push(None);
                    pos = tag.end;
                }
            }
            "img" => {
                if tag.closing {
                    pos = tag.end;
                } else if let Some((close_start, close_end)) =
                    find_simple_close(input, tag.end, "img")
                {
                    let raw_url = &input[tag.end..close_start];
                    if let Some(url) = normalize_image_url(raw_url) {
                        builder.push_image(DescriptionImage {
                            url,
                            link: style.current_link(),
                        });
                    }
                    pos = close_end;
                } else {
                    pos = tag.end;
                }
            }
            "noparse" => {
                if tag.closing {
                    pos = tag.end;
                } else if let Some((close_start, close_end)) =
                    find_simple_close(input, tag.end, "noparse")
                {
                    builder.push_text(
                        &input[tag.end..close_start],
                        style.current_style(),
                        style.current_link(),
                    );
                    pos = close_end;
                } else {
                    builder.push_text(
                        &input[tag.end..],
                        style.current_style(),
                        style.current_link(),
                    );
                    break;
                }
            }
            "code" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    let (inner, next_pos) = if let Some((close_start, close_end)) =
                        find_simple_close(input, tag.end, "code")
                    {
                        (
                            &input[tag.end..close_start],
                            skip_one_newline(input, close_end),
                        )
                    } else {
                        (&input[tag.end..], input.len())
                    };
                    let trimmed = inner.trim_matches('\n');
                    if !trimmed.is_empty() {
                        builder
                            .blocks
                            .push(DescriptionBlock::Code(trimmed.to_owned()));
                    }
                    pos = next_pos;
                }
            }
            "h1" | "h2" | "h3" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    let level = match tag.name.as_str() {
                        "h1" => 1,
                        "h2" => 2,
                        _ => 3,
                    };
                    let (inner, next_pos) = if let Some((close_start, close_end)) =
                        find_matching_close(input, tag.end, &tag.name)
                    {
                        (
                            &input[tag.end..close_start],
                            skip_one_newline(input, close_end),
                        )
                    } else if let Some(eol) = input[tag.end..].find('\n') {
                        (&input[tag.end..tag.end + eol], tag.end + eol + 1)
                    } else {
                        (&input[tag.end..], input.len())
                    };

                    if depth < MAX_NESTING_DEPTH {
                        let inner_blocks = parse_blocks(inner, depth + 1, style.clone());
                        for block in inner_blocks {
                            match block {
                                DescriptionBlock::Paragraph(spans) => {
                                    builder
                                        .blocks
                                        .push(DescriptionBlock::Heading { level, spans });
                                }
                                other => builder.blocks.push(other),
                            }
                        }
                    }

                    pos = next_pos;
                }
            }
            "hr" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    builder.blocks.push(DescriptionBlock::Divider);
                    let next_pos = if let Some((close_start, close_end)) =
                        find_simple_close(input, tag.end, "hr")
                        && input[tag.end..close_start].trim().is_empty()
                    {
                        close_end
                    } else {
                        tag.end
                    };
                    pos = skip_one_newline(input, next_pos);
                }
            }
            "p" => {
                builder.flush_inline();
                pos = skip_one_newline(input, tag.end);
            }
            "previewyoutube" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    if let Some(video_id) = tag.attr.and_then(parse_youtube_id) {
                        builder.blocks.push(DescriptionBlock::YouTube { video_id });
                    }
                    let next_pos = if let Some((close_start, close_end)) =
                        find_simple_close(input, tag.end, "previewyoutube")
                        && input[tag.end..close_start].trim().is_empty()
                    {
                        close_end
                    } else {
                        tag.end
                    };
                    pos = skip_one_newline(input, next_pos);
                }
            }
            "quote" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    let author = tag
                        .attr
                        .map(|raw| raw.trim().trim_matches('"').trim_matches('\'').trim())
                        .filter(|raw| !raw.is_empty())
                        .map(str::to_owned);
                    let (inner, next_pos) = if let Some((close_start, close_end)) =
                        find_matching_close(input, tag.end, "quote")
                    {
                        (
                            &input[tag.end..close_start],
                            skip_one_newline(input, close_end),
                        )
                    } else {
                        (&input[tag.end..], input.len())
                    };

                    if depth < MAX_NESTING_DEPTH {
                        let blocks = parse_blocks(inner, depth + 1, StyleStack::default());
                        if !blocks.is_empty() {
                            builder
                                .blocks
                                .push(DescriptionBlock::Quote { author, blocks });
                        }
                    }

                    pos = next_pos;
                }
            }
            "list" | "olist" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    let ordered = tag.name == "olist";
                    let (inner, next_pos) = if let Some((close_start, close_end)) =
                        find_matching_close(input, tag.end, &tag.name)
                    {
                        (
                            &input[tag.end..close_start],
                            skip_one_newline(input, close_end),
                        )
                    } else {
                        (&input[tag.end..], input.len())
                    };

                    if depth < MAX_NESTING_DEPTH {
                        let items = parse_list_items(inner, depth + 1);
                        if !items.is_empty() {
                            builder
                                .blocks
                                .push(DescriptionBlock::List { ordered, items });
                        }
                    }

                    pos = next_pos;
                }
            }
            "table" => {
                if tag.closing {
                    pos = tag.end;
                } else {
                    builder.flush_inline();
                    let bordered = !tag
                        .attr
                        .is_some_and(|attr| attr.to_ascii_lowercase().contains("noborder"));
                    let (inner, next_pos) = if let Some((close_start, close_end)) =
                        find_matching_close(input, tag.end, "table")
                    {
                        (
                            &input[tag.end..close_start],
                            skip_one_newline(input, close_end),
                        )
                    } else {
                        (&input[tag.end..], input.len())
                    };

                    if depth < MAX_NESTING_DEPTH {
                        let rows = parse_table_rows(inner, depth + 1);
                        if !rows.is_empty() {
                            builder
                                .blocks
                                .push(DescriptionBlock::Table { bordered, rows });
                        }
                    }

                    pos = next_pos;
                }
            }
            "*" => {
                // Stray `[*]` outside a `[list]`/`[olist]` still reads best as a bullet.
                if !tag.closing {
                    builder.push_text("• ", style.current_style(), style.current_link());
                }
                pos = tag.end;
            }
            // Orphaned table child tags outside `[table]` are ignored cleanly.
            "tr" | "td" | "th" => {
                pos = tag.end;
            }
            _ => {
                pos = tag.end;
            }
        }
    }

    builder.finish()
}

fn parse_youtube_id(attr: &str) -> Option<String> {
    let candidate = attr
        .trim()
        .trim_matches('"')
        .trim_matches('\'')
        .split(';')
        .next()?
        .trim();

    if !candidate.is_empty()
        && candidate
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '-' || ch == '_')
    {
        Some(candidate.to_owned())
    } else {
        None
    }
}

fn parse_list_items(inner: &str, depth: usize) -> Vec<Vec<DescriptionBlock>> {
    let mut item_slices = Vec::new();
    let mut pos = 0;
    let mut current_start: Option<usize> = None;

    while let Some(rel) = inner[pos..].find('[') {
        let open = pos + rel;
        let Some(tag) = parse_tag_at(inner, open) else {
            pos = open + 1;
            continue;
        };

        // Skip over nested lists, tables, quotes, code, and noparse blocks so
        // their inner `[*]` markers do not split the outer list.
        if !tag.closing
            && matches!(
                tag.name.as_str(),
                "list" | "olist" | "table" | "quote" | "code" | "noparse"
            )
        {
            let close = if tag.name == "code" || tag.name == "noparse" {
                find_simple_close(inner, tag.end, &tag.name)
            } else {
                find_matching_close(inner, tag.end, &tag.name)
            };
            if let Some((_, close_end)) = close {
                pos = close_end;
                continue;
            }
            break;
        }

        if tag.name == "*" {
            if tag.closing {
                if let Some(start) = current_start.take() {
                    item_slices.push(&inner[start..open]);
                }
            } else {
                if let Some(start) = current_start.take() {
                    item_slices.push(&inner[start..open]);
                }
                current_start = Some(tag.end);
            }
        }

        pos = tag.end;
    }

    if let Some(start) = current_start {
        item_slices.push(&inner[start..]);
    }

    item_slices
        .into_iter()
        .map(|slice| parse_blocks(slice.trim(), depth, StyleStack::default()))
        .filter(|blocks| !blocks.is_empty())
        .collect()
}

fn parse_table_rows(inner: &str, depth: usize) -> Vec<Vec<TableCell>> {
    let mut rows = Vec::new();
    let mut pos = 0;

    while let Some(rel) = inner[pos..].find('[') {
        let open = pos + rel;
        let Some(tag) = parse_tag_at(inner, open) else {
            pos = open + 1;
            continue;
        };

        if !tag.closing && tag.name == "tr" {
            let (row_inner, next_pos) =
                if let Some((close_start, close_end)) = find_matching_close(inner, tag.end, "tr") {
                    (&inner[tag.end..close_start], close_end)
                } else {
                    (&inner[tag.end..], inner.len())
                };
            let cells = parse_table_cells(row_inner, depth);
            if !cells.is_empty() {
                rows.push(cells);
            }
            pos = next_pos;
        } else {
            pos = tag.end;
        }
    }

    rows
}

fn parse_table_cells(row_inner: &str, depth: usize) -> Vec<TableCell> {
    let mut cells = Vec::new();
    let mut pos = 0;

    while let Some(rel) = row_inner[pos..].find('[') {
        let open = pos + rel;
        let Some(tag) = parse_tag_at(row_inner, open) else {
            pos = open + 1;
            continue;
        };

        if !tag.closing && (tag.name == "td" || tag.name == "th") {
            let header = tag.name == "th";
            let (cell_inner, next_pos) = if let Some((close_start, close_end)) =
                find_matching_close(row_inner, tag.end, &tag.name)
            {
                (&row_inner[tag.end..close_start], close_end)
            } else {
                (&row_inner[tag.end..], row_inner.len())
            };
            let blocks = parse_blocks(cell_inner.trim(), depth, StyleStack::default());
            cells.push(TableCell { header, blocks });
            pos = next_pos;
        } else {
            pos = tag.end;
        }
    }

    cells
}

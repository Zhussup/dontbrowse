use std::collections::HashMap;
use std::sync::Arc;

use ratatui::style::{Modifier, Style};
use unicode_width::UnicodeWidthStr;

use super::wrap::{
    StyledLine, StyledSpan, center_line, indent_line, inline_to_styled, prefix_line, wrap_spans,
};
use crate::images::HalfblockImage;
use crate::model::{Article, Block, ImageBlock, Inline, ListItem, TableBlock};
use crate::ui::theme::Theme;

pub struct RenderedArticle {
    pub lines: Vec<StyledLine>,
    pub block_starts: Vec<usize>,
    pub total_lines: usize,
}

pub fn layout_article(
    article: &Article,
    width: u16,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
) -> RenderedArticle {
    let width = width.max(10);
    let mut lines: Vec<StyledLine> = Vec::new();
    let mut block_starts = Vec::with_capacity(article.blocks.len());

    for block in &article.blocks {
        if !lines.is_empty() && !last_blank(&lines) {
            lines.push(StyledLine::default());
        }
        block_starts.push(lines.len());
        render_block(block, width, theme, images, &mut lines);
    }

    let total_lines = lines.len();
    RenderedArticle {
        lines,
        block_starts,
        total_lines,
    }
}

fn render_blocks(
    blocks: &[Block],
    width: u16,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
) -> Vec<StyledLine> {
    let mut lines = Vec::new();
    for (index, block) in blocks.iter().enumerate() {
        if index > 0 && !last_blank(&lines) {
            lines.push(StyledLine::default());
        }
        render_block(block, width, theme, images, &mut lines);
    }
    lines
}

fn render_block(
    block: &Block,
    width: u16,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    out: &mut Vec<StyledLine>,
) {
    match block {
        Block::Heading { level, spans } => {
            let base = heading_style(*level, theme);
            let styled = inline_to_styled(spans, base, theme);
            out.extend(wrap_spans(&styled, width as usize));
            if *level == 1 {
                out.push(styled_rule(width, theme));
            }
        }
        Block::Paragraph(spans) => {
            let styled = inline_to_styled(spans, theme.base(), theme);
            out.extend(wrap_spans(&styled, width as usize));
        }
        Block::List {
            ordered,
            start,
            items,
        } => render_list(*ordered, *start, items, width, theme, images, out),
        Block::Quote(blocks) => {
            let inner_width = width.saturating_sub(2).max(8);
            let inner = render_blocks(blocks, inner_width, theme, images);
            for line in inner {
                if line.is_blank() {
                    out.push(StyledLine::default());
                } else {
                    out.push(prefix_line(&line, "▎ ", Style::new().fg(theme.accent_dim)));
                }
            }
        }
        Block::Code { language, text } => {
            if let Some(language) = language {
                let mut label = StyledLine::default();
                label.push_span(&format!("  {language}"), theme.faint(), None);
                out.push(label);
            }
            for source_line in text.replace('\r', "").split('\n') {
                let expanded = source_line.replace('\t', "    ");
                let clipped = crate::util::text::truncate_to_width(&expanded, width as usize);
                let used = UnicodeWidthStr::width(clipped.as_str());
                let style = Style::new().fg(theme.text).bg(theme.surface_hi);
                let mut line = StyledLine::default();
                line.push_span(&format!(" {clipped}"), style, None);
                if used + 1 < width as usize {
                    line.push_span(&" ".repeat(width as usize - used - 1), style, None);
                }
                out.push(line);
            }
            out.push(StyledLine::default());
        }
        Block::Table(table) => render_table(table, width, theme, out),
        Block::Image(image) => render_image(image, None, width, theme, images, out),
        Block::Figure { image, caption } => {
            render_image(image, Some(caption), width, theme, images, out)
        }
        Block::Divider => out.push(styled_rule(width, theme)),
    }
}

fn render_list(
    ordered: bool,
    start: u64,
    items: &[ListItem],
    width: u16,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    out: &mut Vec<StyledLine>,
) {
    for (index, item) in items.iter().enumerate() {
        let marker = if ordered {
            format!("{}. ", start + index as u64)
        } else {
            "• ".to_string()
        };
        let marker_width = UnicodeWidthStr::width(marker.as_str());
        let content_width = (width as usize).saturating_sub(marker_width).max(8) as u16;
        let mut item_lines = render_blocks(&item.blocks, content_width, theme, images);

        while item_lines.first().is_some_and(StyledLine::is_blank) {
            item_lines.remove(0);
        }
        while item_lines.last().is_some_and(StyledLine::is_blank) {
            item_lines.pop();
        }
        if item_lines.is_empty() {
            continue;
        }

        for (line_index, line) in item_lines.into_iter().enumerate() {
            if line_index == 0 {
                let mut prefixed = StyledLine::default();
                prefixed.push_span(
                    &marker,
                    Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                    None,
                );
                prefixed.spans.extend(line.spans);
                out.push(prefixed);
            } else if line.is_blank() {
                out.push(StyledLine::default());
            } else {
                out.push(indent_line(&line, marker_width, Style::new().bg(theme.bg)));
            }
        }
    }
}

fn render_image(
    image: &ImageBlock,
    caption: Option<&[Inline]>,
    width: u16,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    out: &mut Vec<StyledLine>,
) {
    if let Some(loaded) = find_image(images, &image.id) {
        for line in loaded.lines() {
            let styled = styled_from_ratatui(line);
            out.push(center_line(&styled, width as usize));
        }
    } else {
        let box_width = (width as usize).clamp(16, 56);
        let fill = "░".repeat(box_width);
        out.push(centered_text(&fill, width, theme.faint()));
        let label = if image.alt.is_empty() {
            "  image".to_string()
        } else {
            format!("  image · {}", image.alt)
        };
        out.push(centered_text(
            &crate::util::text::truncate_to_width(&label, width as usize),
            width,
            theme.faint(),
        ));
        out.push(centered_text(&fill, width, theme.faint()));
    }

    if let Some(caption) = caption
        && !caption.is_empty()
    {
        let styled = inline_to_styled(
            caption,
            Style::new()
                .fg(theme.text_dim)
                .add_modifier(Modifier::ITALIC),
            theme,
        );
        for line in wrap_spans(&styled, width as usize) {
            out.push(center_line(&line, width as usize));
        }
    }
    out.push(StyledLine::default());
}

fn find_image<'a>(
    images: &'a HashMap<String, Arc<HalfblockImage>>,
    id: &str,
) -> Option<&'a Arc<HalfblockImage>> {
    let prefix = format!("art:{id}:");
    images
        .iter()
        .find(|(key, _)| key.starts_with(&prefix))
        .map(|(_, value)| value)
}

fn render_table(table: &TableBlock, width: u16, theme: &Theme, out: &mut Vec<StyledLine>) {
    if let Some(caption) = &table.caption
        && !caption.is_empty()
    {
        let styled = inline_to_styled(
            caption,
            Style::new()
                .fg(theme.text_dim)
                .add_modifier(Modifier::ITALIC),
            theme,
        );
        out.extend(wrap_spans(&styled, width as usize));
    }

    let columns = table
        .headers
        .len()
        .max(table.rows.iter().map(Vec::len).max().unwrap_or(0));
    if columns == 0 {
        return;
    }

    let borders = columns + 1;
    let padding = columns * 2;
    let available = (width as usize).saturating_sub(borders + padding);

    if table.rows.is_empty() {
        return;
    }

    let vertical = columns > 6 || available < columns * 3;
    if vertical {
        render_table_vertical(table, width, theme, out);
        return;
    }

    let mut widths = vec![3usize; columns];
    for row in std::iter::once(&table.headers).chain(table.rows.iter()) {
        for (index, cell) in row.iter().enumerate() {
            if index >= columns {
                break;
            }
            let text = cell_text(cell);
            widths[index] = widths[index].max(UnicodeWidthStr::width(text.as_str()));
        }
    }
    for width_value in &mut widths {
        *width_value = (*width_value).clamp(3, 48);
    }

    let mut total: usize = widths.iter().sum();
    while total > available {
        let Some(index) = widths
            .iter()
            .enumerate()
            .filter(|(_, w)| **w > 3)
            .max_by_key(|(_, w)| **w)
            .map(|(i, _)| i)
        else {
            break;
        };
        widths[index] -= 1;
        total -= 1;
    }
    if widths.iter().sum::<usize>() > available {
        render_table_vertical(table, width, theme, out);
        return;
    }

    out.push(table_border(&widths, "┌", "┬", "┐", theme));
    if !table.headers.is_empty() {
        render_table_row(&table.headers, &widths, theme, true, out);
        out.push(table_border(&widths, "├", "┼", "┤", theme));
    }
    for (index, row) in table.rows.iter().enumerate() {
        render_table_row(row, &widths, theme, false, out);
        if index + 1 < table.rows.len() {
            out.push(table_border(&widths, "├", "┼", "┤", theme));
        }
    }
    out.push(table_border(&widths, "└", "┴", "┘", theme));
}

fn render_table_row(
    cells: &[Vec<Inline>],
    widths: &[usize],
    theme: &Theme,
    header: bool,
    out: &mut Vec<StyledLine>,
) {
    let base = if header {
        Style::new()
            .fg(theme.text)
            .bg(theme.surface_hi)
            .add_modifier(Modifier::BOLD)
    } else {
        theme.base()
    };

    let mut cell_lines: Vec<Vec<StyledLine>> = Vec::with_capacity(widths.len());
    for (index, width) in widths.iter().enumerate() {
        let spans = cells
            .get(index)
            .map(|cell| inline_to_styled(cell, base, theme))
            .unwrap_or_default();
        let mut lines = wrap_spans(&spans, *width);
        if lines.is_empty() {
            lines.push(StyledLine::default());
        }
        cell_lines.push(lines);
    }

    let height = cell_lines.iter().map(Vec::len).max().unwrap_or(1);
    let border_style = Style::new().fg(theme.border);

    for row_line in 0..height {
        let mut line = StyledLine::default();
        line.push_span("│", border_style, None);
        for (index, width) in widths.iter().enumerate() {
            line.push_span(" ", base, None);
            if let Some(cell) = cell_lines.get(index).and_then(|lines| lines.get(row_line)) {
                line.spans.extend(cell.spans.clone());
                let used = cell.width();
                if used < *width {
                    line.push_span(&" ".repeat(width - used), base, None);
                }
            } else {
                line.push_span(&" ".repeat(*width), base, None);
            }
            line.push_span(" ", base, None);
            line.push_span("│", border_style, None);
        }
        out.push(line);
    }
}

fn table_border(
    widths: &[usize],
    left: &str,
    middle: &str,
    right: &str,
    theme: &Theme,
) -> StyledLine {
    let style = Style::new().fg(theme.border);
    let mut line = StyledLine::default();
    line.push_span(left, style, None);
    for (index, width) in widths.iter().enumerate() {
        line.push_span(&"─".repeat(width + 2), style, None);
        line.push_span(
            if index + 1 < widths.len() {
                middle
            } else {
                right
            },
            style,
            None,
        );
    }
    line
}

fn render_table_vertical(table: &TableBlock, width: u16, theme: &Theme, out: &mut Vec<StyledLine>) {
    let rule = styled_rule(width, theme);
    for row in &table.rows {
        if table.headers.is_empty() {
            let mut line = StyledLine::default();
            for (index, cell) in row.iter().enumerate() {
                if index > 0 {
                    line.push_span("  ·  ", theme.faint(), None);
                }
                line.spans
                    .extend(inline_to_styled(cell, theme.base(), theme));
            }
            out.extend(wrap_spans(&line.spans, width as usize));
        } else {
            for (index, cell) in row.iter().enumerate() {
                let key = table
                    .headers
                    .get(index)
                    .map(|header| cell_text(header))
                    .unwrap_or_default();
                let mut spans: Vec<StyledSpan> = Vec::new();
                if !key.is_empty() {
                    spans.push(StyledSpan {
                        text: format!("{key}: "),
                        style: Style::new().fg(theme.text_dim).add_modifier(Modifier::BOLD),
                        link: None,
                    });
                }
                spans.extend(inline_to_styled(cell, theme.base(), theme));
                out.extend(wrap_spans(&spans, width as usize));
            }
        }
        out.push(rule.clone());
    }
}

fn heading_style(level: u8, theme: &Theme) -> Style {
    match level {
        1 => Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        2 => Style::new()
            .fg(theme.accent_hi)
            .add_modifier(Modifier::BOLD),
        3 => Style::new().fg(theme.text).add_modifier(Modifier::BOLD),
        _ => Style::new().fg(theme.text_dim).add_modifier(Modifier::BOLD),
    }
}

fn centered_text(text: &str, width: u16, style: Style) -> StyledLine {
    let used = UnicodeWidthStr::width(text);
    let pad = (width as usize).saturating_sub(used) / 2;
    let mut line = StyledLine::default();
    line.push_span(&" ".repeat(pad), Style::new(), None);
    line.push_span(text, style, None);
    line
}

fn styled_from_ratatui(line: ratatui::text::Line<'static>) -> StyledLine {
    let mut out = StyledLine::default();
    for span in line.spans {
        out.spans.push(StyledSpan {
            text: span.content.into_owned(),
            style: span.style,
            link: None,
        });
    }
    out
}

fn cell_text(cell: &[Inline]) -> String {
    cell.iter().map(|span| span.text.as_str()).collect()
}

fn styled_rule(width: u16, theme: &Theme) -> StyledLine {
    let mut line = StyledLine::default();
    line.push_span(
        &"─".repeat(width as usize),
        Style::new().fg(theme.border),
        None,
    );
    line
}

fn last_blank(lines: &[StyledLine]) -> bool {
    lines.last().is_some_and(StyledLine::is_blank)
}

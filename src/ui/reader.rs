use std::collections::HashMap;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::ReaderState;
use crate::images::HalfblockImage;
use crate::render::layout_article;
use crate::ui::links::LinkRegistry;
use crate::ui::theme::Theme;
use crate::ui::toc;
use crate::ui::{centered, clear_overlay, panel, render_lines, truncate};
use crate::util::time::format_date;

#[allow(clippy::too_many_arguments)]
pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    reader: &mut ReaderState,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    measure_max: u16,
    links: &LinkRegistry,
) {
    if area.width < 24 || area.height < 6 {
        return;
    }
    if let Some(article) = &reader.article {
        draw_header(frame, area, theme, article);
        let header_height = 4u16.min(area.height);
        let content_area = Rect::new(
            area.x,
            area.y + header_height,
            area.width,
            area.height.saturating_sub(header_height),
        );
        draw_body(
            frame,
            content_area,
            reader,
            theme,
            images,
            measure_max,
            links,
        );
    } else if reader.loading {
        let rect = centered(area, area.width.clamp(24, 60), 5);
        clear_overlay(frame, rect);
        let block = panel(theme);
        let inner = block.inner(rect);
        frame.render_widget(block, rect);
        frame.render_widget(
            Paragraph::new(vec![
                Line::from(Span::styled(
                    "Loading article…",
                    Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
                )),
                Line::default(),
                Line::from(Span::styled(
                    truncate(&reader.url, inner.width as usize),
                    theme.faint(),
                )),
            ]),
            inner,
        );
    } else if let Some(error) = &reader.error {
        crate::ui::feed::message_panel(
            frame,
            area,
            theme,
            "Could not read this page",
            error,
            theme.danger,
            "r retry  ·  O open externally  ·  Backspace back",
        );
    } else {
        crate::ui::feed::message_panel(
            frame,
            area,
            theme,
            "Nothing open",
            "Press Ctrl+L and enter a URL to start reading.",
            theme.text_dim,
            "Ctrl+L open URL",
        );
    }
}

fn draw_header(frame: &mut Frame<'_>, area: Rect, theme: &Theme, article: &crate::model::Article) {
    let width = area.width;
    let title = truncate(&article.title, width.saturating_sub(2) as usize);
    frame.render_widget(
        Paragraph::new(Span::styled(
            title,
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        ))
        .style(Style::new().bg(theme.bg)),
        Rect::new(area.x, area.y, width, 1),
    );

    let mut meta: Vec<Span<'static>> = Vec::new();
    if let Some(site) = &article.site_name {
        meta.push(Span::styled(site.clone(), Style::new().fg(theme.text_dim)));
    }
    if let Some(byline) = &article.byline {
        if !meta.is_empty() {
            meta.push(Span::styled("  ·  ", theme.faint()));
        }
        meta.push(Span::styled(
            byline.clone(),
            Style::new().fg(theme.text_dim),
        ));
    }
    if let Some(published) = article.published_at {
        if !meta.is_empty() {
            meta.push(Span::styled("  ·  ", theme.faint()));
        }
        meta.push(Span::styled(format_date(published), theme.faint()));
    }
    if !meta.is_empty() {
        meta.push(Span::styled("  ·  ", theme.faint()));
    }
    meta.push(Span::styled(
        format!("{} min read", article.reading_minutes()),
        theme.faint(),
    ));
    meta.push(Span::styled("  ·  ", theme.faint()));
    meta.push(Span::styled(
        article.source.label().to_string(),
        theme.faint(),
    ));
    frame.render_widget(
        Paragraph::new(Line::from(meta)).style(Style::new().bg(theme.bg)),
        Rect::new(area.x, area.y + 1, width, 1),
    );

    frame.render_widget(
        Paragraph::new(theme.gradient_rule(width)).style(Style::new().bg(theme.bg)),
        Rect::new(area.x, area.y + 2, width, 1),
    );
}

fn draw_body(
    frame: &mut Frame<'_>,
    area: Rect,
    reader: &mut ReaderState,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    measure_max: u16,
    links: &LinkRegistry,
) {
    let show_toc = area.width >= 120 && reader.toc_open;
    let constraints = if show_toc {
        vec![
            Constraint::Length(30),
            Constraint::Min(20),
            Constraint::Length(1),
        ]
    } else {
        vec![Constraint::Min(20), Constraint::Length(1)]
    };
    let columns = Layout::horizontal(constraints).split(area);

    let (content_area, rail_area) = if show_toc {
        (columns[1], columns[2])
    } else {
        (columns[0], columns[1])
    };

    let measure = content_area
        .width
        .saturating_sub(4)
        .min(measure_max)
        .max(20);
    let measure_area = Rect::new(
        content_area.x + (content_area.width.saturating_sub(measure)) / 2,
        content_area.y,
        measure.min(content_area.width),
        content_area.height,
    );

    let Some(article) = &reader.article else {
        return;
    };

    let needs_layout = reader
        .cache
        .as_ref()
        .map(|cache| cache.width != measure)
        .unwrap_or(true);
    if needs_layout {
        let rendered = layout_article(article, measure, theme, images);
        reader.cache = Some(crate::app::state::ReaderCache {
            width: measure,
            lines: rendered.lines,
            block_starts: rendered.block_starts,
            total_lines: rendered.total_lines,
        });
    }

    let (total_lines, block_starts) = reader
        .cache
        .as_ref()
        .map(|cache| (cache.total_lines, cache.block_starts.clone()))
        .unwrap_or((0, Vec::new()));

    reader.viewport_h = measure_area.height as usize;
    let max_scroll = total_lines.saturating_sub(reader.viewport_h.max(1));

    if let Some(progress) = reader.pending_progress.take() {
        reader.scroll = (progress.clamp(0.0, 1.0) * max_scroll as f64).round() as usize;
    }
    reader.scroll = reader.scroll.min(max_scroll);

    if let Some(cache) = &reader.cache {
        let visible = measure_area.height as usize;
        for (index, line) in cache
            .lines
            .iter()
            .enumerate()
            .skip(reader.scroll)
            .take(visible)
        {
            let y = measure_area.y + (index - reader.scroll) as u16;
            let mut x = measure_area.x;
            for span in &line.spans {
                if let Some(url) = &span.link {
                    links.insert_span(x, y, &span.text, url);
                }
                x = x.saturating_add(
                    unicode_width::UnicodeWidthStr::width(span.text.as_str()) as u16
                );
            }
        }
        render_lines(
            frame,
            measure_area,
            &cache.lines,
            reader.scroll,
            theme.base(),
        );
    }

    if show_toc {
        let current_scroll = reader.scroll;
        if let Some(article) = &reader.article {
            toc::draw(
                frame,
                columns[0],
                theme,
                article,
                &block_starts,
                current_scroll,
            );
        }
    }

    draw_rail(
        frame,
        rail_area,
        theme,
        total_lines,
        reader.viewport_h,
        reader.scroll,
    );
}

fn draw_rail(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    total: usize,
    viewport: usize,
    scroll: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let max_scroll = total.saturating_sub(viewport.max(1));
    let ratio = if max_scroll == 0 {
        0.0
    } else {
        scroll as f64 / max_scroll as f64
    };
    let filled = ((ratio * area.height as f64).round() as u16).min(area.height);
    let buffer = frame.buffer_mut();
    for row in 0..area.height {
        let symbol = if row < filled { "█" } else { "░" };
        let style = if row < filled {
            Style::new().fg(theme.accent)
        } else {
            Style::new().fg(theme.border)
        };
        buffer.set_stringn(area.x, area.y + row, symbol, 1, style);
    }
}

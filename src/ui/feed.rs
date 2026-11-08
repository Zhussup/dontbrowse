use std::collections::HashMap;
use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::state::FeedState;
use crate::images::HalfblockImage;
use crate::ui::card;
use crate::ui::theme::Theme;
use crate::ui::{centered, clear_overlay, panel, truncate};

#[derive(Clone, Copy, Debug)]
pub struct GridMetrics {
    pub cols: usize,
    pub card_w: u16,
    pub card_h: u16,
    pub gap: u16,
    pub thumb_w: u16,
    pub thumb_h: u16,
    pub compact: bool,
}

pub fn metrics(area: Rect) -> GridMetrics {
    if area.width < 56 {
        return GridMetrics {
            cols: 1,
            card_w: area.width,
            card_h: 7,
            gap: 1,
            thumb_w: 16,
            thumb_h: 5,
            compact: true,
        };
    }
    let gap = 1u16;
    let min_card = 26u16;
    let cols = ((area.width + gap) / (min_card + gap)).clamp(1, 5) as usize;
    let card_w = area.width.saturating_sub(gap * (cols as u16 - 1)) / cols as u16;
    let thumb_w = card_w.saturating_sub(2).max(4);
    let thumb_h = (((thumb_w as f32) * 9.0 / 16.0 / 2.0).round() as u16).max(3);
    GridMetrics {
        cols,
        card_w,
        card_h: thumb_h + 6,
        gap,
        thumb_w,
        thumb_h,
        compact: false,
    }
}

pub fn thumbnail_key(video_id: &str, thumb_w: u16, thumb_h: u16) -> String {
    format!("feed:{video_id}:{thumb_w}x{thumb_h}")
}

#[allow(clippy::too_many_arguments)]
pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    state: &mut FeedState,
    theme: &Theme,
    images: &HashMap<String, Arc<HalfblockImage>>,
    empty_title: &str,
    empty_body: &str,
    tick: u64,
) {
    let metrics = metrics(area);
    state.cols = metrics.cols;

    if state.items().is_empty() {
        if state.loading {
            draw_skeleton_grid(frame, area, theme, metrics, tick);
        } else if let Some(error) = &state.error {
            message_panel(
                frame,
                area,
                theme,
                "Could not load",
                error,
                theme.danger,
                "press r to retry  ·  Ctrl+P for commands",
            );
        } else {
            message_panel(
                frame,
                area,
                theme,
                empty_title,
                empty_body,
                theme.text_dim,
                "press r to refresh  ·  Ctrl+L to open a URL",
            );
        }
        return;
    }

    let row_stride = metrics.card_h + metrics.gap;
    let visible_rows = (area.height / row_stride).max(1) as usize;
    state.viewport_rows = visible_rows;

    let selected_row = state.selection / metrics.cols.max(1);
    if selected_row < state.scroll_row {
        state.scroll_row = selected_row;
    } else if selected_row >= state.scroll_row + visible_rows {
        state.scroll_row = selected_row + 1 - visible_rows;
    }

    for visible in 0..visible_rows {
        let row_index = state.scroll_row + visible;
        let item_row_start = row_index * metrics.cols;
        if item_row_start >= state.items().len() {
            break;
        }
        let y = area.y + visible as u16 * row_stride;
        if y >= area.bottom() {
            break;
        }
        for col in 0..metrics.cols {
            let index = item_row_start + col;
            let Some(video) = state.items().get(index) else {
                break;
            };
            let x = area.x + col as u16 * (metrics.card_w + metrics.gap);
            if x >= area.right() {
                break;
            }
            let width = metrics.card_w.min(area.right() - x);
            let height = metrics.card_h.min(area.bottom() - y);
            let rect = Rect::new(x, y, width, height);
            let key = thumbnail_key(&video.id, metrics.thumb_w, metrics.thumb_h);
            let image = images.get(&key);
            card::draw(
                frame,
                rect,
                theme,
                video,
                index == state.selection,
                image,
                metrics.thumb_h,
                metrics.compact,
                tick,
            );
        }
    }
}

pub fn draw_skeleton_grid(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    metrics: GridMetrics,
    tick: u64,
) {
    let row_stride = metrics.card_h + metrics.gap;
    let rows = (area.height / row_stride).max(1);
    for row in 0..rows {
        let y = area.y + row * row_stride;
        if y >= area.bottom() {
            break;
        }
        for col in 0..metrics.cols {
            let x = area.x + col as u16 * (metrics.card_w + metrics.gap);
            if x >= area.right() {
                break;
            }
            let rect = Rect::new(
                x,
                y,
                metrics.card_w.min(area.right() - x),
                metrics.card_h.min(area.bottom() - y),
            );
            card::draw_skeleton(frame, rect, theme, metrics.compact, tick);
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn message_panel(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    title: &str,
    body: &str,
    title_color: ratatui::style::Color,
    hint: &str,
) {
    let width = area.width.saturating_sub(4).clamp(24, 68);
    let rect = centered(area, width, 7);
    clear_overlay(frame, rect);
    let block = panel(theme).border_style(Style::new().fg(theme.border));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let lines = vec![
        Line::from(Span::styled(
            title.to_string(),
            Style::new().fg(title_color).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
        Line::from(Span::styled(
            truncate(body, inner.width as usize),
            Style::new().fg(theme.text_dim),
        )),
        Line::from(Span::styled(
            truncate(hint, inner.width as usize),
            Style::new().fg(theme.text_faint),
        )),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .style(Style::new().bg(theme.surface))
            .wrap(Wrap { trim: true }),
        inner,
    );
}

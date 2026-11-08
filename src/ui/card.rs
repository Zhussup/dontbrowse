use std::sync::Arc;

use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph, Wrap};

use crate::images::HalfblockImage;
use crate::model::Video;
use crate::ui::theme::Theme;
use crate::ui::{fill_rect, truncate};

#[allow(clippy::too_many_arguments)]
pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    video: &Video,
    selected: bool,
    image: Option<&Arc<HalfblockImage>>,
    thumb_h: u16,
    compact: bool,
    tick: u64,
) {
    if area.width < 8 || area.height < 4 {
        return;
    }
    let bg = if selected {
        theme.selection_bg
    } else {
        theme.surface
    };
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(if selected {
            theme.border_focus
        } else {
            theme.border
        }))
        .style(Style::new().bg(bg));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width < 4 || inner.height < 3 {
        return;
    }

    if compact {
        let columns = Layout::horizontal([
            Constraint::Length(16),
            Constraint::Length(1),
            Constraint::Min(8),
        ])
        .split(inner);
        draw_thumbnail(frame, columns[0], theme, video, image, selected, tick);
        let text_area = columns[2];
        if text_area.height >= 1 {
            let title = Paragraph::new(truncate(&video.title, text_area.width as usize))
                .style(
                    Style::new()
                        .fg(if selected { theme.accent } else { theme.text })
                        .add_modifier(Modifier::BOLD),
                )
                .wrap(Wrap { trim: true });
            frame.render_widget(title, text_area);
        }
        if text_area.height >= 2 {
            let meta = Paragraph::new(Span::styled(
                truncate(&video.meta_line(), text_area.width as usize),
                Style::new().fg(theme.text_dim),
            ));
            frame.render_widget(
                meta,
                Rect::new(text_area.x, text_area.y + 1, text_area.width, 1),
            );
        }
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(thumb_h),
        Constraint::Length(1),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);

    draw_thumbnail(frame, rows[0], theme, video, image, selected, tick);

    let title = Paragraph::new(video.title.clone())
        .style(
            Style::new()
                .fg(if selected { theme.accent } else { theme.text })
                .add_modifier(Modifier::BOLD),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(title, rows[2]);

    let meta = Paragraph::new(Span::styled(
        truncate(&video.meta_line(), rows[3].width as usize),
        Style::new().fg(theme.text_dim),
    ));
    frame.render_widget(meta, rows[3]);
}

fn draw_thumbnail(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    video: &Video,
    image: Option<&Arc<HalfblockImage>>,
    selected: bool,
    tick: u64,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let bg = if selected {
        theme.selection_bg
    } else {
        theme.surface
    };

    if let Some(image) = image {
        let text = Text::from(image.lines());
        frame.render_widget(Paragraph::new(text).style(Style::new().bg(bg)), area);
    } else {
        fill_rect(
            frame,
            area,
            "░",
            Style::new().fg(theme.border).bg(theme.surface_hi),
        );
        if area.height >= 2 {
            let initial = video
                .channel
                .name
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "▶".to_string());
            let label_area = Rect::new(area.x, area.y + area.height / 2, area.width, 1);
            frame.render_widget(
                Paragraph::new(Span::styled(
                    initial,
                    Style::new()
                        .fg(theme.text_faint)
                        .bg(theme.surface_hi)
                        .add_modifier(Modifier::BOLD),
                ))
                .alignment(Alignment::Center),
                label_area,
            );
        }
        // A subtle moving sheen makes loading feel alive.
        if theme.animations {
            let position = ((tick / 2) as u16) % (area.width + 8);
            let sheen = Rect::new(
                area.x + position.saturating_sub(4),
                area.y,
                3.min(area.width),
                area.height,
            );
            fill_rect(
                frame,
                sheen.intersection(area),
                "▒",
                Style::new().fg(theme.border).bg(theme.surface_hi),
            );
        }
    }

    if video.live {
        let badge = Rect::new(area.x, area.y, 7.min(area.width), 1);
        frame.render_widget(
            Paragraph::new(Span::styled(
                "● LIVE",
                Style::new()
                    .fg(theme.bg)
                    .bg(theme.danger)
                    .add_modifier(Modifier::BOLD),
            )),
            badge,
        );
    }

    if let Some(duration) = video.duration_label() {
        let label = format!(" {duration} ");
        let width = (label.chars().count() as u16).min(area.width);
        let badge_area = Rect::new(
            area.x + area.width.saturating_sub(width),
            area.y + area.height.saturating_sub(1),
            width,
            1,
        );
        frame.render_widget(
            Paragraph::new(Span::styled(
                label,
                Style::new().fg(theme.text).bg(theme.surface_hi),
            )),
            badge_area,
        );
    }
}

pub fn draw_skeleton(frame: &mut Frame<'_>, area: Rect, theme: &Theme, compact: bool, tick: u64) {
    if area.width < 8 || area.height < 4 {
        return;
    }
    let block = Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().bg(theme.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 3 {
        return;
    }

    let pulse = (tick / 3).is_multiple_of(2);
    let shade = if pulse {
        theme.border
    } else {
        theme.surface_hi
    };
    let bar = |width: u16| Span::styled("▬".repeat(width as usize), Style::new().fg(shade));

    if compact {
        fill_rect(
            frame,
            Rect::new(inner.x, inner.y, 16.min(inner.width), inner.height),
            "░",
            Style::new().fg(shade).bg(theme.surface_hi),
        );
        let text_x = inner.x + 17.min(inner.width);
        let text_width = inner.width.saturating_sub(17);
        if text_width > 0 {
            frame.render_widget(
                Paragraph::new(bar(text_width.saturating_sub(4))),
                Rect::new(text_x, inner.y, text_width, 1),
            );
            frame.render_widget(
                Paragraph::new(bar(text_width.saturating_sub(10))),
                Rect::new(text_x, inner.y + 1, text_width, 1),
            );
        }
        return;
    }

    let thumb_h = inner.height.saturating_sub(4).max(2);
    fill_rect(
        frame,
        Rect::new(inner.x, inner.y, inner.width, thumb_h),
        "░",
        Style::new().fg(shade).bg(theme.surface_hi),
    );
    let title_y = inner.y + thumb_h + 1;
    if title_y < inner.bottom() {
        frame.render_widget(
            Paragraph::new(bar(inner.width.saturating_sub(3))),
            Rect::new(inner.x, title_y, inner.width, 1),
        );
    }
    if title_y + 1 < inner.bottom() {
        frame.render_widget(
            Paragraph::new(bar(inner.width.saturating_sub(8))),
            Rect::new(inner.x, title_y + 1, inner.width, 1),
        );
    }
}

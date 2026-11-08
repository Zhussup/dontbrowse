use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::model::Article;
use crate::ui::theme::Theme;
use crate::ui::{panel, truncate};

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    article: &Article,
    block_starts: &[usize],
    scroll_line: usize,
) {
    if area.width < 12 || area.height < 4 {
        return;
    }
    let block = panel(theme);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 {
        return;
    }

    let entries: Vec<&crate::model::TocEntry> = article.toc.iter().collect();
    if entries.is_empty() {
        frame.render_widget(
            Paragraph::new(Span::styled("No sections", theme.faint())),
            inner,
        );
        return;
    }

    let mut current = 0usize;
    for (index, entry) in entries.iter().enumerate() {
        if let Some(start) = block_starts.get(entry.block_index)
            && *start <= scroll_line
        {
            current = index;
        }
    }

    let visible = inner.height.saturating_sub(2) as usize;
    let offset = current
        .saturating_sub(visible / 2)
        .min(entries.len().saturating_sub(visible.max(1)));

    let mut lines = vec![
        Line::from(Span::styled(
            "CONTENTS",
            Style::new()
                .fg(theme.text_faint)
                .add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];

    for (index, entry) in entries.iter().enumerate().skip(offset).take(visible) {
        let is_current = index == current;
        let indent = "  ".repeat(entry.level.saturating_sub(1) as usize);
        let prefix = if is_current { "▸ " } else { "  " };
        let available =
            (inner.width as usize).saturating_sub(indent.chars().count() + prefix.chars().count());
        let title = truncate(&entry.title, available);
        let style = if is_current {
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD)
        } else if entry.level <= 2 {
            Style::new().fg(theme.text_dim)
        } else {
            Style::new().fg(theme.text_faint)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("{indent}{prefix}"), style),
            Span::styled(title, style),
        ]));
    }

    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(theme.surface)),
        inner,
    );
}

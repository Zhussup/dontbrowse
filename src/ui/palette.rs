use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::PaletteState;
use crate::ui::theme::Theme;
use crate::ui::{centered, clear_overlay, focused_panel, truncate};

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    state: &PaletteState,
    names: &[&str],
) {
    let width = area.width.saturating_sub(8).clamp(30, 72);
    let height = area.height.saturating_sub(6).clamp(7, 18);
    let rect = centered(area, width, height);
    clear_overlay(frame, rect);
    let block = focused_panel(theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    if inner.height < 3 {
        return;
    }

    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(1),
    ])
    .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("› ", Style::new().fg(theme.accent)),
            Span::styled(state.input.clone(), Style::new().fg(theme.text)),
            Span::styled("▏", Style::new().fg(theme.accent)),
        ]))
        .style(Style::new().bg(theme.surface)),
        rows[0],
    );

    let visible = rows[2];
    let max_items = visible.height as usize;
    let offset = state.selection.saturating_sub(max_items.saturating_sub(1));
    let mut lines = Vec::new();
    for (index, name) in names.iter().enumerate().skip(offset).take(max_items) {
        let selected = index == state.selection;
        let style = if selected {
            Style::new()
                .fg(theme.accent)
                .bg(theme.selection_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.text_dim)
        };
        lines.push(Line::from(Span::styled(
            truncate(name, visible.width as usize),
            style,
        )));
    }
    if lines.is_empty() {
        lines.push(Line::from(Span::styled("no matches", theme.faint())));
    }
    frame.render_widget(
        Paragraph::new(lines).style(Style::new().bg(theme.surface)),
        visible,
    );
}

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::ui::theme::Theme;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    hints: &str,
    message: Option<(&str, ratatui::style::Color)>,
) {
    let mut spans = vec![Span::styled(hints.to_string(), theme.faint())];
    if let Some((text, color)) = message
        && !text.is_empty()
    {
        spans.push(Span::styled("   ", theme.faint()));
        spans.push(Span::styled(text.to_string(), Style::new().fg(color)));
    }
    let line = Line::from(spans);
    frame.render_widget(Paragraph::new(line).style(Style::new().bg(theme.bg)), area);
}

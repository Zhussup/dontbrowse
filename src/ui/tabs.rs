use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};

use crate::app::state::Tab;
use crate::ui::theme::Theme;

pub fn line(theme: &Theme, active: Option<Tab>) -> Line<'static> {
    let mut spans = vec![Span::raw(" ")];
    for (index, tab) in Tab::ALL.iter().enumerate() {
        let style = if Some(*tab) == active {
            Style::new()
                .fg(theme.bg)
                .bg(theme.accent)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::new().fg(theme.text_dim)
        };
        spans.push(Span::styled(format!(" {} ", tab.title()), style));
        if index + 1 < Tab::ALL.len() {
            spans.push(Span::raw(" "));
        }
    }
    Line::from(spans)
}

pub mod backend;
pub mod card;
pub mod feed;
pub mod header;
pub mod help;
pub mod links;
pub mod palette;
pub mod player_bar;
pub mod reader;
pub mod status_bar;
pub mod tabs;
pub mod theme;
pub mod toast;
pub mod toc;

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Clear, Paragraph, Wrap};

use crate::render::StyledLine;
use crate::ui::theme::Theme;

pub fn fill_background(frame: &mut Frame<'_>, theme: &Theme) {
    let area = frame.area();
    frame.render_widget(Block::new().style(Style::new().bg(theme.bg)), area);
}

pub fn panel(theme: &Theme) -> Block<'static> {
    Block::new()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::new().fg(theme.border))
        .style(Style::new().bg(theme.surface))
}

pub fn focused_panel(theme: &Theme) -> Block<'static> {
    panel(theme).border_style(Style::new().fg(theme.border_focus))
}

pub fn centered(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect::new(
        area.x + (area.width.saturating_sub(width)) / 2,
        area.y + (area.height.saturating_sub(height)) / 2,
        width,
        height,
    )
}

pub fn fill_rect(frame: &mut Frame<'_>, rect: Rect, symbol: &str, style: Style) {
    if rect.width == 0 || rect.height == 0 {
        return;
    }
    let line = symbol.repeat(rect.width as usize);
    let buffer = frame.buffer_mut();
    for y in rect.y..rect.y + rect.height {
        buffer.set_stringn(rect.x, y, &line, rect.width as usize, style);
    }
}

pub fn to_line(line: &StyledLine) -> Line<'static> {
    Line::from(
        line.spans
            .iter()
            .map(|span| Span::styled(span.text.clone(), span.style))
            .collect::<Vec<_>>(),
    )
}

pub fn render_lines(
    frame: &mut Frame<'_>,
    area: Rect,
    lines: &[StyledLine],
    scroll: usize,
    style: Style,
) {
    let text = Text::from(lines.iter().map(to_line).collect::<Vec<_>>());
    let paragraph = Paragraph::new(text)
        .style(style)
        .scroll((scroll.min(u16::MAX as usize) as u16, 0));
    frame.render_widget(paragraph, area);
}

pub fn render_wrapped(
    frame: &mut Frame<'_>,
    area: Rect,
    text: &str,
    style: Style,
    block: Option<Block<'static>>,
) {
    let mut paragraph = Paragraph::new(text.to_string())
        .style(style)
        .wrap(Wrap { trim: true });
    if let Some(block) = block {
        paragraph = paragraph.block(block);
    }
    frame.render_widget(paragraph, area);
}

pub fn clear_overlay(frame: &mut Frame<'_>, area: Rect) {
    frame.render_widget(Clear, area);
}

pub fn badge(text: &str, fg: Color, bg: Color) -> Span<'static> {
    Span::styled(
        format!(" {text} "),
        Style::new()
            .fg(fg)
            .bg(bg)
            .add_modifier(ratatui::style::Modifier::BOLD),
    )
}

pub fn truncate(text: &str, width: usize) -> String {
    crate::util::text::truncate_to_width(text, width)
}

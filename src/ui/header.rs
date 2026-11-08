use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::Tab;
use crate::tools::Tools;
use crate::ui::tabs;
use crate::ui::theme::Theme;
use crate::ui::truncate;

pub fn draw(
    frame: &mut Frame<'_>,
    area: Rect,
    theme: &Theme,
    title: &str,
    tools: &Tools,
    active_tab: Option<Tab>,
    hint: &str,
) {
    if area.height < 3 || area.width < 20 {
        return;
    }
    let rows = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    let logo_width = 11u16;
    let tools_width = 24u16.min(area.width / 3);
    let top = Layout::horizontal([
        Constraint::Length(logo_width),
        Constraint::Min(4),
        Constraint::Length(tools_width),
    ])
    .split(rows[0]);

    let logo = theme.gradient_text("dontbrowse");
    frame.render_widget(
        Paragraph::new(logo).style(Style::new().bg(theme.bg)),
        top[0],
    );

    let available = top[1].width.saturating_sub(3) as usize;
    let title_line = Line::from(vec![
        Span::styled("  /  ", theme.faint()),
        Span::styled(truncate(title, available), Style::new().fg(theme.text_dim)),
    ]);
    frame.render_widget(
        Paragraph::new(title_line).style(Style::new().bg(theme.bg)),
        top[1],
    );

    let tool_line = tool_status(tools, theme);
    frame.render_widget(
        Paragraph::new(tool_line)
            .alignment(Alignment::Right)
            .style(Style::new().bg(theme.bg)),
        top[2],
    );

    let tab_line = tabs::line(theme, active_tab);
    let hint_width = (hint.chars().count() as u16 + 2).min(area.width / 2);
    let second =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(hint_width)]).split(rows[1]);
    frame.render_widget(
        Paragraph::new(tab_line).style(Style::new().bg(theme.bg)),
        second[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(hint.to_string(), theme.faint()))
            .alignment(Alignment::Right)
            .style(Style::new().bg(theme.bg)),
        second[1],
    );

    frame.render_widget(
        Paragraph::new(theme.gradient_rule(area.width)).style(Style::new().bg(theme.bg)),
        rows[2],
    );
}

fn tool_status(tools: &Tools, theme: &Theme) -> Line<'static> {
    let mut spans = Vec::new();
    spans.push(Span::styled("yt-dlp ", theme.faint()));
    spans.push(Span::styled(
        if tools.yt_dlp_ok() { "✓" } else { "✗" },
        Style::new()
            .fg(if tools.yt_dlp_ok() {
                theme.success
            } else {
                theme.danger
            })
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled("   mpv ", theme.faint()));
    spans.push(Span::styled(
        if tools.mpv_ok() { "✓" } else { "✗" },
        Style::new()
            .fg(if tools.mpv_ok() {
                theme.success
            } else {
                theme.danger
            })
            .add_modifier(Modifier::BOLD),
    ));
    spans.push(Span::styled(" ", theme.faint()));
    Line::from(spans)
}

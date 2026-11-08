use ratatui::Frame;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::state::{PlayerPhase, PlayerState};
use crate::media::PlaybackMode;
use crate::ui::theme::Theme;
use crate::ui::truncate;
use crate::util::time::format_clock;

pub fn draw(frame: &mut Frame<'_>, area: Rect, theme: &Theme, player: &PlayerState) {
    if area.height < 3 {
        return;
    }
    let Some(video) = &player.current else {
        return;
    };
    let block = crate::ui::panel(theme).style(Style::new().bg(theme.surface));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.height < 2 || inner.width < 10 {
        return;
    }

    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(inner);

    let glyph = match player.phase {
        PlayerPhase::Playing => Span::styled("▶", Style::new().fg(theme.accent)),
        PlayerPhase::Paused => Span::styled("⏸", Style::new().fg(theme.warning)),
        PlayerPhase::Buffering | PlayerPhase::Loading => {
            Span::styled("◌", Style::new().fg(theme.text_dim))
        }
        PlayerPhase::Failed => Span::styled("✕", Style::new().fg(theme.danger)),
        PlayerPhase::Ended => Span::styled("■", Style::new().fg(theme.text_dim)),
        PlayerPhase::Idle => Span::styled("▶", Style::new().fg(theme.text_faint)),
    };

    let mode = match player.mode {
        PlaybackMode::Audio => "AUDIO",
        PlaybackMode::Video => "VIDEO",
    };
    let badge_width = 9u16;
    let top =
        Layout::horizontal([Constraint::Min(10), Constraint::Length(badge_width)]).split(rows[0]);

    let mut line = vec![glyph, Span::raw("  ")];
    line.push(Span::styled(
        truncate(&video.title, top[0].width.saturating_sub(6) as usize),
        Style::new().fg(theme.text).add_modifier(Modifier::BOLD),
    ));
    if !video.channel.name.is_empty() {
        line.push(Span::styled(
            format!("  —  {}", video.channel.name),
            Style::new().fg(theme.text_dim),
        ));
    }
    frame.render_widget(
        Paragraph::new(Line::from(line)).style(Style::new().bg(theme.surface)),
        top[0],
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            format!(" {mode} "),
            Style::new()
                .fg(theme.accent_hi)
                .bg(theme.surface_hi)
                .add_modifier(Modifier::BOLD),
        ))
        .alignment(Alignment::Right)
        .style(Style::new().bg(theme.surface)),
        top[1],
    );

    // Progress row.
    let position = player.display_position();
    let duration = player.duration.unwrap_or(0.0);
    let time = format!("{} / {}", format_clock(position), format_clock(duration));
    let time_width = time.chars().count() as u16;
    let hints = if inner.width > 100 {
        "space pause   , . seek   m mute   v video   x stop"
    } else {
        ""
    };
    let hint_width = hints.chars().count() as u16;
    let bar_width = inner
        .width
        .saturating_sub(time_width + hint_width + 6)
        .max(4) as usize;
    let ratio = player.progress();
    let filled = ((ratio * bar_width as f64).round() as usize).min(bar_width);
    let mut spans = vec![
        Span::styled(time, Style::new().fg(theme.text_dim)),
        Span::raw("  "),
        Span::styled("█".repeat(filled), Style::new().fg(theme.accent)),
        Span::styled(
            "░".repeat(bar_width.saturating_sub(filled)),
            Style::new().fg(theme.border),
        ),
    ];
    if !hints.is_empty() {
        spans.push(Span::raw("   "));
        spans.push(Span::styled(hints.to_string(), theme.faint()));
    }
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(Style::new().bg(theme.surface)),
        rows[1],
    );
}

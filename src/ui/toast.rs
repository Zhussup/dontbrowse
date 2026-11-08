use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::app::state::{Toast, ToastKind};
use crate::ui::theme::Theme;
use crate::ui::{clear_overlay, panel};

pub fn draw(frame: &mut Frame<'_>, area: Rect, theme: &Theme, toast: &Toast) {
    let width = (toast.message.chars().count() as u16 + 6)
        .min(area.width.saturating_sub(4))
        .max(24);
    let height = 3u16.min(area.height);
    let x = area.right().saturating_sub(width + 2);
    let y = area.bottom().saturating_sub(height + 2);
    let rect = Rect::new(x, y, width, height);
    clear_overlay(frame, rect);
    let color = match toast.kind {
        ToastKind::Info => theme.text_dim,
        ToastKind::Success => theme.success,
        ToastKind::Warning => theme.warning,
        ToastKind::Error => theme.danger,
    };
    let block = panel(theme).border_style(Style::new().fg(color));
    let inner = block.inner(rect);
    frame.render_widget(block, rect);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            toast.message.clone(),
            Style::new().fg(color).add_modifier(Modifier::BOLD),
        )))
        .style(Style::new().bg(theme.surface))
        .wrap(Wrap { trim: true }),
        inner,
    );
}

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Paragraph, Wrap};

use crate::ui::theme::Theme;
use crate::ui::{centered, clear_overlay, focused_panel};

const SECTIONS: &[(&str, &[(&str, &str)])] = &[
    (
        "Global",
        &[
            ("q, Ctrl+C", "quit"),
            ("?", "toggle this help"),
            ("Ctrl+P", "command palette"),
            ("Ctrl+L, :", "open a URL or search"),
            ("1 – 4", "switch tabs"),
            ("Tab, Shift+Tab", "next / previous tab"),
            ("Esc", "close overlay or go back"),
            ("gg, G", "jump to top / bottom"),
        ],
    ),
    (
        "Feed",
        &[
            ("h j k l", "move selection"),
            ("Enter", "play audio"),
            ("Shift+Enter", "open video window"),
            ("p", "play audio"),
            ("/", "search"),
            ("r", "refresh"),
            ("s", "subscribe / unsubscribe"),
            ("c", "copy URL"),
            ("L", "add to library"),
            ("i", "video details"),
        ],
    ),
    (
        "Reader",
        &[
            ("j / k", "scroll one line"),
            ("d / u", "half page"),
            ("Ctrl+D / Ctrl+U", "full page"),
            ("[ / ]", "previous / next heading"),
            ("t", "toggle table of contents"),
            ("o", "link picker"),
            ("Enter", "open selected link"),
            ("O", "open in system browser"),
            ("w", "toggle reading width"),
            ("Backspace", "back"),
        ],
    ),
    (
        "Player",
        &[
            ("Space", "pause / resume"),
            (", / .", "seek −5s / +5s"),
            ("< / >", "seek −30s / +30s"),
            ("+ / -", "volume"),
            ("m", "mute"),
            ("v", "audio / video mode"),
            ("x", "stop"),
            ("n", "next in queue"),
        ],
    ),
    (
        "Cookies",
        &[
            ("--cookies FILE", "import a Netscape cookies.txt"),
            ("--cookies-from-browser", "use browser cookies (yt-dlp)"),
            ("Ctrl+P → Import", "import cookies interactively"),
        ],
    ),
];

pub fn draw(frame: &mut Frame<'_>, area: Rect, theme: &Theme, scroll: u16) {
    let width = area.width.saturating_sub(8).clamp(30, 76);
    let height = area.height.saturating_sub(4).clamp(8, 30);
    let rect = centered(area, width, height);
    clear_overlay(frame, rect);
    let block = focused_panel(theme);
    let inner = block.inner(rect);
    frame.render_widget(block, rect);

    let mut lines: Vec<Line<'static>> = vec![
        Line::from(Span::styled(
            "Keyboard shortcuts",
            Style::new().fg(theme.accent).add_modifier(Modifier::BOLD),
        )),
        Line::default(),
    ];

    for (title, entries) in SECTIONS {
        lines.push(Line::from(Span::styled(
            (*title).to_string(),
            Style::new()
                .fg(theme.accent_hi)
                .add_modifier(Modifier::BOLD),
        )));
        for (keys, description) in *entries {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:<18}", keys), Style::new().fg(theme.text)),
                Span::styled((*description).to_string(), Style::new().fg(theme.text_dim)),
            ]));
        }
        lines.push(Line::default());
    }

    let hint_area = Rect::new(
        inner.x,
        inner.y + inner.height.saturating_sub(1),
        inner.width,
        1,
    );
    let body = Rect::new(
        inner.x,
        inner.y,
        inner.width,
        inner.height.saturating_sub(1),
    );
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::new().bg(theme.surface))
            .wrap(Wrap { trim: false })
            .scroll((scroll, 0)),
        body,
    );
    frame.render_widget(
        Paragraph::new(Span::styled(
            "j / k scroll   ·   ? or Esc close",
            theme.faint(),
        ))
        .style(Style::new().bg(theme.surface)),
        hint_area,
    );
}

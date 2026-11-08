use ratatui::style::{Modifier, Style};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::model::{Inline, InlineStyle};
use crate::ui::theme::Theme;

#[derive(Clone, Debug, Default)]
pub struct StyledSpan {
    pub text: String,
    pub style: Style,
    pub link: Option<String>,
}

#[derive(Clone, Debug, Default)]
pub struct StyledLine {
    pub spans: Vec<StyledSpan>,
}

impl StyledLine {
    pub fn width(&self) -> usize {
        self.spans
            .iter()
            .map(|span| UnicodeWidthStr::width(span.text.as_str()))
            .sum()
    }

    pub fn is_blank(&self) -> bool {
        self.spans.iter().all(|span| span.text.trim().is_empty())
    }

    pub fn plain(&self) -> String {
        self.spans.iter().map(|s| s.text.as_str()).collect()
    }

    pub fn push_span(&mut self, text: &str, style: Style, link: Option<&str>) {
        if text.is_empty() {
            return;
        }
        if let Some(last) = self.spans.last_mut()
            && last.style == style
            && last.link.as_deref() == link
        {
            last.text.push_str(text);
            return;
        }
        self.spans.push(StyledSpan {
            text: text.to_string(),
            style,
            link: link.map(str::to_string),
        });
    }
}

pub fn inline_to_styled(spans: &[Inline], base: Style, theme: &Theme) -> Vec<StyledSpan> {
    spans
        .iter()
        .map(|span| StyledSpan {
            text: span.text.clone(),
            style: inline_style(base, span.style, span.link.as_deref(), theme),
            link: span.link.clone(),
        })
        .collect()
}

pub fn inline_style(
    mut base: Style,
    inline: InlineStyle,
    link: Option<&str>,
    theme: &Theme,
) -> Style {
    if inline.bold {
        base = base.add_modifier(Modifier::BOLD);
    }
    if inline.italic {
        base = base.add_modifier(Modifier::ITALIC);
    }
    if inline.strike {
        base = base.add_modifier(Modifier::CROSSED_OUT);
    }
    if inline.underline {
        base = base.add_modifier(Modifier::UNDERLINED);
    }
    if inline.code {
        base = base.fg(theme.accent_hi).bg(theme.surface_hi);
    }
    if inline.sup || inline.sub {
        base = base.add_modifier(Modifier::BOLD);
    }
    if link.is_some() {
        base = base.fg(theme.link).add_modifier(Modifier::UNDERLINED);
    }
    base
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Word,
    Space,
    Break,
}

struct Token {
    text: String,
    style: Style,
    link: Option<String>,
    kind: Kind,
}

pub fn wrap_spans(spans: &[StyledSpan], width: usize) -> Vec<StyledLine> {
    let width = width.max(1);
    let mut lines: Vec<StyledLine> = vec![StyledLine::default()];
    let mut used = 0usize;
    let mut pending_space = false;

    for token in tokenize(spans) {
        match token.kind {
            Kind::Break => {
                pending_space = false;
                lines.push(StyledLine::default());
                used = 0;
            }
            Kind::Space => {
                if used > 0 {
                    pending_space = true;
                }
            }
            Kind::Word => {
                let word_width = UnicodeWidthStr::width(token.text.as_str());
                let space_width = usize::from(pending_space && used > 0);

                if used > 0 && used + space_width + word_width > width {
                    lines.push(StyledLine::default());
                    used = 0;
                    pending_space = false;
                }

                if pending_space && used > 0 {
                    if let Some(line) = lines.last_mut() {
                        line.push_span(" ", token.style, token.link.as_deref());
                    }
                    used += 1;
                }
                pending_space = false;

                if word_width > width && used == 0 {
                    let chunks = split_display(&token.text, width);
                    for (index, chunk) in chunks.iter().enumerate() {
                        if index > 0 {
                            lines.push(StyledLine::default());
                        }
                        if let Some(line) = lines.last_mut() {
                            line.push_span(chunk, token.style, token.link.as_deref());
                        }
                        used = UnicodeWidthStr::width(chunk.as_str());
                    }
                } else {
                    if let Some(line) = lines.last_mut() {
                        line.push_span(&token.text, token.style, token.link.as_deref());
                    }
                    used += word_width;
                }
            }
        }
    }

    if lines.len() > 1 && lines.last().is_some_and(StyledLine::is_blank) {
        lines.pop();
    }
    lines
}

fn tokenize(spans: &[StyledSpan]) -> Vec<Token> {
    let mut tokens = Vec::new();
    for span in spans {
        let mut word = String::new();
        for ch in span.text.chars() {
            if ch == '\n' {
                if !word.is_empty() {
                    tokens.push(Token {
                        text: std::mem::take(&mut word),
                        style: span.style,
                        link: span.link.clone(),
                        kind: Kind::Word,
                    });
                }
                tokens.push(Token {
                    text: "\n".to_string(),
                    style: span.style,
                    link: span.link.clone(),
                    kind: Kind::Break,
                });
            } else if ch.is_whitespace() {
                if !word.is_empty() {
                    tokens.push(Token {
                        text: std::mem::take(&mut word),
                        style: span.style,
                        link: span.link.clone(),
                        kind: Kind::Word,
                    });
                }
                if !matches!(tokens.last().map(|t| t.kind), Some(Kind::Space)) {
                    tokens.push(Token {
                        text: " ".to_string(),
                        style: span.style,
                        link: span.link.clone(),
                        kind: Kind::Space,
                    });
                }
            } else {
                word.push(ch);
            }
        }
        if !word.is_empty() {
            tokens.push(Token {
                text: word,
                style: span.style,
                link: span.link.clone(),
                kind: Kind::Word,
            });
        }
    }
    tokens
}

pub fn split_display(text: &str, width: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut used = 0usize;
    for grapheme in text.graphemes(true) {
        let w = UnicodeWidthStr::width(grapheme);
        if used + w > width && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            used = 0;
        }
        current.push_str(grapheme);
        used += w;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    chunks
}

pub fn indent_line(line: &StyledLine, indent: usize, style: Style) -> StyledLine {
    let mut out = StyledLine::default();
    out.push_span(&" ".repeat(indent), style, None);
    out.spans.extend(line.spans.clone());
    out
}

pub fn prefix_line(line: &StyledLine, prefix: &str, style: Style) -> StyledLine {
    let mut out = StyledLine::default();
    out.push_span(prefix, style, None);
    out.spans.extend(line.spans.clone());
    out
}

pub fn center_line(line: &StyledLine, width: usize) -> StyledLine {
    let used = line.width();
    if used >= width {
        return line.clone();
    }
    indent_line(line, (width - used) / 2, Style::new())
}

pub fn blank_line() -> StyledLine {
    StyledLine::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn span(text: &str) -> StyledSpan {
        StyledSpan {
            text: text.to_string(),
            style: Style::new(),
            link: None,
        }
    }

    #[test]
    fn wraps_words() {
        let lines = wrap_spans(&[span("hello world this is a test")], 11);
        assert!(lines.len() >= 2);
        assert!(lines.iter().all(|l| l.width() <= 11));
    }

    #[test]
    fn breaks_long_urls() {
        let lines = wrap_spans(
            &[span(
                "https://example.com/a/very/long/path/that/does/not/fit",
            )],
            12,
        );
        assert!(lines.len() > 1);
        assert!(lines.iter().all(|l| l.width() <= 12));
    }

    #[test]
    fn hard_breaks() {
        let lines = wrap_spans(&[span("one\ntwo")], 20);
        assert_eq!(lines.len(), 2);
    }
}

use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Remove terminal control sequences, ANSI escapes and bidi control characters.
/// This is a security boundary: untrusted page text must never reach the terminal raw.
pub fn sanitize_text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => {
                // CSI: ESC [ ... final-byte
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for n in chars.by_ref() {
                        if ('@'..='~').contains(&n) {
                            break;
                        }
                    }
                // OSC: ESC ] ... BEL or ST
                } else if chars.peek() == Some(&']') {
                    chars.next();
                    while let Some(n) = chars.next() {
                        if n == '\u{7}' {
                            break;
                        }
                        if n == '\u{1b}' && chars.peek() == Some(&'\\') {
                            chars.next();
                            break;
                        }
                    }
                }
            }
            '\n' | '\t' => out.push(c),
            c if (c as u32) < 0x20 || (0x7f..=0x9f).contains(&(c as u32)) => {}
            c if is_bidi_control(c) => {}
            c => out.push(c),
        }
    }
    out
}

fn is_bidi_control(c: char) -> bool {
    matches!(c as u32, 0x061C | 0x200E | 0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
}

pub fn collapse_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_space = false;
    for c in input.chars() {
        if c.is_whitespace() {
            if !last_space {
                out.push(' ');
                last_space = true;
            }
        } else {
            out.push(c);
            last_space = false;
        }
    }
    out.trim().to_string()
}

pub fn display_width(s: &str) -> usize {
    UnicodeWidthStr::width(s)
}

pub fn truncate_to_width(s: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let total = display_width(s);
    if total <= width {
        return s.to_string();
    }
    let mut out = String::new();
    let mut used = 0usize;
    for g in s.graphemes(true) {
        let w = UnicodeWidthStr::width(g);
        if used + w > width.saturating_sub(1) {
            break;
        }
        out.push_str(g);
        used += w;
    }
    out.push('…');
    out
}

/// Subsequence fuzzy match, returning a score (higher is better) and matched indices.
pub fn fuzzy_match(needle: &str, haystack: &str) -> Option<(i32, Vec<usize>)> {
    if needle.is_empty() {
        return Some((0, Vec::new()));
    }
    let lower_n: Vec<char> = needle.chars().flat_map(char::to_lowercase).collect();
    let lower_h: Vec<char> = haystack.chars().flat_map(char::to_lowercase).collect();
    let mut idx = Vec::new();
    let mut ni = 0;
    for (i, c) in lower_h.iter().enumerate() {
        if ni < lower_n.len() && *c == lower_n[ni] {
            idx.push(i);
            ni += 1;
        }
    }
    if ni != lower_n.len() {
        return None;
    }
    let mut score = 100 - (idx.last().copied().unwrap_or(0) as i32);
    for pair in idx.windows(2) {
        if pair[1] == pair[0] + 1 {
            score += 8;
        }
    }
    Some((score, idx))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_escapes_and_bidi() {
        let s = "hi\u{1b}[31m red\u{1b}[0m\u{202e} evil\u{7}";
        assert_eq!(sanitize_text(s), "hi red evil");
    }

    #[test]
    fn fuzzy() {
        assert!(fuzzy_match("pal", "Command Palette").is_some());
        assert!(fuzzy_match("xyz", "Command Palette").is_none());
    }

    #[test]
    fn truncates() {
        assert_eq!(truncate_to_width("hello", 10), "hello");
        assert_eq!(truncate_to_width("hello world", 6), "hello…");
    }
}

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};

use crate::config::ThemeChoice;
use crate::util::color::{lerp, no_color, rgb_or_256, supports_truecolor};

#[derive(Clone, Debug)]
pub struct Theme {
    pub name: &'static str,
    pub bg: Color,
    pub surface: Color,
    pub surface_hi: Color,
    pub border: Color,
    pub border_focus: Color,
    pub text: Color,
    pub text_dim: Color,
    pub text_faint: Color,
    pub accent: Color,
    pub accent_hi: Color,
    pub accent_dim: Color,
    pub link: Color,
    pub success: Color,
    pub warning: Color,
    pub danger: Color,
    pub selection_bg: Color,
    pub truecolor: bool,
    pub monochrome: bool,
    pub animations: bool,
    pub gradient: [(u8, u8, u8); 3],
}

impl Theme {
    pub fn detect(choice: ThemeChoice, animations: bool) -> Self {
        let truecolor = supports_truecolor();
        let monochrome = no_color();
        let light = match choice {
            ThemeChoice::Dark => false,
            ThemeChoice::Light => true,
            ThemeChoice::Auto => detect_light_background(),
        };
        if light {
            Self::paper(truecolor, monochrome, animations)
        } else {
            Self::ember(truecolor, monochrome, animations)
        }
    }

    fn ember(truecolor: bool, monochrome: bool, animations: bool) -> Self {
        let c = |r: u8, g: u8, b: u8| color(r, g, b, truecolor, monochrome);
        Self {
            name: "Ember",
            bg: c(11, 13, 16),
            surface: c(18, 22, 28),
            surface_hi: c(26, 32, 41),
            border: c(42, 49, 60),
            border_focus: c(240, 162, 75),
            text: c(231, 234, 240),
            text_dim: c(139, 148, 165),
            text_faint: c(92, 101, 117),
            accent: c(242, 166, 90),
            accent_hi: c(255, 201, 122),
            accent_dim: c(138, 90, 43),
            link: c(111, 179, 255),
            success: c(123, 216, 143),
            warning: c(242, 201, 76),
            danger: c(255, 107, 107),
            selection_bg: c(32, 40, 52),
            truecolor,
            monochrome,
            animations,
            gradient: [(255, 208, 138), (242, 166, 90), (217, 108, 95)],
        }
    }

    fn paper(truecolor: bool, monochrome: bool, animations: bool) -> Self {
        let c = |r: u8, g: u8, b: u8| color(r, g, b, truecolor, monochrome);
        Self {
            name: "Paper",
            bg: c(246, 243, 236),
            surface: c(255, 255, 255),
            surface_hi: c(239, 234, 224),
            border: c(217, 210, 197),
            border_focus: c(180, 83, 9),
            text: c(31, 35, 40),
            text_dim: c(95, 102, 114),
            text_faint: c(138, 144, 153),
            accent: c(180, 83, 9),
            accent_hi: c(217, 119, 6),
            accent_dim: c(214, 170, 120),
            link: c(29, 78, 216),
            success: c(21, 128, 61),
            warning: c(161, 98, 7),
            danger: c(185, 28, 28),
            selection_bg: c(253, 235, 211),
            truecolor,
            monochrome,
            animations,
            gradient: [(217, 119, 6), (180, 83, 9), (146, 64, 14)],
        }
    }

    pub fn base(&self) -> Style {
        Style::new().fg(self.text).bg(self.bg)
    }

    pub fn dim(&self) -> Style {
        Style::new().fg(self.text_dim)
    }

    pub fn faint(&self) -> Style {
        Style::new().fg(self.text_faint)
    }

    pub fn accent(&self) -> Style {
        Style::new().fg(self.accent)
    }

    pub fn accent_bold(&self) -> Style {
        Style::new().fg(self.accent).add_modifier(Modifier::BOLD)
    }

    pub fn danger(&self) -> Style {
        Style::new().fg(self.danger)
    }

    pub fn rule_line(&self, width: u16) -> Line<'static> {
        Line::from(Span::styled(
            "─".repeat(width as usize),
            Style::new().fg(self.border),
        ))
    }

    pub fn gradient_rule(&self, width: u16) -> Line<'static> {
        if self.monochrome || width == 0 {
            return self.rule_line(width);
        }
        let spans: Vec<Span<'static>> = (0..width)
            .map(|i| {
                let t = i as f32 / width.max(1) as f32;
                let rgb = gradient_at(self.gradient, t);
                Span::styled(
                    "─",
                    Style::new().fg(rgb_or_256(rgb.0, rgb.1, rgb.2, self.truecolor)),
                )
            })
            .collect();
        Line::from(spans)
    }

    pub fn gradient_text(&self, text: &str) -> Line<'static> {
        if self.monochrome {
            return Line::from(Span::styled(
                text.to_string(),
                Style::new().add_modifier(Modifier::BOLD),
            ));
        }
        let chars: Vec<char> = text.chars().collect();
        let denom = chars.len().max(1) as f32;
        let spans: Vec<Span<'static>> = chars
            .into_iter()
            .enumerate()
            .map(|(index, ch)| {
                let rgb = gradient_at(self.gradient, index as f32 / denom);
                Span::styled(
                    ch.to_string(),
                    Style::new()
                        .fg(rgb_or_256(rgb.0, rgb.1, rgb.2, self.truecolor))
                        .add_modifier(Modifier::BOLD),
                )
            })
            .collect();
        Line::from(spans)
    }

    pub fn selected_bg(&self) -> Color {
        self.selection_bg
    }
}

fn color(r: u8, g: u8, b: u8, truecolor: bool, monochrome: bool) -> Color {
    if monochrome {
        Color::Reset
    } else {
        rgb_or_256(r, g, b, truecolor)
    }
}

fn gradient_at(stops: [(u8, u8, u8); 3], t: f32) -> (u8, u8, u8) {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        lerp(stops[0], stops[1], t * 2.0)
    } else {
        lerp(stops[1], stops[2], (t - 0.5) * 2.0)
    }
}

fn detect_light_background() -> bool {
    if let Ok(value) = std::env::var("COLORFGBG")
        && let Some(bg) = value.split(';').next_back()
        && let Ok(index) = bg.trim().parse::<u8>()
    {
        return index >= 8;
    }
    false
}
